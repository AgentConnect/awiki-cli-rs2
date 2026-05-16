#![cfg(unix)]

use awiki_cli::runtime::bridge::{self, BridgeRequest};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[test]
fn msg_inbox_default_scope_all_merges_local_direct_group_and_mail_like_go() {
    let workspace = TempDir::new("msg-all-inbox-merge").expect("workspace");
    register_ready_msg_identity(workspace.path(), "alice-all-inbox", "alice", "jwt-alice");
    let alice_did = "did:wba:awiki.ai:alice:e1_alice";
    seed_direct_message(
        workspace.path(),
        alice_did,
        "did:wba:awiki.ai:bob:e1_bob",
        "direct-1",
        "hello direct",
        "2026-05-16T10:00:00Z",
        false,
    );
    seed_group_message(
        workspace.path(),
        alice_did,
        "did:wba:awiki.ai:groups:demo:e1_group",
        "group-1",
        "hello group",
        "2026-05-16T10:01:00Z",
        false,
    );
    seed_mail_notification(
        workspace.path(),
        alice_did,
        "mail-1",
        "mail raw",
        "2026-05-16T10:02:00Z",
        false,
    );
    write_msg_ws_config(workspace.path(), "https://placeholder.invalid");

    let output = awiki_cmd(
        &[
            "--identity",
            "alice-all-inbox",
            "msg",
            "inbox",
            "--limit",
            "10",
        ],
        workspace.path(),
    );

    assert_success(&output);
    let envelope = success_json(&output);
    assert_eq!(envelope["summary"], "Loaded 3 inbox messages");
    assert_eq!(
        envelope["data"]["source"],
        "local_direct_cache+local_group_cache"
    );
    assert_eq!(envelope["data"]["total"], 3);
    assert_eq!(message_id(&envelope["data"]["messages"][0]), "mail-1");
    assert_eq!(envelope["data"]["messages"][0]["source_kind"], "mail");
    assert_eq!(
        envelope["data"]["messages"][0]["title"],
        "[邮件] Mail subject"
    );
    assert_contains_text(
        envelope["data"]["messages"][0]["content"]
            .as_str()
            .expect("mail content"),
        "发件人: sender@example.com",
    );
    assert_eq!(message_id(&envelope["data"]["messages"][1]), "group-1");
    assert_eq!(message_id(&envelope["data"]["messages"][2]), "direct-1");
}

#[test]
fn msg_inbox_default_scope_all_unread_filters_local_sources_like_go() {
    let workspace = TempDir::new("msg-all-inbox-unread").expect("workspace");
    register_ready_msg_identity(workspace.path(), "alice-all-unread", "alice", "jwt-alice");
    let alice_did = "did:wba:awiki.ai:alice:e1_alice";
    seed_direct_message(
        workspace.path(),
        alice_did,
        "did:wba:awiki.ai:bob:e1_bob",
        "direct-read",
        "already read",
        "2026-05-16T10:00:00Z",
        true,
    );
    seed_group_message(
        workspace.path(),
        alice_did,
        "did:wba:awiki.ai:groups:demo:e1_group",
        "group-unread",
        "unread group",
        "2026-05-16T10:01:00Z",
        false,
    );
    seed_mail_notification(
        workspace.path(),
        alice_did,
        "mail-read",
        "read mail",
        "2026-05-16T10:02:00Z",
        true,
    );
    write_msg_ws_config(workspace.path(), "https://placeholder.invalid");

    let output = awiki_cmd(
        &[
            "--identity",
            "alice-all-unread",
            "msg",
            "inbox",
            "--unread",
            "--limit",
            "10",
        ],
        workspace.path(),
    );

    assert_success(&output);
    let envelope = success_json(&output);
    assert_eq!(envelope["summary"], "Loaded 1 inbox messages");
    assert_eq!(envelope["data"]["total"], 1);
    assert_eq!(message_id(&envelope["data"]["messages"][0]), "group-unread");
}

