use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{bail, Context, Result};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::agent::{agent_data_paths, AgentDefinition, AgentIdentityRecord, AgentKind};
use crate::runtime::{RuntimeAgentProfile, RuntimeRun, RuntimeRunStatus, RuntimeTask};
use crate::security::runtime_token::{
    current_time_millis, IssuedRuntimeToken, RpcMethod, RuntimeRpcToken, RuntimeTokenScope,
};
use crate::workspace::WorkspaceBindingConfig;
use crate::DaemonConfig;

const DAEMON_SCHEMA_VERSION: i64 = 4;
static AUDIT_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone)]
pub struct DaemonState {
    database_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StateSummary {
    pub database_path: PathBuf,
    pub schema_version: i64,
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

    pub fn store_runtime_token(&self, issued: &IssuedRuntimeToken) -> Result<()> {
        let connection = self.connection()?;
        let allowed_methods_json = serde_json::to_string(&issued.scope.allowed_methods)?;
        let allowed_recipients_json = match issued.scope.allowed_recipients.as_ref() {
            Some(recipients) => Some(serde_json::to_string(recipients)?),
            None => None,
        };
        connection.execute(
            r#"
INSERT INTO runtime_rpc_tokens (
    token_id,
    token_secret_hash,
    agent_did,
    runtime_profile_id,
    run_id,
    allowed_methods_json,
    allowed_recipients_json,
    expires_at_ms,
    single_use,
    revoked_at_ms,
    used_at_ms,
    created_at_ms,
    expires_at,
    created_at
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, NULL, NULL, ?10, ?11, ?12)
"#,
            rusqlite::params![
                issued.token_id,
                issued.token.secret_hash(),
                issued.scope.agent_did,
                issued.scope.runtime_profile_id,
                issued.scope.run_id,
                allowed_methods_json,
                allowed_recipients_json,
                issued.scope.expires_at_ms,
                if issued.scope.single_use {
                    1_i64
                } else {
                    0_i64
                },
                current_time_millis()?,
                issued.scope.expires_at_ms.to_string(),
                current_time_millis()?.to_string(),
            ],
        )?;
        Ok(())
    }

    pub fn revoke_runtime_token(&self, token_id: &str) -> Result<()> {
        let connection = self.connection()?;
        connection.execute(
            "UPDATE runtime_rpc_tokens SET revoked_at_ms = ?1 WHERE token_id = ?2",
            rusqlite::params![current_time_millis()?, token_id],
        )?;
        Ok(())
    }

    pub fn upsert_runtime_agent_profile(&self, profile: &RuntimeAgentProfile) -> Result<()> {
        self.upsert_runtime_agent_profile_with_handle(profile, &profile.agent_did)
    }

    pub fn upsert_runtime_agent_profile_with_handle(
        &self,
        profile: &RuntimeAgentProfile,
        handle: &str,
    ) -> Result<()> {
        profile.validate()?;
        let (local_agent_db_path, message_db_path) = agent_data_paths(&profile.agent_did)?;
        let connection = self.connection()?;
        let now = current_time_millis()?.to_string();
        connection.execute(
            r#"
INSERT INTO agent_definition (
    agent_did,
    handle,
    agent_kind,
    controller_did,
    runtime_plugin_id,
    runtime_profile_id,
    workspace_id,
    policy_id,
    local_agent_db_path,
    message_db_path,
    status,
    created_at,
    updated_at
) VALUES (?1, ?2, 'runtime', ?3, ?4, ?5, ?6, 'default', ?7, ?8, 'active', ?9, ?9)
ON CONFLICT(agent_did) DO UPDATE SET
    controller_did = excluded.controller_did,
    handle = excluded.handle,
    agent_kind = excluded.agent_kind,
    runtime_plugin_id = excluded.runtime_plugin_id,
    runtime_profile_id = excluded.runtime_profile_id,
    workspace_id = excluded.workspace_id,
    local_agent_db_path = excluded.local_agent_db_path,
    message_db_path = excluded.message_db_path,
    status = 'active',
    updated_at = excluded.updated_at
"#,
            rusqlite::params![
                profile.agent_did,
                handle,
                profile.controller_did,
                profile.runtime_plugin_id,
                profile.runtime_profile_id,
                profile.workspace_id,
                local_agent_db_path,
                message_db_path,
                now,
            ],
        )?;
        connection.execute(
            r#"
INSERT INTO runtime_profile (
    runtime_profile_id,
    agent_did,
    runtime_plugin_id,
    display_name,
    status,
    created_at,
    updated_at
) VALUES (?1, ?2, ?3, ?4, 'active', ?5, ?5)
ON CONFLICT(runtime_profile_id) DO UPDATE SET
    agent_did = excluded.agent_did,
    runtime_plugin_id = excluded.runtime_plugin_id,
    display_name = excluded.display_name,
    status = 'active',
    updated_at = excluded.updated_at
"#,
            rusqlite::params![
                profile.runtime_profile_id,
                profile.agent_did,
                profile.runtime_plugin_id,
                profile.display_name,
                now,
            ],
        )?;
        if let (Some(workspace_id), Some(workspace_root), Some(workspace_mode)) = (
            profile.workspace_id.as_deref(),
            profile.workspace_root.as_ref(),
            profile.workspace_mode,
        ) {
            self.upsert_workspace_binding(
                &profile.agent_did,
                &profile.runtime_profile_id,
                &WorkspaceBindingConfig {
                    workspace_id: workspace_id.to_string(),
                    workspace_root: workspace_root.clone(),
                    workspace_mode,
                },
            )?;
        }
        Ok(())
    }

    pub fn upsert_agent_definition(&self, definition: &AgentDefinition) -> Result<()> {
        definition.validate()?;
        let connection = self.connection()?;
        let now = current_time_millis()?.to_string();
        connection.execute(
            r#"
INSERT INTO agent_definition (
    agent_did,
    handle,
    agent_kind,
    controller_did,
    runtime_plugin_id,
    runtime_profile_id,
    workspace_id,
    policy_id,
    local_agent_db_path,
    message_db_path,
    status,
    created_at,
    updated_at
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?12)
ON CONFLICT(agent_did) DO UPDATE SET
    handle = excluded.handle,
    agent_kind = excluded.agent_kind,
    controller_did = excluded.controller_did,
    runtime_plugin_id = excluded.runtime_plugin_id,
    runtime_profile_id = excluded.runtime_profile_id,
    workspace_id = excluded.workspace_id,
    policy_id = excluded.policy_id,
    local_agent_db_path = excluded.local_agent_db_path,
    message_db_path = excluded.message_db_path,
    status = excluded.status,
    updated_at = excluded.updated_at
"#,
            rusqlite::params![
                definition.agent_did,
                definition.handle,
                definition.agent_kind.as_str(),
                definition.controller_did,
                definition.runtime_plugin_id,
                definition.runtime_profile_id,
                definition.workspace_id,
                definition.policy_id,
                definition.local_agent_db_path,
                definition.message_db_path,
                definition.status,
                now,
            ],
        )?;
        Ok(())
    }

    pub fn store_agent_identity(&self, identity: &AgentIdentityRecord) -> Result<()> {
        if identity.agent_did.trim().is_empty() {
            bail!("agent_did must not be empty");
        }
        if identity.handle.trim().is_empty() {
            bail!("handle must not be empty");
        }
        let connection = self.connection()?;
        let now = current_time_millis()?.to_string();
        connection.execute(
            r#"
INSERT INTO agent_identity (
    agent_did,
    handle,
    agent_kind,
    did_document_json,
    endpoint_url,
    key_algorithm,
    public_key,
    auth_private_key_pem,
    e2ee_signing_private_key_pem,
    e2ee_agreement_private_key_pem,
    created_at,
    updated_at
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11)
ON CONFLICT(agent_did) DO UPDATE SET
    handle = excluded.handle,
    agent_kind = excluded.agent_kind,
    did_document_json = excluded.did_document_json,
    endpoint_url = excluded.endpoint_url,
    key_algorithm = excluded.key_algorithm,
    public_key = excluded.public_key,
    auth_private_key_pem = excluded.auth_private_key_pem,
    e2ee_signing_private_key_pem = excluded.e2ee_signing_private_key_pem,
    e2ee_agreement_private_key_pem = excluded.e2ee_agreement_private_key_pem,
    updated_at = excluded.updated_at
"#,
            rusqlite::params![
                identity.agent_did,
                identity.handle,
                identity.agent_kind.as_str(),
                identity.did_document.to_string(),
                identity.endpoint_url,
                identity.key_algorithm,
                identity.public_key,
                identity.auth_private_key_pem,
                identity.e2ee_signing_private_key_pem,
                identity.e2ee_agreement_private_key_pem,
                now,
            ],
        )?;
        Ok(())
    }

    pub fn load_agent_identity(&self, agent_did: &str) -> Result<AgentIdentityRecord> {
        let connection = self.connection()?;
        connection
            .query_row(
                r#"
SELECT
    agent_did,
    handle,
    agent_kind,
    did_document_json,
    endpoint_url,
    key_algorithm,
    public_key,
    auth_private_key_pem,
    e2ee_signing_private_key_pem,
    e2ee_agreement_private_key_pem
FROM agent_identity
WHERE agent_did = ?1
"#,
                [agent_did],
                agent_identity_from_row,
            )
            .with_context(|| format!("load agent identity {agent_did}"))
    }

    pub fn load_agent_definition(&self, agent_did: &str) -> Result<AgentDefinition> {
        let connection = self.connection()?;
        connection
            .query_row(
                r#"
SELECT
    agent_did,
    handle,
    agent_kind,
    controller_did,
    runtime_plugin_id,
    runtime_profile_id,
    workspace_id,
    policy_id,
    local_agent_db_path,
    message_db_path,
    status
FROM agent_definition
WHERE agent_did = ?1
"#,
                [agent_did],
                agent_definition_from_row,
            )
            .with_context(|| format!("load agent definition {agent_did}"))
    }

    pub fn load_daemon_agent_by_handle(&self, handle: &str) -> Result<Option<AgentDefinition>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            r#"
SELECT
    agent_did,
    handle,
    agent_kind,
    controller_did,
    runtime_plugin_id,
    runtime_profile_id,
    workspace_id,
    policy_id,
    local_agent_db_path,
    message_db_path,
    status
FROM agent_definition
WHERE agent_kind = 'daemon' AND handle = ?1
ORDER BY updated_at DESC
LIMIT 1
"#,
        )?;
        let mut rows = statement.query([handle])?;
        let Some(row) = rows.next()? else {
            return Ok(None);
        };
        Ok(Some(agent_definition_from_row(row)?))
    }

    pub fn list_agent_definitions(&self) -> Result<Vec<AgentDefinition>> {
        self.list_agent_definitions_by_kind(None)
    }

    pub fn list_runtime_agent_definitions(&self) -> Result<Vec<AgentDefinition>> {
        self.list_agent_definitions_by_kind(Some(AgentKind::Runtime))
    }

    fn list_agent_definitions_by_kind(
        &self,
        kind: Option<AgentKind>,
    ) -> Result<Vec<AgentDefinition>> {
        let connection = self.connection()?;
        let sql = match kind {
            Some(_) => {
                r#"
SELECT
    agent_did,
    handle,
    agent_kind,
    controller_did,
    runtime_plugin_id,
    runtime_profile_id,
    workspace_id,
    policy_id,
    local_agent_db_path,
    message_db_path,
    status
FROM agent_definition
WHERE agent_kind = ?1
ORDER BY updated_at DESC, agent_did ASC
"#
            }
            None => {
                r#"
SELECT
    agent_did,
    handle,
    agent_kind,
    controller_did,
    runtime_plugin_id,
    runtime_profile_id,
    workspace_id,
    policy_id,
    local_agent_db_path,
    message_db_path,
    status
FROM agent_definition
ORDER BY updated_at DESC, agent_did ASC
"#
            }
        };
        let mut statement = connection.prepare(sql)?;
        let rows = match kind {
            Some(kind) => statement.query_map([kind.as_str()], agent_definition_from_row)?,
            None => statement.query_map([], agent_definition_from_row)?,
        };
        let mut definitions = Vec::new();
        for row in rows {
            definitions.push(row?);
        }
        Ok(definitions)
    }

    pub fn upsert_workspace_binding(
        &self,
        agent_did: &str,
        runtime_profile_id: &str,
        binding: &WorkspaceBindingConfig,
    ) -> Result<()> {
        binding.validate()?;
        let connection = self.connection()?;
        let now = current_time_millis()?.to_string();
        connection.execute(
            r#"
INSERT INTO workspace_binding (
    workspace_id,
    agent_did,
    runtime_profile_id,
    workspace_root,
    workspace_mode,
    status,
    created_at,
    updated_at
) VALUES (?1, ?2, ?3, ?4, ?5, 'active', ?6, ?6)
ON CONFLICT(workspace_id) DO UPDATE SET
    agent_did = excluded.agent_did,
    runtime_profile_id = excluded.runtime_profile_id,
    workspace_root = excluded.workspace_root,
    workspace_mode = excluded.workspace_mode,
    status = 'active',
    updated_at = excluded.updated_at
"#,
            rusqlite::params![
                binding.workspace_id,
                agent_did,
                runtime_profile_id,
                binding.workspace_root.display().to_string(),
                binding.workspace_mode.as_str(),
                now,
            ],
        )?;
        Ok(())
    }

    pub fn insert_runtime_task(&self, task: &RuntimeTask) -> Result<()> {
        task.validate()?;
        let connection = self.connection()?;
        let now = current_time_millis()?;
        connection.execute(
            r#"
INSERT INTO runtime_task (
    task_id,
    agent_did,
    controller_did,
    sender_did,
    conversation_id,
    task_text,
    status,
    created_at_ms,
    updated_at_ms
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'created', ?7, ?7)
ON CONFLICT(task_id) DO UPDATE SET
    status = excluded.status,
    task_text = excluded.task_text,
    updated_at_ms = excluded.updated_at_ms
"#,
            rusqlite::params![
                task.task_id,
                task.agent_did,
                task.controller_did,
                task.sender_did,
                task.conversation_id,
                task.text,
                now,
            ],
        )?;
        Ok(())
    }

    pub fn insert_runtime_run(&self, run: &RuntimeRun) -> Result<()> {
        let connection = self.connection()?;
        let now = current_time_millis()?;
        connection.execute(
            r#"
INSERT INTO runtime_run (
    run_id,
    task_id,
    agent_did,
    runtime_profile_id,
    runtime_plugin_id,
    workspace_id,
    status,
    started_at,
    updated_at,
    started_at_ms,
    updated_at_ms
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8, ?9, ?9)
"#,
            rusqlite::params![
                run.run_id,
                run.task_id,
                run.agent_did,
                run.runtime_profile_id,
                run.runtime_plugin_id,
                run.workspace_id,
                run.status.as_str(),
                now.to_string(),
                now,
            ],
        )?;
        Ok(())
    }

    pub fn update_runtime_run_status(&self, run_id: &str, status: RuntimeRunStatus) -> Result<()> {
        let connection = self.connection()?;
        let now = current_time_millis()?;
        let completed_at = match status {
            RuntimeRunStatus::Finished | RuntimeRunStatus::Failed => Some(now.to_string()),
            RuntimeRunStatus::Pending | RuntimeRunStatus::Running => None,
        };
        let updated = connection.execute(
            r#"
UPDATE runtime_run
SET status = ?1,
    completed_at = COALESCE(?2, completed_at),
    updated_at = ?3,
    completed_at_ms = COALESCE(?4, completed_at_ms),
    updated_at_ms = ?5
WHERE run_id = ?6
"#,
            rusqlite::params![
                status.as_str(),
                completed_at,
                now.to_string(),
                match status {
                    RuntimeRunStatus::Finished | RuntimeRunStatus::Failed => Some(now),
                    RuntimeRunStatus::Pending | RuntimeRunStatus::Running => None,
                },
                now,
                run_id,
            ],
        )?;
        if updated == 0 {
            bail!("runtime run does not exist: {run_id}");
        }
        Ok(())
    }

    pub fn load_runtime_run(&self, run_id: &str) -> Result<RuntimeRun> {
        let connection = self.connection()?;
        connection
            .query_row(
                r#"
SELECT run_id, task_id, agent_did, runtime_profile_id, runtime_plugin_id, workspace_id, status
FROM runtime_run
WHERE run_id = ?1
"#,
                [run_id],
                |row| {
                    let status: String = row.get(6)?;
                    let status = RuntimeRunStatus::parse(&status).map_err(|err| {
                        rusqlite::Error::FromSqlConversionFailure(
                            status.len(),
                            rusqlite::types::Type::Text,
                            Box::new(std::io::Error::new(
                                std::io::ErrorKind::InvalidData,
                                err.to_string(),
                            )),
                        )
                    })?;
                    Ok(RuntimeRun {
                        run_id: row.get(0)?,
                        task_id: row.get(1)?,
                        agent_did: row.get(2)?,
                        runtime_profile_id: row.get(3)?,
                        runtime_plugin_id: row.get(4)?,
                        workspace_id: row.get(5)?,
                        status,
                    })
                },
            )
            .context("load runtime run")
    }

    pub fn authorize_runtime_rpc(
        &self,
        token: &RuntimeRpcToken,
        method: &RpcMethod,
        recipient: Option<&str>,
    ) -> Result<AuthorizedRuntimeContext> {
        let connection = self.connection()?;
        let token_id = token.token_id();
        let record = load_runtime_token_record(&connection, &token_id)?;
        let audit_scope = record.scope_for_audit();
        let mut authorized = false;
        let mut reason = "authorized".to_string();

        let result = (|| {
            if record.token_secret_hash != token.secret_hash() {
                reason = "token_hash_mismatch".to_string();
                bail!("runtime RPC token rejected");
            }
            let now = current_time_millis()?;
            if record.scope.expires_at_ms <= now {
                reason = "token_expired".to_string();
                bail!("runtime RPC token expired");
            }
            if record.revoked_at_ms.is_some() {
                reason = "token_revoked".to_string();
                bail!("runtime RPC token revoked");
            }
            if record.single_use && record.used_at_ms.is_some() {
                reason = "token_already_used".to_string();
                bail!("runtime RPC token already used");
            }
            if !record.scope.allows_method(method) {
                reason = "method_not_allowed".to_string();
                bail!("runtime RPC method not allowed");
            }
            if *method == RpcMethod::MsgSend && !record.scope.allows_recipient(recipient) {
                reason = "recipient_not_allowed".to_string();
                bail!("runtime RPC recipient not allowed");
            }
            authorized = true;
            Ok(AuthorizedRuntimeContext {
                token_id: token_id.clone(),
                agent_did: record.scope.agent_did.clone(),
                runtime_profile_id: record.scope.runtime_profile_id.clone(),
                run_id: record.scope.run_id.clone(),
                method: method.clone(),
            })
        })();

        self.insert_audit_event(&token_id, audit_scope, method, authorized, &reason)?;

        let context = result?;
        if record.single_use {
            connection.execute(
                "UPDATE runtime_rpc_tokens SET used_at_ms = ?1 WHERE token_id = ?2",
                rusqlite::params![current_time_millis()?, token_id],
            )?;
        }
        Ok(context)
    }

    fn insert_audit_event(
        &self,
        token_id: &str,
        scope: RuntimeTokenAuditScope,
        method: &RpcMethod,
        authorized: bool,
        reason: &str,
    ) -> Result<()> {
        let connection = self.connection()?;
        let now = current_time_millis()?;
        let sequence = AUDIT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let audit_id = format!("audit_{now}_{sequence}_{token_id}");
        let detail_json = serde_json::json!({
            "method": method.as_str(),
            "method_level": method.level(),
            "authorized": authorized,
            "reason": reason,
        })
        .to_string();
        connection.execute(
            r#"
INSERT INTO audit_log (
    audit_id,
    event_type,
    agent_did,
    runtime_profile_id,
    run_id,
    token_id,
    detail_json,
    created_at_ms
) VALUES (?1, 'runtime_rpc.authorize', ?2, ?3, ?4, ?5, ?6, ?7)
"#,
            rusqlite::params![
                audit_id,
                scope.agent_did,
                scope.runtime_profile_id,
                scope.run_id,
                token_id,
                detail_json,
                now,
            ],
        )?;
        Ok(())
    }

    pub fn insert_audit_event_json(
        &self,
        event_type: &str,
        agent_did: Option<&str>,
        runtime_profile_id: Option<&str>,
        run_id: Option<&str>,
        token_id: Option<&str>,
        detail: serde_json::Value,
    ) -> Result<()> {
        let connection = self.connection()?;
        let now = current_time_millis()?;
        let sequence = AUDIT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let audit_id = format!(
            "audit_{now}_{sequence}_{}",
            token_id.unwrap_or("agent_management")
        );
        connection.execute(
            r#"
INSERT INTO audit_log (
    audit_id,
    event_type,
    agent_did,
    runtime_profile_id,
    run_id,
    token_id,
    detail_json,
    created_at_ms
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
"#,
            rusqlite::params![
                audit_id,
                event_type,
                agent_did,
                runtime_profile_id,
                run_id,
                token_id,
                detail.to_string(),
                now,
            ],
        )?;
        Ok(())
    }
}

