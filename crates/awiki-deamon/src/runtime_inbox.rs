use anyhow::{Context, Result};
use im_core::ids::{GroupRef, PageLimit, PeerRef};
use im_core::messages::{direct_peer_scope_thread_id, HistoryQuery, InboxScope, ThreadRef};
use serde_json::{json, Value};
use std::{collections::HashMap, path::Path};

use crate::agent::AgentDefinition;
use crate::im_core_adapter::ImCoreAdapter;
use crate::state::DaemonState;
use crate::DaemonConfig;

const INBOX_DEFAULT_LIMIT: u32 = 30;
const MAX_LIMIT: u32 = 100;
const PREVIEW_MAX_CHARS: usize = 120;
const TEXT_MAX_CHARS: usize = 4_000;
const ATTACHMENT_MANIFEST_CONTENT_TYPE: &str = "application/anp-attachment-manifest+json";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeInboxScope {
    All,
    Direct,
    Group,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeInboxThreadKind {
    Direct,
    Group,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeInboxQuery {
    pub runtime_agent_did: String,
    pub scope: RuntimeInboxScope,
    pub limit: u32,
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeInboxThreadQuery {
    pub runtime_agent_did: String,
    pub thread_id: String,
    pub kind: RuntimeInboxThreadKind,
    pub peer_did: Option<String>,
    pub peer_handle: Option<String>,
    pub group_did: Option<String>,
    pub limit: u32,
    pub cursor: Option<String>,
}

pub fn query_runtime_inbox(
    config: &DaemonConfig,
    state: &DaemonState,
    runtime_agent: &AgentDefinition,
    input: RuntimeInboxQuery,
) -> Result<Value> {
    let client = runtime_agent_client(config, state, runtime_agent)?;
    let _ = refresh_runtime_conversation_projection(&client, input.scope, input.limit);
    repair_scoped_direct_conversations(
        &client,
        &config.im_core_sqlite_path,
        client.current_identity().id.as_str(),
        runtime_agent,
    )
    .context("repair runtime inbox direct peer scopes")?;
    let offset = cursor_offset(input.cursor.as_deref())?;
    let records = load_local_conversations(
        &config.im_core_sqlite_path,
        client.current_identity().id.as_str(),
        &runtime_agent.agent_did,
        input.scope,
        input.limit,
        offset,
    )
    .context("read local runtime conversations")?;
    let has_more = records.len() > input.limit as usize;
    let page_records = records
        .into_iter()
        .take(input.limit as usize)
        .collect::<Vec<_>>();
    let consumed_count = page_records.len();
    let items = page_records
        .into_iter()
        .filter_map(|conversation| {
            inbox_item_json(&conversation, &runtime_agent.agent_did).transpose()
        })
        .collect::<Result<Vec<_>>>()?;
    let next_offset = offset + consumed_count;
    let next_cursor = if has_more {
        Some(next_offset.to_string())
    } else {
        None
    };
    Ok(json!({
        "scope": input.scope.as_str(),
        "items": items,
        "next_cursor": next_cursor,
        "fetched_at_ms": crate::security::runtime_token::current_time_millis()?,
    }))
}

pub fn query_runtime_inbox_thread(
    config: &DaemonConfig,
    state: &DaemonState,
    runtime_agent: &AgentDefinition,
    input: RuntimeInboxThreadQuery,
) -> Result<Value> {
    let client = runtime_agent_client(config, state, runtime_agent)?;
    if let Ok(thread) = thread_ref_from_query(&input) {
        let _ = client.messages().history(
            thread,
            HistoryQuery {
                limit: PageLimit(input.limit.min(MAX_LIMIT)),
                cursor: None,
            },
        );
    }
    repair_scoped_direct_conversations(
        &client,
        &config.im_core_sqlite_path,
        client.current_identity().id.as_str(),
        runtime_agent,
    )
    .context("repair runtime inbox direct peer scopes")?;
    let conversation_id = conversation_id_from_thread_query(&input)?;
    let offset = cursor_offset(input.cursor.as_deref())?;
    let mut records = load_local_messages(
        &config.im_core_sqlite_path,
        client.current_identity().id.as_str(),
        &conversation_id,
        input.limit,
        offset,
    )
    .context("read local runtime inbox thread")?;
    let has_more = records.len() > input.limit as usize;
    records.truncate(input.limit as usize);
    records.reverse();
    let title = thread_title_from_records(&input, &records);
    let messages = records
        .iter()
        .map(|message| message_json(message, &runtime_agent.agent_did))
        .collect::<Result<Vec<_>>>()?;
    let next_offset = offset + messages.len();
    Ok(json!({
        "thread_id": input.thread_id,
        "kind": input.kind.as_str(),
        "title": title,
        "messages": messages,
        "next_cursor": if has_more { Some(next_offset.to_string()) } else { None },
        "fetched_at_ms": crate::security::runtime_token::current_time_millis()?,
    }))
}

impl RuntimeInboxScope {
    pub fn parse(input: Option<&str>) -> Result<Self> {
        match input.unwrap_or("all").trim() {
            "" | "all" => Ok(Self::All),
            "direct" => Ok(Self::Direct),
            "group" => Ok(Self::Group),
            other => anyhow::bail!("unsupported runtime inbox scope: {other}"),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Direct => "direct",
            Self::Group => "group",
        }
    }

    fn inbox_scope(self) -> InboxScope {
        match self {
            Self::All => InboxScope::All,
            Self::Direct => InboxScope::DirectOnly,
            Self::Group => InboxScope::GroupOnly,
        }
    }
}

impl RuntimeInboxThreadKind {
    pub fn parse(input: Option<&str>, thread_id: &str) -> Result<Self> {
        let inferred = if thread_id.trim().starts_with("group:") {
            Some(Self::Group)
        } else if thread_id.trim().starts_with("dm:peer-scope:v1:") {
            Some(Self::Direct)
        } else {
            None
        };
        match input.map(str::trim).filter(|value| !value.is_empty()) {
            Some("direct") => Ok(Self::Direct),
            Some("group") => Ok(Self::Group),
            Some(other) => anyhow::bail!("unsupported runtime inbox thread kind: {other}"),
            None => inferred.context("runtime inbox thread kind is required"),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Direct => "direct",
            Self::Group => "group",
        }
    }
}

pub fn clamp_limit(value: Option<u64>, default_value: u32) -> Result<u32> {
    let value = value.unwrap_or(u64::from(default_value));
    if value == 0 {
        anyhow::bail!("limit must be greater than zero");
    }
    Ok(u32::try_from(value).unwrap_or(MAX_LIMIT).min(MAX_LIMIT))
}

fn runtime_agent_client(
    config: &DaemonConfig,
    state: &DaemonState,
    runtime_agent: &AgentDefinition,
) -> Result<im_core::ImClient> {
    let im_core = ImCoreAdapter::open(config)?;
    let identity = state.load_agent_identity(&runtime_agent.agent_did)?;
    let jwt_token = state.load_agent_auth_token(&runtime_agent.agent_did)?;
    im_core.client_for_agent_identity(config, &identity, jwt_token.as_deref())
}

fn refresh_runtime_conversation_projection(
    client: &im_core::ImClient,
    scope: RuntimeInboxScope,
    limit: u32,
) -> Result<()> {
    let inbox_limit = limit.max(INBOX_DEFAULT_LIMIT).min(MAX_LIMIT);
    client.messages().inbox(im_core::messages::InboxQuery {
        scope: scope.inbox_scope(),
        limit: PageLimit(inbox_limit),
        cursor: None,
        unread_only: false,
    })?;
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LocalConversationRecord {
    conversation_id: String,
    message_count: u32,
    unread_count: u32,
    last_message_at: String,
    last_message: Option<LocalMessageRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LocalMessageRecord {
    msg_id: String,
    direction: i64,
    sender_did: String,
    receiver_did: String,
    group_id: String,
    group_did: String,
    content_type: String,
    content: String,
    sent_at: String,
    stored_at: String,
    is_read: bool,
    metadata: String,
}

fn load_local_conversations(
    sqlite_path: &Path,
    owner_identity_id: &str,
    runtime_agent_did: &str,
    scope: RuntimeInboxScope,
    limit: u32,
    offset: usize,
) -> Result<Vec<LocalConversationRecord>> {
    let connection = rusqlite::Connection::open(sqlite_path)?;
    let records = load_local_conversations_from_messages(&connection, owner_identity_id, scope)?;
    let records = normalize_runtime_inbox_conversation_records(records, runtime_agent_did)?;
    Ok(records
        .into_iter()
        .skip(offset)
        .take(limit.min(MAX_LIMIT) as usize + 1)
        .collect())
}

fn load_local_messages(
    sqlite_path: &Path,
    owner_identity_id: &str,
    conversation_id: &str,
    limit: u32,
    offset: usize,
) -> Result<Vec<LocalMessageRecord>> {
    let connection = rusqlite::Connection::open(sqlite_path)?;
    let row_limit = i64::from(limit.min(MAX_LIMIT)) + 1;
    let row_offset = i64::try_from(offset).context("runtime inbox thread offset too large")?;
    let mut statement = connection.prepare(
        r#"
SELECT
    msg_id,
    direction,
    sender_did,
    receiver_did,
    group_id,
    group_did,
    content_type,
    content,
    sent_at,
    stored_at,
    is_read,
    metadata
FROM messages
WHERE owner_identity_id = ?1
  AND COALESCE(NULLIF(conversation_id, ''), thread_id) = ?2
ORDER BY COALESCE(sent_at, stored_at) DESC, msg_id DESC
LIMIT ?3 OFFSET ?4"#,
    )?;
    let rows = statement.query_map(
        (owner_identity_id, conversation_id, row_limit, row_offset),
        |row| {
            Ok(LocalMessageRecord {
                msg_id: optional_string(row, "msg_id")?,
                direction: row.get::<_, Option<i64>>("direction")?.unwrap_or_default(),
                sender_did: optional_string(row, "sender_did")?,
                receiver_did: optional_string(row, "receiver_did")?,
                group_id: optional_string(row, "group_id")?,
                group_did: optional_string(row, "group_did")?,
                content_type: optional_string(row, "content_type")?,
                content: optional_string(row, "content")?,
                sent_at: optional_string(row, "sent_at")?,
                stored_at: optional_string(row, "stored_at")?,
                is_read: row.get::<_, Option<i64>>("is_read")?.unwrap_or_default() != 0,
                metadata: optional_string(row, "metadata")?,
            })
        },
    )?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

fn load_local_conversations_from_messages(
    connection: &rusqlite::Connection,
    owner_identity_id: &str,
    scope: RuntimeInboxScope,
) -> Result<Vec<LocalConversationRecord>> {
    let mut statement = String::from(
        r#"
WITH latest AS (
    SELECT
        COALESCE(NULLIF(conversation_id, ''), thread_id) AS conversation_id,
        COUNT(*) AS message_count,
        SUM(CASE WHEN is_read = 0 AND direction = 0 THEN 1 ELSE 0 END) AS unread_count,
        MAX(COALESCE(NULLIF(sent_at, ''), stored_at)) AS last_message_at
    FROM messages
    WHERE owner_identity_id = ?1
      AND COALESCE(NULLIF(conversation_id, ''), thread_id) <> ''
"#,
    );
    match scope {
        RuntimeInboxScope::All => {}
        RuntimeInboxScope::Direct => statement.push_str(
            "      AND COALESCE(NULLIF(conversation_id, ''), thread_id) NOT LIKE 'group:%'\n",
        ),
        RuntimeInboxScope::Group => statement.push_str(
            "      AND COALESCE(NULLIF(conversation_id, ''), thread_id) LIKE 'group:%'\n",
        ),
    }
    statement.push_str(
        r#"
    GROUP BY COALESCE(NULLIF(conversation_id, ''), thread_id)
)
SELECT
    l.conversation_id,
    l.message_count,
    l.unread_count,
    l.last_message_at,
    m.msg_id,
    m.direction,
    m.sender_did,
    m.receiver_did,
    m.group_id,
    m.group_did,
    m.content_type,
    m.content,
    m.sent_at,
    m.stored_at,
    m.is_read,
    m.metadata
FROM latest l
JOIN messages m
  ON m.owner_identity_id = ?1
 AND COALESCE(NULLIF(m.conversation_id, ''), m.thread_id) = l.conversation_id
 AND COALESCE(NULLIF(m.sent_at, ''), m.stored_at) = l.last_message_at
 AND m.msg_id = (
     SELECT m2.msg_id
     FROM messages m2
     WHERE m2.owner_identity_id = ?1
       AND COALESCE(NULLIF(m2.conversation_id, ''), m2.thread_id) = l.conversation_id
       AND COALESCE(NULLIF(m2.sent_at, ''), m2.stored_at) = l.last_message_at
     ORDER BY m2.msg_id DESC
     LIMIT 1
)
ORDER BY l.last_message_at DESC, l.conversation_id ASC"#,
    );
    let mut statement = connection.prepare(&statement)?;
    let rows = statement.query_map((owner_identity_id,), |row| {
        Ok(LocalConversationRecord {
            conversation_id: optional_string(row, "conversation_id")?,
            message_count: u32_from_i64(
                row.get::<_, Option<i64>>("message_count")?
                    .unwrap_or_default(),
            ),
            unread_count: u32_from_i64(
                row.get::<_, Option<i64>>("unread_count")?
                    .unwrap_or_default(),
            ),
            last_message_at: optional_string(row, "last_message_at")?,
            last_message: Some(LocalMessageRecord {
                msg_id: optional_string(row, "msg_id")?,
                direction: row.get::<_, Option<i64>>("direction")?.unwrap_or_default(),
                sender_did: optional_string(row, "sender_did")?,
                receiver_did: optional_string(row, "receiver_did")?,
                group_id: optional_string(row, "group_id")?,
                group_did: optional_string(row, "group_did")?,
                content_type: optional_string(row, "content_type")?,
                content: optional_string(row, "content")?,
                sent_at: optional_string(row, "sent_at")?,
                stored_at: optional_string(row, "stored_at")?,
                is_read: row.get::<_, Option<i64>>("is_read")?.unwrap_or_default() != 0,
                metadata: optional_string(row, "metadata")?,
            }),
        })
    })?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

fn normalize_runtime_inbox_conversation_records(
    records: Vec<LocalConversationRecord>,
    runtime_agent_did: &str,
) -> Result<Vec<LocalConversationRecord>> {
    let mut by_thread_id = HashMap::<String, LocalConversationRecord>::new();
    for record in records {
        let Some(fields) = conversation_fields(&record, runtime_agent_did)? else {
            continue;
        };
        by_thread_id
            .entry(fields.thread_id.clone())
            .and_modify(|existing| {
                merge_normalized_conversation_record(existing, &record, &fields.thread_id);
            })
            .or_insert(record);
    }
    let mut records = by_thread_id.into_values().collect::<Vec<_>>();
    records.sort_by(|left, right| {
        right
            .last_message_at
            .cmp(&left.last_message_at)
            .then_with(|| left.conversation_id.cmp(&right.conversation_id))
    });
    Ok(records)
}

fn merge_normalized_conversation_record(
    existing: &mut LocalConversationRecord,
    candidate: &LocalConversationRecord,
    canonical_thread_id: &str,
) {
    existing.message_count = existing
        .message_count
        .saturating_add(candidate.message_count);
    existing.unread_count = existing.unread_count.saturating_add(candidate.unread_count);
    if should_replace_conversation_record(existing, candidate, canonical_thread_id) {
        let message_count = existing.message_count;
        let unread_count = existing.unread_count;
        *existing = candidate.clone();
        existing.message_count = message_count;
        existing.unread_count = unread_count;
    }
}

fn should_replace_conversation_record(
    existing: &LocalConversationRecord,
    candidate: &LocalConversationRecord,
    canonical_thread_id: &str,
) -> bool {
    let existing_is_canonical = existing.conversation_id.trim() == canonical_thread_id;
    let candidate_is_canonical = candidate.conversation_id.trim() == canonical_thread_id;
    if candidate_is_canonical != existing_is_canonical {
        return candidate_is_canonical;
    }
    if candidate.last_message_at != existing.last_message_at {
        return candidate.last_message_at > existing.last_message_at;
    }
    existing.last_message.is_none() && candidate.last_message.is_some()
}

fn optional_string(row: &rusqlite::Row<'_>, name: &str) -> rusqlite::Result<String> {
    row.get::<_, Option<String>>(name)
        .map(|value| value.unwrap_or_default())
}

fn u32_from_i64(value: i64) -> u32 {
    u32::try_from(value.max(0)).unwrap_or(u32::MAX)
}

fn inbox_item_json(
    conversation: &LocalConversationRecord,
    runtime_agent_did: &str,
) -> Result<Option<Value>> {
    let Some(fields) = conversation_fields(conversation, runtime_agent_did)? else {
        return Ok(None);
    };
    let last_message = conversation.last_message.as_ref();
    let preview = last_message
        .map(message_preview)
        .unwrap_or_else(|| truncate_chars("", PREVIEW_MAX_CHARS).0);
    let last_content_type = last_message
        .map(message_content_type)
        .unwrap_or_else(|| "text".to_string());
    Ok(Some(json!({
        "thread_id": fields.thread_id,
        "kind": fields.kind,
        "title": fields.title,
        "peer_user_id": fields.peer_user_id,
        "peer_handle": fields.peer_handle,
        "peer_did": fields.peer_did,
        "group_id": fields.group_id,
        "group_did": fields.group_did,
        "last_message_preview": preview,
        "last_message_at_ms": conversation
            .last_message_at
            .as_str()
            .parse_timestamp_ms(),
        "unread_count": conversation.unread_count,
        "has_attachments": last_message.is_some_and(message_has_attachments),
        "last_content_type": last_content_type,
    })))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ConversationFields {
    kind: &'static str,
    thread_id: String,
    title: String,
    peer_user_id: Option<String>,
    peer_handle: Option<String>,
    peer_did: Option<String>,
    group_id: Option<String>,
    group_did: Option<String>,
}

fn conversation_fields(
    conversation: &LocalConversationRecord,
    runtime_agent_did: &str,
) -> Result<Option<ConversationFields>> {
    if let Some(group) = conversation.conversation_id.strip_prefix("group:") {
        let group_did = conversation
            .last_message
            .as_ref()
            .map(|message| first_non_empty([&message.group_did, &message.group_id]))
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| group.to_string());
        return Ok(Some(ConversationFields {
            kind: "group",
            thread_id: format!("group:{group_did}"),
            title: group_did.clone(),
            peer_user_id: None,
            peer_handle: None,
            peer_did: None,
            group_id: Some(group_did.clone()),
            group_did: Some(group_did),
        }));
    }
    let metadata_scope = conversation
        .last_message
        .as_ref()
        .and_then(peer_scope_from_message);
    let peer_did = current_peer_did_from_metadata(metadata_scope.as_ref())
        .or_else(|| {
            conversation
                .last_message
                .as_ref()
                .and_then(|message| message_peer_did(message, runtime_agent_did))
        })
        .or_else(|| direct_peer_from_conversation_id(&conversation.conversation_id));
    if let Some(scope) = metadata_scope {
        let thread_id = scoped_direct_conversation_id(&scope)?;
        let title = scope.full_handle.clone();
        return Ok(Some(ConversationFields {
            kind: "direct",
            thread_id,
            title,
            peer_user_id: Some(scope.user_id),
            peer_handle: Some(scope.full_handle),
            peer_did,
            group_id: None,
            group_did: None,
        }));
    }
    Ok(None)
}

fn message_peer_did(message: &LocalMessageRecord, runtime_agent_did: &str) -> Option<String> {
    message_peer_did_from_parts(
        &message.sender_did,
        &message.receiver_did,
        runtime_agent_did,
    )
}

fn message_peer_did_from_parts(
    sender_did: &str,
    receiver_did: &str,
    runtime_agent_did: &str,
) -> Option<String> {
    let runtime = runtime_agent_did.trim();
    let sender = sender_did.trim();
    let receiver = receiver_did.trim();
    let peer = if sender != runtime { sender } else { receiver };
    (!peer.is_empty()).then(|| peer.to_string())
}

fn thread_ref_from_query(input: &RuntimeInboxThreadQuery) -> Result<ThreadRef> {
    match input.kind {
        RuntimeInboxThreadKind::Direct => {
            let peer = direct_history_peer_from_query(input)
                .context("direct runtime inbox thread requires peer_handle or peer_did")?;
            Ok(ThreadRef::Direct(PeerRef::parse(peer, "")?))
        }
        RuntimeInboxThreadKind::Group => {
            let group = input
                .group_did
                .as_deref()
                .or_else(|| input.thread_id.strip_prefix("group:"))
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .context("group runtime inbox thread requires group_did")?;
            Ok(ThreadRef::Group(GroupRef::parse(group)?))
        }
    }
}

fn conversation_id_from_thread_query(input: &RuntimeInboxThreadQuery) -> Result<String> {
    match input.kind {
        RuntimeInboxThreadKind::Direct => {
            let thread_id = input.thread_id.trim();
            if thread_id.starts_with("dm:peer-scope:v1:") {
                return Ok(thread_id.to_string());
            }
            anyhow::bail!("direct runtime inbox thread requires stable dm:peer-scope:v1 thread_id")
        }
        RuntimeInboxThreadKind::Group => {
            let thread_id = input.thread_id.trim();
            if thread_id.starts_with("group:") {
                return Ok(thread_id.to_string());
            }
            let group = input
                .group_did
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .context("group runtime inbox thread requires group_did")?;
            Ok(format!("group:{group}"))
        }
    }
}

fn thread_title_from_records(
    input: &RuntimeInboxThreadQuery,
    messages: &[LocalMessageRecord],
) -> String {
    if let RuntimeInboxThreadKind::Direct = input.kind {
        if let Some(peer_handle) = input
            .peer_handle
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            return peer_handle.to_string();
        }
    }
    for message in messages {
        match input.kind {
            RuntimeInboxThreadKind::Direct => {
                if let Some(scope) = peer_scope_from_message(message) {
                    return scope.full_handle;
                }
                let candidate =
                    message_peer_did(message, &input.runtime_agent_did).unwrap_or_default();
                if !candidate.trim().is_empty() {
                    return candidate;
                }
            }
            RuntimeInboxThreadKind::Group => {
                let group = first_non_empty([&message.group_did, &message.group_id]);
                if !group.trim().is_empty() {
                    return group;
                }
            }
        }
    }
    match input.kind {
        RuntimeInboxThreadKind::Direct => input
            .peer_handle
            .clone()
            .or_else(|| input.peer_did.clone())
            .or_else(|| direct_peer_from_conversation_id(&input.thread_id))
            .or_else(|| input.thread_id.strip_prefix("dm:").map(str::to_string))
            .unwrap_or_else(|| input.thread_id.clone()),
        RuntimeInboxThreadKind::Group => input
            .group_did
            .clone()
            .or_else(|| input.thread_id.strip_prefix("group:").map(str::to_string))
            .unwrap_or_else(|| input.thread_id.clone()),
    }
}

fn direct_peer_from_conversation_id(conversation_id: &str) -> Option<String> {
    let raw = conversation_id.trim();
    if raw.starts_with("dm:peer-scope:") {
        return None;
    }
    raw.strip_prefix("dm:")
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn direct_history_peer_from_query(input: &RuntimeInboxThreadQuery) -> Option<&str> {
    input
        .peer_handle
        .as_deref()
        .or(input.peer_did.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DirectPeerScope {
    user_id: String,
    full_handle: String,
    current_did: Option<String>,
}

fn peer_scope_from_message(message: &LocalMessageRecord) -> Option<DirectPeerScope> {
    peer_scope_from_metadata(&message.metadata)
}

fn peer_scope_from_metadata(metadata: &str) -> Option<DirectPeerScope> {
    let object = serde_json::from_str::<Value>(metadata)
        .ok()
        .and_then(|value| value.as_object().cloned())?;
    let user_id = object
        .get("peer_user_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())?
        .to_string();
    let full_handle = object
        .get("peer_full_handle")
        .or_else(|| object.get("target_handle"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())?
        .to_ascii_lowercase();
    let current_did = object
        .get("peer_current_did")
        .or_else(|| object.get("resolved_target_did"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    Some(DirectPeerScope {
        user_id,
        full_handle,
        current_did,
    })
}

fn current_peer_did_from_metadata(scope: Option<&DirectPeerScope>) -> Option<String> {
    scope.and_then(|scope| scope.current_did.clone())
}

fn scoped_direct_conversation_id(scope: &DirectPeerScope) -> Result<String> {
    Ok(
        direct_peer_scope_thread_id(&scope.user_id, &scope.full_handle)?
            .as_str()
            .to_string(),
    )
}

fn repair_scoped_direct_conversations(
    client: &im_core::ImClient,
    sqlite_path: &Path,
    owner_identity_id: &str,
    runtime_agent: &AgentDefinition,
) -> Result<()> {
    let mut connection = rusqlite::Connection::open(sqlite_path)?;
    let candidates = load_direct_repair_candidates(&connection, owner_identity_id)?;
    if candidates.is_empty() {
        return Ok(());
    }
    let mut lookup_cache = HashMap::<String, Option<DirectPeerScope>>::new();
    let transaction = connection.transaction()?;
    for candidate in candidates {
        let Some(peer_did) = candidate.peer_did(&runtime_agent.agent_did) else {
            continue;
        };
        let scope = candidate
            .metadata_scope
            .clone()
            .or_else(|| controller_scope_for_candidate(&candidate, runtime_agent, &peer_did))
            .or_else(|| {
                lookup_cache
                    .entry(peer_did.clone())
                    .or_insert_with(|| lookup_peer_scope_by_did(client, &peer_did))
                    .clone()
            });
        let Some(scope) = scope else {
            continue;
        };
        let conversation_id = scoped_direct_conversation_id(&scope)?;
        let metadata = metadata_with_peer_scope(&candidate.metadata, &scope, Some(&peer_did));
        transaction.execute(
            r#"
UPDATE messages
SET conversation_id = ?2,
    thread_id = ?2,
    metadata = ?3
WHERE owner_identity_id = ?4
  AND msg_id = ?1"#,
            rusqlite::params![
                candidate.msg_id,
                conversation_id,
                metadata,
                owner_identity_id,
            ],
        )?;
    }
    transaction.commit()?;
    Ok(())
}

pub fn repair_runtime_controller_inbox_projection(
    config: &DaemonConfig,
    state: &DaemonState,
    runtime_agent_did: &str,
) -> Result<()> {
    let runtime_agent = state.load_agent_definition(runtime_agent_did)?;
    let im_core = ImCoreAdapter::open(config)?;
    let identity = state.load_agent_identity(runtime_agent_did)?;
    let jwt_token = state.load_agent_auth_token(runtime_agent_did)?;
    let client = im_core.client_for_agent_identity(config, &identity, jwt_token.as_deref())?;
    repair_scoped_direct_conversations(
        &client,
        &config.im_core_sqlite_path,
        client.current_identity().id.as_str(),
        &runtime_agent,
    )
}

#[derive(Debug, Clone)]
struct DirectRepairCandidate {
    msg_id: String,
    sender_did: String,
    receiver_did: String,
    conversation_id: String,
    metadata: String,
    metadata_scope: Option<DirectPeerScope>,
}

impl DirectRepairCandidate {
    fn peer_did(&self, runtime_agent_did: &str) -> Option<String> {
        current_peer_did_from_metadata(self.metadata_scope.as_ref())
            .or_else(|| {
                message_peer_did_from_parts(&self.sender_did, &self.receiver_did, runtime_agent_did)
            })
            .or_else(|| direct_peer_from_conversation_id(&self.conversation_id))
    }
}

fn controller_scope_for_candidate(
    candidate: &DirectRepairCandidate,
    runtime_agent: &AgentDefinition,
    peer_did: &str,
) -> Option<DirectPeerScope> {
    if peer_did.trim() != runtime_agent.controller_did.trim() {
        return None;
    }
    let user_id = runtime_agent.controller_user_id.trim();
    let full_handle = runtime_agent.controller_full_handle.trim();
    if user_id.is_empty() || full_handle.is_empty() {
        return None;
    }
    let direct_to_runtime = candidate.receiver_did.trim() == runtime_agent.agent_did.trim()
        || candidate.sender_did.trim() == runtime_agent.agent_did.trim();
    if !direct_to_runtime {
        return None;
    }
    Some(DirectPeerScope {
        user_id: user_id.to_string(),
        full_handle: full_handle.to_ascii_lowercase(),
        current_did: Some(peer_did.trim().to_string()),
    })
}

fn load_direct_repair_candidates(
    connection: &rusqlite::Connection,
    owner_identity_id: &str,
) -> Result<Vec<DirectRepairCandidate>> {
    let mut statement = connection.prepare(
        r#"
SELECT
    msg_id,
    sender_did,
    receiver_did,
    COALESCE(NULLIF(conversation_id, ''), thread_id) AS conversation_id,
    metadata
FROM messages
WHERE owner_identity_id = ?1
  AND COALESCE(NULLIF(conversation_id, ''), thread_id) NOT LIKE 'group:%'
  AND COALESCE(NULLIF(conversation_id, ''), thread_id) NOT LIKE 'dm:peer-scope:%'"#,
    )?;
    let rows = statement.query_map((owner_identity_id,), |row| {
        let sender_did = optional_string(row, "sender_did")?;
        let receiver_did = optional_string(row, "receiver_did")?;
        let metadata = optional_string(row, "metadata")?;
        let candidate = DirectRepairCandidate {
            msg_id: optional_string(row, "msg_id")?,
            sender_did,
            receiver_did,
            conversation_id: optional_string(row, "conversation_id")?,
            metadata_scope: peer_scope_from_metadata(&metadata),
            metadata,
        };
        Ok(candidate)
    })?;
    let candidates = rows.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(candidates)
}

fn lookup_peer_scope_by_did(client: &im_core::ImClient, peer_did: &str) -> Option<DirectPeerScope> {
    let resolution = client
        .directory()
        .resolve_peer(PeerRef::parse(peer_did, "").ok()?)
        .ok()?;
    let handle = resolution.handle?;
    let lookup = client.directory().lookup_handle(handle).ok()?;
    Some(DirectPeerScope {
        user_id: lookup.user_id,
        full_handle: lookup.handle.as_str().to_ascii_lowercase(),
        current_did: Some(lookup.did.as_str().to_string()),
    })
}

fn metadata_with_peer_scope(
    metadata: &str,
    scope: &DirectPeerScope,
    current_did: Option<&str>,
) -> String {
    let mut object = serde_json::from_str::<Value>(metadata)
        .ok()
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default();
    object.insert(
        "peer_user_id".to_string(),
        Value::String(scope.user_id.clone()),
    );
    object.insert(
        "peer_full_handle".to_string(),
        Value::String(scope.full_handle.clone()),
    );
    if let Some(did) = scope
        .current_did
        .as_deref()
        .or(current_did)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        object.insert(
            "peer_current_did".to_string(),
            Value::String(did.to_string()),
        );
        object.insert(
            "resolved_target_did".to_string(),
            Value::String(did.to_string()),
        );
    }
    Value::Object(object).to_string()
}

fn message_json(message: &LocalMessageRecord, runtime_agent_did: &str) -> Result<Value> {
    let (text, truncated) = message_text(message);
    Ok(json!({
        "message_id": message.msg_id,
        "sender_did": message.sender_did,
        "sender_handle": sender_handle_for_message(message, runtime_agent_did),
        "sent_at_ms": message
            .sent_at
            .as_str()
            .parse_timestamp_ms()
            .or_else(|| parse_timestamp_ms(&message.stored_at)),
        "direction": if message.sender_did.trim() == runtime_agent_did || message.direction == 1 {
            "outgoing"
        } else if message.direction == 0 {
            "incoming"
        } else {
            "unknown"
        },
        "content_type": message_content_type(message),
        "text": text,
        "truncated": truncated,
        "attachments": attachment_items(message),
    }))
}

fn sender_handle_for_message(
    message: &LocalMessageRecord,
    runtime_agent_did: &str,
) -> Option<String> {
    if message.sender_did.trim() == runtime_agent_did.trim() {
        return None;
    }
    peer_scope_from_message(message).map(|scope| scope.full_handle)
}

fn message_preview(message: &LocalMessageRecord) -> String {
    if let Some(caption) = attachment_caption(message) {
        return truncate_chars(&caption, PREVIEW_MAX_CHARS).0;
    }
    let (text, _) = message_text_with_limit(message, PREVIEW_MAX_CHARS);
    if !text.trim().is_empty() {
        return text;
    }
    let attachments = attachment_items(message);
    if let Some(filename) = attachments
        .first()
        .and_then(|item| item.get("filename"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return truncate_chars(&format!("附件: {filename}"), PREVIEW_MAX_CHARS).0;
    }
    if !attachments.is_empty() {
        return "附件".to_string();
    }
    String::new()
}

fn message_text(message: &LocalMessageRecord) -> (String, bool) {
    message_text_with_limit(message, TEXT_MAX_CHARS)
}

fn message_text_with_limit(message: &LocalMessageRecord, limit: usize) -> (String, bool) {
    if message_is_attachment_manifest(message) {
        if let Some(payload) = attachment_payload(message) {
            let text = attachment_caption_from_payload(&payload)
                .or_else(|| payload.get("text").and_then(Value::as_str))
                .unwrap_or("");
            return truncate_chars(text, limit);
        }
        return (String::new(), false);
    }
    match message.content_type.trim() {
        "" | "text" | "text/plain" | "text/markdown" => truncate_chars(&message.content, limit),
        "application/json" => serde_json::from_str::<Value>(&message.content)
            .ok()
            .and_then(|payload| {
                payload
                    .get("text")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .map(|text| truncate_chars(&text, limit))
            .unwrap_or_else(|| (String::new(), false)),
        _ => (String::new(), false),
    }
}

fn message_content_type(message: &LocalMessageRecord) -> String {
    if message_has_attachments(message) {
        return "attachment".to_string();
    }
    match message.content_type.trim() {
        "" | "text" | "text/plain" | "text/markdown" => "text".to_string(),
        other => other.to_string(),
    }
}

fn message_has_attachments(message: &LocalMessageRecord) -> bool {
    !attachment_items(message).is_empty()
}

fn attachment_items(message: &LocalMessageRecord) -> Vec<Value> {
    let manifest_items = attachment_items_from_manifest(message);
    if !manifest_items.is_empty() {
        return manifest_items;
    }
    attachment_items_from_metadata(message)
}

fn attachment_items_from_manifest(message: &LocalMessageRecord) -> Vec<Value> {
    let Some(payload) = attachment_payload(message) else {
        return Vec::new();
    };
    payload
        .get("attachments")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_object())
                .filter(|item| is_valid_attachment_object(item))
                .map(|item| {
                    json!({
                        "attachment_id": string_field_any(item, &["attachment_id", "id"]),
                        "filename": string_field_any(item, &["filename", "display_filename", "name"]),
                        "mime_type": string_field_any(item, &["mime_type", "content_type"]),
                        "size_bytes": attachment_size_bytes(item),
                        "download_state": "available",
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn attachment_payload(message: &LocalMessageRecord) -> Option<Value> {
    if !message_is_attachment_manifest(message) {
        return None;
    }
    let value = serde_json::from_str::<Value>(&message.content).ok()?;
    if value.get("attachments").is_some() {
        return Some(value);
    }
    value.get("payload").cloned()
}

fn message_is_attachment_manifest(message: &LocalMessageRecord) -> bool {
    message.content_type.trim() == ATTACHMENT_MANIFEST_CONTENT_TYPE
        || metadata_content_type(&message.metadata)
            .is_some_and(|value| value == ATTACHMENT_MANIFEST_CONTENT_TYPE)
}

fn metadata_content_type(metadata: &str) -> Option<String> {
    metadata_object(metadata).and_then(|object| {
        object
            .get("content_type")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    })
}

fn attachment_items_from_metadata(message: &LocalMessageRecord) -> Vec<Value> {
    let Some(metadata) = metadata_object(&message.metadata) else {
        return Vec::new();
    };
    if let Some(summary) = metadata
        .get("attachment_summary")
        .and_then(Value::as_object)
    {
        if !is_valid_attachment_object(summary) {
            return Vec::new();
        }
        return vec![json!({
            "attachment_id": string_field_any(summary, &["attachment_id", "id"]),
            "filename": string_field_any(summary, &["filename", "display_filename", "name"]),
            "mime_type": string_field_any(summary, &["mime_type", "content_type"]),
            "size_bytes": attachment_size_bytes(summary),
            "download_state": "available",
        })];
    }
    let attachment_id = metadata_string_any(
        &metadata,
        &["attachment_id", "attachmentId", "id"],
        &["attachment_id"],
    );
    let filename = metadata_string_any(
        &metadata,
        &[
            "attachment_filename",
            "filename",
            "display_filename",
            "name",
        ],
        &["attachment_filename"],
    );
    let mime_type = metadata_string_any(
        &metadata,
        &["attachment_mime_type", "attachment_content_type"],
        &["attachment_mime_type"],
    );
    let size_bytes = metadata_size_bytes(&metadata);
    let has_attachment = attachment_id.is_some()
        || filename.is_some()
        || mime_type.is_some()
        || (!size_bytes.is_null()
            && metadata_bool_any(&metadata, &["has_attachments", "hasAttachments"]));
    if !has_attachment {
        return Vec::new();
    }
    vec![json!({
        "attachment_id": attachment_id.map(Value::String).unwrap_or(Value::Null),
        "filename": filename.map(Value::String).unwrap_or(Value::Null),
        "mime_type": mime_type.map(Value::String).unwrap_or(Value::Null),
        "size_bytes": size_bytes,
        "download_state": "available",
    })]
}

fn is_valid_attachment_object(item: &serde_json::Map<String, Value>) -> bool {
    string_field_option(item.get("attachment_id"))
        .or_else(|| string_field_option(item.get("id")))
        .or_else(|| string_field_option(item.get("filename")))
        .or_else(|| string_field_option(item.get("display_filename")))
        .or_else(|| string_field_option(item.get("name")))
        .or_else(|| string_field_option(item.get("mime_type")))
        .is_some()
        || !attachment_size_bytes(item).is_null()
}

fn attachment_caption(message: &LocalMessageRecord) -> Option<String> {
    attachment_payload(message)
        .as_ref()
        .and_then(attachment_caption_from_payload)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn attachment_caption_from_payload(payload: &Value) -> Option<&str> {
    payload
        .get("caption")
        .and_then(Value::as_str)
        .or_else(|| payload.get("text").and_then(Value::as_str))
}

fn attachment_size_bytes(item: &serde_json::Map<String, Value>) -> Value {
    for key in ["size_bytes", "plaintext_size_bytes"] {
        if let Some(value) = item.get(key).and_then(Value::as_u64) {
            return json!(value);
        }
    }
    if let Some(size) = item
        .get("size")
        .or_else(|| item.get("plaintext_size"))
        .and_then(Value::as_str)
        .and_then(parse_size_bytes)
    {
        return json!(size);
    }
    Value::Null
}

fn metadata_object(metadata: &str) -> Option<serde_json::Map<String, Value>> {
    serde_json::from_str::<Value>(metadata)
        .ok()
        .and_then(|value| value.as_object().cloned())
}

fn string_field_any(item: &serde_json::Map<String, Value>, keys: &[&str]) -> Value {
    keys.iter()
        .find_map(|key| string_field_option(item.get(*key)))
        .map(Value::String)
        .unwrap_or(Value::Null)
}

fn string_field_option(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn metadata_string_any(
    metadata: &serde_json::Map<String, Value>,
    direct_keys: &[&str],
    attribute_keys: &[&str],
) -> Option<String> {
    direct_keys
        .iter()
        .find_map(|key| string_field_option(metadata.get(*key)))
        .or_else(|| metadata_attribute_string(metadata, attribute_keys))
}

fn metadata_attribute_string(
    metadata: &serde_json::Map<String, Value>,
    keys: &[&str],
) -> Option<String> {
    let attributes = metadata.get("attributes").and_then(Value::as_array)?;
    attributes
        .iter()
        .filter_map(Value::as_object)
        .find_map(|item| {
            let key = string_field_option(item.get("key"))?;
            if !keys.iter().any(|known| key == *known) {
                return None;
            }
            string_field_option(item.get("value"))
        })
}

fn metadata_bool_any(metadata: &serde_json::Map<String, Value>, keys: &[&str]) -> bool {
    keys.iter()
        .find_map(|key| metadata.get(*key))
        .is_some_and(value_as_bool)
}

fn value_as_bool(value: &Value) -> bool {
    match value {
        Value::Bool(value) => *value,
        Value::String(value) => matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "true" | "1" | "yes"
        ),
        Value::Number(value) => value.as_i64().is_some_and(|value| value != 0),
        _ => false,
    }
}

fn metadata_size_bytes(metadata: &serde_json::Map<String, Value>) -> Value {
    for key in [
        "attachment_size_bytes",
        "size_bytes",
        "plaintext_size_bytes",
        "size",
        "plaintext_size",
    ] {
        if let Some(size) = value_size_bytes(metadata.get(key)) {
            return json!(size);
        }
    }
    if let Some(size) = metadata_attribute_string(
        metadata,
        &[
            "attachment_size_bytes",
            "size_bytes",
            "plaintext_size_bytes",
            "size",
            "plaintext_size",
        ],
    )
    .and_then(|value| parse_size_bytes(&value))
    {
        return json!(size);
    }
    Value::Null
}

fn value_size_bytes(value: Option<&Value>) -> Option<u64> {
    match value {
        Some(Value::Number(value)) => value.as_u64(),
        Some(Value::String(value)) => parse_size_bytes(value),
        _ => None,
    }
}

fn parse_size_bytes(value: &str) -> Option<u64> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    if let Ok(bytes) = value.parse::<u64>() {
        return Some(bytes);
    }
    None
}

fn first_non_empty<const N: usize>(values: [&String; N]) -> String {
    values
        .into_iter()
        .map(|value| value.trim())
        .find(|value| !value.is_empty())
        .unwrap_or_default()
        .to_string()
}

trait TimestampExt {
    fn parse_timestamp_ms(self) -> Option<i64>;
}

impl TimestampExt for &str {
    fn parse_timestamp_ms(self) -> Option<i64> {
        parse_timestamp_ms(self)
    }
}

fn cursor_offset(cursor: Option<&str>) -> Result<usize> {
    match cursor.map(str::trim).filter(|value| !value.is_empty()) {
        Some(value) => value
            .parse::<usize>()
            .context("runtime inbox cursor must be a non-negative offset"),
        None => Ok(0),
    }
}

fn truncate_chars(value: &str, max_chars: usize) -> (String, bool) {
    let mut output = String::new();
    let mut truncated = false;
    for (index, ch) in value.chars().enumerate() {
        if index >= max_chars {
            truncated = true;
            break;
        }
        output.push(ch);
    }
    (output, truncated)
}

fn parse_timestamp_ms(value: &str) -> Option<i64> {
    let parsed =
        time::OffsetDateTime::parse(value, &time::format_description::well_known::Rfc3339).ok()?;
    i64::try_from(parsed.unix_timestamp_nanos() / 1_000_000).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamps_runtime_inbox_limit() {
        assert_eq!(clamp_limit(None, INBOX_DEFAULT_LIMIT).unwrap(), 30);
        assert_eq!(clamp_limit(Some(10), INBOX_DEFAULT_LIMIT).unwrap(), 10);
        assert_eq!(clamp_limit(Some(150), INBOX_DEFAULT_LIMIT).unwrap(), 100);
        assert!(clamp_limit(Some(0), INBOX_DEFAULT_LIMIT).is_err());
    }

    #[test]
    fn parses_thread_kind_from_thread_id() {
        assert_eq!(
            RuntimeInboxThreadKind::parse(None, "dm:peer-scope:v1:alice").unwrap(),
            RuntimeInboxThreadKind::Direct
        );
        assert_eq!(
            RuntimeInboxThreadKind::parse(None, "group:did:example:team").unwrap(),
            RuntimeInboxThreadKind::Group
        );
        assert!(RuntimeInboxThreadKind::parse(None, "direct:did:example:alice").is_err());
        assert!(RuntimeInboxThreadKind::parse(None, "dm:did:example:alice").is_err());
    }

    #[test]
    fn direct_thread_query_requires_stable_peer_scope_id() {
        let input = RuntimeInboxThreadQuery {
            runtime_agent_did: "did:agent:runtime".to_string(),
            thread_id: "dm:did:example:alice".to_string(),
            kind: RuntimeInboxThreadKind::Direct,
            peer_did: Some("did:example:alice".to_string()),
            peer_handle: Some("alice.anpclaw.com".to_string()),
            group_did: None,
            limit: 20,
            cursor: None,
        };

        assert!(conversation_id_from_thread_query(&input).is_err());
    }

    #[test]
    fn plain_json_metadata_content_type_does_not_create_attachment() {
        let message = LocalMessageRecord {
            msg_id: "msg-json".to_string(),
            direction: 0,
            sender_did: "did:human:alice".to_string(),
            receiver_did: "did:agent:runtime".to_string(),
            group_id: String::new(),
            group_did: String::new(),
            content_type: "application/json".to_string(),
            content: r#"{"text":"hello"}"#.to_string(),
            sent_at: "2026-06-04T10:00:00Z".to_string(),
            stored_at: "2026-06-04T10:00:00Z".to_string(),
            is_read: false,
            metadata: r#"{"content_type":"application/json","delivery_state":"accepted"}"#
                .to_string(),
        };

        assert_eq!(message_content_type(&message), "application/json");
        assert!(!message_has_attachments(&message));
        assert!(attachment_items(&message).is_empty());
        assert_eq!(message_preview(&message), "hello");
    }

    #[test]
    fn empty_attachment_manifest_without_summary_is_not_attachment() {
        let message = LocalMessageRecord {
            msg_id: "msg-empty-manifest".to_string(),
            direction: 0,
            sender_did: "did:human:alice".to_string(),
            receiver_did: "did:agent:runtime".to_string(),
            group_id: String::new(),
            group_did: String::new(),
            content_type: ATTACHMENT_MANIFEST_CONTENT_TYPE.to_string(),
            content: r#"{"attachments":[]}"#.to_string(),
            sent_at: "2026-06-04T10:00:00Z".to_string(),
            stored_at: "2026-06-04T10:00:00Z".to_string(),
            is_read: false,
            metadata: r#"{"has_attachments":true}"#.to_string(),
        };

        assert_eq!(
            message_content_type(&message),
            ATTACHMENT_MANIFEST_CONTENT_TYPE
        );
        assert!(!message_has_attachments(&message));
        assert!(attachment_items(&message).is_empty());
    }

    #[test]
    fn metadata_attachment_manifest_content_type_reads_payload_filename() {
        let message = LocalMessageRecord {
            msg_id: "msg-attachment".to_string(),
            direction: 0,
            sender_did: "did:human:alice".to_string(),
            receiver_did: "did:agent:runtime".to_string(),
            group_id: String::new(),
            group_did: String::new(),
            content_type: "application/json".to_string(),
            content: r#"{
                "attachments": [{
                    "attachment_id": "att-1",
                    "filename": "report.md",
                    "mime_type": "text/markdown",
                    "size": "42"
                }],
                "caption": "read this"
            }"#
            .to_string(),
            sent_at: "2026-06-04T10:00:00Z".to_string(),
            stored_at: "2026-06-04T10:00:00Z".to_string(),
            is_read: false,
            metadata: r#"{"content_type":"application/anp-attachment-manifest+json"}"#.to_string(),
        };

        let items = attachment_items(&message);
        assert_eq!(message_content_type(&message), "attachment");
        assert_eq!(message_preview(&message), "read this");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["attachment_id"], "att-1");
        assert_eq!(items[0]["filename"], "report.md");
        assert_eq!(items[0]["mime_type"], "text/markdown");
        assert_eq!(items[0]["size_bytes"], 42);
    }

    #[test]
    fn direct_conversation_without_peer_scope_is_not_listed() {
        let record = LocalConversationRecord {
            conversation_id: "dm:did:human:alice".to_string(),
            message_count: 1,
            unread_count: 1,
            last_message_at: "2026-06-04T10:00:00Z".to_string(),
            last_message: Some(LocalMessageRecord {
                msg_id: "msg-direct".to_string(),
                direction: 0,
                sender_did: "did:human:alice".to_string(),
                receiver_did: "did:agent:runtime".to_string(),
                group_id: String::new(),
                group_did: String::new(),
                content_type: "text/plain".to_string(),
                content: "hello runtime".to_string(),
                sent_at: "2026-06-04T10:00:00Z".to_string(),
                stored_at: "2026-06-04T10:00:00Z".to_string(),
                is_read: false,
                metadata: "{}".to_string(),
            }),
        };

        let item = inbox_item_json(&record, "did:agent:runtime").unwrap();

        assert!(item.is_none());
    }

    #[test]
    fn loads_scoped_conversations_from_messages_when_thread_projection_is_missing() {
        let root = tempfile::tempdir().unwrap();
        let sqlite_path = root.path().join("local-state.sqlite");
        let connection = rusqlite::Connection::open(&sqlite_path).unwrap();
        create_minimal_runtime_inbox_schema(&connection);
        let thread_id = scoped_direct_conversation_id(&DirectPeerScope {
            user_id: "user-alice".to_string(),
            full_handle: "alice.anpclaw.com".to_string(),
            current_did: Some("did:human:alice".to_string()),
        })
        .unwrap();
        connection
            .execute(
                r#"
INSERT INTO messages
    (msg_id, owner_identity_id, owner_did, conversation_id, thread_id, direction,
     sender_did, receiver_did, content_type, content, sent_at, stored_at, is_read, metadata)
VALUES
    (?1, ?2, ?3, ?4, ?4, 0, ?5, ?3, 'text/plain', 'hello runtime',
     '2026-06-04T10:00:00Z', '2026-06-04T10:00:00Z', 0, ?6)"#,
                rusqlite::params![
                    "msg-direct",
                    "runtime-owner",
                    "did:agent:runtime",
                    thread_id,
                    "did:human:alice",
                    r#"{
                        "peer_user_id": "user-alice",
                        "peer_full_handle": "alice.anpclaw.com",
                        "peer_current_did": "did:human:alice"
                    }"#,
                ],
            )
            .unwrap();

        let records = load_local_conversations(
            &sqlite_path,
            "runtime-owner",
            "did:agent:runtime",
            RuntimeInboxScope::All,
            30,
            0,
        )
        .unwrap();
        let items = records
            .iter()
            .filter_map(|record| {
                inbox_item_json(record, "did:agent:runtime")
                    .unwrap()
                    .map(|item| item["thread_id"].as_str().unwrap().to_string())
            })
            .collect::<Vec<_>>();

        assert_eq!(items, vec![thread_id]);
        assert_eq!(records[0].unread_count, 1);
    }

    #[test]
    fn normalizes_direct_inbox_records_to_stable_peer_scope_only() {
        let scoped_id = scoped_direct_conversation_id(&DirectPeerScope {
            user_id: "user-alice".to_string(),
            full_handle: "alice.anpclaw.com".to_string(),
            current_did: Some("did:human:alice".to_string()),
        })
        .unwrap();
        let legacy = LocalConversationRecord {
            conversation_id: "dm:did:human:alice".to_string(),
            message_count: 1,
            unread_count: 1,
            last_message_at: "2026-06-04T09:00:00Z".to_string(),
            last_message: Some(LocalMessageRecord {
                msg_id: "msg-legacy-direct".to_string(),
                direction: 0,
                sender_did: "did:human:alice".to_string(),
                receiver_did: "did:agent:runtime".to_string(),
                group_id: String::new(),
                group_did: String::new(),
                content_type: "text/plain".to_string(),
                content: "legacy hello".to_string(),
                sent_at: "2026-06-04T09:00:00Z".to_string(),
                stored_at: "2026-06-04T09:00:00Z".to_string(),
                is_read: false,
                metadata: r#"{
                    "peer_user_id": "user-alice",
                    "peer_full_handle": "alice.anpclaw.com",
                    "peer_current_did": "did:human:alice"
                }"#
                .to_string(),
            }),
        };
        let scoped = LocalConversationRecord {
            conversation_id: scoped_id.clone(),
            message_count: 2,
            unread_count: 1,
            last_message_at: "2026-06-04T10:00:00Z".to_string(),
            last_message: Some(LocalMessageRecord {
                msg_id: "msg-direct".to_string(),
                direction: 0,
                sender_did: "did:human:alice".to_string(),
                receiver_did: "did:agent:runtime".to_string(),
                group_id: String::new(),
                group_did: String::new(),
                content_type: "text/plain".to_string(),
                content: "hello runtime".to_string(),
                sent_at: "2026-06-04T10:00:00Z".to_string(),
                stored_at: "2026-06-04T10:00:00Z".to_string(),
                is_read: false,
                metadata: r#"{
                    "peer_user_id": "user-alice",
                    "peer_full_handle": "alice.anpclaw.com",
                    "peer_current_did": "did:human:alice"
                }"#
                .to_string(),
            }),
        };

        let records =
            normalize_runtime_inbox_conversation_records(vec![legacy, scoped], "did:agent:runtime")
                .unwrap();

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].message_count, 3);
        assert_eq!(records[0].unread_count, 2);
        let item = inbox_item_json(&records[0], "did:agent:runtime")
            .unwrap()
            .expect("stable scoped inbox item");
        assert_eq!(item["thread_id"], scoped_id);
        assert_eq!(item["title"], "alice.anpclaw.com");
        assert_eq!(item["peer_handle"], "alice.anpclaw.com");
        assert_eq!(item["peer_user_id"], "user-alice");
        assert_eq!(item["peer_did"], "did:human:alice");
    }

    fn create_minimal_runtime_inbox_schema(connection: &rusqlite::Connection) {
        connection
            .execute_batch(
                r#"
CREATE TABLE messages (
    msg_id TEXT NOT NULL,
    owner_identity_id TEXT,
    owner_did TEXT NOT NULL DEFAULT '',
    conversation_id TEXT,
    thread_id TEXT NOT NULL,
    direction INTEGER NOT NULL DEFAULT 0,
    sender_did TEXT,
    receiver_did TEXT,
    group_id TEXT,
    group_did TEXT,
    content_type TEXT DEFAULT 'text',
    content TEXT,
    sent_at TEXT,
    stored_at TEXT NOT NULL,
    is_read INTEGER DEFAULT 0,
    metadata TEXT,
    PRIMARY KEY (owner_identity_id, msg_id)
);
CREATE VIEW threads AS
SELECT
    owner_identity_id,
    owner_did,
    COALESCE(NULLIF(conversation_id, ''), thread_id) AS conversation_id,
    COALESCE(NULLIF(conversation_id, ''), thread_id) AS thread_id,
    COUNT(*) AS message_count,
    SUM(CASE WHEN is_read = 0 AND direction = 0 THEN 1 ELSE 0 END) AS unread_count,
    MAX(COALESCE(NULLIF(sent_at, ''), stored_at)) AS last_message_at
FROM messages
WHERE 1 = 0
GROUP BY owner_identity_id, COALESCE(NULLIF(conversation_id, ''), thread_id);
"#,
            )
            .unwrap();
    }
}
