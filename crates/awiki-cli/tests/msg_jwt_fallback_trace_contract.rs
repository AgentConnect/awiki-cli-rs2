use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[test]
fn direct_send_http_401_refreshes_with_fallback_trace_like_go() {
    let workspace = TempDir::new("msg-jwt-fallback-send").expect("workspace");
    register_ready_msg_identity(workspace.path(), "alice-msg-fallback", "alice", "jwt-stale");
    let bob_did = "did:wba:awiki.ai:bob:e1_bob";
    let server = TestServer::new(vec![
        TestResponse::ok(&json_rpc_error(1401, "expired jwt")),
        TestResponse::ok(&json_rpc_result(json!({
            "access_token": "jwt-refreshed"
        }))),
        TestResponse::ok(&json_rpc_result(json!({
            "accepted": true,
            "final_acceptance": true,
            "message_id": "msg-fallback-refresh-1",
            "operation_id": "op-fallback-refresh-1",
            "target_did": bob_did,
            "accepted_at": "2026-05-17T01:02:03Z",
            "delivery_state": "accepted"
        }))),
    ]);
    write_msg_config(workspace.path(), &server.base_url());

    let output = awiki_trace_cmd(
        &[
            "--identity",
            "alice-msg-fallback",
            "msg",
            "send",
            "--to",
            bob_did,
            "--text",
            "hello after fallback refresh",
        ],
        workspace.path(),
    );

    assert_success(&output);
    let envelope = success_json_with_stderr(&output);
    assert_eq!(envelope["summary"], "Sent a direct text message");
    assert_eq!(envelope["data"]["message"]["id"], "msg-fallback-refresh-1");

    let trace = stderr_text(&output);
    assert_text_contains(&trace, "JWT 续期 / 消息回退时刷新 JWT");
    assert_text_contains(&trace, "远端 RPC / direct send");

    let requests = server.requests();
    assert_eq!(requests.len(), 3);
    assert!(requests[0].starts_with("POST /im/rpc HTTP/1.1"));
    assert_contains_text(&requests[0], "Authorization: Bearer jwt-stale\r\n");
    assert!(requests[1].starts_with("POST /user-service/did-auth/rpc HTTP/1.1"));
    assert_eq!(json_body(&requests[1])["method"], "get_me");
    assert!(requests[2].starts_with("POST /im/rpc HTTP/1.1"));
    assert_contains_text(&requests[2], "Authorization: Bearer jwt-refreshed\r\n");
    assert_eq!(json_body(&requests[2])["method"], "direct.send");

    let auth_path = identity_auth_path(workspace.path(), "alice-msg-fallback");
    let auth: Value =
        serde_json::from_slice(&std::fs::read(auth_path).expect("read auth")).expect("auth json");
    assert_eq!(auth["jwt_token"], "jwt-refreshed");
}

#[test]
fn inbox_http_1401_refreshes_with_fallback_trace_like_go() {
    let workspace = TempDir::new("msg-jwt-fallback-inbox").expect("workspace");
    register_ready_msg_identity(workspace.path(), "bob-msg-fallback", "bob", "jwt-bob-stale");
    let bob_did = "did:wba:awiki.ai:bob:e1_bob";
    let alice_did = "did:wba:awiki.ai:alice:e1_alice";
    let server = TestServer::new(vec![
        TestResponse::ok(&json_rpc_error(1401, "expired inbox jwt")),
        TestResponse::ok(&json_rpc_result(json!({
            "access_token": "jwt-bob-fresh"
        }))),
        TestResponse::ok(&json_rpc_result(json!({
            "messages": [{
                "id": "msg-fallback-inbox-1",
                "type": "text",
                "sender_did": alice_did,
                "receiver_did": bob_did,
                "content_type": "text/plain",
                "content": "hello through refreshed inbox fallback",
                "sent_at": "2026-05-17T02:03:04Z",
                "is_read": false
            }],
            "total": 1,
            "source": "remote_http"
        }))),
    ]);
    write_msg_config(workspace.path(), &server.base_url());

    let output = awiki_trace_cmd(
        &[
            "--identity",
            "bob-msg-fallback",
            "msg",
            "inbox",
            "--scope",
            "direct",
            "--limit",
            "3",
        ],
        workspace.path(),
    );

    assert_success(&output);
    let envelope = success_json_with_stderr(&output);
    assert_eq!(envelope["summary"], "Loaded 1 inbox messages");
    assert_eq!(envelope["data"]["source"], "remote_http");
    assert_eq!(
        envelope["data"]["messages"][0]["id"],
        "msg-fallback-inbox-1"
    );
    assert_eq!(envelope["data"]["with"], "");

    let warnings = envelope["warnings"].as_array().cloned().unwrap_or_default();
    assert!(
        warnings.is_empty(),
        "HTTP inbox refresh should not emit websocket fallback warnings: {warnings:?}"
    );

    let trace = stderr_text(&output);
    assert_text_contains(&trace, "JWT 续期 / 消息回退时刷新 JWT");
    assert_text_contains(&trace, "远端 RPC / inbox get");

    let requests = server.requests();
    assert_eq!(requests.len(), 3);
    assert_eq!(json_body(&requests[0])["method"], "inbox.get");
    assert_contains_text(&requests[0], "Authorization: Bearer jwt-bob-stale\r\n");
    assert!(requests[1].starts_with("POST /user-service/did-auth/rpc HTTP/1.1"));
    assert_eq!(json_body(&requests[1])["method"], "get_me");
    assert_eq!(json_body(&requests[2])["method"], "inbox.get");
    assert_contains_text(&requests[2], "Authorization: Bearer jwt-bob-fresh\r\n");

    let auth_path = identity_auth_path(workspace.path(), "bob-msg-fallback");
    let auth: Value =
        serde_json::from_slice(&std::fs::read(auth_path).expect("read auth")).expect("auth json");
    assert_eq!(auth["jwt_token"], "jwt-bob-fresh");
}

