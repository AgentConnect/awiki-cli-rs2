use awiki_cli::config::{Paths, Resolved};
use awiki_cli::identity::{generate_identity, types::SaveInput, Manager};
use awiki_cli::message::{
    new_secure_e2ee_client_for_record, prepare_secure_e2ee_client_for_record,
    resolve_secure_e2ee_local_document, MessageServiceE2EEClient,
};
use serde_json::{json, Map, Value};
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

#[test]
fn secure_e2ee_client_process_incoming_init_consumes_opk_and_saves_responder_session() {
    let (_resolved, manager, root) = test_context("secure-client-process-init");
    let alice = generated_identity_record(&manager, "alice", "alice-user", "alice");
    let bob = generated_identity_record(&manager, "bob", "bob-user", "bob");
    let mut bob_client = new_secure_e2ee_client_for_record(
        Some(&manager),
        Some(&bob),
        recording_rpc(Rc::new(RefCell::new(Vec::new())), |method, _params| {
            assert_eq!(method, "direct.e2ee.publish_prekey_bundle");
            Ok(Map::new())
        }),
    )
    .expect("construct bob secure client");
    bob_client
        .ensure_fresh_prekey_bundle()
        .expect("create bob local prekeys");
    let bob_bundle = bob_client
        .ensure_fresh_prekey_bundle()
        .expect("load bob bundle from local stores");
    let bob_opk = first_one_time_prekey(&manager, "bob");
    let mut alice_client = new_secure_e2ee_client_for_record(
        Some(&manager),
        Some(&alice),
        recording_rpc(
            Rc::new(RefCell::new(Vec::new())),
            move |method, params| match method {
                "direct.e2ee.get_prekey_bundle" => Ok(json_map!({
                    "prekey_bundle": serde_json::to_value(&bob_bundle).expect("bundle json"),
                    "one_time_prekey": serde_json::to_value(&bob_opk).expect("opk json")
                })),
                "direct.send" => Ok(json_map!({
                    "body": params["body"].clone()
                })),
                other => panic!("unexpected rpc method {other}"),
            },
        ),
    )
    .expect("construct alice secure client");

    let init_response = alice_client
        .send_text(&bob.did, "hello bob", "msg-init", "msg-init")
        .expect("alice sends init");
    let init_body = init_response
        .get("body")
        .cloned()
        .expect("init response body");
    let init_opk_id = init_body["recipient_one_time_prekey_id"]
        .as_str()
        .expect("init OPK id")
        .to_string();
    let decrypted = bob_client
        .process_incoming(notification(
            &alice.did,
            &bob.did,
            "msg-init",
            "application/anp-direct-init+json",
            init_body,
            1.0,
        ))
        .expect("bob processes init");

    assert_eq!(decrypted["state"], "decrypted");
    assert_eq!(
        decrypted["plaintext"]["application_content_type"],
        "text/plain"
    );
    assert_eq!(decrypted["plaintext"]["text"], "hello bob");
    assert!(
        one_time_prekeys(&manager, "bob")
            .iter()
            .all(|prekey| prekey.key_id != init_opk_id),
        "Go deletes the OPK after a successful init"
    );
    let responder_session =
        session_for_peer(&manager, "bob", &alice.did).expect("responder session exists");
    assert_eq!(responder_session.peer_did, alice.did);
    assert_eq!(responder_session.status, "established");
    assert!(!responder_session.is_initiator);
    assert_eq!(responder_session.recv_n, 1);
    assert_eq!(responder_session.send_n, 0);

    std::fs::remove_dir_all(root).expect("remove temp test root");
}

