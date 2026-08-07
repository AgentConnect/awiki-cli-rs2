use super::*;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
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
fn otp_send_and_exchange_share_closed_canonical_inputs() {
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

    let exchange = build_grant_exchange_call(
        "+8613800000000",
        " 12 34 56 ",
        "alice.awiki.info",
        "recover-001",
    )
    .unwrap();
    assert_eq!(
        exchange.endpoint,
        super::super::HANDLE_RECOVERY_EXCHANGE_ENDPOINT
    );
    assert_eq!(
        exchange.body,
        json!({
            "phone": "+8613800000000",
            "code": "123456",
            "handle": "alice.awiki.info",
            "operation_id": "recover-001",
        })
    );

    assert!(build_send_otp_call("+8613800000000", "Alice.awiki.info", "recover-001").is_err());
    assert!(build_grant_exchange_call(
        "+8613800000000",
        "123456",
        "alice.awiki.info",
        "bad operation",
    )
    .is_err());
}

#[test]
fn grant_and_join_transition_responses_are_closed_and_secret_safe() {
    let grant = parse_grant_exchange_result(json!({
        "recovery_grant": "secret-grant",
        "purpose": HANDLE_RECOVERY_PURPOSE,
        "expires_at": "2026-08-03T12:05:00Z"
    }))
    .unwrap();
    assert!(!format!("{grant:?}").contains("secret-grant"));
    assert!(parse_grant_exchange_result(json!({
        "recovery_grant": "secret-grant",
        "purpose": HANDLE_RECOVERY_PURPOSE,
        "expires_at": "2026-08-03T12:05:00Z",
        "unexpected": true
    }))
    .is_err());

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
fn commit_proof_signs_exact_document_projection_and_hash() {
    let private =
        anp::PrivateKeyMaterial::Ed25519(ed25519_dalek::SigningKey::from_bytes(&[7_u8; 32]));
    let document = json!({
        "id": "did:wba:example.invalid:users:alice-new",
        "nested": {"proof": "must-remain"},
        "proof": {"proofValue": "must-be-removed"}
    });
    let prepared = prepare_commit(CommitProofInput {
        operation_id: "recover-001",
        handle: "alice.example.invalid",
        recovery_grant: SecretBytes::from_vec(b"secret-grant".to_vec()),
        expected_binding_generation: "7",
        new_did_document: document,
        bootstrap_device_id: "device-new",
        bootstrap_signing_key_id: "did:wba:example.invalid:users:alice-new#device-new-sign",
        bootstrap_signing_private_key: &private,
        created_at: "2026-08-03T12:00:00Z",
        expires_at: "2026-08-03T12:02:00Z",
        nonce: &[9_u8; 24],
    })
    .unwrap();

    assert_eq!(prepared.call.method, HANDLE_RECOVERY_COMMIT_METHOD);
    assert_eq!(
        prepared.signed_params["new_did_document"]["nested"]["proof"],
        "must-remain"
    );
    assert!(prepared.signed_params["new_did_document"]
        .get("proof")
        .is_none());
    assert!(prepared.call.params["new_did_document"]
        .get("proof")
        .is_some());
    assert!(prepared.request_hash.starts_with("sha256:"));
    assert_eq!(
        URL_SAFE_NO_PAD
            .decode(
                prepared.call.params["bootstrap_device_proof"]["signature"]
                    .as_str()
                    .unwrap()
            )
            .unwrap()
            .len(),
        64
    );
    assert!(!format!("{prepared:?}").contains("secret-grant"));
}
