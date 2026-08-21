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
    pub(crate) identity: LegacyUpgradeIdentityRef,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) root_imported_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LegacyUpgradeIdentityRef {
    pub(crate) custody: crate::internal::identity_join_activation_pending::JoinEnrollmentRef,
    pub(crate) did: crate::ids::Did,
    pub(crate) protocol_device_id: crate::ids::ProtocolDeviceId,
    pub(crate) signing_key_id: String,
    pub(crate) signing_public_key_multibase: String,
    pub(crate) e2ee_key_id: String,
    pub(crate) e2ee_public_key_multibase: String,
    pub(crate) target_document: serde_json::Value,
    pub(crate) target_document_hash: String,
}

impl LegacyUpgradeIdentityRef {
    pub(crate) fn validate(&self) -> crate::ImResult<()> {
        if self.custody.store_id.trim().is_empty()
            || self.custody.identity_id.trim().is_empty()
            || self.custody.enrollment_id.trim().is_empty()
            || self
                .target_document
                .get("id")
                .and_then(serde_json::Value::as_str)
                != Some(self.did.as_str())
            || self.target_document_hash
                != crate::internal::identity_wire::document::document_hash(&self.target_document)?
            || self.signing_public_key_multibase.trim().is_empty()
            || self.e2ee_public_key_multibase.trim().is_empty()
        {
            return Err(crate::ImError::PermissionDenied);
        }
        let manifest = anp::authentication::validate_device_manifest(&self.target_document)
            .map_err(|_| crate::ImError::PermissionDenied)?
            .ok_or(crate::ImError::PermissionDenied)?;
        if manifest.devices.len() != 1
            || manifest.devices[0].device_id != self.protocol_device_id.as_str()
            || manifest.devices[0].signing_key_id != self.signing_key_id
            || manifest.devices[0].e2ee_key_id != self.e2ee_key_id
        {
            return Err(crate::ImError::PermissionDenied);
        }
        for (kid, multibase) in [
            (&self.signing_key_id, &self.signing_public_key_multibase),
            (&self.e2ee_key_id, &self.e2ee_public_key_multibase),
        ] {
            let method = anp::authentication::find_verification_method(&self.target_document, kid)
                .ok_or(crate::ImError::PermissionDenied)?;
            if method
                .get("publicKeyMultibase")
                .and_then(serde_json::Value::as_str)
                != Some(multibase.as_str())
            {
                return Err(crate::ImError::PermissionDenied);
            }
        }
        Ok(())
    }
}

