mod support;

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};
use support::{open_local_state, write_ready_identity, TestIdentity, TestIdentityOptions};

#[test]
fn msg_secure_status_uses_im_core_while_failed_retry_and_drop_remain_unsupported() {
    let workspace = TempDir::new("msg-secure-status-im-core").expect("workspace");
    let alice = save_ready_identity(workspace.path(), "alice-secure", "alice", true);
    let bob = save_ready_identity(workspace.path(), "bob-secure", "bob", false);
    let peer_did = "did:wba:awiki.ai:user:peer:e1_peer";

    seed_secure_outbox(
        workspace.path(),
        SecureOutboxSeed {
            outbox_id: "alice-failed",
            owner_identity_id: "e1_alice_alice-secure",
            owner_did: &alice.did,
            peer_did,
            session_id: "session-alice",
            plaintext: "failed plaintext",
            local_status: "failed",
            last_error_code: "send_failed",
            retry_hint: "retry",
            created_at: "2026-05-17T01:00:00Z",
            updated_at: "2026-05-17T01:00:00Z",
            credential_name: "alice-secure",
        },
    );
    seed_secure_outbox(
        workspace.path(),
        SecureOutboxSeed {
            outbox_id: "bob-failed",
            owner_identity_id: "e1_bob_bob-secure",
            owner_did: &bob.did,
            peer_did,
            session_id: "session-bob",
            plaintext: "other owner plaintext",
            local_status: "failed",
            last_error_code: "",
            retry_hint: "",
            created_at: "2026-05-17T01:01:00Z",
            updated_at: "2026-05-17T01:01:00Z",
            credential_name: "bob-secure",
        },
    );

    let status = awiki_cmd(
        &[
            "--identity",
            "alice-secure",
            "msg",
            "secure",
            "status",
            "--with",
            peer_did,
        ],
        workspace.path(),
    );
    let status = success_json(&status);
    assert_eq!(status["summary"], "Loaded direct secure status");
    assert_eq!(status["data"]["status"]["peer"], peer_did);
    assert_eq!(status["data"]["status"]["state"], "Preparing");
    assert_eq!(status["data"]["status"]["can_send_secure"], false);
    assert_eq!(status["data"]["status"]["pending_outbox_count"], 1);
    assert_eq!(
        status["data"]["status"]["problem"]["code"],
        "PeerKeysUnavailable"
    );

    for (args, command) in [
        (
            &["--identity", "alice-secure", "msg", "secure", "failed"][..],
            "msg.secure.failed",
        ),
        (
            &[
                "--identity",
                "alice-secure",
                "msg",
                "secure",
                "retry",
                "alice-failed",
            ][..],
            "msg.secure.retry",
        ),
        (
            &[
                "--identity",
                "alice-secure",
                "msg",
                "secure",
                "drop",
                "alice-failed",
            ][..],
            "msg.secure.drop",
        ),
    ] {
        let output = awiki_cmd(args, workspace.path());
        assert_secure_direct_unsupported(&output, command);
    }

    let rows = query_rows(
        workspace.path(),
        "SELECT outbox_id, owner_identity_id, local_status, credential_name FROM e2ee_outbox ORDER BY outbox_id",
    );
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["outbox_id"], "alice-failed");
    assert_eq!(rows[0]["owner_identity_id"], "e1_alice_alice-secure");
    assert_eq!(rows[0]["local_status"], "failed");
    assert_eq!(rows[0]["credential_name"], "alice-secure");
    assert_eq!(rows[1]["outbox_id"], "bob-failed");
    assert_eq!(rows[1]["owner_identity_id"], "e1_bob_bob-secure");
    assert_eq!(rows[1]["local_status"], "failed");
    assert_eq!(rows[1]["credential_name"], "bob-secure");
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

struct SecureOutboxSeed<'a> {
    outbox_id: &'a str,
    owner_identity_id: &'a str,
    owner_did: &'a str,
    peer_did: &'a str,
    session_id: &'a str,
    plaintext: &'a str,
    local_status: &'a str,
    last_error_code: &'a str,
    retry_hint: &'a str,
    created_at: &'a str,
    updated_at: &'a str,
    credential_name: &'a str,
}

fn seed_secure_outbox(workspace: &Path, record: SecureOutboxSeed<'_>) {
    let connection = open_local_state(workspace);
    connection
        .execute(
            r#"
	INSERT INTO e2ee_outbox (
	    outbox_id, owner_identity_id, owner_did, peer_did, session_id, original_type, plaintext,
	    local_status, attempt_count, last_error_code, retry_hint, created_at,
	    updated_at, credential_name
	) VALUES (?1, ?2, ?3, ?4, ?5, 'text', ?6, ?7, 0, ?8, ?9, ?10, ?11, ?12)
	"#,
            rusqlite::params![
                record.outbox_id,
                record.owner_identity_id,
                record.owner_did,
                record.peer_did,
                record.session_id,
                record.plaintext,
                record.local_status,
                record.last_error_code,
                record.retry_hint,
                record.created_at,
                record.updated_at,
                record.credential_name,
            ],
        )
        .expect("seed secure outbox");
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

fn assert_secure_direct_unsupported(output: &Output, command: &str) {
    assert_eq!(
        output.status.code(),
        Some(2),
        "unexpected exit status; stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stdout.is_empty(),
        "stdout should be empty: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let envelope: Value =
        serde_json::from_slice(&output.stderr).expect("stderr should be a JSON error envelope");
    assert_eq!(envelope["ok"], false);
    assert_eq!(envelope["error"]["code"], "unsupported_capability");
    assert_eq!(envelope["error"]["details"]["command"], command);
    assert_eq!(envelope["error"]["details"]["capability"], "secure-direct");
    assert_eq!(envelope["error"]["details"]["required_phase"], "Phase 6");
    assert_eq!(
        envelope["error"]["details"]["cutover_status"],
        "unsupported"
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
