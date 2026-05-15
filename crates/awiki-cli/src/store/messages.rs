use super::helpers::{
    bool_to_int, default_string, normalize_credential_name, normalize_metadata,
    normalize_optional_int64, normalize_optional_string, normalize_owner_did, now_utc,
};
use super::{StoreError, StoreResult};
use rusqlite::{params, Connection, Statement};
use serde_json::Value;

#[derive(Debug, Clone, Default)]
pub struct MessageRecord {
    pub msg_id: String,
    pub owner_did: String,
    pub thread_id: String,
    pub direction: i64,
    pub sender_did: String,
    pub receiver_did: String,
    pub group_id: String,
    pub group_did: String,
    pub content_type: String,
    pub content: String,
    pub title: String,
    pub server_seq: Option<i64>,
    pub sent_at: String,
    pub stored_at: String,
    pub is_e2ee: bool,
    pub is_read: bool,
    pub sender_name: String,
    pub metadata: String,
    pub credential_name: String,
}

pub fn store_message(connection: &Connection, record: MessageRecord) -> StoreResult<()> {
    if record.msg_id.trim().is_empty() {
        return Err(StoreError::Invalid("msg_id is required".to_string()));
    }
    if record.thread_id.trim().is_empty() {
        return Err(StoreError::Invalid("thread_id is required".to_string()));
    }
    let now = now_utc();
    let mut statement = connection.prepare(store_message_sql())?;
    execute_store_message(&mut statement, &record, &now)?;
    Ok(())
}

pub fn store_messages_batch(
    connection: &mut Connection,
    records: &[MessageRecord],
) -> StoreResult<()> {
    if records.is_empty() {
        return Ok(());
    }
    let transaction = connection.transaction()?;
    let now = now_utc();
    {
        let mut statement = transaction.prepare(store_message_sql())?;
        for record in records {
            if record.msg_id.trim().is_empty() {
                return Err(StoreError::Invalid("msg_id is required".to_string()));
            }
            if record.thread_id.trim().is_empty() {
                return Err(StoreError::Invalid("thread_id is required".to_string()));
            }
            execute_store_message(&mut statement, record, &now)?;
        }
    }
    transaction.commit()?;
    Ok(())
}

pub fn list_inbox_messages(
    connection: &Connection,
    owner_did: &str,
    limit: i64,
    peer_did: &str,
    unread_only: bool,
    include_local_notifications: bool,
) -> StoreResult<Vec<Value>> {
    let limit = if limit <= 0 { 20 } else { limit };
    let mut statement = String::from(
        r#"
SELECT *
FROM messages
WHERE owner_did = ?1
  AND direction = 0
  AND COALESCE(group_did, group_id) IS NULL"#,
    );
    if !include_local_notifications {
        statement.push_str(" AND NOT (");
        statement.push_str(local_mail_notification_predicate());
        statement.push(')');
    }
    if unread_only {
        statement.push_str(" AND is_read = 0");
    }
    if !peer_did.trim().is_empty() {
        statement.push_str(" AND (sender_did = ?2 OR receiver_did = ?2)");
        statement.push_str(" ORDER BY COALESCE(sent_at, stored_at) DESC LIMIT ?3");
        let owner = normalize_owner_did(owner_did);
        let peer = peer_did.trim().to_string();
        return query_rows_with_params(connection, &statement, &[&owner, &peer, &limit]);
    }
    statement.push_str(" ORDER BY COALESCE(sent_at, stored_at) DESC LIMIT ?2");
    let owner = normalize_owner_did(owner_did);
    query_rows_with_params(connection, &statement, &[&owner, &limit])
}

pub fn list_thread_messages(
    connection: &Connection,
    owner_did: &str,
    thread_id: &str,
    limit: i64,
) -> StoreResult<Vec<Value>> {
    if thread_id.trim().is_empty() {
        return Err(StoreError::Invalid("thread_id is required".to_string()));
    }
    let limit = if limit <= 0 { 50 } else { limit };
    let owner = normalize_owner_did(owner_did);
    let thread = thread_id.trim().to_string();
    query_rows_with_params(
        connection,
        r#"
SELECT *
FROM messages
WHERE owner_did = ?1 AND thread_id = ?2
ORDER BY COALESCE(sent_at, stored_at) DESC
LIMIT ?3"#,
        &[&owner, &thread, &limit],
    )
}

pub fn list_messages_by_ids(
    connection: &Connection,
    owner_did: &str,
    message_ids: &[String],
) -> StoreResult<Vec<Value>> {
    let ids = message_ids
        .iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders = vec!["?"; ids.len()].join(",");
    let statement =
        format!("SELECT * FROM messages WHERE owner_did = ? AND msg_id IN ({placeholders})");
    let owner = normalize_owner_did(owner_did);
    let mut params: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(ids.len() + 1);
    params.push(&owner);
    for id in &ids {
        params.push(id);
    }
    query_rows_with_params(connection, &statement, &params)
}

pub fn mark_messages_read(
    connection: &Connection,
    owner_did: &str,
    message_ids: &[String],
) -> StoreResult<i64> {
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
        "UPDATE messages SET is_read = 1 WHERE owner_did = ? AND msg_id IN ({placeholders})"
    );
    let owner = normalize_owner_did(owner_did);
    let mut params: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(ids.len() + 1);
    params.push(&owner);
    for id in &ids {
        params.push(id);
    }
    let rows = connection.execute(&statement, params.as_slice())?;
    Ok(i64::try_from(rows).unwrap_or(i64::MAX))
}

