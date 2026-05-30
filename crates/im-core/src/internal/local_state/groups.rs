#[cfg(feature = "sqlite")]
use time::OffsetDateTime;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct GroupRecord {
    pub(crate) owner_identity_id: String,
    pub(crate) owner_did: String,
    pub(crate) group_id: String,
    pub(crate) group_did: String,
    pub(crate) name: String,
    pub(crate) group_mode: String,
    pub(crate) slug: String,
    pub(crate) description: String,
    pub(crate) goal: String,
    pub(crate) rules: String,
    pub(crate) message_prompt: String,
    pub(crate) doc_url: String,
    pub(crate) group_owner_did: String,
    pub(crate) group_owner_handle: String,
    pub(crate) my_role: String,
    pub(crate) membership_status: String,
    pub(crate) join_enabled: Option<bool>,
    pub(crate) join_code: String,
    pub(crate) join_code_expires_at: String,
    pub(crate) member_count: Option<i64>,
    pub(crate) last_synced_seq: Option<i64>,
    pub(crate) last_read_seq: Option<i64>,
    pub(crate) last_message_at: String,
    pub(crate) remote_created_at: String,
    pub(crate) remote_updated_at: String,
    pub(crate) stored_at: String,
    pub(crate) metadata: String,
    pub(crate) credential_name: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct GroupMemberRecord {
    pub(crate) owner_identity_id: String,
    pub(crate) owner_did: String,
    pub(crate) group_id: String,
    pub(crate) user_id: String,
    pub(crate) member_did: String,
    pub(crate) member_handle: String,
    pub(crate) profile_url: String,
    pub(crate) role: String,
    pub(crate) status: String,
    pub(crate) joined_at: String,
    pub(crate) sent_message_count: Option<i64>,
    pub(crate) last_synced_at: String,
    pub(crate) metadata: String,
    pub(crate) credential_name: String,
}

#[cfg(feature = "sqlite")]
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct GroupE2eeSummaryRecord {
    pub(crate) record: GroupRecord,
    pub(crate) epoch: Option<String>,
    pub(crate) group_state_version: Option<String>,
}

#[cfg(feature = "sqlite")]
pub(crate) fn upsert_group(
    connection: &rusqlite::Connection,
    record: GroupRecord,
) -> crate::ImResult<()> {
    let owner_did = normalize(&record.owner_did);
    let owner_identity_id = required_owner_identity_id(&record.owner_identity_id)?;
    let group_id = normalize(&record.group_id);
    if owner_did.is_empty() || group_id.is_empty() {
        return Err(crate::ImError::invalid_input(
            None,
            "owner_did and group_id are required",
        ));
    }
    let stored_at = default_string(record.stored_at.clone(), &now_utc());
    connection
        .execute(
            r#"
INSERT INTO groups
    (owner_identity_id, owner_did, group_id, group_did, name, group_mode, slug, description, goal, rules, message_prompt,
     doc_url, group_owner_did, group_owner_handle, my_role, membership_status, join_enabled, join_code,
     join_code_expires_at, member_count, last_synced_seq, last_read_seq, last_message_at,
     remote_created_at, remote_updated_at, stored_at, metadata, credential_name)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28)
ON CONFLICT(owner_identity_id, group_id)
DO UPDATE SET
    owner_did = excluded.owner_did,
    group_did = COALESCE(excluded.group_did, groups.group_did),
    name = COALESCE(excluded.name, groups.name),
    group_mode = excluded.group_mode,
    slug = COALESCE(excluded.slug, groups.slug),
    description = COALESCE(excluded.description, groups.description),
    goal = COALESCE(excluded.goal, groups.goal),
    rules = COALESCE(excluded.rules, groups.rules),
    message_prompt = COALESCE(excluded.message_prompt, groups.message_prompt),
    doc_url = COALESCE(excluded.doc_url, groups.doc_url),
    group_owner_did = COALESCE(excluded.group_owner_did, groups.group_owner_did),
    group_owner_handle = COALESCE(excluded.group_owner_handle, groups.group_owner_handle),
    my_role = COALESCE(excluded.my_role, groups.my_role),
    membership_status = COALESCE(excluded.membership_status, groups.membership_status),
    join_enabled = COALESCE(excluded.join_enabled, groups.join_enabled),
    join_code = COALESCE(excluded.join_code, groups.join_code),
    join_code_expires_at = COALESCE(excluded.join_code_expires_at, groups.join_code_expires_at),
    member_count = COALESCE(excluded.member_count, groups.member_count),
    last_synced_seq = COALESCE(excluded.last_synced_seq, groups.last_synced_seq),
    last_read_seq = COALESCE(excluded.last_read_seq, groups.last_read_seq),
    last_message_at = COALESCE(excluded.last_message_at, groups.last_message_at),
    remote_created_at = COALESCE(excluded.remote_created_at, groups.remote_created_at),
    remote_updated_at = COALESCE(excluded.remote_updated_at, groups.remote_updated_at),
    stored_at = excluded.stored_at,
    metadata = COALESCE(excluded.metadata, groups.metadata),
    credential_name = COALESCE(excluded.credential_name, groups.credential_name)"#,
            rusqlite::params![
                owner_identity_id,
                owner_did,
                group_id,
                optional_string(&record.group_did),
                optional_string(&record.name),
                default_string(record.group_mode.clone(), "general"),
                optional_string(&record.slug),
                optional_string(&record.description),
                optional_string(&record.goal),
                optional_string(&record.rules),
                optional_string(&record.message_prompt),
                optional_string(&record.doc_url),
                optional_string(&record.group_owner_did),
                optional_string(&record.group_owner_handle),
                optional_string(&record.my_role),
                default_string(record.membership_status.clone(), "active"),
                optional_bool(record.join_enabled),
                optional_string(&record.join_code),
                optional_string(&record.join_code_expires_at),
                record.member_count,
                record.last_synced_seq,
                record.last_read_seq,
                optional_string(&record.last_message_at),
                optional_string(&record.remote_created_at),
                optional_string(&record.remote_updated_at),
                stored_at,
                optional_string(&record.metadata),
                normalize(&record.credential_name),
            ],
        )
        .map_err(super::local_state_unavailable)?;
    Ok(())
}