#[test]
fn msg_inbox_default_scope_all_mark_read_updates_local_rows_like_go() {
    let workspace = TempDir::new("msg-all-inbox-mark-read").expect("workspace");
    register_ready_msg_identity(workspace.path(), "alice-all-mark", "alice", "jwt-alice");
    let alice_did = "did:wba:awiki.ai:alice:e1_alice";
    seed_direct_message(
        workspace.path(),
        alice_did,
        "did:wba:awiki.ai:bob:e1_bob",
        "direct-mark",
        "mark direct",
        "2026-05-16T10:00:00Z",
        false,
    );
    seed_group_message(
        workspace.path(),
        alice_did,
        "did:wba:awiki.ai:groups:demo:e1_group",
        "group-mark",
        "mark group",
        "2026-05-16T10:01:00Z",
        false,
    );
    seed_mail_notification(
        workspace.path(),
        alice_did,
        "mail-mark",
        "mark mail",
        "2026-05-16T10:02:00Z",
        false,
    );
    let socket_path = workspace.path().join("runtime").join("message-daemon.sock");
    let (_bridge, bridge_requests) = spawn_bridge_server(
        &socket_path,
        json!({
            "updated_count": 1
        }),
    );
    write_msg_ws_config_with_socket(
        workspace.path(),
        "https://placeholder.invalid",
        &socket_path,
    );

    let output = awiki_cmd(
        &[
            "--identity",
            "alice-all-mark",
            "msg",
            "inbox",
            "--mark-read",
            "--limit",
            "10",
        ],
        workspace.path(),
    );

    assert_success(&output);
    let envelope = success_json(&output);
    assert_eq!(envelope["summary"], "Loaded 3 inbox messages");
    for message in envelope["data"]["messages"].as_array().expect("messages") {
        assert_eq!(message["is_read"], true);
    }

    let mark_read_request = bridge_requests
        .recv_timeout(Duration::from_secs(2))
        .expect("bridge inbox.mark_read request");
    assert_eq!(mark_read_request.method, "inbox.mark_read");
    assert_eq!(mark_read_request.identity_name, "alice-all-mark");
    assert_eq!(
        mark_read_request.params["message_ids"],
        json!(["direct-mark"])
    );

    let rows = query_rows(
        workspace.path(),
        "SELECT msg_id, is_read FROM messages WHERE msg_id IN ('direct-mark', 'group-mark', 'mail-mark') ORDER BY msg_id",
    );
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0]["msg_id"], "direct-mark");
    assert_eq!(rows[0]["is_read"], 1);
    assert_eq!(rows[1]["msg_id"], "group-mark");
    assert_eq!(rows[1]["is_read"], 1);
    assert_eq!(rows[2]["msg_id"], "mail-mark");
    assert_eq!(rows[2]["is_read"], 1);
}

#[test]
fn msg_inbox_default_scope_all_mark_read_ignores_rows_without_ids_like_go() {
    let workspace = TempDir::new("msg-all-inbox-mark-read-no-id").expect("workspace");
    register_ready_msg_identity(workspace.path(), "alice-all-no-id", "alice", "jwt-alice");
    let alice_did = "did:wba:awiki.ai:alice:e1_alice";
    seed_group_message(
        workspace.path(),
        alice_did,
        "did:wba:awiki.ai:groups:no-id:e1_group",
        "",
        "group without id",
        "2026-05-16T10:01:00Z",
        false,
    );
    write_msg_ws_config(workspace.path(), "https://placeholder.invalid");

    let output = awiki_cmd(
        &[
            "--identity",
            "alice-all-no-id",
            "msg",
            "inbox",
            "--mark-read",
            "--limit",
            "10",
        ],
        workspace.path(),
    );

    assert_success(&output);
    let envelope = success_json(&output);
    assert_eq!(envelope["summary"], "Loaded 1 inbox messages");
    assert_eq!(envelope["data"]["messages"][0]["is_read"], 0);
}

