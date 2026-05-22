// Temporary migration-only legacy bridge exception.
// Delete in PR C5/C7 when realtime runner event delivery uses stable im-core
// event sink APIs and this module is only CLI-owned host-notification
// projection, with no compat trait implementation or listener legacy loop ties.

use crate::runtime::host_notify::{
    DirectMessageNotificationData, GroupMessageNotificationData, GroupStateChangedNotificationData,
    HostNotificationData, HostNotificationEvent, HOST_NOTIFICATION_VERSION,
};
use crate::runtime::host_notify_sink::HostNotifySink;
use crate::runtime::listener::{self, Status};
use crate::runtime::listener_contact_sync::{
    normalize_listener_handle, sync_incoming_contact, IncomingContactLookup,
};
use crate::runtime::listener_notification_execute::{
    HostNotifyStatusUpdate, NotificationExecutionResult, NotificationSideEffectFailure,
};
use crate::runtime::listener_notification_plan::{
    IncomingContactSyncRequest, NotificationRoute, NotificationSessionContext,
    SecureNotificationEffect,
};
use crate::store::{self, GroupRecord, MessageRecord};
use crate::{config::Resolved, identity::types::StoredIdentity};
use im_core::prelude::{
    GroupUpdateKind, HostNotificationKind, ImEvent, Message, MessageBodyView, MessageDirection,
    ThreadRef,
};
use rusqlite::Connection;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use time::OffsetDateTime;

pub const IM_EVENT_UNKNOWN_WARNING_PREFIX: &str = "im-core realtime unknown notification";

pub struct CliRealtimeEventSink<'a> {
    pub resolved: &'a Resolved,
    pub status: &'a Arc<Mutex<Status>>,
    pub host_notify: &'a Arc<crate::runtime::host_notify_sink::HostNotifySinkImpl>,
    pub record: &'a StoredIdentity,
}

impl im_core::compat::realtime::RealtimeRunnerEventSink for CliRealtimeEventSink<'_> {
    fn emit(&mut self, event: ImEvent) -> im_core::ImResult<()> {
        if !event_requires_cli_projection(&event) {
            return Ok(());
        }
        let mut connection = store::open(&self.resolved.paths).map_err(|err| {
            im_core::ImError::LocalStateUnavailable {
                detail: err.to_string(),
            }
        })?;
        store::ensure_schema(&connection).map_err(|err| {
            im_core::ImError::LocalStateUnavailable {
                detail: err.to_string(),
            }
        })?;
        let mut guard = self.status.lock().map_err(|_| im_core::ImError::Internal {
            message: "listener status mutex poisoned".to_string(),
        })?;
        let session = NotificationSessionContext {
            identity_name: self.record.identity_name.clone(),
            did: self.record.did.clone(),
            handle: self.record.handle.clone(),
        };
        let mut lookup = |did: &str| {
            crate::runtime::listener_handle_lookup::lookup_listener_handle_by_did(
                self.resolved,
                did,
            )
        };
        let _ = handle_im_event(
            &mut connection,
            Some(self.host_notify.as_ref()),
            &mut guard,
            event,
            &session,
            None,
            Some(&mut lookup),
        );
        let _ = listener::write_status(&guard.status_file, &guard);
        Ok(())
    }
}

pub fn handle_im_event(
    connection: &mut Connection,
    host_notify_sink: Option<&dyn HostNotifySink>,
    status: &mut Status,
    event: ImEvent,
    session: &NotificationSessionContext,
    received_at: Option<OffsetDateTime>,
    lookup_handle_by_did: Option<IncomingContactLookup<'_>>,
) -> NotificationExecutionResult {
    let host_notify_sink = host_notify_sink.map(|sink| sink as &dyn HostNotifySink);
    let mut executor = ImEventExecutor {
        connection,
        host_notify_sink,
        status,
        session,
        received_at,
        lookup_handle_by_did,
        result: empty_result(NotificationRoute::Ignored),
    };
    executor.handle(event);
    executor.finish()
}

pub fn should_legacy_handle_raw_notification_with_im_core_runner(notification: &Value) -> bool {
    crate::runtime::listener_secure_notifications::is_direct_secure_incoming_notification(
        notification,
    )
}

