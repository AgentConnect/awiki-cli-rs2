use std::time::Duration;

pub const SECURE_PREKEY_RETRY_DELAY: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionLoopStartDecision {
    Connect,
    CloseCurrentClientAndExit,
}

pub fn session_loop_start_decision(session_cancelled: bool) -> SessionLoopStartDecision {
    if session_cancelled {
        SessionLoopStartDecision::CloseCurrentClientAndExit
    } else {
        SessionLoopStartDecision::Connect
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectedSessionAction {
    MarkConnected,
    SignalInitialSuccess,
    RefreshStatus,
    FlushQueuedLocalNotifications,
    StartSecurePrekeyRetry,
    StartUnreadSecureDirectInboxPolling,
    ConsumeNotifications,
}

pub const CONNECTED_SESSION_ACTIONS: &[ConnectedSessionAction] = &[
    ConnectedSessionAction::MarkConnected,
    ConnectedSessionAction::SignalInitialSuccess,
    ConnectedSessionAction::RefreshStatus,
    ConnectedSessionAction::FlushQueuedLocalNotifications,
    ConnectedSessionAction::StartSecurePrekeyRetry,
    ConnectedSessionAction::StartUnreadSecureDirectInboxPolling,
    ConnectedSessionAction::ConsumeNotifications,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsumeFinishedAction {
    CancelChildTasks,
    CloseClient,
    MarkDisconnected,
    RefreshStatus,
}

pub const CONSUME_FINISHED_ACTIONS: &[ConsumeFinishedAction] = &[
    ConsumeFinishedAction::CancelChildTasks,
    ConsumeFinishedAction::CloseClient,
    ConsumeFinishedAction::MarkDisconnected,
    ConsumeFinishedAction::RefreshStatus,
];

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct InitialSessionSignal {
    signaled: bool,
}

impl InitialSessionSignal {
    pub fn signal(&mut self) -> bool {
        if self.signaled {
            false
        } else {
            self.signaled = true;
            true
        }
    }

    pub fn has_signaled(&self) -> bool {
        self.signaled
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SecurePrekeyRetryDecision {
    Finished,
    Retry {
        log_line: String,
        sleep_delay: Duration,
    },
}

pub fn secure_prekey_retry_decision(
    identity_name: &str,
    warnings: &[String],
) -> SecurePrekeyRetryDecision {
    if warnings.is_empty() {
        return SecurePrekeyRetryDecision::Finished;
    }
    SecurePrekeyRetryDecision::Retry {
        log_line: format!(
            "listener secure prekey publish retry identity={} warnings={}",
            identity_name,
            warnings.join("; ")
        ),
        sleep_delay: SECURE_PREKEY_RETRY_DELAY,
    }
}
