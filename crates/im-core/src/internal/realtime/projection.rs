use serde_json::{Map, Value};

use crate::ids::{GroupRef, MessageId, PeerRef};
use crate::messages::{
    Message, MessageBodyView, MessageDirection, MessageKind, MessageMetadata,
    MessageMetadataAttribute, ThreadRef,
};
use crate::realtime::{
    GroupUpdateKind, GroupUpdatedEvent, HostNotificationEvent, HostNotificationKind, ImEvent,
    LocalNotificationEvent, MessageReceivedEvent, UnknownNotificationEvent,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotificationProjectionRoute {
    DirectIncoming,
    GroupIncoming,
    GroupStateChanged,
    LocalNotification,
    HostNotification,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationProjection {
    pub route: NotificationProjectionRoute,
    pub event: ImEvent,
}

pub fn project_notification(notification: &Value) -> NotificationProjection {
    match string_value(notification.get("method")).as_str() {
        "direct.incoming" => project_direct_incoming(notification),
        "group.incoming" => project_group_incoming(notification),
        "group.state_changed" => project_group_state_changed(notification),
        "local.notification" | "notification.local" => project_local_notification(notification),
        "host.notification" | "notification.host" => project_host_notification(notification),
        method => unknown_notification(notification, method, "unsupported notification method"),
    }
}

pub fn is_direct_secure_wire_notification(notification: &Value) -> bool {
    if notification.get("method").and_then(Value::as_str) != Some("direct.incoming") {
        return false;
    }
    let content_type = notification
        .pointer("/params/meta/content_type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    anp::direct_e2ee::is_direct_e2ee_wire_content_type(content_type)
}

fn project_direct_incoming(notification: &Value) -> NotificationProjection {
    let Some(params) = map_value(notification.get("params")) else {
        return unknown_notification(notification, "direct.incoming", "missing params");
    };
    let meta = map_value(value_from_object(Some(params), "meta"));
    let body = map_value(value_from_object(Some(params), "body"));
    let target = meta.and_then(|meta| map_value(value_from_object(Some(meta), "target")));
    let receiver = string_from_object(target, "did");
    let sender = string_from_object(meta, "sender_did");
    if receiver.is_empty() || sender.is_empty() {
        return unknown_notification(notification, "direct.incoming", "missing sender or target");
    }

    let content_type = fallback_string(string_from_object(meta, "content_type"), "text/plain");
    let text = direct_text(body);
    let message_id = parse_message_id(
        &fallback_string(
            string_from_object(meta, "message_id"),
            &fallback_string(string_from_object(meta, "operation_id"), "unknown-direct"),
        ),
        "unknown-direct",
    );
    let thread = ThreadRef::Direct(parse_peer(&sender));
    let attachment_projection =
        crate::internal::realtime::attachment_projection::project_attachment(
            &content_type,
            body,
            &thread,
            &message_id,
        );
    let mut metadata = message_metadata(
        meta,
        None,
        Some(content_type.clone()),
        [("notification_method", "direct.incoming")],
    );
    if let Some(attachment) = attachment_projection.as_ref() {
        if let Some(summary) = attachment.summary.as_ref() {
            metadata.attributes.extend(
                crate::internal::realtime::attachment_projection::attachment_summary_attributes(
                    summary,
                ),
            );
        }
    }
    let message = Message {
        id: message_id,
        thread,
        direction: MessageDirection::Incoming,
        sender: parse_peer(&sender),
        receiver: Some(parse_peer(&receiver)),
        group: None,
        body: message_body_view(&content_type, text),
        sent_at: none_if_empty(string_from_object(meta, "created_at")),
        received_at: None,
        metadata,
    };
    NotificationProjection {
        route: NotificationProjectionRoute::DirectIncoming,
        event: message_received_event(message, attachment_projection),
    }
}

fn project_group_incoming(notification: &Value) -> NotificationProjection {
    let Some(params) = map_value(notification.get("params")) else {
        return unknown_notification(notification, "group.incoming", "missing params");
    };
    let meta = map_value(value_from_object(Some(params), "meta"));
    let body = map_value(value_from_object(Some(params), "body"));
    let target = meta.and_then(|meta| map_value(value_from_object(Some(meta), "target")));
    let receiver = string_from_object(target, "did");
    let sender = string_from_object(meta, "sender_did");
    let group_did = string_from_object(body, "group_did");
    if receiver.is_empty() || sender.is_empty() || group_did.is_empty() {
        return unknown_notification(
            notification,
            "group.incoming",
            "missing sender, target, or group",
        );
    }

    let content_type = fallback_string(string_from_object(meta, "content_type"), "text/plain");
    let group_seq = int64_value(value_from_object(body, "group_event_seq"));
    let message_id = fallback_string(
        string_from_object(meta, "message_id"),
        &fallback_group_message_id(&group_did, body, meta, notification),
    );
    let message_id = parse_message_id(&message_id, "unknown-group-message");
    let thread = ThreadRef::Group(parse_group(&group_did));
    let attachment_projection =
        crate::internal::realtime::attachment_projection::project_attachment(
            &content_type,
            body,
            &thread,
            &message_id,
        );
    let mut metadata = message_metadata(
        meta,
        group_seq,
        Some(content_type.clone()),
        [("notification_method", "group.incoming")],
    );
    if let Some(attachment) = attachment_projection.as_ref() {
        if let Some(summary) = attachment.summary.as_ref() {
            metadata.attributes.extend(
                crate::internal::realtime::attachment_projection::attachment_summary_attributes(
                    summary,
                ),
            );
        }
    }
    let message = Message {
        id: message_id,
        thread,
        direction: if sender == receiver {
            MessageDirection::Outgoing
        } else {
            MessageDirection::Incoming
        },
        sender: parse_peer(&sender),
        receiver: Some(parse_peer(&receiver)),
        group: Some(parse_group(&group_did)),
        body: message_body_view(&content_type, string_from_object(body, "text")),
        sent_at: none_if_empty(fallback_string(
            string_from_object(body, "accepted_at"),
            &string_from_object(meta, "created_at"),
        )),
        received_at: None,
        metadata,
    };
    NotificationProjection {
        route: NotificationProjectionRoute::GroupIncoming,
        event: message_received_event(message, attachment_projection),
    }
}

fn project_group_state_changed(notification: &Value) -> NotificationProjection {
    let Some(params) = map_value(notification.get("params")) else {
        return unknown_notification(notification, "group.state_changed", "missing params");
    };
    let body = map_value(value_from_object(Some(params), "body"));
    let group_did = string_from_object(body, "group_did");
    if group_did.is_empty() {
        return unknown_notification(notification, "group.state_changed", "missing group");
    }
    let update_kind = group_update_kind(body);
    NotificationProjection {
        route: NotificationProjectionRoute::GroupStateChanged,
        event: ImEvent::GroupUpdated(GroupUpdatedEvent {
            group: parse_group(&group_did),
            update_kind,
        }),
    }
}

fn project_local_notification(notification: &Value) -> NotificationProjection {
    let params = map_value(notification.get("params"));
    NotificationProjection {
        route: NotificationProjectionRoute::LocalNotification,
        event: ImEvent::LocalNotification(LocalNotificationEvent {
            notification_id: none_if_empty(string_from_object(params, "id")),
            title: none_if_empty(string_from_object(params, "title")),
            body: none_if_empty(string_from_object(params, "body")),
            source: none_if_empty(string_from_object(params, "source")),
        }),
    }
}

fn project_host_notification(notification: &Value) -> NotificationProjection {
    let params = map_value(notification.get("params"));
    let kind = match string_from_object(params, "kind").as_str() {
        "direct" | "direct_message" => HostNotificationKind::DirectMessage,
        "group" | "group_message" => HostNotificationKind::GroupMessage,
        "group_state" => HostNotificationKind::GroupState,
        "mail" => HostNotificationKind::Mail,
        _ => HostNotificationKind::Unknown,
    };
    NotificationProjection {
        route: NotificationProjectionRoute::HostNotification,
        event: ImEvent::HostNotification(HostNotificationEvent {
            event_type: kind,
            title: none_if_empty(string_from_object(params, "title")),
            body: none_if_empty(string_from_object(params, "body")),
            thread: None,
        }),
    }
}

fn unknown_notification(
    notification: &Value,
    notification_type: &str,
    reason: &str,
) -> NotificationProjection {
    NotificationProjection {
        route: NotificationProjectionRoute::Unknown,
        event: ImEvent::UnknownNotification(UnknownNotificationEvent {
            content_type: content_type(notification),
            notification_type: none_if_empty(notification_type.to_string()),
            reason: reason.to_string(),
        }),
    }
}

fn message_received_event(
    message: Message,
    attachment_projection: Option<
        crate::internal::realtime::attachment_projection::AttachmentProjection,
    >,
) -> ImEvent {
    let (attachment_summary, download_action, warnings) =
        if let Some(attachment) = attachment_projection {
            (
                attachment.summary,
                attachment.download_action,
                attachment.warnings,
            )
        } else {
            (None, None, Vec::new())
        };
    ImEvent::MessageReceived(MessageReceivedEvent {
        message,
        attachment_summary,
        download_action,
        warnings,
    })
}

fn direct_text(body: Option<&Map<String, Value>>) -> String {
    let text = string_from_object(body, "text");
    if !text.is_empty() {
        return text;
    }
    string_like_value(value_from_object(body, "payload"))
}

fn message_body_view(content_type: &str, text: String) -> MessageBodyView {
    if content_type == "text/plain" && !text.is_empty() {
        MessageBodyView::Text {
            text,
            kind: MessageKind::Text,
        }
    } else {
        MessageBodyView::Unsupported {
            content_type: none_if_empty(content_type.to_string()),
        }
    }
}

fn message_metadata<const N: usize>(
    meta: Option<&Map<String, Value>>,
    server_sequence: Option<i64>,
    content_type: Option<String>,
    attributes: [(&str, &str); N],
) -> MessageMetadata {
    MessageMetadata {
        operation_id: none_if_empty(string_from_object(meta, "operation_id")),
        server_sequence,
        content_type,
        attributes: attributes
            .into_iter()
            .map(|(key, value)| MessageMetadataAttribute {
                key: key.to_string(),
                value: value.to_string(),
            })
            .collect(),
        ..MessageMetadata::default()
    }
}

fn group_update_kind(body: Option<&Map<String, Value>>) -> GroupUpdateKind {
    match string_from_object(body, "membership_status").as_str() {
        "active" | "activated" => return GroupUpdateKind::MemberAdded,
        "removed" | "left" => return GroupUpdateKind::MemberRemoved,
        _ => {}
    }
    match string_from_object(body, "subject_method").as_str() {
        "group.create" => GroupUpdateKind::Created,
        "group.add" => GroupUpdateKind::MemberAdded,
        "group.remove" | "group.leave" => GroupUpdateKind::MemberRemoved,
        "group.update_profile" | "group.update_policy" => GroupUpdateKind::Updated,
        _ => GroupUpdateKind::Unknown,
    }
}

fn fallback_group_message_id(
    group_did: &str,
    body: Option<&Map<String, Value>>,
    meta: Option<&Map<String, Value>>,
    notification: &Value,
) -> String {
    let group_event_seq = string_like_value(value_from_object(body, "group_event_seq"));
    if !group_event_seq.is_empty() {
        return format!("{group_did}:{group_event_seq}");
    }
    fallback_string(
        string_from_object(meta, "operation_id"),
        &fallback_string(
            content_type(notification).unwrap_or_default(),
            "unknown-group-message",
        ),
    )
}

fn content_type(notification: &Value) -> Option<String> {
    let params = map_value(notification.get("params"));
    let meta = params.and_then(|params| map_value(value_from_object(Some(params), "meta")));
    none_if_empty(string_from_object(meta, "content_type"))
}

fn parse_peer(value: &str) -> PeerRef {
    PeerRef::parse(value, "").unwrap_or_else(|_| PeerRef::parse("unknown", "").unwrap())
}

fn parse_group(value: &str) -> GroupRef {
    GroupRef::parse(value).unwrap_or_else(|_| GroupRef::parse("unknown-group").unwrap())
}

fn parse_message_id(value: &str, fallback: &str) -> MessageId {
    MessageId::parse(value).unwrap_or_else(|_| MessageId::parse(fallback).unwrap())
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

fn int64_value(value: Option<&Value>) -> Option<i64> {
    match value {
        Some(Value::Number(number)) => number.as_i64().or_else(|| {
            number
                .as_u64()
                .and_then(|value| i64::try_from(value).ok())
                .or_else(|| number.as_f64().map(|value| value as i64))
        }),
        Some(Value::String(value)) if !value.is_empty() => value.parse::<i64>().ok(),
        _ => None,
    }
}

fn fallback_string(value: String, fallback: &str) -> String {
    if value.trim().is_empty() {
        fallback.to_string()
    } else {
        value
    }
}

fn none_if_empty(value: String) -> Option<String> {
    if value.trim().is_empty() {
        None
    } else {
        Some(value)
    }
}
