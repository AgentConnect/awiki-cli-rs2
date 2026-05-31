use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

use crate::agent::AgentDefinition;
use crate::commands::setup_daemon_agent;
use crate::registration::{RegistrationToken, UserServiceAgentRegistrationClient};
use crate::state::DaemonState;
use crate::DaemonConfig;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentListOutput {
    pub agents: Vec<AgentDefinition>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentStatusOutput {
    pub agent: AgentDefinition,
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
    Ok(AgentStatusOutput {
        agent: state.load_agent_definition(agent_did)?,
    })
}

pub fn list_runtime_agents(state: &DaemonState) -> Result<AgentListOutput> {
    Ok(AgentListOutput {
        agents: state.list_runtime_agent_definitions()?,
    })
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
