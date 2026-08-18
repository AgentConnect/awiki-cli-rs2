#![cfg(unix)]

use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

mod support;

use support::{
    open_local_state, set_secret_storage_mode, tenant_workspace, write_default_tenant_registry,
    write_tenant_config,
};

#[test]
fn msg_mark_read_websocket_mode_uses_im_core_http_not_legacy_bridge() {
    let workspace = TempDir::new("msg-ws-mark-read-http-cutover").expect("workspace");
    let bob_identity_id =
        register_ready_msg_identity(workspace.path(), "bob-ws-mark-http", "bob", "jwt-bob");
    let bob_did = "did:wba:awiki.ai:bob:e1_bob";
    let alice_did = "did:wba:awiki.ai:alice:e1_alice";
    seed_direct_message(
        workspace.path(),
        &bob_identity_id,
        bob_did,
        alice_did,
        "msg-ws-read-http",
        "hello over HTTP cutover",
        "2026-05-16T01:02:03Z",
    );
    let missing_socket = tenant_workspace(workspace.path())
        .join("runtime")
        .join("missing.sock");
    let server = TestServer::new(vec![TestResponse::ok(&json_rpc_result(json!({
        "updated_count": 1
    })))]);
    write_msg_ws_config(
        workspace.path(),
        &server.base_url(),
        missing_socket.to_str().expect("socket path"),
    );

    let output = awiki_cmd(
        &[
            "--identity",
            "bob-ws-mark-http",
            "msg",
            "mark-read",
            "msg-ws-read-http",
        ],
        workspace.path(),
    );

    assert_success(&output);
    let envelope = success_json(&output);
    assert_eq!(envelope["summary"], "Marked 1 messages as read");
    assert_eq!(envelope["data"]["action"], "mark_read");
    assert_eq!(envelope["data"]["updated_count"], 1);
    assert_eq!(envelope["data"]["message_ids"], json!(["msg-ws-read-http"]));
    assert_no_legacy_websocket_fallback_warning(&envelope);
    assert_eq!(
        is_read(workspace.path(), &bob_identity_id, "msg-ws-read-http"),
        1
    );

    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].starts_with("POST /im/rpc HTTP/1.1"));
    assert_contains_text(&requests[0], "Authorization: Bearer jwt-bob\r\n");
    let body: Value = serde_json::from_str(request_body(&requests[0])).expect("request body");
    assert_eq!(body["method"], "inbox.mark_read");
    assert_eq!(body["params"]["meta"]["sender_did"], bob_did);
    assert_eq!(body["params"]["body"]["user_did"], bob_did);
    assert_eq!(
        body["params"]["body"]["message_ids"],
        json!(["msg-ws-read-http"])
    );
}

#[test]
fn msg_mark_read_websocket_mode_reports_http_failure_without_bridge_fallback() {
    let workspace = TempDir::new("msg-ws-mark-read-http-failure").expect("workspace");
    let bob_identity_id =
        register_ready_msg_identity(workspace.path(), "bob-ws-mark-fail", "bob", "jwt-bob");
    let bob_did = "did:wba:awiki.ai:bob:e1_bob";
    let alice_did = "did:wba:awiki.ai:alice:e1_alice";
    seed_direct_message(
        workspace.path(),
        &bob_identity_id,
        bob_did,
        alice_did,
        "msg-ws-read-fail",
        "still unread after transport failure",
        "2026-05-16T02:03:04Z",
    );
    let missing_socket = tenant_workspace(workspace.path())
        .join("runtime")
        .join("missing.sock");
    let server = TestServer::new(vec![TestResponse::internal_error(
        r#"{"jsonrpc":"2.0","error":{"code":-32603,"message":"http mark-read failed"},"id":"req-1"}"#,
    )]);
    write_msg_ws_config(
        workspace.path(),
        &server.base_url(),
        missing_socket.to_str().expect("socket path"),
    );

    let output = awiki_cmd(
        &[
            "--identity",
            "bob-ws-mark-fail",
            "msg",
            "mark-read",
            "msg-ws-read-fail",
        ],
        workspace.path(),
    );

    assert_failure(&output);
    let envelope = failure_json(&output);
    let message = envelope["error"]["message"]
        .as_str()
        .expect("error message");
    assert_contains_text(message, "message operation: remote service request failed.");
    assert!(
        !message.contains("local websocket bridge request failed"),
        "legacy bridge fallback should not be used, got: {message}"
    );
    assert_eq!(
        is_read(workspace.path(), &bob_identity_id, "msg-ws-read-fail"),
        0
    );

    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    let body: Value = serde_json::from_str(request_body(&requests[0])).expect("request body");
    assert_eq!(body["method"], "inbox.mark_read");
}

