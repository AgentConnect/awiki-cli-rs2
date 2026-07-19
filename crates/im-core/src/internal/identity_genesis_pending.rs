//! Vault-only crash recovery record for one vNext identity Genesis.
//!
//! This is local implementation state, not an ANP or user-service schema. It
//! keeps the generated private material, exact proof and returned token pair
//! encrypted until the normal identity store has committed successfully.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::internal::platform_secret::SecretBytes;
use crate::internal::secret_vault::policy::SecretAccessPolicy;
use crate::internal::secret_vault::record::{SecretKind, SecretMetadata, SecretRef};
use crate::internal::secret_vault::{SealSecretRequest, SecretVault};

const PENDING_GENESIS_SCHEMA_VERSION: u32 = 1;
const PENDING_KEY_VERSION: u32 = 1;

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PendingGenesisRecord {
    schema_version: u32,
    pub(crate) target_handle: String,
    pub(crate) target_domain: String,
    pub(crate) normalized_phone: String,
    pub(crate) local_alias: String,
    pub(crate) display_name: String,
    pub(crate) make_default: bool,
    pub(crate) idempotency_scope: String,
    pub(crate) generated:
        crate::internal::identity_generation::GeneratedVNextIdentityWithDaemonSubkey,
    pub(crate) prepared: crate::internal::identity_wire::device_genesis::PreparedDeviceGenesis,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) account_grant:
        Option<crate::internal::identity_wire::device_genesis::AccountVerificationGrant>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) remote_result:
        Option<crate::internal::identity_wire::device_genesis::DeviceGenesisResult>,
}

impl std::fmt::Debug for PendingGenesisRecord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PendingGenesisRecord")
            .field("schema_version", &self.schema_version)
            .field("target_handle", &self.target_handle)
            .field("target_domain", &self.target_domain)
            .field("normalized_phone", &"<redacted-phone>")
            .field("local_alias", &self.local_alias)
            .field("display_name", &self.display_name)
            .field("make_default", &self.make_default)
            .field("idempotency_scope", &self.idempotency_scope)
            .field("generated", &"<redacted-generated-identity>")
            .field("prepared", &self.prepared)
            .field("has_account_grant", &self.account_grant.is_some())
            .field("has_remote_result", &self.remote_result.is_some())
            .finish()
    }
}

pub(crate) struct NewPendingGenesis {
    pub(crate) target_handle: String,
    pub(crate) target_domain: String,
    pub(crate) normalized_phone: String,
    pub(crate) local_alias: String,
    pub(crate) display_name: String,
    pub(crate) make_default: bool,
    pub(crate) idempotency_scope: String,
    pub(crate) generated:
        crate::internal::identity_generation::GeneratedVNextIdentityWithDaemonSubkey,
    pub(crate) prepared: crate::internal::identity_wire::device_genesis::PreparedDeviceGenesis,
}

impl PendingGenesisRecord {
    pub(crate) fn new(input: NewPendingGenesis) -> crate::ImResult<Self> {
        let record = Self {
            schema_version: PENDING_GENESIS_SCHEMA_VERSION,
            target_handle: input.target_handle,
            target_domain: input.target_domain,
            normalized_phone: input.normalized_phone,
            local_alias: input.local_alias,
            display_name: input.display_name,
            make_default: input.make_default,
            idempotency_scope: input.idempotency_scope,
            generated: input.generated,
            prepared: input.prepared,
            account_grant: None,
            remote_result: None,
        };
        record.validate()?;
        Ok(record)
    }

    pub(crate) fn validate(&self) -> crate::ImResult<()> {
        if self.schema_version != PENDING_GENESIS_SCHEMA_VERSION
            || self.target_handle.trim().is_empty()
            || self.target_handle != self.target_handle.to_ascii_lowercase()
            || self.target_domain.trim().is_empty()
            || self.target_domain != self.target_domain.to_ascii_lowercase()
            || self.normalized_phone.trim().is_empty()
            || self.local_alias.trim().is_empty()
            || self.display_name.trim().is_empty()
            || self.idempotency_scope.trim().is_empty()
            || self.prepared.operation_id.trim().is_empty()
            || self.prepared.did_document != self.generated.did_document
            || self.prepared.bootstrap_device_id != self.generated.protocol_device_id.as_str()
            || self.prepared.bootstrap_device_proof.key_id != self.generated.device_signing_key_id
        {
            return Err(crate::ImError::PermissionDenied);
        }
        // Building the call re-verifies the exact persisted device proof without
        // exposing the placeholder token or performing network I/O.
        crate::internal::identity_wire::device_genesis::build_device_genesis_call(
            &self.prepared,
            "pending-record-validation-token",
        )?;
        if let Some(result) = &self.remote_result {
            if result.did != self.generated.did.as_str()
                || result.device.device_id != self.generated.protocol_device_id.as_str()
                || result.device.signing_key_id != self.generated.device_signing_key_id
                || result.device.e2ee_key_id != self.generated.device_e2ee_key_id
                || result.checkpoint.document_hash
                    != crate::internal::identity_wire::device_genesis::document_hash(
                        &self.generated.did_document,
                    )?
                || result.access_token.trim().is_empty()
                || result.refresh_token.trim().is_empty()
            {
                return Err(crate::ImError::PermissionDenied);
            }
        }
        Ok(())
    }
}

