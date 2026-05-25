use std::collections::BTreeSet;
use std::time::Duration;

pub const WATCH_NEW_IDENTITIES_INTERVAL: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ListenerIdentitySummary {
    pub identity_name: String,
    pub did: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartKnownSessionsEvent {
    ManagerListError(String),
    Identities(Vec<ListenerIdentitySummary>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnsureSessionOutcome {
    Ok,
    Error(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartKnownSessionEnsureOutcome {
    Ensure(EnsureSessionOutcome),
    ContextDone(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WatchNewIdentitiesEvent {
    ContextDone,
    TickManagerListError,
    TickIdentities(Vec<ListenerIdentitySummary>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListenerIdentityWatchAction {
    StartTicker {
        interval: Duration,
    },
    StopTicker,
    EnsureSession {
        identity_name: String,
    },
    RecordSessionError {
        identity_name: String,
        did: String,
        error: String,
    },
    RefreshStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartKnownSessionsDecision {
    ReturnOk,
    ReturnError(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartKnownSessionsStep {
    pub actions: Vec<ListenerIdentityWatchAction>,
    pub decision: StartKnownSessionsDecision,
}

pub fn start_known_sessions_step(
    event: StartKnownSessionsEvent,
    ensure_outcomes: &[StartKnownSessionEnsureOutcome],
) -> StartKnownSessionsStep {
    match event {
        StartKnownSessionsEvent::ManagerListError(error) => StartKnownSessionsStep {
            actions: Vec::new(),
            decision: StartKnownSessionsDecision::ReturnError(error),
        },
        StartKnownSessionsEvent::Identities(identities) => {
            let mut actions = Vec::new();
            for (index, summary) in identities.iter().enumerate() {
                match ensure_outcomes.get(index).cloned().unwrap_or(
                    StartKnownSessionEnsureOutcome::Ensure(EnsureSessionOutcome::Ok),
                ) {
                    StartKnownSessionEnsureOutcome::ContextDone(error) => {
                        return StartKnownSessionsStep {
                            actions,
                            decision: StartKnownSessionsDecision::ReturnError(error),
                        };
                    }
                    StartKnownSessionEnsureOutcome::Ensure(EnsureSessionOutcome::Ok) => {
                        actions.push(ListenerIdentityWatchAction::EnsureSession {
                            identity_name: summary.identity_name.clone(),
                        });
                    }
                    StartKnownSessionEnsureOutcome::Ensure(EnsureSessionOutcome::Error(error)) => {
                        actions.push(ListenerIdentityWatchAction::EnsureSession {
                            identity_name: summary.identity_name.clone(),
                        });
                        actions.push(ListenerIdentityWatchAction::RecordSessionError {
                            identity_name: summary.identity_name.clone(),
                            did: summary.did.clone(),
                            error: error.to_string(),
                        });
                        actions.push(ListenerIdentityWatchAction::RefreshStatus);
                    }
                }
            }
            actions.push(ListenerIdentityWatchAction::RefreshStatus);
            StartKnownSessionsStep {
                actions,
                decision: StartKnownSessionsDecision::ReturnOk,
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WatchNewIdentitiesDecision {
    Continue,
    Exit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchNewIdentitiesStep {
    pub actions: Vec<ListenerIdentityWatchAction>,
    pub decision: WatchNewIdentitiesDecision,
}

pub fn watch_new_identities_start_step() -> WatchNewIdentitiesStep {
    WatchNewIdentitiesStep {
        actions: vec![ListenerIdentityWatchAction::StartTicker {
            interval: WATCH_NEW_IDENTITIES_INTERVAL,
        }],
        decision: WatchNewIdentitiesDecision::Continue,
    }
}

pub fn watch_new_identities_step(
    event: WatchNewIdentitiesEvent,
    existing_identity_names: &[String],
    ensure_outcome_for_identity: impl Fn(&str) -> EnsureSessionOutcome,
) -> WatchNewIdentitiesStep {
    match event {
        WatchNewIdentitiesEvent::ContextDone => WatchNewIdentitiesStep {
            actions: vec![ListenerIdentityWatchAction::StopTicker],
            decision: WatchNewIdentitiesDecision::Exit,
        },
        WatchNewIdentitiesEvent::TickManagerListError => WatchNewIdentitiesStep {
            actions: Vec::new(),
            decision: WatchNewIdentitiesDecision::Continue,
        },
        WatchNewIdentitiesEvent::TickIdentities(identities) => {
            let mut actions = Vec::new();
            let mut seen = existing_identity_names
                .iter()
                .cloned()
                .collect::<BTreeSet<_>>();
            for summary in identities {
                if seen.contains(&summary.identity_name) {
                    continue;
                }
                seen.insert(summary.identity_name.clone());
                actions.push(ListenerIdentityWatchAction::EnsureSession {
                    identity_name: summary.identity_name.clone(),
                });
                let outcome = ensure_outcome_for_identity(&summary.identity_name);
                if let EnsureSessionOutcome::Error(error) = outcome {
                    actions.push(ListenerIdentityWatchAction::RecordSessionError {
                        identity_name: summary.identity_name,
                        did: summary.did,
                        error,
                    });
                    actions.push(ListenerIdentityWatchAction::RefreshStatus);
                }
            }
            actions.push(ListenerIdentityWatchAction::RefreshStatus);
            WatchNewIdentitiesStep {
                actions,
                decision: WatchNewIdentitiesDecision::Continue,
            }
        }
    }
}
