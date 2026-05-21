use awiki_cli::config::Paths;
use awiki_cli::identity::{generate_identity, types::SaveInput, Manager};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[test]
fn msg_send_im_core_mvp_direct_text_posts_im_core_rpc() {
    let workspace = TempDir::new().expect("workspace");
    let manager = identity_manager(workspace.path());
    let alice = register_generated_msg_identity(&manager, "alice-mvp", "alice", "jwt-alice");
    let bob_did = "did:wba:awiki.ai:bob:e1_bob";
    let server = TestServer::new(vec![TestResponse::ok(&json_rpc_result(json!({
        "accepted": true,
        "final_acceptance": true,
        "accepted_at": "2026-05-21T00:00:00Z",
        "delivery_state": "accepted"
    })))]);
    write_msg_config(workspace.path(), &server.base_url());

    let output = awiki_cmd_with_env(
        &[
            "--identity",
            "alice-mvp",
            "msg",
            "send",
            "--to",
            bob_did,
            "--text",
            "hello through im-core",
        ],
        workspace.path(),
        &[("AWIKI_USE_IM_CORE_MVP", "1")],
    );

    let envelope = success_json(&output);
    assert_eq!(envelope["summary"], "Sent a direct text message");
    assert_eq!(envelope["data"]["target"]["did"], bob_did);
    assert_eq!(envelope["data"]["message"]["secure"], false);
    assert_eq!(envelope["data"]["message"]["type"], "text");
    assert_eq!(
        envelope["data"]["delivery"]["target_did"],
        "did:wba:awiki.ai:bob:e1_bob"
    );
    let message_id = envelope["data"]["message"]["id"]
        .as_str()
        .expect("message id");
    assert!(message_id.starts_with("msg-"));

    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].starts_with("POST /im/rpc HTTP/1.1"));
    assert!(
        requests[0].contains("Authorization: Bearer jwt-alice\r\n"),
        "missing bearer auth:\n{}",
        requests[0]
    );
    let body: Value = serde_json::from_str(request_body(&requests[0])).expect("request JSON");
    assert_eq!(body["method"], "direct.send");
    assert_eq!(body["params"]["meta"]["sender_did"], alice.did);
    assert_eq!(
        body["params"]["meta"]["target"],
        json!({"kind": "agent", "did": bob_did})
    );
    assert_eq!(
        body["params"]["body"],
        json!({"text": "hello through im-core"})
    );
    assert_eq!(
        body["params"]["auth"]["scheme"],
        "anp-rfc9421-origin-proof-v1"
    );
}

#[test]
fn msg_send_im_core_mvp_group_text_posts_im_core_rpc() {
    let workspace = TempDir::new().expect("workspace");
    let manager = identity_manager(workspace.path());
    let alice = register_generated_msg_identity(&manager, "alice-group-mvp", "alice", "jwt-alice");
    let group_did = "did:wba:awiki.ai:groups:demo:e1_group";
    let server = TestServer::new(vec![TestResponse::ok(&json_rpc_result(json!({
        "accepted": true,
        "final_acceptance": true,
        "group_did": group_did,
        "message_id": "server-message-id",
        "operation_id": "server-operation-id",
        "group_event_seq": "42",
        "group_state_version": "v42",
        "accepted_at": "2026-05-21T00:00:00Z"
    })))]);
    write_msg_config(workspace.path(), &server.base_url());

    let output = awiki_cmd_with_env(
        &[
            "--identity",
            "alice-group-mvp",
            "msg",
            "send",
            "--group",
            group_did,
            "--text",
            "hello group through im-core",
        ],
        workspace.path(),
        &[("AWIKI_USE_IM_CORE_MVP", "1")],
    );

    let envelope = success_json(&output);
    assert_eq!(envelope["summary"], "Sent a group text message");
    assert_eq!(envelope["data"]["target"]["kind"], "group");
    assert_eq!(envelope["data"]["target"]["did"], group_did);
    assert_eq!(envelope["data"]["message"]["secure"], false);
    assert_eq!(envelope["data"]["message"]["type"], "text");
    assert_eq!(envelope["data"]["message"]["id"], format!("{group_did}:42"));
    assert_eq!(
        envelope["data"]["delivery"]["operation_id"],
        "server-operation-id"
    );
    assert_eq!(envelope["data"]["delivery"]["group_event_seq"], "42");
    assert_eq!(envelope["data"]["source"], "remote_http");

    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].starts_with("POST /im/rpc HTTP/1.1"));
    assert!(
        requests[0].contains("Authorization: Bearer jwt-alice\r\n"),
        "missing bearer auth:\n{}",
        requests[0]
    );
    let body: Value = serde_json::from_str(request_body(&requests[0])).expect("request JSON");
    assert_eq!(body["method"], "group.send");
    assert_eq!(body["params"]["meta"]["sender_did"], alice.did);
    assert_eq!(
        body["params"]["meta"]["target"],
        json!({"kind": "group", "did": group_did})
    );
    assert_eq!(body["params"]["meta"]["content_type"], "text/plain");
    assert_eq!(
        body["params"]["body"],
        json!({"text": "hello group through im-core"})
    );
    assert_eq!(
        body["params"]["auth"]["scheme"],
        "anp-rfc9421-origin-proof-v1"
    );
}

