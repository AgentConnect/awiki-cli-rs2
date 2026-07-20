//! Vault-only crash recovery record for the vNext Handle Recovery lifecycle.
//!
//! The record owns replayable account/session/reconfirmation grants, the
//! begin-authenticated internal account subject, generated private keys, the
//! exact finalize proof and the returned device token pair. None of these
//! values may enter public DTOs or ordinary local-state tables.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use zeroize::{Zeroize, Zeroizing};

use crate::internal::platform_secret::SecretBytes;
use crate::internal::secret_vault::policy::SecretAccessPolicy;
use crate::internal::secret_vault::record::{SecretKind, SecretMetadata, SecretRef};
use crate::internal::secret_vault::{SealSecretRequest, SecretVault};

const SCHEMA_VERSION: u32 = 2;
const KEY_VERSION: u32 = 1;

#[derive(PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PendingRecoveryRecord {
    schema_version: u32,
    pub(crate) binding: crate::internal::handle_discovery::RecoveryHandleBinding,
    pub(crate) begin_operation_id: String,
    pub(crate) begin_grant: Option<String>,
    pub(crate) session:
        Option<crate::internal::identity_wire::device_recovery::RecoverySessionResult>,
    pub(crate) generated:
        Option<crate::internal::identity_generation::GeneratedVNextIdentityWithDaemonSubkey>,
    pub(crate) prepared_finalize:
        Option<crate::internal::identity_wire::device_recovery::PreparedRecoveryFinalize>,
    pub(crate) reconfirmation_token: Option<String>,
    /// Stable business operation for obtaining a fresh token pair after an
    /// otherwise valid finalize replay returns an expired access token.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) token_issue_operation_id: Option<String>,
    pub(crate) remote_result:
        Option<crate::internal::identity_wire::device_recovery::RecoveryFinalizeResult>,
    pub(crate) local_alias: Option<String>,
}

impl std::fmt::Debug for PendingRecoveryRecord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PendingRecoveryRecord")
            .field("schema_version", &self.schema_version)
            .field("binding", &self.binding)
            .field("begin_operation_id", &self.begin_operation_id)
            .field("has_begin_grant", &self.begin_grant.is_some())
            .field("session", &self.session)
            .field("generated", &self.generated.as_ref().map(|_| "<redacted>"))
            .field("prepared_finalize", &self.prepared_finalize)
            .field(
                "has_reconfirmation_token",
                &self.reconfirmation_token.is_some(),
            )
            .field(
                "has_token_issue_operation",
                &self.token_issue_operation_id.is_some(),
            )
            .field("remote_result", &self.remote_result)
            .field("local_alias", &self.local_alias)
            .finish()
    }
}

impl Drop for PendingRecoveryRecord {
    fn drop(&mut self) {
        self.begin_grant.zeroize();
        self.reconfirmation_token.zeroize();
        if let Some(generated) = self.generated.as_mut() {
            generated.root_private_pem.zeroize();
            generated.device_signing_private_pem.zeroize();
            generated.device_e2ee_private_pem.zeroize();
            generated.daemon_subkey_package.private_key_pem.zeroize();
            generated
                .daemon_subkey_package
                .private_key_multibase
                .zeroize();
        }
        // RecoverySessionResult and RecoveryFinalizeResult zeroize their own
        // bearer/session tokens when the containing Options are dropped.
    }
}

impl PendingRecoveryRecord {
    pub(crate) fn new(
        binding: crate::internal::handle_discovery::RecoveryHandleBinding,
        begin_operation_id: String,
        begin_grant: String,
    ) -> crate::ImResult<Self> {
        let record = Self {
            schema_version: SCHEMA_VERSION,
            binding,
            begin_operation_id,
            begin_grant: Some(begin_grant),
            session: None,
            generated: None,
            prepared_finalize: None,
            reconfirmation_token: None,
            token_issue_operation_id: None,
            remote_result: None,
            local_alias: None,
        };
        record.validate()?;
        Ok(record)
    }

