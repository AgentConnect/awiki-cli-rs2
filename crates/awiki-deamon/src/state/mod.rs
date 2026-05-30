use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{bail, Context, Result};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::runtime::{RuntimeAgentProfile, RuntimeRun, RuntimeRunStatus, RuntimeTask};
use crate::security::runtime_token::{
    current_time_millis, IssuedRuntimeToken, RpcMethod, RuntimeRpcToken, RuntimeTokenScope,
};
use crate::workspace::WorkspaceBindingConfig;
use crate::DaemonConfig;

const DAEMON_SCHEMA_VERSION: i64 = 3;
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
        profile.validate()?;
        let connection = self.connection()?;
        let now = current_time_millis()?.to_string();
        connection.execute(
            r#"
INSERT INTO agent_definition (
    agent_did,
    controller_did,
    runtime_profile_id,
    status,
    created_at,
    updated_at
) VALUES (?1, ?2, ?3, 'active', ?4, ?4)
ON CONFLICT(agent_did) DO UPDATE SET
    controller_did = excluded.controller_did,
    runtime_profile_id = excluded.runtime_profile_id,
    status = 'active',
    updated_at = excluded.updated_at
"#,
            rusqlite::params![
                profile.agent_did,
                profile.controller_did,
                profile.runtime_profile_id,
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

        INSERT OR IGNORE INTO schema_migrations (version, applied_at)
        VALUES (1, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'));
        "#,
    )?;
    migrate_runtime_rpc_tokens_v2(connection)?;
    migrate_audit_log_v2(connection)?;
    migrate_runtime_run_v3(connection)?;
    migrate_runtime_task_v3(connection)?;
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
}
