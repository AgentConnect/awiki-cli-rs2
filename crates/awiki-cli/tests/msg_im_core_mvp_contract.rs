use base64::Engine;
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

mod support;

use support::{
    open_local_state, write_default_tenant_registry, write_ready_identity, write_tenant_config,
    TestIdentity, TestIdentityOptions,
};

static TEMP_DIR_COUNTER: AtomicU64 = AtomicU64::new(0);

#[test]
fn msg_send_default_cutover_direct_text_posts_im_core_rpc() {
    let workspace = TempDir::new().expect("workspace");
    let alice =
        register_generated_msg_identity(workspace.path(), "alice-cutover", "alice", "jwt-alice");
    let bob_did = "did:wba:awiki.ai:bob:e1_bob";
    let server = TestServer::new(vec![TestResponse::ok(&json_rpc_result(json!({
        "accepted": true,
        "final_acceptance": true,
        "accepted_at": "2026-05-21T00:00:00Z",
        "delivery_state": "accepted"
    })))]);
    write_msg_config(workspace.path(), &server.base_url());

    let output = awiki_cmd(
        &[
            "--identity",
            "alice-cutover",
            "msg",
            "send",
            "--to",
            bob_did,
            "--text",
            "hello through default cutover",
        ],
        workspace.path(),
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
    assert_contains_header(&requests[0], "Authorization", "Bearer jwt-alice");
    let body: Value = serde_json::from_str(request_body(&requests[0])).expect("request JSON");
    assert_eq!(body["method"], "direct.send");
    assert_eq!(body["params"]["meta"]["sender_did"], alice.did);
    assert_eq!(
        body["params"]["meta"]["target"],
        json!({"kind": "agent", "did": bob_did})
    );
    assert_eq!(
        body["params"]["body"],
        json!({"text": "hello through default cutover"})
    );
    assert_eq!(
        body["params"]["auth"]["scheme"],
        "anp-rfc9421-origin-proof-v1"
    );
}

#[test]
fn msg_send_default_cutover_group_text_posts_im_core_rpc() {
    let workspace = TempDir::new().expect("workspace");
    let alice = register_generated_msg_identity(
        workspace.path(),
        "alice-group-cutover",
        "alice",
        "jwt-alice",
    );
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

    let output = awiki_cmd(
        &[
            "--identity",
            "alice-group-cutover",
            "msg",
            "send",
            "--group",
            group_did,
            "--text",
            "hello group through default cutover",
        ],
        workspace.path(),
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
    assert_contains_header(&requests[0], "Authorization", "Bearer jwt-alice");
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
        json!({"text": "hello group through default cutover"})
    );
    assert_eq!(
        body["params"]["auth"]["scheme"],
        "anp-rfc9421-origin-proof-v1"
    );
}

#[test]
fn msg_inbox_foreground_reconciles_sync_v2_without_hint_and_reads_exact_local_projection() {
    let workspace = TempDir::new().expect("workspace");
    let server = TestServer::new(vec![
        TestResponse::registration(),
        TestResponse::sync_bootstrap(),
        TestResponse::sync_delta_message(),
        TestResponse::message_batch(),
        TestResponse::sync_delta_empty(),
        TestResponse::error(503, r#"{"error":"temporarily unavailable"}"#),
    ]);
    write_msg_config(workspace.path(), &server.base_url());
    let register = awiki_cmd(
        &[
            "id",
            "register",
            "--handle",
            "alice",
            "--phone",
            "13800138000",
            "--otp",
            "123456",
        ],
        workspace.path(),
    );
    let registered = success_json(&register);
    let alice_did = registered["data"]["identity"]["did"]
        .as_str()
        .expect("registered DID");

    let first = awiki_cmd_without_direct_e2ee(
        &[
            "--identity",
            "alice",
            "msg",
            "inbox",
            "--scope",
            "all",
            "--limit",
            "3",
            "--unread",
        ],
        workspace.path(),
    );
    let envelope = success_json(&first);
    assert_eq!(envelope["summary"], "Loaded 1 inbox messages");
    assert_eq!(
        envelope["data"]["messages"][0]["id"],
        "did:wba:awiki.ai:groups:sync:e1_group:1"
    );
    assert_eq!(envelope["data"]["source"], "local_projection");
    assert_eq!(
        envelope["data"]
            .as_object()
            .expect("inbox data")
            .keys()
            .map(String::as_str)
            .collect::<std::collections::BTreeSet<_>>(),
        ["messages", "source", "total"].into_iter().collect()
    );
    assert_eq!(envelope["data"]["messages"][0]["receiver_did"], alice_did);

    let second = awiki_cmd_without_direct_e2ee(
        &[
            "--identity",
            "alice",
            "msg",
            "inbox",
            "--scope",
            "all",
            "--limit",
            "3",
            "--unread",
        ],
        workspace.path(),
    );
    let second_envelope = success_json(&second);
    assert_eq!(
        second_envelope["data"]["messages"],
        envelope["data"]["messages"]
    );
    assert_eq!(second_envelope["data"]["total"], 1);
    let connection = open_local_state(workspace.path());
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM messages WHERE msg_id = 'did:wba:awiki.ai:groups:sync:e1_group:1'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        1,
        "reconciliation must project the logical message exactly once"
    );

    let failed = awiki_cmd_without_direct_e2ee(
        &["--identity", "alice", "msg", "inbox", "--scope", "all"],
        workspace.path(),
    );
    assert_ne!(failed.status.code(), Some(0));
    assert!(
        !String::from_utf8_lossy(&failed.stdout)
            .contains("did:wba:awiki.ai:groups:sync:e1_group:1"),
        "a failed foreground reconcile must not return a stale local projection"
    );

    let requests = server.requests();
    let rpc_bodies = requests
        .iter()
        .map(|request| serde_json::from_str::<Value>(request_body(request)).expect("request JSON"))
        .collect::<Vec<_>>();
    let methods = rpc_bodies
        .iter()
        .filter_map(|body| body["method"].as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        methods,
        [
            "register",
            "direct.e2ee.publish_prekey_bundle",
            "sync.bootstrap",
            "sync.delta",
            "message.get_batch",
            "sync.delta",
            "sync.delta",
        ]
    );
    assert_eq!(
        methods
            .iter()
            .filter(|method| **method == "inbox.get")
            .count(),
        0
    );
    for body in rpc_bodies
        .iter()
        .filter(|body| body["method"] == "sync.delta")
    {
        assert_eq!(body["params"]["body"]["reason"], "foreground_reconcile");
    }
}

#[test]
fn msg_inbox_reconciles_secure_exact_device_rows_without_ordinary_inbox_fallback() {
    let workspace = TempDir::new().expect("workspace");
    let server = TestServer::new(vec![
        TestResponse::registration(),
        TestResponse::ok(&json_rpc_result(json!({
            "messages": [{
                "id": "msg-ordinary-injected-by-secure-inbox",
                "sender_did": "did:wba:awiki.ai:user:bob:e1_bob",
                "receiver_did": "did:wba:awiki.ai:user:alice:e1_alice",
                "content_type": "text/plain",
                "content": "must not bypass sync v2",
                "server_seq": 99
            }],
            "has_more": false,
            "warnings": []
        }))),
        TestResponse::sync_bootstrap(),
        TestResponse::sync_delta_empty(),
    ]);
    write_msg_config(workspace.path(), &server.base_url());
    success_json(&awiki_cmd(
        &[
            "id",
            "register",
            "--handle",
            "alice",
            "--phone",
            "13800138000",
            "--otp",
            "123456",
        ],
        workspace.path(),
    ));

    let output = awiki_cmd_with_direct_e2ee(
        &[
            "--identity",
            "alice",
            "msg",
            "inbox",
            "--scope",
            "direct",
            "--limit",
            "7",
        ],
        workspace.path(),
    );
    let envelope = success_json(&output);
    assert_eq!(envelope["data"]["messages"], json!([]));
    assert_eq!(envelope["data"]["source"], "local_projection");

    let rpc_bodies = server
        .requests()
        .iter()
        .map(|request| serde_json::from_str::<Value>(request_body(request)).unwrap())
        .collect::<Vec<_>>();
    let methods = rpc_bodies
        .iter()
        .filter_map(|body| body["method"].as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        methods,
        [
            "register",
            "direct.e2ee.publish_prekey_bundle",
            "inbox.get",
            "sync.bootstrap",
            "sync.delta"
        ]
    );
    let secure = rpc_bodies
        .iter()
        .find(|body| body["method"] == "inbox.get")
        .unwrap();
    assert_eq!(secure["params"]["body"]["limit"], 7);
    assert_eq!(secure["params"]["body"]["security_profile"], "direct-e2ee");
    assert_eq!(secure["params"]["body"].as_object().unwrap().len(), 3);

    let connection = open_local_state(workspace.path());
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM messages WHERE msg_id = 'msg-ordinary-injected-by-secure-inbox'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        0,
        "secure hydration must not become an ordinary Inbox compatibility path"
    );
}

