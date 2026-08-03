//! Vault-only exact-retry state for Manifest Handle Recovery v1.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::internal::platform_secret::SecretBytes;
use crate::internal::secret_vault::policy::SecretAccessPolicy;
use crate::internal::secret_vault::record::{SecretKind, SecretMetadata, SecretRef};
use crate::internal::secret_vault::{SealSecretRequest, SecretVault};

pub(crate) const CONTRACT_VERSION: &str = "awiki.handle-recovery.v1.contract.3.20260802";
pub(crate) const CONTRACT_HASH: &str =
    "b1c517f6e18fa977fd89239f75f4a333daaddd77404a26f539f4b12b44676b3d";
const SCHEMA_VERSION: u32 = 1;
const KEY_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PendingRecoveryPhase {
    Prepared,
    RemoteCommitPending,
    RemoteCommitted,
    IdentityTransitionPending,
    IdentitySwitched,
    Completed,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RecoveryRemoteResult {
    pub(crate) account_user_id: String,
    pub(crate) handle: String,
    pub(crate) previous_did: String,
    pub(crate) did: String,
    pub(crate) binding_generation: String,
    pub(crate) document_version: u64,
    pub(crate) document_hash: String,
    pub(crate) registry_version: u64,
    pub(crate) bootstrap_device_id: String,
    pub(crate) auth_generation: u64,
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PendingHandleRecovery {
    schema_version: u32,
    contract_version: String,
    contract_hash: String,
    pub(crate) recovery_id: String,
    pub(crate) operation_id: String,
    pub(crate) owner_identity_id: String,
    pub(crate) expected_account_user_id: String,
    pub(crate) local_alias: String,
    pub(crate) display_name: String,
    pub(crate) make_default: bool,
    pub(crate) handle: String,
    pub(crate) previous_did: String,
    pub(crate) expected_binding_generation: String,
    recovery_grant: String,
    pub(crate) grant_expires_at: String,
    pub(crate) generated: crate::internal::identity_generation::GeneratedHandleRecoveryIdentity,
    pub(crate) phase: PendingRecoveryPhase,
    pub(crate) remote_attempted: bool,
    pub(crate) commit_created_at: Option<String>,
    pub(crate) commit_expires_at: Option<String>,
    pub(crate) commit_nonce: Option<String>,
    pub(crate) remote_result: Option<RecoveryRemoteResult>,
    pub(crate) blocked_code: Option<String>,
}

impl std::fmt::Debug for PendingHandleRecovery {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PendingHandleRecovery")
            .field("recovery_id", &self.recovery_id)
            .field("operation_id", &self.operation_id)
            .field("owner_identity_id", &self.owner_identity_id)
            .field("handle", &self.handle)
            .field("previous_did", &self.previous_did)
            .field(
                "expected_binding_generation",
                &self.expected_binding_generation,
            )
            .field("recovery_grant", &"<redacted>")
            .field("grant_expires_at", &self.grant_expires_at)
            .field("generated", &"<redacted-generated-identity>")
            .field("phase", &self.phase)
            .field("remote_attempted", &self.remote_attempted)
            .field("has_commit_proof", &self.commit_nonce.is_some())
            .field("has_remote_result", &self.remote_result.is_some())
            .field("blocked_code", &self.blocked_code)
            .finish()
    }
}

impl PendingHandleRecovery {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        recovery_id: String,
        operation_id: String,
        owner_identity_id: String,
        expected_account_user_id: String,
        local_alias: String,
        display_name: String,
        make_default: bool,
        handle: String,
        previous_did: String,
        expected_binding_generation: String,
        recovery_grant: String,
        grant_expires_at: String,
        generated: crate::internal::identity_generation::GeneratedHandleRecoveryIdentity,
    ) -> crate::ImResult<Self> {
        let pending = Self {
            schema_version: SCHEMA_VERSION,
            contract_version: CONTRACT_VERSION.to_owned(),
            contract_hash: CONTRACT_HASH.to_owned(),
            recovery_id,
            operation_id,
            owner_identity_id,
            expected_account_user_id,
            local_alias,
            display_name,
            make_default,
            handle,
            previous_did,
            expected_binding_generation,
            recovery_grant,
            grant_expires_at,
            generated,
            phase: PendingRecoveryPhase::Prepared,
            remote_attempted: false,
            commit_created_at: None,
            commit_expires_at: None,
            commit_nonce: None,
            remote_result: None,
            blocked_code: None,
        };
        pending.validate()?;
        Ok(pending)
    }

    pub(crate) fn recovery_grant(&self) -> SecretBytes {
        SecretBytes::from_vec(self.recovery_grant.as_bytes().to_vec())
    }

    pub(crate) fn validate(&self) -> crate::ImResult<()> {
        if self.schema_version != SCHEMA_VERSION
            || self.contract_version != CONTRACT_VERSION
            || self.contract_hash != CONTRACT_HASH
            || self.recovery_id.trim().is_empty()
            || self.owner_identity_id.trim().is_empty()
            || self.expected_account_user_id.trim().is_empty()
            || self.local_alias.trim().is_empty()
            || self.display_name.trim().is_empty()
            || self.recovery_grant.trim().is_empty()
            || self.generated.did.as_str() == self.previous_did
            || crate::internal::identity_wire::handle_recovery::canonical_handle(&self.handle)
                .is_err()
            || crate::internal::identity_wire::handle_recovery::validate_operation_id(
                &self.operation_id,
            )
            .is_err()
            || !canonical_generation(&self.expected_binding_generation)
        {
            return Err(crate::ImError::PermissionDenied);
        }
        match self.phase {
            PendingRecoveryPhase::Prepared
                if self.remote_result.is_none()
                    && self.commit_created_at.is_none()
                    && self.commit_expires_at.is_none()
                    && self.commit_nonce.is_none() => {}
            PendingRecoveryPhase::RemoteCommitPending
                if self.remote_result.is_none()
                    && self.remote_attempted
                    && self.commit_created_at.is_some()
                    && self.commit_expires_at.is_some()
                    && self.commit_nonce.is_some() => {}
            PendingRecoveryPhase::RemoteCommitted
            | PendingRecoveryPhase::IdentityTransitionPending
            | PendingRecoveryPhase::IdentitySwitched
            | PendingRecoveryPhase::Completed
                if self.remote_result.is_some() => {}
            PendingRecoveryPhase::Blocked => {}
            _ => return Err(crate::ImError::PermissionDenied),
        }
        Ok(())
    }
}

