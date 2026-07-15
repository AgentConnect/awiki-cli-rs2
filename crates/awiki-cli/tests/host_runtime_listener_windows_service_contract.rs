use awiki_cli::host_runtime::listener_service::{
    self, ListenerPlatformStatusResult, ListenerServiceConfigValue as ConfigValue,
};
use awiki_cli::host_runtime::listener_windows_service;
use awiki_cli::workspace_config::{Paths, Resolved};
use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn windows_listener_service_contract_matches_go_kardianos_config_intent() {
    let resolved = test_resolved();
    let contract = listener_windows_service::service_contract_for(&resolved);

    assert_eq!(
        listener_windows_service::service_platform(),
        "windows-service"
    );
    assert_eq!(contract.platform, "windows-service");
    assert_eq!(contract.name, listener_service::service_name_for(&resolved));
    assert_eq!(
        contract.display_name,
        listener_service::service_display_name_for(Some(&resolved))
    );
    assert_eq!(
        contract.description,
        "awiki-cli realtime websocket listener"
    );
    assert_eq!(
        contract.arguments,
        listener_service::service_arguments_for(&resolved)
    );
    assert_eq!(
        listener_windows_service::service_arguments(&resolved),
        contract.arguments
    );
    assert_eq!(contract.working_directory, "");
    assert!(contract.managed_by_scm);

    let mut expected_env = BTreeMap::new();
    expected_env.insert(
        "AWIKI_CLI_WORKSPACE_HOME_DIR".to_string(),
        resolved.paths.workspace_home_dir.clone(),
    );
    expected_env.insert("AWIKI_CLI_INTERNAL_ENTRY".to_string(), "1".to_string());
    expected_env.insert("AWIKI_LISTENER_SERVICE_MODE".to_string(), "1".to_string());
    assert_eq!(contract.env_vars, expected_env);

    assert_eq!(contract.start_type, "automatic");
    assert!(contract.auto_start);
    assert!(contract.restart_on_failure);
    assert_eq!(contract.restart_delay, "1s");
    assert!(contract.log_output);
    assert_eq!(contract.log_directory, resolved.paths.logs_dir);
    assert_eq!(
        contract.pid_file,
        path_string(&std::path::Path::new(&resolved.paths.state_dir).join("listener.service.pid"))
    );
}

#[test]
fn windows_listener_service_config_plan_preserves_existing_listener_names_and_options() {
    let resolved = test_resolved();
    let plan = listener_windows_service::service_config_plan_for(&resolved);

    assert_eq!(
        listener_windows_service::service_name_for(&resolved),
        plan.name
    );
    assert_eq!(
        listener_windows_service::service_display_name_for(&resolved),
        plan.display_name
    );
    assert_eq!(plan.working_directory, "");
    assert_eq!(
        plan.arguments,
        listener_service::service_arguments_for(&resolved)
    );

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
}

#[test]
fn windows_listener_service_binds_launch_and_pipe_access_to_installing_user() {
    let resolved = test_resolved();
    let user_sid = "S-1-5-21-1000-2000-3000-1001";
    let arguments = listener_windows_service::service_launch_arguments(&resolved, user_sid)
        .expect("service launch arguments");

    assert!(arguments
        .windows(2)
        .any(|pair| { pair == [listener_service::INTERNAL_SERVICE_USER_SID_FLAG, user_sid,] }));
    assert!(arguments.ends_with(&[
        "runtime".to_string(),
        "listener".to_string(),
        "service-run".to_string(),
    ]));
    assert_eq!(
        listener_windows_service::pipe_security_sddl(user_sid).expect("pipe security descriptor"),
        format!("D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GRGW;;;{user_sid})")
    );

    for invalid in ["", "S-1-", "S-1--5", "s-1-5-21", "S-1-5-user"] {
        assert!(!listener_windows_service::valid_user_sid(invalid));
        assert!(listener_windows_service::service_launch_arguments(&resolved, invalid).is_err());
        assert!(listener_windows_service::pipe_security_sddl(invalid).is_err());
    }
}

#[test]
fn windows_listener_status_parser_maps_service_states_without_shelling_out() {
    assert_eq!(
        listener_windows_service::parse_status_state("RUNNING"),
        listener_windows_service::WindowsServiceStatus {
            installed: true,
            running: true,
            state: "running".to_string(),
        }
    );
    assert_eq!(
        listener_windows_service::parse_status_state("Start Pending"),
        listener_windows_service::WindowsServiceStatus {
            installed: true,
            running: true,
            state: "start-pending".to_string(),
        }
    );
    assert_eq!(
        listener_windows_service::parse_status_state("SERVICE_RUNNING"),
        listener_windows_service::WindowsServiceStatus {
            installed: true,
            running: true,
            state: "running".to_string(),
        }
    );
    assert_eq!(
        listener_windows_service::parse_status_state(
            "SERVICE_NAME: awiki-cli-listener\n        STATE              : 4  RUNNING\n"
        ),
        listener_windows_service::WindowsServiceStatus {
            installed: true,
            running: true,
            state: "running".to_string(),
        }
    );
    assert_eq!(
        listener_windows_service::parse_status_state("STOPPED"),
        listener_windows_service::WindowsServiceStatus {
            installed: true,
            running: false,
            state: "stopped".to_string(),
        }
    );
    assert_eq!(
        listener_windows_service::parse_status_state("not installed"),
        listener_windows_service::WindowsServiceStatus {
            installed: false,
            running: false,
            state: "not-installed".to_string(),
        }
    );
    assert_eq!(
        listener_windows_service::parse_status_state(
            "The specified service does not exist as an installed service."
        ),
        listener_windows_service::WindowsServiceStatus {
            installed: false,
            running: false,
            state: "the-specified-service-does-not-exist-as-an-installed-service".to_string(),
        }
    );

    assert_eq!(
        listener_windows_service::status_result_from_state("running"),
        ListenerPlatformStatusResult::Running
    );
    assert_eq!(
        listener_windows_service::status_result_from_state("paused"),
        ListenerPlatformStatusResult::NotRunning
    );
    assert_eq!(
        listener_windows_service::status_result_from_state("missing"),
        ListenerPlatformStatusResult::ErrNotInstalled
    );
}

fn test_resolved() -> Resolved {
    let root = std::env::temp_dir().join(format!(
        "awiki-cli-rs2-listener-windows-service-test-{}-{}",
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
        user_service_endpoint: String::new(),
        message_service_endpoint: String::new(),
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
