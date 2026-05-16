use awiki_cli::config::{Paths, Resolved};
use awiki_cli::runtime::listener::{self, Status};
use awiki_cli::runtime::listener_service::{
    self, ListenerRuntimePolicy, ListenerServiceConfigValue as ConfigValue,
    ListenerServiceLifecycleOperation as Op, ListenerServiceProgramAction as ProgramAction,
    ListenerServiceProgramDecision, ListenerServiceProgramRunAction, ListenerServiceProgramState,
    ListenerServiceStatusSnapshot,
};
use std::collections::BTreeMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[test]
fn listener_service_names_match_go_contract() {
    let resolved = test_resolved();
    assert_eq!(
        listener_service::service_name_for(&resolved),
        format!(
            "awiki-cli-listener-{}",
            first_12_sha256_hex(&resolved.paths.workspace_home_dir)
        )
    );
    let mut empty = resolved.clone();
    empty.paths.workspace_home_dir.clear();
    assert_eq!(
        listener_service::service_name_for(&empty),
        "awiki-cli-listener"
    );
    assert_eq!(
        listener_service::service_display_name_for(None),
        "awiki-cli Listener"
    );
    assert_eq!(
        listener_service::service_display_name_for(Some(&resolved)),
        format!(
            "awiki-cli Listener ({})",
            std::path::Path::new(&resolved.paths.workspace_home_dir)
                .file_name()
                .unwrap()
                .to_string_lossy()
        )
    );
}

#[test]
fn listener_service_config_plan_matches_go_new_service_config_shape() {
    let resolved = test_resolved();
    let plan = listener_service::service_config_plan_for(&resolved, false);

    assert_eq!(plan.name, listener_service::service_name_for(&resolved));
    assert_eq!(
        plan.display_name,
        listener_service::service_display_name_for(Some(&resolved))
    );
    assert_eq!(plan.description, "awiki-cli realtime websocket listener");
    assert_eq!(
        plan.arguments,
        vec![
            "runtime".to_string(),
            "listener".to_string(),
            "service-run".to_string()
        ]
    );
    assert_eq!(plan.working_directory, resolved.paths.workspace_home_dir);

    let mut expected_options = BTreeMap::new();
    expected_options.insert("RunAtLoad".to_string(), ConfigValue::Bool(true));
    expected_options.insert("KeepAlive".to_string(), ConfigValue::Bool(true));
    expected_options.insert("UserService".to_string(), ConfigValue::Bool(true));
    expected_options.insert("DelayedAutoStart".to_string(), ConfigValue::Bool(true));
    expected_options.insert(
        "StartType".to_string(),
        ConfigValue::String("automatic".to_string()),
    );
    expected_options.insert(
        "OnFailure".to_string(),
        ConfigValue::String("restart".to_string()),
    );
    expected_options.insert(
        "OnFailureDelayDuration".to_string(),
        ConfigValue::String("1s".to_string()),
    );
    expected_options.insert("LogOutput".to_string(), ConfigValue::Bool(true));
    expected_options.insert(
        "LogDirectory".to_string(),
        ConfigValue::String(resolved.paths.logs_dir.clone()),
    );
    expected_options.insert(
        "PIDFile".to_string(),
        ConfigValue::String(path_string(
            &std::path::Path::new(&resolved.paths.state_dir).join("listener.service.pid"),
        )),
    );
    assert_eq!(plan.options, expected_options);

    let mut expected_env = BTreeMap::new();
    expected_env.insert(
        "AWIKI_CLI_WORKSPACE_HOME_DIR".to_string(),
        resolved.paths.workspace_home_dir.clone(),
    );
    expected_env.insert("AWIKI_LISTENER_SERVICE_MODE".to_string(), "1".to_string());
    assert_eq!(plan.env_vars, expected_env);

    let windows_plan = listener_service::service_config_plan_for(&resolved, true);
    assert_eq!(windows_plan.working_directory, "");
    assert_eq!(windows_plan.options, plan.options);
    assert_eq!(windows_plan.env_vars, plan.env_vars);

    let mut spaced = resolved.clone();
    spaced.paths.workspace_home_dir = format!("  {}  ", resolved.paths.workspace_home_dir);
    let spaced_plan = listener_service::service_config_plan_for(&spaced, false);
    assert_eq!(
        spaced_plan.name,
        format!(
            "awiki-cli-listener-{}",
            first_12_sha256_hex(spaced.paths.workspace_home_dir.trim())
        )
    );
    assert_eq!(
        spaced_plan.display_name,
        listener_service::service_display_name_for(Some(&spaced))
    );
    assert_eq!(
        spaced_plan.working_directory,
        spaced.paths.workspace_home_dir
    );
    assert_eq!(
        spaced_plan
            .env_vars
            .get("AWIKI_CLI_WORKSPACE_HOME_DIR")
            .expect("workspace env"),
        &spaced.paths.workspace_home_dir
    );
}

