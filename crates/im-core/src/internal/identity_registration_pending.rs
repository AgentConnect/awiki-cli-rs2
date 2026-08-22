//! Vault-only crash-convergence record for the normal `register` flow.
//!
//! The record preserves only public anp-identity references and the exact
//! public vNext document across ambiguous remote results and local activation
//! retries. It never stores identity private keys, OTP plaintext, a Genesis
//! grant/proof, or a refresh token.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::internal::platform_secret::SecretBytes;
use crate::internal::secret_vault::policy::SecretAccessPolicy;
use crate::internal::secret_vault::record::{SecretKind, SecretMetadata, SecretRef};
use crate::internal::secret_vault::{SealSecretRequest, SecretVault};

const SCHEMA_VERSION: u32 = 2;
const KEY_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PendingRegistrationPhase {
    Prepared,
    RemoteCommitted,
    LocalCommitted,
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PendingRegistration {
    schema_version: u32,
    pub(crate) target_handle: String,
    pub(crate) target_domain: String,
    pub(crate) local_alias: String,
    pub(crate) display_name: String,
    pub(crate) make_default: bool,
    pub(crate) verification_kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) verification_target: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) invite_code: Option<String>,
    pub(crate) identity: PendingRegistrationIdentity,
    pub(crate) document_hash: String,
    pub(crate) phase: PendingRegistrationPhase,
    #[serde(default)]
    pub(crate) remote_attempted: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) remote_result: Option<PendingRegistrationRemoteResult>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PendingRegistrationIdentity {
    pub(crate) controller_store_id: String,
    pub(crate) controller_identity_id: String,
    pub(crate) did: crate::ids::Did,
    pub(crate) did_document: serde_json::Value,
    pub(crate) protocol_device_id: crate::ids::ProtocolDeviceId,
    pub(crate) root_key_id: String,
    pub(crate) device_signing_key_id: String,
    pub(crate) device_e2ee_key_id: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub(crate) legacy_daemon_authorization: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) controller_revision_id: Option<String>,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PendingRegistrationRemoteResult {
    pub(crate) did: String,
    pub(crate) user_id: String,
    pub(crate) handle: String,
    pub(crate) full_handle: String,
    pub(crate) binding_generation: String,
    pub(crate) access_token: String,
}

impl std::fmt::Debug for PendingRegistration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PendingRegistration")
            .field("target_handle", &self.target_handle)
            .field("target_domain", &self.target_domain)
            .field("local_alias", &self.local_alias)
            .field("display_name", &self.display_name)
            .field("make_default", &self.make_default)
            .field("verification_kind", &self.verification_kind)
            .field(
                "has_verification_target",
                &self.verification_target.is_some(),
            )
            .field("has_invite_code", &self.invite_code.is_some())
            .field("identity", &self.identity)
            .field("document_hash", &self.document_hash)
            .field("phase", &self.phase)
            .field("remote_attempted", &self.remote_attempted)
            .field("has_remote_result", &self.remote_result.is_some())
            .finish()
    }
}

impl PendingRegistration {
    pub(crate) fn new(
        target_handle: String,
        target_domain: String,
        local_alias: String,
        display_name: String,
        make_default: bool,
        verification_kind: String,
        verification_target: Option<String>,
        invite_code: Option<String>,
        identity: PendingRegistrationIdentity,
    ) -> crate::ImResult<Self> {
        let document_hash =
            crate::internal::identity_wire::document::document_hash(&identity.did_document)?;
        let pending = Self {
            schema_version: SCHEMA_VERSION,
            target_handle,
            target_domain,
            local_alias,
            display_name,
            make_default,
            verification_kind,
            verification_target,
            invite_code,
            identity,
            document_hash,
            phase: PendingRegistrationPhase::Prepared,
            remote_attempted: false,
            remote_result: None,
        };
        pending.validate()?;
        Ok(pending)
    }

    pub(crate) fn validate(&self) -> crate::ImResult<()> {
        if self.schema_version != SCHEMA_VERSION
            || self.target_handle.trim().is_empty()
            || self.target_domain.trim().is_empty()
            || self.local_alias.trim().is_empty()
            || self.display_name.trim().is_empty()
            || !matches!(
                self.verification_kind.as_str(),
                "phone" | "email" | "otp" | "already_verified"
            )
            || self.document_hash
                != crate::internal::identity_wire::document::document_hash(
                    &self.identity.did_document,
                )?
        {
            return Err(crate::ImError::PermissionDenied);
        }
        match (&self.phase, &self.remote_result) {
            (PendingRegistrationPhase::Prepared, None)
            | (
                PendingRegistrationPhase::RemoteCommitted
                | PendingRegistrationPhase::LocalCommitted,
                Some(_),
            ) => {}
            _ => return Err(crate::ImError::PermissionDenied),
        }
        if let Some(remote) = &self.remote_result {
            if !self.remote_attempted
                || remote.did != self.identity.did.as_str()
                || remote.user_id.trim().is_empty()
                || remote.handle != self.target_handle
                || remote.full_handle != format!("{}.{}", self.target_handle, self.target_domain)
                || anp::wns::BindingGeneration::new(remote.binding_generation.clone()).is_err()
                || remote.access_token.trim().is_empty()
            {
                return Err(crate::ImError::PermissionDenied);
            }
        }
        self.identity.validate()?;
        Ok(())
    }

