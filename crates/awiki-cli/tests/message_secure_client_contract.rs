use awiki_cli::config::{Paths, Resolved};
use awiki_cli::identity::{generate_identity, types::SaveInput, Manager};
use awiki_cli::message::{
    new_secure_e2ee_client_for_record, prepare_secure_e2ee_client_for_record,
    resolve_secure_e2ee_local_document,
};
use serde_json::{json, Value};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
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

#[test]
fn secure_e2ee_client_constructor_requires_local_message_service_did_like_go_sdk() {
    let (_resolved, manager, root) = test_context("secure-client-constructor-service");
    let mut record = generated_identity_record(&manager, "alice", "alice-user", "alice");
    record.did_document = Some(json!({
        "id": record.did,
        "service": [
            {
                "type": "ANPMessageService",
                "serviceEndpoint": "https://awiki.ai/anp-im/rpc"
            }
        ]
    }));

    let error = expect_err_without_debug(
        new_secure_e2ee_client_for_record(
            Some(&manager),
            Some(&record),
            Box::new(|_, _| Ok(serde_json::Map::new())),
        ),
        "missing serviceDid should fail like Go SDK",
    );

    assert_eq!(error, "missing field: serviceDid");

    std::fs::remove_dir_all(root).expect("remove temp test root");
}

#[test]
fn secure_e2ee_client_publish_prekey_bundle_generates_go_p5_prekeys_and_double_publishes() {
    let (_resolved, manager, root) = test_context("secure-client-publish");
    let record = generated_identity_record(&manager, "alice", "alice-user", "alice");
    let identity_paths = manager
        .paths_for_identity("alice")
        .expect("identity paths before publish");
    let calls = Rc::new(RefCell::new(Vec::<RecordedRpcCall>::new()));
    let mut client = new_secure_e2ee_client_for_record(
        Some(&manager),
        Some(&record),
        recording_rpc(calls.clone(), |method, _params| {
            assert_eq!(method, "direct.e2ee.publish_prekey_bundle");
            Ok(json_map!({
                "accepted": true,
                "operation_id": "op-publish-result"
            }))
        }),
    )
    .expect("construct secure client");

    let result = client
        .publish_prekey_bundle()
        .expect("publish secure prekey bundle");

    assert_eq!(result.get("accepted"), Some(&Value::Bool(true)));
    let calls = calls.borrow();
    assert_eq!(
        calls.len(),
        2,
        "Go EnsureFreshPrekeyBundle opportunistically publishes, then PublishPrekeyBundle publishes again"
    );
    for call in calls.iter() {
        assert_eq!(call.method, "direct.e2ee.publish_prekey_bundle");
        assert_eq!(call.params["meta"]["sender_did"], record.did);
        assert_eq!(call.params["meta"]["target"]["kind"], "service");
        assert_eq!(call.params["meta"]["target"]["did"], "did:wba:awiki.ai");
        assert_eq!(call.params["meta"]["profile"], "anp.direct.e2ee.v1");
        assert_eq!(
            call.params["meta"]["security_profile"],
            "transport-protected"
        );
        assert!(call.params["meta"]["operation_id"]
            .as_str()
            .expect("publish operation id")
            .starts_with("op-publish-spk-"));
        assert_eq!(
            call.params["body"]["prekey_bundle"]["owner_did"],
            record.did
        );
        assert_eq!(
            call.params["body"]["prekey_bundle"]["static_key_agreement_id"],
            format!("{}#key-3", record.did)
        );
        assert_eq!(
            call.params["body"]["prekey_bundle"]["signed_prekey"]["key_id"],
            "spk-initial"
        );
        assert_eq!(
            call.params["body"]["prekey_bundle"]["signed_prekey"]["expires_at"],
            "2030-01-01T00:00:00Z"
        );
        assert_eq!(
            call.params["body"]["one_time_prekeys"]
                .as_array()
                .expect("one-time prekeys are published")
                .len(),
            16
        );
    }
    let identity_dir = Path::new(&identity_paths.identity_dir);
    assert!(identity_dir
        .join("p5-signed-prekeys")
        .join("spk-initial.pem")
        .is_file());
    assert!(identity_dir
        .join("p5-signed-prekeys")
        .join("spk-initial.json")
        .is_file());
    assert!(identity_dir
        .join("p5-signed-prekeys")
        .join("latest.txt")
        .is_file());
    assert_eq!(
        std::fs::read_dir(identity_dir.join("p5-one-time-prekeys"))
            .expect("read opk root")
            .filter_map(Result::ok)
            .filter(
                |entry| entry.path().extension().and_then(|value| value.to_str()) == Some("json")
            )
            .count(),
        16
    );

    std::fs::remove_dir_all(root).expect("remove temp test root");
}

