use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

pub struct IdentityRegistry<'a> {
    core: &'a crate::core::ImCore,
}

impl<'a> IdentityRegistry<'a> {
    pub(crate) fn new(core: &'a crate::core::ImCore) -> Self {
        Self { core }
    }

    pub fn list(&self) -> crate::ImResult<Vec<super::IdentitySummary>> {
        Ok(self
            .load_registry()?
            .entries
            .into_iter()
            .map(|entry| entry.summary)
            .collect())
    }

    pub async fn list_async(&self) -> crate::ImResult<Vec<super::IdentitySummary>> {
        Ok(self
            .load_registry_async()
            .await?
            .entries
            .into_iter()
            .map(|entry| entry.summary)
            .collect())
    }

    pub fn default_identity(&self) -> crate::ImResult<Option<super::IdentitySummary>> {
        Ok(self.load_registry()?.default_identity())
    }

    pub async fn default_identity_async(&self) -> crate::ImResult<Option<super::IdentitySummary>> {
        Ok(self.load_registry_async().await?.default_identity())
    }

    pub fn delete_local_identity(
        &self,
        selector: super::IdentitySelector,
    ) -> crate::ImResult<super::DeleteLocalIdentityResult> {
        self.delete_local_identity_inner(selector, false)
    }

    /// Deletes one local identity and every business projection owned by its
    /// stable identity ID. Remote account state and other local identities are
    /// not changed.
    pub fn delete_local_identity_data(
        &self,
        selector: super::IdentitySelector,
    ) -> crate::ImResult<super::DeleteLocalIdentityResult> {
        self.delete_local_identity_inner(selector, true)
    }

    fn delete_local_identity_inner(
        &self,
        selector: super::IdentitySelector,
        delete_owner_data: bool,
    ) -> crate::ImResult<super::DeleteLocalIdentityResult> {
        let mut registry = self.load_registry()?;
        let deleted_index = registry.find_index(selector)?;
        let deleted_entry = registry.entries.remove(deleted_index);
        let deleted = deleted_entry.summary.clone();
        #[cfg(feature = "sqlite")]
        if delete_owner_data {
            crate::internal::local_state::owner_scope::delete_owner_data(
                &self.core.inner().sdk_paths().local_state.sqlite_path,
                deleted.id.as_str(),
                deleted.did.as_str(),
            )?;
        }
        #[cfg(not(feature = "sqlite"))]
        let _ = delete_owner_data;
        let protocol_device_id = deleted_entry
            .device_state
            .as_ref()
            .and_then(|state| state.authorization.as_ref())
            .map(|authorization| authorization.protocol_device_id.as_str().to_owned());
        let was_default = deleted.is_default
            || registry.default_alias.as_deref() == deleted_entry.local_alias.as_deref();

        if was_default {
            registry.default_alias = registry
                .entries
                .first()
                .and_then(|entry| entry.local_alias.clone());
        }
        registry.apply_default_flags();
        let next_default = registry.default_identity();
        let local_alias =
            deleted_entry
                .local_alias
                .clone()
                .ok_or_else(|| crate::ImError::IdentityNotFound {
                    selector: deleted.id.as_str().to_owned(),
                })?;
        let outcome = crate::internal::identity_retirement::retire(
            self.core,
            crate::internal::identity_retirement::IdentityRetirementInput {
                identity_id: deleted.id.as_str().to_owned(),
                did: deleted.did.as_str().to_owned(),
                local_alias,
                identity_dir_name: deleted_entry.identity_dir_name(),
                next_default_alias: registry.default_alias.clone(),
                protocol_device_id,
            },
        )?;

        Ok(super::DeleteLocalIdentityResult {
            deleted,
            was_default,
            next_default,
            warnings: outcome.warnings,
        })
    }

    pub async fn delete_local_identity_async(
        &self,
        selector: super::IdentitySelector,
    ) -> crate::ImResult<super::DeleteLocalIdentityResult> {
        let core = self.core.clone();
        crate::internal::runtime::worker::run_blocking(move || {
            IdentityRegistry::new(&core).delete_local_identity(selector)
        })
        .await
        .map_err(|error| crate::ImError::Internal {
            message: error.to_string(),
        })?
    }

    pub async fn delete_local_identity_data_async(
        &self,
        selector: super::IdentitySelector,
    ) -> crate::ImResult<super::DeleteLocalIdentityResult> {
        let core = self.core.clone();
        crate::internal::runtime::worker::run_blocking(move || {
            IdentityRegistry::new(&core).delete_local_identity_data(selector)
        })
        .await
        .map_err(|error| crate::ImError::Internal {
            message: error.to_string(),
        })?
    }

    pub fn resolve(
        &self,
        selector: super::IdentitySelector,
    ) -> crate::ImResult<super::IdentitySummary> {
        let registry = self.load_registry()?;
        self.resolve_from_snapshot(&registry, selector)
    }

    pub async fn resolve_async(
        &self,
        selector: super::IdentitySelector,
    ) -> crate::ImResult<super::IdentitySummary> {
        let registry = self.load_registry_async().await?;
        self.resolve_from_snapshot(&registry, selector)
    }

    pub fn update_display_name_projection(
        &self,
        identity_id: crate::ids::IdentityId,
        display_name: Option<&str>,
    ) -> crate::ImResult<super::IdentitySummary> {
        let identity = self.resolve(super::IdentitySelector::Id(identity_id.clone()))?;
        crate::internal::identity_store::IdentityStore::new(
            &self.core.inner().sdk_paths().identities,
        )
        .set_display_name_projection(&identity, display_name)?;
        self.resolve(super::IdentitySelector::Id(identity_id))
    }

    pub async fn update_display_name_projection_async(
        &self,
        identity_id: crate::ids::IdentityId,
        display_name: Option<String>,
    ) -> crate::ImResult<super::IdentitySummary> {
        let core = self.core.clone();
        crate::internal::runtime::worker::run_blocking(move || {
            IdentityRegistry::new(&core)
                .update_display_name_projection(identity_id, display_name.as_deref())
        })
        .await
        .map_err(|error| crate::ImError::Internal {
            message: error.to_string(),
        })?
    }

    /// Returns a narrowly scoped, secret-free authority for adopting an
    /// ordinary pre-Recovery Registry epoch. Any Recovery marker or tuple
    /// ambiguity returns `None` rather than widening adoption.
    pub fn legacy_registry_epoch_adoption_authority(
        &self,
        selector: super::IdentitySelector,
    ) -> crate::ImResult<Option<super::LegacyRegistryEpochAdoptionAuthority>> {
        let registry = self.load_registry()?;
        let entry = registry.find_entry(selector)?;
        let Some(binding_generation) = entry.binding_generation.as_deref() else {
            return Ok(None);
        };
        let Some(state) = entry.device_state.as_ref() else {
            return Ok(None);
        };
        let (Some(authorization), Some(checkpoint)) =
            (state.authorization.as_ref(), state.checkpoint.as_ref())
        else {
            return Ok(None);
        };
        if state.mode != crate::internal::identity_device_state::IdentityDeviceMode::VNext
            || authorization.status
                != crate::internal::identity_device_state::DeviceAuthorizationStatus::Active
            || entry.user_id.trim().is_empty()
        {
            return Ok(None);
        }
        crate::internal::identity_transition_pending::legacy_registry_epoch_adoption_authority(
            &self.core.inner().sdk_paths().local_state.sqlite_path,
            crate::internal::identity_transition_pending::LegacyAuthorityInput {
                owner_identity_id: entry.summary.id.as_str(),
                account_user_id: &entry.user_id,
                current_did: entry.summary.did.as_str(),
                binding_generation,
                protocol_device_id: authorization.protocol_device_id.as_str(),
                device_auth_generation: authorization.auth_generation,
                document_version: checkpoint.document_version,
                document_hash: &checkpoint.document_hash,
                registry_version: checkpoint.registry_version,
            },
        )
    }

    pub async fn legacy_registry_epoch_adoption_authority_async(
        &self,
        selector: super::IdentitySelector,
    ) -> crate::ImResult<Option<super::LegacyRegistryEpochAdoptionAuthority>> {
        let core = self.core.clone();
        crate::internal::runtime::worker::run_blocking(move || {
            IdentityRegistry::new(&core).legacy_registry_epoch_adoption_authority(selector)
        })
        .await
        .map_err(|error| crate::ImError::Internal {
            message: error.to_string(),
        })?
    }

    pub fn device_summary(
        &self,
        selector: super::IdentitySelector,
    ) -> crate::ImResult<super::IdentityDeviceSummary> {
        let registry = self.load_registry()?;
        let entry = registry.find_entry(selector)?;
        self.device_summary_for_entry(entry)
    }

    pub async fn device_summary_async(
        &self,
        selector: super::IdentitySelector,
    ) -> crate::ImResult<super::IdentityDeviceSummary> {
        let registry = self.load_registry_async().await?;
        let entry = registry.find_entry(selector)?;
        self.device_summary_for_entry(entry)
    }

    pub async fn upgrade_legacy_identity_async(
        &self,
        selector: super::IdentitySelector,
    ) -> crate::ImResult<super::LegacyUpgradeStatus> {
        let identity = self.resolve_async(selector.clone()).await?;
        match crate::internal::identity_legacy_upgrade_runtime::upgrade(self.core, selector).await {
            Ok(status) => Ok(status),
            Err(error) => Ok(super::LegacyUpgradeStatus::RetryRequired {
                identity_id: identity.id.as_str().to_owned(),
                code: crate::internal::identity_legacy_upgrade_runtime::legacy_upgrade_error_code(
                    &error,
                )
                .to_owned(),
            }),
        }
    }

    pub fn legacy_upgrade_status(
        &self,
        selector: super::IdentitySelector,
    ) -> crate::ImResult<super::LegacyUpgradeStatus> {
        let identity = self.resolve(selector)?;
        let alias = identity
            .local_alias
            .as_deref()
            .ok_or(crate::ImError::PermissionDenied)?;
        let index = crate::internal::identity_store::IdentityStore::new(
            &self.core.inner().sdk_paths().identities,
        )
        .load_index()?;
        if index
            .credentials
            .get(alias)
            .and_then(|entry| entry.device_state.as_ref())
            .is_some_and(|state| {
                state.mode == crate::internal::identity_device_state::IdentityDeviceMode::VNext
            })
        {
            return Ok(super::LegacyUpgradeStatus::Completed);
        }
        let pending =
            crate::internal::identity_legacy_upgrade_pending::PendingLegacyUpgradeStore::from_core(
                self.core,
            )?
            .load(alias)?;
        Ok(match pending {
            Some((_, pending))
                if pending.attempt
                    == crate::internal::identity_legacy_upgrade_pending::PendingLegacyUpgradeAttempt::RetryRequired =>
            {
                super::LegacyUpgradeStatus::RetryRequired {
                    identity_id: identity.id.as_str().to_owned(),
                    code: pending
                        .failure_code
                        .unwrap_or_else(|| "legacy_upgrade_failed".to_owned()),
                }
            }
            Some(_) => super::LegacyUpgradeStatus::Running,
            None => super::LegacyUpgradeStatus::Idle,
        })
    }

    /// Inspects all local identities for ANP Identity migration eligibility
    /// without creating a store, changing the index, or deleting old keys.
    pub fn inspect_identity_custody_migration(
        &self,
    ) -> crate::ImResult<super::IdentityCustodyMigrationReport> {
        crate::internal::identity_custody_migration::inspect(self.core)
    }

    /// Async variant of [`Self::inspect_identity_custody_migration`].
    pub async fn inspect_identity_custody_migration_async(
        &self,
    ) -> crate::ImResult<super::IdentityCustodyMigrationReport> {
        let core = (*self.core).clone();
        crate::internal::runtime::worker::run_blocking(move || {
            crate::internal::identity_custody_migration::inspect(&core)
        })
        .await
        .map_err(|error| crate::ImError::Internal {
            message: error.to_string(),
        })?
    }

    /// Copies and verifies every eligible identity before one atomic workspace
    /// cutover, then performs idempotent post-marker cleanup.
    pub fn migrate_identity_custody(
        &self,
    ) -> crate::ImResult<super::IdentityCustodyMigrationReport> {
        crate::internal::identity_custody_migration::migrate(self.core)
    }

    /// Async variant of [`Self::migrate_identity_custody`].
    pub async fn migrate_identity_custody_async(
        &self,
    ) -> crate::ImResult<super::IdentityCustodyMigrationReport> {
        let core = (*self.core).clone();
        crate::internal::runtime::worker::run_blocking(move || {
            crate::internal::identity_custody_migration::migrate(&core)
        })
        .await
        .map_err(|error| crate::ImError::Internal {
            message: error.to_string(),
        })?
    }

    pub fn vault_status(
        &self,
        selector: super::IdentitySelector,
    ) -> crate::ImResult<super::IdentityVaultStatus> {
        let registry = self.load_registry()?;
        if registry.entries.is_empty() {
            let summary = self.resolve_from_snapshot(&registry, selector)?;
            return Ok(self.identity_vault_status(&summary, None));
        }
        let entry = registry.find_entry(selector)?;
        Ok(self.identity_vault_status(&entry.summary, Some(entry)))
    }

    pub async fn vault_status_async(
        &self,
        selector: super::IdentitySelector,
    ) -> crate::ImResult<super::IdentityVaultStatus> {
        let registry = self.load_registry_async().await?;
        if registry.entries.is_empty() {
            let summary = self.resolve_from_snapshot(&registry, selector)?;
            return Ok(self.identity_vault_status(&summary, None));
        }
        let entry = registry.find_entry(selector)?;
        Ok(self.identity_vault_status(&entry.summary, Some(entry)))
    }

    pub fn migrate_identity_vault(
        &self,
        selector: super::IdentitySelector,
    ) -> crate::ImResult<super::IdentityVaultMigrationReport> {
        let context = self.core.inner().identity_vault().cloned().ok_or_else(|| {
            crate::ImError::LocalStateUnavailable {
                detail: "identity vault migration requires identity secret vault open options"
                    .to_owned(),
            }
        })?;
        let registry = self.load_registry()?;
        let entry = registry.find_entry(selector)?;
        let local_alias =
            entry
                .local_alias
                .clone()
                .ok_or_else(|| crate::ImError::IdentityNotFound {
                    selector: entry.summary.id.as_str().to_owned(),
                })?;
        crate::internal::identity_store::IdentityStore::new(
            &self.core.inner().sdk_paths().identities,
        )
        .migrate_identity_to_vault(
            &local_alias,
            context.workspace_id(),
            context.vault_context_device_id().as_str(),
            context.vault().as_ref(),
        )?;
        let status = self.vault_status(super::IdentitySelector::LocalAlias(local_alias))?;
        self.verify_identity_vault_status(status, true)
            .map(|verification| super::IdentityVaultMigrationReport {
                plaintext_compat_retained: verification
                    .status
                    .plaintext_compat_retained
                    .unwrap_or(false),
                warnings: verification.warnings,
                identity: verification.identity,
                status: verification.status,
                migrated: true,
                verified: verification.verified,
            })
    }

    pub async fn migrate_identity_vault_async(
        &self,
        selector: super::IdentitySelector,
    ) -> crate::ImResult<super::IdentityVaultMigrationReport> {
        let context = self.core.inner().identity_vault().cloned().ok_or_else(|| {
            crate::ImError::LocalStateUnavailable {
                detail: "identity vault migration requires identity secret vault open options"
                    .to_owned(),
            }
        })?;
        let registry = self.load_registry_async().await?;
        let entry = registry.find_entry(selector)?;
        let local_alias =
            entry
                .local_alias
                .clone()
                .ok_or_else(|| crate::ImError::IdentityNotFound {
                    selector: entry.summary.id.as_str().to_owned(),
                })?;
        let paths = self.core.inner().sdk_paths().identities.clone();
        let workspace_id = context.workspace_id().to_owned();
        let device_id = context.vault_context_device_id().as_str().to_owned();
        let vault = context.vault();
        let local_alias_for_migration = local_alias.clone();
        crate::internal::runtime::worker::run_blocking(move || {
            crate::internal::identity_store::IdentityStore::new(&paths).migrate_identity_to_vault(
                &local_alias_for_migration,
                &workspace_id,
                &device_id,
                vault.as_ref(),
            )
        })
        .await
        .map_err(|err| crate::ImError::Internal {
            message: err.to_string(),
        })??;
        let status = self
            .vault_status_async(super::IdentitySelector::LocalAlias(local_alias))
            .await?;
        self.verify_identity_vault_status(status, true)
            .map(|verification| super::IdentityVaultMigrationReport {
                plaintext_compat_retained: verification
                    .status
                    .plaintext_compat_retained
                    .unwrap_or(false),
                warnings: verification.warnings,
                identity: verification.identity,
                status: verification.status,
                migrated: true,
                verified: verification.verified,
            })
    }

    pub fn verify_identity_vault(
        &self,
        selector: super::IdentitySelector,
    ) -> crate::ImResult<super::IdentityVaultVerificationReport> {
        let status = self.vault_status(selector)?;
        self.verify_identity_vault_status(status, true)
    }

