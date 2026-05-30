pub mod cli_wrapper;
pub mod config;
pub mod im_core_adapter;
pub mod inbox;
pub mod local_rpc;
pub mod outbox;
pub mod plugins;
pub mod runtime;
pub mod security;
pub mod state;
pub mod workspace;

use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

pub use crate::config::{DaemonConfig, DaemonConfigFile, IdentitySelectorConfig};
pub use crate::im_core_adapter::ImCoreAdapter;
pub use crate::state::{DaemonState, StateSummary};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DaemonCommand {
    Foreground { state_root: PathBuf },
    InitState { state_root: PathBuf },
    Status { state_root: PathBuf },
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
    runtime.block_on(run_command_async(command))
}

pub async fn run_command_async(command: DaemonCommand) -> Result<DaemonStatus> {
    match command {
        DaemonCommand::Foreground { state_root }
        | DaemonCommand::InitState { state_root }
        | DaemonCommand::Status { state_root } => initialize_and_report(state_root).await,
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