#[test]
fn secure_e2ee_client_process_incoming_confirms_first_reply_and_decrypts_follow_up_like_go() {
    let (_resolved, manager, root) = test_context("secure-client-process-roundtrip");
    let alice = generated_identity_record(&manager, "alice", "alice-user", "alice");
    let bob = generated_identity_record(&manager, "bob", "bob-user", "bob");
    let mut clients = connected_clients(&manager, &alice, &bob);
    let init_body = clients
        .alice
        .send_text(&bob.did, "hello bob", "msg-init", "msg-init")
        .expect("alice init")["body"]
        .clone();
    clients
        .bob
        .process_incoming(notification(
            &alice.did,
            &bob.did,
            "msg-init",
            "application/anp-direct-init+json",
            init_body,
            1.0,
        ))
        .expect("bob accepts init");
    let reply_body = clients
        .bob
        .send_json(
            &alice.did,
            Map::from_iter([("ack".to_string(), Value::String("ok".to_string()))]),
            "msg-reply",
            "msg-reply",
        )
        .expect("bob first reply")["body"]
        .clone();

    let confirmed = clients
        .alice
        .process_incoming(notification(
            &bob.did,
            &alice.did,
            "msg-reply",
            "application/anp-direct-cipher+json",
            reply_body,
            2.0,
        ))
        .expect("alice confirms first reply");

    assert_eq!(confirmed["state"], "decrypted");
    assert_eq!(
        confirmed["plaintext"]["application_content_type"],
        "application/json"
    );
    assert_eq!(confirmed["plaintext"]["payload"]["ack"], "ok");
    let alice_session =
        session_for_peer(&manager, "alice", &bob.did).expect("alice session exists");
    assert_eq!(alice_session.status, "established");

    let follow_up_body = clients
        .alice
        .send_json(
            &bob.did,
            Map::from_iter([("event".to_string(), Value::String("wave".to_string()))]),
            "msg-2",
            "msg-2",
        )
        .expect("alice follow-up")["body"]
        .clone();
    let follow_up = clients
        .bob
        .process_incoming(notification(
            &alice.did,
            &bob.did,
            "msg-2",
            "application/anp-direct-cipher+json",
            follow_up_body,
            3.0,
        ))
        .expect("bob decrypts follow-up");

    assert_eq!(follow_up["state"], "decrypted");
    assert_eq!(follow_up["plaintext"]["payload"]["event"], "wave");

    std::fs::remove_dir_all(root).expect("remove temp test root");
}

#[test]
fn secure_e2ee_client_process_incoming_queues_cipher_until_init_replays_pending_in_order() {
    let (_resolved, manager, root) = test_context("secure-client-pending-replay");
    let alice = generated_identity_record(&manager, "alice", "alice-user", "alice");
    let bob = generated_identity_record(&manager, "bob", "bob-user", "bob");
    let mut clients = connected_clients(&manager, &alice, &bob);
    let init_body = clients
        .alice
        .send_text(&bob.did, "hello bob", "msg-init", "msg-init")
        .expect("alice init")["body"]
        .clone();
    let pending_one = clients
        .bob
        .process_incoming(notification(
            &alice.did,
            &bob.did,
            "msg-2",
            "application/anp-direct-cipher+json",
            pending_cipher_body("missing-session-1", "1"),
            2.0,
        ))
        .expect("pending missing session");
    let pending_two = clients
        .bob
        .process_incoming(notification(
            &alice.did,
            &bob.did,
            "msg-3",
            "application/anp-direct-cipher+json",
            pending_cipher_body("missing-session-2", "2"),
            3.0,
        ))
        .expect("second pending missing session");

    assert_eq!(pending_one, json_map!({"state": "pending"}));
    assert_eq!(pending_two, json_map!({"state": "pending"}));
    let result = clients
        .bob
        .process_incoming(notification(
            &alice.did,
            &bob.did,
            "msg-init",
            "application/anp-direct-init+json",
            init_body,
            1.0,
        ))
        .expect("init replays pending");

    assert_eq!(result["state"], "decrypted");
    let pending_results = result["pending_results"]
        .as_array()
        .expect("pending replay results");
    assert_eq!(
        pending_results
            .iter()
            .map(|result| result["state"].as_str().expect("state"))
            .collect::<Vec<_>>(),
        vec!["pending", "pending"],
        "Go replays pending messages in insertion order and includes non-error results"
    );

    std::fs::remove_dir_all(root).expect("remove temp test root");
}

