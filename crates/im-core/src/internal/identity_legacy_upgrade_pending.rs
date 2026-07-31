//! Vault-only exact-retry record for Legacy → Manifest promotion.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::Arc;

use crate::internal::platform_secret::SecretBytes;
use crate::internal::secret_vault::policy::SecretAccessPolicy;
use crate::internal::secret_vault::record::{SecretKind, SecretMetadata, SecretRef};
use crate::internal::secret_vault::{SealSecretRequest, SecretVault};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PendingLegacyUpgradePhase {
    Prepared,
    RemoteCommitted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PendingLegacyUpgradeAttempt {
    Running,
    RetryRequired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PendingLegacyUpgradeRemoteState {
    TargetCommitted,
    LegacyRebuilt,
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PendingLegacyUpgrade {
    schema_version: u32,
    pub(crate) local_alias: String,
    pub(crate) source_document_hash: String,
    pub(crate) root_ref: SecretRef,
    pub(crate) generated: crate::internal::identity_legacy_upgrade::GeneratedLegacyUpgrade,
    pub(crate) phase: PendingLegacyUpgradePhase,
    pub(crate) attempt: PendingLegacyUpgradeAttempt,
    pub(crate) last_attempt_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) failure_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) checkpoint:
        Option<crate::internal::identity_device_state::IdentityInternalCheckpoint>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) access_token: Option<String>,
}

impl std::fmt::Debug for PendingLegacyUpgrade {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PendingLegacyUpgrade")
            .field("local_alias", &self.local_alias)
            .field("source_document_hash", &self.source_document_hash)
            .field("root_ref", &self.root_ref)
            .field("generated", &self.generated)
            .field("phase", &self.phase)
            .field("attempt", &self.attempt)
            .field("last_attempt_at", &self.last_attempt_at)
            .field("failure_code", &self.failure_code)
            .field("has_checkpoint", &self.checkpoint.is_some())
            .field("has_access_token", &self.access_token.is_some())
            .finish()
    }
}

impl PendingLegacyUpgrade {
    pub(crate) fn new(
        local_alias: String,
        source_document_hash: String,
        root_ref: SecretRef,
        generated: crate::internal::identity_legacy_upgrade::GeneratedLegacyUpgrade,
    ) -> crate::ImResult<Self> {
        let pending = Self {
            schema_version: 1,
            local_alias,
            source_document_hash,
            root_ref,
            generated,
            phase: PendingLegacyUpgradePhase::Prepared,
            attempt: PendingLegacyUpgradeAttempt::Running,
            last_attempt_at: now(),
            failure_code: None,
            checkpoint: None,
            access_token: None,
        };
        pending.validate()?;
        Ok(pending)
    }

    pub(crate) fn validate(&self) -> crate::ImResult<()> {
        if self.schema_version != 1
            || self.local_alias.trim().is_empty()
            || self.source_document_hash.trim().is_empty()
            || self.last_attempt_at.trim().is_empty()
            || self.root_ref.kind != SecretKind::IdentityRootPrivate
            || self.generated.target_document_hash
                != crate::internal::identity_wire::document::document_hash(
                    &self.generated.target_document,
                )?
        {
            return Err(crate::ImError::PermissionDenied);
        }
        match self.phase {
            PendingLegacyUpgradePhase::Prepared
                if self.checkpoint.is_none() && self.access_token.is_none() => {}
            PendingLegacyUpgradePhase::RemoteCommitted
                if self.checkpoint.is_some()
                    && self
                        .access_token
                        .as_deref()
                        .is_some_and(|v| !v.trim().is_empty()) => {}
            _ => return Err(crate::ImError::PermissionDenied),
        }
        Ok(())
    }

    pub(crate) fn mark_running(&mut self) {
        self.attempt = PendingLegacyUpgradeAttempt::Running;
        self.last_attempt_at = now();
        self.failure_code = None;
    }

    pub(crate) fn mark_retry_required(&mut self, code: &str) {
        self.attempt = PendingLegacyUpgradeAttempt::RetryRequired;
        self.last_attempt_at = now();
        self.failure_code = Some(code.to_owned());
    }