#[cfg(feature = "sqlite")]
pub(crate) fn upsert_group_e2ee_summary(
    connection: &rusqlite::Connection,
    summary: GroupE2eeSummaryRecord,
) -> crate::ImResult<()> {
    let owner_did = normalize(&summary.record.owner_did);
    let owner_identity_id = required_owner_identity_id(&summary.record.owner_identity_id)?;
    let group_id = normalize(&summary.record.group_id);
    if owner_did.is_empty() || group_id.is_empty() {
        return Err(crate::ImError::invalid_input(
            None,
            "owner_did and group_id are required",
        ));
    }
    if let Some(existing) = get_group_snapshot_for_owner_identity(
        connection,
        &owner_identity_id,
        &owner_did,
        &group_id,
    )? {
        if !should_apply_group_e2ee_summary(
            existing.get("metadata"),
            summary.epoch.as_deref(),
            summary.group_state_version.as_deref(),
        ) {
            return Ok(());
        }
    }
    upsert_group(connection, summary.record)
}

#[cfg(feature = "sqlite")]
pub(crate) fn replace_group_members(
    connection: &mut rusqlite::Connection,
    owner_identity_id: &str,
    owner_did: &str,
    group_id: &str,
    members: &[GroupMemberRecord],
    credential_name: &str,
) -> crate::ImResult<()> {
    let owner_did = normalize(owner_did);
    let owner_identity_id = required_owner_identity_id(owner_identity_id)?;
    let group_id = normalize(group_id);
    if owner_did.is_empty() || group_id.is_empty() {
        return Err(crate::ImError::invalid_input(
            None,
            "owner_did and group_id are required",
        ));
    }
    let transaction = connection
        .transaction()
        .map_err(super::local_state_unavailable)?;
    transaction
        .execute(
            "DELETE FROM group_members WHERE owner_identity_id = ?1 AND group_id = ?2",
            rusqlite::params![owner_identity_id.as_str(), group_id.as_str()],
        )
        .map_err(super::local_state_unavailable)?;
    let now = now_utc();
    {
        let mut statement = transaction
            .prepare(
                r#"
INSERT INTO group_members
    (owner_identity_id, owner_did, group_id, user_id, member_did, member_handle, profile_url, role, status,
     joined_at, sent_message_count, last_synced_at, metadata, credential_name)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)"#,
            )
            .map_err(super::local_state_unavailable)?;
        for member in members {
            let user_id = normalize(&member.user_id);
            if user_id.is_empty() {
                continue;
            }
            let last_synced_at = default_string(member.last_synced_at.clone(), &now);
            statement
                .execute(rusqlite::params![
                    owner_identity_id.as_str(),
                    owner_did.as_str(),
                    group_id.as_str(),
                    user_id,
                    optional_string(&member.member_did),
                    optional_string(&member.member_handle),
                    optional_string(&member.profile_url),
                    optional_string(&member.role),
                    default_string(member.status.clone(), "active"),
                    optional_string(&member.joined_at),
                    member.sent_message_count.or(Some(0)),
                    last_synced_at,
                    optional_string(&member.metadata),
                    normalize(&default_string(
                        member.credential_name.clone(),
                        credential_name,
                    )),
                ])
                .map_err(super::local_state_unavailable)?;
        }
    }
    transaction
        .commit()
        .map_err(super::local_state_unavailable)?;
    Ok(())
}

