use base64::Engine;
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

use support::{
    open_local_state, write_default_tenant_registry, write_ready_identity, write_tenant_config,
    TestIdentity, TestIdentityOptions,
};

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
    let alice_did = "did:wba:awiki.ai:alice:e1_alice";
    let server = TestServer::new(vec![
        TestResponse::registration(),
        TestResponse::prekey_publication(),
        TestResponse::sync_bootstrap(),
        TestResponse::sync_delta_direct(),
        TestResponse::message_batch(),
        TestResponse::directory_lookup(),
        TestResponse::sync_delta_empty(),
        TestResponse::mark_read_state(),
    ]);
    write_msg_config(workspace.path(), &server.base_url());
    let register = awiki_cmd(
        &[
            "id",
            "register",
            "--handle",
            "bob",
            "--phone",
            "13800138000",
            "--otp",
            "123456",
        ],
        workspace.path(),
    );
    assert_success(&register);

    let unsupported_inbox_filter = awiki_cmd(
        &[
            "--identity",
            "bob",
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
        &["--identity", "bob", "msg", "inbox", "--scope", "direct"],
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
            "bob",
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
        &["--identity", "bob", "msg", "mark-read", "msg-direct-1"],
        workspace.path(),
    );
    assert_success(&mark);
    let mark_json = success_json(&mark);
    assert_eq!(mark_json["summary"], "Marked 1 messages as read");
    assert_eq!(mark_json["data"]["message_ids"], json!(["msg-direct-1"]));

    let requests = server.requests();
    let methods = requests
        .iter()
        .map(|request| {
            serde_json::from_str::<Value>(request_body(request)).expect("request body")["method"]
                .as_str()
                .expect("request method")
                .to_owned()
        })
        .collect::<Vec<_>>();
    assert!(methods.starts_with(&[
        "register".to_owned(),
        "direct.e2ee.publish_prekey_bundle".to_owned(),
        "sync.bootstrap".to_owned(),
        "sync.delta".to_owned(),
        "message.get_batch".to_owned(),
    ]));
    assert!(!methods.iter().any(|method| method == "inbox.get"));
    let sync_delta = requests
        .iter()
        .find(|request| {
            serde_json::from_str::<Value>(request_body(request))
                .ok()
                .is_some_and(|body| body["method"] == "sync.delta")
        })
        .expect("sync.delta request");
    assert_eq!(
        serde_json::from_str::<Value>(request_body(sync_delta)).unwrap()["params"]["body"]
            ["reason"],
        "foreground_reconcile"
    );

    assert_eq!(
        methods
            .iter()
            .filter(|method| method.as_str() == "sync.delta")
            .count(),
        2
    );
    assert!(!methods.iter().any(|method| method == "direct.get_history"));

    let mark_request = requests
        .iter()
        .find(|request| {
            serde_json::from_str::<Value>(request_body(request))
                .ok()
                .is_some_and(|body| body["method"] == "read_state.mark_read")
        })
        .expect("mark-read request");
    let mark_body: Value = serde_json::from_str(request_body(mark_request)).expect("mark body");
    assert_eq!(mark_body["method"], "read_state.mark_read");
    assert_eq!(mark_body["params"]["body"]["read_up_to_server_seq"], "1");
    assert_eq!(
        mark_body["params"]["body"]["read_up_to_message_id"],
        "msg-direct-1"
    );
    assert!(!methods.iter().any(|method| method == "inbox.mark_read"));
}

