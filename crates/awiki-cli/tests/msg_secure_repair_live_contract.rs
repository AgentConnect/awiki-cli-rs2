mod support;

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};
use support::{open_local_state, write_ready_identity, TestIdentity, TestIdentityOptions};

#[test]
fn msg_secure_repair_uses_im_core_and_requeues_peer_failed_outbox() {
    let workspace = TempDir::new("msg-live-secure-repair-im-core").expect("workspace");
    write_msg_config(workspace.path());
    let alice = save_ready_identity(workspace.path(), "alice-repair", "alice", true);
    let bob = save_ready_identity(workspace.path(), "bob-repair", "bob", false);
    let carol = save_ready_identity(workspace.path(), "carol-repair", "carol", false);

    seed_secure_session(&alice, &bob.did);
    seed_secure_outbox_row(
        workspace.path(),
        &alice.did,
        &bob.did,
        "repair-failed-bob",
        "failed",
        "alice-repair",
    );
    seed_secure_outbox_row(
        workspace.path(),
        &alice.did,
        &carol.did,
        "repair-failed-carol",
        "failed",
        "alice-repair",
    );

    let output = awiki_cmd(
        &[
            "--identity",
            "alice-repair",
            "msg",
            "secure",
            "repair",
            "--with",
            &bob.did,
        ],
        workspace.path(),
    );

    let envelope = success_json(&output);
    assert_eq!(envelope["summary"], "Repaired direct secure state");
    assert_eq!(envelope["data"]["repair"]["peer"], bob.did);
    assert_eq!(envelope["data"]["repair"]["repaired"], true);
    assert_eq!(envelope["data"]["repair"]["state"], "Preparing");
    assert_eq!(
        envelope["data"]["repair"]["problem"]["code"],
        "PeerKeysUnavailable"
    );
    assert_eq!(
        envelope["data"]["repair"]["prepared_local_send_state"],
        Value::Bool(false)
    );
    assert_warning_contains(
        &envelope,
        "1 failed secure outbox item(s) were moved back to queued",
    );
    assert_warning_contains(&envelope, "direct E2EE prekey preparation failed");
    assert_legacy_session_file_unchanged(&alice, &bob.did);

    let rows = query_rows(
        workspace.path(),
        "SELECT outbox_id, local_status, peer_did FROM e2ee_outbox ORDER BY outbox_id",
    );
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["outbox_id"], "repair-failed-bob");
    assert_eq!(rows[0]["local_status"], "queued");
    assert_eq!(rows[0]["peer_did"], bob.did);
    assert_eq!(rows[1]["outbox_id"], "repair-failed-carol");
    assert_eq!(rows[1]["local_status"], "failed");
    assert_eq!(rows[1]["peer_did"], carol.did);
}

fn save_ready_identity(
    workspace: &Path,
    identity_name: &str,
    handle: &str,
    make_default: bool,
) -> TestIdentity {
    write_ready_identity(
        workspace,
        TestIdentityOptions {
            identity_name,
            handle,
            display_name: identity_name,
            jwt_token: &format!("jwt-{handle}"),
            make_default,
        },
    )
}

fn write_msg_config(workspace: &Path) {
    std::fs::write(
        workspace.join("config.yaml"),
        "services:\n  service_base_url: http://127.0.0.1:9\n",
    )
    .unwrap();
}

fn seed_secure_session(identity: &TestIdentity, peer_did: &str) {
    let root = identity.identity_dir.join("p5-e2ee-sessions");
    std::fs::create_dir_all(&root).expect("create secure session root");
    let session = json!({
        "session_id": "old-session",
        "suite": "ANP-DIRECT-E2EE-X3DH-25519-CHACHA20POLY1305-SHA256-V1",
        "peer_did": peer_did,
        "status": "established",
        "is_initiator": true
    });
    std::fs::write(
        root.join("old-session.json"),
        serde_json::to_vec_pretty(&session).expect("session json"),
    )
    .expect("write secure session");
}

fn seed_secure_outbox_row(
    workspace: &Path,
    owner_did: &str,
    peer_did: &str,
    outbox_id: &str,
    local_status: &str,
    credential_name: &str,
) {
    let connection = open_local_state(workspace);
    connection
        .execute(
            r#"
INSERT INTO e2ee_outbox (
    outbox_id, owner_did, peer_did, session_id, original_type, plaintext,
    local_status, attempt_count, last_error_code, retry_hint, created_at,
    updated_at, credential_name
) VALUES (?1, ?2, ?3, 'old-session', 'text', 'failed repair plaintext', ?4, 0,
          'send_failed', 'retry', '2026-05-16T00:00:00Z', '2026-05-16T00:00:00Z', ?5)
"#,
            rusqlite::params![
                outbox_id,
                owner_did,
                peer_did,
                local_status,
                credential_name
            ],
        )
        .expect("seed secure outbox row");
}

fn assert_legacy_session_file_unchanged(identity: &TestIdentity, peer_did: &str) {
    let session_root = identity.identity_dir.join("p5-e2ee-sessions");
    let sessions = std::fs::read_dir(&session_root)
        .unwrap_or_else(|err| panic!("read session root {session_root:?}: {err}"))
        .filter_map(Result::ok)
        .map(|entry| {
            serde_json::from_slice::<Value>(&std::fs::read(entry.path()).expect("read session"))
                .expect("parse session")
        })
        .collect::<Vec<_>>();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0]["session_id"], "old-session");
    assert_eq!(sessions[0]["peer_did"], peer_did);
    assert_eq!(sessions[0]["status"], "established");
}

fn awiki_cmd(args: &[&str], workspace: &Path) -> Output {
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

fn query_rows(workspace: &Path, sql: &str) -> Vec<Value> {
    let connection = open_local_state(workspace);
    let mut statement = connection.prepare(sql).expect("prepare query");
    let columns = statement
        .column_names()
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>();
    let rows = statement
        .query_map([], |row| {
            let mut value = serde_json::Map::new();
            for (index, column) in columns.iter().enumerate() {
                value.insert(column.clone(), Value::String(row.get(index)?));
            }
            Ok(Value::Object(value))
        })
        .expect("query rows");
    rows.map(|row| row.expect("read row")).collect()
}

fn success_json(output: &Output) -> Value {
    assert_eq!(
        output.status.code(),
        Some(0),
        "unexpected exit status; stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stderr.is_empty(),
        "stderr should be empty: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("stdout should be a JSON success envelope")
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
