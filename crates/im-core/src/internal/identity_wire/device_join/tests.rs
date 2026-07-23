use super::*;
use crate::identity::DeviceJoinObjectProof;
use crate::internal::identity_device_join_runtime::{
    DeviceJoinRemoteCancelRequest, DeviceJoinRemoteRejectRequest,
};
use crate::internal::platform_secret::SecretBytes;
use serde_json::json;

fn proof() -> DeviceJoinObjectProof {
    DeviceJoinObjectProof {
        proof_type: "DataIntegrityProof".to_owned(),
        cryptosuite: "eddsa-jcs-2022".to_owned(),
        verification_method: "did:wba:example.com:user:alice#device-signing".to_owned(),
        proof_purpose: "authentication".to_owned(),
        created: "2026-07-18T12:00:00Z".to_owned(),
        proof_value: "zProof".to_owned(),
    }
}

#[test]
fn registry_request_never_exposes_pending_join_projection() {
    let did = crate::ids::Did::parse("did:wba:example.com:user:alice").unwrap();
    let call = build_registry_call(&did, true);
    assert_eq!(call.method, DEVICE_REGISTRY_GET_METHOD);
    assert_eq!(call.params, json!({"did": did.as_str()}));
}

#[test]
fn reject_reason_is_a_closed_set() {
    let proof = proof();
    let accepted = build_reject_call(DeviceJoinRemoteRejectRequest {
        operation_id: "reject-1",
        join_session_id: "join-1",
        rejecting_device_id: "device-admin",
        reason: "sas_mismatch",
        proof: &proof,
    })
    .unwrap();
    assert_eq!(accepted.params["reason"], "sas_mismatch");

    assert!(build_reject_call(DeviceJoinRemoteRejectRequest {
        operation_id: "reject-2",
        join_session_id: "join-1",
        rejecting_device_id: "device-admin",
        reason: "arbitrary",
        proof: &proof,
    })
    .is_err());
}

#[test]
fn cancel_reason_is_a_closed_set_and_token_is_present() {
    let token = SecretBytes::from_vec(b"session-token".to_vec());
    let accepted = build_cancel_call(DeviceJoinRemoteCancelRequest {
        operation_id: "cancel-1",
        join_session_token: &token,
        join_session_id: "join-1",
        reason: "user_cancelled",
    })
    .unwrap();
    assert_eq!(accepted.params["reason"], "user_cancelled");
    assert_eq!(accepted.params["join_session_token"], "session-token");

    assert!(build_cancel_call(DeviceJoinRemoteCancelRequest {
        operation_id: "cancel-2",
        join_session_token: &token,
        join_session_id: "join-1",
        reason: "arbitrary",
    })
    .is_err());
}

#[test]
fn consumed_status_requires_member_without_management_capability() {
    let valid = json!({
        "join_session_id": "join-1",
        "state": "consumed",
        "session_revision": 4,
        "expires_at": "2026-07-18T12:10:00Z",
        "authorization": {
            "checkpoint": {
                "document_version": 2,
                "document_hash": "sha256:A5xxobNC4cQI8_jCXcJigL_y3k0xyr2cZWU_q5izA9A",
                "registry_version": 2
            },
            "device": {
                "device_id": "device-new",
                "signing_key_id": "did:wba:example.com:user:alice#device-new-sign",
                "e2ee_key_id": "did:wba:example.com:user:alice#device-new-e2ee",
                "status": "active",
                "role": "member",
                "management_ready": false,
                "auth_generation": 1
            }
        }
    });
    assert_eq!(
        parse_new_status_result(valid.clone(), "join-1")
            .unwrap()
            .state,
        DeviceJoinRemoteState::Consumed
    );

    let mut invalid = valid;
    invalid["authorization"]["device"]["management_ready"] = json!(true);
    assert!(parse_new_status_result(invalid, "join-1").is_err());
}

#[test]
fn terminal_transition_requires_monotonic_revision() {
    let valid = json!({
        "join_session_id": "join-1",
        "state": "rejected",
        "session_revision": 2
    });
    assert_eq!(
        parse_reject_result(valid, "join-1")
            .unwrap()
            .session_revision,
        2
    );
    let invalid = json!({
        "join_session_id": "join-1",
        "state": "rejected",
        "session_revision": 0
    });
    assert!(parse_reject_result(invalid, "join-1").is_err());
}

#[test]
fn status_rejects_impossible_revisions_nulls_and_non_utc_times() {
    let pending = json!({
        "join_session_id": "join-1",
        "state": "pending",
        "session_revision": 1,
        "expires_at": "2026-07-18T12:10:00Z"
    });
    assert!(parse_new_status_result(pending.clone(), "join-1").is_ok());

    let mut impossible_revision = pending.clone();
    impossible_revision["session_revision"] = json!(2);
    assert!(parse_new_status_result(impossible_revision, "join-1").is_err());

    let mut explicit_null = pending.clone();
    explicit_null["challenge"] = Value::Null;
    assert!(parse_new_status_result(explicit_null, "join-1").is_err());

    let mut offset_time = pending;
    offset_time["expires_at"] = json!("2026-07-18T20:10:00+08:00");
    assert!(parse_new_status_result(offset_time, "join-1").is_err());
}

#[test]
fn terminal_status_enforces_reason_and_branch_field_closure() {
    let rejected = json!({
        "join_session_id": "join-1",
        "state": "rejected",
        "session_revision": 2,
        "expires_at": "2026-07-18T12:10:00Z",
        "reason": "sas_mismatch",
        "rejected_by_device_id": "device-admin",
        "occurred_at": "2026-07-18T12:05:00Z"
    });
    assert!(parse_new_status_result(rejected.clone(), "join-1").is_ok());

    let mut unknown_reason = rejected.clone();
    unknown_reason["reason"] = json!("policy");
    assert!(parse_new_status_result(unknown_reason, "join-1").is_err());

    let mut cross_branch_field = rejected;
    cross_branch_field["challenge_id"] = json!("challenge-1");
    assert!(parse_new_status_result(cross_branch_field, "join-1").is_err());
}
