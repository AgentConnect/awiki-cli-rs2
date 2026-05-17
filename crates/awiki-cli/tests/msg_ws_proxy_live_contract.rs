#![cfg(unix)]

use awiki_cli::runtime::bridge::{self, BridgeRequest};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[test]
fn msg_send_direct_websocket_mode_uses_local_bridge_like_go() {
    let workspace = TempDir::new("msg-ws-proxy-direct-send").expect("workspace");
    register_ready_msg_identity(workspace.path(), "alice-ws", "alice", "jwt-alice");
    let target_did = "did:wba:awiki.ai:bob:e1_bob";
    let socket_path = workspace.path().join("runtime").join("message-daemon.sock");
    let (_bridge, bridge_requests) = spawn_bridge_server(
        &socket_path,
        json!({
            "accepted": true,
            "message_id": "msg-bridge-direct-1",
            "operation_id": "op-bridge-direct-1",
            "target_did": target_did,
            "accepted_at": "2026-05-16T01:02:03Z",
            "delivery_state": "accepted",
            "source": "local_ws_cache"
        }),
    );
    write_msg_ws_config(
        workspace.path(),
        "https://placeholder.invalid",
        socket_path.to_str().expect("socket path"),
    );

    let output = awiki_cmd(
        &[
            "--identity",
            "alice-ws",
            "msg",
            "send",
            "--to",
            target_did,
            "--text",
            "hello over local bridge",
            "--type",
            "text",
        ],
        workspace.path(),
    );

    assert_success(&output);
    let envelope = success_json(&output);
    assert_eq!(envelope["summary"], "Sent a direct text message");
    assert_eq!(envelope["data"]["action"], "send_message");
    assert_eq!(envelope["data"]["target"]["did"], target_did);
    assert_eq!(envelope["data"]["message"]["id"], "msg-bridge-direct-1");
    assert_eq!(envelope["data"]["message"]["type"], "text");
    assert_eq!(envelope["data"]["message"]["secure"], false);
    assert_eq!(
        envelope["data"]["delivery"]["message_id"],
        "msg-bridge-direct-1"
    );
    assert_eq!(
        envelope["data"]["delivery"]["operation_id"],
        "op-bridge-direct-1"
    );
    assert!(envelope["data"].get("source").is_none());
    assert_no_websocket_http_fallback_warning(&envelope);

    let request = bridge_requests
        .recv_timeout(Duration::from_secs(2))
        .expect("bridge direct.send request");
    assert_eq!(request.method, "direct.send");
    assert_eq!(request.identity_name, "alice-ws");
    assert_eq!(request.params["target"], target_did);
    assert_eq!(request.params["text"], "hello over local bridge");
    assert_eq!(request.params["type"], "text");
}

#[test]
fn msg_send_direct_websocket_mode_falls_back_to_http_with_warning_like_go() {
    let workspace = TempDir::new("msg-ws-proxy-direct-send-fallback").expect("workspace");
    register_ready_msg_identity(workspace.path(), "alice-ws-fallback", "alice", "jwt-alice");
    let target_did = "did:wba:awiki.ai:bob:e1_bob";
    let missing_socket = workspace.path().join("runtime").join("missing.sock");
    let server = TestServer::new(vec![TestResponse::ok(&json_rpc_result(json!({
        "accepted": true,
        "final_acceptance": true,
        "message_id": "msg-http-direct-1",
        "operation_id": "op-http-direct-1",
        "target_did": target_did,
        "accepted_at": "2026-05-16T02:03:04Z",
        "delivery_state": "accepted",
        "source": "remote_http"
    })))]);
    write_msg_ws_config(
        workspace.path(),
        &server.base_url(),
        missing_socket.to_str().expect("socket path"),
    );

    let output = awiki_cmd(
        &[
            "--identity",
            "alice-ws-fallback",
            "msg",
            "send",
            "--to",
            target_did,
            "--text",
            "hello over HTTP fallback",
            "--type",
            "text",
        ],
        workspace.path(),
    );

    assert_success(&output);
    let envelope = success_json(&output);
    assert_eq!(envelope["summary"], "Sent a direct text message");
    assert_eq!(envelope["data"]["action"], "send_message");
    assert_eq!(envelope["data"]["target"]["did"], target_did);
    assert_eq!(envelope["data"]["message"]["id"], "msg-http-direct-1");
    assert_eq!(envelope["data"]["message"]["type"], "text");
    assert_eq!(envelope["data"]["message"]["secure"], false);
    assert_eq!(
        envelope["data"]["delivery"]["message_id"],
        "msg-http-direct-1"
    );
    assert_eq!(
        envelope["data"]["delivery"]["operation_id"],
        "op-http-direct-1"
    );
    assert!(envelope["data"].get("source").is_none());
    assert_websocket_http_fallback_warning_contains(
        &envelope,
        "local websocket bridge unavailable",
    );

    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].starts_with("POST /im/rpc HTTP/1.1"));
    assert_contains_text(&requests[0], "Authorization: Bearer jwt-alice\r\n");
    let body: Value = serde_json::from_str(request_body(&requests[0])).expect("request body");
    assert_eq!(body["method"], "direct.send");
    assert_eq!(body["params"]["meta"]["profile"], "anp.direct.base.v1");
    assert_eq!(
        body["params"]["meta"]["target"],
        json!({"kind": "agent", "did": target_did})
    );
    assert_eq!(
        body["params"]["body"],
        json!({"text": "hello over HTTP fallback"})
    );
    assert_eq!(
        body["params"]["auth"]["scheme"],
        "anp-rfc9421-origin-proof-v1"
    );
}

