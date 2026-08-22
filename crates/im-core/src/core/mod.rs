use std::sync::Arc;

#[cfg(feature = "sqlite")]
use tokio::sync::OnceCell;

mod bootstrap;
mod client;
pub(crate) mod options;

pub use self::bootstrap::{
    CoreBootstrap, LocalStateStatus, MigrationReport, PathCheck, PathValidationReport,
};
pub use self::client::ImClient;
#[cfg(feature = "provider-traits")]
pub use self::options::IdentityCustodyProvider;
pub use self::options::{IdentitySecretStoragePolicy, ImCoreOpenOptions, ImCoreSecretVaultOptions};

pub(crate) struct ImCoreInner {
    pub(crate) sdk_config: crate::ImCoreConfig,
    pub(crate) sdk_paths: crate::ImCorePaths,
    pub(crate) identity_secret_storage_policy: IdentitySecretStoragePolicy,
    pub(crate) identity_vault: Option<options::IdentityVaultContext>,
    pub(crate) device_join_lock: std::sync::Mutex<()>,
    pub(crate) device_revoke_enabled: bool,
    pub(crate) direct_e2ee_v2_enabled: bool,
    pub(crate) device_revoke_lock: tokio::sync::Mutex<()>,
    pub(crate) group_e2ee_v2_enabled: bool,
    pub(crate) handle_recovery_enabled: bool,
    pub(crate) multi_device_audience: Option<String>,
    #[cfg(feature = "provider-traits")]
    pub(crate) identity_custody_provider: Option<IdentityCustodyProvider>,
    pub(crate) handle_recovery_locks: std::sync::Mutex<
        std::collections::HashMap<String, std::sync::Weak<tokio::sync::Mutex<()>>>,
    >,
    pub(crate) direct_rebind_locks: std::sync::Mutex<
        std::collections::HashMap<String, std::sync::Weak<tokio::sync::Mutex<()>>>,
    >,
    pub(crate) device_join_approvals:
        crate::internal::identity_device_join_runtime::DeviceJoinApprovalHandleStore,
    pub(crate) registration_join_preparations:
        crate::internal::identity_registration_join_preparation::RegistrationJoinPreparationStore,
    pub(crate) root_key_transfer_authorizations:
        crate::internal::identity_root_transfer_runtime::RootKeyTransferAuthorizationStore,
    #[cfg(feature = "sqlite")]
    pub(crate) local_state_db: OnceCell<crate::internal::local_state::actor::LocalStateDb>,
}

#[derive(Clone)]
pub struct ImCore {
    inner: Arc<ImCoreInner>,
}

impl ImCore {
    pub async fn open(
        sdk_config: crate::ImCoreConfig,
        sdk_paths: crate::ImCorePaths,
    ) -> crate::ImResult<Self> {
        Self::new(sdk_config, sdk_paths)
    }

    pub async fn open_with_options(
        sdk_config: crate::ImCoreConfig,
        sdk_paths: crate::ImCorePaths,
        options: ImCoreOpenOptions,
    ) -> crate::ImResult<Self> {
        Self::new_with_options(sdk_config, sdk_paths, options)
    }

    pub fn new(
        sdk_config: crate::ImCoreConfig,
        sdk_paths: crate::ImCorePaths,
    ) -> crate::ImResult<Self> {
        Self::new_with_options(sdk_config, sdk_paths, ImCoreOpenOptions::default())
    }

    pub fn new_with_options(
        sdk_config: crate::ImCoreConfig,
        sdk_paths: crate::ImCorePaths,
        options: ImCoreOpenOptions,
    ) -> crate::ImResult<Self> {
        if sdk_config.did_domain.trim().is_empty() {
            return Err(crate::ImError::invalid_input(
                Some("did_domain".to_string()),
                "DID domain must not be empty",
            ));
        }
        let identity_vault = options
            .identity_secret_vault
            .map(options::IdentityVaultContext::from_options)
            .transpose()?
            .map(|context| context.with_policy(options.identity_secret_storage_policy));
        if matches!(
            options.identity_secret_storage_policy,
            IdentitySecretStoragePolicy::VaultRequired
        ) && identity_vault.is_none()
        {
            return Err(crate::ImError::LocalStateUnavailable {
                detail: "identity secret storage policy is VaultRequired but no identity secret vault was provided".to_owned(),
            });
        }
        let multi_device_audience = options.multi_device_audience.filter(|audience| {
            !audience.is_empty() && audience == audience.trim() && audience.chars().count() <= 255
        });
        #[cfg(feature = "provider-traits")]
        let identity_custody_provider = options.identity_custody_provider;
        if options.multi_device_handle_recovery_enabled && multi_device_audience.is_none() {
            return Err(crate::ImError::invalid_input(
                Some("multi_device_audience".to_owned()),
                "Handle Recovery requires the explicit User Service multi-device audience",
            ));
        }
        let core = Self {
            inner: Arc::new(ImCoreInner {
                sdk_config,
                sdk_paths,
                identity_secret_storage_policy: options.identity_secret_storage_policy,
                identity_vault,
                device_join_lock: std::sync::Mutex::new(()),
                device_revoke_enabled: options.multi_device_device_revoke_enabled,
                direct_e2ee_v2_enabled: options.multi_device_direct_e2ee_enabled,
                device_revoke_lock: tokio::sync::Mutex::new(()),
                group_e2ee_v2_enabled: options.multi_device_group_e2ee_enabled,
                handle_recovery_enabled: options.multi_device_handle_recovery_enabled,
                multi_device_audience,
                #[cfg(feature = "provider-traits")]
                identity_custody_provider,
                handle_recovery_locks: std::sync::Mutex::new(std::collections::HashMap::new()),
                direct_rebind_locks: std::sync::Mutex::new(std::collections::HashMap::new()),
                device_join_approvals: Default::default(),
                registration_join_preparations: Default::default(),
                root_key_transfer_authorizations: Default::default(),
                #[cfg(feature = "sqlite")]
                local_state_db: OnceCell::new(),
            }),
        };
        crate::internal::identity_retirement::recover_all(&core)?;
        Ok(core)
    }

