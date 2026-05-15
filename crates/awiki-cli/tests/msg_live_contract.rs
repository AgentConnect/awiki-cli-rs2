use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[test]
fn msg_send_live_posts_direct_rpc_and_persists_outbound_row_like_go() {
    let workspace = TempDir::new("msg-live-send").expect("workspace");
    register_ready_msg_identity(workspace.path(), "alice-msg", "alice", "jwt-alice");
    let bob_did = "did:wba:awiki.ai:bob:e1_bob";
    let server = TestServer::new(vec![TestResponse::ok(
        r#"{"jsonrpc":"2.0","result":{"accepted":true,"final_acceptance":true,"accepted_at":"2026-04-07T01:02:03Z","delivery_state":"accepted"},"id":"req-1"}"#,
    )]);
    write_msg_config(workspace.path(), &server.base_url());

    let output = awiki_cmd(
        &[
            "--identity",
            "alice-msg",
            "msg",
            "send",
            "--to",
            bob_did,
            "--text",
            "hello direct",
        ],
        workspace.path(),
    );

    assert_success(&output);
    let envelope = success_json(&output);
    assert_eq!(envelope["summary"], "Sent a direct text message");
    assert_eq!(envelope["data"]["action"], "send_message");
    assert_eq!(envelope["data"]["target"]["did"], bob_did);
    assert_eq!(envelope["data"]["message"]["type"], "text");
    assert_eq!(envelope["data"]["message"]["secure"], false);
    let message_id = envelope["data"]["message"]["id"]
        .as_str()
        .expect("message id");
    assert!(message_id.starts_with("msg-"));
    assert_eq!(envelope["data"]["delivery"]["message_id"], message_id);
    assert_eq!(envelope["data"]["delivery"]["target_did"], bob_did);

    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].starts_with("POST /im/rpc HTTP/1.1"));
    assert_contains_text(&requests[0], "Authorization: Bearer jwt-alice\r\n");
    let body: Value = serde_json::from_str(request_body(&requests[0])).expect("request body");
    assert_eq!(body["method"], "direct.send");
    assert_eq!(body["params"]["meta"]["profile"], "anp.direct.base.v1");
    assert_eq!(
        body["params"]["meta"]["target"],
        json!({"kind": "agent", "did": bob_did})
    );
    assert_eq!(body["params"]["body"], json!({"text": "hello direct"}));
    assert_eq!(
        body["params"]["auth"]["scheme"],
        "anp-rfc9421-origin-proof-v1"
    );

    let query = awiki_cmd_owned(
        &[
            "debug".to_string(),
            "db".to_string(),
            "query".to_string(),
            format!(
                "SELECT msg_id, direction, content, is_read FROM messages WHERE msg_id = '{message_id}'"
            ),
        ],
        workspace.path(),
    );
    assert_success(&query);
    let rows = success_json(&query)["data"]["rows"]
        .as_array()
        .cloned()
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["content"], "hello direct");
    assert_eq!(rows[0]["direction"], 1);
    assert_eq!(rows[0]["is_read"], 1);
}

