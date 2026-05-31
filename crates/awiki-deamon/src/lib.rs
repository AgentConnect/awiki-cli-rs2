pub mod agent;
pub mod cli_wrapper;
pub mod commands;
pub mod config;
pub mod daemon_cli;
pub mod foreground;
pub mod im_core_adapter;
pub mod inbox;
pub mod local_rpc;
pub mod outbox;
pub mod plugins;
pub mod registration;
pub mod runtime;
pub mod security;
pub mod state;
pub mod workspace;

use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub use crate::config::{DaemonConfig, DaemonConfigFile, IdentitySelectorConfig};
pub use crate::im_core_adapter::ImCoreAdapter;
pub use crate::state::{DaemonState, StateSummary};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DaemonCommand {
    AgentList {
        state_root: PathBuf,
    },
    AgentStatus {
        state_root: PathBuf,
        agent_did: String,
    },
    Foreground {
        options: crate::foreground::ForegroundOptions,
    },
    InitState {
        state_root: PathBuf,
    },
    RuntimeList {
        state_root: PathBuf,
    },
    SetupDaemonAgent {
        state_root: PathBuf,
        options: crate::daemon_cli::SetupDaemonAgentOptions,
    },
    Status {
        state_root: PathBuf,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum DaemonCommandOutput {
    Status(DaemonStatus),
    AgentList(crate::daemon_cli::AgentListOutput),
    AgentStatus(crate::daemon_cli::AgentStatusOutput),
    Foreground(crate::foreground::ForegroundRunSummary),
    RuntimeList(crate::daemon_cli::AgentListOutput),
    SetupDaemonAgent(crate::daemon_cli::SetupDaemonAgentOutput),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DaemonStatus {
    pub state_root: PathBuf,
    pub database_path: PathBuf,
    pub local_socket_path: PathBuf,
    pub im_core_sqlite_path: PathBuf,
    pub daemon_schema_version: i64,
    pub im_core_schema_version: Option<u32>,
}

pub fn run_command(command: DaemonCommand) -> Result<DaemonStatus> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("create daemon runtime")?;
    match runtime.block_on(run_command_async(command))? {
        DaemonCommandOutput::Status(status) => Ok(status),
        _ => anyhow::bail!("command does not return daemon status"),
    }
}

pub fn run_command_json(command: DaemonCommand) -> Result<Value> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("create daemon runtime")?;
    let output = runtime.block_on(run_command_async(command))?;
    command_output_json(output)
}

fn command_output_json(output: DaemonCommandOutput) -> Result<Value> {
    match output {
        DaemonCommandOutput::Status(status) => Ok(serde_json::to_value(status)?),
        DaemonCommandOutput::AgentList(output) | DaemonCommandOutput::RuntimeList(output) => {
            Ok(serde_json::to_value(output)?)
        }
        DaemonCommandOutput::AgentStatus(output) => Ok(serde_json::to_value(output)?),
        DaemonCommandOutput::Foreground(output) => Ok(serde_json::to_value(output)?),
        DaemonCommandOutput::SetupDaemonAgent(output) => Ok(serde_json::to_value(output)?),
    }
}

pub async fn run_command_async(command: DaemonCommand) -> Result<DaemonCommandOutput> {
    match command {
        DaemonCommand::Foreground { options } => Ok(DaemonCommandOutput::Foreground(
            crate::foreground::run_foreground(options).await?,
        )),
        DaemonCommand::InitState { state_root } | DaemonCommand::Status { state_root } => Ok(
            DaemonCommandOutput::Status(initialize_and_report(state_root).await?),
        ),
        DaemonCommand::AgentList { state_root } => {
            let (_config, state, _status) = initialize_state_for_management(state_root).await?;
            let output = crate::daemon_cli::list_agents(&state)?;
            Ok(DaemonCommandOutput::AgentList(output))
        }
        DaemonCommand::AgentStatus {
            state_root,
            agent_did,
        } => {
            let (_config, state, _status) = initialize_state_for_management(state_root).await?;
            let output = crate::daemon_cli::agent_status(&state, &agent_did)?;
            Ok(DaemonCommandOutput::AgentStatus(output))
        }
        DaemonCommand::RuntimeList { state_root } => {
            let (_config, state, _status) = initialize_state_for_management(state_root).await?;
            let output = crate::daemon_cli::list_runtime_agents(&state)?;
            Ok(DaemonCommandOutput::RuntimeList(output))
        }
        DaemonCommand::SetupDaemonAgent {
            state_root,
            options,
        } => {
            let (config, state, _status) = initialize_state_for_management(state_root).await?;
            let output =
                crate::daemon_cli::setup_daemon_agent_from_token(&config, &state, options)?;
            Ok(DaemonCommandOutput::SetupDaemonAgent(output))
        }
    }
}

async fn initialize_and_report(state_root: PathBuf) -> Result<DaemonStatus> {
    let config = DaemonConfig::for_state_root(state_root)?;
    config.validate()?;
    config.ensure_state_layout()?;
    let state = DaemonState::open(&config)?;
    let state_summary = state.initialize()?;
    let im_core_status = ImCoreAdapter::open(&config)?
        .initialize_local_state()
        .await
        .context("initialize im-core local state")?;

    Ok(DaemonStatus {
        state_root: config.state_root,
        database_path: state_summary.database_path,
        local_socket_path: config.local_socket_path,
        im_core_sqlite_path: config.im_core_sqlite_path,
        daemon_schema_version: state_summary.schema_version,
        im_core_schema_version: im_core_status.schema_version,
    })
}

async fn initialize_state_for_management(
    state_root: PathBuf,
) -> Result<(DaemonConfig, DaemonState, DaemonStatus)> {
    let config = DaemonConfig::for_state_root(state_root)?;
    config.validate()?;
    config.ensure_state_layout()?;
    let state = DaemonState::open(&config)?;
    let state_summary = state.initialize()?;
    let im_core_status = ImCoreAdapter::open(&config)?
        .initialize_local_state()
        .await
        .context("initialize im-core local state")?;
    let status = DaemonStatus {
        state_root: config.state_root.clone(),
        database_path: state_summary.database_path,
        local_socket_path: config.local_socket_path.clone(),
        im_core_sqlite_path: config.im_core_sqlite_path.clone(),
        daemon_schema_version: state_summary.schema_version,
        im_core_schema_version: im_core_status.schema_version,
    };
    Ok((config, state, status))
}
