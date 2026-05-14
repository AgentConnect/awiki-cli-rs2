use super::records::{
    ContactHandleBindingRecord, ContactRecord, GroupMemberRecord, GroupRecord, MessageRecord,
    RelationshipEventRecord,
};
use crate::store::helpers::{
    bool_to_int, default_bool_value, default_string, normalize_credential_name, normalize_metadata,
    normalize_optional_bool, normalize_optional_float64, normalize_optional_int64,
    normalize_optional_string, normalize_owner_did, now_utc,
};
use crate::store::StoreResult;
use rusqlite::types::ValueRef;
use rusqlite::{params, OptionalExtension, Transaction};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

#[derive(Debug, Clone)]
pub(super) struct RowMap(BTreeMap<String, SqlValue>);

impl RowMap {
    fn get(&self, key: &str) -> Option<&SqlValue> {
        self.0.get(key)
    }
}

#[derive(Debug, Clone)]
enum SqlValue {
    Null,
    Integer(i64),
    Real(f64),
    Text(String),
    Blob(String),
}

pub(super) fn upsert_recovered_message(
    transaction: &Transaction<'_>,
    record: &MessageRecord,
) -> StoreResult<()> {
    let stored_at = default_string(record.stored_at.clone(), &now_utc());
    transaction.execute(
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
    content_type = excluded.content_type,
    content = excluded.content,
    title = excluded.title,
    server_seq = excluded.server_seq,
    sent_at = excluded.sent_at,
    stored_at = excluded.stored_at,
    is_e2ee = excluded.is_e2ee,
    is_read = excluded.is_read,
    sender_name = excluded.sender_name,
    metadata = excluded.metadata,
    credential_name = excluded.credential_name"#,
        params![
            record.msg_id,
            normalize_owner_did(&record.owner_did),
            record.thread_id,
            record.direction,
            normalize_optional_string(&record.sender_did),
            normalize_optional_string(&record.receiver_did),
            normalize_optional_string(&record.group_id),
            normalize_optional_string(&record.group_did),
            default_string(record.content_type.clone(), "text"),
            record.content,
            normalize_optional_string(&record.title),
            normalize_optional_int64(record.server_seq),
            normalize_optional_string(&record.sent_at),
            stored_at,
            bool_to_int(record.is_e2ee),
            bool_to_int(record.is_read),
            normalize_optional_string(&record.sender_name),
            normalize_metadata(&record.metadata),
            normalize_credential_name(&record.credential_name),
        ],
    )?;
    Ok(())
}

pub(super) fn upsert_recovered_contact(
    transaction: &Transaction<'_>,
    record: &ContactRecord,
) -> StoreResult<()> {
    transaction.execute(
        r#"
INSERT INTO contacts
    (owner_did, did, name, handle, nick_name, bio, profile_md, tags, relationship, source_type, source_name,
     source_group_id, connected_at, recommended_reason, followed, messaged, note, first_seen_at, last_seen_at, metadata)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20)
ON CONFLICT(owner_did, did)
DO UPDATE SET
    name = excluded.name,
    handle = excluded.handle,
    nick_name = excluded.nick_name,
    bio = excluded.bio,
    profile_md = excluded.profile_md,
    tags = excluded.tags,
    relationship = excluded.relationship,
    source_type = excluded.source_type,
    source_name = excluded.source_name,
    source_group_id = excluded.source_group_id,
    connected_at = excluded.connected_at,
    recommended_reason = excluded.recommended_reason,
    followed = excluded.followed,
    messaged = excluded.messaged,
    note = excluded.note,
    first_seen_at = excluded.first_seen_at,
    last_seen_at = excluded.last_seen_at,
    metadata = excluded.metadata"#,
        params![
            normalize_owner_did(&record.owner_did),
            record.did,
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
            normalize_optional_string(&record.first_seen_at),
            normalize_optional_string(&record.last_seen_at),
            normalize_metadata(&record.metadata),
        ],
    )?;
    Ok(())
}

