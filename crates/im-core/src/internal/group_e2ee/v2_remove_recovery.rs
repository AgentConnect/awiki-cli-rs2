//! A missing target alone never authorizes abandoning an uncertain MLS Commit.
use super::v2_product::V2PreparedRemoveSubmission;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

pub(super) fn proven_competing_remove(
    error: &crate::ImError,
    submission: &V2PreparedRemoveSubmission,
) -> bool {
    let crate::ImError::Service {
        code: Some(code),
        data: Some(data),
        ..
    } = error
    else {
        return false;
    };
    if code != "anp.target_not_found" {
        return false;
    }
    let Some(proof) = data
        .get("rejected_membership_submission")
        .and_then(Value::as_object)
    else {
        return false;
    };
    let body = &submission.prepared.body;
    let expected = json!({
        "schema_version": 1, "reason": "competing_commit_accepted",
        "method": "group.e2ee.remove", "operation_id": submission.meta.operation_id,
        "sender_did": submission.meta.sender_did,
        "sender_device_id": submission.meta.sender_device_id,
        "group_state_ref": body.group_state_ref,
        "crypto_group_id_b64u": body.crypto_group_id_b64u, "epoch": body.epoch,
        "member_did": body.member_did, "member_device_id": body.member_device_id,
        "commit_b64u_sha256": URL_SAFE_NO_PAD.encode(Sha256::digest(body.commit_b64u.as_bytes())),
    });
    expected
        .as_object()
        .expect("fixed proof object")
        .iter()
        .all(|(key, value)| proof.get(key) == Some(value))
}

#[cfg(test)]
#[path = "v2_remove_recovery_tests.rs"]
mod tests;
