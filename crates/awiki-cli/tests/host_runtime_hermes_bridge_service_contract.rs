use awiki_cli::host_runtime::hermes_bridge::{
    self, BridgeAdapterCommandPlan, BridgeAdapterProcess, BridgeApplyDecision, BridgeConfig,
    BridgeStatus, RouteState, DEFAULT_WEBHOOK_PORT, SERVICE_ARGUMENTS, SERVICE_DESCRIPTION,
    SERVICE_DISPLAY_NAME_PREFIX, SERVICE_NAME_PREFIX,
};
use awiki_cli::host_runtime::hermes_bridge::{
    BridgeServiceBackend, BridgeServiceStatusSnapshot, BridgeSystemdCommandRunner,
};
use awiki_cli::workspace_config::{Paths, Resolved};
use hermes_bridge::BridgeServiceLifecycleOperation as Op;
use std::fs;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex, MutexGuard,
};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

static ENV_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn hermes_bridge_service_names_match_go_contract() {
    let resolved = test_resolved();
    assert_eq!(
        hermes_bridge::service_name_for(Some(&resolved)),
        format!(
            "{SERVICE_NAME_PREFIX}-{}",
            first_12_sha256_hex(&resolved.paths.workspace_home_dir)
        )
    );

    let mut empty = resolved.clone();
    empty.paths.workspace_home_dir.clear();
    assert_eq!(
        hermes_bridge::service_name_for(Some(&empty)),
        SERVICE_NAME_PREFIX
    );
    assert_eq!(
        hermes_bridge::service_name_for(None),
        format!("{SERVICE_NAME_PREFIX}-{}", first_12_sha256_hex("default"))
    );
    assert_eq!(
        hermes_bridge::service_display_name_for(None),
        SERVICE_DISPLAY_NAME_PREFIX
    );
    assert_eq!(
        hermes_bridge::service_display_name_for(Some(&resolved)),
        format!(
            "{SERVICE_DISPLAY_NAME_PREFIX} ({})",
            std::path::Path::new(&resolved.paths.workspace_home_dir)
                .file_name()
                .unwrap()
                .to_string_lossy()
        )
    );
}

#[test]
fn hermes_bridge_service_config_plan_matches_go_new_service_config() {
    let resolved = test_resolved();
    let plan = hermes_bridge::service_config_plan_for(&resolved, " /tmp/hermes-home ", false);
    assert_eq!(plan.name, hermes_bridge::service_name_for(Some(&resolved)));
    assert_eq!(
        plan.display_name,
        hermes_bridge::service_display_name_for(Some(&resolved))
    );
    assert_eq!(plan.description, SERVICE_DESCRIPTION);
    assert_eq!(plan.arguments, SERVICE_ARGUMENTS);
    assert_eq!(plan.working_directory, resolved.paths.workspace_home_dir);
    assert_eq!(
        plan.env_workspace_home_dir,
        resolved.paths.workspace_home_dir
    );
    assert_eq!(plan.env_hermes_home, "/tmp/hermes-home");
    assert!(plan.user_service);
    assert!(plan.keep_alive);
    assert_eq!(plan.on_failure, "restart");
    assert_eq!(plan.on_failure_delay_duration, "1s");
    assert!(plan.log_output);
    assert_eq!(plan.log_directory, resolved.paths.logs_dir);

    let windows_plan = hermes_bridge::service_config_plan_for(&resolved, "/tmp/hermes-home", true);
    assert_eq!(windows_plan.working_directory, "");
}

#[test]
fn hermes_bridge_systemd_unit_allows_hidden_service_entry() {
    let resolved = test_resolved();
    let _fixture = prepare_bridge_config_fixture(&resolved);
    let unit = hermes_bridge::systemd_unit_for(&resolved).expect("systemd unit");

    assert!(unit
        .content
        .contains("runtime host-notify hermes bridge service-run"));
    assert!(unit
        .content
        .contains("Environment=AWIKI_CLI_INTERNAL_ENTRY=1"));
    assert!(unit.content.contains(&format!(
        "Environment=AWIKI_CLI_WORKSPACE_HOME_DIR={}",
        resolved.paths.workspace_home_dir
    )));
    assert!(unit.content.contains("Environment=HERMES_HOME="));
}

