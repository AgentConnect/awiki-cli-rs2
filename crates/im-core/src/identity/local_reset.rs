//! Exact local custody ownership for explicit Host reset. Provider enumeration is not ownership.

use super::IdentityRegistry;
use crate::internal::identity_handle_recovery_pending::{
    PendingHandleRecoveryStore, PendingHandleRecoveryV4,
};
use crate::internal::identity_join_activation_pending::{
    PendingJoinActivation, PendingJoinActivationStore,
};
use crate::internal::identity_legacy_upgrade_pending::{
    PendingLegacyUpgrade, PendingLegacyUpgradeStore,
};
use crate::internal::identity_registration_pending::{
    PendingRegistration, PendingRegistrationStore,
};
use crate::internal::secret_vault::record::SecretKind;
use crate::provider::ProviderIdentityRef;

impl IdentityRegistry<'_> {
    /// Return only provider identities bound to this Core root's Registry or validated pending work.
    /// Hosts must quiesce local mutations before collecting these references and keep local evidence
    /// until deletion succeeds. Unknown identities in a shared provider are deliberately preserved.
    pub fn local_provider_identity_references(&self) -> crate::ImResult<Vec<ProviderIdentityRef>> {
        let mut references = Vec::new();
        for entry in self.load_registry()?.entries {
            if entry.identity_custody_backend.as_deref() == Some("anp_identity") {
                references.push(ProviderIdentityRef {
                    store_id: entry
                        .anp_identity_store_id
                        .ok_or(crate::ImError::PermissionDenied)?,
                    identity_id: entry
                        .anp_identity_id
                        .ok_or(crate::ImError::PermissionDenied)?,
                    did: entry.summary.did.as_str().to_owned(),
                });
            } else if entry.anp_identity_store_id.is_some() || entry.anp_identity_id.is_some() {
                return Err(crate::ImError::PermissionDenied);
            }
        }
        references.extend(
            crate::internal::identity_device_join::local_provider_identity_references(self.core)?,
        );
        if let Some(context) = self.core.inner().identity_vault() {
            let vault = context.vault();
            for secret in vault.list()? {
                if secret.workspace_id != context.workspace_id()
                    || secret.device_id != context.vault_context_device_id().as_str()
                {
                    continue;
                }
                match secret.kind {
                    SecretKind::IdentityRegistrationPending => {
                        let plaintext = vault.open(&secret)?;
                        let pending: PendingRegistration =
                            serde_json::from_slice(plaintext.expose_secret())
                                .map_err(|_| crate::ImError::PermissionDenied)?;
                        let (bound, pending) = PendingRegistrationStore::from_core(self.core)?
                            .load(&pending.target_handle, &pending.target_domain)?
                            .ok_or(crate::ImError::PermissionDenied)?;
                        if bound != secret {
                            return Err(crate::ImError::PermissionDenied);
                        }
                        references.push(ProviderIdentityRef {
                            store_id: pending.identity.controller_store_id,
                            identity_id: pending.identity.controller_identity_id,
                            did: pending.identity.did.as_str().to_owned(),
                        });
                    }
                    SecretKind::IdentityHandleRecoveryPending => {
                        let plaintext = vault.open(&secret)?;
                        let pending: PendingHandleRecoveryV4 =
                            serde_json::from_slice(plaintext.expose_secret())
                                .map_err(|_| crate::ImError::PermissionDenied)?;
                        let (bound, pending) = PendingHandleRecoveryStore::from_core(self.core)?
                            .load_v4(&pending.operation_id)?
                            .ok_or(crate::ImError::PermissionDenied)?;
                        if bound != secret {
                            return Err(crate::ImError::PermissionDenied);
                        }
                        references.push(ProviderIdentityRef {
                            store_id: pending.identity.store_id,
                            identity_id: pending.identity.identity_id,
                            did: pending.identity.did.as_str().to_owned(),
                        });
                    }
                    SecretKind::IdentityJoinActivationPending => {
                        let plaintext = vault.open(&secret)?;
                        let pending: PendingJoinActivation =
                            serde_json::from_slice(plaintext.expose_secret())
                                .map_err(|_| crate::ImError::PermissionDenied)?;
                        let (bound, pending) = PendingJoinActivationStore::from_core(self.core)?
                            .load(&pending.join_session_id, &pending.did)?
                            .ok_or(crate::ImError::PermissionDenied)?;
                        if bound != secret {
                            return Err(crate::ImError::PermissionDenied);
                        }
                        references.push(ProviderIdentityRef {
                            store_id: pending.custody.store_id,
                            identity_id: pending.custody.identity_id,
                            did: pending.did.as_str().to_owned(),
                        });
                    }
                    SecretKind::IdentityLegacyUpgradePending => {
                        let plaintext = vault.open(&secret)?;
                        let pending: PendingLegacyUpgrade =
                            serde_json::from_slice(plaintext.expose_secret())
                                .map_err(|_| crate::ImError::PermissionDenied)?;
                        let (bound, pending) = PendingLegacyUpgradeStore::from_core(self.core)?
                            .load(&pending.local_alias)?
                            .ok_or(crate::ImError::PermissionDenied)?;
                        if bound != secret {
                            return Err(crate::ImError::PermissionDenied);
                        }
                        references.push(ProviderIdentityRef {
                            store_id: pending.identity.custody.store_id,
                            identity_id: pending.identity.custody.identity_id,
                            did: pending.identity.did.as_str().to_owned(),
                        });
                    }
                    _ => {}
                }
            }
        }
        // Validate the entire plan before any caller starts deletion, including conflicting DID bindings.
        let mut exact = std::collections::BTreeMap::new();
        for reference in references {
            if reference.store_id.is_empty()
                || reference.identity_id.is_empty()
                || reference.did.is_empty()
            {
                return Err(crate::ImError::PermissionDenied);
            }
            let key = (reference.store_id.clone(), reference.identity_id.clone());
            if let Some(previous) = exact.insert(key, reference.clone()) {
                if previous != reference {
                    return Err(crate::ImError::PermissionDenied);
                }
            }
        }
        Ok(exact.into_values().collect())
    }
}

#[cfg(test)]
#[path = "local_reset_tests.rs"]
mod tests;
