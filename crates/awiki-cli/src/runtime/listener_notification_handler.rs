use super::host_notify_sink::{HostNotifySink, NoopHostNotifySink};
use super::listener::Status;
use super::listener_contact_sync::IncomingContactLookup;
use super::listener_notification_execute::{
    execute_listener_notification, execute_listener_notification_with_status,
    HostNotifyStatusUpdate, NotificationExecutionResult,
};
use super::listener_notification_plan::{
    NotificationSessionContext, SecureNotificationNormalization,
};
use rusqlite::Connection;
use serde_json::Value;
use time::OffsetDateTime;

pub fn handle_listener_notification(
    connection: &mut Connection,
    host_notify_sink: Option<&dyn HostNotifySink>,
    status: &mut Status,
    notification: &Value,
    session: &NotificationSessionContext,
    secure_normalization: SecureNotificationNormalization,
    received_at: Option<OffsetDateTime>,
    lookup_handle_by_did: Option<IncomingContactLookup<'_>>,
) -> NotificationExecutionResult {
    let Some(host_notify_sink) = host_notify_sink else {
        let noop = NoopHostNotifySink;
        let mut result = execute_listener_notification(
            connection,
            &noop,
            notification,
            session,
            secure_normalization,
            received_at,
            lookup_handle_by_did,
        );
        // Go dispatchHostNotification returns before status mutation when
        // Supervisor.hostNotify is nil, even for otherwise-notifiable events.
        result.host_notify_last_error = None;
        result.host_notify_status_update = HostNotifyStatusUpdate::Unchanged;
        result.host_notify_status_changed = false;
        return result;
    };

    execute_listener_notification_with_status(
        connection,
        host_notify_sink,
        status,
        notification,
        session,
        secure_normalization,
        received_at,
        lookup_handle_by_did,
    )
}
