use super::*;

use crate::identity::DeviceProof;

const DID: &str = "did:wba:awiki.test:alice";
const DOCUMENT_HASH: &str = "sha256:UD5TmycQ6gS539AFNjM5cGoQUmeq2fQGPpwD00lMPlg";

#[test]
fn token_bearing_calls_are_exact_and_redacted_in_debug() {
    let account_token = SecretBytes::from_vec(b"account-secret-token".to_vec());
    let join_token = SecretBytes::from_vec(b"join-secret-token".to_vec());
    let join_request = sample_join_request();
    let create = build_create_call(DeviceJoinRemoteCreateRequest {
        operation_id: "op-create-1",
        account_verification_token: &account_token,
        join_request: &join_request,
    })
    .unwrap();

    assert_eq!(create.method, DEVICE_JOIN_CREATE_METHOD);
    assert_eq!(
        create
            .params
            .get("account_verification_token")
            .and_then(Value::as_str),
        Some("account-secret-token")
    );
    assert_eq!(create.params.as_object().unwrap().len(), 3);
    let debug = format!("{create:?}");
    assert!(!debug.contains("account-secret-token"));
    assert!(debug.contains("<redacted>"));

    let status = build_new_status_call(&join_token).unwrap();
    assert_eq!(
        status.params,
        json!({"join_session_token": "join-secret-token"})
    );
    assert!(!format!("{status:?}").contains("join-secret-token"));

    let response = sample_response();
    let response_call = build_response_call(DeviceJoinRemoteResponseRequest {
        join_session_token: &join_token,
        response: &response,
    })
    .unwrap();
    assert_eq!(response_call.params.as_object().unwrap().len(), 8);
    assert_eq!(
        response_call
            .params
            .get("join_session_token")
            .and_then(Value::as_str),
        Some("join-secret-token")
    );
    assert!(!format!("{response_call:?}").contains("join-secret-token"));
}

#[test]
fn registry_defaults_missing_pending_list_but_rejects_unrequested_projection() {
    let did = crate::ids::Did::parse(DID).unwrap();
    let without_pending = json!({
        "did": DID,
        "checkpoint": checkpoint_json(),
        "devices": [device_json("dev-admin", "admin", true)],
    });
    let parsed = parse_registry_result(without_pending, &did, true).unwrap();
    assert!(parsed.pending_join_requests.is_empty());
    assert_eq!(parsed.devices.len(), 1);

    let with_pending = json!({
        "did": DID,
        "checkpoint": checkpoint_json(),
        "devices": [device_json("dev-admin", "admin", true)],
        "pending_join_requests": [pending_json()],
    });
    assert!(parse_registry_result(with_pending.clone(), &did, false).is_err());
    let parsed = parse_registry_result(with_pending, &did, true).unwrap();
    assert_eq!(parsed.pending_join_requests.len(), 1);
    assert_eq!(
        parsed.pending_join_requests[0].requested_role,
        DeviceAuthorizationRole::Member
    );
}

#[test]
fn registry_rejects_unknown_or_invalid_nested_fields() {
    let did = crate::ids::Did::parse(DID).unwrap();
    let mut unknown = json!({
        "did": DID,
        "checkpoint": checkpoint_json(),
        "devices": [],
        "unexpected": true,
    });
    assert!(parse_registry_result(unknown.clone(), &did, false).is_err());

    unknown.as_object_mut().unwrap().remove("unexpected");
    unknown["checkpoint"]["unexpected"] = Value::Bool(true);
    assert!(parse_registry_result(unknown, &did, false).is_err());

    let invalid_hash = json!({
        "did": DID,
        "checkpoint": {
            "document_version": 1,
            "document_hash": "sha256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAB",
            "registry_version": 1,
        },
        "devices": [],
    });
    assert!(parse_registry_result(invalid_hash, &did, false).is_err());
}

