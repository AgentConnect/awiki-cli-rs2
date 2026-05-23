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