#[test]
fn hermes_bridge_systemd_gate_defaults_off() {
    assert!(!hermes_bridge::service_enabled_by_env_value(None));
    assert!(!hermes_bridge::service_enabled_by_env_value(Some("")));
    assert!(!hermes_bridge::service_enabled_by_env_value(Some("0")));
    assert!(!hermes_bridge::service_enabled_by_env_value(Some("false")));
    assert!(hermes_bridge::service_enabled_by_env_value(Some("1")));
    assert!(hermes_bridge::service_enabled_by_env_value(Some(" true ")));
}

#[test]
fn hermes_bridge_systemd_status_parser_matches_systemctl_show_values() {
    assert_eq!(
        hermes_bridge::parse_systemd_status("loaded\nactive\n"),
        hermes_bridge::BridgeSystemdStatus {
            installed: true,
            running: true,
            load_state: "loaded".to_string(),
            active_state: "active".to_string(),
        }
    );
    assert_eq!(
        hermes_bridge::parse_systemd_status("loaded\nactivating\n"),
        hermes_bridge::BridgeSystemdStatus {
            installed: true,
            running: true,
            load_state: "loaded".to_string(),
            active_state: "activating".to_string(),
        }
    );
    assert_eq!(
        hermes_bridge::parse_systemd_status("not-found\ninactive\n"),
        hermes_bridge::BridgeSystemdStatus {
            installed: false,
            running: false,
            load_state: "not-found".to_string(),
            active_state: "inactive".to_string(),
        }
    );
}

#[test]
fn hermes_bridge_systemd_status_snapshot_uses_runner_without_live_systemd() {
    let resolved = test_resolved();
    let mut runner = FakeSystemctlRunner::new(vec![Ok("loaded\nactive\n".to_string())]);

    let snapshot =
        hermes_bridge::systemd_status_snapshot_with_runner(&resolved, &mut runner).expect("status");

    assert_eq!(
        runner.calls,
        vec![vec![
            "show".to_string(),
            hermes_bridge::unit_name_for(&resolved),
            "--property=LoadState".to_string(),
            "--property=ActiveState".to_string(),
            "--value".to_string(),
        ]]
    );
    assert_eq!(
        snapshot,
        BridgeServiceStatusSnapshot {
            installed: true,
            running: true,
            platform: "linux-systemd".to_string(),
            service_name: hermes_bridge::service_name_for(Some(&resolved)),
        }
    );
}

#[test]
fn hermes_bridge_systemd_status_snapshot_propagates_real_status_errors() {
    let resolved = test_resolved();
    let mut runner = FakeSystemctlRunner::new(vec![Err("permission denied".to_string())]);

    let err =
        hermes_bridge::systemd_status_snapshot_with_runner(&resolved, &mut runner).unwrap_err();

    assert_eq!(err.to_string(), "permission denied");
}

#[test]
fn hermes_bridge_adapter_command_plan_matches_go_service_program_start() {
    let config = test_bridge_config("/workspace/.hermes");
    let plan = hermes_bridge::adapter_command_plan_for(&config);

    assert_eq!(plan.executable, "/usr/bin/python3");
    assert_eq!(
        plan.arguments,
        vec![
            "/opt/awiki/scripts/hermes_notify_adapter.py",
            "--host",
            "127.0.0.1",
            "--port",
            "8765",
            "--notify-secret",
            "notify-secret",
            "--hermes-webhook-url",
            "http://127.0.0.1:8644/webhooks/notify",
            "--hermes-route-secret",
            "route-secret",
            "--log-level",
            "INFO",
        ]
    );
    assert_eq!(plan.env_hermes_home.as_deref(), Some("/workspace/.hermes"));
    assert!(plan.stdout_inherits_parent);
    assert!(plan.stderr_inherits_parent);

    let no_home = test_bridge_config("");
    let plan = hermes_bridge::adapter_command_plan_for(&no_home);
    assert_eq!(plan.env_hermes_home, None);
}

#[cfg(unix)]
#[test]
fn hermes_bridge_adapter_process_runs_plan_with_inherited_env_override() {
    let temp = TempDir::new("adapter-process-env").expect("temp dir");
    let output = temp.path().join("adapter-output.txt");
    let hermes_home = temp.path().join("hermes-home");
    let script = r#"printf 'home=%s\n' "$HERMES_HOME" > "$1"
printf 'arg=%s\n' "$2" >> "$1"
"#;
    let plan = BridgeAdapterCommandPlan {
        executable: "/bin/sh".to_string(),
        arguments: vec![
            "-c".to_string(),
            script.to_string(),
            "awiki-test-sh".to_string(),
            path_string(&output),
            "adapter-script.py".to_string(),
        ],
        env_hermes_home: Some(path_string(&hermes_home)),
        stdout_inherits_parent: false,
        stderr_inherits_parent: false,
    };

    let mut process = BridgeAdapterProcess::start(&plan).expect("start adapter process");
    let exit = process.wait().expect("wait adapter process").expect("exit");

    assert!(exit.success);
    assert_eq!(exit.code, Some(0));
    assert_eq!(
        fs::read_to_string(&output).expect("adapter output"),
        format!(
            "home={}\narg=adapter-script.py\n",
            path_string(&hermes_home)
        )
    );
}

