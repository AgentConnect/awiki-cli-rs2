use std::path::Path;

// These tables are part of the historical local-state shape. Phase 5 keeps the
// schema and existing rows readable, but no current runtime enqueues, claims,
// or advances legacy Group rebind jobs.
pub(crate) const GROUP_REBIND_RECOVERY_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS group_rebind_outbox (
    job_id TEXT PRIMARY KEY,
    owner_identity_id TEXT NOT NULL,
    group_did TEXT NOT NULL,
    member_handle TEXT NOT NULL,
    previous_member_did TEXT NOT NULL,
    new_member_did TEXT NOT NULL,
    binding_generation TEXT NOT NULL,
    phase TEXT NOT NULL DEFAULT 'pending',
    group_state_ref_json TEXT,
    attempt_count INTEGER NOT NULL DEFAULT 0,
    lease_expires_at TEXT,
    next_attempt_at TEXT,
    last_error_code TEXT,
    last_error_detail TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE(owner_identity_id, group_did, member_handle, binding_generation)
);
CREATE INDEX IF NOT EXISTS idx_group_rebind_outbox_resume
ON group_rebind_outbox(owner_identity_id, phase, next_attempt_at, updated_at);

CREATE TABLE IF NOT EXISTS group_rebind_p6_jobs (
    job_id TEXT PRIMARY KEY,
    owner_identity_id TEXT NOT NULL,
    group_did TEXT NOT NULL,
    event_id TEXT NOT NULL,
    member_handle TEXT NOT NULL,
    previous_member_did TEXT NOT NULL,
    new_member_did TEXT NOT NULL,
    binding_generation TEXT NOT NULL,
    group_state_ref_json TEXT NOT NULL,
    phase TEXT NOT NULL DEFAULT 'awaiting_add',
    attempt_count INTEGER NOT NULL DEFAULT 0,
    lease_expires_at TEXT,
    next_attempt_at TEXT,
    last_error_code TEXT,
    last_error_detail TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE(owner_identity_id, group_did, event_id),
    UNIQUE(owner_identity_id, group_did, member_handle, binding_generation)
);
CREATE INDEX IF NOT EXISTS idx_group_rebind_p6_resume
ON group_rebind_p6_jobs(owner_identity_id, phase, next_attempt_at, updated_at);
"#;

pub(crate) fn previous_recovery_did_matches(
    sqlite_path: &Path,
    owner_identity_id: &str,
    current_did: &str,
    candidate_did: &str,
) -> crate::ImResult<bool> {
    if candidate_did == current_did {
        return Ok(true);
    }
    Ok(
        crate::internal::identity_transition_pending::load_latest_applied_for_owner(
            sqlite_path,
            owner_identity_id,
        )?
        .is_some_and(|marker| {
            marker.owner_identity_id == owner_identity_id
                && marker.current_did == current_did
                && marker.previous_did == candidate_did
        }),
    )
}

pub(crate) fn repair_previous_group_message_directions(
    sqlite_path: &Path,
    owner_identity_id: &str,
    current_did: &str,
) -> crate::ImResult<usize> {
    let Some(marker) = crate::internal::identity_transition_pending::load_latest_applied_for_owner(
        sqlite_path,
        owner_identity_id,
    )?
    .filter(|marker| {
        marker.owner_identity_id == owner_identity_id && marker.current_did == current_did
    }) else {
        return Ok(0);
    };
    let connection = crate::internal::local_state::open_writable(sqlite_path)?;
    connection
        .execute(
            r#"
UPDATE messages
SET direction=1, is_read=1
WHERE owner_identity_id=?1
  AND sender_did=?2
  AND COALESCE(group_did, '')<>''
  AND direction<>1"#,
            rusqlite::params![owner_identity_id, marker.previous_did],
        )
        .map_err(crate::internal::local_state::local_state_unavailable)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_transition_projection_does_not_consume_legacy_group_journal() {
        let dir = tempfile::tempdir().unwrap();
        let sqlite_path = dir.path().join("im.sqlite");
        let marker =
            crate::internal::identity_transition_pending::IdentityTransitionMarker::joined_device(
                &sqlite_path,
                "join-1",
                "user-alice",
                "owner",
                "alice.example.com",
                "did:wba:example.com:alice:e1_old",
                "did:wba:example.com:alice:e1_new",
                "3",
            )
            .unwrap();
        crate::internal::identity_transition_pending::persist(&sqlite_path, &marker).unwrap();
        crate::internal::identity_transition_pending::update_phase(
            &sqlite_path,
            &marker.recovery_id,
            crate::internal::identity_transition_pending::TransitionPhase::Pending,
            crate::internal::identity_transition_pending::TransitionPhase::IdentitySwitched,
        )
        .unwrap();
        crate::internal::identity_transition_pending::mark_applied(
            &sqlite_path,
            &marker.recovery_id,
            crate::internal::identity_transition_pending::TransitionPhase::IdentitySwitched,
            "device-new",
            "1",
            "1",
            "{}",
        )
        .unwrap();
        let db = crate::internal::local_state::open_writable(&sqlite_path).unwrap();
        db.execute(
            r#"INSERT INTO group_rebind_outbox
(job_id,owner_identity_id,group_did,member_handle,previous_member_did,new_member_did,
 binding_generation,phase,created_at,updated_at)
VALUES ('legacy-job','owner','did:wba:example.com:groups:engineering',
        'alice.example.com','did:wba:example.com:alice:e1_old',
        'did:wba:example.com:alice:e1_new','3','blocked','now','now')"#,
            [],
        )
        .unwrap();
        db.execute(
            r#"INSERT INTO messages
(msg_id,owner_identity_id,owner_did,thread_id,direction,sender_did,group_id,group_did,stored_at,is_read)
VALUES ('message-1','owner','did:wba:example.com:alice:e1_new','group-thread',0,
        'did:wba:example.com:alice:e1_old','did:wba:example.com:groups:engineering',
        'did:wba:example.com:groups:engineering','now',0)"#,
            [],
        )
        .unwrap();
        drop(db);

        assert!(previous_recovery_did_matches(
            &sqlite_path,
            "owner",
            "did:wba:example.com:alice:e1_new",
            "did:wba:example.com:alice:e1_old",
        )
        .unwrap());
        assert_eq!(
            repair_previous_group_message_directions(
                &sqlite_path,
                "owner",
                "did:wba:example.com:alice:e1_new",
            )
            .unwrap(),
            1,
        );

        let db = crate::internal::local_state::open_writable(&sqlite_path).unwrap();
        let journal: (String, i64) = db
            .query_row(
                "SELECT phase,attempt_count FROM group_rebind_outbox WHERE job_id='legacy-job'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(journal, ("blocked".to_owned(), 0));
        let message: (i64, i64) = db
            .query_row(
                "SELECT direction,is_read FROM messages WHERE msg_id='message-1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(message, (1, 1));
    }
}