    pub async fn verify_identity_vault_async(
        &self,
        selector: super::IdentitySelector,
    ) -> crate::ImResult<super::IdentityVaultVerificationReport> {
        let status = self.vault_status_async(selector).await?;
        self.verify_identity_vault_status(status, true)
    }

    pub fn load_daemon_subkey_package(
        &self,
        selector: super::IdentitySelector,
    ) -> crate::ImResult<super::DaemonSubkeyPrivatePackage> {
        let registry = self.load_registry()?;
        let entry = registry.find_entry(selector)?;
        let dir_name =
            entry
                .identity_dir_name()
                .ok_or_else(|| crate::ImError::IdentityNotFound {
                    selector: entry.summary.id.as_str().to_string(),
                })?;
        crate::internal::identity_store::IdentityStore::new(
            &self.core.inner().sdk_paths().identities,
        )
        .load_daemon_subkey_package_vault_aware(
            &dir_name,
            &entry.summary.did,
            entry.vault_migration.as_ref(),
            self.core.inner().identity_vault(),
            self.core.inner().identity_secret_storage_policy(),
        )
    }

    pub async fn load_daemon_subkey_package_async(
        &self,
        selector: super::IdentitySelector,
    ) -> crate::ImResult<super::DaemonSubkeyPrivatePackage> {
        let registry = self.load_registry_async().await?;
        let entry = registry.find_entry(selector)?;
        let dir_name =
            entry
                .identity_dir_name()
                .ok_or_else(|| crate::ImError::IdentityNotFound {
                    selector: entry.summary.id.as_str().to_string(),
                })?;
        let paths = self.core.inner().sdk_paths().identities.clone();
        let did = entry.summary.did.clone();
        let metadata = entry.vault_migration.clone();
        let context = self.core.inner().identity_vault().cloned();
        let policy = self.core.inner().identity_secret_storage_policy();
        crate::internal::runtime::worker::run_blocking(move || {
            crate::internal::identity_store::IdentityStore::new(&paths)
                .load_daemon_subkey_package_vault_aware(
                    &dir_name,
                    &did,
                    metadata.as_ref(),
                    context.as_ref(),
                    policy,
                )
        })
        .await
        .map_err(|err| crate::ImError::Internal {
            message: err.to_string(),
        })?
    }

    pub fn ensure_daemon_subkey_package(
        &self,
        selector: super::IdentitySelector,
    ) -> crate::ImResult<super::DaemonSubkeyPrivatePackage> {
        let registry = self.load_registry()?;
        let entry = registry.find_entry(selector)?;
        let prepared = self.prepare_daemon_subkey_ensure(entry)?;
        match prepared {
            EnsureDaemonSubkeyPrepared::Ready { package } => Ok(package),
            EnsureDaemonSubkeyPrepared::UpdateRequired {
                dir_name,
                identity_id,
                did_document,
                package,
                selector,
            } => {
                let client = self.core.client(selector)?;
                let call =
                    crate::internal::identity_wire::update_document::build_update_document_rpc_call(
                        crate::internal::identity_wire::UpdateDocumentRpcParams {
                            did_document: did_document.clone(),
                            is_public: None,
                            is_agent: None,
                            role: None,
                            endpoint_url: None,
                        },
                    );
                use crate::internal::transport::AuthenticatedRpcTransport;
                let mut transport = crate::internal::transport::CoreHttpTransport::new(&client);
                transport.authenticated_rpc(call.endpoint, call.method, call.params)?;
                crate::internal::identity_store::IdentityStore::new(
                    &self.core.inner().sdk_paths().identities,
                )
                .save_did_document(&dir_name, &did_document)?;
                crate::internal::identity_store::IdentityStore::new(
                    &self.core.inner().sdk_paths().identities,
                )
                .save_daemon_subkey_package_with_secret_storage(
                    &dir_name,
                    &identity_id,
                    &package,
                    crate::internal::identity_store::SaveIdentitySecretStorage::from_core(
                        self.core,
                    )?,
                )?;
                Ok(package)
            }
        }
    }

    pub async fn ensure_daemon_subkey_package_async(
        &self,
        selector: super::IdentitySelector,
    ) -> crate::ImResult<super::DaemonSubkeyPrivatePackage> {
        let registry = self.load_registry_async().await?;
        let entry = registry.find_entry(selector)?;
        let core = (*self.core).clone();
        let entry = entry.clone();
        let prepared = crate::internal::runtime::worker::run_blocking(move || {
            IdentityRegistry::new(&core).prepare_daemon_subkey_ensure(&entry)
        })
        .await
        .map_err(|err| crate::ImError::Internal {
            message: err.to_string(),
        })??;
        match prepared {
            EnsureDaemonSubkeyPrepared::Ready { package } => Ok(package),
            EnsureDaemonSubkeyPrepared::UpdateRequired {
                dir_name,
                identity_id,
                did_document,
                package,
                selector,
            } => {
                let client = self.core.client_async(selector).await?;
                let call =
                    crate::internal::identity_wire::update_document::build_update_document_rpc_call(
                        crate::internal::identity_wire::UpdateDocumentRpcParams {
                            did_document: did_document.clone(),
                            is_public: None,
                            is_agent: None,
                            role: None,
                            endpoint_url: None,
                        },
                    );
                use crate::internal::transport::AsyncAuthenticatedRpcTransport;
                let mut transport = crate::internal::transport::CoreHttpTransport::new(&client);
                transport
                    .authenticated_rpc(call.endpoint, call.method, call.params)
                    .await?;
                let paths = self.core.inner().sdk_paths().identities.clone();
                let package_to_save = package.clone();
                let secret_storage =
                    crate::internal::identity_store::SaveIdentitySecretStorage::from_core(
                        self.core,
                    )?;
                crate::internal::runtime::worker::run_blocking(move || {
                    let store = crate::internal::identity_store::IdentityStore::new(&paths);
                    store.save_did_document(&dir_name, &did_document)?;
                    store.save_daemon_subkey_package_with_secret_storage(
                        &dir_name,
                        &identity_id,
                        &package_to_save,
                        secret_storage,
                    )?;
                    Ok::<(), crate::ImError>(())
                })
                .await
                .map_err(|err| crate::ImError::Internal {
                    message: err.to_string(),
                })??;
                Ok(package)
            }
        }
    }

    pub fn revoke_daemon_subkey_authorization(
        &self,
        selector: super::IdentitySelector,
    ) -> crate::ImResult<super::DaemonSubkeyAuthorizationRevokeResult> {
        let registry = self.load_registry()?;
        let entry = registry.find_entry(selector)?;
        let prepared = self.prepare_daemon_subkey_revoke(entry)?;
        match prepared {
            RevokeDaemonSubkeyPrepared::AlreadyRevoked {
                did,
                verification_method,
            } => Ok(super::DaemonSubkeyAuthorizationRevokeResult {
                user_did: did,
                verification_method,
                updated: false,
            }),
            RevokeDaemonSubkeyPrepared::UpdateRequired {
                dir_name,
                did,
                verification_method,
                did_document,
                selector,
            } => {
                let client = self.core.client(selector)?;
                let call =
                    crate::internal::identity_wire::update_document::build_update_document_rpc_call(
                        crate::internal::identity_wire::UpdateDocumentRpcParams {
                            did_document: did_document.clone(),
                            is_public: None,
                            is_agent: None,
                            role: None,
                            endpoint_url: None,
                        },
                    );
                use crate::internal::transport::AuthenticatedRpcTransport;
                let mut transport = crate::internal::transport::CoreHttpTransport::new(&client);
                transport.authenticated_rpc(call.endpoint, call.method, call.params)?;
                crate::internal::identity_store::IdentityStore::new(
                    &self.core.inner().sdk_paths().identities,
                )
                .save_did_document(&dir_name, &did_document)?;
                Ok(super::DaemonSubkeyAuthorizationRevokeResult {
                    user_did: did,
                    verification_method,
                    updated: true,
                })
            }
        }
    }

    pub async fn revoke_daemon_subkey_authorization_async(
        &self,
        selector: super::IdentitySelector,
    ) -> crate::ImResult<super::DaemonSubkeyAuthorizationRevokeResult> {
        let registry = self.load_registry_async().await?;
        let entry = registry.find_entry(selector)?;
        let core = (*self.core).clone();
        let entry = entry.clone();
        let prepared_entry = entry.clone();
        let prepared = crate::internal::runtime::worker::run_blocking(move || {
            IdentityRegistry::new(&core).prepare_daemon_subkey_revoke(&prepared_entry)
        })
        .await
        .map_err(|err| crate::ImError::Internal {
            message: err.to_string(),
        })??;
        match prepared {
            RevokeDaemonSubkeyPrepared::AlreadyRevoked {
                did,
                verification_method,
            } => Ok(super::DaemonSubkeyAuthorizationRevokeResult {
                user_did: did,
                verification_method,
                updated: false,
            }),
            RevokeDaemonSubkeyPrepared::UpdateRequired {
                dir_name,
                did,
                verification_method,
                did_document,
                selector,
            } => {
                if entry.device_state.as_ref().is_some_and(|state| {
                    state.mode == crate::internal::identity_device_state::IdentityDeviceMode::VNext
                }) {
                    let mut state = entry
                        .device_state
                        .clone()
                        .ok_or(crate::ImError::PermissionDenied)?;
                    let expected_checkpoint = state
                        .checkpoint
                        .clone()
                        .ok_or(crate::ImError::PermissionDenied)?;
                    let (client, authorizing_device_id, authorizing_signing_key_id) =
                        crate::internal::identity_device_join::ready_admin_context(
                            self.core, &selector, None,
                        )?;
                    let next_document_hash =
                        crate::internal::identity_wire::document::document_hash(&did_document)?;
                    let expected_result_checkpoint =
                        crate::internal::identity_device_state::IdentityInternalCheckpoint {
                            document_version: expected_checkpoint
                                .document_version
                                .checked_add(1)
                                .ok_or(crate::ImError::PermissionDenied)?,
                            document_hash: next_document_hash.clone(),
                            registry_version: expected_checkpoint.registry_version,
                        };
                    let operation_id = format!(
                        "daemon-subkey-revoke-{}",
                        next_document_hash
                            .strip_prefix("sha256:")
                            .ok_or(crate::ImError::PermissionDenied)?
                    );
                    let prepared =
                        crate::internal::identity_wire::device_document_update::prepare_update(
                            operation_id,
                            expected_checkpoint,
                            did_document.clone(),
                            authorizing_device_id,
                            &authorizing_signing_key_id,
                            &|kid, message| {
                                client
                                    .runtime()
                                    .key_provider
                                    .sign_device_assertion(kid, message)
                            },
                            time::OffsetDateTime::now_utc(),
                        )?;
                    let call =
                        crate::internal::identity_wire::device_document_update::build_update_call(
                            &prepared,
                        )?;
                    use crate::internal::transport::AsyncAuthenticatedRpcTransport;
                    let mut transport = crate::internal::transport::CoreHttpTransport::new(&client);
                    let raw = transport
                        .authenticated_rpc(call.endpoint, call.method, call.params)
                        .await?;
                    let checkpoint =
                        crate::internal::identity_wire::device_document_update::parse_update_result(
                            raw,
                            &did,
                            &expected_result_checkpoint,
                        )?;
                    state.checkpoint = Some(checkpoint);
                    state.validate_for_did(&did)?;
                    let local_alias = client
                        .current_identity()
                        .local_alias
                        .clone()
                        .ok_or(crate::ImError::PermissionDenied)?;
                    let paths = self.core.inner().sdk_paths().identities.clone();
                    crate::internal::runtime::worker::run_blocking(move || {
                        let store = crate::internal::identity_store::IdentityStore::new(&paths);
                        store.save_device_state(&local_alias, state)?;
                        store.save_did_document(&dir_name, &did_document)
                    })
                    .await
                    .map_err(|err| crate::ImError::Internal {
                        message: err.to_string(),
                    })??;
                    return Ok(super::DaemonSubkeyAuthorizationRevokeResult {
                        user_did: did,
                        verification_method,
                        updated: true,
                    });
                }
                let client = self.core.client_async(selector).await?;
                let call =
                    crate::internal::identity_wire::update_document::build_update_document_rpc_call(
                        crate::internal::identity_wire::UpdateDocumentRpcParams {
                            did_document: did_document.clone(),
                            is_public: None,
                            is_agent: None,
                            role: None,
                            endpoint_url: None,
                        },
                    );
                use crate::internal::transport::AsyncAuthenticatedRpcTransport;
                let mut transport = crate::internal::transport::CoreHttpTransport::new(&client);
                transport
                    .authenticated_rpc(call.endpoint, call.method, call.params)
                    .await?;
                let paths = self.core.inner().sdk_paths().identities.clone();
                crate::internal::runtime::worker::run_blocking(move || {
                    crate::internal::identity_store::IdentityStore::new(&paths)
                        .save_did_document(&dir_name, &did_document)
                })
                .await
                .map_err(|err| crate::ImError::Internal {
                    message: err.to_string(),
                })??;
                Ok(super::DaemonSubkeyAuthorizationRevokeResult {
                    user_did: did,
                    verification_method,
                    updated: true,
                })
            }
        }
    }

    fn prepare_daemon_subkey_ensure(
        &self,
        entry: &RegistryEntry,
    ) -> crate::ImResult<EnsureDaemonSubkeyPrepared> {
        let dir_name =
            entry
                .identity_dir_name()
                .ok_or_else(|| crate::ImError::IdentityNotFound {
                    selector: entry.summary.id.as_str().to_string(),
                })?;
        let store = crate::internal::identity_store::IdentityStore::new(
            &self.core.inner().sdk_paths().identities,
        );
        let did = entry.summary.did.clone();
        let mut did_document = store.load_did_document(&dir_name)?;
        let document_id = did_document
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if document_id != did.as_str() {
            return Err(crate::ImError::IdentityNotReady {
                identity: did.as_str().to_string(),
                missing: vec!["did_document_identity_mismatch".to_string()],
            });
        }
        if let Some(package) = match store.load_daemon_subkey_package_vault_aware(
            &dir_name,
            &did,
            entry.vault_migration.as_ref(),
            self.core.inner().identity_vault(),
            self.core.inner().identity_secret_storage_policy(),
        ) {
            Ok(package) => Some(package),
            Err(crate::ImError::IdentityNotFound { .. })
                if !matches!(
                    self.core.inner().identity_secret_storage_policy(),
                    crate::core::IdentitySecretStoragePolicy::VaultRequired
                ) =>
            {
                store.load_daemon_subkey_package_or_legacy(&dir_name, &did, &did_document)?
            }
            Err(crate::ImError::IdentityNotFound { .. }) => None,
            Err(err) => return Err(err),
        } {
            if package.user_did != did {
                return Err(crate::ImError::IdentityNotReady {
                    identity: did.as_str().to_string(),
                    missing: vec!["daemon_subkey_did_mismatch".to_string()],
                });
            }
            crate::internal::identity_daemon_subkey::validate_package_against_did_document(
                &package,
                &did_document,
            )?;
            return Ok(EnsureDaemonSubkeyPrepared::Ready { package });
        }
        if crate::internal::identity_daemon_subkey::did_document_references_daemon_subkey(
            &did_document,
            &did,
        ) {
            return Err(crate::ImError::IdentityNotReady {
                identity: did.as_str().to_string(),
                missing: vec!["daemon_subkey_private_missing".to_string()],
            });
        }
        let signer = self.key_provider_for_entry(
            self.core
                .inner()
                .sdk_paths()
                .identities
                .identity_root_dir
                .join(&dir_name),
            Some(entry),
            &entry.summary,
        )?;
        let subkey = crate::internal::identity_daemon_subkey::generate_for_did(&did);
        crate::internal::identity_daemon_subkey::apply_to_did_document(
            &mut did_document,
            &did,
            &subkey,
        )?;
        crate::internal::identity_daemon_subkey::resign_did_document_with_signer(
            &mut did_document,
            &did,
            signer.as_ref(),
        )?;
        let package = crate::internal::identity_daemon_subkey::package_from_parts(
            did.clone(),
            subkey.verification_method,
            subkey.public_key_multibase,
            subkey.private_key_pem,
        );
        Ok(EnsureDaemonSubkeyPrepared::UpdateRequired {
            dir_name,
            identity_id: entry.summary.id.as_str().to_string(),
            did_document,
            package,
            selector: super::IdentitySelector::Did(did),
        })
    }