#[cfg(feature = "sqlite")]
pub(crate) fn mark_group_left(
    connection: &mut rusqlite::Connection,
    owner_identity_id: &str,
    owner_did: &str,
    group_id: &str,
    group_did: &str,
    credential_name: &str,
) -> crate::ImResult<()> {
    let owner_did = normalize(owner_did);
    let group_id = normalize(group_id);
    if owner_did.is_empty() || group_id.is_empty() {
        return Err(crate::ImError::invalid_input(
            None,
            "owner_did and group_id are required",
        ));
    }
    let now = now_utc();
    let credential_name = normalize(credential_name);
    let owner_identity_id = required_owner_identity_id(owner_identity_id)?;
    let transaction = connection
        .transaction()
        .map_err(super::local_state_unavailable)?;
    transaction
        .execute(
            r#"
INSERT INTO groups
    (owner_identity_id, owner_did, group_id, group_did, group_mode, my_role, membership_status, stored_at,
     credential_name)
VALUES (?1, ?2, ?3, ?4, 'general', NULL, 'left', ?5, ?6)
ON CONFLICT(owner_identity_id, group_id)
DO UPDATE SET
    owner_did = excluded.owner_did,
    group_did = COALESCE(excluded.group_did, groups.group_did),
    my_role = NULL,
    membership_status = 'left',
    stored_at = excluded.stored_at,
    credential_name = COALESCE(excluded.credential_name, groups.credential_name)"#,
            rusqlite::params![
                owner_identity_id.as_str(),
                owner_did.as_str(),
                group_id.as_str(),
                optional_string(group_did),
                now.as_str(),
                credential_name,
            ],
        )
        .map_err(super::local_state_unavailable)?;
    transaction
        .execute(
            "DELETE FROM group_members WHERE owner_identity_id = ?1 AND group_id = ?2",
            rusqlite::params![owner_identity_id.as_str(), group_id.as_str()],
        )
        .map_err(super::local_state_unavailable)?;
    transaction
        .commit()
        .map_err(super::local_state_unavailable)?;
    Ok(())
}

#[cfg(feature = "sqlite")]
pub(crate) fn get_group_snapshot_for_owner_identity(
    connection: &rusqlite::Connection,
    owner_identity_id: &str,
    _owner_did: &str,
    group_id: &str,
) -> crate::ImResult<Option<serde_json::Value>> {
    let group_id = group_id.trim();
    if group_id.is_empty() {
        return Err(crate::ImError::invalid_input(
            Some("group_id".to_string()),
            "group_id is required",
        ));
    }
    let statement = format!(
        r#"
SELECT *
FROM groups
WHERE {} AND (group_id = ?2 OR group_did = ?2)
LIMIT 1"#,
        owner_predicate("groups")
    );
    let mut statement = connection
        .prepare(&statement)
        .map_err(super::local_state_unavailable)?;
    let names = column_names(&statement);
    let mut rows = statement
        .query(rusqlite::params![
            required_owner_identity_id(owner_identity_id)?,
            group_id,
        ])
        .map_err(super::local_state_unavailable)?;
    let Some(row) = rows.next().map_err(super::local_state_unavailable)? else {
        return Ok(None);
    };
    row_to_json(row, &names).map(Some)
}

