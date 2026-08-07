//! Strict AWiki-local wire contract for ordinary multi-device DID updates.
//!
//! This control-plane method may update non-device DID content, but it cannot
//! change the Manifest, device key relationships or root key.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use rand::RngCore as _;
use serde::Deserialize;
use serde_json::{json, Value};
use time::{Duration, OffsetDateTime};

use crate::identity::{DeviceProof, DEVICE_PROOF_TYPE};
use crate::internal::identity_device_state::IdentityInternalCheckpoint;

pub(crate) const DEVICE_DOCUMENT_UPDATE_METHOD: &str = "device_document_update";
pub(crate) const DEVICE_DOCUMENT_UPDATE_PURPOSE: &str = "awiki.device.document.update.v1";
const DEVICE_PROOF_TTL_SECONDS: i64 = 300;

#[derive(Clone, PartialEq)]
pub(crate) struct PreparedDeviceDocumentUpdate {
    pub(crate) operation_id: String,
    pub(crate) expected_checkpoint: IdentityInternalCheckpoint,
    pub(crate) new_document: Value,
    pub(crate) authorizing_device_id: String,
    pub(crate) authorizing_device_proof: DeviceProof,
}

impl std::fmt::Debug for PreparedDeviceDocumentUpdate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreparedDeviceDocumentUpdate")
            .field("operation_id", &self.operation_id)
            .field("expected_checkpoint", &self.expected_checkpoint)
            .field("new_document", &"<redacted-root-signed-document>")
            .field("authorizing_device_id", &self.authorizing_device_id)
            .field("authorizing_device_proof", &"<redacted-device-proof>")
            .finish()
    }
}

pub(crate) struct DeviceDocumentUpdateWireCall {
    pub(crate) endpoint: &'static str,
    pub(crate) method: &'static str,
    pub(crate) params: Value,
}

impl std::fmt::Debug for DeviceDocumentUpdateWireCall {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeviceDocumentUpdateWireCall")
            .field("endpoint", &self.endpoint)
            .field("method", &self.method)
            .field("params", &"<redacted-root-signed-control-request>")
            .finish()
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn prepare_update(
    operation_id: String,
    expected_checkpoint: IdentityInternalCheckpoint,
    new_document: Value,
    authorizing_device_id: String,
    authorizing_signing_key_id: &str,
    authorizing_private_key: &anp::PrivateKeyMaterial,
    now: OffsetDateTime,
) -> crate::ImResult<PreparedDeviceDocumentUpdate> {
    required("operation_id", &operation_id)?;
    crate::ids::ProtocolDeviceId::parse(&authorizing_device_id)?;
    if expected_checkpoint.document_version == 0
        || expected_checkpoint.registry_version == 0
        || !valid_digest(&expected_checkpoint.document_hash)
        || !matches!(authorizing_private_key, anp::PrivateKeyMaterial::Ed25519(_))
    {
        return Err(crate::ImError::PermissionDenied);
    }
    let created_at = format_time(now)?;
    let expires_at = format_time(now + Duration::seconds(DEVICE_PROOF_TTL_SECONDS))?;
    let params = update_params(
        &operation_id,
        &expected_checkpoint,
        &new_document,
        &authorizing_device_id,
    );
    let mut nonce = [0_u8; 24];
    rand::rngs::OsRng
        .try_fill_bytes(&mut nonce)
        .map_err(|_| crate::ImError::Internal {
            message: "generate device document update proof nonce failed".to_owned(),
        })?;
    let mut proof = DeviceProof {
        proof_type: DEVICE_PROOF_TYPE.to_owned(),
        key_id: required("authorizing_signing_key_id", authorizing_signing_key_id)?,
        created_at,
        expires_at,
        nonce: URL_SAFE_NO_PAD.encode(nonce),
        signature: String::new(),
    };
    let signing_object = json!({
        "type": proof.proof_type,
        "purpose": DEVICE_DOCUMENT_UPDATE_PURPOSE,
        "method": DEVICE_DOCUMENT_UPDATE_METHOD,
        "key_id": proof.key_id,
        "created_at": proof.created_at,
        "expires_at": proof.expires_at,
        "nonce": proof.nonce,
        "params": params,
    });
    let signing_input = serde_json_canonicalizer::to_vec(&signing_object).map_err(|error| {
        crate::ImError::Serialization {
            detail: error.to_string(),
        }
    })?;
    proof.signature = URL_SAFE_NO_PAD.encode(
        authorizing_private_key
            .sign_message(&signing_input)
            .map_err(|_| crate::ImError::PermissionDenied)?,
    );
    Ok(PreparedDeviceDocumentUpdate {
        operation_id,
        expected_checkpoint,
        new_document,
        authorizing_device_id,
        authorizing_device_proof: proof,
    })
}

pub(crate) fn build_update_call(
    prepared: &PreparedDeviceDocumentUpdate,
) -> crate::ImResult<DeviceDocumentUpdateWireCall> {
    let params = update_params(
        required_ref("operation_id", &prepared.operation_id)?,
        &prepared.expected_checkpoint,
        &prepared.new_document,
        required_ref("authorizing_device_id", &prepared.authorizing_device_id)?,
    );
    let mut params = params
        .as_object()
        .cloned()
        .ok_or_else(|| crate::ImError::Serialization {
            detail: "device document update params must be an object".to_owned(),
        })?;
    params.insert(
        "authorizing_device_proof".to_owned(),
        serde_json::to_value(&prepared.authorizing_device_proof).map_err(|error| {
            crate::ImError::Serialization {
                detail: error.to_string(),
            }
        })?,
    );
    Ok(DeviceDocumentUpdateWireCall {
        endpoint: super::DID_AUTH_RPC_ENDPOINT,
        method: DEVICE_DOCUMENT_UPDATE_METHOD,
        params: Value::Object(params),
    })
}

pub(crate) fn parse_update_result(
    raw: Value,
    expected_did: &crate::ids::Did,
    expected_checkpoint: &IdentityInternalCheckpoint,
) -> crate::ImResult<IdentityInternalCheckpoint> {
    let raw: RawUpdateResult = serde_json::from_value(raw).map_err(|_| invalid_wire())?;
    let checkpoint = IdentityInternalCheckpoint {
        document_version: raw.checkpoint.document_version,
        document_hash: raw.checkpoint.document_hash,
        registry_version: raw.checkpoint.registry_version,
    };
    if raw.did != expected_did.as_str()
        || checkpoint != *expected_checkpoint
        || !valid_digest(&checkpoint.document_hash)
    {
        return Err(invalid_wire());
    }
    Ok(checkpoint)
}

fn update_params(
    operation_id: &str,
    checkpoint: &IdentityInternalCheckpoint,
    new_document: &Value,
    authorizing_device_id: &str,
) -> Value {
    json!({
        "operation_id": operation_id,
        "expected_document_version": checkpoint.document_version,
        "expected_document_hash": checkpoint.document_hash,
        "expected_registry_version": checkpoint.registry_version,
        "new_document": new_document,
        "authorizing_device_id": authorizing_device_id,
    })
}

fn required(field: &str, value: &str) -> crate::ImResult<String> {
    required_ref(field, value).map(ToOwned::to_owned)
}

fn required_ref<'a>(field: &str, value: &'a str) -> crate::ImResult<&'a str> {
    let value = value.trim();
    if value.is_empty() {
        return Err(crate::ImError::invalid_input(
            Some(field.to_owned()),
            format!("{field} is required"),
        ));
    }
    Ok(value)
}

