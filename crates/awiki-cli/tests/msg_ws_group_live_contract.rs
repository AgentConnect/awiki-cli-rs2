#![cfg(unix)]

use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const IDENTITY: &str = "alice-group-ws";
const BOB_DID: &str = "did:wba:awiki.ai:bob:e1_bob";
const GROUP_DID: &str = "did:wba:awiki.ai:groups:ws:e1_group";

mod support;

use support::{
    set_secret_storage_mode, tenant_workspace, write_default_tenant_registry, write_tenant_config,
};

#[test]
fn msg_send_group_websocket_mode_uses_im_core_http_not_legacy_bridge() {
    let workspace = TempDir::new("gws-send-http-cutover").expect("workspace");
    register_ready_group_identity(workspace.path(), IDENTITY, "alice", "jwt-alice");
    let missing_socket = tenant_workspace(workspace.path())
        .join("runtime")
        .join("missing.sock");
    let server = TestServer::new(vec![TestResponse::ok(&json_rpc_result(json!({
        "accepted": true,
        "final_acceptance": true,
        "group_did": GROUP_DID,
        "message_id": "msg-http-group-send-1",
        "operation_id": "op-http-group-send-1",
        "group_event_seq": "10",
        "group_state_version": "v10",
        "accepted_at": "2026-05-16T02:03:04Z"
    })))]);
    write_group_ws_config(
        workspace.path(),
        &server.base_url(),
        missing_socket.to_str().expect("socket path"),
    );

    let output = awiki_cmd(
        &[
            "--identity",
            IDENTITY,
            "msg",
            "send",
            "--group",
            GROUP_DID,
            "--text",
            "hello group over HTTP",
            "--type",
            "text",
        ],
        workspace.path(),
    );

    assert_success(&output);
    let envelope = success_json(&output);
    assert_eq!(envelope["summary"], "Sent a group text message");
    assert_eq!(envelope["data"]["target"]["kind"], "group");
    assert_eq!(envelope["data"]["target"]["did"], GROUP_DID);
    assert_eq!(envelope["data"]["message"]["id"], format!("{GROUP_DID}:10"));
    assert_eq!(envelope["data"]["message"]["secure"], false);
    assert_eq!(
        envelope["data"]["delivery"]["message_id"],
        "msg-http-group-send-1"
    );
    assert_eq!(
        envelope["data"]["delivery"]["operation_id"],
        "op-http-group-send-1"
    );
    assert_eq!(envelope["data"]["source"], "remote_http");
    assert_no_legacy_websocket_fallback_warning(&envelope);

    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].starts_with("POST /im/rpc HTTP/1.1"));
    assert_contains_text(&requests[0], "Authorization: Bearer jwt-alice\r\n");
    let body: Value = serde_json::from_str(request_body(&requests[0])).expect("request body");
    assert_eq!(body["method"], "group.send");
    assert_eq!(body["params"]["meta"]["profile"], "anp.group.base.v2");
    assert_eq!(
        body["params"]["meta"]["target"],
        json!({"kind": "group", "did": GROUP_DID})
    );
    assert_eq!(
        body["params"]["body"],
        json!({"text": "hello group over HTTP"})
    );
}

#[test]
fn group_messages_websocket_mode_uses_im_core_http_not_legacy_bridge_or_cache() {
    let workspace = TempDir::new("gws-msgs-http-cutover").expect("workspace");
    register_ready_group_identity(workspace.path(), IDENTITY, "alice", "jwt-alice");
    let missing_socket = tenant_workspace(workspace.path())
        .join("runtime")
        .join("missing.sock");
    let server = TestServer::new(vec![TestResponse::ok(&json_rpc_result(json!({
        "messages": [{
            "id": "msg-http-group-message-1",
            "sender_did": BOB_DID,
            "group_did": GROUP_DID,
            "content_type": "text/plain",
            "content": "hello from group HTTP",
            "server_seq": 13,
            "sent_at": "2026-05-16T05:06:07Z",
            "is_read": false
        }],
        "total": 1,
        "has_more": false
    })))]);
    write_group_ws_config(
        workspace.path(),
        &server.base_url(),
        missing_socket.to_str().expect("socket path"),
    );

    let output = awiki_cmd(
        &[
            "--identity",
            IDENTITY,
            "group",
            "messages",
            "--group",
            GROUP_DID,
            "--limit",
            "7",
            "--cursor",
            "3",
        ],
        workspace.path(),
    );

    assert_success(&output);
    let envelope = success_json(&output);
    assert_eq!(envelope["summary"], "Loaded 1 group messages");
    assert_eq!(envelope["data"]["group"], GROUP_DID);
    assert_eq!(envelope["data"]["source"], "remote_http");
    assert_eq!(
        envelope["data"]["messages"][0]["msg_id"],
        "msg-http-group-message-1"
    );
    assert_eq!(
        envelope["data"]["messages"][0]["content"],
        "hello from group HTTP"
    );
    assert_has_group_http_transport_warning(&envelope);

    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].starts_with("POST /im/rpc HTTP/1.1"));
    assert_contains_text(&requests[0], "Authorization: Bearer jwt-alice\r\n");
    let body: Value = serde_json::from_str(request_body(&requests[0])).expect("request body");
    assert_eq!(body["method"], "group.list_messages");
    assert_eq!(body["params"]["meta"]["profile"], "anp.group.local.v1");
    assert_eq!(
        body["params"]["meta"]["target"],
        json!({"kind": "group", "did": GROUP_DID})
    );
    assert_eq!(body["params"]["body"]["group_did"], GROUP_DID);
    assert_eq!(body["params"]["body"]["limit"], 7);
    assert_eq!(body["params"]["body"]["since_seq"], "3");
}

