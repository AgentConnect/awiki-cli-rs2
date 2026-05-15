use super::helpers::{
    default_string, normalize_credential_name, normalize_metadata, normalize_optional_bool,
    normalize_optional_int64, normalize_optional_string, normalize_owner_did, now_utc,
};
use super::{StoreError, StoreResult};
use rusqlite::{params, Connection, Statement};
use serde_json::Value;

#[derive(Debug, Clone, Default)]
pub struct GroupRecord {
    pub owner_did: String,
    pub group_id: String,
    pub group_did: String,
    pub name: String,
    pub group_mode: String,
    pub slug: String,
    pub description: String,
    pub goal: String,
    pub rules: String,
    pub message_prompt: String,
    pub doc_url: String,
    pub group_owner_did: String,
    pub group_owner_handle: String,
    pub my_role: String,
    pub membership_status: String,
    pub join_enabled: Option<bool>,
    pub join_code: String,
    pub join_code_expires_at: String,
    pub member_count: Option<i64>,
    pub last_synced_seq: Option<i64>,
    pub last_read_seq: Option<i64>,
    pub last_message_at: String,
    pub remote_created_at: String,
    pub remote_updated_at: String,
    pub stored_at: String,
    pub metadata: String,
    pub credential_name: String,
}

#[derive(Debug, Clone, Default)]
pub struct GroupMemberRecord {
    pub owner_did: String,
    pub group_id: String,
    pub user_id: String,
    pub member_did: String,
    pub member_handle: String,
    pub profile_url: String,
    pub role: String,
    pub status: String,
    pub joined_at: String,
    pub sent_message_count: Option<i64>,
    pub last_synced_at: String,
    pub metadata: String,
    pub credential_name: String,
}

pub fn upsert_group(connection: &Connection, record: GroupRecord) -> StoreResult<()> {
    let owner_did = normalize_owner_did(&record.owner_did);
    let group_id = record.group_id.trim().to_string();
    if owner_did.is_empty() || group_id.is_empty() {
        return Err(StoreError::Invalid(
            "owner_did and group_id are required".to_string(),
        ));
    }
    let stored_at = default_string(record.stored_at.clone(), &now_utc());
    let mut statement = connection.prepare(upsert_group_sql())?;
    execute_upsert_group(&mut statement, &record, &owner_did, &group_id, &stored_at)?;
    Ok(())
}

pub fn upsert_group_member(connection: &Connection, record: GroupMemberRecord) -> StoreResult<()> {
    let owner_did = normalize_owner_did(&record.owner_did);
    let group_id = record.group_id.trim().to_string();
    let user_id = record.user_id.trim().to_string();
    validate_member_key(&owner_did, &group_id, &user_id)?;
    let last_synced_at = default_string(record.last_synced_at.clone(), &now_utc());
    let mut statement = connection.prepare(upsert_group_member_sql())?;
    execute_upsert_group_member(
        &mut statement,
        &record,
        &owner_did,
        &group_id,
        &user_id,
        &last_synced_at,
        &record.credential_name,
    )?;
    Ok(())
}

pub fn replace_group_members(
    connection: &mut Connection,
    owner_did: &str,
    group_id: &str,
    members: &[GroupMemberRecord],
    credential_name: &str,
) -> StoreResult<()> {
    let owner_did = normalize_owner_did(owner_did);
    let group_id = group_id.trim().to_string();
    if owner_did.is_empty() || group_id.is_empty() {
        return Err(StoreError::Invalid(
            "owner_did and group_id are required".to_string(),
        ));
    }
    let transaction = connection.transaction()?;
    transaction.execute(
        "DELETE FROM group_members WHERE owner_did = ?1 AND group_id = ?2",
        params![owner_did.as_str(), group_id.as_str()],
    )?;
    let now = now_utc();
    {
        let mut statement = transaction.prepare(insert_group_member_sql())?;
        for member in members {
            let user_id = member.user_id.trim().to_string();
            if user_id.is_empty() {
                continue;
            }
            let last_synced_at = default_string(member.last_synced_at.clone(), &now);
            execute_insert_group_member(
                &mut statement,
                member,
                &owner_did,
                &group_id,
                &user_id,
                &last_synced_at,
                credential_name,
            )?;
        }
    }
    transaction.commit()?;
    Ok(())
}