fn valid_digest(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|encoded| {
        encoded.len() == 43
            && encoded
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    })
}

fn format_time(value: OffsetDateTime) -> crate::ImResult<String> {
    value
        .format(&time::format_description::well_known::Rfc3339)
        .map_err(|error| crate::ImError::Serialization {
            detail: error.to_string(),
        })
}

fn invalid_wire() -> crate::ImError {
    crate::ImError::Serialization {
        detail: "device document update response was invalid".to_owned(),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCheckpoint {
    document_version: u64,
    document_hash: String,
    registry_version: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawUpdateResult {
    did: String,
    checkpoint: RawCheckpoint,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_matches_private_document_update_contract_and_redacts_debug() {
        let private = anp::PrivateKeyMaterial::Ed25519(ed25519_dalek::SigningKey::generate(
            &mut rand::rngs::OsRng,
        ));
        let prepared = prepare_update(
            "daemon-subkey-revoke-operation".to_owned(),
            IdentityInternalCheckpoint {
                document_version: 3,
                document_hash: "sha256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".to_owned(),
                registry_version: 2,
            },
            json!({
                "id": "did:wba:awiki.test:user:alice:e1_test",
                "proof": {"proofValue": "root-document-signature"}
            }),
            "dev-admin".to_owned(),
            "did:wba:awiki.test:user:alice:e1_test#dev-admin-sign",
            &private,
            OffsetDateTime::from_unix_timestamp(1_784_515_200).unwrap(),
        )
        .unwrap();
        let call = build_update_call(&prepared).unwrap();
        assert_eq!(call.endpoint, super::super::DID_AUTH_RPC_ENDPOINT);
        assert_eq!(call.method, DEVICE_DOCUMENT_UPDATE_METHOD);
        assert_eq!(
            call.params
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            [
                "authorizing_device_id",
                "authorizing_device_proof",
                "expected_document_hash",
                "expected_document_version",
                "expected_registry_version",
                "new_document",
                "operation_id",
            ]
        );
        let debug = format!("{call:?} {prepared:?}");
        assert!(!debug.contains("root-document-signature"));
        assert!(!debug.contains(&prepared.authorizing_device_proof.signature));
    }

    #[test]
    fn response_is_closed_and_exact() {
        let did = crate::ids::Did::parse("did:wba:awiki.test:user:alice:e1_test").unwrap();
        let checkpoint = IdentityInternalCheckpoint {
            document_version: 4,
            document_hash: "sha256:BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB".to_owned(),
            registry_version: 2,
        };
        let value = json!({
            "did": did.as_str(),
            "checkpoint": {
                "document_version": checkpoint.document_version,
                "document_hash": checkpoint.document_hash,
                "registry_version": checkpoint.registry_version,
            }
        });
        assert_eq!(
            parse_update_result(value.clone(), &did, &checkpoint).unwrap(),
            checkpoint
        );
        let mut extra = value;
        extra["root_private_key"] = json!("forbidden");
        assert!(parse_update_result(extra, &did, &checkpoint).is_err());
    }
}
