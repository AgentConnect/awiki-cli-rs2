use awiki_cli::runtime::listener_supervisor_init::{
    supervisor_init_plan, HostNotifyInitStatus, SupervisorInitAction, SupervisorInitDecision,
    SupervisorInitInputs, SupervisorInitialStatus,
};

#[test]
fn supervisor_init_success_builds_status_and_maps_in_go_order() {
    let inputs = sample_inputs();
    let plan = supervisor_init_plan(inputs.clone(), None, None, None, None, None, None);

    assert_eq!(
        plan.actions,
        vec![
            SupervisorInitAction::OpenStore,
            SupervisorInitAction::EnsureStoreSchema,
            SupervisorInitAction::ResolveRuntimeBootID,
            SupervisorInitAction::ResolveRuntimePaths,
            SupervisorInitAction::NewHostNotifySink,
            SupervisorInitAction::NewRemoteClient,
            SupervisorInitAction::NewIdentityManager,
            SupervisorInitAction::InitSessionsMap,
            SupervisorInitAction::InitLocalNotificationsMap,
            SupervisorInitAction::BuildSupervisorStatus(SupervisorInitialStatus {
                mode: "websocket".to_string(),
                installed: true,
                running: true,
                boot_id: "boot-123".to_string(),
                pid_file: "/runtime/listener.pid".to_string(),
                log_file: "/logs/listener.log".to_string(),
                status_file: "/runtime/listener.status.json".to_string(),
                socket_path: "/runtime/message-daemon.sock".to_string(),
                pid: 4242,
                started_at: "2026-05-17T05:15:00Z".to_string(),
                host_notify: inputs.host_notify_status,
            }),
        ]
    );
    assert_eq!(plan.decision, SupervisorInitDecision::ReturnSupervisor);
}

#[test]
fn open_store_error_returns_without_cleanup_like_go() {
    let plan = supervisor_init_plan(
        sample_inputs(),
        Some("open store failed"),
        None,
        None,
        None,
        None,
        None,
    );

    assert_eq!(plan.actions, vec![SupervisorInitAction::OpenStore]);
    assert_eq!(
        plan.decision,
        SupervisorInitDecision::ReturnError("open store failed".to_string())
    );
}

#[test]
fn store_schema_boot_id_paths_and_host_notify_errors_close_store_only() {
    let cases = [
        (
            Some("ensure schema failed"),
            None,
            None,
            None,
            "ensure schema failed",
            vec![
                SupervisorInitAction::OpenStore,
                SupervisorInitAction::EnsureStoreSchema,
                SupervisorInitAction::CloseStore,
            ],
        ),
        (
            None,
            Some("boot id failed"),
            None,
            None,
            "boot id failed",
            vec![
                SupervisorInitAction::OpenStore,
                SupervisorInitAction::EnsureStoreSchema,
                SupervisorInitAction::ResolveRuntimeBootID,
                SupervisorInitAction::CloseStore,
            ],
        ),
        (
            None,
            None,
            Some("paths failed"),
            None,
            "paths failed",
            vec![
                SupervisorInitAction::OpenStore,
                SupervisorInitAction::EnsureStoreSchema,
                SupervisorInitAction::ResolveRuntimeBootID,
                SupervisorInitAction::ResolveRuntimePaths,
                SupervisorInitAction::CloseStore,
            ],
        ),
        (
            None,
            None,
            None,
            Some("host notify failed"),
            "host notify failed",
            vec![
                SupervisorInitAction::OpenStore,
                SupervisorInitAction::EnsureStoreSchema,
                SupervisorInitAction::ResolveRuntimeBootID,
                SupervisorInitAction::ResolveRuntimePaths,
                SupervisorInitAction::NewHostNotifySink,
                SupervisorInitAction::CloseStore,
            ],
        ),
    ];

    for (schema_error, boot_id_error, paths_error, host_notify_error, want_error, actions) in cases
    {
        let plan = supervisor_init_plan(
            sample_inputs(),
            None,
            schema_error,
            boot_id_error,
            paths_error,
            host_notify_error,
            None,
        );

        assert_eq!(plan.actions, actions);
        assert_eq!(
            plan.decision,
            SupervisorInitDecision::ReturnError(want_error.to_string())
        );
    }
}

#[test]
fn remote_client_error_closes_store_then_host_notify_sink_like_go() {
    let plan = supervisor_init_plan(
        sample_inputs(),
        None,
        None,
        None,
        None,
        None,
        Some("remote client failed"),
    );

    assert_eq!(
        plan.actions,
        vec![
            SupervisorInitAction::OpenStore,
            SupervisorInitAction::EnsureStoreSchema,
            SupervisorInitAction::ResolveRuntimeBootID,
            SupervisorInitAction::ResolveRuntimePaths,
            SupervisorInitAction::NewHostNotifySink,
            SupervisorInitAction::NewRemoteClient,
            SupervisorInitAction::CloseStore,
            SupervisorInitAction::CloseHostNotifySink,
        ]
    );
    assert_eq!(
        plan.decision,
        SupervisorInitDecision::ReturnError("remote client failed".to_string())
    );
}

#[test]
fn supervisor_init_preserves_supplied_host_notify_status() {
    let mut inputs = sample_inputs();
    inputs.host_notify_status = HostNotifyInitStatus {
        enabled: true,
        sink: "openclaw".to_string(),
        file_path: String::new(),
        hook_url: "http://127.0.0.1:18789/hooks/notify".to_string(),
        agent_id: "main".to_string(),
        hook_name: "AWiki".to_string(),
        notify_url: String::new(),
    };
    let plan = supervisor_init_plan(inputs.clone(), None, None, None, None, None, None);

    let Some(SupervisorInitAction::BuildSupervisorStatus(status)) = plan.actions.last() else {
        panic!("last action should build status");
    };
    assert_eq!(status.host_notify, inputs.host_notify_status);
}

fn sample_inputs() -> SupervisorInitInputs {
    SupervisorInitInputs {
        runtime_mode: "websocket".to_string(),
        installed: true,
        boot_id: "boot-123".to_string(),
        pid_file: "/runtime/listener.pid".to_string(),
        log_file: "/logs/listener.log".to_string(),
        status_file: "/runtime/listener.status.json".to_string(),
        socket_path: "/runtime/message-daemon.sock".to_string(),
        pid: 4242,
        started_at: "2026-05-17T05:15:00Z".to_string(),
        host_notify_status: HostNotifyInitStatus {
            enabled: true,
            sink: "log".to_string(),
            ..HostNotifyInitStatus::default()
        },
    }
}