pub fn get_group_snapshot(
    connection: &Connection,
    owner_did: &str,
    group_id: &str,
) -> StoreResult<Value> {
    let owner_did = normalize_owner_did(owner_did);
    let group_id = group_id.trim().to_string();
    let mut statement = connection.prepare(
        r#"
SELECT *
FROM groups
WHERE owner_did = ?1 AND (group_id = ?2 OR group_did = ?2)
LIMIT 1"#,
    )?;
    let names = column_names(&statement);
    let mut rows = statement.query(params![owner_did.as_str(), group_id.as_str()])?;
    if let Some(row) = rows.next()? {
        return row_to_json(row, &names);
    }
    Err(StoreError::NotFound(format!("group not found: {group_id}")))
}

pub fn list_cached_group_members(
    connection: &Connection,
    owner_did: &str,
    group_id: &str,
    limit: i64,
) -> StoreResult<Vec<Value>> {
    let owner_did = normalize_owner_did(owner_did);
    let group_id = group_id.trim().to_string();
    if group_id.is_empty() {
        return Err(StoreError::Invalid("group_id is required".to_string()));
    }
    let limit = if limit <= 0 { 100 } else { limit };
    query_rows_with_params(
        connection,
        r#"
SELECT *
FROM group_members
WHERE owner_did = ?1
  AND group_id IN (
        SELECT ?2
        UNION
        SELECT group_id
        FROM groups
        WHERE owner_did = ?1 AND (group_id = ?2 OR group_did = ?2)
  )
ORDER BY role ASC, member_handle ASC, member_did ASC
LIMIT ?3"#,
        &[&owner_did, &group_id, &limit],
    )
}

pub fn list_group_messages(
    connection: &Connection,
    owner_did: &str,
    group_id: &str,
    limit: i64,
    since_seq: Option<i64>,
) -> StoreResult<Vec<Value>> {
    let owner_did = normalize_owner_did(owner_did);
    let group_id = group_id.trim().to_string();
    if group_id.is_empty() {
        return Err(StoreError::Invalid("group_id is required".to_string()));
    }
    let limit = if limit <= 0 { 50 } else { limit };
    if let Some(since_seq) = since_seq {
        return query_rows_with_params(
            connection,
            r#"
SELECT *
FROM messages
WHERE owner_did = ?1
  AND (group_did = ?2 OR group_id = ?2)
  AND COALESCE(server_seq, 0) > ?3
ORDER BY COALESCE(server_seq, 0) DESC, COALESCE(sent_at, stored_at) DESC
LIMIT ?4"#,
            &[&owner_did, &group_id, &since_seq, &limit],
        );
    }
    query_rows_with_params(
        connection,
        r#"
SELECT *
FROM messages
WHERE owner_did = ?1
  AND (group_did = ?2 OR group_id = ?2)
ORDER BY COALESCE(server_seq, 0) DESC, COALESCE(sent_at, stored_at) DESC
LIMIT ?3"#,
        &[&owner_did, &group_id, &limit],
    )
}

pub fn mark_group_left(
    connection: &mut Connection,
    owner_did: &str,
    group_id: &str,
    group_did: &str,
    credential_name: &str,
) -> StoreResult<()> {
    let owner_did = normalize_owner_did(owner_did);
    let group_id = group_id.trim().to_string();
    if owner_did.is_empty() || group_id.is_empty() {
        return Err(StoreError::Invalid(
            "owner_did and group_id are required".to_string(),
        ));
    }
    let now = now_utc();
    let group_did = normalize_optional_string(group_did);
    let credential_name = normalize_credential_name(credential_name);
    let transaction = connection.transaction()?;
    transaction.execute(
        r#"
INSERT INTO groups
    (owner_did, group_id, group_did, group_mode, my_role, membership_status, stored_at,
     credential_name)
VALUES (?1, ?2, ?3, 'general', NULL, 'left', ?4, ?5)
ON CONFLICT(owner_did, group_id)
DO UPDATE SET
    group_did = COALESCE(excluded.group_did, groups.group_did),
    my_role = NULL,
    membership_status = 'left',
    stored_at = excluded.stored_at,
    credential_name = COALESCE(excluded.credential_name, groups.credential_name)"#,
        params![
            owner_did.as_str(),
            group_id.as_str(),
            group_did,
            now.as_str(),
            credential_name,
        ],
    )?;
    transaction.execute(
        "DELETE FROM group_members WHERE owner_did = ?1 AND group_id = ?2",
        params![owner_did.as_str(), group_id.as_str()],
    )?;
    transaction.commit()?;
    Ok(())
}

