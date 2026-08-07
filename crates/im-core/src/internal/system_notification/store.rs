use chrono::{DateTime, Duration, SecondsFormat, Utc};
use rusqlite::{params, Connection, OptionalExtension, Transaction};

use super::verify::VerifiedSystemNotification;

pub(crate) const SYSTEM_NOTIFICATION_SCHEMA_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS system_notification_receipts (
    owner_identity_id TEXT NOT NULL,
    owner_did TEXT NOT NULL,
    protocol_device_id TEXT NOT NULL,
    event_id TEXT NOT NULL,
    join_session_id TEXT NOT NULL,
    session_revision INTEGER NOT NULL,
    payload_hash TEXT NOT NULL,
    proof_hash TEXT NOT NULL,
    first_seen_at TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    PRIMARY KEY (owner_identity_id, event_id)
);

CREATE INDEX IF NOT EXISTS idx_system_notification_receipts_retention
ON system_notification_receipts(owner_identity_id, expires_at);

CREATE TABLE IF NOT EXISTS system_notification_join_state (
    owner_identity_id TEXT NOT NULL,
    owner_did TEXT NOT NULL,
    protocol_device_id TEXT NOT NULL,
    did TEXT NOT NULL,
    join_session_id TEXT NOT NULL,
    current_event_id TEXT NOT NULL,
    notification_type TEXT NOT NULL,
    state TEXT NOT NULL,
    session_revision INTEGER NOT NULL,
    payload_hash TEXT NOT NULL,
    verified_notification_json TEXT NOT NULL,
    initial_join_request_json TEXT,
    issued_at TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    first_seen_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    terminal INTEGER NOT NULL,
    retain_until TEXT,
    PRIMARY KEY (owner_identity_id, did, join_session_id)
);

CREATE INDEX IF NOT EXISTS idx_system_notification_join_state_list
ON system_notification_join_state(owner_identity_id, terminal, updated_at DESC);
"#;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SystemNotificationApplyInput {
    pub(crate) owner_identity_id: String,
    pub(crate) owner_did: String,
    pub(crate) protocol_device_id: String,
    pub(crate) verified: VerifiedSystemNotification,
    pub(crate) received_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SystemNotificationApplyOutcome {
    Applied(crate::system_notifications::SystemNotificationSnapshot),
    Duplicate,
    IgnoredOlderRevision,
    NoopSameRevision,
}

#[derive(Debug)]
struct CurrentState {
    state: crate::system_notifications::SystemNotificationState,
    revision: u64,
    payload_hash: String,
    terminal: bool,
    first_seen_at: String,
}

pub(crate) fn create_schema(connection: &Connection) -> crate::ImResult<()> {
    connection
        .execute_batch(SYSTEM_NOTIFICATION_SCHEMA_SQL)
        .map_err(crate::internal::local_state::local_state_unavailable)
}

pub(crate) fn apply(
    connection: &mut Connection,
    input: SystemNotificationApplyInput,
) -> crate::ImResult<SystemNotificationApplyOutcome> {
    let transaction = connection
        .transaction()
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    let outcome = apply_transaction(&transaction, &input)?;
    transaction
        .commit()
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    Ok(outcome)
}

