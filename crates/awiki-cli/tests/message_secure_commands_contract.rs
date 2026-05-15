use awiki_cli::config::{Paths, Resolved};
use awiki_cli::identity::{types::SaveInput, Manager};
use awiki_cli::message::{
    secure_drop, secure_failed, secure_status, SecureOutboxActionRequest, SecureStatusRequest,
};
use awiki_cli::store::{self, get_e2ee_outbox, queue_e2ee_outbox, E2EEOutboxRecord};
use rusqlite::Connection;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn secure_failed_requires_ready_identity_and_lists_only_active_identity_failed_outbox() {
    let (resolved, manager, root) = test_context("secure-failed");
    let active = save_identity(&manager, "alice", "alice-user", "alice");
    let other = save_identity(&manager, "bob", "bob-user", "bob");
    let pending = save_identity(&manager, "pending", "", "pending");

    let db = open_store(&resolved);
    seed_outbox(
        &db,
        "alice-failed-new",
        &active.did,
        "alice",
        "failed",
        "2026-01-04T00:00:00Z",
    );
    seed_outbox(
        &db,
        "alice-queued",
        &active.did,
        "alice",
        "queued",
        "2026-01-05T00:00:00Z",
    );
    seed_outbox(
        &db,
        "alice-failed-old",
        &active.did,
        "alice",
        "failed",
        "2026-01-01T00:00:00Z",
    );
    seed_outbox(
        &db,
        "bob-failed",
        &other.did,
        "bob",
        "failed",
        "2026-01-06T00:00:00Z",
    );
    drop(db);

    let result = secure_failed(
        &resolved,
        &manager,
        SecureStatusRequest {
            identity_name: "alice".to_string(),
            ..Default::default()
        },
    )
    .expect("secure_failed for ready identity");

    assert_eq!(result.summary, "Loaded 2 failed secure outbox record(s)");
    assert_eq!(result.data["total"], 2);
    assert_eq!(
        outbox_ids(result.data["failed"].as_array().expect("failed rows array")),
        vec!["alice-failed-new", "alice-failed-old"]
    );
    for row in result.data["failed"].as_array().expect("failed rows array") {
        assert_eq!(text(row, "owner_did"), active.did);
        assert_eq!(text(row, "credential_name"), "alice");
        assert_eq!(text(row, "local_status"), "failed");
    }

    let err = secure_failed(
        &resolved,
        &manager,
        SecureStatusRequest {
            identity_name: pending.identity_name,
            ..Default::default()
        },
    )
    .expect_err("secure_failed should reject identity that is not ready for messaging");
    assert!(
        err.to_string()
            .contains("requires user registration before messaging"),
        "unexpected error: {err}"
    );

    std::fs::remove_dir_all(root).expect("remove temp test root");
}