#[test]
fn msg_mark_read_with_sync_v2_binding_writes_thread_read_state() {
    let workspace = TempDir::new().expect("workspace");
    let server = TestServer::new(vec![
        TestResponse::registration(),
        TestResponse::sync_bootstrap(),
        TestResponse::sync_delta_message(),
        TestResponse::message_batch(),
        TestResponse::mark_read_state(),
        TestResponse::sync_delta_empty(),
        TestResponse::sync_delta_empty(),
    ]);
    write_msg_config(workspace.path(), &server.base_url());
    let register = awiki_cmd(
        &[
            "id",
            "register",
            "--handle",
            "alice",
            "--phone",
            "13800138000",
            "--otp",
            "123456",
        ],
        workspace.path(),
    );
    success_json(&register);
    let inbox = awiki_cmd_without_direct_e2ee(
        &[
            "--identity",
            "alice",
            "msg",
            "inbox",
            "--scope",
            "all",
            "--limit",
            "3",
        ],
        workspace.path(),
    );
    let inbox = success_json(&inbox);
    let message_id = inbox["data"]["messages"][0]["id"]
        .as_str()
        .expect("projected message id");

    let mark_read = awiki_cmd_without_direct_e2ee(
        &["--identity", "alice", "msg", "mark-read", message_id],
        workspace.path(),
    );
    let envelope = success_json(&mark_read);

    assert_eq!(envelope["summary"], "Marked 1 messages as read");
    assert_eq!(envelope["data"]["action"], "mark_read");
    assert_eq!(envelope["data"]["updated_count"], 1);
    assert_eq!(envelope["data"]["message_ids"], json!([message_id]));
    let methods = server
        .requests()
        .iter()
        .map(|request| serde_json::from_str::<Value>(request_body(request)).unwrap())
        .filter_map(|body| body["method"].as_str().map(str::to_owned))
        .collect::<Vec<_>>();
    assert_eq!(
        methods.last().map(String::as_str),
        Some("read_state.mark_read")
    );
    assert!(!methods.iter().any(|method| method == "inbox.mark_read"));
    let connection = open_local_state(workspace.path());
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM thread_read_state WHERE read_watermark_seq = '1'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        1
    );

    let refreshed = awiki_cmd_without_direct_e2ee(
        &[
            "--identity",
            "alice",
            "msg",
            "inbox",
            "--scope",
            "all",
            "--limit",
            "3",
        ],
        workspace.path(),
    );
    let refreshed = success_json(&refreshed);
    assert_eq!(refreshed["data"]["messages"][0]["is_read"], true);

    let unread = awiki_cmd_without_direct_e2ee(
        &[
            "--identity",
            "alice",
            "msg",
            "inbox",
            "--scope",
            "all",
            "--limit",
            "3",
            "--unread",
        ],
        workspace.path(),
    );
    let unread = success_json(&unread);
    assert_eq!(unread["data"]["messages"], json!([]));
    assert_eq!(unread["data"]["total"], 0);
}

