use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::agent::AgentDefinition;
use crate::commands::setup_daemon_agent;
use crate::plugins::hermes::{HermesGateway, HERMES_RUNTIME_PLUGIN_ID};
use crate::registration::{RegistrationToken, UserServiceAgentRegistrationClient};
use crate::runtime::RuntimeInstallStatus;
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

pub fn agent_status(state: &DaemonState, agent_did: &str) -> Result<AgentStatusOutput> {
    if agent_did.trim().is_empty() {
        bail!("--agent-did is required");
    }
    let agent = state.load_agent_definition(agent_did)?;
    let hermes = if agent.runtime_plugin_id.as_deref() == Some(HERMES_RUNTIME_PLUGIN_ID) {
        Some(hermes_status_for_agent(state, agent_did)?)
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

fn hermes_status_for_agent(state: &DaemonState, agent_did: &str) -> Result<HermesAgentStatus> {
    let profile = state.load_hermes_profile(agent_did)?;
    let installation =
        crate::plugins::hermes::StdioHermesGateway::from_env().check_installation()?;
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
