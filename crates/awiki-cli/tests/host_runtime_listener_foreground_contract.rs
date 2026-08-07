use awiki_cli::host_runtime::listener_foreground::{
    listener_accept_loop_step, listener_foreground_run_plan, listener_start_socket_plan,
    ListenerAcceptLoopAction, ListenerAcceptLoopDecision, ListenerAcceptLoopEvent,
    ListenerForegroundDecision, ListenerForegroundRunAction, ListenerStartSocketAction,
};
use awiki_cli::m_core_cli_adapter::realtime::{
    listener_runner_selection, ListenerRunHostKind, ListenerRunnerAction, ListenerRunnerMode,
};

#[test]
fn run_rejects_non_websocket_mode_before_side_effects_like_go() {
    let plan = listener_foreground_run_plan(
        "http",
        "/tmp/awiki.sock",
        Some("pid should not run"),
        Some("status should not run"),
        Some("listen should not run"),
        Some("sessions should not run"),
    );

    assert_eq!(
        plan.actions,
        vec![ListenerForegroundRunAction::ValidateWebSocketMode {
            mode: "http".to_string(),
        }]
    );
    assert_eq!(
        plan.decision,
        ListenerForegroundDecision::ReturnError(
            "runtime mode must be websocket before starting the listener".to_string(),
        )
    );
}

#[test]
fn run_writes_pid_then_status_before_starting_socket() {
    let plan = listener_foreground_run_plan("websocket", "/tmp/awiki.sock", None, None, None, None);

    assert_eq!(
        plan.actions,
        vec![
            ListenerForegroundRunAction::ValidateWebSocketMode {
                mode: "websocket".to_string(),
            },
            ListenerForegroundRunAction::WritePid,
            ListenerForegroundRunAction::WriteStatus,
            ListenerForegroundRunAction::StartSocket(ListenerStartSocketAction::ListenBridge {
                socket_path: "/tmp/awiki.sock".to_string(),
            }),
            ListenerForegroundRunAction::StartSocket(ListenerStartSocketAction::StoreListener),
            ListenerForegroundRunAction::StartSocket(
                ListenerStartSocketAction::SetBridgeAvailable { available: true },
            ),
            ListenerForegroundRunAction::StartSocket(ListenerStartSocketAction::SpawnAcceptLoop),
            ListenerForegroundRunAction::StartKnownSessions,
            ListenerForegroundRunAction::SpawnWatchNewIdentities,
            ListenerForegroundRunAction::RunImCoreRealtimeRunner,
            ListenerForegroundRunAction::WaitForContextDone,
        ]
    );
    assert_eq!(plan.decision, ListenerForegroundDecision::ReturnOk);
}

#[test]
fn listener_runner_selection_defaults_foreground_and_service_to_sdk_hosts() {
    let foreground = listener_runner_selection(ListenerRunHostKind::Foreground);
    assert_eq!(foreground.mode, ListenerRunnerMode::ImCore);
    assert_eq!(
        foreground.actions,
        vec![ListenerRunnerAction::UseImCoreRunner {
            host: ListenerRunHostKind::Foreground,
        }]
    );

    let service = listener_runner_selection(ListenerRunHostKind::Service);
    assert_eq!(service.mode, ListenerRunnerMode::ImCore);
    assert_eq!(
        service.actions,
        vec![ListenerRunnerAction::UseImCoreRunner {
            host: ListenerRunHostKind::Service,
        }]
    );
}

#[test]
fn listener_binds_exact_local_alias_and_never_guesses_handle_recovery_transition() {
    let source = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src/host_runtime/listener_supervisor_run.rs"),
    )
    .unwrap();

    assert!(source.contains("IdentitySelector::LocalAlias(identity_name.clone())"));
    assert!(source.contains("client.active_sync_account_binding().await"));
    assert!(source.contains("ListenerSyncDisposition::AuthRevoked"));
    assert!(source.contains("session.stop().await"));
    for forbidden in [
        "HandleRecovery",
        "handle_recovery",
        "recover_handle",
        "recover-handle",
    ] {
        assert!(!source.contains(forbidden), "{forbidden}");
    }
}

#[test]
fn pid_error_stops_before_status_write_like_go() {
    let plan = listener_foreground_run_plan(
        "websocket",
        "/tmp/awiki.sock",
        Some("write pid failed"),
        None,
        None,
        None,
    );

    assert_eq!(
        plan.actions,
        vec![
            ListenerForegroundRunAction::ValidateWebSocketMode {
                mode: "websocket".to_string(),
            },
            ListenerForegroundRunAction::WritePid,
        ]
    );
    assert_eq!(
        plan.decision,
        ListenerForegroundDecision::ReturnError("write pid failed".to_string())
    );
}