#[test]
fn msg_history_default_cutover_direct_reconciles_sync_v2_and_reads_local_history() {
    let workspace = TempDir::new().expect("workspace");
    let bob_did = "did:wba:awiki.ai:bob:e1_bob";
    let server = TestServer::new(vec![
        TestResponse::registration(),
        TestResponse::sync_bootstrap(),
        TestResponse::sync_delta_empty(),
    ]);
    write_msg_config(workspace.path(), &server.base_url());
    let register = awiki_cmd(
        &[
            "id",
            "register",
            "--handle",
            "alice",
            "--phone",
            "13800138000",
            "--otp",
            "123456",
        ],
        workspace.path(),
    );
    let registered = success_json(&register);
    let alice_did = registered["data"]["identity"]["did"]
        .as_str()
        .expect("registered DID");
    let alice_identity_id = registered["data"]["identity"]["unique_id"]
        .as_str()
        .expect("registered identity ID");
    seed_message(
        workspace.path(),
        alice_identity_id,
        alice_did,
        "msg-history-cutover-1",
        "",
        "text/plain",
        "",
    );

    let output = awiki_cmd(
        &[
            "--identity",
            "alice",
            "msg",
            "history",
            "--with",
            bob_did,
            "--limit",
            "4",
        ],
        workspace.path(),
    );

    let envelope = success_json(&output);
    assert_eq!(envelope["summary"], "Loaded 1 direct history messages");
    assert_eq!(
        envelope["data"]["messages"][0]["id"],
        "msg-history-cutover-1"
    );
    assert_eq!(envelope["data"]["with"], bob_did);
    assert_eq!(envelope["data"]["source"], "local");
    assert_eq!(envelope["data"]["messages"][0]["direction"], 0);

    let requests = server.requests();
    let methods = requests
        .iter()
        .map(|request| {
            serde_json::from_str::<Value>(request_body(request)).expect("request JSON")["method"]
                .as_str()
                .expect("RPC method")
                .to_owned()
        })
        .collect::<Vec<_>>();
    assert_eq!(
        methods,
        [
            "register",
            "direct.e2ee.publish_prekey_bundle",
            "sync.bootstrap",
            "sync.delta",
        ]
    );
    assert!(!methods.iter().any(|method| method == "direct.get_history"));
}