    pub(crate) fn rebuild_from_proven_remote_legacy(
        &mut self,
        remote_document: &serde_json::Value,
        root_private_pem: &str,
    ) -> crate::ImResult<()> {
        if self.phase != PendingLegacyUpgradePhase::Prepared
            || self.checkpoint.is_some()
            || self.access_token.is_some()
        {
            return Err(crate::ImError::PermissionDenied);
        }
        crate::internal::identity_legacy_upgrade::rebuild_legacy_upgrade_target(
            &mut self.generated,
            remote_document,
            root_private_pem,
        )?;
        self.source_document_hash =
            crate::internal::identity_wire::document::document_hash(remote_document)?;
        self.validate()
    }

    pub(crate) fn reconcile_remote_document(
        &mut self,
        remote_document: &serde_json::Value,
        root_private_pem: &str,
    ) -> crate::ImResult<PendingLegacyUpgradeRemoteState> {
        let remote_hash = crate::internal::identity_wire::document::document_hash(remote_document)?;
        if remote_hash == self.generated.target_document_hash {
            return Ok(PendingLegacyUpgradeRemoteState::TargetCommitted);
        }
        self.rebuild_from_proven_remote_legacy(remote_document, root_private_pem)?;
        Ok(PendingLegacyUpgradeRemoteState::LegacyRebuilt)
    }
}

pub(crate) struct PendingLegacyUpgradeStore {
    workspace_id: String,
    device_id: String,
    vault: Arc<dyn SecretVault + Send + Sync>,
}

impl PendingLegacyUpgradeStore {
    pub(crate) fn from_core(core: &crate::core::ImCore) -> crate::ImResult<Self> {
        let context =
            core.inner()
                .identity_vault()
                .ok_or_else(|| crate::ImError::LocalStateUnavailable {
                    detail: "Legacy upgrade requires Vault storage".to_owned(),
                })?;
        Ok(Self {
            workspace_id: context.workspace_id().to_owned(),
            device_id: context.vault_context_device_id().as_str().to_owned(),
            vault: context.vault(),
        })
    }

    pub(crate) fn load(
        &self,
        local_alias: &str,
    ) -> crate::ImResult<Option<(SecretRef, PendingLegacyUpgrade)>> {
        let key_id = pending_key_id(local_alias);
        let Some(secret_ref) = self.vault.list()?.into_iter().find(|secret_ref| {
            secret_ref.workspace_id == self.workspace_id
                && secret_ref.device_id == self.device_id
                && secret_ref.kind == SecretKind::IdentityLegacyUpgradePending
                && secret_ref.key_id == key_id
        }) else {
            return Ok(None);
        };
        let opened = self.vault.open(&secret_ref)?;
        let pending: PendingLegacyUpgrade = serde_json::from_slice(opened.expose_secret())
            .map_err(|_| crate::ImError::PermissionDenied)?;
        pending.validate()?;
        Ok(Some((secret_ref, pending)))
    }

    pub(crate) fn save(&self, pending: &PendingLegacyUpgrade) -> crate::ImResult<SecretRef> {
        pending.validate()?;
        let plaintext =
            serde_json::to_vec(pending).map_err(|error| crate::ImError::Serialization {
                detail: error.to_string(),
            })?;
        self.vault.seal(SealSecretRequest {
            metadata: SecretMetadata {
                workspace_id: self.workspace_id.clone(),
                device_id: self.device_id.clone(),
                identity_id: self.root_identity_id(pending),
                did: Some(pending.generated.did.as_str().to_owned()),
                kind: SecretKind::IdentityLegacyUpgradePending,
                key_id: pending_key_id(&pending.local_alias),
                key_version: 1,
                policy: SecretAccessPolicy::no_prompt_local_secret(),
            },
            plaintext: SecretBytes::from_vec(plaintext),
        })
    }

    pub(crate) fn delete(&self, secret_ref: &SecretRef) -> crate::ImResult<()> {
        if secret_ref.kind != SecretKind::IdentityLegacyUpgradePending {
            return Err(crate::ImError::PermissionDenied);
        }
        self.vault.delete(secret_ref)
    }