    pub fn identities(&self) -> crate::identity::IdentityRegistry<'_> {
        crate::identity::IdentityRegistry::new(self)
    }

    pub fn device_join(&self) -> crate::identity::DeviceJoinService<'_> {
        crate::identity::DeviceJoinService::new(self)
    }

    pub fn device_revoke(&self) -> crate::identity::DeviceRevokeService<'_> {
        crate::identity::DeviceRevokeService::new(self)
    }

    pub fn handle_recovery(&self) -> crate::identity::HandleRecoveryService<'_> {
        crate::identity::HandleRecoveryService::new(self)
    }

    pub fn bootstrap(&self) -> CoreBootstrap<'_> {
        CoreBootstrap::new(self)
    }

    pub fn onboarding(&self) -> crate::onboarding::SkillOnboardingService<'_> {
        crate::onboarding::SkillOnboardingService::new(self)
    }

    /// Builds one independent vNext Agent DID with exactly one bootstrap
    /// device. The returned secret material is intended for immediate transfer
    /// into a trusted host's SecretVault-backed pending record.
    pub fn generate_vnext_agent_bootstrap(
        &self,
        kind: crate::identity::AgentIdentityKind,
        handle_local_part: &str,
    ) -> crate::ImResult<crate::identity::VNextAgentBootstrapMaterial> {
        let local_part = canonical_agent_handle_local_part(handle_local_part)?;
        let generated = crate::internal::identity_generation::generate_vnext_agent_handle_identity(
            &self.inner.sdk_config().did_domain,
            kind,
            &local_part,
            self.inner.sdk_config().anp_service_endpoint.as_ref(),
            self.inner.sdk_config().anp_service_did.as_ref(),
        )?;
        vnext_agent_bootstrap_material(kind, local_part, generated)
    }

    /// Prepares a same-DID vNext target for an existing Legacy Agent.
    ///
    /// The old root key and Handle service are verified and preserved. Only a
    /// fresh bootstrap device signing/E2EE key pair and random device ID are
    /// added. This function does not perform the remote `update_document` or
    /// persist any secret material.
    pub fn prepare_vnext_agent_legacy_upgrade(
        &self,
        kind: crate::identity::AgentIdentityKind,
        handle_local_part: &str,
        legacy_did_document: serde_json::Value,
        root_private_key_pem: String,
    ) -> crate::ImResult<crate::identity::VNextAgentBootstrapMaterial> {
        let local_part = canonical_agent_handle_local_part(handle_local_part)?;
        validate_preserved_agent_handle(
            &legacy_did_document,
            kind,
            &local_part,
            &self.inner.sdk_config().did_domain,
        )?;
        let generated = crate::internal::identity_legacy_upgrade::build_legacy_upgrade(
            &legacy_did_document,
            &root_private_key_pem,
        )?;
        let root_private = anp::PrivateKeyMaterial::from_pem(&root_private_key_pem)
            .map_err(|_| crate::ImError::PermissionDenied)?;
        let signing_private = anp::PrivateKeyMaterial::from_pem(&generated.signing_private_pem)
            .map_err(|_| crate::ImError::PermissionDenied)?;
        let e2ee_private = anp::PrivateKeyMaterial::from_pem(&generated.e2ee_private_pem)
            .map_err(|_| crate::ImError::PermissionDenied)?;
        let did = generated.did;
        let identity_id = did
            .as_str()
            .rsplit(':')
            .next()
            .filter(|value| !value.is_empty())
            .unwrap_or(did.as_str())
            .to_owned();
        Ok(crate::identity::VNextAgentBootstrapMaterial {
            kind,
            handle_local_part: local_part,
            identity_id,
            root_key_id: format!("{}#key-1", did.as_str()),
            root_public_key_pem: root_private.public_key().to_pem(),
            root_private_key_pem,
            device_signing_key_id: generated.signing_key_id,
            device_signing_public_key_pem: signing_private.public_key().to_pem(),
            device_signing_private_key_pem: generated.signing_private_pem,
            device_e2ee_key_id: generated.e2ee_key_id,
            device_e2ee_public_key_pem: e2ee_private.public_key().to_pem(),
            device_e2ee_private_key_pem: generated.e2ee_private_pem,
            did,
            did_document: generated.target_document,
            document_hash: generated.target_document_hash,
            protocol_device_id: generated.protocol_device_id,
        })
    }

    /// Reconciles a crash-interrupted same-DID Agent Legacy upgrade without
    /// issuing a remote mutation. A committed target is reported explicitly;
    /// a still-Legacy remote document rebuilds the target with the exact same
    /// device identity and private keys but a fresh root proof and extensions.
    pub fn reconcile_vnext_agent_legacy_upgrade(
        &self,
        source_legacy_did_document: &serde_json::Value,
        mut target: crate::identity::VNextAgentBootstrapMaterial,
        remote_did_document: &serde_json::Value,
        root_private_key_pem: &str,
    ) -> crate::ImResult<crate::identity::VNextAgentLegacyUpgradeReconciliation> {
        validate_vnext_agent_legacy_target(self, &target)?;
        validate_preserved_agent_handle(
            source_legacy_did_document,
            target.kind,
            &target.handle_local_part,
            &self.inner.sdk_config().did_domain,
        )?;
        let root_private = anp::PrivateKeyMaterial::from_pem(root_private_key_pem)
            .map_err(|_| crate::ImError::PermissionDenied)?;
        if root_private.public_key().to_pem() != target.root_public_key_pem {
            return Err(crate::ImError::PermissionDenied);
        }

        let generated = legacy_upgrade_generated_from_target(&target);
        let mut source_rebuilt = generated.clone();
        crate::internal::identity_legacy_upgrade::rebuild_legacy_upgrade_target(
            &mut source_rebuilt,
            source_legacy_did_document,
            root_private_key_pem,
        )?;
        if document_without_proof(&source_rebuilt.target_document)
            != document_without_proof(&target.did_document)
        {
            return Err(crate::ImError::PermissionDenied);
        }

        let remote_hash =
            crate::internal::identity_wire::document::document_hash(remote_did_document)?;
        if remote_hash == target.document_hash && remote_did_document == &target.did_document {
            return Ok(crate::identity::VNextAgentLegacyUpgradeReconciliation::TargetCommitted);
        }
        validate_preserved_agent_handle(
            remote_did_document,
            target.kind,
            &target.handle_local_part,
            &self.inner.sdk_config().did_domain,
        )?;
        let mut rebuilt = generated;
        crate::internal::identity_legacy_upgrade::rebuild_legacy_upgrade_target(
            &mut rebuilt,
            remote_did_document,
            root_private_key_pem,
        )?;
        target.did_document = rebuilt.target_document;
        target.document_hash = rebuilt.target_document_hash;
        validate_vnext_agent_legacy_target(self, &target)?;
        Ok(crate::identity::VNextAgentLegacyUpgradeReconciliation::LegacyRebuilt { target })
    }

    /// Recovers an exact bootstrap-device session after the reconciler proves
    /// that the target document committed remotely. This performs only signed
    /// `get_me`, Registry validation and authoritative Handle lookup; it never
    /// calls `update_document`.
    pub async fn refresh_committed_vnext_agent_legacy_upgrade_session(
        &self,
        target: &crate::identity::VNextAgentBootstrapMaterial,
    ) -> crate::ImResult<crate::identity::VNextAgentLegacyUpgradeSession> {
        validate_vnext_agent_legacy_target(self, target)?;
        let full_handle = format!(
            "{}.{}",
            target.handle_local_part,
            self.inner.sdk_config().did_domain
        );
        let client = self.client_with_identity_material_and_signing_key_id(
            crate::identity::HostedIdentityMaterial {
                identity_id: target.identity_id.clone(),
                did: target.did.as_str().to_owned(),
                handle: Some(full_handle.clone()),
                display_name: Some("AWiki Agent".to_owned()),
                did_document: target.did_document.clone(),
                default_signing_private_key_pem: target.device_signing_private_key_pem.clone(),
                e2ee_agreement_private_key_pem: Some(target.device_e2ee_private_key_pem.clone()),
                auth_token: None,
            },
            &target.device_signing_key_id,
        )?;
        let mut transport = crate::internal::transport::CoreHttpTransport::new_pending_device(
            &client,
            client.runtime().key_provider.clone(),
            crate::internal::transport::ExpectedDeviceAccessOwned {
                did: target.did.as_str().to_owned(),
                user_id: String::new(),
                device_id: target.protocol_device_id.as_str().to_owned(),
                key_id: target.device_signing_key_id.clone(),
                auth_generation: 1,
                role: crate::internal::identity_device_state::DeviceAuthorizationRole::Admin,
                management_ready: true,
            },
        );
        let access_token = transport.refresh_jwt_async().await?;
        let user_id = transport.pending_device_user_id()?;
        let registry_call =
            crate::internal::identity_wire::device_join::build_registry_call(&target.did, false);
        let raw = crate::internal::transport::AsyncAuthenticatedRpcTransport::authenticated_rpc(
            &mut transport,
            registry_call.endpoint,
            registry_call.method,
            registry_call.params,
        )
        .await?;
        validate_vnext_agent_legacy_registry(target, raw)?;
        let lookup = crate::internal::handle_discovery::resolve_authoritative_handle_binding_async(
            &client,
            &full_handle,
        )
        .await?;
        if lookup.did != target.did {
            return Err(crate::ImError::PermissionDenied);
        }
        let binding_generation = lookup
            .binding_generation
            .ok_or(crate::ImError::PermissionDenied)?;
        let canonical_generation = anp::wns::BindingGeneration::new(binding_generation.clone())
            .map_err(|_| crate::ImError::PermissionDenied)?;
        if canonical_generation.to_string() != binding_generation {
            return Err(crate::ImError::PermissionDenied);
        }
        Ok(crate::identity::VNextAgentLegacyUpgradeSession {
            did: target.did.clone(),
            user_id,
            binding_generation,
            access_token,
        })
    }

    pub fn client(&self, selector: crate::identity::IdentitySelector) -> crate::ImResult<ImClient> {
        let runtime = self.identities().load_runtime(selector)?;
        Ok(ImClient::new(self.inner.clone(), runtime))
    }

    pub async fn client_async(
        &self,
        selector: crate::identity::IdentitySelector,
    ) -> crate::ImResult<ImClient> {
        let runtime = self.identities().load_runtime_async(selector).await?;
        let client = ImClient::new(self.inner.clone(), runtime);
        if self.inner().device_revoke_enabled() {
            let _ =
                crate::internal::identity_device_revoke::recover_pending_for_client(self, &client)
                    .await;
        }
        Ok(client)
    }

    pub fn client_with_identity_material(
        &self,
        material: crate::identity::HostedIdentityMaterial,
    ) -> crate::ImResult<ImClient> {
        self.client_with_identity_material_inner(material, None)
    }

    /// Creates an exact-device client from material held by a trusted host.
    ///
    /// The material is accepted only when the canonical vNext document,
    /// Manifest, private keys and Device Access claims form one exact active
    /// ready-admin authorization. Ordinary Hosted/Legacy material continues to
    /// use [`Self::client_with_identity_material`] and receives no sync binding.
    pub fn client_with_device_identity_material(
        &self,
        material: crate::identity::HostBackedDeviceIdentityMaterial,
    ) -> crate::ImResult<ImClient> {
        self.client_with_device_identity_material_inner(material, None)
    }

    #[cfg(feature = "identity-native-anp")]
    pub(crate) fn client_with_pending_anp_identity(
        &self,
        identity: anp_identity::ManagedIdentity,
        handle: Option<&str>,
        display_name: &str,
        protocol_device_id: &crate::ids::ProtocolDeviceId,
    ) -> crate::ImResult<ImClient> {
        let public = identity
            .public_identity()
            .map_err(crate::internal::identity_custody::map_facade_error)?;
        let did = crate::ids::Did::parse(&public.reference.did)?;
        let identity_id = crate::ids::IdentityId::parse(
            did.as_str()
                .rsplit(':')
                .next()
                .filter(|value| !value.is_empty())
                .ok_or(crate::ImError::PermissionDenied)?,
        )?;
        let handle = handle
            .map(|handle| crate::ids::Handle::parse(handle, &self.inner.sdk_config().did_domain))
            .transpose()?;
        let provider: std::sync::Arc<dyn crate::internal::key_provider::IdentitySigner> =
            std::sync::Arc::new(
                crate::internal::key_provider::AnpIdentitySigner::new_ephemeral(identity),
            );
        let identity_session = provider.async_session();
        let runtime = crate::internal::identity_runtime::ClientIdentityRuntime {
            summary: crate::identity::IdentitySummary {
                id: identity_id.clone(),
                did: did.clone(),
                handle,
                display_name: Some(display_name.to_owned()),
                local_alias: None,
                device_id: Some(protocol_device_id.as_str().to_owned()),
                is_default: false,
                readiness: crate::identity::IdentityReadiness {
                    ready_for_auth: true,
                    ready_for_messaging: false,
                    missing: Vec::new(),
                },
            },
            did_document_path: std::path::PathBuf::new(),
            private_key_path: std::path::PathBuf::new(),
            e2ee_agreement_private_key_path: std::path::PathBuf::new(),
            auth_state_path: std::path::PathBuf::new(),
            key_provider: provider,
            identity_session,
            owner: crate::internal::identity_runtime::LocalOwnerContext {
                identity_id,
                current_did: did,
                sync_account: None,
            },
        };
        Ok(ImClient::new(self.inner.clone(), runtime))
    }

    #[cfg(feature = "identity-native-anp")]
    #[doc(hidden)]
    pub fn client_with_anp_delegated_identity(
        &self,
        identity: anp_identity::ManagedIdentity,
    ) -> crate::ImResult<ImClient> {
        let public = identity
            .public_identity()
            .map_err(crate::internal::identity_custody::map_facade_error)?;
        if public.state != anp_identity::PublicIdentityState::Active {
            return Err(crate::ImError::PermissionDenied);
        }
        let did = crate::ids::Did::parse(&public.reference.did)?;
        let identity_id = crate::ids::IdentityId::parse(
            did.as_str()
                .rsplit(':')
                .next()
                .filter(|value| !value.is_empty())
                .ok_or(crate::ImError::PermissionDenied)?,
        )?;
        let provider: std::sync::Arc<dyn crate::internal::key_provider::IdentitySigner> =
            std::sync::Arc::new(
                crate::internal::key_provider::AnpIdentitySigner::new_ephemeral(identity),
            );
        provider.ensure_request_signing_available()?;
        let identity_session = provider.async_session();
        let runtime = crate::internal::identity_runtime::ClientIdentityRuntime {
            summary: crate::identity::IdentitySummary {
                id: identity_id.clone(),
                did: did.clone(),
                handle: None,
                display_name: None,
                local_alias: None,
                device_id: None,
                is_default: false,
                readiness: crate::identity::IdentityReadiness {
                    ready_for_auth: true,
                    ready_for_messaging: false,
                    missing: Vec::new(),
                },
            },
            did_document_path: std::path::PathBuf::new(),
            private_key_path: std::path::PathBuf::new(),
            e2ee_agreement_private_key_path: std::path::PathBuf::new(),
            auth_state_path: std::path::PathBuf::new(),
            key_provider: provider,
            identity_session,
            owner: crate::internal::identity_runtime::LocalOwnerContext {
                identity_id,
                current_did: did,
                sync_account: None,
            },
        };
        Ok(ImClient::new(self.inner.clone(), runtime))
    }

    /// Creates an exact-device client whose validated replacement access tokens
    /// are durably committed by the trusted host.
    pub fn client_with_device_identity_material_and_auth_persistence(
        &self,
        material: crate::identity::HostBackedDeviceIdentityMaterial,
        auth_token_persistence: std::sync::Arc<dyn crate::identity::HostBackedAuthTokenPersistence>,
    ) -> crate::ImResult<ImClient> {
        self.client_with_device_identity_material_inner(material, Some(auth_token_persistence))
    }

    fn client_with_device_identity_material_inner(
        &self,
        material: crate::identity::HostBackedDeviceIdentityMaterial,
        auth_token_persistence: Option<
            std::sync::Arc<dyn crate::identity::HostBackedAuthTokenPersistence>,
        >,
    ) -> crate::ImResult<ImClient> {
        let identity_id = crate::ids::IdentityId::parse(&material.identity_id)?;
        let did = crate::ids::Did::parse(&material.did)?;
        let expected_identity_id = did
            .as_str()
            .rsplit(':')
            .next()
            .filter(|value| !value.is_empty())
            .ok_or(crate::ImError::PermissionDenied)?;
        if identity_id.as_str() != expected_identity_id {
            return Err(crate::ImError::PermissionDenied);
        }
        let handle = material
            .handle
            .as_deref()
            .map(|handle| crate::ids::Handle::parse(handle, &self.inner.sdk_config().did_domain))
            .transpose()?
            .ok_or(crate::ImError::PermissionDenied)?;
        validate_handle_service_for_did(&material.did_document, &did, &handle)?;
        let protocol_device_id = material.protocol_device_id.clone();
        let account_id = material.account_id.clone();
        let binding_generation = material.binding_generation.clone();
        let auth_generation_number = material
            .auth_generation
            .parse::<u64>()
            .ok()
            .filter(|generation| *generation > 0)
            .filter(|generation| generation.to_string() == material.auth_generation)
            .ok_or(crate::ImError::PermissionDenied)?;
        let device_signing_key_id = material.device_signing_key_id.clone();
        let device_e2ee_key_id = material.device_e2ee_key_id.clone();
        let display_name = material.display_name.clone();
        let key_provider: std::sync::Arc<dyn crate::internal::key_provider::IdentitySigner> = std::sync::Arc::new(
            crate::internal::key_provider::HostBackedDeviceIdentitySigner::new_with_auth_token_persistence(
                &material,
                auth_token_persistence,
            )?,
        );
        let identity_session = key_provider.async_session();
        let runtime = crate::internal::identity_runtime::ClientIdentityRuntime {
            summary: crate::identity::IdentitySummary {
                id: identity_id.clone(),
                did: did.clone(),
                handle: Some(handle),
                display_name,
                local_alias: None,
                device_id: Some(protocol_device_id.as_str().to_owned()),
                is_default: false,
                readiness: crate::identity::IdentityReadiness {
                    ready_for_auth: true,
                    ready_for_messaging: true,
                    missing: Vec::new(),
                },
            },
            did_document_path: std::path::PathBuf::new(),
            private_key_path: std::path::PathBuf::new(),
            e2ee_agreement_private_key_path: std::path::PathBuf::new(),
            auth_state_path: std::path::PathBuf::new(),
            key_provider,
            identity_session,
            owner: crate::internal::identity_runtime::LocalOwnerContext {
                identity_id,
                current_did: did,
                sync_account: Some(crate::internal::identity_runtime::SyncAccountSeed::new(
                    account_id,
                    protocol_device_id,
                    Some(binding_generation),
                    auth_generation_number.to_string(),
                    device_signing_key_id,
                    device_e2ee_key_id,
                    crate::internal::identity_device_state::DeviceAuthorizationRole::Admin,
                    true,
                )),
            },
        };
        Ok(ImClient::new(self.inner.clone(), runtime))
    }

    pub(crate) fn client_with_identity_material_and_signing_key_id(
        &self,
        material: crate::identity::HostedIdentityMaterial,
        request_signing_key_id: &str,
    ) -> crate::ImResult<ImClient> {
        self.client_with_identity_material_inner(material, Some(request_signing_key_id))
    }

    fn client_with_identity_material_inner(
        &self,
        material: crate::identity::HostedIdentityMaterial,
        request_signing_key_id: Option<&str>,
    ) -> crate::ImResult<ImClient> {
        let identity_id = crate::ids::IdentityId::parse(&material.identity_id)?;
        let did = crate::ids::Did::parse(&material.did)?;
        let handle = material
            .handle
            .as_deref()
            .map(|handle| crate::ids::Handle::parse(handle, &self.inner.sdk_config().did_domain))
            .transpose()?;
        let key_provider: std::sync::Arc<dyn crate::internal::key_provider::IdentitySigner> = std::sync::Arc::new(match request_signing_key_id {
            Some(key_id) => {
                crate::internal::key_provider::HostedIdentitySigner::new_for_request_signing_key(
                    &material, key_id,
                )?
            }
            None => crate::internal::key_provider::HostedIdentitySigner::new(&material)?,
        });
        let identity_session = key_provider.async_session();
        let runtime = crate::internal::identity_runtime::ClientIdentityRuntime {
            summary: crate::identity::IdentitySummary {
                id: identity_id.clone(),
                did: did.clone(),
                handle,
                display_name: material.display_name,
                local_alias: None,
                device_id: None,
                is_default: false,
                readiness: crate::identity::IdentityReadiness {
                    ready_for_auth: true,
                    ready_for_messaging: true,
                    missing: Vec::new(),
                },
            },
            did_document_path: std::path::PathBuf::new(),
            private_key_path: std::path::PathBuf::new(),
            e2ee_agreement_private_key_path: std::path::PathBuf::new(),
            auth_state_path: std::path::PathBuf::new(),
            key_provider,
            identity_session,
            owner: crate::internal::identity_runtime::LocalOwnerContext {
                identity_id,
                current_did: did,
                sync_account: None,
            },
        };
        Ok(ImClient::new(self.inner.clone(), runtime))
    }

    pub(crate) fn inner(&self) -> &ImCoreInner {
        &self.inner
    }
}

