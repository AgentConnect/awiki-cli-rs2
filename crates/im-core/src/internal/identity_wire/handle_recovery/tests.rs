use super::*;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ed25519_dalek::Verifier as _;
use serde_json::json;

#[test]
fn otp_target_matches_frozen_contract_vectors() {
    assert_eq!(
        recovery_otp_target("alice.awiki.info", "recover-001").unwrap(),
        "sha256:9eab9450e7ea90b353c8564eff52cffc476c1691879aaf5ac013d451e18d5c2b"
    );
    assert_eq!(
        recovery_otp_target("bob.chat.example", "恢复-42").unwrap(),
        "sha256:89c43b77ab12e058b2e060410b691e0380aabe0ac163e69153aeceac70d5ec5b"
    );
}

#[test]
fn otp_send_uses_closed_canonical_inputs() {
    let send = build_send_otp_call("+8613800000000", "alice.awiki.info", "recover-001").unwrap();
    assert_eq!(send.endpoint, super::super::HANDLE_RPC_ENDPOINT);
    assert_eq!(send.method, "send_otp");
    assert_eq!(
        send.params,
        json!({
            "phone": "+8613800000000",
            "purpose": HANDLE_RECOVERY_PURPOSE,
            "handle": "alice",
            "domain": "awiki.info",
            "full_handle": "alice.awiki.info",
            "operation_id": "recover-001",
        })
    );

    assert!(build_send_otp_call("+8613800000000", "Alice.awiki.info", "recover-001").is_err());
    assert!(build_send_otp_call("+8613800000000", "alice.awiki.info", "bad operation",).is_err());
}

#[test]
fn attestation_issue_wire_contract_is_closed_and_secret_safe() {
    let call = build_attestation_issue_call_v1("recover-v4-001").unwrap();
    assert_eq!(call.endpoint, super::super::DID_AUTH_RPC_ENDPOINT);
    assert_eq!(call.method, HANDLE_RECOVERY_ATTESTATION_ISSUE_V1_METHOD);
    assert_eq!(call.params, json!({"operation_id": "recover-v4-001"}));
    assert!(build_attestation_issue_call_v1("bad operation").is_err());

    let value = parse_attestation_issue_result_v1(json!({
        "attestation": "headerheader.payloadpayload.signaturesignature",
        "expires_at": "2026-08-22T12:00:00Z",
    }))
    .unwrap();
    assert_eq!(
        value.expose_attestation(),
        "headerheader.payloadpayload.signaturesignature"
    );
    assert!(!format!("{value:?}").contains("headerheader.payloadpayload.signaturesignature"));
    assert!(parse_attestation_issue_result_v1(json!({
        "attestation": "headerheader.payloadpayload.signaturesignature",
        "expires_at": "2026-08-22T12:00:00Z",
        "claims": {"previous_did": "must-not-cross"},
    }))
    .is_err());
    assert!(parse_attestation_issue_result_v1(json!({
        "attestation": "not a jwt",
        "expires_at": "2026-08-22T12:00:00Z",
    }))
    .is_err());
}

#[test]
fn join_transition_response_is_closed_and_secret_safe() {
    let transition = parse_account_verification_result(json!({
        "account_verification_token": "join-secret",
        "purpose": "awiki.device.join.v1",
        "expires_at": "2026-08-03T12:05:00Z",
        "account_user_id": "user-1",
        "handle": "alice.example.invalid",
        "did": "did:wba:example.invalid:users:alice-new",
        "identity_transition": {
            "kind": "handle_recovery",
            "previous_did": "did:wba:example.invalid:users:alice-old",
            "current_did": "did:wba:example.invalid:users:alice-new",
            "binding_generation": "8"
        }
    }))
    .unwrap();
    assert!(transition.identity_transition.is_some());
    assert!(!format!("{transition:?}").contains("join-secret"));

    let partial = json!({
        "account_verification_token": "join-secret",
        "purpose": "awiki.device.join.v1",
        "expires_at": "2026-08-03T12:05:00Z",
        "account_user_id": "user-1"
    });
    assert!(parse_account_verification_result(partial).is_err());
}