#[test]
fn msg_mark_read_websocket_mode_reports_transport_unavailable_without_cache_fallback() {
    let workspace = TempDir::new("msg-ws-mark-read-transport-unavailable").expect("workspace");
    let bob_identity_id = register_ready_msg_identity(
        workspace.path(),
        "bob-ws-mark-unavailable",
        "bob",
        "jwt-bob",
    );
    let bob_did = "did:wba:awiki.ai:bob:e1_bob";
    let alice_did = "did:wba:awiki.ai:alice:e1_alice";
    seed_direct_message(
        workspace.path(),
        &bob_identity_id,
        bob_did,
        alice_did,
        "msg-ws-read-unavailable",
        "still unread after unavailable transport",
        "2026-05-16T03:04:05Z",
    );
    let missing_socket = tenant_workspace(workspace.path())
        .join("runtime")
        .join("missing.sock");
    write_msg_ws_config(
        workspace.path(),
        &closed_local_url(),
        missing_socket.to_str().expect("socket path"),
    );

    let output = awiki_cmd(
        &[
            "--identity",
            "bob-ws-mark-unavailable",
            "msg",
            "mark-read",
            "msg-ws-read-unavailable",
        ],
        workspace.path(),
    );

    assert_transport_unavailable_without_legacy_fallback(&output);
    assert_eq!(
        is_read(
            workspace.path(),
            &bob_identity_id,
            "msg-ws-read-unavailable"
        ),
        0
    );
}

#[test]
fn msg_mark_read_websocket_mode_keeps_group_and_mail_rows_local_only() {
    let workspace = TempDir::new("msg-ws-mark-read-local-only").expect("workspace");
    let bob_identity_id =
        register_ready_msg_identity(workspace.path(), "bob-ws-mark-local", "bob", "jwt-bob");
    let bob_did = "did:wba:awiki.ai:bob:e1_bob";
    seed_group_message(
        workspace.path(),
        &bob_identity_id,
        bob_did,
        "did:wba:awiki.ai:groups:demo:e1_group",
        "msg-group-read-local",
        "group hello",
        "2026-05-16T04:05:06Z",
    );
    seed_mail_notification(
        workspace.path(),
        &bob_identity_id,
        bob_did,
        "msg-mail-read-local",
        "mail hello",
        "2026-05-16T04:06:07Z",
    );
    let missing_socket = tenant_workspace(workspace.path())
        .join("runtime")
        .join("missing.sock");
    let server = TestServer::new(Vec::new());
    write_msg_ws_config(
        workspace.path(),
        &server.base_url(),
        missing_socket.to_str().expect("socket path"),
    );

    let output = awiki_cmd(
        &[
            "--identity",
            "bob-ws-mark-local",
            "msg",
            "mark-read",
            "msg-group-read-local",
            "msg-mail-read-local",
        ],
        workspace.path(),
    );

    assert_success(&output);
    let envelope = success_json(&output);
    assert_eq!(envelope["summary"], "Marked 2 messages as read");
    assert_eq!(envelope["data"]["action"], "mark_read");
    assert_eq!(envelope["data"]["updated_count"], 2);
    assert_eq!(
        envelope["data"]["message_ids"],
        json!(["msg-group-read-local", "msg-mail-read-local"])
    );
    assert_no_legacy_websocket_fallback_warning(&envelope);
    assert!(server.requests().is_empty());
    assert_eq!(
        is_read(workspace.path(), &bob_identity_id, "msg-group-read-local"),
        1
    );
    assert_eq!(
        is_read(workspace.path(), &bob_identity_id, "msg-mail-read-local"),
        1
    );
}

fn register_ready_msg_identity(
    workspace: &Path,
    identity_name: &str,
    handle: &str,
    jwt_token: &str,
) -> String {
    set_secret_storage_mode(workspace, "file_compat");
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

    let tenant = tenant_workspace(workspace);
    let index_path = tenant.join("identities").join("index.json");
    let mut index: Value = serde_json::from_slice(&std::fs::read(&index_path).unwrap()).unwrap();
    let unique_id = index["credentials"][identity_name]["unique_id"]
        .as_str()
        .unwrap()
        .to_string();
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
    unique_id
}

