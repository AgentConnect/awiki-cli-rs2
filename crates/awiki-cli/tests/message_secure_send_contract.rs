use awiki_cli::config::{Paths, Resolved};
use awiki_cli::identity::{generate_identity, types::SaveInput, Manager};
use awiki_cli::message::{
    send, send_secure_direct_with_sender, SecureDirectSendOutcome, SecureDirectSendRequest,
    SendRequest,
};
use awiki_cli::store;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn secure_send_with_sender_requires_e2ee_keys_before_sender_or_store_side_effects() {
    let (resolved, manager, root) = test_context("secure-send-key-gate");
    save_identity_without_e2ee_keys(&manager, "alice", "alice-user", "alice");
    let mut sender_called = false;

    let err = send_secure_direct_with_sender(
        &resolved,
        &manager,
        send_request("alice", "did:wba:awiki.ai:user:bob:e1_bob", "hello secure"),
        |_request| {
            sender_called = true;
            SecureDirectSendOutcome::Error("unexpected".to_string())
        },
    )
    .expect_err("missing E2EE keys should fail");

    assert_eq!(
        err.to_string(),
        "secure direct messaging requires DID signing and X25519 E2EE private keys"
    );
    assert!(!sender_called);
    let connection = open_store(&resolved);
    let rows = store::list_e2ee_outbox(
        &connection,
        "did:wba:awiki.ai:user:alice:e1_alice",
        "alice",
        "",
    )
    .expect("list e2ee outbox");
    assert!(rows.is_empty());

    std::fs::remove_dir_all(root).expect("remove temp test root");
}

#[test]
fn production_send_routes_secure_on_to_secure_key_gate_like_go() {
    let (resolved, manager, root) = test_context("secure-send-production-route");
    save_identity_without_e2ee_keys(&manager, "alice", "alice-user", "alice");

    let err = send(
        &resolved,
        &manager,
        send_request("alice", "did:wba:awiki.ai:user:bob:e1_bob", "hello secure"),
    )
    .expect_err("secure production send should reach key gate");

    assert_eq!(
        err.to_string(),
        "secure direct messaging requires DID signing and X25519 E2EE private keys"
    );

    std::fs::remove_dir_all(root).expect("remove temp test root");
}

#[test]
fn secure_send_with_sender_queues_pending_confirmation_like_go() {
    let (resolved, manager, root) = test_context("secure-send-pending");
    let active = generated_identity_record(&manager, "alice", "alice-user", "alice");
    let peer_did = "did:wba:awiki.ai:user:bob:e1_bob";
    let mut sent_requests = Vec::<SecureDirectSendRequest>::new();

    let result = send_secure_direct_with_sender(
        &resolved,
        &manager,
        send_request("alice", peer_did, "queued secure"),
        |request| {
            sent_requests.push(request);
            SecureDirectSendOutcome::Error("pending confirmation required".to_string())
        },
    )
    .expect("queued secure result");

    assert_eq!(
        result.summary,
        "Queued secure direct message pending peer confirmation"
    );
    assert!(result.warnings.is_empty());
    assert_eq!(result.data["action"], "queue_secure_message");
    assert_eq!(result.data["target"]["did"], peer_did);
    assert_eq!(result.data["target"]["handle"], "");
    assert_eq!(result.data["target"]["kind"], "direct");
    assert_eq!(result.data["message"]["type"], "text");
    assert_eq!(result.data["message"]["secure"], true);
    assert_eq!(result.data["message"]["queued"], true);
    assert_eq!(result.data["delivery"]["delivery_state"], "queued");
    assert_eq!(result.data["delivery"]["target_did"], peer_did);
    assert!(result.data["delivery"]["outbox_id"]
        .as_str()
        .unwrap_or_default()
        .starts_with("local-"));
    assert_eq!(sent_requests.len(), 1);
    assert_eq!(sent_requests[0].target_did, peer_did);
    assert_eq!(sent_requests[0].plaintext, "queued secure");
    assert_eq!(sent_requests[0].message_type, "text");
    assert!(sent_requests[0].message_id.starts_with("msg-"));
    assert_eq!(sent_requests[0].operation_id, sent_requests[0].message_id);

    let connection = open_store(&resolved);
    let rows = store::list_e2ee_outbox(&connection, &active.did, "alice", "queued")
        .expect("list queued outbox");
    assert_eq!(rows.len(), 1);
    assert_eq!(text(&rows[0], "peer_did"), peer_did);
    assert_eq!(text(&rows[0], "original_type"), "text");
    assert_eq!(text(&rows[0], "plaintext"), "queued secure");
    assert_eq!(text(&rows[0], "local_status"), "queued");
    assert_eq!(text(&rows[0], "credential_name"), "alice");
    let metadata: Value = serde_json::from_str(text(&rows[0], "metadata")).expect("metadata");
    assert_eq!(metadata["reason"], "pending_confirmation");

    std::fs::remove_dir_all(root).expect("remove temp test root");
}

