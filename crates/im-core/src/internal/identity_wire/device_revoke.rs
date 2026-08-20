//! Strict AWiki-local wire contract for permanent device revocation.
//!
//! Document/Registry checkpoints and proofs are first-party control-plane
//! fields. This module does not define or extend ANP wire schemas.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use rand::RngCore as _;
use serde::Deserialize;
use serde_json::{json, Value};
use time::{Duration, OffsetDateTime};

use crate::identity::{DeviceProof, DEVICE_PROOF_TYPE};
use crate::internal::identity_device_state::IdentityInternalCheckpoint;

pub(crate) const DEVICE_REVOKE_METHOD: &str = "device_revoke";
pub(crate) const DEVICE_REVOKE_PURPOSE: &str = "awiki.device.revoke.v1";
const DEVICE_PROOF_TTL_SECONDS: i64 = 300;

#[derive(Clone, PartialEq)]
pub(crate) struct PreparedDeviceRevoke {
    pub(crate) operation_id: String,
    pub(crate) target_device_id: String,
    pub(crate) expected_checkpoint: IdentityInternalCheckpoint,
    pub(crate) new_document: Value,
    pub(crate) authorizing_device_id: String,
    pub(crate) authorizing_device_proof: DeviceProof,
}

impl std::fmt::Debug for PreparedDeviceRevoke {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreparedDeviceRevoke")
            .field("operation_id", &self.operation_id)
            .field("target_device_id", &self.target_device_id)
            .field("expected_checkpoint", &self.expected_checkpoint)
            .field("new_document", &"<redacted-root-signed-document>")
            .field("authorizing_device_id", &self.authorizing_device_id)
            .field("authorizing_device_proof", &"<redacted-device-proof>")
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct DeviceRevokeRemoteResult {
    pub(crate) target_device_id: String,
    pub(crate) auth_generation: u64,
    pub(crate) checkpoint: IdentityInternalCheckpoint,
}

pub(crate) struct DeviceRevokeWireCall {
    pub(crate) endpoint: &'static str,
    pub(crate) method: &'static str,
    pub(crate) params: Value,
}

impl std::fmt::Debug for DeviceRevokeWireCall {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeviceRevokeWireCall")
            .field("endpoint", &self.endpoint)
            .field("method", &self.method)
            .field("params", &"<redacted-root-signed-control-request>")
            .finish()
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn prepare_revoke(
    operation_id: String,
    target_device_id: String,
    expected_checkpoint: IdentityInternalCheckpoint,
    new_document: Value,
    authorizing_device_id: String,
    authorizing_signing_key_id: &str,
    signer: &dyn Fn(&str, &[u8]) -> crate::ImResult<Vec<u8>>,
    now: OffsetDateTime,
) -> crate::ImResult<PreparedDeviceRevoke> {
    required("operation_id", &operation_id)?;
    crate::ids::ProtocolDeviceId::parse(&target_device_id)?;
    crate::ids::ProtocolDeviceId::parse(&authorizing_device_id)?;
    if authorizing_device_id == target_device_id
        || expected_checkpoint.document_version == 0
        || expected_checkpoint.registry_version == 0
        || !valid_digest(&expected_checkpoint.document_hash)
    {
        return Err(crate::ImError::PermissionDenied);
    }
    let created_at = format_time(now)?;
    let expires_at = format_time(now + Duration::seconds(DEVICE_PROOF_TTL_SECONDS))?;
    let params = revoke_params(
        &operation_id,
        &target_device_id,
        &expected_checkpoint,
        &new_document,
        &authorizing_device_id,
    );
    let mut nonce = [0_u8; 24];
    rand::rngs::OsRng
        .try_fill_bytes(&mut nonce)
        .map_err(|_| crate::ImError::Internal {
            message: "generate device revoke proof nonce failed".to_owned(),
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
        "purpose": DEVICE_REVOKE_PURPOSE,
        "method": DEVICE_REVOKE_METHOD,
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
    proof.signature = URL_SAFE_NO_PAD.encode(signer(&proof.key_id, &signing_input)?);
    Ok(PreparedDeviceRevoke {
        operation_id,
        target_device_id,
        expected_checkpoint,
        new_document,
        authorizing_device_id,
        authorizing_device_proof: proof,
    })
}

pub(crate) fn build_revoke_call(
    prepared: &PreparedDeviceRevoke,
) -> crate::ImResult<DeviceRevokeWireCall> {
    let params = revoke_params(
        required_ref("operation_id", &prepared.operation_id)?,
        required_ref("target_device_id", &prepared.target_device_id)?,
        &prepared.expected_checkpoint,
        &prepared.new_document,
        required_ref("authorizing_device_id", &prepared.authorizing_device_id)?,
    );
    let mut params = params
        .as_object()
        .cloned()
        .ok_or_else(|| crate::ImError::Serialization {
            detail: "device revoke params must be an object".to_owned(),
        })?;
    params.insert(
        "authorizing_device_proof".to_owned(),
        serde_json::to_value(&prepared.authorizing_device_proof).map_err(|error| {
            crate::ImError::Serialization {
                detail: error.to_string(),
            }
        })?,
    );
    Ok(DeviceRevokeWireCall {
        endpoint: super::DID_AUTH_RPC_ENDPOINT,
        method: DEVICE_REVOKE_METHOD,
        params: Value::Object(params),
    })
}

pub(crate) fn parse_revoke_result(
    raw: Value,
    expected_target_device_id: &str,
    expected_auth_generation: u64,
    expected_checkpoint: &IdentityInternalCheckpoint,
) -> crate::ImResult<DeviceRevokeRemoteResult> {
    let raw: RawRevokeResult = serde_json::from_value(raw).map_err(|_| invalid_wire())?;
    let checkpoint = IdentityInternalCheckpoint {
        document_version: raw.checkpoint.document_version,
        document_hash: raw.checkpoint.document_hash,
        registry_version: raw.checkpoint.registry_version,
    };
    if raw.target_device_id != expected_target_device_id
        || raw.status != "revoked"
        || raw.auth_generation != expected_auth_generation
        || checkpoint != *expected_checkpoint
        || !valid_digest(&checkpoint.document_hash)
    {
        return Err(invalid_wire());
    }
    Ok(DeviceRevokeRemoteResult {
        target_device_id: raw.target_device_id,
        auth_generation: raw.auth_generation,
        checkpoint,
    })
}

fn revoke_params(
    operation_id: &str,
    target_device_id: &str,
    checkpoint: &IdentityInternalCheckpoint,
    new_document: &Value,
    authorizing_device_id: &str,
) -> Value {
    json!({
        "operation_id": operation_id,
        "target_device_id": target_device_id,
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
        detail: "device revoke response was invalid".to_owned(),
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
struct RawRevokeResult {
    target_device_id: String,
    status: String,
    auth_generation: u64,
    checkpoint: RawCheckpoint,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn call_debug_redacts_document_and_proof() {
        let prepared = PreparedDeviceRevoke {
            operation_id: "revoke-op".to_owned(),
            target_device_id: "dev-target".to_owned(),
            expected_checkpoint: IdentityInternalCheckpoint {
                document_version: 3,
                document_hash: "sha256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".to_owned(),
                registry_version: 2,
            },
            new_document: json!({"proof": {"proofValue": "root-proof-secret-shape"}}),
            authorizing_device_id: "dev-admin".to_owned(),
            authorizing_device_proof: DeviceProof {
                proof_type: DEVICE_PROOF_TYPE.to_owned(),
                key_id: "did:wba:awiki.test:user:alice:e1_x#admin-sign".to_owned(),
                created_at: "2026-07-20T00:00:00Z".to_owned(),
                expires_at: "2026-07-20T00:05:00Z".to_owned(),
                nonce: "proof-nonce".to_owned(),
                signature: "device-signature-secret-shape".to_owned(),
            },
        };
        let call = build_revoke_call(&prepared).unwrap();
        let debug = format!("{call:?} {prepared:?}");
        assert!(!debug.contains("root-proof-secret-shape"));
        assert!(!debug.contains("device-signature-secret-shape"));
    }

    #[test]
    fn response_is_closed_and_exact() {
        let checkpoint = IdentityInternalCheckpoint {
            document_version: 4,
            document_hash: "sha256:BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB".to_owned(),
            registry_version: 3,
        };
        let value = json!({
            "target_device_id": "dev-target",
            "status": "revoked",
            "auth_generation": 2,
            "checkpoint": {
                "document_version": 4,
                "document_hash": checkpoint.document_hash,
                "registry_version": 3,
            }
        });
        assert!(parse_revoke_result(value.clone(), "dev-target", 2, &checkpoint).is_ok());
        let mut extra = value;
        extra["root_private_key"] = json!("forbidden");
        assert!(parse_revoke_result(extra, "dev-target", 2, &checkpoint).is_err());
    }

    #[test]
    fn request_matches_awiki_private_rpc_contract_without_cross_domain_fields() {
        let private = anp::PrivateKeyMaterial::Ed25519(ed25519_dalek::SigningKey::generate(
            &mut rand::rngs::OsRng,
        ));
        let checkpoint = IdentityInternalCheckpoint {
            document_version: 3,
            document_hash: "sha256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".to_owned(),
            registry_version: 2,
        };
        let prepared = prepare_revoke(
            "device-revoke-operation".to_owned(),
            "dev-target".to_owned(),
            checkpoint,
            json!({
                "id": "did:wba:awiki.test:user:alice:e1_test",
                "proof": {"proofValue": "root-document-signature"}
            }),
            "dev-admin".to_owned(),
            "did:wba:awiki.test:user:alice:e1_test#dev-admin-sign",
            &|_, message| {
                private
                    .sign_message(message)
                    .map_err(|_| crate::ImError::PermissionDenied)
            },
            OffsetDateTime::from_unix_timestamp(1_784_515_200).unwrap(),
        )
        .unwrap();
        let call = build_revoke_call(&prepared).unwrap();
        assert_eq!(call.endpoint, super::super::DID_AUTH_RPC_ENDPOINT);
        assert_eq!(call.method, DEVICE_REVOKE_METHOD);
        let keys = call
            .params
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>();
        assert_eq!(
            keys,
            [
                "authorizing_device_id",
                "authorizing_device_proof",
                "expected_document_hash",
                "expected_document_version",
                "expected_registry_version",
                "new_document",
                "operation_id",
                "target_device_id",
            ]
        );
        assert_eq!(
            call.params["new_document"]["proof"]["proofValue"],
            "root-document-signature"
        );
        for forbidden in [
            "deviceManifest.epoch",
            "mapping_version",
            "registry_hash",
            "transition_proof",
            "root_private_key",
        ] {
            assert!(!call.params.as_object().unwrap().contains_key(forbidden));
        }
    }
}
