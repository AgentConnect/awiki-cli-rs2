use crate::host_runtime::host_notify::{
    DirectMessageNotificationData, GroupMessageNotificationData, GroupStateChangedNotificationData,
    HostNotificationAttachmentDownloadAction, HostNotificationAttachmentSummary,
    HostNotificationData, HostNotificationEvent, HOST_NOTIFICATION_VERSION,
};
use crate::host_runtime::host_notify_sink::HostNotifySink;
use crate::host_runtime::listener::{self, Status};
use im_core::prelude::{
    AttachmentDownloadAction, AttachmentMessageSummary, GroupUpdateKind, HostNotificationKind,
    ImEvent, Message, MessageBodyView, MessageReceivedEvent, RealtimeConnectionState, ThreadRef,
};
use std::sync::{Arc, Mutex};
use time::OffsetDateTime;

pub const IM_EVENT_UNKNOWN_WARNING_PREFIX: &str = "im-core realtime unknown notification";

pub struct CliRealtimeEventSink<'a> {
    pub status: &'a Arc<Mutex<Status>>,
    pub host_notify: &'a Arc<crate::host_runtime::host_notify_sink::HostNotifySinkImpl>,
    pub identity_name: &'a str,
    pub did: &'a str,
}

impl CliRealtimeEventSink<'_> {
    pub fn emit(&mut self, event: ImEvent) -> im_core::ImResult<()> {
        let mut guard = self.status.lock().map_err(|_| im_core::ImError::Internal {
            message: "listener status mutex poisoned".to_string(),
        })?;
        handle_im_event(
            Some(self.host_notify.as_ref()),
            &mut guard,
            event,
            None,
            Some(self.identity_name),
            Some(self.did),
        );
        let _ = listener::write_status(&guard.status_file, &guard);
        Ok(())
    }
}

