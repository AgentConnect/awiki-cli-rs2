#![cfg(unix)]

use rusqlite::types::ValueRef;
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

mod support;

use support::{open_local_state, set_secret_storage_mode};

#[test]
fn msg_inbox_default_scope_all_does_not_fallback_to_legacy_local_cache() {
    let workspace = TempDir::new("msg-all-inbox-no-legacy-cache").expect("workspace");
    let alice_identity_id =
        register_ready_msg_identity(workspace.path(), "alice-no-cache", "alice", "jwt-alice");
    let alice_did = "did:wba:awiki.ai:alice:e1_alice";
    seed_direct_message(
        workspace.path(),
        &alice_identity_id,
        alice_did,
        "did:wba:awiki.ai:bob:e1_bob",
        "direct-1",
        "hello direct",
        "2026-05-16T10:00:00Z",
        false,
    );
    seed_group_message(
        workspace.path(),
        &alice_identity_id,
        alice_did,
        "did:wba:awiki.ai:groups:demo:e1_group",
        "group-1",
        "hello group",
        "2026-05-16T10:01:00Z",
        false,
    );
    seed_mail_notification(
        workspace.path(),
        &alice_identity_id,
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
            "alice-no-cache",
            "msg",
            "inbox",
            "--limit",
            "10",
        ],
        workspace.path(),
    );

    assert_transport_unavailable(&output);
}

#[test]
fn msg_inbox_group_scope_without_target_filter_does_not_fallback_to_legacy_group_cache() {
    let workspace = TempDir::new("msg-group-inbox-no-legacy-cache").expect("workspace");
    let alice_identity_id =
        register_ready_msg_identity(workspace.path(), "alice-group-cache", "alice", "jwt-alice");
    let alice_did = "did:wba:awiki.ai:alice:e1_alice";
    seed_group_message(
        workspace.path(),
        &alice_identity_id,
        alice_did,
        "did:wba:awiki.ai:groups:demo:e1_group",
        "group-local-1",
        "hello group",
        "2026-05-16T10:01:00Z",
        false,
    );
    write_msg_ws_config(workspace.path(), "https://placeholder.invalid");

    let output = awiki_cmd(
        &[
            "--identity",
            "alice-group-cache",
            "msg",
            "inbox",
            "--scope",
            "group",
            "--limit",
            "10",
        ],
        workspace.path(),
    );

    assert_transport_unavailable(&output);
}

#[test]
fn msg_inbox_target_filters_are_cutover_unsupported() {
    let workspace = TempDir::new("msg-inbox-target-filters").expect("workspace");
    let alice_identity_id =
        register_ready_msg_identity(workspace.path(), "alice-filter", "alice", "jwt-alice");
    let alice_did = "did:wba:awiki.ai:alice:e1_alice";
    seed_direct_message(
        workspace.path(),
        &alice_identity_id,
        alice_did,
        "did:wba:awiki.ai:bob:e1_bob",
        "direct-filter",
        "direct filter",
        "2026-05-16T10:00:00Z",
        false,
    );
    seed_group_message(
        workspace.path(),
        &alice_identity_id,
        alice_did,
        "did:wba:awiki.ai:groups:demo:e1_group",
        "group-filter",
        "group filter",
        "2026-05-16T10:01:00Z",
        false,
    );
    write_msg_ws_config(workspace.path(), "https://placeholder.invalid");

    let with_filter = awiki_cmd(
        &[
            "--identity",
            "alice-filter",
            "msg",
            "inbox",
            "--with",
            "did:wba:awiki.ai:bob:e1_bob",
            "--limit",
            "10",
        ],
        workspace.path(),
    );
    assert_unsupported_capability(&with_filter, "msg.inbox", "inbox-target-filters", "Phase 3");

    let group_filter = awiki_cmd(
        &[
            "--identity",
            "alice-filter",
            "msg",
            "inbox",
            "--scope",
            "group",
            "--group",
            "did:wba:awiki.ai:groups:demo:e1_group",
            "--limit",
            "10",
        ],
        workspace.path(),
    );
    assert_unsupported_capability(
        &group_filter,
        "msg.inbox",
        "inbox-target-filters",
        "Phase 3",
    );

    let group_flag_on_default_scope = awiki_cmd(
        &[
            "--identity",
            "alice-filter",
            "msg",
            "inbox",
            "--group",
            "did:wba:awiki.ai:groups:demo:e1_group",
            "--limit",
            "10",
        ],
        workspace.path(),
    );
    assert_unsupported_capability(
        &group_flag_on_default_scope,
        "msg.inbox",
        "inbox-target-filters",
        "Phase 3",
    );
}