#[test]
fn secure_status_returns_redacted_session_and_outbox_summary_for_active_ready_identity() {
    let (resolved, manager, root) = test_context("secure-status");
    let active = save_identity(&manager, "alice", "alice-user", "alice");
    let other = save_identity(&manager, "bob", "bob-user", "bob");
    let pending = save_identity(&manager, "pending", "", "pending");

    let peer_b = "did:wba:awiki.ai:user:bob:e1_bob";
    let peer_c = "did:wba:awiki.ai:user:carol:e1_carol";
    let paths = manager
        .paths_for_identity("alice")
        .expect("paths for alice identity");
    let session_dir = PathBuf::from(paths.identity_dir).join("p5-e2ee-sessions");
    write_json(
        &session_dir.join("z-bob.json"),
        &json!({
            "session_id": "session-bob",
            "suite": "ANP-DIRECT-E2EE-X3DH-25519-CHACHA20POLY1305-SHA256-V1",
            "peer_did": peer_b,
            "status": "established",
            "is_initiator": true,
            "send_n": 3,
            "recv_n": 4,
            "previous_send_chain_length": 2,
            "root_key_b64u": "root-secret",
            "send_chain_key_b64u": "send-secret",
            "recv_chain_key_b64u": "recv-secret",
            "ratchet_private_key_b64u": "ratchet-private-secret",
            "skipped_message_keys": [{
                "dh_pub_b64u": "skipped-dh",
                "n": 7,
                "message_key_b64u": "skipped-message-secret",
                "nonce_b64u": "skipped-nonce-secret"
            }]
        }),
    );
    write_json(
        &session_dir.join("a-carol.json"),
        &json!({
            "session_id": "session-carol",
            "suite": "ANP-DIRECT-E2EE-X3DH-25519-CHACHA20POLY1305-SHA256-V1",
            "peer_did": peer_c,
            "status": "established",
            "is_initiator": false,
            "send_n": 1,
            "recv_n": 2,
            "previous_send_chain_length": 0,
            "root_key_b64u": "carol-root-secret",
            "skipped_message_keys": []
        }),
    );

    let db = open_store(&resolved);
    seed_status_outbox(
        &db,
        E2EEOutboxRecord {
            outbox_id: "status-bob-failed".to_string(),
            owner_did: active.did.clone(),
            peer_did: peer_b.to_string(),
            session_id: "session-bob".to_string(),
            original_type: "text".to_string(),
            plaintext: "secret failed body".to_string(),
            local_status: "failed".to_string(),
            attempt_count: 2,
            sent_msg_id: "sent-1".to_string(),
            sent_server_seq: Some(42),
            last_error_code: "E_DELIVERY".to_string(),
            retry_hint: "retry later".to_string(),
            failed_msg_id: "failed-1".to_string(),
            failed_server_seq: Some(41),
            metadata: r#"{"secret":"outbox-metadata"}"#.to_string(),
            last_attempt_at: "2026-01-04T00:00:00Z".to_string(),
            created_at: "2026-01-01T00:00:00Z".to_string(),
            updated_at: "2026-01-04T00:00:00Z".to_string(),
            credential_name: "alice".to_string(),
        },
    );
    seed_status_outbox(
        &db,
        E2EEOutboxRecord {
            outbox_id: "status-bob-blank".to_string(),
            owner_did: active.did.clone(),
            peer_did: peer_b.to_string(),
            session_id: "session-bob".to_string(),
            original_type: "text".to_string(),
            plaintext: "secret blank body".to_string(),
            local_status: "queued".to_string(),
            created_at: "2026-01-02T00:00:00Z".to_string(),
            updated_at: "2026-01-05T00:00:00Z".to_string(),
            credential_name: "alice".to_string(),
            ..Default::default()
        },
    );
    db.execute(
        "UPDATE e2ee_outbox SET local_status = '' WHERE outbox_id = ?1",
        ["status-bob-blank"],
    )
    .expect("set blank local_status");
    seed_status_outbox(
        &db,
        E2EEOutboxRecord {
            outbox_id: "status-carol-queued".to_string(),
            owner_did: active.did.clone(),
            peer_did: peer_c.to_string(),
            plaintext: "secret filtered peer body".to_string(),
            local_status: "queued".to_string(),
            credential_name: "alice".to_string(),
            updated_at: "2026-01-06T00:00:00Z".to_string(),
            ..Default::default()
        },
    );
    seed_status_outbox(
        &db,
        E2EEOutboxRecord {
            outbox_id: "status-other-owner".to_string(),
            owner_did: other.did.clone(),
            peer_did: peer_b.to_string(),
            plaintext: "secret other owner body".to_string(),
            local_status: "failed".to_string(),
            credential_name: "bob".to_string(),
            updated_at: "2026-01-07T00:00:00Z".to_string(),
            ..Default::default()
        },
    );
    drop(db);

    let result = secure_status(
        &resolved,
        &manager,
        SecureStatusRequest {
            identity_name: "alice".to_string(),
            with: peer_b.to_string(),
        },
    )
    .expect("secure_status for active ready identity");

    assert_eq!(
        result.summary,
        "Loaded 1 secure session(s) and 2 secure outbox record(s)"
    );
    assert_eq!(result.data["with"], peer_b);
    let sessions = result.data["sessions"].as_array().expect("sessions array");
    assert_eq!(sessions.len(), 1);
    assert_eq!(text(&sessions[0], "session_id"), "session-bob");
    assert_eq!(text(&sessions[0], "peer_did"), peer_b);
    assert_eq!(text(&sessions[0], "status"), "established");
    assert_eq!(
        text(&sessions[0], "suite"),
        "ANP-DIRECT-E2EE-X3DH-25519-CHACHA20POLY1305-SHA256-V1"
    );
    assert_eq!(sessions[0]["is_initiator"], true);
    assert_eq!(sessions[0]["send_n"], 3);
    assert_eq!(sessions[0]["recv_n"], 4);
    assert_eq!(sessions[0]["previous_send_chain_length"], 2);
    assert_eq!(sessions[0]["skipped_key_count"], 1);
    assert_object_keys(
        &sessions[0],
        &[
            "is_initiator",
            "peer_did",
            "previous_send_chain_length",
            "recv_n",
            "send_n",
            "session_id",
            "skipped_key_count",
            "status",
            "suite",
        ],
    );

    let unfiltered = secure_status(
        &resolved,
        &manager,
        SecureStatusRequest {
            identity_name: "alice".to_string(),
            ..Default::default()
        },
    )
    .expect("unfiltered secure_status");
    assert_eq!(
        text(&unfiltered.data["sessions"][0], "peer_did"),
        peer_b,
        "sessions should be sorted by peer_did"
    );
    assert_eq!(
        text(&unfiltered.data["sessions"][1], "peer_did"),
        peer_c,
        "sessions should be sorted by peer_did"
    );

    let outbox = result.data["outbox"].as_object().expect("outbox object");
    assert_eq!(outbox["total"], 2);
    assert_eq!(outbox["by_status"]["failed"], 1);
    assert_eq!(outbox["by_status"]["unknown"], 1);
    assert!(outbox["by_status"].get("queued").is_none());
    let records = outbox["records"].as_array().expect("outbox records array");
    assert_eq!(
        outbox_ids(records),
        vec!["status-bob-blank", "status-bob-failed"]
    );
    for row in records {
        assert_eq!(text(row, "peer_did"), peer_b);
        assert_object_keys(
            row,
            &[
                "attempt_count",
                "created_at",
                "failed_msg_id",
                "failed_server_seq",
                "last_attempt_at",
                "last_error_code",
                "local_status",
                "original_type",
                "outbox_id",
                "peer_did",
                "retry_hint",
                "sent_msg_id",
                "sent_server_seq",
                "session_id",
                "updated_at",
            ],
        );
    }

    let encoded = serde_json::to_string(&result.data).expect("encode secure_status data");
    for forbidden in [
        "root_key_b64u",
        "send_chain_key_b64u",
        "recv_chain_key_b64u",
        "ratchet_private_key_b64u",
        "message_key_b64u",
        "nonce_b64u",
        "root-secret",
        "send-secret",
        "recv-secret",
        "ratchet-private-secret",
        "skipped-message-secret",
        "skipped-nonce-secret",
        "plaintext",
        "metadata",
        "owner_did",
        "credential_name",
        "secret failed body",
        "secret blank body",
        "secret filtered peer body",
        "secret other owner body",
        "outbox-metadata",
    ] {
        assert!(
            !encoded.contains(forbidden),
            "secure_status leaked {forbidden:?} in {encoded}"
        );
    }

    let err = secure_status(
        &resolved,
        &manager,
        SecureStatusRequest {
            identity_name: pending.identity_name,
            ..Default::default()
        },
    )
    .expect_err("secure_status should reject identity that is not ready for messaging");
    assert!(
        err.to_string()
            .contains("requires user registration before messaging"),
        "unexpected error: {err}"
    );

    std::fs::remove_dir_all(root).expect("remove temp test root");
}

