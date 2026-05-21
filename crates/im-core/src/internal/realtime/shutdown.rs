#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RealtimeShutdownDecision {
    Continue,
    Exit,
}

pub fn shutdown_decision(shutdown_requested: bool) -> RealtimeShutdownDecision {
    if shutdown_requested {
        RealtimeShutdownDecision::Exit
    } else {
        RealtimeShutdownDecision::Continue
    }
}
