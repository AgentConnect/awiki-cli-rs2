use awiki_cli::config::{Paths, Resolved};
use awiki_cli::identity::{types::SaveInput, Manager};
use awiki_cli::message::{
    secure_drop, secure_failed, SecureOutboxActionRequest, SecureStatusRequest,
};
use awiki_cli::store::{self, get_e2ee_outbox, queue_e2ee_outbox, E2EEOutboxRecord};
use rusqlite::Connection;
use serde_json::Value;
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

fn outbox_ids(rows: &[Value]) -> Vec<&str> {
    rows.iter().map(|row| text(row, "outbox_id")).collect()
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