fn register_ready_msg_identity(
    workspace: &Path,
    identity_name: &str,
    handle: &str,
    jwt_token: &str,
) {
    let create = awiki_cmd(
        &[
            "--migration",
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

fn identity_auth_path(workspace: &Path, identity_name: &str) -> PathBuf {
    let index_path = workspace.join("identities").join("index.json");
    let index: Value = serde_json::from_slice(&std::fs::read(index_path).unwrap()).unwrap();
    let dir_name = index["credentials"][identity_name]["dir_name"]
        .as_str()
        .unwrap();
    workspace
        .join("identities")
        .join(dir_name)
        .join("auth.json")
}

fn write_msg_config(workspace: &Path, base_url: &str) {
    std::fs::write(
        workspace.join("config.yaml"),
        format!("runtime:\n  mode: http\nservices:\n  service_base_url: {base_url}\n"),
    )
    .unwrap();
}

fn awiki_cmd(args: &[&str], workspace: &Path) -> Output {
    awiki_cmd_owned(
        &args
            .iter()
            .map(|arg| (*arg).to_string())
            .collect::<Vec<_>>(),
        workspace,
    )
}

fn awiki_trace_cmd(args: &[&str], workspace: &Path) -> Output {
    let args = args
        .iter()
        .map(|arg| (*arg).to_string())
        .collect::<Vec<_>>();
    awiki_trace_cmd_owned(&args, workspace)
}

fn awiki_cmd_owned(args: &[String], workspace: &Path) -> Output {
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

fn awiki_trace_cmd_owned(args: &[String], workspace: &Path) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_awiki-cli"));
    command
        .args(args)
        .env("AWIKI_CLI_WORKSPACE_HOME_DIR", workspace)
        .env("AWIKI_CLI_UPDATE_CACHE_ONLY", "1")
        .env("AWIKI_CLI_TRACE_TIMING", "1")
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

fn success_json_with_stderr(output: &Output) -> Value {
    let envelope: Value =
        serde_json::from_slice(&output.stdout).expect("stdout should be a JSON success envelope");
    assert_eq!(envelope["ok"], true);
    envelope
}

fn stderr_text(output: &Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(!stderr.is_empty(), "stderr should contain trace output");
    stderr
}

fn request_body(raw: &str) -> &str {
    raw.split("\r\n\r\n").nth(1).unwrap_or_default()
}

fn json_body(raw: &str) -> Value {
    serde_json::from_str(request_body(raw)).expect("request body json")
}

fn assert_text_contains(haystack: &str, needle: &str) {
    assert!(
        haystack.contains(needle),
        "expected text to contain {needle:?}, got:\n{haystack}"
    );
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

fn json_rpc_error(code: i64, message: &str) -> String {
    json!({
        "jsonrpc": "2.0",
        "error": {
            "code": code,
            "message": message,
        },
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