pub(crate) struct PendingGenesisStore {
    workspace_id: String,
    device_id: String,
    vault: std::sync::Arc<dyn SecretVault + Send + Sync>,
}

impl std::fmt::Debug for PendingGenesisStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PendingGenesisStore")
            .field("workspace_id", &self.workspace_id)
            .field("device_id", &self.device_id)
            .field("vault", &"<redacted-secret-vault>")
            .finish()
    }
}

impl PendingGenesisStore {
    pub(crate) fn from_core(core: &crate::core::ImCore) -> crate::ImResult<Self> {
        if core.inner().identity_secret_storage_policy()
            != crate::core::IdentitySecretStoragePolicy::VaultRequired
        {
            return Err(crate::ImError::LocalStateUnavailable {
                detail: "vNext Genesis requires IdentitySecretStoragePolicy::VaultRequired"
                    .to_owned(),
            });
        }
        let context =
            core.inner()
                .identity_vault()
                .ok_or_else(|| crate::ImError::LocalStateUnavailable {
                    detail: "vNext Genesis requires an available identity secret vault".to_owned(),
                })?;
        Ok(Self {
            workspace_id: context.workspace_id().to_owned(),
            device_id: context.vault_context_device_id().as_str().to_owned(),
            vault: context.vault(),
        })
    }

    pub(crate) fn load(
        &self,
        target_handle: &str,
        target_domain: &str,
    ) -> crate::ImResult<Option<(SecretRef, PendingGenesisRecord)>> {
        let key_id = pending_key_id(target_handle, target_domain);
        let matches = self
            .vault
            .list()?
            .into_iter()
            .filter(|secret_ref| {
                secret_ref.workspace_id == self.workspace_id
                    && secret_ref.device_id == self.device_id
                    && secret_ref.kind == SecretKind::IdentityGenesisPending
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
        let record: PendingGenesisRecord = serde_json::from_slice(plaintext.expose_secret())
            .map_err(|_| crate::ImError::PermissionDenied)?;
        record.validate()?;
        if record.target_handle != target_handle || record.target_domain != target_domain {
            return Err(crate::ImError::PermissionDenied);
        }
        Ok(Some((secret_ref, record)))
    }

    pub(crate) fn save(&self, record: &PendingGenesisRecord) -> crate::ImResult<SecretRef> {
        record.validate()?;
        let plaintext =
            serde_json::to_vec(record).map_err(|err| crate::ImError::Serialization {
                detail: err.to_string(),
            })?;
        let secret_ref = self.vault.seal(SealSecretRequest {
            metadata: SecretMetadata {
                workspace_id: self.workspace_id.clone(),
                device_id: self.device_id.clone(),
                identity_id: Some(record.generated.unique_id.clone()),
                did: Some(record.generated.did.as_str().to_owned()),
                kind: SecretKind::IdentityGenesisPending,
                key_id: pending_key_id(&record.target_handle, &record.target_domain),
                key_version: PENDING_KEY_VERSION,
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
            || secret_ref.kind != SecretKind::IdentityGenesisPending
        {
            return Err(crate::ImError::PermissionDenied);
        }
        self.vault.delete(secret_ref)
    }
}

fn pending_key_id(target_handle: &str, target_domain: &str) -> String {
    let digest = Sha256::digest(format!("{target_handle}@{target_domain}").as_bytes());
    format!("genesis-{}", URL_SAFE_NO_PAD.encode(digest))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_redacts_generated_identity_phone_and_token_state() {
        let generated = crate::internal::identity_generation::generate_vnext_handle_identity_with_default_daemon_subkey(
            "awiki.info", "alice", None, None,
        ).unwrap();
        let prepared = crate::internal::identity_wire::device_genesis::prepare_device_genesis(
            &generated,
            "op-test".to_owned(),
            time::OffsetDateTime::now_utc(),
        )
        .unwrap();
        let record = PendingGenesisRecord::new(NewPendingGenesis {
            target_handle: "alice".to_owned(),
            target_domain: "awiki.info".to_owned(),
            normalized_phone: "+8613800000000".to_owned(),
            local_alias: "alice".to_owned(),
            display_name: "Alice".to_owned(),
            make_default: true,
            idempotency_scope: "scope-test".to_owned(),
            generated,
            prepared,
        })
        .unwrap();
        let debug = format!("{record:?}");
        assert!(!debug.contains("+8613800000000"));
        assert!(!debug.contains(record.generated.root_private_pem.trim()));
        assert!(debug.contains("redacted-generated-identity"));
    }
}
