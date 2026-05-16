use awiki_cli::config::{Paths, Resolved};
use awiki_cli::runtime::hermes_bridge::{
    self, BridgeApplyDecision, BridgeConfig, BridgeStatus, RouteState, DEFAULT_WEBHOOK_PORT,
    SERVICE_ARGUMENTS, SERVICE_DESCRIPTION, SERVICE_DISPLAY_NAME_PREFIX, SERVICE_NAME_PREFIX,
};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

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

fn test_bridge_config(hermes_home: &str) -> BridgeConfig {
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
        route_state: test_route_state(hermes_home),
        adapter_script: "/opt/awiki/scripts/hermes_notify_adapter.py".to_string(),
        python_executable: "/usr/bin/python3".to_string(),
    }
}

fn test_route_state(hermes_home: &str) -> RouteState {
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
        warnings: Vec::new(),
    }
}