#[test]
fn v4_intent_matches_frozen_jcs_and_hash_golden_vector() {
    let intent = golden_intent_v4();
    assert_eq!(
        String::from_utf8(canonical_intent_v4(&intent).unwrap()).unwrap(),
        concat!(
            "{\"account_user_id\":\"user-fixture-1\",",
            "\"bootstrap_device_id\":\"device-fixture-new\",",
            "\"bootstrap_signing_key_id\":\"did:wba:example.invalid:users:alice-new#device-signing-key-1\",",
            "\"bootstrap_signing_public_key\":{\"crv\":\"Ed25519\",\"kty\":\"OKP\",\"x\":\"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\"},",
            "\"contract_version\":\"awiki.handle-recovery.v1.contract.4.20260807\",",
            "\"expected_binding_generation\":\"7\",",
            "\"expected_previous_did\":\"did:wba:example.invalid:users:alice-old\",",
            "\"full_handle\":\"alice.example.invalid\",",
            "\"new_did\":\"did:wba:example.invalid:users:alice-new\",",
            "\"new_did_document_hash\":\"sha256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\",",
            "\"operation_id\":\"recover-v4-001\",",
            "\"schema_version\":\"1\"}"
        )
    );
    assert_eq!(
        intent_hash_v4(&intent).unwrap(),
        "sha256:SlQnFpLKCK0OFEKnA2492wGZ8WsD_w35-l_wTccWbUA"
    );
}

#[test]
fn v4_exchange_uses_exact_closed_request_and_authoritative_response() {
    let call = build_grant_exchange_call_v4(
        "+8613800000000",
        " 12 34 56 ",
        "alice.example.invalid",
        "recover-v4-001",
        "did:wba:example.invalid:users:alice-new#device-signing-key-1",
        &json!({
            "kty": "OKP",
            "crv": "Ed25519",
            "x": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
        }),
    )
    .unwrap();
    assert_eq!(call.endpoint, HANDLE_RECOVERY_V4_EXCHANGE_ENDPOINT);
    assert_eq!(call.method, "POST");
    assert_eq!(
        call.body,
        json!({
            "contract_version": HANDLE_RECOVERY_V4_CONTRACT_VERSION,
            "phone": "+8613800000000",
            "code": "123456",
            "full_handle": "alice.example.invalid",
            "operation_id": "recover-v4-001",
            "bootstrap_signing_key_id": "did:wba:example.invalid:users:alice-new#device-signing-key-1",
            "bootstrap_signing_public_key": {
                "kty": "OKP",
                "crv": "Ed25519",
                "x": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
            }
        })
    );
    assert!(call.body.get("handle").is_none());

    let parsed = parse_grant_exchange_result_v4(
        json!({
            "contract_version": HANDLE_RECOVERY_V4_CONTRACT_VERSION,
            "recovery_grant": "secret-grant-v4",
            "purpose": HANDLE_RECOVERY_PURPOSE,
            "expires_at": "2026-08-07T12:05:00Z",
            "current_binding": {
                "account_user_id": "user-fixture-1",
                "full_handle": "alice.example.invalid",
                "current_did": "did:wba:example.invalid:users:alice-old",
                "binding_generation": "7"
            }
        }),
        "alice.example.invalid",
    )
    .unwrap();
    assert_eq!(parsed.current_binding.binding_generation, "7");
    assert!(!format!("{parsed:?}").contains("secret-grant-v4"));

    assert!(parse_grant_exchange_result_v4(
        json!({
            "contract_version": HANDLE_RECOVERY_V4_CONTRACT_VERSION,
            "recovery_grant": "secret-grant-v4",
            "purpose": HANDLE_RECOVERY_PURPOSE,
            "expires_at": "2026-08-07T12:05:00.000Z",
            "current_binding": {
                "account_user_id": "user-fixture-1",
                "full_handle": "alice.example.invalid",
                "current_did": "did:wba:example.invalid:users:alice-old",
                "binding_generation": "7"
            },
            "unexpected": true
        }),
        "alice.example.invalid"
    )
    .is_err());
}