    pub(crate) fn validate(&self) -> crate::ImResult<()> {
        if self.schema_version != SCHEMA_VERSION
            || self.begin_operation_id.trim().is_empty()
            || self.binding.mapping_generation == 0
            || self.binding.handle.as_str()
                != format!("{}.{}", self.binding.local_part, self.binding.domain)
            || self.binding.local_part != self.binding.local_part.to_ascii_lowercase()
            || self.binding.domain != self.binding.domain.to_ascii_lowercase()
        {
            return Err(crate::ImError::PermissionDenied);
        }
        if self.session.is_none()
            && self
                .begin_grant
                .as_deref()
                .is_none_or(|value| value.trim().is_empty())
        {
            return Err(crate::ImError::PermissionDenied);
        }
        if let Some(session) = &self.session {
            if session.old_did != self.binding.did.as_str()
                || session.account_user_id.trim().is_empty()
                || session.account_user_id.trim() != session.account_user_id
                || session.recovery_session_id.trim().is_empty()
                || session.recovery_session_token.trim().is_empty()
            {
                return Err(crate::ImError::PermissionDenied);
            }
        }

        let finalize_parts = [
            self.generated.is_some(),
            self.prepared_finalize.is_some(),
            self.reconfirmation_token.is_some(),
            self.local_alias.is_some(),
        ];
        if finalize_parts.iter().any(|present| *present)
            && finalize_parts.iter().any(|present| !*present)
        {
            return Err(crate::ImError::PermissionDenied);
        }
        if let (Some(generated), Some(prepared), Some(reconfirmation), Some(local_alias)) = (
            self.generated.as_ref(),
            self.prepared_finalize.as_ref(),
            self.reconfirmation_token.as_deref(),
            self.local_alias.as_deref(),
        ) {
            let session = self
                .session
                .as_ref()
                .ok_or(crate::ImError::PermissionDenied)?;
            if generated.did == self.binding.did
                || !matches!(
                    session.state,
                    crate::internal::identity_wire::device_recovery::RecoveryRemoteState::Ready
                        | crate::internal::identity_wire::device_recovery::RecoveryRemoteState::Consumed
                )
                || prepared.expected_handle_mapping_generation != self.binding.mapping_generation
                || prepared.new_did_document != generated.did_document
                || prepared.bootstrap_device_id != generated.protocol_device_id.as_str()
                || prepared.bootstrap_device_proof.key_id != generated.device_signing_key_id
                || reconfirmation.trim().is_empty()
                || local_alias.trim().is_empty()
            {
                return Err(crate::ImError::PermissionDenied);
            }
            crate::internal::identity_wire::device_recovery::build_recovery_finalize_call(
                prepared,
                &session.recovery_session_token,
                reconfirmation,
            )?;
        }
        if let Some(result) = &self.remote_result {
            let generated = self
                .generated
                .as_ref()
                .ok_or(crate::ImError::PermissionDenied)?;
            let session = self
                .session
                .as_ref()
                .ok_or(crate::ImError::PermissionDenied)?;
            let next_mapping_generation = self
                .binding
                .mapping_generation
                .checked_add(1)
                .ok_or(crate::ImError::PermissionDenied)?;
            if result.recovery_session_id != session.recovery_session_id
                || result.state
                    != crate::internal::identity_wire::device_recovery::RecoveryRemoteState::Consumed
                || session.state
                    != crate::internal::identity_wire::device_recovery::RecoveryRemoteState::Consumed
                || result.old_did != self.binding.did.as_str()
                || result.did != generated.did.as_str()
                || result.handle != self.binding.handle.as_str()
                || result.user_id != session.account_user_id
                || result.handle_mapping_generation != next_mapping_generation
                || result.device.device_id != generated.protocol_device_id.as_str()
                || result.device.signing_key_id != generated.device_signing_key_id
                || result.device.e2ee_key_id != generated.device_e2ee_key_id
                || result.device.status != "active"
                || result.device.role != "admin"
                || !result.device.management_ready
                || result.device.auth_generation != 1
                || result.checkpoint.document_version != 1
                || result.checkpoint.registry_version != 1
                || result.checkpoint.document_hash
                    != crate::internal::identity_wire::device_genesis::document_hash(
                        &generated.did_document,
                    )?
                || result.access_token.trim().is_empty()
                || result.refresh_token.trim().is_empty()
                || result.token_expires_at.trim().is_empty()
            {
                return Err(crate::ImError::PermissionDenied);
            }
            crate::ids::ProtocolDeviceId::parse(&result.device.device_id)
                .map_err(|_| crate::ImError::PermissionDenied)?;
            result.device_state().validate_for_did(&generated.did)?;
        }
        if self
            .token_issue_operation_id
            .as_deref()
            .is_some_and(|operation_id| operation_id.trim().is_empty())
            || (self.token_issue_operation_id.is_some()
                && (self.generated.is_none()
                    || self.prepared_finalize.is_none()
                    || self.session.is_none()))
        {
            return Err(crate::ImError::PermissionDenied);
        }
        Ok(())
    }

