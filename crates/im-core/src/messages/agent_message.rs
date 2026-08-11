use serde::Deserialize;

use super::SendMessageRequest;

pub const AGENT_MESSAGE_SCHEMA_V1: &str = "awiki.agent.message.v1";
pub const AGENT_MESSAGE_V1_MAX_COMPACT_BYTES: usize = 8 * 1024;
pub const AGENT_MESSAGE_V1_MAX_TASK_NAME_CHARS: usize = 120;
pub const AGENT_MESSAGE_V1_MAX_SUMMARY_CHARS: usize = 240;
pub const AGENT_MESSAGE_V1_MAX_DETAIL_CHARS: usize = 2_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentMessageKind {
    Message,
    TaskResult,
    Alert,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentMessageRequestedLevel {
    Normal,
    Urgent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentMessageAction {
    OpenConversation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentMessageV1 {
    pub event_id: String,
    pub task_name: String,
    pub kind: AgentMessageKind,
    /// Business intent only. Hosts must not map this directly to platform priority or sound.
    pub requested_level: AgentMessageRequestedLevel,
    pub summary: String,
    pub detail: Option<String>,
    pub action: AgentMessageAction,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentMessageProjection {
    Valid(AgentMessageV1),
    /// Exact visible schema, but malformed, oversized, or unsafe. No raw data is retained here.
    Invalid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentMessageProjectionScope {
    /// Direct route whose decrypted/E2EE provenance has been excluded by the
    /// authoritative Core/bridge message context.
    DirectTransportProtected,
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MessagePayloadProjection {
    Ordinary,
    VisibleValid,
    VisibleInvalid,
    Control,
}

impl MessagePayloadProjection {
    pub(crate) fn is_control(self) -> bool {
        matches!(self, Self::Control)
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawAgentMessageV1 {
    schema: String,
    event_id: String,
    task_name: String,
    kind: RawAgentMessageKind,
    level: RawAgentMessageLevel,
    content: RawAgentMessageContent,
    action: RawAgentMessageAction,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum RawAgentMessageKind {
    Message,
    TaskResult,
    Alert,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum RawAgentMessageLevel {
    Normal,
    Urgent,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawAgentMessageContent {
    summary: String,
    #[serde(default)]
    detail: StrictOptionalString,
}

#[derive(Debug, Default)]
enum StrictOptionalString {
    #[default]
    Missing,
    Present(String),
}

impl<'de> Deserialize<'de> for StrictOptionalString {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        value
            .as_str()
            .map(|value| Self::Present(value.to_owned()))
            .ok_or_else(|| serde::de::Error::custom("detail must be a string when present"))
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawAgentMessageAction {
    #[serde(rename = "type")]
    kind: RawAgentMessageActionKind,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum RawAgentMessageActionKind {
    OpenConversation,
}

/// Classifies only the exact visible schema. `None` means this is not
/// `awiki.agent.message.v1`; callers may continue their ordinary/control rules.
pub fn project_agent_message_payload(
    payload: &serde_json::Value,
) -> Option<AgentMessageProjection> {
    let object = payload.as_object()?;
    if object.get("schema").and_then(serde_json::Value::as_str) != Some(AGENT_MESSAGE_SCHEMA_V1) {
        return None;
    }
    Some(match decode_agent_message_v1(payload) {
        Some(message) => AgentMessageProjection::Valid(message),
        None => AgentMessageProjection::Invalid,
    })
}

/// Applies the MVP transport-protected Direct-only scope policy in Core.
/// Exact-schema Group, E2EE, or unverified raw-thread messages remain visible
/// only as a generic invalid placeholder and can never carry urgent/card
/// fields across the SDK boundary.
pub fn project_agent_message_payload_for_scope(
    payload: &serde_json::Value,
    scope: AgentMessageProjectionScope,
) -> Option<AgentMessageProjection> {
    Some(match project_agent_message_payload(payload)? {
        AgentMessageProjection::Valid(_)
            if scope != AgentMessageProjectionScope::DirectTransportProtected =>
        {
            AgentMessageProjection::Invalid
        }
        projection => projection,
    })
}

/// Canonical local preflight shared by Core send and CLI dry-run. Receiver
/// capability and urgent authorization are intentionally absent from this
/// boundary: the Receiving Home owns the authoritative capability gate.
pub fn validate_agent_message_send_request(request: &SendMessageRequest) -> crate::ImResult<()> {
    let super::MessageBody::Payload { payload } = &request.body else {
        return Ok(());
    };
    let Some(projection) = project_agent_message_payload(payload) else {
        return Ok(());
    };
    let AgentMessageProjection::Valid(_message) = projection else {
        return Err(crate::ImError::invalid_input(
            Some("payload".to_owned()),
            "awiki.agent.message.v1 payload is malformed, oversized, or unsafe",
        ));
    };
    if !matches!(request.target, super::MessageTarget::Direct(_)) {
        return Err(crate::ImError::unsupported("agent_message_direct_only"));
    }
    if !matches!(
        request.security,
        super::MessageSecurityMode::DefaultPlain | super::MessageSecurityMode::Plain
    ) {
        return Err(crate::ImError::unsupported(
            "agent_message_transport_protected_only",
        ));
    }
    if request.client_message_id.is_none() {
        return Err(crate::ImError::invalid_input(
            Some("client_message_id".to_owned()),
            "awiki.agent.message.v1 requires a stable client_message_id",
        ));
    }
    if request
        .delivery
        .idempotency_key
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_none()
    {
        return Err(crate::ImError::invalid_input(
            Some("idempotency_key".to_owned()),
            "awiki.agent.message.v1 requires a stable idempotency_key",
        ));
    }
    Ok(())
}

pub(crate) fn classify_message_payload_for_projection(
    content_type: &str,
    content: &str,
    sender_did: &str,
) -> MessagePayloadProjection {
    if !content_type.trim().eq_ignore_ascii_case("application/json") {
        return MessagePayloadProjection::Ordinary;
    }
    let content = content.trim();
    if content.is_empty() {
        return MessagePayloadProjection::Control;
    }
    let Ok(value) = serde_json::from_str::<serde_json::Value>(content) else {
        return MessagePayloadProjection::Ordinary;
    };
    match project_agent_message_payload(&value) {
        Some(AgentMessageProjection::Valid(_)) => {
            return MessagePayloadProjection::VisibleValid;
        }
        Some(AgentMessageProjection::Invalid) => {
            return MessagePayloadProjection::VisibleInvalid;
        }
        None => {}
    }
    let is_awiki_schema = value
        .as_object()
        .and_then(|object| object.get("schema"))
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .map(|schema| schema.starts_with("awiki."))
        .unwrap_or(false);
    if is_awiki_schema
        || (is_daemon_control_sender(sender_did) && is_daemon_control_payload_value(&value))
    {
        MessagePayloadProjection::Control
    } else {
        MessagePayloadProjection::Ordinary
    }
}

pub(crate) fn sanitize_projected_json_payload(payload: serde_json::Value) -> serde_json::Value {
    match project_agent_message_payload(&payload) {
        Some(AgentMessageProjection::Valid(message)) => message.to_payload_value(),
        Some(AgentMessageProjection::Invalid) => {
            serde_json::json!({ "schema": AGENT_MESSAGE_SCHEMA_V1 })
        }
        None => payload,
    }
}

impl AgentMessageV1 {
    pub(crate) fn to_payload_value(&self) -> serde_json::Value {
        let kind = match self.kind {
            AgentMessageKind::Message => "message",
            AgentMessageKind::TaskResult => "task_result",
            AgentMessageKind::Alert => "alert",
        };
        let level = match self.requested_level {
            AgentMessageRequestedLevel::Normal => "normal",
            AgentMessageRequestedLevel::Urgent => "urgent",
        };
        let action = match self.action {
            AgentMessageAction::OpenConversation => "open_conversation",
        };
        let mut content = serde_json::Map::from_iter([(
            "summary".to_owned(),
            serde_json::Value::String(self.summary.clone()),
        )]);
        if let Some(detail) = &self.detail {
            content.insert(
                "detail".to_owned(),
                serde_json::Value::String(detail.clone()),
            );
        }
        serde_json::json!({
            "schema": AGENT_MESSAGE_SCHEMA_V1,
            "event_id": self.event_id,
            "task_name": self.task_name,
            "kind": kind,
            "level": level,
            "content": content,
            "action": { "type": action },
        })
    }
}

fn decode_agent_message_v1(payload: &serde_json::Value) -> Option<AgentMessageV1> {
    if serde_json::to_vec(payload).ok()?.len() > AGENT_MESSAGE_V1_MAX_COMPACT_BYTES {
        return None;
    }
    let raw = serde_json::from_value::<RawAgentMessageV1>(payload.clone()).ok()?;
    if raw.schema != AGENT_MESSAGE_SCHEMA_V1
        || !valid_event_id(&raw.event_id)
        || !safe_text(&raw.task_name, AGENT_MESSAGE_V1_MAX_TASK_NAME_CHARS, false)
        || !safe_text(
            &raw.content.summary,
            AGENT_MESSAGE_V1_MAX_SUMMARY_CHARS,
            false,
        )
        || matches!(
            &raw.content.detail,
            StrictOptionalString::Present(detail)
                if !safe_text(detail, AGENT_MESSAGE_V1_MAX_DETAIL_CHARS, true)
        )
    {
        return None;
    }
    Some(AgentMessageV1 {
        event_id: raw.event_id,
        task_name: raw.task_name,
        kind: match raw.kind {
            RawAgentMessageKind::Message => AgentMessageKind::Message,
            RawAgentMessageKind::TaskResult => AgentMessageKind::TaskResult,
            RawAgentMessageKind::Alert => AgentMessageKind::Alert,
        },
        requested_level: match raw.level {
            RawAgentMessageLevel::Normal => AgentMessageRequestedLevel::Normal,
            RawAgentMessageLevel::Urgent => AgentMessageRequestedLevel::Urgent,
        },
        summary: raw.content.summary,
        detail: match raw.content.detail {
            StrictOptionalString::Missing => None,
            StrictOptionalString::Present(detail) => Some(detail),
        },
        action: match raw.action.kind {
            RawAgentMessageActionKind::OpenConversation => AgentMessageAction::OpenConversation,
        },
    })
}

fn valid_event_id(value: &str) -> bool {
    let bytes = value.as_bytes();
    (8..=160).contains(&bytes.len())
        && bytes.first().is_some_and(u8::is_ascii_alphanumeric)
        && bytes
            .iter()
            .skip(1)
            .all(|byte| byte.is_ascii_alphanumeric() || b"._:-".contains(byte))
}

fn safe_text(value: &str, max_chars: usize, allow_newline: bool) -> bool {
    if value.is_empty()
        || value.trim() != value
        || value.chars().count() > max_chars
        || value
            .chars()
            .any(|character| unsafe_character(character, allow_newline))
    {
        return false;
    }
    let lower = value.to_ascii_lowercase();
    ![
        "```",
        "file://",
        "blob:",
        "authorization: bearer",
        "-----begin",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
        && ![
            "password=",
            "password:",
            "token=",
            "token:",
            "secret=",
            "secret:",
            "private_key",
            "private key",
            "api_key",
            "apikey",
            "access_key",
            "akia",
        ]
        .iter()
        .any(|needle| lower.contains(needle))
        && !value.split_whitespace().any(looks_like_absolute_path)
}

fn unsafe_character(character: char, allow_newline: bool) -> bool {
    if allow_newline && character == '\n' {
        return false;
    }
    character.is_control()
        || matches!(
            character,
            '\u{061c}'
                | '\u{00ad}'
                | '\u{034f}'
                | '\u{200e}'
                | '\u{200f}'
                | '\u{200b}'..='\u{200d}'
                | '\u{202a}'..='\u{202e}'
                | '\u{2060}'
                | '\u{2066}'..='\u{2069}'
                | '\u{feff}'
        )
}

fn looks_like_absolute_path(token: &str) -> bool {
    let token = token.trim_matches(|character: char| {
        matches!(character, '(' | ')' | '[' | ']' | '{' | '}' | ',' | ';')
    });
    token.starts_with('/')
        || token.starts_with("\\\\")
        || (token.len() >= 3
            && token.as_bytes()[0].is_ascii_alphabetic()
            && token.as_bytes()[1] == b':'
            && matches!(token.as_bytes()[2], b'\\' | b'/'))
}

fn is_daemon_control_sender(sender_did: &str) -> bool {
    sender_did.trim().contains(":agent:daemon:")
}

fn is_daemon_control_payload_value(value: &serde_json::Value) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    object.contains_key("daemon")
        || object.contains_key("runtimes")
        || object.contains_key("command_id")
        || object.contains_key("events")
        || object
            .get("command")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .map(|command| command.starts_with("agent."))
            .unwrap_or(false)
}