#[test]
fn secure_e2ee_client_send_text_without_session_fetches_prekey_and_sends_init() {
    let (_resolved, manager, root) = test_context("secure-client-send-init");
    let alice = generated_identity_record(&manager, "alice", "alice-user", "alice");
    let bob = generated_identity_record(&manager, "bob", "bob-user", "bob");
    let bob_did = bob.did.clone();
    let bob_bundle = remote_prekey_bundle(&bob);
    let bob_opk = one_time_prekey_json("opk-bob-001");
    let calls = Rc::new(RefCell::new(Vec::<RecordedRpcCall>::new()));
    let mut client = new_secure_e2ee_client_for_record(
        Some(&manager),
        Some(&alice),
        recording_rpc(calls.clone(), move |method, params| match method {
            "direct.e2ee.get_prekey_bundle" => {
                assert_eq!(params["meta"]["target"]["did"], "did:wba:awiki.ai");
                assert_eq!(params["body"]["target_did"], bob_did);
                assert_eq!(params["body"]["require_opk"], true);
                Ok(json_map!({
                    "prekey_bundle": bob_bundle.clone(),
                    "one_time_prekey": bob_opk.clone(),
                }))
            }
            "direct.send" => Ok(json_map!({
                "accepted": true,
                "message_id": params["meta"]["message_id"].clone(),
                "operation_id": params["meta"]["operation_id"].clone(),
                "target_did": params["meta"]["target"]["did"].clone(),
            })),
            other => panic!("unexpected rpc method {other}"),
        }),
    )
    .expect("construct secure client");

    let result = client
        .send_text(&bob.did, "hello bob", "msg-init", "msg-init")
        .expect("send secure init");

    assert_eq!(result["accepted"], true);
    assert_eq!(result["message_id"], "msg-init");
    let calls = calls.borrow();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].method, "direct.e2ee.get_prekey_bundle");
    assert!(
        calls[0].params["meta"]["operation_id"]
            .as_str()
            .expect("get-prekey operation id")
            .starts_with("op-get-prekey-"),
        "get-prekey operation id should have Go prefix"
    );
    assert_eq!(calls[1].method, "direct.send");
    assert_eq!(calls[1].params["meta"]["sender_did"], alice.did);
    assert_eq!(calls[1].params["meta"]["target"]["kind"], "agent");
    assert_eq!(calls[1].params["meta"]["target"]["did"], bob.did);
    assert_eq!(
        calls[1].params["meta"]["content_type"],
        "application/anp-direct-init+json"
    );
    assert_eq!(calls[1].params["meta"]["operation_id"], "msg-init");
    assert_eq!(calls[1].params["meta"]["message_id"], "msg-init");
    assert_eq!(
        calls[1].params["body"]["sender_static_key_agreement_id"],
        format!("{}#key-3", alice.did)
    );
    assert_eq!(
        calls[1].params["body"]["recipient_bundle_id"],
        "bundle-bob-001"
    );
    assert_eq!(
        calls[1].params["body"]["recipient_signed_prekey_id"],
        "spk-bob-001"
    );
    assert_eq!(
        calls[1].params["body"]["recipient_one_time_prekey_id"],
        "opk-bob-001"
    );
    let session_id = calls[1].params["body"]["session_id"]
        .as_str()
        .expect("init session id");
    let session_path = Path::new(
        &manager
            .paths_for_identity("alice")
            .expect("alice identity paths")
            .identity_dir,
    )
    .join("p5-e2ee-sessions")
    .join(format!("{session_id}.json"));
    let saved_session: Value =
        serde_json::from_slice(&std::fs::read(&session_path).expect("read saved session"))
            .expect("parse saved session");
    assert_eq!(saved_session["peer_did"], bob.did);
    assert_eq!(saved_session["status"], "pending-confirmation");
    assert_eq!(saved_session["is_initiator"], true);
    assert_eq!(saved_session["send_n"], 1);

    std::fs::remove_dir_all(root).expect("remove temp test root");
}

