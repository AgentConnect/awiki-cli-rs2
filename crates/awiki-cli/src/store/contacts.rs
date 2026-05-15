use super::helpers::{
    bool_to_int, default_bool_value, default_string, normalize_credential_name, normalize_metadata,
    normalize_optional_bool, normalize_optional_string, normalize_owner_did, now_utc,
};
use super::{StoreError, StoreResult};
use rusqlite::{params, Connection, OptionalExtension, Statement};
use serde_json::Value;

#[derive(Debug, Clone, Default)]
pub struct ContactRecord {
    pub owner_did: String,
    pub did: String,
    pub name: String,
    pub handle: String,
    pub nick_name: String,
    pub bio: String,
    pub profile_md: String,
    pub tags: String,
    pub relationship: String,
    pub source_type: String,
    pub source_name: String,
    pub source_group_id: String,
    pub connected_at: String,
    pub recommended_reason: String,
    pub followed: Option<bool>,
    pub messaged: Option<bool>,
    pub note: String,
    pub first_seen_at: String,
    pub last_seen_at: String,
    pub metadata: String,
    pub credential_name: String,
}

#[derive(Debug, Clone, Default)]
struct ContactHandleBindingRecord {
    owner_did: String,
    handle: String,
    did: String,
    is_current: bool,
    first_seen_at: String,
    last_seen_at: String,
    source_type: String,
    source_group_id: String,
    metadata: String,
    credential_name: String,
}

pub fn get_contact_by_did(
    connection: &Connection,
    owner_did: &str,
    did: &str,
) -> StoreResult<Value> {
    query_one_json(
        connection,
        "SELECT * FROM contacts WHERE owner_did = ?1 AND did = ?2",
        &[&normalize_owner_did(owner_did), &did.trim().to_string()],
    )
}

pub fn get_current_contact_by_handle(
    connection: &Connection,
    owner_did: &str,
    handle: &str,
) -> StoreResult<Value> {
    query_one_json(
        connection,
        "SELECT * FROM contacts WHERE owner_did = ?1 AND handle = ?2",
        &[&normalize_owner_did(owner_did), &handle.trim().to_string()],
    )
}

