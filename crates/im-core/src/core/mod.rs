use std::sync::Arc;

mod bootstrap;
mod client;

pub use self::bootstrap::{
    CoreBootstrap, LocalStateStatus, MigrationReport, PathCheck, PathValidationReport,
};
pub use self::client::ImClient;

pub(crate) struct ImCoreInner {
    pub(crate) sdk_config: crate::ImCoreConfig,
    pub(crate) sdk_paths: crate::ImCorePaths,
}

#[derive(Clone)]
pub struct ImCore {
    inner: Arc<ImCoreInner>,
}

impl ImCore {
    pub fn new(
        sdk_config: crate::ImCoreConfig,
        sdk_paths: crate::ImCorePaths,
    ) -> crate::ImResult<Self> {
        if sdk_config.did_domain.trim().is_empty() {
            return Err(crate::ImError::invalid_input(
                Some("did_domain".to_string()),
                "DID domain must not be empty",
            ));
        }
        Ok(Self {
            inner: Arc::new(ImCoreInner {
                sdk_config,
                sdk_paths,
            }),
        })
    }

    pub fn identities(&self) -> crate::identity::IdentityRegistry<'_> {
        crate::identity::IdentityRegistry::new(self)
    }

    pub fn bootstrap(&self) -> CoreBootstrap<'_> {
        CoreBootstrap::new(self)
    }

    pub fn client(&self, selector: crate::identity::IdentitySelector) -> crate::ImResult<ImClient> {
        let runtime = self.identities().load_runtime(selector)?;
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
}
