use awiki_cli::config::Paths;
use awiki_cli::legacy_identity::{types::SaveInput, Manager};
use awiki_cli::legacy_store::{self as store, E2EEOutboxRecord};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn msg_secure_repair_uses_im_core_and_requeues_peer_failed_outbox() {
    let workspace = TempDir::new("msg-live-secure-repair-im-core").expect("workspace");
    write_msg_config(workspace.path());
    let manager = Manager::new(test_paths(workspace.path()));
    let alice = save_ready_identity(&manager, "alice-repair", "alice");
    let bob = save_ready_identity(&manager, "bob-repair", "bob");
    let carol = save_ready_identity(&manager, "carol-repair", "carol");

    seed_secure_session(&manager, "alice-repair", &bob.did);
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
    assert_warning_contains(
        &envelope,
        "1 failed secure outbox item(s) were moved back to queued",
    );
    assert_legacy_session_file_unchanged(&manager, "alice-repair", &bob.did);

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

fn write_msg_config(workspace: &Path) {
    std::fs::write(
        workspace.join("config.yaml"),
        "services:\n  service_base_url: http://127.0.0.1:9\n",
    )
    .unwrap();
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

fn seed_secure_session(manager: &Manager, identity_name: &str, peer_did: &str) {
    let paths = manager
        .paths_for_identity(identity_name)
        .expect("identity paths");
    let root = Path::new(&paths.identity_dir).join("p5-e2ee-sessions");
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
    let paths = test_paths(workspace);
    let connection = store::open(&paths).expect("open store");
    store::ensure_schema(&connection).expect("ensure store schema");
    store::queue_e2ee_outbox(
        &connection,
        E2EEOutboxRecord {
            outbox_id: outbox_id.to_string(),
            owner_did: owner_did.to_string(),
            peer_did: peer_did.to_string(),
            session_id: "old-session".to_string(),
            original_type: "text".to_string(),
            plaintext: "failed repair plaintext".to_string(),
            local_status: local_status.to_string(),
            last_error_code: "send_failed".to_string(),
            retry_hint: "retry".to_string(),
            created_at: "2026-05-16T00:00:00Z".to_string(),
            updated_at: "2026-05-16T00:00:00Z".to_string(),
            credential_name: credential_name.to_string(),
            ..E2EEOutboxRecord::default()
        },
    )
    .expect("seed secure outbox row");
}

fn assert_legacy_session_file_unchanged(manager: &Manager, identity_name: &str, peer_did: &str) {
    let paths = manager
        .paths_for_identity(identity_name)
        .expect("identity paths");
    let session_root = Path::new(&paths.identity_dir).join("p5-e2ee-sessions");
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

fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
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