#[test]
fn wait_for_service_status_with_waits_for_bridge_availability_like_go() {
    let statuses = [
        Status {
            installed: true,
            running: true,
            bridge_available: false,
            ..Status::default()
        },
        Status {
            installed: true,
            running: true,
            bridge_available: true,
            ..Status::default()
        },
    ];
    let mut call_count = 0usize;
    let status = listener_service::wait_for_service_status_with(
        || {
            let current = statuses
                .get(call_count)
                .cloned()
                .unwrap_or_else(|| statuses[statuses.len() - 1].clone());
            call_count += 1;
            Ok(current)
        },
        true,
        true,
        "",
        Duration::from_millis(100),
        Duration::from_millis(1),
    )
    .expect("wait for bridge");

    assert!(status.bridge_available);
    assert!(call_count >= 2);
}

#[test]
fn wait_for_service_status_with_waits_for_expected_boot_id_like_go() {
    let statuses = [
        Status {
            installed: true,
            running: true,
            bridge_available: true,
            boot_id: "boot-old".to_string(),
            ..Status::default()
        },
        Status {
            installed: true,
            running: true,
            bridge_available: true,
            boot_id: "boot-new".to_string(),
            ..Status::default()
        },
    ];
    let mut call_count = 0usize;
    let status = listener_service::wait_for_service_status_with(
        || {
            let current = statuses
                .get(call_count)
                .cloned()
                .unwrap_or_else(|| statuses[statuses.len() - 1].clone());
            call_count += 1;
            Ok(current)
        },
        true,
        true,
        "boot-new",
        Duration::from_millis(100),
        Duration::from_millis(1),
    )
    .expect("wait for boot id");

    assert_eq!(status.boot_id, "boot-new");
    assert!(call_count >= 2);
}

#[test]
fn listener_service_mode_detection_matches_go_contract() {
    let args = vec![
        "awiki-cli".to_string(),
        "runtime".to_string(),
        "listener".to_string(),
        "service-run".to_string(),
    ];
    assert!(listener_service::running_in_listener_service_mode_with(
        Some("1"),
        &[]
    ));
    assert!(listener_service::running_in_listener_service_mode_with(
        Some(" true "),
        &[]
    ));
    assert!(listener_service::running_in_listener_service_mode_with(
        None, &args
    ));
    assert!(!listener_service::running_in_listener_service_mode_with(
        Some("false"),
        &args[..3]
    ));
}

#[test]
fn foreground_signal_plan_matches_go_platform_files() {
    use listener_service::ForegroundSignal::{Interrupt, Sigterm};

    assert_eq!(
        listener_service::foreground_signal_plan_for_platform(false),
        vec![Interrupt, Sigterm]
    );
    assert_eq!(
        listener_service::foreground_signal_plan_for_platform(true),
        vec![Interrupt]
    );
    assert_eq!(
        listener_service::foreground_signal_plan(),
        listener_service::foreground_signal_plan_for_platform(cfg!(windows))
    );
}

#[test]
fn child_process_plan_matches_go_sysproc_platform_files() {
    assert!(listener_service::listener_child_process_plan_for_platform(false).setsid);
    assert!(!listener_service::listener_child_process_plan_for_platform(true).setsid);
    assert_eq!(
        listener_service::listener_child_process_plan(),
        listener_service::listener_child_process_plan_for_platform(cfg!(windows))
    );
}

