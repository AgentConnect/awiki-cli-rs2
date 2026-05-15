use super::host_notify::{apply_host_notification_handles, normalize_host_notification};
use super::listener_contact_sync::normalize_listener_handle;
use super::listener_message_records::{
    message_record_from_direct_incoming, message_record_from_group_incoming,
    message_record_from_mail_notification, records_from_group_state_changed,
};
use super::listener_secure_notifications::is_direct_secure_incoming_notification;
use crate::runtime::host_notify::HostNotificationEvent;
use crate::store::{GroupMemberRecord, GroupRecord, MessageRecord};
use serde_json::Value;
use time::OffsetDateTime;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NotificationSessionContext {
    pub identity_name: String,
    pub did: String,
    pub handle: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SecureNotificationNormalization {
    KeepOriginal,
    Replace(Value),
    Drop,
}

impl Default for SecureNotificationNormalization {
    fn default() -> Self {
        Self::KeepOriginal
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SecureNotificationEffect {
    NotSecure,
    KeptOriginal,
    Replaced,
    Dropped,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotificationRoute {
    DirectIncoming,
    MailNotification,
    GroupIncoming,
    GroupStateChanged,
    Ignored,
    DroppedBySecureNormalization,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncomingContactSyncRequest {
    pub owner_did: String,
    pub sender_did: String,
    pub source_type: String,
    pub source_group_id: String,
}

#[derive(Debug, Clone)]
pub enum NotificationSideEffect {
    SyncIncomingContact(IncomingContactSyncRequest),
    ApplyHostNotificationHandles {
        sender_handle: String,
        recipient_handle: String,
    },
    StoreMessage(MessageRecord),
    UpsertGroup(GroupRecord),
    UpsertGroupMember(GroupMemberRecord),
    DispatchHostNotification {
        event: Option<HostNotificationEvent>,
        should_notify: bool,
    },
}

#[derive(Debug, Clone)]
pub struct NotificationHandlePlan {
    pub route: NotificationRoute,
    pub secure_effect: SecureNotificationEffect,
    pub notification: Option<Value>,
    pub host_event: Option<HostNotificationEvent>,
    pub side_effects: Vec<NotificationSideEffect>,
}

pub fn handle_notification_plan(
    notification: &Value,
    session: &NotificationSessionContext,
    secure_normalization: SecureNotificationNormalization,
    received_at: Option<OffsetDateTime>,
    mut sync_incoming_contact: impl FnMut(&IncomingContactSyncRequest) -> anyhow::Result<String>,
) -> NotificationHandlePlan {
    let (notification, secure_effect) =
        apply_secure_normalization(notification, secure_normalization);
    let Some(notification) = notification else {
        return NotificationHandlePlan {
            route: NotificationRoute::DroppedBySecureNormalization,
            secure_effect,
            notification: None,
            host_event: None,
            side_effects: Vec::new(),
        };
    };

    let mut host_event = normalize_host_notification(&notification, received_at);
    let should_notify = host_event.is_some();
    let mut side_effects = Vec::new();
    if let Some(record) = message_record_from_direct_incoming(&notification, &session.identity_name)
    {
        let request = IncomingContactSyncRequest {
            owner_did: session.did.trim().to_string(),
            sender_did: record.sender_did.clone(),
            source_type: "direct.incoming".to_string(),
            source_group_id: String::new(),
        };
        let sender_handle = sync_incoming_contact(&request).unwrap_or_default();
        let recipient_handle = normalize_listener_handle(&session.handle);
        side_effects.push(NotificationSideEffect::SyncIncomingContact(request));
        apply_handles(
            &mut host_event,
            &sender_handle,
            &recipient_handle,
            &mut side_effects,
        );
        side_effects.push(NotificationSideEffect::StoreMessage(record));
        side_effects.push(dispatch_host_notification(
            host_event.clone(),
            should_notify,
        ));
        return NotificationHandlePlan {
            route: NotificationRoute::DirectIncoming,
            secure_effect,
            notification: Some(notification),
            host_event,
            side_effects,
        };
    }

    if let Some(record) =
        message_record_from_mail_notification(&notification, &session.identity_name)
    {
        side_effects.push(NotificationSideEffect::StoreMessage(record));
        side_effects.push(dispatch_host_notification(
            host_event.clone(),
            should_notify,
        ));
        return NotificationHandlePlan {
            route: NotificationRoute::MailNotification,
            secure_effect,
            notification: Some(notification),
            host_event,
            side_effects,
        };
    }

    if let Some(record) = message_record_from_group_incoming(&notification, &session.identity_name)
    {
        let request = IncomingContactSyncRequest {
            owner_did: session.did.trim().to_string(),
            sender_did: record.sender_did.clone(),
            source_type: "group.incoming".to_string(),
            source_group_id: record.group_did.clone(),
        };
        let sender_handle = sync_incoming_contact(&request).unwrap_or_default();
        let recipient_handle = normalize_listener_handle(&session.handle);
        side_effects.push(NotificationSideEffect::SyncIncomingContact(request));
        apply_handles(
            &mut host_event,
            &sender_handle,
            &recipient_handle,
            &mut side_effects,
        );
        side_effects.push(NotificationSideEffect::StoreMessage(record));
        side_effects.push(dispatch_host_notification(
            host_event.clone(),
            should_notify,
        ));
        return NotificationHandlePlan {
            route: NotificationRoute::GroupIncoming,
            secure_effect,
            notification: Some(notification),
            host_event,
            side_effects,
        };
    }

    if let Some(records) = records_from_group_state_changed(&notification, &session.identity_name) {
        side_effects.push(NotificationSideEffect::UpsertGroup(records.group));
        if let Some(member) = records.member {
            side_effects.push(NotificationSideEffect::UpsertGroupMember(member));
        }
        side_effects.push(NotificationSideEffect::StoreMessage(records.message));
        side_effects.push(dispatch_host_notification(
            host_event.clone(),
            should_notify,
        ));
        return NotificationHandlePlan {
            route: NotificationRoute::GroupStateChanged,
            secure_effect,
            notification: Some(notification),
            host_event,
            side_effects,
        };
    }

    NotificationHandlePlan {
        route: NotificationRoute::Ignored,
        secure_effect,
        notification: Some(notification),
        host_event,
        side_effects,
    }
}

fn apply_secure_normalization(
    notification: &Value,
    secure_normalization: SecureNotificationNormalization,
) -> (Option<Value>, SecureNotificationEffect) {
    if !is_direct_secure_incoming_notification(notification) {
        return (
            Some(notification.clone()),
            SecureNotificationEffect::NotSecure,
        );
    }
    match secure_normalization {
        SecureNotificationNormalization::KeepOriginal => (
            Some(notification.clone()),
            SecureNotificationEffect::KeptOriginal,
        ),
        SecureNotificationNormalization::Replace(notification) => {
            (Some(notification), SecureNotificationEffect::Replaced)
        }
        SecureNotificationNormalization::Drop => (None, SecureNotificationEffect::Dropped),
    }
}

fn apply_handles(
    host_event: &mut Option<HostNotificationEvent>,
    sender_handle: &str,
    recipient_handle: &str,
    side_effects: &mut Vec<NotificationSideEffect>,
) {
    if let Some(event) = host_event {
        apply_host_notification_handles(event, sender_handle, recipient_handle);
    }
    side_effects.push(NotificationSideEffect::ApplyHostNotificationHandles {
        sender_handle: sender_handle.to_string(),
        recipient_handle: recipient_handle.to_string(),
    });
}

fn dispatch_host_notification(
    event: Option<HostNotificationEvent>,
    should_notify: bool,
) -> NotificationSideEffect {
    NotificationSideEffect::DispatchHostNotification {
        event,
        should_notify,
    }
}