pub(super) fn upsert_recovered_contact_handle_binding(
    transaction: &Transaction<'_>,
    record: &ContactHandleBindingRecord,
) -> StoreResult<()> {
    transaction.execute(
        r#"
INSERT INTO contact_handle_bindings
    (owner_did, handle, did, is_current, first_seen_at, last_seen_at, source_type, source_group_id, metadata, credential_name)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
ON CONFLICT(owner_did, handle, did)
DO UPDATE SET
    is_current = excluded.is_current,
    first_seen_at = excluded.first_seen_at,
    last_seen_at = excluded.last_seen_at,
    source_type = excluded.source_type,
    source_group_id = excluded.source_group_id,
    metadata = excluded.metadata,
    credential_name = excluded.credential_name"#,
        params![
            normalize_owner_did(&record.owner_did),
            record.handle,
            record.did,
            bool_to_int(record.is_current),
            record.first_seen_at,
            record.last_seen_at,
            normalize_optional_string(&record.source_type),
            normalize_optional_string(&record.source_group_id),
            normalize_metadata(&record.metadata),
            normalize_credential_name(&record.credential_name),
        ],
    )?;
    Ok(())
}

pub(super) fn upsert_recovered_relationship_event(
    transaction: &Transaction<'_>,
    record: &RelationshipEventRecord,
) -> StoreResult<()> {
    transaction.execute(
        r#"
INSERT INTO relationship_events
    (event_id, owner_did, target_did, target_handle, event_type, source_type, source_name, source_group_id,
     reason, score, status, created_at, updated_at, metadata, credential_name)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
ON CONFLICT(event_id)
DO UPDATE SET
    owner_did = excluded.owner_did,
    target_did = excluded.target_did,
    target_handle = excluded.target_handle,
    event_type = excluded.event_type,
    source_type = excluded.source_type,
    source_name = excluded.source_name,
    source_group_id = excluded.source_group_id,
    reason = excluded.reason,
    score = excluded.score,
    status = excluded.status,
    created_at = excluded.created_at,
    updated_at = excluded.updated_at,
    metadata = excluded.metadata,
    credential_name = excluded.credential_name"#,
        params![
            record.event_id,
            normalize_owner_did(&record.owner_did),
            record.target_did,
            normalize_optional_string(&record.target_handle),
            record.event_type,
            normalize_optional_string(&record.source_type),
            normalize_optional_string(&record.source_name),
            normalize_optional_string(&record.source_group_id),
            normalize_optional_string(&record.reason),
            normalize_optional_float64(record.score),
            default_string(record.status.clone(), "pending"),
            record.created_at,
            record.updated_at,
            normalize_metadata(&record.metadata),
            normalize_credential_name(&record.credential_name),
        ],
    )?;
    Ok(())
}

pub(super) fn upsert_recovered_group(
    transaction: &Transaction<'_>,
    record: &GroupRecord,
) -> StoreResult<()> {
    let stored_at = default_string(record.stored_at.clone(), &now_utc());
    transaction.execute(
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
    credential_name = excluded.credential_name"#,
        params![
            normalize_owner_did(&record.owner_did),
            record.group_id,
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
        ],
    )?;
    Ok(())
}

pub(super) fn upsert_recovered_group_member(
    transaction: &Transaction<'_>,
    record: &GroupMemberRecord,
) -> StoreResult<()> {
    let last_synced_at = default_string(record.last_synced_at.clone(), &now_utc());
    transaction.execute(
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
    credential_name = excluded.credential_name"#,
        params![
            normalize_owner_did(&record.owner_did),
            record.group_id,
            record.user_id,
            normalize_optional_string(&record.member_did),
            normalize_optional_string(&record.member_handle),
            normalize_optional_string(&record.profile_url),
            normalize_optional_string(&record.role),
            default_string(record.status.clone(), "active"),
            normalize_optional_string(&record.joined_at),
            normalize_optional_int64(record.sent_message_count),
            last_synced_at,
            normalize_metadata(&record.metadata),
            normalize_credential_name(&record.credential_name),
        ],
    )?;
    Ok(())
}

