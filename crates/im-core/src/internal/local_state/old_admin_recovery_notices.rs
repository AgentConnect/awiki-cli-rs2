use rusqlite::{params, Connection, OptionalExtension, Transaction};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

pub(crate) const OLD_ADMIN_RECOVERY_NOTICES_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS old_admin_recovery_notices (
    owner_identity_id   TEXT NOT NULL,
    owner_did           TEXT NOT NULL,
    owner_device_id     TEXT NOT NULL,
    event_id            TEXT NOT NULL,
    source_event_id     TEXT NOT NULL,
    recovery_session_id TEXT NOT NULL,
    handle              TEXT NOT NULL,
    requested_at        TEXT NOT NULL,
    cancellable_until   TEXT NOT NULL,
    dismissed_at        TEXT,
    stored_at           TEXT NOT NULL,
    PRIMARY KEY (owner_identity_id, owner_device_id, event_id),
    UNIQUE (owner_identity_id, owner_device_id, recovery_session_id)
);

CREATE INDEX IF NOT EXISTS old_admin_recovery_notices_active_idx
ON old_admin_recovery_notices (
    owner_identity_id,
    owner_device_id,
    owner_did,
    dismissed_at,
    cancellable_until
);
"#;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OldAdminRecoveryNoticeRecord {
    pub(crate) owner_identity_id: String,
    pub(crate) owner_did: String,
    pub(crate) owner_device_id: String,
    pub(crate) event_id: String,
    pub(crate) source_event_id: String,
    pub(crate) recovery_session_id: String,
    pub(crate) handle: String,
    pub(crate) requested_at: String,
    pub(crate) cancellable_until: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OldAdminRecoveryNoticeScope {
    pub(crate) owner_identity_id: String,
    pub(crate) owner_did: String,
    pub(crate) owner_device_id: String,
}

pub(crate) fn create_schema(connection: &Connection) -> crate::ImResult<()> {
    connection
        .execute_batch(OLD_ADMIN_RECOVERY_NOTICES_SQL)
        .map_err(super::local_state_unavailable)
}

pub(crate) fn upsert_tx(
    transaction: &Transaction<'_>,
    record: &OldAdminRecoveryNoticeRecord,
) -> crate::ImResult<()> {
    validate_record(record)?;

    if let Some(existing) = load_by_event_id(
        transaction,
        &record.owner_identity_id,
        &record.owner_device_id,
        &record.event_id,
    )? {
        if existing == *record {
            return Ok(());
        }
        return Err(projection_conflict(
            "durable recovery notice event projection conflicts with local state",
        ));
    }

    let inserted = transaction
        .execute(
            r#"
INSERT OR IGNORE INTO old_admin_recovery_notices (
    owner_identity_id,
    owner_did,
    owner_device_id,
    event_id,
    source_event_id,
    recovery_session_id,
    handle,
    requested_at,
    cancellable_until,
    dismissed_at,
    stored_at
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, NULL, ?10)"#,
            params![
                record.owner_identity_id,
                record.owner_did,
                record.owner_device_id,
                record.event_id,
                record.source_event_id,
                record.recovery_session_id,
                record.handle,
                record.requested_at,
                record.cancellable_until,
                now_rfc3339(),
            ],
        )
        .map_err(super::local_state_unavailable)?;
    if inserted == 1 {
        return Ok(());
    }

    if let Some(existing) = load_by_event_id(
        transaction,
        &record.owner_identity_id,
        &record.owner_device_id,
        &record.event_id,
    )? {
        if existing == *record {
            return Ok(());
        }
        return Err(projection_conflict(
            "durable recovery notice event projection conflicts with local state",
        ));
    }
    Err(projection_conflict(
        "recovery session is already bound to another durable event",
    ))
}

pub(crate) fn upsert(
    connection: &mut Connection,
    record: &OldAdminRecoveryNoticeRecord,
) -> crate::ImResult<()> {
    crate::internal::local_state::schema::ensure_schema(connection)?;
    let transaction = connection
        .transaction()
        .map_err(super::local_state_unavailable)?;
    upsert_tx(&transaction, record)?;
    transaction.commit().map_err(super::local_state_unavailable)
}

