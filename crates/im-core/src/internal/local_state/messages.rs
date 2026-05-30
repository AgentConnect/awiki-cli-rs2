#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct MessageRecord {
    pub(crate) msg_id: String,
    pub(crate) owner_identity_id: String,
    pub(crate) owner_did: String,
    pub(crate) thread_id: String,
    pub(crate) direction: i64,
    pub(crate) sender_did: String,
    pub(crate) receiver_did: String,
    pub(crate) group_id: String,
    pub(crate) group_did: String,
    pub(crate) content_type: String,
    pub(crate) content: String,
    pub(crate) title: String,
    pub(crate) server_seq: Option<i64>,
    pub(crate) sent_at: String,
    pub(crate) stored_at: String,
    pub(crate) is_e2ee: bool,
    pub(crate) is_read: bool,
    pub(crate) sender_name: String,
    pub(crate) metadata: String,
    pub(crate) credential_name: String,
}

#[cfg(feature = "sqlite")]
pub(crate) fn upsert_message(
    connection: &rusqlite::Connection,
    record: &MessageRecord,
) -> crate::ImResult<()> {
    crate::internal::local_state::schema::ensure_schema(connection)?;
    let msg_id = required("msg_id", &record.msg_id)?;
    let owner_identity_id = required("owner_identity_id", &record.owner_identity_id)?;
    let owner_did = required("owner_did", &record.owner_did)?;
    let thread_id = required("thread_id", &record.thread_id)?;
    let stored_at = default_string(&record.stored_at, &now_utc_like());
    connection
        .execute(
            r#"
INSERT INTO messages
    (msg_id, owner_identity_id, owner_did, thread_id, direction, sender_did, receiver_did,
     group_id, group_did, content_type, content, title, server_seq, sent_at, stored_at,
     is_e2ee, is_read, sender_name, metadata, credential_name)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20)
ON CONFLICT(owner_identity_id, msg_id) DO UPDATE SET
    owner_did = excluded.owner_did,
    thread_id = excluded.thread_id,
    direction = excluded.direction,
    sender_did = excluded.sender_did,
    receiver_did = excluded.receiver_did,
    group_id = excluded.group_id,
    group_did = excluded.group_did,
    content_type = excluded.content_type,
    content = excluded.content,
    title = excluded.title,
    server_seq = COALESCE(excluded.server_seq, messages.server_seq),
    sent_at = excluded.sent_at,
    stored_at = excluded.stored_at,
    is_e2ee = CASE WHEN excluded.is_e2ee = 1 OR messages.is_e2ee = 1 THEN 1 ELSE 0 END,
    is_read = CASE WHEN excluded.is_read = 1 OR messages.is_read = 1 THEN 1 ELSE 0 END,
    sender_name = excluded.sender_name,
    metadata = excluded.metadata,
    credential_name = excluded.credential_name"#,
            rusqlite::params![
                msg_id,
                owner_identity_id,
                owner_did,
                thread_id,
                record.direction,
                nullable_text(&record.sender_did),
                nullable_text(&record.receiver_did),
                nullable_text(&record.group_id),
                nullable_text(&record.group_did),
                default_string(&record.content_type, "text/plain"),
                nullable_text(&record.content),
                nullable_text(&record.title),
                record.server_seq,
                nullable_text(&record.sent_at),
                stored_at,
                record.is_e2ee,
                record.is_read,
                nullable_text(&record.sender_name),
                nullable_text(&record.metadata),
                record.credential_name.trim(),
            ],
        )
        .map_err(super::local_state_unavailable)?;
    Ok(())
}

#[cfg(feature = "sqlite")]
pub(crate) fn upsert_messages(
    connection: &rusqlite::Connection,
    records: &[MessageRecord],
) -> crate::ImResult<()> {
    for record in records {
        upsert_message(connection, record)?;
    }
    Ok(())
}

#[cfg(feature = "sqlite")]
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct MarkReadClassification {
    pub(crate) direct_ids: Vec<String>,
    pub(crate) group_ids: Vec<String>,
    pub(crate) local_only_ids: Vec<String>,
}

