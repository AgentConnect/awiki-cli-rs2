use anyhow::Result;
use im_core::{
    core::{CoreBootstrap, LocalStateStatus},
    ImCore,
};

use crate::DaemonConfig;

#[derive(Clone)]
pub struct ImCoreAdapter {
    core: ImCore,
}

impl ImCoreAdapter {
    pub fn open(config: &DaemonConfig) -> Result<Self> {
        let core = ImCore::new(config.im_core_config()?, config.im_core_paths())?;
        Ok(Self { core })
    }

    pub fn bootstrap(&self) -> CoreBootstrap<'_> {
        self.core.bootstrap()
    }

    pub async fn initialize_local_state(&self) -> Result<LocalStateStatus> {
        Ok(self.bootstrap().initialize_local_state_async().await?)
    }

    pub fn client(&self) -> Result<im_core::ImClient> {
        let selector = crate::IdentitySelectorConfig::to_im_core_selector(
            &crate::IdentitySelectorConfig::Default,
        )?;
        Ok(self.core.client(selector)?)
    }

    pub fn client_for_config(&self, config: &DaemonConfig) -> Result<im_core::ImClient> {
        Ok(self
            .core
            .client(config.identity_selector.to_im_core_selector()?)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn adapter_initializes_im_core_state() {
        let root = tempfile::tempdir().unwrap();
        let config = DaemonConfig::for_state_root(root.path()).unwrap();
        let status = ImCoreAdapter::open(&config)
            .unwrap()
            .initialize_local_state()
            .await
            .unwrap();

        assert!(config.im_core_sqlite_path.exists());
        assert!(status.schema_version.is_some());
    }
}
