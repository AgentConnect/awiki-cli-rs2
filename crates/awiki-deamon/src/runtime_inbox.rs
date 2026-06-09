use anyhow::{Context, Result};
use im_core::ids::{GroupRef, PageLimit, PeerRef};
use im_core::messages::{HistoryQuery, InboxScope, ThreadRef};
use serde_json::{json, Value};
use std::path::Path;

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
    let items = records
        .into_iter()
        .take(input.limit as usize)
        .map(|conversation| inbox_item_json(&conversation, &runtime_agent.agent_did))
        .collect::<Result<Vec<_>>>()?;
    let next_offset = offset + items.len();
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
    let thread = thread_ref_from_query(&input)?;
    let _ = client.messages().history(
        thread,
        HistoryQuery {
            limit: PageLimit(input.limit.min(MAX_LIMIT)),
            cursor: None,
            inbox_history_options: None,
        },
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
        } else if thread_id.trim().starts_with("direct:")
            || thread_id.trim().starts_with("dm:")
            || thread_id.trim().starts_with("did:")
        {
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
        inbox_history_options: None,
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
    m.stored_at
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
    stored_at
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
) -> Result<Value> {
    let (kind, peer_did, group_id, group_did) =
        conversation_fields(conversation, runtime_agent_did);
    let thread_id = thread_id_from_fields(kind, peer_did.as_deref(), group_did.as_deref());
    let last_message = conversation.last_message.as_ref();
    let preview = last_message
        .map(message_preview)
        .unwrap_or_else(|| truncate_chars("", PREVIEW_MAX_CHARS).0);
    let last_content_type = last_message
        .map(message_content_type)
        .unwrap_or_else(|| "text".to_string());
    Ok(json!({
        "thread_id": thread_id,
        "kind": kind,
        "title": conversation_title(conversation, peer_did.as_deref(), group_did.as_deref()),
        "peer_did": peer_did,
        "group_id": group_id,
        "group_did": group_did,
        "last_message_preview": preview,
        "last_message_at_ms": conversation
            .last_message_at
            .as_str()
            .parse_timestamp_ms(),
        "unread_count": conversation.unread_count,
        "has_attachments": last_message.is_some_and(message_has_attachments),
        "last_content_type": last_content_type,
    }))
}

fn conversation_fields(
    conversation: &LocalConversationRecord,
    runtime_agent_did: &str,
) -> (&'static str, Option<String>, Option<String>, Option<String>) {
    if let Some(group) = conversation.conversation_id.strip_prefix("group:") {
        let group_did = conversation
            .last_message
            .as_ref()
            .map(|message| first_non_empty([&message.group_did, &message.group_id]))
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| group.to_string());
        return ("group", None, Some(group_did.clone()), Some(group_did));
    }
    let peer = conversation
        .last_message
        .as_ref()
        .map(|message| {
            if message.sender_did.trim() != runtime_agent_did {
                message.sender_did.clone()
            } else {
                message.receiver_did.clone()
            }
        })
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            conversation
                .conversation_id
                .strip_prefix("direct:")
                .or_else(|| conversation.conversation_id.strip_prefix("dm:"))
                .map(str::to_string)
        })
        .unwrap_or_else(|| conversation.conversation_id.clone());
    ("direct", Some(peer), None, None)
}

fn thread_id_from_fields(kind: &str, peer_did: Option<&str>, group_did: Option<&str>) -> String {
    if kind == "group" {
        return format!("group:{}", group_did.unwrap_or_default());
    }
    format!("direct:{}", peer_did.unwrap_or_default())
}

fn conversation_title(
    conversation: &LocalConversationRecord,
    peer_did: Option<&str>,
    group_did: Option<&str>,
) -> String {
    peer_did
        .map(str::to_string)
        .or_else(|| group_did.map(str::to_string))
        .or_else(|| Some(conversation.conversation_id.clone()))
        .unwrap_or_else(|| "Unknown".to_string())
}

fn thread_ref_from_query(input: &RuntimeInboxThreadQuery) -> Result<ThreadRef> {
    match input.kind {
        RuntimeInboxThreadKind::Direct => {
            let peer = input
                .peer_did
                .as_deref()
                .or_else(|| input.thread_id.strip_prefix("direct:"))
                .or_else(|| input.thread_id.strip_prefix("dm:"))
                .or_else(|| {
                    input
                        .thread_id
                        .starts_with("did:")
                        .then_some(input.thread_id.as_str())
                })
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .context("direct runtime inbox thread requires peer_did")?;
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
            let peer = input
                .peer_did
                .as_deref()
                .or_else(|| input.thread_id.strip_prefix("direct:"))
                .or_else(|| input.thread_id.strip_prefix("dm:"))
                .or_else(|| {
                    input
                        .thread_id
                        .starts_with("did:")
                        .then_some(input.thread_id.as_str())
                })
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .context("direct runtime inbox thread requires peer_did")?;
            Ok(format!("dm:{peer}"))
        }
        RuntimeInboxThreadKind::Group => {
            let group = input
                .group_did
                .as_deref()
                .or_else(|| input.thread_id.strip_prefix("group:"))
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
    for message in messages {
        match input.kind {
            RuntimeInboxThreadKind::Direct => {
                let candidate = if message.sender_did.trim() != input.runtime_agent_did {
                    message.sender_did.as_str()
                } else {
                    message.receiver_did.as_str()
                };
                if !candidate.trim().is_empty() {
                    return candidate.to_string();
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
            .peer_did
            .clone()
            .or_else(|| input.thread_id.strip_prefix("direct:").map(str::to_string))
            .unwrap_or_else(|| input.thread_id.clone()),
        RuntimeInboxThreadKind::Group => input
            .group_did
            .clone()
            .or_else(|| input.thread_id.strip_prefix("group:").map(str::to_string))
            .unwrap_or_else(|| input.thread_id.clone()),
    }
}

fn message_json(message: &LocalMessageRecord, runtime_agent_did: &str) -> Result<Value> {
    let (text, truncated) = message_text(message);
    Ok(json!({
        "message_id": message.msg_id,
        "sender_did": message.sender_did,
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
            RuntimeInboxThreadKind::parse(None, "direct:did:example:alice").unwrap(),
            RuntimeInboxThreadKind::Direct
        );
        assert_eq!(
            RuntimeInboxThreadKind::parse(None, "group:did:example:team").unwrap(),
            RuntimeInboxThreadKind::Group
        );
    }
}
