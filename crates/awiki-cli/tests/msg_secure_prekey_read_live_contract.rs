mod support;

use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use support::{write_ready_identity, TestIdentity, TestIdentityOptions};

#[test]
fn msg_inbox_target_filter_is_unsupported_before_legacy_secure_prekey_side_effects() {
    let workspace = TempDir::new("msg-secure-prekey-inbox-filter").expect("workspace");
    let alice_did = "did:wba:awiki.ai:alice:e1_alice";

    let inbox = awiki_cmd(
        &[
            "--identity",
            "bob-secure-read",
            "msg",
            "inbox",
            "--scope",
            "direct",
            "--with",
            alice_did,
            "--limit",
            "5",
        ],
        workspace.path(),
    );
    assert_unsupported_capability(&inbox, "msg.inbox", "inbox-target-filters", "Phase 3");
}

#[test]
fn msg_history_live_reads_without_legacy_secure_prekey_publish_side_effect() {
    let workspace = TempDir::new("msg-secure-prekey-history").expect("workspace");
    let bob =
        register_generated_msg_identity(workspace.path(), "bob-secure-history", "bob", "jwt-bob");
    let alice_did = "did:wba:awiki.ai:alice:e1_alice";
    let message = json!({
        "id": "msg-secure-prekey-history-1",
        "type": "text",
        "sender_did": alice_did,
        "receiver_did": bob.did,
        "content_type": "text/plain",
        "content": "hello bob",
        "sent_at": "2026-04-07T01:02:03Z",
        "is_read": false,
    });
    let server = TestServer::new(vec![TestResponse::ok(&json_rpc_result(json!({
        "messages": [message],
        "total": 1,
        "source": "remote_http"
    })))]);
    write_msg_config(workspace.path(), &server.base_url());

    let history = awiki_cmd(
        &[
            "--identity",
            "bob-secure-history",
            "msg",
            "history",
            "--with",
            alice_did,
            "--limit",
            "5",
        ],
        workspace.path(),
    );
    assert_success(&history);
    let history_json = success_json(&history);
    assert_eq!(history_json["summary"], "Loaded 1 direct history messages");
    assert_eq!(
        history_json["data"]["messages"][0]["id"],
        "msg-secure-prekey-history-1"
    );
    assert!(history_json.get("warnings").is_none());

    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    let bodies = request_json_bodies(&requests);
    assert_eq!(rpc_methods(&bodies), vec!["direct.get_history"]);
    assert_eq!(bodies[0]["params"]["body"]["peer_did"], alice_did);
}

#[test]
fn msg_history_live_keeps_read_success_without_legacy_secure_prekey_warning() {
    let workspace = TempDir::new("msg-secure-prekey-history-success").expect("workspace");
    let bob =
        register_generated_msg_identity(workspace.path(), "bob-secure-success", "bob", "jwt-bob");
    let alice_did = "did:wba:awiki.ai:alice:e1_alice";
    let message = json!({
        "id": "msg-secure-prekey-success-1",
        "type": "text",
        "sender_did": alice_did,
        "receiver_did": bob.did,
        "content_type": "text/plain",
        "content": "hello after warning",
        "sent_at": "2026-04-07T01:02:03Z",
        "is_read": false,
    });
    let server = TestServer::new(vec![TestResponse::ok(&json_rpc_result(json!({
        "messages": [message],
        "total": 1,
        "source": "remote_http"
    })))]);
    write_msg_config(workspace.path(), &server.base_url());

    let history = awiki_cmd(
        &[
            "--identity",
            "bob-secure-success",
            "msg",
            "history",
            "--with",
            alice_did,
        ],
        workspace.path(),
    );
    assert_success(&history);
    let history_json = success_json(&history);
    assert_eq!(history_json["summary"], "Loaded 1 direct history messages");
    assert_eq!(
        history_json["data"]["messages"][0]["id"],
        "msg-secure-prekey-success-1"
    );
    assert!(history_json.get("warnings").is_none());

    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    let bodies = request_json_bodies(&requests);
    assert_eq!(rpc_methods(&bodies), vec!["direct.get_history"]);
}

fn register_generated_msg_identity(
    workspace: &Path,
    identity_name: &str,
    handle: &str,
    jwt_token: &str,
) -> TestIdentity {
    write_ready_identity(
        workspace,
        TestIdentityOptions {
            identity_name,
            handle,
            display_name: identity_name,
            jwt_token,
            make_default: true,
        },
    )
}

fn write_msg_config(workspace: &Path, base_url: &str) {
    std::fs::write(
        workspace.join("config.yaml"),
        format!("runtime:\n  mode: http\nservices:\n  service_base_url: {base_url}\n"),
    )
    .unwrap();
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

fn error_json(output: &Output) -> Value {
    assert!(
        !output.status.success(),
        "command should fail\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stderr).expect("error JSON")
}

fn assert_unsupported_capability(
    output: &Output,
    command: &str,
    capability: &str,
    required_phase: &str,
) {
    assert_eq!(
        output.status.code(),
        Some(2),
        "unexpected exit status; stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let envelope = error_json(output);
    assert_eq!(envelope["error"]["code"], "unsupported_capability");
    assert_eq!(envelope["error"]["details"]["command"], command);
    assert_eq!(envelope["error"]["details"]["capability"], capability);
    assert_eq!(
        envelope["error"]["details"]["required_phase"],
        required_phase
    );
}

fn request_body(raw: &str) -> &str {
    raw.split("\r\n\r\n").nth(1).unwrap_or_default()
}

fn request_json_bodies(requests: &[String]) -> Vec<Value> {
    requests
        .iter()
        .map(|request| serde_json::from_str(request_body(request)).expect("request body"))
        .collect()
}

fn rpc_methods(bodies: &[Value]) -> Vec<&str> {
    bodies
        .iter()
        .map(|body| body["method"].as_str().expect("rpc method"))
        .collect()
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
