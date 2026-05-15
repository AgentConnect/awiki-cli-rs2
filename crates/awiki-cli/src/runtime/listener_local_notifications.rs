use serde_json::{Map, Value};
use std::collections::BTreeMap;

pub type LocalNotification = Map<String, Value>;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct LocalNotificationQueue {
    notifications: BTreeMap<String, Vec<LocalNotification>>,
}

impl LocalNotificationQueue {
    pub fn queue_local_notification(
        &mut self,
        recipient_did: impl Into<String>,
        notification: Option<LocalNotification>,
    ) {
        let recipient_did = recipient_did.into();
        if recipient_did.trim().is_empty() {
            return;
        }
        let Some(notification) = notification else {
            return;
        };
        self.notifications
            .entry(recipient_did)
            .or_default()
            .push(notification);
    }

    pub fn flush_queued_local_notifications(
        &mut self,
        current_did: Option<&str>,
    ) -> Vec<LocalNotification> {
        let Some(current_did) = current_did else {
            return Vec::new();
        };
        if current_did.trim().is_empty() {
            return Vec::new();
        }
        self.notifications.remove(current_did).unwrap_or_default()
    }
}