    fn prepare_daemon_subkey_revoke(
        &self,
        entry: &RegistryEntry,
    ) -> crate::ImResult<RevokeDaemonSubkeyPrepared> {
        let dir_name =
            entry
                .identity_dir_name()
                .ok_or_else(|| crate::ImError::IdentityNotFound {
                    selector: entry.summary.id.as_str().to_string(),
                })?;
        let store = crate::internal::identity_store::IdentityStore::new(
            &self.core.inner().sdk_paths().identities,
        );
        let did = entry.summary.did.clone();
        let verification_method =
            crate::internal::identity_daemon_subkey::expected_verification_method(&did);
        let mut did_document = store.load_did_document(&dir_name)?;
        let document_id = did_document
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if document_id != did.as_str() {
            return Err(crate::ImError::IdentityNotReady {
                identity: did.as_str().to_string(),
                missing: vec!["did_document_identity_mismatch".to_string()],
            });
        }
        if !crate::internal::identity_daemon_subkey::remove_from_did_document(
            &mut did_document,
            &did,
        )? {
            return Ok(RevokeDaemonSubkeyPrepared::AlreadyRevoked {
                did,
                verification_method,
            });
        }
        let signer = self.key_provider_for_entry(
            self.core
                .inner()
                .sdk_paths()
                .identities
                .identity_root_dir
                .join(&dir_name),
            Some(entry),
            &entry.summary,
        )?;
        crate::internal::identity_daemon_subkey::resign_did_document_with_signer(
            &mut did_document,
            &did,
            signer.as_ref(),
        )?;
        Ok(RevokeDaemonSubkeyPrepared::UpdateRequired {
            dir_name,
            did: did.clone(),
            verification_method,
            did_document,
            selector: super::IdentitySelector::Did(did),
        })
    }

    fn resolve_from_snapshot(
        &self,
        registry: &RegistrySnapshot,
        selector: super::IdentitySelector,
    ) -> crate::ImResult<super::IdentitySummary> {
        match selector {
            super::IdentitySelector::Default => registry
                .default_identity()
                .ok_or(crate::ImError::DefaultIdentityMissing),
            super::IdentitySelector::LocalAlias(alias) => {
                let alias = alias.trim();
                if alias.is_empty() {
                    return Err(crate::ImError::invalid_input(
                        Some("identity".to_string()),
                        "local alias must not be empty",
                    ));
                }
                registry
                    .find(|entry| entry.local_alias.as_deref() == Some(alias))
                    .map(|entry| entry.summary.clone())
                    .map_or_else(
                        || {
                            if registry.entries.is_empty() {
                                self.summary_for_local_alias(alias.to_string())
                            } else {
                                Err(crate::ImError::IdentityNotFound {
                                    selector: alias.to_string(),
                                })
                            }
                        },
                        Ok,
                    )
            }
            super::IdentitySelector::Did(did) => registry
                .find(|entry| entry.summary.did == did)
                .map(|entry| entry.summary.clone())
                .map_or_else(
                    || {
                        if registry.entries.is_empty() {
                            self.summary_for_did(did)
                        } else {
                            Err(crate::ImError::IdentityNotFound {
                                selector: did.as_str().to_string(),
                            })
                        }
                    },
                    Ok,
                ),
            super::IdentitySelector::Id(id) => registry
                .find(|entry| entry.summary.id == id)
                .map(|entry| entry.summary.clone())
                .ok_or_else(|| crate::ImError::IdentityNotFound {
                    selector: id.as_str().to_string(),
                }),
            super::IdentitySelector::Handle(handle) => registry
                .find(|entry| entry.summary.handle.as_ref() == Some(&handle))
                .map(|entry| entry.summary.clone())
                .ok_or_else(|| crate::ImError::IdentityNotFound {
                    selector: handle.as_str().to_string(),
                }),
        }
    }

    fn summary_for_local_alias(&self, alias: String) -> crate::ImResult<super::IdentitySummary> {
        let alias = alias.trim();
        if alias.is_empty() {
            return Err(crate::ImError::invalid_input(
                Some("identity".to_string()),
                "local alias must not be empty",
            ));
        }
        let did = crate::ids::Did::parse(format!(
            "did:awiki:{}",
            alias
                .chars()
                .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '-' })
                .collect::<String>()
        ))?;
        Ok(super::IdentitySummary {
            id: crate::ids::IdentityId::parse(alias)?,
            did,
            handle: None,
            display_name: None,
            local_alias: Some(alias.to_string()),
            device_id: None,
            is_default: false,
            readiness: super::IdentityReadiness {
                ready_for_auth: false,
                ready_for_messaging: false,
                missing: vec![
                    super::IdentityMissingItem::DidDocument,
                    super::IdentityMissingItem::PrivateKey,
                    super::IdentityMissingItem::AuthState,
                ],
            },
        })
    }

    fn summary_for_did(&self, did: crate::ids::Did) -> crate::ImResult<super::IdentitySummary> {
        let id = did.as_str().replace(':', "-");
        Ok(super::IdentitySummary {
            id: crate::ids::IdentityId::parse(id)?,
            did,
            handle: None,
            display_name: None,
            local_alias: None,
            device_id: None,
            is_default: false,
            readiness: super::IdentityReadiness {
                ready_for_auth: false,
                ready_for_messaging: false,
                missing: vec![
                    super::IdentityMissingItem::DidDocument,
                    super::IdentityMissingItem::PrivateKey,
                    super::IdentityMissingItem::AuthState,
                ],
            },
        })
    }

    pub fn register_handle(
        &self,
        request: super::RegisterHandleRequest,
    ) -> crate::ImResult<super::HandleRegistrationResult> {
        crate::internal::identity_registration_runtime::IdentityRegistrationRuntime::new(
            self.core,
            crate::internal::transport::CorePlainTransport::new(self.core),
        )
        .register_handle(request)
        .map(|result| result.sdk_result)
    }

    pub async fn register_handle_async(
        &self,
        request: super::RegisterHandleRequest,
    ) -> crate::ImResult<super::HandleRegistrationResult> {
        crate::internal::identity_registration_runtime::IdentityRegistrationRuntime::new(
            self.core,
            crate::internal::transport::CorePlainTransport::new(self.core),
        )
        .register_handle_async(request)
        .await
        .map(|result| result.sdk_result)
    }

    #[cfg(feature = "mcp-trusted-registration")]
    pub async fn register_handle_with_service_bearer_async(
        &self,
        request: super::RegisterHandleRequest,
        bearer_token: impl Into<String>,
    ) -> crate::ImResult<super::HandleRegistrationResult> {
        crate::internal::identity_registration_runtime::IdentityRegistrationRuntime::new(
            self.core,
            crate::internal::transport::CorePlainTransport::new_with_register_bearer_token(
                self.core,
                bearer_token,
            ),
        )
        .register_handle_async(request)
        .await
        .map(|result| result.sdk_result)
    }
}