    pub(crate) fn recovery_session_id(&self) -> Option<&str> {
        self.session
            .as_ref()
            .map(|session| session.recovery_session_id.as_str())
    }

    pub(crate) fn replace_begin_grant(&mut self, replacement: Option<String>) {
        if let Some(current) = self.begin_grant.as_mut() {
            current.zeroize();
        }
        self.begin_grant = replacement;
    }

    pub(crate) fn replace_reconfirmation_token(&mut self, replacement: String) {
        if let Some(current) = self.reconfirmation_token.as_mut() {
            current.zeroize();
        }
        self.reconfirmation_token = Some(replacement);
    }
}

pub(crate) struct PendingRecoveryStore {
    workspace_id: String,
    device_id: String,
    vault: std::sync::Arc<dyn SecretVault + Send + Sync>,
}

impl std::fmt::Debug for PendingRecoveryStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PendingRecoveryStore")
            .field("workspace_id", &self.workspace_id)
            .field("device_id", &self.device_id)
            .field("vault", &"<redacted-secret-vault>")
            .finish()
    }
}

impl PendingRecoveryStore {
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
                    detail: "Handle Recovery requires an available identity secret vault"
                        .to_owned(),
                })?;
        Ok(Self {
            workspace_id: context.workspace_id().to_owned(),
            device_id: context.vault_context_device_id().as_str().to_owned(),
            vault: context.vault(),
        })
    }

    pub(crate) fn load_by_handle(
        &self,
        handle: &crate::ids::Handle,
    ) -> crate::ImResult<Option<(SecretRef, PendingRecoveryRecord)>> {
        self.load_by_key(&pending_key_id(handle.as_str()))
    }

    pub(crate) fn load_by_session(
        &self,
        recovery_session_id: &str,
    ) -> crate::ImResult<Option<(SecretRef, PendingRecoveryRecord)>> {
        let mut matches = self
            .list()?
            .into_iter()
            .filter(|(_, record)| record.recovery_session_id() == Some(recovery_session_id));
        let found = matches.next();
        if matches.next().is_some() {
            return Err(crate::ImError::PermissionDenied);
        }
        Ok(found)
    }

    pub(crate) fn list(&self) -> crate::ImResult<Vec<(SecretRef, PendingRecoveryRecord)>> {
        self.vault
            .list()?
            .into_iter()
            .filter(|secret_ref| {
                secret_ref.workspace_id == self.workspace_id
                    && secret_ref.device_id == self.device_id
                    && secret_ref.kind == SecretKind::IdentityRecoveryPending
            })
            .map(|secret_ref| {
                let record = self.open_record(&secret_ref)?;
                Ok((secret_ref, record))
            })
            .collect()
    }

    pub(crate) fn save(&self, record: &PendingRecoveryRecord) -> crate::ImResult<SecretRef> {
        record.validate()?;
        let plaintext = Zeroizing::new(serde_json::to_vec(record).map_err(|error| {
            crate::ImError::Serialization {
                detail: error.to_string(),
            }
        })?);
        // Keep the SecretRef stable for the full lifecycle. Switching this
        // metadata to the generated identity after preparation would publish a
        // second record and make restart lookup ambiguous. The new identity
        // material remains encrypted inside the record until activation.
        let identity_id = identity_suffix(&record.binding.did);
        let did = record.binding.did.as_str().to_owned();
        let secret_ref = self.vault.seal(SealSecretRequest {
            metadata: SecretMetadata {
                workspace_id: self.workspace_id.clone(),
                device_id: self.device_id.clone(),
                identity_id: Some(identity_id),
                did: Some(did),
                kind: SecretKind::IdentityRecoveryPending,
                key_id: pending_key_id(record.binding.handle.as_str()),
                key_version: KEY_VERSION,
                policy: SecretAccessPolicy::no_prompt_local_secret(),
            },
            // The only duplicate is moved immediately into SecretBytes, whose
            // Drop zeroizes it. The serialization buffer is Zeroizing too.
            plaintext: SecretBytes::from_vec(plaintext.as_slice().to_vec()),
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
            || secret_ref.kind != SecretKind::IdentityRecoveryPending
        {
            return Err(crate::ImError::PermissionDenied);
        }
        self.vault.delete(secret_ref)
    }

    fn load_by_key(
        &self,
        key_id: &str,
    ) -> crate::ImResult<Option<(SecretRef, PendingRecoveryRecord)>> {
        let mut matches = self.vault.list()?.into_iter().filter(|secret_ref| {
            secret_ref.workspace_id == self.workspace_id
                && secret_ref.device_id == self.device_id
                && secret_ref.kind == SecretKind::IdentityRecoveryPending
                && secret_ref.key_id == key_id
        });
        let found = matches.next();
        if matches.next().is_some() {
            return Err(crate::ImError::PermissionDenied);
        }
        found
            .map(|secret_ref| {
                let record = self.open_record(&secret_ref)?;
                Ok((secret_ref, record))
            })
            .transpose()
    }

    fn open_record(&self, secret_ref: &SecretRef) -> crate::ImResult<PendingRecoveryRecord> {
        let plaintext = self.vault.open(secret_ref)?;
        let record: PendingRecoveryRecord = serde_json::from_slice(plaintext.expose_secret())
            .map_err(|_| crate::ImError::PermissionDenied)?;
        record.validate()?;
        if secret_ref.key_id != pending_key_id(record.binding.handle.as_str()) {
            return Err(crate::ImError::PermissionDenied);
        }
        Ok(record)
    }
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PendingRecoveryCancelRecord {
    schema_version: u32,
    pub(crate) old_did: crate::ids::Did,
    pub(crate) signing_key_id: String,
    pub(crate) prepared: crate::internal::identity_wire::device_recovery::PreparedRecoveryCancel,
}

impl std::fmt::Debug for PendingRecoveryCancelRecord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PendingRecoveryCancelRecord")
            .field("schema_version", &self.schema_version)
            .field("old_did", &self.old_did)
            .field("signing_key_id", &self.signing_key_id)
            .field("prepared", &self.prepared)
            .finish()
    }
}