#[cfg(feature = "sqlite")]
impl MarkReadClassification {
    pub(crate) fn local_ids(&self) -> Vec<String> {
        let mut ids = self.direct_ids.clone();
        ids.extend(self.group_ids.iter().cloned());
        ids.extend(self.local_only_ids.iter().cloned());
        ids
    }
}

#[cfg(feature = "sqlite")]
pub(crate) fn classify_mark_read_ids(
    connection: &rusqlite::Connection,
    owner_did: &str,
    message_ids: &[String],
) -> crate::ImResult<MarkReadClassification> {
    let _ = (connection, owner_did, message_ids);
    Err(crate::ImError::invalid_input(
        Some("owner_identity_id".to_owned()),
        "owner_identity_id is required",
    ))
}

#[cfg(feature = "sqlite")]
pub(crate) fn classify_mark_read_ids_for_owner_identity(
    connection: &rusqlite::Connection,
    owner_identity_id: &str,
    _owner_did: &str,
    message_ids: &[String],
) -> crate::ImResult<MarkReadClassification> {
    let rows = list_message_classification_rows(connection, owner_identity_id, message_ids)?;
    classify_mark_read_ids_from_rows(message_ids, rows)
}

#[cfg(feature = "sqlite")]
fn classify_mark_read_ids_from_rows(
    message_ids: &[String],
    rows: Vec<MessageClassificationRow>,
) -> crate::ImResult<MarkReadClassification> {
    let mut result = MarkReadClassification::default();
    for id in message_ids {
        let Some(row) = rows.iter().find(|row| row.msg_id == id.trim()) else {
            result.direct_ids.push(id.clone());
            continue;
        };
        if row.is_local_mail_notification() {
            result.local_only_ids.push(id.clone());
        } else if row.is_group_message() {
            result.group_ids.push(id.clone());
        } else {
            result.direct_ids.push(id.clone());
        }
    }
    Ok(result)
}

#[cfg(feature = "sqlite")]
pub(crate) fn mark_messages_read(
    connection: &rusqlite::Connection,
    owner_did: &str,
    message_ids: &[String],
) -> crate::ImResult<i64> {
    let _ = (connection, owner_did, message_ids);
    Err(crate::ImError::invalid_input(
        Some("owner_identity_id".to_owned()),
        "owner_identity_id is required",
    ))
}

#[cfg(feature = "sqlite")]
pub(crate) fn mark_messages_read_for_owner_identity(
    connection: &rusqlite::Connection,
    owner_identity_id: &str,
    _owner_did: &str,
    message_ids: &[String],
) -> crate::ImResult<i64> {
    mark_messages_read_for_owner(connection, owner_identity_id, message_ids)
}

#[cfg(feature = "sqlite")]
fn mark_messages_read_for_owner(
    connection: &rusqlite::Connection,
    owner_identity_id: &str,
    message_ids: &[String],
) -> crate::ImResult<i64> {
    let ids = message_ids
        .iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    if ids.is_empty() {
        return Ok(0);
    }
    let placeholders = vec!["?"; ids.len()].join(",");
    let statement = format!(
        "UPDATE messages SET is_read = 1 WHERE {} AND msg_id IN ({placeholders})",
        owner_predicate()
    );
    let owner_identity_id = normalize_owner_identity_id(owner_identity_id);
    required("owner_identity_id", &owner_identity_id)?;
    let mut params: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(ids.len() + 1);
    params.push(&owner_identity_id);
    for id in &ids {
        params.push(id);
    }
    let rows = connection
        .execute(&statement, params.as_slice())
        .map_err(super::local_state_unavailable)?;
    Ok(i64::try_from(rows).unwrap_or(i64::MAX))
}