#[test]
fn msg_inbox_im_core_mvp_direct_posts_im_core_rpc() {
    let workspace = TempDir::new().expect("workspace");
    let manager = identity_manager(workspace.path());
    let alice = register_generated_read_identity(&manager, "alice-inbox-mvp", "alice", "jwt-alice");
    let bob_did = "did:wba:awiki.ai:bob:e1_bob";
    let server = TestServer::new(vec![TestResponse::ok(&json_rpc_result(json!({
        "messages": [{
            "id": "msg-inbox-mvp-1",
            "sender_did": bob_did,
            "receiver_did": alice.did,
            "content": "hello inbox",
            "content_type": "text/plain",
            "sent_at": "2026-05-21T00:00:00Z",
            "server_seq": 7,
            "is_read": false
        }],
        "total": 1,
        "source": "remote_http"
    })))]);
    write_msg_config(workspace.path(), &server.base_url());

    let output = awiki_cmd_with_env(
        &[
            "--identity",
            "alice-inbox-mvp",
            "msg",
            "inbox",
            "--scope",
            "direct",
            "--limit",
            "3",
            "--unread",
        ],
        workspace.path(),
        &[("AWIKI_USE_IM_CORE_MVP", "1")],
    );

    let envelope = success_json(&output);
    assert_eq!(envelope["summary"], "Loaded 1 inbox messages");
    assert_eq!(envelope["data"]["messages"][0]["id"], "msg-inbox-mvp-1");
    assert_eq!(envelope["data"]["source"], "remote_http");

    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].starts_with("POST /im/rpc HTTP/1.1"));
    assert!(
        requests[0].contains("Authorization: Bearer jwt-alice\r\n"),
        "missing bearer auth:\n{}",
        requests[0]
    );
    let body: Value = serde_json::from_str(request_body(&requests[0])).expect("request JSON");
    assert_eq!(body["method"], "inbox.get");
    assert_eq!(body["params"]["meta"]["sender_did"], alice.did);
    assert_eq!(body["params"]["body"]["user_did"], alice.did);
    assert_eq!(body["params"]["body"]["limit"], 3);
    assert_eq!(body["params"]["body"].get("peer_did"), None);
}

#[test]
fn msg_history_im_core_mvp_direct_posts_im_core_rpc() {
    let workspace = TempDir::new().expect("workspace");
    let manager = identity_manager(workspace.path());
    let alice =
        register_generated_read_identity(&manager, "alice-history-mvp", "alice", "jwt-alice");
    let bob_did = "did:wba:awiki.ai:bob:e1_bob";
    let server = TestServer::new(vec![TestResponse::ok(&json_rpc_result(json!({
        "messages": [{
            "id": "msg-history-mvp-1",
            "sender_did": alice.did,
            "receiver_did": bob_did,
            "content": "hello history",
            "content_type": "text/plain",
            "sent_at": "2026-05-21T00:00:00Z",
            "server_seq": 9
        }],
        "total": 1,
        "source": "remote_http",
        "resolved_dids": [bob_did]
    })))]);
    write_msg_config(workspace.path(), &server.base_url());

    let output = awiki_cmd_with_env(
        &[
            "--identity",
            "alice-history-mvp",
            "msg",
            "history",
            "--with",
            bob_did,
            "--limit",
            "4",
            "--cursor",
            "8",
        ],
        workspace.path(),
        &[("AWIKI_USE_IM_CORE_MVP", "1")],
    );

    let envelope = success_json(&output);
    assert_eq!(envelope["summary"], "Loaded 1 direct history messages");
    assert_eq!(envelope["data"]["messages"][0]["id"], "msg-history-mvp-1");
    assert_eq!(envelope["data"]["with"], bob_did);
    assert_eq!(envelope["data"]["source"], "remote_http");

    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].starts_with("POST /im/rpc HTTP/1.1"));
    assert!(
        requests[0].contains("Authorization: Bearer jwt-alice\r\n"),
        "missing bearer auth:\n{}",
        requests[0]
    );
    let body: Value = serde_json::from_str(request_body(&requests[0])).expect("request JSON");
    assert_eq!(body["method"], "direct.get_history");
    assert_eq!(body["params"]["meta"]["sender_did"], alice.did);
    assert_eq!(body["params"]["body"]["user_did"], alice.did);
    assert_eq!(body["params"]["body"]["peer_did"], bob_did);
    assert_eq!(body["params"]["body"]["limit"], 4);
    assert_eq!(body["params"]["body"]["since_seq"], "8");
}

fn awiki_cmd_with_env(args: &[&str], workspace: &Path, envs: &[(&str, &str)]) -> Output {
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
    for (key, value) in envs {
        command.env(key, value);
    }
    command.output().expect("run awiki-cli")
}

