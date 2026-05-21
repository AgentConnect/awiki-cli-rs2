use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RealtimeOptions {
    pub reconnect: ReconnectPolicy,
    pub event_buffer: usize,
    pub subscriptions: Vec<RealtimeSubscription>,
}

impl Default for RealtimeOptions {
    fn default() -> Self {
        Self {
            reconnect: ReconnectPolicy::Disabled,
            event_buffer: 128,
            subscriptions: vec![
                RealtimeSubscription::Messages,
                RealtimeSubscription::Groups,
                RealtimeSubscription::Notifications,
            ],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RealtimeSubscription {
    Messages,
    Groups,
    Notifications,
    HostNotifications,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReconnectPolicy {
    Disabled,
    Fixed {
        delay_ms: u64,
        max_attempts: Option<u32>,
    },
    Exponential {
        base_delay_ms: u64,
        max_delay_ms: u64,
        max_attempts: Option<u32>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RealtimeStatus {
    pub connected: bool,
    pub state: RealtimeConnectionState,
    pub subscriptions: Vec<RealtimeSubscription>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RealtimeExit {
    pub reason: RealtimeExitReason,
    pub reconnect_attempts: u32,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RealtimeExitReason {
    ShutdownRequested,
    ConnectionClosed,
    AuthFailed,
    TransportUnavailable,
    FatalError,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RealtimeConnectionState {
    Disconnected,
    Connecting,
    Connected,
    Reconnecting,
    Closed,
}
