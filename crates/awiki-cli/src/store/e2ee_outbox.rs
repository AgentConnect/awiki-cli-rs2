use super::helpers::{
    default_string, generate_id, normalize_credential_name, normalize_metadata,
    normalize_optional_int64, normalize_optional_string, normalize_owner_did, now_utc,
};
use super::{StoreError, StoreResult};
use rusqlite::{params, Connection};
use serde_json::{json, Map, Number, Value};

#[derive(Debug, Clone, Default)]
pub struct E2EEOutboxRecord {
    pub outbox_id: String,
    pub owner_did: String,
    pub peer_did: String,
    pub session_id: String,
    pub original_type: String,
    pub plaintext: String,
    pub local_status: String,
    pub attempt_count: i64,
    pub sent_msg_id: String,
    pub sent_server_seq: Option<i64>,
    pub last_error_code: String,
    pub retry_hint: String,
    pub failed_msg_id: String,
    pub failed_server_seq: Option<i64>,
    pub metadata: String,
    pub last_attempt_at: String,
    pub created_at: String,
    pub updated_at: String,
    pub credential_name: String,
}

pub fn queue_e2ee_outbox(connection: &Connection, record: E2EEOutboxRecord) -> StoreResult<String> {
    let outbox_id = default_string(record.outbox_id, &generate_id());
    let now = now_utc();
    connection.execute(
        r#"
INSERT INTO e2ee_outbox
    (outbox_id, owner_did, peer_did, session_id, original_type, plaintext, local_status,
     attempt_count, sent_msg_id, sent_server_seq, last_error_code, retry_hint, failed_msg_id,
     failed_server_seq, metadata, last_attempt_at, created_at, updated_at, credential_name)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)"#,
        params![
            outbox_id,
            normalize_owner_did(&record.owner_did),
            record.peer_did,
            normalize_optional_string(&record.session_id),
            default_string(record.original_type, "text"),
            record.plaintext,
            default_string(record.local_status, "queued"),
            record.attempt_count,
            normalize_optional_string(&record.sent_msg_id),
            normalize_optional_int64(record.sent_server_seq),
            normalize_optional_string(&record.last_error_code),
            normalize_optional_string(&record.retry_hint),
            normalize_optional_string(&record.failed_msg_id),
            normalize_optional_int64(record.failed_server_seq),
            normalize_metadata(&record.metadata),
            normalize_optional_string(&record.last_attempt_at),
            default_string(record.created_at, &now),
            default_string(record.updated_at, &now),
            normalize_credential_name(&record.credential_name),
        ],
    )?;
    Ok(outbox_id)
}

pub fn mark_e2ee_outbox_sent(
    connection: &Connection,
    outbox_id: &str,
    owner_did: &str,
    session_id: &str,
    sent_msg_id: &str,
    sent_server_seq: Option<i64>,
    metadata: &str,
) -> StoreResult<()> {
    connection.execute(
        r#"
UPDATE e2ee_outbox
SET session_id = COALESCE(?1, session_id),
    local_status = 'sent',
    attempt_count = attempt_count + 1,
    sent_msg_id = COALESCE(?2, sent_msg_id),
    sent_server_seq = COALESCE(?3, sent_server_seq),
    metadata = COALESCE(?4, metadata),
    last_attempt_at = ?5,
    updated_at = ?6,
    last_error_code = NULL,
    retry_hint = NULL,
    failed_msg_id = NULL,
    failed_server_seq = NULL
WHERE outbox_id = ?7 AND owner_did = ?8"#,
        params![
            normalize_optional_string(session_id),
            normalize_optional_string(sent_msg_id),
            normalize_optional_int64(sent_server_seq),
            normalize_metadata(metadata),
            now_utc(),
            now_utc(),
            outbox_id,
            normalize_owner_did(owner_did),
        ],
    )?;
    Ok(())
}

pub fn mark_e2ee_outbox_failed(
    connection: &Connection,
    outbox_id: &str,
    owner_did: &str,
    error_code: &str,
    retry_hint: &str,
    failed_msg_id: &str,
    failed_server_seq: Option<i64>,
    metadata: &str,
) -> StoreResult<()> {
    connection.execute(
        r#"
UPDATE e2ee_outbox
SET local_status = 'failed',
    last_error_code = ?1,
    retry_hint = COALESCE(?2, retry_hint),
    failed_msg_id = COALESCE(?3, failed_msg_id),
    failed_server_seq = COALESCE(?4, failed_server_seq),
    metadata = COALESCE(?5, metadata),
    updated_at = ?6
WHERE outbox_id = ?7 AND owner_did = ?8"#,
        params![
            error_code,
            normalize_optional_string(retry_hint),
            normalize_optional_string(failed_msg_id),
            normalize_optional_int64(failed_server_seq),
            normalize_metadata(metadata),
            now_utc(),
            outbox_id,
            normalize_owner_did(owner_did),
        ],
    )?;
    Ok(())
}