#[test]
fn v4_commit_and_result_get_sign_distinct_exact_transcripts() {
    let signing_key = ed25519_dalek::SigningKey::from_bytes(&[7_u8; 32]);
    let private = anp::PrivateKeyMaterial::Ed25519(signing_key.clone());
    let document = json!({
        "id": "did:wba:example.invalid:users:alice-new",
        "nested": {"proof": "preserved"},
        "proof": {"proofValue": "removed-only-from-hash"}
    });
    let intent = signing_intent_v4(&signing_key, &document);
    let intent_hash = intent_hash_v4(&intent).unwrap();
    let commit = prepare_commit_v4(CommitProofInputV4 {
        proof: KeyPossessionProofInputV4 {
            intent: &intent,
            intent_hash: &intent_hash,
            audience: "awiki-user-service-recovery",
            created_at: "2026-08-07T12:00:00Z",
            expires_at: "2026-08-07T12:02:00Z",
            nonce: &[9_u8; 32],
        },
        recovery_grant: SecretBytes::from_vec(b"secret-grant-v4".to_vec()),
        predecessor_did_document: json!({
            "id": "did:wba:example.invalid:users:alice-old",
            "deactivated": true,
            "successorDid": "did:wba:example.invalid:users:alice-new",
            "proof": {"type": "DataIntegrityProof"}
        }),
        new_did_document: document,
    })
    .unwrap();
    let commit_signature = private.sign_message(commit.signing_input()).unwrap();
    let commit = complete_commit_v4(commit, &commit_signature).unwrap();
    assert_eq!(commit.call.method, HANDLE_RECOVERY_COMMIT_V4_METHOD);
    assert_eq!(
        sorted_keys(&commit.call.params),
        vec![
            "bootstrap_key_possession_proof",
            "intent",
            "intent_hash",
            "new_did_document",
            "predecessor_did_document",
            "recovery_grant",
        ]
    );
    assert_eq!(
        sorted_keys(&commit.signed_object),
        vec![
            "audience",
            "created_at",
            "expires_at",
            "intent_hash",
            "key_id",
            "method",
            "nonce",
            "operation_id",
            "purpose",
            "type",
        ]
    );
    assert_eq!(
        commit.signed_object["purpose"],
        HANDLE_RECOVERY_COMMIT_V4_PURPOSE
    );
    assert_v4_proof_signature(&signing_key, &commit.signed_object, &commit.call.params);

    let result_get = prepare_result_get_v4(KeyPossessionProofInputV4 {
        intent: &intent,
        intent_hash: &intent_hash,
        audience: "awiki-user-service-recovery",
        created_at: "2026-08-07T12:00:00Z",
        expires_at: "2026-08-07T12:02:00Z",
        nonce: &[10_u8; 32],
    })
    .unwrap();
    let result_signature = private.sign_message(result_get.signing_input()).unwrap();
    let result_get = complete_result_get_v4(result_get, &result_signature).unwrap();
    assert_eq!(result_get.call.method, HANDLE_RECOVERY_RESULT_GET_V4_METHOD);
    assert_eq!(
        sorted_keys(&result_get.call.params),
        vec![
            "bootstrap_key_possession_proof",
            "contract_version",
            "intent",
            "intent_hash",
        ]
    );
    assert_eq!(
        result_get.signed_object["purpose"],
        HANDLE_RECOVERY_RESULT_GET_V4_PURPOSE
    );
    assert_ne!(commit.signed_object, result_get.signed_object);
    assert_v4_proof_signature(
        &signing_key,
        &result_get.signed_object,
        &result_get.call.params,
    );
}