#[test]
fn msg_history_default_cutover_group_reconciles_sync_v2_and_reads_local_history() {
    let workspace = TempDir::new().expect("workspace");
    let group_did = "did:wba:awiki.ai:groups:demo:e1_group";
    let server = TestServer::new(vec![
        TestResponse::registration(),
        TestResponse::sync_bootstrap(),
        TestResponse::sync_delta_empty(),
    ]);
    write_msg_config(workspace.path(), &server.base_url());
    let register = awiki_cmd(
        &[
            "id",
            "register",
            "--handle",
            "alice",
            "--phone",
            "13800138000",
            "--otp",
            "123456",
        ],
        workspace.path(),
    );
    let registered = success_json(&register);
    let alice_did = registered["data"]["identity"]["did"]
        .as_str()
        .expect("registered DID");
    let alice_identity_id = registered["data"]["identity"]["unique_id"]
        .as_str()
        .expect("registered identity ID");
    seed_message(
        workspace.path(),
        alice_identity_id,
        alice_did,
        "msg-group-history-cutover-1",
        group_did,
        "text/plain",
        "",
    );

    let output = awiki_cmd(
        &[
            "--identity",
            "alice",
            "msg",
            "history",
            "--group",
            group_did,
            "--limit",
            "6",
        ],
        workspace.path(),
    );

    let envelope = success_json(&output);
    assert_eq!(envelope["summary"], "Loaded 1 group history messages");
    assert_eq!(
        envelope["data"]["messages"][0]["id"],
        "msg-group-history-cutover-1"
    );
    assert_eq!(envelope["data"]["messages"][0]["group_did"], group_did);
    assert_eq!(envelope["data"]["group"], group_did);
    assert_eq!(envelope["data"]["source"], "local");

    let requests = server.requests();
    let methods = requests
        .iter()
        .map(|request| {
            serde_json::from_str::<Value>(request_body(request)).expect("request JSON")["method"]
                .as_str()
                .expect("RPC method")
                .to_owned()
        })
        .collect::<Vec<_>>();
    assert_eq!(
        methods,
        [
            "register",
            "direct.e2ee.publish_prekey_bundle",
            "sync.bootstrap",
            "sync.delta",
        ]
    );
    assert!(!methods.iter().any(|method| method == "group.list_messages"));
}

