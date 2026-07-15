use rusqlite::Connection;

pub(crate) const TABLE_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS conversation_registry (
    owner_identity_id TEXT NOT NULL,
    owner_did         TEXT NOT NULL DEFAULT '',
    conversation_id   TEXT NOT NULL,
    thread_kind       TEXT NOT NULL,
    thread_id         TEXT NOT NULL,
    activity_at       TEXT NOT NULL,
    created_at        TEXT NOT NULL,
    updated_at        TEXT NOT NULL,
    is_active         INTEGER NOT NULL DEFAULT 1,
    PRIMARY KEY (owner_identity_id, conversation_id)
);
CREATE INDEX IF NOT EXISTS idx_conversation_registry_owner_activity
ON conversation_registry(owner_identity_id, is_active, activity_at DESC, conversation_id DESC);
"#;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ConversationRegistryRecord {
    pub(crate) owner_identity_id: String,
    pub(crate) owner_did: String,
    pub(crate) conversation_id: String,
    pub(crate) thread_kind: String,
    pub(crate) thread_id: String,
    pub(crate) activity_at: String,
}

pub(crate) fn create_schema(connection: &Connection) -> crate::ImResult<()> {
    connection
        .execute_batch(TABLE_SQL)
        .map_err(super::local_state_unavailable)?;
    Ok(())
}

pub(crate) fn backfill_from_summaries(connection: &Connection) -> crate::ImResult<usize> {
    let changed = connection
        .execute(
            r#"
INSERT OR IGNORE INTO conversation_registry
    (owner_identity_id, owner_did, conversation_id, thread_kind, thread_id,
     activity_at, created_at, updated_at, is_active)
SELECT owner_identity_id,
       owner_did,
       conversation_id,
       CASE WHEN conversation_id LIKE 'group:%' THEN 'group' ELSE 'thread' END,
       thread_id,
       COALESCE(NULLIF(last_message_at, ''), updated_at),
       updated_at,
       updated_at,
       1
FROM conversation_summaries"#,
            [],
        )
        .map_err(super::local_state_unavailable)?;
    Ok(changed)
}

pub(crate) fn ensure(
    connection: &Connection,
    record: &ConversationRegistryRecord,
) -> crate::ImResult<()> {
    for (field, value) in [
        ("owner_identity_id", record.owner_identity_id.as_str()),
        ("owner_did", record.owner_did.as_str()),
        ("conversation_id", record.conversation_id.as_str()),
        ("thread_kind", record.thread_kind.as_str()),
        ("thread_id", record.thread_id.as_str()),
        ("activity_at", record.activity_at.as_str()),
    ] {
        if value.trim().is_empty() {
            return Err(crate::ImError::invalid_input(
                Some(field.to_owned()),
                format!("{field} is required"),
            ));
        }
    }
    connection
        .execute(
            r#"
INSERT INTO conversation_registry
    (owner_identity_id, owner_did, conversation_id, thread_kind, thread_id,
     activity_at, created_at, updated_at, is_active)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6, ?6, 1)
ON CONFLICT(owner_identity_id, conversation_id) DO UPDATE SET
    owner_did = excluded.owner_did,
    thread_kind = excluded.thread_kind,
    thread_id = excluded.thread_id,
    activity_at = CASE
        WHEN excluded.activity_at > conversation_registry.activity_at THEN excluded.activity_at
        ELSE conversation_registry.activity_at
    END,
    updated_at = excluded.updated_at,
    is_active = 1"#,
            rusqlite::params![
                record.owner_identity_id,
                record.owner_did,
                record.conversation_id,
                record.thread_kind,
                record.thread_id,
                record.activity_at,
            ],
        )
        .map_err(super::local_state_unavailable)?;
    Ok(())
}

pub(crate) fn ensure_validated(
    connection: &Connection,
    owner_identity_id: &str,
    owner_did: &str,
    conversation_id: &str,
) -> crate::ImResult<()> {
    let conversation_id = conversation_id.trim();
    let (thread_kind, thread_id) = if let Some(group_ref) = conversation_id.strip_prefix("group:") {
        let group_ref = group_ref.trim();
        crate::ids::GroupRef::parse(group_ref)?;
        let active = connection
            .query_row(
                r#"
SELECT COALESCE(NULLIF(TRIM(group_id), ''), TRIM(group_did))
FROM groups
WHERE owner_identity_id = ?1
  AND (group_id = ?2 OR group_did = ?2)
  AND COALESCE(NULLIF(TRIM(membership_status), ''), 'active')
      NOT IN ('left', 'removed', 'inactive', 'non_member')
LIMIT 1"#,
                (owner_identity_id, group_ref),
                |row| row.get::<_, String>(0),
            )
            .map_err(|err| match err {
                rusqlite::Error::QueryReturnedNoRows => crate::ImError::invalid_input(
                    Some("conversation_id".to_owned()),
                    "group conversation requires an active local membership projection",
                ),
                other => super::local_state_unavailable(other),
            })?;
        ("group".to_owned(), active)
    } else if conversation_id.starts_with("dm:peer-scope:v1:") {
        let route = super::direct_peer_routes::get(connection, owner_identity_id, conversation_id)?
            .ok_or_else(|| {
                crate::ImError::invalid_input(
                    Some("conversation_id".to_owned()),
                    "Direct conversation requires an owner-scoped canonical peer route",
                )
            })?;
        ("direct".to_owned(), route.conversation_id)
    } else {
        return Err(crate::ImError::invalid_input(
            Some("conversation_id".to_owned()),
            "conversation must use a canonical group: or dm:peer-scope:v1: id",
        ));
    };
    ensure(
        connection,
        &ConversationRegistryRecord {
            owner_identity_id: owner_identity_id.to_owned(),
            owner_did: owner_did.to_owned(),
            conversation_id: conversation_id.to_owned(),
            thread_kind,
            thread_id,
            activity_at: now_utc_like(),
        },
    )
}