#[test]
fn status_views_are_strictly_separated_and_enforce_state_invariants() {
    let pending = json!({
        "join_session_id": "join-1",
        "state": "pending",
        "expires_at": "2026-07-19T00:10:00Z",
    });
    assert!(parse_new_status_result(pending.clone(), "join-1").is_ok());

    let mut leaked_response = pending.clone();
    leaked_response["challenge_response"] = serde_json::to_value(sample_response()).unwrap();
    assert!(parse_new_status_result(leaked_response, "join-1").is_err());

    let mut invalid_pending = pending;
    invalid_pending["challenge"] = serde_json::to_value(sample_challenge()).unwrap();
    assert!(parse_new_status_result(invalid_pending, "join-1").is_err());

    let new_response_verified = json!({
        "join_session_id": "join-1",
        "state": "response_verified",
        "expires_at": "2026-07-19T00:10:00Z",
        "challenge": sample_challenge(),
    });
    assert!(parse_new_status_result(new_response_verified, "join-1").is_ok());

    let admin_response_verified = json!({
        "join_session_id": "join-1",
        "state": "response_verified",
        "expires_at": "2026-07-19T00:10:00Z",
        "challenge": sample_challenge(),
        "challenge_response": sample_response(),
    });
    let parsed = parse_admin_status_result(admin_response_verified, "join-1").unwrap();
    assert!(parsed.challenge_response.is_some());

    let admin_missing_response = json!({
        "join_session_id": "join-1",
        "state": "response_verified",
        "expires_at": "2026-07-19T00:10:00Z",
        "challenge": sample_challenge(),
    });
    assert!(parse_admin_status_result(admin_missing_response, "join-1").is_err());
}

#[test]
fn status_rejects_unknown_nested_proof_and_mismatched_bindings() {
    let mut challenge = serde_json::to_value(sample_challenge()).unwrap();
    challenge["authorizing_device_proof"]["unexpected"] = Value::Bool(true);
    let nested_unknown = json!({
        "join_session_id": "join-1",
        "state": "challenge_sent",
        "expires_at": "2026-07-19T00:10:00Z",
        "challenge": challenge,
    });
    assert!(parse_new_status_result(nested_unknown, "join-1").is_err());

    let mut response = sample_response();
    response.challenge_id = "challenge-other".to_owned();
    let mismatch = json!({
        "join_session_id": "join-1",
        "state": "response_verified",
        "expires_at": "2026-07-19T00:10:00Z",
        "challenge": sample_challenge(),
        "challenge_response": response,
    });
    assert!(parse_admin_status_result(mismatch, "join-1").is_err());
}

#[test]
fn consumed_status_requires_authorization_and_transition_results_match_inputs() {
    let consumed = json!({
        "join_session_id": "join-1",
        "state": "consumed",
        "expires_at": "2026-07-19T00:10:00Z",
        "authorization": {
            "checkpoint": checkpoint_json(),
            "device": device_json("dev-new", "member", false),
        },
    });
    assert!(parse_new_status_result(consumed, "join-1").is_ok());

    let missing_authorization = json!({
        "join_session_id": "join-1",
        "state": "consumed",
        "expires_at": "2026-07-19T00:10:00Z",
    });
    assert!(parse_new_status_result(missing_authorization, "join-1").is_err());

    let challenge = sample_challenge();
    assert!(parse_challenge_result(
        json!({
            "join_session_id": "join-1",
            "state": "challenge_sent",
            "challenge_id": "challenge-1",
        }),
        &challenge,
    )
    .is_ok());
    assert!(parse_challenge_result(
        json!({
            "join_session_id": "join-1",
            "state": "challenge_sent",
            "challenge_id": "challenge-other",
        }),
        &challenge,
    )
    .is_err());
}

fn checkpoint_json() -> Value {
    json!({
        "document_version": 7,
        "document_hash": DOCUMENT_HASH,
        "registry_version": 3,
    })
}

