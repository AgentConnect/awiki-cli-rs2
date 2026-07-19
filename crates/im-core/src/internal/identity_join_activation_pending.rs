//! Vault-only crash recovery record for new-device Join activation.
//!
//! The exact legacy DIDWba authorization and returned device token pair are
//! replayable credentials, so they never enter the ordinary Join state file.
//! This local record is retained until the rootless vNext identity and token
//! pair have both committed successfully.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::internal::platform_secret::SecretBytes;
use crate::internal::secret_vault::policy::SecretAccessPolicy;
use crate::internal::secret_vault::record::{SecretKind, SecretMetadata, SecretRef};
use crate::internal::secret_vault::{SealSecretRequest, SecretVault};

const SCHEMA_VERSION: u32 = 1;
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
    pub(crate) prepared_token_issue:
        crate::internal::identity_wire::device_genesis::PreparedDeviceTokenIssue,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) token_result:
        Option<crate::internal::identity_wire::device_genesis::DeviceTokenIssueResult>,
}

impl std::fmt::Debug for PendingJoinActivation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PendingJoinActivation")
            .field("schema_version", &self.schema_version)
            .field("join_session_id", &self.join_session_id)
            .field("did", &self.did)
            .field("document", &"<validated-public-document>")
            .field("authorization", &self.authorization)
            .field("prepared_token_issue", &self.prepared_token_issue)
            .field("has_token_result", &self.token_result.is_some())
            .finish()
    }
}

impl PendingJoinActivation {
    pub(crate) fn new(
        join_session_id: String,
        did: crate::ids::Did,
        resolved_document: serde_json::Value,
        authorization: crate::internal::identity_device_join_runtime::DeviceJoinRemoteAuthorization,
        prepared_token_issue: crate::internal::identity_wire::device_genesis::PreparedDeviceTokenIssue,
    ) -> crate::ImResult<Self> {
        let record = Self {
            schema_version: SCHEMA_VERSION,
            join_session_id,
            did,
            resolved_document,
            authorization,
            prepared_token_issue,
            token_result: None,
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
            || self.prepared_token_issue.did != self.did.as_str()
            || self.prepared_token_issue.device_id != self.authorization.device.device_id
            || self.prepared_token_issue.signing_key_id != self.authorization.device.signing_key_id
            || self.prepared_token_issue.expected_scopes
                != vec!["device:read".to_owned(), "message:connect".to_owned()]
            || self.authorization.device.management_ready
            || self.authorization.device.auth_generation == 0
            || crate::internal::identity_wire::device_genesis::document_hash(
                &self.resolved_document,
            )? != self.authorization.checkpoint.document_hash
        {
            return Err(crate::ImError::PermissionDenied);
        }
        crate::internal::identity_wire::device_genesis::verify_prepared_device_token_issue(
            &self.prepared_token_issue,
            &self.resolved_document,
            &service_domain_from_did(&self.did)?,
        )?;
        if let Some(result) = &self.token_result {
            if result.device_id != self.authorization.device.device_id
                || result.auth_generation != self.authorization.device.auth_generation
                || result.scopes != self.prepared_token_issue.expected_scopes
                || result.user_id.trim().is_empty()
                || result.access_token.trim().is_empty()
                || result.refresh_token.trim().is_empty()
            {
                return Err(crate::ImError::PermissionDenied);
            }
        }
        Ok(())
    }
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
                identity_id: Some(identity_suffix(&record.did)),
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
    fn debug_redacts_replayable_authorization_and_tokens() {
        let record_name = std::any::type_name::<PendingJoinActivation>();
        assert!(record_name.contains("PendingJoinActivation"));
        let prepared = crate::internal::identity_wire::device_genesis::PreparedDeviceTokenIssue {
            operation_id: "op-token".to_owned(),
            did: "did:wba:awiki.info:user:alice:e1_root".to_owned(),
            device_id: "dev-new".to_owned(),
            signing_key_id: "did:wba:awiki.info:user:alice:e1_root#dev-new-sign".to_owned(),
            expected_scopes: vec!["device:read".to_owned(), "message:connect".to_owned()],
            authorization: "DIDWba authorization-secret".to_owned(),
        };
        let debug = format!("{prepared:?}");
        assert!(!debug.contains("authorization-secret"));
        assert!(debug.contains("redacted-didwba-authorization"));
    }
}
