use awiki_cli::config::{Paths, Resolved};
use awiki_cli::runtime::{self, bridge};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn bridge_request_response_and_error_shapes_match_go_contract() {
    let request = bridge::BridgeRequest {
        method: "msg.inbox".to_string(),
        params: serde_json::Map::new(),
        identity_name: "alice".to_string(),
    };
    let request_json = serde_json::to_value(&request).expect("request json");
    assert_eq!(request_json["method"], "msg.inbox");
    assert_eq!(request_json["identity_name"], "alice");
    assert_eq!(request_json["params"], serde_json::json!({}));

    let response = bridge::BridgeResponse {
        ok: false,
        result: serde_json::Map::new(),
        error: Some(bridge::BridgeError {
            code: "unavailable".to_string(),
            message: "listener down".to_string(),
        }),
    };
    let response_json = serde_json::to_value(&response).expect("response json");
    assert_eq!(response_json["ok"], false);
    assert_eq!(response_json["error"]["code"], "unavailable");
    assert_eq!(response_json["error"]["message"], "listener down");
    assert!(response_json.get("result").is_none());

    assert_eq!(
        bridge::BridgeCallError::new(
            "bridge_dial",
            "local websocket bridge unavailable",
            "dial failed"
        )
        .to_string(),
        "local websocket bridge request failed: local websocket bridge unavailable: dial failed"
    );
    assert_eq!(
        bridge::BridgeCallError::new("bridge_read", "bridge returned failure without details", "")
            .to_string(),
        "local websocket bridge request failed: bridge returned failure without details"
    );
}

#[test]
fn bridge_default_endpoint_uses_state_dir_or_workspace_runtime_dir_like_go() {
    let paths = Paths {
        workspace_home_dir: "/tmp/awiki-workspace".to_string(),
        state_dir: "/tmp/awiki-state".to_string(),
        ..test_paths()
    };
    assert_eq!(
        bridge::default_bridge_endpoint(&paths),
        platform_default_endpoint("/tmp/awiki-workspace", "/tmp/awiki-state")
    );

    let paths = Paths {
        workspace_home_dir: "/tmp/awiki-workspace".to_string(),
        state_dir: String::new(),
        ..test_paths()
    };
    assert_eq!(
        bridge::default_bridge_endpoint(&paths),
        platform_default_endpoint("/tmp/awiki-workspace", "")
    );
}

#[test]
fn bridge_resolve_shortens_long_unix_socket_path_like_go_runtime_resolve() {
    let long_state_dir = std::path::Path::new("/tmp").join("very-long-runtime-dir-".repeat(10));
    let resolved = Resolved {
        runtime_mode: "websocket".to_string(),
        paths: Paths {
            workspace_home_dir: "/tmp/awiki-workspace".to_string(),
            state_dir: long_state_dir.to_string_lossy().into_owned(),
            ..test_paths()
        },
        ..test_resolved()
    };
    let runtime = runtime::resolve(&resolved);
    assert_eq!(runtime.mode, bridge::MODE_WEBSOCKET);

    #[cfg(not(windows))]
    {
        assert!(runtime.socket_path.len() <= bridge::MAX_UNIX_SOCKET_PATH_BYTES);
        assert!(runtime.socket_path.starts_with(
            &std::env::temp_dir()
                .join("awiki-cli-")
                .to_string_lossy()
                .into_owned()
        ));
        assert!(runtime.socket_path.ends_with(".sock"));
    }

    #[cfg(windows)]
    {
        assert!(runtime.socket_path.starts_with(r"\\.\pipe\awiki-cli-"));
    }
}

#[test]
fn bridge_resolve_keeps_configured_short_socket_path_like_go() {
    let resolved = Resolved {
        runtime_mode: "websocket".to_string(),
        runtime_socket_path: "/tmp/custom-awiki.sock".to_string(),
        paths: Paths {
            workspace_home_dir: "/tmp/awiki-workspace".to_string(),
            state_dir: "/tmp/.awiki-cli/runtime".to_string(),
            ..test_paths()
        },
        ..test_resolved()
    };
    let runtime = runtime::resolve(&resolved);
    assert_eq!(
        runtime.socket_path,
        bridge::normalize_bridge_endpoint("/tmp/custom-awiki.sock")
    );
}