pub(super) fn normalize_recovered_current_handles(
    transaction: &Transaction<'_>,
    owner_did: &str,
    affected_handles: &BTreeSet<String>,
) -> StoreResult<()> {
    for handle in affected_handles {
        let current_did: Option<String> = transaction
            .query_row(
                r#"
SELECT did
FROM contact_handle_bindings
WHERE owner_did = ?1 AND handle = ?2
ORDER BY COALESCE(last_seen_at, first_seen_at, '') DESC, did DESC"#,
                params![owner_did, handle],
                |row| row.get(0),
            )
            .optional()?;
        let Some(current_did) = current_did else {
            continue;
        };
        transaction.execute(
            r#"
UPDATE contact_handle_bindings
SET is_current = CASE WHEN did = ?1 THEN 1 ELSE 0 END
WHERE owner_did = ?2 AND handle = ?3"#,
            params![current_did, normalize_owner_did(owner_did), handle],
        )?;
        transaction.execute(
            r#"
UPDATE contacts
SET handle = CASE WHEN did = ?1 THEN ?2 ELSE NULL END
WHERE owner_did = ?3 AND (did = ?1 OR handle = ?2)"#,
            params![current_did, handle, normalize_owner_did(owner_did)],
        )?;
    }
    Ok(())
}

pub(super) fn query_maps_with_param(
    transaction: &Transaction<'_>,
    sql: &str,
    value: &str,
) -> StoreResult<Vec<RowMap>> {
    let mut statement = transaction.prepare(sql)?;
    let names = statement
        .column_names()
        .into_iter()
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    let mut rows = statement.query([value])?;
    rows_to_maps(&mut rows, &names)
}

pub(super) fn query_one_map1(
    transaction: &Transaction<'_>,
    sql: &str,
    first: &str,
) -> StoreResult<Option<RowMap>> {
    let mut statement = transaction.prepare(sql)?;
    let names = statement
        .column_names()
        .into_iter()
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    let mut rows = statement.query([first])?;
    Ok(rows_to_maps(&mut rows, &names)?.into_iter().next())
}

pub(super) fn query_one_map2(
    transaction: &Transaction<'_>,
    sql: &str,
    first: &str,
    second: &str,
) -> StoreResult<Option<RowMap>> {
    let mut statement = transaction.prepare(sql)?;
    let names = statement
        .column_names()
        .into_iter()
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    let mut rows = statement.query(params![first, second])?;
    Ok(rows_to_maps(&mut rows, &names)?.into_iter().next())
}

pub(super) fn query_one_map3(
    transaction: &Transaction<'_>,
    sql: &str,
    first: &str,
    second: &str,
    third: &str,
) -> StoreResult<Option<RowMap>> {
    let mut statement = transaction.prepare(sql)?;
    let names = statement
        .column_names()
        .into_iter()
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    let mut rows = statement.query(params![first, second, third])?;
    Ok(rows_to_maps(&mut rows, &names)?.into_iter().next())
}

fn rows_to_maps(rows: &mut rusqlite::Rows<'_>, names: &[String]) -> StoreResult<Vec<RowMap>> {
    let mut results = Vec::new();
    while let Some(row) = rows.next()? {
        let mut object = BTreeMap::new();
        for (index, name) in names.iter().enumerate() {
            object.insert(name.clone(), value_ref_to_sql_value(row.get_ref(index)?));
        }
        results.push(RowMap(object));
    }
    Ok(results)
}

fn value_ref_to_sql_value(value: ValueRef<'_>) -> SqlValue {
    match value {
        ValueRef::Null => SqlValue::Null,
        ValueRef::Integer(value) => SqlValue::Integer(value),
        ValueRef::Real(value) => SqlValue::Real(value),
        ValueRef::Text(value) => SqlValue::Text(String::from_utf8_lossy(value).into_owned()),
        ValueRef::Blob(value) => SqlValue::Blob(String::from_utf8_lossy(value).into_owned()),
    }
}

pub(super) fn delete_rows_for_owners(
    transaction: &Transaction<'_>,
    table: &str,
    owners: &[String],
) -> StoreResult<()> {
    if owners.is_empty() {
        return Ok(());
    }
    let sql = format!(
        "DELETE FROM {table} WHERE owner_did IN ({})",
        placeholders(owners.len())
    );
    transaction.execute(&sql, rusqlite::params_from_iter(owners.iter()))?;
    Ok(())
}

