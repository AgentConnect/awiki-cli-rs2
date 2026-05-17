use awiki_cli::config::Paths;
use awiki_cli::identity::{types::SaveInput, Manager};
use awiki_cli::store::{self, E2EEOutboxRecord};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn msg_secure_status_live_routes_cli_and_redacts_local_state_like_go() {
    let workspace = TempDir::new("msg-secure-status-live").expect("workspace");
    let manager = Manager::new(test_paths(workspace.path()));
    let alice = save_ready_identity(&manager, "alice-status", "alice");
    let peer_did = "did:wba:awiki.ai:user:bob:e1_bob";
    let other_peer_did = "did:wba:awiki.ai:user:carol:e1_carol";

    seed_secure_session(
        &manager,
        "alice-status",
        "session-bob",
        peer_did,
        "established",
    );
    seed_secure_session(
        &manager,
        "alice-status",
        "session-carol",
        other_peer_did,
        "pending-confirmation",
    );
    seed_secure_outbox(
        workspace.path(),
        E2EEOutboxRecord {
            outbox_id: "status-bob-failed".to_string(),
            owner_did: alice.did.clone(),
            peer_did: peer_did.to_string(),
            session_id: "session-bob".to_string(),
            original_type: "text".to_string(),
            plaintext: "secret failed plaintext".to_string(),
            local_status: "failed".to_string(),
            last_error_code: "send_failed".to_string(),
            retry_hint: "retry".to_string(),
            metadata: "{\"private\":\"metadata\"}".to_string(),
            created_at: "2026-05-17T00:00:00Z".to_string(),
            updated_at: "2026-05-17T00:03:00Z".to_string(),
            credential_name: "alice-status".to_string(),
            ..E2EEOutboxRecord::default()
        },
    );
    seed_secure_outbox(
        workspace.path(),
        E2EEOutboxRecord {
            outbox_id: "status-bob-queued".to_string(),
            owner_did: alice.did.clone(),
            peer_did: peer_did.to_string(),
            session_id: "session-bob".to_string(),
            original_type: "json".to_string(),
            plaintext: "{\"secret\":true}".to_string(),
            local_status: "queued".to_string(),
            created_at: "2026-05-17T00:01:00Z".to_string(),
            updated_at: "2026-05-17T00:02:00Z".to_string(),
            credential_name: "alice-status".to_string(),
            ..E2EEOutboxRecord::default()
        },
    );
    seed_secure_outbox(
        workspace.path(),
        E2EEOutboxRecord {
            outbox_id: "status-carol-failed".to_string(),
            owner_did: alice.did.clone(),
            peer_did: other_peer_did.to_string(),
            session_id: "session-carol".to_string(),
            original_type: "text".to_string(),
            plaintext: "other peer plaintext".to_string(),
            local_status: "failed".to_string(),
            created_at: "2026-05-17T00:04:00Z".to_string(),
            updated_at: "2026-05-17T00:04:00Z".to_string(),
            credential_name: "alice-status".to_string(),
            ..E2EEOutboxRecord::default()
        },
    );

    let output = awiki_cmd(
        &[
            "--identity",
            "alice-status",
            "msg",
            "secure",
            "status",
            "--with",
            peer_did,
        ],
        workspace.path(),
    );

    let envelope = success_json(&output);
    assert_eq!(
        envelope["summary"],
        "Loaded 1 secure session(s) and 2 secure outbox record(s)"
    );
    assert_eq!(envelope["data"]["with"], peer_did);

    let sessions = envelope["data"]["sessions"]
        .as_array()
        .expect("sessions array");
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0]["session_id"], "session-bob");
    assert_eq!(sessions[0]["peer_did"], peer_did);
    assert_eq!(sessions[0]["status"], "established");
    assert_eq!(sessions[0]["send_n"], 7);
    assert_eq!(sessions[0]["recv_n"], 3);
    assert_eq!(sessions[0]["previous_send_chain_length"], 2);
    assert_eq!(sessions[0]["skipped_key_count"], 2);
    assert_absent(&sessions[0], "root_key_b64u");
    assert_absent(&sessions[0], "ratchet_private_key_b64u");

    assert_eq!(envelope["data"]["outbox"]["total"], 2);
    assert_eq!(envelope["data"]["outbox"]["by_status"]["failed"], 1);
    assert_eq!(envelope["data"]["outbox"]["by_status"]["queued"], 1);
    let records = envelope["data"]["outbox"]["records"]
        .as_array()
        .expect("outbox records array");
    assert_eq!(
        outbox_ids(records),
        vec!["status-bob-failed", "status-bob-queued"]
    );
    for record in records {
        assert_eq!(record["peer_did"], peer_did);
        assert_absent(record, "owner_did");
        assert_absent(record, "credential_name");
        assert_absent(record, "plaintext");
        assert_absent(record, "metadata");
    }
}