#[cfg(feature = "sqlite")]
fn list_message_classification_rows(
    connection: &rusqlite::Connection,
    owner_identity_id: &str,
    message_ids: &[String],
) -> crate::ImResult<Vec<MessageClassificationRow>> {
    let ids = message_ids
        .iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders = vec!["?"; ids.len()].join(",");
    let statement = format!(
        r#"
SELECT msg_id, group_id, group_did, content_type, metadata
FROM messages
WHERE {} AND msg_id IN ({placeholders})"#,
        owner_predicate()
    );
    let owner_identity_id = normalize_owner_identity_id(owner_identity_id);
    required("owner_identity_id", &owner_identity_id)?;
    let mut params: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(ids.len() + 1);
    params.push(&owner_identity_id);
    for id in &ids {
        params.push(id);
    }
    let mut statement = connection
        .prepare(&statement)
        .map_err(super::local_state_unavailable)?;
    let rows = statement
        .query_map(params.as_slice(), |row| {
            Ok(MessageClassificationRow {
                msg_id: row.get::<_, Option<String>>("msg_id")?.unwrap_or_default(),
                group_id: row
                    .get::<_, Option<String>>("group_id")?
                    .unwrap_or_default(),
                group_did: row
                    .get::<_, Option<String>>("group_did")?
                    .unwrap_or_default(),
                content_type: row
                    .get::<_, Option<String>>("content_type")?
                    .unwrap_or_default(),
                metadata: row
                    .get::<_, Option<String>>("metadata")?
                    .unwrap_or_default(),
            })
        })
        .map_err(super::local_state_unavailable)?;
    let mut result = Vec::new();
    for row in rows {
        result.push(row.map_err(super::local_state_unavailable)?);
    }
    Ok(result)
}

#[cfg(feature = "sqlite")]
#[derive(Debug, Clone, PartialEq, Eq)]
struct MessageClassificationRow {
    msg_id: String,
    group_id: String,
    group_did: String,
    content_type: String,
    metadata: String,
}

#[cfg(feature = "sqlite")]
impl MessageClassificationRow {
    fn is_group_message(&self) -> bool {
        !self.group_did.trim().is_empty() || !self.group_id.trim().is_empty()
    }

    fn is_local_mail_notification(&self) -> bool {
        if self.content_type.trim() == "mail.notification" {
            return true;
        }
        parse_metadata(&self.metadata)
            .get("source_kind")
            .and_then(serde_json::Value::as_str)
            .map(|value| value.trim() == "mail")
            .unwrap_or(false)
    }
}

#[cfg(feature = "sqlite")]
fn parse_metadata(value: &str) -> serde_json::Map<String, serde_json::Value> {
    if value.trim().is_empty() {
        return serde_json::Map::new();
    }
    serde_json::from_str(value).unwrap_or_default()
}

#[cfg(feature = "sqlite")]
fn normalize_owner_identity_id(value: &str) -> String {
    value.trim().to_string()
}

#[cfg(feature = "sqlite")]
fn owner_predicate() -> &'static str {
    "owner_identity_id = ?"
}

#[cfg(feature = "sqlite")]
fn required(field: &str, value: &str) -> crate::ImResult<String> {
    let value = value.trim();
    if value.is_empty() {
        return Err(crate::ImError::invalid_input(
            Some(field.to_owned()),
            format!("{field} must not be empty"),
        ));
    }
    Ok(value.to_owned())
}

#[cfg(feature = "sqlite")]
fn nullable_text(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

#[cfg(feature = "sqlite")]
fn default_string(value: &str, fallback: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        fallback.to_owned()
    } else {
        value.to_owned()
    }
}

#[cfg(feature = "sqlite")]
fn now_utc_like() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_owned())
}

#[cfg(all(test, feature = "sqlite"))]
mod tests {
    use super::*;
    use rusqlite::Connection;

