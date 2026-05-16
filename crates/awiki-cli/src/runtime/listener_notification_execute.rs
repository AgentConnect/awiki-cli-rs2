use super::host_notify_sink::HostNotifySink;
use super::listener::{clear_host_notify_error_if_present, write_host_notify_error_if_changed};
use super::listener_contact_sync::{sync_incoming_contact, IncomingContactLookup};
use super::listener_notification_plan::{
    handle_notification_plan, NotificationRoute, NotificationSessionContext,
    NotificationSideEffect, SecureNotificationEffect, SecureNotificationNormalization,
};
use crate::runtime::listener::Status;
use crate::store::{store_message, upsert_group, upsert_group_member};
use rusqlite::Connection;
use serde_json::Value;
use time::OffsetDateTime;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostNotifyStatusUpdate {
    Unchanged,
    ClearError,
    SetError(String),
}

impl HostNotifyStatusUpdate {
    pub fn apply_to_status(&self, status: &mut Status) -> bool {
        match self {
            HostNotifyStatusUpdate::Unchanged => false,
            HostNotifyStatusUpdate::ClearError => clear_host_notify_error_if_present(status),
            HostNotifyStatusUpdate::SetError(error) => {
                write_host_notify_error_if_changed(status, error)
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationSideEffectFailure {
    pub effect: String,
    pub error: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationExecutionResult {
    pub route: NotificationRoute,
    pub secure_effect: SecureNotificationEffect,
    pub applied_effect_count: usize,
    pub failed_effect_count: usize,
    pub applied_effects: Vec<String>,
    pub failed_effects: Vec<NotificationSideEffectFailure>,
    pub host_notify_last_error: Option<String>,
    pub host_notify_status_update: HostNotifyStatusUpdate,
}

pub fn execute_listener_notification(
    connection: &mut Connection,
    host_notify_sink: &dyn HostNotifySink,
    notification: &Value,
    session: &NotificationSessionContext,
    secure_normalization: SecureNotificationNormalization,
    received_at: Option<OffsetDateTime>,
    mut lookup_handle_by_did: Option<IncomingContactLookup<'_>>,
) -> NotificationExecutionResult {
    let plan = handle_notification_plan(
        notification,
        session,
        secure_normalization,
        received_at,
        |request| {
            let lookup = lookup_handle_by_did
                .as_mut()
                .map(|lookup| &mut **lookup as IncomingContactLookup<'_>);
            sync_incoming_contact(
                connection,
                &request.owner_did,
                &request.sender_did,
                &request.source_type,
                &request.source_group_id,
                lookup,
            )
        },
    );

    let mut result = NotificationExecutionResult {
        route: plan.route,
        secure_effect: plan.secure_effect,
        applied_effect_count: 0,
        failed_effect_count: 0,
        applied_effects: Vec::new(),
        failed_effects: Vec::new(),
        host_notify_last_error: None,
        host_notify_status_update: HostNotifyStatusUpdate::Unchanged,
    };

    for effect in plan.side_effects {
        execute_side_effect(connection, host_notify_sink, effect, &mut result);
    }

    result.applied_effect_count = result.applied_effects.len();
    result.failed_effect_count = result.failed_effects.len();
    result
}

fn execute_side_effect(
    connection: &Connection,
    host_notify_sink: &dyn HostNotifySink,
    effect: NotificationSideEffect,
    result: &mut NotificationExecutionResult,
) {
    let description = describe_side_effect(&effect);
    let success_update = host_notify_success_update(&effect);
    let outcome = match effect {
        NotificationSideEffect::SyncIncomingContact(_) => Ok(()),
        NotificationSideEffect::ApplyHostNotificationHandles { .. } => Ok(()),
        NotificationSideEffect::StoreMessage(record) => {
            store_message(connection, record).map_err(|err| err.to_string())
        }
        NotificationSideEffect::UpsertGroup(record) => {
            upsert_group(connection, record).map_err(|err| err.to_string())
        }
        NotificationSideEffect::UpsertGroupMember(record) => {
            upsert_group_member(connection, record).map_err(|err| err.to_string())
        }
        NotificationSideEffect::DispatchHostNotification {
            event,
            should_notify,
        } => {
            if should_notify {
                if let Some(event) = event.as_ref() {
                    host_notify_sink
                        .notify(event)
                        .map_err(|err| err.to_string())
                } else {
                    Ok(())
                }
            } else {
                Ok(())
            }
        }
    };

    match outcome {
        Ok(()) => {
            if let Some(update) = success_update {
                result.host_notify_last_error = None;
                result.host_notify_status_update = update;
            }
            result.applied_effects.push(description);
        }
        Err(error) => {
            if description.starts_with("dispatch_host_notification ") {
                result.host_notify_last_error = Some(error.clone());
                result.host_notify_status_update = HostNotifyStatusUpdate::SetError(error.clone());
            }
            result.failed_effects.push(NotificationSideEffectFailure {
                effect: description,
                error,
            });
        }
    }
}

fn host_notify_success_update(effect: &NotificationSideEffect) -> Option<HostNotifyStatusUpdate> {
    match effect {
        NotificationSideEffect::DispatchHostNotification {
            event,
            should_notify,
        } if *should_notify && event.is_some() => Some(HostNotifyStatusUpdate::ClearError),
        NotificationSideEffect::DispatchHostNotification { .. } => {
            Some(HostNotifyStatusUpdate::Unchanged)
        }
        _ => None,
    }
}

fn describe_side_effect(effect: &NotificationSideEffect) -> String {
    match effect {
        NotificationSideEffect::SyncIncomingContact(request) => format!(
            "sync_incoming_contact sender_did={} source_type={} source_group_id={}",
            request.sender_did, request.source_type, request.source_group_id
        ),
        NotificationSideEffect::ApplyHostNotificationHandles {
            sender_handle,
            recipient_handle,
        } => format!(
            "apply_host_notification_handles sender_handle={} recipient_handle={}",
            sender_handle, recipient_handle
        ),
        NotificationSideEffect::StoreMessage(record) => {
            format!("store_message msg_id={}", record.msg_id)
        }
        NotificationSideEffect::UpsertGroup(record) => {
            format!("upsert_group group_id={}", record.group_id)
        }
        NotificationSideEffect::UpsertGroupMember(record) => {
            format!(
                "upsert_group_member group_id={} user_id={}",
                record.group_id, record.user_id
            )
        }
        NotificationSideEffect::DispatchHostNotification {
            event,
            should_notify,
        } => format!(
            "dispatch_host_notification should_notify={} event_id={}",
            should_notify,
            event.as_ref().map(|event| event.id.as_str()).unwrap_or("")
        ),
    }
}