pub(crate) fn ensure_from_summary(
    connection: &Connection,
    owner_identity_id: &str,
    conversation_id: &str,
) -> crate::ImResult<()> {
    let row = connection.query_row(
        r#"
SELECT owner_did, thread_id, last_message_at, updated_at
FROM conversation_summaries
WHERE owner_identity_id = ?1 AND conversation_id = ?2"#,
        (owner_identity_id, conversation_id),
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        },
    );
    let (owner_did, summary_thread_id, last_message_at, updated_at) = match row {
        Ok(row) => row,
        Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(()),
        Err(err) => return Err(super::local_state_unavailable(err)),
    };
    let (thread_kind, thread_id) = conversation_id
        .strip_prefix("group:")
        .map(|id| ("group", id.to_owned()))
        .unwrap_or(("thread", summary_thread_id));
    ensure(
        connection,
        &ConversationRegistryRecord {
            owner_identity_id: owner_identity_id.to_owned(),
            owner_did,
            conversation_id: conversation_id.to_owned(),
            thread_kind: thread_kind.to_owned(),
            thread_id,
            activity_at: if last_message_at.trim().is_empty() {
                updated_at
            } else {
                last_message_at
            },
        },
    )
}

pub(crate) fn deactivate(
    connection: &Connection,
    owner_identity_id: &str,
    conversation_id: &str,
) -> crate::ImResult<()> {
    connection
        .execute(
            "UPDATE conversation_registry SET is_active = 0, updated_at = ?1 WHERE owner_identity_id = ?2 AND conversation_id = ?3",
            rusqlite::params![now_utc_like(), owner_identity_id.trim(), conversation_id.trim()],
        )
        .map_err(super::local_state_unavailable)?;
    Ok(())
}

pub(crate) fn now_utc_like() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_keeps_empty_conversation_independent_of_summary() {
        let db = Connection::open_in_memory().unwrap();
        create_schema(&db).unwrap();
        let record = ConversationRegistryRecord {
            owner_identity_id: "owner-1".into(),
            owner_did: "did:example:owner".into(),
            conversation_id: "group:g1".into(),
            thread_kind: "group".into(),
            thread_id: "g1".into(),
            activity_at: "2026-07-13T00:00:00Z".into(),
        };
        ensure(&db, &record).unwrap();
        ensure(&db, &record).unwrap();
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM conversation_registry", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            1
        );
    }

    #[test]
    fn validated_creation_requires_route_or_active_membership() {
        let db = Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        assert!(ensure_validated(
            &db,
            "owner-1",
            "did:example:owner",
            "dm:peer-scope:v1:missing"
        )
        .is_err());
        assert!(ensure_validated(&db, "owner-1", "did:example:owner", "group:missing").is_err());

        db.execute(
            r#"INSERT INTO groups
               (owner_identity_id, owner_did, group_id, membership_status, stored_at)
               VALUES ('owner-1', 'did:example:owner', 'g1', 'active', '2026-07-13T00:00:00Z')"#,
            [],
        )
        .unwrap();
        ensure_validated(&db, "owner-1", "did:example:owner", "group:g1").unwrap();
        ensure_validated(&db, "owner-1", "did:example:owner", "group:g1").unwrap();
        assert_eq!(
            db.query_row(
                "SELECT COUNT(*) FROM conversation_registry WHERE conversation_id = 'group:g1'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
            1
        );
    }

    #[test]
    fn summary_registration_keeps_unresolved_direct_message_visible() {
        let db = Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        let conversation_id = "dm:did:example:peer";
        db.execute(
            r#"
INSERT INTO conversation_summaries
    (owner_identity_id, owner_did, conversation_id, thread_id,
     message_count, unread_count, unread_mention_count,
     last_message_at, updated_at)
VALUES ('owner-1', 'did:example:owner', ?1, ?1, 1, 1, 0,
        '2026-07-13T00:00:00Z', '2026-07-13T00:00:00Z')"#,
            [conversation_id],
        )
        .unwrap();
        ensure_from_summary(&db, "owner-1", conversation_id).unwrap();

        let ids = db
            .prepare("SELECT conversation_id FROM conversation_registry ORDER BY conversation_id")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(ids, vec![conversation_id]);
    }
}
