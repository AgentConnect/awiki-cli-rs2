use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{bail, Context, Result};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::agent::{
    agent_data_paths, AgentDefinition, AgentIdentityRecord, AgentKind,
    GENERIC_CLI_RUNTIME_PLUGIN_ID,
};
use crate::runtime::{RuntimeAgentProfile, RuntimeRun, RuntimeRunStatus, RuntimeTask};
use crate::security::runtime_token::{
    current_time_millis, IssuedRuntimeToken, RpcMethod, RuntimeRpcToken, RuntimeTokenScope,
};
use crate::workspace::{WorkspaceBindingConfig, WorkspaceMode};
use crate::DaemonConfig;

const DAEMON_SCHEMA_VERSION: i64 = 10;
static AUDIT_SEQUENCE: AtomicU64 = AtomicU64::new(1);

const DEFAULT_CLI_RECIPIENT_POLICY_JSON: &str = r#"{"mode":"controller-only"}"#;
const DEFAULT_CLI_DRIVER_CONFIG_JSON: &str = "{}";

#[derive(Debug, Clone)]
pub struct DaemonState {
    database_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StateSummary {
    pub database_path: PathBuf,
    pub schema_version: i64,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CliRuntimeProfileRecord {
    pub runtime_profile_id: String,
    pub driver_id: String,
    pub binary_path: Option<PathBuf>,
    pub config_home: Option<PathBuf>,
    pub auth_mode: Option<String>,
    pub default_model: Option<String>,
    pub default_sandbox: Option<String>,
    pub default_workspace_mode: WorkspaceMode,
    pub recipient_policy_json: Value,
    pub driver_config_json: Value,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CliDriverRunRecord {
    pub run_id: String,
    pub agent_did: String,
    pub runtime_profile_id: String,
    pub driver_id: String,
    pub controller_did: String,
    pub conversation_id: Option<String>,
    pub route_key: String,
    pub workspace_id: Option<String>,
    pub workspace_root: Option<PathBuf>,
    pub workspace_instance_path: Option<PathBuf>,
    pub workspace_mode: Option<WorkspaceMode>,
    pub is_security_boundary: bool,
    pub command_json: Value,
    pub output_json: Value,
    pub final_output_path: Option<PathBuf>,
    pub native_session_id: Option<String>,
    pub synthetic_session_id: Option<String>,
    pub status: String,
    pub fallback_final_source: Option<String>,
}

impl CliDriverRunRecord {
    pub fn validate(&self) -> Result<()> {
        for (field_name, value) in [
            ("run_id", self.run_id.as_str()),
            ("agent_did", self.agent_did.as_str()),
            ("runtime_profile_id", self.runtime_profile_id.as_str()),
            ("driver_id", self.driver_id.as_str()),
            ("controller_did", self.controller_did.as_str()),
            ("route_key", self.route_key.as_str()),
            ("status", self.status.as_str()),
        ] {
            if value.trim().is_empty() {
                bail!("{field_name} must not be empty");
            }
        }
        if !self.command_json.is_object() {
            bail!("command_json must be a JSON object");
        }
        if !self.output_json.is_object() {
            bail!("output_json must be a JSON object");
        }
        Ok(())
    }
}

impl CliRuntimeProfileRecord {
    pub fn for_driver(
        runtime_profile_id: impl Into<String>,
        driver_id: impl Into<String>,
    ) -> Result<Self> {
        let record = Self {
            runtime_profile_id: runtime_profile_id.into(),
            driver_id: normalize_cli_driver_id_for_storage(&driver_id.into())?,
            binary_path: None,
            config_home: None,
            auth_mode: None,
            default_model: None,
            default_sandbox: Some("read-only".to_string()),
            default_workspace_mode: WorkspaceMode::SharedRoot,
            recipient_policy_json: serde_json::from_str(DEFAULT_CLI_RECIPIENT_POLICY_JSON)?,
            driver_config_json: serde_json::from_str(DEFAULT_CLI_DRIVER_CONFIG_JSON)?,
            status: "active".to_string(),
        };
        record.validate()?;
        Ok(record)
    }

    pub fn validate(&self) -> Result<()> {
        if self.runtime_profile_id.trim().is_empty() {
            bail!("runtime_profile_id must not be empty");
        }
        let normalized_driver_id = normalize_cli_driver_id_for_storage(&self.driver_id)?;
        if normalized_driver_id != self.driver_id {
            bail!("driver_id must be canonical lowercase");
        }
        if self
            .binary_path
            .as_ref()
            .is_some_and(|path| path.as_os_str().is_empty())
        {
            bail!("binary_path must not be empty when present");
        }
        if self
            .config_home
            .as_ref()
            .is_some_and(|path| path.as_os_str().is_empty())
        {
            bail!("config_home must not be empty when present");
        }
        for (field_name, value) in [
            ("auth_mode", self.auth_mode.as_deref()),
            ("default_model", self.default_model.as_deref()),
            ("default_sandbox", self.default_sandbox.as_deref()),
        ] {
            if value.is_some_and(|value| value.trim().is_empty()) {
                bail!("{field_name} must not be empty when present");
            }
        }
        if !self.recipient_policy_json.is_object() {
            bail!("recipient_policy_json must be a JSON object");
        }
        if !self.driver_config_json.is_object() {
            bail!("driver_config_json must be a JSON object");
        }
        if self.status.trim().is_empty() {
            bail!("CLI runtime profile status must not be empty");
        }
        Ok(())
    }
}

impl std::fmt::Debug for CliRuntimeProfileRecord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CliRuntimeProfileRecord")
            .field("runtime_profile_id", &self.runtime_profile_id)
            .field("driver_id", &self.driver_id)
            .field("binary_path", &self.binary_path)
            .field("config_home", &self.config_home)
            .field("auth_mode", &self.auth_mode)
            .field("default_model", &self.default_model)
            .field("default_sandbox", &self.default_sandbox)
            .field("default_workspace_mode", &self.default_workspace_mode)
            .field("recipient_policy_json", &self.recipient_policy_json)
            .field("driver_config_json", &"<redacted-driver-config>")
            .field("status", &self.status)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HermesProfileRecord {
    pub agent_did: String,
    pub runtime_profile_id: String,
    pub hermes_profile: String,
    pub hermes_home: PathBuf,
    pub hermes_version: Option<String>,
    pub awiki_skills_version: String,
    pub status: String,
}

impl HermesProfileRecord {
    pub fn validate(&self) -> Result<()> {
        if self.agent_did.trim().is_empty() {
            bail!("agent_did must not be empty");
        }
        if self.runtime_profile_id.trim().is_empty() {
            bail!("runtime_profile_id must not be empty");
        }
        if self.hermes_profile.trim().is_empty() {
            bail!("hermes_profile must not be empty");
        }
        if self.hermes_home.as_os_str().is_empty() {
            bail!("hermes_home must not be empty");
        }
        if self.awiki_skills_version.trim().is_empty() {
            bail!("awiki_skills_version must not be empty");
        }
        if self.status.trim().is_empty() {
            bail!("hermes profile status must not be empty");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HermesSessionRoute {
    pub agent_did: String,
    pub runtime_profile_id: String,
    pub controller_did: String,
    pub conversation_id: Option<String>,
    pub session_kind: String,
}

impl HermesSessionRoute {
    pub fn new(
        agent_did: impl Into<String>,
        runtime_profile_id: impl Into<String>,
        controller_did: impl Into<String>,
        conversation_id: Option<String>,
        session_kind: impl Into<String>,
    ) -> Self {
        Self {
            agent_did: agent_did.into(),
            runtime_profile_id: runtime_profile_id.into(),
            controller_did: controller_did.into(),
            conversation_id,
            session_kind: session_kind.into(),
        }
    }

    pub fn validate(&self) -> Result<()> {
        if self.agent_did.trim().is_empty() {
            bail!("agent_did must not be empty");
        }
        if self.runtime_profile_id.trim().is_empty() {
            bail!("runtime_profile_id must not be empty");
        }
        if self.controller_did.trim().is_empty() {
            bail!("controller_did must not be empty");
        }
        if self.session_kind.trim().is_empty() {
            bail!("session_kind must not be empty");
        }
        Ok(())
    }

    pub fn route_key(&self) -> String {
        format!(
            "hermes:{}:{}:{}:{}",
            self.agent_did,
            self.controller_did,
            self.conversation_id
                .as_deref()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or("no-conversation"),
            self.session_kind
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HermesNativeSessionRecord {
    pub id: String,
    pub runtime_session_id: String,
    pub agent_did: String,
    pub runtime_profile_id: String,
    pub controller_did: String,
    pub conversation_id: Option<String>,
    pub route_key: String,
    pub hermes_profile: String,
    pub hermes_session_id: String,
    pub session_kind: String,
    pub status: String,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

impl HermesNativeSessionRecord {
    pub fn active(
        route: &HermesSessionRoute,
        hermes_profile: impl Into<String>,
        hermes_session_id: impl Into<String>,
    ) -> Result<Self> {
        route.validate()?;
        let route_key = route.route_key();
        let hermes_session_id = hermes_session_id.into();
        let id = stable_hermes_session_record_id(&route_key, &hermes_session_id);
        let now = current_time_millis()?;
        Ok(Self {
            runtime_session_id: format!("rs_{id}"),
            id,
            agent_did: route.agent_did.clone(),
            runtime_profile_id: route.runtime_profile_id.clone(),
            controller_did: route.controller_did.clone(),
            conversation_id: route.conversation_id.clone(),
            route_key,
            hermes_profile: hermes_profile.into(),
            hermes_session_id,
            session_kind: route.session_kind.clone(),
            status: "active".to_string(),
            created_at_ms: now,
            updated_at_ms: now,
        })
    }

    pub fn validate(&self) -> Result<()> {
        if self.id.trim().is_empty() {
            bail!("hermes native session id must not be empty");
        }
        if self.runtime_session_id.trim().is_empty() {
            bail!("runtime_session_id must not be empty");
        }
        if self.agent_did.trim().is_empty() {
            bail!("agent_did must not be empty");
        }
        if self.runtime_profile_id.trim().is_empty() {
            bail!("runtime_profile_id must not be empty");
        }
        if self.controller_did.trim().is_empty() {
            bail!("controller_did must not be empty");
        }
        if self.route_key.trim().is_empty() {
            bail!("route_key must not be empty");
        }
        if self.hermes_profile.trim().is_empty() {
            bail!("hermes_profile must not be empty");
        }
        if self.hermes_session_id.trim().is_empty() {
            bail!("hermes_session_id must not be empty");
        }
        if self.session_kind.trim().is_empty() {
            bail!("session_kind must not be empty");
        }
        if self.status.trim().is_empty() {
            bail!("session status must not be empty");
        }
        Ok(())
    }
}

fn stable_hermes_session_record_id(route_key: &str, hermes_session_id: &str) -> String {
    let digest = Sha256::digest(route_key.as_bytes());
    let digest = Sha256::digest([digest.as_slice(), hermes_session_id.as_bytes()].concat());
    format!("hns_{:x}", digest)
}

fn normalize_cli_driver_id_for_storage(input: &str) -> Result<String> {
    let value = input.trim().to_ascii_lowercase();
    if value.is_empty() {
        bail!("driver_id must not be empty");
    }
    if !value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
    {
        bail!("driver_id contains unsupported characters");
    }
    Ok(match value.as_str() {
        "codex-cli" => "codex".to_string(),
        "gemini-cli" => "gemini".to_string(),
        _ => value,
    })
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
        let allowed_message_security_json = match issued.scope.allowed_message_security.as_ref() {
            Some(security_modes) => Some(serde_json::to_string(security_modes)?),
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
    allowed_message_security_json,
    expires_at_ms,
    single_use,
    revoked_at_ms,
    used_at_ms,
    created_at_ms,
    expires_at,
    created_at
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, NULL, NULL, ?11, ?12, ?13)
"#,
            rusqlite::params![
                issued.token_id,
                issued.token.secret_hash(),
                issued.scope.agent_did,
                issued.scope.runtime_profile_id,
                issued.scope.run_id,
                allowed_methods_json,
                allowed_recipients_json,
                allowed_message_security_json,
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

    pub fn store_agent_auth_token(&self, agent_did: &str, jwt_token: &str) -> Result<()> {
        if agent_did.trim().is_empty() {
            bail!("agent_did must not be empty");
        }
        let jwt_token = jwt_token.trim();
        if jwt_token.is_empty() {
            bail!("agent auth token must not be empty");
        }
        let connection = self.connection()?;
        let now = current_time_millis()?;
        connection.execute(
            r#"
INSERT INTO agent_auth_state (
    agent_did,
    jwt_token,
    updated_at_ms
) VALUES (?1, ?2, ?3)
ON CONFLICT(agent_did) DO UPDATE SET
    jwt_token = excluded.jwt_token,
    updated_at_ms = excluded.updated_at_ms
"#,
            rusqlite::params![agent_did, jwt_token, now],
        )?;
        Ok(())
    }

    pub fn load_agent_auth_token(&self, agent_did: &str) -> Result<Option<String>> {
        let connection = self.connection()?;
        let mut statement =
            connection.prepare("SELECT jwt_token FROM agent_auth_state WHERE agent_did = ?1")?;
        let mut rows = statement.query([agent_did])?;
        let Some(row) = rows.next()? else {
            return Ok(None);
        };
        Ok(Some(row.get(0)?))
    }

    pub fn list_agent_auth_tokens(&self) -> Result<Vec<(String, String)>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            r#"
SELECT agent_did, jwt_token
FROM agent_auth_state
ORDER BY agent_did ASC
"#,
        )?;
        let rows = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        let mut tokens = Vec::new();
        for row in rows {
            tokens.push(row?);
        }
        Ok(tokens)
    }

    pub fn upsert_cli_runtime_profile(&self, profile: &CliRuntimeProfileRecord) -> Result<()> {
        profile.validate()?;
        let connection = self.connection()?;
        let now = current_time_millis()?;
        connection.execute(
            r#"
INSERT INTO cli_runtime_profile (
    runtime_profile_id,
    driver_id,
    binary_path,
    config_home,
    auth_mode,
    default_model,
    default_sandbox,
    default_workspace_mode,
    recipient_policy_json,
    driver_config_json,
    status,
    created_at_ms,
    updated_at_ms
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?12)
ON CONFLICT(runtime_profile_id) DO UPDATE SET
    driver_id = excluded.driver_id,
    binary_path = excluded.binary_path,
    config_home = excluded.config_home,
    auth_mode = excluded.auth_mode,
    default_model = excluded.default_model,
    default_sandbox = excluded.default_sandbox,
    default_workspace_mode = excluded.default_workspace_mode,
    recipient_policy_json = excluded.recipient_policy_json,
    driver_config_json = excluded.driver_config_json,
    status = excluded.status,
    updated_at_ms = excluded.updated_at_ms
"#,
            rusqlite::params![
                profile.runtime_profile_id,
                profile.driver_id,
                profile
                    .binary_path
                    .as_ref()
                    .map(|path| path.display().to_string()),
                profile
                    .config_home
                    .as_ref()
                    .map(|path| path.display().to_string()),
                profile.auth_mode,
                profile.default_model,
                profile.default_sandbox,
                profile.default_workspace_mode.as_str(),
                profile.recipient_policy_json.to_string(),
                profile.driver_config_json.to_string(),
                profile.status,
                now,
            ],
        )?;
        Ok(())
    }

    pub fn load_cli_runtime_profile(
        &self,
        runtime_profile_id: &str,
    ) -> Result<CliRuntimeProfileRecord> {
        let connection = self.connection()?;
        connection
            .query_row(
                r#"
SELECT
    runtime_profile_id,
    driver_id,
    binary_path,
    config_home,
    auth_mode,
    default_model,
    default_sandbox,
    default_workspace_mode,
    recipient_policy_json,
    driver_config_json,
    status
FROM cli_runtime_profile
WHERE runtime_profile_id = ?1
"#,
                [runtime_profile_id],
                cli_runtime_profile_from_row,
            )
            .with_context(|| format!("load CLI runtime profile {runtime_profile_id}"))
    }

    pub fn list_cli_runtime_profiles(&self) -> Result<Vec<CliRuntimeProfileRecord>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            r#"
SELECT
    runtime_profile_id,
    driver_id,
    binary_path,
    config_home,
    auth_mode,
    default_model,
    default_sandbox,
    default_workspace_mode,
    recipient_policy_json,
    driver_config_json,
    status
FROM cli_runtime_profile
ORDER BY runtime_profile_id ASC
"#,
        )?;
        let rows = statement.query_map([], cli_runtime_profile_from_row)?;
        let mut profiles = Vec::new();
        for row in rows {
            profiles.push(row?);
        }
        Ok(profiles)
    }

    pub fn upsert_hermes_profile(&self, profile: &HermesProfileRecord) -> Result<()> {
        profile.validate()?;
        let connection = self.connection()?;
        let now = current_time_millis()?;
        connection.execute(
            r#"
INSERT INTO hermes_profiles (
    agent_did,
    runtime_profile_id,
    hermes_profile,
    hermes_home,
    hermes_version,
    awiki_skills_version,
    status,
    created_at_ms,
    updated_at_ms
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)
ON CONFLICT(agent_did) DO UPDATE SET
    runtime_profile_id = excluded.runtime_profile_id,
    hermes_profile = excluded.hermes_profile,
    hermes_home = excluded.hermes_home,
    hermes_version = excluded.hermes_version,
    awiki_skills_version = excluded.awiki_skills_version,
    status = excluded.status,
    updated_at_ms = excluded.updated_at_ms
"#,
            rusqlite::params![
                profile.agent_did,
                profile.runtime_profile_id,
                profile.hermes_profile,
                profile.hermes_home.display().to_string(),
                profile.hermes_version,
                profile.awiki_skills_version,
                profile.status,
                now,
            ],
        )?;
        Ok(())
    }

    pub fn load_hermes_profile(&self, agent_did: &str) -> Result<HermesProfileRecord> {
        let connection = self.connection()?;
        connection
            .query_row(
                r#"
SELECT
    agent_did,
    runtime_profile_id,
    hermes_profile,
    hermes_home,
    hermes_version,
    awiki_skills_version,
    status
FROM hermes_profiles
WHERE agent_did = ?1
"#,
                [agent_did],
                |row| {
                    let hermes_home: String = row.get(3)?;
                    Ok(HermesProfileRecord {
                        agent_did: row.get(0)?,
                        runtime_profile_id: row.get(1)?,
                        hermes_profile: row.get(2)?,
                        hermes_home: PathBuf::from(hermes_home),
                        hermes_version: row.get(4)?,
                        awiki_skills_version: row.get(5)?,
                        status: row.get(6)?,
                    })
                },
            )
            .with_context(|| format!("load Hermes profile for agent {agent_did}"))
    }

    pub fn store_hermes_native_session(&self, session: &HermesNativeSessionRecord) -> Result<()> {
        session.validate()?;
        let connection = self.connection()?;
        connection
            .execute(
                r#"
INSERT INTO hermes_native_sessions (
    id,
    runtime_session_id,
    agent_did,
    runtime_profile_id,
    controller_did,
    conversation_id,
    route_key,
    hermes_profile,
    hermes_session_id,
    session_kind,
    status,
    created_at_ms,
    updated_at_ms
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
ON CONFLICT(id) DO UPDATE SET
    runtime_session_id = excluded.runtime_session_id,
    agent_did = excluded.agent_did,
    runtime_profile_id = excluded.runtime_profile_id,
    controller_did = excluded.controller_did,
    conversation_id = excluded.conversation_id,
    route_key = excluded.route_key,
    hermes_profile = excluded.hermes_profile,
    hermes_session_id = excluded.hermes_session_id,
    session_kind = excluded.session_kind,
    status = excluded.status,
    updated_at_ms = excluded.updated_at_ms
"#,
                rusqlite::params![
                    session.id,
                    session.runtime_session_id,
                    session.agent_did,
                    session.runtime_profile_id,
                    session.controller_did,
                    session.conversation_id,
                    session.route_key,
                    session.hermes_profile,
                    session.hermes_session_id,
                    session.session_kind,
                    session.status,
                    session.created_at_ms,
                    session.updated_at_ms,
                ],
            )
            .with_context(|| format!("store Hermes native session {}", session.route_key))?;
        Ok(())
    }

    pub fn load_active_hermes_session_by_route(
        &self,
        route: &HermesSessionRoute,
    ) -> Result<Option<HermesNativeSessionRecord>> {
        route.validate()?;
        let route_key = route.route_key();
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            r#"
SELECT
    id,
    runtime_session_id,
    agent_did,
    runtime_profile_id,
    controller_did,
    conversation_id,
    route_key,
    hermes_profile,
    hermes_session_id,
    session_kind,
    status,
    created_at_ms,
    updated_at_ms
FROM hermes_native_sessions
WHERE route_key = ?1
  AND status = 'active'
ORDER BY updated_at_ms DESC
LIMIT 1
"#,
        )?;
        let mut rows = statement.query([route_key])?;
        let Some(row) = rows.next()? else {
            return Ok(None);
        };
        Ok(Some(hermes_native_session_from_row(row)?))
    }

    pub fn mark_hermes_session_status(&self, id: &str, status: &str) -> Result<()> {
        if id.trim().is_empty() {
            bail!("hermes native session id must not be empty");
        }
        if status.trim().is_empty() {
            bail!("hermes native session status must not be empty");
        }
        let connection = self.connection()?;
        let updated = connection.execute(
            r#"
UPDATE hermes_native_sessions
SET status = ?1,
    updated_at_ms = ?2
WHERE id = ?3
"#,
            rusqlite::params![status, current_time_millis()?, id],
        )?;
        if updated == 0 {
            bail!("Hermes native session does not exist: {id}");
        }
        Ok(())
    }

    pub fn reset_active_hermes_session_by_route(
        &self,
        route: &HermesSessionRoute,
    ) -> Result<usize> {
        route.validate()?;
        let route_key = route.route_key();
        let connection = self.connection()?;
        let updated = connection.execute(
            r#"
UPDATE hermes_native_sessions
SET status = 'reset',
    updated_at_ms = ?1
WHERE route_key = ?2
  AND status = 'active'
"#,
            rusqlite::params![current_time_millis()?, route_key],
        )?;
        Ok(updated)
    }

    pub fn count_active_hermes_sessions_for_agent(&self, agent_did: &str) -> Result<usize> {
        if agent_did.trim().is_empty() {
            bail!("agent_did must not be empty");
        }
        let connection = self.connection()?;
        let count: i64 = connection.query_row(
            r#"
SELECT COUNT(*)
FROM hermes_native_sessions
WHERE agent_did = ?1
  AND status = 'active'
"#,
            [agent_did],
            |row| row.get(0),
        )?;
        Ok(count.max(0) as usize)
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

    pub fn load_runtime_agent_profile(&self, agent_did: &str) -> Result<RuntimeAgentProfile> {
        let definition = self.load_agent_definition(agent_did)?;
        if definition.agent_kind != AgentKind::Runtime {
            bail!("agent is not a runtime agent");
        }
        let runtime_profile_id = definition
            .runtime_profile_id
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .context("runtime agent is missing runtime_profile_id")?;
        let connection = self.connection()?;
        let mut profile = connection
            .query_row(
                r#"
SELECT
    runtime_profile_id,
    agent_did,
    runtime_plugin_id,
    display_name
FROM runtime_profile
WHERE runtime_profile_id = ?1
"#,
                [runtime_profile_id],
                |row| {
                    Ok(RuntimeAgentProfile {
                        runtime_profile_id: row.get(0)?,
                        agent_did: row.get(1)?,
                        runtime_plugin_id: row.get(2)?,
                        display_name: row.get(3)?,
                        controller_did: definition.controller_did.clone(),
                        workspace_id: definition.workspace_id.clone(),
                        workspace_root: None,
                        workspace_mode: None,
                    })
                },
            )
            .context("load runtime profile")?;
        if let Some(workspace_id) = definition.workspace_id.as_deref() {
            let binding: (String, WorkspaceMode) = connection.query_row(
                r#"
SELECT workspace_root, workspace_mode
FROM workspace_binding
WHERE workspace_id = ?1
"#,
                [workspace_id],
                |row| {
                    let root: String = row.get(0)?;
                    let mode: String = row.get(1)?;
                    let mode = WorkspaceMode::parse(&mode).map_err(|err| {
                        rusqlite::Error::FromSqlConversionFailure(
                            mode.len(),
                            rusqlite::types::Type::Text,
                            Box::new(std::io::Error::new(
                                std::io::ErrorKind::InvalidData,
                                err.to_string(),
                            )),
                        )
                    })?;
                    Ok((root, mode))
                },
            )?;
            profile.workspace_root = Some(PathBuf::from(binding.0));
            profile.workspace_mode = Some(binding.1);
        }
        profile.validate()?;
        Ok(profile)
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

    pub fn load_runtime_task(&self, task_id: &str) -> Result<RuntimeTask> {
        let connection = self.connection()?;
        connection
            .query_row(
                r#"
SELECT
    task_id,
    agent_did,
    controller_did,
    sender_did,
    conversation_id,
    task_text
FROM runtime_task
WHERE task_id = ?1
"#,
                [task_id],
                |row| {
                    Ok(RuntimeTask {
                        task_id: row.get(0)?,
                        agent_did: row.get(1)?,
                        controller_did: row.get(2)?,
                        sender_did: row.get(3)?,
                        conversation_id: row.get(4)?,
                        text: row.get(5)?,
                    })
                },
            )
            .context("load runtime task")
    }

    pub fn load_runtime_task_for_run(&self, run_id: &str) -> Result<RuntimeTask> {
        let run = self.load_runtime_run(run_id)?;
        self.load_runtime_task(&run.task_id)
    }

    pub fn upsert_cli_driver_run(&self, record: &CliDriverRunRecord) -> Result<()> {
        record.validate()?;
        let connection = self.connection()?;
        let now = current_time_millis()?;
        connection.execute(
            r#"
INSERT INTO cli_driver_run (
    run_id,
    agent_did,
    runtime_profile_id,
    driver_id,
    controller_did,
    conversation_id,
    route_key,
    workspace_id,
    workspace_root,
    workspace_instance_path,
    workspace_mode,
    is_security_boundary,
    command_json,
    output_json,
    final_output_path,
    native_session_id,
    synthetic_session_id,
    status,
    fallback_final_source,
    created_at_ms,
    updated_at_ms
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?20)
ON CONFLICT(run_id) DO UPDATE SET
    agent_did = excluded.agent_did,
    runtime_profile_id = excluded.runtime_profile_id,
    driver_id = excluded.driver_id,
    controller_did = excluded.controller_did,
    conversation_id = excluded.conversation_id,
    route_key = excluded.route_key,
    workspace_id = excluded.workspace_id,
    workspace_root = excluded.workspace_root,
    workspace_instance_path = excluded.workspace_instance_path,
    workspace_mode = excluded.workspace_mode,
    is_security_boundary = excluded.is_security_boundary,
    command_json = excluded.command_json,
    output_json = excluded.output_json,
    final_output_path = excluded.final_output_path,
    native_session_id = excluded.native_session_id,
    synthetic_session_id = excluded.synthetic_session_id,
    status = excluded.status,
    fallback_final_source = excluded.fallback_final_source,
    updated_at_ms = excluded.updated_at_ms
"#,
            rusqlite::params![
                record.run_id,
                record.agent_did,
                record.runtime_profile_id,
                record.driver_id,
                record.controller_did,
                record.conversation_id,
                record.route_key,
                record.workspace_id,
                record.workspace_root.as_ref().map(|path| path.display().to_string()),
                record
                    .workspace_instance_path
                    .as_ref()
                    .map(|path| path.display().to_string()),
                record.workspace_mode.map(WorkspaceMode::as_str),
                if record.is_security_boundary { 1 } else { 0 },
                record.command_json.to_string(),
                record.output_json.to_string(),
                record
                    .final_output_path
                    .as_ref()
                    .map(|path| path.display().to_string()),
                record.native_session_id,
                record.synthetic_session_id,
                record.status,
                record.fallback_final_source,
                now,
            ],
        )?;
        Ok(())
    }

    pub fn load_cli_driver_run(&self, run_id: &str) -> Result<CliDriverRunRecord> {
        let connection = self.connection()?;
        connection
            .query_row(
                r#"
SELECT
    run_id,
    agent_did,
    runtime_profile_id,
    driver_id,
    controller_did,
    conversation_id,
    route_key,
    workspace_id,
    workspace_root,
    workspace_instance_path,
    workspace_mode,
    is_security_boundary,
    command_json,
    output_json,
    final_output_path,
    native_session_id,
    synthetic_session_id,
    status,
    fallback_final_source
FROM cli_driver_run
WHERE run_id = ?1
"#,
                [run_id],
                cli_driver_run_from_row,
            )
            .context("load cli driver run")
    }

    pub fn authorize_runtime_rpc(
        &self,
        token: &RuntimeRpcToken,
        method: &RpcMethod,
        recipient: Option<&str>,
    ) -> Result<AuthorizedRuntimeContext> {
        self.authorize_runtime_rpc_with_message_policy(
            token,
            method,
            recipient.into_iter().collect::<Vec<_>>(),
            None,
        )
    }

    pub fn authorize_runtime_rpc_with_message_policy<'a>(
        &self,
        token: &RuntimeRpcToken,
        method: &RpcMethod,
        recipient_candidates: impl IntoIterator<Item = &'a str>,
        message_security: Option<&str>,
    ) -> Result<AuthorizedRuntimeContext> {
        self.authorize_runtime_rpc_internal(
            token,
            method,
            recipient_candidates,
            message_security,
            true,
        )
    }

    pub fn authorize_runtime_rpc_for_recipient_resolution(
        &self,
        token: &RuntimeRpcToken,
        method: &RpcMethod,
    ) -> Result<AuthorizedRuntimeContext> {
        self.authorize_runtime_rpc_internal(token, method, std::iter::empty::<&str>(), None, false)
    }

    fn authorize_runtime_rpc_internal<'a>(
        &self,
        token: &RuntimeRpcToken,
        method: &RpcMethod,
        recipient_candidates: impl IntoIterator<Item = &'a str>,
        message_security: Option<&str>,
        enforce_message_policy: bool,
    ) -> Result<AuthorizedRuntimeContext> {
        let connection = self.connection()?;
        let token_id = token.token_id();
        let record = load_runtime_token_record(&connection, &token_id)?;
        let audit_scope = record.scope_for_audit();
        let recipient_candidates = recipient_candidates
            .into_iter()
            .filter_map(|candidate| {
                let candidate = candidate.trim();
                (!candidate.is_empty()).then(|| candidate.to_string())
            })
            .collect::<Vec<_>>();
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
            if *method == RpcMethod::MsgSend && enforce_message_policy {
                if !record
                    .scope
                    .allows_recipient_candidates(recipient_candidates.iter().map(String::as_str))
                {
                    reason = "recipient_not_allowed".to_string();
                    bail!("runtime RPC recipient not allowed");
                }
                if !record.scope.allows_message_security(message_security) {
                    reason = "message_security_not_allowed".to_string();
                    bail!("runtime RPC message security not allowed");
                }
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

        self.insert_audit_event(
            &token_id,
            audit_scope,
            method,
            authorized,
            &reason,
            &recipient_candidates,
            message_security,
            enforce_message_policy,
        )?;

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
        recipient_candidates: &[String],
        message_security: Option<&str>,
        enforce_message_policy: bool,
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
            "recipient_candidates": recipient_candidates,
            "message_security": message_security,
            "message_policy_enforced": enforce_message_policy,
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

        CREATE TABLE IF NOT EXISTS cli_runtime_profile (
            runtime_profile_id TEXT PRIMARY KEY,
            driver_id TEXT NOT NULL,
            binary_path TEXT,
            config_home TEXT,
            auth_mode TEXT,
            default_model TEXT,
            default_sandbox TEXT,
            default_workspace_mode TEXT NOT NULL,
            recipient_policy_json TEXT NOT NULL,
            driver_config_json TEXT NOT NULL,
            status TEXT NOT NULL,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS cli_driver_run (
            run_id TEXT PRIMARY KEY,
            agent_did TEXT NOT NULL,
            runtime_profile_id TEXT NOT NULL,
            driver_id TEXT NOT NULL,
            controller_did TEXT NOT NULL,
            conversation_id TEXT,
            route_key TEXT NOT NULL,
            workspace_id TEXT,
            workspace_root TEXT,
            workspace_instance_path TEXT,
            workspace_mode TEXT,
            is_security_boundary INTEGER NOT NULL DEFAULT 0,
            command_json TEXT NOT NULL DEFAULT '{}',
            output_json TEXT NOT NULL DEFAULT '{}',
            final_output_path TEXT,
            native_session_id TEXT,
            synthetic_session_id TEXT,
            status TEXT NOT NULL,
            fallback_final_source TEXT,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL
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
            allowed_message_security_json TEXT,
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

        CREATE TABLE IF NOT EXISTS agent_auth_state (
            agent_did TEXT PRIMARY KEY,
            jwt_token TEXT NOT NULL,
            updated_at_ms INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS hermes_profiles (
            agent_did TEXT PRIMARY KEY,
            runtime_profile_id TEXT NOT NULL,
            hermes_profile TEXT NOT NULL,
            hermes_home TEXT NOT NULL,
            hermes_version TEXT,
            awiki_skills_version TEXT NOT NULL,
            status TEXT NOT NULL,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS hermes_native_sessions (
            id TEXT PRIMARY KEY,
            runtime_session_id TEXT NOT NULL,
            agent_did TEXT NOT NULL,
            runtime_profile_id TEXT NOT NULL,
            controller_did TEXT NOT NULL,
            conversation_id TEXT,
            route_key TEXT NOT NULL,
            hermes_profile TEXT NOT NULL,
            hermes_session_id TEXT NOT NULL,
            session_kind TEXT NOT NULL,
            status TEXT NOT NULL,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL
        );

        CREATE UNIQUE INDEX IF NOT EXISTS idx_hermes_native_sessions_active_route
        ON hermes_native_sessions(route_key)
        WHERE status = 'active';

        INSERT OR IGNORE INTO schema_migrations (version, applied_at)
        VALUES (1, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'));
        "#,
    )?;
    migrate_runtime_rpc_tokens_v2(connection)?;
    migrate_audit_log_v2(connection)?;
    migrate_runtime_run_v3(connection)?;
    migrate_runtime_task_v3(connection)?;
    migrate_agent_definition_v4(connection)?;
    migrate_agent_auth_state_v5(connection)?;
    migrate_hermes_profiles_v6(connection)?;
    migrate_hermes_native_sessions_v7(connection)?;
    migrate_cli_runtime_profile_v8(connection)?;
    migrate_runtime_rpc_tokens_v9(connection)?;
    migrate_cli_driver_run_v10(connection)?;
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

fn migrate_cli_driver_run_v10(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS cli_driver_run (
            run_id TEXT PRIMARY KEY,
            agent_did TEXT NOT NULL,
            runtime_profile_id TEXT NOT NULL,
            driver_id TEXT NOT NULL,
            controller_did TEXT NOT NULL,
            conversation_id TEXT,
            route_key TEXT NOT NULL,
            workspace_id TEXT,
            workspace_root TEXT,
            workspace_instance_path TEXT,
            workspace_mode TEXT,
            is_security_boundary INTEGER NOT NULL DEFAULT 0,
            command_json TEXT NOT NULL DEFAULT '{}',
            output_json TEXT NOT NULL DEFAULT '{}',
            final_output_path TEXT,
            native_session_id TEXT,
            synthetic_session_id TEXT,
            status TEXT NOT NULL,
            fallback_final_source TEXT,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL
        );
        "#,
    )?;
    for (column, definition) in [
        ("agent_did", "TEXT NOT NULL DEFAULT ''"),
        ("runtime_profile_id", "TEXT NOT NULL DEFAULT ''"),
        ("driver_id", "TEXT NOT NULL DEFAULT ''"),
        ("controller_did", "TEXT NOT NULL DEFAULT ''"),
        ("conversation_id", "TEXT"),
        ("route_key", "TEXT NOT NULL DEFAULT ''"),
        ("workspace_id", "TEXT"),
        ("workspace_root", "TEXT"),
        ("workspace_instance_path", "TEXT"),
        ("workspace_mode", "TEXT"),
        ("is_security_boundary", "INTEGER NOT NULL DEFAULT 0"),
        ("command_json", "TEXT NOT NULL DEFAULT '{}'"),
        ("output_json", "TEXT NOT NULL DEFAULT '{}'"),
        ("final_output_path", "TEXT"),
        ("native_session_id", "TEXT"),
        ("synthetic_session_id", "TEXT"),
        ("status", "TEXT NOT NULL DEFAULT 'created'"),
        ("fallback_final_source", "TEXT"),
        ("created_at_ms", "INTEGER NOT NULL DEFAULT 0"),
        ("updated_at_ms", "INTEGER NOT NULL DEFAULT 0"),
    ] {
        add_column_if_missing(connection, "cli_driver_run", column, definition)?;
    }
    Ok(())
}

fn migrate_runtime_rpc_tokens_v9(connection: &Connection) -> Result<()> {
    add_column_if_missing(
        connection,
        "runtime_rpc_tokens",
        "allowed_message_security_json",
        "TEXT",
    )?;
    Ok(())
}

fn migrate_cli_runtime_profile_v8(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS cli_runtime_profile (
            runtime_profile_id TEXT PRIMARY KEY,
            driver_id TEXT NOT NULL,
            binary_path TEXT,
            config_home TEXT,
            auth_mode TEXT,
            default_model TEXT,
            default_sandbox TEXT,
            default_workspace_mode TEXT NOT NULL,
            recipient_policy_json TEXT NOT NULL,
            driver_config_json TEXT NOT NULL,
            status TEXT NOT NULL,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL
        );
        "#,
    )?;
    for (column, definition) in [
        ("driver_id", "TEXT NOT NULL DEFAULT ''"),
        ("binary_path", "TEXT"),
        ("config_home", "TEXT"),
        ("auth_mode", "TEXT"),
        ("default_model", "TEXT"),
        ("default_sandbox", "TEXT"),
        (
            "default_workspace_mode",
            "TEXT NOT NULL DEFAULT 'shared-root'",
        ),
        (
            "recipient_policy_json",
            "TEXT NOT NULL DEFAULT '{\"mode\":\"controller-only\"}'",
        ),
        ("driver_config_json", "TEXT NOT NULL DEFAULT '{}'"),
        ("status", "TEXT NOT NULL DEFAULT 'active'"),
        ("created_at_ms", "INTEGER NOT NULL DEFAULT 0"),
        ("updated_at_ms", "INTEGER NOT NULL DEFAULT 0"),
    ] {
        add_column_if_missing(connection, "cli_runtime_profile", column, definition)?;
    }
    migrate_legacy_cli_runtime_profiles(connection)?;
    Ok(())
}

fn migrate_legacy_cli_runtime_profiles(connection: &Connection) -> Result<()> {
    for (legacy_plugin_id, driver_id) in [
        ("runtime.cli.codex", "codex"),
        ("runtime.cli.claude-code", "claude-code"),
        ("runtime.cli.gemini-cli", "gemini"),
    ] {
        connection.execute(
            r#"
INSERT INTO cli_runtime_profile (
    runtime_profile_id,
    driver_id,
    default_workspace_mode,
    recipient_policy_json,
    driver_config_json,
    status,
    created_at_ms,
    updated_at_ms
)
SELECT
    runtime_profile_id,
    ?2,
    'shared-root',
    ?3,
    '{}',
    status,
    0,
    0
FROM runtime_profile
WHERE runtime_plugin_id = ?1
  AND COALESCE(runtime_profile_id, '') <> ''
ON CONFLICT(runtime_profile_id) DO UPDATE SET
    driver_id = excluded.driver_id,
    default_workspace_mode = excluded.default_workspace_mode,
    recipient_policy_json = excluded.recipient_policy_json,
    driver_config_json = excluded.driver_config_json,
    status = excluded.status,
    updated_at_ms = excluded.updated_at_ms
"#,
            rusqlite::params![
                legacy_plugin_id,
                driver_id,
                DEFAULT_CLI_RECIPIENT_POLICY_JSON,
            ],
        )?;
        connection.execute(
            "UPDATE runtime_profile SET runtime_plugin_id = ?1 WHERE runtime_plugin_id = ?2",
            rusqlite::params![GENERIC_CLI_RUNTIME_PLUGIN_ID, legacy_plugin_id],
        )?;
        connection.execute(
            "UPDATE agent_definition SET runtime_plugin_id = ?1 WHERE runtime_plugin_id = ?2",
            rusqlite::params![GENERIC_CLI_RUNTIME_PLUGIN_ID, legacy_plugin_id],
        )?;
    }
    Ok(())
}

fn migrate_agent_auth_state_v5(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS agent_auth_state (
            agent_did TEXT PRIMARY KEY,
            jwt_token TEXT NOT NULL,
            updated_at_ms INTEGER NOT NULL
        );
        "#,
    )?;
    Ok(())
}

fn migrate_hermes_profiles_v6(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS hermes_profiles (
            agent_did TEXT PRIMARY KEY,
            runtime_profile_id TEXT NOT NULL,
            hermes_profile TEXT NOT NULL,
            hermes_home TEXT NOT NULL,
            hermes_version TEXT,
            awiki_skills_version TEXT NOT NULL,
            status TEXT NOT NULL,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL
        );
        "#,
    )?;
    for (column, definition) in [
        ("runtime_profile_id", "TEXT NOT NULL DEFAULT ''"),
        ("hermes_profile", "TEXT NOT NULL DEFAULT ''"),
        ("hermes_home", "TEXT NOT NULL DEFAULT ''"),
        ("hermes_version", "TEXT"),
        ("awiki_skills_version", "TEXT NOT NULL DEFAULT ''"),
        ("status", "TEXT NOT NULL DEFAULT 'unknown'"),
        ("created_at_ms", "INTEGER NOT NULL DEFAULT 0"),
        ("updated_at_ms", "INTEGER NOT NULL DEFAULT 0"),
    ] {
        add_column_if_missing(connection, "hermes_profiles", column, definition)?;
    }
    Ok(())
}

fn migrate_hermes_native_sessions_v7(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS hermes_native_sessions (
            id TEXT PRIMARY KEY,
            runtime_session_id TEXT NOT NULL,
            agent_did TEXT NOT NULL,
            runtime_profile_id TEXT NOT NULL,
            controller_did TEXT NOT NULL,
            conversation_id TEXT,
            route_key TEXT NOT NULL,
            hermes_profile TEXT NOT NULL,
            hermes_session_id TEXT NOT NULL,
            session_kind TEXT NOT NULL,
            status TEXT NOT NULL,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL
        );

        CREATE UNIQUE INDEX IF NOT EXISTS idx_hermes_native_sessions_active_route
        ON hermes_native_sessions(route_key)
        WHERE status = 'active';
        "#,
    )?;
    for (column, definition) in [
        ("runtime_session_id", "TEXT NOT NULL DEFAULT ''"),
        ("agent_did", "TEXT NOT NULL DEFAULT ''"),
        ("runtime_profile_id", "TEXT NOT NULL DEFAULT ''"),
        ("controller_did", "TEXT NOT NULL DEFAULT ''"),
        ("conversation_id", "TEXT"),
        ("route_key", "TEXT NOT NULL DEFAULT ''"),
        ("hermes_profile", "TEXT NOT NULL DEFAULT ''"),
        ("hermes_session_id", "TEXT NOT NULL DEFAULT ''"),
        ("session_kind", "TEXT NOT NULL DEFAULT 'conversation'"),
        ("status", "TEXT NOT NULL DEFAULT 'active'"),
        ("created_at_ms", "INTEGER NOT NULL DEFAULT 0"),
        ("updated_at_ms", "INTEGER NOT NULL DEFAULT 0"),
    ] {
        add_column_if_missing(connection, "hermes_native_sessions", column, definition)?;
    }
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
    allowed_message_security_json,
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
            let allowed_message_security_json: Option<String> = row.get(6)?;
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
            let allowed_message_security = allowed_message_security_json
                .as_ref()
                .map(|json| serde_json::from_str(json))
                .transpose()
                .map_err(|err| {
                    rusqlite::Error::FromSqlConversionFailure(
                        allowed_message_security_json
                            .as_deref()
                            .unwrap_or_default()
                            .len(),
                        rusqlite::types::Type::Text,
                        Box::new(err),
                    )
                })?;
            let single_use = row.get::<_, i64>(8)? != 0;
            Ok(RuntimeTokenRecord {
                token_secret_hash: row.get(0)?,
                scope: RuntimeTokenScope {
                    agent_did: row.get(1)?,
                    runtime_profile_id: row.get(2)?,
                    run_id: row.get(3)?,
                    allowed_methods,
                    allowed_recipients,
                    allowed_message_security,
                    expires_at_ms: row.get(7)?,
                    single_use,
                },
                single_use,
                revoked_at_ms: row.get(9)?,
                used_at_ms: row.get(10)?,
            })
        },
    )?;
    Ok(record)
}

fn cli_runtime_profile_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<CliRuntimeProfileRecord> {
    let default_workspace_mode_raw: String = row.get(7)?;
    let default_workspace_mode =
        WorkspaceMode::parse(&default_workspace_mode_raw).map_err(|err| {
            rusqlite::Error::FromSqlConversionFailure(
                default_workspace_mode_raw.len(),
                rusqlite::types::Type::Text,
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    err.to_string(),
                )),
            )
        })?;
    let recipient_policy_raw: String = row.get(8)?;
    let recipient_policy_json = serde_json::from_str(&recipient_policy_raw).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(
            recipient_policy_raw.len(),
            rusqlite::types::Type::Text,
            Box::new(err),
        )
    })?;
    let driver_config_raw: String = row.get(9)?;
    let driver_config_json = serde_json::from_str(&driver_config_raw).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(
            driver_config_raw.len(),
            rusqlite::types::Type::Text,
            Box::new(err),
        )
    })?;
    Ok(CliRuntimeProfileRecord {
        runtime_profile_id: row.get(0)?,
        driver_id: row.get(1)?,
        binary_path: row.get::<_, Option<String>>(2)?.map(PathBuf::from),
        config_home: row.get::<_, Option<String>>(3)?.map(PathBuf::from),
        auth_mode: row.get(4)?,
        default_model: row.get(5)?,
        default_sandbox: row.get(6)?,
        default_workspace_mode,
        recipient_policy_json,
        driver_config_json,
        status: row.get(10)?,
    })
}

fn cli_driver_run_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<CliDriverRunRecord> {
    let workspace_mode_raw: Option<String> = row.get(10)?;
    let workspace_mode = match workspace_mode_raw {
        Some(raw) => Some(WorkspaceMode::parse(&raw).map_err(|err| {
            rusqlite::Error::FromSqlConversionFailure(
                raw.len(),
                rusqlite::types::Type::Text,
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    err.to_string(),
                )),
            )
        })?),
        None => None,
    };
    let command_raw: String = row.get(12)?;
    let command_json = serde_json::from_str(&command_raw).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(
            command_raw.len(),
            rusqlite::types::Type::Text,
            Box::new(err),
        )
    })?;
    let output_raw: String = row.get(13)?;
    let output_json = serde_json::from_str(&output_raw).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(
            output_raw.len(),
            rusqlite::types::Type::Text,
            Box::new(err),
        )
    })?;
    Ok(CliDriverRunRecord {
        run_id: row.get(0)?,
        agent_did: row.get(1)?,
        runtime_profile_id: row.get(2)?,
        driver_id: row.get(3)?,
        controller_did: row.get(4)?,
        conversation_id: row.get(5)?,
        route_key: row.get(6)?,
        workspace_id: row.get(7)?,
        workspace_root: row.get::<_, Option<String>>(8)?.map(PathBuf::from),
        workspace_instance_path: row.get::<_, Option<String>>(9)?.map(PathBuf::from),
        workspace_mode,
        is_security_boundary: row.get::<_, i64>(11)? != 0,
        command_json,
        output_json,
        final_output_path: row.get::<_, Option<String>>(14)?.map(PathBuf::from),
        native_session_id: row.get(15)?,
        synthetic_session_id: row.get(16)?,
        status: row.get(17)?,
        fallback_final_source: row.get(18)?,
    })
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

