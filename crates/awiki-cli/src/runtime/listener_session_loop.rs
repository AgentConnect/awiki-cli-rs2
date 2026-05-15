use std::time::Duration;

pub const SESSION_RECONNECT_BASE_DELAY: Duration = Duration::from_secs(1);
pub const SESSION_RECONNECT_MAX_DELAY: Duration = Duration::from_secs(30);
pub const SECURE_PREKEY_RETRY_DELAY: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextSleep {
    Completed,
    Cancelled,
}

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
pub enum SessionLoopRetryPhase {
    ConnectFailure,
    ConsumeFailure,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionLoopRetryDecision {
    Retry {
        phase: SessionLoopRetryPhase,
        sleep_delay: Duration,
        next_delay: Duration,
    },
    ExitAfterCancelledSleep {
        phase: SessionLoopRetryPhase,
        sleep_delay: Duration,
        next_delay: Duration,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsumeFinishedDecision {
    ExitAfterSessionCancelled { next_delay: Duration },
    Sleep(SessionLoopRetryDecision),
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionLoopBackoff {
    next_delay: Duration,
}

impl Default for SessionLoopBackoff {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionLoopBackoff {
    pub fn new() -> Self {
        Self {
            next_delay: SESSION_RECONNECT_BASE_DELAY,
        }
    }

    pub fn next_delay(&self) -> Duration {
        self.next_delay
    }

    pub fn connected(&mut self) -> Duration {
        self.next_delay = SESSION_RECONNECT_BASE_DELAY;
        self.next_delay
    }

    pub fn connect_failed(&mut self, sleep: ContextSleep) -> SessionLoopRetryDecision {
        self.retry_after_failure(SessionLoopRetryPhase::ConnectFailure, sleep)
    }

    pub fn consume_finished(
        &mut self,
        session_cancelled: bool,
        sleep: ContextSleep,
    ) -> ConsumeFinishedDecision {
        if session_cancelled {
            return ConsumeFinishedDecision::ExitAfterSessionCancelled {
                next_delay: self.next_delay,
            };
        }
        ConsumeFinishedDecision::Sleep(
            self.retry_after_failure(SessionLoopRetryPhase::ConsumeFailure, sleep),
        )
    }

    fn retry_after_failure(
        &mut self,
        phase: SessionLoopRetryPhase,
        sleep: ContextSleep,
    ) -> SessionLoopRetryDecision {
        let sleep_delay = self.next_delay;
        match sleep {
            ContextSleep::Completed => {
                self.next_delay = capped_double_delay(self.next_delay);
                SessionLoopRetryDecision::Retry {
                    phase,
                    sleep_delay,
                    next_delay: self.next_delay,
                }
            }
            ContextSleep::Cancelled => SessionLoopRetryDecision::ExitAfterCancelledSleep {
                phase,
                sleep_delay,
                next_delay: self.next_delay,
            },
        }
    }
}

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

fn capped_double_delay(delay: Duration) -> Duration {
    let doubled = delay.saturating_mul(2);
    if doubled > SESSION_RECONNECT_MAX_DELAY {
        SESSION_RECONNECT_MAX_DELAY
    } else {
        doubled
    }
}