#[test]
fn secure_drop_marks_active_identity_outbox_dropped_and_returns_summary() {
    let (resolved, manager, root) = test_context("secure-drop");
    let active = save_identity(&manager, "alice", "alice-user", "alice");
    let other = save_identity(&manager, "bob", "bob-user", "bob");

    let db = open_store(&resolved);
    seed_outbox(
        &db,
        "drop-me",
        &active.did,
        "alice",
        "failed",
        "2026-01-01T00:00:00Z",
    );
    seed_outbox(
        &db,
        "keep-other-owner",
        &other.did,
        "bob",
        "failed",
        "2026-01-02T00:00:00Z",
    );
    drop(db);

    let result = secure_drop(
        &resolved,
        &manager,
        SecureOutboxActionRequest {
            identity_name: "alice".to_string(),
            outbox_id: "drop-me".to_string(),
        },
    )
    .expect("secure_drop for active identity row");

    assert_eq!(result.summary, "Dropped secure outbox record drop-me");
    assert_eq!(result.data["outbox_id"], "drop-me");
    assert_eq!(result.data["status"], "dropped");

    let db = open_store(&resolved);
    let dropped = get_e2ee_outbox(&db, "drop-me", &active.did, "alice").expect("dropped row");
    assert_eq!(text(&dropped, "local_status"), "dropped");
    let untouched =
        get_e2ee_outbox(&db, "keep-other-owner", &other.did, "bob").expect("other owner row");
    assert_eq!(text(&untouched, "local_status"), "failed");

    std::fs::remove_dir_all(root).expect("remove temp test root");
}