    fn root_identity_id(&self, pending: &PendingLegacyUpgrade) -> Option<String> {
        pending.root_ref.identity_id.clone()
    }
}

fn pending_key_id(local_alias: &str) -> String {
    let digest = Sha256::digest(local_alias.trim().as_bytes());
    format!("legacy-upgrade-{}", URL_SAFE_NO_PAD.encode(digest))
}

fn now() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::internal::platform_secret::DeviceVaultRootKey;
    use crate::internal::secret_vault::{FileSecretVault, FileSecretVaultStore};

    #[test]
    fn retry_required_status_and_code_survive_vault_restart() {
        let root = tempfile::tempdir().unwrap();
        let vault_path = root.path().join("vault");
        let root_key = [91_u8; 32];
        let legacy =
            crate::internal::identity_generation::generate_handle_identity_with_default_daemon_subkey(
                "example.test",
                "alice",
                None,
                None,
            )
            .unwrap()
            .identity;
        let generated = crate::internal::identity_legacy_upgrade::build_legacy_upgrade(
            &legacy.did_document,
            &legacy.key1_private_pem,
        )
        .unwrap();
        let root_ref = SecretRef {
            workspace_id: "workspace-1".to_owned(),
            device_id: "vault-device-1".to_owned(),
            identity_id: Some(legacy.unique_id),
            did: Some(legacy.did.as_str().to_owned()),
            kind: SecretKind::IdentityRootPrivate,
            key_id: format!("{}#key-1", legacy.did.as_str()),
            key_version: 1,
        };
        let mut pending = PendingLegacyUpgrade::new(
            "alice".to_owned(),
            crate::internal::identity_wire::document::document_hash(&legacy.did_document).unwrap(),
            root_ref,
            generated,
        )
        .unwrap();
        pending.mark_retry_required("transport_unavailable");

        let first_store = PendingLegacyUpgradeStore {
            workspace_id: "workspace-1".to_owned(),
            device_id: "vault-device-1".to_owned(),
            vault: Arc::new(FileSecretVault::new(
                DeviceVaultRootKey::from_bytes(root_key),
                FileSecretVaultStore::new(&vault_path),
            )),
        };
        first_store.save(&pending).unwrap();
        drop(first_store);

        let restarted_store = PendingLegacyUpgradeStore {
            workspace_id: "workspace-1".to_owned(),
            device_id: "vault-device-1".to_owned(),
            vault: Arc::new(FileSecretVault::new(
                DeviceVaultRootKey::from_bytes(root_key),
                FileSecretVaultStore::new(&vault_path),
            )),
        };
        let (_, loaded) = restarted_store.load("alice").unwrap().unwrap();

        assert_eq!(loaded.attempt, PendingLegacyUpgradeAttempt::RetryRequired);
        assert_eq!(loaded.generated, pending.generated);
        assert_eq!(
            loaded.failure_code.as_deref(),
            Some("transport_unavailable")
        );
    }

    #[test]
    fn remote_reconciliation_preserves_exact_committed_target_and_pending_keys() {
        let legacy =
            crate::internal::identity_generation::generate_handle_identity_with_default_daemon_subkey(
                "example.test",
                "alice",
                None,
                None,
            )
            .unwrap()
            .identity;
        let generated = crate::internal::identity_legacy_upgrade::build_legacy_upgrade(
            &legacy.did_document,
            &legacy.key1_private_pem,
        )
        .unwrap();
        let root_ref = SecretRef {
            workspace_id: "workspace-1".to_owned(),
            device_id: "vault-device-1".to_owned(),
            identity_id: Some(legacy.unique_id),
            did: Some(legacy.did.as_str().to_owned()),
            kind: SecretKind::IdentityRootPrivate,
            key_id: format!("{}#key-1", legacy.did.as_str()),
            key_version: 1,
        };
        let mut pending = PendingLegacyUpgrade::new(
            "alice".to_owned(),
            crate::internal::identity_wire::document::document_hash(&legacy.did_document).unwrap(),
            root_ref,
            generated.clone(),
        )
        .unwrap();

        let state = pending
            .reconcile_remote_document(&generated.target_document, &legacy.key1_private_pem)
            .unwrap();

        assert_eq!(state, PendingLegacyUpgradeRemoteState::TargetCommitted);
        assert_eq!(pending.generated, generated);
    }

    #[test]
    fn remote_legacy_retry_refreshes_proof_without_generating_a_second_device() {
        let legacy =
            crate::internal::identity_generation::generate_handle_identity_with_default_daemon_subkey(
                "example.test",
                "alice",
                None,
                None,
            )
            .unwrap()
            .identity;
        let generated = crate::internal::identity_legacy_upgrade::build_legacy_upgrade(
            &legacy.did_document,
            &legacy.key1_private_pem,
        )
        .unwrap();
        let root_ref = SecretRef {
            workspace_id: "workspace-1".to_owned(),
            device_id: "vault-device-1".to_owned(),
            identity_id: Some(legacy.unique_id),
            did: Some(legacy.did.as_str().to_owned()),
            kind: SecretKind::IdentityRootPrivate,
            key_id: format!("{}#key-1", legacy.did.as_str()),
            key_version: 1,
        };
        let mut pending = PendingLegacyUpgrade::new(
            "alice".to_owned(),
            crate::internal::identity_wire::document::document_hash(&legacy.did_document).unwrap(),
            root_ref,
            generated.clone(),
        )
        .unwrap();
        let mut current_remote_legacy = legacy.did_document.clone();
        current_remote_legacy["x-awiki-server-extension"] = serde_json::json!({"revision": 2});

        let state = pending
            .reconcile_remote_document(&current_remote_legacy, &legacy.key1_private_pem)
            .unwrap();

        assert_eq!(state, PendingLegacyUpgradeRemoteState::LegacyRebuilt);
        assert_eq!(
            pending.generated.protocol_device_id,
            generated.protocol_device_id
        );
        assert_eq!(pending.generated.signing_key_id, generated.signing_key_id);
        assert_eq!(
            pending.generated.signing_private_pem,
            generated.signing_private_pem
        );
        assert_eq!(pending.generated.e2ee_key_id, generated.e2ee_key_id);
        assert_eq!(
            pending.generated.e2ee_private_pem,
            generated.e2ee_private_pem
        );
        assert_ne!(
            pending.generated.target_document_hash,
            generated.target_document_hash
        );
        assert_eq!(
            pending.generated.target_document["x-awiki-server-extension"],
            serde_json::json!({"revision": 2})
        );
    }

    #[test]
    fn retry_rejects_a_different_remote_manifest_without_replacing_pending_material() {
        let legacy =
            crate::internal::identity_generation::generate_handle_identity_with_default_daemon_subkey(
                "example.test",
                "alice",
                None,
                None,
            )
            .unwrap()
            .identity;
        let generated = crate::internal::identity_legacy_upgrade::build_legacy_upgrade(
            &legacy.did_document,
            &legacy.key1_private_pem,
        )
        .unwrap();
        let root_ref = SecretRef {
            workspace_id: "workspace-1".to_owned(),
            device_id: "vault-device-1".to_owned(),
            identity_id: Some(legacy.unique_id),
            did: Some(legacy.did.as_str().to_owned()),
            kind: SecretKind::IdentityRootPrivate,
            key_id: format!("{}#key-1", legacy.did.as_str()),
            key_version: 1,
        };
        let mut pending = PendingLegacyUpgrade::new(
            "alice".to_owned(),
            crate::internal::identity_wire::document::document_hash(&legacy.did_document).unwrap(),
            root_ref,
            generated.clone(),
        )
        .unwrap();
        let mut different_manifest = generated.target_document.clone();
        different_manifest["x-awiki-other-commit"] = serde_json::json!(true);

        assert!(matches!(
            pending.reconcile_remote_document(&different_manifest, &legacy.key1_private_pem),
            Err(crate::ImError::PermissionDenied)
        ));
        assert_eq!(pending.generated, generated);
    }
}