#[test]
fn secure_e2ee_client_process_incoming_returns_undecryptable_and_unsupported_like_go() {
    let (_resolved, manager, root) = test_context("secure-client-undecryptable");
    let alice = generated_identity_record(&manager, "alice", "alice-user", "alice");
    let bob = generated_identity_record(&manager, "bob", "bob-user", "bob");
    let mut clients = connected_clients(&manager, &alice, &bob);
    let init_body = clients
        .alice
        .send_text(&bob.did, "hello bob", "msg-init", "msg-init")
        .expect("alice init")["body"]
        .clone();
    clients
        .bob
        .process_incoming(notification(
            &alice.did,
            &bob.did,
            "msg-init",
            "application/anp-direct-init+json",
            init_body,
            1.0,
        ))
        .expect("bob accepts init");
    let reply_body = clients
        .bob
        .send_json(
            &alice.did,
            Map::from_iter([("ack".to_string(), Value::String("ok".to_string()))]),
            "msg-reply",
            "msg-reply",
        )
        .expect("bob first reply")["body"]
        .clone();
    clients
        .alice
        .process_incoming(notification(
            &bob.did,
            &alice.did,
            "msg-reply",
            "application/anp-direct-cipher+json",
            reply_body,
            2.0,
        ))
        .expect("alice confirms first reply");
    let mut corrupted_body = clients
        .alice
        .send_json(
            &bob.did,
            Map::from_iter([("event".to_string(), Value::String("wave".to_string()))]),
            "msg-2",
            "msg-2",
        )
        .expect("alice follow-up")["body"]
        .clone();
    let ciphertext = corrupted_body["ciphertext_b64u"]
        .as_str()
        .expect("ciphertext");
    corrupted_body["ciphertext_b64u"] = Value::String(corrupt_b64u_tail(ciphertext));

    let undecryptable = clients
        .bob
        .process_incoming(notification(
            &alice.did,
            &bob.did,
            "msg-2",
            "application/anp-direct-cipher+json",
            corrupted_body,
            2.0,
        ))
        .expect("decrypt failures return undecryptable");
    assert_eq!(undecryptable, json_map!({"state": "undecryptable"}));

    let unsupported = clients
        .bob
        .process_incoming(notification(
            &alice.did,
            &bob.did,
            "msg-x",
            "text/plain",
            json!({}),
            3.0,
        ))
        .expect_err("unsupported content type should fail");
    assert_eq!(unsupported, "unsupported content type: text/plain");

    std::fs::remove_dir_all(root).expect("remove temp test root");
}

#[test]
fn secure_e2ee_client_decrypt_history_page_sorts_by_server_seq_then_message_id_like_go() {
    let (_resolved, manager, root) = test_context("secure-client-history-sort");
    let alice = generated_identity_record(&manager, "alice", "alice-user", "alice");
    let bob = generated_identity_record(&manager, "bob", "bob-user", "bob");
    let mut clients = connected_clients(&manager, &alice, &bob);
    let first = notification(
        &alice.did,
        &bob.did,
        "msg-b",
        "application/seq-one-b",
        json!({}),
        1.0,
    );
    let mut earlier_tie = first.clone();
    earlier_tie
        .get_mut("meta")
        .and_then(Value::as_object_mut)
        .expect("meta")
        .insert("message_id".to_string(), Value::String("msg-a".to_string()));
    earlier_tie
        .get_mut("meta")
        .and_then(Value::as_object_mut)
        .expect("meta")
        .insert(
            "content_type".to_string(),
            Value::String("application/seq-one-a".to_string()),
        );
    let mut later = first.clone();
    later
        .get_mut("meta")
        .and_then(Value::as_object_mut)
        .expect("meta")
        .insert("message_id".to_string(), Value::String("msg-c".to_string()));
    later
        .get_mut("meta")
        .and_then(Value::as_object_mut)
        .expect("meta")
        .insert(
            "content_type".to_string(),
            Value::String("application/seq-two".to_string()),
        );
    later.insert("server_seq".to_string(), json!(2.0));

    let error = clients
        .bob
        .decrypt_history_page(vec![later, first, earlier_tie])
        .expect_err("sorted first unsupported message should stop history processing");

    assert_eq!(error, "unsupported content type: application/seq-one-a");

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

struct ConnectedClients {
    alice: MessageServiceE2EEClient,
    bob: MessageServiceE2EEClient,
}

fn connected_clients(
    manager: &Manager,
    alice: &awiki_cli::identity::types::StoredIdentity,
    bob: &awiki_cli::identity::types::StoredIdentity,
) -> ConnectedClients {
    let mut bob_seed = new_secure_e2ee_client_for_record(
        Some(manager),
        Some(bob),
        recording_rpc(Rc::new(RefCell::new(Vec::new())), |method, _params| {
            assert_eq!(method, "direct.e2ee.publish_prekey_bundle");
            Ok(Map::new())
        }),
    )
    .expect("construct bob seed client");
    let bob_bundle = bob_seed
        .ensure_fresh_prekey_bundle()
        .expect("seed bob prekey bundle");
    let bob_opk = first_one_time_prekey(manager, "bob");
    let bob_bundle_for_alice = bob_bundle.clone();
    let bob_opk_for_alice = bob_opk.clone();
    let alice_client = new_secure_e2ee_client_for_record(
        Some(manager),
        Some(alice),
        recording_rpc(Rc::new(RefCell::new(Vec::new())), move |method, params| match method {
            "direct.e2ee.get_prekey_bundle" => Ok(json_map!({
                "prekey_bundle": serde_json::to_value(&bob_bundle_for_alice).expect("bob bundle json"),
                "one_time_prekey": serde_json::to_value(&bob_opk_for_alice).expect("bob opk json"),
            })),
            "direct.send" => Ok(json_map!({
                "body": params["body"].clone()
            })),
            other => panic!("unexpected alice rpc method {other}"),
        }),
    )
    .expect("construct alice client");
    let bob_client = new_secure_e2ee_client_for_record(
        Some(manager),
        Some(bob),
        recording_rpc(
            Rc::new(RefCell::new(Vec::new())),
            |method, params| match method {
                "direct.send" => Ok(json_map!({
                    "body": params["body"].clone()
                })),
                other => panic!("unexpected bob rpc method {other}"),
            },
        ),
    )
    .expect("construct bob client");
    ConnectedClients {
        alice: alice_client,
        bob: bob_client,
    }
}

fn notification(
    sender_did: &str,
    recipient_did: &str,
    message_id: &str,
    content_type: &str,
    body: Value,
    server_seq: f64,
) -> Map<String, Value> {
    json_map!({
        "meta": {
            "sender_did": sender_did,
            "target": {
                "kind": "agent",
                "did": recipient_did,
            },
            "message_id": message_id,
            "profile": "anp.direct.e2ee.v1",
            "security_profile": "direct-e2ee",
            "content_type": content_type,
        },
        "body": body,
        "server_seq": server_seq,
    })
}

fn first_one_time_prekey(
    manager: &Manager,
    identity_name: &str,
) -> awiki_cli::anpsdk::OneTimePrekey {
    one_time_prekeys(manager, identity_name)
        .into_iter()
        .next()
        .expect("at least one OPK")
}

fn one_time_prekeys(
    manager: &Manager,
    identity_name: &str,
) -> Vec<awiki_cli::anpsdk::OneTimePrekey> {
    let paths = manager
        .paths_for_identity(identity_name)
        .expect("identity paths");
    let mut prekeys = std::fs::read_dir(Path::new(&paths.identity_dir).join("p5-one-time-prekeys"))
        .expect("read OPK root")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("json"))
        .map(|path| {
            serde_json::from_slice(&std::fs::read(&path).expect("read OPK json"))
                .expect("parse OPK json")
        })
        .collect::<Vec<_>>();
    prekeys
        .sort_by(|left: &awiki_cli::anpsdk::OneTimePrekey, right| left.key_id.cmp(&right.key_id));
    prekeys
}