impl PendingRecoveryCancelRecord {
    pub(crate) fn new(
        old_did: crate::ids::Did,
        signing_key_id: String,
        prepared: crate::internal::identity_wire::device_recovery::PreparedRecoveryCancel,
    ) -> crate::ImResult<Self> {
        let record = Self {
            schema_version: SCHEMA_VERSION,
            old_did,
            signing_key_id,
            prepared,
        };
        record.validate()?;
        Ok(record)
    }

    fn validate(&self) -> crate::ImResult<()> {
        if self.schema_version != SCHEMA_VERSION
            || self.prepared.operation_id.trim().is_empty()
            || self.prepared.recovery_session_id.trim().is_empty()
            || self.prepared.authorizing_device_id.trim().is_empty()
            || self.signing_key_id != self.prepared.authorizing_device_proof.key_id
            || !self
                .signing_key_id
                .starts_with(&format!("{}#", self.old_did.as_str()))
        {
            return Err(crate::ImError::PermissionDenied);
        }
        Ok(())
    }
}

pub(crate) struct PendingRecoveryCancelStore {
    workspace_id: String,
    device_id: String,
    vault: std::sync::Arc<dyn SecretVault + Send + Sync>,
}

impl PendingRecoveryCancelStore {
    pub(crate) fn from_core(core: &crate::core::ImCore) -> crate::ImResult<Self> {
        let context =
            core.inner()
                .identity_vault()
                .ok_or_else(|| crate::ImError::LocalStateUnavailable {
                    detail: "Handle Recovery cancel requires an available identity secret vault"
                        .to_owned(),
                })?;
        if core.inner().identity_secret_storage_policy()
            != crate::core::IdentitySecretStoragePolicy::VaultRequired
        {
            return Err(crate::ImError::LocalStateUnavailable {
                detail:
                    "Handle Recovery cancel requires IdentitySecretStoragePolicy::VaultRequired"
                        .to_owned(),
            });
        }
        Ok(Self {
            workspace_id: context.workspace_id().to_owned(),
            device_id: context.vault_context_device_id().as_str().to_owned(),
            vault: context.vault(),
        })
    }