impl std::fmt::Debug for PendingLegacyUpgrade {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PendingLegacyUpgrade")
            .field("local_alias", &self.local_alias)
            .field("source_document_hash", &self.source_document_hash)
            .field("root_ref", &self.root_ref)
            .field("identity", &self.identity)
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
        identity: LegacyUpgradeIdentityRef,
    ) -> crate::ImResult<Self> {
        let pending = Self {
            schema_version: 2,
            local_alias,
            source_document_hash,
            root_ref,
            identity,
            phase: PendingLegacyUpgradePhase::Prepared,
            attempt: PendingLegacyUpgradeAttempt::Running,
            last_attempt_at: now(),
            failure_code: None,
            checkpoint: None,
            access_token: None,
            root_imported_at: None,
        };
        pending.validate()?;
        Ok(pending)
    }

    pub(crate) fn validate(&self) -> crate::ImResult<()> {
        self.identity.validate()?;
        if self.schema_version != 2
            || self.local_alias.trim().is_empty()
            || self.source_document_hash.trim().is_empty()
            || self.last_attempt_at.trim().is_empty()
            || self.root_ref.kind != SecretKind::IdentityRootPrivate
        {
            return Err(crate::ImError::PermissionDenied);
        }
        match self.phase {
            PendingLegacyUpgradePhase::Prepared
                if self.checkpoint.is_none()
                    && self.access_token.is_none()
                    && self.root_imported_at.is_none() => {}
            PendingLegacyUpgradePhase::RemoteCommitted
                if self.checkpoint.is_some()
                    && self
                        .access_token
                        .as_deref()
                        .is_some_and(|v| !v.trim().is_empty())
                    && self
                        .root_imported_at
                        .as_deref()
                        .is_some_and(|value| !value.trim().is_empty()) => {}
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
        signer: &dyn crate::internal::key_provider::IdentitySigner,
    ) -> crate::ImResult<()> {
        if self.phase != PendingLegacyUpgradePhase::Prepared
            || self.checkpoint.is_some()
            || self.access_token.is_some()
        {
            return Err(crate::ImError::PermissionDenied);
        }
        crate::internal::identity_legacy_upgrade::rebuild_custodied_legacy_upgrade_target(
            &mut self.identity,
            remote_document,
            signer,
        )?;
        self.source_document_hash =
            crate::internal::identity_wire::document::document_hash(remote_document)?;
        self.validate()
    }

    pub(crate) fn reconcile_remote_document(
        &mut self,
        remote_document: &serde_json::Value,
        signer: &dyn crate::internal::key_provider::IdentitySigner,
    ) -> crate::ImResult<PendingLegacyUpgradeRemoteState> {
        let remote_hash = crate::internal::identity_wire::document::document_hash(remote_document)?;
        if remote_hash == self.identity.target_document_hash {
            return Ok(PendingLegacyUpgradeRemoteState::TargetCommitted);
        }
        self.rebuild_from_proven_remote_legacy(remote_document, signer)?;
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
                did: Some(pending.identity.did.as_str().to_owned()),
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

    struct RootSigner {
        document: serde_json::Value,
        root_key_id: String,
        root_private_pem: String,
    }

    impl crate::internal::key_provider::IdentitySigner for RootSigner {
        fn did_document(&self) -> crate::ImResult<serde_json::Value> {
            Ok(self.document.clone())
        }

        fn optional_did_document(&self) -> crate::ImResult<Option<serde_json::Value>> {
            Ok(Some(self.document.clone()))
        }

        fn request_signing_key_id(&self) -> crate::ImResult<String> {
            Ok(self.root_key_id.clone())
        }

        fn sign(&self, kid: &str, message: &[u8]) -> crate::ImResult<Vec<u8>> {
            self.sign_root(kid, message)
        }

        fn sign_root(&self, kid: &str, message: &[u8]) -> crate::ImResult<Vec<u8>> {
            if kid != self.root_key_id {
                return Err(crate::ImError::PermissionDenied);
            }
            crate::internal::key_provider::sign_private_pem(
                &self.root_private_pem,
                message,
                "legacy test root",
            )
        }

        fn ecdh(
            &self,
            _kid: &str,
            _peer_public: &[u8],
        ) -> crate::ImResult<zeroize::Zeroizing<[u8; 32]>> {
            Err(crate::ImError::PermissionDenied)
        }

        fn auth_state(&self) -> crate::ImResult<crate::internal::auth::state::AuthStateSnapshot> {
            Ok(Default::default())
        }

        fn valid_auth_token(&self) -> crate::ImResult<Option<String>> {
            Ok(None)
        }

        fn persist_auth_token(&self, _token: &str) -> crate::ImResult<()> {
            Ok(())
        }
    }

    fn root_signer(legacy: &crate::internal::identity_generation::GeneratedIdentity) -> RootSigner {
        RootSigner {
            document: legacy.did_document.clone(),
            root_key_id: format!("{}#key-1", legacy.did.as_str()),
            root_private_pem: legacy.key1_private_pem.clone(),
        }
    }

    fn custody_identity(
        generated: &crate::internal::identity_legacy_upgrade::GeneratedLegacyUpgrade,
    ) -> LegacyUpgradeIdentityRef {
        let method = |kid: &str| {
            generated.target_document["verificationMethod"]
                .as_array()
                .unwrap()
                .iter()
                .find(|method| method["id"] == kid)
                .unwrap()["publicKeyMultibase"]
                .as_str()
                .unwrap()
                .to_owned()
        };
        LegacyUpgradeIdentityRef {
            custody: crate::internal::identity_join_activation_pending::JoinEnrollmentRef {
                store_id: "store-1".to_owned(),
                identity_id: "identity-1".to_owned(),
                enrollment_id: "enrollment-1".to_owned(),
            },
            did: generated.did.clone(),
            protocol_device_id: generated.protocol_device_id.clone(),
            signing_key_id: generated.signing_key_id.clone(),
            signing_public_key_multibase: method(&generated.signing_key_id),
            e2ee_key_id: generated.e2ee_key_id.clone(),
            e2ee_public_key_multibase: method(&generated.e2ee_key_id),
            target_document: generated.target_document.clone(),
            target_document_hash: generated.target_document_hash.clone(),
        }
    }

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
            identity_id: Some(legacy.unique_id.clone()),
            did: Some(legacy.did.as_str().to_owned()),
            kind: SecretKind::IdentityRootPrivate,
            key_id: format!("{}#key-1", legacy.did.as_str()),
            key_version: 1,
        };
        let mut pending = PendingLegacyUpgrade::new(
            "alice".to_owned(),
            crate::internal::identity_wire::document::document_hash(&legacy.did_document).unwrap(),
            root_ref,
            custody_identity(&generated),
        )
        .unwrap();
        pending.mark_retry_required("transport_unavailable");
        let encoded = serde_json::to_string(&pending).unwrap();
        assert!(!encoded.contains("PRIVATE KEY"));
        assert!(!encoded.contains("private_pem"));

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
        assert_eq!(loaded.identity, pending.identity);
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
            identity_id: Some(legacy.unique_id.clone()),
            did: Some(legacy.did.as_str().to_owned()),
            kind: SecretKind::IdentityRootPrivate,
            key_id: format!("{}#key-1", legacy.did.as_str()),
            key_version: 1,
        };
        let mut pending = PendingLegacyUpgrade::new(
            "alice".to_owned(),
            crate::internal::identity_wire::document::document_hash(&legacy.did_document).unwrap(),
            root_ref,
            custody_identity(&generated),
        )
        .unwrap();

        let state = pending
            .reconcile_remote_document(&generated.target_document, &root_signer(&legacy))
            .unwrap();

        assert_eq!(state, PendingLegacyUpgradeRemoteState::TargetCommitted);
        assert_eq!(pending.identity, custody_identity(&generated));
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
            identity_id: Some(legacy.unique_id.clone()),
            did: Some(legacy.did.as_str().to_owned()),
            kind: SecretKind::IdentityRootPrivate,
            key_id: format!("{}#key-1", legacy.did.as_str()),
            key_version: 1,
        };
        let mut pending = PendingLegacyUpgrade::new(
            "alice".to_owned(),
            crate::internal::identity_wire::document::document_hash(&legacy.did_document).unwrap(),
            root_ref,
            custody_identity(&generated),
        )
        .unwrap();
        let mut current_remote_legacy = legacy.did_document.clone();
        current_remote_legacy["x-awiki-server-extension"] = serde_json::json!({"revision": 2});

        let state = pending
            .reconcile_remote_document(&current_remote_legacy, &root_signer(&legacy))
            .unwrap();

        assert_eq!(state, PendingLegacyUpgradeRemoteState::LegacyRebuilt);
        assert_eq!(
            pending.identity.protocol_device_id,
            generated.protocol_device_id
        );
        assert_eq!(pending.identity.signing_key_id, generated.signing_key_id);
        assert_eq!(pending.identity.e2ee_key_id, generated.e2ee_key_id);
        assert_ne!(
            pending.identity.target_document_hash,
            generated.target_document_hash
        );
        assert_eq!(
            pending.identity.target_document["x-awiki-server-extension"],
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
            identity_id: Some(legacy.unique_id.clone()),
            did: Some(legacy.did.as_str().to_owned()),
            kind: SecretKind::IdentityRootPrivate,
            key_id: format!("{}#key-1", legacy.did.as_str()),
            key_version: 1,
        };
        let mut pending = PendingLegacyUpgrade::new(
            "alice".to_owned(),
            crate::internal::identity_wire::document::document_hash(&legacy.did_document).unwrap(),
            root_ref,
            custody_identity(&generated),
        )
        .unwrap();
        let mut different_manifest = generated.target_document.clone();
        different_manifest["x-awiki-other-commit"] = serde_json::json!(true);

        assert!(matches!(
            pending.reconcile_remote_document(&different_manifest, &root_signer(&legacy)),
            Err(crate::ImError::PermissionDenied)
        ));
        assert_eq!(pending.identity, custody_identity(&generated));
    }
}
