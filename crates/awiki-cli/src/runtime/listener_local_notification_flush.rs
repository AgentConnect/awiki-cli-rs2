use crate::runtime::listener_local_notifications::{LocalNotification, LocalNotificationQueue};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalNotificationFlushTargetSession {
    pub context_id: String,
    pub session_id: String,
    pub current_did: Option<String>,
}

impl LocalNotificationFlushTargetSession {
    pub fn new(
        context_id: impl Into<String>,
        session_id: impl Into<String>,
        current_did: Option<impl Into<String>>,
    ) -> Self {
        Self {
            context_id: context_id.into(),
            session_id: session_id.into(),
            current_did: current_did.map(Into::into),
        }
    }

    pub fn current_did(&self) -> Option<&str> {
        self.current_did.as_deref()
    }
}

pub fn flush_queued_local_notifications(
    queue: &mut LocalNotificationQueue,
    target_session: Option<&LocalNotificationFlushTargetSession>,
    mut handle_notification: impl FnMut(&str, &LocalNotificationFlushTargetSession, LocalNotification),
) {
    let Some(target_session) = target_session else {
        return;
    };
    let queued = queue.flush_queued_local_notifications(target_session.current_did());
    for notification in queued {
        handle_notification(&target_session.context_id, target_session, notification);
    }
}
