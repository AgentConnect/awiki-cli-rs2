use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{bail, Context, Result};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::agent::{agent_data_paths, AgentDefinition, AgentIdentityRecord, AgentKind};
use crate::runtime::{RuntimeAgentProfile, RuntimeRun, RuntimeRunStatus, RuntimeTask};
use crate::security::runtime_token::{
    current_time_millis, IssuedRuntimeToken, RpcMethod, RuntimeRpcToken, RuntimeTokenScope,
};
use crate::workspace::{WorkspaceBindingConfig, WorkspaceMode};
use crate::DaemonConfig;

mod delegated_state;
mod records;
mod row_mappers;
mod runtime_auth;
mod runtime_profiles;
mod runtime_tasks;
mod schema;

pub use records::*;
pub use runtime_auth::AuthorizedRuntimeContext;
pub use schema::current_schema_version;

use records::{stable_id_suffix, validate_control_command_status};
use schema::initialize_schema;

#[derive(Debug, Clone)]
pub struct DaemonState {
    database_path: PathBuf,
}

impl DaemonState {
    pub fn open(config: &DaemonConfig) -> Result<Self> {
        Ok(Self {
            database_path: config.daemon_db_path.clone(),
        })
    }

    pub fn initialize(&self) -> Result<StateSummary> {
        if let Some(parent) = self.database_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("create state directory {}", parent.display()))?;
        }
        let connection = Connection::open(&self.database_path)
            .with_context(|| format!("open daemon database {}", self.database_path.display()))?;
        initialize_schema(&connection)?;

        Ok(StateSummary {
            database_path: self.database_path.clone(),
            schema_version: current_schema_version(&connection)?,
        })
    }

    pub fn connection(&self) -> Result<Connection> {
        Connection::open(&self.database_path)
            .with_context(|| format!("open daemon database {}", self.database_path.display()))
    }
}

#[cfg(test)]
mod tests;
