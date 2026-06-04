use awiki_cli::host_runtime::{self, bridge};
use awiki_cli::workspace_config::{Paths, Resolved};
use std::sync::atomic::{AtomicU64, Ordering};
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

    let empty_request: bridge::BridgeRequest =
        serde_json::from_str("{}").expect("empty request shape");
    assert_eq!(empty_request.method, "");
    assert_eq!(empty_request.identity_name, "");
    assert!(empty_request.params.is_empty());

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

    let null_result: bridge::BridgeResponse =
        serde_json::from_str(r#"{"ok":true,"result":null}"#).expect("null result response");
    assert!(null_result.result.is_empty());

    let missing_ok: bridge::BridgeResponse =
        serde_json::from_str(r#"{"result":{"count":1}}"#).expect("missing ok response");
    assert!(!missing_ok.ok);
    assert_eq!(missing_ok.result["count"], serde_json::json!(1));

    let missing_message: bridge::BridgeResponse =
        serde_json::from_str(r#"{"ok":false,"error":{"code":"failed"}}"#)
            .expect("missing error message response");
    assert_eq!(
        missing_message.error.expect("bridge error").message,
        String::new()
    );
}

#[test]
fn bridge_server_framing_dispatches_one_newline_terminated_request() {
    let request = serde_json::json!({
        "method": "direct.send",
        "identity_name": "alice",
        "params": {
            "target": "did:example:bob",
            "text": "hello"
        }
    });
    let mut stream = MemoryDuplex::new(format!("{request}\nignored-second-frame\n").into_bytes());
    let mut seen_request = None;

    bridge::handle_bridge_connection_once(&mut stream, |request| {
        seen_request = Some(request);
        Ok(serde_json::Map::from_iter([(
            "message_id".to_string(),
            serde_json::json!("msg-123"),
        )]))
    })
    .expect("handle bridge connection");

    let seen_request = seen_request.expect("dispatch should be called");
    assert_eq!(seen_request.method, "direct.send");
    assert_eq!(seen_request.identity_name, "alice");
    assert_eq!(
        seen_request.params["target"],
        serde_json::json!("did:example:bob")
    );
    assert_eq!(seen_request.params["text"], serde_json::json!("hello"));

    assert!(stream.output_string().ends_with('\n'));
    let response_json: serde_json::Value =
        serde_json::from_slice(stream.output()).expect("response json");
    assert_eq!(response_json["ok"], true);
    assert_eq!(response_json["result"]["message_id"], "msg-123");
    assert!(response_json.get("error").is_none());
}

#[test]
fn bridge_server_framing_reports_invalid_json_without_dispatch() {
    let mut stream = MemoryDuplex::new(b"not-json\n".to_vec());
    let mut called = false;

    bridge::handle_bridge_connection_once(&mut stream, |_request| {
        called = true;
        Ok(serde_json::Map::new())
    })
    .expect("invalid json response write");

    assert!(!called);
    let response_json: serde_json::Value =
        serde_json::from_slice(stream.output()).expect("response json");
    assert_eq!(response_json["ok"], false);
    assert!(response_json["error"]["message"]
        .as_str()
        .expect("error message")
        .contains("expected ident"));
    assert!(response_json["error"].get("code").is_none());
    assert!(response_json.get("result").is_none());
}

#[test]
fn bridge_server_framing_requires_newline_like_go_read_bytes() {
    let request = serde_json::json!({
        "method": "direct.send",
        "identity_name": "alice",
        "params": {}
    });
    let mut stream = MemoryDuplex::new(request.to_string().into_bytes());
    let mut called = false;

    bridge::handle_bridge_connection_once(&mut stream, |_request| {
        called = true;
        Ok(serde_json::Map::new())
    })
    .expect("eof response write");

    assert!(!called);
    let response_json: serde_json::Value =
        serde_json::from_slice(stream.output()).expect("response json");
    assert_eq!(response_json["ok"], false);
    assert_eq!(response_json["error"]["message"], "EOF");
    assert!(response_json.get("result").is_none());
}

#[test]
fn bridge_server_framing_writes_dispatch_errors_as_bridge_errors() {
    let request = serde_json::json!({
        "method": "group.unknown",
        "identity_name": "alice",
        "params": {}
    });
    let mut stream = MemoryDuplex::new(format!("{request}\n").into_bytes());

    bridge::handle_bridge_connection_once(&mut stream, |_request| {
        anyhow::bail!("unsupported websocket bridge method: group.unknown");
    })
    .expect("dispatch error response write");

    let response_json: serde_json::Value =
        serde_json::from_slice(stream.output()).expect("response json");
    assert_eq!(response_json["ok"], false);
    assert_eq!(
        response_json["error"]["message"],
        "unsupported websocket bridge method: group.unknown"
    );
    assert!(response_json["error"].get("code").is_none());
    assert!(response_json.get("result").is_none());
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
fn windows_bridge_endpoint_helpers_match_go_named_pipe_rules() {
    assert_eq!(
        bridge::windows_default_bridge_endpoint_from_workspace(r"C:\Users\alice\.awiki-cli"),
        r"\\.\pipe\awiki-cli-28b39b869f8ed623"
    );
    assert_eq!(
        bridge::windows_default_bridge_endpoint_for_parts("", r"C:\Temp\awiki-cli"),
        r"\\.\pipe\awiki-cli-77c3b9844ad97fe4"
    );
    assert_eq!(
        bridge::windows_default_bridge_endpoint_for_parts(
            r"  C:\Users\alice\.awiki-cli  ",
            r"C:\Temp\awiki-cli"
        ),
        r"\\.\pipe\awiki-cli-5ad18bdfc9cd556b"
    );

    assert_eq!(
        bridge::normalize_windows_bridge_endpoint("  ", r"C:\Temp\awiki-cli"),
        r"\\.\pipe\awiki-cli-77c3b9844ad97fe4"
    );
    assert_eq!(
        bridge::normalize_windows_bridge_endpoint(r"  \\.\PIPE\Awiki-Cli-Test  ", ""),
        r"\\.\PIPE\Awiki-Cli-Test"
    );

    assert!(bridge::is_windows_named_pipe_endpoint(
        r"\\.\pipe\awiki-cli-test"
    ));
    assert!(bridge::is_windows_named_pipe_endpoint(
        r"\\.\PIPE\AWIKI-CLI-TEST"
    ));
    assert!(!bridge::is_windows_named_pipe_endpoint(
        r" \\.\pipe\awiki-cli-test"
    ));
    assert!(!bridge::is_windows_named_pipe_endpoint(
        r"C:\tmp\awiki.sock"
    ));
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
    let runtime = host_runtime::resolve(&resolved);
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
    let runtime = host_runtime::resolve(&resolved);
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

#[cfg(unix)]
#[test]
fn call_local_bridge_uses_health_probe_then_request_connection() {
    let socket_dir = TempDir::new_short("abr").expect("temp socket dir");
    let socket_path = socket_dir.path().join("s.sock");
    let (_server, events) = spawn_two_connection_bridge_server(
        &socket_path,
        serde_json::json!({
            "ok": true,
            "result": {
                "accepted": true,
                "message_id": "msg-123",
                "seq": 42
            }
        })
        .to_string(),
    );

    let result = bridge::call_local_bridge(
        sample_bridge_request(),
        &test_resolved_with_socket(socket_path.to_str().expect("socket path")),
    )
    .expect("call local bridge");

    assert_eq!(result.get("accepted"), Some(&serde_json::json!(true)));
    assert_eq!(
        result.get("message_id"),
        Some(&serde_json::json!("msg-123"))
    );
    assert_eq!(result.get("seq"), Some(&serde_json::json!(42)));

    let event = events
        .recv_timeout(std::time::Duration::from_secs(2))
        .expect("health probe event");
    assert!(matches!(event, BridgeServerEvent::ProbeAccepted));

    let event = events
        .recv_timeout(std::time::Duration::from_secs(2))
        .expect("request event");
    let BridgeServerEvent::Request(request_json) = event else {
        panic!("second bridge connection should carry request JSON");
    };
    assert_eq!(request_json["method"], "direct.send");
    assert_eq!(request_json["identity_name"], "alice");
    assert_eq!(request_json["params"]["target"], "did:example:bob");
    assert_eq!(request_json["params"]["text"], "hello over bridge");
}

#[cfg(unix)]
#[test]
fn call_local_bridge_failure_response_uses_bridge_error_message() {
    let socket_dir = TempDir::new_short("abr").expect("temp socket dir");
    let socket_path = socket_dir.path().join("s.sock");
    let (_server, _events) = spawn_two_connection_bridge_server(
        &socket_path,
        serde_json::json!({
            "ok": false,
            "error": {
                "code": "session_missing",
                "message": "websocket session is not connected for identity alice"
            }
        })
        .to_string(),
    );

    let err = bridge::call_local_bridge(
        sample_bridge_request(),
        &test_resolved_with_socket(socket_path.to_str().expect("socket path")),
    )
    .expect_err("bridge error response should fail");

    assert_bridge_call_error_phase(&err, "bridge_read");
    assert_eq!(
        err.to_string(),
        "local websocket bridge request failed: websocket session is not connected for identity alice"
    );
}

#[cfg(unix)]
#[test]
fn call_local_bridge_failure_without_error_details_matches_go_message() {
    let socket_dir = TempDir::new_short("abr").expect("temp socket dir");
    let socket_path = socket_dir.path().join("s.sock");
    let (_server, _events) = spawn_two_connection_bridge_server(
        &socket_path,
        serde_json::json!({ "ok": false }).to_string(),
    );

    let err = bridge::call_local_bridge(
        sample_bridge_request(),
        &test_resolved_with_socket(socket_path.to_str().expect("socket path")),
    )
    .expect_err("bridge failure without error details should fail");

    assert_bridge_call_error_phase(&err, "bridge_read");
    assert_eq!(
        err.to_string(),
        "local websocket bridge request failed: bridge returned failure without details"
    );
}

#[cfg(unix)]
#[test]
fn call_local_bridge_invalid_json_response_reports_decode_error() {
    let socket_dir = TempDir::new_short("abr").expect("temp socket dir");
    let socket_path = socket_dir.path().join("s.sock");
    let (_server, _events) =
        spawn_two_connection_bridge_server(&socket_path, "not-json\n".to_string());

    let err = bridge::call_local_bridge(
        sample_bridge_request(),
        &test_resolved_with_socket(socket_path.to_str().expect("socket path")),
    )
    .expect_err("invalid bridge response should fail");

    assert_bridge_call_error_phase(&err, "bridge_read");
    assert!(
        err.to_string().starts_with(
            "local websocket bridge request failed: decode websocket bridge response:"
        ),
        "unexpected error: {err}"
    );
}

#[cfg(unix)]
#[test]
fn call_local_bridge_wraps_unavailable_health_probe_like_go() {
    let workspace = TempDir::new().expect("temp workspace");
    let socket_path = workspace.path().join("runtime").join("missing.sock");

    let err = bridge::call_local_bridge(
        sample_bridge_request(),
        &test_resolved_with_socket(socket_path.to_str().expect("socket path")),
    )
    .expect_err("missing bridge should fail during health probe");

    assert_bridge_call_error_phase(&err, "bridge_health_probe");
    assert!(
        err.to_string()
            .contains("local websocket bridge request failed: local websocket bridge unavailable:"),
        "unexpected error: {err}"
    );
}

#[cfg(unix)]
#[test]
fn bridge_health_probe_reports_unavailable_unix_socket() {
    let workspace = TempDir::new().expect("temp workspace");
    let socket_path = workspace.path().join("runtime").join("missing.sock");

    let err = bridge::bridge_health_probe(
        socket_path.to_str().expect("socket path"),
        std::time::Duration::from_millis(25),
    )
    .expect_err("missing bridge socket should not pass health probe");

    assert!(
        !err.to_string().trim().is_empty(),
        "health probe error should include the dial failure"
    );
}

#[cfg(unix)]
#[test]
fn listen_bridge_removes_stale_socket_path_before_binding() {
    let socket_dir = TempDir::new_short("abr").expect("temp socket dir");
    let socket_path = socket_dir.path().join("s.sock");
    std::fs::create_dir_all(socket_path.parent().expect("socket parent"))
        .expect("create runtime dir");
    std::fs::write(&socket_path, b"stale socket placeholder").expect("write stale path");
    assert!(socket_path.exists());

    let listener =
        bridge::listen_bridge(socket_path.to_str().expect("socket path")).expect("listen bridge");

    assert!(
        bridge::bridge_health_probe(
            socket_path.to_str().expect("socket path"),
            std::time::Duration::from_millis(250),
        )
        .is_ok(),
        "fresh listener should accept a health probe after replacing stale path"
    );
    drop(listener);
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

#[cfg(unix)]
#[derive(Debug)]
enum BridgeServerEvent {
    ProbeAccepted,
    Request(serde_json::Value),
}

#[cfg(unix)]
fn spawn_two_connection_bridge_server(
    socket_path: &std::path::Path,
    response_line: String,
) -> (
    std::thread::JoinHandle<()>,
    std::sync::mpsc::Receiver<BridgeServerEvent>,
) {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixListener;

    std::fs::create_dir_all(socket_path.parent().expect("socket parent"))
        .expect("create socket parent");
    let _ = std::fs::remove_file(socket_path);
    let listener = UnixListener::bind(socket_path).expect("bind test bridge socket");
    listener
        .set_nonblocking(true)
        .expect("set bridge listener nonblocking");
    let (events_tx, events_rx) = std::sync::mpsc::channel();
    let response_line = if response_line.ends_with('\n') {
        response_line
    } else {
        format!("{response_line}\n")
    };

    let handle = std::thread::spawn(move || {
        let (probe_stream, _) =
            accept_unix_connection(&listener).expect("accept health probe connection");
        let _ = events_tx.send(BridgeServerEvent::ProbeAccepted);
        drop(probe_stream);

        let (mut request_stream, _) =
            accept_unix_connection(&listener).expect("accept bridge request connection");
        request_stream
            .set_read_timeout(Some(std::time::Duration::from_secs(2)))
            .expect("set request read timeout");
        let mut request_line = String::new();
        BufReader::new(request_stream.try_clone().expect("clone request stream"))
            .read_line(&mut request_line)
            .expect("read bridge request line");
        let request_json =
            serde_json::from_str(request_line.trim_end()).expect("decode bridge request json");
        let _ = events_tx.send(BridgeServerEvent::Request(request_json));
        request_stream
            .write_all(response_line.as_bytes())
            .expect("write bridge response");
    });

    (handle, events_rx)
}

#[cfg(unix)]
fn accept_unix_connection(
    listener: &std::os::unix::net::UnixListener,
) -> std::io::Result<(
    std::os::unix::net::UnixStream,
    std::os::unix::net::SocketAddr,
)> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        match listener.accept() {
            Ok(accepted) => return Ok(accepted),
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                if std::time::Instant::now() >= deadline {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        "timed out accepting bridge test connection",
                    ));
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            Err(err) => return Err(err),
        }
    }
}

#[cfg(unix)]
fn sample_bridge_request() -> bridge::BridgeRequest {
    let mut params = serde_json::Map::new();
    params.insert("target".to_string(), serde_json::json!("did:example:bob"));
    params.insert("text".to_string(), serde_json::json!("hello over bridge"));
    bridge::BridgeRequest {
        method: "direct.send".to_string(),
        params,
        identity_name: "alice".to_string(),
    }
}

#[cfg(unix)]
fn test_resolved_with_socket(socket_path: &str) -> Resolved {
    let mut paths = test_paths();
    if let Some(parent) = std::path::Path::new(socket_path).parent() {
        paths.state_dir = parent.to_string_lossy().into_owned();
    }
    Resolved {
        runtime_mode: "websocket".to_string(),
        runtime_socket_path: socket_path.to_string(),
        paths,
        ..test_resolved()
    }
}

#[cfg(unix)]
fn assert_bridge_call_error_phase(err: &anyhow::Error, phase: &str) {
    let bridge_err = err
        .downcast_ref::<bridge::BridgeCallError>()
        .expect("error should preserve BridgeCallError details");
    assert_eq!(bridge_err.phase, phase);
}

#[derive(Debug)]
struct MemoryDuplex {
    input: std::io::Cursor<Vec<u8>>,
    output: Vec<u8>,
}

impl MemoryDuplex {
    fn new(input: Vec<u8>) -> Self {
        Self {
            input: std::io::Cursor::new(input),
            output: Vec::new(),
        }
    }

    fn output(&self) -> &[u8] {
        &self.output
    }

    fn output_string(&self) -> String {
        String::from_utf8(self.output.clone()).expect("response utf8")
    }
}

impl std::io::Read for MemoryDuplex {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        std::io::Read::read(&mut self.input, buffer)
    }
}

impl std::io::Write for MemoryDuplex {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        self.output.extend_from_slice(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
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
        Self::new_under(&std::env::temp_dir(), "awiki-cli-rs2-runtime-bridge-test")
    }

    #[cfg(unix)]
    fn new_short(prefix: &str) -> std::io::Result<Self> {
        Self::new_under(std::path::Path::new("/tmp"), prefix)
    }

    fn new_under(root: &std::path::Path, prefix: &str) -> std::io::Result<Self> {
        static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let thread_id = format!("{:?}", std::thread::current().id())
            .chars()
            .filter(|ch| ch.is_ascii_alphanumeric())
            .collect::<String>();
        let path = root.join(format!(
            "{prefix}-{}-{nonce}-{thread_id}-{counter}",
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