pub(crate) fn list_active(
    connection: &Connection,
    scope: &OldAdminRecoveryNoticeScope,
    now: OffsetDateTime,
) -> crate::ImResult<Vec<OldAdminRecoveryNoticeRecord>> {
    validate_scope(scope)?;
    crate::internal::local_state::schema::ensure_schema(connection)?;
    let mut statement = connection
        .prepare(
            r#"
SELECT owner_identity_id,
       owner_did,
       owner_device_id,
       event_id,
       source_event_id,
       recovery_session_id,
       handle,
       requested_at,
       cancellable_until
FROM old_admin_recovery_notices
WHERE owner_identity_id = ?1
  AND owner_did = ?2
  AND owner_device_id = ?3
  AND dismissed_at IS NULL
ORDER BY requested_at DESC, event_id ASC"#,
        )
        .map_err(super::local_state_unavailable)?;
    let rows = statement
        .query_map(
            params![
                scope.owner_identity_id,
                scope.owner_did,
                scope.owner_device_id
            ],
            record_from_row,
        )
        .map_err(super::local_state_unavailable)?;
    let mut records = Vec::new();
    for row in rows {
        let record = row.map_err(super::local_state_unavailable)?;
        if is_active_at(&record, now)? {
            records.push(record);
        }
    }
    Ok(records)
}

pub(crate) fn get_active(
    connection: &Connection,
    scope: &OldAdminRecoveryNoticeScope,
    event_id: &str,
    now: OffsetDateTime,
) -> crate::ImResult<Option<OldAdminRecoveryNoticeRecord>> {
    validate_scope(scope)?;
    validate_identifier("event_id", event_id)?;
    crate::internal::local_state::schema::ensure_schema(connection)?;
    let record = connection
        .query_row(
            r#"
SELECT owner_identity_id,
       owner_did,
       owner_device_id,
       event_id,
       source_event_id,
       recovery_session_id,
       handle,
       requested_at,
       cancellable_until
FROM old_admin_recovery_notices
WHERE owner_identity_id = ?1
  AND owner_did = ?2
  AND owner_device_id = ?3
  AND event_id = ?4
  AND dismissed_at IS NULL"#,
            params![
                scope.owner_identity_id,
                scope.owner_did,
                scope.owner_device_id,
                event_id
            ],
            record_from_row,
        )
        .optional()
        .map_err(super::local_state_unavailable)?;
    record
        .map(|record| {
            if is_active_at(&record, now)? {
                Ok(Some(record))
            } else {
                Ok(None)
            }
        })
        .unwrap_or(Ok(None))
}

pub(crate) fn dismiss_active(
    connection: &mut Connection,
    scope: &OldAdminRecoveryNoticeScope,
    event_id: &str,
    now: OffsetDateTime,
) -> crate::ImResult<bool> {
    let Some(record) = get_active(connection, scope, event_id, now)? else {
        return Ok(false);
    };
    let changed = connection
        .execute(
            r#"
UPDATE old_admin_recovery_notices
SET dismissed_at = ?1
WHERE owner_identity_id = ?2
  AND owner_did = ?3
  AND owner_device_id = ?4
  AND event_id = ?5
  AND dismissed_at IS NULL"#,
            params![
                format_rfc3339(now)?,
                record.owner_identity_id,
                record.owner_did,
                record.owner_device_id,
                record.event_id,
            ],
        )
        .map_err(super::local_state_unavailable)?;
    Ok(changed == 1)
}

fn load_by_event_id(
    connection: &Connection,
    owner_identity_id: &str,
    owner_device_id: &str,
    event_id: &str,
) -> crate::ImResult<Option<OldAdminRecoveryNoticeRecord>> {
    connection
        .query_row(
            r#"
SELECT owner_identity_id,
       owner_did,
       owner_device_id,
       event_id,
       source_event_id,
       recovery_session_id,
       handle,
       requested_at,
       cancellable_until
FROM old_admin_recovery_notices
WHERE owner_identity_id = ?1
  AND owner_device_id = ?2
  AND event_id = ?3"#,
            params![owner_identity_id, owner_device_id, event_id],
            record_from_row,
        )
        .optional()
        .map_err(super::local_state_unavailable)
}

