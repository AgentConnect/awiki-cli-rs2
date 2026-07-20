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
pub use self::options::{IdentitySecretStoragePolicy, ImCoreOpenOptions, ImCoreSecretVaultOptions};

pub(crate) struct ImCoreInner {
    pub(crate) sdk_config: crate::ImCoreConfig,
    pub(crate) sdk_paths: crate::ImCorePaths,
    pub(crate) identity_secret_storage_policy: IdentitySecretStoragePolicy,
    pub(crate) identity_vault: Option<options::IdentityVaultContext>,
    pub(crate) device_join_lock: std::sync::Mutex<()>,
    pub(crate) device_join_enabled: bool,
    pub(crate) root_key_transfer_enabled: bool,
    pub(crate) handle_recovery_enabled: bool,
    pub(crate) handle_recovery_lock: tokio::sync::Mutex<()>,
    pub(crate) device_join_approvals:
        crate::internal::identity_device_join_runtime::DeviceJoinApprovalHandleStore,
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
        Ok(Self {
            inner: Arc::new(ImCoreInner {
                sdk_config,
                sdk_paths,
                identity_secret_storage_policy: options.identity_secret_storage_policy,
                identity_vault,
                device_join_lock: std::sync::Mutex::new(()),
                device_join_enabled: options.multi_device_join_enabled,
                root_key_transfer_enabled: options.multi_device_root_transfer_enabled,
                handle_recovery_enabled: options.multi_device_handle_recovery_enabled,
                handle_recovery_lock: tokio::sync::Mutex::new(()),
                device_join_approvals: Default::default(),
                #[cfg(feature = "sqlite")]
                local_state_db: OnceCell::new(),
            }),
        })
    }

    pub fn identities(&self) -> crate::identity::IdentityRegistry<'_> {
        crate::identity::IdentityRegistry::new(self)
    }

    pub fn device_join(&self) -> crate::identity::DeviceJoinService<'_> {
        crate::identity::DeviceJoinService::new(self)
    }

    pub fn root_key_transfer(&self) -> crate::identity::RootKeyTransferService<'_> {
        crate::identity::RootKeyTransferService::new(self)
    }

    pub fn handle_recovery(&self) -> crate::identity::HandleRecoveryService<'_> {
        crate::identity::HandleRecoveryService::new(self)
    }

    pub fn bootstrap(&self) -> CoreBootstrap<'_> {
        CoreBootstrap::new(self)
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
        Ok(ImClient::new(self.inner.clone(), runtime))
    }

    pub fn client_with_identity_material(
        &self,
        material: crate::identity::HostedIdentityMaterial,
    ) -> crate::ImResult<ImClient> {
        let identity_id = crate::ids::IdentityId::parse(&material.identity_id)?;
        let did = crate::ids::Did::parse(&material.did)?;
        let handle = material
            .handle
            .as_deref()
            .map(|handle| crate::ids::Handle::parse(handle, &self.inner.sdk_config().did_domain))
            .transpose()?;
        let key_provider = std::sync::Arc::new(
            crate::internal::key_provider::HostedKeyMaterialProvider::new(&material)?,
        );
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
            owner: crate::internal::identity_runtime::LocalOwnerContext {
                identity_id,
                current_did: did,
            },
        };
        Ok(ImClient::new(self.inner.clone(), runtime))
    }

    pub(crate) fn inner(&self) -> &ImCoreInner {
        &self.inner
    }
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

    pub(crate) fn device_join_enabled(&self) -> bool {
        self.device_join_enabled
    }

    pub(crate) fn root_key_transfer_enabled(&self) -> bool {
        self.root_key_transfer_enabled
    }

    pub(crate) fn handle_recovery_enabled(&self) -> bool {
        self.handle_recovery_enabled
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

    #[tokio::test]
    async fn local_state_db_concurrent_first_open_shares_actor() {
        let root = tempfile::tempdir().unwrap();
        let core = ImCore::new(
            crate::ImCoreConfig {
                service_base_url: crate::ServiceEndpoint::parse("https://example.test").unwrap(),
                did_domain: "awiki.test".to_owned(),
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
