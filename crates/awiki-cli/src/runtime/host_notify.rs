use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use time::OffsetDateTime;

pub const HOST_NOTIFICATION_VERSION: &str = "1.0";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostNotificationEvent {
    pub version: String,
    pub id: String,
    pub topic: String,
    pub received_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<HostNotificationData>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum HostNotificationData {
    Direct(DirectMessageNotificationData),
    Group(GroupMessageNotificationData),
    GroupState(GroupStateChangedNotificationData),
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirectMessageNotificationData {
    pub channel: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub source_kind: String,
    pub message_id: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub operation_id: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub conversation_id: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub sender_handle: String,
    pub sender_did: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub recipient_handle: String,
    pub recipient_did: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub profile: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub security_profile: String,
    pub content_type: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub text: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub created_at: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub mailbox_address: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub mailbox_did: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub from_addr: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub subject: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub preview: String,
    #[serde(skip_serializing_if = "is_false")]
    pub has_attachments: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupMessageNotificationData {
    pub channel: String,
    pub message_id: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub operation_id: String,
    pub group_did: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub sender_handle: String,
    pub sender_did: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub recipient_handle: String,
    pub recipient_did: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub profile: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub security_profile: String,
    pub content_type: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub text: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub group_state_version: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub group_event_seq: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub accepted_at: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupStateChangedNotificationData {
    pub channel: String,
    pub event_id: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub event_type: String,
    pub group_did: String,
    pub recipient_did: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub actor_did: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub subject_did: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub subject_method: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub membership_status: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub group_state_version: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub group_event_seq: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub changed_at: String,
}

pub fn normalize_host_notification(
    notification: &Value,
    received_at: Option<OffsetDateTime>,
) -> Option<HostNotificationEvent> {
    let received_at = normalize_received_at(received_at);
    match string_value(notification.get("method")).as_str() {
        "direct.incoming" => normalize_direct_incoming(notification, received_at),
        "mail.notification" => normalize_mail_notification(notification, received_at),
        "group.incoming" => normalize_group_incoming(notification, received_at),
        "group.state_changed" => normalize_group_state_changed(notification, received_at),
        _ => None,
    }
}

pub fn apply_host_notification_handles(
    event: &mut HostNotificationEvent,
    sender_handle: &str,
    recipient_handle: &str,
) {
    let Some(data) = event.data.as_mut() else {
        return;
    };
    match data {
        HostNotificationData::Direct(data) => {
            data.sender_handle =
                fallback_string(sender_handle.trim().to_string(), &data.sender_handle);
            data.recipient_handle =
                fallback_string(recipient_handle.trim().to_string(), &data.recipient_handle);
        }
        HostNotificationData::Group(data) => {
            data.sender_handle =
                fallback_string(sender_handle.trim().to_string(), &data.sender_handle);
            data.recipient_handle =
                fallback_string(recipient_handle.trim().to_string(), &data.recipient_handle);
        }
        HostNotificationData::GroupState(_) => {}
    }
}

fn normalize_direct_incoming(
    notification: &Value,
    received_at: OffsetDateTime,
) -> Option<HostNotificationEvent> {
    let params = map_value(notification.get("params"));
    let meta = map_value(value_from_object(params, "meta"));
    let body = map_value(value_from_object(params, "body"));
    let target = map_value(value_from_object(meta, "target"));
    let recipient_did = string_from_object(target, "did");
    let sender_did = string_from_object(meta, "sender_did");
    if recipient_did.is_empty() || sender_did.is_empty() {
        return None;
    }
    let message_id = resolve_direct_message_id(meta, notification);
    Some(HostNotificationEvent {
        version: HOST_NOTIFICATION_VERSION.to_string(),
        id: message_id.clone(),
        topic: "im.message.received".to_string(),
        received_at: format_go_rfc3339(received_at),
        data: Some(HostNotificationData::Direct(
            DirectMessageNotificationData {
                channel: "direct".to_string(),
                source_kind: "im".to_string(),
                message_id,
                operation_id: string_from_object(meta, "operation_id"),
                conversation_id: string_from_object(body, "conversation_id"),
                sender_did,
                recipient_did,
                profile: string_from_object(meta, "profile"),
                security_profile: string_from_object(meta, "security_profile"),
                content_type: fallback_string(
                    string_from_object(meta, "content_type"),
                    "text/plain",
                ),
                text: string_from_object(body, "text"),
                created_at: string_from_object(meta, "created_at"),
                ..DirectMessageNotificationData::default()
            },
        )),
    })
}

fn normalize_group_incoming(
    notification: &Value,
    received_at: OffsetDateTime,
) -> Option<HostNotificationEvent> {
    let params = map_value(notification.get("params"));
    let meta = map_value(value_from_object(params, "meta"));
    let body = map_value(value_from_object(params, "body"));
    let target = map_value(value_from_object(meta, "target"));
    let recipient_did = string_from_object(target, "did");
    let group_did = string_from_object(body, "group_did");
    let sender_did = string_from_object(meta, "sender_did");
    if recipient_did.is_empty() || group_did.is_empty() || sender_did.is_empty() {
        return None;
    }
    let message_id = resolve_group_message_id(meta, body, notification);
    Some(HostNotificationEvent {
        version: HOST_NOTIFICATION_VERSION.to_string(),
        id: message_id.clone(),
        topic: "im.group.message.received".to_string(),
        received_at: format_go_rfc3339(received_at),
        data: Some(HostNotificationData::Group(GroupMessageNotificationData {
            channel: "group".to_string(),
            message_id,
            operation_id: string_from_object(meta, "operation_id"),
            group_did,
            sender_did,
            recipient_did,
            profile: string_from_object(meta, "profile"),
            security_profile: string_from_object(meta, "security_profile"),
            content_type: fallback_string(string_from_object(meta, "content_type"), "text/plain"),
            text: string_from_object(body, "text"),
            group_state_version: string_like_value(value_from_object(body, "group_state_version")),
            group_event_seq: string_like_value(value_from_object(body, "group_event_seq")),
            accepted_at: string_from_object(body, "accepted_at"),
            ..GroupMessageNotificationData::default()
        })),
    })
}

fn normalize_group_state_changed(
    notification: &Value,
    received_at: OffsetDateTime,
) -> Option<HostNotificationEvent> {
    let params = map_value(notification.get("params"));
    let meta = map_value(value_from_object(params, "meta"));
    let body = map_value(value_from_object(params, "body"));
    let target = map_value(value_from_object(meta, "target"));
    let recipient_did = string_from_object(target, "did");
    let group_did = string_from_object(body, "group_did");
    if recipient_did.is_empty() || group_did.is_empty() {
        return None;
    }
    let event_id = resolve_group_state_event_id(meta, body, notification);
    Some(HostNotificationEvent {
        version: HOST_NOTIFICATION_VERSION.to_string(),
        id: event_id.clone(),
        topic: "im.group.state.changed".to_string(),
        received_at: format_go_rfc3339(received_at),
        data: Some(HostNotificationData::GroupState(
            GroupStateChangedNotificationData {
                channel: "group".to_string(),
                event_id,
                event_type: fallback_string(
                    string_from_object(body, "event_type"),
                    &infer_group_state_event_type(body),
                ),
                group_did,
                recipient_did,
                actor_did: string_from_object(body, "actor_did"),
                subject_did: string_from_object(body, "subject_did"),
                subject_method: string_from_object(body, "subject_method"),
                membership_status: string_from_object(body, "membership_status"),
                group_state_version: string_like_value(value_from_object(
                    body,
                    "group_state_version",
                )),
                group_event_seq: string_like_value(value_from_object(body, "group_event_seq")),
                changed_at: string_from_object(body, "changed_at"),
            },
        )),
    })
}

fn normalize_mail_notification(
    notification: &Value,
    received_at: OffsetDateTime,
) -> Option<HostNotificationEvent> {
    let params = map_value(notification.get("params"));
    let mailbox_did = string_from_object(params, "mailbox_did");
    if mailbox_did.is_empty() {
        return None;
    }
    let message_id = fallback_string(
        string_from_object(params, "message_id"),
        &generated_host_notification_id(notification),
    );
    let subject = string_from_object(params, "subject");
    let preview = string_from_object(params, "preview");
    let has_attachments = bool_value(value_from_object(params, "has_attachments"));
    Some(HostNotificationEvent {
        version: HOST_NOTIFICATION_VERSION.to_string(),
        id: message_id.clone(),
        topic: "im.message.received".to_string(),
        received_at: format_go_rfc3339(received_at),
        data: Some(HostNotificationData::Direct(
            DirectMessageNotificationData {
                channel: "mail".to_string(),
                source_kind: "mail".to_string(),
                message_id,
                recipient_did: mailbox_did.clone(),
                content_type: "mail.notification".to_string(),
                text: build_mail_notification_event_text(&subject, &preview, has_attachments),
                mailbox_address: string_from_object(params, "mailbox_address"),
                mailbox_did,
                from_addr: string_from_object(params, "from_addr"),
                subject,
                preview,
                has_attachments,
                ..DirectMessageNotificationData::default()
            },
        )),
    })
}

fn normalize_received_at(received_at: Option<OffsetDateTime>) -> OffsetDateTime {
    received_at
        .unwrap_or_else(OffsetDateTime::now_utc)
        .to_offset(time::UtcOffset::UTC)
}

fn resolve_direct_message_id(meta: Option<&Map<String, Value>>, notification: &Value) -> String {
    let message_id = string_from_object(meta, "message_id");
    if !message_id.is_empty() {
        return message_id;
    }
    let operation_id = string_from_object(meta, "operation_id");
    if !operation_id.is_empty() {
        return operation_id;
    }
    generated_host_notification_id(notification)
}

fn resolve_group_message_id(
    meta: Option<&Map<String, Value>>,
    body: Option<&Map<String, Value>>,
    notification: &Value,
) -> String {
    let message_id = string_from_object(meta, "message_id");
    if !message_id.is_empty() {
        return message_id;
    }
    let group_did = string_from_object(body, "group_did");
    let group_event_seq = string_like_value(value_from_object(body, "group_event_seq"));
    if !group_did.is_empty() && !group_event_seq.is_empty() {
        return format!("{group_did}:{group_event_seq}");
    }
    let operation_id = string_from_object(meta, "operation_id");
    if !operation_id.is_empty() {
        return operation_id;
    }
    generated_host_notification_id(notification)
}

fn resolve_group_state_event_id(
    meta: Option<&Map<String, Value>>,
    body: Option<&Map<String, Value>>,
    notification: &Value,
) -> String {
    let event_id = string_from_object(body, "event_id");
    if !event_id.is_empty() {
        return event_id;
    }
    let group_did = string_from_object(body, "group_did");
    let group_event_seq = string_like_value(value_from_object(body, "group_event_seq"));
    if !group_did.is_empty() && !group_event_seq.is_empty() {
        return format!("{group_did}:{group_event_seq}");
    }
    let operation_id = string_from_object(meta, "operation_id");
    if !operation_id.is_empty() {
        return operation_id;
    }
    generated_host_notification_id(notification)
}

fn infer_group_state_event_type(body: Option<&Map<String, Value>>) -> String {
    match string_from_object(body, "membership_status").as_str() {
        "active" | "activated" => return "member-activated".to_string(),
        "removed" => return "member-removed".to_string(),
        "left" => return "member-left".to_string(),
        _ => {}
    }
    match string_from_object(body, "subject_method").as_str() {
        "group.add" => "member-activated".to_string(),
        "group.remove" => "member-removed".to_string(),
        "group.leave" => "member-left".to_string(),
        "group.update_profile" => "group-profile-updated".to_string(),
        "group.update_policy" => "group-policy-updated".to_string(),
        _ => String::new(),
    }
}

fn build_mail_notification_event_text(
    subject: &str,
    preview: &str,
    has_attachments: bool,
) -> String {
    let trimmed_preview = preview.trim();
    if !trimmed_preview.is_empty() {
        return trimmed_preview.to_string();
    }
    let trimmed_subject = subject.trim();
    if !trimmed_subject.is_empty() {
        return format!("[邮件] {trimmed_subject}");
    }
    if has_attachments {
        return "[邮件] 收到一封包含附件的邮件".to_string();
    }
    "[邮件] 收到一封新邮件".to_string()
}

fn generated_host_notification_id(notification: &Value) -> String {
    let raw =
        serde_json::to_vec(notification).unwrap_or_else(|_| notification.to_string().into_bytes());
    let sum = Sha256::digest(raw);
    format!("hostevt-{}", hex_prefix(&sum, 8))
}

fn hex_prefix(bytes: &[u8], count: usize) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(count * 2);
    for byte in bytes.iter().take(count) {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

fn map_value(value: Option<&Value>) -> Option<&Map<String, Value>> {
    value.and_then(Value::as_object)
}

fn value_from_object<'a>(object: Option<&'a Map<String, Value>>, key: &str) -> Option<&'a Value> {
    object.and_then(|object| object.get(key))
}

fn string_from_object(object: Option<&Map<String, Value>>, key: &str) -> String {
    string_value(value_from_object(object, key))
}

fn string_value(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(value)) => value.clone(),
        _ => String::new(),
    }
}

fn string_like_value(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(value)) => value.clone(),
        Some(Value::Number(number)) => {
            if let Some(value) = number.as_i64() {
                value.to_string()
            } else if let Some(value) = number.as_u64() {
                value.to_string()
            } else if let Some(value) = number.as_f64() {
                format!("{value:.0}")
            } else {
                String::new()
            }
        }
        _ => String::new(),
    }
}

fn bool_value(value: Option<&Value>) -> bool {
    match value {
        Some(Value::Bool(value)) => *value,
        Some(Value::Number(number)) => number
            .as_i64()
            .map(|value| value != 0)
            .or_else(|| number.as_u64().map(|value| value != 0))
            .or_else(|| number.as_f64().map(|value| value != 0.0))
            .unwrap_or(false),
        Some(Value::String(value)) => value == "1" || value.eq_ignore_ascii_case("true"),
        _ => false,
    }
}

fn fallback_string(value: String, fallback: &str) -> String {
    if value.trim().is_empty() {
        fallback.to_string()
    } else {
        value
    }
}

fn format_go_rfc3339(value: OffsetDateTime) -> String {
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

fn is_false(value: &bool) -> bool {
    !*value
}