    pub(crate) fn load(
        &self,
        recovery_session_id: &str,
        authorizing_device_id: &str,
    ) -> crate::ImResult<Option<(SecretRef, PendingRecoveryCancelRecord)>> {
        let key_id = cancel_key_id(recovery_session_id, authorizing_device_id);
        let mut matches = self.vault.list()?.into_iter().filter(|secret_ref| {
            secret_ref.workspace_id == self.workspace_id
                && secret_ref.device_id == self.device_id
                && secret_ref.kind == SecretKind::IdentityRecoveryCancelPending
                && secret_ref.key_id == key_id
        });
        let found = matches.next();
        if matches.next().is_some() {
            return Err(crate::ImError::PermissionDenied);
        }
        found
            .map(|secret_ref| {
                let plaintext = self.vault.open(&secret_ref)?;
                let record: PendingRecoveryCancelRecord =
                    serde_json::from_slice(plaintext.expose_secret())
                        .map_err(|_| crate::ImError::PermissionDenied)?;
                record.validate()?;
                if record.prepared.recovery_session_id != recovery_session_id
                    || record.prepared.authorizing_device_id != authorizing_device_id
                {
                    return Err(crate::ImError::PermissionDenied);
                }
                Ok((secret_ref, record))
            })
            .transpose()
    }