#[test]
fn service_program_start_plan_matches_go_state_and_run_loop_ordering() {
    let already_running = listener_service::service_program_start_plan(
        program_state(true, true, true),
        Some("must not construct supervisor"),
    );
    assert_eq!(
        already_running.actions,
        vec![ProgramAction::LockProgram, ProgramAction::UnlockProgram]
    );
    assert!(already_running.run_loop_actions.is_empty());
    assert_eq!(
        already_running.decision,
        ListenerServiceProgramDecision::ReturnOk
    );

    let init_error = listener_service::service_program_start_plan(
        program_state(false, false, false),
        Some("open store failed"),
    );
    assert_eq!(
        init_error.actions,
        vec![
            ProgramAction::LockProgram,
            ProgramAction::NewSupervisor,
            ProgramAction::UnlockProgram,
        ]
    );
    assert!(init_error.run_loop_actions.is_empty());
    assert_eq!(
        init_error.decision,
        ListenerServiceProgramDecision::ReturnError("open store failed".to_string())
    );

    let started =
        listener_service::service_program_start_plan(program_state(false, false, false), None);
    assert_eq!(
        started.actions,
        vec![
            ProgramAction::LockProgram,
            ProgramAction::NewSupervisor,
            ProgramAction::CreateCancelContext,
            ProgramAction::CreateDoneChannel,
            ProgramAction::StoreSupervisor,
            ProgramAction::StoreCancel,
            ProgramAction::StoreDone,
            ProgramAction::SpawnRunLoop,
            ProgramAction::UnlockProgram,
        ]
    );
    assert_eq!(
        started.run_loop_actions,
        vec![
            ListenerServiceProgramRunAction::RunSupervisor,
            ListenerServiceProgramRunAction::SendRunResult,
            ListenerServiceProgramRunAction::CleanupRuntimeArtifacts,
            ListenerServiceProgramRunAction::CloseDone,
        ]
    );
    assert_eq!(started.decision, ListenerServiceProgramDecision::ReturnOk);
}

#[test]
fn service_program_stop_plan_matches_go_clear_cancel_close_wait_ordering() {
    let stopped_empty =
        listener_service::service_program_stop_plan(program_state(false, false, false), false);
    assert_eq!(
        stopped_empty.actions,
        vec![
            ProgramAction::LockProgram,
            ProgramAction::SnapshotState,
            ProgramAction::ClearCancel,
            ProgramAction::ClearDone,
            ProgramAction::ClearSupervisor,
            ProgramAction::UnlockProgram,
        ]
    );
    assert_eq!(
        stopped_empty.decision,
        ListenerServiceProgramDecision::ReturnOk
    );

    let stopped =
        listener_service::service_program_stop_plan(program_state(true, true, true), true);
    assert_eq!(
        stopped.actions,
        vec![
            ProgramAction::LockProgram,
            ProgramAction::SnapshotState,
            ProgramAction::ClearCancel,
            ProgramAction::ClearDone,
            ProgramAction::ClearSupervisor,
            ProgramAction::UnlockProgram,
            ProgramAction::CancelContext,
            ProgramAction::CloseSupervisor,
            ProgramAction::WaitDone {
                timeout: Duration::from_secs(15),
            },
        ]
    );
    assert_eq!(stopped.decision, ListenerServiceProgramDecision::ReturnOk);

    let timeout =
        listener_service::service_program_stop_plan(program_state(true, true, true), false);
    assert_eq!(
        timeout.decision,
        ListenerServiceProgramDecision::ReturnError("listener service stop timed out".to_string())
    );

    let cancel_only =
        listener_service::service_program_stop_plan(program_state(false, true, false), false);
    assert_eq!(
        cancel_only.actions,
        vec![
            ProgramAction::LockProgram,
            ProgramAction::SnapshotState,
            ProgramAction::ClearCancel,
            ProgramAction::ClearDone,
            ProgramAction::ClearSupervisor,
            ProgramAction::UnlockProgram,
            ProgramAction::CancelContext,
        ]
    );

    let supervisor_only =
        listener_service::service_program_stop_plan(program_state(true, false, false), false);
    assert_eq!(
        supervisor_only.actions,
        vec![
            ProgramAction::LockProgram,
            ProgramAction::SnapshotState,
            ProgramAction::ClearCancel,
            ProgramAction::ClearDone,
            ProgramAction::ClearSupervisor,
            ProgramAction::UnlockProgram,
            ProgramAction::CloseSupervisor,
        ]
    );
}