#[test]
fn msg_inbox_history_and_mark_read_live_match_go_output_shape() {
    let workspace = TempDir::new("msg-live-read").expect("workspace");
    register_ready_msg_identity(workspace.path(), "bob-msg", "bob", "jwt-bob");
    let alice_did = "did:wba:awiki.ai:alice:e1_alice";
    let bob_did = "did:wba:awiki.ai:bob:e1_bob";
    let message = json!({
        "id": "msg-direct-1",
        "type": "text",
        "sender_did": alice_did,
        "receiver_did": bob_did,
        "content_type": "text/plain",
        "content": "hello bob",
        "sent_at": "2026-04-07T01:02:03Z",
        "is_read": false,
    });
    let server = TestServer::new(vec![
        TestResponse::ok(&json_rpc_result(json!({
            "messages": [message.clone()],
            "total": 1,
            "source": "remote_http"
        }))),
        TestResponse::ok(&json_rpc_result(json!({
            "did": alice_did,
            "handle": "alice.awiki.ai",
            "full_handle": "alice.awiki.ai"
        }))),
        TestResponse::ok(&json_rpc_result(json!({
            "messages": [message.clone()],
            "total": 1,
            "source": "remote_http"
        }))),
        TestResponse::ok(r#"{"jsonrpc":"2.0","result":{"updated_count":1},"id":"req-1"}"#),
    ]);
    write_msg_config(workspace.path(), &server.base_url());

    let inbox = awiki_cmd(
        &[
            "--identity",
            "bob-msg",
            "msg",
            "inbox",
            "--scope",
            "direct",
            "--with",
            alice_did,
            "--unread",
        ],
        workspace.path(),
    );
    assert_success(&inbox);
    let inbox_json = success_json(&inbox);
    assert_eq!(inbox_json["summary"], "Loaded 1 direct inbox messages");
    assert_eq!(inbox_json["data"]["messages"][0]["id"], "msg-direct-1");
    assert_eq!(inbox_json["data"]["messages"][0]["content"], "hello bob");

    let history = awiki_cmd(
        &[
            "--identity",
            "bob-msg",
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
    assert_eq!(history_json["data"]["messages"][0]["id"], "msg-direct-1");

    let mark = awiki_cmd(
        &["--identity", "bob-msg", "msg", "mark-read", "msg-direct-1"],
        workspace.path(),
    );
    assert_success(&mark);
    let mark_json = success_json(&mark);
    assert_eq!(mark_json["summary"], "Marked 1 messages as read");
    assert_eq!(mark_json["data"]["message_ids"], json!(["msg-direct-1"]));

    let requests = server.requests();
    assert_eq!(requests.len(), 4);
    assert!(requests[0].starts_with("POST /im/rpc HTTP/1.1"));
    assert!(requests[1].starts_with("POST /user-service/handle/rpc HTTP/1.1"));
    assert!(requests[2].starts_with("POST /im/rpc HTTP/1.1"));
    assert!(requests[3].starts_with("POST /im/rpc HTTP/1.1"));

    let inbox_body: Value = serde_json::from_str(request_body(&requests[0])).expect("inbox body");
    assert_eq!(inbox_body["method"], "inbox.get");
    assert_eq!(
        inbox_body["params"]["meta"]["profile"],
        "anp.inbox.local.v1"
    );
    assert_eq!(inbox_body["params"]["body"]["limit"], 20);
    assert_eq!(inbox_body["params"]["body"].get("peer_did"), None);

    let history_body: Value =
        serde_json::from_str(request_body(&requests[2])).expect("history body");
    assert_eq!(history_body["method"], "direct.get_history");
    assert_eq!(history_body["params"]["body"]["peer_did"], alice_did);
    assert_eq!(history_body["params"]["body"]["limit"], 5);

    let mark_body: Value = serde_json::from_str(request_body(&requests[3])).expect("mark body");
    assert_eq!(mark_body["method"], "inbox.mark_read");
    assert_eq!(
        mark_body["params"]["body"]["message_ids"],
        json!(["msg-direct-1"])
    );

    let query = awiki_cmd_owned(
        &[
            "debug".to_string(),
            "db".to_string(),
            "query".to_string(),
            "SELECT msg_id, direction, content, is_read FROM messages WHERE msg_id = 'msg-direct-1'".to_string(),
        ],
        workspace.path(),
    );
    assert_success(&query);
    let rows = success_json(&query)["data"]["rows"]
        .as_array()
        .cloned()
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["direction"], 0);
    assert_eq!(rows[0]["content"], "hello bob");
    assert_eq!(rows[0]["is_read"], 1);

    let contacts = query_rows(
        workspace.path(),
        "SELECT did, handle, messaged FROM contacts WHERE did = 'did:wba:awiki.ai:alice:e1_alice'",
    );
    assert_eq!(contacts.len(), 1);
    assert_eq!(contacts[0]["handle"], "alice");
    assert_eq!(contacts[0]["messaged"], 1);
}

#[test]
fn msg_history_with_handle_merges_local_handle_history_cache_like_go() {
    let workspace = TempDir::new("msg-live-handle-history").expect("workspace");
    register_ready_msg_identity(workspace.path(), "bob-msg", "bob", "jwt-bob");
    let bob_did = "did:wba:awiki.ai:bob:e1_bob";
    let alice_old = "did:wba:awiki.ai:alice:e1_old";
    let alice_new = "did:wba:awiki.ai:alice:e1_new";
    seed_contact(
        workspace.path(),
        bob_did,
        alice_old,
        "alice",
        "2026-04-07T01:00:00Z",
    );
    seed_direct_message(
        workspace.path(),
        bob_did,
        alice_old,
        "msg-old",
        "hello from old DID",
        "2026-04-07T01:00:00Z",
    );
    let remote_message = json!({
        "id": "msg-new",
        "type": "text",
        "sender_did": alice_new,
        "receiver_did": bob_did,
        "content_type": "text/plain",
        "content": "hello from new DID",
        "sent_at": "2026-04-07T02:00:00Z",
        "is_read": false,
    });
    let server = TestServer::new(vec![
        TestResponse::ok(&json_rpc_result(json!({
            "did": alice_new,
            "handle": "alice.awiki.ai",
            "full_handle": "alice.awiki.ai"
        }))),
        TestResponse::ok(&json_rpc_result(json!({
            "messages": [remote_message],
            "total": 1,
            "source": "remote_http"
        }))),
    ]);
    write_msg_config(workspace.path(), &server.base_url());

    let history = awiki_cmd(
        &[
            "--identity",
            "bob-msg",
            "msg",
            "history",
            "--with",
            "alice",
            "--limit",
            "5",
        ],
        workspace.path(),
    );
    assert_success(&history);
    let envelope = success_json(&history);
    assert_eq!(envelope["summary"], "Loaded 2 direct history messages");
    assert_eq!(envelope["data"]["source"], "remote_http+handle_history");
    assert_eq!(envelope["data"]["messages"][0]["id"], "msg-new");
    assert_eq!(envelope["data"]["messages"][1]["msg_id"], "msg-old");
    assert_eq!(
        envelope["data"]["resolved_dids"],
        json!([alice_new, alice_old])
    );

    let bindings = query_rows(
        workspace.path(),
        "SELECT did, is_current FROM contact_handle_bindings WHERE handle = 'alice' ORDER BY is_current DESC, last_seen_at DESC",
    );
    assert_eq!(bindings.len(), 2);
    assert_eq!(bindings[0]["did"], alice_new);
    assert_eq!(bindings[0]["is_current"], 1);
    assert_eq!(bindings[1]["did"], alice_old);
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

fn write_msg_config(workspace: &Path, base_url: &str) {
    std::fs::write(
        workspace.join("config.yaml"),
        format!("services:\n  service_base_url: {base_url}\n"),
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

fn request_body(raw: &str) -> &str {
    raw.split("\r\n\r\n").nth(1).unwrap_or_default()
}

fn assert_contains_text(haystack: &str, needle: &str) {
    assert!(
        haystack.contains(needle),
        "expected request to contain {needle:?}, got:\n{haystack}"
    );
}

fn query_rows(workspace: &Path, sql: &str) -> Vec<Value> {
    let query = awiki_cmd_owned(
        &[
            "debug".to_string(),
            "db".to_string(),
            "query".to_string(),
            sql.to_string(),
        ],
        workspace,
    );
    assert_success(&query);
    success_json(&query)["data"]["rows"]
        .as_array()
        .cloned()
        .unwrap()
}

fn seed_contact(workspace: &Path, owner_did: &str, peer_did: &str, handle: &str, seen_at: &str) {
    execute_sql(
        workspace,
        format!(
            "INSERT INTO contacts (owner_did, did, handle, messaged, first_seen_at, last_seen_at) VALUES ('{owner_did}', '{peer_did}', '{handle}', 1, '{seen_at}', '{seen_at}')",
        ),
    );
    execute_sql(
        workspace,
        format!(
            "UPDATE contact_handle_bindings SET is_current = 1, last_seen_at = '{seen_at}', credential_name = 'bob-msg' WHERE owner_did = '{owner_did}' AND handle = '{handle}' AND did = '{peer_did}'",
        ),
    );
}

fn seed_direct_message(
    workspace: &Path,
    owner_did: &str,
    peer_did: &str,
    msg_id: &str,
    content: &str,
    sent_at: &str,
) {
    let thread_id = format!("dm:{owner_did}:{peer_did}");
    let statement = format!(
        "INSERT INTO messages (msg_id, owner_did, thread_id, direction, sender_did, receiver_did, content_type, content, sent_at, stored_at, is_read, credential_name) VALUES ('{msg_id}', '{owner_did}', '{thread_id}', 0, '{peer_did}', '{owner_did}', 'text/plain', '{content}', '{sent_at}', '{sent_at}', 0, 'bob-msg')",
    );
    execute_sql(workspace, statement);
}

fn execute_sql(workspace: &Path, statement: String) {
    assert_success(&awiki_cmd_owned(
        &[
            "debug".to_string(),
            "db".to_string(),
            "query".to_string(),
            statement,
        ],
        workspace,
    ));
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
