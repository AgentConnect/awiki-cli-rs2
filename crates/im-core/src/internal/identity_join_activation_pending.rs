//! Vault-only crash recovery record for new-device Join activation.
//!
//! Device private keys remain in anp-identity. This vault record retains only
//! public custody references and the returned access token until the rootless
//! identity projection has committed successfully.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::internal::platform_secret::SecretBytes;
use crate::internal::secret_vault::policy::SecretAccessPolicy;
use crate::internal::secret_vault::record::{SecretKind, SecretMetadata, SecretRef};
use crate::internal::secret_vault::{SealSecretRequest, SecretVault};

const SCHEMA_VERSION: u32 = 2;
const KEY_VERSION: u32 = 1;

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PendingJoinActivation {
    schema_version: u32,
    pub(crate) join_session_id: String,
    pub(crate) did: crate::ids::Did,
    pub(crate) resolved_document: serde_json::Value,
    pub(crate) authorization:
        crate::internal::identity_device_join_runtime::DeviceJoinRemoteAuthorization,
    pub(crate) custody: JoinEnrollmentRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) access_result:
        Option<crate::internal::identity_device_join_runtime::DeviceJoinAccessResult>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct JoinEnrollmentRef {
    pub(crate) store_id: String,
    pub(crate) identity_id: String,
    pub(crate) enrollment_id: String,
}

impl std::fmt::Debug for PendingJoinActivation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PendingJoinActivation")
            .field("schema_version", &self.schema_version)
            .field("join_session_id", &self.join_session_id)
            .field("did", &self.did)
            .field("document", &"<validated-public-document>")
            .field("authorization", &self.authorization)
            .field("custody", &self.custody)
            .field("has_access_result", &self.access_result.is_some())
            .finish()
    }
}

impl PendingJoinActivation {
    pub(crate) fn new(
        join_session_id: String,
        did: crate::ids::Did,
        resolved_document: serde_json::Value,
        authorization: crate::internal::identity_device_join_runtime::DeviceJoinRemoteAuthorization,
        custody: JoinEnrollmentRef,
    ) -> crate::ImResult<Self> {
        let record = Self {
            schema_version: SCHEMA_VERSION,
            join_session_id,
            did,
            resolved_document,
            authorization,
            custody,
            access_result: None,
        };
        record.validate()?;
        Ok(record)
    }

    pub(crate) fn validate(&self) -> crate::ImResult<()> {
        if self.schema_version != SCHEMA_VERSION
            || self.join_session_id.trim().is_empty()
            || self
                .resolved_document
                .get("id")
                .and_then(serde_json::Value::as_str)
                != Some(self.did.as_str())
            || self.authorization.device.management_ready
            || self.authorization.device.role
                != crate::internal::identity_device_state::DeviceAuthorizationRole::Member
            || self.authorization.device.auth_generation == 0
            || crate::internal::identity_wire::document::document_hash(&self.resolved_document)?
                != self.authorization.checkpoint.document_hash
            || self.custody.store_id.trim().is_empty()
            || self.custody.identity_id.trim().is_empty()
            || self.custody.enrollment_id.trim().is_empty()
        {
            return Err(crate::ImError::PermissionDenied);
        }
        let signing_method = find_method(
            &self.resolved_document,
            &self.authorization.device.signing_key_id,
        )?;
        let e2ee_method = find_method(
            &self.resolved_document,
            &self.authorization.device.e2ee_key_id,
        )?;
        let signing_public =
            crate::internal::identity_wire::document::extract_identity_public_key(signing_method)?;
        let e2ee_public =
            crate::internal::identity_wire::document::extract_identity_public_key(e2ee_method)?;
        if !matches!(signing_public, anp::PublicKeyMaterial::Ed25519(_))
            || !matches!(e2ee_public, anp::PublicKeyMaterial::X25519(_))
            || !anp::authentication::is_authentication_authorized(
                &self.resolved_document,
                &self.authorization.device.signing_key_id,
            )
            || !anp::authentication::is_assertion_method_authorized(
                &self.resolved_document,
                &self.authorization.device.signing_key_id,
            )
        {
            return Err(crate::ImError::PermissionDenied);
        }
        if let Some(result) = &self.access_result {
            if result.user_id.trim().is_empty() || result.access_token.trim().is_empty() {
                return Err(crate::ImError::PermissionDenied);
            }
            crate::internal::access_token::validate_device_access_token(
                &result.access_token,
                &crate::internal::access_token::ExpectedDeviceAccess {
                    did: self.did.as_str(),
                    user_id: &result.user_id,
                    device_id: &self.authorization.device.device_id,
                    key_id: &self.authorization.device.signing_key_id,
                    auth_generation: self.authorization.device.auth_generation,
                    role: self.authorization.device.role,
                    management_ready: false,
                },
            )?;
        }
        Ok(())
    }
}