#[cfg(unix)]
#[test]
fn hermes_bridge_adapter_process_stop_kills_running_child_like_go_stop() {
    let plan = BridgeAdapterCommandPlan {
        executable: "/bin/sh".to_string(),
        arguments: vec![
            "-c".to_string(),
            "exec sleep 30".to_string(),
            "awiki-test-sh".to_string(),
        ],
        env_hermes_home: None,
        stdout_inherits_parent: false,
        stderr_inherits_parent: false,
    };
    let mut process = BridgeAdapterProcess::start(&plan).expect("start adapter process");

    assert!(process.is_running().expect("running child"));
    process
        .stop_with_timeout(Duration::from_secs(2), Duration::from_millis(10))
        .expect("stop adapter process");

    assert!(!process.is_running().expect("stopped child"));
    assert_eq!(process.wait().expect("wait after stop"), None);
}

#[cfg(unix)]
#[test]
fn hermes_bridge_run_service_starts_adapter_until_stop_requested_like_go_run() {
    let temp = TempDir::new("run-service-stop").expect("temp dir");
    let marker = temp.path().join("started.txt");
    let plan = BridgeAdapterCommandPlan {
        executable: "/bin/sh".to_string(),
        arguments: vec![
            "-c".to_string(),
            format!("printf started > {}; exec sleep 30", shell_quote(&marker)),
            "awiki-test-sh".to_string(),
        ],
        env_hermes_home: None,
        stdout_inherits_parent: false,
        stderr_inherits_parent: false,
    };
    let calls = Arc::new(AtomicUsize::new(0));
    let stop_calls = calls.clone();

    hermes_bridge::run_bridge_service_with_stop(
        &plan,
        move || stop_calls.fetch_add(1, Ordering::SeqCst) >= 2,
        Duration::from_millis(10),
    )
    .expect("run bridge service");

    assert_eq!(
        fs::read_to_string(&marker).expect("adapter marker"),
        "started"
    );
    assert!(calls.load(Ordering::SeqCst) >= 3);
}

#[test]
fn hermes_bridge_status_from_parts_matches_go_config_warning_boundary() {
    let status = hermes_bridge::status_from_parts(
        "awiki-cli-hermes-bridge".to_string(),
        Err(anyhow::anyhow!("workspace configuration is required")),
        Ok(None),
        |_| unreachable!("health probe should not run when config fails"),
    );

    assert_eq!(status.service_name, "awiki-cli-hermes-bridge");
    assert!(!status.installed);
    assert!(!status.running);
    assert!(!status.bridge_available);
    assert!(status.config.is_none());
    assert_eq!(status.warnings, vec!["workspace configuration is required"]);
}

#[test]
fn hermes_bridge_status_from_parts_matches_go_service_status_warning_boundary() {
    let status = hermes_bridge::status_from_parts(
        "initial-name".to_string(),
        Ok(test_bridge_config_with_route_warnings(
            "/workspace/.hermes",
            &["route warning"],
        )),
        Err(anyhow::anyhow!("status failed")),
        |_| unreachable!("health probe should not run when service is not running"),
    );

    assert_eq!(status.service_name, "initial-name");
    assert_eq!(status.service_platform, "");
    assert!(!status.installed);
    assert!(!status.running);
    assert!(!status.bridge_available);
    assert_eq!(status.health_url, "http://127.0.0.1:8765/healthz");
    assert!(status.config.is_some());
    assert_eq!(
        status.warnings,
        vec![
            "Hermes bridge service status unavailable: status failed",
            "route warning",
        ]
    );
}