#[test]
fn v4_proof_rejects_wrong_nonce_timestamp_lifetime_and_intent_hash() {
    let signing_key = ed25519_dalek::SigningKey::from_bytes(&[7_u8; 32]);
    let document = json!({"id": "did:wba:example.invalid:users:alice-new"});
    let intent = signing_intent_v4(&signing_key, &document);
    let hash = intent_hash_v4(&intent).unwrap();

    let prepare = |nonce: &[u8], created_at: &str, expires_at: &str, intent_hash: &str| {
        prepare_result_get_v4(KeyPossessionProofInputV4 {
            intent: &intent,
            intent_hash,
            audience: "awiki-user-service-recovery",
            created_at,
            expires_at,
            nonce,
        })
    };
    assert!(prepare(
        &[1_u8; 31],
        "2026-08-07T12:00:00Z",
        "2026-08-07T12:02:00Z",
        &hash
    )
    .is_err());
    assert!(prepare(
        &[1_u8; 32],
        "2026-08-07T12:00:00.000Z",
        "2026-08-07T12:02:00Z",
        &hash
    )
    .is_err());
    assert!(prepare(
        &[1_u8; 32],
        "2026-08-07T12:00:00Z",
        "2026-08-07T12:02:01Z",
        &hash
    )
    .is_err());
    assert!(prepare(
        &[1_u8; 32],
        "2026-08-07T12:00:00Z",
        "2026-08-07T12:02:00Z",
        "sha256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
    )
    .is_err());
}

#[test]
fn v4_result_get_union_is_closed_and_absent_is_non_terminal() {
    let operation_id = "recover-v4-001";
    let intent_hash = "sha256:SlQnFpLKCK0OFEKnA2492wGZ8WsD_w35-l_wTccWbUA";
    let result = committed_result_v4(operation_id, intent_hash);
    assert!(matches!(
        parse_result_get_v4(
            json!({"state": "committed", "result": result}),
            operation_id,
            intent_hash
        )
        .unwrap(),
        RecoveryResultGetV4::Committed(_)
    ));
    assert_eq!(
        parse_result_get_v4(
            json!({
                "state": "result_absent",
                "padding": HANDLE_RECOVERY_RESULT_ABSENT_PADDING
            }),
            operation_id,
            intent_hash
        )
        .unwrap(),
        RecoveryResultGetV4::ResultAbsent
    );
    assert!(parse_result_get_v4(
        json!({
            "state": "result_absent",
            "padding": "short",
        }),
        operation_id,
        intent_hash
    )
    .is_err());
    assert!(parse_result_get_v4(
        json!({
            "state": "result_absent",
            "padding": HANDLE_RECOVERY_RESULT_ABSENT_PADDING,
            "terminal": true
        }),
        operation_id,
        intent_hash
    )
    .is_err());
}

#[test]
fn v4_stable_error_tables_match_frozen_contract() {
    let server = [
        ("handle_recovery.invalid_request", -32004, false),
        ("handle_recovery.capability_disabled", -32001, false),
        ("handle_recovery.grant_invalid", -32000, false),
        ("handle_recovery.grant_expired", -32000, true),
        ("handle_recovery.proof_invalid", -32004, false),
        ("handle_recovery.intent_conflict", -32003, false),
        (
            "handle_recovery.state_changed_requires_new_operation",
            -32003,
            false,
        ),
        ("handle_recovery.temporarily_unavailable", -32004, true),
    ];
    for (code, rpc_code, retryable) in server {
        let parsed = RecoveryServerErrorCodeV4::parse(code).unwrap();
        assert_eq!(parsed.as_str(), code);
        assert_eq!(parsed.json_rpc_code(), rpc_code);
        assert_eq!(parsed.retryable(), retryable);
    }
    assert!(RecoveryServerErrorCodeV4::parse("handle_recovery.unknown").is_none());

    let exchange = [
        ("handle_recovery_exchange.invalid_request", false),
        ("handle_recovery_exchange.factor_invalid", false),
        ("handle_recovery_exchange.capability_disabled", false),
        ("handle_recovery_exchange.rate_limited", true),
        ("handle_recovery_exchange.temporarily_unavailable", true),
    ];
    for (code, retryable) in exchange {
        let parsed = RecoveryExchangeErrorCodeV4::parse(code).unwrap();
        assert_eq!(parsed.as_str(), code);
        assert_eq!(parsed.retryable(), retryable);
    }
    assert!(RecoveryExchangeErrorCodeV4::parse("handle_recovery_exchange.unknown").is_none());
}

