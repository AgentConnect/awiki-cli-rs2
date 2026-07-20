use anyhow::{Context, Result};
use im_core::ids::PageLimit;
use im_core::messages::{HistoryQuery, InboxScope};
use serde_json::{json, Value};

use crate::agent::AgentDefinition;
use crate::im_core_adapter::ImCoreAdapter;
use crate::state::DaemonState;
use crate::DaemonConfig;

mod local_store;
mod projection;

use local_store::{load_local_conversations, load_local_messages, mark_local_conversation_read};
use projection::{
    conversation_id_from_thread_query, cursor_offset, display_fallback_json, inbox_item_json,
    message_json, repair_scoped_direct_conversations, thread_group_did, thread_peer_did,
    thread_ref_from_query, thread_title_from_records,
};

pub use projection::repair_runtime_controller_inbox_projection;

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
                inbox_history_options: None,
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
    let peer_did = thread_peer_did(&input, &records);
    let group_did = thread_group_did(&input, &records);
    let messages = records
        .iter()
        .map(|message| message_json(message, &runtime_agent.agent_did))
        .collect::<Result<Vec<_>>>()?;
    mark_local_conversation_read(
        &config.im_core_sqlite_path,
        client.current_identity().id.as_str(),
        &conversation_id,
    )
    .context("mark runtime inbox thread read")?;
    let next_offset = offset + messages.len();
    Ok(json!({
        "thread_id": input.thread_id,
        "kind": input.kind.as_str(),
        "title": title,
        "display": display_fallback_json(&title),
        "peer_did": peer_did,
        "group_did": group_did,
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
    let inbox_limit = limit.clamp(INBOX_DEFAULT_LIMIT, MAX_LIMIT);
    client.messages().inbox(im_core::messages::InboxQuery {
        scope: scope.inbox_scope(),
        limit: PageLimit(inbox_limit),
        cursor: None,
        unread_only: false,
        inbox_history_options: None,
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::local_store::{LocalConversationRecord, LocalMessageRecord};
    use super::projection::{
        attachment_items, message_content_type, message_has_attachments, message_preview,
        normalize_runtime_inbox_conversation_records, scoped_direct_conversation_id,
        DirectPeerScope,
    };
    use super::*;

    #[test]
    fn clamps_runtime_inbox_limit() {
        assert_eq!(clamp_limit(None, 30).unwrap(), 30);
        assert_eq!(clamp_limit(Some(10), 30).unwrap(), 10);
        assert_eq!(clamp_limit(Some(150), 30).unwrap(), 100);
        assert!(clamp_limit(Some(0), 30).is_err());
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
    fn marks_runtime_inbox_thread_conversation_read() {
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
        for (msg_id, direction, is_read) in [
            ("msg-unread-1", 0, 0),
            ("msg-unread-2", 0, 0),
            ("msg-outgoing", 1, 0),
            ("msg-read", 0, 1),
        ] {
            connection
                .execute(
                    r#"
INSERT INTO messages
    (msg_id, owner_identity_id, owner_did, conversation_id, thread_id, direction,
     sender_did, receiver_did, content_type, content, sent_at, stored_at, is_read, metadata)
VALUES
    (?1, ?2, ?3, ?4, ?4, ?5, ?6, ?3, 'text/plain', 'hello runtime',
     '2026-06-04T10:00:00Z', '2026-06-04T10:00:00Z', ?7, ?8)"#,
                    rusqlite::params![
                        msg_id,
                        "runtime-owner",
                        "did:agent:runtime",
                        thread_id,
                        direction,
                        "did:human:alice",
                        is_read,
                        r#"{
                            "peer_user_id": "user-alice",
                            "peer_full_handle": "alice.anpclaw.com",
                            "peer_current_did": "did:human:alice"
                        }"#,
                    ],
                )
                .unwrap();
        }

        assert_eq!(
            mark_local_conversation_read(&sqlite_path, "runtime-owner", &thread_id).unwrap(),
            2
        );
        let unread_count: i64 = connection
            .query_row(
                r#"
SELECT COUNT(*)
FROM messages
WHERE owner_identity_id = ?1
  AND conversation_id = ?2
  AND direction = 0
  AND COALESCE(is_read, 0) = 0"#,
                rusqlite::params!["runtime-owner", thread_id],
                |row| row.get(0),
            )
            .unwrap();
        let outgoing_read: i64 = connection
            .query_row(
                "SELECT is_read FROM messages WHERE owner_identity_id = ?1 AND msg_id = ?2",
                rusqlite::params!["runtime-owner", "msg-outgoing"],
                |row| row.get(0),
            )
            .unwrap();

        assert_eq!(unread_count, 0);
        assert_eq!(outgoing_read, 0);
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