fn write_msg_ws_config(workspace: &Path, base_url: &str, socket_path: &str) {
    write_default_tenant_registry(workspace, base_url, "awiki.ai");
    write_tenant_config(
        workspace,
        format!("schema_version: 1\nruntime:\n  mode: websocket\n  socket_path: {socket_path}\n")
            .as_str(),
    );
}

fn seed_direct_message(
    workspace: &Path,
    owner_identity_id: &str,
    owner_did: &str,
    peer_did: &str,
    msg_id: &str,
    content: &str,
    sent_at: &str,
) {
    let conversation_id = format!("dm:{peer_did}");
    execute_sql(
        workspace,
        r#"
	INSERT INTO messages
	    (msg_id, owner_identity_id, owner_did, conversation_id, thread_id, direction, sender_did, receiver_did, content_type, content,
	     sent_at, stored_at, is_read, credential_name)
	VALUES (?1, ?2, ?3, ?4, ?4, 0, ?5, ?3, 'text/plain', ?6, ?7, ?7, 0, 'bob-msg')"#,
        (
            msg_id,
            owner_identity_id,
            owner_did,
            conversation_id,
            peer_did,
            content,
            sent_at,
        ),
    );
}

fn seed_group_message(
    workspace: &Path,
    owner_identity_id: &str,
    owner_did: &str,
    group_did: &str,
    msg_id: &str,
    content: &str,
    sent_at: &str,
) {
    execute_sql(
        workspace,
        r#"
	INSERT INTO messages
	    (msg_id, owner_identity_id, owner_did, conversation_id, thread_id, direction, sender_did, group_id, group_did, content_type,
	     content, sent_at, stored_at, is_read, credential_name)
	VALUES (?1, ?2, ?3, ?4, ?4, 0, 'did:wba:awiki.ai:alice:e1_alice', ?5, ?5, 'text/plain',
	        ?6, ?7, ?7, 0, 'bob-msg')"#,
        (
            msg_id,
            owner_identity_id,
            owner_did,
            format!("group:{group_did}"),
            group_did,
            content,
            sent_at,
        ),
    );
}

fn seed_mail_notification(
    workspace: &Path,
    owner_identity_id: &str,
    owner_did: &str,
    msg_id: &str,
    content: &str,
    sent_at: &str,
) {
    execute_sql(
        workspace,
        r#"
	INSERT INTO messages
	    (msg_id, owner_identity_id, owner_did, conversation_id, thread_id, direction, sender_did, receiver_did, content_type, content,
	     sent_at, stored_at, is_read, metadata, credential_name)
	VALUES (?1, ?2, ?3, ?4, ?4, 0, 'did:wba:mail:system', ?3, 'mail.notification', ?5,
	        ?6, ?6, 0, '{"source_kind":"mail"}', 'bob-msg')"#,
        (
            msg_id,
            owner_identity_id,
            owner_did,
            "mail:bob@awiki.ai",
            content,
            sent_at,
        ),
    );
}

fn execute_sql<P>(workspace: &Path, statement: &str, params: P)
where
    P: rusqlite::Params,
{
    let connection = open_local_state(workspace);
    connection
        .execute(statement, params)
        .expect("execute test sql");
}

fn is_read(workspace: &Path, owner_identity_id: &str, message_id: &str) -> i64 {
    let connection = open_local_state(workspace);
    connection
        .query_row(
            "SELECT is_read FROM messages WHERE owner_identity_id = ?1 AND msg_id = ?2",
            (owner_identity_id, message_id),
            |row| row.get(0),
        )
        .expect("read message is_read")
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

fn assert_failure(output: &Output) {
    assert_ne!(
        output.status.code(),
        Some(0),
        "expected failure; stdout:\n{}\nstderr:\n{}",
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
    let raw = if output.stdout.is_empty() {
        &output.stderr
    } else {
        &output.stdout
    };
    let envelope: Value =
        serde_json::from_slice(raw).expect("output should be a JSON error envelope");
    assert_eq!(envelope["ok"], false);
    envelope
}

fn assert_transport_unavailable_without_legacy_fallback(output: &Output) {
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

    fn internal_error(body: &str) -> Self {
        Self {
            status: 500,
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