#[test]
fn bridge_prepare_and_available_match_go_local_endpoint_helpers() {
    let workspace = TempDir::new().expect("temp workspace");
    let socket_path = workspace.path().join("runtime").join("message-daemon.sock");
    bridge::prepare_bridge_endpoint(socket_path.to_str().expect("socket path"))
        .expect("prepare bridge endpoint");

    #[cfg(not(windows))]
    {
        assert!(socket_path.parent().expect("parent").is_dir());
        assert!(!bridge::bridge_endpoint_available(
            socket_path.to_str().expect("socket path")
        ));
        std::fs::write(&socket_path, b"socket placeholder").expect("write placeholder");
        assert!(bridge::bridge_endpoint_available(
            socket_path.to_str().expect("socket path")
        ));
    }

    #[cfg(windows)]
    {
        assert!(bridge::prepare_bridge_endpoint(r"\\.\pipe\awiki-cli-test").is_ok());
        assert!(bridge::prepare_bridge_endpoint(r"C:\tmp\awiki.sock").is_err());
    }
}

fn platform_default_endpoint(workspace_home_dir: &str, state_dir: &str) -> String {
    #[cfg(not(windows))]
    {
        let root = if state_dir.trim().is_empty() {
            std::path::Path::new(workspace_home_dir).join("runtime")
        } else {
            std::path::Path::new(state_dir).to_path_buf()
        };
        root.join("message-daemon.sock")
            .to_string_lossy()
            .into_owned()
    }
    #[cfg(windows)]
    {
        let workspace = if workspace_home_dir.trim().is_empty() {
            std::env::temp_dir()
                .join("awiki-cli")
                .to_string_lossy()
                .into_owned()
        } else {
            workspace_home_dir.to_string()
        };
        use sha2::{Digest, Sha256};
        let digest = Sha256::digest(workspace.as_bytes());
        format!(r"\\.\pipe\awiki-cli-{}", &format!("{digest:x}")[..16])
    }
}

fn test_resolved() -> Resolved {
    Resolved {
        paths: test_paths(),
        config_schema_version: 1,
        active_identity: String::new(),
        runtime_mode: "websocket".to_string(),
        runtime_socket_path: String::new(),
        runtime_listener_enabled: true,
        runtime_listener_auto_install: true,
        runtime_listener_auto_start: true,
        host_notify_enabled: true,
        host_notify_sink: "log".to_string(),
        host_notify_file_path: String::new(),
        host_notify_openclaw_hook_url: String::new(),
        host_notify_openclaw_agent_id: String::new(),
        host_notify_openclaw_hook_name: String::new(),
        host_notify_hermes_notify_url: String::new(),
        host_notify_hermes_deliver: String::new(),
        output_format: "json".to_string(),
        no_color: false,
        service_base_url: "https://awiki.ai".to_string(),
        did_domain: "awiki.ai".to_string(),
        anp_service_endpoint: "https://awiki.ai/anp-im/rpc".to_string(),
        anp_service_did: "did:wba:awiki.ai".to_string(),
        mail_service_url: "https://awiki.ai".to_string(),
        ca_bundle: String::new(),
        update_disable_strict_version: false,
        update_metadata_cache_ttl_seconds: 0,
        config_exists: false,
        config_error: String::new(),
        env_hits: Vec::new(),
        sources: std::collections::BTreeMap::new(),
    }
}

fn test_paths() -> Paths {
    Paths {
        workspace_home_dir: "/tmp/awiki-workspace".to_string(),
        root_dir: "/tmp/awiki-workspace".to_string(),
        config_dir: "/tmp/awiki-workspace".to_string(),
        data_dir: "/tmp/awiki-workspace/data".to_string(),
        state_dir: "/tmp/awiki-workspace/runtime".to_string(),
        cache_dir: "/tmp/awiki-workspace/cache".to_string(),
        logs_dir: "/tmp/awiki-workspace/logs".to_string(),
        config_file: "/tmp/awiki-workspace/config.yaml".to_string(),
        identity_dir: "/tmp/awiki-workspace/identities".to_string(),
        database_file: "/tmp/awiki-workspace/awiki-cli.db".to_string(),
        legacy_credentials_dir: String::new(),
        legacy_data_dir: String::new(),
    }
}

struct TempDir {
    path: std::path::PathBuf,
}

impl TempDir {
    fn new() -> std::io::Result<Self> {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "awiki-cli-rs2-runtime-bridge-test-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path)?;
        Ok(Self { path })
    }

    fn path(&self) -> &std::path::Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