pub fn handle_im_event(
    host_notify_sink: Option<&dyn HostNotifySink>,
    status: &mut Status,
    event: ImEvent,
    received_at: Option<OffsetDateTime>,
    identity_name: Option<&str>,
    did: Option<&str>,
) -> CliImEventResult {
    let mut result = CliImEventResult::default();
    match event {
        ImEvent::ConnectionStateChanged(event) => {
            result.route = CliImEventRoute::ConnectionStateChanged;
            update_connection_status(status, identity_name, did, event.state, event.reason);
        }
        ImEvent::MessageReceived(event) => {
            let route = message_route(&event.message);
            let host_event = host_notification_from_message_event(event, received_at);
            dispatch_host_notification(host_notify_sink, status, host_event, &mut result);
            result.route = route;
        }
        ImEvent::GroupUpdated(event) => {
            result.route = CliImEventRoute::GroupStateChanged;
            dispatch_host_notification(
                host_notify_sink,
                status,
                Some(host_notification_from_group_update(
                    event.group.as_str(),
                    event.update_kind,
                    received_at,
                )),
                &mut result,
            );
        }
        ImEvent::HostNotification(event) => {
            result.route = CliImEventRoute::HostNotification;
            let host_event = host_notification_from_sdk_host_event(event, received_at);
            if host_event.is_none() {
                push_warning(status, "host notification event missing title/body");
            }
            dispatch_host_notification(host_notify_sink, status, host_event, &mut result);
        }
        ImEvent::UnknownNotification(event) => {
            result.route = CliImEventRoute::UnknownNotification;
            let warning = format!(
                "{IM_EVENT_UNKNOWN_WARNING_PREFIX}: type={} content_type={} reason={}",
                event.notification_type.unwrap_or_default(),
                event.content_type.unwrap_or_default(),
                event.reason
            );
            push_warning(status, &warning);
        }
        ImEvent::MessageUpdated(_) | ImEvent::LocalNotification(_) => {
            result.route = CliImEventRoute::Ignored;
        }
    }
    result
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CliImEventRoute {
    DirectIncoming,
    GroupIncoming,
    GroupStateChanged,
    HostNotification,
    UnknownNotification,
    ConnectionStateChanged,
    #[default]
    Ignored,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CliImEventResult {
    pub route: CliImEventRoute,
    pub dispatched_host_notification: bool,
    pub host_notify_last_error: Option<String>,
    pub host_notify_status_changed: bool,
}

fn update_connection_status(
    status: &mut Status,
    identity_name: Option<&str>,
    did: Option<&str>,
    state: RealtimeConnectionState,
    reason: Option<String>,
) {
    let connected = matches!(state, RealtimeConnectionState::Connected);
    let Some(identity_name) = identity_name
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        for session in &mut status.sessions {
            apply_connection_state(session, connected, reason.as_deref());
        }
        return;
    };
    if let Some(session) = status
        .sessions
        .iter_mut()
        .find(|session| session.identity_name == identity_name)
    {
        if let Some(did) = did.map(str::trim).filter(|value| !value.is_empty()) {
            session.did = did.to_string();
        }
        apply_connection_state(session, connected, reason.as_deref());
        return;
    }
    status
        .sessions
        .push(crate::host_runtime::listener::SessionStatus {
            identity_name: identity_name.to_string(),
            did: did.unwrap_or_default().trim().to_string(),
            connected,
            last_error: if connected {
                String::new()
            } else {
                reason.unwrap_or_default()
            },
        });
    status
        .sessions
        .sort_by(|left, right| left.identity_name.cmp(&right.identity_name));
}

fn apply_connection_state(
    session: &mut crate::host_runtime::listener::SessionStatus,
    connected: bool,
    reason: Option<&str>,
) {
    session.connected = connected;
    if connected {
        session.last_error.clear();
    } else if let Some(reason) = reason.filter(|value| !value.trim().is_empty()) {
        session.last_error = reason.to_string();
    }
}

fn dispatch_host_notification(
    host_notify_sink: Option<&dyn HostNotifySink>,
    status: &mut Status,
    event: Option<HostNotificationEvent>,
    result: &mut CliImEventResult,
) {
    let Some(event) = event else {
        return;
    };
    let Some(host_notify_sink) = host_notify_sink else {
        return;
    };
    match host_notify_sink.notify(&event) {
        Ok(()) => {
            result.dispatched_host_notification = true;
            result.host_notify_last_error = None;
            if listener::clear_host_notify_error_if_present(status) {
                result.host_notify_status_changed = true;
            }
        }
        Err(error) => {
            let error = error.to_string();
            result.host_notify_last_error = Some(error.clone());
            if listener::write_host_notify_error_if_changed(status, &error) {
                result.host_notify_status_changed = true;
            }
        }
    }
}

fn push_warning(status: &mut Status, warning: &str) {
    if !status.warnings.iter().any(|known| known == warning) {
        status.warnings.push(warning.to_string());
    }
}

fn message_route(message: &Message) -> CliImEventRoute {
    if group_did_for_message(message).is_empty() {
        CliImEventRoute::DirectIncoming
    } else {
        CliImEventRoute::GroupIncoming
    }
}

fn group_did_for_message(message: &Message) -> String {
    message
        .group
        .as_ref()
        .map(|group| group.as_str().trim().to_string())
        .or_else(|| match &message.thread {
            ThreadRef::Group(group) => Some(group.as_str().trim().to_string()),
            _ => None,
        })
        .unwrap_or_default()
}

fn message_content_type(message: &Message) -> String {
    message
        .metadata
        .content_type
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| match &message.body {
            MessageBodyView::Text { .. } => "text/plain".to_string(),
            MessageBodyView::Unsupported { content_type } => content_type
                .as_ref()
                .map(|value| value.trim())
                .filter(|value| !value.is_empty())
                .unwrap_or("application/octet-stream")
                .to_string(),
        })
}

fn host_notification_from_message_event(
    event: MessageReceivedEvent,
    received_at: Option<OffsetDateTime>,
) -> Option<HostNotificationEvent> {
    let MessageReceivedEvent {
        message,
        attachment_summary,
        download_action,
        warnings: _,
    } = event;
    host_notification_from_message(
        &message,
        received_at,
        "",
        "",
        attachment_summary.as_ref(),
        download_action.as_ref(),
    )
}

