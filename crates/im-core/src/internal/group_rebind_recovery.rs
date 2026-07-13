use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::Path;

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct P4RebindJob {
    pub(crate) job_id: String,
    pub(crate) owner_identity_id: String,
    pub(crate) group_did: String,
    pub(crate) member_handle: String,
    pub(crate) previous_member_did: String,
    pub(crate) new_member_did: String,
    pub(crate) binding_generation: String,
    pub(crate) phase: String,
    pub(crate) group_state_ref_json: Option<String>,
    pub(crate) attempt_count: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct P6RebindJob {
    pub(crate) job_id: String,
    pub(crate) owner_identity_id: String,
    pub(crate) group_did: String,
    pub(crate) event_id: String,
    pub(crate) member_handle: String,
    pub(crate) previous_member_did: String,
    pub(crate) new_member_did: String,
    pub(crate) binding_generation: String,
    pub(crate) group_state_ref_json: String,
    pub(crate) phase: String,
    pub(crate) attempt_count: i64,
}

pub(crate) fn enqueue_recovery_jobs(
    sqlite_path: &Path,
    owner_identity_id: &str,
    member_handle: &str,
    previous_dids: &[String],
    new_did: &str,
    binding_generation: &str,
) -> crate::ImResult<usize> {
    if previous_dids.is_empty() || !canonical_generation(binding_generation) {
        return Ok(0);
    }
    let mut connection = crate::internal::local_state::open_writable(sqlite_path)?;
    crate::internal::local_state::schema::ensure_schema(&connection)?;
    enqueue_recovery_jobs_for_connection(
        &mut connection,
        owner_identity_id,
        member_handle,
        previous_dids,
        new_did,
        binding_generation,
    )
}

pub(crate) fn enqueue_recovery_jobs_for_connection(
    connection: &mut rusqlite::Connection,
    owner_identity_id: &str,
    member_handle: &str,
    previous_dids: &[String],
    new_did: &str,
    binding_generation: &str,
) -> crate::ImResult<usize> {
    let owner_identity_id = required("owner_identity_id", owner_identity_id)?;
    let member_handle = canonical_recovery_handle(member_handle)?;
    let new_did = required("new_member_did", new_did)?;
    if !canonical_generation(binding_generation) {
        return Err(crate::ImError::invalid_input(
            Some("binding_generation".to_owned()),
            "binding generation must be a canonical positive decimal string",
        ));
    }
    let previous: std::collections::BTreeSet<_> = previous_dids
        .iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty() && *value != new_did)
        .collect();
    if previous.is_empty() {
        return Ok(0);
    }
    let candidates = {
        let mut statement = connection
            .prepare(
                r#"
SELECT gm.group_id, gm.member_did
     , gm.anchor_value, gm.handle_binding_generation
FROM group_members gm
JOIN groups g
  ON g.owner_identity_id = gm.owner_identity_id AND g.group_id = gm.group_id
WHERE gm.owner_identity_id = ?1
  AND gm.anchor_kind = 'handle'
  AND COALESCE(gm.status, 'active') = 'active'
  AND COALESCE(g.membership_status, 'active') NOT IN ('left','removed','inactive','non_member')"#,
            )
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        let rows = statement
            .query_map(rusqlite::params![owner_identity_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            })
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        let mut values = Vec::new();
        for row in rows {
            let value = row.map_err(crate::internal::local_state::local_state_unavailable)?;
            if previous.contains(value.1.trim())
                && recovery_handle_anchor_matches(&value.2, &value.1, &member_handle)
                && value.3.as_deref().is_some_and(|generation| {
                    canonical_generation(generation)
                        && decimal_generation_cmp(binding_generation, generation)
                            == std::cmp::Ordering::Greater
                })
            {
                values.push((value.0, value.1));
            }
        }
        values
    };
    let now = now();
    let transaction = connection
        .transaction()
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    let mut inserted = 0;
    for (group_did, previous_did) in candidates {
        let job_id = stable_job_id(&[
            owner_identity_id,
            &group_did,
            &member_handle.full,
            binding_generation,
        ]);
        inserted += transaction
            .execute(
                r#"
INSERT INTO group_rebind_outbox
    (job_id, owner_identity_id, group_did, member_handle, previous_member_did,
     new_member_did, binding_generation, phase, created_at, updated_at)
VALUES (?1,?2,?3,?4,?5,?6,?7,'pending',?8,?8)
ON CONFLICT(owner_identity_id, group_did, member_handle, binding_generation) DO NOTHING"#,
                rusqlite::params![
                    job_id,
                    owner_identity_id,
                    group_did,
                    member_handle.full,
                    previous_did,
                    new_did,
                    binding_generation,
                    now,
                ],
            )
            .map_err(crate::internal::local_state::local_state_unavailable)?;
    }
    transaction
        .commit()
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    Ok(inserted)
}