#[test]
fn hermes_bridge_status_from_parts_probes_health_only_when_running_like_go() {
    let mut probed = Vec::new();
    let status = hermes_bridge::status_from_parts(
        "initial-name".to_string(),
        Ok(test_bridge_config("/workspace/.hermes")),
        Ok(Some(hermes_bridge::BridgeServiceStatusSnapshot {
            installed: true,
            running: true,
            platform: "linux-systemd".to_string(),
            service_name: "platform-name".to_string(),
        })),
        |url| {
            probed.push(url.to_string());
            false
        },
    );

    assert_eq!(status.service_name, "platform-name");
    assert_eq!(status.service_platform, "linux-systemd");
    assert!(status.installed);
    assert!(status.running);
    assert!(!status.bridge_available);
    assert_eq!(probed, vec!["http://127.0.0.1:8765/healthz"]);
    assert_eq!(
        status.warnings,
        vec!["Hermes bridge health endpoint is not responding"]
    );

    let mut stopped_probe_count = 0usize;
    let stopped = hermes_bridge::status_from_parts(
        "initial-name".to_string(),
        Ok(test_bridge_config("/workspace/.hermes")),
        Ok(Some(hermes_bridge::BridgeServiceStatusSnapshot {
            installed: true,
            running: false,
            platform: "linux-systemd".to_string(),
            service_name: "platform-name".to_string(),
        })),
        |_| {
            stopped_probe_count += 1;
            true
        },
    );
    assert!(stopped.installed);
    assert!(!stopped.running);
    assert!(!stopped.bridge_available);
    assert_eq!(stopped_probe_count, 0);
    assert!(stopped.warnings.is_empty());
}

#[test]
fn hermes_bridge_status_from_parts_keeps_current_rust_local_boundary() {
    let status = hermes_bridge::status_from_parts(
        "initial-name".to_string(),
        Ok(test_bridge_config_with_route_warnings(
            "/workspace/.hermes",
            &["route warning"],
        )),
        Ok(None),
        |_| unreachable!("health probe should not run without running service"),
    );

    assert_eq!(status.service_name, "initial-name");
    assert_eq!(status.service_platform, "rust-local");
    assert!(!status.installed);
    assert!(!status.running);
    assert!(!status.bridge_available);
    assert_eq!(status.warnings, vec!["route warning"]);
    assert!(status.config.is_some());
}

#[test]
fn hermes_bridge_service_mode_detection_matches_hidden_go_command() {
    let args = vec![
        "awiki-cli".to_string(),
        "runtime".to_string(),
        "host-notify".to_string(),
        "hermes".to_string(),
        "bridge".to_string(),
        "service-run".to_string(),
    ];
    assert!(hermes_bridge::running_in_bridge_service_mode_with(&args));
    assert!(!hermes_bridge::running_in_bridge_service_mode_with(
        &args[..5]
    ));
}

#[test]
fn hermes_bridge_health_helper_matches_go_2xx_boundary() {
    let mut called_with = Vec::new();
    assert!(hermes_bridge::bridge_health_available_with(
        " http://127.0.0.1:8765/healthz ",
        |url| {
            called_with.push(url.to_string());
            Ok(204)
        }
    ));
    assert_eq!(called_with, vec!["http://127.0.0.1:8765/healthz"]);

    let ok_status = |_url: &str| Ok(200);
    assert!(!hermes_bridge::bridge_health_available_with("", ok_status));
    assert!(!hermes_bridge::bridge_health_available_with(
        "http://127.0.0.1/healthz",
        |_| Ok(199)
    ));
    assert!(!hermes_bridge::bridge_health_available_with(
        "http://127.0.0.1/healthz",
        |_| Ok(300)
    ));
    assert!(!hermes_bridge::bridge_health_available_with(
        "http://127.0.0.1/healthz",
        |_| Err(anyhow::anyhow!("dial failed"))
    ));
}

#[test]
fn wait_for_bridge_status_with_requires_running_and_health_like_go() {
    let statuses = [
        BridgeStatus {
            running: true,
            bridge_available: false,
            ..BridgeStatus::default()
        },
        BridgeStatus {
            running: true,
            bridge_available: true,
            ..BridgeStatus::default()
        },
    ];
    let mut call_count = 0usize;
    let status = hermes_bridge::wait_for_bridge_status_with(
        || {
            let current = statuses
                .get(call_count)
                .cloned()
                .unwrap_or_else(|| statuses[statuses.len() - 1].clone());
            call_count += 1;
            Ok(current)
        },
        true,
        Duration::from_millis(100),
        Duration::from_millis(1),
    )
    .expect("wait for bridge");

    assert!(status.running);
    assert!(status.bridge_available);
    assert!(call_count >= 2);
}

