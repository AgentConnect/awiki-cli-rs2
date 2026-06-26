#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct MessageRecord {
    pub(crate) msg_id: String,
    pub(crate) owner_identity_id: String,
    pub(crate) owner_did: String,
    pub(crate) conversation_id: String,
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
    pub(crate) mentions_current_user: bool,
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
    let conversation_id = required("conversation_id", &stable_conversation_id(record))?;
    let thread_id = conversation_id.clone();
    let stored_at = default_string(&record.stored_at, &now_utc_like());
    let mentions_current_user = mentions_current_user_for_projection(
        &owner_did,
        record.direction,
        &record.thread_id,
        &record.group_id,
        &record.group_did,
        &record.content_type,
        &record.content,
    );
    connection
        .execute(
            r#"
INSERT INTO messages
    (msg_id, owner_identity_id, owner_did, conversation_id, thread_id, direction, sender_did, receiver_did,
     group_id, group_did, content_type, content, title, server_seq, sent_at, stored_at,
     is_e2ee, is_read, sender_name, metadata, mentions_current_user, credential_name)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22)
ON CONFLICT(owner_identity_id, msg_id) DO UPDATE SET
    owner_did = excluded.owner_did,
    conversation_id = excluded.conversation_id,
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
    mentions_current_user = excluded.mentions_current_user,
    credential_name = excluded.credential_name"#,
            rusqlite::params![
                msg_id,
                owner_identity_id,
                owner_did,
                conversation_id,
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
                mentions_current_user,
                record.credential_name.trim(),
            ],
        )
        .map_err(super::local_state_unavailable)?;
    merge_legacy_direct_did_conversation(connection, &owner_identity_id, &conversation_id, record)?;
    Ok(())
}

#[cfg(feature = "sqlite")]
pub(crate) fn mentions_current_user_for_projection(
    owner_did: &str,
    direction: i64,
    thread_id: &str,
    group_id: &str,
    group_did: &str,
    content_type: &str,
    content: &str,
) -> bool {
    if direction != 0 || !is_group_projection(thread_id, group_id, group_did) {
        return false;
    }
    let content_type = content_type.trim();
    if content_type != "application/json"
        && content_type != crate::attachments::attachment_manifest_content_type()
    {
        return false;
    }
    let owner_did = owner_did.trim();
    if owner_did.is_empty() {
        return false;
    }
    let Ok(value) = serde_json::from_str::<serde_json::Value>(content) else {
        return false;
    };
    let Ok(payload) = crate::messages::parse_message_mention_payload(&value) else {
        return false;
    };
    payload
        .mentions
        .iter()
        .any(|mention| match &mention.target {
            crate::messages::MessageMentionTarget::Human { did, .. } => did.trim() == owner_did,
            crate::messages::MessageMentionTarget::GroupSelector { selector } => matches!(
                selector,
                crate::messages::MessageMentionSelector::All
                    | crate::messages::MessageMentionSelector::Humans
            ),
            crate::messages::MessageMentionTarget::Agent { .. } => false,
        })
}

