use rusqlite::types::ValueRef;
use serde_json::{json, Map, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

mod support;

use support::{open_local_state, write_ready_identity, TestIdentity, TestIdentityOptions};

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

    let rows = query_rows(
        workspace.path(),
        &format!(
            "SELECT msg_id, direction, content, is_read FROM messages WHERE msg_id = '{message_id}'"
        ),
    );
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["content"], "hello direct");
    assert_eq!(rows[0]["direction"], 1);
    assert_eq!(rows[0]["is_read"], 1);
}

#[test]
fn msg_send_secure_on_alias_dry_run_reaches_im_core_secure_plan() {
    let workspace = TempDir::new("msg-live-secure-send").expect("workspace");
    let bob_did = "did:wba:awiki.ai:bob:e1_bob";

    let output = awiki_cmd(
        &[
            "--identity",
            "alice-secure",
            "msg",
            "send",
            "--to",
            bob_did,
            "--text",
            "hello secure live",
            "--secure",
            "on",
            "--dry-run",
        ],
        workspace.path(),
    );

    assert_success(&output);
    let envelope = success_json(&output);
    assert_eq!(envelope["summary"], "Dry run: message send planned");
    assert_eq!(envelope["data"]["plan"]["action"], "direct.send");
    assert_eq!(envelope["data"]["plan"]["target"]["did"], bob_did);
    assert_eq!(envelope["data"]["plan"]["secure"], true);
    assert_eq!(envelope["data"]["plan"]["security"], "required");
    assert_warning_contains(
        &envelope,
        "--secure on is deprecated; use --secure required.",
    );
}

#[test]
fn msg_secure_init_is_stable_unsupported_without_secure_direct_legacy_path() {
    let workspace = TempDir::new("msg-live-secure-init").expect("workspace");
    write_msg_config(workspace.path(), "https://placeholder.invalid");
    register_generated_msg_identity(workspace.path(), "alice-init", "alice", "jwt-alice");
    let bob = register_generated_msg_identity(workspace.path(), "bob-init", "bob", "jwt-bob");
    let server = TestServer::new(vec![]);
    write_msg_config(workspace.path(), &server.base_url());

    let output = awiki_cmd(
        &[
            "--identity",
            "alice-init",
            "msg",
            "secure",
            "init",
            "--with",
            &bob.did,
        ],
        workspace.path(),
    );

    assert_secure_direct_unsupported(&output, "msg.secure.init");
    assert_eq!(server.requests().len(), 0);
}

#[test]
fn msg_secure_retry_is_stable_unsupported_without_secure_direct_legacy_path() {
    let workspace = TempDir::new("msg-live-secure-retry").expect("workspace");
    write_msg_config(workspace.path(), "https://placeholder.invalid");
    register_generated_msg_identity(workspace.path(), "alice-retry", "alice", "jwt-alice");
    register_generated_msg_identity(workspace.path(), "bob-retry", "bob", "jwt-bob");
    let server = TestServer::new(vec![]);
    write_msg_config(workspace.path(), &server.base_url());

    let output = awiki_cmd(
        &[
            "--identity",
            "alice-retry",
            "msg",
            "secure",
            "retry",
            "retry-live-1",
        ],
        workspace.path(),
    );

    assert_secure_direct_unsupported(&output, "msg.secure.retry");
    assert_eq!(server.requests().len(), 0);
}