#[test]
fn msg_inbox_scope_group_reads_local_group_cache_like_go() {
    let workspace = TempDir::new("msg-group-inbox-filter").expect("workspace");
    register_ready_msg_identity(workspace.path(), "alice-group-filter", "alice", "jwt-alice");
    let alice_did = "did:wba:awiki.ai:alice:e1_alice";
    let group_a = "did:wba:awiki.ai:groups:alpha:e1_group";
    let group_b = "did:wba:awiki.ai:groups:beta:e1_group";
    seed_direct_message(
        workspace.path(),
        alice_did,
        "did:wba:awiki.ai:bob:e1_bob",
        "direct-not-group",
        "not group",
        "2026-05-16T10:00:00Z",
        false,
    );
    seed_group_message(
        workspace.path(),
        alice_did,
        group_a,
        "group-alpha",
        "alpha group",
        "2026-05-16T10:01:00Z",
        false,
    );
    seed_group_message(
        workspace.path(),
        alice_did,
        group_b,
        "group-beta",
        "beta group",
        "2026-05-16T10:02:00Z",
        false,
    );
    write_msg_ws_config(workspace.path(), "https://placeholder.invalid");

    let output = awiki_cmd(
        &[
            "--identity",
            "alice-group-filter",
            "msg",
            "inbox",
            "--scope",
            "group",
            "--group",
            group_a,
            "--limit",
            "10",
        ],
        workspace.path(),
    );

    assert_success(&output);
    let envelope = success_json(&output);
    assert_eq!(envelope["summary"], "Loaded 1 group inbox messages");
    assert_eq!(envelope["data"]["source"], "local_group_cache");
    assert_eq!(envelope["data"]["group"], group_a);
    assert_eq!(envelope["data"]["total"], 1);
    assert_eq!(message_id(&envelope["data"]["messages"][0]), "group-alpha");
}

#[test]
fn msg_inbox_scope_group_without_group_reads_all_group_cache_like_go() {
    let workspace = TempDir::new("msg-group-inbox-all-groups").expect("workspace");
    register_ready_msg_identity(workspace.path(), "alice-group-all", "alice", "jwt-alice");
    let alice_did = "did:wba:awiki.ai:alice:e1_alice";
    seed_direct_message(
        workspace.path(),
        alice_did,
        "did:wba:awiki.ai:bob:e1_bob",
        "direct-not-returned",
        "not group",
        "2026-05-16T10:00:00Z",
        false,
    );
    seed_group_message(
        workspace.path(),
        alice_did,
        "did:wba:awiki.ai:groups:alpha:e1_group",
        "group-alpha-all",
        "alpha group",
        "2026-05-16T10:01:00Z",
        false,
    );
    seed_group_message(
        workspace.path(),
        alice_did,
        "did:wba:awiki.ai:groups:beta:e1_group",
        "group-beta-all",
        "beta group",
        "2026-05-16T10:02:00Z",
        false,
    );
    write_msg_ws_config(workspace.path(), "https://placeholder.invalid");

    let output = awiki_cmd(
        &[
            "--identity",
            "alice-group-all",
            "msg",
            "inbox",
            "--scope",
            "group",
            "--limit",
            "10",
        ],
        workspace.path(),
    );

    assert_success(&output);
    let envelope = success_json(&output);
    assert_eq!(envelope["summary"], "Loaded 2 group inbox messages");
    assert_eq!(envelope["data"]["source"], "local_group_cache");
    assert_eq!(envelope["data"]["group"], "");
    assert_eq!(envelope["data"]["total"], 2);
    assert_eq!(
        message_id(&envelope["data"]["messages"][0]),
        "group-beta-all"
    );
    assert_eq!(
        message_id(&envelope["data"]["messages"][1]),
        "group-alpha-all"
    );
}

