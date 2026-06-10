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
    let _ = repair_scoped_direct_conversations(
        &client,
        &config.im_core_sqlite_path,
        client.current_identity().id.as_str(),
        &runtime_agent.agent_did,
    );
    let offset = cursor_offset(input.cursor.as_deref())?;
    let records = load_local_conversations(
        &config.im_core_sqlite_path,
        client.current_identity().id.as_str(),
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
    let _ = repair_scoped_direct_conversations(
        &client,
        &config.im_core_sqlite_path,
        client.current_identity().id.as_str(),
        &runtime_agent.agent_did,
    );
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
    metadata: String,
}

fn load_local_conversations(
    sqlite_path: &Path,
    owner_identity_id: &str,
    scope: RuntimeInboxScope,
    limit: u32,
    offset: usize,
) -> Result<Vec<LocalConversationRecord>> {
    let connection = rusqlite::Connection::open(sqlite_path)?;
    let mut statement = String::from(
        r#"
SELECT
    t.conversation_id,
    t.message_count,
    t.unread_count,
    t.last_message_at,
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
    m.metadata
FROM threads t
LEFT JOIN messages m
  ON m.owner_identity_id = t.owner_identity_id
 AND COALESCE(NULLIF(m.conversation_id, ''), m.thread_id) = t.conversation_id
 AND COALESCE(m.sent_at, m.stored_at) = t.last_message_at
 AND m.msg_id = (
     SELECT m2.msg_id
     FROM messages m2
     WHERE m2.owner_identity_id = t.owner_identity_id
       AND COALESCE(NULLIF(m2.conversation_id, ''), m2.thread_id) = t.conversation_id
       AND COALESCE(m2.sent_at, m2.stored_at) = t.last_message_at
     ORDER BY m2.msg_id DESC
     LIMIT 1
 )
WHERE t.owner_identity_id = ?1"#,
    );
    match scope {
        RuntimeInboxScope::All => {}
        RuntimeInboxScope::Direct => {
            statement.push_str(" AND t.conversation_id NOT LIKE 'group:%'")
        }
        RuntimeInboxScope::Group => statement.push_str(" AND t.conversation_id LIKE 'group:%'"),
    }
    statement.push_str(
        r#"
ORDER BY t.last_message_at DESC, t.conversation_id ASC
LIMIT ?2 OFFSET ?3"#,
    );
    let row_limit = i64::from(limit.min(MAX_LIMIT)) + 1;
    let row_offset = i64::try_from(offset).context("runtime inbox offset too large")?;
    let mut statement = connection.prepare(&statement)?;
    let rows = statement.query_map((owner_identity_id, row_limit, row_offset), |row| {
        let msg_id = optional_string(row, "msg_id")?;
        let last_message = if msg_id.trim().is_empty() {
            None
        } else {
            Some(LocalMessageRecord {
                msg_id,
                direction: row.get::<_, Option<i64>>("direction")?.unwrap_or_default(),
                sender_did: optional_string(row, "sender_did")?,
                receiver_did: optional_string(row, "receiver_did")?,
                group_id: optional_string(row, "group_id")?,
                group_did: optional_string(row, "group_did")?,
                content_type: optional_string(row, "content_type")?,
                content: optional_string(row, "content")?,
                sent_at: optional_string(row, "sent_at")?,
                stored_at: optional_string(row, "stored_at")?,
                metadata: optional_string(row, "metadata")?,
            })
        };
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
            last_message,
        })
    })?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
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
                metadata: optional_string(row, "metadata")?,
            })
        },
    )?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
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
    runtime_agent_did: &str,
) -> Result<()> {
    let mut connection = rusqlite::Connection::open(sqlite_path)?;
    let candidates = load_direct_repair_candidates(&connection, owner_identity_id)?;
    if candidates.is_empty() {
        return Ok(());
    }
    let mut lookup_cache = HashMap::<String, Option<DirectPeerScope>>::new();
    let transaction = connection.transaction()?;
    for candidate in candidates {
        let Some(peer_did) = candidate.peer_did(runtime_agent_did) else {
            continue;
        };
        let scope = candidate.metadata_scope.or_else(|| {
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
    if text.trim().is_empty() && message_has_attachments(message) {
        return "附件".to_string();
    }
    text
}

fn message_text(message: &LocalMessageRecord) -> (String, bool) {
    message_text_with_limit(message, TEXT_MAX_CHARS)
}

fn message_text_with_limit(message: &LocalMessageRecord, limit: usize) -> (String, bool) {
    if message.content_type.trim() == ATTACHMENT_MANIFEST_CONTENT_TYPE {
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
                .map(|item| {
                    json!({
                        "attachment_id": string_field(item.get("attachment_id")),
                        "filename": string_field(item.get("filename")),
                        "mime_type": string_field(item.get("mime_type")),
                        "size_bytes": attachment_size_bytes(item),
                        "download_state": "available",
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn attachment_payload(message: &LocalMessageRecord) -> Option<Value> {
    if message.content_type.trim() != ATTACHMENT_MANIFEST_CONTENT_TYPE {
        return None;
    }
    serde_json::from_str::<Value>(&message.content).ok()
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

fn string_field(value: Option<&Value>) -> Value {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| json!(value))
        .unwrap_or(Value::Null)
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
}