#[test]
fn wait_for_bridge_status_with_returns_last_status_on_timeout_like_go() {
    let mut call_count = 0usize;
    let status = hermes_bridge::wait_for_bridge_status_with(
        || {
            call_count += 1;
            Ok(BridgeStatus {
                running: true,
                bridge_available: false,
                health_url: format!("attempt-{call_count}"),
                ..BridgeStatus::default()
            })
        },
        true,
        Duration::from_millis(3),
        Duration::from_millis(1),
    )
    .expect("timeout returns status");

    assert!(status.running);
    assert!(!status.bridge_available);
    assert!(status.health_url.starts_with("attempt-"));
    assert!(call_count >= 1);

    let mut err_count = 0usize;
    let status = hermes_bridge::wait_for_bridge_status_with(
        || {
            err_count += 1;
            Err(anyhow::anyhow!("status unavailable"))
        },
        false,
        Duration::from_millis(3),
        Duration::from_millis(1),
    )
    .expect("errors are ignored until timeout");
    assert_eq!(status.service_name, "");
    assert_eq!(status.service_platform, "");
    assert!(!status.installed);
    assert!(!status.running);
    assert!(!status.bridge_available);
    assert_eq!(status.health_url, "");
    assert!(status.config.is_none());
    assert!(status.warnings.is_empty());
    assert!(err_count >= 1);
}

#[test]
fn apply_decision_matches_go_apply_branching() {
    assert_eq!(
        hermes_bridge::apply_decision_for(&BridgeStatus {
            installed: false,
            running: false,
            ..BridgeStatus::default()
        }),
        BridgeApplyDecision::EnsureInstalledThenStart
    );
    assert_eq!(
        hermes_bridge::apply_decision_for(&BridgeStatus {
            installed: true,
            running: true,
            ..BridgeStatus::default()
        }),
        BridgeApplyDecision::Restart
    );
    assert_eq!(
        hermes_bridge::apply_decision_for(&BridgeStatus {
            installed: true,
            running: false,
            ..BridgeStatus::default()
        }),
        BridgeApplyDecision::Start
    );
}

#[test]
fn lifecycle_operation_plans_match_go_service_branching() {
    assert_eq!(
        hermes_bridge::ensure_installed_plan(false),
        vec![Op::InstallIfMissing, Op::ReturnStatus]
    );
    assert_eq!(
        hermes_bridge::ensure_installed_plan(true),
        vec![Op::ReturnStatus]
    );

    assert_eq!(
        hermes_bridge::start_service_plan(false, false),
        vec![Op::EnsureInstalled, Op::Start, Op::WaitForRunning]
    );
    assert_eq!(
        hermes_bridge::start_service_plan(false, true),
        vec![Op::EnsureInstalled, Op::ReturnStatus]
    );
    assert_eq!(
        hermes_bridge::start_service_plan(true, false),
        vec![Op::Start, Op::WaitForRunning]
    );
    assert_eq!(
        hermes_bridge::start_service_plan(true, true),
        vec![Op::ReturnStatus]
    );

    assert_eq!(
        hermes_bridge::stop_service_plan(false, false),
        vec![Op::ReturnStatus]
    );
    assert_eq!(
        hermes_bridge::stop_service_plan(true, false),
        vec![Op::WaitForStopped]
    );
    assert_eq!(
        hermes_bridge::stop_service_plan(true, true),
        vec![Op::Stop, Op::WaitForStopped]
    );

    assert_eq!(
        hermes_bridge::restart_service_plan(false),
        vec![Op::ErrorNotInstalled]
    );
    assert_eq!(
        hermes_bridge::restart_service_plan(true),
        vec![Op::Restart, Op::WaitForRunning]
    );

    assert_eq!(
        hermes_bridge::uninstall_service_plan(false, false),
        vec![Op::ReturnStatus]
    );
    assert_eq!(
        hermes_bridge::uninstall_service_plan(true, false),
        vec![Op::Uninstall, Op::ReturnStatus]
    );
    assert_eq!(
        hermes_bridge::uninstall_service_plan(true, true),
        vec![Op::Stop, Op::Uninstall, Op::ReturnStatus]
    );
}

#[test]
fn apply_service_plan_matches_go_apply_branching() {
    assert_eq!(
        hermes_bridge::apply_service_plan(&BridgeStatus {
            installed: false,
            running: false,
            ..BridgeStatus::default()
        }),
        vec![Op::EnsureInstalled, Op::Start]
    );
    assert_eq!(
        hermes_bridge::apply_service_plan(&BridgeStatus {
            installed: true,
            running: true,
            ..BridgeStatus::default()
        }),
        vec![Op::Restart]
    );
    assert_eq!(
        hermes_bridge::apply_service_plan(&BridgeStatus {
            installed: true,
            running: false,
            ..BridgeStatus::default()
        }),
        vec![Op::Start]
    );
}

