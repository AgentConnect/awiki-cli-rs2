use super::Pending;
use crate::internal::platform_secret::SecretBytes;
use crate::internal::secret_vault::policy::SecretAccessPolicy;
use crate::internal::secret_vault::record::{SecretKind, SecretMetadata, SecretRef};
use crate::internal::secret_vault::{SealSecretRequest, SecretVault};

pub(super) struct Store {
    workspace: String,
    device: String,
    vault: std::sync::Arc<dyn SecretVault + Send + Sync>,
}

impl Store {
    pub(super) fn new(core: &crate::ImCore) -> crate::ImResult<Self> {
        if core.inner().identity_secret_storage_policy()
            != crate::IdentitySecretStoragePolicy::VaultRequired
        {
            return Err(crate::ImError::PermissionDenied);
        }
        let context = core
            .inner()
            .identity_vault()
            .ok_or(crate::ImError::PermissionDenied)?;
        Ok(Self {
            workspace: context.workspace_id().into(),
            device: context.vault_context_device_id().as_str().into(),
            vault: context.vault(),
        })
    }

    fn reference(&self, did: &crate::ids::Did) -> crate::ImResult<Option<SecretRef>> {
        let matches = self
            .vault
            .list()?
            .into_iter()
            .filter(|r| {
                r.workspace_id == self.workspace
                    && r.device_id == self.device
                    && r.kind == SecretKind::IdentityServicesUpdatePending
                    && r.did.as_deref() == Some(did.as_str())
            })
            .collect::<Vec<_>>();
        if matches.len() > 1 {
            return Err(crate::ImError::PermissionDenied);
        }
        Ok(matches.into_iter().next())
    }

    pub(super) fn load(&self, did: &crate::ids::Did) -> crate::ImResult<Option<Pending>> {
        self.reference(did)?
            .map(|reference| {
                let plaintext = self.vault.open(&reference)?;
                let pending: Pending = serde_json::from_slice(plaintext.expose_secret())
                    .map_err(|_| crate::ImError::PermissionDenied)?;
                pending.validate()?;
                if pending.did != *did {
                    return Err(crate::ImError::PermissionDenied);
                }
                Ok(pending)
            })
            .transpose()
    }

    pub(super) fn save(&self, pending: &Pending) -> crate::ImResult<()> {
        pending.validate()?;
        let bytes = zeroize::Zeroizing::new(
            serde_json::to_vec(pending).map_err(|_| crate::ImError::PermissionDenied)?,
        );
        let reference = self.vault.seal(SealSecretRequest {
            metadata: SecretMetadata {
                workspace_id: self.workspace.clone(),
                device_id: self.device.clone(),
                identity_id: None,
                did: Some(pending.did.as_str().into()),
                kind: SecretKind::IdentityServicesUpdatePending,
                key_id: "ordinary-services-update".into(),
                key_version: 1,
                policy: SecretAccessPolicy::no_prompt_local_secret(),
            },
            plaintext: SecretBytes::from_vec(bytes.to_vec()),
        })?;
        if self.vault.open(&reference)?.expose_secret() != bytes.as_slice() {
            return Err(crate::ImError::PermissionDenied);
        }
        Ok(())
    }

    pub(super) fn delete(&self, did: &crate::ids::Did) -> crate::ImResult<()> {
        if let Some(reference) = self.reference(did)? {
            self.vault.delete(&reference)?;
        }
        Ok(())
    }
}
