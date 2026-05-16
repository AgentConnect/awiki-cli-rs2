use super::bridge::MODE_WEBSOCKET;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListenerStartSocketAction {
    ListenBridge { socket_path: String },
    StoreListener,
    SetBridgeAvailable { available: bool },
    SpawnAcceptLoop,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListenerForegroundRunAction {
    ValidateWebSocketMode { mode: String },
    WritePid,
    WriteStatus,
    StartSocket(ListenerStartSocketAction),
    StartKnownSessions,
    SpawnWatchNewIdentities,
    WaitForContextDone,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListenerForegroundDecision {
    ReturnOk,
    ReturnError(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListenerStartSocketPlan {
    pub actions: Vec<ListenerStartSocketAction>,
    pub decision: ListenerForegroundDecision,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListenerForegroundRunPlan {
    pub actions: Vec<ListenerForegroundRunAction>,
    pub decision: ListenerForegroundDecision,
}

pub fn listener_start_socket_plan(
    socket_path: &str,
    listen_bridge_error: Option<&str>,
) -> ListenerStartSocketPlan {
    let mut actions = vec![ListenerStartSocketAction::ListenBridge {
        socket_path: socket_path.to_string(),
    }];

    if let Some(error) = listen_bridge_error {
        return ListenerStartSocketPlan {
            actions,
            decision: ListenerForegroundDecision::ReturnError(error.to_string()),
        };
    }

    actions.push(ListenerStartSocketAction::StoreListener);
    actions.push(ListenerStartSocketAction::SetBridgeAvailable { available: true });
    actions.push(ListenerStartSocketAction::SpawnAcceptLoop);

    ListenerStartSocketPlan {
        actions,
        decision: ListenerForegroundDecision::ReturnOk,
    }
}

pub fn listener_foreground_run_plan(
    runtime_mode: &str,
    socket_path: &str,
    write_pid_error: Option<&str>,
    write_status_error: Option<&str>,
    listen_bridge_error: Option<&str>,
    start_known_sessions_error: Option<&str>,
) -> ListenerForegroundRunPlan {
    let mut actions = vec![ListenerForegroundRunAction::ValidateWebSocketMode {
        mode: runtime_mode.to_string(),
    }];

    if runtime_mode != MODE_WEBSOCKET {
        return ListenerForegroundRunPlan {
            actions,
            decision: ListenerForegroundDecision::ReturnError(
                "runtime mode must be websocket before starting the listener".to_string(),
            ),
        };
    }

    actions.push(ListenerForegroundRunAction::WritePid);
    if let Some(error) = write_pid_error {
        return ListenerForegroundRunPlan {
            actions,
            decision: ListenerForegroundDecision::ReturnError(error.to_string()),
        };
    }

    actions.push(ListenerForegroundRunAction::WriteStatus);
    if let Some(error) = write_status_error {
        return ListenerForegroundRunPlan {
            actions,
            decision: ListenerForegroundDecision::ReturnError(error.to_string()),
        };
    }

    let start_socket_plan = listener_start_socket_plan(socket_path, listen_bridge_error);
    actions.extend(
        start_socket_plan
            .actions
            .into_iter()
            .map(ListenerForegroundRunAction::StartSocket),
    );
    if let ListenerForegroundDecision::ReturnError(error) = start_socket_plan.decision {
        return ListenerForegroundRunPlan {
            actions,
            decision: ListenerForegroundDecision::ReturnError(error),
        };
    }

    actions.push(ListenerForegroundRunAction::StartKnownSessions);
    if let Some(error) = start_known_sessions_error {
        return ListenerForegroundRunPlan {
            actions,
            decision: ListenerForegroundDecision::ReturnError(error.to_string()),
        };
    }

    actions.push(ListenerForegroundRunAction::SpawnWatchNewIdentities);
    actions.push(ListenerForegroundRunAction::WaitForContextDone);

    ListenerForegroundRunPlan {
        actions,
        decision: ListenerForegroundDecision::ReturnOk,
    }
}