pub(crate) fn apply_transaction(
    transaction: &Transaction<'_>,
    input: &SystemNotificationApplyInput,
) -> crate::ImResult<SystemNotificationApplyOutcome> {
    let notification = &input.verified.envelope.notification;
    let existing_receipt = transaction
        .query_row(
            "SELECT payload_hash, proof_hash FROM system_notification_receipts
             WHERE owner_identity_id = ?1 AND event_id = ?2",
            params![input.owner_identity_id, notification.event_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    if let Some((payload_hash, proof_hash)) = existing_receipt {
        if payload_hash == input.verified.payload_hash && proof_hash == input.verified.proof_hash {
            return Ok(SystemNotificationApplyOutcome::Duplicate);
        }
        return Err(revision_conflict(
            "event_id was already consumed with different canonical content",
        ));
    }

    let current = transaction
        .query_row(
            "SELECT state, session_revision, payload_hash, terminal, first_seen_at
             FROM system_notification_join_state
             WHERE owner_identity_id = ?1 AND did = ?2 AND join_session_id = ?3",
            params![
                input.owner_identity_id,
                notification.did,
                notification.join_session_id
            ],
            |row| {
                let state = row.get::<_, String>(0)?;
                let revision = row.get::<_, i64>(1)?;
                let payload_hash = row.get::<_, String>(2)?;
                let terminal = row.get::<_, i64>(3)? != 0;
                let first_seen_at = row.get::<_, String>(4)?;
                Ok((state, revision, payload_hash, terminal, first_seen_at))
            },
        )
        .optional()
        .map_err(crate::internal::local_state::local_state_unavailable)?
        .map(|(state, revision, payload_hash, terminal, first_seen_at)| {
            let revision = u64::try_from(revision).map_err(|_| persisted_state_invalid())?;
            Ok::<CurrentState, crate::ImError>(CurrentState {
                state: crate::system_notifications::SystemNotificationState::parse(&state)?,
                revision,
                payload_hash,
                terminal,
                first_seen_at,
            })
        })
        .transpose()?;

    let projected_outcome = reducer_outcome(
        current.as_ref(),
        notification.state,
        notification.session_revision,
        &input.verified.payload_hash,
    )?;
    insert_receipt(transaction, input)?;
    persist_initial_join_request(transaction, input)?;
    if projected_outcome != ReducerOutcome::Apply {
        return Ok(match projected_outcome {
            ReducerOutcome::IgnoreOlder => SystemNotificationApplyOutcome::IgnoredOlderRevision,
            ReducerOutcome::Noop => SystemNotificationApplyOutcome::NoopSameRevision,
            ReducerOutcome::Apply => unreachable!(),
        });
    }

    let first_seen_at = current
        .as_ref()
        .map(|current| current.first_seen_at.clone())
        .unwrap_or_else(|| format_time(input.received_at));
    let terminal = notification.state.is_terminal();
    let retain_until = terminal.then(|| format_time(input.received_at + Duration::days(30)));
    let verified_notification_json =
        serde_json::to_string(&notification.canonical_value).map_err(serialization)?;
    let initial_join_request_json = notification
        .initial_join_request
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .map_err(serialization)?;
    transaction
        .execute(
            "INSERT INTO system_notification_join_state (
                owner_identity_id, owner_did, protocol_device_id, did, join_session_id,
                current_event_id, notification_type, state, session_revision, payload_hash,
                verified_notification_json, initial_join_request_json, issued_at, expires_at,
                first_seen_at, updated_at, terminal, retain_until
             ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18
             )
             ON CONFLICT(owner_identity_id, did, join_session_id) DO UPDATE SET
                owner_did=excluded.owner_did,
                protocol_device_id=excluded.protocol_device_id,
                current_event_id=excluded.current_event_id,
                notification_type=excluded.notification_type,
                state=excluded.state,
                session_revision=excluded.session_revision,
                payload_hash=excluded.payload_hash,
                verified_notification_json=excluded.verified_notification_json,
                initial_join_request_json=COALESCE(
                    system_notification_join_state.initial_join_request_json,
                    excluded.initial_join_request_json
                ),
                issued_at=excluded.issued_at,
                expires_at=excluded.expires_at,
                updated_at=excluded.updated_at,
                terminal=excluded.terminal,
                retain_until=excluded.retain_until",
            params![
                input.owner_identity_id,
                input.owner_did,
                input.protocol_device_id,
                notification.did,
                notification.join_session_id,
                notification.event_id,
                notification.kind.as_wire_type(),
                notification.state.as_str(),
                i64::try_from(notification.session_revision).map_err(|_| invalid_revision())?,
                input.verified.payload_hash,
                verified_notification_json,
                initial_join_request_json,
                notification.issued_at,
                notification.expires_at,
                first_seen_at,
                format_time(input.received_at),
                i64::from(terminal),
                retain_until,
            ],
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?;

    Ok(SystemNotificationApplyOutcome::Applied(snapshot(
        notification,
        first_seen_at,
    )))
}

fn persist_initial_join_request(
    transaction: &Transaction<'_>,
    input: &SystemNotificationApplyInput,
) -> crate::ImResult<()> {
    let Some(join_request) = input
        .verified
        .envelope
        .notification
        .initial_join_request
        .as_ref()
    else {
        return Ok(());
    };
    let join_request_json = serde_json::to_string(join_request).map_err(serialization)?;
    transaction
        .execute(
            "UPDATE system_notification_join_state
             SET initial_join_request_json = COALESCE(initial_join_request_json, ?1)
             WHERE owner_identity_id = ?2 AND did = ?3 AND join_session_id = ?4",
            params![
                join_request_json,
                input.owner_identity_id,
                input.verified.envelope.notification.did,
                input.verified.envelope.notification.join_session_id,
            ],
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    Ok(())
}

fn insert_receipt(
    transaction: &Transaction<'_>,
    input: &SystemNotificationApplyInput,
) -> crate::ImResult<()> {
    let notification = &input.verified.envelope.notification;
    transaction
        .execute(
            "INSERT INTO system_notification_receipts (
                owner_identity_id, owner_did, protocol_device_id, event_id, join_session_id,
                session_revision, payload_hash, proof_hash, first_seen_at, expires_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                input.owner_identity_id,
                input.owner_did,
                input.protocol_device_id,
                notification.event_id,
                notification.join_session_id,
                i64::try_from(notification.session_revision).map_err(|_| invalid_revision())?,
                input.verified.payload_hash,
                input.verified.proof_hash,
                format_time(input.received_at),
                notification.expires_at,
            ],
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    Ok(())
}

pub(crate) fn list(
    connection: &Connection,
    owner_identity_id: &str,
    owner_did: &str,
    protocol_device_id: &str,
    include_terminal: bool,
    limit: u32,
) -> crate::ImResult<Vec<crate::system_notifications::SystemNotificationSnapshot>> {
    let mut statement = connection
        .prepare(
            "SELECT current_event_id, did, join_session_id, notification_type, state,
                    session_revision, issued_at, expires_at, first_seen_at, terminal
             FROM system_notification_join_state
             WHERE owner_identity_id = ?1 AND owner_did = ?2 AND protocol_device_id = ?3
               AND (
                    (terminal = 0 AND julianday(expires_at) > julianday('now'))
                    OR (?4 = 1 AND terminal = 1)
               )
             ORDER BY updated_at DESC, join_session_id ASC
             LIMIT ?5",
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    let rows = statement
        .query_map(
            params![
                owner_identity_id,
                owner_did,
                protocol_device_id,
                i64::from(include_terminal),
                i64::from(limit)
            ],
            snapshot_from_row,
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(crate::internal::local_state::local_state_unavailable)?
        .into_iter()
        .map(snapshot_from_persisted)
        .collect()
}

pub(crate) fn get_by_event_id(
    connection: &Connection,
    owner_identity_id: &str,
    owner_did: &str,
    protocol_device_id: &str,
    event_id: &str,
) -> crate::ImResult<Option<crate::system_notifications::SystemNotificationSnapshot>> {
    connection
        .query_row(
            "SELECT current_event_id, did, join_session_id, notification_type, state,
                    session_revision, issued_at, expires_at, first_seen_at, terminal
             FROM system_notification_join_state
             WHERE owner_identity_id = ?1 AND owner_did = ?2
               AND protocol_device_id = ?3 AND current_event_id = ?4
               AND (terminal = 1 OR julianday(expires_at) > julianday('now'))",
            params![owner_identity_id, owner_did, protocol_device_id, event_id],
            snapshot_from_row,
        )
        .optional()
        .map_err(crate::internal::local_state::local_state_unavailable)?
        .map(snapshot_from_persisted)
        .transpose()
}

pub(crate) fn get_verified_by_session(
    connection: &Connection,
    owner_identity_id: &str,
    owner_did: &str,
    protocol_device_id: &str,
    join_session_id: &str,
) -> crate::ImResult<Option<super::wire::JoinNotification>> {
    connection
        .query_row(
            "SELECT verified_notification_json, initial_join_request_json
             FROM system_notification_join_state
             WHERE owner_identity_id = ?1 AND owner_did = ?2
               AND protocol_device_id = ?3 AND join_session_id = ?4
               AND (terminal = 1 OR julianday(expires_at) > julianday('now'))
             ORDER BY updated_at DESC LIMIT 1",
            params![
                owner_identity_id,
                owner_did,
                protocol_device_id,
                join_session_id
            ],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)),
        )
        .optional()
        .map_err(crate::internal::local_state::local_state_unavailable)?
        .map(|(current, initial)| parse_verified_json(current, initial))
        .transpose()
}

pub(crate) fn list_verified(
    connection: &Connection,
    owner_identity_id: &str,
    owner_did: &str,
    protocol_device_id: &str,
    include_terminal: bool,
    limit: u32,
) -> crate::ImResult<Vec<super::wire::JoinNotification>> {
    let mut statement = connection
        .prepare(
            "SELECT verified_notification_json, initial_join_request_json
             FROM system_notification_join_state
             WHERE owner_identity_id = ?1 AND owner_did = ?2 AND protocol_device_id = ?3
               AND (
                    (terminal = 0 AND julianday(expires_at) > julianday('now'))
                    OR (?4 = 1 AND terminal = 1)
               )
             ORDER BY updated_at DESC, join_session_id ASC
             LIMIT ?5",
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    let rows = statement
        .query_map(
            params![
                owner_identity_id,
                owner_did,
                protocol_device_id,
                i64::from(include_terminal),
                i64::from(limit)
            ],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)),
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(crate::internal::local_state::local_state_unavailable)?
        .into_iter()
        .map(|(current, initial)| parse_verified_json(current, initial))
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReducerOutcome {
    Apply,
    IgnoreOlder,
    Noop,
}

fn reducer_outcome(
    current: Option<&CurrentState>,
    next_state: crate::system_notifications::SystemNotificationState,
    next_revision: u64,
    next_payload_hash: &str,
) -> crate::ImResult<ReducerOutcome> {
    validate_state_revision(next_state, next_revision)?;
    let Some(current) = current else {
        return Ok(ReducerOutcome::Apply);
    };
    if next_revision < current.revision {
        return Ok(ReducerOutcome::IgnoreOlder);
    }
    if next_revision == current.revision {
        if next_state == current.state && next_payload_hash == current.payload_hash {
            return Ok(ReducerOutcome::Noop);
        }
        return Err(revision_conflict(
            "same session revision has different state or canonical payload",
        ));
    }
    if current.terminal || current.state.is_terminal() {
        return Err(revision_conflict(
            "terminal notification state cannot reopen",
        ));
    }
    if !reachable(current.state, next_state, next_revision) {
        return Err(revision_conflict(
            "higher revision is not reachable from current notification state",
        ));
    }
    Ok(ReducerOutcome::Apply)
}

fn validate_state_revision(
    state: crate::system_notifications::SystemNotificationState,
    revision: u64,
) -> crate::ImResult<()> {
    use crate::system_notifications::SystemNotificationState as S;
    let valid = match state {
        S::Pending => revision == 1,
        S::ChallengeSent => revision == 2,
        S::ResponseVerified => revision == 3,
        S::Consumed => revision == 4,
        S::Cancelled | S::Rejected | S::Expired => (2..=4).contains(&revision),
    };
    if valid {
        Ok(())
    } else {
        Err(invalid_revision())
    }
}

fn reachable(
    current: crate::system_notifications::SystemNotificationState,
    next: crate::system_notifications::SystemNotificationState,
    next_revision: u64,
) -> bool {
    use crate::system_notifications::SystemNotificationState as S;
    match current {
        S::Pending => matches!(
            next,
            S::ChallengeSent
                | S::ResponseVerified
                | S::Consumed
                | S::Cancelled
                | S::Rejected
                | S::Expired
        ),
        S::ChallengeSent => {
            matches!(
                next,
                S::ResponseVerified | S::Consumed | S::Cancelled | S::Rejected | S::Expired
            ) && next_revision >= 3
        }
        S::ResponseVerified => {
            matches!(next, S::Consumed | S::Cancelled | S::Rejected | S::Expired)
                && next_revision == 4
        }
        S::Consumed | S::Cancelled | S::Rejected | S::Expired => false,
    }
}

fn snapshot(
    notification: &super::wire::JoinNotification,
    first_seen_at: String,
) -> crate::system_notifications::SystemNotificationSnapshot {
    crate::system_notifications::SystemNotificationSnapshot {
        event_id: notification.event_id.clone(),
        did: notification.did.clone(),
        join_session_id: notification.join_session_id.clone(),
        kind: notification.kind,
        state: notification.state,
        session_revision: notification.session_revision,
        issued_at: notification.issued_at.clone(),
        expires_at: notification.expires_at.clone(),
        first_seen_at,
        terminal: notification.state.is_terminal(),
    }
}

type PersistedSnapshot = (
    String,
    String,
    String,
    String,
    String,
    i64,
    String,
    String,
    String,
    bool,
);

fn snapshot_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<PersistedSnapshot> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
        row.get(7)?,
        row.get(8)?,
        row.get::<_, i64>(9)? != 0,
    ))
}

fn snapshot_from_persisted(
    row: PersistedSnapshot,
) -> crate::ImResult<crate::system_notifications::SystemNotificationSnapshot> {
    Ok(crate::system_notifications::SystemNotificationSnapshot {
        event_id: row.0,
        did: row.1,
        join_session_id: row.2,
        kind: crate::system_notifications::SystemNotificationKind::parse(&row.3)?,
        state: crate::system_notifications::SystemNotificationState::parse(&row.4)?,
        session_revision: u64::try_from(row.5).map_err(|_| persisted_state_invalid())?,
        issued_at: row.6,
        expires_at: row.7,
        first_seen_at: row.8,
        terminal: row.9,
    })
}

fn parse_verified_json(
    raw: String,
    initial_join_request_json: Option<String>,
) -> crate::ImResult<super::wire::JoinNotification> {
    let value = serde_json::from_str(&raw).map_err(serialization)?;
    let mut notification = super::wire::parse_verified_notification(value)?;
    if let Some(raw) = initial_join_request_json {
        let request = serde_json::from_str(&raw).map_err(serialization)?;
        notification.initial_join_request = Some(request);
    }
    Ok(notification)
}

fn format_time(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn revision_conflict(message: &str) -> crate::ImError {
    crate::ImError::Service {
        status_code: None,
        code: Some("system.notification.revision_conflict".to_owned()),
        message: message.to_owned(),
        data: None,
    }
}

fn invalid_revision() -> crate::ImError {
    super::wire::invalid("system notification state/revision pair is invalid")
}

fn persisted_state_invalid() -> crate::ImError {
    crate::ImError::LocalStateUnavailable {
        detail: "persisted system notification state is invalid".to_owned(),
    }
}

fn serialization(error: impl std::fmt::Display) -> crate::ImError {
    crate::ImError::Serialization {
        detail: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::system_notifications::SystemNotificationState as S;

    fn current(state: S, revision: u64, payload_hash: &str) -> CurrentState {
        CurrentState {
            state,
            revision,
            payload_hash: payload_hash.to_owned(),
            terminal: state.is_terminal(),
            first_seen_at: "2026-07-23T02:00:00Z".to_owned(),
        }
    }

    #[test]
    fn reducer_uses_none_revision_zero_baseline_and_allows_reachable_gaps() {
        assert_eq!(
            reducer_outcome(None, S::Pending, 1, "sha256:a").unwrap(),
            ReducerOutcome::Apply
        );
        assert_eq!(
            reducer_outcome(
                Some(&current(S::Pending, 1, "sha256:a")),
                S::ResponseVerified,
                3,
                "sha256:b",
            )
            .unwrap(),
            ReducerOutcome::Apply
        );
        assert_eq!(
            reducer_outcome(
                Some(&current(S::ChallengeSent, 2, "sha256:b")),
                S::Consumed,
                4,
                "sha256:c",
            )
            .unwrap(),
            ReducerOutcome::Apply
        );
    }

    #[test]
    fn reducer_dedupes_and_rejects_same_revision_conflict_or_terminal_reopen() {
        let pending = current(S::Pending, 1, "sha256:a");
        assert_eq!(
            reducer_outcome(Some(&pending), S::Pending, 1, "sha256:a").unwrap(),
            ReducerOutcome::Noop
        );
        assert!(reducer_outcome(Some(&pending), S::Pending, 1, "sha256:b").is_err());
        let terminal = current(S::Rejected, 2, "sha256:t");
        assert!(reducer_outcome(Some(&terminal), S::ResponseVerified, 3, "sha256:x").is_err());
    }

    #[test]
    fn reducer_ignores_lower_revision_without_reopening_projection() {
        let response = current(S::ResponseVerified, 3, "sha256:r");
        assert_eq!(
            reducer_outcome(Some(&response), S::ChallengeSent, 2, "sha256:c").unwrap(),
            ReducerOutcome::IgnoreOlder
        );
    }

    #[test]
    fn schema_is_secret_free_and_has_receipt_and_session_projection() {
        let connection = Connection::open_in_memory().unwrap();
        create_schema(&connection).unwrap();
        for table in [
            "system_notification_receipts",
            "system_notification_join_state",
        ] {
            let count: i64 = connection
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
                    [table],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(count, 1);
        }
        let columns = connection
            .prepare("PRAGMA table_info(system_notification_join_state)")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        for forbidden in [
            "join_session_token",
            "sas",
            "private_key",
            "origin_proof",
            "raw_envelope",
        ] {
            assert!(!columns.iter().any(|column| column == forbidden));
        }
    }

    #[test]
    fn system_notification_reads_are_scoped_to_exact_protocol_device() {
        let connection = Connection::open_in_memory().unwrap();
        create_schema(&connection).unwrap();
        connection
            .execute(
                "INSERT INTO system_notification_join_state (
                    owner_identity_id, owner_did, protocol_device_id, did, join_session_id,
                    current_event_id, notification_type, state, session_revision, payload_hash,
                    verified_notification_json, issued_at, expires_at, first_seen_at, updated_at,
                    terminal, retain_until
                 ) VALUES (
                    'owner-1', 'did:example:owner', 'dev-a', 'did:example:owner', 'join-a',
                    'evt-a', 'awiki.device.join-requested.v1', 'pending', 1, 'sha256:payload',
                    '{}', '2026-07-23T02:00:00Z', '2099-07-23T02:10:00Z',
                    '2026-07-23T02:00:01Z', '2026-07-23T02:00:01Z', 0, NULL
                 )",
                [],
            )
            .unwrap();

        assert_eq!(
            list(
                &connection,
                "owner-1",
                "did:example:owner",
                "dev-a",
                false,
                10,
            )
            .unwrap()
            .len(),
            1
        );
        assert!(list(
            &connection,
            "owner-1",
            "did:example:owner",
            "dev-b",
            false,
            10,
        )
        .unwrap()
        .is_empty());
        assert!(get_by_event_id(
            &connection,
            "owner-1",
            "did:example:owner",
            "dev-b",
            "evt-a",
        )
        .unwrap()
        .is_none());
    }

    #[test]
    fn initial_join_request_survives_later_projection_and_late_lower_revision() {
        let fixture_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/multi_device_v1/system-notification-v1.json");
        let fixture: serde_json::Value =
            serde_json::from_slice(&std::fs::read(fixture_path).unwrap()).unwrap();
        let mut requested = fixture["p3_vector"]["request"].clone();
        requested["method"] = serde_json::Value::String("direct.incoming".to_owned());
        requested["params"]["body"]["payload"]["expires_at"] =
            serde_json::Value::String("2099-07-23T02:10:00Z".to_owned());
        let requested_envelope = super::super::wire::parse_envelope(&requested).unwrap();

        let mut claimed = requested.clone();
        claimed["params"]["meta"]["operation_id"] =
            serde_json::Value::String("evt-claimed".to_owned());
        claimed["params"]["meta"]["message_id"] =
            serde_json::Value::String("evt-claimed".to_owned());
        claimed["params"]["body"]["payload"]["type"] =
            serde_json::Value::String("awiki.device.join-claimed.v1".to_owned());
        claimed["params"]["body"]["payload"]["event_id"] =
            serde_json::Value::String("evt-claimed".to_owned());
        claimed["params"]["body"]["payload"]["state"] =
            serde_json::Value::String("challenge_sent".to_owned());
        claimed["params"]["body"]["payload"]["session_revision"] = serde_json::json!(2);
        claimed["params"]["body"]["payload"]["payload"] = serde_json::json!({
            "state": "challenge_sent",
            "claimed_by_device_id": "dev-AAECAwQFBgcICQoLDA0ODw",
            "challenge_id": "challenge-1",
            "challenge_expires_at": "2026-07-23T02:10:00Z"
        });
        let claimed_envelope = super::super::wire::parse_envelope(&claimed).unwrap();

        let mut connection = Connection::open_in_memory().unwrap();
        create_schema(&connection).unwrap();
        let received_at = DateTime::parse_from_rfc3339("2026-07-23T02:00:01Z")
            .unwrap()
            .with_timezone(&Utc);
        let input = |verified: VerifiedSystemNotification| SystemNotificationApplyInput {
            owner_identity_id: "owner-1".to_owned(),
            owner_did: "did:wba:example.com:agents:alice:e1_alice".to_owned(),
            protocol_device_id: "dev-admin".to_owned(),
            verified,
            received_at,
        };
        apply(
            &mut connection,
            input(VerifiedSystemNotification {
                envelope: claimed_envelope,
                payload_hash: "sha256:claimed".to_owned(),
                proof_hash: "sha256:claimed-proof".to_owned(),
            }),
        )
        .unwrap();
        assert_eq!(
            apply(
                &mut connection,
                input(VerifiedSystemNotification {
                    envelope: requested_envelope,
                    payload_hash: "sha256:requested".to_owned(),
                    proof_hash: "sha256:requested-proof".to_owned(),
                }),
            )
            .unwrap(),
            SystemNotificationApplyOutcome::IgnoredOlderRevision
        );

        let stored = get_verified_by_session(
            &connection,
            "owner-1",
            "did:wba:example.com:agents:alice:e1_alice",
            "dev-admin",
            "join-oKGio6SlpqeoqaqrrK2urw",
        )
        .unwrap()
        .unwrap();
        assert!(matches!(
            stored.payload,
            super::super::wire::JoinPayload::Claimed(_)
        ));
        assert_eq!(
            stored.initial_join_request.unwrap().device_id,
            "dev-AAECAwQFBgcICQoLDA0ODw"
        );
    }
}