fn record_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<OldAdminRecoveryNoticeRecord> {
    Ok(OldAdminRecoveryNoticeRecord {
        owner_identity_id: row.get(0)?,
        owner_did: row.get(1)?,
        owner_device_id: row.get(2)?,
        event_id: row.get(3)?,
        source_event_id: row.get(4)?,
        recovery_session_id: row.get(5)?,
        handle: row.get(6)?,
        requested_at: row.get(7)?,
        cancellable_until: row.get(8)?,
    })
}

fn validate_record(record: &OldAdminRecoveryNoticeRecord) -> crate::ImResult<()> {
    validate_scope(&OldAdminRecoveryNoticeScope {
        owner_identity_id: record.owner_identity_id.clone(),
        owner_did: record.owner_did.clone(),
        owner_device_id: record.owner_device_id.clone(),
    })?;
    validate_identifier("event_id", &record.event_id)?;
    validate_identifier("source_event_id", &record.source_event_id)?;
    validate_identifier("recovery_session_id", &record.recovery_session_id)?;
    if record.handle.trim().is_empty() || record.handle.len() > 512 {
        return Err(invalid_record("handle is invalid"));
    }
    let requested_at = parse_rfc3339("requested_at", &record.requested_at)?;
    let cancellable_until = parse_rfc3339("cancellable_until", &record.cancellable_until)?;
    if cancellable_until <= requested_at {
        return Err(invalid_record(
            "cancellable_until must be later than requested_at",
        ));
    }
    Ok(())
}

fn validate_scope(scope: &OldAdminRecoveryNoticeScope) -> crate::ImResult<()> {
    let owner_identity_id = scope.owner_identity_id.trim();
    if owner_identity_id.is_empty() || owner_identity_id.len() > 512 {
        return Err(crate::ImError::invalid_input(
            Some("owner_identity_id".to_owned()),
            "owner_identity_id is invalid",
        ));
    }
    crate::ids::Did::parse(&scope.owner_did)?;
    crate::ids::ProtocolDeviceId::parse(&scope.owner_device_id)?;
    Ok(())
}

fn validate_identifier(field: &'static str, value: &str) -> crate::ImResult<()> {
    let value = value.trim();
    if value.is_empty()
        || value.len() > 256
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
    {
        return Err(crate::ImError::invalid_input(
            Some(field.to_owned()),
            format!("{field} is invalid"),
        ));
    }
    Ok(())
}

fn is_active_at(
    record: &OldAdminRecoveryNoticeRecord,
    now: OffsetDateTime,
) -> crate::ImResult<bool> {
    Ok(parse_rfc3339("cancellable_until", &record.cancellable_until)? > now)
}

fn parse_rfc3339(field: &'static str, value: &str) -> crate::ImResult<OffsetDateTime> {
    OffsetDateTime::parse(value.trim(), &Rfc3339).map_err(|_| {
        crate::ImError::invalid_input(Some(field.to_owned()), format!("{field} is invalid"))
    })
}

fn format_rfc3339(value: OffsetDateTime) -> crate::ImResult<String> {
    value
        .to_offset(time::UtcOffset::UTC)
        .format(&Rfc3339)
        .map_err(|_| invalid_record("timestamp formatting failed"))
}

fn now_rfc3339() -> String {
    format_rfc3339(OffsetDateTime::now_utc()).unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_owned())
}

fn invalid_record(message: impl Into<String>) -> crate::ImError {
    crate::ImError::invalid_input(Some("old_admin_recovery_notice".to_owned()), message)
}