pub fn update_e2ee_outbox_status(
    connection: &Connection,
    outbox_id: &str,
    owner_did: &str,
    credential_name: &str,
    status: &str,
) -> StoreResult<()> {
    if !owner_did.trim().is_empty() {
        connection.execute(
            "UPDATE e2ee_outbox SET local_status = ?1, updated_at = ?2 WHERE outbox_id = ?3 AND owner_did = ?4",
            params![status, now_utc(), outbox_id, normalize_owner_did(owner_did)],
        )?;
        return Ok(());
    }
    connection.execute(
        "UPDATE e2ee_outbox SET local_status = ?1, updated_at = ?2 WHERE outbox_id = ?3 AND credential_name = ?4",
        params![status, now_utc(), outbox_id, normalize_credential_name(credential_name)],
    )?;
    Ok(())
}

pub fn set_e2ee_outbox_failure_by_id(
    connection: &Connection,
    outbox_id: &str,
    owner_did: &str,
    credential_name: &str,
    error_code: &str,
    retry_hint: &str,
    metadata: &str,
) -> StoreResult<()> {
    if !owner_did.trim().is_empty() {
        connection.execute(
            r#"
UPDATE e2ee_outbox
SET local_status = 'failed',
    last_error_code = ?1,
    retry_hint = COALESCE(?2, retry_hint),
    metadata = COALESCE(?3, metadata),
    updated_at = ?4
WHERE outbox_id = ?5 AND owner_did = ?6"#,
            params![
                error_code,
                normalize_optional_string(retry_hint),
                normalize_metadata(metadata),
                now_utc(),
                outbox_id,
                normalize_owner_did(owner_did),
            ],
        )?;
        return Ok(());
    }
    connection.execute(
        r#"
UPDATE e2ee_outbox
SET local_status = 'failed',
    last_error_code = ?1,
    retry_hint = COALESCE(?2, retry_hint),
    metadata = COALESCE(?3, metadata),
    updated_at = ?4
WHERE outbox_id = ?5 AND credential_name = ?6"#,
        params![
            error_code,
            normalize_optional_string(retry_hint),
            normalize_metadata(metadata),
            now_utc(),
            outbox_id,
            normalize_credential_name(credential_name),
        ],
    )?;
    Ok(())
}

pub fn get_e2ee_outbox(
    connection: &Connection,
    outbox_id: &str,
    owner_did: &str,
    credential_name: &str,
) -> StoreResult<Value> {
    if !owner_did.trim().is_empty() {
        return query_one_with_params(
            connection,
            "SELECT * FROM e2ee_outbox WHERE outbox_id = ?1 AND owner_did = ?2",
            &[&outbox_id, &normalize_owner_did(owner_did)],
        );
    }
    query_one_with_params(
        connection,
        "SELECT * FROM e2ee_outbox WHERE outbox_id = ?1 AND credential_name = ?2",
        &[&outbox_id, &normalize_credential_name(credential_name)],
    )
}

pub fn list_e2ee_outbox(
    connection: &Connection,
    owner_did: &str,
    credential_name: &str,
    local_status: &str,
) -> StoreResult<Vec<Value>> {
    match (
        !owner_did.trim().is_empty(),
        !local_status.trim().is_empty(),
    ) {
        (true, true) => query_rows_with_params(
            connection,
            "SELECT * FROM e2ee_outbox WHERE owner_did = ?1 AND local_status = ?2 ORDER BY updated_at DESC",
            &[&normalize_owner_did(owner_did), &local_status],
        ),
        (true, false) => query_rows_with_params(
            connection,
            "SELECT * FROM e2ee_outbox WHERE owner_did = ?1 ORDER BY updated_at DESC",
            &[&normalize_owner_did(owner_did)],
        ),
        (false, true) => query_rows_with_params(
            connection,
            "SELECT * FROM e2ee_outbox WHERE credential_name = ?1 AND local_status = ?2 ORDER BY updated_at DESC",
            &[&normalize_credential_name(credential_name), &local_status],
        ),
        (false, false) => query_rows_with_params(
            connection,
            "SELECT * FROM e2ee_outbox WHERE credential_name = ?1 ORDER BY updated_at DESC",
            &[&normalize_credential_name(credential_name)],
        ),
    }
}

fn query_one_with_params(
    connection: &Connection,
    statement: &str,
    params: &[&dyn rusqlite::ToSql],
) -> StoreResult<Value> {
    let rows = query_rows_with_params(connection, statement, params)?;
    rows.into_iter()
        .next()
        .ok_or_else(|| StoreError::NotFound("query returned no rows".to_string()))
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
        let mut object = Map::new();
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
        rusqlite::types::ValueRef::Integer(value) => json!(value),
        rusqlite::types::ValueRef::Real(value) => Number::from_f64(value)
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