fn golden_intent_v4() -> crate::internal::identity_handle_recovery_pending::RecoveryIntentV4 {
    crate::internal::identity_handle_recovery_pending::RecoveryIntentV4 {
        schema_version: "1".to_owned(),
        contract_version: HANDLE_RECOVERY_V4_CONTRACT_VERSION.to_owned(),
        operation_id: "recover-v4-001".to_owned(),
        account_user_id: "user-fixture-1".to_owned(),
        full_handle: "alice.example.invalid".to_owned(),
        expected_previous_did: "did:wba:example.invalid:users:alice-old".to_owned(),
        expected_binding_generation: "7".to_owned(),
        new_did: "did:wba:example.invalid:users:alice-new".to_owned(),
        new_did_document_hash: "sha256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".to_owned(),
        bootstrap_device_id: "device-fixture-new".to_owned(),
        bootstrap_signing_key_id: "did:wba:example.invalid:users:alice-new#device-signing-key-1"
            .to_owned(),
        bootstrap_signing_public_key: json!({
            "kty": "OKP",
            "crv": "Ed25519",
            "x": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
        }),
    }
}

fn signing_intent_v4(
    signing_key: &ed25519_dalek::SigningKey,
    document: &serde_json::Value,
) -> crate::internal::identity_handle_recovery_pending::RecoveryIntentV4 {
    let mut intent = golden_intent_v4();
    intent.new_did_document_hash = new_did_document_hash_v4(document).unwrap();
    intent.bootstrap_signing_public_key = json!({
        "kty": "OKP",
        "crv": "Ed25519",
        "x": URL_SAFE_NO_PAD.encode(signing_key.verifying_key().to_bytes())
    });
    intent
}

fn assert_v4_proof_signature(
    signing_key: &ed25519_dalek::SigningKey,
    signed_object: &serde_json::Value,
    params: &serde_json::Value,
) {
    let bytes = serde_json_canonicalizer::to_vec(signed_object).unwrap();
    let encoded = params["bootstrap_key_possession_proof"]["signature"]
        .as_str()
        .unwrap();
    let signature_bytes: [u8; 64] = URL_SAFE_NO_PAD.decode(encoded).unwrap().try_into().unwrap();
    let signature = ed25519_dalek::Signature::from_bytes(&signature_bytes);
    signing_key
        .verifying_key()
        .verify(&bytes, &signature)
        .unwrap();
}

fn committed_result_v4(operation_id: &str, intent_hash: &str) -> serde_json::Value {
    json!({
        "state": "recovered",
        "operation_id": operation_id,
        "intent_hash": intent_hash,
        "intent_schema_version": "1",
        "contract_version": HANDLE_RECOVERY_V4_CONTRACT_VERSION,
        "account_user_id": "user-fixture-1",
        "full_handle": "alice.example.invalid",
        "previous_did": "did:wba:example.invalid:users:alice-old",
        "current_did": "did:wba:example.invalid:users:alice-new",
        "binding_generation": "8",
        "checkpoint": {
            "document_version": 1,
            "document_hash": "sha256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
            "registry_version": 1
        },
        "bootstrap_device": {
            "device_id": "device-fixture-new",
            "status": "active",
            "role": "admin",
            "management_ready": true,
            "auth_generation": 1
        },
        "committed_at": "2026-08-07T12:00:00Z"
    })
}

fn sorted_keys(value: &serde_json::Value) -> Vec<&str> {
    let mut keys = value
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect::<Vec<_>>();
    keys.sort_unstable();
    keys
}