impl IdentityRegistry<'_> {
    pub fn plan_default_identity_change(
        &self,
        selector: super::IdentitySelector,
    ) -> crate::ImResult<super::DefaultIdentityChange> {
        let previous = self.default_identity()?;
        let next = self.resolve(selector)?;
        Ok(super::DefaultIdentityChange {
            previous,
            next,
            requires_default_identity_write: true,
            warnings: Vec::new(),
        })
    }

    pub async fn plan_default_identity_change_async(
        &self,
        selector: super::IdentitySelector,
    ) -> crate::ImResult<super::DefaultIdentityChange> {
        let previous = self.default_identity_async().await?;
        let next = self.resolve_async(selector).await?;
        Ok(super::DefaultIdentityChange {
            previous,
            next,
            requires_default_identity_write: true,
            warnings: Vec::new(),
        })
    }

    pub(crate) fn load_runtime(
        &self,
        selector: super::IdentitySelector,
    ) -> crate::ImResult<crate::internal::identity_runtime::ClientIdentityRuntime> {
        let registry = self.load_registry()?;
        let (summary, entry) = if registry.entries.is_empty() {
            (self.resolve_from_snapshot(&registry, selector)?, None)
        } else {
            let entry = registry.find_entry(selector)?;
            (entry.summary.clone(), Some(entry))
        };
        let identity_root = &self.core.inner().sdk_paths().identities.identity_root_dir;
        let identity_dir_name = entry
            .and_then(|entry| entry.dir_name.as_deref())
            .or(summary.local_alias.as_deref())
            .unwrap_or_else(|| summary.id.as_str());
        let identity_dir = identity_root.join(identity_dir_name);
        let key_provider = self.key_provider_for_entry(identity_dir.clone(), entry, &summary)?;
        let sync_account = sync_account_seed(entry)?;
        Ok(crate::internal::identity_runtime::ClientIdentityRuntime {
            summary: summary.clone(),
            did_document_path: first_existing_path(
                &identity_dir,
                &["did.json", "did_document.json"],
            ),
            private_key_path: first_existing_path(
                &identity_dir,
                &["private.key", "key-1-private.pem"],
            ),
            e2ee_agreement_private_key_path: first_existing_path(
                &identity_dir,
                &["e2ee-agreement-private.pem", "key-3-private.pem"],
            ),
            auth_state_path: identity_dir.join("auth.json"),
            key_provider,
            owner: crate::internal::identity_runtime::LocalOwnerContext {
                identity_id: summary.id,
                current_did: summary.did,
                sync_account,
            },
        })
    }

    pub(crate) async fn load_runtime_async(
        &self,
        selector: super::IdentitySelector,
    ) -> crate::ImResult<crate::internal::identity_runtime::ClientIdentityRuntime> {
        let registry = self.load_registry_async().await?;
        let (summary, entry) = if registry.entries.is_empty() {
            (self.resolve_from_snapshot(&registry, selector)?, None)
        } else {
            let entry = registry.find_entry(selector)?;
            (entry.summary.clone(), Some(entry))
        };
        let identity_root = &self.core.inner().sdk_paths().identities.identity_root_dir;
        let identity_dir_name = entry
            .and_then(|entry| entry.dir_name.as_deref())
            .or(summary.local_alias.as_deref())
            .unwrap_or_else(|| summary.id.as_str());
        let identity_dir = identity_root.join(identity_dir_name);
        let key_provider = self.key_provider_for_entry(identity_dir.clone(), entry, &summary)?;
        let sync_account = sync_account_seed(entry)?;
        Ok(crate::internal::identity_runtime::ClientIdentityRuntime {
            summary: summary.clone(),
            did_document_path: first_existing_path_async(
                &identity_dir,
                &["did.json", "did_document.json"],
            )
            .await,
            private_key_path: first_existing_path_async(
                &identity_dir,
                &["private.key", "key-1-private.pem"],
            )
            .await,
            e2ee_agreement_private_key_path: first_existing_path_async(
                &identity_dir,
                &["e2ee-agreement-private.pem", "key-3-private.pem"],
            )
            .await,
            auth_state_path: identity_dir.join("auth.json"),
            key_provider,
            owner: crate::internal::identity_runtime::LocalOwnerContext {
                identity_id: summary.id,
                current_did: summary.did,
                sync_account,
            },
        })
    }

    fn load_registry(&self) -> crate::ImResult<RegistrySnapshot> {
        let paths = &self.core.inner().sdk_paths().identities;
        let mut snapshot = match fs::read(&paths.registry_path) {
            Ok(raw) => parse_registry(&raw)?,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => RegistrySnapshot {
                default_alias: default_alias_from_file(paths.default_identity_path.as_deref())?,
                entries: Vec::new(),
            },
            Err(err) => {
                return Err(crate::ImError::CredentialFileUnreadable {
                    path_kind: "identity_registry".to_string(),
                    detail: err.to_string(),
                });
            }
        };
        if let Some(default_alias) =
            default_alias_from_file(paths.default_identity_path.as_deref())?
        {
            snapshot.default_alias = Some(default_alias);
        }
        snapshot.apply_default_flags();
        snapshot.validate()?;
        Ok(snapshot)
    }

    async fn load_registry_async(&self) -> crate::ImResult<RegistrySnapshot> {
        let paths = &self.core.inner().sdk_paths().identities;
        let mut snapshot = match tokio::fs::read(&paths.registry_path).await {
            Ok(raw) => parse_registry(&raw)?,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => RegistrySnapshot {
                default_alias: default_alias_from_file_async(paths.default_identity_path.clone())
                    .await?,
                entries: Vec::new(),
            },
            Err(err) => {
                return Err(crate::ImError::CredentialFileUnreadable {
                    path_kind: "identity_registry".to_string(),
                    detail: err.to_string(),
                });
            }
        };
        if let Some(default_alias) =
            default_alias_from_file_async(paths.default_identity_path.clone()).await?
        {
            snapshot.default_alias = Some(default_alias);
        }
        snapshot.apply_default_flags();
        snapshot.validate()?;
        Ok(snapshot)
    }

    fn key_provider_for_entry(
        &self,
        identity_dir: PathBuf,
        entry: Option<&RegistryEntry>,
        summary: &super::IdentitySummary,
    ) -> crate::ImResult<Arc<dyn crate::internal::key_provider::IdentitySigner>> {
        if let Some(entry) = entry {
            let has_anp_binding = entry.identity_custody_backend.is_some()
                || entry.anp_identity_store_id.is_some()
                || entry.anp_identity_id.is_some();
            if has_anp_binding {
                if entry.identity_custody_backend.as_deref() != Some("anp_identity") {
                    return Err(crate::ImError::IdentityNotReady {
                        identity: summary.did.as_str().to_owned(),
                        missing: vec!["anp_identity_backend_marker".to_owned()],
                    });
                }
                let expected_store_id =
                    entry.anp_identity_store_id.as_deref().ok_or_else(|| {
                        crate::ImError::IdentityNotReady {
                            identity: summary.did.as_str().to_owned(),
                            missing: vec!["anp_identity_store_id".to_owned()],
                        }
                    })?;
                let expected_identity_id = entry.anp_identity_id.as_deref().ok_or_else(|| {
                    crate::ImError::IdentityNotReady {
                        identity: summary.did.as_str().to_owned(),
                        missing: vec!["anp_identity_id".to_owned()],
                    }
                })?;
                let store = crate::internal::identity_custody::open_controller_store(self.core)?;
                if store.manifest().store_id != expected_store_id {
                    return Err(crate::ImError::PermissionDenied);
                }
                let identity = store
                    .open_identity(summary.did.as_str())
                    .map_err(crate::internal::identity_custody::map_error)?;
                if identity.identity_id() != expected_identity_id {
                    return Err(crate::ImError::PermissionDenied);
                }
                let provider = if let Some(auth_ref) = entry.anp_identity_auth_ref.clone() {
                    let context = self.core.inner().identity_vault().ok_or_else(|| {
                        crate::ImError::IdentityNotReady {
                            identity: summary.did.as_str().to_owned(),
                            missing: vec!["identity_secret_vault".to_owned()],
                        }
                    })?;
                    crate::internal::key_provider::AnpIdentitySigner::new_vault(
                        identity,
                        context.vault(),
                        auth_ref,
                    )?
                } else {
                    crate::internal::key_provider::AnpIdentitySigner::new_file(
                        identity,
                        identity_dir.join("auth.json"),
                    )
                };
                return Ok(Arc::new(provider));
            }
        }
        let policy = self.core.inner().identity_secret_storage_policy();
        let metadata = entry.and_then(|entry| entry.vault_migration.as_ref());
        let is_vnext = entry
            .and_then(|entry| entry.device_state.as_ref())
            .is_some_and(|state| {
                state.mode == crate::internal::identity_device_state::IdentityDeviceMode::VNext
            });
        if is_vnext {
            let metadata = metadata.ok_or_else(|| crate::ImError::IdentityNotReady {
                identity: summary.did.as_str().to_owned(),
                missing: vec!["identity_vault_metadata".to_owned()],
            })?;
            if !vault_metadata_is_verified(metadata) {
                return Err(crate::ImError::IdentityNotReady {
                    identity: summary.did.as_str().to_owned(),
                    missing: vec!["identity_vault_metadata_verified".to_owned()],
                });
            }
            let context = self.core.inner().identity_vault().ok_or_else(|| {
                crate::ImError::IdentityNotReady {
                    identity: summary.did.as_str().to_owned(),
                    missing: vec!["identity_secret_vault".to_owned()],
                }
            })?;
            if !vault_context_matches_metadata(context, metadata) {
                return Err(crate::ImError::IdentityNotReady {
                    identity: summary.did.as_str().to_owned(),
                    missing: vec!["identity_vault_context_mismatch".to_owned()],
                });
            }
            let refs = metadata.vnext_key_material_refs().ok_or_else(|| {
                crate::ImError::IdentityNotReady {
                    identity: summary.did.as_str().to_owned(),
                    missing: vec!["vnext_vault_key_refs".to_owned()],
                }
            })?;
            return Ok(Arc::new(
                crate::internal::key_provider::vault::VaultBackedIdentitySigner::new_vnext(
                    identity_dir,
                    context.vault(),
                    refs,
                ),
            ));
        }
        if metadata.is_some_and(|metadata| metadata.vnext_key_material_refs().is_some()) {
            return Err(crate::ImError::IdentityNotReady {
                identity: summary.did.as_str().to_owned(),
                missing: vec!["vnext_device_state".to_owned()],
            });
        }
        if let Some(metadata) = metadata {
            if vault_metadata_is_verified(metadata) {
                if let Some(context) = self.core.inner().identity_vault() {
                    if vault_context_matches_metadata(context, metadata) {
                        return Ok(Arc::new(
                            crate::internal::key_provider::vault::VaultBackedIdentitySigner::new(
                                identity_dir,
                                context.vault(),
                                metadata.legacy_key_material_refs(),
                            ),
                        ));
                    }
                    if matches!(
                        policy,
                        crate::core::IdentitySecretStoragePolicy::VaultRequired
                    ) {
                        return Err(crate::ImError::IdentityNotReady {
                            identity: summary.did.as_str().to_owned(),
                            missing: vec!["identity_vault_context_mismatch".to_owned()],
                        });
                    }
                } else if matches!(
                    policy,
                    crate::core::IdentitySecretStoragePolicy::VaultRequired
                ) {
                    return Err(crate::ImError::LocalStateUnavailable {
                        detail:
                            "identity has verified vault metadata but no identity secret vault was provided"
                                .to_owned(),
                    });
                }
            } else if matches!(
                policy,
                crate::core::IdentitySecretStoragePolicy::VaultRequired
            ) {
                return Err(crate::ImError::IdentityNotReady {
                    identity: summary.did.as_str().to_owned(),
                    missing: vec!["identity_vault_metadata_verified".to_owned()],
                });
            }
        } else if matches!(
            policy,
            crate::core::IdentitySecretStoragePolicy::VaultRequired
        ) {
            return Err(crate::ImError::IdentityNotReady {
                identity: summary.did.as_str().to_owned(),
                missing: vec!["identity_vault_metadata".to_owned()],
            });
        }
        Ok(Arc::new(
            crate::internal::key_provider::FileBackedIdentitySigner::new(identity_dir),
        ))
    }

    fn device_summary_for_entry(
        &self,
        entry: &RegistryEntry,
    ) -> crate::ImResult<super::IdentityDeviceSummary> {
        let Some(state) = entry.device_state.as_ref() else {
            return Ok(super::IdentityDeviceSummary {
                identity: entry.summary.clone(),
                mode: super::IdentityDeviceMode::Legacy,
                protocol_device_id: None,
                role: None,
                signing_key_id: None,
                e2ee_key_id: None,
                readiness: super::IdentityDeviceReadiness::Legacy,
                blocked_reason: None,
            });
        };
        state.validate_for_did(&entry.summary.did)?;
        if state.mode == crate::internal::identity_device_state::IdentityDeviceMode::Legacy {
            return Ok(super::IdentityDeviceSummary {
                identity: entry.summary.clone(),
                mode: super::IdentityDeviceMode::Legacy,
                protocol_device_id: None,
                role: None,
                signing_key_id: None,
                e2ee_key_id: None,
                readiness: super::IdentityDeviceReadiness::Legacy,
                blocked_reason: None,
            });
        }

        let authorization =
            state
                .authorization
                .as_ref()
                .ok_or_else(|| crate::ImError::IdentityNotReady {
                    identity: entry.summary.did.as_str().to_owned(),
                    missing: vec!["device_authorization".to_owned()],
                })?;
        let (local_root_available, local_blocker) = self.vnext_local_key_state(entry);
        let (readiness, blocked_reason) =
            match state.readiness(local_root_available, local_blocker.as_deref()) {
                crate::internal::identity_device_state::LocalDeviceReadiness::Legacy => {
                    (super::IdentityDeviceReadiness::Legacy, None)
                }
                crate::internal::identity_device_state::LocalDeviceReadiness::MemberReady => {
                    (super::IdentityDeviceReadiness::MemberReady, None)
                }
                crate::internal::identity_device_state::LocalDeviceReadiness::AdminAwaitingRoot => {
                    (super::IdentityDeviceReadiness::AdminAwaitingRoot, None)
                }
                crate::internal::identity_device_state::LocalDeviceReadiness::AdminReady => {
                    (super::IdentityDeviceReadiness::AdminReady, None)
                }
                crate::internal::identity_device_state::LocalDeviceReadiness::Blocked {
                    reason,
                } => (super::IdentityDeviceReadiness::Blocked, Some(reason)),
            };
        Ok(super::IdentityDeviceSummary {
            identity: entry.summary.clone(),
            mode: super::IdentityDeviceMode::VNext,
            protocol_device_id: Some(authorization.protocol_device_id.clone()),
            role: Some(match authorization.role {
                crate::internal::identity_device_state::DeviceAuthorizationRole::Member => {
                    super::IdentityDeviceRole::Member
                }
                crate::internal::identity_device_state::DeviceAuthorizationRole::Admin => {
                    super::IdentityDeviceRole::Admin
                }
            }),
            signing_key_id: Some(authorization.signing_key_id.clone()),
            e2ee_key_id: Some(authorization.e2ee_key_id.clone()),
            readiness,
            blocked_reason,
        })
    }

    fn vnext_local_key_state(&self, entry: &RegistryEntry) -> (bool, Option<String>) {
        let Some(metadata) = entry.vault_migration.as_ref() else {
            return (false, Some("identity_vault_metadata_missing".to_owned()));
        };
        if !vault_metadata_is_verified(metadata) {
            return (false, Some("identity_vault_metadata_unverified".to_owned()));
        }
        let Some(context) = self.core.inner().identity_vault() else {
            return (false, Some("identity_secret_vault_unavailable".to_owned()));
        };
        if !vault_context_matches_metadata(context, metadata) {
            return (false, Some("identity_vault_context_mismatch".to_owned()));
        }
        let Some(refs) = metadata.vnext_key_material_refs() else {
            return (false, Some("vnext_vault_key_refs_missing".to_owned()));
        };
        let Some(dir_name) = entry.identity_dir_name() else {
            return (false, Some("identity_directory_missing".to_owned()));
        };
        let identity_dir = match local_identity_dir(
            &self.core.inner().sdk_paths().identities.identity_root_dir,
            &dir_name,
        ) {
            Ok(path) => path,
            Err(_) => return (false, Some("identity_directory_invalid".to_owned())),
        };
        let provider = crate::internal::key_provider::vault::VaultBackedIdentitySigner::new_vnext(
            identity_dir,
            context.vault(),
            refs,
        );
        use crate::internal::key_provider::IdentitySigner;
        if provider.ensure_request_signing_available().is_err()
            || provider.ensure_agreement_available().is_err()
            || provider.auth_state().is_err()
        {
            return (false, Some("device_key_material_unavailable".to_owned()));
        }
        (provider.ensure_root_control_available().is_ok(), None)
    }

    fn verify_identity_vault_status(
        &self,
        status: super::IdentityVaultStatus,
        require_vault_backend: bool,
    ) -> crate::ImResult<super::IdentityVaultVerificationReport> {
        if require_vault_backend
            && status.selected_backend != super::IdentitySecretStorageBackend::Vault
        {
            return Err(crate::ImError::IdentityVault {
                failure: identity_vault_failure_from_status(&status),
            });
        }
        let verify = || -> crate::ImResult<()> {
            let runtime =
                self.load_runtime(super::IdentitySelector::Id(status.identity.id.clone()))?;
            let _ = runtime.key_provider.optional_did_document()?;
            runtime.key_provider.ensure_request_signing_available()?;
            runtime.key_provider.ensure_agreement_available()?;
            let _ = runtime.key_provider.auth_state()?;
            Ok(())
        };
        verify().map_err(|error| crate::ImError::IdentityVault {
            failure: match error {
                crate::ImError::PermissionDenied
                | crate::ImError::CredentialFileUnreadable { .. }
                | crate::ImError::Io { .. } => crate::IdentityVaultFailure::RecordOpenFailed,
                crate::ImError::IdentityVault { failure } => failure,
                _ => crate::IdentityVaultFailure::VerificationFailed,
            },
        })?;
        let mut warnings = status.warnings.clone();
        if status.plaintext_compat_retained.unwrap_or(false) {
            warnings.push("identity plaintext compatibility files are still retained".to_owned());
        }
        Ok(super::IdentityVaultVerificationReport {
            identity: status.identity.clone(),
            status,
            verified: true,
            warnings,
        })
    }

    fn identity_vault_status(
        &self,
        summary: &super::IdentitySummary,
        entry: Option<&RegistryEntry>,
    ) -> super::IdentityVaultStatus {
        let policy = self.core.inner().identity_secret_storage_policy();
        let context = self.core.inner().identity_vault();
        if entry.is_some_and(|entry| {
            entry.identity_custody_backend.as_deref() == Some("anp_identity")
                && entry.anp_identity_store_id.is_some()
                && entry.anp_identity_id.is_some()
        }) {
            let vault_selected = context.is_some()
                && !matches!(policy, crate::core::IdentitySecretStoragePolicy::FileCompat);
            let mut missing = Vec::new();
            let mut warnings = vec![
                "identity_vault_status reports the deprecated AWiki vault view of ANP Identity custody"
                    .to_owned(),
            ];
            if !vault_selected
                && matches!(
                    policy,
                    crate::core::IdentitySecretStoragePolicy::VaultRequired
                )
            {
                missing.push("identity_secret_vault".to_owned());
            }
            if matches!(policy, crate::core::IdentitySecretStoragePolicy::FileCompat) {
                warnings.push("identity secret storage policy is file_compat".to_owned());
            }
            return super::IdentityVaultStatus {
                identity: summary.clone(),
                storage_policy: policy,
                selected_backend: if vault_selected {
                    super::IdentitySecretStorageBackend::Vault
                } else {
                    super::IdentitySecretStorageBackend::FileCompat
                },
                vault_available: context.is_some(),
                vault_metadata_present: true,
                vault_metadata_verified: true,
                workspace_id: context.map(|context| context.workspace_id().to_owned()),
                device_id: context
                    .map(|context| context.vault_context_device_id().as_str().to_owned()),
                plaintext_compat_retained: Some(false),
                missing,
                warnings,
            };
        }
        let metadata = entry.and_then(|entry| entry.vault_migration.as_ref());
        let vault_metadata_present = metadata.is_some();
        let vault_metadata_verified = metadata.map(vault_metadata_is_verified).unwrap_or(false);
        let vault_workspace_matches = metadata
            .zip(context)
            .map(|(metadata, context)| metadata.workspace_id == context.workspace_id())
            .unwrap_or(false);
        let vault_device_matches = metadata
            .zip(context)
            .map(|(metadata, context)| {
                metadata.device_id == context.vault_context_device_id().as_str()
            })
            .unwrap_or(false);
        let vault_context_matches = vault_workspace_matches && vault_device_matches;
        let selected_backend =
            if vault_metadata_verified && context.is_some() && vault_context_matches {
                super::IdentitySecretStorageBackend::Vault
            } else {
                super::IdentitySecretStorageBackend::FileCompat
            };
        let mut missing = Vec::new();
        let mut warnings = Vec::new();
        if !vault_metadata_present {
            missing.push("identity_vault_metadata".to_owned());
        }
        if vault_metadata_present && !vault_metadata_verified {
            missing.push("identity_vault_metadata_verified".to_owned());
        }
        if context.is_none() {
            missing.push("identity_secret_vault".to_owned());
        }
        if vault_metadata_present && context.is_some() && !vault_workspace_matches {
            missing.push("identity_vault_workspace_match".to_owned());
            warnings.push(
                "identity vault metadata workspace does not match provided vault context"
                    .to_owned(),
            );
        }
        if vault_metadata_present && context.is_some() && !vault_device_matches {
            missing.push("identity_vault_device_match".to_owned());
            warnings.push(
                "identity vault metadata device does not match provided vault context".to_owned(),
            );
        }
        if metadata
            .map(|metadata| metadata.plaintext_compat_retained)
            .unwrap_or(false)
        {
            warnings.push("identity plaintext compatibility files are still retained".to_owned());
        }
        if matches!(policy, crate::core::IdentitySecretStoragePolicy::FileCompat) {
            warnings.push("identity secret storage policy is file_compat".to_owned());
        }
        super::IdentityVaultStatus {
            identity: summary.clone(),
            storage_policy: policy,
            selected_backend,
            vault_available: context.is_some(),
            vault_metadata_present,
            vault_metadata_verified,
            workspace_id: metadata.map(|metadata| metadata.workspace_id.clone()),
            device_id: metadata.map(|metadata| metadata.device_id.clone()),
            plaintext_compat_retained: metadata.map(|metadata| metadata.plaintext_compat_retained),
            missing,
            warnings,
        }
    }
}

fn identity_vault_failure_from_status(
    status: &super::IdentityVaultStatus,
) -> crate::IdentityVaultFailure {
    if !status.vault_available {
        crate::IdentityVaultFailure::Unavailable
    } else if !status.vault_metadata_present {
        crate::IdentityVaultFailure::MetadataMissing
    } else if !status.vault_metadata_verified {
        crate::IdentityVaultFailure::MetadataUnverified
    } else if status
        .missing
        .iter()
        .any(|item| item == "identity_vault_workspace_match")
    {
        crate::IdentityVaultFailure::WorkspaceMismatch
    } else if status
        .missing
        .iter()
        .any(|item| item == "identity_vault_device_match")
    {
        crate::IdentityVaultFailure::DeviceMismatch
    } else {
        crate::IdentityVaultFailure::VerificationFailed
    }
}

fn first_existing_path(identity_dir: &Path, names: &[&str]) -> std::path::PathBuf {
    names
        .iter()
        .map(|name| identity_dir.join(name))
        .find(|path| path.exists())
        .unwrap_or_else(|| identity_dir.join(names[0]))
}

async fn first_existing_path_async(identity_dir: &Path, names: &[&str]) -> std::path::PathBuf {
    for name in names {
        let path = identity_dir.join(name);
        if tokio::fs::try_exists(&path).await.unwrap_or(false) {
            return path;
        }
    }
    identity_dir.join(names[0])
}

#[derive(Debug, Clone)]
struct RegistrySnapshot {
    default_alias: Option<String>,
    entries: Vec<RegistryEntry>,
}

impl RegistrySnapshot {
    fn default_identity(&self) -> Option<super::IdentitySummary> {
        self.find(|entry| entry.summary.is_default)
            .map(|entry| entry.summary.clone())
            .or_else(|| {
                self.default_alias.as_deref().and_then(|alias| {
                    self.find(|entry| entry.local_alias.as_deref() == Some(alias))
                        .map(|entry| entry.summary.clone())
                })
            })
    }

    fn find(&self, predicate: impl Fn(&RegistryEntry) -> bool) -> Option<&RegistryEntry> {
        self.entries.iter().find(|entry| predicate(entry))
    }

    fn find_index(&self, selector: super::IdentitySelector) -> crate::ImResult<usize> {
        match selector {
            super::IdentitySelector::Default => {
                let default = self
                    .default_identity()
                    .ok_or(crate::ImError::DefaultIdentityMissing)?;
                self.entries
                    .iter()
                    .position(|entry| entry.summary == default)
                    .ok_or_else(|| crate::ImError::IdentityNotFound {
                        selector: "default".to_string(),
                    })
            }
            super::IdentitySelector::LocalAlias(alias) => {
                let alias = alias.trim();
                if alias.is_empty() {
                    return Err(crate::ImError::invalid_input(
                        Some("identity".to_string()),
                        "local alias must not be empty",
                    ));
                }
                self.entries
                    .iter()
                    .position(|entry| entry.local_alias.as_deref() == Some(alias))
                    .ok_or_else(|| crate::ImError::IdentityNotFound {
                        selector: alias.to_string(),
                    })
            }
            super::IdentitySelector::Did(did) => self
                .entries
                .iter()
                .position(|entry| entry.summary.did == did)
                .ok_or_else(|| crate::ImError::IdentityNotFound {
                    selector: did.as_str().to_string(),
                }),
            super::IdentitySelector::Id(id) => self
                .entries
                .iter()
                .position(|entry| entry.summary.id == id)
                .ok_or_else(|| crate::ImError::IdentityNotFound {
                    selector: id.as_str().to_string(),
                }),
            super::IdentitySelector::Handle(handle) => self
                .entries
                .iter()
                .position(|entry| entry.summary.handle.as_ref() == Some(&handle))
                .ok_or_else(|| crate::ImError::IdentityNotFound {
                    selector: handle.as_str().to_string(),
                }),
        }
    }

    fn find_entry(&self, selector: super::IdentitySelector) -> crate::ImResult<&RegistryEntry> {
        let index = self.find_index(selector)?;
        self.entries
            .get(index)
            .ok_or_else(|| crate::ImError::IdentityNotFound {
                selector: index.to_string(),
            })
    }

    fn apply_default_flags(&mut self) {
        let default_alias = self.default_alias.clone();
        for entry in &mut self.entries {
            if let Some(alias) = default_alias.as_deref() {
                entry.summary.is_default = entry.local_alias.as_deref() == Some(alias);
            }
        }
    }

    fn validate(&self) -> crate::ImResult<()> {
        let mut identity_ids = BTreeSet::new();
        let mut live_dids = BTreeSet::new();
        let mut aliases = BTreeSet::new();
        let mut handles = BTreeSet::new();
        let mut default_count = 0usize;

        for entry in &self.entries {
            validate_unique_registry_value(
                &mut identity_ids,
                "identity_id",
                entry.summary.id.as_str(),
            )?;
            validate_unique_registry_value(&mut live_dids, "live DID", entry.summary.did.as_str())?;
            if let Some(alias) = entry.local_alias.as_deref() {
                validate_unique_registry_value(&mut aliases, "local alias", alias)?;
            }
            if let Some(handle) = entry.summary.handle.as_ref() {
                validate_unique_registry_value(&mut handles, "handle", handle.as_str())?;
            }
            if entry.summary.is_default {
                default_count += 1;
            }
        }

        if default_count > 1 {
            return Err(registry_invariant_error(
                "registry must not contain more than one default identity",
            ));
        }

        if let Some(default_alias) = self.default_alias.as_deref() {
            let default_alias = default_alias.trim();
            if !default_alias.is_empty()
                && !self.entries.is_empty()
                && !self.entries.iter().any(|entry| {
                    entry
                        .local_alias
                        .as_deref()
                        .map(str::trim)
                        .is_some_and(|alias| alias == default_alias)
                })
            {
                return Err(registry_invariant_error(format!(
                    "default identity alias `{default_alias}` does not match any registry identity"
                )));
            }
        }

        Ok(())
    }
}

