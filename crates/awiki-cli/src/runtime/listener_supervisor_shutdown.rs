#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ShutdownSession {
    pub identity_name: String,
    pub has_cancel: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SupervisorShutdownAction {
    LockSessions,
    CancelSession { identity_name: String },
    CloseCurrentClient { identity_name: String },
    UnlockSessions,
    CloseListener,
    CloseHostNotify,
    CloseDatabase,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SupervisorShutdownDecision {
    ReturnOk,
    ReturnDatabaseError(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorShutdownPlan {
    pub actions: Vec<SupervisorShutdownAction>,
    pub ignored_listener_error: Option<String>,
    pub ignored_host_notify_error: Option<String>,
    pub decision: SupervisorShutdownDecision,
}

pub fn supervisor_shutdown_plan(
    sessions: &[ShutdownSession],
    has_listener: bool,
    listener_close_error: Option<&str>,
    has_host_notify: bool,
    host_notify_close_error: Option<&str>,
    has_database: bool,
    database_close_error: Option<&str>,
) -> SupervisorShutdownPlan {
    let mut actions = vec![SupervisorShutdownAction::LockSessions];
    for session in sessions {
        if session.has_cancel {
            actions.push(SupervisorShutdownAction::CancelSession {
                identity_name: session.identity_name.clone(),
            });
        }
        actions.push(SupervisorShutdownAction::CloseCurrentClient {
            identity_name: session.identity_name.clone(),
        });
    }
    actions.push(SupervisorShutdownAction::UnlockSessions);

    let ignored_listener_error = if has_listener {
        actions.push(SupervisorShutdownAction::CloseListener);
        listener_close_error.map(str::to_string)
    } else {
        None
    };
    let ignored_host_notify_error = if has_host_notify {
        actions.push(SupervisorShutdownAction::CloseHostNotify);
        host_notify_close_error.map(str::to_string)
    } else {
        None
    };
    let decision = if has_database {
        actions.push(SupervisorShutdownAction::CloseDatabase);
        match database_close_error {
            Some(error) => SupervisorShutdownDecision::ReturnDatabaseError(error.to_string()),
            None => SupervisorShutdownDecision::ReturnOk,
        }
    } else {
        SupervisorShutdownDecision::ReturnOk
    };

    SupervisorShutdownPlan {
        actions,
        ignored_listener_error,
        ignored_host_notify_error,
        decision,
    }
}
