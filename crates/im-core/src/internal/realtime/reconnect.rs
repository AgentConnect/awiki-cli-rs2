use std::time::Duration;

pub const SESSION_RECONNECT_BASE_DELAY: Duration = Duration::from_secs(1);
pub const SESSION_RECONNECT_MAX_DELAY: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextSleep {
    Completed,
    Cancelled,
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

fn capped_double_delay(delay: Duration) -> Duration {
    let doubled = delay.saturating_mul(2);
    if doubled > SESSION_RECONNECT_MAX_DELAY {
        SESSION_RECONNECT_MAX_DELAY
    } else {
        doubled
    }
}
