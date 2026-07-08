use awiki_cli::host_runtime::listener_launchd::{self, LaunchdStatus};
use awiki_cli::host_runtime::listener_service;
use awiki_cli::workspace_config::{Paths, Resolved};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn listener_launchd_agent_matches_go_launch_agent_contract() {
    let resolved = test_resolved();
    let executable = "/Applications/AWiki CLI/bin/awiki-cli";
    let home = std::path::Path::new("/Users/alice");
    let agent = listener_launchd::agent_for_home_and_executable(&resolved, home, executable)
        .expect("agent");
    let label = listener_service::service_name_for(&resolved);

    assert_eq!(listener_launchd::service_platform(), "launchd");
    assert_eq!(agent.label, label);
    assert_eq!(listener_launchd::label_for(&resolved), agent.label);
    assert_eq!(listener_launchd::name_for(&resolved), agent.label);
    assert_eq!(
        agent.path,
        home.join("Library")
            .join("LaunchAgents")
            .join(format!("{}.plist", agent.label))
    );
    assert_eq!(
        listener_launchd::plist_path_for_home(&resolved, home).expect("plist path"),
        agent.path
    );

    assert!(agent
        .content
        .contains("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"));
    assert!(agent.content.contains("<plist version=\"1.0\">"));
    assert!(agent.content.contains("<key>Disabled</key>\n\t<false/>"));
    assert!(agent.content.contains("<key>Label</key>"));
    assert!(agent
        .content
        .contains(&format!("<string>{}</string>", agent.label)));
    assert!(agent.content.contains("<key>ProgramArguments</key>"));
    assert!(agent
        .content
        .contains("<string>/Applications/AWiki CLI/bin/awiki-cli</string>"));
    assert!(agent.content.contains("<string>runtime</string>"));
    assert!(agent.content.contains("<string>listener</string>"));
    assert!(agent.content.contains("<string>service-run</string>"));
    assert!(agent.content.contains("<key>RunAtLoad</key>\n\t<true/>"));
    assert!(agent.content.contains("<key>KeepAlive</key>\n\t<true/>"));
    assert!(agent
        .content
        .contains("<key>SessionCreate</key>\n\t<false/>"));
    assert!(agent.content.contains("<key>WorkingDirectory</key>"));
    assert!(agent.content.contains(&format!(
        "<string>{}</string>",
        resolved.paths.workspace_home_dir
    )));
    assert!(agent.content.contains("<key>EnvironmentVariables</key>"));
    assert!(agent
        .content
        .contains("<key>AWIKI_CLI_WORKSPACE_HOME_DIR</key>"));
    assert!(agent.content.contains(&format!(
        "<string>{}</string>",
        resolved.paths.workspace_home_dir
    )));
    assert!(agent
        .content
        .contains("<key>AWIKI_LISTENER_SERVICE_MODE</key>"));
    assert!(agent.content.contains("<string>1</string>"));
    assert!(agent.content.contains("<key>StandardOutPath</key>"));
    assert!(agent.content.contains("<key>StandardErrorPath</key>"));
    assert!(agent.content.contains(&format!(
        "<string>{}/{}.out.log</string>",
        resolved.paths.logs_dir, agent.label
    )));
    assert!(agent.content.contains(&format!(
        "<string>{}/{}.err.log</string>",
        resolved.paths.logs_dir, agent.label
    )));

    let plan = listener_service::service_config_plan_for(&resolved, false);
    assert_eq!(plan.description, "awiki-cli realtime websocket listener");
    assert_eq!(
        plan.options
            .get("PIDFile")
            .expect("PIDFile option from shared config plan"),
        &listener_service::ListenerServiceConfigValue::String(path_string(
            &std::path::Path::new(&resolved.paths.state_dir).join("listener.service.pid")
        ))
    );
}

#[test]
fn listener_launchd_plist_escapes_xml_contract_values() {
    let mut resolved = test_resolved();
    resolved.paths.workspace_home_dir = "/tmp/awiki & <workspace>".to_string();
    resolved.paths.logs_dir = "/tmp/awiki \"logs\"".to_string();

    let plist =
        listener_launchd::plist_content_for_executable(&resolved, "/tmp/bin/awiki-cli&test");

    assert!(plist.contains("<string>/tmp/bin/awiki-cli&amp;test</string>"));
    assert!(plist.contains("<string>/tmp/awiki &amp; &lt;workspace&gt;</string>"));
    assert!(plist.contains("<string>/tmp/awiki &#34;logs&#34;/"));
    assert!(!plist.contains("/tmp/awiki & <workspace>"));
}

#[test]
fn listener_launchd_status_parser_maps_launchctl_list_output() {
    assert_eq!(
        listener_launchd::parse_launchctl_status(
            r#"{
    "LimitLoadToSessionType" = "Aqua";
    "Label" = "awiki-cli-listener-test";
    "OnDemand" = true;
    "LastExitStatus" = 0;
    "PID" = 30656;
}"#
        ),
        LaunchdStatus {
            installed: true,
            running: true,
            pid: Some(30656),
            last_exit_status: Some(0),
            raw_state: "{".to_string(),
        }
    );

    assert_eq!(
        listener_launchd::parse_launchctl_status(
            r#"{
    "Label" = "awiki-cli-listener-test";
    "LastExitStatus" = 78;
}"#
        ),
        LaunchdStatus {
            installed: true,
            running: false,
            pid: None,
            last_exit_status: Some(78),
            raw_state: "{".to_string(),
        }
    );

    assert_eq!(
        listener_launchd::parse_launchctl_status("Could not find service \"awiki-cli-listener\""),
        LaunchdStatus {
            installed: false,
            running: false,
            pid: None,
            last_exit_status: None,
            raw_state: "Could not find service \"awiki-cli-listener\"".to_string(),
        }
    );

    assert_eq!(
        listener_launchd::parse_launchctl_status(""),
        LaunchdStatus {
            installed: false,
            running: false,
            pid: None,
            last_exit_status: None,
            raw_state: String::new(),
        }
    );
}

fn test_resolved() -> Resolved {
    let root = std::env::temp_dir().join(format!(
        "awiki-cli-rs2-listener-launchd-test-{}-{}",
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
