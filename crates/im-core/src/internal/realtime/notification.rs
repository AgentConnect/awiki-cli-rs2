use std::collections::VecDeque;

use serde_json::{Map, Value};

pub const LISTENER_WS_NOTIFICATION_QUEUE_CAPACITY: usize = 128;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ListenerWsNotificationQueue {
    notifications: VecDeque<Map<String, Value>>,
    capacity: usize,
}

impl ListenerWsNotificationQueue {
    pub(crate) fn with_capacity(capacity: usize) -> Self {
        Self {
            notifications: VecDeque::new(),
            capacity,
        }
    }

    pub(crate) fn push(&mut self, notification: Map<String, Value>) -> bool {
        if self.notifications.len() >= self.capacity {
            return false;
        }
        self.notifications.push_back(notification);
        true
    }

    pub(crate) fn pop(&mut self) -> Option<Map<String, Value>> {
        self.notifications.pop_front()
    }

    pub(crate) fn len(&self) -> usize {
        self.notifications.len()
    }
}