    #[test]
    fn local_state_messages_classifies_mark_read_ids_by_owner_identity() {
        let db = Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        db.execute(
            r#"
INSERT INTO messages
    (msg_id, owner_identity_id, owner_did, thread_id, direction, sender_did, receiver_did, content_type, content, stored_at)
VALUES (?1, ?2, ?3, ?4, 0, ?5, ?3, 'text/plain', 'direct', '2026-05-21T00:00:00Z')"#,
            (
                "direct-1",
                "alice-id",
                "did:example:alice",
                "dm:alice:bob",
                "did:example:bob",
            ),
        )
        .unwrap();
        db.execute(
            r#"
INSERT INTO messages
    (msg_id, owner_identity_id, owner_did, thread_id, direction, group_id, group_did, content_type, content, stored_at)
VALUES (?1, ?2, ?3, ?4, 0, ?5, ?5, 'text/plain', 'group', '2026-05-21T00:00:00Z')"#,
            (
                "group-1",
                "alice-id",
                "did:example:alice",
                "group:one",
                "did:example:group",
            ),
        )
        .unwrap();
        db.execute(
            r#"
INSERT INTO messages
    (msg_id, owner_identity_id, owner_did, thread_id, direction, content_type, content, stored_at, metadata)
VALUES (?1, ?2, ?3, ?4, 0, 'mail.notification', 'mail', '2026-05-21T00:00:00Z', ?5)"#,
            (
                "mail-1",
                "alice-id",
                "did:example:alice",
                "mail:inbox",
                r#"{"source_kind":"mail"}"#,
            ),
        )
        .unwrap();

        let classified = classify_mark_read_ids_for_owner_identity(
            &db,
            "alice-id",
            "did:example:alice",
            &[
                "direct-1".to_string(),
                "group-1".to_string(),
                "mail-1".to_string(),
                "missing-1".to_string(),
            ],
        )
        .unwrap();

        assert_eq!(classified.direct_ids, vec!["direct-1", "missing-1"]);
        assert_eq!(classified.group_ids, vec!["group-1"]);
        assert_eq!(classified.local_only_ids, vec!["mail-1"]);
    }

    #[test]
    fn local_state_messages_mark_read_respects_owner() {
        let db = Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        for (identity, owner) in [("owner-1-id", "did:owner-1"), ("owner-2-id", "did:owner-2")] {
            db.execute(
                r#"
INSERT INTO messages
    (msg_id, owner_identity_id, owner_did, thread_id, direction, content_type, content, stored_at, is_read)
VALUES (?1, ?2, ?3, 'thread', 0, 'text/plain', 'hello', '2026-05-21T00:00:00Z', 0)"#,
                ("shared", identity, owner),
            )
            .unwrap();
        }

        let updated = mark_messages_read_for_owner_identity(
            &db,
            "owner-1-id",
            "did:owner-1",
            &["shared".to_string()],
        )
        .unwrap();

        assert_eq!(updated, 1);
        assert_eq!(is_read(&db, "owner-1-id"), 1);
        assert_eq!(is_read(&db, "owner-2-id"), 0);
    }

    #[test]
    fn local_state_owner_mark_read_uses_identity_without_legacy_did_fallback() {
        let db = Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        db.execute(
            r#"
INSERT INTO messages
    (msg_id, owner_identity_id, owner_did, thread_id, direction, content_type, content, stored_at, is_read)
VALUES ('stable', 'alice-id', 'did:alice-old', 'thread', 0, 'text/plain', 'hello', '2026-05-21T00:00:00Z', 0)"#,
            [],
        )
        .unwrap();
        db.execute(
            r#"
INSERT INTO messages
    (msg_id, owner_identity_id, owner_did, thread_id, direction, content_type, content, stored_at, is_read)
VALUES ('same-did-other-id', 'mallory-id', 'did:alice-new', 'thread', 0, 'text/plain', 'hello', '2026-05-21T00:00:00Z', 0)"#,
            [],
        )
        .unwrap();
        db.execute(
            r#"
INSERT INTO messages
    (msg_id, owner_identity_id, owner_did, thread_id, direction, content_type, content, stored_at, is_read)
VALUES ('other', 'bob-id', 'did:alice-new', 'thread', 0, 'text/plain', 'hello', '2026-05-21T00:00:00Z', 0)"#,
            [],
        )
        .unwrap();

        let updated = mark_messages_read_for_owner_identity(
            &db,
            "alice-id",
            "did:alice-new",
            &[
                "stable".to_string(),
                "same-did-other-id".to_string(),
                "other".to_string(),
            ],
        )
        .unwrap();

        assert_eq!(updated, 1);
        assert_eq!(read_by_msg_id(&db, "stable"), 1);
        assert_eq!(read_by_msg_id(&db, "same-did-other-id"), 0);
        assert_eq!(read_by_msg_id(&db, "other"), 0);
    }

