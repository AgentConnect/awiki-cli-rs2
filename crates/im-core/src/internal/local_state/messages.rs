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
    let rows = list_message_classification_rows(connection, "", owner_did, message_ids)?;
    classify_mark_read_ids_from_rows(message_ids, rows)
}

#[cfg(feature = "sqlite")]
pub(crate) fn classify_mark_read_ids_for_owner_identity(
    connection: &rusqlite::Connection,
    owner_identity_id: &str,
    owner_did: &str,
    message_ids: &[String],
) -> crate::ImResult<MarkReadClassification> {
    let rows =
        list_message_classification_rows(connection, owner_identity_id, owner_did, message_ids)?;
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
    mark_messages_read_for_owner(connection, "", owner_did, message_ids)
}

#[cfg(feature = "sqlite")]
pub(crate) fn mark_messages_read_for_owner_identity(
    connection: &rusqlite::Connection,
    owner_identity_id: &str,
    owner_did: &str,
    message_ids: &[String],
) -> crate::ImResult<i64> {
    mark_messages_read_for_owner(connection, owner_identity_id, owner_did, message_ids)
}

#[cfg(feature = "sqlite")]
fn mark_messages_read_for_owner(
    connection: &rusqlite::Connection,
    owner_identity_id: &str,
    owner_did: &str,
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
    let owner = normalize_owner_did(owner_did);
    let mut params: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(ids.len() + 2);
    params.push(&owner_identity_id);
    params.push(&owner);
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
    owner_did: &str,
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
    let owner = normalize_owner_did(owner_did);
    let mut params: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(ids.len() + 2);
    params.push(&owner_identity_id);
    params.push(&owner);
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
fn normalize_owner_did(value: &str) -> String {
    value.trim().to_string()
}

#[cfg(feature = "sqlite")]
fn normalize_owner_identity_id(value: &str) -> String {
    value.trim().to_string()
}

#[cfg(feature = "sqlite")]
fn owner_predicate() -> &'static str {
    "(owner_identity_id = ? OR ((owner_identity_id IS NULL OR TRIM(owner_identity_id) = '') AND owner_did = ?))"
}

#[cfg(all(test, feature = "sqlite"))]
mod tests {
    use super::*;
    use rusqlite::Connection;

    #[test]
    fn local_state_messages_classifies_mark_read_ids_like_legacy() {
        let db = Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        db.execute(
            r#"
INSERT INTO messages
    (msg_id, owner_did, thread_id, direction, sender_did, receiver_did, content_type, content, stored_at)
VALUES (?1, ?2, ?3, 0, ?4, ?2, 'text/plain', 'direct', '2026-05-21T00:00:00Z')"#,
            (
                "direct-1",
                "did:example:alice",
                "dm:alice:bob",
                "did:example:bob",
            ),
        )
        .unwrap();
        db.execute(
            r#"
INSERT INTO messages
    (msg_id, owner_did, thread_id, direction, group_id, group_did, content_type, content, stored_at)
VALUES (?1, ?2, ?3, 0, ?4, ?4, 'text/plain', 'group', '2026-05-21T00:00:00Z')"#,
            (
                "group-1",
                "did:example:alice",
                "group:one",
                "did:example:group",
            ),
        )
        .unwrap();
        db.execute(
            r#"
INSERT INTO messages
    (msg_id, owner_did, thread_id, direction, content_type, content, stored_at, metadata)
VALUES (?1, ?2, ?3, 0, 'mail.notification', 'mail', '2026-05-21T00:00:00Z', ?4)"#,
            (
                "mail-1",
                "did:example:alice",
                "mail:inbox",
                r#"{"source_kind":"mail"}"#,
            ),
        )
        .unwrap();

        let classified = classify_mark_read_ids(
            &db,
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
        for owner in ["did:owner-1", "did:owner-2"] {
            db.execute(
                r#"
INSERT INTO messages
    (msg_id, owner_did, thread_id, direction, content_type, content, stored_at, is_read)
VALUES (?1, ?2, 'thread', 0, 'text/plain', 'hello', '2026-05-21T00:00:00Z', 0)"#,
                ("shared", owner),
            )
            .unwrap();
        }

        let updated = mark_messages_read(&db, "did:owner-1", &["shared".to_string()]).unwrap();

        assert_eq!(updated, 1);
        assert_eq!(is_read(&db, "did:owner-1"), 1);
        assert_eq!(is_read(&db, "did:owner-2"), 0);
    }

    #[test]
    fn local_state_owner_mark_read_prefers_identity_and_falls_back_to_legacy_did() {
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
    (msg_id, owner_did, thread_id, direction, content_type, content, stored_at, is_read)
VALUES ('legacy', 'did:alice-new', 'thread', 0, 'text/plain', 'hello', '2026-05-21T00:00:00Z', 0)"#,
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
                "legacy".to_string(),
                "other".to_string(),
            ],
        )
        .unwrap();

        assert_eq!(updated, 2);
        assert_eq!(read_by_msg_id(&db, "stable"), 1);
        assert_eq!(read_by_msg_id(&db, "legacy"), 1);
        assert_eq!(read_by_msg_id(&db, "other"), 0);
    }

    fn is_read(db: &Connection, owner: &str) -> i64 {
        db.query_row(
            "SELECT is_read FROM messages WHERE owner_did = ?1 AND msg_id = 'shared'",
            [owner],
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