pub(super) fn count_rows_for_owners(
    transaction: &Transaction<'_>,
    table: &str,
    owners: &[String],
) -> StoreResult<i64> {
    if owners.is_empty() {
        return Ok(0);
    }
    let sql = format!(
        "SELECT COUNT(*) FROM {table} WHERE owner_did IN ({})",
        placeholders(owners.len())
    );
    Ok(
        transaction.query_row(&sql, rusqlite::params_from_iter(owners.iter()), |row| {
            row.get(0)
        })?,
    )
}

fn placeholders(count: usize) -> String {
    std::iter::repeat("?")
        .take(count)
        .collect::<Vec<_>>()
        .join(",")
}

pub(super) fn string_from_row(row: &RowMap, key: &str) -> String {
    match row.get(key) {
        Some(SqlValue::Text(value)) => value.clone(),
        Some(SqlValue::Blob(value)) => value.clone(),
        _ => String::new(),
    }
}

pub(super) fn bool_from_row(row: &RowMap, key: &str) -> bool {
    match row.get(key) {
        Some(SqlValue::Integer(value)) => *value != 0,
        Some(SqlValue::Real(value)) => *value != 0.0,
        Some(SqlValue::Text(value)) => value == "1" || value.eq_ignore_ascii_case("true"),
        Some(SqlValue::Blob(value)) => value == "1" || value.eq_ignore_ascii_case("true"),
        Some(SqlValue::Null) | None => false,
    }
}

pub(super) fn bool_ptr_from_row(row: &RowMap, key: &str) -> Option<bool> {
    match row.get(key) {
        Some(SqlValue::Null) | None => None,
        Some(_) => Some(bool_from_row(row, key)),
    }
}

pub(super) fn int_from_row(row: &RowMap, key: &str) -> i64 {
    match row.get(key) {
        Some(SqlValue::Integer(value)) => *value,
        Some(SqlValue::Real(value)) => *value as i64,
        Some(SqlValue::Text(value)) | Some(SqlValue::Blob(value)) => {
            parse_i64_prefix(value).unwrap_or(0)
        }
        Some(SqlValue::Null) | None => 0,
    }
}

pub(super) fn int64_ptr_from_row(row: &RowMap, key: &str) -> Option<i64> {
    match row.get(key) {
        Some(SqlValue::Integer(value)) => Some(*value),
        Some(SqlValue::Real(value)) => Some(*value as i64),
        Some(SqlValue::Text(value)) | Some(SqlValue::Blob(value)) => {
            if value.trim().is_empty() {
                None
            } else {
                Some(parse_i64_prefix(value).unwrap_or(0))
            }
        }
        Some(SqlValue::Null) | None => None,
    }
}

pub(super) fn float64_ptr_from_row(row: &RowMap, key: &str) -> Option<f64> {
    match row.get(key) {
        Some(SqlValue::Real(value)) => Some(*value),
        Some(SqlValue::Integer(value)) => Some(*value as f64),
        Some(SqlValue::Text(value)) | Some(SqlValue::Blob(value)) => {
            if value.trim().is_empty() {
                None
            } else {
                Some(parse_f64_prefix(value).unwrap_or(0.0))
            }
        }
        Some(SqlValue::Null) | None => None,
    }
}

fn parse_i64_prefix(raw: &str) -> Option<i64> {
    let trimmed = raw.trim_start();
    if trimmed.is_empty() {
        return None;
    }
    let mut end = 0;
    for (index, ch) in trimmed.char_indices() {
        if index == 0 && (ch == '+' || ch == '-') {
            end = ch.len_utf8();
            continue;
        }
        if ch.is_ascii_digit() {
            end = index + ch.len_utf8();
            continue;
        }
        break;
    }
    if end == 0 || trimmed[..end].trim_matches(['+', '-']).is_empty() {
        None
    } else {
        trimmed[..end].parse::<i64>().ok()
    }
}

fn parse_f64_prefix(raw: &str) -> Option<f64> {
    let trimmed = raw.trim_start();
    if trimmed.is_empty() {
        return None;
    }
    trimmed
        .split_whitespace()
        .next()
        .and_then(|value| value.parse::<f64>().ok())
}

pub(super) fn zero_counts(tables: &[&str]) -> BTreeMap<String, i64> {
    tables
        .iter()
        .map(|table| ((*table).to_string(), 0))
        .collect()
}

pub(super) fn store_file_exists(path: &str) -> bool {
    Path::new(path).metadata().is_ok()
}