    #[test]
    fn local_state_messages_upsert_stores_owner_identity_and_replaces_existing() {
        let db = Connection::open_in_memory().unwrap();
        upsert_message(
            &db,
            &MessageRecord {
                msg_id: "msg-1".to_owned(),
                owner_identity_id: "alice-id".to_owned(),
                owner_did: "did:example:alice".to_owned(),
                thread_id: "dm:alice:bob".to_owned(),
                direction: 1,
                sender_did: "did:example:alice".to_owned(),
                receiver_did: "did:example:bob".to_owned(),
                content_type: "text/plain".to_owned(),
                content: "first".to_owned(),
                stored_at: "2026-05-24T00:00:00Z".to_owned(),
                is_e2ee: true,
                is_read: true,
                credential_name: "alice".to_owned(),
                ..MessageRecord::default()
            },
        )
        .unwrap();
        upsert_message(
            &db,
            &MessageRecord {
                msg_id: "msg-1".to_owned(),
                owner_identity_id: "alice-id".to_owned(),
                owner_did: "did:example:alice".to_owned(),
                thread_id: "dm:alice:bob".to_owned(),
                direction: 1,
                sender_did: "did:example:alice".to_owned(),
                receiver_did: "did:example:bob".to_owned(),
                content_type: "text/plain".to_owned(),
                content: "second".to_owned(),
                stored_at: "2026-05-24T00:00:01Z".to_owned(),
                is_e2ee: true,
                is_read: true,
                credential_name: "alice".to_owned(),
                ..MessageRecord::default()
            },
        )
        .unwrap();

        let row = db
            .query_row(
                "SELECT owner_identity_id, content, stored_at, is_e2ee FROM messages WHERE msg_id = 'msg-1'",
                [],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, i64>(3)?,
                    ))
                },
            )
            .unwrap();

        assert_eq!(row.0, "alice-id");
        assert_eq!(row.1, "second");
        assert_eq!(row.2, "2026-05-24T00:00:01Z");
        assert_eq!(row.3, 1);
    }

    #[test]
    fn local_state_messages_upsert_does_not_revert_read_or_e2ee_flags() {
        let db = Connection::open_in_memory().unwrap();
        upsert_message(
            &db,
            &MessageRecord {
                msg_id: "msg-flags".to_owned(),
                owner_identity_id: "owner-id".to_owned(),
                owner_did: "did:owner".to_owned(),
                thread_id: "dm:did:owner:did:peer".to_owned(),
                direction: 0,
                content: "read secure".to_owned(),
                stored_at: "2026-01-01T00:00:00Z".to_owned(),
                is_e2ee: true,
                is_read: true,
                ..MessageRecord::default()
            },
        )
        .unwrap();
        upsert_message(
            &db,
            &MessageRecord {
                msg_id: "msg-flags".to_owned(),
                owner_identity_id: "owner-id".to_owned(),
                owner_did: "did:owner".to_owned(),
                thread_id: "dm:did:owner:did:peer".to_owned(),
                direction: 0,
                content: "later projection".to_owned(),
                stored_at: "2026-01-02T00:00:00Z".to_owned(),
                is_e2ee: false,
                is_read: false,
                ..MessageRecord::default()
            },
        )
        .unwrap();

        let (is_e2ee, is_read): (i64, i64) = db
            .query_row(
                "SELECT is_e2ee, is_read FROM messages WHERE msg_id = 'msg-flags'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(is_e2ee, 1);
        assert_eq!(is_read, 1);
    }

    fn is_read(db: &Connection, owner_identity_id: &str) -> i64 {
        db.query_row(
            "SELECT is_read FROM messages WHERE owner_identity_id = ?1 AND msg_id = 'shared'",
            [owner_identity_id],
            |row| row.get(0),
        )
        .unwrap()
    }

    fn read_by_msg_id(db: &Connection, msg_id: &str) -> i64 {
        db.query_row(
            "SELECT is_read FROM messages WHERE msg_id = ?1",
            [msg_id],
            |row| row.get(0),
        )
        .unwrap()
    }
}