#[test]
fn listener_service_lifecycle_plans_match_go_branching() {
    assert_eq!(
        listener_service::ensure_installed_plan(status(false, false)),
        vec![
            Op::NewService,
            Op::CheckStatus,
            Op::InstallIfMissing,
            Op::ReturnStatus
        ]
    );
    assert_eq!(
        listener_service::ensure_installed_plan(status(true, true)),
        vec![Op::NewService, Op::CheckStatus, Op::ReturnStatus]
    );

    assert_eq!(
        listener_service::start_service_plan("http", status(false, false), None, "boot-new"),
        vec![Op::ValidateWebSocketMode]
    );
    assert_eq!(
        listener_service::start_service_plan(
            "websocket",
            status(false, false),
            Some(status(false, false)),
            "boot-new"
        ),
        vec![
            Op::ValidateWebSocketMode,
            Op::NewService,
            Op::CheckStatus,
            Op::CallEnsureInstalled,
            Op::RecheckStatus,
            Op::ErrorNotInstalledAfterAutoInstall,
        ]
    );
    assert_eq!(
        listener_service::start_service_plan(
            "websocket",
            status(false, false),
            Some(status(true, false)),
            "boot-new"
        ),
        vec![
            Op::ValidateWebSocketMode,
            Op::NewService,
            Op::CheckStatus,
            Op::CallEnsureInstalled,
            Op::RecheckStatus,
            Op::PrepareBootId,
            Op::ServiceStart,
            Op::WaitForRunning {
                expected_boot_id: "boot-new".to_string(),
            },
        ]
    );
    assert_eq!(
        listener_service::start_service_plan(" websocket ", status(true, true), None, "boot-new"),
        vec![
            Op::ValidateWebSocketMode,
            Op::NewService,
            Op::CheckStatus,
            Op::ReturnStatus,
        ]
    );
    assert_eq!(
        listener_service::start_service_plan("websocket", status(true, false), None, "boot-new"),
        vec![
            Op::ValidateWebSocketMode,
            Op::NewService,
            Op::CheckStatus,
            Op::PrepareBootId,
            Op::ServiceStart,
            Op::WaitForRunning {
                expected_boot_id: "boot-new".to_string(),
            },
        ]
    );

    assert_eq!(
        listener_service::stop_service_plan(status(false, false)),
        vec![Op::NewService, Op::CheckStatus, Op::ReturnStatus]
    );
    assert_eq!(
        listener_service::stop_service_plan(status(true, false)),
        vec![
            Op::NewService,
            Op::CheckStatus,
            Op::CleanupRuntimeArtifacts,
            Op::WaitForStopped,
        ]
    );
    assert_eq!(
        listener_service::stop_service_plan(status(true, true)),
        vec![
            Op::NewService,
            Op::CheckStatus,
            Op::ServiceStop,
            Op::CleanupRuntimeArtifacts,
            Op::WaitForStopped,
        ]
    );

    assert_eq!(
        listener_service::restart_service_plan(status(false, false), "boot-new"),
        vec![Op::NewService, Op::CheckStatus, Op::ErrorNotInstalled]
    );
    assert_eq!(
        listener_service::restart_service_plan(status(true, false), "boot-new"),
        vec![
            Op::NewService,
            Op::CheckStatus,
            Op::PrepareBootId,
            Op::ServiceRestart,
            Op::WaitForRunning {
                expected_boot_id: "boot-new".to_string(),
            },
        ]
    );

    assert_eq!(
        listener_service::uninstall_service_plan(status(false, false)),
        vec![
            Op::NewService,
            Op::CheckStatus,
            Op::CleanupRuntimeArtifacts,
            Op::ReturnStatus,
        ]
    );
    assert_eq!(
        listener_service::uninstall_service_plan(status(true, false)),
        vec![
            Op::NewService,
            Op::CheckStatus,
            Op::ServiceUninstall,
            Op::CleanupRuntimeArtifacts,
            Op::ReturnStatus,
        ]
    );
    assert_eq!(
        listener_service::uninstall_service_plan(status(true, true)),
        vec![
            Op::NewService,
            Op::CheckStatus,
            Op::ServiceStop,
            Op::ServiceUninstall,
            Op::CleanupRuntimeArtifacts,
            Op::ReturnStatus,
        ]
    );
}

