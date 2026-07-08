use anyhow::Result;
use im_core::{
    core::{CoreBootstrap, LocalStateStatus},
    ImCore,
};

use crate::agent::AgentIdentityRecord;
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

    pub fn client_for_agent_identity(
        &self,
        config: &DaemonConfig,
        identity: &AgentIdentityRecord,
        jwt_token: Option<&str>,
    ) -> Result<im_core::ImClient> {
        let _ = config;
        Ok(self
            .core
            .client_with_identity_material(hosted_identity_material(identity, jwt_token)?)?)
    }

    pub fn client_for_did(&self, did: &str) -> Result<im_core::ImClient> {
        Ok(self
            .core
            .client(im_core::IdentitySelector::Did(im_core::ids::Did::parse(
                did,
            )?))?)
    }
}

pub fn hosted_identity_material(
    identity: &AgentIdentityRecord,
    jwt_token: Option<&str>,
) -> Result<im_core::HostedIdentityMaterial> {
    Ok(im_core::HostedIdentityMaterial {
        identity_id: identity_alias(&identity.agent_did),
        did: identity.agent_did.clone(),
        handle: Some(identity.handle.clone()),
        display_name: Some(identity.handle.clone()),
        did_document: identity.did_document.clone(),
        default_signing_private_key_pem: identity.auth_private_key_pem.clone(),
        e2ee_agreement_private_key_pem: identity.e2ee_agreement_private_key_pem.clone(),
        auth_token: jwt_token
            .map(str::trim)
            .filter(|token| !token.is_empty())
            .map(ToOwned::to_owned),
    })
}

fn identity_alias(did: &str) -> String {
    did.chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{generate_agent_identity, AgentKind};

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

    #[test]
    fn client_for_agent_identity_does_not_write_identity_secret_files() {
        let root = tempfile::tempdir().unwrap();
        let config = DaemonConfig::for_state_root(root.path()).unwrap();
        let identity = generate_agent_identity(&config, AgentKind::Runtime, "runtime-hosted-test")
            .unwrap()
            .into_record("runtime-hosted-test".to_string(), AgentKind::Runtime);
        let adapter = ImCoreAdapter::open(&config).unwrap();

        let client = adapter
            .client_for_agent_identity(&config, &identity, Some("jwt-hosted"))
            .unwrap();

        assert_eq!(client.did().as_str(), identity.agent_did);
        let status = client.auth().status().unwrap();
        assert!(status.has_session);
        let identity_dir = config
            .identity_root_dir
            .join(identity_alias(&identity.agent_did));
        assert!(!identity_dir.join("private.key").exists());
        assert!(!identity_dir.join("e2ee-agreement-private.pem").exists());
        assert!(!identity_dir.join("auth.json").exists());
    }
}