pub fn touch_group_after_message(
    connection: &Connection,
    owner_did: &str,
    group_id: &str,
    group_did: &str,
    last_message_at: &str,
    last_synced_seq: Option<i64>,
    credential_name: &str,
    metadata: &str,
) -> StoreResult<()> {
    let owner_did = normalize_owner_did(owner_did);
    let group_id = group_id.trim().to_string();
    if owner_did.is_empty() || group_id.is_empty() {
        return Err(StoreError::Invalid(
            "owner_did and group_id are required".to_string(),
        ));
    }
    let now = now_utc();
    connection.execute(
        r#"
INSERT INTO groups
    (owner_did, group_id, group_did, group_mode, membership_status, last_synced_seq,
     last_message_at, stored_at, metadata, credential_name)
VALUES (?1, ?2, ?3, 'general', 'active', ?4, ?5, ?6, ?7, ?8)
ON CONFLICT(owner_did, group_id)
DO UPDATE SET
    group_did = COALESCE(excluded.group_did, groups.group_did),
    last_synced_seq = CASE
        WHEN excluded.last_synced_seq IS NULL THEN groups.last_synced_seq
        WHEN groups.last_synced_seq IS NULL THEN excluded.last_synced_seq
        WHEN excluded.last_synced_seq > groups.last_synced_seq THEN excluded.last_synced_seq
        ELSE groups.last_synced_seq
    END,
    last_message_at = COALESCE(excluded.last_message_at, groups.last_message_at),
    stored_at = excluded.stored_at,
    metadata = COALESCE(excluded.metadata, groups.metadata),
    credential_name = COALESCE(excluded.credential_name, groups.credential_name)"#,
        params![
            owner_did.as_str(),
            group_id.as_str(),
            normalize_optional_string(group_did),
            normalize_optional_int64(last_synced_seq),
            normalize_optional_string(last_message_at),
            now.as_str(),
            normalize_metadata(metadata),
            normalize_credential_name(credential_name),
        ],
    )?;
    Ok(())
}

fn execute_upsert_group(
    statement: &mut Statement<'_>,
    record: &GroupRecord,
    owner_did: &str,
    group_id: &str,
    stored_at: &str,
) -> StoreResult<usize> {
    Ok(statement.execute(params![
        owner_did,
        group_id,
        normalize_optional_string(&record.group_did),
        normalize_optional_string(&record.name),
        default_string(record.group_mode.clone(), "general"),
        normalize_optional_string(&record.slug),
        normalize_optional_string(&record.description),
        normalize_optional_string(&record.goal),
        normalize_optional_string(&record.rules),
        normalize_optional_string(&record.message_prompt),
        normalize_optional_string(&record.doc_url),
        normalize_optional_string(&record.group_owner_did),
        normalize_optional_string(&record.group_owner_handle),
        normalize_optional_string(&record.my_role),
        default_string(record.membership_status.clone(), "active"),
        normalize_optional_bool(record.join_enabled),
        normalize_optional_string(&record.join_code),
        normalize_optional_string(&record.join_code_expires_at),
        normalize_optional_int64(record.member_count),
        normalize_optional_int64(record.last_synced_seq),
        normalize_optional_int64(record.last_read_seq),
        normalize_optional_string(&record.last_message_at),
        normalize_optional_string(&record.remote_created_at),
        normalize_optional_string(&record.remote_updated_at),
        stored_at,
        normalize_metadata(&record.metadata),
        normalize_credential_name(&record.credential_name),
    ])?)
}

fn execute_upsert_group_member(
    statement: &mut Statement<'_>,
    record: &GroupMemberRecord,
    owner_did: &str,
    group_id: &str,
    user_id: &str,
    last_synced_at: &str,
    credential_name: &str,
) -> StoreResult<usize> {
    Ok(statement.execute(params![
        owner_did,
        group_id,
        user_id,
        normalize_optional_string(&record.member_did),
        normalize_optional_string(&record.member_handle),
        normalize_optional_string(&record.profile_url),
        normalize_optional_string(&record.role),
        default_string(record.status.clone(), "active"),
        normalize_optional_string(&record.joined_at),
        normalize_optional_int64(record.sent_message_count.or(Some(0))),
        last_synced_at,
        normalize_metadata(&record.metadata),
        normalize_credential_name(&default_string(
            record.credential_name.clone(),
            credential_name,
        )),
    ])?)
}