fn session_for_peer(
    manager: &Manager,
    identity_name: &str,
    peer_did: &str,
) -> Option<awiki_cli::anpsdk::DirectSessionState> {
    let paths = manager
        .paths_for_identity(identity_name)
        .expect("identity paths");
    let mut session_paths =
        std::fs::read_dir(Path::new(&paths.identity_dir).join("p5-e2ee-sessions"))
            .expect("read session root")
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("json"))
            .collect::<Vec<_>>();
    session_paths.sort();
    for path in session_paths {
        let session: awiki_cli::anpsdk::DirectSessionState =
            serde_json::from_slice(&std::fs::read(&path).expect("read session json"))
                .expect("parse session json");
        if session.peer_did == peer_did {
            return Some(session);
        }
    }
    None
}

fn pending_cipher_body(session_id: &str, n: &str) -> Value {
    json!({
        "session_id": session_id,
        "suite": "ANP-DIRECT-E2EE-X3DH-25519-CHACHA20POLY1305-SHA256-V1",
        "ratchet_header": {
            "dh_pub_b64u": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
            "pn": "0",
            "n": n,
        },
        "ciphertext_b64u": "AAAAAAAAAAAAAAAAAAAAAA",
    })
}

fn corrupt_b64u_tail(value: &str) -> String {
    if value.is_empty() {
        return "A".to_string();
    }
    let replacement = if value.ends_with('A') { 'B' } else { 'A' };
    format!("{}{}", &value[..value.len() - 1], replacement)
}

fn remote_prekey_bundle(record: &awiki_cli::identity::types::StoredIdentity) -> Value {
    let signing_private = awiki_cli::anpsdk::PrivateKeyMaterial::from_pem(&record.key1_private_pem)
        .expect("record signing private key");
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