pub fn current_schema_version(connection: &Connection) -> Result<i64> {
    let version = connection.query_row(
        "SELECT version FROM schema_migrations ORDER BY version DESC LIMIT 1",
        [],
        |row| row.get(0),
    )?;
    Ok(version)
}

fn initialize_schema(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        r#"
        PRAGMA foreign_keys = ON;

        CREATE TABLE IF NOT EXISTS schema_migrations (
            version INTEGER PRIMARY KEY,
            applied_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS agent_definition (
            agent_did TEXT PRIMARY KEY,
            controller_did TEXT NOT NULL,
            runtime_profile_id TEXT,
            status TEXT NOT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS runtime_profile (
            runtime_profile_id TEXT PRIMARY KEY,
            agent_did TEXT,
            runtime_plugin_id TEXT NOT NULL,
            display_name TEXT,
            status TEXT NOT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS workspace_binding (
            workspace_id TEXT PRIMARY KEY,
            agent_did TEXT,
            runtime_profile_id TEXT,
            workspace_root TEXT NOT NULL,
            workspace_mode TEXT NOT NULL,
            status TEXT NOT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS runtime_run (
            run_id TEXT PRIMARY KEY,
            task_id TEXT DEFAULT '',
            agent_did TEXT NOT NULL,
            runtime_profile_id TEXT NOT NULL,
            runtime_plugin_id TEXT NOT NULL,
            workspace_id TEXT,
            status TEXT NOT NULL,
            started_at TEXT NOT NULL,
            completed_at TEXT,
            updated_at TEXT NOT NULL,
            started_at_ms INTEGER NOT NULL DEFAULT 0,
            completed_at_ms INTEGER,
            updated_at_ms INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS runtime_task (
            task_id TEXT PRIMARY KEY,
            agent_did TEXT NOT NULL,
            controller_did TEXT NOT NULL,
            sender_did TEXT NOT NULL,
            conversation_id TEXT,
            task_text TEXT NOT NULL,
            status TEXT NOT NULL,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS runtime_rpc_tokens (
            token_id TEXT PRIMARY KEY,
            token_secret_hash TEXT NOT NULL,
            agent_did TEXT NOT NULL,
            runtime_profile_id TEXT NOT NULL,
            run_id TEXT NOT NULL,
            allowed_methods_json TEXT NOT NULL,
            allowed_recipients_json TEXT,
            expires_at TEXT NOT NULL DEFAULT '',
            expires_at_ms INTEGER NOT NULL,
            single_use INTEGER NOT NULL DEFAULT 0,
            revoked_at TEXT,
            revoked_at_ms INTEGER,
            used_at TEXT,
            used_at_ms INTEGER,
            created_at TEXT NOT NULL DEFAULT '',
            created_at_ms INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS audit_log (
            audit_id TEXT PRIMARY KEY,
            event_type TEXT NOT NULL,
            agent_did TEXT,
            runtime_profile_id TEXT,
            run_id TEXT,
            token_id TEXT,
            detail_json TEXT,
            created_at TEXT NOT NULL DEFAULT '',
            created_at_ms INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS agent_identity (
            agent_did TEXT PRIMARY KEY,
            handle TEXT NOT NULL,
            agent_kind TEXT NOT NULL,
            did_document_json TEXT NOT NULL,
            endpoint_url TEXT,
            key_algorithm TEXT NOT NULL,
            public_key TEXT NOT NULL,
            auth_private_key_pem TEXT NOT NULL,
            e2ee_signing_private_key_pem TEXT,
            e2ee_agreement_private_key_pem TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );

        INSERT OR IGNORE INTO schema_migrations (version, applied_at)
        VALUES (1, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'));
        "#,
    )?;
    migrate_runtime_rpc_tokens_v2(connection)?;
    migrate_audit_log_v2(connection)?;
    migrate_runtime_run_v3(connection)?;
    migrate_runtime_task_v3(connection)?;
    migrate_agent_definition_v4(connection)?;
    connection.execute(
        "INSERT OR IGNORE INTO schema_migrations (version, applied_at) VALUES (2, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
        [],
    )?;
    connection.execute(
        "INSERT OR IGNORE INTO schema_migrations (version, applied_at) VALUES (?1, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
        [DAEMON_SCHEMA_VERSION],
    )?;
    Ok(())
}

fn migrate_agent_definition_v4(connection: &Connection) -> Result<()> {
    for (column, definition) in [
        ("handle", "TEXT NOT NULL DEFAULT ''"),
        ("agent_kind", "TEXT NOT NULL DEFAULT 'runtime'"),
        ("runtime_plugin_id", "TEXT"),
        ("workspace_id", "TEXT"),
        ("policy_id", "TEXT NOT NULL DEFAULT 'default'"),
        ("local_agent_db_path", "TEXT NOT NULL DEFAULT ''"),
        ("message_db_path", "TEXT NOT NULL DEFAULT ''"),
    ] {
        add_column_if_missing(connection, "agent_definition", column, definition)?;
    }
    connection.execute_batch(
        r#"
        UPDATE agent_definition
        SET handle = agent_did
        WHERE handle = '';

        UPDATE agent_definition
        SET agent_kind = 'runtime'
        WHERE agent_kind = '';

        UPDATE agent_definition
        SET policy_id = 'default'
        WHERE policy_id = '';

        UPDATE agent_definition
        SET runtime_plugin_id = (
            SELECT runtime_profile.runtime_plugin_id
            FROM runtime_profile
            WHERE runtime_profile.runtime_profile_id = agent_definition.runtime_profile_id
            LIMIT 1
        )
        WHERE runtime_plugin_id IS NULL
          AND runtime_profile_id IS NOT NULL;

        UPDATE agent_definition
        SET local_agent_db_path = 'agents/' || replace(replace(replace(agent_did, ':', '_'), '/', '_'), '#', '_') || '/agent.db'
        WHERE local_agent_db_path = '';

        UPDATE agent_definition
        SET message_db_path = 'agents/' || replace(replace(replace(agent_did, ':', '_'), '/', '_'), '#', '_') || '/messages.db'
        WHERE message_db_path = '';
        "#,
    )?;
    Ok(())
}

fn migrate_runtime_run_v3(connection: &Connection) -> Result<()> {
    for (column, definition) in [
        ("task_id", "TEXT DEFAULT ''"),
        ("started_at_ms", "INTEGER NOT NULL DEFAULT 0"),
        ("completed_at_ms", "INTEGER"),
        ("updated_at_ms", "INTEGER NOT NULL DEFAULT 0"),
    ] {
        add_column_if_missing(connection, "runtime_run", column, definition)?;
    }
    Ok(())
}

fn migrate_runtime_task_v3(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS runtime_task (
            task_id TEXT PRIMARY KEY,
            agent_did TEXT NOT NULL,
            controller_did TEXT NOT NULL,
            sender_did TEXT NOT NULL,
            conversation_id TEXT,
            task_text TEXT NOT NULL,
            status TEXT NOT NULL,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL
        );
        "#,
    )?;
    Ok(())
}

fn migrate_runtime_rpc_tokens_v2(connection: &Connection) -> Result<()> {
    for (column, definition) in [
        ("token_secret_hash", "TEXT NOT NULL DEFAULT ''"),
        ("expires_at_ms", "INTEGER NOT NULL DEFAULT 0"),
        ("single_use", "INTEGER NOT NULL DEFAULT 0"),
        ("revoked_at_ms", "INTEGER"),
        ("used_at_ms", "INTEGER"),
        ("created_at_ms", "INTEGER NOT NULL DEFAULT 0"),
    ] {
        add_column_if_missing(connection, "runtime_rpc_tokens", column, definition)?;
    }
    Ok(())
}

fn migrate_audit_log_v2(connection: &Connection) -> Result<()> {
    add_column_if_missing(
        connection,
        "audit_log",
        "created_at_ms",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    Ok(())
}

fn add_column_if_missing(
    connection: &Connection,
    table: &str,
    column: &str,
    definition: &str,
) -> Result<()> {
    let mut statement = connection.prepare(&format!("PRAGMA table_info({table})"))?;
    let columns = statement.query_map([], |row| row.get::<_, String>(1))?;
    for existing in columns {
        if existing? == column {
            return Ok(());
        }
    }
    connection.execute(
        &format!("ALTER TABLE {table} ADD COLUMN {column} {definition}"),
        [],
    )?;
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthorizedRuntimeContext {
    pub token_id: String,
    pub agent_did: String,
    pub runtime_profile_id: String,
    pub run_id: String,
    pub method: RpcMethod,
}

#[derive(Debug, Clone)]
struct RuntimeTokenRecord {
    token_secret_hash: String,
    scope: RuntimeTokenScope,
    single_use: bool,
    revoked_at_ms: Option<i64>,
    used_at_ms: Option<i64>,
}

#[derive(Debug, Clone)]
struct RuntimeTokenAuditScope {
    agent_did: String,
    runtime_profile_id: String,
    run_id: String,
}

impl RuntimeTokenRecord {
    fn scope_for_audit(&self) -> RuntimeTokenAuditScope {
        RuntimeTokenAuditScope {
            agent_did: self.scope.agent_did.clone(),
            runtime_profile_id: self.scope.runtime_profile_id.clone(),
            run_id: self.scope.run_id.clone(),
        }
    }
}

fn load_runtime_token_record(
    connection: &Connection,
    token_id: &str,
) -> Result<RuntimeTokenRecord> {
    let record = connection.query_row(
        r#"
SELECT
    token_secret_hash,
    agent_did,
    runtime_profile_id,
    run_id,
    allowed_methods_json,
    allowed_recipients_json,
    expires_at_ms,
    single_use,
    revoked_at_ms,
    used_at_ms
FROM runtime_rpc_tokens
WHERE token_id = ?1
"#,
        [token_id],
        |row| {
            let allowed_methods_json: String = row.get(4)?;
            let allowed_recipients_json: Option<String> = row.get(5)?;
            let allowed_methods: Vec<RpcMethod> = serde_json::from_str(&allowed_methods_json)
                .map_err(|err| {
                    rusqlite::Error::FromSqlConversionFailure(
                        allowed_methods_json.len(),
                        rusqlite::types::Type::Text,
                        Box::new(err),
                    )
                })?;
            let allowed_recipients = allowed_recipients_json
                .as_ref()
                .map(|json| serde_json::from_str(json))
                .transpose()
                .map_err(|err| {
                    rusqlite::Error::FromSqlConversionFailure(
                        allowed_recipients_json.as_deref().unwrap_or_default().len(),
                        rusqlite::types::Type::Text,
                        Box::new(err),
                    )
                })?;
            let single_use = row.get::<_, i64>(7)? != 0;
            Ok(RuntimeTokenRecord {
                token_secret_hash: row.get(0)?,
                scope: RuntimeTokenScope {
                    agent_did: row.get(1)?,
                    runtime_profile_id: row.get(2)?,
                    run_id: row.get(3)?,
                    allowed_methods,
                    allowed_recipients,
                    expires_at_ms: row.get(6)?,
                    single_use,
                },
                single_use,
                revoked_at_ms: row.get(8)?,
                used_at_ms: row.get(9)?,
            })
        },
    )?;
    Ok(record)
}

fn agent_definition_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<AgentDefinition> {
    let kind_raw: String = row.get(2)?;
    let agent_kind = AgentKind::parse(&kind_raw).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(
            kind_raw.len(),
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                err.to_string(),
            )),
        )
    })?;
    Ok(AgentDefinition {
        agent_did: row.get(0)?,
        handle: row.get(1)?,
        agent_kind,
        controller_did: row.get(3)?,
        runtime_plugin_id: row.get(4)?,
        runtime_profile_id: row.get(5)?,
        workspace_id: row.get(6)?,
        policy_id: row.get(7)?,
        local_agent_db_path: row.get(8)?,
        message_db_path: row.get(9)?,
        status: row.get(10)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initialize_creates_required_tables() {
        let root = tempfile::tempdir().unwrap();
        let config = DaemonConfig::for_state_root(root.path()).unwrap();
        let summary = DaemonState::open(&config).unwrap().initialize().unwrap();
        assert_eq!(summary.schema_version, DAEMON_SCHEMA_VERSION);

        let connection = Connection::open(&config.daemon_db_path).unwrap();
        for table in [
            "schema_migrations",
            "agent_definition",
            "runtime_profile",
            "workspace_binding",
            "runtime_task",
            "runtime_run",
            "runtime_rpc_tokens",
            "audit_log",
            "agent_identity",
        ] {
            let count: i64 = connection
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                    [table],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(count, 1, "missing table {table}");
        }
    }

    #[test]
    fn agent_definition_v4_roundtrips_daemon_and_runtime_agents() {
        let root = tempfile::tempdir().unwrap();
        let config = DaemonConfig::for_state_root(root.path()).unwrap();
        let state = DaemonState::open(&config).unwrap();
        state.initialize().unwrap();

        let definition = AgentDefinition {
            agent_did: "did:agent:daemon".to_string(),
            handle: "alice-daemon".to_string(),
            agent_kind: AgentKind::Daemon,
            controller_did: "did:human:alice".to_string(),
            runtime_plugin_id: None,
            runtime_profile_id: None,
            workspace_id: None,
            policy_id: "default".to_string(),
            local_agent_db_path: "agents/daemon/agent.db".to_string(),
            message_db_path: "agents/daemon/messages.db".to_string(),
            status: "active".to_string(),
        };
        state.upsert_agent_definition(&definition).unwrap();

        assert_eq!(
            state.load_agent_definition("did:agent:daemon").unwrap(),
            definition
        );
        assert_eq!(state.list_agent_definitions().unwrap().len(), 1);
        assert_eq!(state.list_runtime_agent_definitions().unwrap().len(), 0);
    }

    #[test]
    fn agent_identity_record_roundtrips_without_debug_leaking_private_key() {
        let root = tempfile::tempdir().unwrap();
        let config = DaemonConfig::for_state_root(root.path()).unwrap();
        let state = DaemonState::open(&config).unwrap();
        state.initialize().unwrap();
        let identity = AgentIdentityRecord {
            agent_did: "did:agent:daemon".to_string(),
            handle: "alice-daemon".to_string(),
            agent_kind: AgentKind::Daemon,
            did_document: serde_json::json!({ "id": "did:agent:daemon" }),
            endpoint_url: Some("https://example.test/anp-im/rpc".to_string()),
            key_algorithm: "JsonWebKey2020".to_string(),
            public_key: "public".to_string(),
            auth_private_key_pem: "private-secret".to_string(),
            e2ee_signing_private_key_pem: "signing-secret".to_string(),
            e2ee_agreement_private_key_pem: "agreement-secret".to_string(),
        };
        state.store_agent_identity(&identity).unwrap();

        let loaded = state.load_agent_identity("did:agent:daemon").unwrap();
        assert_eq!(loaded.agent_did, identity.agent_did);
        assert_eq!(loaded.auth_private_key_pem, "private-secret");
        let debug = format!("{loaded:?}");
        assert!(!debug.contains("private-secret"));
        assert!(!debug.contains("signing-secret"));
        assert!(!debug.contains("agreement-secret"));
    }
}

fn agent_identity_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<AgentIdentityRecord> {
    let kind_raw: String = row.get(2)?;
    let agent_kind = AgentKind::parse(&kind_raw).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(
            kind_raw.len(),
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                err.to_string(),
            )),
        )
    })?;
    let did_document_json: String = row.get(3)?;
    let did_document = serde_json::from_str(&did_document_json).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(
            did_document_json.len(),
            rusqlite::types::Type::Text,
            Box::new(err),
        )
    })?;
    Ok(AgentIdentityRecord {
        agent_did: row.get(0)?,
        handle: row.get(1)?,
        agent_kind,
        did_document,
        endpoint_url: row.get(4)?,
        key_algorithm: row.get(5)?,
        public_key: row.get(6)?,
        auth_private_key_pem: row.get(7)?,
        e2ee_signing_private_key_pem: row.get(8)?,
        e2ee_agreement_private_key_pem: row.get(9)?,
    })
}
