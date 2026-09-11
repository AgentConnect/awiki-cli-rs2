use std::sync::OnceLock;

use anyhow::Result;

use crate::plugins::acp::connection::AcpProcessPool;
use crate::plugins::acp::runner::AcpRuntimePlugin;
use crate::plugins::acp::ACP_RUNTIME_PLUGIN_ID;
use crate::plugins::generic_cli::{GenericCliDriverRegistry, GENERIC_CLI_RUNTIME_PLUGIN_ID};
use crate::plugins::hermes::{
    repair_hermes_profile_if_needed, HermesGateway, HermesRuntimePlugin, HERMES_RUNTIME_PLUGIN_ID,
};
use crate::runtime::{RuntimeAgentProfile, RuntimePlugin};
use crate::{DaemonConfig, DaemonState};

static ACP_PROCESS_POOL: OnceLock<AcpProcessPool> = OnceLock::new();

pub fn with_runtime_plugin<G, F, R>(
    config: &DaemonConfig,
    state: &DaemonState,
    profile: &RuntimeAgentProfile,
    hermes_gateway: G,
    operation: F,
) -> Result<Option<R>>
where
    G: HermesGateway + Clone,
    F: FnOnce(&dyn RuntimePlugin) -> Result<R>,
{
    match profile.runtime_plugin_id.as_str() {
        HERMES_RUNTIME_PLUGIN_ID => {
            let definition = state.load_agent_definition(&profile.agent_did)?;
            let hermes_profile = match repair_hermes_profile_if_needed(
                config,
                state,
                profile,
                &definition.handle,
            )? {
                Some(repaired) => repaired.record,
                None => state.load_hermes_profile(&profile.agent_did)?,
            };
            let plugin =
                HermesRuntimePlugin::with_state(hermes_gateway, hermes_profile, state.clone());
            operation(&plugin).map(Some)
        }
        GENERIC_CLI_RUNTIME_PLUGIN_ID => {
            let cli_profile = state.load_cli_runtime_profile(&profile.runtime_profile_id)?;
            let plugin = GenericCliDriverRegistry::new(cli_profile);
            operation(&plugin).map(Some)
        }
        ACP_RUNTIME_PLUGIN_ID => {
            let acp_profile = state.load_acp_runtime_profile(&profile.runtime_profile_id)?;
            let pool = ACP_PROCESS_POOL
                .get_or_init(AcpProcessPool::default)
                .clone();
            let plugin = AcpRuntimePlugin::with_state(pool, acp_profile, state.clone());
            operation(&plugin).map(Some)
        }
        _ => Ok(None),
    }
}