#[test]
fn hermes_bridge_status_for_with_backend_uses_rust_local_when_backend_unsupported() {
    let resolved = test_resolved();
    let mut backend = FakeBridgeBackend::new(vec![None]);
    let mut health_calls = 0usize;

    let status = hermes_bridge::status_from_parts(
        hermes_bridge::service_name_for(Some(&resolved)),
        Ok(test_bridge_config("/workspace/.hermes")),
        backend.status_snapshot(&resolved),
        |_| {
            health_calls += 1;
            true
        },
    );

    assert_eq!(status.service_platform, "rust-local");
    assert!(!status.installed);
    assert!(!status.running);
    assert!(!status.bridge_available);
    assert_eq!(health_calls, 0);
    assert_eq!(backend.events, vec!["status"]);
}

#[test]
fn hermes_bridge_start_with_backend_installs_starts_and_waits_for_health() {
    let resolved = test_resolved();
    let _fixture = prepare_bridge_config_fixture(&resolved);
    let mut backend = FakeBridgeBackend::new(vec![
        Some(snapshot(false, false)),
        Some(snapshot(false, false)),
        Some(snapshot(true, false)),
        Some(snapshot(true, true)),
    ]);
    let mut health_calls = 0usize;
    let mut health = |_url: &str| {
        health_calls += 1;
        true
    };

    let status = hermes_bridge::start_service_with_backend(
        &resolved,
        &mut backend,
        &mut health,
        Duration::from_millis(50),
        Duration::from_millis(1),
    )
    .expect("start");

    assert!(status.installed);
    assert!(status.running);
    assert!(status.bridge_available);
    assert_eq!(
        backend.events,
        vec!["status", "status", "install", "status", "start", "status"]
    );
    assert_eq!(health_calls, 1);
}

#[test]
fn hermes_bridge_start_with_backend_timeout_returns_last_status_not_error() {
    let resolved = test_resolved();
    let _fixture = prepare_bridge_config_fixture(&resolved);
    let mut backend = FakeBridgeBackend::new(vec![
        Some(snapshot(true, false)),
        Some(snapshot(true, true)),
        Some(snapshot(true, true)),
        Some(snapshot(true, true)),
        Some(snapshot(true, true)),
        Some(snapshot(true, true)),
        Some(snapshot(true, true)),
    ]);
    let mut health = |_url: &str| false;

    let status = hermes_bridge::start_service_with_backend(
        &resolved,
        &mut backend,
        &mut health,
        Duration::from_millis(3),
        Duration::from_millis(1),
    )
    .expect("timeout returns last status");

    assert!(status.installed);
    assert!(status.running);
    assert!(!status.bridge_available);
    assert!(status
        .warnings
        .contains(&"Hermes bridge health endpoint is not responding".to_string()));
    assert_eq!(backend.events[0], "status");
    assert_eq!(backend.events[1], "start");
}

#[test]
fn hermes_bridge_restart_requires_installed_like_go() {
    let resolved = test_resolved();
    let mut backend = FakeBridgeBackend::new(vec![Some(snapshot(false, false))]);
    let mut health = |_url: &str| true;

    let err = hermes_bridge::restart_service_with_backend(
        &resolved,
        &mut backend,
        &mut health,
        Duration::from_millis(10),
        Duration::from_millis(1),
    )
    .unwrap_err();

    assert_eq!(err.to_string(), "Hermes bridge service is not installed");
    assert_eq!(backend.events, vec!["status"]);
}

#[test]
fn hermes_bridge_uninstall_with_backend_stops_running_service_first() {
    let resolved = test_resolved();
    let _fixture = prepare_bridge_config_fixture(&resolved);
    let mut backend = FakeBridgeBackend::new(vec![
        Some(snapshot(true, true)),
        Some(snapshot(false, false)),
    ]);
    let mut health = |_url: &str| true;

    let status =
        hermes_bridge::uninstall_service_with_backend(&resolved, &mut backend, &mut health)
            .expect("uninstall");

    assert!(!status.installed);
    assert!(!status.running);
    assert_eq!(
        backend.events,
        vec!["status", "stop", "uninstall", "status"]
    );
}

fn first_12_sha256_hex(value: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(value.as_bytes());
    format!("{digest:x}")[..12].to_string()
}