#[cfg(feature = "sqlite")]
fn is_group_projection(thread_id: &str, group_id: &str, group_did: &str) -> bool {
    !group_id.trim().is_empty()
        || !group_did.trim().is_empty()
        || thread_id.trim().starts_with("group:")
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
pub(crate) fn list_direct_messages_for_owner_identity(
    connection: &rusqlite::Connection,
    owner_identity_id: &str,
    conversation_ids: &[String],
    limit: i64,
) -> crate::ImResult<Vec<MessageRecord>> {
    let owner_identity_id = required("owner_identity_id", owner_identity_id)?;
    let conversation_ids = normalized_conversation_ids(conversation_ids);
    if conversation_ids.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders = vec!["?"; conversation_ids.len()].join(",");
    let statement = format!(
        r#"
SELECT msg_id,
       owner_identity_id,
       owner_did,
       conversation_id,
       thread_id,
       direction,
       sender_did,
       receiver_did,
       group_id,
       group_did,
       content_type,
       content,
       title,
       server_seq,
       sent_at,
       stored_at,
       is_e2ee,
       is_read,
       sender_name,
       metadata,
       mentions_current_user,
       credential_name
FROM messages
WHERE owner_identity_id = ?
  AND NULLIF(TRIM(COALESCE(group_id, '')), '') IS NULL
  AND NULLIF(TRIM(COALESCE(group_did, '')), '') IS NULL
  AND COALESCE(NULLIF(conversation_id, ''), thread_id) IN ({placeholders})
ORDER BY COALESCE(NULLIF(sent_at, ''), stored_at) DESC, msg_id DESC
LIMIT ?"#
    );
    let mut params: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(conversation_ids.len() + 2);
    params.push(&owner_identity_id);
    for conversation_id in &conversation_ids {
        params.push(conversation_id);
    }
    params.push(&limit);

    let mut statement = connection
        .prepare(&statement)
        .map_err(super::local_state_unavailable)?;
    let rows = statement
        .query_map(params.as_slice(), message_record_from_row)
        .map_err(super::local_state_unavailable)?;
    let mut result = Vec::new();
    for row in rows {
        result.push(row.map_err(super::local_state_unavailable)?);
    }
    Ok(result)
}

#[cfg(feature = "sqlite")]
pub(crate) fn reconcile_peer_scope_direct_conversations(
    connection: &rusqlite::Connection,
    owner_identity_id: &str,
) -> crate::ImResult<()> {
    let owner_identity_id = required("owner_identity_id", owner_identity_id)?;
    for candidate in peer_scope_direct_candidates(connection, &owner_identity_id)? {
        let record = MessageRecord {
            owner_identity_id: owner_identity_id.clone(),
            owner_did: candidate.owner_did,
            conversation_id: candidate.conversation_id.clone(),
            thread_id: candidate.conversation_id.clone(),
            sender_did: candidate.sender_did,
            receiver_did: candidate.receiver_did,
            metadata: candidate.metadata,
            ..MessageRecord::default()
        };
        merge_legacy_direct_did_conversation(
            connection,
            &owner_identity_id,
            &candidate.conversation_id,
            &record,
        )?;
    }
    Ok(())
}

#[cfg(feature = "sqlite")]
fn normalized_conversation_ids(conversation_ids: &[String]) -> Vec<String> {
    let mut ids = Vec::new();
    for conversation_id in conversation_ids {
        push_unique(&mut ids, conversation_id.trim().to_owned());
    }
    ids
}

#[cfg(feature = "sqlite")]
fn message_record_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<MessageRecord> {
    Ok(MessageRecord {
        msg_id: row.get::<_, Option<String>>("msg_id")?.unwrap_or_default(),
        owner_identity_id: row
            .get::<_, Option<String>>("owner_identity_id")?
            .unwrap_or_default(),
        owner_did: row
            .get::<_, Option<String>>("owner_did")?
            .unwrap_or_default(),
        conversation_id: row
            .get::<_, Option<String>>("conversation_id")?
            .unwrap_or_default(),
        thread_id: row
            .get::<_, Option<String>>("thread_id")?
            .unwrap_or_default(),
        direction: row.get::<_, Option<i64>>("direction")?.unwrap_or_default(),
        sender_did: row
            .get::<_, Option<String>>("sender_did")?
            .unwrap_or_default(),
        receiver_did: row
            .get::<_, Option<String>>("receiver_did")?
            .unwrap_or_default(),
        group_id: row
            .get::<_, Option<String>>("group_id")?
            .unwrap_or_default(),
        group_did: row
            .get::<_, Option<String>>("group_did")?
            .unwrap_or_default(),
        content_type: row
            .get::<_, Option<String>>("content_type")?
            .unwrap_or_default(),
        content: row.get::<_, Option<String>>("content")?.unwrap_or_default(),
        title: row.get::<_, Option<String>>("title")?.unwrap_or_default(),
        server_seq: row.get::<_, Option<i64>>("server_seq")?,
        sent_at: row.get::<_, Option<String>>("sent_at")?.unwrap_or_default(),
        stored_at: row
            .get::<_, Option<String>>("stored_at")?
            .unwrap_or_default(),
        is_e2ee: row.get::<_, Option<bool>>("is_e2ee")?.unwrap_or(false),
        is_read: row.get::<_, Option<bool>>("is_read")?.unwrap_or(false),
        sender_name: row
            .get::<_, Option<String>>("sender_name")?
            .unwrap_or_default(),
        metadata: row
            .get::<_, Option<String>>("metadata")?
            .unwrap_or_default(),
        mentions_current_user: row
            .get::<_, Option<bool>>("mentions_current_user")?
            .unwrap_or(false),
        credential_name: row
            .get::<_, Option<String>>("credential_name")?
            .unwrap_or_default(),
    })
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
fn stable_conversation_id(record: &MessageRecord) -> String {
    if let Some(value) = non_empty(record.conversation_id.as_str()) {
        return value.to_owned();
    }
    if let Some(group) =
        non_empty(record.group_id.as_str()).or_else(|| non_empty(&record.group_did))
    {
        return crate::internal::local_state::owner_scope::group_conversation_id(group);
    }
    if is_mail_record(record) {
        if let Some(source) = record
            .thread_id
            .trim()
            .strip_prefix("mail:")
            .filter(|value| !value.trim().is_empty())
        {
            return crate::internal::local_state::owner_scope::mail_conversation_id(source);
        }
        return crate::internal::local_state::owner_scope::mail_conversation_id("inbox");
    }
    let owner_did = record.owner_did.trim();
    let peer = if record.sender_did.trim() != owner_did {
        record.sender_did.trim()
    } else {
        record.receiver_did.trim()
    };
    if !peer.is_empty() {
        return crate::internal::local_state::owner_scope::direct_conversation_id(peer);
    }
    if let Some(conversation_id) =
        crate::internal::local_state::owner_scope::direct_conversation_id_from_thread_alias(
            &record.thread_id,
            &record.owner_did,
        )
    {
        return conversation_id;
    }
    default_string(&record.thread_id, "")
}

#[cfg(feature = "sqlite")]
#[derive(Debug, Clone, PartialEq, Eq)]
struct PeerScopeDirectCandidate {
    conversation_id: String,
    owner_did: String,
    sender_did: String,
    receiver_did: String,
    metadata: String,
}

#[cfg(feature = "sqlite")]
fn peer_scope_direct_candidates(
    connection: &rusqlite::Connection,
    owner_identity_id: &str,
) -> crate::ImResult<Vec<PeerScopeDirectCandidate>> {
    let mut statement = connection
        .prepare(
            r#"
SELECT DISTINCT
    COALESCE(NULLIF(conversation_id, ''), thread_id) AS conversation_id,
    owner_did,
    sender_did,
    receiver_did,
    metadata
FROM messages
WHERE owner_identity_id = ?1
  AND COALESCE(NULLIF(conversation_id, ''), thread_id) LIKE 'dm:peer-scope:%'
  AND NULLIF(TRIM(COALESCE(group_id, '')), '') IS NULL
  AND NULLIF(TRIM(COALESCE(group_did, '')), '') IS NULL
  AND metadata LIKE '%peer_full_handle%'"#,
        )
        .map_err(super::local_state_unavailable)?;
    let rows = statement
        .query_map([owner_identity_id], |row| {
            Ok(PeerScopeDirectCandidate {
                conversation_id: row
                    .get::<_, Option<String>>("conversation_id")?
                    .unwrap_or_default(),
                owner_did: row
                    .get::<_, Option<String>>("owner_did")?
                    .unwrap_or_default(),
                sender_did: row
                    .get::<_, Option<String>>("sender_did")?
                    .unwrap_or_default(),
                receiver_did: row
                    .get::<_, Option<String>>("receiver_did")?
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
fn merge_legacy_direct_did_conversation(
    connection: &rusqlite::Connection,
    owner_identity_id: &str,
    conversation_id: &str,
    record: &MessageRecord,
) -> crate::ImResult<()> {
    if !conversation_id.trim().starts_with("dm:peer-scope:")
        || !record.group_id.trim().is_empty()
        || !record.group_did.trim().is_empty()
    {
        return Ok(());
    }
    let legacy_conversation_ids =
        legacy_direct_conversation_ids_for_peer(connection, owner_identity_id, record)?;
    if legacy_conversation_ids.is_empty() {
        return Ok(());
    }
    let placeholders = vec!["?"; legacy_conversation_ids.len()].join(",");
    let statement = format!(
        r#"
UPDATE messages
SET conversation_id = ?,
    thread_id = ?
WHERE owner_identity_id = ?
  AND NULLIF(TRIM(COALESCE(group_id, '')), '') IS NULL
  AND NULLIF(TRIM(COALESCE(group_did, '')), '') IS NULL
  AND COALESCE(NULLIF(conversation_id, ''), thread_id) IN ({placeholders})"#
    );
    let mut params: Vec<&dyn rusqlite::ToSql> =
        Vec::with_capacity(legacy_conversation_ids.len() + 3);
    params.push(&conversation_id);
    params.push(&conversation_id);
    params.push(&owner_identity_id);
    for legacy_id in &legacy_conversation_ids {
        params.push(legacy_id);
    }
    connection
        .execute(&statement, params.as_slice())
        .map_err(super::local_state_unavailable)?;
    Ok(())
}

#[cfg(feature = "sqlite")]
fn legacy_direct_conversation_ids_for_peer(
    connection: &rusqlite::Connection,
    owner_identity_id: &str,
    record: &MessageRecord,
) -> crate::ImResult<Vec<String>> {
    let metadata = parse_metadata(&record.metadata);
    let mut ids = Vec::new();
    for did in [
        peer_current_did_from_metadata(&metadata),
        direct_peer_did(record),
    ]
    .into_iter()
    .flatten()
    {
        push_unique(
            &mut ids,
            crate::internal::local_state::owner_scope::direct_conversation_id(&did),
        );
    }
    if let Some(full_handle) = metadata
        .get("peer_full_handle")
        .and_then(serde_json::Value::as_str)
        .map(normalize_full_handle)
        .filter(|value| !value.is_empty())
    {
        for did in peer_dids_for_handle(record, &full_handle) {
            push_unique(
                &mut ids,
                crate::internal::local_state::owner_scope::direct_conversation_id(&did),
            );
        }
        for conversation_id in legacy_direct_conversation_ids_matching_handle(
            connection,
            owner_identity_id,
            &full_handle,
        )? {
            push_unique(&mut ids, conversation_id);
        }
    }
    Ok(ids)
}

#[cfg(feature = "sqlite")]
fn peer_current_did_from_metadata(
    metadata: &serde_json::Map<String, serde_json::Value>,
) -> Option<String> {
    ["peer_current_did", "resolved_target_did"]
        .into_iter()
        .find_map(|key| {
            metadata
                .get(key)
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|value| value.starts_with("did:"))
                .map(str::to_string)
        })
}

#[cfg(feature = "sqlite")]
fn peer_current_did(record: &MessageRecord) -> Option<String> {
    let metadata = parse_metadata(&record.metadata);
    peer_current_did_from_metadata(&metadata)
}

#[cfg(feature = "sqlite")]
fn direct_peer_did(record: &MessageRecord) -> Option<String> {
    if !record.group_id.trim().is_empty() || !record.group_did.trim().is_empty() {
        return None;
    }
    let owner_did = record.owner_did.trim();
    let peer = if record.sender_did.trim() != owner_did {
        record.sender_did.trim()
    } else {
        record.receiver_did.trim()
    };
    (!peer.is_empty() && peer.starts_with("did:")).then(|| peer.to_string())
}

#[cfg(feature = "sqlite")]
fn peer_dids_for_handle(record: &MessageRecord, full_handle: &str) -> Vec<String> {
    let mut dids = Vec::new();
    let current_did = peer_current_did(record).unwrap_or_default();
    for did in [
        record.sender_did.as_str(),
        record.receiver_did.as_str(),
        current_did.as_str(),
    ] {
        let did = did.trim();
        if did_full_handle(did).as_deref() == Some(full_handle) {
            push_unique(&mut dids, did.to_string());
        }
    }
    dids
}

#[cfg(feature = "sqlite")]
fn legacy_direct_conversation_ids_matching_handle(
    connection: &rusqlite::Connection,
    owner_identity_id: &str,
    full_handle: &str,
) -> crate::ImResult<Vec<String>> {
    let mut statement = connection
        .prepare(
            r#"
SELECT DISTINCT
    COALESCE(NULLIF(conversation_id, ''), thread_id) AS conversation_id,
    sender_did,
    receiver_did
FROM messages
WHERE owner_identity_id = ?1
  AND NULLIF(TRIM(COALESCE(group_id, '')), '') IS NULL
  AND NULLIF(TRIM(COALESCE(group_did, '')), '') IS NULL
  AND COALESCE(NULLIF(conversation_id, ''), thread_id) LIKE 'dm:did:wba:%'"#,
        )
        .map_err(super::local_state_unavailable)?;
    let rows = statement
        .query_map([owner_identity_id], |row| {
            Ok((
                row.get::<_, Option<String>>("conversation_id")?
                    .unwrap_or_default(),
                row.get::<_, Option<String>>("sender_did")?
                    .unwrap_or_default(),
                row.get::<_, Option<String>>("receiver_did")?
                    .unwrap_or_default(),
            ))
        })
        .map_err(super::local_state_unavailable)?;
    let mut ids = Vec::new();
    for row in rows {
        let (conversation_id, sender_did, receiver_did) =
            row.map_err(super::local_state_unavailable)?;
        let conversation_did = did_from_direct_conversation_id(&conversation_id);
        if [
            conversation_did.as_deref(),
            Some(sender_did.as_str()),
            Some(receiver_did.as_str()),
        ]
        .into_iter()
        .flatten()
        .any(|did| did_full_handle(did).as_deref() == Some(full_handle))
        {
            push_unique(&mut ids, conversation_id);
        }
    }
    Ok(ids)
}

#[cfg(feature = "sqlite")]
fn did_from_direct_conversation_id(conversation_id: &str) -> Option<String> {
    conversation_id
        .trim()
        .strip_prefix("dm:")
        .map(str::trim)
        .filter(|value| value.starts_with("did:"))
        .map(str::to_string)
}

#[cfg(feature = "sqlite")]
fn did_full_handle(did: &str) -> Option<String> {
    let mut parts = did.trim().strip_prefix("did:wba:")?.split(':');
    let domain = parts.next()?.trim();
    if domain.is_empty() {
        return None;
    }
    let path_parts = parts
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .take_while(|part| !part.starts_with("e1"))
        .collect::<Vec<_>>();
    let first_path = path_parts.first().copied()?;
    let local = match first_path {
        "user" | "users" => path_parts.get(1).copied()?,
        "agent" | "agents" => path_parts.last().copied()?,
        "group" | "groups" => return None,
        other => other,
    };
    if local.is_empty() || local.starts_with("e1") {
        return None;
    }
    Some(normalize_full_handle(&format!("{local}.{domain}")))
}

#[cfg(feature = "sqlite")]
fn normalize_full_handle(value: &str) -> String {
    value.trim().trim_end_matches('.').to_ascii_lowercase()
}

#[cfg(feature = "sqlite")]
fn push_unique(values: &mut Vec<String>, value: String) {
    if !value.trim().is_empty() && !values.iter().any(|known| known == &value) {
        values.push(value);
    }
}

#[cfg(feature = "sqlite")]
fn is_mail_record(record: &MessageRecord) -> bool {
    if record.content_type.trim() == "mail.notification" {
        return true;
    }
    parse_metadata(&record.metadata)
        .get("source_kind")
        .and_then(serde_json::Value::as_str)
        .map(|value| value.trim() == "mail")
        .unwrap_or(false)
}

#[cfg(feature = "sqlite")]
fn non_empty(value: &str) -> Option<&str> {
    let value = value.trim();
    (!value.is_empty()).then_some(value)
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
    (msg_id, owner_identity_id, owner_did, conversation_id, thread_id, direction, sender_did, receiver_did, content_type, content, stored_at)
VALUES (?1, ?2, ?3, ?4, ?4, 0, ?5, ?3, 'text/plain', 'direct', '2026-05-21T00:00:00Z')"#,
            (
                "direct-1",
                "alice-id",
                "did:example:alice",
                "dm:did:example:bob",
                "did:example:bob",
            ),
        )
        .unwrap();
        db.execute(
            r#"
INSERT INTO messages
    (msg_id, owner_identity_id, owner_did, conversation_id, thread_id, direction, group_id, group_did, content_type, content, stored_at)
VALUES (?1, ?2, ?3, ?4, ?4, 0, ?5, ?5, 'text/plain', 'group', '2026-05-21T00:00:00Z')"#,
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
    (msg_id, owner_identity_id, owner_did, conversation_id, thread_id, direction, content_type, content, stored_at, metadata)
VALUES (?1, ?2, ?3, ?4, ?4, 0, 'mail.notification', 'mail', '2026-05-21T00:00:00Z', ?5)"#,
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
                conversation_id: "dm:did:example:bob".to_owned(),
                thread_id: "dm:did:example:bob".to_owned(),
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
                conversation_id: "dm:did:example:bob".to_owned(),
                thread_id: "dm:did:example:bob".to_owned(),
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
                "SELECT owner_identity_id, conversation_id, thread_id, content, stored_at, is_e2ee FROM messages WHERE msg_id = 'msg-1'",
                [],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, i64>(5)?,
                    ))
                },
            )
            .unwrap();

        assert_eq!(row.0, "alice-id");
        assert_eq!(row.1, "dm:did:example:bob");
        assert_eq!(row.2, row.1);
        assert_eq!(row.3, "second");
        assert_eq!(row.4, "2026-05-24T00:00:01Z");
        assert_eq!(row.5, 1);
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
                conversation_id: "dm:did:peer".to_owned(),
                thread_id: "dm:did:peer".to_owned(),
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
                conversation_id: "dm:did:peer".to_owned(),
                thread_id: "dm:did:peer".to_owned(),
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

    #[test]
    fn local_state_messages_upsert_normalizes_legacy_direct_thread_alias() {
        let db = Connection::open_in_memory().unwrap();
        upsert_message(
            &db,
            &MessageRecord {
                msg_id: "msg-legacy-thread".to_owned(),
                owner_identity_id: "owner-id".to_owned(),
                owner_did: "did:owner".to_owned(),
                thread_id: "dm:did:owner:did:peer".to_owned(),
                direction: 0,
                content: "legacy alias".to_owned(),
                stored_at: "2026-01-01T00:00:00Z".to_owned(),
                ..MessageRecord::default()
            },
        )
        .unwrap();

        let (conversation_id, thread_id): (String, String) = db
            .query_row(
                "SELECT conversation_id, thread_id FROM messages WHERE msg_id = 'msg-legacy-thread'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();

        assert_eq!(conversation_id, "dm:did:peer");
        assert_eq!(thread_id, conversation_id);
    }

    #[test]
    fn local_state_messages_merge_rotated_did_direct_rows_into_peer_scope() {
        let db = Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        db.execute(
            r#"
INSERT INTO messages
    (msg_id, owner_identity_id, owner_did, conversation_id, thread_id, direction, sender_did, receiver_did,
     content_type, content, sent_at, stored_at, is_read)
VALUES (?1, ?2, ?3, ?4, ?4, 0, ?5, ?3,
        'text/plain', 'old did message', '2026-06-10T00:00:00Z', '2026-06-10T00:00:00Z', 1)"#,
            (
                "msg-old",
                "owner-id",
                "did:wba:anpclaw.com:zhuocheng:e1_owner",
                "dm:did:wba:anpclaw.com:zhuochengtest:e1_old",
                "did:wba:anpclaw.com:zhuochengtest:e1_old",
            ),
        )
        .unwrap();
        let scope = crate::internal::local_state::owner_scope::DirectPeerScope::new(
            "peer-user-id",
            "zhuochengtest.anpclaw.com",
        )
        .unwrap();
        let scoped_conversation_id =
            crate::internal::local_state::owner_scope::direct_conversation_id_for_peer_scope(
                &scope,
            );

        upsert_message(
            &db,
            &MessageRecord {
                msg_id: "msg-new".to_owned(),
                owner_identity_id: "owner-id".to_owned(),
                owner_did: "did:wba:anpclaw.com:zhuocheng:e1_owner".to_owned(),
                conversation_id: scoped_conversation_id.clone(),
                thread_id: scoped_conversation_id.clone(),
                direction: 1,
                sender_did: "did:wba:anpclaw.com:zhuocheng:e1_owner".to_owned(),
                receiver_did: "did:wba:anpclaw.com:zhuochengtest:e1_new".to_owned(),
                content_type: "text/plain".to_owned(),
                content: "new scoped message".to_owned(),
                stored_at: "2026-06-10T00:00:01Z".to_owned(),
                metadata: r#"{"peer_user_id":"peer-user-id","peer_full_handle":"zhuochengtest.anpclaw.com","peer_current_did":"did:wba:anpclaw.com:zhuochengtest:e1_new"}"#.to_owned(),
                ..MessageRecord::default()
            },
        )
        .unwrap();

        let conversations = db
            .prepare(
                "SELECT DISTINCT conversation_id FROM messages WHERE owner_identity_id = 'owner-id' ORDER BY conversation_id",
            )
            .unwrap()
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(conversations, vec![scoped_conversation_id.clone()]);
        let (conversation_id, message_count): (String, i64) = db
            .query_row(
                "SELECT conversation_id, message_count FROM threads WHERE owner_identity_id = 'owner-id'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(conversation_id, scoped_conversation_id);
        assert_eq!(message_count, 2);
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