#[test]
fn msg_mark_read_default_cutover_posts_im_core_rpc_and_updates_local_cache() {
    let workspace = TempDir::new().expect("workspace");
    let alice = register_generated_read_identity(
        workspace.path(),
        "alice-mark-cutover",
        "alice",
        "jwt-alice",
    );
    let server = TestServer::new(vec![TestResponse::ok(&json_rpc_result(json!({
        "updated_count": 4
    })))]);
    write_msg_config(workspace.path(), &server.base_url());
    seed_message(
        workspace.path(),
        &alice.unique_id,
        &alice.did,
        "direct-mark-cutover",
        "",
        "text/plain",
        "",
    );

    let output = awiki_cmd(
        &[
            "--identity",
            "alice-mark-cutover",
            "msg",
            "mark-read",
            "direct-mark-cutover",
        ],
        workspace.path(),
    );

    let envelope = success_json(&output);
    assert_eq!(envelope["summary"], "Marked 4 messages as read");
    assert_eq!(envelope["data"]["action"], "mark_read");
    assert_eq!(envelope["data"]["updated_count"], 4);
    assert_eq!(
        envelope["data"]["message_ids"],
        json!(["direct-mark-cutover"])
    );
    assert_eq!(
        is_read(workspace.path(), &alice.unique_id, "direct-mark-cutover"),
        1
    );

    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].starts_with("POST /im/rpc HTTP/1.1"));
    assert_contains_header(&requests[0], "Authorization", "Bearer jwt-alice");
    let body: Value = serde_json::from_str(request_body(&requests[0])).expect("request JSON");
    assert_eq!(body["method"], "inbox.mark_read");
    assert_eq!(body["params"]["meta"]["sender_did"], alice.did);
    assert_eq!(body["params"]["body"]["user_did"], alice.did);
    assert_eq!(
        body["params"]["body"]["message_ids"],
        json!(["direct-mark-cutover"])
    );
}

#[test]
fn msg_mark_read_default_cutover_keeps_group_and_mail_local_only() {
    let workspace = TempDir::new().expect("workspace");
    let alice = register_generated_read_identity(
        workspace.path(),
        "alice-local-mark-cutover",
        "alice",
        "jwt-alice",
    );
    let server = TestServer::new(Vec::new());
    write_msg_config(workspace.path(), &server.base_url());
    seed_message(
        workspace.path(),
        &alice.unique_id,
        &alice.did,
        "group-mark-cutover",
        "did:wba:awiki.ai:groups:demo:e1_group",
        "text/plain",
        "",
    );
    seed_message(
        workspace.path(),
        &alice.unique_id,
        &alice.did,
        "mail-mark-cutover",
        "",
        "mail.notification",
        r#"{"source_kind":"mail"}"#,
    );

    let output = awiki_cmd(
        &[
            "--identity",
            "alice-local-mark-cutover",
            "msg",
            "mark-read",
            "group-mark-cutover",
            "mail-mark-cutover",
        ],
        workspace.path(),
    );

    let envelope = success_json(&output);
    assert_eq!(envelope["summary"], "Marked 2 messages as read");
    assert_eq!(envelope["data"]["updated_count"], 2);
    assert_eq!(server.requests().len(), 0);
    assert_eq!(
        is_read(workspace.path(), &alice.unique_id, "group-mark-cutover"),
        1
    );
    assert_eq!(
        is_read(workspace.path(), &alice.unique_id, "mail-mark-cutover"),
        1
    );
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
    command.output().expect("run awiki-cli")
}

fn awiki_cmd_without_direct_e2ee(args: &[&str], workspace: &Path) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_awiki-cli"));
    command
        .args(args)
        .env("AWIKI_CLI_WORKSPACE_HOME_DIR", workspace)
        .env("HOME", workspace.join("home"))
        .env("USERPROFILE", workspace.join("home"))
        .env("AWIKI_CLI_UPDATE_CACHE_ONLY", "1")
        .env("AWIKI_MULTI_DEVICE_DIRECT_E2EE_ENABLED", "0")
        .env_remove("AWIKI_WORKSPACE")
        .env_remove("AWIKI_WORKSPACE_HOME")
        .env_remove("AWIKI_HOME")
        .env_remove("AVIKI_WORKSPACE_HOME")
        .env_remove("AWIKI_FORMAT")
        .env_remove("AVIKI_FORMAT");
    command.output().expect("run awiki-cli")
}

