use std::collections::{BTreeMap, VecDeque};

use serde_json::{Map, Value};

use super::frame::{
    build_ws_rpc_request, classify_incoming_message, next_ws_rpc_request_id,
    pending_failure_response, IncomingWsMessage,
};
use super::notification::{ListenerWsNotificationQueue, LISTENER_WS_NOTIFICATION_QUEUE_CAPACITY};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListenerWsDispatchOutcome {
    RoutedResponse { request_id: String },
    DroppedResponse { request_id: String },
    QueuedNotification,
    DroppedNotification,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ListenerWsPendingDispatch {
    pending: BTreeMap<String, VecDeque<Map<String, Value>>>,
    notifications: ListenerWsNotificationQueue,
}

impl Default for ListenerWsPendingDispatch {
    fn default() -> Self {
        Self::with_notification_capacity(LISTENER_WS_NOTIFICATION_QUEUE_CAPACITY)
    }
}

impl ListenerWsPendingDispatch {
    pub fn with_notification_capacity(notification_capacity: usize) -> Self {
        Self {
            pending: BTreeMap::new(),
            notifications: ListenerWsNotificationQueue::with_capacity(notification_capacity),
        }
    }

    pub fn prepare_ws_rpc_request(
        &mut self,
        next_id: &mut i64,
        method: &str,
        params: Option<Map<String, Value>>,
    ) -> Map<String, Value> {
        let request_id = next_ws_rpc_request_id(next_id);
        self.register_pending(request_id.clone());
        build_ws_rpc_request(&request_id, method, params)
    }

    pub fn register_pending(&mut self, request_id: impl Into<String>) -> bool {
        self.pending
            .insert(request_id.into(), VecDeque::new())
            .is_none()
    }

    pub fn remove_pending(&mut self, request_id: &str) -> Option<Vec<Map<String, Value>>> {
        self.pending
            .remove(request_id)
            .map(|responses| responses.into_iter().collect())
    }

    pub fn has_pending(&self, request_id: &str) -> bool {
        self.pending.contains_key(request_id)
    }

    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }

    pub fn take_pending_response(&mut self, request_id: &str) -> Option<Map<String, Value>> {
        self.pending
            .get_mut(request_id)
            .and_then(VecDeque::pop_front)
    }

    pub fn route_incoming_message(
        &mut self,
        message: Map<String, Value>,
    ) -> ListenerWsDispatchOutcome {
        match classify_incoming_message(&message) {
            IncomingWsMessage::Response { request_id } => {
                if let Some(responses) = self.pending.get_mut(&request_id) {
                    responses.push_back(message);
                    ListenerWsDispatchOutcome::RoutedResponse { request_id }
                } else {
                    ListenerWsDispatchOutcome::DroppedResponse { request_id }
                }
            }
            IncomingWsMessage::Notification => self.queue_notification(message),
        }
    }

    pub fn fail_pending_requests(&mut self, error: &str) -> Vec<String> {
        let request_ids = self.pending.keys().cloned().collect::<Vec<_>>();
        for request_id in &request_ids {
            if let Some(responses) = self.pending.get_mut(request_id) {
                responses.push_back(pending_failure_response(request_id, error));
            }
        }
        request_ids
    }

    pub fn queue_notification(
        &mut self,
        notification: Map<String, Value>,
    ) -> ListenerWsDispatchOutcome {
        if !self.notifications.push(notification) {
            return ListenerWsDispatchOutcome::DroppedNotification;
        }
        ListenerWsDispatchOutcome::QueuedNotification
    }

    pub fn pop_notification(&mut self) -> Option<Map<String, Value>> {
        self.notifications.pop()
    }

    pub fn notification_len(&self) -> usize {
        self.notifications.len()
    }
}