fn test_resolved() -> Resolved {
    let root = std::env::temp_dir().join(format!(
        "awiki-cli-rs2-hermes-bridge-service-test-{}-{}",
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
        host_notify_sink: "hermes".to_string(),
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

fn path_string(path: &std::path::Path) -> String {
    path.to_string_lossy().into_owned()
}

fn prepare_bridge_config_fixture(resolved: &Resolved) -> BridgeConfigFixture {
    let env_lock = ENV_LOCK.lock().expect("env lock");
    let workspace = std::path::Path::new(&resolved.paths.workspace_home_dir);
    let hermes_home = workspace.join("hermes-home");
    let bin_dir = workspace.join("bin");
    fs::create_dir_all(&hermes_home).expect("Hermes home");
    fs::create_dir_all(&bin_dir).expect("test bin");
    fs::write(
        hermes_home.join("config.yaml"),
        r#"platforms:
  webhook:
    enabled: true
    extra:
      port: 8644
      routes:
        notify:
          secret: route-secret
          deliver: log
"#,
    )
    .expect("Hermes config");
    fs::write(
        &resolved.paths.config_file,
        r#"runtime:
  host_notify:
    enabled: true
    sink: hermes
    hermes:
      notify_url: http://127.0.0.1:8765/notify/host-event
      deliver: log
      secret: notify-secret
    webhook:
      notify_url: http://127.0.0.1:8765/notify/host-event
      secret: notify-secret
"#,
    )
    .expect("awiki config");
    write_executable(&bin_dir.join("python3"), "#!/bin/sh\nexit 0\n");
    let adapter_script = adapter_script_fixture_path();
    fs::create_dir_all(adapter_script.parent().expect("adapter script parent"))
        .expect("adapter script dir");
    fs::write(&adapter_script, "def main():\n    pass\n").expect("adapter script");

    let mut path_entries = vec![bin_dir];
    if let Some(current_path) = std::env::var_os("PATH") {
        path_entries.extend(std::env::split_paths(&current_path));
    }
    let joined_path = std::env::join_paths(path_entries).expect("join PATH");
    BridgeConfigFixture {
        _env_guards: vec![
            EnvGuard::set("HERMES_HOME", &hermes_home),
            EnvGuard::set("PATH", joined_path),
            EnvGuard::remove("AWIKI_HOST_NOTIFY_HERMES_SECRET"),
            EnvGuard::remove("AWIKI_HOST_NOTIFY_WEBHOOK_SECRET"),
        ],
        _env_lock: env_lock,
    }
}

fn adapter_script_fixture_path() -> std::path::PathBuf {
    let exe_path = std::env::current_exe().expect("current test exe");
    exe_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new(""))
        .join("..")
        .join("scripts")
        .join("hermes_notify_adapter.py")
}

fn write_executable(path: &std::path::Path, body: &str) {
    fs::write(path, body).expect("write executable");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(path)
            .expect("executable metadata")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).expect("chmod executable");
    }
}

#[cfg(unix)]
fn shell_quote(path: &std::path::Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "'\\''"))
}

struct BridgeConfigFixture {
    _env_guards: Vec<EnvGuard>,
    _env_lock: MutexGuard<'static, ()>,
}

struct EnvGuard {
    key: &'static str,
    original: Option<std::ffi::OsString>,
}

impl EnvGuard {
    fn set(key: &'static str, value: impl AsRef<std::ffi::OsStr>) -> Self {
        let original = std::env::var_os(key);
        std::env::set_var(key, value);
        Self { key, original }
    }

    fn remove(key: &'static str) -> Self {
        let original = std::env::var_os(key);
        std::env::remove_var(key);
        Self { key, original }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        if let Some(value) = self.original.as_ref() {
            std::env::set_var(self.key, value);
        } else {
            std::env::remove_var(self.key);
        }
    }
}

struct TempDir {
    path: std::path::PathBuf,
}

impl TempDir {
    fn new(name: &str) -> std::io::Result<Self> {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "awiki-cli-rs2-hermes-bridge-service-{name}-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&path)?;
        Ok(Self { path })
    }