pub(crate) struct PendingHandleRecoveryStore {
    workspace_id: String,
    device_id: String,
    vault: std::sync::Arc<dyn SecretVault + Send + Sync>,
}

impl PendingHandleRecoveryStore {
    pub(crate) fn from_core(core: &crate::core::ImCore) -> crate::ImResult<Self> {
        if core.inner().identity_secret_storage_policy()
            != crate::core::IdentitySecretStoragePolicy::VaultRequired
        {
            return Err(crate::ImError::LocalStateUnavailable {
                detail: "Handle Recovery requires IdentitySecretStoragePolicy::VaultRequired"
                    .to_owned(),
            });
        }
        let context =
            core.inner()
                .identity_vault()
                .ok_or_else(|| crate::ImError::LocalStateUnavailable {
                    detail: "Handle Recovery requires an available identity SecretVault".to_owned(),
                })?;
        Ok(Self {
            workspace_id: context.workspace_id().to_owned(),
            device_id: context.vault_context_device_id().as_str().to_owned(),
            vault: context.vault(),
        })
    }

    pub(crate) fn load(
        &self,
        recovery_id: &str,
    ) -> crate::ImResult<Option<(SecretRef, PendingHandleRecovery)>> {
        let matches = self
            .vault
            .list()?
            .into_iter()
            .filter(|secret_ref| {
                secret_ref.workspace_id == self.workspace_id
                    && secret_ref.device_id == self.device_id
                    && secret_ref.kind == SecretKind::IdentityHandleRecoveryPending
                    && secret_ref.key_id == pending_key_id(recovery_id)
            })
            .collect::<Vec<_>>();
        if matches.len() > 1 {
            return Err(crate::ImError::PermissionDenied);
        }
        let Some(secret_ref) = matches.into_iter().next() else {
            return Ok(None);
        };
        let plaintext = self.vault.open(&secret_ref)?;
        let pending: PendingHandleRecovery = serde_json::from_slice(plaintext.expose_secret())
            .map_err(|_| crate::ImError::PermissionDenied)?;
        pending.validate()?;
        if pending.recovery_id != recovery_id {
            return Err(crate::ImError::PermissionDenied);
        }
        Ok(Some((secret_ref, pending)))
    }

    pub(crate) fn save(&self, pending: &PendingHandleRecovery) -> crate::ImResult<SecretRef> {
        pending.validate()?;
        let plaintext =
            serde_json::to_vec(pending).map_err(|error| crate::ImError::Serialization {
                detail: error.to_string(),
            })?;
        self.vault.seal(SealSecretRequest {
            metadata: SecretMetadata {
                workspace_id: self.workspace_id.clone(),
                device_id: self.device_id.clone(),
                identity_id: Some(pending.owner_identity_id.clone()),
                did: Some(pending.generated.did.as_str().to_owned()),
                kind: SecretKind::IdentityHandleRecoveryPending,
                key_id: pending_key_id(&pending.recovery_id),
                key_version: KEY_VERSION,
                policy: SecretAccessPolicy::no_prompt_local_secret(),
            },
            plaintext: SecretBytes::from_vec(plaintext),
        })
    }
}

fn pending_key_id(recovery_id: &str) -> String {
    format!(
        "handle-recovery-{}",
        URL_SAFE_NO_PAD.encode(Sha256::digest(recovery_id.as_bytes()))
    )
}

fn canonical_generation(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(|byte| byte.is_ascii_digit())
        && value.as_bytes()[0] != b'0'
}

#[cfg(test)]
mod tests {
    #[test]
    fn contract_identity_is_frozen() {
        assert_eq!(
            super::CONTRACT_VERSION,
            "awiki.handle-recovery.v1.contract.3.20260802"
        );
        assert_eq!(super::CONTRACT_HASH.len(), 64);
    }
}