    pub(crate) fn replace_prepared_document(
        &mut self,
        did_document: serde_json::Value,
        controller_revision_id: Option<String>,
    ) -> crate::ImResult<()> {
        if self.phase != PendingRegistrationPhase::Prepared
            || self.remote_result.is_some()
            || !self.remote_attempted
        {
            return Err(crate::ImError::PermissionDenied);
        }
        self.identity.did_document = did_document;
        self.identity.controller_revision_id = controller_revision_id;
        self.document_hash =
            crate::internal::identity_wire::document::document_hash(&self.identity.did_document)?;
        self.remote_attempted = false;
        self.validate()
    }
}

impl PendingRegistrationIdentity {
    pub(crate) fn validate(&self) -> crate::ImResult<()> {
        let daemon_kid = format!("{}#daemon-key-1", self.did.as_str());
        let daemon_method_present =
            anp::authentication::find_verification_method(&self.did_document, &daemon_kid)
                .is_some();
        let daemon_authenticated =
            anp::authentication::is_authentication_authorized(&self.did_document, &daemon_kid);
        let daemon_assertion =
            anp::authentication::is_assertion_method_authorized(&self.did_document, &daemon_kid);
        let daemon_authorization_valid = if self.legacy_daemon_authorization {
            daemon_method_present && daemon_authenticated && !daemon_assertion
        } else {
            !daemon_method_present && !daemon_authenticated && !daemon_assertion
        };
        if self.controller_store_id.trim().is_empty()
            || self.controller_identity_id.trim().is_empty()
            || self
                .did_document
                .get("id")
                .and_then(serde_json::Value::as_str)
                != Some(self.did.as_str())
            || self.root_key_id != format!("{}#key-1", self.did.as_str())
            || self.device_signing_key_id
                != format!(
                    "{}#{}-sign",
                    self.did.as_str(),
                    self.protocol_device_id.as_str()
                )
            || self.device_e2ee_key_id
                != format!(
                    "{}#{}-e2ee",
                    self.did.as_str(),
                    self.protocol_device_id.as_str()
                )
            || self
                .controller_revision_id
                .as_ref()
                .is_some_and(|revision_id| revision_id.trim().is_empty())
        {
            return Err(crate::ImError::PermissionDenied);
        }
        let manifest = anp::authentication::validate_device_manifest(&self.did_document)
            .map_err(|_| crate::ImError::PermissionDenied)?
            .ok_or(crate::ImError::PermissionDenied)?;
        if manifest.devices.len() != 1
            || manifest.devices[0].device_id != self.protocol_device_id.as_str()
            || manifest.devices[0].signing_key_id != self.device_signing_key_id
            || manifest.devices[0].e2ee_key_id != self.device_e2ee_key_id
            || !daemon_authorization_valid
        {
            return Err(crate::ImError::PermissionDenied);
        }
        Ok(())
    }
}

pub(crate) struct PendingRegistrationStore {
    workspace_id: String,
    device_id: String,
    vault: std::sync::Arc<dyn SecretVault + Send + Sync>,
}

impl PendingRegistrationStore {
    pub(crate) fn from_core(core: &crate::core::ImCore) -> crate::ImResult<Self> {
        if core.inner().identity_secret_storage_policy()
            != crate::core::IdentitySecretStoragePolicy::VaultRequired
        {
            return Err(crate::ImError::LocalStateUnavailable {
                detail: "registration requires IdentitySecretStoragePolicy::VaultRequired"
                    .to_owned(),
            });
        }
        let context =
            core.inner()
                .identity_vault()
                .ok_or_else(|| crate::ImError::LocalStateUnavailable {
                    detail: "registration requires an available identity SecretVault".to_owned(),
                })?;
        Ok(Self {
            workspace_id: context.workspace_id().to_owned(),
            device_id: context.vault_context_device_id().as_str().to_owned(),
            vault: context.vault(),
        })
    }