#[test]
fn status_error_stops_before_socket_start_like_go() {
    let plan = listener_foreground_run_plan(
        "websocket",
        "/tmp/awiki.sock",
        None,
        Some("write status failed"),
        None,
        None,
    );

    assert_eq!(
        plan.actions,
        vec![
            ListenerForegroundRunAction::ValidateWebSocketMode {
                mode: "websocket".to_string(),
            },
            ListenerForegroundRunAction::WritePid,
            ListenerForegroundRunAction::WriteStatus,
        ]
    );
    assert_eq!(
        plan.decision,
        ListenerForegroundDecision::ReturnError("write status failed".to_string())
    );
}

#[test]
fn start_socket_listen_error_stops_before_listener_store_and_bridge_available() {
    let plan = listener_start_socket_plan("/tmp/awiki.sock", Some("listen failed"));

    assert_eq!(
        plan.actions,
        vec![ListenerStartSocketAction::ListenBridge {
            socket_path: "/tmp/awiki.sock".to_string(),
        }]
    );
    assert_eq!(
        plan.decision,
        ListenerForegroundDecision::ReturnError("listen failed".to_string())
    );
}

#[test]
fn start_socket_success_stores_listener_sets_bridge_available_and_spawns_accept_loop() {
    let plan = listener_start_socket_plan("/tmp/awiki.sock", None);

    assert_eq!(
        plan.actions,
        vec![
            ListenerStartSocketAction::ListenBridge {
                socket_path: "/tmp/awiki.sock".to_string(),
            },
            ListenerStartSocketAction::StoreListener,
            ListenerStartSocketAction::SetBridgeAvailable { available: true },
            ListenerStartSocketAction::SpawnAcceptLoop,
        ]
    );
    assert_eq!(plan.decision, ListenerForegroundDecision::ReturnOk);
}

#[test]
fn run_propagates_socket_listen_error_before_starting_known_sessions() {
    let plan = listener_foreground_run_plan(
        "websocket",
        "/tmp/awiki.sock",
        None,
        None,
        Some("listen failed"),
        None,
    );

    assert_eq!(
        plan.actions,
        vec![
            ListenerForegroundRunAction::ValidateWebSocketMode {
                mode: "websocket".to_string(),
            },
            ListenerForegroundRunAction::WritePid,
            ListenerForegroundRunAction::WriteStatus,
            ListenerForegroundRunAction::StartSocket(ListenerStartSocketAction::ListenBridge {
                socket_path: "/tmp/awiki.sock".to_string(),
            }),
        ]
    );
    assert_eq!(
        plan.decision,
        ListenerForegroundDecision::ReturnError("listen failed".to_string())
    );
}

#[test]
fn known_sessions_error_stops_before_identity_watch_like_go() {
    let plan = listener_foreground_run_plan(
        "websocket",
        "/tmp/awiki.sock",
        None,
        None,
        None,
        Some("list identities failed"),
    );

    assert_eq!(
        plan.actions,
        vec![
            ListenerForegroundRunAction::ValidateWebSocketMode {
                mode: "websocket".to_string(),
            },
            ListenerForegroundRunAction::WritePid,
            ListenerForegroundRunAction::WriteStatus,
            ListenerForegroundRunAction::StartSocket(ListenerStartSocketAction::ListenBridge {
                socket_path: "/tmp/awiki.sock".to_string(),
            }),
            ListenerForegroundRunAction::StartSocket(ListenerStartSocketAction::StoreListener),
            ListenerForegroundRunAction::StartSocket(
                ListenerStartSocketAction::SetBridgeAvailable { available: true },
            ),
            ListenerForegroundRunAction::StartSocket(ListenerStartSocketAction::SpawnAcceptLoop),
            ListenerForegroundRunAction::StartKnownSessions,
        ]
    );
    assert_eq!(
        plan.decision,
        ListenerForegroundDecision::ReturnError("list identities failed".to_string())
    );
}

#[test]
fn accept_loop_accepted_connection_spawns_handle_conn_and_continues() {
    let step = listener_accept_loop_step(ListenerAcceptLoopEvent::Accepted {
        connection_id: "conn-1".to_string(),
    });

    assert_eq!(
        step.actions,
        vec![ListenerAcceptLoopAction::SpawnHandleConn {
            connection_id: "conn-1".to_string(),
        }]
    );
    assert_eq!(step.decision, ListenerAcceptLoopDecision::Continue);
}

#[test]
fn accept_loop_accept_error_returns_without_spawning_handler() {
    let step = listener_accept_loop_step(ListenerAcceptLoopEvent::AcceptError {
        error: "listener closed".to_string(),
    });

    assert!(step.actions.is_empty());
    assert_eq!(step.decision, ListenerAcceptLoopDecision::Exit);
}