fn awiki_cmd_with_direct_e2ee(args: &[&str], workspace: &Path) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_awiki-cli"));
    command
        .args(args)
        .env("AWIKI_CLI_WORKSPACE_HOME_DIR", workspace)
        .env("HOME", workspace.join("home"))
        .env("USERPROFILE", workspace.join("home"))
        .env("AWIKI_CLI_UPDATE_CACHE_ONLY", "1")
        .env("AWIKI_MULTI_DEVICE_DIRECT_E2EE_ENABLED", "1")
        .env_remove("AWIKI_WORKSPACE")
        .env_remove("AWIKI_WORKSPACE_HOME")
        .env_remove("AWIKI_HOME")
        .env_remove("AVIKI_WORKSPACE_HOME")
        .env_remove("AWIKI_FORMAT")
        .env_remove("AVIKI_FORMAT");
    command.output().expect("run awiki-cli")
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

fn register_generated_read_identity(
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
    write_tenant_config(workspace, "schema_version: 1\nruntime:\n  mode: http\n");
}

fn seed_message(
    workspace: &Path,
    owner_identity_id: &str,
    owner_did: &str,
    message_id: &str,
    group_did: &str,
    content_type: &str,
    metadata: &str,
) {
    let connection = open_local_state(workspace);
    connection
        .execute(
            r#"
	INSERT INTO messages
	    (msg_id, owner_identity_id, owner_did, conversation_id, thread_id, direction, sender_did, receiver_did, group_id, group_did,
	     content_type, content, stored_at, metadata, is_read)
	VALUES (?1, ?2, ?3, ?4, ?4, 0, 'did:wba:awiki.ai:bob:e1_bob', ?3, ?5, ?5, ?6, 'hello',
	        '2026-05-21T00:00:00Z', ?7, 0)"#,
            (
                message_id,
                owner_identity_id,
                owner_did,
                conversation_id_for_fixture(message_id, group_did, content_type, metadata),
                group_did,
                content_type,
                metadata,
            ),
        )
        .unwrap();
}

fn conversation_id_for_fixture(
    _message_id: &str,
    group_did: &str,
    content_type: &str,
    metadata: &str,
) -> String {
    if !group_did.is_empty() {
        format!("group:{group_did}")
    } else if content_type == "mail.notification" || metadata.contains(r#""source_kind":"mail""#) {
        "mail:alice@awiki.ai".to_string()
    } else {
        "dm:did:wba:awiki.ai:bob:e1_bob".to_string()
    }
}