#[test]
fn msg_inbox_history_and_mark_read_live_match_go_output_shape() {
    let workspace = TempDir::new("msg-live-read").expect("workspace");
    let bob = register_ready_msg_identity(workspace.path(), "bob-msg", "bob", "jwt-bob");
    let alice_did = "did:wba:awiki.ai:alice:e1_alice";
    let bob_did = bob.did.as_str();
    let message = json!({
        "id": "msg-direct-1",
        "type": "text",
        "sender_did": alice_did,
        "receiver_did": bob_did,
        "content_type": "text/plain",
        "content": "hello bob",
        "sent_at": "2026-04-07T01:02:03Z",
        "is_read": false,
        "peer_user_id": "user-alice",
        "peer_full_handle": "alice.awiki.ai",
    });
    let server = TestServer::new(vec![
        TestResponse::ok(&json_rpc_result(json!({
            "messages": [message.clone()],
            "total": 1,
            "source": "remote_http"
        }))),
        TestResponse::ok(&json_rpc_result(json!({
            "messages": [message.clone()],
            "total": 1,
            "source": "remote_http"
        }))),
        TestResponse::ok(r#"{"jsonrpc":"2.0","result":{"updated_count":1},"id":"req-1"}"#),
    ]);
    write_msg_config(workspace.path(), &server.base_url());

    let unsupported_inbox_filter = awiki_cmd(
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
    assert_unsupported_capability(
        &unsupported_inbox_filter,
        "msg.inbox",
        "inbox-target-filters",
        "Phase 3",
    );

    let inbox = awiki_cmd(
        &["--identity", "bob-msg", "msg", "inbox", "--scope", "direct"],
        workspace.path(),
    );
    assert_success(&inbox);
    let inbox_json = success_json(&inbox);
    assert_eq!(inbox_json["summary"], "Loaded 1 inbox messages");
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
    assert_eq!(requests.len(), 3);
    assert!(requests[0].starts_with("POST /im/rpc HTTP/1.1"));
    assert!(requests[1].starts_with("POST /im/rpc HTTP/1.1"));
    assert!(requests[2].starts_with("POST /im/rpc HTTP/1.1"));

    let inbox_body: Value = serde_json::from_str(request_body(&requests[0])).expect("inbox body");
    assert_eq!(inbox_body["method"], "inbox.get");
    assert_eq!(
        inbox_body["params"]["meta"]["profile"],
        "anp.inbox.local.v1"
    );
    assert_eq!(inbox_body["params"]["body"]["limit"], 20);
    assert_eq!(inbox_body["params"]["body"].get("peer_did"), None);

    let history_body: Value =
        serde_json::from_str(request_body(&requests[1])).expect("history body");
    assert_eq!(history_body["method"], "direct.get_history");
    assert_eq!(history_body["params"]["body"]["peer_did"], alice_did);
    assert_eq!(history_body["params"]["body"]["limit"], 5);

    let mark_body: Value = serde_json::from_str(request_body(&requests[2])).expect("mark body");
    assert_eq!(mark_body["method"], "inbox.mark_read");
    assert_eq!(
        mark_body["params"]["body"]["message_ids"],
        json!(["msg-direct-1"])
    );
}

#[test]
fn msg_history_with_handle_merges_local_handle_history_cache_like_go() {
    let workspace = TempDir::new("msg-live-handle-history").expect("workspace");
    let bob = register_ready_msg_identity(workspace.path(), "bob-msg", "bob", "jwt-bob");
    let bob_did = bob.did.as_str();
    let alice_old = "did:wba:awiki.ai:alice:e1_old";
    let alice_new = "did:wba:awiki.ai:alice:e1_new";
    seed_contact(
        workspace.path(),
        &bob.unique_id,
        bob_did,
        alice_old,
        "alice",
        "2026-04-07T01:00:00Z",
    );
    seed_direct_message(
        workspace.path(),
        &bob.unique_id,
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
    assert_eq!(envelope["summary"], "Loaded 1 direct history messages");
    assert_eq!(envelope["data"]["source"], "remote_http");
    assert_eq!(envelope["data"]["messages"][0]["msg_id"], "msg-new");
    assert_eq!(
        envelope["data"]["resolved_dids"],
        json!([alice_new, alice_old])
    );

    let bindings = query_rows(
        workspace.path(),
        "SELECT did, is_current FROM contact_handle_bindings WHERE handle = 'alice' ORDER BY is_current DESC, last_seen_at DESC",
    );
    assert_eq!(bindings.len(), 1);
    assert_eq!(bindings[0]["did"], alice_old);
    assert_eq!(bindings[0]["is_current"], 1);
}

#[test]
fn msg_history_with_handle_filters_secure_wire_rows_from_local_handle_history_cache_like_go() {
    let workspace = TempDir::new("msg-live-secure-handle-history").expect("workspace");
    let bob = register_ready_msg_identity(workspace.path(), "bob-msg", "bob", "jwt-bob");
    let bob_did = bob.did.as_str();
    let alice_old = "did:wba:awiki.ai:alice:e1_old";
    let alice_new = "did:wba:awiki.ai:alice:e1_new";
    seed_contact(
        workspace.path(),
        &bob.unique_id,
        bob_did,
        alice_old,
        "alice",
        "2026-04-07T01:00:00Z",
    );
    seed_direct_message_with_type(
        workspace.path(),
        &bob.unique_id,
        bob_did,
        alice_old,
        "msg-wire",
        "application/anp-direct-cipher+json",
        r#"{"session_id":"sid-1"}"#,
        "2026-04-07T02:00:00Z",
    );
    seed_direct_message_with_type(
        workspace.path(),
        &bob.unique_id,
        bob_did,
        alice_old,
        "msg-plain",
        "text/plain",
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
        "sent_at": "2026-04-07T03:00:00Z",
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
    let messages = success_json(&history)["data"]["messages"]
        .as_array()
        .cloned()
        .unwrap();

    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0]["msg_id"], "msg-new");
    assert!(!messages.iter().any(|message| {
        message["msg_id"] == "msg-wire"
            || message["content_type"] == "application/anp-direct-cipher+json"
    }));
}

fn register_ready_msg_identity(
    workspace: &Path,
    identity_name: &str,
    handle: &str,
    jwt_token: &str,
) -> TestIdentity {
    register_generated_msg_identity(workspace, identity_name, handle, jwt_token)
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

fn assert_warning_contains(envelope: &Value, expected: &str) {
    let warnings = envelope["warnings"].as_array().expect("warnings array");
    assert!(
        warnings.iter().any(|warning| warning
            .as_str()
            .is_some_and(|value| value.contains(expected))),
        "expected warning {expected:?}; got {warnings:?}"
    );
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

fn assert_secure_direct_unsupported(output: &Output, command: &str) {
    assert_unsupported_capability(output, command, "secure-direct", "Phase 6");
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
    assert_eq!(
        envelope["error"]["details"]["cutover_status"],
        "unsupported"
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
        "expected request to contain {needle:?}, got:\n{haystack}"
    );
}

fn query_rows(workspace: &Path, sql: &str) -> Vec<Value> {
    let connection = open_local_state(workspace);
    let mut statement = connection.prepare(sql).expect("prepare test query");
    let names = statement
        .column_names()
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>();
    statement
        .query_map([], |row| {
            let mut object = Map::new();
            for (index, name) in names.iter().enumerate() {
                object.insert(name.clone(), sqlite_value_to_json(row.get_ref(index)?));
            }
            Ok(Value::Object(object))
        })
        .expect("run test query")
        .map(|row| row.expect("read test row"))
        .collect()
}

fn seed_contact(
    workspace: &Path,
    owner_identity_id: &str,
    owner_did: &str,
    peer_did: &str,
    handle: &str,
    seen_at: &str,
) {
    execute_sql(
        workspace,
        format!(
            "INSERT INTO contacts (owner_identity_id, owner_did, did, handle, messaged, first_seen_at, last_seen_at) VALUES ('{owner_identity_id}', '{owner_did}', '{peer_did}', '{handle}', 1, '{seen_at}', '{seen_at}')",
        ),
    );
    execute_sql(
        workspace,
        format!(
            "INSERT INTO contact_handle_bindings (owner_identity_id, owner_did, handle, did, is_current, first_seen_at, last_seen_at, credential_name) VALUES ('{owner_identity_id}', '{owner_did}', '{handle}', '{peer_did}', 1, '{seen_at}', '{seen_at}', 'bob-msg') ON CONFLICT(owner_identity_id, handle, did) DO UPDATE SET is_current = excluded.is_current, last_seen_at = excluded.last_seen_at, credential_name = excluded.credential_name",
        ),
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
    seed_direct_message_with_type(
        workspace,
        owner_identity_id,
        owner_did,
        peer_did,
        msg_id,
        "text/plain",
        content,
        sent_at,
    );
}

fn seed_direct_message_with_type(
    workspace: &Path,
    owner_identity_id: &str,
    owner_did: &str,
    peer_did: &str,
    msg_id: &str,
    content_type: &str,
    content: &str,
    sent_at: &str,
) {
    let conversation_id = format!("dm:{peer_did}");
    let statement = format!(
        "INSERT INTO messages (msg_id, owner_identity_id, owner_did, conversation_id, thread_id, direction, sender_did, receiver_did, content_type, content, sent_at, stored_at, is_read, credential_name) VALUES ('{msg_id}', '{owner_identity_id}', '{owner_did}', '{conversation_id}', '{conversation_id}', 0, '{peer_did}', '{owner_did}', '{content_type}', '{content}', '{sent_at}', '{sent_at}', 0, 'bob-msg')",
    );
    execute_sql(workspace, statement);
}

fn execute_sql(workspace: &Path, statement: String) {
    let connection = open_local_state(workspace);
    connection
        .execute_batch(&statement)
        .expect("execute test sql");
}

fn sqlite_value_to_json(value: ValueRef<'_>) -> Value {
    match value {
        ValueRef::Null => Value::Null,
        ValueRef::Integer(value) => json!(value),
        ValueRef::Real(value) => json!(value),
        ValueRef::Text(value) => Value::String(String::from_utf8_lossy(value).into_owned()),
        ValueRef::Blob(value) => {
            Value::Array(value.iter().copied().map(|byte| json!(byte)).collect())
        }
    }
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
