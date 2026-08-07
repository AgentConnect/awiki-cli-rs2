use std::collections::VecDeque;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SystemNotificationState {
    Pending,
    ChallengeSent,
    ResponseVerified,
    Consumed,
    Cancelled,
    Rejected,
    Expired,
}

impl SystemNotificationState {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::ChallengeSent => "challenge_sent",
            Self::ResponseVerified => "response_verified",
            Self::Consumed => "consumed",
            Self::Cancelled => "cancelled",
            Self::Rejected => "rejected",
            Self::Expired => "expired",
        }
    }

    pub(crate) fn parse(value: &str) -> crate::ImResult<Self> {
        match value {
            "pending" => Ok(Self::Pending),
            "challenge_sent" => Ok(Self::ChallengeSent),
            "response_verified" => Ok(Self::ResponseVerified),
            "consumed" => Ok(Self::Consumed),
            "cancelled" => Ok(Self::Cancelled),
            "rejected" => Ok(Self::Rejected),
            "expired" => Ok(Self::Expired),
            _ => Err(crate::ImError::Serialization {
                detail: format!("unknown persisted system notification state {value:?}"),
            }),
        }
    }

    pub(crate) fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Consumed | Self::Cancelled | Self::Rejected | Self::Expired
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SystemNotificationKind {
    JoinRequested,
    JoinClaimed,
    JoinResponseVerified,
    JoinCompleted,
    JoinCancelled,
    JoinRejected,
    JoinExpired,
}

impl SystemNotificationKind {
    pub(crate) fn as_wire_type(self) -> &'static str {
        match self {
            Self::JoinRequested => "awiki.device.join-requested.v1",
            Self::JoinClaimed => "awiki.device.join-claimed.v1",
            Self::JoinResponseVerified => "awiki.device.join-response-verified.v1",
            Self::JoinCompleted => "awiki.device.join-completed.v1",
            Self::JoinCancelled => "awiki.device.join-cancelled.v1",
            Self::JoinRejected => "awiki.device.join-rejected.v1",
            Self::JoinExpired => "awiki.device.join-expired.v1",
        }
    }

    pub(crate) fn parse(value: &str) -> crate::ImResult<Self> {
        match value {
            "awiki.device.join-requested.v1" => Ok(Self::JoinRequested),
            "awiki.device.join-claimed.v1" => Ok(Self::JoinClaimed),
            "awiki.device.join-response-verified.v1" => Ok(Self::JoinResponseVerified),
            "awiki.device.join-completed.v1" => Ok(Self::JoinCompleted),
            "awiki.device.join-cancelled.v1" => Ok(Self::JoinCancelled),
            "awiki.device.join-rejected.v1" => Ok(Self::JoinRejected),
            "awiki.device.join-expired.v1" => Ok(Self::JoinExpired),
            _ => Err(crate::ImError::Serialization {
                detail: format!("unknown persisted system notification type {value:?}"),
            }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SystemNotificationSnapshot {
    pub event_id: String,
    pub did: String,
    pub join_session_id: String,
    pub kind: SystemNotificationKind,
    pub state: SystemNotificationState,
    pub session_revision: u64,
    pub issued_at: String,
    pub expires_at: String,
    pub first_seen_at: String,
    pub terminal: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SystemNotificationListQuery {
    pub limit: Option<u32>,
    pub include_terminal: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SystemNotificationChange {
    Reset {
        items: Vec<SystemNotificationSnapshot>,
    },
    Changed {
        item: SystemNotificationSnapshot,
    },
    RepairRequired {
        reason: String,
    },
}

pub struct SystemNotificationChangeSession {
    pub(crate) store:
        Arc<crate::internal::runtime_store::system_notification_store::SystemNotificationStore>,
    pub(crate) receiver: tokio::sync::broadcast::Receiver<SystemNotificationChange>,
    pub(crate) initial: VecDeque<SystemNotificationChange>,
    closed: bool,
}

impl SystemNotificationChangeSession {
    pub(crate) fn new(
        store: Arc<
            crate::internal::runtime_store::system_notification_store::SystemNotificationStore,
        >,
        receiver: tokio::sync::broadcast::Receiver<SystemNotificationChange>,
        initial: Vec<SystemNotificationChange>,
    ) -> Self {
        Self {
            store,
            receiver,
            initial: initial.into(),
            closed: false,
        }
    }

    pub async fn next_change(&mut self) -> Option<SystemNotificationChange> {
        if self.closed {
            return None;
        }
        if let Some(change) = self.initial.pop_front() {
            return Some(change);
        }
        match self.receiver.recv().await {
            Ok(change) => Some(change),
            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                Some(self.store.repair_required("subscriber_lag"))
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => None,
        }
    }

    pub async fn stop(&mut self) -> crate::ImResult<()> {
        self.closed = true;
        self.receiver.resubscribe();
        Ok(())
    }
}
