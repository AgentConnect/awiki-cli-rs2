//! Registration operation facts and current device qualification are independent.
//! Only the authenticated, exact-bootstrap query can produce these observations.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::internal::identity_device_state::IdentityInternalCheckpoint;
use crate::internal::identity_registration_pending::PendingRegistration;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CurrentRegistrationDocument {
    pub(crate) document: Value,
    pub(crate) checkpoint: IdentityInternalCheckpoint,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ResultEnvelope {
    state: String,
    result: Option<RegistrationFacts>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RegistrationFacts {
    user_id: String,
    did: String,
    full_handle: String,
    binding_generation: String,
    operation_id: String,
    request_hash: String,
    document_hash: String,
    bootstrap_device: BootstrapDevice,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BootstrapDevice {
    device_id: String,
    signing_key_id: String,
    e2ee_key_id: String,
}

pub(crate) fn query(pending: &PendingRegistration) -> crate::ImResult<Value> {
    Ok(json!({
        "operation_id": pending.registration_operation_id.as_deref().ok_or(crate::ImError::PermissionDenied)?,
        "request_hash": pending.registration_request_hash.as_deref().ok_or(crate::ImError::PermissionDenied)?,
        "full_handle": format!("{}.{}", pending.target_handle, pending.target_domain),
    }))
}

/// Remove only this optional response extension before the existing closed Registry parser.
pub(crate) fn take_result(
    pending: &PendingRegistration,
    raw: &mut Value,
    user_id: &str,
) -> crate::ImResult<String> {
    let envelope: ResultEnvelope = serde_json::from_value(
        raw.as_object_mut()
            .and_then(|raw| raw.remove("registration_result"))
            .ok_or(crate::ImError::PermissionDenied)?,
    )
    .map_err(|_| crate::ImError::PermissionDenied)?;
    // Absent does not authorize cancellation, a new candidate, or local activation.
    if envelope.state != "committed" {
        return Err(crate::ImError::PermissionDenied);
    }
    let facts = envelope.result.ok_or(crate::ImError::PermissionDenied)?;
    if facts.user_id != user_id
        || facts.did != pending.identity.did.as_str()
        || facts.full_handle != format!("{}.{}", pending.target_handle, pending.target_domain)
        || Some(&facts.operation_id) != pending.registration_operation_id.as_ref()
        || Some(&facts.request_hash) != pending.registration_request_hash.as_ref()
        || facts.document_hash != pending.document_hash
        || facts.bootstrap_device.device_id != pending.identity.protocol_device_id.as_str()
        || facts.bootstrap_device.signing_key_id != pending.identity.device_signing_key_id
        || facts.bootstrap_device.e2ee_key_id != pending.identity.device_e2ee_key_id
        || anp::wns::BindingGeneration::new(facts.binding_generation.clone()).is_err()
    {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(facts.binding_generation)
}

impl CurrentRegistrationDocument {
    pub(crate) fn validate(&self, pending: &PendingRegistration) -> crate::ImResult<()> {
        if pending.did_method != crate::identity::DidMethod::Web
            || self.checkpoint.document_version == 0
            || self.checkpoint.registry_version == 0
            || self.document.get("id").and_then(Value::as_str)
                != Some(pending.identity.did.as_str())
            || self.document.get("proof").is_some()
            || super::document::document_hash(&self.document)? != self.checkpoint.document_hash
            || !anp::authentication::validate_did_document_method(&self.document, true)
        {
            return Err(crate::ImError::PermissionDenied);
        }
        let manifest = anp::authentication::validate_device_manifest(&self.document)
            .map_err(|_| crate::ImError::PermissionDenied)?
            .ok_or(crate::ImError::PermissionDenied)?;
        if !manifest.devices.iter().any(|entry| {
            entry.device_id == pending.identity.protocol_device_id.as_str()
                && entry.signing_key_id == pending.identity.device_signing_key_id
                && entry.e2ee_key_id == pending.identity.device_e2ee_key_id
        }) {
            return Err(crate::ImError::PermissionDenied);
        }
        for kid in [
            &pending.identity.device_signing_key_id,
            &pending.identity.device_e2ee_key_id,
        ] {
            let key = |document: &Value| -> crate::ImResult<anp::PublicKeyMaterial> {
                let vm = anp::authentication::find_verification_method(document, kid)
                    .ok_or(crate::ImError::PermissionDenied)?;
                if vm.get("controller").and_then(Value::as_str)
                    != Some(pending.identity.did.as_str())
                {
                    return Err(crate::ImError::PermissionDenied);
                }
                super::document::extract_identity_public_key(&vm)
            };
            use anp::PublicKeyMaterial;
            let equal = match (key(&pending.identity.did_document)?, key(&self.document)?) {
                (PublicKeyMaterial::Ed25519(a), PublicKeyMaterial::Ed25519(b)) => {
                    a.to_bytes() == b.to_bytes()
                }
                (PublicKeyMaterial::X25519(a), PublicKeyMaterial::X25519(b)) => a == b,
                _ => false,
            };
            if !equal {
                return Err(crate::ImError::PermissionDenied);
            }
        }
        crate::core::validate_handle_service_for_did(
            &self.document,
            &pending.identity.did,
            &crate::ids::Handle::parse(
                &format!("{}.{}", pending.target_handle, pending.target_domain),
                "",
            )?,
        )
    }
}