pub(crate) fn reconcile_missing_recovery_jobs(
    sqlite_path: &Path,
    owner_identity_id: &str,
    expected_handle: &str,
    expected_current_did: &str,
    lookup: &crate::directory::HandleLookupResult,
) -> crate::ImResult<usize> {
    let expected_handle = canonical_recovery_handle(expected_handle)?;
    let resolved_handle = canonical_recovery_handle(lookup.handle.as_str())?;
    if resolved_handle != expected_handle {
        return Err(crate::ImError::invalid_input(
            Some("handle".to_owned()),
            "authoritative Handle lookup returned a different full Handle",
        ));
    }
    if !lookup
        .status
        .as_deref()
        .is_some_and(|status| status.trim().eq_ignore_ascii_case("active"))
    {
        return Err(crate::ImError::invalid_input(
            Some("status".to_owned()),
            "authoritative Handle binding is not active",
        ));
    }
    let expected_current_did = required("current_did", expected_current_did)?;
    if lookup.did.as_str() != expected_current_did {
        return Err(crate::ImError::invalid_input(
            Some("did".to_owned()),
            "authoritative Handle DID does not match the current signing DID",
        ));
    }
    if did_wba_domain(expected_current_did).as_deref() != Some(expected_handle.domain.as_str()) {
        return Err(crate::ImError::invalid_input(
            Some("did".to_owned()),
            "current signing DID provider domain does not match the full Handle",
        ));
    }
    let binding_generation = lookup
        .binding_generation
        .as_deref()
        .filter(|generation| canonical_generation(generation))
        .ok_or_else(|| {
            crate::ImError::invalid_input(
                Some("binding_generation".to_owned()),
                "authoritative Handle lookup omitted a canonical positive binding generation",
            )
        })?;

    let mut connection = crate::internal::local_state::open_writable(sqlite_path)?;
    let previous_dids = {
        let mut statement = connection
            .prepare(
                "SELECT did FROM identity_did_history WHERE owner_identity_id=?1 AND status='previous' AND did<>?2 ORDER BY did",
            )
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        let rows = statement
            .query_map(
                rusqlite::params![owner_identity_id, expected_current_did],
                |row| row.get::<_, String>(0),
            )
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(crate::internal::local_state::local_state_unavailable)?
    };
    enqueue_recovery_jobs_for_connection(
        &mut connection,
        owner_identity_id,
        &expected_handle.full,
        &previous_dids,
        expected_current_did,
        binding_generation,
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CanonicalRecoveryHandle {
    full: String,
    local_part: String,
    domain: String,
}

fn canonical_recovery_handle(value: &str) -> crate::ImResult<CanonicalRecoveryHandle> {
    let value = required("member_handle", value)?.to_ascii_lowercase();
    let value = value
        .strip_prefix("wba://")
        .unwrap_or(&value)
        .trim_start_matches('@')
        .trim_end_matches('.')
        .to_owned();
    let Some((local_part, domain)) = value.split_once('.') else {
        return Err(crate::ImError::invalid_input(
            Some("member_handle".to_owned()),
            "member handle must be a full Handle including provider domain",
        ));
    };
    if local_part.is_empty() || domain.is_empty() {
        return Err(crate::ImError::invalid_input(
            Some("member_handle".to_owned()),
            "member handle must include non-empty local part and provider domain",
        ));
    }
    Ok(CanonicalRecoveryHandle {
        full: value.clone(),
        local_part: local_part.to_owned(),
        domain: domain.to_owned(),
    })
}

fn recovery_handle_anchor_matches(
    stored_anchor: &str,
    previous_member_did: &str,
    expected: &CanonicalRecoveryHandle,
) -> bool {
    let stored = normalize_handle_key(stored_anchor);
    if stored == expected.full {
        return true;
    }
    stored == expected.local_part
        && did_wba_domain(previous_member_did).as_deref() == Some(expected.domain.as_str())
}

fn normalize_handle_key(value: &str) -> String {
    let value = value.trim().to_ascii_lowercase();
    value
        .strip_prefix("wba://")
        .unwrap_or(&value)
        .trim_start_matches('@')
        .trim_end_matches('.')
        .to_owned()
}

fn did_wba_domain(did: &str) -> Option<String> {
    let mut segments = did.trim().split(':');
    match (segments.next(), segments.next(), segments.next()) {
        (Some(method), Some(network), Some(domain))
            if method.eq_ignore_ascii_case("did") && network.eq_ignore_ascii_case("wba") =>
        {
            let domain = domain.trim().trim_end_matches('.').to_ascii_lowercase();
            (!domain.is_empty()).then_some(domain)
        }
        _ => None,
    }
}

pub(crate) fn project_rebind_event(
    connection: &rusqlite::Connection,
    record: &crate::internal::local_state::messages::MessageRecord,
) -> crate::ImResult<bool> {
    let Ok(payload) = serde_json::from_str::<Value>(&record.content) else {
        return Ok(false);
    };
    let event_type = text(&payload, "type").replace('-', "_");
    if event_type != "member_credential_rebound" {
        return Ok(false);
    }
    let group_did_value = text(&payload, "group_did");
    let group_did = required("group_did", &group_did_value)?;
    let owner_metadata: Option<Option<String>> = connection
        .query_row(
            "SELECT metadata FROM groups WHERE owner_identity_id=?1 AND (group_id=?2 OR group_did=?2) AND my_role='owner' AND COALESCE(membership_status,'active')='active' LIMIT 1",
            rusqlite::params![record.owner_identity_id, group_did],
            |row| row.get(0),
        )
        .optional()
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    let Some(owner_metadata) = owner_metadata else {
        return Ok(false);
    };
    if owner_metadata
        .as_deref()
        .and_then(metadata_e2ee_classification)
        == Some(false)
    {
        return Ok(false);
    }
    let event_id = payload
        .get("sync_event_id")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(record.msg_id.as_str());
    let handle_value = text(&payload, "subject_handle");
    let old_did_value = text(&payload, "previous_subject_did");
    let new_did_value = text(&payload, "subject_did");
    let generation_value = text(&payload, "handle_binding_generation");
    let handle = required("subject_handle", &handle_value)?;
    let old_did = required("previous_subject_did", &old_did_value)?;
    let new_did = required("subject_did", &new_did_value)?;
    let generation = required("handle_binding_generation", &generation_value)?;
    if !canonical_generation(generation) {
        return Err(crate::ImError::invalid_input(
            Some("handle_binding_generation".to_owned()),
            "rebind event generation must be canonical",
        ));
    }
    let mut statement = connection
        .prepare("SELECT binding_generation FROM group_rebind_p6_jobs WHERE owner_identity_id=?1 AND group_did=?2 AND member_handle=?3")
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    let generations = statement
        .query_map(
            rusqlite::params![record.owner_identity_id, group_did, handle],
            |row| row.get::<_, String>(0),
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    for known in generations {
        let known = known.map_err(crate::internal::local_state::local_state_unavailable)?;
        if decimal_generation_cmp(&known, generation) != std::cmp::Ordering::Less {
            return Ok(false);
        }
    }
    let version_value = text(&payload, "group_state_version");
    let version = required("group_state_version", &version_value)?;
    if !canonical_generation(version) && version != "0" {
        return Err(crate::ImError::invalid_input(
            Some("group_state_version".to_owned()),
            "rebind event state version must be decimal",
        ));
    }
    let state_ref = serde_json::json!({
        "group_did": group_did,
        "group_state_version": version,
    })
    .to_string();
    let job_id = stable_job_id(&[&record.owner_identity_id, group_did, event_id]);
    let now = now();
    let inserted = connection
        .execute(
            r#"
INSERT INTO group_rebind_p6_jobs
    (job_id, owner_identity_id, group_did, event_id, member_handle,
     previous_member_did, new_member_did, binding_generation, group_state_ref_json,
     phase, created_at, updated_at)
VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,'awaiting_add',?10,?10)
ON CONFLICT DO NOTHING"#,
            rusqlite::params![
                job_id,
                record.owner_identity_id,
                group_did,
                event_id,
                handle,
                old_did,
                new_did,
                generation,
                state_ref,
                now,
            ],
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    Ok(inserted > 0)
}

pub(crate) fn next_p4_job(
    sqlite_path: &Path,
    owner_identity_id: &str,
) -> crate::ImResult<Option<P4RebindJob>> {
    let mut connection = crate::internal::local_state::open_writable(sqlite_path)?;
    crate::internal::local_state::schema::ensure_schema(&connection)?;
    claim_p4_job(&mut connection, owner_identity_id)
}

fn claim_p4_job(
    connection: &mut rusqlite::Connection,
    owner_identity_id: &str,
) -> crate::ImResult<Option<P4RebindJob>> {
    let transaction = connection
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    let now = now();
    let job_id: Option<String> = transaction
        .query_row(
            r#"
SELECT job_id
FROM group_rebind_outbox
WHERE owner_identity_id=?1
  AND (phase='pending' OR (phase='sending' AND lease_expires_at<=?2))
  AND (next_attempt_at IS NULL OR next_attempt_at<=?2)
ORDER BY created_at, job_id LIMIT 1"#,
            rusqlite::params![owner_identity_id, now],
            |row| row.get(0),
        )
        .optional()
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    let Some(job_id) = job_id else {
        transaction
            .commit()
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        return Ok(None);
    };
    let lease = lease_deadline();
    transaction
        .execute(
            "UPDATE group_rebind_outbox SET phase='sending', lease_expires_at=?2, attempt_count=attempt_count+1, updated_at=?3 WHERE job_id=?1",
            rusqlite::params![job_id, lease, now],
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    let job = transaction
        .query_row(
            r#"SELECT job_id, owner_identity_id, group_did, member_handle, previous_member_did,
       new_member_did, binding_generation, phase, group_state_ref_json, attempt_count
FROM group_rebind_outbox WHERE job_id=?1"#,
            [&job_id],
            p4_job_from_row,
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    transaction
        .commit()
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    Ok(Some(job))
}

pub(crate) fn next_p6_job(
    sqlite_path: &Path,
    owner_identity_id: &str,
) -> crate::ImResult<Option<P6RebindJob>> {
    let mut connection = crate::internal::local_state::open_writable(sqlite_path)?;
    crate::internal::local_state::schema::ensure_schema(&connection)?;
    claim_p6_job(&mut connection, owner_identity_id)
}

fn claim_p6_job(
    connection: &mut rusqlite::Connection,
    owner_identity_id: &str,
) -> crate::ImResult<Option<P6RebindJob>> {
    let transaction = connection
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    let now = now();
    let candidate: Option<(String, String)> = transaction
        .query_row(
            r#"
SELECT job_id, phase
FROM group_rebind_p6_jobs
WHERE owner_identity_id=?1
  AND (
    phase IN ('awaiting_add','add_repair','awaiting_remove','remove_repair')
    OR (phase IN ('executing_add','executing_add_repair','executing_remove','executing_remove_repair') AND lease_expires_at<=?2)
  )
  AND (next_attempt_at IS NULL OR next_attempt_at<=?2)
ORDER BY created_at, job_id LIMIT 1"#,
            rusqlite::params![owner_identity_id, now],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    let Some((job_id, stored_phase)) = candidate else {
        transaction
            .commit()
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        return Ok(None);
    };
    let work_phase = stored_phase
        .strip_prefix("executing_")
        .unwrap_or(&stored_phase);
    let executing_phase = format!("executing_{work_phase}");
    transaction
        .execute(
            "UPDATE group_rebind_p6_jobs SET phase=?2, lease_expires_at=?3, attempt_count=attempt_count+1, updated_at=?4 WHERE job_id=?1",
            rusqlite::params![job_id, executing_phase, lease_deadline(), now],
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    let mut job = transaction
        .query_row(
            r#"SELECT job_id, owner_identity_id, group_did, event_id, member_handle,
       previous_member_did, new_member_did, binding_generation, group_state_ref_json,
       phase, attempt_count FROM group_rebind_p6_jobs WHERE job_id=?1"#,
            [&job_id],
            p6_job_from_row,
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    job.phase = work_phase.to_owned();
    transaction
        .commit()
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    Ok(Some(job))
}

pub(crate) fn update_p4_job(
    sqlite_path: &Path,
    job_id: &str,
    phase: &str,
    state_ref: Option<&anp::group_e2ee::GroupStateRef>,
    error: Option<&str>,
) -> crate::ImResult<()> {
    update_job(
        sqlite_path,
        "group_rebind_outbox",
        job_id,
        phase,
        state_ref.map(|value| serde_json::to_string(value).unwrap_or_default()),
        error,
    )
}

pub(crate) fn project_applied_p4_rebind(
    sqlite_path: &Path,
    job: &P4RebindJob,
) -> crate::ImResult<()> {
    let expected_handle = canonical_recovery_handle(&job.member_handle)?;
    if !canonical_generation(&job.binding_generation) {
        return Err(crate::ImError::invalid_input(
            Some("binding_generation".to_owned()),
            "rebind projection requires a canonical positive binding generation",
        ));
    }
    let mut connection = crate::internal::local_state::open_writable(sqlite_path)?;
    crate::internal::local_state::schema::ensure_schema(&connection)?;
    let transaction = connection
        .transaction()
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    let candidates = {
        let mut statement = transaction
            .prepare(
                r#"
SELECT user_id, member_did, anchor_value, COALESCE(handle_binding_generation, '')
FROM group_members
WHERE owner_identity_id=?1
  AND group_id=?2
  AND anchor_kind='handle'
  AND COALESCE(status, 'active')='active'"#,
            )
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        let rows = statement
            .query_map(
                rusqlite::params![job.owner_identity_id, job.group_did],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                },
            )
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        let mut values = Vec::new();
        for row in rows {
            let value = row.map_err(crate::internal::local_state::local_state_unavailable)?;
            if recovery_handle_anchor_matches(&value.2, &value.1, &expected_handle) {
                values.push(value);
            }
        }
        values
    };
    if candidates.len() != 1 {
        return Err(crate::ImError::LocalStateUnavailable {
            detail: format!(
                "accepted group rebind matched {} active local Handle member rows",
                candidates.len()
            ),
        });
    }
    let (user_id, stored_did, _, stored_generation) = &candidates[0];
    if stored_did == &job.new_member_did
        && canonical_generation(stored_generation)
        && decimal_generation_cmp(stored_generation, &job.binding_generation)
            != std::cmp::Ordering::Less
    {
        transaction
            .commit()
            .map_err(crate::internal::local_state::local_state_unavailable)?;
        return Ok(());
    }
    if stored_did != &job.previous_member_did {
        return Err(crate::ImError::LocalStateUnavailable {
            detail: "accepted group rebind does not continue from the projected member DID"
                .to_owned(),
        });
    }
    if !canonical_generation(stored_generation)
        || decimal_generation_cmp(&job.binding_generation, stored_generation)
            != std::cmp::Ordering::Greater
    {
        return Err(crate::ImError::LocalStateUnavailable {
            detail: "accepted group rebind does not advance the projected Handle generation"
                .to_owned(),
        });
    }
    let updated = transaction
        .execute(
            r#"
UPDATE group_members
SET member_did=?4,
    member_handle=?5,
    anchor_value=?5,
    handle_binding_generation=?6,
    last_synced_at=?7
WHERE owner_identity_id=?1 AND group_id=?2 AND user_id=?3 AND member_did=?8"#,
            rusqlite::params![
                job.owner_identity_id,
                job.group_did,
                user_id,
                job.new_member_did,
                expected_handle.full,
                job.binding_generation,
                now(),
                job.previous_member_did,
            ],
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    if updated != 1 {
        return Err(crate::ImError::LocalStateUnavailable {
            detail: "accepted group rebind member projection changed concurrently".to_owned(),
        });
    }
    transaction
        .commit()
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    Ok(())
}

pub(crate) fn update_p6_job(
    sqlite_path: &Path,
    job_id: &str,
    phase: &str,
    error: Option<&str>,
) -> crate::ImResult<()> {
    update_job(
        sqlite_path,
        "group_rebind_p6_jobs",
        job_id,
        phase,
        None,
        error,
    )
}

pub(crate) fn is_group_send_paused(
    sqlite_path: &Path,
    owner_identity_id: &str,
    group_did: &str,
) -> crate::ImResult<bool> {
    if !sqlite_path.exists() {
        return Ok(false);
    }
    let connection = crate::internal::local_state::open_writable(sqlite_path)?;
    crate::internal::local_state::schema::ensure_schema(&connection)?;
    connection
        .query_row(
            r#"
SELECT EXISTS(
  SELECT 1 FROM group_rebind_p6_jobs
  WHERE owner_identity_id=?1 AND group_did=?2 AND phase <> 'complete'
  UNION ALL
  SELECT 1 FROM group_rebind_outbox
  WHERE owner_identity_id=?1 AND group_did=?2 AND phase='awaiting_p6'
)"#,
            rusqlite::params![owner_identity_id, group_did],
            |row| row.get(0),
        )
        .map_err(crate::internal::local_state::local_state_unavailable)
}

pub(crate) fn paused_groups(
    sqlite_path: &Path,
    owner_identity_id: &str,
) -> crate::ImResult<Vec<String>> {
    let connection = crate::internal::local_state::open_writable(sqlite_path)?;
    crate::internal::local_state::schema::ensure_schema(&connection)?;
    let mut statement = connection
        .prepare(
            r#"
SELECT DISTINCT group_did FROM (
  SELECT group_did FROM group_rebind_p6_jobs
  WHERE owner_identity_id=?1 AND phase <> 'complete'
  UNION ALL
  SELECT group_did FROM group_rebind_outbox
  WHERE owner_identity_id=?1 AND phase='awaiting_p6'
)
ORDER BY group_did"#,
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    let rows = statement
        .query_map([owner_identity_id], |row| row.get::<_, String>(0))
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    let mut values = Vec::new();
    for row in rows {
        values.push(row.map_err(crate::internal::local_state::local_state_unavailable)?);
    }
    Ok(values)
}

pub(crate) fn recovery_items(
    sqlite_path: &Path,
    owner_identity_id: &str,
) -> crate::ImResult<Vec<crate::groups::GroupRebindRecoveryItem>> {
    let connection = crate::internal::local_state::open_writable(sqlite_path)?;
    crate::internal::local_state::schema::ensure_schema(&connection)?;
    let mut statement = connection
        .prepare(
            r#"
SELECT group_did, layer, phase
FROM (
  SELECT group_did, 'p4' AS layer, phase, updated_at FROM group_rebind_outbox
  WHERE owner_identity_id=?1
  UNION ALL
  SELECT group_did, 'p6' AS layer, phase, updated_at FROM group_rebind_p6_jobs
  WHERE owner_identity_id=?1
)
ORDER BY updated_at DESC, group_did, layer
LIMIT 500"#,
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    let rows = statement
        .query_map([owner_identity_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    let mut seen = std::collections::BTreeSet::new();
    let mut items = Vec::new();
    for row in rows {
        let (group_did, layer, phase) =
            row.map_err(crate::internal::local_state::local_state_unavailable)?;
        if !seen.insert((group_did.clone(), layer.clone())) {
            continue;
        }
        items.push(crate::groups::GroupRebindRecoveryItem {
            group: crate::ids::GroupRef::parse(group_did)?,
            layer,
            blocked: phase == "blocked",
            phase,
        });
    }
    Ok(items)
}

pub(crate) fn awaiting_p6_groups(
    sqlite_path: &Path,
    owner_identity_id: &str,
) -> crate::ImResult<Vec<String>> {
    let connection = crate::internal::local_state::open_writable(sqlite_path)?;
    crate::internal::local_state::schema::ensure_schema(&connection)?;
    let mut statement = connection
        .prepare(
            "SELECT DISTINCT group_did FROM group_rebind_outbox WHERE owner_identity_id=?1 AND phase='awaiting_p6' ORDER BY group_did",
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    let rows = statement
        .query_map([owner_identity_id], |row| row.get::<_, String>(0))
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    let mut groups = Vec::new();
    for row in rows {
        groups.push(row.map_err(crate::internal::local_state::local_state_unavailable)?);
    }
    Ok(groups)
}

pub(crate) fn complete_transport_p4_jobs(
    sqlite_path: &Path,
    owner_identity_id: &str,
    group_did: &str,
    limit: u32,
) -> crate::ImResult<u32> {
    if limit == 0 {
        return Ok(0);
    }
    let connection = crate::internal::local_state::open_writable(sqlite_path)?;
    crate::internal::local_state::schema::ensure_schema(&connection)?;
    let classification =
        group_security_classification_for_connection(&connection, owner_identity_id, group_did)?;
    if classification != Some(false) {
        return Ok(0);
    }
    let mut statement = connection
        .prepare(
            "SELECT job_id FROM group_rebind_outbox WHERE owner_identity_id=?1 AND group_did=?2 AND phase='awaiting_p6' ORDER BY created_at,job_id LIMIT ?3",
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    let rows = statement
        .query_map(
            rusqlite::params![owner_identity_id, group_did, limit],
            |row| row.get::<_, String>(0),
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    let job_ids = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    drop(statement);
    let now = now();
    let mut updated = 0_u32;
    for job_id in job_ids {
        updated += connection
            .execute(
                "UPDATE group_rebind_outbox SET phase='complete',lease_expires_at=NULL,next_attempt_at=NULL,last_error_code=NULL,last_error_detail=NULL,updated_at=?2 WHERE job_id=?1 AND phase='awaiting_p6'",
                rusqlite::params![job_id, now],
            )
            .map_err(crate::internal::local_state::local_state_unavailable)? as u32;
    }
    Ok(updated)
}

pub(crate) fn group_security_classification(
    sqlite_path: &Path,
    owner_identity_id: &str,
    group_did: &str,
) -> crate::ImResult<Option<bool>> {
    let connection = crate::internal::local_state::open_writable(sqlite_path)?;
    crate::internal::local_state::schema::ensure_schema(&connection)?;
    group_security_classification_for_connection(&connection, owner_identity_id, group_did)
}

pub(crate) fn group_uses_e2ee(
    sqlite_path: &Path,
    owner_identity_id: &str,
    group_did: &str,
) -> crate::ImResult<bool> {
    let connection = crate::internal::local_state::open_writable(sqlite_path)?;
    crate::internal::local_state::schema::ensure_schema(&connection)?;
    Ok(
        group_security_classification_for_connection(&connection, owner_identity_id, group_did)?
            .unwrap_or(true),
    )
}

fn group_security_classification_for_connection(
    connection: &rusqlite::Connection,
    owner_identity_id: &str,
    group_did: &str,
) -> crate::ImResult<Option<bool>> {
    let metadata: Option<String> = connection
        .query_row(
            "SELECT metadata FROM groups WHERE owner_identity_id=?1 AND (group_id=?2 OR group_did=?2) LIMIT 1",
            rusqlite::params![owner_identity_id, group_did],
            |row| row.get(0),
        )
        .optional()
        .map_err(crate::internal::local_state::local_state_unavailable)?
        .flatten();
    Ok(metadata.and_then(|metadata| metadata_e2ee_classification(&metadata)))
}

pub(crate) fn complete_from_verified_remove_notice(
    sqlite_path: &Path,
    owner_identity_id: &str,
    group_did: &str,
    subject_did: &str,
    subject_status: &str,
    group_state_version: &str,
    member_dids: &[String],
) -> crate::ImResult<bool> {
    if subject_status.trim() != "removed" || !canonical_generation(group_state_version) {
        return Ok(false);
    }
    let connection = crate::internal::local_state::open_writable(sqlite_path)?;
    crate::internal::local_state::schema::ensure_schema(&connection)?;
    let roster = member_dids
        .iter()
        .map(String::as_str)
        .collect::<std::collections::BTreeSet<_>>();
    if roster.contains(subject_did) {
        return Ok(false);
    }
    let mut statement = connection
        .prepare("SELECT job_id,group_state_ref_json,new_member_did FROM group_rebind_outbox WHERE owner_identity_id=?1 AND group_did=?2 AND previous_member_did=?3 AND phase='awaiting_p6'")
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    let rows = statement
        .query_map(
            rusqlite::params![owner_identity_id, group_did, subject_did],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    let mut matching = Vec::new();
    for row in rows {
        let (job_id, state_ref, new_member_did) =
            row.map_err(crate::internal::local_state::local_state_unavailable)?;
        let matches = roster.contains(new_member_did.as_str())
            && state_ref
                .as_deref()
                .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
                .and_then(|value| {
                    value
                        .get("group_state_version")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                })
                .is_some_and(|version| version == group_state_version);
        if matches {
            matching.push(job_id);
        }
    }
    drop(statement);
    let now = now();
    let mut updated = 0;
    for job_id in matching {
        updated += connection.execute("UPDATE group_rebind_outbox SET phase='complete',lease_expires_at=NULL,next_attempt_at=NULL,updated_at=?2 WHERE job_id=?1 AND phase='awaiting_p6'", rusqlite::params![job_id, now])
            .map_err(crate::internal::local_state::local_state_unavailable)?;
    }
    Ok(updated > 0)
}

fn metadata_e2ee_classification(metadata: &str) -> Option<bool> {
    let metadata = serde_json::from_str::<Value>(metadata).ok()?;
    let profiles = [
        metadata.get("message_security_profile"),
        metadata.get("required_security_profile"),
        metadata
            .get("group_policy")
            .and_then(|policy| policy.get("message_security_profile")),
    ]
    .into_iter()
    .flatten()
    .filter_map(Value::as_str)
    .map(|value| value.trim().to_ascii_lowercase())
    .filter(|value| !value.is_empty())
    .collect::<Vec<_>>();
    if profiles.iter().any(|profile| profile == "group-e2ee") {
        return Some(true);
    }
    if !profiles.is_empty()
        && profiles
            .iter()
            .all(|profile| matches!(profile.as_str(), "transport-protected" | "transport"))
    {
        Some(false)
    } else {
        None
    }
}

fn update_job(
    sqlite_path: &Path,
    table: &str,
    job_id: &str,
    phase: &str,
    state_ref_json: Option<String>,
    error: Option<&str>,
) -> crate::ImResult<()> {
    let connection = crate::internal::local_state::open_writable(sqlite_path)?;
    let next_attempt_at = error.map(|_| retry_deadline(&connection, table, job_id));
    let sql = format!(
        "UPDATE {table} SET phase=?2, group_state_ref_json=COALESCE(?3,group_state_ref_json), lease_expires_at=NULL, next_attempt_at=?4, last_error_detail=?5, updated_at=?6 WHERE job_id=?1"
    );
    connection
        .execute(
            &sql,
            rusqlite::params![job_id, phase, state_ref_json, next_attempt_at, error, now()],
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    Ok(())
}

fn p4_job_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<P4RebindJob> {
    Ok(P4RebindJob {
        job_id: row.get(0)?,
        owner_identity_id: row.get(1)?,
        group_did: row.get(2)?,
        member_handle: row.get(3)?,
        previous_member_did: row.get(4)?,
        new_member_did: row.get(5)?,
        binding_generation: row.get(6)?,
        phase: row.get(7)?,
        group_state_ref_json: row.get(8)?,
        attempt_count: row.get(9)?,
    })
}

fn p6_job_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<P6RebindJob> {
    Ok(P6RebindJob {
        job_id: row.get(0)?,
        owner_identity_id: row.get(1)?,
        group_did: row.get(2)?,
        event_id: row.get(3)?,
        member_handle: row.get(4)?,
        previous_member_did: row.get(5)?,
        new_member_did: row.get(6)?,
        binding_generation: row.get(7)?,
        group_state_ref_json: row.get(8)?,
        phase: row.get(9)?,
        attempt_count: row.get(10)?,
    })
}

fn stable_job_id(parts: &[&str]) -> String {
    let mut digest = Sha256::new();
    for part in parts {
        digest.update(part.as_bytes());
        digest.update([0]);
    }
    let bytes = digest.finalize();
    let mut value = String::from("op-rebind-");
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(value, "{byte:02x}");
    }
    value
}

fn canonical_generation(value: &str) -> bool {
    !value.is_empty()
        && value != "0"
        && !value.starts_with('0')
        && value.bytes().all(|byte| byte.is_ascii_digit())
}

fn decimal_generation_cmp(left: &str, right: &str) -> std::cmp::Ordering {
    left.len().cmp(&right.len()).then_with(|| left.cmp(right))
}

fn required<'a>(field: &str, value: &'a str) -> crate::ImResult<&'a str> {
    let value = value.trim();
    if value.is_empty() {
        return Err(crate::ImError::invalid_input(
            Some(field.to_owned()),
            format!("{field} is required"),
        ));
    }
    Ok(value)
}

fn text(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_owned()
}

fn now() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_owned())
}

fn lease_deadline() -> String {
    timestamp_after(time::Duration::minutes(2))
}

fn retry_deadline(connection: &rusqlite::Connection, table: &str, job_id: &str) -> String {
    let sql = format!("SELECT attempt_count FROM {table} WHERE job_id=?1");
    let attempts = connection
        .query_row(&sql, [job_id], |row| row.get::<_, i64>(0))
        .unwrap_or(1)
        .clamp(1, 8);
    timestamp_after(time::Duration::seconds(1_i64 << attempts))
}

fn timestamp_after(duration: time::Duration) -> String {
    (time::OffsetDateTime::now_utc() + duration)
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_owned())
}

use rusqlite::OptionalExtension as _;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovery_outbox_is_idempotent_and_restart_readable() {
        let mut db = rusqlite::Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        db.execute("INSERT INTO groups (owner_identity_id,owner_did,group_id,group_did,my_role,membership_status,stored_at) VALUES ('owner','did:old','did:group','did:group','member','active','now')", []).unwrap();
        db.execute("INSERT INTO group_members (owner_identity_id,owner_did,group_id,user_id,member_did,member_handle,anchor_kind,anchor_value,handle_binding_generation,status,last_synced_at) VALUES ('owner','did:old','did:group','peer','did:old','alice.example.com','handle','alice.example.com','1','active','now')", []).unwrap();
        let old = vec!["did:old".to_owned()];
        assert_eq!(
            enqueue_recovery_jobs_for_connection(
                &mut db,
                "owner",
                "alice.example.com",
                &old,
                "did:new",
                "2"
            )
            .unwrap(),
            1
        );
        assert_eq!(
            enqueue_recovery_jobs_for_connection(
                &mut db,
                "owner",
                "alice.example.com",
                &old,
                "did:new",
                "2"
            )
            .unwrap(),
            0
        );
        let count: i64 = db
            .query_row("SELECT COUNT(*) FROM group_rebind_outbox", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn applied_rebind_advances_projection_used_by_next_recovery() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.sqlite3");
        let db = crate::internal::local_state::open_writable(&path).unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        db.execute("INSERT INTO groups (owner_identity_id,owner_did,group_id,group_did,my_role,membership_status,stored_at) VALUES ('owner','did:current','did:group','did:group','member','active','now')", []).unwrap();
        db.execute("INSERT INTO group_members (owner_identity_id,owner_did,group_id,user_id,member_did,member_handle,anchor_kind,anchor_value,handle_binding_generation,status,last_synced_at) VALUES ('owner','did:current','did:group','peer','did:wba:example.com:alice:e1_old','alice','handle','alice','1','active','now')", []).unwrap();
        drop(db);

        let applied = P4RebindJob {
            job_id: "job-2".to_owned(),
            owner_identity_id: "owner".to_owned(),
            group_did: "did:group".to_owned(),
            member_handle: "alice.example.com".to_owned(),
            previous_member_did: "did:wba:example.com:alice:e1_old".to_owned(),
            new_member_did: "did:wba:example.com:alice:e1_middle".to_owned(),
            binding_generation: "2".to_owned(),
            phase: "sending".to_owned(),
            group_state_ref_json: None,
            attempt_count: 1,
        };
        project_applied_p4_rebind(&path, &applied).unwrap();
        project_applied_p4_rebind(&path, &applied).unwrap();

        let mut db = crate::internal::local_state::open_writable(&path).unwrap();
        let projected: (String, String, String) = db
            .query_row(
                "SELECT member_did,anchor_value,handle_binding_generation FROM group_members",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(
            projected,
            (
                "did:wba:example.com:alice:e1_middle".to_owned(),
                "alice.example.com".to_owned(),
                "2".to_owned(),
            )
        );

        assert_eq!(
            enqueue_recovery_jobs_for_connection(
                &mut db,
                "owner",
                "alice.example.com",
                &[
                    "did:wba:example.com:alice:e1_old".to_owned(),
                    "did:wba:example.com:alice:e1_middle".to_owned(),
                ],
                "did:wba:example.com:alice:e1_new",
                "3",
            )
            .unwrap(),
            1
        );
        let previous: String = db
            .query_row(
                "SELECT previous_member_did FROM group_rebind_outbox",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(previous, "did:wba:example.com:alice:e1_middle");
    }

    #[test]
    fn recovery_outbox_accepts_only_domain_bound_legacy_local_part_anchors() {
        let mut db = rusqlite::Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        db.execute_batch(
            r#"
INSERT INTO groups (owner_identity_id,owner_did,group_id,group_did,my_role,membership_status,stored_at) VALUES
 ('owner','did:new','did:group:legacy','did:group:legacy','member','active','now'),
 ('owner','did:new','did:group:exact','did:group:exact','member','active','now'),
 ('owner','did:new','did:group:cross-domain','did:group:cross-domain','member','active','now'),
 ('owner','did:new','did:group:not-previous','did:group:not-previous','member','active','now'),
 ('owner','did:new','did:group:did-only','did:group:did-only','member','active','now'),
 ('owner','did:new','did:group:inactive','did:group:inactive','member','active','now');
INSERT INTO group_members
 (owner_identity_id,owner_did,group_id,user_id,member_did,member_handle,anchor_kind,anchor_value,handle_binding_generation,status,last_synced_at) VALUES
 ('owner','did:new','did:group:legacy','legacy','did:wba:example.com:alice:e1_old','alice','handle','alice','1','active','now'),
 ('owner','did:new','did:group:exact','exact','did:wba:example.com:alice:e1_old','alice.example.com','handle','alice.example.com','1','active','now'),
 ('owner','did:new','did:group:cross-domain','cross','did:wba:other.test:alice:e1_old','alice','handle','alice','1','active','now'),
 ('owner','did:new','did:group:not-previous','unknown','did:wba:example.com:alice:e1_unknown','alice','handle','alice','1','active','now'),
 ('owner','did:new','did:group:did-only','did-only','did:wba:example.com:alice:e1_old','alice','did','did:wba:example.com:alice:e1_old',NULL,'active','now'),
 ('owner','did:new','did:group:inactive','inactive','did:wba:example.com:alice:e1_old','alice','handle','alice','1','removed','now');
"#,
        )
        .unwrap();
        let previous = vec![
            "did:wba:example.com:alice:e1_old".to_owned(),
            "did:wba:other.test:alice:e1_old".to_owned(),
        ];

        let error = enqueue_recovery_jobs_for_connection(
            &mut db,
            "owner",
            "alice",
            &previous,
            "did:wba:example.com:alice:e1_new",
            "2",
        )
        .unwrap_err();
        assert!(error.to_string().contains("full Handle"));

        assert_eq!(
            enqueue_recovery_jobs_for_connection(
                &mut db,
                "owner",
                "WBA://Alice.Example.Com.",
                &previous,
                "did:wba:example.com:alice:e1_new",
                "2",
            )
            .unwrap(),
            2
        );

        let mut statement = db
            .prepare("SELECT group_did,member_handle FROM group_rebind_outbox ORDER BY group_did")
            .unwrap();
        let jobs = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(
            jobs,
            vec![
                ("did:group:exact".to_owned(), "alice.example.com".to_owned(),),
                (
                    "did:group:legacy".to_owned(),
                    "alice.example.com".to_owned(),
                ),
            ]
        );
    }

    #[test]
    fn reconcile_requires_current_active_authoritative_binding_and_newer_generation() {
        let dir = tempfile::tempdir().unwrap();
        let sqlite_path = dir.path().join("im.sqlite");
        let db = crate::internal::local_state::open_writable(&sqlite_path).unwrap();
        db.execute_batch(
            r#"
INSERT INTO identity_did_history
 (owner_identity_id,did,status,first_seen_at,last_seen_at) VALUES
 ('owner','did:wba:example.com:alice:e1_old','previous','now','now'),
 ('owner','did:wba:example.com:alice:e1_new','current','now','now');
INSERT INTO groups
 (owner_identity_id,owner_did,group_id,group_did,my_role,membership_status,stored_at) VALUES
 ('owner','did:wba:example.com:alice:e1_new','did:group','did:group','member','active','now');
INSERT INTO group_members
 (owner_identity_id,owner_did,group_id,user_id,member_did,member_handle,anchor_kind,anchor_value,handle_binding_generation,status,last_synced_at) VALUES
 ('owner','did:wba:example.com:alice:e1_new','did:group','alice','did:wba:example.com:alice:e1_old','alice','handle','alice','2','active','now');
"#,
        )
        .unwrap();
        drop(db);
        let mut lookup = crate::directory::HandleLookupResult {
            handle: crate::ids::Handle::parse("alice.example.com", "").unwrap(),
            did: crate::ids::Did::parse("did:wba:example.com:alice:e1_new").unwrap(),
            user_id: "user-alice".to_owned(),
            domain: Some("example.com".to_owned()),
            status: Some("active".to_owned()),
            binding_generation: Some("2".to_owned()),
            profile: None,
            warnings: Vec::new(),
        };

        assert_eq!(
            reconcile_missing_recovery_jobs(
                &sqlite_path,
                "owner",
                "alice.example.com",
                "did:wba:example.com:alice:e1_new",
                &lookup,
            )
            .unwrap(),
            0,
            "an equal generation must not create a rollback/no-op job"
        );

        lookup.binding_generation = None;
        assert!(reconcile_missing_recovery_jobs(
            &sqlite_path,
            "owner",
            "alice.example.com",
            "did:wba:example.com:alice:e1_new",
            &lookup,
        )
        .is_err());
        lookup.binding_generation = Some("03".to_owned());
        assert!(reconcile_missing_recovery_jobs(
            &sqlite_path,
            "owner",
            "alice.example.com",
            "did:wba:example.com:alice:e1_new",
            &lookup,
        )
        .is_err());
        lookup.binding_generation = Some("3".to_owned());
        lookup.status = Some("revoked".to_owned());
        assert!(reconcile_missing_recovery_jobs(
            &sqlite_path,
            "owner",
            "alice.example.com",
            "did:wba:example.com:alice:e1_new",
            &lookup,
        )
        .is_err());
        lookup.status = Some("active".to_owned());
        lookup.did = crate::ids::Did::parse("did:wba:example.com:alice:e1_other").unwrap();
        assert!(reconcile_missing_recovery_jobs(
            &sqlite_path,
            "owner",
            "alice.example.com",
            "did:wba:example.com:alice:e1_new",
            &lookup,
        )
        .is_err());
        lookup.did = crate::ids::Did::parse("did:wba:other.test:alice:e1_new").unwrap();
        assert!(reconcile_missing_recovery_jobs(
            &sqlite_path,
            "owner",
            "alice.example.com",
            "did:wba:other.test:alice:e1_new",
            &lookup,
        )
        .is_err());
        lookup.did = crate::ids::Did::parse("did:wba:example.com:alice:e1_new").unwrap();

        assert_eq!(
            reconcile_missing_recovery_jobs(
                &sqlite_path,
                "owner",
                "alice.example.com",
                "did:wba:example.com:alice:e1_new",
                &lookup,
            )
            .unwrap(),
            1
        );
        assert_eq!(
            reconcile_missing_recovery_jobs(
                &sqlite_path,
                "owner",
                "alice.example.com",
                "did:wba:example.com:alice:e1_new",
                &lookup,
            )
            .unwrap(),
            0
        );
    }

    #[test]
    fn rebound_event_creates_owner_job_only_once_and_pauses_send() {
        let db = rusqlite::Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        db.execute("INSERT INTO groups (owner_identity_id,owner_did,group_id,group_did,my_role,membership_status,stored_at) VALUES ('owner','did:owner','did:group','did:group','owner','active','now')", []).unwrap();
        let record = crate::internal::local_state::messages::MessageRecord {
            msg_id: "did:group:9".to_owned(),
            owner_identity_id: "owner".to_owned(),
            owner_did: "did:owner".to_owned(),
            content: serde_json::json!({
                "type":"member_credential_rebound","group_did":"did:group",
                "group_state_version":"9","subject_handle":"alice.example.com",
                "previous_subject_did":"did:old","subject_did":"did:new",
                "handle_binding_generation":"2","sync_event_id":"evt-9"
            })
            .to_string(),
            ..Default::default()
        };
        assert!(project_rebind_event(&db, &record).unwrap());
        assert!(!project_rebind_event(&db, &record).unwrap());
        let mut replay = record.clone();
        replay.msg_id = "did:group:10".to_owned();
        replay.content = replay.content.replace("evt-9", "evt-10");
        assert!(!project_rebind_event(&db, &replay).unwrap());
        let mut stale = replay;
        stale.msg_id = "did:group:11".to_owned();
        stale.content = stale.content.replace("evt-10", "evt-11").replace(
            "\"handle_binding_generation\":\"2\"",
            "\"handle_binding_generation\":\"1\"",
        );
        assert!(!project_rebind_event(&db, &stale).unwrap());
        let phase: String = db
            .query_row("SELECT phase FROM group_rebind_p6_jobs", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(phase, "awaiting_add");
    }

    #[test]
    fn rebound_event_skips_explicit_transport_group() {
        let db = rusqlite::Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        db.execute("INSERT INTO groups (owner_identity_id,owner_did,group_id,group_did,my_role,membership_status,metadata,stored_at) VALUES ('owner','did:owner','did:group','did:group','owner','active','{\"message_security_profile\":\"transport-protected\"}','now')", []).unwrap();
        let record = crate::internal::local_state::messages::MessageRecord {
            msg_id: "evt".to_owned(), owner_identity_id: "owner".to_owned(), owner_did: "did:owner".to_owned(),
            content: serde_json::json!({"type":"member_credential_rebound","group_did":"did:group","group_state_version":"9","subject_handle":"alice.example.com","previous_subject_did":"did:old","subject_did":"did:new","handle_binding_generation":"2"}).to_string(),
            ..Default::default()
        };
        assert!(!project_rebind_event(&db, &record).unwrap());
    }

    #[test]
    fn p4_claim_is_exclusive_and_expired_lease_is_restartable() {
        let mut db = rusqlite::Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        db.execute(
            "INSERT INTO group_rebind_outbox (job_id,owner_identity_id,group_did,member_handle,previous_member_did,new_member_did,binding_generation,phase,created_at,updated_at) VALUES ('job','owner','did:group','alice.example.com','did:old','did:new','2','pending','now','now')",
            [],
        ).unwrap();
        let first = claim_p4_job(&mut db, "owner").unwrap().unwrap();
        assert_eq!(first.phase, "sending");
        assert_eq!(first.attempt_count, 1);
        assert!(claim_p4_job(&mut db, "owner").unwrap().is_none());
        db.execute(
            "UPDATE group_rebind_outbox SET lease_expires_at='1970-01-01T00:00:00Z'",
            [],
        )
        .unwrap();
        let resumed = claim_p4_job(&mut db, "owner").unwrap().unwrap();
        assert_eq!(resumed.attempt_count, 2);
    }

    #[test]
    fn p6_claim_preserves_work_phase_and_prevents_two_workers() {
        let mut db = rusqlite::Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        db.execute(
            "INSERT INTO group_rebind_p6_jobs (job_id,owner_identity_id,group_did,event_id,member_handle,previous_member_did,new_member_did,binding_generation,group_state_ref_json,phase,created_at,updated_at) VALUES ('job','owner','did:group','evt','alice.example.com','did:old','did:new','2','{}','add_repair','now','now')",
            [],
        ).unwrap();
        let first = claim_p6_job(&mut db, "owner").unwrap().unwrap();
        assert_eq!(first.phase, "add_repair");
        assert!(claim_p6_job(&mut db, "owner").unwrap().is_none());
        db.execute(
            "UPDATE group_rebind_p6_jobs SET lease_expires_at='1970-01-01T00:00:00Z'",
            [],
        )
        .unwrap();
        assert_eq!(
            claim_p6_job(&mut db, "owner").unwrap().unwrap().phase,
            "add_repair"
        );
    }

    #[test]
    fn recovered_non_owner_waiting_for_owner_pauses_send() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.sqlite3");
        let db = crate::internal::local_state::open_writable(&path).unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        db.execute(
            "INSERT INTO group_rebind_outbox (job_id,owner_identity_id,group_did,member_handle,previous_member_did,new_member_did,binding_generation,phase,created_at,updated_at) VALUES ('job','owner','did:group','alice.example.com','did:old','did:new','2','awaiting_p6','now','now')",
            [],
        ).unwrap();
        drop(db);
        assert!(is_group_send_paused(&path, "owner", "did:group").unwrap());
        assert_eq!(paused_groups(&path, "owner").unwrap(), vec!["did:group"]);
        // Local send readiness after Add is not proof that owner Remove completed.
        assert!(is_group_send_paused(&path, "owner", "did:group").unwrap());
    }

    #[test]
    fn recovery_items_keep_latest_status_per_group_layer() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.sqlite3");
        let db = crate::internal::local_state::open_writable(&path).unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        db.execute("INSERT INTO group_rebind_outbox (job_id,owner_identity_id,group_did,member_handle,previous_member_did,new_member_did,binding_generation,phase,created_at,updated_at) VALUES ('old','owner','did:group','alice.example.com','did:old','did:new','2','complete','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z')", []).unwrap();
        db.execute("INSERT INTO group_rebind_outbox (job_id,owner_identity_id,group_did,member_handle,previous_member_did,new_member_did,binding_generation,phase,created_at,updated_at) VALUES ('new','owner','did:group','alice.example.com','did:new','did:newer','3','awaiting_p6','2026-01-02T00:00:00Z','2026-01-02T00:00:00Z')", []).unwrap();
        db.execute("INSERT INTO group_rebind_p6_jobs (job_id,owner_identity_id,group_did,event_id,member_handle,previous_member_did,new_member_did,binding_generation,group_state_ref_json,phase,last_error_detail,created_at,updated_at) VALUES ('p6','owner','did:group','evt','alice.example.com','did:new','did:newer','3','{}','blocked','sensitive transport detail','2026-01-03T00:00:00Z','2026-01-03T00:00:00Z')", []).unwrap();
        drop(db);

        let items = recovery_items(&path, "owner").unwrap();
        assert_eq!(items.len(), 2);
        let p4 = items.iter().find(|item| item.layer == "p4").unwrap();
        assert_eq!(p4.phase, "awaiting_p6");
        assert!(!p4.blocked);
        let p6 = items.iter().find(|item| item.layer == "p6").unwrap();
        assert_eq!(p6.phase, "blocked");
        assert!(p6.blocked);
    }

    #[test]
    fn add_applied_and_blocked_jobs_stay_paused_until_remove_completes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.sqlite3");
        let db = crate::internal::local_state::open_writable(&path).unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        db.execute("INSERT INTO group_rebind_p6_jobs (job_id,owner_identity_id,group_did,event_id,member_handle,previous_member_did,new_member_did,binding_generation,group_state_ref_json,phase,created_at,updated_at) VALUES ('job','owner','did:group','evt','alice.example.com','did:old','did:new','2','{}','awaiting_remove','now','now')", []).unwrap();
        drop(db);
        assert!(is_group_send_paused(&path, "owner", "did:group").unwrap());
        update_p6_job(&path, "job", "blocked", Some("key package missing")).unwrap();
        assert!(is_group_send_paused(&path, "owner", "did:group").unwrap());
        update_p6_job(&path, "job", "complete", None).unwrap();
        assert!(!is_group_send_paused(&path, "owner", "did:group").unwrap());
    }

    #[test]
    fn only_matching_verified_remove_notice_completes_recovered_member() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.sqlite3");
        let db = crate::internal::local_state::open_writable(&path).unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        db.execute("INSERT INTO group_rebind_outbox (job_id,owner_identity_id,group_did,member_handle,previous_member_did,new_member_did,binding_generation,phase,group_state_ref_json,created_at,updated_at) VALUES ('job','owner','did:group','alice.example.com','did:old','did:new','2','awaiting_p6','{\"group_did\":\"did:group\",\"group_state_version\":\"9\"}','now','now')", []).unwrap();
        drop(db);
        let add_applied = vec!["did:old".to_owned(), "did:new".to_owned()];
        let remove_applied = vec!["did:new".to_owned()];
        assert!(!complete_from_verified_remove_notice(
            &path,
            "owner",
            "did:group",
            "did:new",
            "active",
            "9",
            &add_applied
        )
        .unwrap());
        assert!(is_group_send_paused(&path, "owner", "did:group").unwrap());
        assert!(!complete_from_verified_remove_notice(
            &path,
            "owner",
            "did:group",
            "did:old",
            "removed",
            "8",
            &add_applied
        )
        .unwrap());
        assert!(!complete_from_verified_remove_notice(
            &path,
            "owner",
            "did:group",
            "did:old",
            "removed",
            "9",
            &add_applied
        )
        .unwrap());
        assert!(complete_from_verified_remove_notice(
            &path,
            "owner",
            "did:group",
            "did:old",
            "removed",
            "9",
            &remove_applied
        )
        .unwrap());
        assert!(!is_group_send_paused(&path, "owner", "did:group").unwrap());
        assert!(!complete_from_verified_remove_notice(
            &path,
            "owner",
            "did:group",
            "did:old",
            "removed",
            "9",
            &remove_applied
        )
        .unwrap());
    }

    #[test]
    fn group_security_metadata_is_structural_and_unknown_fails_closed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.sqlite3");
        let db = crate::internal::local_state::open_writable(&path).unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        let insert = |group: &str, metadata: Option<&str>| {
            db.execute(
                "INSERT INTO groups (owner_identity_id,owner_did,group_id,group_did,metadata,stored_at) VALUES ('owner','did:owner',?1,?1,?2,'now')",
                rusqlite::params![group, metadata],
            ).unwrap();
        };
        insert(
            "did:transport",
            Some(r#"{"message_security_profile":"transport-protected"}"#),
        );
        insert(
            "did:e2ee",
            Some(r#"{"group_policy":{"message_security_profile":"group-e2ee"}}"#),
        );
        insert(
            "did:required-transport",
            Some(r#"{"required_security_profile":"transport-protected"}"#),
        );
        insert(
            "did:required-e2ee",
            Some(r#"{"required_security_profile":"group-e2ee"}"#),
        );
        insert(
            "did:conflicting",
            Some(
                r#"{"required_security_profile":"group-e2ee","message_security_profile":"transport-protected"}"#,
            ),
        );
        insert("did:malformed", Some("group-e2ee"));
        insert(
            "did:malformed-fields",
            Some(
                r#"{"required_security_profile":42,"group_policy":{"message_security_profile":42}}"#,
            ),
        );
        insert("did:missing", None);
        drop(db);
        assert!(!group_uses_e2ee(&path, "owner", "did:transport").unwrap());
        assert!(group_uses_e2ee(&path, "owner", "did:e2ee").unwrap());
        assert!(!group_uses_e2ee(&path, "owner", "did:required-transport").unwrap());
        assert!(group_uses_e2ee(&path, "owner", "did:required-e2ee").unwrap());
        assert!(group_uses_e2ee(&path, "owner", "did:conflicting").unwrap());
        assert!(group_uses_e2ee(&path, "owner", "did:malformed").unwrap());
        assert!(group_uses_e2ee(&path, "owner", "did:malformed-fields").unwrap());
        assert!(group_uses_e2ee(&path, "owner", "did:missing").unwrap());
    }

    #[test]
    fn only_explicit_transport_profile_completes_awaiting_p6_job() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.sqlite3");
        let db = crate::internal::local_state::open_writable(&path).unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        for (group, metadata) in [
            (
                "did:transport",
                r#"{"required_security_profile":"transport-protected"}"#,
            ),
            ("did:e2ee", r#"{"required_security_profile":"group-e2ee"}"#),
            ("did:unknown", r#"{}"#),
        ] {
            db.execute(
                "INSERT INTO groups (owner_identity_id,owner_did,group_id,group_did,metadata,stored_at) VALUES ('owner','did:owner',?1,?1,?2,'now')",
                rusqlite::params![group, metadata],
            )
            .unwrap();
            db.execute(
                "INSERT INTO group_rebind_outbox (job_id,owner_identity_id,group_did,member_handle,previous_member_did,new_member_did,binding_generation,phase,created_at,updated_at) VALUES (?1,'owner',?2,'alice.example.com','did:old','did:new','2','awaiting_p6','now','now')",
                rusqlite::params![format!("job-{group}"), group],
            )
            .unwrap();
        }
        db.execute(
            "INSERT INTO group_rebind_outbox (job_id,owner_identity_id,group_did,member_handle,previous_member_did,new_member_did,binding_generation,phase,created_at,updated_at) VALUES ('job-transport-new','owner','did:transport','alice.example.com','did:new','did:newer','3','awaiting_p6','later','later')",
            [],
        )
        .unwrap();
        drop(db);

        assert_eq!(
            awaiting_p6_groups(&path, "owner").unwrap(),
            vec!["did:e2ee", "did:transport", "did:unknown"]
        );
        assert_eq!(
            complete_transport_p4_jobs(&path, "owner", "did:transport", 1).unwrap(),
            1
        );
        assert!(awaiting_p6_groups(&path, "owner")
            .unwrap()
            .contains(&"did:transport".to_owned()));
        assert_eq!(
            complete_transport_p4_jobs(&path, "owner", "did:transport", 1).unwrap(),
            1
        );
        assert_eq!(
            complete_transport_p4_jobs(&path, "owner", "did:e2ee", 1).unwrap(),
            0
        );
        assert_eq!(
            complete_transport_p4_jobs(&path, "owner", "did:unknown", 1).unwrap(),
            0
        );
        assert_eq!(
            awaiting_p6_groups(&path, "owner").unwrap(),
            vec!["did:e2ee", "did:unknown"]
        );
    }
}
