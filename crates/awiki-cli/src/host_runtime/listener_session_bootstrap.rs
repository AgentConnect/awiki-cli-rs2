use std::time::Duration;

pub const SESSION_BOOTSTRAP_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CurrentIdentityLookup {
    NotNeeded,
    Ok { identity_name: String },
    Error(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionBootstrapOutcome {
    InitialOk,
    InitialError(String),
    Timeout,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnsureSessionAction {
    ResolveCurrentIdentity,
    CheckExistingSession {
        identity_name: String,
    },
    CreateSession {
        identity_name: String,
    },
    InsertSession {
        identity_name: String,
    },
    StartRunSessionLoop {
        identity_name: String,
    },
    WaitInitialResult {
        identity_name: String,
        timeout: Duration,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnsureSessionReturn {
    None,
    Existing { identity_name: String },
    New { identity_name: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnsureSessionDecision {
    ReturnOk {
        session: EnsureSessionReturn,
    },
    ReturnError {
        session: EnsureSessionReturn,
        error: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnsureSessionPlan {
    pub identity_name: String,
    pub actions: Vec<EnsureSessionAction>,
    pub decision: EnsureSessionDecision,
}

pub fn ensure_session_bootstrap_plan(
    requested_identity_name: &str,
    existing_session_names: &[String],
    current_identity: CurrentIdentityLookup,
    bootstrap_outcome: SessionBootstrapOutcome,
) -> EnsureSessionPlan {
    let mut actions = Vec::new();
    let requested = requested_identity_name.trim();
    let identity_name = if requested.is_empty() {
        actions.push(EnsureSessionAction::ResolveCurrentIdentity);
        match current_identity {
            CurrentIdentityLookup::Ok { identity_name } => identity_name,
            CurrentIdentityLookup::Error(error) => {
                return EnsureSessionPlan {
                    identity_name: String::new(),
                    actions,
                    decision: EnsureSessionDecision::ReturnError {
                        session: EnsureSessionReturn::None,
                        error,
                    },
                };
            }
            CurrentIdentityLookup::NotNeeded => String::new(),
        }
    } else {
        requested.to_string()
    };

    actions.push(EnsureSessionAction::CheckExistingSession {
        identity_name: identity_name.clone(),
    });
    if existing_session_names
        .iter()
        .any(|existing| existing == &identity_name)
    {
        return EnsureSessionPlan {
            identity_name: identity_name.clone(),
            actions,
            decision: EnsureSessionDecision::ReturnOk {
                session: EnsureSessionReturn::Existing { identity_name },
            },
        };
    }

    actions.push(EnsureSessionAction::CreateSession {
        identity_name: identity_name.clone(),
    });
    actions.push(EnsureSessionAction::InsertSession {
        identity_name: identity_name.clone(),
    });
    actions.push(EnsureSessionAction::StartRunSessionLoop {
        identity_name: identity_name.clone(),
    });
    actions.push(EnsureSessionAction::WaitInitialResult {
        identity_name: identity_name.clone(),
        timeout: SESSION_BOOTSTRAP_TIMEOUT,
    });

    let session = EnsureSessionReturn::New {
        identity_name: identity_name.clone(),
    };
    let decision = match bootstrap_outcome {
        SessionBootstrapOutcome::InitialOk => EnsureSessionDecision::ReturnOk { session },
        SessionBootstrapOutcome::InitialError(error) => {
            EnsureSessionDecision::ReturnError { session, error }
        }
        SessionBootstrapOutcome::Timeout => EnsureSessionDecision::ReturnError {
            session,
            error: format!("websocket session bootstrap timed out for identity {identity_name}"),
        },
    };

    EnsureSessionPlan {
        identity_name,
        actions,
        decision,
    }
}