#[test]
fn listener_apply_runtime_policy_plan_matches_go_branching() {
    assert_eq!(
        listener_service::apply_runtime_policy_plan(
            policy(false, true, true, true),
            status(true, true)
        ),
        vec![Op::CallStopService]
    );
    assert_eq!(
        listener_service::apply_runtime_policy_plan(
            policy(true, false, true, true),
            status(true, true)
        ),
        vec![Op::CallStopService]
    );
    assert_eq!(
        listener_service::apply_runtime_policy_plan(
            policy(true, true, true, true),
            status(false, false)
        ),
        vec![Op::CallEnsureInstalled, Op::CallStartService]
    );
    assert_eq!(
        listener_service::apply_runtime_policy_plan(
            policy(true, true, true, false),
            status(false, false)
        ),
        vec![Op::CallEnsureInstalled, Op::ReturnStatus]
    );
    assert_eq!(
        listener_service::apply_runtime_policy_plan(
            policy(true, true, false, true),
            status(false, false)
        ),
        vec![Op::CheckStatus, Op::ReturnStatus]
    );
    assert_eq!(
        listener_service::apply_runtime_policy_plan(
            policy(true, true, false, true),
            status(true, false)
        ),
        vec![Op::CheckStatus, Op::CallStartService]
    );
    assert_eq!(
        listener_service::apply_runtime_policy_plan(
            policy(true, true, false, false),
            status(true, false)
        ),
        vec![Op::ReturnStatus]
    );

    let mut resolved = test_resolved();
    resolved.runtime_mode = "WEBSOCKET".to_string();
    resolved.runtime_listener_enabled = true;
    resolved.runtime_listener_auto_install = false;
    resolved.runtime_listener_auto_start = true;
    assert_eq!(
        ListenerRuntimePolicy::from_resolved(&resolved),
        policy(true, true, false, true)
    );
}

#[test]
fn boot_id_helpers_and_cleanup_runtime_artifacts_match_go_boundary() {
    let resolved = test_resolved();
    let boot_id = listener_service::prepare_expected_boot_id(&resolved).expect("prepare boot id");
    assert!(boot_id.starts_with("boot-"));
    assert_eq!(
        listener_service::resolve_runtime_boot_id(&resolved).expect("resolve boot id"),
        boot_id
    );

    let paths = listener::paths(&resolved).expect("runtime paths");
    listener::write_pid(&paths.pid_file, 42).expect("write pid");
    listener::write_status(
        &paths.status_file,
        &Status {
            running: true,
            ..Status::default()
        },
    )
    .expect("write status");
    std::fs::write(&paths.socket_path, b"socket placeholder").expect("write socket placeholder");

    listener_service::cleanup_runtime_artifacts(&resolved);

    assert!(!std::path::Path::new(&paths.pid_file).exists());
    assert!(!std::path::Path::new(&paths.status_file).exists());
    assert!(!std::path::Path::new(&paths.socket_path).exists());
    assert!(!std::path::Path::new(&listener::boot_id_path(&resolved).unwrap()).exists());
    assert!(listener_service::resolve_runtime_boot_id(&resolved)
        .expect("fallback boot id")
        .starts_with("boot-"));
}

