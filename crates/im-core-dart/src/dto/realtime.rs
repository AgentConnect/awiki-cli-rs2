use super::message::DartMessage;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartRealtimeCapability {
    pub status_supported: bool,
    pub connect_supported: bool,
    pub runner_exposed: bool,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartRealtimeStatus {
    pub connected: bool,
    pub state: String,
    pub subscriptions: Vec<String>,
    pub last_error: Option<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartRealtimeOptions {
    pub reconnect: String,
    pub event_buffer: u32,
    pub reconnect_delay_ms: Option<u64>,
    pub reconnect_base_delay_ms: Option<u64>,
    pub reconnect_max_delay_ms: Option<u64>,
    pub reconnect_max_attempts: Option<u32>,
    pub subscriptions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartRealtimeEvent {
    pub kind: String,
    pub state: Option<String>,
    pub reason: Option<String>,
    pub message: Option<DartMessage>,
    pub message_id: Option<String>,
    pub thread_kind: Option<String>,
    pub thread_id: Option<String>,
    pub update_kind: Option<String>,
    pub group: Option<String>,
    pub notification_id: Option<String>,
    pub title: Option<String>,
    pub body: Option<String>,
    pub source: Option<String>,
    pub host_kind: Option<String>,
    pub content_type: Option<String>,
    pub notification_type: Option<String>,
}