fn resolve_from_registry(
    registry: &RegistrySnapshot,
    selector: super::IdentitySelector,
) -> crate::ImResult<super::IdentitySummary> {
    match selector {
        super::IdentitySelector::Default => registry
            .default_identity()
            .ok_or(crate::ImError::DefaultIdentityMissing),
        super::IdentitySelector::LocalAlias(alias) => {
            let alias = alias.trim();
            if alias.is_empty() {
                return Err(crate::ImError::invalid_input(
                    Some("identity".to_string()),
                    "local alias must not be empty",
                ));
            }
            registry
                .find(|entry| entry.local_alias.as_deref() == Some(alias))
                .map(|entry| entry.summary.clone())
                .ok_or_else(|| crate::ImError::IdentityNotFound {
                    selector: alias.to_string(),
                })
        }
        super::IdentitySelector::Did(did) => registry
            .find(|entry| entry.summary.did == did)
            .map(|entry| entry.summary.clone())
            .ok_or_else(|| crate::ImError::IdentityNotFound {
                selector: did.as_str().to_string(),
            }),
        super::IdentitySelector::Id(id) => registry
            .find(|entry| entry.summary.id == id)
            .map(|entry| entry.summary.clone())
            .ok_or_else(|| crate::ImError::IdentityNotFound {
                selector: id.as_str().to_string(),
            }),
        super::IdentitySelector::Handle(handle) => registry
            .find(|entry| entry.summary.handle.as_ref() == Some(&handle))
            .map(|entry| entry.summary.clone())
            .ok_or_else(|| crate::ImError::IdentityNotFound {
                selector: handle.as_str().to_string(),
            }),
    }
}

#[derive(Debug, Clone)]
struct RegistryEntry {
    local_alias: Option<String>,
    dir_name: Option<String>,
    user_id: String,
    binding_generation: Option<String>,
    summary: super::IdentitySummary,
    vault_migration: Option<crate::internal::identity_store::IdentityVaultMigrationMetadata>,
    identity_custody_backend: Option<String>,
    anp_identity_store_id: Option<String>,
    anp_identity_id: Option<String>,
    anp_identity_auth_ref: Option<crate::internal::secret_vault::record::SecretRef>,
    device_state: Option<crate::internal::identity_device_state::IdentityDeviceState>,
}

#[derive(Debug, Clone)]
enum EnsureDaemonSubkeyPrepared {
    Ready {
        package: super::DaemonSubkeyPrivatePackage,
    },
    UpdateRequired {
        dir_name: String,
        identity_id: String,
        did_document: Value,
        package: super::DaemonSubkeyPrivatePackage,
        selector: super::IdentitySelector,
    },
}

#[derive(Debug, Clone)]
enum RevokeDaemonSubkeyPrepared {
    AlreadyRevoked {
        did: crate::ids::Did,
        verification_method: String,
    },
    UpdateRequired {
        dir_name: String,
        did: crate::ids::Did,
        verification_method: String,
        did_document: Value,
        selector: super::IdentitySelector,
    },
}

impl RegistryEntry {
    fn identity_dir_name(&self) -> Option<String> {
        self.dir_name
            .as_deref()
            .or(self.local_alias.as_deref())
            .or_else(|| Some(self.summary.id.as_str()))
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    }
}

fn sync_account_seed(
    entry: Option<&RegistryEntry>,
) -> crate::ImResult<Option<crate::internal::identity_runtime::SyncAccountSeed>> {
    let Some(entry) = entry else {
        return Ok(None);
    };
    let Some(device_state) = entry.device_state.as_ref() else {
        return Ok(None);
    };
    if device_state.mode != crate::internal::identity_device_state::IdentityDeviceMode::VNext {
        return Ok(None);
    }
    let Some(authorization) = device_state.authorization.as_ref() else {
        return Ok(None);
    };
    let account_id = entry.user_id.trim();
    if account_id.is_empty() {
        return Ok(None);
    }
    if account_id != entry.user_id {
        return Err(crate::ImError::IdentityBindingConflict {
            detail: "identity index account id is not canonical".to_owned(),
        });
    }
    let identity_generation = entry
        .binding_generation
        .as_deref()
        .map(|generation| {
            anp::wns::BindingGeneration::new(generation.to_owned())
                .map(|generation| generation.to_string())
                .map_err(|_| crate::ImError::IdentityBindingConflict {
                    detail: "identity index Handle generation is not canonical".to_owned(),
                })
        })
        .transpose()?;
    Ok(Some(
        crate::internal::identity_runtime::SyncAccountSeed::new(
            account_id.to_owned(),
            authorization.protocol_device_id.clone(),
            identity_generation,
            // DeviceAuthorizationProjection is the frozen signed v1 token /
            // registry boundary and therefore remains u64. Convert exactly
            // once here; the v2 public DTO, runtime context, actor commands,
            // and SQLite repositories keep the generation as a canonical
            // decimal String and never narrow it back to u64.
            authorization.auth_generation.to_string(),
            authorization.signing_key_id.clone(),
            authorization.e2ee_key_id.clone(),
            authorization.role,
            authorization.management_ready,
        ),
    ))
}

#[derive(Debug, Deserialize, Serialize)]
struct SdkRegistryFile {
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    default_identity: Option<String>,
    #[serde(default)]
    identities: Vec<SdkIdentityRecord>,
}

#[derive(Debug, Deserialize, Serialize)]
struct SdkIdentityRecord {
    id: String,
    did: String,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    dir_name: Option<String>,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    handle: Option<String>,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    display_name: Option<String>,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    local_alias: Option<String>,
    #[serde(default)]
    #[serde(skip_serializing_if = "String::is_empty")]
    user_id: String,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    binding_generation: Option<String>,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    device_id: Option<String>,
    #[serde(default)]
    is_default: bool,
    #[serde(default)]
    ready_for_auth: bool,
    #[serde(default)]
    ready_for_messaging: bool,
    #[serde(default)]
    missing: Vec<String>,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    vault_migration: Option<crate::internal::identity_store::IdentityVaultMigrationMetadata>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    identity_custody_backend: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    anp_identity_store_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    anp_identity_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    anp_identity_auth_ref: Option<crate::internal::secret_vault::record::SecretRef>,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    device_state: Option<crate::internal::identity_device_state::IdentityDeviceState>,
}

#[derive(Debug, Deserialize)]
struct LegacyRegistryFile {
    #[serde(default)]
    default_credential_name: String,
    #[serde(default)]
    credentials: BTreeMap<String, LegacyIdentityRecord>,
}

#[derive(Debug, Deserialize)]
struct LegacyIdentityRecord {
    #[serde(default)]
    credential_name: String,
    #[serde(default)]
    dir_name: String,
    #[serde(default)]
    did: String,
    #[serde(default)]
    unique_id: String,
    #[serde(default, deserialize_with = "deserialize_string_lossy")]
    user_id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    handle: String,
    #[serde(default)]
    full_handle: String,
    #[serde(default)]
    binding_generation: Option<String>,
    #[serde(default)]
    is_default: bool,
    #[serde(default)]
    vault_migration: Option<crate::internal::identity_store::IdentityVaultMigrationMetadata>,
    #[serde(default)]
    identity_custody_backend: Option<String>,
    #[serde(default)]
    anp_identity_store_id: Option<String>,
    #[serde(default)]
    anp_identity_id: Option<String>,
    #[serde(default)]
    anp_identity_auth_ref: Option<crate::internal::secret_vault::record::SecretRef>,
    #[serde(default)]
    device_state: Option<crate::internal::identity_device_state::IdentityDeviceState>,
}

fn parse_registry(raw: &[u8]) -> crate::ImResult<RegistrySnapshot> {
    if let Ok(value) = serde_json::from_slice::<serde_json::Value>(raw) {
        if value.as_object().is_some_and(|object| {
            object.contains_key("identities") || object.contains_key("default_identity")
        }) {
            let file: SdkRegistryFile =
                serde_json::from_value(value).map_err(|err| crate::ImError::Serialization {
                    detail: err.to_string(),
                })?;
            return sdk_registry_snapshot(file);
        }
    }
    let file: LegacyRegistryFile =
        serde_json::from_slice(raw).map_err(|err| crate::ImError::Serialization {
            detail: err.to_string(),
        })?;
    legacy_registry_snapshot(file)
}

fn sdk_registry_snapshot(file: SdkRegistryFile) -> crate::ImResult<RegistrySnapshot> {
    let mut entries = Vec::with_capacity(file.identities.len());
    for record in file.identities {
        let local_alias = record
            .local_alias
            .clone()
            .or_else(|| Some(record.id.clone()).filter(|value| !value.trim().is_empty()));
        let dir_name = record
            .dir_name
            .clone()
            .or_else(|| local_alias.clone())
            .or_else(|| Some(record.id.clone()).filter(|value| !value.trim().is_empty()));
        let did = crate::ids::Did::parse(&record.did)?;
        if let Some(state) = record.device_state.as_ref() {
            state.validate_for_did(&did)?;
        }
        entries.push(RegistryEntry {
            local_alias,
            dir_name,
            user_id: record.user_id,
            binding_generation: record.binding_generation,
            summary: super::IdentitySummary {
                id: crate::ids::IdentityId::parse(record.id)?,
                did,
                handle: optional_handle(record.handle)?,
                display_name: record.display_name,
                local_alias: record.local_alias,
                device_id: record.device_id,
                is_default: record.is_default,
                readiness: super::IdentityReadiness {
                    ready_for_auth: record.ready_for_auth,
                    ready_for_messaging: record.ready_for_messaging,
                    missing: record
                        .missing
                        .into_iter()
                        .map(identity_missing_item)
                        .collect(),
                },
            },
            vault_migration: record.vault_migration,
            identity_custody_backend: record.identity_custody_backend,
            anp_identity_store_id: record.anp_identity_store_id,
            anp_identity_id: record.anp_identity_id,
            anp_identity_auth_ref: record.anp_identity_auth_ref,
            device_state: record.device_state,
        });
    }
    let mut snapshot = RegistrySnapshot {
        default_alias: optional_trimmed_string(file.default_identity),
        entries,
    };
    snapshot.apply_default_flags();
    snapshot.validate()?;
    Ok(snapshot)
}

#[cfg(test)]
fn write_registry(path: &Path, registry: &RegistrySnapshot) -> crate::ImResult<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let file = SdkRegistryFile {
        default_identity: registry.default_alias.clone(),
        identities: registry
            .entries
            .iter()
            .map(|entry| SdkIdentityRecord {
                id: entry.summary.id.as_str().to_string(),
                did: entry.summary.did.as_str().to_string(),
                dir_name: entry.dir_name.clone(),
                handle: entry
                    .summary
                    .handle
                    .as_ref()
                    .map(|handle| handle.as_str().to_string()),
                display_name: entry.summary.display_name.clone(),
                local_alias: entry
                    .local_alias
                    .clone()
                    .or_else(|| entry.summary.local_alias.clone()),
                user_id: entry.user_id.clone(),
                binding_generation: entry.binding_generation.clone(),
                device_id: entry.summary.device_id.clone(),
                is_default: entry.summary.is_default,
                ready_for_auth: entry.summary.readiness.ready_for_auth,
                ready_for_messaging: entry.summary.readiness.ready_for_messaging,
                missing: entry
                    .summary
                    .readiness
                    .missing
                    .iter()
                    .map(identity_missing_item_to_string)
                    .collect(),
                vault_migration: entry.vault_migration.clone(),
                identity_custody_backend: entry.identity_custody_backend.clone(),
                anp_identity_store_id: entry.anp_identity_store_id.clone(),
                anp_identity_id: entry.anp_identity_id.clone(),
                anp_identity_auth_ref: entry.anp_identity_auth_ref.clone(),
                device_state: entry.device_state.clone(),
            })
            .collect(),
    };
    let raw = serde_json::to_vec_pretty(&file).map_err(|err| crate::ImError::Serialization {
        detail: err.to_string(),
    })?;
    fs::write(path, raw)?;
    Ok(())
}

fn local_identity_dir(root: &Path, dir_name: &str) -> crate::ImResult<PathBuf> {
    let relative = Path::new(dir_name);
    if relative.is_absolute()
        || relative
            .components()
            .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        return Err(crate::ImError::invalid_input(
            Some("identity".to_string()),
            "local identity directory name must be a simple relative path segment",
        ));
    }
    Ok(root.join(relative))
}

fn legacy_registry_snapshot(file: LegacyRegistryFile) -> crate::ImResult<RegistrySnapshot> {
    let mut entries = Vec::with_capacity(file.credentials.len());
    for (alias, record) in file.credentials {
        let id = first_non_empty([&record.unique_id, &record.credential_name, &alias])
            .unwrap_or(&alias)
            .to_string();
        let handle = first_non_empty([&record.full_handle, &record.handle, ""]);
        let dir_name = first_non_empty([&record.dir_name, &record.unique_id, &alias])
            .unwrap_or(&alias)
            .to_string();
        let missing = legacy_readiness_missing(&record, handle);
        let did = crate::ids::Did::parse(&record.did)?;
        if let Some(state) = record.device_state.as_ref() {
            state.validate_for_did(&did)?;
        }
        entries.push(RegistryEntry {
            local_alias: Some(alias.clone()),
            dir_name: Some(dir_name),
            user_id: record.user_id,
            binding_generation: record.binding_generation,
            summary: super::IdentitySummary {
                id: crate::ids::IdentityId::parse(id)?,
                did,
                handle: optional_handle(handle.map(str::to_string))?,
                display_name: Some(record.name).filter(|value| !value.trim().is_empty()),
                local_alias: Some(alias),
                device_id: None,
                is_default: record.is_default,
                readiness: super::IdentityReadiness {
                    ready_for_auth: true,
                    ready_for_messaging: missing.is_empty(),
                    missing,
                },
            },
            vault_migration: record.vault_migration,
            identity_custody_backend: record.identity_custody_backend,
            anp_identity_store_id: record.anp_identity_store_id,
            anp_identity_id: record.anp_identity_id,
            anp_identity_auth_ref: record.anp_identity_auth_ref,
            device_state: record.device_state,
        });
    }
    let mut snapshot = RegistrySnapshot {
        default_alias: optional_trimmed_string(Some(file.default_credential_name)),
        entries,
    };
    snapshot.apply_default_flags();
    snapshot.validate()?;
    Ok(snapshot)
}

fn legacy_readiness_missing(
    record: &LegacyIdentityRecord,
    handle: Option<&str>,
) -> Vec<super::IdentityMissingItem> {
    let mut missing = Vec::new();
    if record.user_id.trim().is_empty() {
        missing.push(super::IdentityMissingItem::Other(
            "registration".to_string(),
        ));
    }
    if handle.map(str::trim).unwrap_or_default().is_empty() {
        missing.push(super::IdentityMissingItem::Handle);
    }
    missing
}

fn vault_metadata_is_verified(
    metadata: &crate::internal::identity_store::IdentityVaultMigrationMetadata,
) -> bool {
    metadata.schema_version
        == crate::internal::identity_store::IDENTITY_VAULT_MIGRATION_SCHEMA_VERSION
        && matches!(
            metadata.status,
            crate::internal::identity_store::IdentityVaultMigrationStatus::Verified
        )
        && metadata.backend == "vault"
        && metadata.unlock_policy == "explicit_root_key"
        && !metadata.workspace_id.trim().is_empty()
        && !metadata.device_id.trim().is_empty()
}

fn vault_context_matches_metadata(
    context: &crate::core::options::IdentityVaultContext,
    metadata: &crate::internal::identity_store::IdentityVaultMigrationMetadata,
) -> bool {
    context.workspace_id() == metadata.workspace_id
        && context.vault_context_device_id().as_str() == metadata.device_id
}

fn default_alias_from_file(path: Option<&Path>) -> crate::ImResult<Option<String>> {
    let Some(path) = path else {
        return Ok(None);
    };
    match fs::read_to_string(path) {
        Ok(value) => Ok(Some(value.trim().to_string()).filter(|value| !value.is_empty())),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(crate::ImError::CredentialFileUnreadable {
            path_kind: "default_identity".to_string(),
            detail: err.to_string(),
        }),
    }
}

async fn default_alias_from_file_async(
    path: Option<std::path::PathBuf>,
) -> crate::ImResult<Option<String>> {
    let Some(path) = path else {
        return Ok(None);
    };
    match tokio::fs::read_to_string(path).await {
        Ok(value) => Ok(Some(value.trim().to_string()).filter(|value| !value.is_empty())),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(crate::ImError::CredentialFileUnreadable {
            path_kind: "default_identity".to_string(),
            detail: err.to_string(),
        }),
    }
}