#[test]
fn msg_secure_failed_live_routes_cli_and_filters_active_identity_like_go() {
    let workspace = TempDir::new("msg-secure-failed-live").expect("workspace");
    let manager = Manager::new(test_paths(workspace.path()));
    let alice = save_ready_identity(&manager, "alice-failed", "alice");
    let bob = save_ready_identity(&manager, "bob-failed", "bob");
    let peer_did = "did:wba:awiki.ai:user:peer:e1_peer";

    seed_secure_outbox(
        workspace.path(),
        E2EEOutboxRecord {
            outbox_id: "alice-failed-new".to_string(),
            owner_did: alice.did.clone(),
            peer_did: peer_did.to_string(),
            session_id: "session-alice-new".to_string(),
            original_type: "text".to_string(),
            plaintext: "new failed plaintext".to_string(),
            local_status: "failed".to_string(),
            last_error_code: "send_failed".to_string(),
            retry_hint: "retry".to_string(),
            created_at: "2026-05-17T01:00:00Z".to_string(),
            updated_at: "2026-05-17T01:03:00Z".to_string(),
            credential_name: "alice-failed".to_string(),
            ..E2EEOutboxRecord::default()
        },
    );
    seed_secure_outbox(
        workspace.path(),
        E2EEOutboxRecord {
            outbox_id: "alice-queued".to_string(),
            owner_did: alice.did.clone(),
            peer_did: peer_did.to_string(),
            session_id: "session-alice-queued".to_string(),
            original_type: "text".to_string(),
            plaintext: "queued plaintext".to_string(),
            local_status: "queued".to_string(),
            created_at: "2026-05-17T01:01:00Z".to_string(),
            updated_at: "2026-05-17T01:04:00Z".to_string(),
            credential_name: "alice-failed".to_string(),
            ..E2EEOutboxRecord::default()
        },
    );
    seed_secure_outbox(
        workspace.path(),
        E2EEOutboxRecord {
            outbox_id: "alice-failed-old".to_string(),
            owner_did: alice.did.clone(),
            peer_did: peer_did.to_string(),
            session_id: "session-alice-old".to_string(),
            original_type: "json".to_string(),
            plaintext: "{\"failed\":true}".to_string(),
            local_status: "failed".to_string(),
            last_error_code: "send_failed".to_string(),
            retry_hint: "retry".to_string(),
            created_at: "2026-05-17T01:02:00Z".to_string(),
            updated_at: "2026-05-17T01:02:00Z".to_string(),
            credential_name: "alice-failed".to_string(),
            ..E2EEOutboxRecord::default()
        },
    );
    seed_secure_outbox(
        workspace.path(),
        E2EEOutboxRecord {
            outbox_id: "bob-failed-hidden".to_string(),
            owner_did: bob.did.clone(),
            peer_did: peer_did.to_string(),
            session_id: "session-bob".to_string(),
            original_type: "text".to_string(),
            plaintext: "bob failed plaintext".to_string(),
            local_status: "failed".to_string(),
            last_error_code: "send_failed".to_string(),
            retry_hint: "retry".to_string(),
            created_at: "2026-05-17T01:05:00Z".to_string(),
            updated_at: "2026-05-17T01:05:00Z".to_string(),
            credential_name: "bob-failed".to_string(),
            ..E2EEOutboxRecord::default()
        },
    );

    let output = awiki_cmd(
        &["--identity", "alice-failed", "msg", "secure", "failed"],
        workspace.path(),
    );

    let envelope = success_json(&output);
    assert_eq!(
        envelope["summary"],
        "Loaded 2 failed secure outbox record(s)"
    );
    assert_eq!(envelope["data"]["total"], 2);
    let rows = envelope["data"]["failed"]
        .as_array()
        .expect("failed rows array");
    assert_eq!(
        outbox_ids(rows),
        vec!["alice-failed-new", "alice-failed-old"]
    );
    for row in rows {
        assert_eq!(row["owner_did"], alice.did);
        assert_eq!(row["credential_name"], "alice-failed");
        assert_eq!(row["local_status"], "failed");
    }
}

fn save_ready_identity(
    manager: &Manager,
    identity_name: &str,
    handle: &str,
) -> awiki_cli::identity::types::StoredIdentity {
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

fn seed_secure_session(
    manager: &Manager,
    identity_name: &str,
    session_id: &str,
    peer_did: &str,
    status: &str,
) {
    let paths = manager
        .paths_for_identity(identity_name)
        .expect("identity paths");
    let root = Path::new(&paths.identity_dir).join("p5-e2ee-sessions");
    std::fs::create_dir_all(&root).expect("create secure session root");
    let session = json!({
        "session_id": session_id,
        "suite": "ANP-DIRECT-E2EE-X3DH-25519-CHACHA20POLY1305-SHA256-V1",
        "peer_did": peer_did,
        "status": status,
        "is_initiator": true,
        "send_n": 7,
        "recv_n": 3,
        "previous_send_chain_length": 2,
        "skipped_message_keys": [{"n": 1}, {"n": 2}],
        "root_key_b64u": "secret-root",
        "ratchet_private_key_b64u": "secret-ratchet-private",
    });
    std::fs::write(
        root.join(format!("{session_id}.json")),
        serde_json::to_vec_pretty(&session).expect("session json"),
    )
    .expect("write secure session");
}

fn seed_secure_outbox(workspace: &Path, record: E2EEOutboxRecord) {
    let paths = test_paths(workspace);
    let connection = store::open(&paths).expect("open store");
    store::ensure_schema(&connection).expect("ensure store schema");
    store::queue_e2ee_outbox(&connection, record).expect("seed secure outbox");
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

fn outbox_ids(rows: &[Value]) -> Vec<&str> {
    rows.iter()
        .map(|row| {
            row.get("outbox_id")
                .and_then(Value::as_str)
                .expect("outbox_id string")
        })
        .collect()
}

fn assert_absent(value: &Value, field: &str) {
    assert!(
        value.get(field).is_none(),
        "{field} should be absent from {value:?}"
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