    fn path(&self) -> &std::path::Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

struct FakeSystemctlRunner {
    calls: Vec<Vec<String>>,
    outputs: Vec<anyhow::Result<String>>,
}

impl FakeSystemctlRunner {
    fn new(outputs: Vec<Result<String, String>>) -> Self {
        Self {
            calls: Vec::new(),
            outputs: outputs
                .into_iter()
                .map(|result| result.map_err(anyhow::Error::msg))
                .collect(),
        }
    }
}

impl BridgeSystemdCommandRunner for FakeSystemctlRunner {
    fn run(&mut self, args: &[&str]) -> anyhow::Result<String> {
        self.calls
            .push(args.iter().map(|value| (*value).to_string()).collect());
        if self.outputs.is_empty() {
            return Ok(String::new());
        }
        self.outputs.remove(0)
    }
}

struct FakeBridgeBackend {
    events: Vec<&'static str>,
    snapshots: Vec<Option<BridgeServiceStatusSnapshot>>,
    repeat: Option<Option<BridgeServiceStatusSnapshot>>,
}

impl FakeBridgeBackend {
    fn new(snapshots: Vec<Option<BridgeServiceStatusSnapshot>>) -> Self {
        Self {
            events: Vec::new(),
            snapshots,
            repeat: None,
        }
    }
}

impl BridgeServiceBackend for FakeBridgeBackend {
    fn status_snapshot(
        &mut self,
        _resolved: &Resolved,
    ) -> anyhow::Result<Option<BridgeServiceStatusSnapshot>> {
        self.events.push("status");
        if let Some(snapshot) = self.repeat.as_ref() {
            return Ok(snapshot.clone());
        }
        Ok(if self.snapshots.is_empty() {
            None
        } else {
            self.snapshots.remove(0)
        })
    }

    fn install(&mut self, _resolved: &Resolved) -> anyhow::Result<()> {
        self.events.push("install");
        Ok(())
    }

    fn start(&mut self, _resolved: &Resolved) -> anyhow::Result<()> {
        self.events.push("start");
        Ok(())
    }

    fn stop(&mut self, _resolved: &Resolved) -> anyhow::Result<()> {
        self.events.push("stop");
        Ok(())
    }

    fn restart(&mut self, _resolved: &Resolved) -> anyhow::Result<()> {
        self.events.push("restart");
        Ok(())
    }

    fn uninstall(&mut self, _resolved: &Resolved) -> anyhow::Result<()> {
        self.events.push("uninstall");
        Ok(())
    }
}

fn snapshot(installed: bool, running: bool) -> BridgeServiceStatusSnapshot {
    BridgeServiceStatusSnapshot {
        installed,
        running,
        platform: "linux-systemd".to_string(),
        service_name: "awiki-cli-hermes-bridge-test.service".to_string(),
    }
}

fn test_bridge_config(hermes_home: &str) -> BridgeConfig {
    test_bridge_config_with_route_warnings(hermes_home, &[])
}

fn test_bridge_config_with_route_warnings(hermes_home: &str, warnings: &[&str]) -> BridgeConfig {
    BridgeConfig {
        notify_url: "http://127.0.0.1:8765/notify/host-event".to_string(),
        health_url: "http://127.0.0.1:8765/healthz".to_string(),
        adapter_host: "127.0.0.1".to_string(),
        adapter_port: 8765,
        notify_secret: "notify-secret".to_string(),
        notify_secret_source: "config_file".to_string(),
        hermes_home: hermes_home.to_string(),
        hermes_config_file: "/workspace/.hermes/config.yaml".to_string(),
        hermes_webhook_url: "http://127.0.0.1:8644/webhooks/notify".to_string(),
        route_name: "notify".to_string(),
        route_secret: "route-secret".to_string(),
        route_state: test_route_state(hermes_home, warnings),
        adapter_script: "/opt/awiki/scripts/hermes_notify_adapter.py".to_string(),
        python_executable: "/usr/bin/python3".to_string(),
    }
}

fn test_route_state(hermes_home: &str, warnings: &[&str]) -> RouteState {
    RouteState {
        hermes_home: hermes_home.to_string(),
        config_file: "/workspace/.hermes/config.yaml".to_string(),
        env_file: "/workspace/.hermes/.env".to_string(),
        config_exists: true,
        webhook_enabled: true,
        webhook_port: DEFAULT_WEBHOOK_PORT,
        route_name: "notify".to_string(),
        route_configured: true,
        route_secret: "route-secret".to_string(),
        route_secret_configured: true,
        deliver: "feishu".to_string(),
        deliver_uses_home_channel: true,
        home_channel_key: "FEISHU_HOME_CHANNEL".to_string(),
        home_channel: "chat-id".to_string(),
        home_channel_configured: true,
        home_channel_supported: true,
        feishu_credentials_configured: true,
        notify_webhook_url: "http://127.0.0.1:8644/webhooks/notify".to_string(),
        warnings: warnings.iter().map(|value| (*value).to_string()).collect(),
    }
}