fn optional_handle(value: Option<String>) -> crate::ImResult<Option<crate::ids::Handle>> {
    value
        .map(|value| value.trim().trim_start_matches('@').to_string())
        .filter(|value| !value.is_empty())
        .map(|value| crate::ids::Handle::parse(value, ""))
        .transpose()
}

fn identity_missing_item(value: String) -> super::IdentityMissingItem {
    match value.trim() {
        "did_document" | "DidDocument" => super::IdentityMissingItem::DidDocument,
        "private_key" | "PrivateKey" => super::IdentityMissingItem::PrivateKey,
        "auth_state" | "AuthState" => super::IdentityMissingItem::AuthState,
        "handle" | "Handle" => super::IdentityMissingItem::Handle,
        "message_endpoint" | "MessageEndpoint" => super::IdentityMissingItem::MessageEndpoint,
        other => super::IdentityMissingItem::Other(other.to_string()),
    }
}

fn identity_missing_item_to_string(value: &super::IdentityMissingItem) -> String {
    match value {
        super::IdentityMissingItem::DidDocument => "did_document".to_string(),
        super::IdentityMissingItem::PrivateKey => "private_key".to_string(),
        super::IdentityMissingItem::AuthState => "auth_state".to_string(),
        super::IdentityMissingItem::Handle => "handle".to_string(),
        super::IdentityMissingItem::MessageEndpoint => "message_endpoint".to_string(),
        super::IdentityMissingItem::Other(value) => value.clone(),
    }
}

fn deserialize_string_lossy<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<Value>::deserialize(deserializer)?;
    Ok(value
        .and_then(|value| value.as_str().map(ToString::to_string))
        .unwrap_or_default())
}

fn first_non_empty<const N: usize>(values: [&str; N]) -> Option<&str> {
    values.into_iter().find(|value| !value.trim().is_empty())
}

fn validate_unique_registry_value(
    seen: &mut BTreeSet<String>,
    label: &str,
    value: &str,
) -> crate::ImResult<()> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(());
    }
    if !seen.insert(value.to_owned()) {
        return Err(registry_invariant_error(format!(
            "duplicate {label} `{value}` in identity registry"
        )));
    }
    Ok(())
}

fn registry_invariant_error(message: impl Into<String>) -> crate::ImError {
    crate::ImError::invalid_input(Some("identity_registry".to_owned()), message.into())
}