    pub(crate) fn save(&self, record: &PendingRecoveryCancelRecord) -> crate::ImResult<SecretRef> {
        record.validate()?;
        let plaintext = Zeroizing::new(serde_json::to_vec(record).map_err(|error| {
            crate::ImError::Serialization {
                detail: error.to_string(),
            }
        })?);
        let secret_ref = self.vault.seal(SealSecretRequest {
            metadata: SecretMetadata {
                workspace_id: self.workspace_id.clone(),
                device_id: self.device_id.clone(),
                identity_id: Some(identity_suffix(&record.old_did)),
                did: Some(record.old_did.as_str().to_owned()),
                kind: SecretKind::IdentityRecoveryCancelPending,
                key_id: cancel_key_id(
                    &record.prepared.recovery_session_id,
                    &record.prepared.authorizing_device_id,
                ),
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

    pub(crate) fn delete(&self, secret_ref: &SecretRef) -> crate::ImResult<()> {
        if secret_ref.workspace_id != self.workspace_id
            || secret_ref.device_id != self.device_id
            || secret_ref.kind != SecretKind::IdentityRecoveryCancelPending
        {
            return Err(crate::ImError::PermissionDenied);
        }
        self.vault.delete(secret_ref)
    }
}

fn pending_key_id(handle: &str) -> String {
    format!(
        "handle-recovery-{}",
        URL_SAFE_NO_PAD.encode(Sha256::digest(handle.as_bytes()))
    )
}

fn cancel_key_id(recovery_session_id: &str, authorizing_device_id: &str) -> String {
    format!(
        "handle-recovery-cancel-{}",
        URL_SAFE_NO_PAD.encode(Sha256::digest(
            format!("{recovery_session_id}\0{authorizing_device_id}").as_bytes()
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
    use super::*;

    fn test_config() -> crate::ImCoreConfig {
        crate::ImCoreConfig {
            service_base_url: crate::ServiceEndpoint::parse("https://example.test").unwrap(),
            did_domain: "awiki.info".to_owned(),
            user_service_endpoint: None,
            message_service_endpoint: None,
            mail_service_endpoint: None,
            anp_service_endpoint: None,
            anp_service_did: None,
            ca_bundle: None,
            transport_policy: crate::MessageTransportPolicy::HttpOnly,
        }
    }

    fn test_paths(root: &std::path::Path) -> crate::ImCorePaths {
        crate::ImCorePaths {
            identities: crate::IdentityRegistryPaths {
                identity_root_dir: root.join("identities"),
                registry_path: root.join("identities").join("registry.json"),
                default_identity_path: Some(root.join("identities").join("default")),
            },
            local_state: crate::LocalStatePaths {
                sqlite_path: root.join("local").join("im.sqlite"),
            },
            runtime: crate::RuntimePaths {
                cache_dir: root.join("cache"),
                temp_dir: root.join("tmp"),
            },
        }
    }

    fn open_vault_core(root: &std::path::Path) -> crate::ImCore {
        crate::ImCore::new_with_options(
            test_config(),
            test_paths(root),
            crate::ImCoreOpenOptions::default().with_identity_secret_vault(
                crate::IdentitySecretStoragePolicy::VaultRequired,
                crate::ImCoreSecretVaultOptions::new(
                    crate::vault::DeviceVaultRootKey::from_bytes([91_u8; 32]),
                    root.join("vault"),
                    "recovery-test-workspace",
                    "recovery-test-vault-device",
                ),
            ),
        )
        .unwrap()
    }

    fn binding() -> crate::internal::handle_discovery::RecoveryHandleBinding {
        crate::internal::handle_discovery::recovery_handle_binding_from_value(
            "alice.awiki.info",
            &serde_json::json!({
                "did": "did:wba:awiki.info:user:alice:e1_old",
                "full_handle": "alice.awiki.info",
                "status": "active",
                "binding_generation": "3"
            }),
        )
        .unwrap()
    }

    #[test]
    fn debug_redacts_all_replayable_and_private_material() {
        let record = PendingRecoveryRecord::new(
            binding(),
            "recovery-begin-op".to_owned(),
            "begin-token-secret".to_owned(),
        )
        .unwrap();
        let debug = format!("{record:?}");
        assert!(!debug.contains("begin-token-secret"));
        assert!(debug.contains("has_begin_grant"));
        assert!(std::mem::needs_drop::<PendingRecoveryRecord>());
        assert!(std::mem::needs_drop::<
            crate::internal::identity_wire::device_recovery::RecoverySessionResult,
        >());
        assert!(std::mem::needs_drop::<
            crate::internal::identity_wire::device_recovery::RecoveryFinalizeResult,
        >());
        let forbidden_unzeroized_copy = ["plaintext", ".clone()"].concat();
        assert!(!include_str!("identity_recovery_pending.rs").contains(&forbidden_unzeroized_copy));
    }

    #[test]
    fn pending_finalize_semantic_retry_keeps_identity_and_refreshes_evidence() {
        let root = tempfile::tempdir().unwrap();
        let core = open_vault_core(root.path());
        let store = PendingRecoveryStore::from_core(&core).unwrap();
        let mut record = PendingRecoveryRecord::new(
            binding(),
            "recovery-begin-op-stable".to_owned(),
            "begin-token-secret".to_owned(),
        )
        .unwrap();
        let first_ref = store.save(&record).unwrap();

        record.session = Some(
            crate::internal::identity_wire::device_recovery::RecoverySessionResult {
                recovery_session_id: "recovery-session-stable".to_owned(),
                recovery_session_token: "session-token-secret".to_owned(),
                account_user_id: "user-alice".to_owned(),
                old_did: record.binding.did.as_str().to_owned(),
                state: crate::internal::identity_wire::device_recovery::RecoveryRemoteState::Ready,
                cooling_until: "2030-01-01T00:00:00Z".to_owned(),
                expires_at: "2030-01-02T00:00:00Z".to_owned(),
            },
        );
        record.replace_begin_grant(None);
        let generated = crate::internal::identity_generation::generate_vnext_handle_identity_with_default_daemon_subkey(
            "awiki.info", "alice", None, None,
        ).unwrap();
        let prepared = crate::internal::identity_wire::device_recovery::prepare_recovery_finalize(
            &generated,
            "recovery-finalize-op-stable".to_owned(),
            record.binding.mapping_generation,
            time::OffsetDateTime::parse(
                "2029-12-31T00:00:00Z",
                &time::format_description::well_known::Rfc3339,
            )
            .unwrap(),
        )
        .unwrap();
        record.generated = Some(generated.clone());
        record.prepared_finalize = Some(prepared.clone());
        record.replace_reconfirmation_token("reconfirmation-token-secret".to_owned());
        record.token_issue_operation_id = Some("recovery-token-op-first".to_owned());
        record.local_alias = Some("alice-recovered-stable".to_owned());

        let prepared_ref = store.save(&record).unwrap();
        assert_eq!(first_ref, prepared_ref);
        assert_eq!(store.list().unwrap().len(), 1);
        drop(store);
        drop(core);

        let restarted = open_vault_core(root.path());
        let restarted_store = PendingRecoveryStore::from_core(&restarted).unwrap();
        let (restarted_ref, mut restarted_record) = restarted_store
            .load_by_session("recovery-session-stable")
            .unwrap()
            .unwrap();
        assert_eq!(restarted_ref, first_ref);
        assert_eq!(
            restarted_record.begin_operation_id,
            "recovery-begin-op-stable"
        );
        let restarted_generated = restarted_record.generated.as_ref().unwrap();
        assert_eq!(restarted_generated.did, generated.did);
        assert_eq!(restarted_generated.unique_id, generated.unique_id);
        assert_eq!(restarted_generated.did_document, generated.did_document);
        assert!(restarted_generated.root_private_pem == generated.root_private_pem);
        assert!(
            restarted_generated.device_signing_private_pem == generated.device_signing_private_pem
        );
        assert!(restarted_generated.device_e2ee_private_pem == generated.device_e2ee_private_pem);
        assert!(
            restarted_generated
                .daemon_subkey_package
                .private_key_material()
                .trim()
                == generated
                    .daemon_subkey_package
                    .private_key_material()
                    .trim()
        );
        assert_eq!(restarted_record.prepared_finalize.as_ref(), Some(&prepared));
        assert_eq!(
            restarted_record.token_issue_operation_id.as_deref(),
            Some("recovery-token-op-first")
        );
        assert!(
            restarted_record.reconfirmation_token.as_deref() == Some("reconfirmation-token-secret")
        );

        let first_proof_nonce = restarted_record
            .prepared_finalize
            .as_ref()
            .unwrap()
            .bootstrap_device_proof
            .nonce
            .clone();
        let refreshed = crate::internal::identity_wire::device_recovery::prepare_recovery_finalize(
            restarted_record.generated.as_ref().unwrap(),
            restarted_record
                .prepared_finalize
                .as_ref()
                .unwrap()
                .operation_id
                .clone(),
            restarted_record
                .prepared_finalize
                .as_ref()
                .unwrap()
                .expected_handle_mapping_generation,
            time::OffsetDateTime::parse(
                "2029-12-31T00:01:00Z",
                &time::format_description::well_known::Rfc3339,
            )
            .unwrap(),
        )
        .unwrap();
        restarted_record.prepared_finalize = Some(refreshed);
        restarted_record
            .replace_reconfirmation_token("fresh-reconfirmation-token-secret".to_owned());
        // A lost device_token_issue response may outlive that operation's
        // access TTL. Rotating this credential operation must not rotate the
        // Recovery cutover operation, document, or generated keys.
        restarted_record.token_issue_operation_id = Some("recovery-token-op-second".to_owned());
        let retry_ref = restarted_store.save(&restarted_record).unwrap();
        assert_eq!(retry_ref, first_ref);
        drop(restarted_record);

        let (_, retried_record) = restarted_store
            .load_by_session("recovery-session-stable")
            .unwrap()
            .unwrap();
        let retried_generated = retried_record.generated.as_ref().unwrap();
        let retried_prepared = retried_record.prepared_finalize.as_ref().unwrap();
        assert_eq!(retried_prepared.operation_id, prepared.operation_id);
        assert_eq!(
            retried_prepared.expected_handle_mapping_generation,
            prepared.expected_handle_mapping_generation
        );
        assert_eq!(retried_prepared.new_did_document, prepared.new_did_document);
        assert_eq!(
            retried_prepared.bootstrap_device_id,
            prepared.bootstrap_device_id
        );
        assert_ne!(
            retried_prepared.bootstrap_device_proof.nonce,
            first_proof_nonce
        );
        assert_eq!(retried_generated.did, generated.did);
        assert!(retried_generated.root_private_pem == generated.root_private_pem);
        assert!(
            retried_generated.device_signing_private_pem == generated.device_signing_private_pem
        );
        assert!(retried_generated.device_e2ee_private_pem == generated.device_e2ee_private_pem);
        assert!(
            retried_record.reconfirmation_token.as_deref()
                == Some("fresh-reconfirmation-token-secret")
        );
        assert_eq!(
            retried_record.token_issue_operation_id.as_deref(),
            Some("recovery-token-op-second")
        );

        let vault_record = std::fs::read_to_string(
            std::fs::read_dir(root.path().join("vault/records"))
                .unwrap()
                .next()
                .unwrap()
                .unwrap()
                .path(),
        )
        .unwrap();
        for secret in [
            "session-token-secret",
            "reconfirmation-token-secret",
            "fresh-reconfirmation-token-secret",
            generated.root_private_pem.as_str(),
            generated.device_signing_private_pem.as_str(),
            generated.device_e2ee_private_pem.as_str(),
        ] {
            assert!(!vault_record.contains(secret));
        }
    }

    #[test]
    fn fresh_begin_grant_keeps_the_same_operation_and_handle_binding() {
        let root = tempfile::tempdir().unwrap();
        let core = open_vault_core(root.path());
        let store = PendingRecoveryStore::from_core(&core).unwrap();
        let mut record = PendingRecoveryRecord::new(
            binding(),
            "recovery-begin-stable-operation".to_owned(),
            "expired-begin-grant".to_owned(),
        )
        .unwrap();
        let first_ref = store.save(&record).unwrap();
        let stable_binding = record.binding.clone();

        record.replace_begin_grant(Some("fresh-begin-grant".to_owned()));
        let retry_ref = store.save(&record).unwrap();
        let (_, restarted) = store
            .load_by_handle(&stable_binding.handle)
            .unwrap()
            .unwrap();

        assert_eq!(retry_ref, first_ref);
        assert_eq!(restarted.begin_operation_id, record.begin_operation_id);
        assert_eq!(restarted.binding, stable_binding);
        assert!(restarted.begin_grant.as_deref() == Some("fresh-begin-grant"));
        assert!(restarted.session.is_none());
        assert!(restarted.generated.is_none());
    }
}