fn execute_insert_group_member(
    statement: &mut Statement<'_>,
    record: &GroupMemberRecord,
    owner_did: &str,
    group_id: &str,
    user_id: &str,
    last_synced_at: &str,
    credential_name: &str,
) -> StoreResult<usize> {
    Ok(statement.execute(params![
        owner_did,
        group_id,
        user_id,
        normalize_optional_string(&record.member_did),
        normalize_optional_string(&record.member_handle),
        normalize_optional_string(&record.profile_url),
        normalize_optional_string(&record.role),
        default_string(record.status.clone(), "active"),
        normalize_optional_string(&record.joined_at),
        normalize_optional_int64(record.sent_message_count.or(Some(0))),
        last_synced_at,
        normalize_metadata(&record.metadata),
        normalize_credential_name(&default_string(
            record.credential_name.clone(),
            credential_name,
        )),
    ])?)
}

fn validate_member_key(owner_did: &str, group_id: &str, user_id: &str) -> StoreResult<()> {
    if owner_did.is_empty() || group_id.is_empty() || user_id.is_empty() {
        return Err(StoreError::Invalid(
            "owner_did, group_id, and user_id are required".to_string(),
        ));
    }
    Ok(())
}

fn query_rows_with_params(
    connection: &Connection,
    statement: &str,
    params: &[&dyn rusqlite::ToSql],
) -> StoreResult<Vec<Value>> {
    let mut statement = connection.prepare(statement)?;
    let names = column_names(&statement);
    let mut rows = statement.query(params)?;
    let mut results = Vec::new();
    while let Some(row) = rows.next()? {
        results.push(row_to_json(row, &names)?);
    }
    Ok(results)
}

fn column_names(statement: &Statement<'_>) -> Vec<String> {
    statement
        .column_names()
        .into_iter()
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>()
}

fn row_to_json(row: &rusqlite::Row<'_>, names: &[String]) -> StoreResult<Value> {
    let mut object = serde_json::Map::new();
    for (index, name) in names.iter().enumerate() {
        object.insert(name.clone(), value_ref_to_json(row.get_ref(index)?));
    }
    Ok(Value::Object(object))
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

fn upsert_group_sql() -> &'static str {
    r#"
INSERT INTO groups
    (owner_did, group_id, group_did, name, group_mode, slug, description, goal, rules, message_prompt,
     doc_url, group_owner_did, group_owner_handle, my_role, membership_status, join_enabled, join_code,
     join_code_expires_at, member_count, last_synced_seq, last_read_seq, last_message_at,
     remote_created_at, remote_updated_at, stored_at, metadata, credential_name)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27)
ON CONFLICT(owner_did, group_id)
DO UPDATE SET
    group_did = excluded.group_did,
    name = excluded.name,
    group_mode = excluded.group_mode,
    slug = excluded.slug,
    description = excluded.description,
    goal = excluded.goal,
    rules = excluded.rules,
    message_prompt = excluded.message_prompt,
    doc_url = excluded.doc_url,
    group_owner_did = excluded.group_owner_did,
    group_owner_handle = excluded.group_owner_handle,
    my_role = excluded.my_role,
    membership_status = excluded.membership_status,
    join_enabled = excluded.join_enabled,
    join_code = excluded.join_code,
    join_code_expires_at = excluded.join_code_expires_at,
    member_count = excluded.member_count,
    last_synced_seq = excluded.last_synced_seq,
    last_read_seq = excluded.last_read_seq,
    last_message_at = excluded.last_message_at,
    remote_created_at = excluded.remote_created_at,
    remote_updated_at = excluded.remote_updated_at,
    stored_at = excluded.stored_at,
    metadata = excluded.metadata,
    credential_name = excluded.credential_name"#
}

fn insert_group_member_sql() -> &'static str {
    r#"
INSERT INTO group_members
    (owner_did, group_id, user_id, member_did, member_handle, profile_url, role, status,
     joined_at, sent_message_count, last_synced_at, metadata, credential_name)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)"#
}

fn upsert_group_member_sql() -> &'static str {
    r#"
INSERT INTO group_members
    (owner_did, group_id, user_id, member_did, member_handle, profile_url, role, status,
     joined_at, sent_message_count, last_synced_at, metadata, credential_name)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
ON CONFLICT(owner_did, group_id, user_id)
DO UPDATE SET
    member_did = excluded.member_did,
    member_handle = excluded.member_handle,
    profile_url = excluded.profile_url,
    role = excluded.role,
    status = excluded.status,
    joined_at = excluded.joined_at,
    sent_message_count = excluded.sent_message_count,
    last_synced_at = excluded.last_synced_at,
    metadata = excluded.metadata,
    credential_name = excluded.credential_name"#
}