fn find_method<'a>(
    document: &'a serde_json::Value,
    key_id: &str,
) -> crate::ImResult<&'a serde_json::Value> {
    document
        .get("verificationMethod")
        .and_then(serde_json::Value::as_array)
        .and_then(|methods| {
            methods
                .iter()
                .find(|method| method.get("id").and_then(serde_json::Value::as_str) == Some(key_id))
        })
        .ok_or(crate::ImError::PermissionDenied)
}

pub(crate) struct PendingJoinActivationStore {
    workspace_id: String,
    device_id: String,
    vault: std::sync::Arc<dyn SecretVault + Send + Sync>,
}

impl std::fmt::Debug for PendingJoinActivationStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PendingJoinActivationStore")
            .field("workspace_id", &self.workspace_id)
            .field("device_id", &self.device_id)
            .field("vault", &"<redacted-secret-vault>")
            .finish()
    }
}

impl PendingJoinActivationStore {
    pub(crate) fn from_core(core: &crate::core::ImCore) -> crate::ImResult<Self> {
        if core.inner().identity_secret_storage_policy()
            != crate::core::IdentitySecretStoragePolicy::VaultRequired
        {
            return Err(crate::ImError::LocalStateUnavailable {
                detail:
                    "new-device Join activation requires IdentitySecretStoragePolicy::VaultRequired"
                        .to_owned(),
            });
        }
        let context =
            core.inner()
                .identity_vault()
                .ok_or_else(|| crate::ImError::LocalStateUnavailable {
                    detail:
                        "new-device Join activation requires an available identity secret vault"
                            .to_owned(),
                })?;
        Ok(Self {
            workspace_id: context.workspace_id().to_owned(),
            device_id: context.vault_context_device_id().as_str().to_owned(),
            vault: context.vault(),
        })
    }

    pub(crate) fn load(
        &self,
        join_session_id: &str,
        did: &crate::ids::Did,
    ) -> crate::ImResult<Option<(SecretRef, PendingJoinActivation)>> {
        let key_id = pending_key_id(join_session_id);
        let matches = self
            .vault
            .list()?
            .into_iter()
            .filter(|secret_ref| {
                secret_ref.workspace_id == self.workspace_id
                    && secret_ref.device_id == self.device_id
                    && secret_ref.kind == SecretKind::IdentityJoinActivationPending
                    && secret_ref.key_id == key_id
            })
            .collect::<Vec<_>>();
        if matches.len() > 1 {
            return Err(crate::ImError::PermissionDenied);
        }
        let Some(secret_ref) = matches.into_iter().next() else {
            return Ok(None);
        };
        let plaintext = self.vault.open(&secret_ref)?;
        let record: PendingJoinActivation = serde_json::from_slice(plaintext.expose_secret())
            .map_err(|_| crate::ImError::PermissionDenied)?;
        record.validate()?;
        if record.join_session_id != join_session_id || &record.did != did {
            return Err(crate::ImError::PermissionDenied);
        }
        Ok(Some((secret_ref, record)))
    }

    pub(crate) fn save(&self, record: &PendingJoinActivation) -> crate::ImResult<SecretRef> {
        record.validate()?;
        let plaintext =
            serde_json::to_vec(record).map_err(|error| crate::ImError::Serialization {
                detail: error.to_string(),
            })?;
        let secret_ref = self.vault.seal(SealSecretRequest {
            metadata: SecretMetadata {
                workspace_id: self.workspace_id.clone(),
                device_id: self.device_id.clone(),
                identity_id: Some(record.custody.identity_id.clone()),
                did: Some(record.did.as_str().to_owned()),
                kind: SecretKind::IdentityJoinActivationPending,
                key_id: pending_key_id(&record.join_session_id),
                key_version: KEY_VERSION,
                policy: SecretAccessPolicy::no_prompt_local_secret(),
            },
            plaintext: SecretBytes::from_vec(plaintext.clone()),
        })?;
        let opened = self.vault.open(&secret_ref)?;
        if opened.expose_secret() != plaintext.as_slice() {
            return Err(crate::ImError::PermissionDenied);
        }
        Ok(secret_ref)
    }

    pub(crate) fn delete(&self, secret_ref: &SecretRef) -> crate::ImResult<()> {
        if secret_ref.workspace_id != self.workspace_id
            || secret_ref.device_id != self.device_id
            || secret_ref.kind != SecretKind::IdentityJoinActivationPending
        {
            return Err(crate::ImError::PermissionDenied);
        }
        self.vault.delete(secret_ref)
    }
}

pub(crate) fn service_domain_from_did(did: &crate::ids::Did) -> crate::ImResult<String> {
    let domain = did
        .as_str()
        .strip_prefix("did:wba:")
        .and_then(|rest| rest.split(':').next())
        .filter(|value| !value.trim().is_empty())
        .ok_or(crate::ImError::PermissionDenied)?;
    Ok(domain.to_ascii_lowercase())
}

pub(crate) fn identity_suffix(did: &crate::ids::Did) -> String {
    did.as_str()
        .rsplit(':')
        .next()
        .filter(|value| !value.is_empty())
        .unwrap_or(did.as_str())
        .to_owned()
}