fn is_read(workspace: &Path, owner_identity_id: &str, message_id: &str) -> i64 {
    let connection = open_local_state(workspace);
    connection
        .query_row(
            "SELECT is_read FROM messages WHERE owner_identity_id = ?1 AND msg_id = ?2",
            (owner_identity_id, message_id),
            |row| row.get(0),
        )
        .unwrap()
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

    fn error(status: u16, body: &str) -> Self {
        Self {
            status,
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

    fn sync_delta_message() -> Self {
        Self::ok("__DYNAMIC_SYNC_DELTA_MESSAGE_RESPONSE__")
    }

    fn message_batch() -> Self {
        Self::ok("__DYNAMIC_MESSAGE_BATCH_RESPONSE__")
    }

    fn sync_delta_empty() -> Self {
        Self::ok("__DYNAMIC_SYNC_DELTA_EMPTY_RESPONSE__")
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
                let follows_with_prekey = response.body == "__DYNAMIC_REGISTRATION_RESPONSE__";
                let Some(stream) = accept_with_timeout(&listener) else {
                    break;
                };
                handle_connection(stream, &server_requests, response);
                if follows_with_prekey {
                    let Some(stream) = accept_with_timeout(&listener) else {
                        break;
                    };
                    handle_connection(stream, &server_requests, TestResponse::prekey_publication());
                }
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
    // Real CLI registration and Core bootstrap can contend with sibling
    // workspace tests before the first request reaches this local fixture.
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
        "__DYNAMIC_SYNC_DELTA_MESSAGE_RESPONSE__" => {
            let binding = device_binding_from_request(request);
            rpc_result_for_request(
                request,
                json!({
                    "mode": "delta",
                    "server_time": "2026-08-02T00:00:01Z",
                    "events": [{
                        "event_id": "event-sync-v2-1",
                        "stream_epoch": "1",
                        "event_seq": "1",
                        "event_type": "message.created",
                        "schema_version": 1,
                        "ignore_safe": false,
                        "account_id": binding.account_id,
                        "recipient_device_id": null,
                        "origin_did": "did:wba:awiki.ai:user:bob:e1_bob",
                        "origin_device_id": "device-bob",
                        "aggregate_kind": "group_message",
                        "aggregate_id": "msg-sync-v2-1",
                        "state_version": null,
                        "thread_key": "did:wba:awiki.ai:groups:sync:e1_group",
                        "occurred_at": "2026-08-02T00:00:01Z",
                        "payload": {
                            "message_kind": "group_plain",
                            "direction": "incoming",
                            "group_did": "did:wba:awiki.ai:groups:sync:e1_group",
                            "sender_did_snapshot": "did:wba:awiki.ai:user:bob:e1_bob",
                            "recipient_did_snapshot": binding.did,
                            "client_message_id": "msg-sync-v2-1"
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
        "__DYNAMIC_MESSAGE_BATCH_RESPONSE__" => {
            let binding = device_binding_from_request(request);
            rpc_result_for_request(
                request,
                json!({
                    "items": [{
                        "event_id": "event-sync-v2-1",
                        "message": {
                            "id": "msg-sync-v2-1",
                            "thread_kind": "group",
                            "group_did": "did:wba:awiki.ai:groups:sync:e1_group",
                            "sender_did": "did:wba:awiki.ai:user:bob:e1_bob",
                            "receiver_did": binding.did,
                            "content_type": "text/plain",
                            "content": "hello from foreground reconcile",
                            "server_seq": "1",
                            "created_at": "2026-08-02T00:00:01Z",
                            "client_msg_id": "msg-sync-v2-1"
                        }
                    }],
                    "unavailable": []
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

fn registration_response(request: &str) -> String {
    let rpc: Value =
        serde_json::from_str(request_body(request)).expect("registration request JSON");
    let params = &rpc["params"];
    let document = &params["did_document"];
    let did = document["id"].as_str().expect("registration DID");
    let device = &document["deviceManifest"]["devices"][0];
    let device_id = device["device_id"]
        .as_str()
        .expect("registration device_id");
    let key_id = device["signing_key_id"]
        .as_str()
        .expect("registration signing_key_id");
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
        .expect("one_time_prekeys");
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
            .expect("token user_id")
            .to_owned(),
        device_id: claims["device_id"]
            .as_str()
            .expect("token device_id")
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
        "jti": format!("msg-sync-v2-{device_id}")
    });
    format!(
        "e30.{}.signature",
        base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(serde_json::to_vec(&claims).expect("serialize test access token"))
    )
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
    let request = String::from_utf8_lossy(&raw).into_owned();
    let body = dynamic_response_body(&request, &response.body);
    requests.lock().expect("requests mutex").push(request);
    let reason = if response.status == 200 { "OK" } else { "ERR" };
    let body = body.as_bytes();
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

fn assert_contains_header(haystack: &str, header_name: &str, expected_value: &str) {
    assert!(
        haystack.lines().any(|line| {
            line.split_once(':').is_some_and(|(name, value)| {
                name.trim().eq_ignore_ascii_case(header_name) && value.trim() == expected_value
            })
        }),
        "missing {header_name}: {expected_value}:\n{haystack}"
    );
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
        let counter = TEMP_DIR_COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "awiki-cli-rs2-msg-im-core-cutover-test-{}-{counter}-{nanos}",
            std::process::id(),
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
