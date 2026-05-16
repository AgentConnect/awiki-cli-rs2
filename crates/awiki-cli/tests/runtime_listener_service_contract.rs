use awiki_cli::config::{Paths, Resolved};
use awiki_cli::runtime::listener::{self, Status};
use awiki_cli::runtime::listener_service;
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

fn path_string(path: &std::path::Path) -> String {
    path.to_string_lossy().into_owned()
}