#[test]
fn secure_drop_missing_outbox_id_errors_without_changing_unrelated_rows() {
    let (resolved, manager, root) = test_context("secure-drop-missing");
    let active = save_identity(&manager, "alice", "alice-user", "alice");
    let other = save_identity(&manager, "bob", "bob-user", "bob");

    let db = open_store(&resolved);
    seed_outbox(
        &db,
        "alice-failed",
        &active.did,
        "alice",
        "failed",
        "2026-01-01T00:00:00Z",
    );
    seed_outbox(
        &db,
        "bob-failed",
        &other.did,
        "bob",
        "failed",
        "2026-01-02T00:00:00Z",
    );
    drop(db);

    let err = secure_drop(
        &resolved,
        &manager,
        SecureOutboxActionRequest {
            identity_name: "alice".to_string(),
            outbox_id: "missing-outbox".to_string(),
        },
    )
    .expect_err("missing outbox id should fail");
    assert!(
        err.to_string().contains("query returned no rows"),
        "unexpected missing outbox error: {err}"
    );

    let db = open_store(&resolved);
    let active_row =
        get_e2ee_outbox(&db, "alice-failed", &active.did, "alice").expect("active row");
    assert_eq!(text(&active_row, "local_status"), "failed");
    let other_row = get_e2ee_outbox(&db, "bob-failed", &other.did, "bob").expect("other row");
    assert_eq!(text(&other_row, "local_status"), "failed");

    std::fs::remove_dir_all(root).expect("remove temp test root");
}