#[cfg(feature = "sqlite")]
pub(crate) fn list_cached_group_members_for_owner_identity(
    connection: &rusqlite::Connection,
    owner_identity_id: &str,
    _owner_did: &str,
    group_id: &str,
    limit: i64,
) -> crate::ImResult<Vec<serde_json::Value>> {
    let group_id = group_id.trim();
    if group_id.is_empty() {
        return Err(crate::ImError::invalid_input(
            Some("group_id".to_string()),
            "group_id is required",
        ));
    }
    let limit = if limit <= 0 { 100 } else { limit };
    let statement = format!(
        r#"
SELECT *
FROM group_members
WHERE {} AND group_id IN (
    SELECT ?2
    UNION
    SELECT group_id
    FROM groups
    WHERE {} AND (group_id = ?2 OR group_did = ?2)
)
ORDER BY role ASC, member_handle ASC, member_did ASC
LIMIT ?3"#,
        owner_predicate("group_members"),
        owner_predicate("groups")
    );
    query_rows(
        connection,
        &statement,
        &[
            &required_owner_identity_id(owner_identity_id)?,
            &group_id,
            &limit,
        ],
    )
}

#[cfg(feature = "sqlite")]
pub(crate) fn list_group_messages_for_owner_identity(
    connection: &rusqlite::Connection,
    owner_identity_id: &str,
    _owner_did: &str,
    group_id: &str,
    limit: i64,
    since_seq: Option<i64>,
) -> crate::ImResult<Vec<serde_json::Value>> {
    let group_id = group_id.trim();
    if group_id.is_empty() {
        return Err(crate::ImError::invalid_input(
            Some("group_id".to_string()),
            "group_id is required",
        ));
    }
    let limit = if limit <= 0 { 50 } else { limit };
    if let Some(since_seq) = since_seq {
        let statement = format!(
            r#"
SELECT *
FROM messages
WHERE {} AND (group_did = ?2 OR group_id = ?2)
  AND COALESCE(server_seq, 0) > ?3
ORDER BY COALESCE(server_seq, 0) DESC, COALESCE(sent_at, stored_at) DESC
LIMIT ?4"#,
            owner_predicate("messages")
        );
        return query_rows(
            connection,
            &statement,
            &[
                &required_owner_identity_id(owner_identity_id)?,
                &group_id,
                &since_seq,
                &limit,
            ],
        );
    }
    let statement = format!(
        r#"
SELECT *
FROM messages
WHERE {} AND (group_did = ?2 OR group_id = ?2)
ORDER BY COALESCE(server_seq, 0) DESC, COALESCE(sent_at, stored_at) DESC
LIMIT ?3"#,
        owner_predicate("messages")
    );
    query_rows(
        connection,
        &statement,
        &[
            &required_owner_identity_id(owner_identity_id)?,
            &group_id,
            &limit,
        ],
    )
}