#[test]
fn secure_e2ee_client_send_json_retries_get_prekey_without_opk_like_go() {
    let (_resolved, manager, root) = test_context("secure-client-opk-retry");
    let alice = generated_identity_record(&manager, "alice", "alice-user", "alice");
    let bob = generated_identity_record(&manager, "bob", "bob-user", "bob");
    let bob_bundle = remote_prekey_bundle(&bob);
    let calls = Rc::new(RefCell::new(Vec::<RecordedRpcCall>::new()));
    let mut client = new_secure_e2ee_client_for_record(
        Some(&manager),
        Some(&alice),
        recording_rpc(calls.clone(), move |method, params| match method {
            "direct.e2ee.get_prekey_bundle" => {
                if params["body"]["require_opk"] == true {
                    return Err("4003 anp.direct.e2ee.opk_unavailable".to_string());
                }
                Ok(json_map!({
                    "prekey_bundle": bob_bundle.clone()
                }))
            }
            "direct.send" => Ok(json_map!({
                "accepted": true,
                "message_id": params["meta"]["message_id"].clone(),
                "operation_id": params["meta"]["operation_id"].clone(),
                "target_did": params["meta"]["target"]["did"].clone(),
            })),
            other => panic!("unexpected rpc method {other}"),
        }),
    )
    .expect("construct secure client");

    client
        .send_json(
            &bob.did,
            serde_json::Map::from_iter([("event".to_string(), Value::String("wave".to_string()))]),
            "msg-json",
            "msg-json",
        )
        .expect("send secure json init after OPK retry");

    let calls = calls.borrow();
    assert_eq!(calls.len(), 3);
    assert_eq!(calls[0].params["body"]["require_opk"], true);
    assert_eq!(calls[1].method, "direct.e2ee.get_prekey_bundle");
    assert_eq!(calls[1].params["body"]["require_opk"], false);
    assert_eq!(
        calls[2].params["meta"]["content_type"],
        "application/anp-direct-init+json"
    );
    assert!(calls[2].params["body"]
        .as_object()
        .expect("init body")
        .get("recipient_one_time_prekey_id")
        .is_none());

    std::fs::remove_dir_all(root).expect("remove temp test root");
}