#[test]
fn msg_history_with_handle_merges_local_handle_history_cache_like_go() {
    let workspace = TempDir::new("msg-live-handle-history").expect("workspace");
    let alice_old = "did:wba:awiki.ai:alice:e1_old";
    let alice_new = "did:wba:awiki.ai:alice:e1_new";
    let server = TestServer::new(vec![
        TestResponse::registration(),
        TestResponse::prekey_publication(),
        TestResponse::sync_bootstrap(),
        TestResponse::sync_delta_empty(),
    ]);
    write_msg_config(workspace.path(), &server.base_url());
    let (bob_identity_id, bob_did) = register_sync_v2_identity(workspace.path(), "bob");
    seed_contact(
        workspace.path(),
        &bob_identity_id,
        &bob_did,
        alice_old,
        "alice",
        "2026-04-07T01:00:00Z",
    );
    seed_direct_message(
        workspace.path(),
        &bob_identity_id,
        &bob_did,
        alice_old,
        "msg-old",
        "hello from old DID",
        "2026-04-07T01:00:00Z",
    );
    seed_contact(
        workspace.path(),
        &bob_identity_id,
        &bob_did,
        alice_new,
        "alice",
        "2026-04-07T02:00:00Z",
    );
    seed_direct_message(
        workspace.path(),
        &bob_identity_id,
        &bob_did,
        alice_new,
        "msg-new",
        "hello from new DID",
        "2026-04-07T02:00:00Z",
    );

    let history = awiki_cmd(
        &[
            "--identity",
            "bob",
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
    assert_eq!(envelope["data"]["source"], "local");
    assert_eq!(envelope["data"]["messages"][0]["msg_id"], "msg-new");
    assert_eq!(envelope["data"]["messages"][1]["msg_id"], "msg-old");
    assert_eq!(envelope["data"]["resolved_dids"], json!([]));

    let bindings = query_rows(
        workspace.path(),
        "SELECT did, is_current FROM contact_handle_bindings WHERE handle = 'alice' ORDER BY is_current DESC, last_seen_at DESC",
    );
    assert_eq!(bindings.len(), 2);
    assert_eq!(bindings[0]["did"], alice_new);
    assert_eq!(bindings[0]["is_current"], 1);
    assert_eq!(bindings[1]["did"], alice_old);
    assert_eq!(bindings[1]["is_current"], 0);

    let methods = server
        .requests()
        .iter()
        .map(|request| {
            serde_json::from_str::<Value>(request_body(request)).expect("request body")["method"]
                .as_str()
                .expect("request method")
                .to_owned()
        })
        .collect::<Vec<_>>();
    assert_eq!(
        methods,
        [
            "register",
            "direct.e2ee.publish_prekey_bundle",
            "sync.bootstrap",
            "sync.delta"
        ]
    );
}

#[test]
fn msg_history_with_handle_filters_secure_wire_rows_from_local_handle_history_cache_like_go() {
    let workspace = TempDir::new("msg-live-secure-handle-history").expect("workspace");
    let alice_old = "did:wba:awiki.ai:alice:e1_old";
    let alice_new = "did:wba:awiki.ai:alice:e1_new";
    let server = TestServer::new(vec![
        TestResponse::registration(),
        TestResponse::prekey_publication(),
        TestResponse::sync_bootstrap(),
        TestResponse::sync_delta_empty(),
    ]);
    write_msg_config(workspace.path(), &server.base_url());
    let (bob_identity_id, bob_did) = register_sync_v2_identity(workspace.path(), "bob");
    seed_contact(
        workspace.path(),
        &bob_identity_id,
        &bob_did,
        alice_old,
        "alice",
        "2026-04-07T01:00:00Z",
    );
    seed_direct_message_with_type(
        workspace.path(),
        &bob_identity_id,
        &bob_did,
        alice_old,
        "msg-wire",
        "application/anp-direct-cipher+json",
        r#"{"session_id":"sid-1"}"#,
        "2026-04-07T02:00:00Z",
    );
    execute_sql(
        workspace.path(),
        "UPDATE messages SET hydration_state = 'discovered' WHERE msg_id = 'msg-wire'".to_string(),
    );
    seed_direct_message_with_type(
        workspace.path(),
        &bob_identity_id,
        &bob_did,
        alice_old,
        "msg-plain",
        "text/plain",
        "hello from old DID",
        "2026-04-07T01:00:00Z",
    );
    seed_contact(
        workspace.path(),
        &bob_identity_id,
        &bob_did,
        alice_new,
        "alice",
        "2026-04-07T03:00:00Z",
    );
    seed_direct_message(
        workspace.path(),
        &bob_identity_id,
        &bob_did,
        alice_new,
        "msg-new",
        "hello from new DID",
        "2026-04-07T03:00:00Z",
    );

    let history = awiki_cmd(
        &[
            "--identity",
            "bob",
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

    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0]["msg_id"], "msg-new");
    assert_eq!(messages[1]["msg_id"], "msg-plain");
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

fn register_sync_v2_identity(workspace: &Path, handle: &str) -> (String, String) {
    let register = awiki_cmd(
        &[
            "id",
            "register",
            "--handle",
            handle,
            "--phone",
            "13800138000",
            "--otp",
            "123456",
        ],
        workspace,
    );
    let registered = success_json(&register);
    let identity = &registered["data"]["identity"];
    (
        identity["unique_id"]
            .as_str()
            .expect("registered identity ID")
            .to_owned(),
        identity["did"].as_str().expect("registered DID").to_owned(),
    )
}

fn register_generated_msg_identity(
    workspace: &Path,
    identity_name: &str,
    handle: &str,
    jwt_token: &str,
) -> TestIdentity {
    let identity = write_ready_identity(
        workspace,
        TestIdentityOptions {
            identity_name,
            handle,
            display_name: identity_name,
            jwt_token,
            make_default: true,
        },
    );
    migrate_identity_to_vault(workspace);
    identity
}

fn migrate_identity_to_vault(workspace: &Path) {
    let output = awiki_cmd(&["--migration", "id", "vault", "migrate"], workspace);
    assert_eq!(
        output.status.code(),
        Some(0),
        "vault migration failed; stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn write_msg_config(workspace: &Path, base_url: &str) {
    write_default_tenant_registry(workspace, base_url, "awiki.ai");
    write_tenant_config(workspace, "runtime:\n  mode: http\n");
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
            "UPDATE contact_handle_bindings SET is_current = 0 WHERE owner_identity_id = '{owner_identity_id}' AND handle = '{handle}' AND did <> '{peer_did}'; INSERT INTO contact_handle_bindings (owner_identity_id, owner_did, handle, did, is_current, first_seen_at, last_seen_at, credential_name) VALUES ('{owner_identity_id}', '{owner_did}', '{handle}', '{peer_did}', 1, '{seen_at}', '{seen_at}', 'bob-msg') ON CONFLICT(owner_identity_id, handle, did) DO UPDATE SET is_current = excluded.is_current, last_seen_at = excluded.last_seen_at, credential_name = excluded.credential_name",
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

#[allow(
    clippy::too_many_arguments,
    reason = "test fixture mirrors one message row"
)]
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

    fn registration() -> Self {
        Self::ok("__DYNAMIC_REGISTRATION_RESPONSE__")
    }

    fn prekey_publication() -> Self {
        Self::ok("__DYNAMIC_PREKEY_PUBLICATION_RESPONSE__")
    }

    fn sync_bootstrap() -> Self {
        Self::ok("__DYNAMIC_SYNC_BOOTSTRAP_RESPONSE__")
    }

    fn sync_delta_direct() -> Self {
        Self::ok("__DYNAMIC_SYNC_DELTA_DIRECT_RESPONSE__")
    }

    fn sync_delta_empty() -> Self {
        Self::ok("__DYNAMIC_SYNC_DELTA_EMPTY_RESPONSE__")
    }

    fn message_batch() -> Self {
        Self::ok("__DYNAMIC_MESSAGE_BATCH_RESPONSE__")
    }

    fn directory_lookup() -> Self {
        Self::ok("__DYNAMIC_DIRECTORY_LOOKUP_RESPONSE__")
    }

    fn mark_read_state() -> Self {
        Self::ok("__DYNAMIC_MARK_READ_STATE_RESPONSE__")
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

fn dynamic_response_body(request: &str, marker: &str) -> String {
    match marker {
        "__DYNAMIC_REGISTRATION_RESPONSE__" => registration_response(request),
        "__DYNAMIC_PREKEY_PUBLICATION_RESPONSE__" => prekey_publication_response(request),
        "__DYNAMIC_SYNC_BOOTSTRAP_RESPONSE__" => {
            let binding = device_binding_from_request(request);
            rpc_result_for_request(
                request,
                json!({
                    "mode": "tail_only",
                    "account_id": binding.account_id,
                    "device_id": binding.device_id,
                    "server_time": "2026-08-02T00:00:00Z",
                    "cursor": {"stream_epoch": "1", "scan_seq": "0"},
                    "read_state_baseline": [],
                    "group_state_baseline": [],
                    "warnings": []
                }),
            )
        }
        "__DYNAMIC_SYNC_DELTA_DIRECT_RESPONSE__" => {
            let binding = device_binding_from_request(request);
            rpc_result_for_request(
                request,
                json!({
                    "mode": "delta",
                    "server_time": "2026-08-02T00:00:01Z",
                    "events": [{
                        "event_id": "event-direct-1",
                        "stream_epoch": "1",
                        "event_seq": "1",
                        "event_type": "message.created",
                        "schema_version": 1,
                        "ignore_safe": false,
                        "account_id": binding.account_id,
                        "recipient_device_id": null,
                        "origin_did": "did:wba:awiki.ai:alice:e1_alice",
                        "origin_device_id": "device-alice",
                        "aggregate_kind": "direct_message",
                        "aggregate_id": "msg-direct-1",
                        "state_version": null,
                        "thread_key": "remote-thread-alice",
                        "occurred_at": "2026-08-02T00:00:01Z",
                        "payload": {
                            "message_kind": "direct_plain",
                            "direction": "incoming",
                            "sender_did_snapshot": "did:wba:awiki.ai:alice:e1_alice",
                            "recipient_did_snapshot": binding.did,
                            "client_message_id": "msg-direct-1"
                        },
                        "source": {}
                    }],
                    "next_cursor": {"stream_epoch": "1", "scan_seq": "1"},
                    "has_more": false,
                    "recovery": null,
                    "warnings": []
                }),
            )
        }
        "__DYNAMIC_SYNC_DELTA_EMPTY_RESPONSE__" => rpc_result_for_request(
            request,
            json!({
                "mode": "delta",
                "server_time": "2026-08-02T00:00:02Z",
                "events": [],
                "next_cursor": {"stream_epoch": "1", "scan_seq": "1"},
                "has_more": false,
                "recovery": null,
                "warnings": []
            }),
        ),
        "__DYNAMIC_MESSAGE_BATCH_RESPONSE__" => {
            let binding = device_binding_from_request(request);
            rpc_result_for_request(
                request,
                json!({
                    "items": [{
                        "event_id": "event-direct-1",
                        "message": direct_message(&binding.did)
                    }],
                    "unavailable": []
                }),
            )
        }
        "__DYNAMIC_DIRECTORY_LOOKUP_RESPONSE__" => rpc_result_for_request(
            request,
            json!({
                "did": "did:wba:awiki.ai:alice:e1_alice",
                "user_id": "user-alice",
                "handle": "alice",
                "full_handle": "alice.awiki.ai",
                "domain": "awiki.ai",
                "status": "active",
                "binding_generation": "1"
            }),
        ),
        "__DYNAMIC_MARK_READ_STATE_RESPONSE__" => {
            let rpc: Value = serde_json::from_str(request_body(request)).expect("mark-read RPC");
            let body = &rpc["params"]["body"];
            rpc_result_for_request(
                request,
                json!({
                    "user_did": body["user_did"].clone(),
                    "thread": body["thread"].clone(),
                    "updated_count": 1,
                    "remote_acknowledged": true,
                    "partial": false,
                    "fallback_used": false,
                    "pending_remote_ack": false,
                    "read_watermark_server_seq": body["read_up_to_server_seq"].clone(),
                    "previous_read_watermark_server_seq": null,
                    "read_watermark_message_id": body["read_up_to_message_id"].clone(),
                    "advanced": true,
                    "read_at": "2026-08-02T00:00:03Z",
                    "unread_count": 0,
                    "warnings": []
                }),
            )
        }
        body => body.to_owned(),
    }
}

fn direct_message(receiver_did: &str) -> Value {
    json!({
        "id": "msg-direct-1",
        "type": "text",
        "thread_kind": "direct",
        "sender_did": "did:wba:awiki.ai:alice:e1_alice",
        "receiver_did": receiver_did,
        "content_type": "text/plain",
        "content": "hello bob",
        "server_seq": "1",
        "created_at": "2026-08-02T00:00:01Z",
        "sent_at": "2026-08-02T00:00:01Z",
        "client_msg_id": "msg-direct-1",
        "is_read": false,
        "peer_user_id": "user-alice",
        "peer_full_handle": "alice.awiki.ai"
    })
}

fn registration_response(request: &str) -> String {
    let rpc: Value =
        serde_json::from_str(request_body(request)).expect("registration request JSON");
    let params = &rpc["params"];
    let document = &params["did_document"];
    let did = document["id"].as_str().expect("registration DID");
    let device = &document["deviceManifest"]["devices"][0];
    let device_id = device["device_id"]
        .as_str()
        .expect("registration device ID");
    let key_id = device["signing_key_id"]
        .as_str()
        .expect("registration signing key ID");
    let handle = params["handle"].as_str().expect("registration handle");
    let account_id = format!("user-{handle}");
    json!({
        "jsonrpc": "2.0",
        "result": {
            "state": "registered",
            "did": did,
            "user_id": account_id,
            "message": "Registration successful",
            "access_token": device_access_token(did, &account_id, device_id, key_id),
            "handle": handle,
            "domain": "awiki.ai",
            "full_handle": format!("{handle}.awiki.ai"),
            "binding_generation": "1"
        },
        "id": rpc["id"].clone()
    })
    .to_string()
}

fn prekey_publication_response(request: &str) -> String {
    let rpc: Value = serde_json::from_str(request_body(request)).expect("prekey request JSON");
    let bundle = &rpc["params"]["body"]["prekey_bundle"];
    let published_opk_count = rpc["params"]["body"]["one_time_prekeys"]
        .as_array()
        .map(Vec::len)
        .expect("one-time prekeys");
    rpc_result_for_request(
        request,
        json!({
            "published": true,
            "owner_did": bundle["owner_did"].clone(),
            "owner_device_id": bundle["owner_device_id"].clone(),
            "bundle_id": bundle["bundle_id"].clone(),
            "published_at": "2026-08-02T00:00:00Z",
            "published_opk_count": published_opk_count
        }),
    )
}

fn rpc_result_for_request(request: &str, result: Value) -> String {
    let rpc: Value = serde_json::from_str(request_body(request)).expect("RPC request JSON");
    json!({"jsonrpc": "2.0", "result": result, "id": rpc["id"].clone()}).to_string()
}

struct DeviceBinding {
    did: String,
    account_id: String,
    device_id: String,
}

fn device_binding_from_request(request: &str) -> DeviceBinding {
    let token = request
        .lines()
        .find_map(|line| {
            line.split_once(':').and_then(|(name, value)| {
                name.trim()
                    .eq_ignore_ascii_case("authorization")
                    .then(|| value.trim().strip_prefix("Bearer ").map(str::to_owned))
                    .flatten()
            })
        })
        .expect("device bearer token");
    let payload = token.split('.').nth(1).expect("JWT payload");
    let claims: Value = serde_json::from_slice(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(payload)
            .expect("decode JWT payload"),
    )
    .expect("JWT claims");
    DeviceBinding {
        did: claims["did"].as_str().expect("token did").to_owned(),
        account_id: claims["user_id"]
            .as_str()
            .expect("token account ID")
            .to_owned(),
        device_id: claims["device_id"]
            .as_str()
            .expect("token device ID")
            .to_owned(),
    }
}

fn device_access_token(did: &str, account_id: &str, device_id: &str, key_id: &str) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let claims = json!({
        "iss": "user-service",
        "aud": ["awiki-user-service", "awiki-message-service"],
        "sub": did,
        "type": "access",
        "purpose": "awiki.device.access.v1",
        "did": did,
        "user_id": account_id,
        "device_id": device_id,
        "key_id": key_id,
        "auth_generation": 1,
        "scopes": ["device:manage", "device:read", "message:connect"],
        "iat": now,
        "nbf": now,
        "exp": now + 3600,
        "jti": format!("msg-live-{device_id}")
    });
    format!(
        "e30.{}.signature",
        base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(serde_json::to_vec(&claims).expect("serialize device access token"))
    )
}

fn handle_connection(
    mut stream: TcpStream,
    requests: &Arc<Mutex<Vec<String>>>,
    response: TestResponse,
) {
    let request = read_http_request(&mut stream);
    let body = dynamic_response_body(&request, &response.body);
    requests.lock().expect("requests mutex").push(request);
    let body = body.as_bytes();
    let raw = format!(
        "HTTP/1.1 {} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        response.status,
        body.len(),
        String::from_utf8_lossy(body)
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