#[test]
fn host_notify_error_status_writes_only_when_changed_like_go() {
    let resolved = test_resolved();
    let paths = listener::paths(&resolved).expect("runtime paths");
    let mut status = Status {
        mode: "websocket".to_string(),
        running: true,
        status_file: paths.status_file.clone(),
        host_notify: listener::HostNotifyStatus {
            enabled: true,
            sink: "capture".to_string(),
            ..listener::HostNotifyStatus::default()
        },
        ..Status::default()
    };

    assert!(listener::write_host_notify_error_if_changed(
        &mut status,
        "sink boom"
    ));
    let loaded = listener::read_status(&paths.status_file).expect("read first status");
    assert_eq!(loaded.host_notify.last_error, "sink boom");

    status.mode = "changed-but-not-written".to_string();
    assert!(!listener::write_host_notify_error_if_changed(
        &mut status,
        "sink boom"
    ));
    let loaded = listener::read_status(&paths.status_file).expect("read unchanged status");
    assert_eq!(loaded.mode, "websocket");
    assert_eq!(loaded.host_notify.last_error, "sink boom");

    assert!(listener::write_host_notify_error_if_changed(
        &mut status,
        "sink retry failed"
    ));
    let loaded = listener::read_status(&paths.status_file).expect("read changed status");
    assert_eq!(loaded.mode, "changed-but-not-written");
    assert_eq!(loaded.host_notify.last_error, "sink retry failed");

    status.mode = "clear-written".to_string();
    assert!(listener::clear_host_notify_error_if_present(&mut status));
    let loaded = listener::read_status(&paths.status_file).expect("read cleared status");
    assert_eq!(loaded.mode, "clear-written");
    assert!(loaded.host_notify.last_error.is_empty());

    status.mode = "clear-not-written".to_string();
    assert!(!listener::clear_host_notify_error_if_present(&mut status));
    let loaded = listener::read_status(&paths.status_file).expect("read no-op clear status");
    assert_eq!(loaded.mode, "clear-written");
    assert!(loaded.host_notify.last_error.is_empty());
}

fn first_12_sha256_hex(value: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(value.as_bytes());
    format!("{digest:x}")[..12].to_string()
}

fn test_resolved() -> Resolved {
    let root = std::env::temp_dir().join(format!(
        "awiki-cli-rs2-listener-service-test-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).expect("create test root");
    Resolved {
        paths: Paths {
            workspace_home_dir: path_string(&root),
            root_dir: path_string(&root),
            config_dir: path_string(&root),
            data_dir: path_string(&root.join("data")),
            state_dir: path_string(&root.join("runtime")),
            cache_dir: path_string(&root.join("cache")),
            logs_dir: path_string(&root.join("logs")),
            config_file: path_string(&root.join("config.yaml")),
            identity_dir: path_string(&root.join("identities")),
            database_file: path_string(&root.join("data").join("awiki.db")),
            legacy_credentials_dir: path_string(&root.join("credentials")),
            legacy_data_dir: path_string(&root.join("legacy-data")),
        },
        runtime_mode: "websocket".to_string(),
        runtime_socket_path: path_string(&root.join("runtime").join("message-daemon.sock")),
        runtime_listener_enabled: true,
        runtime_listener_auto_install: true,
        runtime_listener_auto_start: true,
        host_notify_enabled: true,
        host_notify_sink: "log".to_string(),
        config_schema_version: 0,
        active_identity: String::new(),
        host_notify_file_path: String::new(),
        host_notify_openclaw_hook_url: String::new(),
        host_notify_openclaw_agent_id: String::new(),
        host_notify_openclaw_hook_name: String::new(),
        host_notify_hermes_notify_url: String::new(),
        host_notify_hermes_deliver: String::new(),
        output_format: "json".to_string(),
        no_color: false,
        service_base_url: String::new(),
        did_domain: String::new(),
        anp_service_endpoint: String::new(),
        anp_service_did: String::new(),
        mail_service_url: String::new(),
        ca_bundle: String::new(),
        update_disable_strict_version: false,
        update_metadata_cache_ttl_seconds: 0,
        config_exists: false,
        config_error: String::new(),
        env_hits: Vec::new(),
        sources: Default::default(),
    }
}

fn status(installed: bool, running: bool) -> ListenerServiceStatusSnapshot {
    ListenerServiceStatusSnapshot { installed, running }
}

fn policy(
    websocket_mode: bool,
    listener_enabled: bool,
    auto_install: bool,
    auto_start: bool,
) -> ListenerRuntimePolicy {
    ListenerRuntimePolicy {
        websocket_mode,
        listener_enabled,
        auto_install,
        auto_start,
    }
}

fn program_state(
    has_supervisor: bool,
    has_cancel: bool,
    has_done: bool,
) -> ListenerServiceProgramState {
    ListenerServiceProgramState {
        has_supervisor,
        has_cancel,
        has_done,
    }
}

fn path_string(path: &std::path::Path) -> String {
    path.to_string_lossy().into_owned()
}