#[test]
fn secure_e2ee_client_rejects_mismatched_ids_and_pending_follow_up_like_go() {
    let (_resolved, manager, root) = test_context("secure-client-pending");
    let alice = generated_identity_record(&manager, "alice", "alice-user", "alice");
    let bob = generated_identity_record(&manager, "bob", "bob-user", "bob");
    let bob_bundle = remote_prekey_bundle(&bob);
    let calls = Rc::new(RefCell::new(Vec::<RecordedRpcCall>::new()));
    let mut client = new_secure_e2ee_client_for_record(
        Some(&manager),
        Some(&alice),
        recording_rpc(calls.clone(), move |method, _params| match method {
            "direct.e2ee.get_prekey_bundle" => Ok(json_map!({
                "prekey_bundle": bob_bundle.clone()
            })),
            "direct.send" => Ok(json_map!({"accepted": true})),
            other => panic!("unexpected rpc method {other}"),
        }),
    )
    .expect("construct secure client");

    let mismatch = client
        .send_text(&bob.did, "bad ids", "op-mismatch", "msg-mismatch")
        .expect_err("mismatched operation/message IDs should fail");
    assert_eq!(
        mismatch,
        "invalid field: direct-e2ee requires operation_id to equal message_id"
    );

    client
        .send_text(&bob.did, "first", "msg-first", "msg-first")
        .expect("first init should save pending session");
    let pending = client
        .send_json(
            &bob.did,
            serde_json::Map::from_iter([(
                "event".to_string(),
                Value::String("blocked".to_string()),
            )]),
            "msg-blocked",
            "msg-blocked",
        )
        .expect_err("pending confirmation follow-up should fail");
    assert_eq!(pending, "invalid field: session pending confirmation");
    assert_eq!(
        calls
            .borrow()
            .iter()
            .filter(|call| call.method == "direct.send")
            .count(),
        1,
        "pending follow-up must not send another direct.send RPC"
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

#[derive(Debug, Clone)]
struct RecordedRpcCall {
    method: String,
    params: Value,
}

fn recording_rpc<F>(
    calls: Rc<RefCell<Vec<RecordedRpcCall>>>,
    mut handler: F,
) -> Box<awiki_cli::message::SecureE2EERpc>
where
    F: FnMut(
            &str,
            serde_json::Map<String, Value>,
        ) -> Result<serde_json::Map<String, Value>, String>
        + 'static,
{
    Box::new(move |method, params| {
        calls.borrow_mut().push(RecordedRpcCall {
            method: method.to_string(),
            params: Value::Object(params.clone()),
        });
        handler(method, params)
    })
}

fn remote_prekey_bundle(record: &awiki_cli::identity::types::StoredIdentity) -> Value {
    let signing_private = awiki_cli::anpsdk::PrivateKeyMaterial::from_pem(&record.key1_private_pem)
        .expect("record signing private key");
    let _agreement_private =
        awiki_cli::anpsdk::PrivateKeyMaterial::from_pem(&record.e2ee_agreement_private_pem)
            .expect("record agreement private key");
    let signed_prekey_private = generated_x25519_private_key();
    let signed_prekey = awiki_cli::anpsdk::SignedPrekey {
        key_id: "spk-bob-001".to_string(),
        public_key_b64u: x25519_public_key_b64u(&signed_prekey_private),
        expires_at: "2030-01-01T00:00:00Z".to_string(),
    };
    serde_json::to_value(
        awiki_cli::anpsdk::build_prekey_bundle(
            "bundle-bob-001",
            &record.did,
            &format!("{}#key-3", record.did),
            signed_prekey,
            &signing_private,
            &format!("{}#key-1", record.did),
            Some("2026-05-16T00:00:00Z"),
        )
        .expect("build remote prekey bundle"),
    )
    .expect("serialize remote prekey bundle")
}

fn one_time_prekey_json(key_id: &str) -> Value {
    let private_key = generated_x25519_private_key();
    json!({
        "key_id": key_id,
        "public_key_b64u": x25519_public_key_b64u(&private_key),
    })
}

fn generated_x25519_private_key() -> awiki_cli::anpsdk::PrivateKeyMaterial {
    awiki_cli::anpsdk::create_did_wba_document(
        "awiki.ai",
        awiki_cli::anpsdk::DidDocumentOptions::default(),
    )
    .expect("create DID document with E2EE key")
    .load_private_key("key-3")
    .expect("load generated X25519 key agreement private key")
}

fn x25519_public_key_b64u(private_key: &awiki_cli::anpsdk::PrivateKeyMaterial) -> String {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
    match private_key.public_key() {
        awiki_cli::anpsdk::PublicKeyMaterial::X25519(bytes) => URL_SAFE_NO_PAD.encode(bytes),
        other => panic!("expected X25519 public key, got {other}"),
    }
}

macro_rules! json_map {
    ($($json:tt)+) => {{
        match serde_json::json!($($json)+) {
            serde_json::Value::Object(map) => map,
            _ => panic!("json_map! expects object"),
        }
    }};
}

use json_map;
