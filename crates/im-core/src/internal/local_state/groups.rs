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
pub(crate) fn get_group_snapshot_for_owner_identity(
    connection: &rusqlite::Connection,
    owner_identity_id: &str,
    owner_did: &str,
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
WHERE {} AND (group_id = ?3 OR group_did = ?3)
LIMIT 1"#,
        owner_predicate("groups")
    );
    let mut statement = connection
        .prepare(&statement)
        .map_err(super::local_state_unavailable)?;
    let names = column_names(&statement);
    let mut rows = statement
        .query(rusqlite::params![
            normalize(owner_identity_id),
            normalize(owner_did),
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
    owner_did: &str,
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
    SELECT ?3
    UNION
    SELECT group_id
    FROM groups
    WHERE {} AND (group_id = ?3 OR group_did = ?3)
)
ORDER BY role ASC, member_handle ASC, member_did ASC
LIMIT ?4"#,
        owner_predicate("group_members"),
        owner_predicate("groups")
    );
    query_rows(
        connection,
        &statement,
        &[
            &normalize(owner_identity_id),
            &normalize(owner_did),
            &group_id,
            &limit,
        ],
    )
}

#[cfg(feature = "sqlite")]
pub(crate) fn list_group_messages_for_owner_identity(
    connection: &rusqlite::Connection,
    owner_identity_id: &str,
    owner_did: &str,
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
WHERE {} AND (group_did = ?3 OR group_id = ?3)
  AND COALESCE(server_seq, 0) > ?4
ORDER BY COALESCE(server_seq, 0) DESC, COALESCE(sent_at, stored_at) DESC
LIMIT ?5"#,
            owner_predicate("messages")
        );
        return query_rows(
            connection,
            &statement,
            &[
                &normalize(owner_identity_id),
                &normalize(owner_did),
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
WHERE {} AND (group_did = ?3 OR group_id = ?3)
ORDER BY COALESCE(server_seq, 0) DESC, COALESCE(sent_at, stored_at) DESC
LIMIT ?4"#,
        owner_predicate("messages")
    );
    query_rows(
        connection,
        &statement,
        &[
            &normalize(owner_identity_id),
            &normalize(owner_did),
            &group_id,
            &limit,
        ],
    )
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
    format!(
        "({alias}.owner_identity_id = ?1 OR (({alias}.owner_identity_id IS NULL OR TRIM({alias}.owner_identity_id) = '') AND {alias}.owner_did = ?2))"
    )
}

#[cfg(feature = "sqlite")]
fn normalize(value: &str) -> String {
    value.trim().to_string()
}

#[cfg(all(test, feature = "sqlite"))]
mod tests {
    use super::*;
    use rusqlite::Connection;

    #[test]
    fn local_state_groups_read_cache_with_owner_identity_fallback() {
        let db = Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        db.execute(
            r#"
INSERT INTO groups
    (owner_identity_id, owner_did, group_id, group_did, name, stored_at, credential_name)
VALUES (NULL, ?1, ?2, ?3, ?4, ?5, ?6)"#,
            (
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
VALUES (NULL, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 0, ?9, ?10)"#,
            (
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
VALUES (?1, NULL, ?2, ?3, 0, ?4, ?5, ?6, 'text/plain', 'hello', 7, ?7, ?7, ?8)"#,
            (
                "msg-group-1",
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
}