fn host_notification_from_message(
    message: &Message,
    received_at: Option<OffsetDateTime>,
    sender_handle: &str,
    recipient_handle: &str,
    attachment_summary: Option<&AttachmentMessageSummary>,
    download_action: Option<&AttachmentDownloadAction>,
) -> Option<HostNotificationEvent> {
    let received_at = format_go_rfc3339(received_at.unwrap_or_else(OffsetDateTime::now_utc));
    let group_did = group_did_for_message(message);
    if !group_did.is_empty() {
        let notification_message_id = host_group_message_id(message);
        return Some(HostNotificationEvent {
            version: HOST_NOTIFICATION_VERSION.to_string(),
            id: notification_message_id.clone(),
            topic: "im.group.message.received".to_string(),
            received_at,
            data: Some(HostNotificationData::Group(GroupMessageNotificationData {
                channel: "group".to_string(),
                message_id: notification_message_id,
                operation_id: message.metadata.operation_id.clone().unwrap_or_default(),
                group_did,
                sender_handle: sender_handle.to_string(),
                sender_did: message.sender.as_str().to_string(),
                recipient_handle: recipient_handle.to_string(),
                recipient_did: message
                    .receiver
                    .as_ref()
                    .map(|peer| peer.as_str().to_string())
                    .unwrap_or_default(),
                content_type: message_content_type(message),
                text: notification_text_body(message, attachment_summary),
                group_event_seq: message
                    .metadata
                    .server_sequence
                    .map(|value| value.to_string())
                    .unwrap_or_default(),
                accepted_at: message.sent_at.clone().unwrap_or_default(),
                has_attachments: attachment_summary.is_some(),
                attachment: host_attachment_summary(attachment_summary),
                download_action: host_attachment_download_action(download_action),
                ..GroupMessageNotificationData::default()
            })),
        });
    }
    let recipient_did = message
        .receiver
        .as_ref()
        .map(|peer| peer.as_str().trim().to_string())
        .unwrap_or_default();
    if recipient_did.is_empty() || message.sender.as_str().trim().is_empty() {
        return None;
    }
    Some(HostNotificationEvent {
        version: HOST_NOTIFICATION_VERSION.to_string(),
        id: message.id.as_str().to_string(),
        topic: "im.message.received".to_string(),
        received_at,
        data: Some(HostNotificationData::Direct(
            DirectMessageNotificationData {
                channel: "direct".to_string(),
                source_kind: "im".to_string(),
                message_id: message.id.as_str().to_string(),
                operation_id: message.metadata.operation_id.clone().unwrap_or_default(),
                sender_handle: sender_handle.to_string(),
                sender_did: message.sender.as_str().to_string(),
                recipient_handle: recipient_handle.to_string(),
                recipient_did,
                content_type: message_content_type(message),
                text: notification_text_body(message, attachment_summary),
                created_at: message.sent_at.clone().unwrap_or_default(),
                has_attachments: attachment_summary.is_some(),
                attachment: host_attachment_summary(attachment_summary),
                download_action: host_attachment_download_action(download_action),
                ..DirectMessageNotificationData::default()
            },
        )),
    })
}

fn host_group_message_id(message: &Message) -> String {
    message_attribute(message, "raw_message_id")
        .unwrap_or_else(|| message.id.as_str().trim().to_string())
}