fn event_requires_cli_projection(event: &ImEvent) -> bool {
    matches!(
        event,
        ImEvent::MessageReceived(_)
            | ImEvent::GroupUpdated(_)
            | ImEvent::HostNotification(_)
            | ImEvent::UnknownNotification(_)
    )
}

struct ImEventExecutor<'a, 'lookup> {
    connection: &'a mut Connection,
    host_notify_sink: Option<&'a dyn HostNotifySink>,
    status: &'a mut Status,
    session: &'a NotificationSessionContext,
    received_at: Option<OffsetDateTime>,
    lookup_handle_by_did: Option<IncomingContactLookup<'lookup>>,
    result: NotificationExecutionResult,
}

impl ImEventExecutor<'_, '_> {
    fn handle(&mut self, event: ImEvent) {
        match event {
            ImEvent::MessageReceived(event) => self.handle_message_received(event.message),
            ImEvent::GroupUpdated(event) => {
                self.handle_group_updated(event.group.as_str(), event.update_kind)
            }
            ImEvent::HostNotification(event) => self.handle_host_notification(event),
            ImEvent::UnknownNotification(event) => self.handle_unknown_notification(event),
            ImEvent::ConnectionStateChanged(_)
            | ImEvent::MessageUpdated(_)
            | ImEvent::LocalNotification(_) => {}
        }
    }

    fn handle_message_received(&mut self, message: Message) {
        let Some(record) = message_record_from_im_message(&message, self.session) else {
            self.warn("message received event missing owner, thread, or message id");
            return;
        };
        self.result.route = if record.group_did.trim().is_empty() {
            NotificationRoute::DirectIncoming
        } else {
            NotificationRoute::GroupIncoming
        };

        let sender_handle = if !record.sender_did.trim().is_empty() {
            let request = IncomingContactSyncRequest {
                owner_did: self.session.did.trim().to_string(),
                sender_did: record.sender_did.clone(),
                source_type: if record.group_did.trim().is_empty() {
                    "direct.incoming".to_string()
                } else {
                    "group.incoming".to_string()
                },
                source_group_id: record.group_did.clone(),
            };
            self.apply_sync_incoming_contact(&request)
        } else {
            String::new()
        };
        let recipient_handle = normalize_listener_handle(&self.session.handle);
        let host_event = host_notification_from_message(
            &message,
            self.received_at,
            &sender_handle,
            &recipient_handle,
        );

        self.apply_store_message(record);
        self.dispatch_host_notification(host_event);
    }

    fn handle_group_updated(&mut self, group: &str, update_kind: GroupUpdateKind) {
        let group = group.trim();
        if self.session.did.trim().is_empty() || group.is_empty() {
            self.warn("group updated event missing owner or group");
            return;
        }
        self.result.route = NotificationRoute::GroupStateChanged;
        self.apply_upsert_group(GroupRecord {
            owner_did: self.session.did.trim().to_string(),
            group_id: group.to_string(),
            group_did: group.to_string(),
            last_message_at: store::now_utc(),
            metadata: json!({
                "source": "im-core.realtime",
                "update_kind": group_update_kind_label(&update_kind),
            })
            .to_string(),
            credential_name: self.session.identity_name.clone(),
            ..GroupRecord::default()
        });
        self.dispatch_host_notification(Some(host_notification_from_group_update(
            group,
            &self.session.did,
            update_kind,
            self.received_at,
        )));
    }

    fn handle_host_notification(&mut self, event: im_core::prelude::HostNotificationEvent) {
        self.result.route = NotificationRoute::Ignored;
        let Some(host_event) = host_notification_from_sdk_host_event(event, self.received_at)
        else {
            self.warn("host notification event missing title/body");
            return;
        };
        self.dispatch_host_notification(Some(host_event));
    }

    fn handle_unknown_notification(&mut self, event: im_core::prelude::UnknownNotificationEvent) {
        let warning = format!(
            "{IM_EVENT_UNKNOWN_WARNING_PREFIX}: type={} content_type={} reason={}",
            event.notification_type.unwrap_or_default(),
            event.content_type.unwrap_or_default(),
            event.reason
        );
        if !self.status.warnings.iter().any(|known| known == &warning) {
            self.status.warnings.push(warning);
        }
        let _ = listener::write_status(&self.status.status_file, self.status);
    }