fn hermes_native_session_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<HermesNativeSessionRecord> {
    Ok(HermesNativeSessionRecord {
        id: row.get(0)?,
        runtime_session_id: row.get(1)?,
        agent_did: row.get(2)?,
        runtime_profile_id: row.get(3)?,
        controller_did: row.get(4)?,
        conversation_id: row.get(5)?,
        route_key: row.get(6)?,
        hermes_profile: row.get(7)?,
        hermes_session_id: row.get(8)?,
        session_kind: row.get(9)?,
        status: row.get(10)?,
        created_at_ms: row.get(11)?,
        updated_at_ms: row.get(12)?,
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
            "agent_auth_state",
            "cli_runtime_profile",
            "cli_driver_run",
            "hermes_profiles",
            "hermes_native_sessions",
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
    fn cli_runtime_profile_roundtrips_with_controller_only_default_policy() {
        let root = tempfile::tempdir().unwrap();
        let config = DaemonConfig::for_state_root(root.path()).unwrap();
        let state = DaemonState::open(&config).unwrap();
        state.initialize().unwrap();

        let mut profile =
            CliRuntimeProfileRecord::for_driver("profile_generic_cli_alice", "codex-cli").unwrap();
        profile.binary_path = Some(PathBuf::from("/usr/local/bin/codex"));
        profile.config_home = Some(PathBuf::from("/tmp/codex-config"));
        profile.auth_mode = Some("user-local".to_string());
        profile.default_model = Some("gpt-5-codex".to_string());
        profile.driver_config_json = serde_json::json!({
            "api_key": "driver-secret-value"
        });
        state.upsert_cli_runtime_profile(&profile).unwrap();

        let loaded = state
            .load_cli_runtime_profile("profile_generic_cli_alice")
            .unwrap();
        assert_eq!(loaded.runtime_profile_id, "profile_generic_cli_alice");
        assert_eq!(loaded.driver_id, "codex");
        assert_eq!(
            loaded.recipient_policy_json,
            serde_json::json!({ "mode": "controller-only" })
        );
        assert_eq!(loaded.default_workspace_mode, WorkspaceMode::SharedRoot);
        assert_eq!(
            state.list_cli_runtime_profiles().unwrap(),
            vec![loaded.clone()]
        );
        assert!(!format!("{loaded:?}").contains("driver-secret-value"));
    }

    #[test]
    fn cli_runtime_profile_rejects_invalid_policy_and_driver() {
        let mut profile =
            CliRuntimeProfileRecord::for_driver("profile_generic_cli_alice", "codex").unwrap();
        profile.recipient_policy_json = serde_json::json!(["did:human:alice"]);
        assert!(profile
            .validate()
            .unwrap_err()
            .to_string()
            .contains("policy"));

        let error = CliRuntimeProfileRecord::for_driver("profile_generic_cli_alice", " ");
        assert!(error.unwrap_err().to_string().contains("driver_id"));
    }

    #[test]
    fn cli_runtime_profile_v8_migrates_legacy_cli_plugin_types() {
        let root = tempfile::tempdir().unwrap();
        let db_path = root.path().join("daemon.db");
        let connection = Connection::open(&db_path).unwrap();
        connection
            .execute_batch(
                r#"
                CREATE TABLE schema_migrations (
                    version INTEGER PRIMARY KEY,
                    applied_at TEXT NOT NULL
                );
                INSERT INTO schema_migrations (version, applied_at)
                VALUES (7, 'legacy-fixture');

                CREATE TABLE runtime_profile (
                    runtime_profile_id TEXT PRIMARY KEY,
                    agent_did TEXT,
                    runtime_plugin_id TEXT NOT NULL,
                    display_name TEXT,
                    status TEXT NOT NULL,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL
                );
                INSERT INTO runtime_profile (
                    runtime_profile_id,
                    agent_did,
                    runtime_plugin_id,
                    display_name,
                    status,
                    created_at,
                    updated_at
                ) VALUES
                    ('profile_codex', 'did:agent:codex', 'runtime.cli.codex', 'Codex', 'active', '0', '0'),
                    ('profile_claude', 'did:agent:claude', 'runtime.cli.claude-code', 'Claude', 'active', '0', '0'),
                    ('profile_gemini', 'did:agent:gemini', 'runtime.cli.gemini-cli', 'Gemini', 'active', '0', '0'),
                    ('profile_hermes', 'did:agent:hermes', 'runtime.hermes', 'Hermes', 'active', '0', '0');

                CREATE TABLE agent_definition (
                    agent_did TEXT PRIMARY KEY,
                    controller_did TEXT NOT NULL,
                    runtime_profile_id TEXT,
                    status TEXT NOT NULL,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL,
                    handle TEXT NOT NULL DEFAULT '',
                    agent_kind TEXT NOT NULL DEFAULT 'runtime',
                    runtime_plugin_id TEXT,
                    workspace_id TEXT,
                    policy_id TEXT NOT NULL DEFAULT 'default',
                    local_agent_db_path TEXT NOT NULL DEFAULT '',
                    message_db_path TEXT NOT NULL DEFAULT ''
                );
                INSERT INTO agent_definition (
                    agent_did,
                    controller_did,
                    runtime_profile_id,
                    status,
                    created_at,
                    updated_at,
                    handle,
                    agent_kind,
                    runtime_plugin_id,
                    policy_id,
                    local_agent_db_path,
                    message_db_path
                ) VALUES
                    ('did:agent:codex', 'did:human:alice', 'profile_codex', 'active', '0', '0', 'codex', 'runtime', 'runtime.cli.codex', 'default', 'agents/codex/agent.db', 'agents/codex/messages.db'),
                    ('did:agent:hermes', 'did:human:alice', 'profile_hermes', 'active', '0', '0', 'hermes', 'runtime', 'runtime.hermes', 'default', 'agents/hermes/agent.db', 'agents/hermes/messages.db');
                "#,
            )
            .unwrap();
        drop(connection);

        let config = DaemonConfig::for_state_root(root.path()).unwrap();
        let state = DaemonState::open(&config).unwrap();
        state.initialize().unwrap();

        let connection = Connection::open(db_path).unwrap();
        let runtime_plugins: Vec<(String, String)> = {
            let mut statement = connection
                .prepare(
                    "SELECT runtime_profile_id, runtime_plugin_id FROM runtime_profile ORDER BY runtime_profile_id",
                )
                .unwrap();
            statement
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
                .unwrap()
                .collect::<rusqlite::Result<Vec<_>>>()
                .unwrap()
        };
        assert_eq!(
            runtime_plugins,
            vec![
                ("profile_claude".to_string(), "generic-cli".to_string()),
                ("profile_codex".to_string(), "generic-cli".to_string()),
                ("profile_gemini".to_string(), "generic-cli".to_string()),
                ("profile_hermes".to_string(), "runtime.hermes".to_string()),
            ]
        );

        let cli_profiles = state.list_cli_runtime_profiles().unwrap();
        let mapped: Vec<(String, String, serde_json::Value)> = cli_profiles
            .into_iter()
            .map(|profile| {
                (
                    profile.runtime_profile_id,
                    profile.driver_id,
                    profile.recipient_policy_json,
                )
            })
            .collect();
        assert_eq!(
            mapped,
            vec![
                (
                    "profile_claude".to_string(),
                    "claude-code".to_string(),
                    serde_json::json!({ "mode": "controller-only" })
                ),
                (
                    "profile_codex".to_string(),
                    "codex".to_string(),
                    serde_json::json!({ "mode": "controller-only" })
                ),
                (
                    "profile_gemini".to_string(),
                    "gemini".to_string(),
                    serde_json::json!({ "mode": "controller-only" })
                ),
            ]
        );

        let migrated_agent = state.load_agent_definition("did:agent:codex").unwrap();
        assert_eq!(
            migrated_agent.runtime_plugin_id.as_deref(),
            Some("generic-cli")
        );
        let hermes_agent = state.load_agent_definition("did:agent:hermes").unwrap();
        assert_eq!(
            hermes_agent.runtime_plugin_id.as_deref(),
            Some("runtime.hermes")
        );
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

    #[test]
    fn agent_auth_token_roundtrips_without_audit_log_side_effects() {
        let root = tempfile::tempdir().unwrap();
        let config = DaemonConfig::for_state_root(root.path()).unwrap();
        let state = DaemonState::open(&config).unwrap();
        state.initialize().unwrap();

        state
            .store_agent_auth_token("did:agent:daemon", "jwt-secret-value")
            .unwrap();

        assert_eq!(
            state
                .load_agent_auth_token("did:agent:daemon")
                .unwrap()
                .as_deref(),
            Some("jwt-secret-value")
        );
        assert_eq!(
            state.list_agent_auth_tokens().unwrap(),
            vec![(
                "did:agent:daemon".to_string(),
                "jwt-secret-value".to_string()
            )]
        );

        let audit_count: i64 = state
            .connection()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM audit_log", [], |row| row.get(0))
            .unwrap();
        assert_eq!(audit_count, 0);
    }

    #[test]
    fn hermes_native_session_roundtrips_and_resets_active_route() {
        let root = tempfile::tempdir().unwrap();
        let config = DaemonConfig::for_state_root(root.path()).unwrap();
        let state = DaemonState::open(&config).unwrap();
        state.initialize().unwrap();
        let route = HermesSessionRoute::new(
            "did:agent:hermes",
            "profile_hermes_alice",
            "did:human:alice",
            Some("direct:did:human:alice".to_string()),
            "conversation",
        );
        let session =
            HermesNativeSessionRecord::active(&route, "awiki_alice_hermes", "hermes-session-1")
                .unwrap();

        state.store_hermes_native_session(&session).unwrap();
        assert_eq!(
            state
                .load_active_hermes_session_by_route(&route)
                .unwrap()
                .unwrap(),
            session
        );

        let connection = state.connection().unwrap();
        let unique_index_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = 'idx_hermes_native_sessions_active_route'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(unique_index_count, 1);
        drop(connection);

        assert_eq!(
            state.reset_active_hermes_session_by_route(&route).unwrap(),
            1
        );
        assert!(state
            .load_active_hermes_session_by_route(&route)
            .unwrap()
            .is_none());

        let replacement =
            HermesNativeSessionRecord::active(&route, "awiki_alice_hermes", "hermes-session-2")
                .unwrap();
        state.store_hermes_native_session(&replacement).unwrap();
        assert_eq!(
            state
                .load_active_hermes_session_by_route(&route)
                .unwrap()
                .unwrap()
                .hermes_session_id,
            "hermes-session-2"
        );

        let reopened = DaemonState::open(&config).unwrap();
        assert_eq!(
            reopened
                .load_active_hermes_session_by_route(&route)
                .unwrap()
                .unwrap()
                .hermes_session_id,
            "hermes-session-2"
        );
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