#[test]
fn msg_inbox_scope_group_unread_filters_local_group_cache_like_go() {
    let workspace = TempDir::new("msg-group-inbox-unread").expect("workspace");
    register_ready_msg_identity(workspace.path(), "alice-group-unread", "alice", "jwt-alice");
    let alice_did = "did:wba:awiki.ai:alice:e1_alice";
    let group = "did:wba:awiki.ai:groups:demo:e1_group";
    seed_group_message(
        workspace.path(),
        alice_did,
        group,
        "group-read",
        "already read",
        "2026-05-16T10:01:00Z",
        true,
    );
    seed_group_message(
        workspace.path(),
        alice_did,
        group,
        "group-unread",
        "unread group",
        "2026-05-16T10:02:00Z",
        false,
    );
    write_msg_ws_config(workspace.path(), "https://placeholder.invalid");

    let output = awiki_cmd(
        &[
            "--identity",
            "alice-group-unread",
            "msg",
            "inbox",
            "--scope",
            "group",
            "--group",
            group,
            "--unread",
            "--limit",
            "10",
        ],
        workspace.path(),
    );

    assert_success(&output);
    let envelope = success_json(&output);
    assert_eq!(envelope["summary"], "Loaded 1 group inbox messages");
    assert_eq!(envelope["data"]["total"], 1);
    assert_eq!(message_id(&envelope["data"]["messages"][0]), "group-unread");
}

#[test]
fn msg_inbox_scope_group_mark_read_updates_local_rows_like_go() {
    let workspace = TempDir::new("msg-group-inbox-mark-read").expect("workspace");
    register_ready_msg_identity(workspace.path(), "alice-group-mark", "alice", "jwt-alice");
    let alice_did = "did:wba:awiki.ai:alice:e1_alice";
    let group = "did:wba:awiki.ai:groups:demo:e1_group";
    seed_group_message(
        workspace.path(),
        alice_did,
        group,
        "group-mark-read",
        "mark group",
        "2026-05-16T10:01:00Z",
        false,
    );
    write_msg_ws_config(workspace.path(), "https://placeholder.invalid");

    let output = awiki_cmd(
        &[
            "--identity",
            "alice-group-mark",
            "msg",
            "inbox",
            "--scope",
            "group",
            "--group",
            group,
            "--mark-read",
            "--limit",
            "10",
        ],
        workspace.path(),
    );

    assert_success(&output);
    let envelope = success_json(&output);
    assert_eq!(envelope["summary"], "Loaded 1 group inbox messages");
    assert_eq!(envelope["data"]["messages"][0]["is_read"], true);

    let rows = query_rows(
        workspace.path(),
        "SELECT msg_id, is_read FROM messages WHERE msg_id = 'group-mark-read'",
    );
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["msg_id"], "group-mark-read");
    assert_eq!(rows[0]["is_read"], 1);
}