fn message_attribute(message: &Message, key: &str) -> Option<String> {
    message
        .metadata
        .attributes
        .iter()
        .find(|attribute| attribute.key == key)
        .map(|attribute| attribute.value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn host_notification_from_group_update(
    group: &str,
    update_kind: GroupUpdateKind,
    received_at: Option<OffsetDateTime>,
) -> HostNotificationEvent {
    HostNotificationEvent {
        version: HOST_NOTIFICATION_VERSION.to_string(),
        id: format!("{}:{}", group, group_update_kind_label(&update_kind)),
        topic: "im.group.state.changed".to_string(),
        received_at: format_go_rfc3339(received_at.unwrap_or_else(OffsetDateTime::now_utc)),
        data: Some(HostNotificationData::GroupState(
            GroupStateChangedNotificationData {
                channel: "group".to_string(),
                event_id: format!("{}:{}", group, group_update_kind_label(&update_kind)),
                event_type: group_update_kind_label(&update_kind).to_string(),
                group_did: group.to_string(),
                changed_at: format_go_rfc3339(OffsetDateTime::now_utc()),
                ..GroupStateChangedNotificationData::default()
            },
        )),
    }
}

fn host_notification_from_sdk_host_event(
    event: im_core::prelude::HostNotificationEvent,
    received_at: Option<OffsetDateTime>,
) -> Option<HostNotificationEvent> {
    let title = event.title.unwrap_or_default();
    let body = event.body.unwrap_or_default();
    let text = if body.trim().is_empty() {
        title.clone()
    } else {
        body.clone()
    };
    if title.trim().is_empty() && body.trim().is_empty() {
        return None;
    }
    let event_id = format!(
        "im-core-host:{}:{}",
        host_notification_kind_label(&event.event_type),
        stable_notification_text(&title, &body)
    );
    Some(HostNotificationEvent {
        version: HOST_NOTIFICATION_VERSION.to_string(),
        id: event_id.clone(),
        topic: match event.event_type {
            HostNotificationKind::GroupMessage => "im.group.message.received",
            HostNotificationKind::GroupState => "im.group.state.changed",
            _ => "im.message.received",
        }
        .to_string(),
        received_at: format_go_rfc3339(received_at.unwrap_or_else(OffsetDateTime::now_utc)),
        data: Some(HostNotificationData::Direct(
            DirectMessageNotificationData {
                channel: host_notification_kind_label(&event.event_type).to_string(),
                source_kind: "im-core".to_string(),
                message_id: event_id,
                content_type: "text/plain".to_string(),
                text,
                subject: title,
                preview: body,
                ..DirectMessageNotificationData::default()
            },
        )),
    })
}

fn text_body(message: &Message) -> String {
    match &message.body {
        MessageBodyView::Text { text, .. } => text.clone(),
        MessageBodyView::Unsupported { .. } => String::new(),
    }
}

fn notification_text_body(
    message: &Message,
    attachment_summary: Option<&AttachmentMessageSummary>,
) -> String {
    let text = text_body(message);
    if !text.trim().is_empty() {
        return text;
    }
    let Some(summary) = attachment_summary else {
        return text;
    };
    let filename = summary
        .filename
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let mime_type = summary
        .mime_type
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    match (filename, mime_type, summary.size_bytes) {
        (Some(filename), Some(mime_type), Some(size)) => {
            format!("[attachment] {filename} ({mime_type}, {size} bytes)")
        }
        (Some(filename), Some(mime_type), None) => format!("[attachment] {filename} ({mime_type})"),
        (Some(filename), None, _) => format!("[attachment] {filename}"),
        (None, Some(mime_type), _) => format!("[attachment] {mime_type}"),
        (None, None, _) => "[attachment]".to_string(),
    }
}

fn host_attachment_summary(
    summary: Option<&AttachmentMessageSummary>,
) -> Option<HostNotificationAttachmentSummary> {
    let summary = summary?;
    Some(HostNotificationAttachmentSummary {
        attachment_id: summary.attachment_id.clone().unwrap_or_default(),
        filename: summary.filename.clone().unwrap_or_default(),
        mime_type: summary.mime_type.clone().unwrap_or_default(),
        size_bytes: summary.size_bytes,
        content_type: summary.content_type.clone().unwrap_or_default(),
    })
}

fn host_attachment_download_action(
    action: Option<&AttachmentDownloadAction>,
) -> Option<HostNotificationAttachmentDownloadAction> {
    let action = action?;
    let mut host_action = HostNotificationAttachmentDownloadAction {
        command: "msg.attachment.download".to_string(),
        message_id: action.message_id.as_str().to_string(),
        attachment_id: action.attachment_id.clone().unwrap_or_default(),
        ..HostNotificationAttachmentDownloadAction::default()
    };
    match &action.thread {
        ThreadRef::Direct(peer) => {
            host_action.with = peer.as_str().to_string();
        }
        ThreadRef::Group(group) => {
            host_action.group = group.as_str().to_string();
        }
        ThreadRef::Thread(_) => {}
    }
    Some(host_action)
}

fn group_update_kind_label(kind: &GroupUpdateKind) -> &'static str {
    match kind {
        GroupUpdateKind::Created => "created",
        GroupUpdateKind::Updated => "updated",
        GroupUpdateKind::MemberAdded => "member_added",
        GroupUpdateKind::MemberRemoved => "member_removed",
        GroupUpdateKind::MessageAdded => "message_added",
        GroupUpdateKind::Unknown => "unknown",
    }
}

fn host_notification_kind_label(kind: &HostNotificationKind) -> &'static str {
    match kind {
        HostNotificationKind::DirectMessage => "direct",
        HostNotificationKind::GroupMessage => "group",
        HostNotificationKind::GroupState => "group_state",
        HostNotificationKind::Mail => "mail",
        HostNotificationKind::Unknown => "unknown",
    }
}

fn stable_notification_text(title: &str, body: &str) -> String {
    format!("{}:{}", title.trim(), body.trim())
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .chars()
        .take(80)
        .collect::<String>()
}

fn format_go_rfc3339(value: OffsetDateTime) -> String {
    let value = value.to_offset(time::UtcOffset::UTC);
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
