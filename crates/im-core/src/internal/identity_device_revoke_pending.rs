//! Vault-only exact-retry intent for permanent device revocation.
//!
//! The record contains only public documents, signatures and AWiki-internal
//! checkpoints, but Vault authentication prevents local tampering with a
//! management operation that may be replayed after a crash.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use crate::internal::identity_device_state::{
    DeviceAuthorizationRole, DeviceAuthorizationStatus, IdentityInternalCheckpoint,
};
use crate::internal::identity_wire::device_revoke::DeviceRevokeRemoteResult;
use crate::internal::platform_secret::SecretBytes;
use crate::internal::secret_vault::policy::SecretAccessPolicy;
use crate::internal::secret_vault::record::{SecretKind, SecretMetadata, SecretRef};
use crate::internal::secret_vault::{SealSecretRequest, SecretVault};

const SCHEMA_VERSION: u32 = 1;
const KEY_VERSION: u32 = 1;
const MAX_PENDING_REVOKES_PER_IDENTITY: usize = 100;

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PendingDeviceRevoke {
    schema_version: u32,
    pub(crate) did: crate::ids::Did,
    pub(crate) operation_id: String,
    pub(crate) target_device_id: String,
    pub(crate) target_auth_generation: u64,
    pub(crate) expected_checkpoint: IdentityInternalCheckpoint,
    pub(crate) new_document: serde_json::Value,
    pub(crate) authorizing_device:
        crate::internal::identity_device_join_runtime::DeviceJoinRemoteDeviceSummary,
    pub(crate) remote_result: Option<DeviceRevokeRemoteResult>,
}

impl std::fmt::Debug for PendingDeviceRevoke {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PendingDeviceRevoke")
            .field("schema_version", &self.schema_version)
            .field("did", &self.did)
            .field("operation_id", &self.operation_id)
            .field("target_device_id", &self.target_device_id)
            .field("target_auth_generation", &self.target_auth_generation)
            .field("expected_checkpoint", &self.expected_checkpoint)
            .field("new_document", &"<redacted-root-signed-document>")
            .field("authorizing_device_id", &self.authorizing_device.device_id)
            .field("has_remote_result", &self.remote_result.is_some())
            .finish()
    }
}

impl PendingDeviceRevoke {
    pub(crate) fn new(
        did: crate::ids::Did,
        operation_id: String,
        target_device_id: String,
        target_auth_generation: u64,
        expected_checkpoint: IdentityInternalCheckpoint,
        new_document: serde_json::Value,
        authorizing_device: crate::internal::identity_device_join_runtime::DeviceJoinRemoteDeviceSummary,
    ) -> crate::ImResult<Self> {
        let record = Self {
            schema_version: SCHEMA_VERSION,
            did,
            operation_id,
            target_device_id,
            target_auth_generation,
            expected_checkpoint,
            new_document,
            authorizing_device,
            remote_result: None,
        };
        record.validate()?;
        Ok(record)
    }

    pub(crate) fn expected_result_checkpoint(&self) -> crate::ImResult<IdentityInternalCheckpoint> {
        Ok(IdentityInternalCheckpoint {
            document_version: self
                .expected_checkpoint
                .document_version
                .checked_add(1)
                .ok_or(crate::ImError::PermissionDenied)?,
            document_hash: crate::internal::identity_wire::document::document_hash(
                &self.new_document,
            )?,
            registry_version: self
                .expected_checkpoint
                .registry_version
                .checked_add(1)
                .ok_or(crate::ImError::PermissionDenied)?,
        })
    }