fn test_context(name: &str) -> (Resolved, Manager, PathBuf) {
    let root = temp_root(name);
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::create_dir_all(root.join("runtime")).expect("create runtime dir");
    std::fs::create_dir_all(root.join("cache")).expect("create cache dir");
    std::fs::create_dir_all(root.join("logs")).expect("create logs dir");

    let resolved = Resolved {
        paths: Paths {
            workspace_home_dir: path_string(&root),
            root_dir: path_string(&root),
            config_dir: path_string(&root),
            data_dir: path_string(&root.join("data")),
            state_dir: path_string(&root.join("runtime")),
            cache_dir: path_string(&root.join("cache")),
            logs_dir: path_string(&root.join("logs")),
            config_file: path_string(&root.join("config.yaml")),
            identity_dir: path_string(&root.join("identities")),
            database_file: path_string(&root.join("data").join("awiki-cli.db")),
            legacy_credentials_dir: path_string(&root.join("legacy-credentials")),
            legacy_data_dir: path_string(&root.join("legacy-data")),
        },
        config_schema_version: 1,
        active_identity: "alice".to_string(),
        runtime_mode: "websocket".to_string(),
        runtime_socket_path: path_string(&root.join("runtime").join("message-daemon.sock")),
        runtime_listener_enabled: true,
        runtime_listener_auto_install: true,
        runtime_listener_auto_start: true,
        host_notify_enabled: true,
        host_notify_sink: "log".to_string(),
        host_notify_file_path: String::new(),
        host_notify_openclaw_hook_url: String::new(),
        host_notify_openclaw_agent_id: String::new(),
        host_notify_openclaw_hook_name: String::new(),
        host_notify_hermes_notify_url: String::new(),
        host_notify_hermes_deliver: String::new(),
        output_format: "json".to_string(),
        no_color: true,
        service_base_url: "https://awiki.ai".to_string(),
        did_domain: "awiki.ai".to_string(),
        anp_service_endpoint: "https://awiki.ai/anp-im/rpc".to_string(),
        anp_service_did: "did:wba:awiki.ai".to_string(),
        mail_service_url: String::new(),
        ca_bundle: String::new(),
        update_disable_strict_version: false,
        update_metadata_cache_ttl_seconds: 0,
        config_exists: false,
        config_error: String::new(),
        env_hits: Vec::new(),
        sources: BTreeMap::new(),
    };
    let manager = Manager::new(resolved.paths.clone());
    (resolved, manager, root)
}

fn save_identity(
    manager: &Manager,
    identity_name: &str,
    user_id: &str,
    handle: &str,
) -> awiki_cli::identity::types::StoredIdentity {
    manager
        .save(SaveInput {
            identity_name: identity_name.to_string(),
            did: format!("did:wba:awiki.ai:user:{identity_name}:e1_{identity_name}"),
            unique_id: format!("e1_{identity_name}"),
            user_id: user_id.to_string(),
            display_name: identity_name.to_string(),
            handle: handle.to_string(),
            full_handle: format!("{handle}.awiki.ai"),
            jwt_token: "test-token".to_string(),
            ..Default::default()
        })
        .expect("save test identity")
}

fn open_store(resolved: &Resolved) -> Connection {
    let db = store::open(&resolved.paths).expect("open sqlite store");
    store::ensure_schema(&db).expect("ensure sqlite schema");
    db
}

fn seed_outbox(
    db: &Connection,
    outbox_id: &str,
    owner_did: &str,
    credential_name: &str,
    local_status: &str,
    updated_at: &str,
) {
    queue_e2ee_outbox(
        db,
        E2EEOutboxRecord {
            outbox_id: outbox_id.to_string(),
            owner_did: owner_did.to_string(),
            peer_did: "did:wba:awiki.ai:user:peer:e1_peer".to_string(),
            session_id: format!("session-{outbox_id}"),
            original_type: "text".to_string(),
            plaintext: format!("plaintext:{outbox_id}"),
            local_status: local_status.to_string(),
            created_at: "2026-01-01T00:00:00Z".to_string(),
            updated_at: updated_at.to_string(),
            credential_name: credential_name.to_string(),
            ..Default::default()
        },
    )
    .expect("seed e2ee outbox row");
}

fn seed_status_outbox(db: &Connection, record: E2EEOutboxRecord) {
    queue_e2ee_outbox(db, record).expect("seed secure status outbox row");
}

fn outbox_ids(rows: &[Value]) -> Vec<&str> {
    rows.iter().map(|row| text(row, "outbox_id")).collect()
}

fn assert_object_keys(value: &Value, expected: &[&str]) {
    let object = value.as_object().expect("json object");
    let mut keys = object.keys().map(String::as_str).collect::<Vec<_>>();
    keys.sort_unstable();
    assert_eq!(keys, expected);
}

fn text<'a>(record: &'a Value, field: &str) -> &'a str {
    record
        .get(field)
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("missing string field {field}: {record:?}"))
}

fn temp_root(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "awiki-cli-rs2-{name}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ))
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn write_json(path: &Path, value: &Value) {
    std::fs::create_dir_all(path.parent().expect("json parent")).expect("create json parent dir");
    std::fs::write(path, serde_json::to_vec(value).expect("encode json")).expect("write json");
}