#[cfg(feature = "sqlite")]
pub(crate) fn list_active_group_refs_for_owner_identity(
    connection: &rusqlite::Connection,
    owner_identity_id: &str,
    _owner_did: &str,
    limit: i64,
) -> crate::ImResult<Vec<String>> {
    let limit = if limit <= 0 { 50 } else { limit };
    let statement = format!(
        r#"
SELECT COALESCE(NULLIF(TRIM(group_did), ''), group_id) AS group_ref
FROM groups
WHERE {}
  AND TRIM(COALESCE(group_id, '')) <> ''
  AND COALESCE(NULLIF(TRIM(membership_status), ''), 'active') NOT IN ('left', 'removed', 'inactive', 'non_member')
ORDER BY
  CASE WHEN TRIM(COALESCE(last_message_at, '')) = '' THEN 1 ELSE 0 END,
  COALESCE(last_message_at, stored_at) DESC,
  stored_at DESC,
  group_id ASC
LIMIT ?2"#,
        owner_predicate("groups")
    );
    let mut statement = connection
        .prepare(&statement)
        .map_err(super::local_state_unavailable)?;
    let rows = statement
        .query_map(
            rusqlite::params![required_owner_identity_id(owner_identity_id)?, limit],
            |row| row.get::<_, Option<String>>("group_ref"),
        )
        .map_err(super::local_state_unavailable)?;
    let mut result = Vec::new();
    for row in rows {
        let group_ref = row
            .map_err(super::local_state_unavailable)?
            .unwrap_or_default();
        if !group_ref.trim().is_empty() {
            result.push(group_ref);
        }
    }
    Ok(result)
}

#[cfg(feature = "sqlite")]
fn query_rows(
    connection: &rusqlite::Connection,
    statement: &str,
    params: &[&dyn rusqlite::ToSql],
) -> crate::ImResult<Vec<serde_json::Value>> {
    let mut statement = connection
        .prepare(statement)
        .map_err(super::local_state_unavailable)?;
    let names = column_names(&statement);
    let mut rows = statement
        .query(params)
        .map_err(super::local_state_unavailable)?;
    let mut result = Vec::new();
    while let Some(row) = rows.next().map_err(super::local_state_unavailable)? {
        result.push(row_to_json(row, &names)?);
    }
    Ok(result)
}

#[cfg(feature = "sqlite")]
fn column_names(statement: &rusqlite::Statement<'_>) -> Vec<String> {
    statement
        .column_names()
        .into_iter()
        .map(ToOwned::to_owned)
        .collect()
}

#[cfg(feature = "sqlite")]
fn row_to_json(row: &rusqlite::Row<'_>, names: &[String]) -> crate::ImResult<serde_json::Value> {
    let mut object = serde_json::Map::new();
    for (index, name) in names.iter().enumerate() {
        object.insert(
            name.clone(),
            value_ref_to_json(row.get_ref(index).map_err(super::local_state_unavailable)?),
        );
    }
    Ok(serde_json::Value::Object(object))
}

#[cfg(feature = "sqlite")]
fn value_ref_to_json(value: rusqlite::types::ValueRef<'_>) -> serde_json::Value {
    match value {
        rusqlite::types::ValueRef::Null => serde_json::Value::Null,
        rusqlite::types::ValueRef::Integer(value) => serde_json::json!(value),
        rusqlite::types::ValueRef::Real(value) => serde_json::Number::from_f64(value)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
        rusqlite::types::ValueRef::Text(value) => {
            serde_json::Value::String(String::from_utf8_lossy(value).into_owned())
        }
        rusqlite::types::ValueRef::Blob(value) => {
            serde_json::Value::String(String::from_utf8_lossy(value).into_owned())
        }
    }
}

#[cfg(feature = "sqlite")]
fn owner_predicate(alias: &str) -> String {
    format!("{alias}.owner_identity_id = ?1")
}

#[cfg(feature = "sqlite")]
fn normalize(value: &str) -> String {
    value.trim().to_string()
}

#[cfg(feature = "sqlite")]
fn normalize_owner_identity_id(value: &str) -> String {
    value.trim().to_string()
}

#[cfg(feature = "sqlite")]
fn required_owner_identity_id(value: &str) -> crate::ImResult<String> {
    let value = normalize_owner_identity_id(value);
    if value.is_empty() {
        return Err(crate::ImError::invalid_input(
            Some("owner_identity_id".to_owned()),
            "owner_identity_id is required",
        ));
    }
    Ok(value)
}

#[cfg(feature = "sqlite")]
fn default_string(value: String, fallback: &str) -> String {
    if value.trim().is_empty() {
        fallback.to_string()
    } else {
        value
    }
}

#[cfg(feature = "sqlite")]
fn optional_string(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

#[cfg(feature = "sqlite")]
fn optional_bool(value: Option<bool>) -> Option<i64> {
    value.map(i64::from)
}

#[cfg(feature = "sqlite")]
fn now_utc() -> String {
    let value = OffsetDateTime::now_utc();
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        value.year(),
        u8::from(value.month()),
        value.day(),
        value.hour(),
        value.minute(),
        value.second()
    )
}