#[test]
fn msg_inbox_mark_read_side_effect_is_cutover_unsupported_and_leaves_local_rows_unchanged() {
    let workspace = TempDir::new("msg-inbox-mark-read-unsupported").expect("workspace");
    let alice_identity_id =
        register_ready_msg_identity(workspace.path(), "alice-mark", "alice", "jwt-alice");
    let alice_did = "did:wba:awiki.ai:alice:e1_alice";
    seed_direct_message(
        workspace.path(),
        &alice_identity_id,
        alice_did,
        "did:wba:awiki.ai:bob:e1_bob",
        "direct-mark",
        "mark direct",
        "2026-05-16T10:00:00Z",
        false,
    );
    seed_group_message(
        workspace.path(),
        &alice_identity_id,
        alice_did,
        "did:wba:awiki.ai:groups:demo:e1_group",
        "group-mark",
        "mark group",
        "2026-05-16T10:01:00Z",
        false,
    );
    seed_mail_notification(
        workspace.path(),
        &alice_identity_id,
        alice_did,
        "mail-mark",
        "mark mail",
        "2026-05-16T10:02:00Z",
        false,
    );
    write_msg_ws_config(workspace.path(), "https://placeholder.invalid");

    let output = awiki_cmd(
        &[
            "--identity",
            "alice-mark",
            "msg",
            "inbox",
            "--mark-read",
            "--limit",
            "10",
        ],
        workspace.path(),
    );

    assert_unsupported_capability(
        &output,
        "msg.inbox",
        "inbox-mark-read-side-effect",
        "Phase 3",
    );

    let rows = query_rows(
        workspace.path(),
        "SELECT msg_id, is_read FROM messages WHERE msg_id IN ('direct-mark', 'group-mark', 'mail-mark') ORDER BY msg_id",
    );
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0]["msg_id"], "direct-mark");
    assert_eq!(rows[0]["is_read"], 0);
    assert_eq!(rows[1]["msg_id"], "group-mark");
    assert_eq!(rows[1]["is_read"], 0);
    assert_eq!(rows[2]["msg_id"], "mail-mark");
    assert_eq!(rows[2]["is_read"], 0);
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
    let unique_id = identity["unique_id"].as_str().unwrap().to_string();
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

fn write_msg_ws_config(workspace: &Path, base_url: &str) {
    let socket_path = workspace.join("runtime").join("missing.sock");
    std::fs::write(
        workspace.join("config.yaml"),
        format!(
            "runtime:\n  mode: websocket\n  socket_path: {}\nservices:\n  service_base_url: {base_url}\n",
            socket_path.to_string_lossy()
        ),
    )
    .unwrap();
}

fn seed_direct_message(
    workspace: &Path,
    owner_identity_id: &str,
    owner_did: &str,
    peer_did: &str,
    msg_id: &str,
    content: &str,
    sent_at: &str,
    is_read: bool,
) {
    let conversation_id = direct_conversation_id(peer_did);
    let is_read = if is_read { 1 } else { 0 };
    execute_sql(
        workspace,
        format!(
            "INSERT INTO messages (msg_id, owner_identity_id, owner_did, conversation_id, thread_id, direction, sender_did, receiver_did, content_type, content, sent_at, stored_at, is_read, credential_name) VALUES ('{msg_id}', '{owner_identity_id}', '{owner_did}', '{conversation_id}', '{conversation_id}', 0, '{peer_did}', '{owner_did}', 'text/plain', '{content}', '{sent_at}', '{sent_at}', {is_read}, 'alice-msg')",
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
    is_read: bool,
) {
    let is_read = if is_read { 1 } else { 0 };
    execute_sql(
        workspace,
        format!(
            "INSERT INTO messages (msg_id, owner_identity_id, owner_did, conversation_id, thread_id, direction, sender_did, group_id, group_did, content_type, content, sent_at, stored_at, is_read, credential_name) VALUES ('{msg_id}', '{owner_identity_id}', '{owner_did}', 'group:{group_did}', 'group:{group_did}', 0, 'did:wba:awiki.ai:carol:e1_carol', '{group_did}', '{group_did}', 'text/plain', '{content}', '{sent_at}', '{sent_at}', {is_read}, 'alice-msg')",
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
    is_read: bool,
) {
    let is_read = if is_read { 1 } else { 0 };
    execute_sql(
        workspace,
        format!(
            "INSERT INTO messages (msg_id, owner_identity_id, owner_did, conversation_id, thread_id, direction, sender_did, receiver_did, content_type, content, title, sent_at, stored_at, is_read, metadata, credential_name) VALUES ('{msg_id}', '{owner_identity_id}', '{owner_did}', 'mail:alice@awiki.ai', 'mail:alice@awiki.ai', 0, 'did:wba:mail:system', '{owner_did}', 'mail.notification', '{content}', 'Mail subject', '{sent_at}', '{sent_at}', {is_read}, '{{\"source_kind\":\"mail\",\"mailbox_address\":\"alice@awiki.ai\",\"from_addr\":\"sender@example.com\",\"subject\":\"Mail subject\",\"preview\":\"Preview text\",\"has_attachments\":true}}', 'alice-msg')",
        ),
    );
}

fn direct_conversation_id(peer_did: &str) -> String {
    format!("dm:{peer_did}")
}

fn execute_sql(workspace: &Path, statement: String) {
    let connection = open_local_state(workspace);
    connection
        .execute_batch(&statement)
        .expect("execute test sql");
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

fn assert_transport_unavailable(output: &Output) {
    assert_eq!(
        output.status.code(),
        Some(1),
        "unexpected exit status; stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let envelope = error_json(output);
    assert_eq!(
        envelope["error"]["code"], "transport_unavailable",
        "unexpected error envelope: {envelope}"
    );
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

fn error_json(output: &Output) -> Value {
    assert!(
        !output.status.success(),
        "command should fail\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stderr).expect("error JSON")
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