fn register_generated_msg_identity(
    manager: &Manager,
    identity_name: &str,
    handle: &str,
    jwt_token: &str,
) -> awiki_cli::identity::types::StoredIdentity {
    let generated = generate_identity(
        "awiki.ai",
        "https://awiki.ai/anp-im/rpc",
        "did:wba:awiki.ai",
    )
    .expect("generate identity");
    manager
        .save(SaveInput {
            identity_name: identity_name.to_string(),
            did: generated.did,
            unique_id: generated.unique_id,
            user_id: format!("user-{handle}"),
            display_name: identity_name.to_string(),
            handle: handle.to_string(),
            full_handle: format!("{handle}.awiki.ai"),
            jwt_token: jwt_token.to_string(),
            did_document: Some(generated.did_document),
            key1_private_pem: generated.key1_private_pem,
            key1_public_pem: generated.key1_public_pem,
            e2ee_signing_private_pem: generated.e2ee_signing_private_pem,
            e2ee_agreement_private_pem: generated.e2ee_agreement_private_pem,
            ..SaveInput::default()
        })
        .expect("save generated message identity")
}

fn register_generated_read_identity(
    manager: &Manager,
    identity_name: &str,
    handle: &str,
    jwt_token: &str,
) -> awiki_cli::identity::types::StoredIdentity {
    let generated = generate_identity(
        "awiki.ai",
        "https://awiki.ai/anp-im/rpc",
        "did:wba:awiki.ai",
    )
    .expect("generate identity");
    manager
        .save(SaveInput {
            identity_name: identity_name.to_string(),
            did: generated.did,
            unique_id: generated.unique_id,
            user_id: format!("user-{handle}"),
            display_name: identity_name.to_string(),
            handle: handle.to_string(),
            full_handle: format!("{handle}.awiki.ai"),
            jwt_token: jwt_token.to_string(),
            did_document: Some(generated.did_document),
            key1_private_pem: generated.key1_private_pem,
            key1_public_pem: generated.key1_public_pem,
            ..SaveInput::default()
        })
        .expect("save generated read identity")
}

fn identity_manager(workspace: &Path) -> Manager {
    Manager::new(test_paths(workspace))
}

fn test_paths(workspace: &Path) -> Paths {
    for directory in ["data", "runtime", "cache", "logs"] {
        std::fs::create_dir_all(workspace.join(directory)).expect("create workspace subdir");
    }
    Paths {
        workspace_home_dir: path_string(workspace),
        root_dir: path_string(workspace),
        config_dir: path_string(workspace),
        data_dir: path_string(&workspace.join("data")),
        state_dir: path_string(&workspace.join("runtime")),
        cache_dir: path_string(&workspace.join("cache")),
        logs_dir: path_string(&workspace.join("logs")),
        config_file: path_string(&workspace.join("config.yaml")),
        identity_dir: path_string(&workspace.join("identities")),
        database_file: path_string(&workspace.join("data").join("awiki-cli.db")),
        legacy_credentials_dir: path_string(&workspace.join("legacy-credentials")),
        legacy_data_dir: path_string(&workspace.join("legacy-data")),
    }
}

fn write_msg_config(workspace: &Path, base_url: &str) {
    std::fs::write(
        workspace.join("config.yaml"),
        format!("runtime:\n  mode: http\nservices:\n  service_base_url: {base_url}\n"),
    )
    .unwrap();
}

fn success_json(output: &Output) -> Value {
    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stderr.is_empty(),
        "stderr should be empty: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("success JSON")
}

fn request_body(raw: &str) -> &str {
    raw.split("\r\n\r\n").nth(1).unwrap_or_default()
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
                let Some(stream) = accept_with_timeout(&listener) else {
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
    let mut buffer = [0_u8; 8192];
    let mut raw = Vec::new();
    loop {
        let read = stream.read(&mut buffer).expect("read request");
        if read == 0 {
            break;
        }
        raw.extend_from_slice(&buffer[..read]);
        if request_complete(&raw) {
            break;
        }
    }
    requests
        .lock()
        .expect("requests mutex")
        .push(String::from_utf8_lossy(&raw).into_owned());
    let reason = if response.status == 200 { "OK" } else { "ERR" };
    let body = response.body.as_bytes();
    let header = format!(
        "HTTP/1.1 {} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        response.status,
        body.len()
    );
    stream.write_all(header.as_bytes()).expect("write header");
    stream.write_all(body).expect("write body");
}

fn request_complete(raw: &[u8]) -> bool {
    let text = String::from_utf8_lossy(raw);
    let Some((headers, body)) = text.split_once("\r\n\r\n") else {
        return false;
    };
    let content_length = headers
        .lines()
        .find_map(|line| {
            line.strip_prefix("Content-Length:")
                .or_else(|| line.strip_prefix("content-length:"))
        })
        .and_then(|value| value.trim().parse::<usize>().ok())
        .unwrap_or_default();
    body.len() >= content_length
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new() -> std::io::Result<Self> {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "awiki-cli-rs2-msg-im-core-mvp-test-{}-{nanos}",
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