#[test]
fn msg_send_group_websocket_mode_reports_http_transport_failure_without_bridge_fallback() {
    let workspace = TempDir::new("gws-send-http-failure").expect("workspace");
    register_ready_group_identity(workspace.path(), IDENTITY, "alice", "jwt-alice");
    let missing_socket = tenant_workspace(workspace.path())
        .join("runtime")
        .join("missing.sock");
    write_group_ws_config(
        workspace.path(),
        &closed_local_url(),
        missing_socket.to_str().expect("socket path"),
    );

    let output = awiki_cmd(
        &[
            "--identity",
            IDENTITY,
            "msg",
            "send",
            "--group",
            GROUP_DID,
            "--text",
            "http failure should not try bridge fallback",
        ],
        workspace.path(),
    );

    assert_transport_unavailable_without_bridge_fallback(&output);
}

#[test]
fn group_messages_websocket_mode_reports_http_transport_failure_without_cache_fallback() {
    let workspace = TempDir::new("gws-msgs-http-failure").expect("workspace");
    register_ready_group_identity(workspace.path(), IDENTITY, "alice", "jwt-alice");
    let missing_socket = tenant_workspace(workspace.path())
        .join("runtime")
        .join("missing.sock");
    write_group_ws_config(
        workspace.path(),
        &closed_local_url(),
        missing_socket.to_str().expect("socket path"),
    );

    let output = awiki_cmd(
        &[
            "--identity",
            IDENTITY,
            "group",
            "messages",
            "--group",
            GROUP_DID,
        ],
        workspace.path(),
    );

    assert_transport_unavailable_without_bridge_fallback(&output);
}

fn register_ready_group_identity(
    workspace: &Path,
    identity_name: &str,
    handle: &str,
    jwt_token: &str,
) {
    write_default_tenant_registry(workspace, "https://awiki.ai", "awiki.ai");
    set_secret_storage_mode(workspace, "file_compat");
    let create = awiki_cmd(
        &[
            "--migration",
            "id",
            "create",
            "--name",
            "Group WS User",
            "--identity",
            identity_name,
        ],
        workspace,
    );
    assert_success(&create);

    let tenant = tenant_workspace(workspace);
    let index_path = tenant.join("identities").join("index.json");
    let mut index: Value = serde_json::from_slice(&std::fs::read(&index_path).unwrap()).unwrap();
    let did = format!("did:wba:awiki.ai:{handle}:e1_{handle}");
    index["credentials"][identity_name]["did"] = json!(did);
    index["credentials"][identity_name]["handle"] = json!(handle);
    index["credentials"][identity_name]["full_handle"] = json!(format!("{handle}.awiki.ai"));
    index["credentials"][identity_name]["user_id"] = json!(format!("user-{handle}"));
    std::fs::write(&index_path, serde_json::to_vec_pretty(&index).unwrap()).unwrap();

    let dir_name = index["credentials"][identity_name]["dir_name"]
        .as_str()
        .unwrap();
    let identity_dir = tenant.join("identities").join(dir_name);
    let identity_path = identity_dir.join("identity.json");
    let mut identity: Value =
        serde_json::from_slice(&std::fs::read(&identity_path).unwrap()).unwrap();
    let original_did = identity["did"].as_str().unwrap().to_string();
    identity["did"] = json!(did);
    identity["handle"] = json!(handle);
    identity["full_handle"] = json!(format!("{handle}.awiki.ai"));
    identity["user_id"] = json!(format!("user-{handle}"));
    std::fs::write(
        &identity_path,
        serde_json::to_vec_pretty(&identity).unwrap(),
    )
    .unwrap();

    let document_path = identity_dir.join("did_document.json");
    let mut document: Value =
        serde_json::from_slice(&std::fs::read(&document_path).unwrap()).unwrap();
    document["id"] = json!(original_did);
    std::fs::write(
        &document_path,
        serde_json::to_vec_pretty(&document).unwrap(),
    )
    .unwrap();

    std::fs::write(
        identity_dir.join("auth.json"),
        serde_json::to_vec_pretty(&json!({ "jwt_token": jwt_token })).unwrap(),
    )
    .unwrap();

    set_secret_storage_mode(workspace, "vault_required");
    let migrate = awiki_cmd(&["--migration", "id", "vault", "migrate"], workspace);
    assert_success(&migrate);
}