fn device_json(device_id: &str, role: &str, management_ready: bool) -> Value {
    json!({
        "device_id": device_id,
        "signing_key_id": format!("{DID}#{device_id}-sign"),
        "e2ee_key_id": format!("{DID}#{device_id}-e2ee"),
        "status": "active",
        "role": role,
        "management_ready": management_ready,
        "auth_generation": 1,
    })
}

fn pending_json() -> Value {
    json!({
        "join_session_id": "join-1",
        "device_id": "dev-new",
        "signing_key_id": format!("{DID}#dev-new-sign"),
        "e2ee_key_id": format!("{DID}#dev-new-e2ee"),
        "requested_role": "member",
        "issued_at": "2026-07-19T00:00:00Z",
        "expires_at": "2026-07-19T00:10:00Z",
    })
}

fn sample_join_request() -> DeviceJoinRequest {
    DeviceJoinRequest {
        request_type: crate::identity::DEVICE_JOIN_REQUEST_TYPE.to_owned(),
        did: DID.to_owned(),
        join_session_id: "join-1".to_owned(),
        device_id: "dev-new".to_owned(),
        signing_public_key: json!({
            "id": format!("{DID}#dev-new-sign"),
            "type": "Multikey",
            "controller": DID,
            "publicKeyMultibase": "zSigning",
        }),
        e2ee_public_key: json!({
            "id": format!("{DID}#dev-new-e2ee"),
            "type": "X25519KeyAgreementKey2019",
            "controller": DID,
            "publicKeyMultibase": "zE2ee",
        }),
        pairing_public_key: "pairing".to_owned(),
        profiles: Vec::new(),
        requested_role: "member".to_owned(),
        issued_at: "2026-07-19T00:00:00Z".to_owned(),
        expires_at: "2026-07-19T00:10:00Z".to_owned(),
        signature: "signature".to_owned(),
    }
}

fn sample_challenge() -> DeviceJoinChallenge {
    DeviceJoinChallenge {
        operation_id: "op-challenge-1".to_owned(),
        join_session_id: "join-1".to_owned(),
        challenge_id: "challenge-1".to_owned(),
        admin_device_id: "dev-admin".to_owned(),
        admin_pairing_public_key: "admin-pairing".to_owned(),
        ciphertext: crate::identity::EncryptedJoinChallenge {
            algorithm: crate::identity::DEVICE_JOIN_CHALLENGE_ALGORITHM.to_owned(),
            nonce_b64u: "nonce".to_owned(),
            ciphertext_b64u: "ciphertext".to_owned(),
        },
        challenge_expires_at: "2026-07-19T00:05:00Z".to_owned(),
        authorizing_device_proof: DeviceProof {
            proof_type: crate::identity::DEVICE_PROOF_TYPE.to_owned(),
            key_id: format!("{DID}#dev-admin-sign"),
            created_at: "2026-07-19T00:00:00Z".to_owned(),
            expires_at: "2026-07-19T00:05:00Z".to_owned(),
            nonce: "nonce".to_owned(),
            signature: "signature".to_owned(),
        },
    }
}

fn sample_response() -> DeviceJoinChallengeResponse {
    DeviceJoinChallengeResponse {
        operation_id: "op-response-1".to_owned(),
        join_session_id: "join-1".to_owned(),
        challenge_id: "challenge-1".to_owned(),
        challenge_hash: "sha256:challenge".to_owned(),
        join_request_hash: "sha256:join".to_owned(),
        pairing_transcript_hash: "sha256:transcript".to_owned(),
        new_device_proof: DeviceProof {
            proof_type: crate::identity::DEVICE_PROOF_TYPE.to_owned(),
            key_id: format!("{DID}#dev-new-sign"),
            created_at: "2026-07-19T00:00:00Z".to_owned(),
            expires_at: "2026-07-19T00:05:00Z".to_owned(),
            nonce: "nonce".to_owned(),
            signature: "signature".to_owned(),
        },
    }
}