    fn apply_sync_incoming_contact(&mut self, request: &IncomingContactSyncRequest) -> String {
        let lookup = self
            .lookup_handle_by_did
            .as_mut()
            .map(|lookup| &mut **lookup as IncomingContactLookup<'_>);
        let outcome = sync_incoming_contact(
            self.connection,
            &request.owner_did,
            &request.sender_did,
            &request.source_type,
            &request.source_group_id,
            lookup,
        );
        match outcome {
            Ok(handle) => {
                self.result.applied_effects.push(format!(
                    "sync_incoming_contact sender_did={} source_type={} source_group_id={}",
                    request.sender_did, request.source_type, request.source_group_id
                ));
                handle
            }
            Err(error) => {
                self.result
                    .failed_effects
                    .push(NotificationSideEffectFailure {
                        effect: format!(
                            "sync_incoming_contact sender_did={} source_type={} source_group_id={}",
                            request.sender_did, request.source_type, request.source_group_id
                        ),
                        error: error.to_string(),
                    });
                String::new()
            }
        }
    }

    fn apply_store_message(&mut self, record: MessageRecord) {
        let msg_id = record.msg_id.clone();
        match store::store_message(self.connection, record) {
            Ok(()) => self
                .result
                .applied_effects
                .push(format!("store_message msg_id={msg_id}")),
            Err(error) => self
                .result
                .failed_effects
                .push(NotificationSideEffectFailure {
                    effect: format!("store_message msg_id={msg_id}"),
                    error: error.to_string(),
                }),
        }
    }

    fn apply_upsert_group(&mut self, record: GroupRecord) {
        let group_id = record.group_id.clone();
        match store::upsert_group(self.connection, record) {
            Ok(()) => self
                .result
                .applied_effects
                .push(format!("upsert_group group_id={group_id}")),
            Err(error) => self
                .result
                .failed_effects
                .push(NotificationSideEffectFailure {
                    effect: format!("upsert_group group_id={group_id}"),
                    error: error.to_string(),
                }),
        }
    }

    fn dispatch_host_notification(&mut self, event: Option<HostNotificationEvent>) {
        let should_notify = event.is_some();
        let effect = format!(
            "dispatch_host_notification should_notify={} event_id={}",
            should_notify,
            event.as_ref().map(|event| event.id.as_str()).unwrap_or("")
        );
        let Some(event) = event else {
            self.result.applied_effects.push(effect);
            return;
        };
        let Some(host_notify_sink) = self.host_notify_sink else {
            self.result.applied_effects.push(effect);
            return;
        };
        match host_notify_sink.notify(&event) {
            Ok(()) => {
                self.result.host_notify_last_error = None;
                self.result.host_notify_status_update = HostNotifyStatusUpdate::ClearError;
                self.result.host_notify_status_changed = self
                    .result
                    .host_notify_status_update
                    .apply_to_status(self.status);
                self.result.applied_effects.push(effect);
            }
            Err(error) => {
                let error = error.to_string();
                self.result.host_notify_last_error = Some(error.clone());
                self.result.host_notify_status_update =
                    HostNotifyStatusUpdate::SetError(error.clone());
                self.result.host_notify_status_changed = self
                    .result
                    .host_notify_status_update
                    .apply_to_status(self.status);
                self.result
                    .failed_effects
                    .push(NotificationSideEffectFailure { effect, error });
            }
        }
    }

    fn warn(&mut self, warning: &str) {
        if !self.status.warnings.iter().any(|known| known == warning) {
            self.status.warnings.push(warning.to_string());
        }
        let _ = listener::write_status(&self.status.status_file, self.status);
    }

    fn finish(mut self) -> NotificationExecutionResult {
        self.result.applied_effect_count = self.result.applied_effects.len();
        self.result.failed_effect_count = self.result.failed_effects.len();
        self.result
    }
}

fn empty_result(route: NotificationRoute) -> NotificationExecutionResult {
    NotificationExecutionResult {
        route,
        secure_effect: SecureNotificationEffect::NotSecure,
        applied_effect_count: 0,
        failed_effect_count: 0,
        applied_effects: Vec::new(),
        failed_effects: Vec::new(),
        host_notify_last_error: None,
        host_notify_status_update: HostNotifyStatusUpdate::Unchanged,
        host_notify_status_changed: false,
    }
}

fn message_record_from_im_message(
    message: &Message,
    session: &NotificationSessionContext,
) -> Option<MessageRecord> {
    let owner_did = owner_did_for_message(message, session);
    if owner_did.is_empty() || message.id.as_str().trim().is_empty() {
        return None;
    }
    let sender_did = message.sender.as_str().trim().to_string();
    let receiver_did = message
        .receiver
        .as_ref()
        .map(|peer| peer.as_str().trim().to_string())
        .unwrap_or_default();
    let group_did = group_did_for_message(message);
    let peer_did = if sender_did == owner_did {
        receiver_did.as_str()
    } else {
        sender_did.as_str()
    };
    let has_thread_peer = !peer_did.trim().is_empty();
    let thread_id = match &message.thread {
        ThreadRef::Group(group) => store::make_thread_id(&owner_did, "", group.as_str()),
        ThreadRef::Direct(peer) => store::make_thread_id(&owner_did, peer.as_str(), ""),
        ThreadRef::Thread(thread) => thread.as_str().to_string(),
    };
    if thread_id.trim().is_empty() {
        return None;
    }

    if group_did.trim().is_empty() && !has_thread_peer {
        return None;
    }

    Some(MessageRecord {
        msg_id: message.id.as_str().to_string(),
        owner_did,
        thread_id,
        direction: direction_value(&message.direction),
        sender_did,
        receiver_did,
        group_id: group_did.clone(),
        group_did,
        content_type: message_content_type(message),
        content: message_content(message),
        server_seq: message.metadata.server_sequence,
        sent_at: message
            .sent_at
            .clone()
            .or_else(|| message.received_at.clone())
            .unwrap_or_else(store::now_utc),
        is_read: matches!(message.direction, MessageDirection::Outgoing),
        metadata: message_metadata_value(message),
        credential_name: session.identity_name.clone(),
        ..MessageRecord::default()
    })
}

fn owner_did_for_message(message: &Message, session: &NotificationSessionContext) -> String {
    if let Some(receiver) = message.receiver.as_ref() {
        let receiver = receiver.as_str().trim();
        if !receiver.is_empty() {
            return receiver.to_string();
        }
    }
    session.did.trim().to_string()
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

fn direction_value(direction: &MessageDirection) -> i64 {
    match direction {
        MessageDirection::Outgoing => 1,
        MessageDirection::Incoming | MessageDirection::Unknown => 0,
    }
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

fn message_content(message: &Message) -> String {
    match &message.body {
        MessageBodyView::Text { text, .. } => text.clone(),
        MessageBodyView::Unsupported { content_type } => json!({
            "unsupported": true,
            "content_type": content_type,
        })
        .to_string(),
    }
}

fn message_metadata_value(message: &Message) -> String {
    serde_json::to_string(&message.metadata).unwrap_or_default()
}

fn host_notification_from_message(
    message: &Message,
    received_at: Option<OffsetDateTime>,
    sender_handle: &str,
    recipient_handle: &str,
) -> Option<HostNotificationEvent> {
    let received_at = format_go_rfc3339(received_at.unwrap_or_else(OffsetDateTime::now_utc));
    let group_did = group_did_for_message(message);
    if !group_did.is_empty() {
        return Some(HostNotificationEvent {
            version: HOST_NOTIFICATION_VERSION.to_string(),
            id: message.id.as_str().to_string(),
            topic: "im.group.message.received".to_string(),
            received_at,
            data: Some(HostNotificationData::Group(GroupMessageNotificationData {
                channel: "group".to_string(),
                message_id: message.id.as_str().to_string(),
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
                text: text_body(message),
                group_event_seq: message
                    .metadata
                    .server_sequence
                    .map(|value| value.to_string())
                    .unwrap_or_default(),
                accepted_at: message.sent_at.clone().unwrap_or_default(),
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
                text: text_body(message),
                created_at: message.sent_at.clone().unwrap_or_default(),
                ..DirectMessageNotificationData::default()
            },
        )),
    })
}

fn host_notification_from_group_update(
    group: &str,
    recipient_did: &str,
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
                recipient_did: recipient_did.to_string(),
                changed_at: store::now_utc(),
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