fn pending_key_id(join_session_id: &str) -> String {
    let digest = Sha256::digest(join_session_id.as_bytes());
    format!("join-activation-{}", URL_SAFE_NO_PAD.encode(digest))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_activation_accepts_canonical_join_okp_jwk_methods() {
        let did = crate::ids::Did::parse("did:wba:awiki.info:user:alice:e1_root").unwrap();
        let signing_key_id = format!("{}#dev-new-sign", did.as_str());
        let e2ee_key_id = format!("{}#dev-new-e2ee", did.as_str());
        let signing_private =
            anp::PrivateKeyMaterial::Ed25519(ed25519_dalek::SigningKey::from_bytes(&[42; 32]));
        let e2ee_private =
            anp::PrivateKeyMaterial::X25519(x25519_dalek::StaticSecret::from([24; 32]));
        let document = serde_json::json!({
            "id": did.as_str(),
            "verificationMethod": [
                okp_jwk_method(
                    &signing_key_id,
                    did.as_str(),
                    &signing_private.public_key(),
                ),
                okp_jwk_method(
                    &e2ee_key_id,
                    did.as_str(),
                    &e2ee_private.public_key(),
                ),
            ],
            "authentication": [signing_key_id.clone()],
            "assertionMethod": [signing_key_id.clone()],
            "keyAgreement": [e2ee_key_id.clone()],
        });
        let document_hash =
            crate::internal::identity_wire::document::document_hash(&document).unwrap();

        let pending = PendingJoinActivation::new(
            "join-1".to_owned(),
            did,
            document,
            crate::internal::identity_device_join_runtime::DeviceJoinRemoteAuthorization {
                checkpoint:
                    crate::internal::identity_device_state::IdentityInternalCheckpoint {
                        document_version: 2,
                        document_hash,
                        registry_version: 2,
                    },
                device:
                    crate::internal::identity_device_join_runtime::DeviceJoinRemoteDeviceSummary {
                        device_id: "dev-new".to_owned(),
                        signing_key_id,
                        e2ee_key_id,
                        status:
                            crate::internal::identity_device_state::DeviceAuthorizationStatus::Active,
                        role:
                            crate::internal::identity_device_state::DeviceAuthorizationRole::Member,
                        management_ready: false,
                        auth_generation: 1,
                    },
            },
            JoinEnrollmentRef {
                store_id: "store-1".to_owned(),
                identity_id: "identity-1".to_owned(),
                enrollment_id: "enrollment-1".to_owned(),
            },
        );

        assert!(pending.is_ok());
    }

    fn okp_jwk_method(
        key_id: &str,
        did: &str,
        public_key: &anp::PublicKeyMaterial,
    ) -> serde_json::Value {
        let (curve, bytes) = match public_key {
            anp::PublicKeyMaterial::Ed25519(key) => ("Ed25519", key.to_bytes().to_vec()),
            anp::PublicKeyMaterial::X25519(key) => ("X25519", key.to_vec()),
            _ => panic!("test requires an OKP public key"),
        };
        serde_json::json!({
            "id": key_id,
            "type": "JsonWebKey2020",
            "controller": did,
            "publicKeyJwk": {
                "kty": "OKP",
                "crv": curve,
                "x": URL_SAFE_NO_PAD.encode(bytes),
            },
        })
    }

    #[test]
    fn debug_contains_only_public_custody_and_redacts_token() {
        let record_name = std::any::type_name::<PendingJoinActivation>();
        assert!(record_name.contains("PendingJoinActivation"));
        let debug = format!(
            "{:?}",
            PendingJoinActivation {
                schema_version: SCHEMA_VERSION,
                join_session_id: "join-1".to_owned(),
                did: crate::ids::Did::parse("did:wba:awiki.info:user:alice:e1_root").unwrap(),
                resolved_document: serde_json::json!({}),
                authorization:
                    crate::internal::identity_device_join_runtime::DeviceJoinRemoteAuthorization {
                        checkpoint:
                            crate::internal::identity_device_state::IdentityInternalCheckpoint {
                                document_version: 1,
                                document_hash: "hash".to_owned(),
                                registry_version: 1,
                            },
                        device:
                            crate::internal::identity_device_join_runtime::DeviceJoinRemoteDeviceSummary {
                                device_id: "dev-new".to_owned(),
                                signing_key_id: "sign".to_owned(),
                                e2ee_key_id: "e2ee".to_owned(),
                                status: crate::internal::identity_device_state::DeviceAuthorizationStatus::Active,
                                role: crate::internal::identity_device_state::DeviceAuthorizationRole::Member,
                                management_ready: false,
                                auth_generation: 1,
                            },
                    },
                custody: JoinEnrollmentRef {
                    store_id: "store-1".to_owned(),
                    identity_id: "identity-1".to_owned(),
                    enrollment_id: "enrollment-1".to_owned(),
                },
                access_result: Some(
                    crate::internal::identity_device_join_runtime::DeviceJoinAccessResult {
                        user_id: "user-1".to_owned(),
                        access_token: "access-token-secret".to_owned(),
                    },
                ),
            }
        );
        assert!(!debug.contains("access-token-secret"));
    }
}