fn legacy_upgrade_generated_from_target(
    target: &crate::identity::VNextAgentBootstrapMaterial,
) -> crate::internal::identity_legacy_upgrade::GeneratedLegacyUpgrade {
    crate::internal::identity_legacy_upgrade::GeneratedLegacyUpgrade {
        did: target.did.clone(),
        protocol_device_id: target.protocol_device_id.clone(),
        signing_key_id: target.device_signing_key_id.clone(),
        signing_private_pem: target.device_signing_private_key_pem.clone(),
        e2ee_key_id: target.device_e2ee_key_id.clone(),
        e2ee_private_pem: target.device_e2ee_private_key_pem.clone(),
        target_document: target.did_document.clone(),
        target_document_hash: target.document_hash.clone(),
    }
}

fn document_without_proof(document: &serde_json::Value) -> serde_json::Value {
    let mut document = document.clone();
    if let Some(object) = document.as_object_mut() {
        object.remove("proof");
    }
    document
}

fn validate_vnext_agent_legacy_target(
    core: &ImCore,
    target: &crate::identity::VNextAgentBootstrapMaterial,
) -> crate::ImResult<()> {
    let canonical_local_part = canonical_agent_handle_local_part(&target.handle_local_part)?;
    let expected_identity_id = target
        .did
        .as_str()
        .rsplit(':')
        .next()
        .filter(|value| !value.is_empty())
        .ok_or(crate::ImError::PermissionDenied)?;
    let document_hash =
        crate::internal::identity_wire::document::document_hash(&target.did_document)?;
    let expected_root_key_id = format!("{}#key-1", target.did.as_str());
    let expected_signing_key_id = format!(
        "{}#{}-sign",
        target.did.as_str(),
        target.protocol_device_id.as_str()
    );
    let expected_e2ee_key_id = format!(
        "{}#{}-e2ee",
        target.did.as_str(),
        target.protocol_device_id.as_str()
    );
    if canonical_local_part != target.handle_local_part
        || target.identity_id != expected_identity_id
        || target
            .did_document
            .get("id")
            .and_then(serde_json::Value::as_str)
            != Some(target.did.as_str())
        || document_hash != target.document_hash
        || target.root_key_id != expected_root_key_id
        || target.device_signing_key_id != expected_signing_key_id
        || target.device_e2ee_key_id != expected_e2ee_key_id
        || !anp::authentication::validate_did_document_binding(&target.did_document, true)
    {
        return Err(crate::ImError::PermissionDenied);
    }
    validate_preserved_agent_handle(
        &target.did_document,
        target.kind,
        &target.handle_local_part,
        &core.inner.sdk_config().did_domain,
    )?;
    let manifest = anp::authentication::validate_device_manifest(&target.did_document)
        .map_err(|_| crate::ImError::PermissionDenied)?
        .ok_or(crate::ImError::PermissionDenied)?;
    if manifest.devices.len() != 1
        || manifest.devices[0].device_id != target.protocol_device_id.as_str()
        || manifest.devices[0].signing_key_id != target.device_signing_key_id
        || manifest.devices[0].e2ee_key_id != target.device_e2ee_key_id
    {
        return Err(crate::ImError::PermissionDenied);
    }
    validate_bootstrap_private_key(
        &target.did_document,
        &target.root_key_id,
        &target.root_private_key_pem,
        &target.root_public_key_pem,
        BootstrapPrivateKeyRole::Signing,
    )?;
    validate_bootstrap_private_key(
        &target.did_document,
        &target.device_signing_key_id,
        &target.device_signing_private_key_pem,
        &target.device_signing_public_key_pem,
        BootstrapPrivateKeyRole::Signing,
    )?;
    validate_bootstrap_private_key(
        &target.did_document,
        &target.device_e2ee_key_id,
        &target.device_e2ee_private_key_pem,
        &target.device_e2ee_public_key_pem,
        BootstrapPrivateKeyRole::Agreement,
    )
}

