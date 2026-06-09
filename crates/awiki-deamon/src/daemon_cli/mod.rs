use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use std::path::PathBuf;

use crate::agent::{agent_data_paths, generate_product_handle, AgentDefinition, AgentKind};
use crate::commands::setup_daemon_agent;
use crate::plugins::hermes::{HermesGateway, HERMES_RUNTIME_PLUGIN_ID};
use crate::registration::{
    AgentInventoryClient, AgentLatestStatusUpdateItem, AgentRegistrationExchangeRequest,
    DidAuthMaterial, RegistrationToken, RegistrationTokenMetadata,
    UserServiceAgentRegistrationClient,
};
use crate::runtime::RuntimeInstallStatus;
use crate::service::{
    current_platform_label, manage_service, require_service_state_root_is_product, ServiceAction,
    ServicePlatform, ServiceStatus,
};
use crate::state::DaemonState;
use crate::DaemonConfig;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentListOutput {
    pub agents: Vec<AgentDefinition>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentStatusOutput {
    pub agent: AgentDefinition,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hermes: Option<HermesAgentStatus>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HermesAgentStatus {
    pub agent_did: String,
    pub runtime_profile_id: String,
    pub hermes_profile: String,
    pub hermes_home: String,
    pub awiki_skills_version: String,
    pub profile_status: String,
    pub installation: RuntimeInstallStatus,
    pub active_session_count: usize,
    pub runner_status: String,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetupDaemonAgentOutput {
    pub agent: AgentDefinition,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallOptions {
    pub token: String,
    pub state_root: PathBuf,
    pub base_url: String,
    pub download_base_url: Option<String>,
    pub foreground: bool,
    pub no_service: bool,
    pub print_json: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstallOutput {
    pub status: String,
    pub state_root: PathBuf,
    pub daemon_agent_did: String,
    pub handle: String,
    pub service: ServiceStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetupDaemonAgentOptions {
    pub handle: String,
    pub controller_did: String,
    pub registration_token: String,
}

pub fn list_agents(state: &DaemonState) -> Result<AgentListOutput> {
    Ok(AgentListOutput {
        agents: state.list_agent_definitions()?,
    })
}

pub fn agent_status(
    config: &DaemonConfig,
    state: &DaemonState,
    agent_did: &str,
) -> Result<AgentStatusOutput> {
    if agent_did.trim().is_empty() {
        bail!("--agent-did is required");
    }
    let agent = state.load_agent_definition(agent_did)?;
    let hermes = if agent.runtime_plugin_id.as_deref() == Some(HERMES_RUNTIME_PLUGIN_ID) {
        Some(hermes_status_for_agent(config, state, agent_did)?)
    } else {
        None
    };
    Ok(AgentStatusOutput { agent, hermes })
}

pub fn list_runtime_agents(state: &DaemonState) -> Result<AgentListOutput> {
    Ok(AgentListOutput {
        agents: state.list_runtime_agent_definitions()?,
    })
}

fn hermes_status_for_agent(
    config: &DaemonConfig,
    state: &DaemonState,
    agent_did: &str,
) -> Result<HermesAgentStatus> {
    let profile = state.load_hermes_profile(agent_did)?;
    let installation =
        crate::plugins::hermes::StdioHermesGateway::from_config(config).check_installation()?;
    Ok(HermesAgentStatus {
        agent_did: profile.agent_did.clone(),
        runtime_profile_id: profile.runtime_profile_id.clone(),
        hermes_profile: profile.hermes_profile.clone(),
        hermes_home: profile.hermes_home.display().to_string(),
        awiki_skills_version: profile.awiki_skills_version.clone(),
        profile_status: profile.status.clone(),
        installation,
        active_session_count: state.count_active_hermes_sessions_for_agent(agent_did)?,
        runner_status: "lazy".to_string(),
        last_error: load_latest_hermes_error(state, agent_did)?,
    })
}

fn load_latest_hermes_error(state: &DaemonState, agent_did: &str) -> Result<Option<String>> {
    let connection = state.connection()?;
    let mut statement = connection.prepare(
        r#"
SELECT detail_json
FROM audit_log
WHERE agent_did = ?1
  AND event_type = 'hermes.error'
ORDER BY created_at_ms DESC
LIMIT 1
"#,
    )?;
    let mut rows = statement.query([agent_did])?;
    let Some(row) = rows.next()? else {
        return Ok(None);
    };
    let detail_json: Option<String> = row.get(0)?;
    let Some(detail_json) = detail_json else {
        return Ok(Some("hermes.error".to_string()));
    };
    let value: Value = serde_json::from_str(&detail_json).unwrap_or(Value::Null);
    Ok(value
        .get("error")
        .or_else(|| value.get("reason"))
        .and_then(Value::as_str)
        .map(public_hermes_error_detail)
        .or_else(|| Some("hermes.error".to_string())))
}

fn public_hermes_error_detail(value: &str) -> String {
    let value = value.trim();
    if value.is_empty() || contains_sensitive_diagnostic_fragment(value) {
        return "hermes.error".to_string();
    }
    truncate_diagnostic(value, 512)
}

fn contains_sensitive_diagnostic_fragment(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    [
        "rtok_",
        "tok_",
        "runtime_rpc_token",
        "registration_token",
        "jwt",
        "auth_private_key",
        "private_key",
        "begin private key",
        "secret",
        "bearer ",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
}

fn truncate_diagnostic(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let truncated: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{truncated}...")
    } else {
        truncated
    }
}

pub fn setup_daemon_agent_from_token(
    config: &DaemonConfig,
    state: &DaemonState,
    options: SetupDaemonAgentOptions,
) -> Result<SetupDaemonAgentOutput> {
    if options.handle.trim().is_empty() {
        bail!("--handle is required");
    }
    if options.controller_did.trim().is_empty() {
        bail!("--controller-did is required");
    }
    let registration_client =
        UserServiceAgentRegistrationClient::new(&config.user_service_base_url)?;
    let agent = setup_daemon_agent(
        config,
        state,
        &registration_client,
        &options.handle,
        &options.controller_did,
        RegistrationToken::new(options.registration_token)?,
    )?;
    Ok(SetupDaemonAgentOutput { agent })
}

pub async fn install_product_daemon(options: InstallOptions) -> Result<InstallOutput> {
    let mut config = DaemonConfig::for_state_root(options.state_root)?;
    config.service_base_url = options.base_url.trim().trim_end_matches('/').to_string();
    config.user_service_base_url = config.service_base_url.clone();
    config.message_service_base_url = config.service_base_url.clone();
    config.mail_service_base_url = config.service_base_url.clone();
    config.download_base_url = options
        .download_base_url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.trim_end_matches('/').to_string())
        .unwrap_or_else(|| format!("{}/daemon", config.service_base_url));
    config.did_domain = service_host_from_base_url(&config.service_base_url)?;
    config.anp_service_endpoint = format!("{}/anp-im/rpc", config.service_base_url);
    config.anp_service_did = format!("did:wba:{}", config.did_domain);
    config.validate()?;
    config.ensure_state_layout()?;
    config.write_persistent_config()?;
    if !options.foreground && !options.no_service {
        require_service_state_root_is_product(&config)?;
    }
    let state = DaemonState::open(&config)?;
    state.initialize()?;
    let im_core = crate::ImCoreAdapter::open(&config)?;
    im_core
        .initialize_local_state()
        .await
        .context("initialize im-core local state")?;

    let registration_client =
        UserServiceAgentRegistrationClient::new(&config.user_service_base_url)?;
    let token = RegistrationToken::new(options.token)?;
    let agent = install_or_recover_daemon_agent(&config, &state, &registration_client, token)?;

    sync_one_agent_identity(&config, &state, &im_core, &agent)?;

    let service = if options.foreground || options.no_service {
        ServiceStatus {
            platform: ServicePlatform::Foreground,
            installed: false,
            running: false,
            unit_path: None,
            detail: Some(if options.no_service {
                "service installation skipped by --no-service".to_string()
            } else {
                "foreground mode requested".to_string()
            }),
        }
    } else {
        let executable = crate::service::default_executable_path()?;
        manage_service(&config, &executable, ServiceAction::Install)?
    };

    update_daemon_latest_status(&registration_client, &config, &state, &agent, &service)
        .context("update daemon latest status")?;

    Ok(InstallOutput {
        status: "ready".to_string(),
        state_root: config.state_root,
        daemon_agent_did: agent.agent_did,
        handle: agent.handle,
        service,
    })
}

fn service_host_from_base_url(base_url: &str) -> Result<String> {
    let trimmed = base_url.trim();
    let without_scheme = trimmed
        .strip_prefix("https://")
        .or_else(|| trimmed.strip_prefix("http://"))
        .context("--base-url must start with http:// or https://")?;
    let authority = without_scheme
        .split('/')
        .next()
        .unwrap_or_default()
        .split('@')
        .next_back()
        .unwrap_or_default();
    let host = authority
        .split(':')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    if host.is_empty() {
        bail!("--base-url must include a hostname");
    }
    Ok(host)
}

fn install_or_recover_daemon_agent<C>(
    config: &DaemonConfig,
    state: &DaemonState,
    client: &C,
    token: RegistrationToken,
) -> Result<AgentDefinition>
where
    C: AgentInventoryClient + crate::registration::AgentRegistrationClient,
{
    let existing = existing_daemon_agent(state)?;
    let metadata = match client.verify_token(&token) {
        Ok(metadata) => Some(metadata),
        Err(error) => {
            if let Some(existing) = existing {
                if is_token_already_consumed_error(&error) {
                    return Ok(existing);
                }
            }
            return Err(error).context("verify daemon registration token");
        }
    };
    let metadata = metadata.context("verify daemon registration token")?;
    daemon_token_metadata_is_valid(&metadata)?;

    if let Some(existing) = existing {
        return recover_existing_daemon_agent(state, client, existing, metadata, token);
    }

    let handle = metadata
        .handle
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| {
            generate_product_handle("awiki-daemon-")
                .unwrap_or_else(|_| "awiki-daemon-local".to_string())
        });
    setup_daemon_agent(
        config,
        state,
        client,
        &handle,
        &metadata.controller_did,
        token,
    )
}

fn recover_existing_daemon_agent<C>(
    state: &DaemonState,
    client: &C,
    mut existing: AgentDefinition,
    metadata: RegistrationTokenMetadata,
    token: RegistrationToken,
) -> Result<AgentDefinition>
where
    C: crate::registration::AgentRegistrationClient,
{
    let identity = state.load_agent_identity(&existing.agent_did)?;
    let exchange = client.exchange_token(AgentRegistrationExchangeRequest {
        token,
        agent_kind: AgentKind::Daemon,
        controller_did: metadata.controller_did.clone(),
        handle: metadata
            .handle
            .clone()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| existing.handle.clone()),
        did_document: identity.did_document.clone(),
        endpoint_url: identity.endpoint_url.clone(),
        key_algorithm: identity.key_algorithm.clone(),
        public_key: identity.public_key.clone(),
        allow_existing_agent_did: true,
    })?;
    if exchange.did != existing.agent_did {
        bail!("registration token exchange returned a different DID");
    }
    if exchange.agent_kind != AgentKind::Daemon {
        bail!("registration token exchange returned a non-daemon agent kind");
    }
    if exchange.controller_did.trim().is_empty() {
        bail!("registration token exchange returned an empty controller_did");
    }
    let (local_agent_db_path, message_db_path) = agent_data_paths(&existing.agent_did)?;
    existing.handle = exchange.handle;
    existing.controller_did = exchange.controller_did;
    existing.local_agent_db_path = local_agent_db_path;
    existing.message_db_path = message_db_path;
    existing.status = "active".to_string();
    state.upsert_agent_definition(&existing)?;
    Ok(existing)
}

fn existing_daemon_agent(state: &DaemonState) -> Result<Option<AgentDefinition>> {
    Ok(state
        .list_agent_definitions()?
        .into_iter()
        .find(|agent| agent.agent_kind == AgentKind::Daemon))
}

fn daemon_token_metadata_is_valid(metadata: &RegistrationTokenMetadata) -> Result<()> {
    if metadata.agent_kind != AgentKind::Daemon {
        bail!("registration token is not for a daemon agent");
    }
    if metadata.controller_did.trim().is_empty() {
        bail!("registration token is missing controller_did");
    }
    Ok(())
}

fn is_token_already_consumed_error(error: &anyhow::Error) -> bool {
    let message = error.to_string().to_ascii_lowercase();
    message.contains("used") || message.contains("already")
}

#[cfg(test)]
fn install_or_recover_product_daemon_agent_for_test<C>(
    config: &DaemonConfig,
    state: &DaemonState,
    client: &C,
    token: RegistrationToken,
) -> Result<AgentDefinition>
where
    C: AgentInventoryClient + crate::registration::AgentRegistrationClient,
{
    install_or_recover_daemon_agent(config, state, client, token)
}

fn sync_one_agent_identity(
    config: &DaemonConfig,
    state: &DaemonState,
    im_core: &crate::ImCoreAdapter,
    agent: &AgentDefinition,
) -> Result<()> {
    let identity = state.load_agent_identity(&agent.agent_did)?;
    let jwt_token = state.load_agent_auth_token(&agent.agent_did)?;
    let _ = im_core.client_for_agent_identity(config, &identity, jwt_token.as_deref())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{agent_data_paths, generate_agent_identity, AgentKind};
    use crate::registration::{
        AgentRegistrationClient, AgentRegistrationExchangeRequest, AgentRegistrationExchangeResult,
    };
    use std::sync::{Arc, Mutex};

    #[derive(Debug, Clone)]
    struct MockInstallClient {
        verify_result: Arc<Mutex<Result<RegistrationTokenMetadata, String>>>,
        exchange_requests: Arc<Mutex<Vec<AgentRegistrationExchangeRequest>>>,
        latest_items: Arc<Mutex<Vec<AgentLatestStatusUpdateItem>>>,
    }

    impl MockInstallClient {
        fn active_daemon_token() -> Self {
            Self {
                verify_result: Arc::new(Mutex::new(Ok(RegistrationTokenMetadata {
                    token_id: "agtok_daemon".to_string(),
                    agent_kind: AgentKind::Daemon,
                    handle: Some("alice-mac-daemon".to_string()),
                    controller_user_id: Some("user-alice".to_string()),
                    controller_full_handle: Some("alice.anpclaw.com".to_string()),
                    controller_did: "did:human:alice".to_string(),
                    status: "active".to_string(),
                    scope: json!({}),
                }))),
                exchange_requests: Arc::new(Mutex::new(Vec::new())),
                latest_items: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn used_token() -> Self {
            let client = Self::active_daemon_token();
            *client.verify_result.lock().unwrap() = Err("used".to_string());
            client
        }

        fn exchange_count(&self) -> usize {
            self.exchange_requests.lock().unwrap().len()
        }

        fn latest_items(&self) -> Vec<AgentLatestStatusUpdateItem> {
            self.latest_items.lock().unwrap().clone()
        }
    }

    impl AgentRegistrationClient for MockInstallClient {
        fn exchange_token(
            &self,
            request: AgentRegistrationExchangeRequest,
        ) -> Result<AgentRegistrationExchangeResult> {
            self.exchange_requests.lock().unwrap().push(request.clone());
            let did = request
                .did_document
                .get("id")
                .and_then(Value::as_str)
                .unwrap()
                .to_string();
            Ok(AgentRegistrationExchangeResult {
                token_id: "agtok_daemon".to_string(),
                did,
                user_id: Some("agent-user-1".to_string()),
                agent_kind: request.agent_kind,
                controller_user_id: "user-alice".to_string(),
                controller_full_handle: "alice.anpclaw.com".to_string(),
                controller_did: request.controller_did,
                handle: request.handle,
                status: "registered".to_string(),
            })
        }
    }

    impl AgentInventoryClient for MockInstallClient {
        fn verify_token(&self, _token: &RegistrationToken) -> Result<RegistrationTokenMetadata> {
            match &*self.verify_result.lock().unwrap() {
                Ok(metadata) => Ok(metadata.clone()),
                Err(reason) => anyhow::bail!("agent registration token verify failed: {reason}"),
            }
        }

        fn sync_controller_scope(
            &self,
            daemon_agent_did: &str,
            _auth: &DidAuthMaterial,
        ) -> Result<Value> {
            Ok(json!({
                "agent_did": daemon_agent_did,
                "controller_user_id": "user-alice",
                "controller_full_handle": "alice.anpclaw.com",
                "controller_did": "did:human:alice",
                "updated_count": 1,
            }))
        }

        fn update_latest_status(
            &self,
            daemon_agent_did: &str,
            statuses: Vec<AgentLatestStatusUpdateItem>,
            _auth: &DidAuthMaterial,
        ) -> Result<Value> {
            self.latest_items.lock().unwrap().extend(statuses);
            Ok(json!({
                "updated": [{
                    "agent_did": daemon_agent_did,
                    "controller_did": "did:human:alice",
                    "status": "ready",
                }]
            }))
        }
    }

    fn fixture() -> (tempfile::TempDir, DaemonConfig, DaemonState) {
        let root = tempfile::tempdir().unwrap();
        let config = DaemonConfig::for_state_root(root.path()).unwrap();
        config.ensure_state_layout().unwrap();
        let state = DaemonState::open(&config).unwrap();
        state.initialize().unwrap();
        (root, config, state)
    }

    fn store_existing_daemon(config: &DaemonConfig, state: &DaemonState) -> AgentDefinition {
        let identity = generate_agent_identity(config, AgentKind::Daemon, "alice-mac-daemon")
            .unwrap()
            .into_record("alice-mac-daemon".to_string(), AgentKind::Daemon);
        let agent_did = identity.agent_did.clone();
        state.store_agent_identity(&identity).unwrap();
        let (local_agent_db_path, message_db_path) = agent_data_paths(&agent_did).unwrap();
        let definition = AgentDefinition {
            agent_did,
            handle: "alice-mac-daemon".to_string(),
            agent_kind: AgentKind::Daemon,
            controller_user_id: "user-alice".to_string(),
            controller_full_handle: "alice.anpclaw.com".to_string(),
            controller_scope_key: "controller-scope:v1:test-alice-anpclaw-com".to_string(),
            controller_did: "did:human:alice".to_string(),
            runtime_plugin_id: None,
            runtime_profile_id: None,
            workspace_id: None,
            policy_id: "default".to_string(),
            local_agent_db_path,
            message_db_path,
            status: "active".to_string(),
        };
        state.upsert_agent_definition(&definition).unwrap();
        definition
    }

    #[test]
    fn product_install_exchanges_active_daemon_token() {
        let (_root, config, state) = fixture();
        let client = MockInstallClient::active_daemon_token();

        let agent = install_or_recover_product_daemon_agent_for_test(
            &config,
            &state,
            &client,
            RegistrationToken::new("raw-token-long-enough").unwrap(),
        )
        .unwrap();

        assert_eq!(agent.agent_kind, AgentKind::Daemon);
        assert_eq!(agent.controller_did, "did:human:alice");
        assert_eq!(agent.handle, "alice-mac-daemon");
        assert_eq!(client.exchange_count(), 1);
    }

    #[test]
    fn product_install_recovers_existing_daemon_with_new_token() {
        let (_root, config, state) = fixture();
        let existing = store_existing_daemon(&config, &state);
        let client = MockInstallClient::active_daemon_token();

        let agent = install_or_recover_product_daemon_agent_for_test(
            &config,
            &state,
            &client,
            RegistrationToken::new("raw-token-long-enough").unwrap(),
        )
        .unwrap();

        let requests = client.exchange_requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert!(requests[0].allow_existing_agent_did);
        assert_eq!(requests[0].agent_kind, AgentKind::Daemon);
        assert_eq!(requests[0].controller_did, "did:human:alice");
        assert_eq!(
            requests[0].did_document.get("id").and_then(Value::as_str),
            Some(existing.agent_did.as_str())
        );
        assert_eq!(agent.agent_did, existing.agent_did);
        assert_eq!(
            state
                .load_agent_definition(&existing.agent_did)
                .unwrap()
                .controller_did,
            "did:human:alice"
        );
    }

    #[test]
    fn product_install_recovers_existing_daemon_when_token_already_used() {
        let (_root, config, state) = fixture();
        let existing = store_existing_daemon(&config, &state);
        let client = MockInstallClient::used_token();

        let agent = install_or_recover_product_daemon_agent_for_test(
            &config,
            &state,
            &client,
            RegistrationToken::new("raw-token-long-enough").unwrap(),
        )
        .unwrap();

        assert_eq!(agent.agent_did, existing.agent_did);
        assert_eq!(client.exchange_count(), 0);
    }

    #[test]
    fn latest_status_uses_daemon_did_auth_and_contract_platform() {
        let (_root, config, state) = fixture();
        let agent = store_existing_daemon(&config, &state);
        let im_core = crate::ImCoreAdapter::open(&config).unwrap();
        sync_one_agent_identity(&config, &state, &im_core, &agent).unwrap();
        let client = MockInstallClient::active_daemon_token();
        let service = ServiceStatus {
            platform: ServicePlatform::Foreground,
            installed: false,
            running: true,
            unit_path: None,
            detail: None,
        };

        update_daemon_latest_status(&client, &config, &state, &agent, &service).unwrap();

        let items = client.latest_items();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].agent_did, agent.agent_did);
        assert_eq!(items[0].agent_kind, AgentKind::Daemon);
        assert_eq!(items[0].status, "ready");
        assert_eq!(items[0].service.as_deref(), Some("foreground"));
        assert!(
            matches!(
                items[0].platform.as_deref(),
                Some("darwin-arm64")
                    | Some("darwin-amd64")
                    | Some("linux-arm64")
                    | Some("linux-amd64")
            ),
            "unexpected platform {:?}",
            items[0].platform
        );
        assert_eq!(
            items[0].diagnostics_summary["installation_status"],
            "not_installed"
        );
        assert_eq!(items[0].diagnostics_summary["runner_status"], "running");
        assert_eq!(
            items[0].diagnostics_summary["config_summary"]["service_installed"],
            false
        );
        assert!(items[0].diagnostics_summary["service_installed"].is_null());
        assert!(!items[0].diagnostics_summary.to_string().contains("token"));
    }
}

fn update_daemon_latest_status(
    client: &impl AgentInventoryClient,
    config: &DaemonConfig,
    state: &DaemonState,
    agent: &AgentDefinition,
    service: &ServiceStatus,
) -> Result<()> {
    let auth_paths = crate::im_core_adapter::agent_identity_auth_paths(config, &agent.agent_did);
    let auth = DidAuthMaterial {
        did_document_path: auth_paths.0,
        private_key_path: auth_paths.1,
        bearer_token: state.load_agent_auth_token(&agent.agent_did)?,
    };
    let response = client.update_latest_status(
        &agent.agent_did,
        vec![AgentLatestStatusUpdateItem {
            agent_did: agent.agent_did.clone(),
            agent_kind: AgentKind::Daemon,
            status: "ready".to_string(),
            last_seen_at: None,
            version: Some(env!("CARGO_PKG_VERSION").to_string()),
            min_supported_version: Some("0.1.0".to_string()),
            platform: Some(current_platform_label()),
            service: Some(
                match service.platform {
                    ServicePlatform::LaunchAgent => "launch_agent",
                    ServicePlatform::SystemdUser => "systemd_user",
                    ServicePlatform::Foreground => "foreground",
                    ServicePlatform::Unsupported => "unsupported",
                }
                .to_string(),
            ),
            needs_upgrade: false,
            needs_config: false,
            last_error_code: None,
            last_error_summary: None,
            diagnostics_summary: json!({
                "installation_status": if service.installed { "installed" } else { "not_installed" },
                "runner_status": if service.running { "running" } else { "not_running" },
                "config_summary": {
                    "service_installed": service.installed,
                },
            }),
        }],
        &auth,
    )?;
    crate::agent_status::sync_controller_did_from_latest_response(
        state,
        &agent.agent_did,
        &response,
    )?;
    Ok(())
}