#[cfg(feature = "sqlite")]
fn should_apply_group_e2ee_summary(
    existing_metadata: Option<&serde_json::Value>,
    next_epoch: Option<&str>,
    next_group_state_version: Option<&str>,
) -> bool {
    let Some(existing_metadata) = existing_metadata.and_then(decode_metadata_value) else {
        return true;
    };
    let existing_epoch = metadata_string(&existing_metadata, &["group_e2ee", "epoch"])
        .as_deref()
        .and_then(parse_u64);
    let next_epoch = next_epoch.and_then(parse_u64);
    match (existing_epoch, next_epoch) {
        (Some(existing), Some(next)) if next < existing => return false,
        (Some(existing), Some(next)) if next > existing => return true,
        _ => {}
    }

    let existing_version = metadata_string(&existing_metadata, &["group_state_version"])
        .or_else(|| metadata_string(&existing_metadata, &["group_e2ee", "group_state_version"]));
    let existing_version = existing_version.as_deref();
    match compare_group_state_versions(next_group_state_version, existing_version) {
        Some(std::cmp::Ordering::Less) => false,
        Some(_) => true,
        None => existing_version.is_none(),
    }
}

#[cfg(feature = "sqlite")]
fn decode_metadata_value(value: &serde_json::Value) -> Option<serde_json::Value> {
    match value {
        serde_json::Value::Object(_) => Some(value.clone()),
        serde_json::Value::String(raw) => serde_json::from_str(raw).ok(),
        _ => None,
    }
}