fn optional_trimmed_string(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::internal::platform_secret::{DeviceVaultRootKey, SecretBytes};
    use crate::internal::secret_vault::{
        FileSecretVault, FileSecretVaultStore, SealSecretRequest, SecretAccessPolicy, SecretKind,
        SecretMetadata, SecretVault,
    };
    use serde_json::json;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    #[test]
    fn identity_registry_rejects_duplicate_live_did() {
        let err = parse_registry(
            br#"{
              "identities": [
                {"id":"alice-id","did":"did:example:shared","local_alias":"alice"},
                {"id":"bob-id","did":"did:example:shared","local_alias":"bob"}
              ]
            }"#,
        )
        .unwrap_err();

        assert_registry_error_contains(err, "duplicate live DID");
    }

    #[test]
    fn identity_registry_rejects_duplicate_identity_id() {
        let err = parse_registry(
            br#"{
              "identities": [
                {"id":"same-id","did":"did:example:alice","local_alias":"alice"},
                {"id":"same-id","did":"did:example:bob","local_alias":"bob"}
              ]
            }"#,
        )
        .unwrap_err();

        assert_registry_error_contains(err, "duplicate identity_id");
    }

    #[test]
    fn identity_registry_rejects_duplicate_alias() {
        let err = parse_registry(
            br#"{
              "identities": [
                {"id":"alice-id","did":"did:example:alice","local_alias":"shared"},
                {"id":"bob-id","did":"did:example:bob","local_alias":"shared"}
              ]
            }"#,
        )
        .unwrap_err();

        assert_registry_error_contains(err, "duplicate local alias");
    }

    #[test]
    fn identity_registry_rejects_duplicate_handle() {
        let err = parse_registry(
            br#"{
              "identities": [
                {"id":"alice-id","did":"did:example:alice","local_alias":"alice","handle":"shared.awiki.test"},
                {"id":"bob-id","did":"did:example:bob","local_alias":"bob","handle":"shared.awiki.test"}
              ]
            }"#,
        )
        .unwrap_err();

        assert_registry_error_contains(err, "duplicate handle");
    }

    #[test]
    fn identity_registry_rejects_missing_default_alias() {
        let err = parse_registry(
            br#"{
              "default_identity": "missing",
              "identities": [
                {"id":"alice-id","did":"did:example:alice","local_alias":"alice"}
              ]
            }"#,
        )
        .unwrap_err();

        assert_registry_error_contains(err, "default identity alias");
    }

    #[tokio::test]
    async fn vnext_device_summary_and_sync_binding_use_exact_persisted_identifiers() {
        use crate::internal::identity_device_state::{
            DeviceAuthorizationProjection, DeviceAuthorizationRole, DeviceAuthorizationStatus,
            IdentityDeviceMode, IdentityDeviceState, IdentityInternalCheckpoint,
            IDENTITY_DEVICE_STATE_SCHEMA_VERSION,
        };

        let root = tempfile::tempdir().unwrap();
        let paths = test_paths(root.path());
        let registry_path = paths.identities.registry_path.clone();
        let local_state_path = paths.local_state.sqlite_path.clone();
        let vault_dir = root.path().join("vault");
        let generated = crate::internal::identity_generation::generate_vnext_handle_identity_with_default_daemon_subkey(
            "awiki.info",
            "registry-summary",
            None,
            None,
        )
        .unwrap();
        let did = generated.did.clone();
        let protocol_device_id = generated.protocol_device_id.clone();
        let signing_key_id = generated.device_signing_key_id.clone();
        let e2ee_key_id = generated.device_e2ee_key_id.clone();
        let vault = Arc::new(FileSecretVault::new(
            DeviceVaultRootKey::from_bytes([29_u8; 32]),
            FileSecretVaultStore::new(&vault_dir),
        ));
        crate::internal::identity_store::IdentityStore::new(&paths.identities)
            .save_identity_with_secret_storage(
                crate::internal::identity_store::SaveIdentityInput {
                    local_alias: "alice".to_owned(),
                    did: did.clone(),
                    unique_id: "alice-id".to_owned(),
                    user_id: "user-1".to_owned(),
                    display_name: "Alice".to_owned(),
                    handle: "alice".to_owned(),
                    full_handle: "alice.awiki.info".to_owned(),
                    binding_generation: Some("184467440737095516160000000000000000001".to_owned()),
                    jwt_token: "device-token".to_owned(),
                    did_document: Some(generated.did_document.clone()),
                    key_mode: crate::internal::identity_store::SaveIdentityKeyMode::VNext {
                        root_key_id: generated.root_key_id.clone(),
                        device_signing_key_id: signing_key_id.clone(),
                        device_e2ee_key_id: e2ee_key_id.clone(),
                    },
                    device_state: Some(IdentityDeviceState {
                        schema_version: IDENTITY_DEVICE_STATE_SCHEMA_VERSION,
                        mode: IdentityDeviceMode::VNext,
                        authorization: Some(DeviceAuthorizationProjection {
                            protocol_device_id: protocol_device_id.clone(),
                            signing_key_id: signing_key_id.clone(),
                            e2ee_key_id: e2ee_key_id.clone(),
                            status: DeviceAuthorizationStatus::Active,
                            role: DeviceAuthorizationRole::Admin,
                            management_ready: true,
                            auth_generation: 1,
                        }),
                        checkpoint: Some(IdentityInternalCheckpoint {
                            document_version: 9,
                            document_hash: "sha256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
                                .to_owned(),
                            registry_version: 4,
                        }),
                    }),
                    key1_private_pem: generated.root_private_pem,
                    key1_public_pem: generated.root_public_pem,
                    e2ee_signing_private_pem: generated.device_signing_private_pem,
                    e2ee_agreement_private_pem: generated.device_e2ee_private_pem,
                    daemon_subkey_package: None,
                    make_default: true,
                },
                crate::internal::identity_store::SaveIdentitySecretStorage::Vault {
                    workspace_id: "workspace-a".to_owned(),
                    device_id: "vault-a".to_owned(),
                    vault,
                },
            )
            .unwrap();
        let core = crate::ImCore::new_with_options(
            test_config(),
            paths,
            crate::ImCoreOpenOptions::default().with_identity_secret_vault(
                crate::IdentitySecretStoragePolicy::VaultRequired,
                crate::ImCoreSecretVaultOptions::new(
                    DeviceVaultRootKey::from_bytes([29_u8; 32]),
                    &vault_dir,
                    "workspace-a",
                    "vault-a",
                ),
            ),
        )
        .unwrap();

        let summary = core
            .identities()
            .device_summary(crate::identity::IdentitySelector::Default)
            .unwrap();
        assert_eq!(summary.mode, crate::identity::IdentityDeviceMode::VNext);
        assert_eq!(
            summary.role,
            Some(crate::identity::IdentityDeviceRole::Admin)
        );
        assert_eq!(
            summary.readiness,
            crate::identity::IdentityDeviceReadiness::AdminReady
        );
        assert_eq!(
            summary
                .protocol_device_id
                .as_ref()
                .map(|value| value.as_str()),
            Some(protocol_device_id.as_str())
        );
        assert_eq!(
            summary.signing_key_id.as_deref(),
            Some(signing_key_id.as_str())
        );
        assert_eq!(summary.e2ee_key_id.as_deref(), Some(e2ee_key_id.as_str()));
        assert!(summary.blocked_reason.is_none());
        let public_json = serde_json::to_string(&summary).unwrap();
        assert!(!public_json.contains("document_version"));
        assert!(!public_json.contains("document_hash"));
        assert!(!public_json.contains("registry_version"));
        assert!(!public_json.contains("root_private"));
        assert!(!public_json.contains("vault"));

        let active_binding = core
            .client(crate::identity::IdentitySelector::Default)
            .unwrap()
            .active_sync_account_binding()
            .await
            .unwrap();
        assert_eq!(
            active_binding,
            crate::identity::ActiveSyncAccountBinding {
                owner_identity_id: "alice-id".to_owned(),
                account_id: "user-1".to_owned(),
                current_did: did.as_str().to_owned(),
                protocol_device_id: protocol_device_id.as_str().to_owned(),
                identity_generation: "184467440737095516160000000000000000001".to_owned(),
                device_auth_generation: "1".to_owned(),
            }
        );
        let persisted_binding = rusqlite::Connection::open(local_state_path)
            .unwrap()
            .query_row(
                "SELECT owner_identity_id, account_id, current_did, device_id,
                        identity_generation, device_auth_generation
                 FROM identity_account_bindings",
                [],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, String>(5)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(
            persisted_binding,
            (
                active_binding.owner_identity_id,
                active_binding.account_id,
                active_binding.current_did,
                active_binding.protocol_device_id,
                active_binding.identity_generation,
                active_binding.device_auth_generation,
            )
        );

        let mut registry_json: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&registry_path).unwrap()).unwrap();
        registry_json["credentials"]["alice"]
            .as_object_mut()
            .unwrap()
            .remove("device_state");
        std::fs::write(
            &registry_path,
            serde_json::to_vec_pretty(&registry_json).unwrap(),
        )
        .unwrap();
        let error = core
            .identities()
            .load_runtime(crate::identity::IdentitySelector::Default)
            .err()
            .expect("vNext vault refs without vNext state must fail closed");
        assert_eq!(
            error,
            crate::ImError::IdentityNotReady {
                identity: did.as_str().to_owned(),
                missing: vec!["vnext_device_state".to_owned()],
            }
        );
    }

    #[tokio::test]
    async fn active_sync_binding_fetches_missing_generation_from_wns_and_persists_both_projections()
    {
        use crate::internal::identity_device_state::{
            DeviceAuthorizationProjection, DeviceAuthorizationRole, DeviceAuthorizationStatus,
            IdentityDeviceMode, IdentityDeviceState, IdentityInternalCheckpoint,
            IDENTITY_DEVICE_STATE_SCHEMA_VERSION,
        };

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let generation = "184467440737095516160000000000000000099";
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(2)))
                .unwrap();
            let mut request = Vec::new();
            let mut buffer = [0_u8; 2048];
            while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                let read = stream.read(&mut buffer).unwrap();
                assert!(read > 0, "WNS request closed before headers");
                request.extend_from_slice(&buffer[..read]);
            }
            let request = String::from_utf8_lossy(&request);
            assert!(
                request.starts_with("GET /.well-known/handle/alice "),
                "unexpected WNS request: {request}"
            );
            let body = format!(
                r#"{{"handle":"alice.awiki.test","did":"did:wba:awiki.test:alice:e1_root","status":"active","binding_generation":"{generation}"}}"#
            );
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
            stream.flush().unwrap();
        });

        let root = tempfile::tempdir().unwrap();
        let paths = test_paths(root.path());
        let vault_dir = root.path().join("vault");
        let vault = Arc::new(FileSecretVault::new(
            DeviceVaultRootKey::from_bytes([55_u8; 32]),
            FileSecretVaultStore::new(&vault_dir),
        ));
        let did = crate::ids::Did::parse("did:wba:awiki.test:alice:e1_root").unwrap();
        let signing_key_id = format!("{}#dev-a-sign", did.as_str());
        let e2ee_key_id = format!("{}#dev-a-e2ee", did.as_str());
        crate::internal::identity_store::IdentityStore::new(&paths.identities)
            .save_identity_with_secret_storage(
                crate::internal::identity_store::SaveIdentityInput {
                    local_alias: "alice".to_owned(),
                    did: did.clone(),
                    unique_id: "alice-id".to_owned(),
                    user_id: "account-alice".to_owned(),
                    display_name: "Alice".to_owned(),
                    handle: "alice".to_owned(),
                    full_handle: "alice.awiki.test".to_owned(),
                    binding_generation: None,
                    jwt_token: "device-token".to_owned(),
                    did_document: Some(json!({"id": did.as_str()})),
                    key_mode: crate::internal::identity_store::SaveIdentityKeyMode::VNext {
                        root_key_id: format!("{}#key-1", did.as_str()),
                        device_signing_key_id: signing_key_id.clone(),
                        device_e2ee_key_id: e2ee_key_id.clone(),
                    },
                    device_state: Some(IdentityDeviceState {
                        schema_version: IDENTITY_DEVICE_STATE_SCHEMA_VERSION,
                        mode: IdentityDeviceMode::VNext,
                        authorization: Some(DeviceAuthorizationProjection {
                            protocol_device_id: crate::ids::ProtocolDeviceId::parse("dev-a")
                                .unwrap(),
                            signing_key_id,
                            e2ee_key_id,
                            status: DeviceAuthorizationStatus::Active,
                            role: DeviceAuthorizationRole::Member,
                            management_ready: false,
                            auth_generation: 9,
                        }),
                        checkpoint: Some(IdentityInternalCheckpoint {
                            document_version: 2,
                            document_hash: "sha256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
                                .to_owned(),
                            registry_version: 2,
                        }),
                    }),
                    key1_private_pem: "root-private".to_owned(),
                    key1_public_pem: "root-public".to_owned(),
                    e2ee_signing_private_pem: "device-signing-private".to_owned(),
                    e2ee_agreement_private_pem: "device-e2ee-private".to_owned(),
                    daemon_subkey_package: None,
                    make_default: true,
                },
                crate::internal::identity_store::SaveIdentitySecretStorage::Vault {
                    workspace_id: "workspace-a".to_owned(),
                    device_id: "vault-a".to_owned(),
                    vault,
                },
            )
            .unwrap();
        let mut config = test_config();
        config.service_base_url =
            crate::ServiceEndpoint::parse(format!("http://{address}")).unwrap();
        config.user_service_endpoint = Some(config.service_base_url.clone());
        let core = crate::ImCore::new_with_options(
            config,
            paths.clone(),
            crate::ImCoreOpenOptions::default().with_identity_secret_vault(
                crate::IdentitySecretStoragePolicy::VaultRequired,
                crate::ImCoreSecretVaultOptions::new(
                    DeviceVaultRootKey::from_bytes([55_u8; 32]),
                    &vault_dir,
                    "workspace-a",
                    "vault-a",
                ),
            ),
        )
        .unwrap();

        let binding = core
            .client(crate::identity::IdentitySelector::Default)
            .unwrap()
            .active_sync_account_binding()
            .await
            .unwrap();
        server.join().unwrap();

        assert_eq!(binding.identity_generation, generation);
        assert_eq!(binding.device_auth_generation, "9");
        let index = crate::internal::identity_store::IdentityStore::new(&paths.identities)
            .load_index()
            .unwrap();
        let entry = &index.credentials["alice"];
        assert_eq!(entry.binding_generation.as_deref(), Some(generation));
        let identity: Value = serde_json::from_slice(
            &std::fs::read(
                paths
                    .identities
                    .identity_root_dir
                    .join(&entry.dir_name)
                    .join("identity.json"),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(identity["binding_generation"].as_str(), Some(generation));
    }

    #[tokio::test]
    async fn legacy_and_hosted_clients_fail_closed_for_active_sync_binding() {
        let root = tempfile::tempdir().unwrap();
        let paths = test_paths(root.path());
        crate::internal::identity_store::IdentityStore::new(&paths.identities)
            .save_identity(crate::internal::identity_store::SaveIdentityInput {
                local_alias: "legacy".to_owned(),
                did: crate::ids::Did::parse("did:example:legacy").unwrap(),
                unique_id: "legacy-id".to_owned(),
                user_id: "account-legacy".to_owned(),
                display_name: "Legacy".to_owned(),
                handle: "legacy".to_owned(),
                full_handle: "legacy.awiki.test".to_owned(),
                binding_generation: Some("1".to_owned()),
                jwt_token: "legacy-token".to_owned(),
                did_document: Some(json!({"id": "did:example:legacy"})),
                key_mode: crate::internal::identity_store::SaveIdentityKeyMode::LegacyKey1,
                device_state: None,
                key1_private_pem: "legacy-private".to_owned(),
                key1_public_pem: "legacy-public".to_owned(),
                e2ee_signing_private_pem: String::new(),
                e2ee_agreement_private_pem: "legacy-agreement".to_owned(),
                daemon_subkey_package: None,
                make_default: true,
            })
            .unwrap();
        let core = crate::ImCore::new(test_config(), paths).unwrap();
        let legacy_error = core
            .client(crate::identity::IdentitySelector::Default)
            .unwrap()
            .active_sync_account_binding()
            .await
            .unwrap_err();
        assert!(matches!(
            legacy_error,
            crate::ImError::UnsupportedCapability { capability }
                if capability == "active-sync-account-binding"
        ));

        let hosted_did = "did:wba:awiki.test:agent:hosted:e1_demo";
        let hosted_error = core
            .client_with_identity_material(crate::identity::HostedIdentityMaterial {
                identity_id: "hosted-id".to_owned(),
                did: hosted_did.to_owned(),
                handle: Some("hosted.awiki.test".to_owned()),
                display_name: Some("Hosted".to_owned()),
                did_document: json!({"id": hosted_did}),
                default_signing_private_key_pem: "hosted-signing".to_owned(),
                e2ee_agreement_private_key_pem: Some("hosted-agreement".to_owned()),
                auth_token: None,
            })
            .unwrap()
            .active_sync_account_binding()
            .await
            .unwrap_err();
        assert!(matches!(
            hosted_error,
            crate::ImError::UnsupportedCapability { capability }
                if capability == "active-sync-account-binding"
        ));
    }

    #[test]
    fn identity_vault_status_and_runtime_use_verified_matching_vault_context() {
        let root = tempfile::tempdir().unwrap();
        let identity_dir = root.path().join("identities").join("alice-id");
        std::fs::create_dir_all(&identity_dir).unwrap();
        let bundle = anp::authentication::create_did_wba_document(
            "alice.example",
            anp::authentication::DidDocumentOptions::default(),
        )
        .unwrap();
        let did = bundle.did().unwrap().to_owned();
        let signing_pem = bundle.keys[anp::authentication::VM_KEY_AUTH]
            .private_key_pem
            .clone();
        let agreement_pem = bundle.keys[anp::authentication::VM_KEY_E2EE_AGREEMENT]
            .private_key_pem
            .clone();
        std::fs::write(
            identity_dir.join("did_document.json"),
            serde_json::to_vec(&bundle.did_document).unwrap(),
        )
        .unwrap();
        std::fs::write(
            identity_dir.join("key-1-private.pem"),
            "file-signing-secret",
        )
        .unwrap();
        std::fs::write(
            identity_dir.join("e2ee-agreement-private.pem"),
            "file-agreement-secret",
        )
        .unwrap();
        std::fs::write(
            identity_dir.join("auth.json"),
            serde_json::to_vec(&json!({"jwt_token": "file-token-secret"})).unwrap(),
        )
        .unwrap();
        let vault_dir = root.path().join("vault");
        let vault = FileSecretVault::new(
            DeviceVaultRootKey::from_bytes([31_u8; 32]),
            FileSecretVaultStore::new(&vault_dir),
        );
        let signing_ref = vault
            .seal(SealSecretRequest {
                metadata: test_secret_metadata(
                    "workspace-a",
                    "device-a",
                    "alice-id",
                    &did,
                    SecretKind::IdentityRootPrivate,
                    "key-1",
                ),
                plaintext: SecretBytes::from_vec(signing_pem.as_bytes().to_vec()),
            })
            .unwrap();
        let agreement_ref = vault
            .seal(SealSecretRequest {
                metadata: test_secret_metadata(
                    "workspace-a",
                    "device-a",
                    "alice-id",
                    &did,
                    SecretKind::IdentityE2eeAgreementPrivate,
                    "key-3",
                ),
                plaintext: SecretBytes::from_vec(agreement_pem.as_bytes().to_vec()),
            })
            .unwrap();
        let auth_ref = vault
            .seal(SealSecretRequest {
                metadata: test_secret_metadata(
                    "workspace-a",
                    "device-a",
                    "alice-id",
                    &did,
                    SecretKind::AuthJwt,
                    "auth.json",
                ),
                plaintext: SecretBytes::from_vec(
                    crate::internal::auth::state::auth_state_json_for_token("vault-token-secret")
                        .unwrap(),
                ),
            })
            .unwrap();
        let registry_path = root.path().join("identities").join("registry.json");
        std::fs::write(
            &registry_path,
            serde_json::to_vec_pretty(&json!({
                "default_credential_name": "alice",
                "credentials": {
                    "alice": {
                        "credential_name": "alice",
                        "dir_name": "alice-id",
                        "did": did,
                        "unique_id": "alice-id",
                        "user_id": "user-1",
                        "name": "Alice",
                        "handle": "alice",
                        "full_handle": "alice.example",
                        "is_default": true,
                        "vault_migration": {
                            "schema_version": 1,
                            "status": "verified",
                            "backend": "vault",
                            "unlock_policy": "explicit_root_key",
                            "migrated_at": "2026-07-03T00:00:00Z",
                            "workspace_id": "workspace-a",
                            "device_id": "device-a",
                            "plaintext_compat_retained": true,
                            "refs": {
                                "default_signing_private": signing_ref,
                                "e2ee_agreement_private": agreement_ref,
                                "auth_jwt": auth_ref
                            }
                        }
                    }
                }
            }))
            .unwrap(),
        )
        .unwrap();
        let core = crate::ImCore::new_with_options(
            test_config(),
            test_paths(root.path()),
            crate::ImCoreOpenOptions::default().with_identity_secret_vault(
                crate::IdentitySecretStoragePolicy::VaultRequired,
                crate::ImCoreSecretVaultOptions::new(
                    DeviceVaultRootKey::from_bytes([31_u8; 32]),
                    &vault_dir,
                    "workspace-a",
                    "device-a",
                ),
            ),
        )
        .unwrap();

        let status = core
            .identities()
            .vault_status(crate::identity::IdentitySelector::Default)
            .unwrap();
        assert_eq!(
            status.selected_backend,
            crate::identity::IdentitySecretStorageBackend::Vault
        );
        assert!(status.vault_available);
        assert!(status.vault_metadata_present);
        assert!(status.vault_metadata_verified);
        assert_eq!(status.workspace_id.as_deref(), Some("workspace-a"));
        assert_eq!(status.device_id.as_deref(), Some("device-a"));
        assert_eq!(status.plaintext_compat_retained, Some(true));
        assert!(status.missing.is_empty());

        let runtime = core
            .identities()
            .load_runtime(crate::identity::IdentitySelector::Default)
            .unwrap();
        runtime
            .key_provider
            .ensure_request_signing_available()
            .unwrap();
        runtime.key_provider.ensure_agreement_available().unwrap();
        runtime
            .key_provider
            .ensure_root_control_available()
            .unwrap();
        assert_eq!(
            runtime.key_provider.valid_auth_token().unwrap().as_deref(),
            Some("vault-token-secret")
        );

        let written = parse_registry(&std::fs::read(&registry_path).unwrap()).unwrap();
        let rewritten = root
            .path()
            .join("identities")
            .join("rewritten-registry.json");
        write_registry(&rewritten, &written).unwrap();
        let reparsed = parse_registry(&std::fs::read(rewritten).unwrap()).unwrap();
        assert!(reparsed.entries[0].vault_migration.is_some());

        let open_with = |key: [u8; 32], workspace_id: &str, device_id: &str| {
            crate::ImCore::new_with_options(
                test_config(),
                test_paths(root.path()),
                crate::ImCoreOpenOptions::default().with_identity_secret_vault(
                    crate::IdentitySecretStoragePolicy::VaultRequired,
                    crate::ImCoreSecretVaultOptions::new(
                        DeviceVaultRootKey::from_bytes(key),
                        &vault_dir,
                        workspace_id,
                        device_id,
                    ),
                ),
            )
            .unwrap()
        };

        let wrong_workspace = open_with([31_u8; 32], "workspace-b", "device-a");
        let status = wrong_workspace
            .identities()
            .vault_status(crate::identity::IdentitySelector::Default)
            .unwrap();
        assert!(status
            .missing
            .contains(&"identity_vault_workspace_match".to_owned()));
        assert!(!status
            .missing
            .contains(&"identity_vault_device_match".to_owned()));
        assert_eq!(
            wrong_workspace
                .identities()
                .verify_identity_vault(crate::identity::IdentitySelector::Default)
                .unwrap_err(),
            crate::ImError::IdentityVault {
                failure: crate::IdentityVaultFailure::WorkspaceMismatch,
            }
        );

        let wrong_device = open_with([31_u8; 32], "workspace-a", "device-b");
        let status = wrong_device
            .identities()
            .vault_status(crate::identity::IdentitySelector::Default)
            .unwrap();
        assert!(!status
            .missing
            .contains(&"identity_vault_workspace_match".to_owned()));
        assert!(status
            .missing
            .contains(&"identity_vault_device_match".to_owned()));
        assert_eq!(
            wrong_device
                .identities()
                .verify_identity_vault(crate::identity::IdentitySelector::Default)
                .unwrap_err(),
            crate::ImError::IdentityVault {
                failure: crate::IdentityVaultFailure::DeviceMismatch,
            }
        );

        let wrong_root = open_with([32_u8; 32], "workspace-a", "device-a");
        assert_eq!(
            wrong_root
                .identities()
                .verify_identity_vault(crate::identity::IdentitySelector::Default)
                .unwrap_err(),
            crate::ImError::IdentityVault {
                failure: crate::IdentityVaultFailure::RecordOpenFailed,
            }
        );

        let unavailable = crate::ImCore::new_with_options(
            test_config(),
            test_paths(root.path()),
            crate::ImCoreOpenOptions {
                identity_secret_storage_policy: crate::IdentitySecretStoragePolicy::VaultPreferred,
                identity_secret_vault: None,
                multi_device_device_revoke_enabled: false,
                multi_device_direct_e2ee_enabled: false,
                multi_device_group_e2ee_enabled: false,
                multi_device_handle_recovery_enabled: false,
                multi_device_audience: None,
            },
        )
        .unwrap();
        assert_eq!(
            unavailable
                .identities()
                .verify_identity_vault(crate::identity::IdentitySelector::Default)
                .unwrap_err(),
            crate::ImError::IdentityVault {
                failure: crate::IdentityVaultFailure::Unavailable,
            }
        );

        let original_registry = std::fs::read(&registry_path).unwrap();
        let mut registry_json: serde_json::Value =
            serde_json::from_slice(&original_registry).unwrap();
        registry_json["credentials"]["alice"]
            .as_object_mut()
            .unwrap()
            .remove("vault_migration");
        std::fs::write(
            &registry_path,
            serde_json::to_vec_pretty(&registry_json).unwrap(),
        )
        .unwrap();
        let missing_metadata = open_with([31_u8; 32], "workspace-a", "device-a");
        assert_eq!(
            missing_metadata
                .identities()
                .verify_identity_vault(crate::identity::IdentitySelector::Default)
                .unwrap_err(),
            crate::ImError::IdentityVault {
                failure: crate::IdentityVaultFailure::MetadataMissing,
            }
        );

        let mut registry_json: serde_json::Value =
            serde_json::from_slice(&original_registry).unwrap();
        registry_json["credentials"]["alice"]["vault_migration"]["backend"] =
            json!("tampered-backend");
        std::fs::write(
            &registry_path,
            serde_json::to_vec_pretty(&registry_json).unwrap(),
        )
        .unwrap();
        let unverified_metadata = open_with([31_u8; 32], "workspace-a", "device-a");
        assert_eq!(
            unverified_metadata
                .identities()
                .verify_identity_vault(crate::identity::IdentitySelector::Default)
                .unwrap_err(),
            crate::ImError::IdentityVault {
                failure: crate::IdentityVaultFailure::MetadataUnverified,
            }
        );
        std::fs::write(&registry_path, original_registry).unwrap();
    }

    #[test]
    fn retirement_tombstone_does_not_wipe_reregistered_identity_vault() {
        use crate::internal::identity_device_state::{
            DeviceAuthorizationProjection, DeviceAuthorizationRole, DeviceAuthorizationStatus,
            IdentityDeviceMode, IdentityDeviceState, IdentityInternalCheckpoint,
            IDENTITY_DEVICE_STATE_SCHEMA_VERSION,
        };

        // Regression: a Completed retirement tombstone is keyed by identity_id,
        // which is the deterministic DID suffix for a handle. Re-registering
        // the same handle reuses the same identity_id, so the tombstone replay
        // on the next open must NOT wipe the re-registered identity's vault
        // records (that used to destroy live secrets and fail the next open
        // with identity_vault_record_open_failed).
        let root = tempfile::tempdir().unwrap();
        let paths = test_paths(root.path());
        let vault_dir = root.path().join("vault");
        let vault = Arc::new(FileSecretVault::new(
            DeviceVaultRootKey::from_bytes([29_u8; 32]),
            FileSecretVaultStore::new(&vault_dir),
        ));
        let generated = crate::internal::identity_generation::generate_vnext_handle_identity_with_default_daemon_subkey(
            "awiki.info",
            "retirement-replay",
            None,
            None,
        )
        .unwrap();

        let open_core = || {
            crate::ImCore::new_with_options(
                test_config(),
                test_paths(root.path()),
                crate::ImCoreOpenOptions::default().with_identity_secret_vault(
                    crate::IdentitySecretStoragePolicy::VaultRequired,
                    crate::ImCoreSecretVaultOptions::new(
                        DeviceVaultRootKey::from_bytes([29_u8; 32]),
                        &vault_dir,
                        "workspace-a",
                        "vault-a",
                    ),
                ),
            )
            .unwrap()
        };

        let save_identity = || {
            let did = generated.did.clone();
            let signing_key_id = generated.device_signing_key_id.clone();
            let e2ee_key_id = generated.device_e2ee_key_id.clone();
            crate::internal::identity_store::IdentityStore::new(&paths.identities)
                .save_identity_with_secret_storage(
                    crate::internal::identity_store::SaveIdentityInput {
                        local_alias: "alice".to_owned(),
                        did: did.clone(),
                        unique_id: "alice-id".to_owned(),
                        user_id: "user-1".to_owned(),
                        display_name: "Alice".to_owned(),
                        handle: "alice".to_owned(),
                        full_handle: "alice.awiki.info".to_owned(),
                        binding_generation: Some(
                            "184467440737095516160000000000000000001".to_owned(),
                        ),
                        jwt_token: "device-token".to_owned(),
                        did_document: Some(generated.did_document.clone()),
                        key_mode: crate::internal::identity_store::SaveIdentityKeyMode::VNext {
                            root_key_id: generated.root_key_id.clone(),
                            device_signing_key_id: signing_key_id.clone(),
                            device_e2ee_key_id: e2ee_key_id.clone(),
                        },
                        device_state: Some(IdentityDeviceState {
                            schema_version: IDENTITY_DEVICE_STATE_SCHEMA_VERSION,
                            mode: IdentityDeviceMode::VNext,
                            authorization: Some(DeviceAuthorizationProjection {
                                protocol_device_id: generated.protocol_device_id.clone(),
                                signing_key_id: signing_key_id.clone(),
                                e2ee_key_id: e2ee_key_id.clone(),
                                status: DeviceAuthorizationStatus::Active,
                                role: DeviceAuthorizationRole::Admin,
                                management_ready: true,
                                auth_generation: 1,
                            }),
                            checkpoint: Some(IdentityInternalCheckpoint {
                                document_version: 9,
                                document_hash: "sha256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
                                    .to_owned(),
                                registry_version: 4,
                            }),
                        }),
                        key1_private_pem: generated.root_private_pem.clone(),
                        key1_public_pem: generated.root_public_pem.clone(),
                        e2ee_signing_private_pem: generated.device_signing_private_pem.clone(),
                        e2ee_agreement_private_pem: generated.device_e2ee_private_pem.clone(),
                        daemon_subkey_package: None,
                        make_default: true,
                    },
                    crate::internal::identity_store::SaveIdentitySecretStorage::Vault {
                        workspace_id: "workspace-a".to_owned(),
                        device_id: "vault-a".to_owned(),
                        vault: vault.clone(),
                    },
                )
                .unwrap();
        };

        // First registration, then local deletion: registry
        // entry, identity dir and vault records are removed and a durable
        // Completed tombstone is left behind.
        save_identity();
        let core = open_core();
        core.identities()
            .verify_identity_vault(crate::identity::IdentitySelector::Default)
            .unwrap();
        drop(core);
        let core = open_core();
        core.identities()
            .delete_local_identity(crate::identity::IdentitySelector::Default)
            .unwrap();
        drop(core);
        let store = crate::internal::identity_store::IdentityStore::new(&paths.identities);
        assert!(store.load_index().unwrap().credentials.is_empty());

        // Re-register the same handle: same unique_id and DID, fresh vault
        // records.
        save_identity();
        let core = open_core();
        core.identities()
            .verify_identity_vault(crate::identity::IdentitySelector::Default)
            .unwrap();
        drop(core);

        // Re-open replays the Completed tombstone. The fix must skip vault
        // cleanup for the currently registered identity. Without the fix this
        // open (and every later open) deletes the fresh records and
        // verify_identity_vault fails with identity_vault_record_open_failed.
        for _ in 0..2 {
            let core = open_core();
            core.identities()
                .verify_identity_vault(crate::identity::IdentitySelector::Default)
                .unwrap();
            let runtime = core
                .identities()
                .load_runtime(crate::identity::IdentitySelector::Default)
                .unwrap();
            assert_eq!(
                runtime.key_provider.request_signing_key_id().unwrap(),
                generated.device_signing_key_id
            );
            assert_eq!(
                runtime.key_provider.valid_auth_token().unwrap().as_deref(),
                Some("device-token")
            );
        }

        // The durable tombstone is preserved (so late operations from the
        // retired registration are still covered), but it no longer destroys
        // the live identity's records.
        let retirements = paths
            .identities
            .identity_root_dir
            .join(".identity-retirements");
        let record_paths = std::fs::read_dir(&retirements)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("json"))
            .collect::<Vec<_>>();
        assert_eq!(record_paths.len(), 1);
        let record: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&record_paths[0]).unwrap()).unwrap();
        assert_eq!(record["phase"], json!("completed"));
    }

    #[test]
    fn identity_vault_migrate_and_verify_public_api_use_vault_provider() {
        let root = tempfile::tempdir().unwrap();
        let paths = test_paths(root.path());
        let store = crate::internal::identity_store::IdentityStore::new(&paths.identities);
        let bundle = anp::authentication::create_did_wba_document(
            "vault-migration.example",
            anp::authentication::DidDocumentOptions::default(),
        )
        .unwrap();
        let did = crate::ids::Did::parse(bundle.did().unwrap()).unwrap();
        store
            .save_identity(crate::internal::identity_store::SaveIdentityInput {
                local_alias: "alice".to_owned(),
                did: did.clone(),
                unique_id: "alice-id".to_owned(),
                user_id: "user-1".to_owned(),
                display_name: "Alice".to_owned(),
                handle: "alice".to_owned(),
                full_handle: "alice.example".to_owned(),
                binding_generation: None,
                jwt_token: "jwt-secret-value".to_owned(),
                did_document: Some(bundle.did_document),
                key_mode: crate::internal::identity_store::SaveIdentityKeyMode::LegacyKey1,
                device_state: None,
                key1_private_pem: bundle.keys[anp::authentication::VM_KEY_AUTH]
                    .private_key_pem
                    .clone(),
                key1_public_pem: bundle.keys[anp::authentication::VM_KEY_AUTH]
                    .public_key_pem
                    .clone(),
                e2ee_signing_private_pem: String::new(),
                e2ee_agreement_private_pem: bundle.keys[anp::authentication::VM_KEY_E2EE_AGREEMENT]
                    .private_key_pem
                    .clone(),
                daemon_subkey_package: None,
                make_default: true,
            })
            .unwrap();
        let vault_dir = root.path().join("vault");
        let core = crate::ImCore::new_with_options(
            test_config(),
            paths,
            crate::ImCoreOpenOptions::default().with_identity_secret_vault(
                crate::IdentitySecretStoragePolicy::VaultRequired,
                crate::ImCoreSecretVaultOptions::new(
                    DeviceVaultRootKey::from_bytes([41_u8; 32]),
                    &vault_dir,
                    "workspace-a",
                    "device-a",
                ),
            ),
        )
        .unwrap();

        let report = core
            .identities()
            .migrate_identity_vault(crate::identity::IdentitySelector::LocalAlias(
                "alice".to_owned(),
            ))
            .unwrap();

        assert!(report.migrated);
        assert!(report.verified);
        assert!(report.plaintext_compat_retained);
        assert_eq!(
            report.status.selected_backend,
            crate::identity::IdentitySecretStorageBackend::Vault
        );
        assert_eq!(report.identity.did, did);

        let verification = core
            .identities()
            .verify_identity_vault(crate::identity::IdentitySelector::LocalAlias(
                "alice".to_owned(),
            ))
            .unwrap();
        assert!(verification.verified);
        assert_eq!(
            verification.status.selected_backend,
            crate::identity::IdentitySecretStorageBackend::Vault
        );
    }

    #[test]
    fn anp_identity_file_backend_loads_runtime_without_awiki_private_key_files() {
        let root = tempfile::tempdir().unwrap();
        let paths = test_paths(root.path());
        std::fs::create_dir_all(paths.identities.identity_root_dir.join("alice-id")).unwrap();
        let mut custody = anp_identity::DidStore::initialize_local_file(
            paths.identities.identity_root_dir.join(".anp-identity"),
        )
        .unwrap();
        let identity = custody.create_identity(anp_identity_spec("file")).unwrap();
        let did = identity.did().to_string();
        let store_id = custody.manifest().store_id.clone();
        let identity_id = identity.identity_id().to_string();
        crate::internal::auth::state::persist_jwt_token(
            &paths
                .identities
                .identity_root_dir
                .join("alice-id/auth.json"),
            "file-anp-token",
        )
        .unwrap();
        write_anp_identity_registry(
            &paths.identities.registry_path,
            &did,
            &store_id,
            &identity_id,
            None,
        );
        let core = crate::ImCore::new(test_config(), paths.clone()).unwrap();

        let runtime = core
            .identities()
            .load_runtime(crate::identity::IdentitySelector::Default)
            .unwrap();
        runtime
            .key_provider
            .ensure_request_signing_available()
            .unwrap();
        runtime.key_provider.ensure_agreement_available().unwrap();
        runtime
            .key_provider
            .ensure_root_control_available()
            .unwrap();
        assert_eq!(
            runtime.key_provider.valid_auth_token().unwrap().as_deref(),
            Some("file-anp-token")
        );
        let identity_dir = paths.identities.identity_root_dir.join("alice-id");
        for name in [
            "private.key",
            "key-1-private.pem",
            "e2ee-agreement-private.pem",
        ] {
            assert!(!identity_dir.join(name).exists());
        }
    }

    #[test]
    fn anp_identity_vault_backend_reuses_the_injected_workspace_root_and_auth_ref() {
        let root = tempfile::tempdir().unwrap();
        let paths = test_paths(root.path());
        let vault_dir = root.path().join("vault");
        std::fs::create_dir_all(paths.identities.identity_root_dir.join("alice-id")).unwrap();
        let mut custody = anp_identity::DidStore::initialize_injected(
            paths.identities.identity_root_dir.join(".anp-identity"),
            "awiki-workspace-vault:workspace-a",
            [62_u8; 32],
        )
        .unwrap();
        let identity = custody.create_identity(anp_identity_spec("vault")).unwrap();
        let did = identity.did().to_string();
        let store_id = custody.manifest().store_id.clone();
        let identity_id = identity.identity_id().to_string();
        let vault = FileSecretVault::new(
            DeviceVaultRootKey::from_bytes([62_u8; 32]),
            FileSecretVaultStore::new(&vault_dir),
        );
        let auth_ref = vault
            .seal(SealSecretRequest {
                metadata: test_secret_metadata(
                    "workspace-a",
                    "device-a",
                    "alice-id",
                    &did,
                    SecretKind::AuthJwt,
                    "auth.json",
                ),
                plaintext: SecretBytes::from_vec(
                    crate::internal::auth::state::auth_state_json_for_token("vault-anp-token")
                        .unwrap(),
                ),
            })
            .unwrap();
        write_anp_identity_registry(
            &paths.identities.registry_path,
            &did,
            &store_id,
            &identity_id,
            Some(auth_ref),
        );
        let core = crate::ImCore::new_with_options(
            test_config(),
            paths,
            crate::ImCoreOpenOptions::default().with_identity_secret_vault(
                crate::IdentitySecretStoragePolicy::VaultRequired,
                crate::ImCoreSecretVaultOptions::new(
                    DeviceVaultRootKey::from_bytes([62_u8; 32]),
                    &vault_dir,
                    "workspace-a",
                    "device-a",
                ),
            ),
        )
        .unwrap();

        let runtime = core
            .identities()
            .load_runtime(crate::identity::IdentitySelector::Default)
            .unwrap();
        runtime
            .key_provider
            .ensure_request_signing_available()
            .unwrap();
        runtime.key_provider.ensure_agreement_available().unwrap();
        assert_eq!(
            runtime.key_provider.valid_auth_token().unwrap().as_deref(),
            Some("vault-anp-token")
        );
    }

    #[test]
    fn new_identity_creation_uses_anp_custody_then_commits_only_a_public_projection() {
        let root = tempfile::tempdir().unwrap();
        let paths = test_paths(root.path());
        let core = crate::ImCore::new(test_config(), paths.clone()).unwrap();
        let mut custody = crate::internal::identity_custody::open_controller_store(&core).unwrap();
        let identity = custody
            .create_identity(anp_identity_spec("created"))
            .unwrap();
        let did = crate::ids::Did::parse(identity.did()).unwrap();
        let root_kid = identity
            .keys()
            .iter()
            .find(|key| key.role == anp_identity::KeyRole::RootControl)
            .unwrap()
            .kid
            .clone();
        let device_kid = identity
            .keys()
            .iter()
            .find(|key| key.role == anp_identity::KeyRole::DeviceSigning)
            .unwrap()
            .kid
            .clone();
        let agreement_kid = identity
            .keys()
            .iter()
            .find(|key| key.role == anp_identity::KeyRole::E2eeAgreement)
            .unwrap()
            .kid
            .clone();
        crate::internal::identity_store::IdentityStore::new(&paths.identities)
            .save_anp_identity_projection(
                crate::internal::identity_store::SaveIdentityInput {
                    local_alias: "alice".to_string(),
                    did: did.clone(),
                    unique_id: "created-anp-id".to_string(),
                    user_id: "user-alice".to_string(),
                    display_name: "Alice".to_string(),
                    handle: "alice".to_string(),
                    full_handle: "alice.example.com".to_string(),
                    binding_generation: None,
                    jwt_token: "new-anp-token".to_string(),
                    did_document: Some(identity.document().clone()),
                    key_mode: crate::internal::identity_store::SaveIdentityKeyMode::VNext {
                        root_key_id: root_kid,
                        device_signing_key_id: device_kid,
                        device_e2ee_key_id: agreement_kid,
                    },
                    device_state: None,
                    key1_private_pem: String::new(),
                    key1_public_pem: String::new(),
                    e2ee_signing_private_pem: String::new(),
                    e2ee_agreement_private_pem: String::new(),
                    daemon_subkey_package: None,
                    make_default: true,
                },
                crate::internal::identity_store::AnpIdentityProjectionStorage::from_core(
                    &core,
                    custody.manifest().store_id.clone(),
                    identity.identity_id().to_string(),
                )
                .unwrap(),
            )
            .unwrap();

        let runtime = core
            .identities()
            .load_runtime(crate::identity::IdentitySelector::Default)
            .unwrap();
        runtime
            .key_provider
            .ensure_request_signing_available()
            .unwrap();
        runtime.key_provider.ensure_agreement_available().unwrap();
        assert_eq!(runtime.summary.did, did);
        assert!(
            std::fs::read_dir(paths.identities.identity_root_dir.join("created-anp-id"))
                .unwrap()
                .all(|entry| {
                    !entry
                        .unwrap()
                        .file_name()
                        .to_string_lossy()
                        .contains("private")
                })
        );
    }

    #[test]
    fn vault_required_without_vault_options_fails_closed() {
        let root = tempfile::tempdir().unwrap();
        let err = match crate::ImCore::new_with_options(
            test_config(),
            test_paths(root.path()),
            crate::ImCoreOpenOptions {
                identity_secret_storage_policy: crate::IdentitySecretStoragePolicy::VaultRequired,
                identity_secret_vault: None,
                multi_device_device_revoke_enabled: false,
                multi_device_direct_e2ee_enabled: false,
                multi_device_group_e2ee_enabled: false,
                multi_device_handle_recovery_enabled: false,
                multi_device_audience: None,
            },
        ) {
            Ok(_) => panic!("VaultRequired without vault options should fail"),
            Err(err) => err,
        };

        let message = err.to_string();
        assert!(message.contains("VaultRequired"));
        assert!(!message.contains("root key"));
    }

    fn write_anp_identity_registry(
        path: &Path,
        did: &str,
        store_id: &str,
        identity_id: &str,
        auth_ref: Option<crate::internal::secret_vault::record::SecretRef>,
    ) {
        std::fs::write(
            path,
            serde_json::to_vec_pretty(&json!({
                "schema_version": 5,
                "default_credential_name": "alice",
                "credentials": {
                    "alice": {
                        "credential_name": "alice",
                        "dir_name": "alice-id",
                        "did": did,
                        "unique_id": "alice-id",
                        "user_id": "user-alice",
                        "name": "Alice",
                        "handle": "alice",
                        "full_handle": "alice.example.com",
                        "is_default": true,
                        "identity_custody_backend": "anp_identity",
                        "anp_identity_store_id": store_id,
                        "anp_identity_id": identity_id,
                        "anp_identity_auth_ref": auth_ref,
                    }
                }
            }))
            .unwrap(),
        )
        .unwrap();
    }

    fn anp_identity_spec(name: &str) -> anp_identity::DidCreateSpec {
        anp_identity::DidCreateSpec {
            profile: anp_identity::DidProfile::E1,
            domain: "example.com".to_string(),
            port: None,
            path_segments: vec!["awiki".to_string(), name.to_string()],
            capabilities: anp_identity::Capabilities { did_wba: true },
            managed_keys: vec![
                anp_identity::ManagedKeySpec {
                    fragment: "root".to_string(),
                    role: anp_identity::KeyRole::RootControl,
                },
                anp_identity::ManagedKeySpec {
                    fragment: "device".to_string(),
                    role: anp_identity::KeyRole::DeviceSigning,
                },
                anp_identity::ManagedKeySpec {
                    fragment: "agreement".to_string(),
                    role: anp_identity::KeyRole::E2eeAgreement,
                },
            ],
            external_keys: Vec::new(),
            services: Vec::new(),
            agent_description_url: None,
            extensions: vec![anp_identity::DidExtensionSpec::DeviceManifest(
                anp_identity::DeviceManifestSpec {
                    devices: vec![anp_identity::DeviceManifestEntrySpec {
                        device_id: "device-a".to_string(),
                        signing_key_id: "#device".to_string(),
                        e2ee_key_id: "#agreement".to_string(),
                        profiles: vec!["anp.core.binding.v1".to_string()],
                    }],
                },
            )],
        }
    }

    fn assert_registry_error_contains(err: crate::ImError, expected: &str) {
        match err {
            crate::ImError::InvalidInput {
                field: Some(field),
                message,
            } => {
                assert_eq!(field, "identity_registry");
                assert!(
                    message.contains(expected),
                    "expected `{message}` to contain `{expected}`"
                );
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    fn test_secret_metadata(
        workspace_id: &str,
        device_id: &str,
        identity_id: &str,
        did: &str,
        kind: SecretKind,
        key_id: &str,
    ) -> SecretMetadata {
        SecretMetadata {
            workspace_id: workspace_id.to_owned(),
            device_id: device_id.to_owned(),
            identity_id: Some(identity_id.to_owned()),
            did: Some(did.to_owned()),
            kind,
            key_id: key_id.to_owned(),
            key_version: 1,
            policy: SecretAccessPolicy::no_prompt_local_secret(),
        }
    }

    fn test_config() -> crate::ImCoreConfig {
        crate::ImCoreConfig {
            service_base_url: crate::ServiceEndpoint::parse("https://example.test").unwrap(),
            did_domain: "awiki.test".to_owned(),
            client_version_info: None,
            user_service_endpoint: None,
            message_service_endpoint: None,
            mail_service_endpoint: None,
            anp_service_endpoint: None,
            anp_service_did: None,
            ca_bundle: None,
            transport_policy: crate::MessageTransportPolicy::HttpOnly,
        }
    }

    fn test_paths(root: &Path) -> crate::ImCorePaths {
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
}
