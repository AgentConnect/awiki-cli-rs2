use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::agent::{agent_data_paths, AgentDefinition, AgentIdentityRecord, AgentKind};
use crate::runtime::{RuntimeAgentProfile, RuntimeRun, RuntimeRunStatus, RuntimeTask};
use crate::secret_vault::DaemonSecretVault;
use crate::security::runtime_token::{
    current_time_millis, IssuedRuntimeToken, RpcMethod, RuntimeRpcToken, RuntimeTokenScope,
};
use crate::workspace::{WorkspaceBindingConfig, WorkspaceMode};
use crate::DaemonConfig;
use im_core::vault::DEVICE_VAULT_ROOT_KEY_LEN;

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
    secret_vault: Option<Arc<DaemonSecretVault>>,
}

impl DaemonState {
    pub fn open(config: &DaemonConfig) -> Result<Self> {
        Ok(Self {
            database_path: config.daemon_db_path.clone(),
            secret_vault: Some(Arc::new(
                DaemonSecretVault::from_config(config).context("open daemon secret vault")?,
            )),
        })
    }

    pub fn open_with_secret_vault(config: &DaemonConfig, secret_vault: DaemonSecretVault) -> Self {
        Self {
            database_path: config.daemon_db_path.clone(),
            secret_vault: Some(Arc::new(secret_vault)),
        }
    }

    pub fn open_with_root_key_bytes(
        config: &DaemonConfig,
        bytes: [u8; DEVICE_VAULT_ROOT_KEY_LEN],
    ) -> Self {
        Self::open_with_secret_vault(
            config,
            DaemonSecretVault::from_root_key_bytes(config, bytes),
        )
    }

    #[cfg(test)]
    pub(crate) fn open_without_secret_vault_for_legacy(config: &DaemonConfig) -> Self {
        Self {
            database_path: config.daemon_db_path.clone(),
            secret_vault: None,
        }
    }

    pub fn initialize(&self) -> Result<StateSummary> {
        if let Some(parent) = self.database_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("create state directory {}", parent.display()))?;
        }
        let connection = Connection::open(&self.database_path)
            .with_context(|| format!("open daemon database {}", self.database_path.display()))?;
        initialize_schema(&connection)?;
        drop(connection);
        self.ensure_cli_route_hash_salt()?;
        let connection = self.connection()?;

        Ok(StateSummary {
            database_path: self.database_path.clone(),
            schema_version: current_schema_version(&connection)?,
        })
    }

    pub fn connection(&self) -> Result<Connection> {
        let connection = Connection::open(&self.database_path)
            .with_context(|| format!("open daemon database {}", self.database_path.display()))?;
        connection.execute_batch(
            r#"
            PRAGMA foreign_keys = ON;
            PRAGMA secure_delete = ON;
            "#,
        )?;
        Ok(connection)
    }

    pub(crate) fn secret_vault(&self) -> Option<&DaemonSecretVault> {
        self.secret_vault.as_deref()
    }
}

#[cfg(test)]
mod tests;