pub fn resolve_contact_handle_by_did(
    connection: &Connection,
    owner_did: &str,
    did: &str,
) -> StoreResult<String> {
    let owner_did = normalize_owner_did(owner_did);
    let did = did.trim().to_string();
    let contact_handle = connection
        .query_row(
            "SELECT handle FROM contacts WHERE owner_did = ?1 AND did = ?2 AND TRIM(COALESCE(handle, '')) <> ''",
            params![owner_did.as_str(), did.as_str()],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()?
        .flatten()
        .unwrap_or_default();
    if !contact_handle.is_empty() {
        return Ok(contact_handle);
    }
    let binding_handle = connection
        .query_row(
            r#"
SELECT handle
FROM contact_handle_bindings
WHERE owner_did = ?1 AND did = ?2
ORDER BY is_current DESC, last_seen_at DESC
LIMIT 1"#,
            params![owner_did.as_str(), did.as_str()],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()?
        .flatten()
        .unwrap_or_default();
    Ok(binding_handle)
}

pub fn list_dids_by_handle(
    connection: &Connection,
    owner_did: &str,
    handle: &str,
) -> StoreResult<Vec<String>> {
    let owner_did = normalize_owner_did(owner_did);
    let handle = handle.trim().to_string();
    let mut statement = connection.prepare(
        r#"
SELECT did
FROM contact_handle_bindings
WHERE owner_did = ?1 AND handle = ?2
ORDER BY is_current DESC, last_seen_at DESC"#,
    )?;
    let mut rows = statement.query(params![owner_did.as_str(), handle.as_str()])?;
    let mut result = Vec::new();
    while let Some(row) = rows.next()? {
        let did = row.get::<_, Option<String>>(0)?.unwrap_or_default();
        if did.is_empty() || result.iter().any(|known| known == &did) {
            continue;
        }
        result.push(did);
    }
    if !result.is_empty() {
        return Ok(result);
    }
    let did = connection
        .query_row(
            "SELECT did FROM contacts WHERE owner_did = ?1 AND handle = ?2",
            params![owner_did.as_str(), handle.as_str()],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()?
        .flatten()
        .unwrap_or_default();
    if did.is_empty() {
        Ok(Vec::new())
    } else {
        Ok(vec![did])
    }
}

pub fn upsert_contact(connection: &mut Connection, record: ContactRecord) -> StoreResult<()> {
    if record.did.trim().is_empty() {
        return Err(StoreError::Invalid("contact did is required".to_string()));
    }
    let owner_did = normalize_owner_did(&record.owner_did);
    let did = record.did.trim().to_string();
    let handle = record.handle.trim().to_string();
    let transaction = connection.transaction()?;
    let now = now_utc();
    let existing_by_did = query_contact_did_handle(
        &transaction,
        "SELECT did, handle FROM contacts WHERE owner_did = ?1 AND did = ?2",
        &owner_did,
        &did,
    )?;
    let existing_by_handle = if handle.is_empty() {
        Vec::new()
    } else {
        query_contact_did_handle(
            &transaction,
            "SELECT did, handle FROM contacts WHERE owner_did = ?1 AND handle = ?2",
            &owner_did,
            &handle,
        )?
    };
    if !handle.is_empty() && !existing_by_handle.is_empty() && existing_by_handle[0].0.trim() != did
    {
        transaction.execute(
            "UPDATE contacts SET handle = NULL, last_seen_at = ?1 WHERE owner_did = ?2 AND did = ?3",
            params![now.as_str(), owner_did.as_str(), existing_by_handle[0].0.as_str()],
        )?;
    }
    if existing_by_did.is_empty() {
        insert_contact(&transaction, &record, &owner_did, &did, &now)?;
    } else {
        update_contact(&transaction, &record, &owner_did, &did, &handle, &now)?;
    }
    if !handle.is_empty() {
        upsert_contact_handle_binding(
            &transaction,
            ContactHandleBindingRecord {
                owner_did: owner_did.clone(),
                handle,
                did,
                is_current: true,
                first_seen_at: default_string(record.first_seen_at.clone(), &now),
                last_seen_at: default_string(record.last_seen_at.clone(), &now),
                source_type: record.source_type.clone(),
                source_group_id: record.source_group_id.clone(),
                metadata: record.metadata.clone(),
                credential_name: record.credential_name.clone(),
            },
        )?;
    }
    transaction.commit()?;
    Ok(())
}

fn insert_contact(
    connection: &Connection,
    record: &ContactRecord,
    owner_did: &str,
    did: &str,
    now: &str,
) -> StoreResult<()> {
    connection.execute(
        r#"
INSERT INTO contacts
    (owner_did, did, name, handle, nick_name, bio, profile_md, tags, relationship, source_type,
     source_name, source_group_id, connected_at, recommended_reason, followed, messaged, note,
     first_seen_at, last_seen_at, metadata)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20)"#,
        params![
            owner_did,
            did,
            normalize_optional_string(&record.name),
            normalize_optional_string(&record.handle),
            normalize_optional_string(&record.nick_name),
            normalize_optional_string(&record.bio),
            normalize_optional_string(&record.profile_md),
            normalize_optional_string(&record.tags),
            normalize_optional_string(&record.relationship),
            normalize_optional_string(&record.source_type),
            normalize_optional_string(&record.source_name),
            normalize_optional_string(&record.source_group_id),
            normalize_optional_string(&record.connected_at),
            normalize_optional_string(&record.recommended_reason),
            default_bool_value(record.followed),
            default_bool_value(record.messaged),
            normalize_optional_string(&record.note),
            default_string(record.first_seen_at.clone(), now),
            default_string(record.last_seen_at.clone(), now),
            normalize_metadata(&record.metadata),
        ],
    )?;
    Ok(())
}

fn update_contact(
    connection: &Connection,
    record: &ContactRecord,
    owner_did: &str,
    did: &str,
    handle: &str,
    now: &str,
) -> StoreResult<()> {
    connection.execute(
        r#"
UPDATE contacts
SET name = COALESCE(?1, name),
    handle = COALESCE(?2, handle),
    nick_name = COALESCE(?3, nick_name),
    bio = COALESCE(?4, bio),
    profile_md = COALESCE(?5, profile_md),
    tags = COALESCE(?6, tags),
    relationship = COALESCE(?7, relationship),
    source_type = COALESCE(?8, source_type),
    source_name = COALESCE(?9, source_name),
    source_group_id = COALESCE(?10, source_group_id),
    connected_at = COALESCE(?11, connected_at),
    recommended_reason = COALESCE(?12, recommended_reason),
    followed = COALESCE(?13, followed),
    messaged = COALESCE(?14, messaged),
    note = COALESCE(?15, note),
    first_seen_at = COALESCE(?16, first_seen_at),
    last_seen_at = ?17,
    metadata = COALESCE(?18, metadata)
WHERE owner_did = ?19 AND did = ?20"#,
        params![
            normalize_optional_string(&record.name),
            normalize_optional_string(handle),
            normalize_optional_string(&record.nick_name),
            normalize_optional_string(&record.bio),
            normalize_optional_string(&record.profile_md),
            normalize_optional_string(&record.tags),
            normalize_optional_string(&record.relationship),
            normalize_optional_string(&record.source_type),
            normalize_optional_string(&record.source_name),
            normalize_optional_string(&record.source_group_id),
            normalize_optional_string(&record.connected_at),
            normalize_optional_string(&record.recommended_reason),
            normalize_optional_bool(record.followed),
            normalize_optional_bool(record.messaged),
            normalize_optional_string(&record.note),
            normalize_optional_string(&record.first_seen_at),
            now,
            normalize_metadata(&record.metadata),
            owner_did,
            did,
        ],
    )?;
    Ok(())
}

fn upsert_contact_handle_binding(
    connection: &Connection,
    record: ContactHandleBindingRecord,
) -> StoreResult<()> {
    let handle = record.handle.trim().to_string();
    let did = record.did.trim().to_string();
    if handle.is_empty() || did.is_empty() {
        return Ok(());
    }
    let owner_did = normalize_owner_did(&record.owner_did);
    let first_seen_at = default_string(record.first_seen_at.clone(), &now_utc());
    let last_seen_at = default_string(record.last_seen_at.clone(), &first_seen_at);
    if record.is_current {
        connection.execute(
            r#"
UPDATE contact_handle_bindings
SET is_current = 0,
    last_seen_at = CASE
        WHEN last_seen_at IS NULL OR last_seen_at < ?1 THEN ?2
        ELSE last_seen_at
    END
WHERE owner_did = ?3 AND handle = ?4 AND did <> ?5"#,
            params![
                last_seen_at.as_str(),
                last_seen_at.as_str(),
                owner_did.as_str(),
                handle.as_str(),
                did.as_str(),
            ],
        )?;
    }
    connection.execute(
        r#"
INSERT INTO contact_handle_bindings
    (owner_did, handle, did, is_current, first_seen_at, last_seen_at, source_type,
     source_group_id, metadata, credential_name)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
ON CONFLICT(owner_did, handle, did)
DO UPDATE SET
    is_current = excluded.is_current,
    first_seen_at = COALESCE(contact_handle_bindings.first_seen_at, excluded.first_seen_at),
    last_seen_at = excluded.last_seen_at,
    source_type = COALESCE(excluded.source_type, contact_handle_bindings.source_type),
    source_group_id = COALESCE(excluded.source_group_id, contact_handle_bindings.source_group_id),
    metadata = COALESCE(excluded.metadata, contact_handle_bindings.metadata),
    credential_name = COALESCE(excluded.credential_name, contact_handle_bindings.credential_name)"#,
        params![
            owner_did.as_str(),
            handle.as_str(),
            did.as_str(),
            bool_to_int(record.is_current),
            first_seen_at.as_str(),
            last_seen_at.as_str(),
            normalize_optional_string(&record.source_type),
            normalize_optional_string(&record.source_group_id),
            normalize_metadata(&record.metadata),
            normalize_credential_name(&record.credential_name),
        ],
    )?;
    Ok(())
}

fn query_contact_did_handle(
    connection: &Connection,
    statement: &str,
    owner_did: &str,
    value: &str,
) -> StoreResult<Vec<(String, String)>> {
    let mut statement = connection.prepare(statement)?;
    let rows = statement.query_map(params![owner_did, value], |row| {
        Ok((
            row.get::<_, Option<String>>(0)?.unwrap_or_default(),
            row.get::<_, Option<String>>(1)?.unwrap_or_default(),
        ))
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

fn query_one_json(
    connection: &Connection,
    statement: &str,
    params: &[&dyn rusqlite::ToSql],
) -> StoreResult<Value> {
    let mut statement = connection.prepare(statement)?;
    let names = column_names(&statement);
    let mut rows = statement.query(params)?;
    if let Some(row) = rows.next()? {
        return row_to_json(row, &names);
    }
    Err(StoreError::NotFound("query returned no rows".to_string()))
}

fn column_names(statement: &Statement<'_>) -> Vec<String> {
    statement
        .column_names()
        .into_iter()
        .map(ToOwned::to_owned)
        .collect()
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