#[test]
fn secure_send_with_sender_success_persists_e2ee_outbound_message_like_go() {
    let (resolved, manager, root) = test_context("secure-send-success");
    let active = generated_identity_record(&manager, "alice", "alice-user", "alice");
    let peer_did = "did:wba:awiki.ai:user:bob:e1_bob";
    let mut sent_requests = Vec::<SecureDirectSendRequest>::new();

    let result = send_secure_direct_with_sender(
        &resolved,
        &manager,
        send_request("alice", peer_did, "hello secure world"),
        |request| {
            sent_requests.push(request);
            SecureDirectSendOutcome::Success {
                accepted: true,
                message_id: String::new(),
                operation_id: String::new(),
                target_did: String::new(),
                accepted_at: "2026-04-23T09:30:00Z".to_string(),
                final_acceptance: false,
                delivery_state: "accepted".to_string(),
            }
        },
    )
    .expect("secure send success");

    assert_eq!(result.summary, "Sent a direct text message");
    assert!(result.warnings.is_empty());
    assert_eq!(result.data["action"], "send_message");
    assert_eq!(result.data["target"]["did"], peer_did);
    assert_eq!(result.data["target"]["handle"], "");
    assert_eq!(result.data["message"]["type"], "text");
    assert_eq!(result.data["message"]["secure"], true);
    assert_eq!(result.data["message"]["sent_at"], "2026-04-23T09:30:00Z");
    let message_id = result.data["message"]["id"].as_str().expect("message id");
    assert!(message_id.starts_with("msg-"));
    assert_eq!(result.data["delivery"]["message_id"], message_id);
    assert_eq!(result.data["delivery"]["operation_id"], message_id);
    assert_eq!(result.data["delivery"]["target_did"], peer_did);
    assert_eq!(sent_requests.len(), 1);
    assert_eq!(sent_requests[0].message_id, message_id);
    assert_eq!(sent_requests[0].operation_id, message_id);

    let connection = open_store(&resolved);
    let messages = store::list_messages_by_ids(&connection, &active.did, &[message_id.to_string()])
        .expect("stored messages");
    assert_eq!(messages.len(), 1);
    assert_eq!(text(&messages[0], "owner_did"), active.did);
    assert_eq!(text(&messages[0], "sender_did"), active.did);
    assert_eq!(text(&messages[0], "receiver_did"), peer_did);
    assert_eq!(text(&messages[0], "content_type"), "text/plain");
    assert_eq!(text(&messages[0], "content"), "hello secure world");
    assert_eq!(int(&messages[0], "direction"), 1);
    assert_eq!(int(&messages[0], "is_e2ee"), 1);
    assert_eq!(int(&messages[0], "is_read"), 1);
    assert_eq!(text(&messages[0], "credential_name"), "alice");
    let metadata: Value = serde_json::from_str(text(&messages[0], "metadata")).expect("metadata");
    assert_eq!(metadata["delivery_state"], "accepted");
    assert_eq!(metadata["operation_id"], message_id);

    std::fs::remove_dir_all(root).expect("remove temp test root");
}

fn send_request(identity_name: &str, target: &str, text: &str) -> SendRequest {
    SendRequest {
        identity_name: identity_name.to_string(),
        target: target.to_string(),
        text: text.to_string(),
        secure_mode: "on".to_string(),
        ..SendRequest::default()
    }
}

fn generated_identity_record(
    manager: &Manager,
    identity_name: &str,
    user_id: &str,
    handle: &str,
) -> awiki_cli::identity::types::StoredIdentity {
    let generated = generate_identity(
        "awiki.ai",
        "https://awiki.ai/anp-im/rpc",
        "did:wba:awiki.ai",
    )
    .expect("generate identity");
    manager
        .save(SaveInput {
            identity_name: identity_name.to_string(),
            did: generated.did,
            unique_id: generated.unique_id,
            user_id: user_id.to_string(),
            display_name: identity_name.to_string(),
            handle: handle.to_string(),
            full_handle: format!("{handle}.awiki.ai"),
            jwt_token: "test-token".to_string(),
            did_document: Some(generated.did_document),
            key1_private_pem: generated.key1_private_pem,
            key1_public_pem: generated.key1_public_pem,
            e2ee_signing_private_pem: generated.e2ee_signing_private_pem,
            e2ee_agreement_private_pem: generated.e2ee_agreement_private_pem,
            ..SaveInput::default()
        })
        .expect("save generated test identity")
}

fn save_identity_without_e2ee_keys(
    manager: &Manager,
    identity_name: &str,
    user_id: &str,
    handle: &str,
) -> awiki_cli::identity::types::StoredIdentity {
    manager
        .save(SaveInput {
            identity_name: identity_name.to_string(),
            did: format!("did:wba:awiki.ai:user:{handle}:e1_{handle}"),
            unique_id: format!("e1_{handle}"),
            user_id: user_id.to_string(),
            display_name: identity_name.to_string(),
            handle: handle.to_string(),
            full_handle: format!("{handle}.awiki.ai"),
            jwt_token: "test-token".to_string(),
            ..SaveInput::default()
        })
        .expect("save identity without e2ee keys")
}

fn open_store(resolved: &Resolved) -> rusqlite::Connection {
    let connection = store::open(&resolved.paths).expect("open store");
    store::ensure_schema(&connection).expect("ensure schema");
    connection
}

fn text<'a>(record: &'a Value, field: &str) -> &'a str {
    record
        .get(field)
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("missing string field {field}: {record:?}"))
}

fn int(record: &Value, field: &str) -> i64 {
    record
        .get(field)
        .and_then(Value::as_i64)
        .unwrap_or_else(|| panic!("missing int field {field}: {record:?}"))
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
        sources: Default::default(),
    };
    let manager = Manager::new(resolved.paths.clone());
    (resolved, manager, root)
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