#[test]
fn msg_inbox_group_flag_without_group_scope_keeps_default_all_route_like_go() {
    let workspace = TempDir::new("msg-group-flag-default-all").expect("workspace");
    register_ready_msg_identity(workspace.path(), "alice-group-flag", "alice", "jwt-alice");
    let alice_did = "did:wba:awiki.ai:alice:e1_alice";
    let group_a = "did:wba:awiki.ai:groups:alpha:e1_group";
    seed_group_message(
        workspace.path(),
        alice_did,
        group_a,
        "group-alpha-default",
        "alpha group",
        "2026-05-16T10:01:00Z",
        false,
    );
    seed_group_message(
        workspace.path(),
        alice_did,
        "did:wba:awiki.ai:groups:beta:e1_group",
        "group-beta-default",
        "beta group",
        "2026-05-16T10:02:00Z",
        false,
    );
    write_msg_ws_config(workspace.path(), "https://placeholder.invalid");

    let output = awiki_cmd(
        &[
            "--identity",
            "alice-group-flag",
            "msg",
            "inbox",
            "--group",
            group_a,
            "--limit",
            "10",
        ],
        workspace.path(),
    );

    assert_success(&output);
    let envelope = success_json(&output);
    assert_eq!(envelope["summary"], "Loaded 2 inbox messages");
    assert_eq!(
        envelope["data"]["source"],
        "local_direct_cache+local_group_cache"
    );
    assert!(envelope["data"].get("group").is_none());
    assert_eq!(
        message_id(&envelope["data"]["messages"][0]),
        "group-beta-default"
    );
    assert_eq!(
        message_id(&envelope["data"]["messages"][1]),
        "group-alpha-default"
    );
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

fn write_msg_ws_config(workspace: &Path, base_url: &str) {
    let socket_path = workspace.join("runtime").join("missing.sock");
    write_msg_ws_config_with_socket(workspace, base_url, &socket_path);
}

fn write_msg_ws_config_with_socket(workspace: &Path, base_url: &str, socket_path: &Path) {
    std::fs::write(
        workspace.join("config.yaml"),
        format!(
            "runtime:\n  mode: websocket\n  socket_path: {}\nservices:\n  service_base_url: {base_url}\n",
            socket_path.to_string_lossy()
        ),
    )
    .unwrap();
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

fn seed_direct_message(
    workspace: &Path,
    owner_did: &str,
    peer_did: &str,
    msg_id: &str,
    content: &str,
    sent_at: &str,
    is_read: bool,
) {
    let thread_id = direct_thread_id(owner_did, peer_did);
    let is_read = if is_read { 1 } else { 0 };
    execute_sql(
        workspace,
        format!(
            "INSERT INTO messages (msg_id, owner_did, thread_id, direction, sender_did, receiver_did, content_type, content, sent_at, stored_at, is_read, credential_name) VALUES ('{msg_id}', '{owner_did}', '{thread_id}', 0, '{peer_did}', '{owner_did}', 'text/plain', '{content}', '{sent_at}', '{sent_at}', {is_read}, 'alice-msg')",
        ),
    );
}

fn seed_group_message(
    workspace: &Path,
    owner_did: &str,
    group_did: &str,
    msg_id: &str,
    content: &str,
    sent_at: &str,
    is_read: bool,
) {
    let is_read = if is_read { 1 } else { 0 };
    execute_sql(
        workspace,
        format!(
            "INSERT INTO messages (msg_id, owner_did, thread_id, direction, sender_did, group_id, group_did, content_type, content, sent_at, stored_at, is_read, credential_name) VALUES ('{msg_id}', '{owner_did}', 'group:{group_did}', 0, 'did:wba:awiki.ai:carol:e1_carol', '{group_did}', '{group_did}', 'text/plain', '{content}', '{sent_at}', '{sent_at}', {is_read}, 'alice-msg')",
        ),
    );
}

fn seed_mail_notification(
    workspace: &Path,
    owner_did: &str,
    msg_id: &str,
    content: &str,
    sent_at: &str,
    is_read: bool,
) {
    let is_read = if is_read { 1 } else { 0 };
    execute_sql(
        workspace,
        format!(
            "INSERT INTO messages (msg_id, owner_did, thread_id, direction, sender_did, receiver_did, content_type, content, title, sent_at, stored_at, is_read, metadata, credential_name) VALUES ('{msg_id}', '{owner_did}', 'mail:alice@awiki.ai', 0, 'did:wba:mail:system', '{owner_did}', 'mail.notification', '{content}', 'Mail subject', '{sent_at}', '{sent_at}', {is_read}, '{{\"source_kind\":\"mail\",\"mailbox_address\":\"alice@awiki.ai\",\"from_addr\":\"sender@example.com\",\"subject\":\"Mail subject\",\"preview\":\"Preview text\",\"has_attachments\":true}}', 'alice-msg')",
        ),
    );
}

fn direct_thread_id(owner_did: &str, peer_did: &str) -> String {
    let mut pair = [owner_did.to_string(), peer_did.to_string()];
    pair.sort();
    format!("dm:{}:{}", pair[0], pair[1])
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

fn message_id(message: &Value) -> String {
    message
        .get("id")
        .or_else(|| message.get("message_id"))
        .or_else(|| message.get("msg_id"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn assert_contains_text(haystack: &str, needle: &str) {
    assert!(
        haystack.contains(needle),
        "expected text to contain {needle:?}, got:\n{haystack}"
    );
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