#[cfg(feature = "sqlite")]
fn metadata_string(value: &serde_json::Value, path: &[&str]) -> Option<String> {
    let mut current = value;
    for key in path {
        current = current.get(*key)?;
    }
    current
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

#[cfg(feature = "sqlite")]
fn compare_group_state_versions(
    next: Option<&str>,
    existing: Option<&str>,
) -> Option<std::cmp::Ordering> {
    let next = next.map(str::trim).filter(|value| !value.is_empty())?;
    let existing = existing.map(str::trim).filter(|value| !value.is_empty())?;
    match (parse_version_ordinal(next), parse_version_ordinal(existing)) {
        (Some(next), Some(existing)) => Some(next.cmp(&existing)),
        _ if next == existing => Some(std::cmp::Ordering::Equal),
        _ => None,
    }
}

#[cfg(feature = "sqlite")]
fn parse_version_ordinal(value: &str) -> Option<u64> {
    parse_u64(value).or_else(|| {
        let digits: String = value
            .chars()
            .rev()
            .take_while(|ch| ch.is_ascii_digit())
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        if digits.is_empty() {
            None
        } else {
            digits.parse().ok()
        }
    })
}

#[cfg(feature = "sqlite")]
fn parse_u64(value: &str) -> Option<u64> {
    value.trim().parse().ok()
}

#[cfg(all(test, feature = "sqlite"))]
mod tests {
    use super::*;
    use rusqlite::Connection;

    #[test]
    fn local_state_groups_read_cache_with_owner_identity_scope() {
        let db = Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        db.execute(
            r#"
INSERT INTO groups
    (owner_identity_id, owner_did, group_id, group_did, name, stored_at, credential_name)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)"#,
            (
                "alice-identity",
                "did:owner",
                "group-key",
                "did:group",
                "Demo Group",
                "2026-05-21T00:00:00Z",
                "alice",
            ),
        )
        .unwrap();
        db.execute(
            r#"
INSERT INTO group_members
    (owner_identity_id, owner_did, group_id, user_id, member_did, member_handle, role, status,
     joined_at, sent_message_count, last_synced_at, credential_name)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 0, ?10, ?11)"#,
            (
                "alice-identity",
                "did:owner",
                "group-key",
                "did:member",
                "did:member",
                "member.awiki.ai",
                "member",
                "active",
                "2026-05-21T00:00:00Z",
                "2026-05-21T00:00:00Z",
                "alice",
            ),
        )
        .unwrap();
        db.execute(
            r#"
INSERT INTO messages
    (msg_id, owner_identity_id, owner_did, thread_id, direction, sender_did, group_id, group_did,
     content_type, content, server_seq, sent_at, stored_at, credential_name)
VALUES (?1, ?2, ?3, ?4, 0, ?5, ?6, ?7, 'text/plain', 'hello', 7, ?8, ?8, ?9)"#,
            (
                "msg-group-1",
                "alice-identity",
                "did:owner",
                "thread:group-key",
                "did:member",
                "group-key",
                "did:group",
                "2026-05-21T00:00:00Z",
                "alice",
            ),
        )
        .unwrap();

        let snapshot =
            get_group_snapshot_for_owner_identity(&db, "alice-identity", "did:owner", "did:group")
                .unwrap()
                .unwrap();
        assert_eq!(snapshot["name"], "Demo Group");

        let members = list_cached_group_members_for_owner_identity(
            &db,
            "alice-identity",
            "did:owner",
            "did:group",
            10,
        )
        .unwrap();
        assert_eq!(members.len(), 1);
        assert_eq!(members[0]["member_did"], "did:member");

        let messages = list_group_messages_for_owner_identity(
            &db,
            "alice-identity",
            "did:owner",
            "did:group",
            10,
            Some(1),
        )
        .unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["msg_id"], "msg-group-1");
    }

    #[test]
    fn local_state_groups_lists_active_group_refs_for_recovery() {
        let db = Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        db.execute(
            r#"
INSERT INTO groups
    (owner_identity_id, owner_did, group_id, group_did, name, membership_status, last_message_at, stored_at, credential_name)
VALUES
    ('alice-id', 'did:owner', 'group-a', 'did:group:a', 'A', 'active', '2026-05-21T00:00:02Z', '2026-05-21T00:00:02Z', 'alice'),
    ('alice-id', 'did:owner', 'group-b', '', 'B', 'active', '2026-05-21T00:00:03Z', '2026-05-21T00:00:03Z', 'alice'),
    ('alice-id', 'did:owner', 'group-left', 'did:group:left', 'Left', 'left', '2026-05-21T00:00:04Z', '2026-05-21T00:00:04Z', 'alice'),
    ('other-id', 'did:other', 'group-other', 'did:group:other', 'Other', 'active', '2026-05-21T00:00:05Z', '2026-05-21T00:00:05Z', 'other')"#,
            [],
        )
        .unwrap();

        let refs =
            list_active_group_refs_for_owner_identity(&db, "alice-id", "did:owner", 10).unwrap();

        assert_eq!(refs, vec!["group-b", "did:group:a"]);
    }

    #[test]
    fn group_e2ee_summary_upsert_does_not_revert_to_older_epoch() {
        let db = Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        upsert_group_e2ee_summary(
            &db,
            summary_record("did:group:e2ee", Some("5"), Some("state-5"), "newer-crypto"),
        )
        .unwrap();

        upsert_group_e2ee_summary(
            &db,
            summary_record("did:group:e2ee", Some("4"), Some("state-4"), "older-crypto"),
        )
        .unwrap();

        let snapshot =
            get_group_snapshot_for_owner_identity(&db, "alice-id", "did:owner", "did:group:e2ee")
                .unwrap()
                .unwrap();
        let metadata: serde_json::Value =
            serde_json::from_str(snapshot["metadata"].as_str().unwrap()).unwrap();
        assert_eq!(metadata["group_e2ee"]["epoch"], "5");
        assert_eq!(metadata["group_e2ee"]["group_state_version"], "state-5");
        assert_eq!(
            metadata["group_e2ee"]["crypto_group_id_b64u"],
            "newer-crypto"
        );
    }

    #[test]
    fn group_e2ee_summary_upsert_uses_version_when_epoch_is_missing() {
        let db = Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        upsert_group_e2ee_summary(
            &db,
            summary_record("did:group:e2ee", None, Some("state-11"), "newer-crypto"),
        )
        .unwrap();

        upsert_group_e2ee_summary(
            &db,
            summary_record("did:group:e2ee", None, Some("state-10"), "older-crypto"),
        )
        .unwrap();

        let snapshot =
            get_group_snapshot_for_owner_identity(&db, "alice-id", "did:owner", "did:group:e2ee")
                .unwrap()
                .unwrap();
        let metadata: serde_json::Value =
            serde_json::from_str(snapshot["metadata"].as_str().unwrap()).unwrap();
        assert_eq!(metadata["group_e2ee"]["group_state_version"], "state-11");
        assert_eq!(
            metadata["group_e2ee"]["crypto_group_id_b64u"],
            "newer-crypto"
        );
    }

    #[test]
    fn group_e2ee_summary_upsert_rejects_missing_version_over_existing_version() {
        let db = Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        upsert_group_e2ee_summary(
            &db,
            summary_record("did:group:e2ee", None, Some("state-11"), "newer-crypto"),
        )
        .unwrap();

        upsert_group_e2ee_summary(
            &db,
            summary_record("did:group:e2ee", None, None, "unknown-crypto"),
        )
        .unwrap();

        let snapshot =
            get_group_snapshot_for_owner_identity(&db, "alice-id", "did:owner", "did:group:e2ee")
                .unwrap()
                .unwrap();
        let metadata: serde_json::Value =
            serde_json::from_str(snapshot["metadata"].as_str().unwrap()).unwrap();
        assert_eq!(metadata["group_e2ee"]["group_state_version"], "state-11");
        assert_eq!(
            metadata["group_e2ee"]["crypto_group_id_b64u"],
            "newer-crypto"
        );
    }

    #[test]
    fn group_e2ee_summary_upsert_accepts_missing_version_for_empty_cache() {
        let db = Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();

        upsert_group_e2ee_summary(
            &db,
            summary_record("did:group:e2ee", None, None, "first-crypto"),
        )
        .unwrap();

        let snapshot =
            get_group_snapshot_for_owner_identity(&db, "alice-id", "did:owner", "did:group:e2ee")
                .unwrap()
                .unwrap();
        let metadata: serde_json::Value =
            serde_json::from_str(snapshot["metadata"].as_str().unwrap()).unwrap();
        assert_eq!(
            metadata["group_e2ee"]["crypto_group_id_b64u"],
            "first-crypto"
        );
    }

    fn summary_record(
        group_did: &str,
        epoch: Option<&str>,
        group_state_version: Option<&str>,
        crypto_group_id_b64u: &str,
    ) -> GroupE2eeSummaryRecord {
        let mut group_e2ee = serde_json::Map::new();
        if let Some(epoch) = epoch {
            group_e2ee.insert(
                "epoch".to_owned(),
                serde_json::Value::String(epoch.to_owned()),
            );
        }
        if let Some(group_state_version) = group_state_version {
            group_e2ee.insert(
                "group_state_version".to_owned(),
                serde_json::Value::String(group_state_version.to_owned()),
            );
        }
        group_e2ee.insert(
            "crypto_group_id_b64u".to_owned(),
            serde_json::Value::String(crypto_group_id_b64u.to_owned()),
        );
        let mut metadata = serde_json::Map::new();
        metadata.insert(
            "message_security_profile".to_owned(),
            serde_json::Value::String("group-e2ee".to_owned()),
        );
        metadata.insert(
            "group_e2ee".to_owned(),
            serde_json::Value::Object(group_e2ee),
        );
        if let Some(group_state_version) = group_state_version {
            metadata.insert(
                "group_state_version".to_owned(),
                serde_json::Value::String(group_state_version.to_owned()),
            );
        }
        GroupE2eeSummaryRecord {
            record: GroupRecord {
                owner_identity_id: "alice-id".to_owned(),
                owner_did: "did:owner".to_owned(),
                group_id: group_did.to_owned(),
                group_did: group_did.to_owned(),
                membership_status: "active".to_owned(),
                metadata: serde_json::Value::Object(metadata).to_string(),
                credential_name: "alice-id".to_owned(),
                ..GroupRecord::default()
            },
            epoch: epoch.map(ToOwned::to_owned),
            group_state_version: group_state_version.map(ToOwned::to_owned),
        }
    }
}