fn projection_conflict(message: impl Into<String>) -> crate::ImError {
    crate::ImError::Service {
        status_code: None,
        code: Some("identity.recovery_notice_conflict".to_owned()),
        message: message.into(),
        data: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(value: &str) -> OffsetDateTime {
        OffsetDateTime::parse(value, &Rfc3339).unwrap()
    }

    fn record(event_id: &str, session_id: &str) -> OldAdminRecoveryNoticeRecord {
        OldAdminRecoveryNoticeRecord {
            owner_identity_id: "identity-alice".to_owned(),
            owner_did: "did:wba:awiki.test:user:alice".to_owned(),
            owner_device_id: "dev-old-admin".to_owned(),
            event_id: event_id.to_owned(),
            source_event_id: event_id
                .strip_prefix("identity-recovery-started:")
                .unwrap_or(event_id)
                .to_owned(),
            recovery_session_id: session_id.to_owned(),
            handle: "alice.awiki.test".to_owned(),
            requested_at: "2030-01-01T00:00:00Z".to_owned(),
            cancellable_until: "2030-01-02T00:00:00Z".to_owned(),
        }
    }

    fn scope() -> OldAdminRecoveryNoticeScope {
        OldAdminRecoveryNoticeScope {
            owner_identity_id: "identity-alice".to_owned(),
            owner_did: "did:wba:awiki.test:user:alice".to_owned(),
            owner_device_id: "dev-old-admin".to_owned(),
        }
    }

    fn insert(connection: &mut Connection, record: &OldAdminRecoveryNoticeRecord) {
        upsert(connection, record).unwrap();
    }

    #[test]
    fn exact_replay_is_idempotent_but_conflicting_projection_fails_closed() {
        let mut db = Connection::open_in_memory().unwrap();
        let first = record("identity-recovery-started:event-1", "session-1");
        insert(&mut db, &first);
        insert(&mut db, &first);

        let count: i64 = db
            .query_row(
                "SELECT COUNT(*) FROM old_admin_recovery_notices",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);

        let mut conflicting = first.clone();
        conflicting.handle = "mallory.awiki.test".to_owned();
        let error = upsert(&mut db, &conflicting).unwrap_err();
        assert!(error.to_string().contains("conflict"));

        let other_event = record("identity-recovery-started:event-2", "session-1");
        let error = upsert(&mut db, &other_event).unwrap_err();
        assert!(error.to_string().contains("another durable event"));
    }

    #[test]
    fn active_list_get_and_dismiss_are_owner_device_scoped() {
        let mut db = Connection::open_in_memory().unwrap();
        let first = record("identity-recovery-started:event-1", "session-1");
        insert(&mut db, &first);
        let now = at("2030-01-01T12:00:00Z");

        assert_eq!(
            list_active(&db, &scope(), now).unwrap(),
            vec![first.clone()]
        );
        assert_eq!(
            get_active(&db, &scope(), &first.event_id, now).unwrap(),
            Some(first.clone())
        );

        let mut other_device = scope();
        other_device.owner_device_id = "dev-other-admin".to_owned();
        assert!(list_active(&db, &other_device, now).unwrap().is_empty());

        assert!(dismiss_active(&mut db, &scope(), &first.event_id, now).unwrap());
        assert!(!dismiss_active(&mut db, &scope(), &first.event_id, now).unwrap());
        assert!(list_active(&db, &scope(), now).unwrap().is_empty());
        assert!(get_active(&db, &scope(), &first.event_id, now)
            .unwrap()
            .is_none());
    }

    #[test]
    fn expired_notice_is_hidden_and_records_are_restart_safe_and_secret_free() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("local.sqlite3");
        let event_id = "identity-recovery-started:event-restart";
        {
            let mut db = crate::internal::local_state::open_writable(&path).unwrap();
            insert(&mut db, &record(event_id, "session-restart"));
        }
        {
            let db = crate::internal::local_state::open_writable(&path).unwrap();
            assert_eq!(
                list_active(&db, &scope(), at("2030-01-01T23:59:59Z"))
                    .unwrap()
                    .len(),
                1
            );
            assert!(list_active(&db, &scope(), at("2030-01-02T00:00:00Z"))
                .unwrap()
                .is_empty());
            let columns = db
                .prepare("PRAGMA table_info(old_admin_recovery_notices)")
                .unwrap()
                .query_map([], |row| row.get::<_, String>(1))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            for forbidden in ["otp", "token", "proof", "private_key", "email", "location"] {
                assert!(!columns.iter().any(|column| column.contains(forbidden)));
            }
        }
    }
}