    pub(crate) fn load(
        &self,
        handle: &str,
        domain: &str,
    ) -> crate::ImResult<Option<(SecretRef, PendingRegistration)>> {
        let key_id = pending_key_id(handle, domain);
        let matches = self
            .vault
            .list()?
            .into_iter()
            .filter(|secret_ref| {
                secret_ref.workspace_id == self.workspace_id
                    && secret_ref.device_id == self.device_id
                    && secret_ref.kind == SecretKind::IdentityRegistrationPending
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
        let pending: PendingRegistration = serde_json::from_slice(plaintext.expose_secret())
            .map_err(|_| crate::ImError::PermissionDenied)?;
        pending.validate()?;
        if pending.target_handle != handle || pending.target_domain != domain {
            return Err(crate::ImError::PermissionDenied);
        }
        Ok(Some((secret_ref, pending)))
    }

    pub(crate) fn save(&self, pending: &PendingRegistration) -> crate::ImResult<SecretRef> {
        pending.validate()?;
        let plaintext =
            serde_json::to_vec(pending).map_err(|err| crate::ImError::Serialization {
                detail: err.to_string(),
            })?;
        let secret_ref = self.vault.seal(SealSecretRequest {
            metadata: SecretMetadata {
                workspace_id: self.workspace_id.clone(),
                device_id: self.device_id.clone(),
                identity_id: Some(pending.identity.controller_identity_id.clone()),
                did: Some(pending.identity.did.as_str().to_owned()),
                kind: SecretKind::IdentityRegistrationPending,
                key_id: pending_key_id(&pending.target_handle, &pending.target_domain),
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
            || secret_ref.kind != SecretKind::IdentityRegistrationPending
        {
            return Err(crate::ImError::PermissionDenied);
        }
        self.vault.delete(secret_ref)
    }
}

fn pending_key_id(handle: &str, domain: &str) -> String {
    let digest = Sha256::digest(format!("{handle}@{domain}").as_bytes());
    format!("registration-{}", URL_SAFE_NO_PAD.encode(digest))
}

fn is_false(value: &bool) -> bool {
    !*value
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_registration_contains_no_verification_or_refresh_secret() {
        let pending = PendingRegistration::new(
            "alice".to_owned(),
            "example.test".to_owned(),
            "alice".to_owned(),
            "Alice".to_owned(),
            true,
            "phone".to_owned(),
            Some("+15555550123".to_owned()),
            Some("invite-public-id".to_owned()),
            identity(),
        )
        .unwrap();

        let encoded = serde_json::to_string(&pending).unwrap();
        assert!(!encoded.contains("otp"));
        assert!(!encoded.contains("grant"));
        assert!(!encoded.contains("refresh_token"));
        assert!(!encoded.contains("PRIVATE KEY"));
        assert!(!encoded.contains("private_pem"));
        assert!(!format!("{pending:?}").contains("PRIVATE KEY"));
    }

    #[test]
    fn prepared_document_replacement_keeps_custody_and_key_references() {
        let mut pending = PendingRegistration::new(
            "alice".to_owned(),
            "example.test".to_owned(),
            "alice".to_owned(),
            "Alice".to_owned(),
            true,
            "already_verified".to_owned(),
            None,
            None,
            identity(),
        )
        .unwrap();
        pending.remote_attempted = true;
        let original = pending.identity.clone();
        let original_hash = pending.document_hash.clone();
        let mut replacement = pending.identity.did_document.clone();
        replacement["proof"]["created"] = serde_json::json!("2099-01-01T00:00:00Z");

        pending
            .replace_prepared_document(replacement, Some("revision-next".to_owned()))
            .unwrap();

        assert_eq!(pending.identity.did, original.did);
        assert_eq!(
            pending.identity.controller_identity_id,
            original.controller_identity_id
        );
        assert_eq!(
            pending.identity.device_e2ee_key_id,
            original.device_e2ee_key_id
        );
        assert_ne!(pending.identity.did_document, original.did_document);
        assert_ne!(pending.document_hash, original_hash);
        assert!(!pending.remote_attempted);
        pending.validate().unwrap();
    }

    fn identity() -> PendingRegistrationIdentity {
        let create = crate::internal::identity_generation::vnext_handle_anp_identity_create_spec(
            "example.test",
            "alice",
            None,
            None,
        )
        .unwrap();
        let root = tempfile::tempdir().unwrap();
        let mut store = anp_identity::DidStore::initialize_local_file(root.path()).unwrap();
        let generated = store
            .create_identity(crate::internal::identity_custody::native_create_spec(
                create.spec,
            ))
            .unwrap();
        let manifest = anp::authentication::validate_device_manifest(generated.document())
            .unwrap()
            .unwrap();
        let device = &manifest.devices[0];
        PendingRegistrationIdentity {
            controller_store_id: "controller-store".to_owned(),
            controller_identity_id: "controller-identity".to_owned(),
            did: crate::ids::Did::parse(generated.did()).unwrap(),
            did_document: generated.document().clone(),
            protocol_device_id: crate::ids::ProtocolDeviceId::parse(&device.device_id).unwrap(),
            root_key_id: format!("{}#key-1", generated.did()),
            device_signing_key_id: device.signing_key_id.clone(),
            device_e2ee_key_id: device.e2ee_key_id.clone(),
            legacy_daemon_authorization: false,
            controller_revision_id: Some("revision-1".to_owned()),
        }
    }
}
