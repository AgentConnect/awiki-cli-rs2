use anyhow::Result;
use im_core::{
    core::{CoreBootstrap, LocalStateStatus},
    identity::{IdentityMissingItem, IdentityReadiness, IdentitySummary},
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
        sync_agent_identity_to_im_core(config, identity, jwt_token)?;
        Ok(self
            .core
            .client(im_core::IdentitySelector::Did(im_core::ids::Did::parse(
                &identity.agent_did,
            )?))?)
    }

    pub fn client_for_did(&self, did: &str) -> Result<im_core::ImClient> {
        Ok(self
            .core
            .client(im_core::IdentitySelector::Did(im_core::ids::Did::parse(
                did,
            )?))?)
    }
}

pub fn sync_agent_identity_to_im_core(
    config: &DaemonConfig,
    identity: &AgentIdentityRecord,
    jwt_token: Option<&str>,
) -> Result<()> {
    let alias = identity_alias(&identity.agent_did);
    let identity_dir = config.identity_root_dir.join(&alias);
    std::fs::create_dir_all(&identity_dir)?;
    std::fs::write(
        identity_dir.join("did.json"),
        serde_json::to_vec_pretty(&identity.did_document)?,
    )?;
    std::fs::write(
        identity_dir.join("private.key"),
        &identity.auth_private_key_pem,
    )?;
    if !identity.e2ee_agreement_private_key_pem.trim().is_empty() {
        std::fs::write(
            identity_dir.join("e2ee-agreement-private.pem"),
            &identity.e2ee_agreement_private_key_pem,
        )?;
    }
    if let Some(token) = jwt_token.map(str::trim).filter(|value| !value.is_empty()) {
        std::fs::write(
            identity_dir.join("auth.json"),
            serde_json::json!({ "jwt_token": token }).to_string(),
        )?;
    } else if !identity_dir.join("auth.json").exists() {
        std::fs::write(identity_dir.join("auth.json"), "{}")?;
    }
    let mut identities = identity_summaries_from_registry(config)?;
    identities.retain(|entry| entry.did.as_str() != identity.agent_did);
    identities.push(IdentitySummary {
        id: im_core::ids::IdentityId::parse(&alias)?,
        did: im_core::ids::Did::parse(&identity.agent_did)?,
        handle: Some(im_core::ids::Handle::parse(&identity.handle, "")?),
        display_name: Some(identity.handle.clone()),
        local_alias: Some(alias.clone()),
        device_id: None,
        is_default: false,
        readiness: IdentityReadiness {
            ready_for_auth: true,
            ready_for_messaging: true,
            missing: Vec::new(),
        },
    });
    let registry = serde_json::json!({
        "default_identity": alias,
        "identities": identities.into_iter().map(identity_summary_json).collect::<Vec<_>>(),
    });
    std::fs::write(
        &config.identity_registry_path,
        serde_json::to_vec_pretty(&registry)?,
    )?;
    if let Some(default_path) = config.default_identity_path.as_ref() {
        std::fs::write(default_path, format!("{alias}\n"))?;
    }
    Ok(())
}

pub fn agent_identity_auth_paths(
    config: &DaemonConfig,
    agent_did: &str,
) -> (std::path::PathBuf, std::path::PathBuf) {
    let identity_dir = config.identity_root_dir.join(identity_alias(agent_did));
    (
        identity_dir.join("did.json"),
        identity_dir.join("private.key"),
    )
}

fn identity_summaries_from_registry(config: &DaemonConfig) -> Result<Vec<IdentitySummary>> {
    if !config.identity_registry_path.exists() {
        return Ok(Vec::new());
    }
    let core = ImCore::new(config.im_core_config()?, config.im_core_paths())?;
    Ok(core.identities().list()?)
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

fn identity_summary_json(summary: IdentitySummary) -> serde_json::Value {
    serde_json::json!({
        "id": summary.id.as_str(),
        "did": summary.did.as_str(),
        "dir_name": summary.local_alias.as_deref().unwrap_or(summary.id.as_str()),
        "handle": summary.handle.as_ref().map(im_core::ids::Handle::as_str),
        "display_name": summary.display_name,
        "local_alias": summary.local_alias,
        "device_id": summary.device_id,
        "is_default": summary.is_default,
        "ready_for_auth": summary.readiness.ready_for_auth,
        "ready_for_messaging": summary.readiness.ready_for_messaging,
        "missing": summary.readiness.missing.into_iter().map(missing_item).collect::<Vec<_>>(),
    })
}

fn missing_item(item: IdentityMissingItem) -> &'static str {
    match item {
        IdentityMissingItem::DidDocument => "did_document",
        IdentityMissingItem::PrivateKey => "private_key",
        IdentityMissingItem::AuthState => "auth_state",
        IdentityMissingItem::Handle => "handle",
        IdentityMissingItem::MessageEndpoint => "message_endpoint",
        IdentityMissingItem::Other(_) => "other",
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