fn query_rows_with_params(
    connection: &Connection,
    statement: &str,
    params: &[&dyn rusqlite::ToSql],
) -> StoreResult<Vec<Value>> {
    let mut statement = connection.prepare(statement)?;
    let names = statement
        .column_names()
        .into_iter()
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    let mut rows = statement.query(params)?;
    let mut results = Vec::new();
    while let Some(row) = rows.next()? {
        let mut object = serde_json::Map::new();
        for (index, name) in names.iter().enumerate() {
            object.insert(name.clone(), value_ref_to_json(row.get_ref(index)?));
        }
        results.push(Value::Object(object));
    }
    Ok(results)
}

fn value_ref_to_json(value: rusqlite::types::ValueRef<'_>) -> Value {
    match value {
        rusqlite::types::ValueRef::Null => Value::Null,
        rusqlite::types::ValueRef::Integer(value) => serde_json::json!(value),
        rusqlite::types::ValueRef::Real(value) => serde_json::Number::from_f64(value)
            .map(Value::Number)
            .unwrap_or(Value::Null),
        rusqlite::types::ValueRef::Text(value) => {
            Value::String(String::from_utf8_lossy(value).into_owned())
        }
        rusqlite::types::ValueRef::Blob(value) => {
            Value::String(String::from_utf8_lossy(value).into_owned())
        }
    }
}

fn execute_store_message(
    statement: &mut Statement<'_>,
    record: &MessageRecord,
    now: &str,
) -> StoreResult<usize> {
    let owner_did = normalize_owner_did(&record.owner_did);
    let sender_did = normalize_optional_string(&record.sender_did);
    let receiver_did = normalize_optional_string(&record.receiver_did);
    let group_id = normalize_optional_string(&record.group_id);
    let group_did = normalize_optional_string(&record.group_did);
    let content_type = default_string(record.content_type.clone(), "text");
    let title = normalize_optional_string(&record.title);
    let server_seq = normalize_optional_int64(record.server_seq);
    let sent_at = normalize_optional_string(&record.sent_at);
    let stored_at = default_string(record.stored_at.clone(), now);
    let sender_name = normalize_optional_string(&record.sender_name);
    let metadata = normalize_metadata(&record.metadata);
    let credential_name = normalize_credential_name(&record.credential_name);
    Ok(statement.execute(params![
        record.msg_id.as_str(),
        owner_did,
        record.thread_id.as_str(),
        record.direction,
        sender_did,
        receiver_did,
        group_id,
        group_did,
        content_type,
        record.content.as_str(),
        title,
        server_seq,
        sent_at,
        stored_at,
        bool_to_int(record.is_e2ee),
        bool_to_int(record.is_read),
        sender_name,
        metadata,
        credential_name,
    ])?)
}

fn store_message_sql() -> &'static str {
    r#"
INSERT INTO messages
    (msg_id, owner_did, thread_id, direction, sender_did, receiver_did, group_id, group_did,
     content_type, content, title, server_seq, sent_at, stored_at, is_e2ee, is_read,
     sender_name, metadata, credential_name)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)
ON CONFLICT(msg_id, owner_did)
DO UPDATE SET
    thread_id = excluded.thread_id,
    direction = excluded.direction,
    sender_did = excluded.sender_did,
    receiver_did = excluded.receiver_did,
    group_id = excluded.group_id,
    group_did = excluded.group_did,
    content_type = CASE
        WHEN excluded.content_type IN ('application/anp-direct-init+json', 'application/anp-direct-cipher+json')
             AND messages.content_type NOT IN ('application/anp-direct-init+json', 'application/anp-direct-cipher+json')
        THEN messages.content_type
        ELSE excluded.content_type
    END,
    content = CASE
        WHEN excluded.content_type IN ('application/anp-direct-init+json', 'application/anp-direct-cipher+json')
             AND messages.content_type NOT IN ('application/anp-direct-init+json', 'application/anp-direct-cipher+json')
        THEN messages.content
        ELSE excluded.content
    END,
    title = excluded.title,
    server_seq = COALESCE(excluded.server_seq, messages.server_seq),
    sent_at = COALESCE(excluded.sent_at, messages.sent_at),
    is_e2ee = CASE WHEN excluded.is_e2ee = 1 OR messages.is_e2ee = 1 THEN 1 ELSE 0 END,
    is_read = CASE WHEN excluded.is_read = 1 OR messages.is_read = 1 THEN 1 ELSE 0 END,
    sender_name = COALESCE(excluded.sender_name, messages.sender_name),
    metadata = CASE
        WHEN excluded.content_type IN ('application/anp-direct-init+json', 'application/anp-direct-cipher+json')
             AND messages.content_type NOT IN ('application/anp-direct-init+json', 'application/anp-direct-cipher+json')
        THEN messages.metadata
        ELSE COALESCE(excluded.metadata, messages.metadata)
    END,
    credential_name = COALESCE(excluded.credential_name, messages.credential_name)"#
}

fn local_mail_notification_predicate() -> &'static str {
    r#"COALESCE(content_type, '') = 'mail.notification' OR COALESCE(metadata, '') LIKE '%"source_kind":"mail"%'"#
}