fn spawn_bridge_server(
    socket_path: &Path,
    result: Value,
) -> (thread::JoinHandle<()>, mpsc::Receiver<BridgeRequest>) {
    let listener =
        bridge::listen_bridge(socket_path.to_str().expect("socket path")).expect("listen bridge");
    listener
        .set_nonblocking(true)
        .expect("set bridge listener nonblocking");
    let (requests_tx, requests_rx) = mpsc::channel();
    let response_line = json!({ "ok": true, "result": result }).to_string() + "\n";

    let handle = thread::spawn(move || loop {
        let Ok((mut conn, _)) = accept_unix_connection(&listener) else {
            return;
        };
        conn.set_read_timeout(Some(Duration::from_secs(2)))
            .expect("set bridge read timeout");
        let mut request_line = String::new();
        let Ok(read) = BufReader::new(conn.try_clone().expect("clone bridge client"))
            .read_line(&mut request_line)
        else {
            return;
        };
        if read == 0 || request_line.trim().is_empty() {
            continue;
        }

        let request: BridgeRequest =
            serde_json::from_str(request_line.trim_end()).expect("decode bridge request");
        requests_tx.send(request).expect("send bridge request");
        conn.write_all(response_line.as_bytes())
            .expect("write bridge response");
        break;
    });

    (handle, requests_rx)
}

fn accept_unix_connection(
    listener: &std::os::unix::net::UnixListener,
) -> std::io::Result<(
    std::os::unix::net::UnixStream,
    std::os::unix::net::SocketAddr,
)> {
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
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
                thread::sleep(Duration::from_millis(10));
            }
            Err(err) => return Err(err),
        }
    }
}

fn register_ready_msg_identity(
    workspace: &Path,
    identity_name: &str,
    handle: &str,
    jwt_token: &str,
) {
    let create = awiki_cmd(
        &[
            "id",
            "create",
            "--name",
            "Message User",
            "--identity",
            identity_name,
        ],
        workspace,
    );
    assert_success(&create);

    let index_path = workspace.join("identities").join("index.json");
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
    let identity_dir = workspace.join("identities").join(dir_name);
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
}

fn write_msg_ws_config(workspace: &Path, base_url: &str, socket_path: &str) {
    std::fs::write(
        workspace.join("config.yaml"),
        format!(
            "runtime:\n  mode: websocket\n  socket_path: {socket_path}\nservices:\n  service_base_url: {base_url}\n"
        ),
    )
    .unwrap();
}

fn awiki_cmd(args: &[&str], workspace: &Path) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_awiki-cli"));
    command
        .args(args)
        .env("AWIKI_CLI_WORKSPACE_HOME_DIR", workspace)
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

fn assert_no_websocket_http_fallback_warning(envelope: &Value) {
    let warnings = envelope["warnings"].as_array().cloned().unwrap_or_default();
    assert!(
        warnings.is_empty()
            || warnings.iter().all(|warning| !warning
                .as_str()
                .unwrap_or_default()
                .contains("used HTTP fallback")),
        "direct send should not include websocket HTTP fallback warning: {warnings:?}"
    );
}

fn assert_websocket_http_fallback_warning_contains(envelope: &Value, detail: &str) {
    let warnings = envelope["warnings"].as_array().cloned().unwrap_or_default();
    assert!(
        warnings.iter().any(|warning| {
            let warning = warning.as_str().unwrap_or_default();
            warning.contains("WebSocket listener was unavailable for this identity; used HTTP fallback. Details:")
                && warning.contains(detail)
        }),
        "direct send should include websocket HTTP fallback warning with {detail:?}: {warnings:?}"
    );
}

fn request_body(raw: &str) -> &str {
    raw.split("\r\n\r\n").nth(1).unwrap_or_default()
}

fn assert_contains_text(haystack: &str, needle: &str) {
    assert!(
        haystack.contains(needle),
        "expected request to contain {needle:?}, got:\n{haystack}"
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
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        match listener.accept() {
            Ok((stream, _)) => return Some(stream),
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
                .find_map(|line| line.strip_prefix("Content-Length: "))
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
