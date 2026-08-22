use anyhow::{Context, Result};
use im_core::{
    core::{CoreBootstrap, LocalStateStatus},
    ImCore,
};

use crate::agent::{AgentIdentityRecord, AgentKind};
use crate::state::{AgentDeviceIdentityRecord, DaemonState};
use crate::DaemonConfig;

#[derive(Clone)]
struct DaemonAgentAuthTokenPersistence {
    state: DaemonState,
    agent_did: String,
    protocol_device_id: String,
    device_signing_key_id: String,
    auth_generation: u64,
}

impl im_core::HostBackedAuthTokenPersistence for DaemonAgentAuthTokenPersistence {
    fn persist_auth_token(&self, token: &str) -> im_core::ImResult<()> {
        self.state
            .replace_agent_device_access_token(
                &self.agent_did,
                &self.protocol_device_id,
                &self.device_signing_key_id,
                self.auth_generation,
                token,
            )
            .map_err(|_| im_core::ImError::CredentialFileUnreadable {
                path_kind: "daemon_agent_device_access_token".to_owned(),
                detail: "validated Device Access token could not be committed to the daemon vault"
                    .to_owned(),
            })
    }
}

#[derive(Clone)]
pub struct ImCoreAdapter {
    core: ImCore,
}

impl ImCoreAdapter {
    pub fn open(config: &DaemonConfig) -> Result<Self> {
        let core = ImCore::new(config.im_core_config()?, config.im_core_paths())?;
        core.identities()
            .migrate_identity_custody()
            .context("migrate daemon im-core identity custody")?;
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

    pub fn generate_vnext_agent_bootstrap(
        &self,
        kind: AgentKind,
        handle_local_part: &str,
    ) -> Result<im_core::VNextAgentBootstrapMaterial> {
        Ok(self.core.generate_vnext_agent_bootstrap(
            match kind {
                AgentKind::Daemon => im_core::AgentIdentityKind::Daemon,
                AgentKind::Runtime => im_core::AgentIdentityKind::Runtime,
            },
            handle_local_part,
        )?)
    }

    pub fn prepare_vnext_agent_legacy_upgrade(
        &self,
        kind: AgentKind,
        handle_local_part: &str,
        legacy_did_document: serde_json::Value,
        root_private_key_pem: String,
    ) -> Result<im_core::VNextAgentBootstrapMaterial> {
        Ok(self.core.prepare_vnext_agent_legacy_upgrade(
            match kind {
                AgentKind::Daemon => im_core::AgentIdentityKind::Daemon,
                AgentKind::Runtime => im_core::AgentIdentityKind::Runtime,
            },
            handle_local_part,
            legacy_did_document,
            root_private_key_pem,
        )?)
    }

    pub fn reconcile_vnext_agent_legacy_upgrade(
        &self,
        source_legacy_did_document: &serde_json::Value,
        target: im_core::VNextAgentBootstrapMaterial,
        remote_did_document: &serde_json::Value,
        root_private_key_pem: &str,
    ) -> Result<im_core::VNextAgentLegacyUpgradeReconciliation> {
        Ok(self.core.reconcile_vnext_agent_legacy_upgrade(
            source_legacy_did_document,
            target,
            remote_did_document,
            root_private_key_pem,
        )?)
    }

    pub fn refresh_committed_vnext_agent_legacy_upgrade_session(
        &self,
        target: &im_core::VNextAgentBootstrapMaterial,
    ) -> Result<im_core::VNextAgentLegacyUpgradeSession> {
        let adapter = self.clone();
        let target = target.clone();
        if tokio::runtime::Handle::try_current().is_ok() {
            let join = std::thread::Builder::new()
                .name("awiki-agent-legacy-session-recovery".to_owned())
                .spawn(move || {
                    let runtime = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .context("create Agent Legacy session recovery runtime")?;
                    runtime
                        .block_on(
                            adapter
                                .core
                                .refresh_committed_vnext_agent_legacy_upgrade_session(&target),
                        )
                        .map_err(anyhow::Error::from)
                })
                .context("spawn Agent Legacy session recovery runtime thread")?;
            return join.join().map_err(|_| {
                anyhow::anyhow!("Agent Legacy session recovery runtime thread panicked")
            })?;
        }
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .context("create Agent Legacy session recovery runtime")?;
        runtime
            .block_on(
                self.core
                    .refresh_committed_vnext_agent_legacy_upgrade_session(&target),
            )
            .map_err(anyhow::Error::from)
    }

    pub fn client_for_agent(
        &self,
        _config: &DaemonConfig,
        state: &DaemonState,
        agent_did: &str,
    ) -> Result<im_core::ImClient> {
        let identity = state
            .load_agent_device_identity(agent_did)?
            .with_context(|| {
                format!(
                    "agent_identity_migration_required: exact device identity missing for {agent_did}"
                )
            })?;
        let material = host_backed_device_identity_material(&identity)?;
        let persistence = std::sync::Arc::new(DaemonAgentAuthTokenPersistence {
            state: state.clone(),
            agent_did: identity.agent_did.clone(),
            protocol_device_id: identity.protocol_device_id.clone(),
            device_signing_key_id: identity.device_signing_key_id.clone(),
            auth_generation: identity.auth_generation,
        });
        Ok(self
            .core
            .client_with_device_identity_material_and_auth_persistence(material, persistence)?)
    }

    pub fn client_for_agent_device_identity(
        &self,
        identity: &AgentDeviceIdentityRecord,
    ) -> Result<im_core::ImClient> {
        identity.validate()?;
        if identity.identity_status != "active" {
            anyhow::bail!(
                "agent_device_identity_unavailable: identity status is {}",
                identity.identity_status
            );
        }
        Ok(self
            .core
            .client_with_device_identity_material(host_backed_device_identity_material(
                identity,
            )?)?)
    }

    pub fn client_for_delegated_signing_identity(
        &self,
        identity_id: String,
        did: String,
        did_document: serde_json::Value,
        private_key_pem: String,
    ) -> Result<im_core::ImClient> {
        Ok(self
            .core
            .client_with_identity_material(im_core::HostedIdentityMaterial {
                identity_id,
                did,
                handle: None,
                display_name: None,
                did_document,
                default_signing_private_key_pem: private_key_pem,
                e2ee_agreement_private_key_pem: None,
                auth_token: None,
            })?)
    }

    pub fn client_for_anp_delegated_identity(
        &self,
        identity: anp_identity::ManagedIdentity,
    ) -> Result<im_core::ImClient> {
        Ok(self.core.client_with_anp_delegated_identity(identity)?)
    }
}

fn host_backed_device_identity_material(
    identity: &AgentDeviceIdentityRecord,
) -> Result<im_core::HostBackedDeviceIdentityMaterial> {
    let protocol_device_id = im_core::ids::ProtocolDeviceId::parse(&identity.protocol_device_id)?;
    let authorization_status = match identity.authorization_status.as_str() {
        "active" => im_core::IdentityDeviceAuthorizationStatus::Active,
        "revoked" => im_core::IdentityDeviceAuthorizationStatus::Revoked,
        other => anyhow::bail!("unsupported device authorization status: {other}"),
    };
    let role = match identity.role.as_str() {
        "admin" => im_core::IdentityDeviceRole::Admin,
        "member" => im_core::IdentityDeviceRole::Member,
        other => anyhow::bail!("unsupported device role: {other}"),
    };
    Ok(im_core::HostBackedDeviceIdentityMaterial {
        identity_id: identity.identity_id.clone(),
        did: identity.agent_did.clone(),
        handle: Some(identity.full_handle.clone()),
        display_name: Some(identity.display_name.clone()),
        account_id: identity.account_id.clone(),
        binding_generation: identity.binding_generation.clone(),
        did_document: identity.did_document.clone(),
        protocol_device_id,
        device_signing_key_id: identity.device_signing_key_id.clone(),
        device_signing_private_key_pem: identity.device_signing_private_key_pem.clone(),
        device_e2ee_key_id: identity.device_e2ee_key_id.clone(),
        device_e2ee_private_key_pem: identity.device_e2ee_private_key_pem.clone(),
        root_key_id: identity.root_key_id.clone(),
        root_private_key_pem: identity.root_private_key_pem.clone(),
        authorization_status,
        role,
        management_ready: identity.management_ready,
        auth_generation: identity.auth_generation.to_string(),
        access_token: identity.access_token.clone(),
    })
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
        e2ee_agreement_private_key_pem: Some(identity.e2ee_agreement_private_key_pem.clone()),
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

    #[test]
    fn delegated_signing_client_is_memory_only_and_ready_to_refresh_auth() {
        let root = tempfile::tempdir().unwrap();
        let config = DaemonConfig::for_state_root(root.path()).unwrap();
        let identity = generate_agent_identity(&config, AgentKind::Daemon, "delegated-hosted-test")
            .unwrap()
            .into_record("delegated-hosted-test".to_string(), AgentKind::Daemon);
        let adapter = ImCoreAdapter::open(&config).unwrap();

        let client = adapter
            .client_for_delegated_signing_identity(
                "delegated-inbox-test".to_owned(),
                identity.agent_did.clone(),
                identity.did_document.clone(),
                identity.auth_private_key_pem.clone(),
            )
            .unwrap();

        let status = client.auth().status().unwrap();
        assert!(!status.has_session);
        assert!(status.needs_refresh);
        assert!(!config
            .identity_root_dir
            .join("delegated-inbox-test")
            .exists());
    }
}
