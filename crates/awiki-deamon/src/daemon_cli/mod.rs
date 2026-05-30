use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

use crate::agent::AgentDefinition;
use crate::state::DaemonState;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentListOutput {
    pub agents: Vec<AgentDefinition>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentStatusOutput {
    pub agent: AgentDefinition,
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
