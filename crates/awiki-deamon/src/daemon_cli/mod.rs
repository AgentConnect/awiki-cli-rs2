use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
#[cfg(test)]
use serde_json::json;
use serde_json::Value;

use std::path::PathBuf;

use crate::agent::{agent_data_paths, generate_product_handle, AgentDefinition, AgentKind};
use crate::commands::setup_daemon_agent;
use crate::plugins::hermes::{HermesGateway, HERMES_RUNTIME_PLUGIN_ID};
#[cfg(test)]
use crate::registration::DidAuthMaterial;
use crate::registration::{
    AgentInventoryClient, AgentLatestStatusUpdateItem, AgentRegistrationExchangeRequest,
    AgentRegistrationExchangeResult, RegistrationToken, RegistrationTokenMetadata,
    UserServiceAgentRegistrationClient,
};
use crate::runtime::RuntimeInstallStatus;
use crate::service::{
    current_platform_label, manage_service, require_service_state_root_is_product, ServiceAction,
    ServicePlatform, ServiceStatus,
};
use crate::state::{controller_scope_key, DaemonState};
use crate::upgrade::check_release_status;
use crate::{DaemonConfig, DaemonPersistentConfig};

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
    crate::cli_runtime_env::capture_and_write(config)
        .context("capture daemon CLI runtime environment")?;
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
    ensure_install_environment_matches_existing_state(&config)?;
    config.ensure_state_layout()?;
    config.write_persistent_config()?;
    crate::cli_runtime_env::capture_and_write(&config)
        .context("capture daemon CLI runtime environment")?;
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
        let executable = crate::service::product_current_executable_path()?;
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

fn ensure_install_environment_matches_existing_state(config: &DaemonConfig) -> Result<()> {
    let existing = DaemonPersistentConfig::read_optional(&config.config_file_path)?;
    let Some(existing_env) = PersistedEnvironment::from_persistent(&existing) else {
        return Ok(());
    };
    let target_env = PersistedEnvironment::from_config(config);
    if existing_env.matches_target(&target_env) {
        return Ok(());
    }
    bail!(
        "daemon_environment_mismatch\n\
         这个 state-root 已经属于另一个 AWiki 环境，不能直接复用安装。\n\n\
         state-root: {}\n\
         本机已有环境: {} / did_domain={} / anp_service_did={}\n\
         当前安装环境: {} / did_domain={} / anp_service_did={}\n\n\
         你可以这样处理：\n\
         1. 如果你想继续使用已有环境，请切换到对应 APP 环境后重新复制安装命令。\n\
         2. 如果你确实要切换环境，请先清理宿主机上的 AWiki Daemon 数据后再安装。\n\n\
         清理命令：\n\
           {}\n\n\
         注意：清理会删除宿主机上的所有 AWiki Daemon 本地数据，包括身份、数据库、日志、归档、Runtime Profile 和已下载的 Daemon 二进制。此操作不可恢复。",
        config.state_root.display(),
        existing_env.service_base_url,
        existing_env.did_domain,
        existing_env.anp_service_did,
        target_env.service_base_url,
        target_env.did_domain,
        target_env.anp_service_did,
        daemon_cleanup_command(&target_env.download_base_url),
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PersistedEnvironment {
    service_base_url: String,
    did_domain: String,
    anp_service_did: String,
    download_base_url: String,
}

impl PersistedEnvironment {
    fn from_config(config: &DaemonConfig) -> Self {
        Self {
            service_base_url: normalize_url_for_compare(&config.service_base_url),
            did_domain: normalize_token_for_compare(&config.did_domain),
            anp_service_did: normalize_token_for_compare(&config.anp_service_did),
            download_base_url: normalize_url_for_compare(&config.download_base_url),
        }
    }

    fn from_persistent(config: &DaemonPersistentConfig) -> Option<Self> {
        let service_base_url = config
            .base_url
            .as_deref()
            .map(normalize_url_for_compare)
            .filter(|value| !value.is_empty())?;
        let did_domain = config
            .did_domain
            .as_deref()
            .map(normalize_token_for_compare)
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| {
                service_host_from_base_url(&service_base_url)
                    .unwrap_or_else(|_| String::new())
                    .to_ascii_lowercase()
            });
        let anp_service_did = config
            .anp_service_did
            .as_deref()
            .map(normalize_token_for_compare)
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| format!("did:wba:{did_domain}"));
        let download_base_url = config
            .download_base_url
            .as_deref()
            .map(normalize_url_for_compare)
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| format!("{service_base_url}/daemon"));
        Some(Self {
            service_base_url,
            did_domain,
            anp_service_did,
            download_base_url,
        })
    }

    fn matches_target(&self, target: &Self) -> bool {
        self.service_base_url == target.service_base_url
            && self.did_domain == target.did_domain
            && self.anp_service_did == target.anp_service_did
    }
}