fn write_group_ws_config(workspace: &Path, base_url: &str, socket_path: &str) {
    write_default_tenant_registry(workspace, base_url, "awiki.ai");
    write_tenant_config(
        workspace,
        format!("runtime:\n  mode: websocket\n  socket_path: {socket_path}\n").as_str(),
    );
}

fn closed_local_url() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind closed local url");
    let address = listener.local_addr().expect("local addr");
    drop(listener);
    format!("http://{address}")
}

fn awiki_cmd(args: &[&str], workspace: &Path) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_awiki-cli"));
    command
        .args(args)
        .env("AWIKI_CLI_WORKSPACE_HOME_DIR", workspace)
        .env("HOME", workspace.join("home"))
        .env("USERPROFILE", workspace.join("home"))
        .env("AWIKI_CLI_UPDATE_CACHE_ONLY", "1")
        .env_remove("AWIKI_WORKSPACE")
        .env_remove("AWIKI_WORKSPACE_HOME")
        .env_remove("AWIKI_HOME")
        .env_remove("AVIKI_WORKSPACE_HOME")
        .env_remove("AWIKI_FORMAT")
        .env_remove("AVIKI_FORMAT");
    command.output().expect("run awiki-cli binary")
}

fn assert_success(output: &Output) {
    assert_eq!(
        output.status.code(),
        Some(0),
        "unexpected exit status; stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn success_json(output: &Output) -> Value {
    assert!(
        output.stderr.is_empty(),
        "stderr should be empty: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let envelope: Value =
        serde_json::from_slice(&output.stdout).expect("stdout should be a JSON success envelope");
    assert_eq!(envelope["ok"], true);
    envelope
}

fn failure_json(output: &Output) -> Value {
    assert!(
        !output.status.success(),
        "expected failure; stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stderr).expect("stderr should be a JSON error envelope")
}

fn assert_transport_unavailable_without_bridge_fallback(output: &Output) {
    assert_eq!(
        output.status.code(),
        Some(1),
        "unexpected exit status; stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let envelope = failure_json(output);
    assert_eq!(envelope["error"]["code"], "transport_unavailable");
    let message = envelope["error"]["message"].as_str().expect("message");
    assert_contains_text(message, "message transport is unavailable");
    assert!(
        !message.contains("local websocket bridge request failed"),
        "legacy bridge fallback should not be used, got: {message}"
    );
    assert!(
        !message.contains("loaded data from local cache"),
        "legacy cache fallback should not be used, got: {message}"
    );
}

fn assert_no_legacy_websocket_fallback_warning(envelope: &Value) {
    let warnings = envelope["warnings"].as_array().cloned().unwrap_or_default();
    assert!(
        warnings.iter().all(|warning| {
            let warning = warning.as_str().unwrap_or_default();
            !warning.contains("used HTTP fallback")
                && !warning.contains("loaded data from local cache")
        }),
        "unexpected legacy websocket fallback warning: {warnings:?}"
    );
}

fn assert_has_group_http_transport_warning(envelope: &Value) {
    let warnings = envelope["warnings"].as_array().cloned().unwrap_or_default();
    assert!(
        warnings.iter().any(|warning| warning
            .as_str()
            .unwrap_or_default()
            .contains("Group lifecycle commands use HTTP transport")),
        "expected group HTTP transport warning, got: {warnings:?}"
    );
    assert_no_legacy_websocket_fallback_warning(envelope);
}

fn request_body(raw: &str) -> &str {
    raw.split("\r\n\r\n").nth(1).unwrap_or_default()
}

fn assert_contains_text(haystack: &str, needle: &str) {
    let header_probe = needle.strip_suffix("\r\n").unwrap_or(needle);
    if let Some((header_name, expected_value)) = header_probe.split_once(':') {
        let header_name = header_name.trim();
        let expected_value = expected_value.trim();
        if !header_name.is_empty()
            && haystack.lines().any(|line| {
                line.split_once(':').is_some_and(|(name, value)| {
                    name.trim().eq_ignore_ascii_case(header_name)
                        && (expected_value.is_empty() || value.trim() == expected_value)
                })
            })
        {
            return;
        }
    }
    assert!(
        haystack.contains(needle),
        "expected text to contain {needle:?}, got:\n{haystack}"
    );
}

fn json_rpc_result(result: Value) -> String {
    json!({
        "jsonrpc": "2.0",
        "result": result,
        "id": "req-1",
    })
    .to_string()
}

#[derive(Debug, Clone)]
struct TestResponse {
    status: u16,
    body: String,
}

impl TestResponse {
    fn ok(body: &str) -> Self {
        Self {
            status: 200,
            body: body.to_string(),
        }
    }
}

struct TestServer {
    address: String,
    requests: Arc<Mutex<Vec<String>>>,
    join: Option<thread::JoinHandle<()>>,
}

impl TestServer {
    fn new(responses: Vec<TestResponse>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
        listener
            .set_nonblocking(true)
            .expect("set test server nonblocking");
        let address = format!("http://{}", listener.local_addr().expect("local addr"));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let server_requests = Arc::clone(&requests);
        let join = thread::spawn(move || {
            for response in responses {
                let stream = accept_with_timeout(&listener);
                let Some(stream) = stream else {
                    break;
                };
                handle_connection(stream, &server_requests, response);
            }
        });
        Self {
            address,
            requests,
            join: Some(join),
        }
    }

    fn base_url(&self) -> String {
        self.address.clone()
    }

    fn requests(&self) -> Vec<String> {
        self.requests.lock().expect("requests mutex").clone()
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

fn accept_with_timeout(listener: &TcpListener) -> Option<TcpStream> {
    // Parallel workspace tests can delay the debug CLI process startup on macOS.
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                stream
                    .set_nonblocking(false)
                    .expect("set test stream blocking");
                return Some(stream);
            }
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                if std::time::Instant::now() >= deadline {
                    return None;
                }
                thread::sleep(Duration::from_millis(10));
            }
            Err(_) => return None,
        }
    }
}

fn handle_connection(
    mut stream: TcpStream,
    requests: &Arc<Mutex<Vec<String>>>,
    response: TestResponse,
) {
    let request = read_http_request(&mut stream);
    requests.lock().expect("requests mutex").push(request);
    let body = response.body.as_bytes();
    let raw = format!(
        "HTTP/1.1 {} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        response.status,
        body.len(),
        response.body
    );
    stream.write_all(raw.as_bytes()).expect("write response");
}

fn read_http_request(stream: &mut TcpStream) -> String {
    let mut raw = Vec::new();
    let mut buf = [0_u8; 512];
    loop {
        let count = stream.read(&mut buf).expect("read request");
        if count == 0 {
            break;
        }
        raw.extend_from_slice(&buf[..count]);
        if let Some(header_end) = find_header_end(&raw) {
            let headers = String::from_utf8_lossy(&raw[..header_end]).to_string();
            let content_length = headers
                .lines()
                .find_map(|line| {
                    line.split_once(':').and_then(|(name, value)| {
                        name.trim()
                            .eq_ignore_ascii_case("content-length")
                            .then_some(value)
                    })
                })
                .and_then(|value| value.trim().parse::<usize>().ok())
                .unwrap_or_default();
            let expected = header_end + content_length;
            while raw.len() < expected {
                let count = stream.read(&mut buf).expect("read request body");
                if count == 0 {
                    break;
                }
                raw.extend_from_slice(&buf[..count]);
            }
            break;
        }
    }
    String::from_utf8_lossy(&raw).into_owned()
}

fn find_header_end(raw: &[u8]) -> Option<usize> {
    raw.windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|index| index + 4)
}

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(prefix: &str) -> std::io::Result<Self> {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "awiki-cli-rs2-{prefix}-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path)?;
        Ok(Self { path })
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