#[derive(Clone, Copy)]
enum BootstrapPrivateKeyRole {
    Signing,
    Agreement,
}

fn validate_bootstrap_private_key(
    document: &serde_json::Value,
    key_id: &str,
    private_key_pem: &str,
    expected_public_key_pem: &str,
    role: BootstrapPrivateKeyRole,
) -> crate::ImResult<()> {
    let methods = document
        .get("verificationMethod")
        .and_then(serde_json::Value::as_array)
        .ok_or(crate::ImError::PermissionDenied)?;
    let matching = methods
        .iter()
        .filter(|method| method.get("id").and_then(serde_json::Value::as_str) == Some(key_id))
        .collect::<Vec<_>>();
    if matching.len() != 1 {
        return Err(crate::ImError::PermissionDenied);
    }
    let private = anp::PrivateKeyMaterial::from_pem(private_key_pem)
        .map_err(|_| crate::ImError::PermissionDenied)?;
    let algorithm_matches = match role {
        BootstrapPrivateKeyRole::Signing => {
            matches!(&private, anp::PrivateKeyMaterial::Ed25519(_))
        }
        BootstrapPrivateKeyRole::Agreement => {
            matches!(&private, anp::PrivateKeyMaterial::X25519(_))
        }
    };
    let public =
        crate::internal::identity_wire::document::extract_identity_public_key(matching[0])?;
    if !algorithm_matches
        || private.public_key().to_pem() != public.to_pem()
        || private.public_key().to_pem() != expected_public_key_pem
    {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(())
}

fn validate_vnext_agent_legacy_registry(
    target: &crate::identity::VNextAgentBootstrapMaterial,
    raw: serde_json::Value,
) -> crate::ImResult<()> {
    let registry = crate::internal::identity_wire::device_join::parse_registry_result(
        raw,
        &target.did,
        false,
    )?;
    let matching = registry
        .devices
        .iter()
        .filter(|device| device.device_id == target.protocol_device_id.as_str())
        .collect::<Vec<_>>();
    if registry.devices.len() != 1
        || matching.len() != 1
        || matching[0].signing_key_id != target.device_signing_key_id
        || matching[0].e2ee_key_id != target.device_e2ee_key_id
        || matching[0].role
            != crate::internal::identity_device_state::DeviceAuthorizationRole::Admin
        || !matching[0].management_ready
        || matching[0].auth_generation != 1
        || registry.checkpoint.document_hash != target.document_hash
    {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(())
}

fn canonical_agent_handle_local_part(value: &str) -> crate::ImResult<String> {
    let value = value.trim().to_ascii_lowercase();
    if !anp::wns::validate_local_part(&value) {
        return Err(crate::ImError::invalid_input(
            Some("handle_local_part".to_owned()),
            "Agent Handle local-part must be a canonical WNS local-part without a domain",
        ));
    }
    Ok(value)
}

fn validate_preserved_agent_handle(
    document: &serde_json::Value,
    kind: crate::identity::AgentIdentityKind,
    local_part: &str,
    domain: &str,
) -> crate::ImResult<()> {
    let did = crate::ids::Did::parse(
        document
            .get("id")
            .and_then(serde_json::Value::as_str)
            .ok_or(crate::ImError::PermissionDenied)?,
    )?;
    let expected_agent_prefix = format!("did:wba:{}:agent:{}:", domain.trim(), kind.as_str());
    if !did.as_str().starts_with(&expected_agent_prefix)
        || !anp::authentication::validate_did_document_binding(document, true)
    {
        return Err(crate::ImError::PermissionDenied);
    }
    let handle = crate::ids::Handle::parse(format!("{local_part}.{}", domain.trim()), "")?;
    validate_handle_service_for_did(document, &did, &handle)
}

pub(crate) fn validate_handle_service_for_did(
    document: &serde_json::Value,
    did: &crate::ids::Did,
    handle: &crate::ids::Handle,
) -> crate::ImResult<()> {
    if document.get("id").and_then(serde_json::Value::as_str) != Some(did.as_str()) {
        return Err(crate::ImError::PermissionDenied);
    }
    let (local_part, domain) = handle
        .as_str()
        .split_once('.')
        .filter(|(local_part, domain)| !local_part.is_empty() && !domain.is_empty())
        .ok_or(crate::ImError::PermissionDenied)?;
    let normalized_handle = anp::wns::normalize_handle(handle.as_str())
        .map_err(|_| crate::ImError::PermissionDenied)?;
    if normalized_handle != handle.as_str() {
        return Err(crate::ImError::PermissionDenied);
    }
    let expected_endpoint = anp::wns::build_resolution_url(local_part, domain);
    let handle_services = anp::wns::extract_handle_service_from_did_document(document);
    if handle_services.len() != 1 {
        return Err(crate::ImError::PermissionDenied);
    }
    let service = &handle_services[0];
    let expected_absolute_id = format!("{}#handle", did.as_str());
    let service_id = service.get("id").and_then(serde_json::Value::as_str);
    if (!matches!(service_id, Some("#handle")) && service_id != Some(expected_absolute_id.as_str()))
        || service
            .get("serviceEndpoint")
            .and_then(serde_json::Value::as_str)
            != Some(expected_endpoint.as_str())
    {
        return Err(crate::ImError::PermissionDenied);
    }
    Ok(())
}

fn vnext_agent_bootstrap_material(
    kind: crate::identity::AgentIdentityKind,
    local_part: String,
    generated: crate::internal::identity_generation::GeneratedVNextIdentityWithDaemonSubkey,
) -> crate::ImResult<crate::identity::VNextAgentBootstrapMaterial> {
    let document_hash =
        crate::internal::identity_wire::document::document_hash(&generated.did_document)?;
    Ok(crate::identity::VNextAgentBootstrapMaterial {
        kind,
        handle_local_part: local_part,
        identity_id: generated.unique_id,
        did: generated.did,
        did_document: generated.did_document,
        document_hash,
        protocol_device_id: generated.protocol_device_id,
        root_key_id: generated.root_key_id,
        root_private_key_pem: generated.root_private_pem,
        root_public_key_pem: generated.root_public_pem,
        device_signing_key_id: generated.device_signing_key_id,
        device_signing_private_key_pem: generated.device_signing_private_pem,
        device_signing_public_key_pem: generated.device_signing_public_pem,
        device_e2ee_key_id: generated.device_e2ee_key_id,
        device_e2ee_private_key_pem: generated.device_e2ee_private_pem,
        device_e2ee_public_key_pem: generated.device_e2ee_public_pem,
    })
}

impl ImCoreInner {
    pub(crate) fn sdk_config(&self) -> &crate::ImCoreConfig {
        &self.sdk_config
    }

    pub(crate) fn sdk_paths(&self) -> &crate::ImCorePaths {
        &self.sdk_paths
    }

    pub(crate) fn identity_secret_storage_policy(&self) -> IdentitySecretStoragePolicy {
        self.identity_secret_storage_policy
    }

    pub(crate) fn identity_vault(&self) -> Option<&options::IdentityVaultContext> {
        self.identity_vault.as_ref()
    }

    pub(crate) fn device_revoke_enabled(&self) -> bool {
        self.device_revoke_enabled
    }

    pub(crate) fn direct_e2ee_v2_enabled(&self) -> bool {
        self.direct_e2ee_v2_enabled
    }

    pub(crate) fn group_e2ee_v2_enabled(&self) -> bool {
        self.group_e2ee_v2_enabled
    }

    pub(crate) fn handle_recovery_enabled(&self) -> bool {
        self.handle_recovery_enabled
    }

    pub(crate) fn multi_device_audience(&self) -> Option<&str> {
        self.multi_device_audience.as_deref()
    }

    #[cfg(feature = "provider-traits")]
    pub(crate) fn identity_custody_provider(
        &self,
    ) -> Option<&Arc<dyn crate::provider::IdentityCustody>> {
        self.identity_custody_provider
            .as_ref()
            .map(|provider| &provider.inner)
    }

    pub(crate) fn handle_recovery_lock(
        &self,
        owner_identity_id: &str,
    ) -> Arc<tokio::sync::Mutex<()>> {
        let scope = format!(
            "{}\0{}",
            crate::internal::identity_transition_pending::state_root_fingerprint(
                &self.sdk_paths.local_state.sqlite_path,
            ),
            owner_identity_id,
        );
        let mut locks = self
            .handle_recovery_locks
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(lock) = locks.get(&scope).and_then(std::sync::Weak::upgrade) {
            return lock;
        }
        let lock = Arc::new(tokio::sync::Mutex::new(()));
        locks.insert(scope, Arc::downgrade(&lock));
        lock
    }

    pub(crate) fn direct_rebind_lock(
        &self,
        owner_identity_id: &str,
        conversation_id: &str,
    ) -> Arc<tokio::sync::Mutex<()>> {
        let scope = format!(
            "{}\0{}\0{}",
            crate::internal::identity_transition_pending::state_root_fingerprint(
                &self.sdk_paths.local_state.sqlite_path,
            ),
            owner_identity_id,
            conversation_id,
        );
        let mut locks = self
            .direct_rebind_locks
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        locks.retain(|_, lock| lock.strong_count() > 0);
        if let Some(lock) = locks.get(&scope).and_then(std::sync::Weak::upgrade) {
            return lock;
        }
        let lock = Arc::new(tokio::sync::Mutex::new(()));
        locks.insert(scope, Arc::downgrade(&lock));
        lock
    }

    #[cfg(feature = "sqlite")]
    pub(crate) async fn local_state_db(
        &self,
    ) -> crate::ImResult<crate::internal::local_state::actor::LocalStateDb> {
        self.local_state_db
            .get_or_try_init(|| {
                crate::internal::local_state::actor::LocalStateDb::open(
                    self.sdk_paths.local_state.sqlite_path.clone(),
                )
            })
            .await
            .cloned()
    }
}

#[cfg(all(test, feature = "sqlite"))]
mod tests {
    use super::*;

    #[test]
    fn handle_recovery_requires_exact_explicit_multi_device_audience() {
        for invalid in [
            None,
            Some(""),
            Some(" awiki-user-service"),
            Some("awiki-user-service "),
        ] {
            let root = tempfile::tempdir().unwrap();
            let mut options =
                ImCoreOpenOptions::default().with_multi_device_handle_recovery_enabled(true);
            options.multi_device_audience = invalid.map(str::to_owned);
            let error =
                match ImCore::new_with_options(test_config(), test_paths(root.path()), options) {
                    Ok(_) => panic!("invalid recovery audience must fail"),
                    Err(error) => error,
                };
            assert!(matches!(
                error,
                crate::ImError::InvalidInput {
                    field: Some(ref field),
                    ..
                } if field == "multi_device_audience"
            ));
        }

        let root = tempfile::tempdir().unwrap();
        let too_long = "a".repeat(256);
        let error = match ImCore::new_with_options(
            test_config(),
            test_paths(root.path()),
            ImCoreOpenOptions::default()
                .with_multi_device_handle_recovery_enabled(true)
                .with_multi_device_audience(too_long),
        ) {
            Ok(_) => panic!("overlong recovery audience must fail"),
            Err(error) => error,
        };
        assert!(matches!(
            error,
            crate::ImError::InvalidInput {
                field: Some(ref field),
                ..
            } if field == "multi_device_audience"
        ));

        let root = tempfile::tempdir().unwrap();
        let core = ImCore::new_with_options(
            test_config(),
            test_paths(root.path()),
            ImCoreOpenOptions::default()
                .with_multi_device_handle_recovery_enabled(true)
                .with_multi_device_audience("awiki-user-service"),
        )
        .unwrap();
        assert_eq!(
            core.inner().multi_device_audience(),
            Some("awiki-user-service")
        );
    }

    #[test]
    fn handle_recovery_locks_are_identity_scoped() {
        let root = tempfile::tempdir().unwrap();
        let core = test_core(root.path());
        let alice = core.inner().handle_recovery_lock("owner-alice");
        let alice_again = core.inner().handle_recovery_lock("owner-alice");
        let bob = core.inner().handle_recovery_lock("owner-bob");
        assert!(Arc::ptr_eq(&alice, &alice_again));
        assert!(!Arc::ptr_eq(&alice, &bob));
        let _alice_guard = alice.try_lock().unwrap();
        assert!(bob.try_lock().is_ok());
    }

    #[test]
    fn direct_rebind_locks_are_owner_and_conversation_scoped() {
        let root = tempfile::tempdir().unwrap();
        let core = test_core(root.path());
        let first = core
            .inner()
            .direct_rebind_lock("owner-alice", "conversation-a");
        let same = core
            .inner()
            .direct_rebind_lock("owner-alice", "conversation-a");
        let other_conversation = core
            .inner()
            .direct_rebind_lock("owner-alice", "conversation-b");
        let other_owner = core
            .inner()
            .direct_rebind_lock("owner-bob", "conversation-a");

        assert!(Arc::ptr_eq(&first, &same));
        assert!(!Arc::ptr_eq(&first, &other_conversation));
        assert!(!Arc::ptr_eq(&first, &other_owner));
        let _first_guard = first.try_lock().unwrap();
        assert!(other_conversation.try_lock().is_ok());
        assert!(other_owner.try_lock().is_ok());
    }

    #[test]
    fn agent_legacy_reconcile_preserves_device_keys_and_fails_closed_on_other_manifest() {
        let root = tempfile::tempdir().unwrap();
        let core = test_core(root.path());
        let mut legacy =
            crate::internal::identity_generation::generate_identity_with_default_daemon_subkey(
                "awiki.test",
                ["agent", "daemon", "edgehost"],
                None,
                None,
            )
            .unwrap()
            .identity;
        legacy
            .did_document
            .get_mut("service")
            .and_then(serde_json::Value::as_array_mut)
            .unwrap()
            .push(anp::wns::build_handle_service_entry(
                legacy.did.as_str(),
                "edgehost",
                "awiki.test",
            ));
        crate::internal::identity_daemon_subkey::resign_did_document_with_key1(
            &mut legacy.did_document,
            &legacy.did,
            &legacy.key1_private_pem,
        )
        .unwrap();
        let target = core
            .prepare_vnext_agent_legacy_upgrade(
                crate::identity::AgentIdentityKind::Daemon,
                "edgehost",
                legacy.did_document.clone(),
                legacy.key1_private_pem.clone(),
            )
            .unwrap();

        let committed = core
            .reconcile_vnext_agent_legacy_upgrade(
                &legacy.did_document,
                target.clone(),
                &target.did_document,
                &legacy.key1_private_pem,
            )
            .unwrap();
        assert_eq!(
            committed,
            crate::identity::VNextAgentLegacyUpgradeReconciliation::TargetCommitted
        );

        let mut refreshed_legacy = legacy.did_document.clone();
        refreshed_legacy["agentExtension"] = serde_json::json!({"revision": 2});
        crate::internal::identity_daemon_subkey::resign_did_document_with_key1(
            &mut refreshed_legacy,
            &legacy.did,
            &legacy.key1_private_pem,
        )
        .unwrap();
        let rebuilt = core
            .reconcile_vnext_agent_legacy_upgrade(
                &legacy.did_document,
                target.clone(),
                &refreshed_legacy,
                &legacy.key1_private_pem,
            )
            .unwrap();
        let crate::identity::VNextAgentLegacyUpgradeReconciliation::LegacyRebuilt {
            target: rebuilt,
        } = rebuilt
        else {
            panic!("expected rebuilt Legacy target");
        };
        assert_eq!(rebuilt.did, target.did);
        assert_eq!(rebuilt.protocol_device_id, target.protocol_device_id);
        assert_eq!(
            rebuilt.device_signing_private_key_pem,
            target.device_signing_private_key_pem
        );
        assert_eq!(
            rebuilt.device_e2ee_private_key_pem,
            target.device_e2ee_private_key_pem
        );
        assert_eq!(
            rebuilt.did_document.get("agentExtension"),
            Some(&serde_json::json!({"revision": 2}))
        );

        let other_target = core
            .prepare_vnext_agent_legacy_upgrade(
                crate::identity::AgentIdentityKind::Daemon,
                "edgehost",
                legacy.did_document.clone(),
                legacy.key1_private_pem.clone(),
            )
            .unwrap();
        assert_eq!(
            core.reconcile_vnext_agent_legacy_upgrade(
                &legacy.did_document,
                target,
                &other_target.did_document,
                &legacy.key1_private_pem,
            )
            .unwrap_err(),
            crate::ImError::PermissionDenied
        );
    }

    fn test_core(root: &std::path::Path) -> ImCore {
        ImCore::new(test_config(), test_paths(root)).unwrap()
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

    #[tokio::test]
    async fn local_state_db_concurrent_first_open_shares_actor() {
        let root = tempfile::tempdir().unwrap();
        let core = ImCore::new(
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
            },
            crate::ImCorePaths {
                identities: crate::IdentityRegistryPaths {
                    identity_root_dir: root.path().join("identities"),
                    registry_path: root.path().join("identities").join("registry.json"),
                    default_identity_path: Some(root.path().join("identities").join("default")),
                },
                local_state: crate::LocalStatePaths {
                    sqlite_path: root.path().join("local").join("im.sqlite"),
                },
                runtime: crate::RuntimePaths {
                    cache_dir: root.path().join("cache"),
                    temp_dir: root.path().join("tmp"),
                },
            },
        )
        .unwrap();

        let inner = core.inner.clone();
        let mut tasks = Vec::new();
        for _ in 0..8 {
            let inner = inner.clone();
            tasks.push(tokio::spawn(async move {
                let db = inner.local_state_db().await.unwrap();
                db.current_schema_version().await.unwrap()
            }));
        }

        for task in tasks {
            assert_eq!(
                task.await.unwrap(),
                crate::internal::local_state::schema::SCHEMA_VERSION
            );
        }
        assert!(core.inner().local_state_db.get().is_some());
    }
}