fn normalize_url_for_compare(value: &str) -> String {
    value.trim().trim_end_matches('/').to_ascii_lowercase()
}

fn normalize_token_for_compare(value: &str) -> String {
    value.trim().to_ascii_lowercase()
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
    let metadata = client
        .verify_token(&token)
        .context("verify daemon registration token")?;
    daemon_token_metadata_is_valid(&metadata)?;

    if let Some(existing) = existing {
        ensure_existing_daemon_matches_token_scope(config, &existing, &metadata)?;
        return recover_existing_daemon_agent(state, client, existing, metadata, token);
    }

    let handle = metadata
        .handle
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| {
            generate_product_handle("edgehost-").unwrap_or_else(|_| "edgehost-local".to_string())
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
        name: None,
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
    let exchange_scope_key = controller_scope_key(
        &exchange.controller_user_id,
        &exchange.controller_full_handle,
    )?;
    existing.handle = exchange.handle.clone();
    existing.controller_user_id = exchange.controller_user_id.clone();
    existing.controller_full_handle = exchange.controller_full_handle.clone();
    existing.controller_scope_key = exchange_scope_key;
    existing.controller_did = exchange.controller_did.clone();
    existing.local_agent_db_path = local_agent_db_path;
    existing.message_db_path = message_db_path;
    existing.status = "active".to_string();
    state.upsert_agent_definition(&existing)?;
    store_exchange_auth_token(state, &exchange)?;
    Ok(existing)
}

fn store_exchange_auth_token(
    state: &DaemonState,
    exchange: &AgentRegistrationExchangeResult,
) -> Result<()> {
    if let Some(token) = exchange
        .access_token
        .as_deref()
        .map(str::trim)
        .filter(|token| !token.is_empty())
    {
        state
            .store_agent_auth_token(&exchange.did, token)
            .context("store agent auth token from registration exchange")?;
    }
    Ok(())
}

fn existing_daemon_agent(state: &DaemonState) -> Result<Option<AgentDefinition>> {
    let daemons = state
        .list_agent_definitions()?
        .into_iter()
        .filter(|agent| agent.agent_kind == AgentKind::Daemon)
        .collect::<Vec<_>>();
    match daemons.len() {
        0 => Ok(None),
        1 => Ok(daemons.into_iter().next()),
        count => bail!(
            "daemon_local_state_conflict: this machine has {count} Daemon records. Reset the local Daemon state before installing."
        ),
    }
}

fn daemon_token_metadata_is_valid(metadata: &RegistrationTokenMetadata) -> Result<()> {
    if metadata.agent_kind != AgentKind::Daemon {
        bail!("registration token is not for a daemon agent");
    }
    if metadata.controller_did.trim().is_empty() {
        bail!("registration token is missing controller_did");
    }
    if metadata.controller_user_id.trim().is_empty() {
        bail!("registration token is missing controller_user_id");
    }
    if metadata.controller_full_handle.trim().is_empty() {
        bail!("registration token is missing controller_full_handle");
    }
    Ok(())
}

fn ensure_existing_daemon_matches_token_scope(
    config: &DaemonConfig,
    existing: &AgentDefinition,
    metadata: &RegistrationTokenMetadata,
) -> Result<()> {
    let token_controller_user_id = metadata.controller_user_id.trim();
    let token_controller_full_handle = metadata.controller_full_handle.trim();
    if existing.controller_user_id == token_controller_user_id
        && existing.controller_full_handle == token_controller_full_handle
    {
        return Ok(());
    }
    bail!(
        "daemon_controller_scope_mismatch\n\
         这台电脑已经安装了属于 @{} 的 Daemon。\n\
         当前安装命令属于 @{}，因此不能继续安装。\n\n\
         你可以这样处理：\n\
         1. 如果你想继续使用 @{}，请切换到对应账号后重新复制安装命令。\n\
         2. 如果你确实要改用 @{}，请先清理宿主机上的 AWiki Daemon 数据后再安装。\n\n\
         清理命令：\n\
           {}\n\n\
         注意：清理会删除宿主机上的所有 AWiki Daemon 本地数据，包括身份、数据库、日志、归档、Runtime Profile 和已下载的 Daemon 二进制。此操作不可恢复。\n\n\
         本机账号范围: {} / {}\n\
         安装命令范围: {} / {}",
        existing.controller_full_handle,
        token_controller_full_handle,
        existing.controller_full_handle,
        token_controller_full_handle,
        daemon_cleanup_command(&config.download_base_url),
        existing.controller_user_id,
        existing.controller_full_handle,
        token_controller_user_id,
        token_controller_full_handle,
    )
}

fn daemon_cleanup_command(download_base_url: &str) -> String {
    let base_url = download_base_url.trim().trim_end_matches('/');
    let base_url = if base_url.is_empty() {
        "https://awiki.ai/daemon"
    } else {
        base_url
    };
    format!("curl -fsSL {base_url}/cleanup.sh | sh")
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
        AgentInvocationAuthorization, AgentRegistrationClient, AgentRegistrationExchangeRequest,
        AgentRegistrationExchangeResult, ControllerSenderScope,
    };
    use std::sync::{Arc, Mutex};

    #[derive(Debug, Clone)]
    struct MockInstallClient {
        verify_result: Arc<Mutex<Result<RegistrationTokenMetadata, String>>>,
        exchange_requests: Arc<Mutex<Vec<AgentRegistrationExchangeRequest>>>,
        latest_items: Arc<Mutex<Vec<AgentLatestStatusUpdateItem>>>,
        exchange_scope: Arc<Mutex<(String, String)>>,
    }

    impl MockInstallClient {
        fn active_daemon_token() -> Self {
            Self {
                verify_result: Arc::new(Mutex::new(Ok(RegistrationTokenMetadata {
                    token_id: "agtok_daemon".to_string(),
                    agent_kind: AgentKind::Daemon,
                    handle: Some("alice-mac-daemon".to_string()),
                    controller_user_id: "user-alice".to_string(),
                    controller_full_handle: "alice.anpclaw.com".to_string(),
                    controller_did: "did:human:alice".to_string(),
                    status: "active".to_string(),
                    scope: json!({}),
                }))),
                exchange_requests: Arc::new(Mutex::new(Vec::new())),
                latest_items: Arc::new(Mutex::new(Vec::new())),
                exchange_scope: Arc::new(Mutex::new((
                    "user-alice".to_string(),
                    "alice.anpclaw.com".to_string(),
                ))),
            }
        }

        fn active_daemon_token_without_handle() -> Self {
            let client = Self::active_daemon_token();
            let mut metadata = match &*client.verify_result.lock().unwrap() {
                Ok(metadata) => metadata.clone(),
                Err(reason) => panic!("unexpected mock verify error: {reason}"),
            };
            metadata.handle = None;
            *client.verify_result.lock().unwrap() = Ok(metadata);
            client
        }

        fn active_daemon_token_for_scope(
            controller_user_id: &str,
            controller_full_handle: &str,
            controller_did: &str,
            handle: &str,
        ) -> Self {
            let client = Self::active_daemon_token();
            *client.verify_result.lock().unwrap() = Ok(RegistrationTokenMetadata {
                token_id: "agtok_daemon".to_string(),
                agent_kind: AgentKind::Daemon,
                handle: Some(handle.to_string()),
                controller_user_id: controller_user_id.to_string(),
                controller_full_handle: controller_full_handle.to_string(),
                controller_did: controller_did.to_string(),
                status: "active".to_string(),
                scope: json!({}),
            });
            *client.exchange_scope.lock().unwrap() = (
                controller_user_id.to_string(),
                controller_full_handle.to_string(),
            );
            client
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
            let (controller_user_id, controller_full_handle) =
                self.exchange_scope.lock().unwrap().clone();
            Ok(AgentRegistrationExchangeResult {
                token_id: "agtok_daemon".to_string(),
                did,
                user_id: Some("agent-user-1".to_string()),
                agent_kind: request.agent_kind,
                controller_user_id,
                controller_full_handle,
                controller_did: request.controller_did,
                handle: request.handle,
                status: "registered".to_string(),
                access_token: Some("jwt-agent-secret".to_string()),
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

        fn verify_controller_sender(
            &self,
            _daemon_agent_did: &str,
            sender_did: &str,
            _auth: &DidAuthMaterial,
        ) -> Result<ControllerSenderScope> {
            if sender_did == "did:human:alice" || sender_did == "did:human:alice-new" {
                Ok(ControllerSenderScope {
                    controller_user_id: "user-alice".to_string(),
                    controller_full_handle: "alice.anpclaw.com".to_string(),
                    controller_did: sender_did.to_string(),
                    sender_did: sender_did.to_string(),
                })
            } else {
                anyhow::bail!("controller_scope_mismatch")
            }
        }

        fn authorize_agent_invocation(
            &self,
            _daemon_agent_did: &str,
            _agent_did: &str,
            _sender_did: &str,
            _source_conversation_id: Option<&str>,
            _source_message_id: Option<&str>,
            _auth: &DidAuthMaterial,
        ) -> Result<AgentInvocationAuthorization> {
            anyhow::bail!("authorize_agent_invocation is not used in daemon CLI tests")
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

        fn archive_agent(
            &self,
            _daemon_agent_did: &str,
            _agent_did: &str,
            _auth: &DidAuthMaterial,
        ) -> Result<Value> {
            Ok(json!({ "archived": [] }))
        }
    }

    fn fixture() -> (tempfile::TempDir, DaemonConfig, DaemonState) {
        let root = tempfile::tempdir().unwrap();
        let config = DaemonConfig::for_state_root(root.path()).unwrap();
        config.ensure_state_layout().unwrap();
        let state = DaemonState::open_with_root_key_bytes(&config, [23_u8; 32]);
        state.initialize().unwrap();
        (root, config, state)
    }

    fn write_status_manifest(root: &std::path::Path, latest: &str) -> PathBuf {
        let releases = root.join("releases");
        std::fs::create_dir_all(&releases).unwrap();
        let manifest = releases.join("manifest.json");
        std::fs::write(
            &manifest,
            serde_json::to_vec_pretty(&json!({
                "latest": latest,
                "packages": []
            }))
            .unwrap(),
        )
        .unwrap();
        manifest
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

    fn store_existing_daemon_for_scope(
        config: &DaemonConfig,
        state: &DaemonState,
        handle: &str,
        controller_user_id: &str,
        controller_full_handle: &str,
        controller_did: &str,
    ) -> AgentDefinition {
        let identity = generate_agent_identity(config, AgentKind::Daemon, handle)
            .unwrap()
            .into_record(handle.to_string(), AgentKind::Daemon);
        let agent_did = identity.agent_did.clone();
        state.store_agent_identity(&identity).unwrap();
        let (local_agent_db_path, message_db_path) = agent_data_paths(&agent_did).unwrap();
        let controller_scope_key =
            crate::state::controller_scope_key(controller_user_id, controller_full_handle).unwrap();
        let definition = AgentDefinition {
            agent_did,
            handle: handle.to_string(),
            agent_kind: AgentKind::Daemon,
            controller_user_id: controller_user_id.to_string(),
            controller_full_handle: controller_full_handle.to_string(),
            controller_scope_key,
            controller_did: controller_did.to_string(),
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
        assert_eq!(
            state
                .load_agent_auth_token(&agent.agent_did)
                .unwrap()
                .as_deref(),
            Some("jwt-agent-secret")
        );
    }

    #[test]
    fn product_install_generates_policy_safe_handle_when_token_has_no_handle() {
        let (_root, config, state) = fixture();
        let client = MockInstallClient::active_daemon_token_without_handle();

        let agent = install_or_recover_product_daemon_agent_for_test(
            &config,
            &state,
            &client,
            RegistrationToken::new("raw-token-long-enough").unwrap(),
        )
        .unwrap();

        assert_eq!(agent.agent_kind, AgentKind::Daemon);
        assert!(agent.handle.starts_with("edgehost-"));
        assert!(!agent.handle.starts_with("awiki-"));
        let requests = client.exchange_requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].handle, agent.handle);
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
        let recovered = state.load_agent_definition(&existing.agent_did).unwrap();
        assert_eq!(
            state
                .load_agent_auth_token(&existing.agent_did)
                .unwrap()
                .as_deref(),
            Some("jwt-agent-secret")
        );
        assert_eq!(recovered.controller_user_id, "user-alice");
        assert_eq!(recovered.controller_full_handle, "alice.anpclaw.com");
        assert_eq!(recovered.controller_did, "did:human:alice");
        assert_eq!(
            recovered.controller_scope_key,
            crate::state::controller_scope_key("user-alice", "alice.anpclaw.com").unwrap()
        );
    }

    #[test]
    fn product_install_recovers_existing_daemon_when_controller_did_rotates() {
        let (_root, config, state) = fixture();
        let existing = store_existing_daemon(&config, &state);
        let client = MockInstallClient::active_daemon_token_for_scope(
            "user-alice",
            "alice.anpclaw.com",
            "did:human:alice-new",
            "alice-mac-daemon",
        );

        let agent = install_or_recover_product_daemon_agent_for_test(
            &config,
            &state,
            &client,
            RegistrationToken::new("raw-token-long-enough").unwrap(),
        )
        .unwrap();

        assert_eq!(agent.agent_did, existing.agent_did);
        assert_eq!(agent.controller_did, "did:human:alice-new");
        assert_eq!(client.exchange_count(), 1);
    }

    #[test]
    fn product_install_rejects_existing_daemon_with_different_controller_handle() {
        let (_root, config, state) = fixture();
        let existing = store_existing_daemon(&config, &state);
        let client = MockInstallClient::active_daemon_token_for_scope(
            "user-alice",
            "alice-alt.anpclaw.com",
            "did:human:alice-alt",
            "alice-alt-mac-daemon",
        );

        let error = install_or_recover_product_daemon_agent_for_test(
            &config,
            &state,
            &client,
            RegistrationToken::new("raw-token-long-enough").unwrap(),
        )
        .unwrap_err();

        let message = error.to_string();
        assert!(message.contains("daemon_controller_scope_mismatch"));
        assert!(message.contains("这台电脑已经安装了属于 @alice.anpclaw.com 的 Daemon"));
        assert!(message.contains("当前安装命令属于 @alice-alt.anpclaw.com"));
        assert!(message.contains("请先清理宿主机上的 AWiki Daemon 数据后再安装"));
        assert!(message.contains("curl -fsSL https://awiki.ai/daemon/cleanup.sh | sh"));
        assert!(message.contains("此操作不可恢复"));
        assert!(message.contains("@alice.anpclaw.com"));
        assert!(message.contains("@alice-alt.anpclaw.com"));
        assert_eq!(client.exchange_count(), 0);
        let unchanged = state.load_agent_definition(&existing.agent_did).unwrap();
        assert_eq!(unchanged.controller_user_id, "user-alice");
        assert_eq!(unchanged.controller_full_handle, "alice.anpclaw.com");
    }

    #[test]
    fn product_install_rejects_existing_daemon_with_different_controller_user() {
        let (_root, config, state) = fixture();
        store_existing_daemon(&config, &state);
        let client = MockInstallClient::active_daemon_token_for_scope(
            "user-bob",
            "bob.anpclaw.com",
            "did:human:bob",
            "bob-mac-daemon",
        );

        let error = install_or_recover_product_daemon_agent_for_test(
            &config,
            &state,
            &client,
            RegistrationToken::new("raw-token-long-enough").unwrap(),
        )
        .unwrap_err();

        let message = error.to_string();
        assert!(message.contains("daemon_controller_scope_mismatch"));
        assert!(message.contains("这台电脑已经安装了属于 @alice.anpclaw.com 的 Daemon"));
        assert!(message.contains("当前安装命令属于 @bob.anpclaw.com"));
        assert!(message.contains("@alice.anpclaw.com"));
        assert!(message.contains("@bob.anpclaw.com"));
        assert_eq!(client.exchange_count(), 0);
    }

    #[test]
    fn product_install_rejects_existing_daemon_when_token_already_used() {
        let (_root, config, state) = fixture();
        store_existing_daemon(&config, &state);
        let client = MockInstallClient::used_token();

        let error = install_or_recover_product_daemon_agent_for_test(
            &config,
            &state,
            &client,
            RegistrationToken::new("raw-token-long-enough").unwrap(),
        )
        .unwrap_err();

        assert!(error
            .to_string()
            .contains("verify daemon registration token"));
        assert_eq!(client.exchange_count(), 0);
    }

    #[test]
    fn product_install_rejects_local_state_with_multiple_daemons() {
        let (_root, config, state) = fixture();
        store_existing_daemon_for_scope(
            &config,
            &state,
            "alice-mac-daemon",
            "user-alice",
            "alice.anpclaw.com",
            "did:human:alice",
        );
        store_existing_daemon_for_scope(
            &config,
            &state,
            "bob-mac-daemon",
            "user-bob",
            "bob.anpclaw.com",
            "did:human:bob",
        );
        let client = MockInstallClient::active_daemon_token();

        let error = install_or_recover_product_daemon_agent_for_test(
            &config,
            &state,
            &client,
            RegistrationToken::new("raw-token-long-enough").unwrap(),
        )
        .unwrap_err();

        let message = error.to_string();
        assert!(message.contains("daemon_local_state_conflict"));
        assert!(message.contains("2 Daemon records"));
        assert_eq!(client.exchange_count(), 0);
    }

    #[test]
    fn product_install_environment_guard_rejects_cross_environment_state() {
        let (root, mut config, _state) = fixture();
        config.service_base_url = "https://awiki.ai".to_string();
        config.user_service_base_url = "https://awiki.ai".to_string();
        config.message_service_base_url = "https://awiki.ai".to_string();
        config.mail_service_base_url = "https://awiki.ai".to_string();
        config.download_base_url = "https://awiki.ai/daemon".to_string();
        config.did_domain = "awiki.ai".to_string();
        config.anp_service_endpoint = "https://awiki.ai/anp-im/rpc".to_string();
        config.anp_service_did = "did:wba:awiki.ai".to_string();
        config.write_persistent_config().unwrap();
        let before = std::fs::read_to_string(&config.config_file_path).unwrap();

        let mut target = DaemonConfig::for_state_root(root.path()).unwrap();
        target.service_base_url = "https://anpclaw.com".to_string();
        target.user_service_base_url = target.service_base_url.clone();
        target.message_service_base_url = target.service_base_url.clone();
        target.mail_service_base_url = target.service_base_url.clone();
        target.download_base_url = "https://anpclaw.com/daemon".to_string();
        target.did_domain = "anpclaw.com".to_string();
        target.anp_service_endpoint = "https://anpclaw.com/anp-im/rpc".to_string();
        target.anp_service_did = "did:wba:anpclaw.com".to_string();

        let error = ensure_install_environment_matches_existing_state(&target).unwrap_err();

        let message = error.to_string();
        assert!(message.contains("daemon_environment_mismatch"));
        assert!(message.contains("本机已有环境: https://awiki.ai"));
        assert!(message.contains("当前安装环境: https://anpclaw.com"));
        assert!(message.contains("did_domain=awiki.ai"));
        assert!(message.contains("did_domain=anpclaw.com"));
        assert!(message.contains("curl -fsSL https://anpclaw.com/daemon/cleanup.sh | sh"));
        assert_eq!(
            std::fs::read_to_string(&config.config_file_path).unwrap(),
            before
        );
    }

    #[test]
    fn product_install_environment_guard_accepts_same_environment() {
        let (_root, mut config, _state) = fixture();
        config.service_base_url = "https://awiki.ai".to_string();
        config.user_service_base_url = "https://awiki.ai".to_string();
        config.message_service_base_url = "https://awiki.ai".to_string();
        config.mail_service_base_url = "https://awiki.ai".to_string();
        config.download_base_url = "https://mirror.example/daemon".to_string();
        config.did_domain = "awiki.ai".to_string();
        config.anp_service_endpoint = "https://awiki.ai/anp-im/rpc".to_string();
        config.anp_service_did = "did:wba:awiki.ai".to_string();
        config.write_persistent_config().unwrap();

        let mut target = config.clone();
        target.service_base_url = "https://awiki.ai/".trim_end_matches('/').to_string();
        target.download_base_url = "https://awiki.ai/daemon".to_string();

        ensure_install_environment_matches_existing_state(&target).unwrap();
    }

    #[test]
    fn latest_status_uses_daemon_did_auth_and_contract_platform() {
        let (root, mut config, state) = fixture();
        write_status_manifest(root.path(), crate::upgrade::CURRENT_DAEMON_VERSION);
        config.download_base_url = format!("file://{}", root.path().display());
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
        assert!(items[0].diagnostics_summary["bootstrap_public_key_b64u"].is_null());
        assert!(
            items[0].diagnostics_summary["config_summary"]["bootstrap_public_key_b64u"]
                .as_str()
                .is_some()
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
    let auth = crate::controller_scope::daemon_auth_material(config, state, agent)?;
    let release = check_release_status(config);
    let response = client.update_latest_status(
        &agent.agent_did,
        vec![AgentLatestStatusUpdateItem {
            agent_did: agent.agent_did.clone(),
            agent_kind: AgentKind::Daemon,
            status: if release.needs_upgrade {
                "needs_upgrade"
            } else {
                "ready"
            }
            .to_string(),
            last_seen_at: None,
            version: Some(release.current_version.clone()),
            latest_version: release.latest_version.clone(),
            min_supported_version: None,
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
            needs_upgrade: release.needs_upgrade,
            needs_config: false,
            last_error_code: None,
            last_error_summary: None,
            diagnostics_summary: crate::agent_status::daemon_latest_diagnostics_summary(
                config, state, agent, service, &release,
            ),
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
