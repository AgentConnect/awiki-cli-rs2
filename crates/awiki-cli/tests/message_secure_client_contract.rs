use awiki_cli::config::{Paths, Resolved};
use awiki_cli::identity::{generate_identity, types::SaveInput, Manager};
use awiki_cli::message::{
    prepare_secure_e2ee_client_for_record, resolve_secure_e2ee_local_document,
};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn prepare_secure_e2ee_client_for_record_requires_manager_and_record_like_go() {
    let (_resolved, manager, root) = test_context("secure-client-required");
    let record = generated_identity_record(&manager, "alice", "alice-user", "alice");

    let manager_error = expect_err_without_debug(
        prepare_secure_e2ee_client_for_record(None, Some(&record)),
        "missing manager should fail",
    );
    assert_eq!(manager_error, "identity manager is required");

    let record_error = expect_err_without_debug(
        prepare_secure_e2ee_client_for_record(Some(&manager), None),
        "missing record should fail",
    );
    assert_eq!(record_error, "identity record is required");

    std::fs::remove_dir_all(root).expect("remove temp test root");
}

#[test]
fn prepare_secure_e2ee_client_for_record_parses_keys_and_creates_go_p5_store_roots() {
    let (_resolved, manager, root) = test_context("secure-client-store-roots");
    let record = generated_identity_record(&manager, "alice", "alice-user", "alice");
    let identity_paths = manager
        .paths_for_identity("alice")
        .expect("identity paths before prepare");

    let prepared = prepare_secure_e2ee_client_for_record(Some(&manager), Some(&record))
        .expect("prepare secure client context");

    assert_eq!(prepared.owner_did, record.did);
    assert_eq!(prepared.signing_key_id, format!("{}#key-1", record.did));
    assert_eq!(prepared.agreement_key_id, format!("{}#key-3", record.did));
    assert!(matches!(
        prepared.signing_private,
        awiki_cli::anpsdk::PrivateKeyMaterial::Ed25519(_)
    ));
    assert!(matches!(
        prepared.agreement_private,
        awiki_cli::anpsdk::PrivateKeyMaterial::X25519(_)
    ));
    assert!(Path::new(&identity_paths.identity_dir)
        .join("p5-e2ee-sessions")
        .is_dir());
    assert!(Path::new(&identity_paths.identity_dir)
        .join("p5-signed-prekeys")
        .is_dir());
    assert!(Path::new(&identity_paths.identity_dir)
        .join("p5-one-time-prekeys")
        .is_dir());

    std::fs::remove_dir_all(root).expect("remove temp test root");
}

#[test]
fn prepare_secure_e2ee_client_for_record_preserves_go_key_parse_error_prefixes() {
    let (_resolved, manager, root) = test_context("secure-client-key-errors");
    let mut record = generated_identity_record(&manager, "alice", "alice-user", "alice");
    record.key1_private_pem = "not pem".to_string();

    let signing_error = expect_err_without_debug(
        prepare_secure_e2ee_client_for_record(Some(&manager), Some(&record)),
        "invalid signing key should fail",
    );
    assert!(
        signing_error.starts_with("parse DID signing private key:"),
        "unexpected signing error: {signing_error}"
    );

    let mut record = manager.load("alice").expect("reload record");
    record.e2ee_agreement_private_pem = "not pem".to_string();
    let agreement_error = expect_err_without_debug(
        prepare_secure_e2ee_client_for_record(Some(&manager), Some(&record)),
        "invalid agreement key should fail",
    );
    assert!(
        agreement_error.starts_with("parse E2EE agreement private key:"),
        "unexpected agreement error: {agreement_error}"
    );

    std::fs::remove_dir_all(root).expect("remove temp test root");
}

#[test]
fn resolve_secure_e2ee_local_document_prefers_current_record_then_local_identity_store() {
    let (_resolved, manager, root) = test_context("secure-client-local-doc");
    let alice = generated_identity_record(&manager, "alice", "alice-user", "alice");
    let bob = generated_identity_record(&manager, "bob", "bob-user", "bob");
    let override_document = json!({
        "id": alice.did,
        "marker": "current-record-wins",
    });
    let mut current = alice.clone();
    current.did_document = Some(override_document.clone());

    assert_eq!(
        resolve_secure_e2ee_local_document(Some(&manager), Some(&current), &alice.did),
        Some(override_document)
    );
    assert_eq!(
        resolve_secure_e2ee_local_document(Some(&manager), Some(&current), &bob.did)
            .and_then(|value| value.get("id").cloned()),
        Some(Value::String(bob.did.clone()))
    );
    assert_eq!(
        resolve_secure_e2ee_local_document(Some(&manager), Some(&current), "did:wba:missing"),
        None
    );
    assert_eq!(
        resolve_secure_e2ee_local_document(None, Some(&current), &bob.did),
        None
    );

    std::fs::remove_dir_all(root).expect("remove temp test root");
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

fn expect_err_without_debug<T, E>(result: Result<T, E>, message: &str) -> E {
    match result {
        Ok(_) => panic!("{message}"),
        Err(err) => err,
    }
}
