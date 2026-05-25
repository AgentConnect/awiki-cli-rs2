use awiki_cli::config::Paths;
use awiki_cli::legacy_identity::{types::SaveInput, Manager};
use awiki_cli::legacy_store::{self as store, E2EEOutboxRecord};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn msg_secure_status_uses_im_core_while_failed_retry_and_drop_remain_unsupported() {
    let workspace = TempDir::new("msg-secure-status-im-core").expect("workspace");
    let manager = Manager::new(test_paths(workspace.path()));
    let alice = save_ready_identity(&manager, "alice-secure", "alice");
    let bob = save_ready_identity(&manager, "bob-secure", "bob");
    let peer_did = "did:wba:awiki.ai:user:peer:e1_peer";

    seed_secure_outbox(
        workspace.path(),
        E2EEOutboxRecord {
            outbox_id: "alice-failed".to_string(),
            owner_did: alice.did.clone(),
            peer_did: peer_did.to_string(),
            session_id: "session-alice".to_string(),
            original_type: "text".to_string(),
            plaintext: "failed plaintext".to_string(),
            local_status: "failed".to_string(),
            last_error_code: "send_failed".to_string(),
            retry_hint: "retry".to_string(),
            created_at: "2026-05-17T01:00:00Z".to_string(),
            updated_at: "2026-05-17T01:00:00Z".to_string(),
            credential_name: "alice-secure".to_string(),
            ..E2EEOutboxRecord::default()
        },
    );
    seed_secure_outbox(
        workspace.path(),
        E2EEOutboxRecord {
            outbox_id: "bob-failed".to_string(),
            owner_did: bob.did.clone(),
            peer_did: peer_did.to_string(),
            session_id: "session-bob".to_string(),
            original_type: "text".to_string(),
            plaintext: "other owner plaintext".to_string(),
            local_status: "failed".to_string(),
            created_at: "2026-05-17T01:01:00Z".to_string(),
            updated_at: "2026-05-17T01:01:00Z".to_string(),
            credential_name: "bob-secure".to_string(),
            ..E2EEOutboxRecord::default()
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
        "SELECT outbox_id, local_status, credential_name FROM e2ee_outbox ORDER BY outbox_id",
    );
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["outbox_id"], "alice-failed");
    assert_eq!(rows[0]["local_status"], "failed");
    assert_eq!(rows[0]["credential_name"], "alice-secure");
    assert_eq!(rows[1]["outbox_id"], "bob-failed");
    assert_eq!(rows[1]["local_status"], "failed");
    assert_eq!(rows[1]["credential_name"], "bob-secure");
}

fn save_ready_identity(
    manager: &Manager,
    identity_name: &str,
    handle: &str,
) -> awiki_cli::legacy_identity::types::StoredIdentity {
    manager
        .save(SaveInput {
            identity_name: identity_name.to_string(),
            did: format!("did:wba:awiki.ai:user:{handle}:e1_{handle}"),
            unique_id: format!("e1_{handle}_{identity_name}"),
            user_id: format!("user-{handle}"),
            display_name: identity_name.to_string(),
            handle: handle.to_string(),
            full_handle: format!("{handle}.awiki.ai"),
            jwt_token: format!("jwt-{handle}"),
            ..SaveInput::default()
        })
        .expect("save ready identity")
}

fn seed_secure_outbox(workspace: &Path, record: E2EEOutboxRecord) {
    let paths = test_paths(workspace);
    let connection = store::open(&paths).expect("open store");
    store::ensure_schema(&connection).expect("ensure store schema");
    store::queue_e2ee_outbox(&connection, record).expect("seed secure outbox");
}

fn query_rows(workspace: &Path, sql: &str) -> Vec<Value> {
    let paths = test_paths(workspace);
    let connection = store::open(&paths).expect("open store");
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

fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
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