    pub(crate) fn validate(&self) -> crate::ImResult<()> {
        if self.schema_version != SCHEMA_VERSION
            || self.operation_id.trim().is_empty()
            || self.target_auth_generation == 0
            || self.target_device_id == self.authorizing_device.device_id
            || self.authorizing_device.status != DeviceAuthorizationStatus::Active
            || self.authorizing_device.role != DeviceAuthorizationRole::Admin
            || !self.authorizing_device.management_ready
            || self.authorizing_device.auth_generation == 0
            || self
                .new_document
                .get("id")
                .and_then(serde_json::Value::as_str)
                != Some(self.did.as_str())
            || !anp::authentication::validate_did_document_binding(&self.new_document, true)
        {
            return Err(crate::ImError::PermissionDenied);
        }
        crate::ids::ProtocolDeviceId::parse(&self.target_device_id)?;
        crate::ids::ProtocolDeviceId::parse(&self.authorizing_device.device_id)?;
        let manifest = anp::authentication::validate_device_manifest(&self.new_document)
            .map_err(|_| crate::ImError::PermissionDenied)?
            .ok_or(crate::ImError::PermissionDenied)?;
        if manifest
            .devices
            .iter()
            .any(|device| device.device_id == self.target_device_id)
        {
            return Err(crate::ImError::PermissionDenied);
        }
        let authorizing = manifest
            .devices
            .iter()
            .find(|device| device.device_id == self.authorizing_device.device_id)
            .ok_or(crate::ImError::PermissionDenied)?;
        if authorizing.signing_key_id != self.authorizing_device.signing_key_id
            || authorizing.e2ee_key_id != self.authorizing_device.e2ee_key_id
        {
            return Err(crate::ImError::PermissionDenied);
        }
        let expected_generation = self
            .target_auth_generation
            .checked_add(1)
            .ok_or(crate::ImError::PermissionDenied)?;
        let expected_result = self.expected_result_checkpoint()?;
        if self.remote_result.as_ref().is_some_and(|result| {
            result.target_device_id != self.target_device_id
                || result.auth_generation != expected_generation
                || result.checkpoint != expected_result
        }) {
            return Err(crate::ImError::PermissionDenied);
        }
        Ok(())
    }
}

pub(crate) struct PendingDeviceRevokeStore {
    workspace_id: String,
    vault_context_device_id: String,
    vault: std::sync::Arc<dyn SecretVault + Send + Sync>,
}

impl std::fmt::Debug for PendingDeviceRevokeStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PendingDeviceRevokeStore")
            .field("workspace_id", &self.workspace_id)
            .field("vault_context_device_id", &self.vault_context_device_id)
            .field("vault", &"<redacted-secret-vault>")
            .finish()
    }
}

impl PendingDeviceRevokeStore {
    pub(crate) fn from_core(core: &crate::core::ImCore) -> crate::ImResult<Self> {
        if core.inner().identity_secret_storage_policy()
            != crate::core::IdentitySecretStoragePolicy::VaultRequired
        {
            return Err(crate::ImError::LocalStateUnavailable {
                detail: "device revoke requires IdentitySecretStoragePolicy::VaultRequired"
                    .to_owned(),
            });
        }
        let context =
            core.inner()
                .identity_vault()
                .ok_or_else(|| crate::ImError::LocalStateUnavailable {
                    detail: "device revoke requires an available identity secret vault".to_owned(),
                })?;
        Ok(Self {
            workspace_id: context.workspace_id().to_owned(),
            vault_context_device_id: context.vault_context_device_id().as_str().to_owned(),
            vault: context.vault(),
        })
    }

    pub(crate) fn load(
        &self,
        did: &crate::ids::Did,
        target_device_id: &str,
    ) -> crate::ImResult<Option<(SecretRef, PendingDeviceRevoke)>> {
        let key_id = pending_key_id(did, target_device_id);
        let mut matches = self.vault.list()?.into_iter().filter(|secret_ref| {
            secret_ref.workspace_id == self.workspace_id
                && secret_ref.device_id == self.vault_context_device_id
                && secret_ref.kind == SecretKind::IdentityDeviceRevokePending
                && secret_ref.key_id == key_id
                && secret_ref.did.as_deref() == Some(did.as_str())
        });
        let found = matches.next();
        if matches.next().is_some() {
            return Err(crate::ImError::PermissionDenied);
        }
        found
            .map(|secret_ref| {
                let plaintext = self.vault.open(&secret_ref)?;
                let record: PendingDeviceRevoke = serde_json::from_slice(plaintext.expose_secret())
                    .map_err(|_| crate::ImError::PermissionDenied)?;
                record.validate()?;
                if record.did != *did || record.target_device_id != target_device_id {
                    return Err(crate::ImError::PermissionDenied);
                }
                Ok((secret_ref, record))
            })
            .transpose()
    }

