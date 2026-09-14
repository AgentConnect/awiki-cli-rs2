//! Ordinary hosted Web registration bytes shared with User Service's S00 contract.
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde_json::{json, Value};
use sha2::{Digest as _, Sha256};
use time::{Duration, OffsetDateTime};

use crate::identity::{DeviceProof, DEVICE_PROOF_TYPE};
use crate::internal::identity_registration_pending::PendingRegistration;

pub(crate) fn business_projection(
    params: &Value,
    audience: &str,
    full_handle: &str,
) -> crate::ImResult<Value> {
    if audience.trim().is_empty() || audience != audience.trim() {
        return Err(crate::ImError::PermissionDenied);
    }
    let document = params
        .get("did_document")
        .ok_or(crate::ImError::PermissionDenied)?;
    let manifest = anp::authentication::validate_device_manifest(document)
        .map_err(|_| crate::ImError::PermissionDenied)?
        .ok_or(crate::ImError::PermissionDenied)?;
    if manifest.devices.len() != 1 {
        return Err(crate::ImError::PermissionDenied);
    }
    let device = &manifest.devices[0];
    Ok(json!({
        "contract": "awiki.register.web.v1",
        "audience": audience,
        "registration_operation_id": params["registration_operation_id"],
        "full_handle": full_handle,
        "did": document["id"],
        "bootstrap_device": {"device_id": device.device_id, "signing_key_id": device.signing_key_id, "e2ee_key_id": device.e2ee_key_id},
        "document_hash": super::document::document_hash(document)?,
        "account": {"phone": params["phone"], "email": params["email"]},
        "profile": {"name": params["name"], "avatar": params["avatar"], "role": params["role"],
            "endpoint_url": params["endpoint_url"], "description": params["description"],
            "is_public": params.get("is_public").cloned().unwrap_or(json!(false)),
            "is_agent": params.get("is_agent").cloned().unwrap_or(json!(false))},
        "invite_code_hash": params["invite_code"].as_str().filter(|value| !value.is_empty())
            .map(|value| format!("sha256:{}", URL_SAFE_NO_PAD.encode(Sha256::digest(value.as_bytes())))),
    }))
}

pub(crate) fn request_hash(projection: &Value) -> crate::ImResult<String> {
    let bytes = serde_json_canonicalizer::to_vec(projection)
        .map_err(|_| crate::ImError::PermissionDenied)?;
    Ok(Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

pub(crate) fn signing_input(
    operation_id: &str,
    request_hash: &str,
    audience: &str,
    proof: &DeviceProof,
) -> crate::ImResult<Vec<u8>> {
    serde_json_canonicalizer::to_vec(&json!({
        "type": proof.proof_type, "purpose": "awiki.identity.register.web.v1", "method": "register",
        "audience": audience, "operation_id": operation_id, "request_hash": request_hash,
        "key_id": proof.key_id, "created_at": proof.created_at, "expires_at": proof.expires_at, "nonce": proof.nonce,
    })).map_err(|_| crate::ImError::PermissionDenied)
}

pub(crate) fn prepare(
    pending: &mut PendingRegistration,
    params: &Value,
    audience: &str,
) -> crate::ImResult<(DeviceProof, Vec<u8>)> {
    pending.validate()?;
    if pending.did_method != crate::identity::DidMethod::Web {
        return Err(crate::ImError::PermissionDenied);
    }
    let operation_id = pending
        .registration_operation_id
        .as_deref()
        .ok_or(crate::ImError::PermissionDenied)?;
    let hash = request_hash(&business_projection(
        params,
        audience,
        &format!("{}.{}", pending.target_handle, pending.target_domain),
    )?)?;
    if pending
        .registration_request_hash
        .as_ref()
        .is_some_and(|old| old != &hash)
    {
        return Err(crate::ImError::IdentityBindingConflict {
            detail: "pending Web registration business content changed".to_owned(),
        });
    }
    let now = OffsetDateTime::now_utc()
        .replace_nanosecond(0)
        .map_err(|_| crate::ImError::PermissionDenied)?;
    let format = time::format_description::well_known::Rfc3339;
    let proof = DeviceProof {
        proof_type: DEVICE_PROOF_TYPE.to_owned(),
        key_id: pending.identity.device_signing_key_id.clone(),
        created_at: now
            .format(&format)
            .map_err(|_| crate::ImError::PermissionDenied)?,
        expires_at: (now + Duration::seconds(300))
            .format(&format)
            .map_err(|_| crate::ImError::PermissionDenied)?,
        nonce: uuid::Uuid::new_v4().simple().to_string(),
        signature: String::new(),
    };
    let bytes = signing_input(operation_id, &hash, audience, &proof)?;
    pending.registration_request_hash = Some(hash);
    Ok((proof, bytes))
}