    pub(crate) fn save(&self, record: &PendingDeviceRevoke) -> crate::ImResult<SecretRef> {
        record.validate()?;
        let plaintext = Zeroizing::new(serde_json::to_vec(record).map_err(|error| {
            crate::ImError::Serialization {
                detail: error.to_string(),
            }
        })?);
        let secret_ref = self.vault.seal(SealSecretRequest {
            metadata: SecretMetadata {
                workspace_id: self.workspace_id.clone(),
                device_id: self.vault_context_device_id.clone(),
                identity_id: Some(identity_suffix(&record.did)),
                did: Some(record.did.as_str().to_owned()),
                kind: SecretKind::IdentityDeviceRevokePending,
                key_id: pending_key_id(&record.did, &record.target_device_id),
                key_version: KEY_VERSION,
                policy: SecretAccessPolicy::no_prompt_local_secret(),
            },
            plaintext: SecretBytes::from_vec(plaintext.as_slice().to_vec()),
        })?;
        if self.vault.open(&secret_ref)?.expose_secret() != plaintext.as_slice() {
            return Err(crate::ImError::PermissionDenied);
        }
        Ok(secret_ref)
    }

    pub(crate) fn list_for_identity(
        &self,
        did: &crate::ids::Did,
    ) -> crate::ImResult<Vec<(SecretRef, PendingDeviceRevoke)>> {
        let mut references = self
            .vault
            .list()?
            .into_iter()
            .filter(|secret_ref| {
                secret_ref.workspace_id == self.workspace_id
                    && secret_ref.device_id == self.vault_context_device_id
                    && secret_ref.kind == SecretKind::IdentityDeviceRevokePending
                    && secret_ref.did.as_deref() == Some(did.as_str())
            })
            .collect::<Vec<_>>();
        references.sort_by(|left, right| left.key_id.cmp(&right.key_id));
        if references.len() > MAX_PENDING_REVOKES_PER_IDENTITY {
            return Err(crate::ImError::LocalStateUnavailable {
                detail: "too many pending device revoke records".to_owned(),
            });
        }
        let mut records = Vec::with_capacity(references.len());
        for secret_ref in references {
            let plaintext = self.vault.open(&secret_ref)?;
            let record: PendingDeviceRevoke = serde_json::from_slice(plaintext.expose_secret())
                .map_err(|_| crate::ImError::PermissionDenied)?;
            record.validate()?;
            if record.did != *did {
                return Err(crate::ImError::PermissionDenied);
            }
            records.push((secret_ref, record));
        }
        Ok(records)
    }

    pub(crate) fn delete(&self, secret_ref: &SecretRef) -> crate::ImResult<()> {
        if secret_ref.workspace_id != self.workspace_id
            || secret_ref.device_id != self.vault_context_device_id
            || secret_ref.kind != SecretKind::IdentityDeviceRevokePending
        {
            return Err(crate::ImError::PermissionDenied);
        }
        self.vault.delete(secret_ref)
    }
}

fn pending_key_id(did: &crate::ids::Did, target_device_id: &str) -> String {
    format!(
        "device-revoke-{}",
        URL_SAFE_NO_PAD.encode(Sha256::digest(
            format!("{}\0{target_device_id}", did.as_str()).as_bytes()
        ))
    )
}

fn identity_suffix(did: &crate::ids::Did) -> String {
    did.as_str()
        .rsplit(':')
        .next()
        .filter(|value| !value.is_empty())
        .unwrap_or(did.as_str())
        .to_owned()
}

#[cfg(test)]
mod tests {
    #[test]
    fn debug_does_not_render_document_or_proofs() {
        let source = include_str!("identity_device_revoke_pending.rs");
        let forbidden_root = ["root", "_private", "_key"].concat();
        let forbidden_device = ["device", "_private", "_key"].concat();
        assert!(!source.contains(&forbidden_root));
        assert!(!source.contains(&forbidden_device));
        assert!(source.contains("<redacted-root-signed-document>"));
    }
}
