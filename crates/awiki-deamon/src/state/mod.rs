use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{bail, Context, Result};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior};
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

const DAEMON_SCHEMA_VERSION: i64 = 20;
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
    pub controller_user_id: String,
    pub controller_full_handle: String,
    pub controller_scope_key: String,
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
            ("controller_user_id", self.controller_user_id.as_str()),
            (
                "controller_full_handle",
                self.controller_full_handle.as_str(),
            ),
            ("controller_scope_key", self.controller_scope_key.as_str()),
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

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserDelegatedIdentityRecord {
    pub user_did: String,
    pub verification_method: String,
    pub app_instance_id: String,
    pub controller_did: String,
    pub daemon_agent_did: String,
    pub public_key_multibase: String,
    pub private_key_material: String,
    pub allowed_scopes_json: Value,
    pub status: String,
    pub expires_at: Option<String>,
    pub bootstrap_id: String,
    pub idempotency_key: String,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BootstrapReplayRecord {
    pub bootstrap_id: String,
    pub idempotency_key: String,
    pub payload_hash: String,
    pub user_did: String,
    pub verification_method: String,
    pub app_instance_id: String,
    pub daemon_agent_did: String,
    pub status: String,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootstrapStoreOutcome {
    Inserted,
    Duplicate,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppMessageAgentBindingRecord {
    pub binding_id: String,
    pub user_did: String,
    pub inbox_auth_verification_method: String,
    pub app_instance_id: String,
    pub bootstrap_id: String,
    pub idempotency_key: String,
    pub daemon_agent_did: String,
    pub runtime_agent_did: String,
    pub runtime_profile_id: String,
    pub role: String,
    pub desired_agent_json: Value,
    pub capability_policy_json: Value,
    pub status: String,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub revoked_at_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InboxCursorRecord {
    pub owner_did: String,
    pub inbox_scope: String,
    pub cursor: Option<String>,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessedMessageRecord {
    pub owner_did: String,
    pub message_id: String,
    pub schema: String,
    pub processed_at_ms: i64,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageEventRecord {
    pub event_id: String,
    pub owner_did: String,
    pub conversation_id: Option<String>,
    pub message_id: String,
    pub message_kind: String,
    pub sender_did: String,
    pub received_at: Option<String>,
    pub plain_text_ref_or_excerpt: Option<String>,
    pub content_hash: String,
    pub schema: String,
    pub processing_status: String,
    pub retention_class: String,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageSyncOutboxRecord {
    pub idempotency_key: String,
    pub owner_did: String,
    pub app_instance_id: String,
    pub payload_json: Value,
    pub status: String,
    pub attempt_count: i64,
    pub next_attempt_at_ms: i64,
    pub last_error_code: Option<String>,
    pub last_error_summary: Option<String>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub sent_at_ms: Option<i64>,
}

impl std::fmt::Debug for UserDelegatedIdentityRecord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UserDelegatedIdentityRecord")
            .field("user_did", &self.user_did)
            .field("verification_method", &self.verification_method)
            .field("app_instance_id", &self.app_instance_id)
            .field("controller_did", &self.controller_did)
            .field("daemon_agent_did", &self.daemon_agent_did)
            .field("public_key_multibase", &self.public_key_multibase)
            .field("private_key_material", &"<redacted-private-key>")
            .field("allowed_scopes_json", &self.allowed_scopes_json)
            .field("status", &self.status)
            .field("expires_at", &self.expires_at)
            .field("bootstrap_id", &self.bootstrap_id)
            .field("idempotency_key", &self.idempotency_key)
            .field("created_at_ms", &self.created_at_ms)
            .field("updated_at_ms", &self.updated_at_ms)
            .finish()
    }
}

impl UserDelegatedIdentityRecord {
    pub fn validate(&self) -> Result<()> {
        for (field_name, value) in [
            ("user_did", self.user_did.as_str()),
            ("verification_method", self.verification_method.as_str()),
            ("app_instance_id", self.app_instance_id.as_str()),
            ("controller_did", self.controller_did.as_str()),
            ("daemon_agent_did", self.daemon_agent_did.as_str()),
            ("public_key_multibase", self.public_key_multibase.as_str()),
            ("private_key_material", self.private_key_material.as_str()),
            ("status", self.status.as_str()),
            ("bootstrap_id", self.bootstrap_id.as_str()),
            ("idempotency_key", self.idempotency_key.as_str()),
        ] {
            if value.trim().is_empty() {
                bail!("{field_name} must not be empty");
            }
        }
        if !self.allowed_scopes_json.is_array() {
            bail!("allowed_scopes_json must be a JSON array");
        }
        Ok(())
    }
}

impl AppMessageAgentBindingRecord {
    pub fn validate(&self) -> Result<()> {
        for (field_name, value) in [
            ("binding_id", self.binding_id.as_str()),
            ("user_did", self.user_did.as_str()),
            (
                "inbox_auth_verification_method",
                self.inbox_auth_verification_method.as_str(),
            ),
            ("app_instance_id", self.app_instance_id.as_str()),
            ("bootstrap_id", self.bootstrap_id.as_str()),
            ("idempotency_key", self.idempotency_key.as_str()),
            ("daemon_agent_did", self.daemon_agent_did.as_str()),
            ("runtime_agent_did", self.runtime_agent_did.as_str()),
            ("runtime_profile_id", self.runtime_profile_id.as_str()),
            ("role", self.role.as_str()),
            ("status", self.status.as_str()),
        ] {
            if value.trim().is_empty() {
                bail!("{field_name} must not be empty");
            }
        }
        if !self.desired_agent_json.is_object() {
            bail!("desired_agent_json must be a JSON object");
        }
        if !self.capability_policy_json.is_object() {
            bail!("capability_policy_json must be a JSON object");
        }
        Ok(())
    }
}

impl InboxCursorRecord {
    pub fn validate(&self) -> Result<()> {
        if self.owner_did.trim().is_empty() {
            bail!("owner_did must not be empty");
        }
        if self.inbox_scope.trim().is_empty() {
            bail!("inbox_scope must not be empty");
        }
        if self
            .cursor
            .as_deref()
            .is_some_and(|cursor| cursor.trim().is_empty())
        {
            bail!("cursor must not be empty when present");
        }
        Ok(())
    }
}

impl ProcessedMessageRecord {
    pub fn validate(&self) -> Result<()> {
        for (field_name, value) in [
            ("owner_did", self.owner_did.as_str()),
            ("message_id", self.message_id.as_str()),
            ("schema", self.schema.as_str()),
            ("status", self.status.as_str()),
        ] {
            if value.trim().is_empty() {
                bail!("{field_name} must not be empty");
            }
        }
        Ok(())
    }
}

impl MessageEventRecord {
    pub fn validate(&self) -> Result<()> {
        for (field_name, value) in [
            ("event_id", self.event_id.as_str()),
            ("owner_did", self.owner_did.as_str()),
            ("message_id", self.message_id.as_str()),
            ("message_kind", self.message_kind.as_str()),
            ("sender_did", self.sender_did.as_str()),
            ("content_hash", self.content_hash.as_str()),
            ("schema", self.schema.as_str()),
            ("processing_status", self.processing_status.as_str()),
            ("retention_class", self.retention_class.as_str()),
        ] {
            if value.trim().is_empty() {
                bail!("{field_name} must not be empty");
            }
        }
        if self
            .plain_text_ref_or_excerpt
            .as_deref()
            .is_some_and(|value| value.chars().count() > 512)
        {
            bail!("plain_text_ref_or_excerpt must be a short projection");
        }
        Ok(())
    }
}

impl MessageSyncOutboxRecord {
    pub fn validate(&self) -> Result<()> {
        for (field_name, value) in [
            ("idempotency_key", self.idempotency_key.as_str()),
            ("owner_did", self.owner_did.as_str()),
            ("app_instance_id", self.app_instance_id.as_str()),
            ("status", self.status.as_str()),
        ] {
            if value.trim().is_empty() {
                bail!("{field_name} must not be empty");
            }
        }
        if !self.payload_json.is_object() {
            bail!("message sync outbox payload_json must be a JSON object");
        }
        Ok(())
    }
}

impl BootstrapReplayRecord {
    pub fn validate(&self) -> Result<()> {
        for (field_name, value) in [
            ("bootstrap_id", self.bootstrap_id.as_str()),
            ("idempotency_key", self.idempotency_key.as_str()),
            ("payload_hash", self.payload_hash.as_str()),
            ("user_did", self.user_did.as_str()),
            ("verification_method", self.verification_method.as_str()),
            ("app_instance_id", self.app_instance_id.as_str()),
            ("daemon_agent_did", self.daemon_agent_did.as_str()),
            ("status", self.status.as_str()),
        ] {
            if value.trim().is_empty() {
                bail!("{field_name} must not be empty");
            }
        }
        Ok(())
    }
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
    pub controller_scope_key: String,
    pub conversation_id: Option<String>,
    pub session_kind: String,
}

impl HermesSessionRoute {
    pub fn new(
        agent_did: impl Into<String>,
        runtime_profile_id: impl Into<String>,
        controller_scope_key: impl Into<String>,
        conversation_id: Option<String>,
        session_kind: impl Into<String>,
    ) -> Self {
        Self {
            agent_did: agent_did.into(),
            runtime_profile_id: runtime_profile_id.into(),
            controller_scope_key: controller_scope_key.into(),
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
        if self.controller_scope_key.trim().is_empty() {
            bail!("controller_scope_key must not be empty");
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
            self.controller_scope_key,
            self.conversation_id
                .as_deref()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or(""),
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
    pub controller_scope_key: String,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeDaemonBindingRecord {
    pub runtime_agent_did: String,
    pub daemon_agent_did: String,
    pub controller_user_id: String,
    pub controller_full_handle: String,
    pub controller_scope_key: String,
    pub controller_did: String,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeRetryQueueRecord {
    pub retry_id: String,
    pub original_run_id: String,
    pub task_id: String,
    pub agent_did: String,
    pub runtime_profile_id: String,
    pub runtime_plugin_id: String,
    pub workspace_id: Option<String>,
    pub status: String,
    pub requested_by_command_id: String,
    pub attempts: i64,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeFinalOutboxRecord {
    pub idempotency_key: String,
    pub run_id: String,
    pub agent_did: String,
    pub runtime_profile_id: String,
    pub controller_scope_key: String,
    pub controller_did: String,
    pub conversation_id: Option<String>,
    pub final_text: String,
    pub security: String,
    pub status: String,
    pub attempt_count: i64,
    pub next_attempt_at_ms: i64,
    pub last_error_code: Option<String>,
    pub last_error_summary: Option<String>,
    pub message_id: Option<String>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub sent_at_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeAgentCreateRequestRecord {
    pub daemon_agent_did: String,
    pub controller_scope_key: String,
    pub controller_did: String,
    pub client_request_id: String,
    pub runtime_agent_did: String,
    pub command_id: String,
    pub outcome_json: Value,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControlCommandStateRecord {
    pub daemon_agent_did: String,
    pub controller_scope_key: String,
    pub command_id: String,
    pub command: String,
    pub message_id: String,
    pub status: String,
    pub target_version: Option<String>,
    pub result_json: Value,
    pub error_summary: Option<String>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

impl RuntimeRetryQueueRecord {
    pub fn validate(&self) -> Result<()> {
        for (field_name, value) in [
            ("retry_id", self.retry_id.as_str()),
            ("original_run_id", self.original_run_id.as_str()),
            ("task_id", self.task_id.as_str()),
            ("agent_did", self.agent_did.as_str()),
            ("runtime_profile_id", self.runtime_profile_id.as_str()),
            ("runtime_plugin_id", self.runtime_plugin_id.as_str()),
            (
                "requested_by_command_id",
                self.requested_by_command_id.as_str(),
            ),
            ("status", self.status.as_str()),
        ] {
            if value.trim().is_empty() {
                bail!("{field_name} must not be empty");
            }
        }
        Ok(())
    }
}

impl RuntimeFinalOutboxRecord {
    pub fn validate(&self) -> Result<()> {
        for (field_name, value) in [
            ("idempotency_key", self.idempotency_key.as_str()),
            ("run_id", self.run_id.as_str()),
            ("agent_did", self.agent_did.as_str()),
            ("runtime_profile_id", self.runtime_profile_id.as_str()),
            ("controller_scope_key", self.controller_scope_key.as_str()),
            ("controller_did", self.controller_did.as_str()),
            ("final_text", self.final_text.as_str()),
            ("security", self.security.as_str()),
            ("status", self.status.as_str()),
        ] {
            if value.trim().is_empty() {
                bail!("{field_name} must not be empty");
            }
        }
        if !matches!(
            self.status.as_str(),
            "pending" | "sending" | "sent" | "failed_terminal"
        ) {
            bail!("runtime final outbox status is unsupported");
        }
        Ok(())
    }
}

impl RuntimeAgentCreateRequestRecord {
    pub fn validate(&self) -> Result<()> {
        for (field_name, value) in [
            ("daemon_agent_did", self.daemon_agent_did.as_str()),
            ("controller_scope_key", self.controller_scope_key.as_str()),
            ("controller_did", self.controller_did.as_str()),
            ("client_request_id", self.client_request_id.as_str()),
            ("runtime_agent_did", self.runtime_agent_did.as_str()),
            ("command_id", self.command_id.as_str()),
        ] {
            if value.trim().is_empty() {
                bail!("{field_name} must not be empty");
            }
        }
        if !self.outcome_json.is_object() {
            bail!("runtime agent create outcome must be a JSON object");
        }
        Ok(())
    }
}

impl ControlCommandStateRecord {
    pub fn validate(&self) -> Result<()> {
        for (field_name, value) in [
            ("daemon_agent_did", self.daemon_agent_did.as_str()),
            ("controller_scope_key", self.controller_scope_key.as_str()),
            ("command_id", self.command_id.as_str()),
            ("command", self.command.as_str()),
            ("message_id", self.message_id.as_str()),
            ("status", self.status.as_str()),
        ] {
            if value.trim().is_empty() {
                bail!("{field_name} must not be empty");
            }
        }
        validate_control_command_status(&self.status)?;
        Ok(())
    }
}

fn validate_control_command_status(status: &str) -> Result<()> {
    if !matches!(
        status,
        "in_progress" | "restart_scheduled" | "succeeded" | "failed"
    ) {
        bail!("control command status is unsupported");
    }
    Ok(())
}

impl RuntimeDaemonBindingRecord {
    pub fn validate(&self) -> Result<()> {
        if self.runtime_agent_did.trim().is_empty() {
            bail!("runtime_agent_did must not be empty");
        }
        if self.daemon_agent_did.trim().is_empty() {
            bail!("daemon_agent_did must not be empty");
        }
        if self.controller_user_id.trim().is_empty() {
            bail!("controller_user_id must not be empty");
        }
        if self.controller_full_handle.trim().is_empty() {
            bail!("controller_full_handle must not be empty");
        }
        if self.controller_scope_key.trim().is_empty() {
            bail!("controller_scope_key must not be empty");
        }
        if self.controller_did.trim().is_empty() {
            bail!("controller_did must not be empty");
        }
        Ok(())
    }
}

impl HermesNativeSessionRecord {
    pub fn active(
        route: &HermesSessionRoute,
        controller_did: impl Into<String>,
        hermes_profile: impl Into<String>,
        hermes_session_id: impl Into<String>,
    ) -> Result<Self> {
        route.validate()?;
        let controller_did = controller_did.into();
        if controller_did.trim().is_empty() {
            bail!("controller_did must not be empty");
        }
        let route_key = route.route_key();
        let hermes_session_id = hermes_session_id.into();
        let id = stable_hermes_session_record_id(&route_key, &hermes_session_id);
        let now = current_time_millis()?;
        Ok(Self {
            runtime_session_id: format!("rs_{id}"),
            id,
            agent_did: route.agent_did.clone(),
            runtime_profile_id: route.runtime_profile_id.clone(),
            controller_scope_key: route.controller_scope_key.clone(),
            controller_did,
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
        if self.controller_scope_key.trim().is_empty() {
            bail!("controller_scope_key must not be empty");
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

pub fn controller_scope_key(
    controller_user_id: &str,
    controller_full_handle: &str,
) -> Result<String> {
    let controller_user_id = controller_user_id.trim();
    let controller_full_handle = controller_full_handle.trim().to_ascii_lowercase();
    if controller_user_id.is_empty() {
        bail!("controller_user_id must not be empty");
    }
    if controller_full_handle.is_empty() {
        bail!("controller_full_handle must not be empty");
    }
    let material = format!("user:{controller_user_id}\nhandle:{controller_full_handle}");
    Ok(format!(
        "controller-scope:v1:{:x}",
        Sha256::digest(material.as_bytes())
    ))
}

pub fn legacy_controller_scope_key_for_did(controller_did: &str) -> Result<String> {
    let controller_did = controller_did.trim();
    if controller_did.is_empty() {
        bail!("controller_did must not be empty");
    }
    Ok(format!(
        "controller-scope:legacy-did:{:x}",
        Sha256::digest(controller_did.as_bytes())
    ))
}

fn stable_id_suffix(input: &str) -> String {
    let digest = Sha256::digest(input.as_bytes());
    digest
        .iter()
        .take(8)
        .map(|byte| format!("{byte:02x}"))
        .collect()
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
    controller_user_id,
    controller_full_handle,
    controller_scope_key,
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
) VALUES (?1, ?2, 'runtime', ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'default', ?10, ?11, 'active', ?12, ?12)
ON CONFLICT(agent_did) DO UPDATE SET
    controller_user_id = excluded.controller_user_id,
    controller_full_handle = excluded.controller_full_handle,
    controller_scope_key = excluded.controller_scope_key,
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
                profile.controller_user_id,
                profile.controller_full_handle,
                profile.controller_scope_key,
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
    controller_user_id,
    controller_full_handle,
    controller_scope_key,
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
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?15)
ON CONFLICT(agent_did) DO UPDATE SET
    handle = excluded.handle,
    agent_kind = excluded.agent_kind,
    controller_user_id = excluded.controller_user_id,
    controller_full_handle = excluded.controller_full_handle,
    controller_scope_key = excluded.controller_scope_key,
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
                definition.controller_user_id,
                definition.controller_full_handle,
                definition.controller_scope_key,
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

    pub fn update_controller_did_for_agent_family(
        &self,
        daemon_agent_did: &str,
        controller_did: &str,
    ) -> Result<usize> {
        if daemon_agent_did.trim().is_empty() {
            bail!("daemon_agent_did must not be empty");
        }
        if controller_did.trim().is_empty() {
            bail!("controller_did must not be empty");
        }
        let connection = self.connection()?;
        let now = current_time_millis()?;
        let mut updated = 0usize;
        updated += connection.execute(
            r#"
UPDATE agent_definition
SET controller_did = ?1,
    updated_at = ?2
WHERE agent_did = ?3
   OR agent_did IN (
       SELECT runtime_agent_did
       FROM runtime_daemon_binding
       WHERE daemon_agent_did = ?3
   )
"#,
            rusqlite::params![controller_did, now.to_string(), daemon_agent_did],
        )?;
        updated += connection.execute(
            r#"
UPDATE runtime_daemon_binding
SET controller_did = ?1,
    updated_at_ms = ?2
WHERE daemon_agent_did = ?3
"#,
            rusqlite::params![controller_did, now, daemon_agent_did],
        )?;
        updated += connection.execute(
            r#"
UPDATE runtime_task
SET controller_did = ?1,
    updated_at_ms = ?2
WHERE agent_did IN (
    SELECT runtime_agent_did
    FROM runtime_daemon_binding
    WHERE daemon_agent_did = ?3
)
  AND status IN ('pending', 'running')
"#,
            rusqlite::params![controller_did, now, daemon_agent_did],
        )?;
        updated += connection.execute(
            r#"
UPDATE runtime_agent_create_request
SET controller_did = ?1,
    updated_at_ms = ?2
WHERE daemon_agent_did = ?3
"#,
            rusqlite::params![controller_did, now, daemon_agent_did],
        )?;
        updated += connection.execute(
            r#"
UPDATE agent_status_query_throttle
SET controller_did = ?1
WHERE daemon_agent_did = ?2
"#,
            rusqlite::params![controller_did, daemon_agent_did],
        )?;
        Ok(updated)
    }

    pub fn mark_agent_archived(&self, agent_did: &str) -> Result<usize> {
        if agent_did.trim().is_empty() {
            bail!("agent_did must not be empty");
        }
        let connection = self.connection()?;
        let now_ms = current_time_millis()?;
        let now = now_ms.to_string();
        let mut updated = 0usize;
        updated += connection.execute(
            r#"
UPDATE agent_definition
SET status = 'archived',
    updated_at = ?2
WHERE agent_did = ?1
"#,
            rusqlite::params![agent_did, now],
        )?;
        updated += connection.execute(
            r#"
UPDATE runtime_profile
SET status = 'archived',
    updated_at = ?2
WHERE agent_did = ?1
"#,
            rusqlite::params![agent_did, now],
        )?;
        updated += connection.execute(
            r#"
UPDATE workspace_binding
SET status = 'archived',
    updated_at = ?2
WHERE agent_did = ?1
"#,
            rusqlite::params![agent_did, now],
        )?;
        updated += connection.execute(
            r#"
UPDATE cli_runtime_profile
SET status = 'archived',
    updated_at_ms = ?2
WHERE runtime_profile_id IN (
    SELECT runtime_profile_id
    FROM agent_definition
    WHERE agent_did = ?1
      AND runtime_profile_id IS NOT NULL
)
"#,
            rusqlite::params![agent_did, now_ms],
        )?;
        updated += connection.execute(
            r#"
UPDATE hermes_profiles
SET status = 'archived',
    updated_at_ms = ?2
WHERE agent_did = ?1
"#,
            rusqlite::params![agent_did, now_ms],
        )?;
        updated += connection.execute(
            r#"
UPDATE hermes_native_sessions
SET status = 'archived',
    updated_at_ms = ?2
WHERE agent_did = ?1
  AND status = 'active'
"#,
            rusqlite::params![agent_did, now_ms],
        )?;
        updated += connection.execute(
            r#"
UPDATE runtime_task
SET status = 'archived',
    updated_at_ms = ?2
WHERE agent_did = ?1
  AND status IN ('created', 'pending', 'running')
"#,
            rusqlite::params![agent_did, now_ms],
        )?;
        updated += connection.execute(
            r#"
UPDATE runtime_retry_queue
SET status = 'archived',
    updated_at_ms = ?2
WHERE agent_did = ?1
  AND status IN ('queued', 'running')
"#,
            rusqlite::params![agent_did, now_ms],
        )?;
        Ok(updated)
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

    pub fn store_bootstrap_state(
        &self,
        identity: &UserDelegatedIdentityRecord,
        replay: &BootstrapReplayRecord,
    ) -> Result<BootstrapStoreOutcome> {
        identity.validate()?;
        replay.validate()?;
        if identity.user_did != replay.user_did
            || identity.verification_method != replay.verification_method
            || identity.app_instance_id != replay.app_instance_id
            || identity.daemon_agent_did != replay.daemon_agent_did
            || identity.bootstrap_id != replay.bootstrap_id
            || identity.idempotency_key != replay.idempotency_key
            || identity.status != replay.status
        {
            bail!("bootstrap replay and delegated identity records do not match");
        }
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(existing) = load_bootstrap_replay_by_id_or_key(
            &transaction,
            &replay.bootstrap_id,
            &replay.idempotency_key,
        )? {
            if existing.payload_hash != replay.payload_hash {
                bail!("daemon bootstrap replay conflict");
            }
            if existing.bootstrap_id != replay.bootstrap_id
                || existing.idempotency_key != replay.idempotency_key
                || existing.user_did != replay.user_did
                || existing.verification_method != replay.verification_method
                || existing.app_instance_id != replay.app_instance_id
                || existing.daemon_agent_did != replay.daemon_agent_did
            {
                bail!("daemon bootstrap replay identity conflict");
            }
            return Ok(BootstrapStoreOutcome::Duplicate);
        }
        let now = current_time_millis()?;
        transaction.execute(
            r#"
INSERT INTO bootstrap_replay (
    bootstrap_id,
    idempotency_key,
    payload_hash,
    user_did,
    verification_method,
    app_instance_id,
    daemon_agent_did,
    status,
    created_at_ms,
    updated_at_ms
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9)
"#,
            rusqlite::params![
                &replay.bootstrap_id,
                &replay.idempotency_key,
                &replay.payload_hash,
                &replay.user_did,
                &replay.verification_method,
                &replay.app_instance_id,
                &replay.daemon_agent_did,
                &replay.status,
                now,
            ],
        )?;
        transaction.execute(
            r#"
INSERT INTO user_delegated_identity (
    user_did,
    verification_method,
    app_instance_id,
    controller_did,
    daemon_agent_did,
    public_key_multibase,
    private_key_material,
    allowed_scopes_json,
    status,
    expires_at,
    bootstrap_id,
    idempotency_key,
    created_at_ms,
    updated_at_ms
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?13)
ON CONFLICT(verification_method) DO UPDATE SET
    user_did = excluded.user_did,
    app_instance_id = excluded.app_instance_id,
    controller_did = excluded.controller_did,
    daemon_agent_did = excluded.daemon_agent_did,
    public_key_multibase = excluded.public_key_multibase,
    private_key_material = excluded.private_key_material,
    allowed_scopes_json = excluded.allowed_scopes_json,
    status = excluded.status,
    expires_at = excluded.expires_at,
    bootstrap_id = excluded.bootstrap_id,
    idempotency_key = excluded.idempotency_key,
    updated_at_ms = excluded.updated_at_ms
"#,
            rusqlite::params![
                &identity.user_did,
                &identity.verification_method,
                &identity.app_instance_id,
                &identity.controller_did,
                &identity.daemon_agent_did,
                &identity.public_key_multibase,
                &identity.private_key_material,
                identity.allowed_scopes_json.to_string(),
                &identity.status,
                &identity.expires_at,
                &identity.bootstrap_id,
                &identity.idempotency_key,
                now,
            ],
        )?;
        transaction.commit()?;
        Ok(BootstrapStoreOutcome::Inserted)
    }

    pub fn load_user_delegated_identity(
        &self,
        verification_method: &str,
    ) -> Result<Option<UserDelegatedIdentityRecord>> {
        let connection = self.connection()?;
        connection
            .query_row(
                r#"
SELECT
    user_did,
    verification_method,
    app_instance_id,
    controller_did,
    daemon_agent_did,
    public_key_multibase,
    private_key_material,
    allowed_scopes_json,
    status,
    expires_at,
    bootstrap_id,
    idempotency_key,
    created_at_ms,
    updated_at_ms
FROM user_delegated_identity
WHERE verification_method = ?1
"#,
                [verification_method],
                user_delegated_identity_from_row,
            )
            .optional()
            .context("load user delegated identity")
    }

    pub fn load_bootstrap_replay(
        &self,
        bootstrap_id: &str,
    ) -> Result<Option<BootstrapReplayRecord>> {
        let connection = self.connection()?;
        connection
            .query_row(
                r#"
SELECT
    bootstrap_id,
    idempotency_key,
    payload_hash,
    user_did,
    verification_method,
    app_instance_id,
    daemon_agent_did,
    status,
    created_at_ms,
    updated_at_ms
FROM bootstrap_replay
WHERE bootstrap_id = ?1
"#,
                [bootstrap_id],
                bootstrap_replay_from_row,
            )
            .optional()
            .context("load bootstrap replay")
    }

    pub fn upsert_app_message_agent_binding(
        &self,
        record: &AppMessageAgentBindingRecord,
    ) -> Result<()> {
        record.validate()?;
        let connection = self.connection()?;
        let now = current_time_millis()?;
        connection.execute(
            r#"
INSERT INTO app_message_agent_binding (
    binding_id,
    user_did,
    inbox_auth_verification_method,
    app_instance_id,
    bootstrap_id,
    idempotency_key,
    daemon_agent_did,
    runtime_agent_did,
    runtime_profile_id,
    role,
    desired_agent_json,
    capability_policy_json,
    status,
    created_at_ms,
    updated_at_ms,
    revoked_at_ms
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
ON CONFLICT(binding_id) DO UPDATE SET
    user_did = excluded.user_did,
    inbox_auth_verification_method = excluded.inbox_auth_verification_method,
    app_instance_id = excluded.app_instance_id,
    bootstrap_id = excluded.bootstrap_id,
    idempotency_key = excluded.idempotency_key,
    daemon_agent_did = excluded.daemon_agent_did,
    runtime_agent_did = excluded.runtime_agent_did,
    runtime_profile_id = excluded.runtime_profile_id,
    role = excluded.role,
    desired_agent_json = excluded.desired_agent_json,
    capability_policy_json = excluded.capability_policy_json,
    status = excluded.status,
    updated_at_ms = excluded.updated_at_ms,
    revoked_at_ms = excluded.revoked_at_ms
"#,
            rusqlite::params![
                &record.binding_id,
                &record.user_did,
                &record.inbox_auth_verification_method,
                &record.app_instance_id,
                &record.bootstrap_id,
                &record.idempotency_key,
                &record.daemon_agent_did,
                &record.runtime_agent_did,
                &record.runtime_profile_id,
                &record.role,
                record.desired_agent_json.to_string(),
                record.capability_policy_json.to_string(),
                &record.status,
                if record.created_at_ms > 0 {
                    record.created_at_ms
                } else {
                    now
                },
                now,
                record.revoked_at_ms,
            ],
        )?;
        Ok(())
    }

    pub fn revoke_other_active_app_message_agent_bindings(
        &self,
        user_did: &str,
        role: &str,
        keep_binding_id: &str,
    ) -> Result<usize> {
        let connection = self.connection()?;
        let now = current_time_millis()?;
        let affected = connection.execute(
            r#"
UPDATE app_message_agent_binding
SET revoked_at_ms = ?1,
    updated_at_ms = ?1
WHERE user_did = ?2
  AND role = ?3
  AND binding_id <> ?4
  AND revoked_at_ms IS NULL
  AND status IN ('message_agent_ready', 'message_agent_active', 'message_agent_ensuring')
"#,
            rusqlite::params![now, user_did, role, keep_binding_id],
        )?;
        Ok(affected)
    }

    pub fn load_active_app_message_agent_binding(
        &self,
        user_did: &str,
        app_instance_id: &str,
        role: &str,
    ) -> Result<Option<AppMessageAgentBindingRecord>> {
        let connection = self.connection()?;
        connection
            .query_row(
                r#"
SELECT
    binding_id,
    user_did,
    inbox_auth_verification_method,
    app_instance_id,
    bootstrap_id,
    idempotency_key,
    daemon_agent_did,
    runtime_agent_did,
    runtime_profile_id,
    role,
    desired_agent_json,
    capability_policy_json,
    status,
    created_at_ms,
    updated_at_ms,
    revoked_at_ms
FROM app_message_agent_binding
WHERE user_did = ?1
  AND app_instance_id = ?2
  AND role = ?3
  AND revoked_at_ms IS NULL
  AND status IN ('message_agent_ready', 'message_agent_active', 'message_agent_ensuring')
ORDER BY updated_at_ms DESC
LIMIT 1
"#,
                rusqlite::params![user_did, app_instance_id, role],
                app_message_agent_binding_from_row,
            )
            .optional()
            .context("load active app message agent binding")
    }

    pub fn load_app_message_agent_binding(
        &self,
        binding_id: &str,
    ) -> Result<Option<AppMessageAgentBindingRecord>> {
        let connection = self.connection()?;
        connection
            .query_row(
                r#"
SELECT
    binding_id,
    user_did,
    inbox_auth_verification_method,
    app_instance_id,
    bootstrap_id,
    idempotency_key,
    daemon_agent_did,
    runtime_agent_did,
    runtime_profile_id,
    role,
    desired_agent_json,
    capability_policy_json,
    status,
    created_at_ms,
    updated_at_ms,
    revoked_at_ms
FROM app_message_agent_binding
WHERE binding_id = ?1
"#,
                [binding_id],
                app_message_agent_binding_from_row,
            )
            .optional()
            .context("load app message agent binding")
    }

    pub fn load_active_app_message_agent_binding_by_runtime(
        &self,
        runtime_agent_did: &str,
    ) -> Result<Option<AppMessageAgentBindingRecord>> {
        let connection = self.connection()?;
        connection
            .query_row(
                r#"
SELECT
    binding_id,
    user_did,
    inbox_auth_verification_method,
    app_instance_id,
    bootstrap_id,
    idempotency_key,
    daemon_agent_did,
    runtime_agent_did,
    runtime_profile_id,
    role,
    desired_agent_json,
    capability_policy_json,
    status,
    created_at_ms,
    updated_at_ms,
    revoked_at_ms
FROM app_message_agent_binding
WHERE runtime_agent_did = ?1
  AND revoked_at_ms IS NULL
  AND status IN ('message_agent_ready', 'message_agent_active', 'message_agent_ensuring')
ORDER BY updated_at_ms DESC
LIMIT 1
"#,
                [runtime_agent_did],
                app_message_agent_binding_from_row,
            )
            .optional()
            .context("load active app message agent binding by runtime")
    }

    pub fn list_active_app_message_agent_bindings(
        &self,
    ) -> Result<Vec<AppMessageAgentBindingRecord>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            r#"
SELECT
    binding_id,
    user_did,
    inbox_auth_verification_method,
    app_instance_id,
    bootstrap_id,
    idempotency_key,
    daemon_agent_did,
    runtime_agent_did,
    runtime_profile_id,
    role,
    desired_agent_json,
    capability_policy_json,
    status,
    created_at_ms,
    updated_at_ms,
    revoked_at_ms
FROM app_message_agent_binding
WHERE revoked_at_ms IS NULL
  AND status IN ('message_agent_ready', 'message_agent_active', 'message_agent_ensuring')
ORDER BY updated_at_ms ASC
"#,
        )?;
        let rows = statement.query_map([], app_message_agent_binding_from_row)?;
        let mut bindings = Vec::new();
        for row in rows {
            bindings.push(row?);
        }
        Ok(bindings)
    }

    pub fn load_inbox_cursor(
        &self,
        owner_did: &str,
        inbox_scope: &str,
    ) -> Result<Option<InboxCursorRecord>> {
        let connection = self.connection()?;
        connection
            .query_row(
                r#"
SELECT owner_did, inbox_scope, cursor, updated_at_ms
FROM inbox_cursor
WHERE owner_did = ?1 AND inbox_scope = ?2
"#,
                rusqlite::params![owner_did, inbox_scope],
                inbox_cursor_from_row,
            )
            .optional()
            .context("load inbox cursor")
    }

    pub fn upsert_inbox_cursor(&self, record: &InboxCursorRecord) -> Result<()> {
        record.validate()?;
        let connection = self.connection()?;
        let now = current_time_millis()?;
        connection.execute(
            r#"
INSERT INTO inbox_cursor (
    owner_did,
    inbox_scope,
    cursor,
    updated_at_ms
) VALUES (?1, ?2, ?3, ?4)
ON CONFLICT(owner_did, inbox_scope) DO UPDATE SET
    cursor = excluded.cursor,
    updated_at_ms = excluded.updated_at_ms
"#,
            rusqlite::params![
                &record.owner_did,
                &record.inbox_scope,
                &record.cursor,
                if record.updated_at_ms > 0 {
                    record.updated_at_ms
                } else {
                    now
                },
            ],
        )?;
        Ok(())
    }

    pub fn load_processed_message(
        &self,
        owner_did: &str,
        message_id: &str,
    ) -> Result<Option<ProcessedMessageRecord>> {
        let connection = self.connection()?;
        connection
            .query_row(
                r#"
SELECT owner_did, message_id, schema, processed_at_ms, status
FROM processed_message
WHERE owner_did = ?1 AND message_id = ?2
"#,
                rusqlite::params![owner_did, message_id],
                processed_message_from_row,
            )
            .optional()
            .context("load processed message")
    }

    pub fn try_insert_processed_message(&self, record: &ProcessedMessageRecord) -> Result<bool> {
        record.validate()?;
        let connection = self.connection()?;
        let now = current_time_millis()?;
        let inserted = connection.execute(
            r#"
INSERT OR IGNORE INTO processed_message (
    owner_did,
    message_id,
    schema,
    processed_at_ms,
    status
) VALUES (?1, ?2, ?3, ?4, ?5)
"#,
            rusqlite::params![
                &record.owner_did,
                &record.message_id,
                &record.schema,
                if record.processed_at_ms > 0 {
                    record.processed_at_ms
                } else {
                    now
                },
                &record.status,
            ],
        )?;
        Ok(inserted > 0)
    }

    pub fn mark_processed_message_status(
        &self,
        owner_did: &str,
        message_id: &str,
        status: &str,
    ) -> Result<()> {
        if status.trim().is_empty() {
            bail!("processed message status must not be empty");
        }
        let connection = self.connection()?;
        connection.execute(
            r#"
UPDATE processed_message
SET status = ?3,
    processed_at_ms = ?4
WHERE owner_did = ?1 AND message_id = ?2
"#,
            rusqlite::params![owner_did, message_id, status, current_time_millis()?],
        )?;
        Ok(())
    }

    pub fn upsert_message_event(&self, record: &MessageEventRecord) -> Result<()> {
        record.validate()?;
        let connection = self.connection()?;
        let now = current_time_millis()?;
        connection.execute(
            r#"
INSERT INTO message_event (
    event_id,
    owner_did,
    conversation_id,
    message_id,
    message_kind,
    sender_did,
    received_at,
    plain_text_ref_or_excerpt,
    content_hash,
    schema,
    processing_status,
    retention_class,
    created_at_ms,
    updated_at_ms
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
ON CONFLICT(event_id) DO UPDATE SET
    conversation_id = excluded.conversation_id,
    message_kind = excluded.message_kind,
    sender_did = excluded.sender_did,
    received_at = excluded.received_at,
    plain_text_ref_or_excerpt = excluded.plain_text_ref_or_excerpt,
    content_hash = excluded.content_hash,
    schema = excluded.schema,
    processing_status = excluded.processing_status,
    retention_class = excluded.retention_class,
    updated_at_ms = excluded.updated_at_ms
"#,
            rusqlite::params![
                &record.event_id,
                &record.owner_did,
                &record.conversation_id,
                &record.message_id,
                &record.message_kind,
                &record.sender_did,
                &record.received_at,
                &record.plain_text_ref_or_excerpt,
                &record.content_hash,
                &record.schema,
                &record.processing_status,
                &record.retention_class,
                if record.created_at_ms > 0 {
                    record.created_at_ms
                } else {
                    now
                },
                if record.updated_at_ms > 0 {
                    record.updated_at_ms
                } else {
                    now
                },
            ],
        )?;
        Ok(())
    }

    pub fn load_message_event(&self, event_id: &str) -> Result<Option<MessageEventRecord>> {
        let connection = self.connection()?;
        connection
            .query_row(
                r#"
SELECT
    event_id,
    owner_did,
    conversation_id,
    message_id,
    message_kind,
    sender_did,
    received_at,
    plain_text_ref_or_excerpt,
    content_hash,
    schema,
    processing_status,
    retention_class,
    created_at_ms,
    updated_at_ms
FROM message_event
WHERE event_id = ?1
"#,
                [event_id],
                message_event_from_row,
            )
            .optional()
            .context("load message event")
    }

    pub fn upsert_message_sync_outbox(&self, record: &MessageSyncOutboxRecord) -> Result<()> {
        record.validate()?;
        let connection = self.connection()?;
        let now = current_time_millis()?;
        connection.execute(
            r#"
INSERT INTO message_sync_outbox (
    idempotency_key,
    owner_did,
    app_instance_id,
    payload_json,
    status,
    attempt_count,
    next_attempt_at_ms,
    last_error_code,
    last_error_summary,
    created_at_ms,
    updated_at_ms,
    sent_at_ms
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
ON CONFLICT(idempotency_key) DO UPDATE SET
    payload_json = CASE WHEN message_sync_outbox.status = 'sent' THEN message_sync_outbox.payload_json ELSE excluded.payload_json END,
    status = CASE WHEN message_sync_outbox.status = 'sent' THEN message_sync_outbox.status ELSE excluded.status END,
    next_attempt_at_ms = CASE WHEN message_sync_outbox.status = 'sent' THEN message_sync_outbox.next_attempt_at_ms ELSE excluded.next_attempt_at_ms END,
    last_error_code = CASE WHEN message_sync_outbox.status = 'sent' THEN message_sync_outbox.last_error_code ELSE excluded.last_error_code END,
    last_error_summary = CASE WHEN message_sync_outbox.status = 'sent' THEN message_sync_outbox.last_error_summary ELSE excluded.last_error_summary END,
    updated_at_ms = excluded.updated_at_ms,
    sent_at_ms = CASE WHEN message_sync_outbox.status = 'sent' THEN message_sync_outbox.sent_at_ms ELSE excluded.sent_at_ms END
"#,
            rusqlite::params![
                &record.idempotency_key,
                &record.owner_did,
                &record.app_instance_id,
                record.payload_json.to_string(),
                &record.status,
                record.attempt_count,
                record.next_attempt_at_ms,
                &record.last_error_code,
                &record.last_error_summary,
                if record.created_at_ms > 0 {
                    record.created_at_ms
                } else {
                    now
                },
                if record.updated_at_ms > 0 {
                    record.updated_at_ms
                } else {
                    now
                },
                record.sent_at_ms,
            ],
        )?;
        Ok(())
    }

    pub fn load_message_sync_outbox(
        &self,
        idempotency_key: &str,
    ) -> Result<Option<MessageSyncOutboxRecord>> {
        let connection = self.connection()?;
        connection
            .query_row(
                r#"
SELECT
    idempotency_key,
    owner_did,
    app_instance_id,
    payload_json,
    status,
    attempt_count,
    next_attempt_at_ms,
    last_error_code,
    last_error_summary,
    created_at_ms,
    updated_at_ms,
    sent_at_ms
FROM message_sync_outbox
WHERE idempotency_key = ?1
"#,
                [idempotency_key],
                message_sync_outbox_from_row,
            )
            .optional()
            .context("load message sync outbox")
    }

    pub fn list_due_message_sync_outbox(
        &self,
        now_ms: i64,
        limit: usize,
    ) -> Result<Vec<MessageSyncOutboxRecord>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            r#"
SELECT
    idempotency_key,
    owner_did,
    app_instance_id,
    payload_json,
    status,
    attempt_count,
    next_attempt_at_ms,
    last_error_code,
    last_error_summary,
    created_at_ms,
    updated_at_ms,
    sent_at_ms
FROM message_sync_outbox
WHERE status = 'pending'
  AND next_attempt_at_ms <= ?1
ORDER BY created_at_ms ASC, idempotency_key ASC
LIMIT ?2
"#,
        )?;
        let rows = statement.query_map(
            rusqlite::params![now_ms, limit.max(1) as i64],
            message_sync_outbox_from_row,
        )?;
        let mut records = Vec::new();
        for row in rows {
            records.push(row?);
        }
        Ok(records)
    }

    pub fn mark_message_sync_outbox_sending(&self, idempotency_key: &str) -> Result<bool> {
        let connection = self.connection()?;
        let updated = connection.execute(
            r#"
UPDATE message_sync_outbox
SET status = 'sending',
    attempt_count = attempt_count + 1,
    updated_at_ms = ?1
WHERE idempotency_key = ?2
  AND status = 'pending'
"#,
            rusqlite::params![current_time_millis()?, idempotency_key],
        )?;
        Ok(updated > 0)
    }

    pub fn mark_message_sync_outbox_sent(&self, idempotency_key: &str) -> Result<()> {
        let now = current_time_millis()?;
        let connection = self.connection()?;
        let updated = connection.execute(
            r#"
UPDATE message_sync_outbox
SET status = 'sent',
    sent_at_ms = ?1,
    updated_at_ms = ?1,
    last_error_code = NULL,
    last_error_summary = NULL
WHERE idempotency_key = ?2
"#,
            rusqlite::params![now, idempotency_key],
        )?;
        if updated == 0 {
            bail!("message sync outbox does not exist: {idempotency_key}");
        }
        Ok(())
    }

    pub fn mark_message_sync_outbox_retry(
        &self,
        idempotency_key: &str,
        next_attempt_at_ms: i64,
        error_code: &str,
        error_summary: &str,
    ) -> Result<()> {
        let connection = self.connection()?;
        let updated = connection.execute(
            r#"
UPDATE message_sync_outbox
SET status = 'pending',
    next_attempt_at_ms = ?1,
    last_error_code = ?2,
    last_error_summary = ?3,
    updated_at_ms = ?4
WHERE idempotency_key = ?5
  AND status = 'sending'
"#,
            rusqlite::params![
                next_attempt_at_ms,
                error_code,
                error_summary,
                current_time_millis()?,
                idempotency_key,
            ],
        )?;
        if updated == 0 {
            bail!("message sync outbox is not sending: {idempotency_key}");
        }
        Ok(())
    }

    pub fn recover_stale_message_sync_outbox_sending(
        &self,
        stale_before_ms: i64,
        next_attempt_at_ms: i64,
    ) -> Result<usize> {
        let connection = self.connection()?;
        let updated = connection.execute(
            r#"
UPDATE message_sync_outbox
SET status = 'pending',
    next_attempt_at_ms = ?1,
    last_error_code = COALESCE(last_error_code, 'message_sync_delivery_recovered'),
    last_error_summary = COALESCE(last_error_summary, 'Recovered stale message sync delivery attempt'),
    updated_at_ms = ?2
WHERE status = 'sending'
  AND updated_at_ms <= ?3
"#,
            rusqlite::params![next_attempt_at_ms, current_time_millis()?, stale_before_ms],
        )?;
        Ok(updated)
    }

    pub fn mark_message_sync_outbox_failed_terminal(
        &self,
        idempotency_key: &str,
        error_code: &str,
        error_summary: &str,
    ) -> Result<()> {
        let connection = self.connection()?;
        let updated = connection.execute(
            r#"
UPDATE message_sync_outbox
SET status = 'failed_terminal',
    last_error_code = ?1,
    last_error_summary = ?2,
    updated_at_ms = ?3
WHERE idempotency_key = ?4
"#,
            rusqlite::params![
                error_code,
                error_summary,
                current_time_millis()?,
                idempotency_key,
            ],
        )?;
        if updated == 0 {
            bail!("message sync outbox does not exist: {idempotency_key}");
        }
        Ok(())
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
    controller_scope_key,
    controller_did,
    conversation_id,
    route_key,
    hermes_profile,
    hermes_session_id,
    session_kind,
    status,
    created_at_ms,
    updated_at_ms
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
ON CONFLICT(id) DO UPDATE SET
    runtime_session_id = excluded.runtime_session_id,
    agent_did = excluded.agent_did,
    runtime_profile_id = excluded.runtime_profile_id,
    controller_scope_key = excluded.controller_scope_key,
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
                    session.controller_scope_key,
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
    controller_scope_key,
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

    pub fn reset_active_hermes_sessions_for_runtime_controller_scope(
        &self,
        agent_did: &str,
        runtime_profile_id: &str,
        controller_scope_key: &str,
    ) -> Result<usize> {
        if agent_did.trim().is_empty() {
            bail!("agent_did must not be empty");
        }
        if runtime_profile_id.trim().is_empty() {
            bail!("runtime_profile_id must not be empty");
        }
        if controller_scope_key.trim().is_empty() {
            bail!("controller_scope_key must not be empty");
        }
        let connection = self.connection()?;
        let updated = connection.execute(
            r#"
UPDATE hermes_native_sessions
SET status = 'reset',
    updated_at_ms = ?1
WHERE agent_did = ?2
  AND runtime_profile_id = ?3
  AND controller_scope_key = ?4
  AND status = 'active'
"#,
            rusqlite::params![
                current_time_millis()?,
                agent_did,
                runtime_profile_id,
                controller_scope_key,
            ],
        )?;
        Ok(updated)
    }

    pub fn upsert_runtime_daemon_binding(
        &self,
        runtime_agent_did: &str,
        daemon_agent_did: &str,
        controller_user_id: &str,
        controller_full_handle: &str,
        controller_scope_key: &str,
        controller_did: &str,
    ) -> Result<()> {
        let record = RuntimeDaemonBindingRecord {
            runtime_agent_did: runtime_agent_did.to_string(),
            daemon_agent_did: daemon_agent_did.to_string(),
            controller_user_id: controller_user_id.to_string(),
            controller_full_handle: controller_full_handle.to_string(),
            controller_scope_key: controller_scope_key.to_string(),
            controller_did: controller_did.to_string(),
            created_at_ms: current_time_millis()?,
            updated_at_ms: current_time_millis()?,
        };
        record.validate()?;
        let connection = self.connection()?;
        connection.execute(
            r#"
INSERT INTO runtime_daemon_binding (
    runtime_agent_did,
    daemon_agent_did,
    controller_user_id,
    controller_full_handle,
    controller_scope_key,
    controller_did,
    created_at_ms,
    updated_at_ms
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
ON CONFLICT(runtime_agent_did) DO UPDATE SET
    daemon_agent_did = excluded.daemon_agent_did,
    controller_user_id = excluded.controller_user_id,
    controller_full_handle = excluded.controller_full_handle,
    controller_scope_key = excluded.controller_scope_key,
    controller_did = excluded.controller_did,
    updated_at_ms = excluded.updated_at_ms
"#,
            rusqlite::params![
                record.runtime_agent_did,
                record.daemon_agent_did,
                record.controller_user_id,
                record.controller_full_handle,
                record.controller_scope_key,
                record.controller_did,
                record.created_at_ms,
                record.updated_at_ms,
            ],
        )?;
        Ok(())
    }

    pub fn load_runtime_daemon_binding(
        &self,
        runtime_agent_did: &str,
    ) -> Result<Option<RuntimeDaemonBindingRecord>> {
        if runtime_agent_did.trim().is_empty() {
            bail!("runtime_agent_did must not be empty");
        }
        let connection = self.connection()?;
        connection
            .query_row(
                r#"
SELECT
    runtime_agent_did,
    daemon_agent_did,
    controller_user_id,
    controller_full_handle,
    controller_scope_key,
    controller_did,
    created_at_ms,
    updated_at_ms
FROM runtime_daemon_binding
WHERE runtime_agent_did = ?1
"#,
                [runtime_agent_did],
                |row| {
                    Ok(RuntimeDaemonBindingRecord {
                        runtime_agent_did: row.get(0)?,
                        daemon_agent_did: row.get(1)?,
                        controller_user_id: row.get(2)?,
                        controller_full_handle: row.get(3)?,
                        controller_scope_key: row.get(4)?,
                        controller_did: row.get(5)?,
                        created_at_ms: row.get(6)?,
                        updated_at_ms: row.get(7)?,
                    })
                },
            )
            .optional()
            .context("load runtime daemon binding")
    }

    pub fn runtime_agent_belongs_to_daemon_scope(
        &self,
        runtime_agent_did: &str,
        daemon_agent_did: &str,
        controller_scope_key: &str,
    ) -> Result<bool> {
        let Some(binding) = self.load_runtime_daemon_binding(runtime_agent_did)? else {
            return Ok(false);
        };
        Ok(binding.daemon_agent_did == daemon_agent_did
            && binding.controller_scope_key == controller_scope_key)
    }

    pub fn store_runtime_agent_create_request(
        &self,
        record: &RuntimeAgentCreateRequestRecord,
    ) -> Result<()> {
        record.validate()?;
        let connection = self.connection()?;
        connection.execute(
            r#"
INSERT INTO runtime_agent_create_request (
    daemon_agent_did,
    controller_scope_key,
    controller_did,
    client_request_id,
    runtime_agent_did,
    command_id,
    outcome_json,
    created_at_ms,
    updated_at_ms
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
ON CONFLICT(daemon_agent_did, controller_scope_key, client_request_id) DO UPDATE SET
    updated_at_ms = excluded.updated_at_ms
"#,
            rusqlite::params![
                record.daemon_agent_did,
                record.controller_scope_key,
                record.controller_did,
                record.client_request_id,
                record.runtime_agent_did,
                record.command_id,
                record.outcome_json.to_string(),
                record.created_at_ms,
                record.updated_at_ms,
            ],
        )?;
        Ok(())
    }

    pub fn load_runtime_agent_create_request(
        &self,
        daemon_agent_did: &str,
        controller_scope_key: &str,
        client_request_id: &str,
    ) -> Result<Option<RuntimeAgentCreateRequestRecord>> {
        if daemon_agent_did.trim().is_empty() {
            bail!("daemon_agent_did must not be empty");
        }
        if controller_scope_key.trim().is_empty() {
            bail!("controller_scope_key must not be empty");
        }
        if client_request_id.trim().is_empty() {
            bail!("client_request_id must not be empty");
        }
        let connection = self.connection()?;
        connection
            .query_row(
                r#"
SELECT
    daemon_agent_did,
    controller_scope_key,
    controller_did,
    client_request_id,
    runtime_agent_did,
    command_id,
    outcome_json,
    created_at_ms,
    updated_at_ms
FROM runtime_agent_create_request
WHERE daemon_agent_did = ?1
  AND controller_scope_key = ?2
  AND client_request_id = ?3
"#,
                rusqlite::params![daemon_agent_did, controller_scope_key, client_request_id],
                runtime_agent_create_request_record_from_row,
            )
            .optional()
            .context("load runtime agent create request")
    }

    pub fn try_begin_control_command(
        &self,
        daemon_agent_did: &str,
        controller_scope_key: &str,
        command_id: &str,
        command: &str,
        message_id: &str,
        target_version: Option<&str>,
    ) -> Result<Option<ControlCommandStateRecord>> {
        for (field_name, value) in [
            ("daemon_agent_did", daemon_agent_did),
            ("controller_scope_key", controller_scope_key),
            ("command_id", command_id),
            ("command", command),
            ("message_id", message_id),
        ] {
            if value.trim().is_empty() {
                bail!("{field_name} must not be empty");
            }
        }
        let now = current_time_millis()?;
        let connection = self.connection()?;
        let inserted = connection.execute(
            r#"
INSERT OR IGNORE INTO control_command_state (
    daemon_agent_did,
    controller_scope_key,
    command_id,
    command,
    message_id,
    status,
    target_version,
    result_json,
    error_summary,
    created_at_ms,
    updated_at_ms
) VALUES (?1, ?2, ?3, ?4, ?5, 'in_progress', ?6, '{}', NULL, ?7, ?7)
"#,
            rusqlite::params![
                daemon_agent_did,
                controller_scope_key,
                command_id,
                command,
                message_id,
                target_version,
                now,
            ],
        )?;
        if inserted > 0 {
            Ok(None)
        } else {
            self.load_control_command_state(daemon_agent_did, controller_scope_key, command_id)
        }
    }

    pub fn load_control_command_state(
        &self,
        daemon_agent_did: &str,
        controller_scope_key: &str,
        command_id: &str,
    ) -> Result<Option<ControlCommandStateRecord>> {
        let connection = self.connection()?;
        connection
            .query_row(
                r#"
SELECT
    daemon_agent_did,
    controller_scope_key,
    command_id,
    command,
    message_id,
    status,
    target_version,
    result_json,
    error_summary,
    created_at_ms,
    updated_at_ms
FROM control_command_state
WHERE daemon_agent_did = ?1
  AND controller_scope_key = ?2
  AND command_id = ?3
"#,
                rusqlite::params![daemon_agent_did, controller_scope_key, command_id],
                control_command_state_record_from_row,
            )
            .optional()
            .context("load control command state")
    }

    pub fn mark_control_command_state(
        &self,
        daemon_agent_did: &str,
        controller_scope_key: &str,
        command_id: &str,
        status: &str,
        result_json: Value,
        error_summary: Option<&str>,
    ) -> Result<()> {
        for (field_name, value) in [
            ("daemon_agent_did", daemon_agent_did),
            ("controller_scope_key", controller_scope_key),
            ("command_id", command_id),
        ] {
            if value.trim().is_empty() {
                bail!("{field_name} must not be empty");
            }
        }
        validate_control_command_status(status)?;
        let connection = self.connection()?;
        let updated = connection.execute(
            r#"
UPDATE control_command_state
SET status = ?1,
    result_json = ?2,
    error_summary = ?3,
    updated_at_ms = ?4
WHERE daemon_agent_did = ?5
  AND controller_scope_key = ?6
  AND command_id = ?7
"#,
            rusqlite::params![
                status,
                result_json.to_string(),
                error_summary,
                current_time_millis()?,
                daemon_agent_did,
                controller_scope_key,
                command_id,
            ],
        )?;
        if updated == 0 {
            bail!("control command state does not exist: {command_id}");
        }
        Ok(())
    }

    pub fn list_runtime_agent_definitions_for_daemon(
        &self,
        daemon_agent_did: &str,
    ) -> Result<Vec<AgentDefinition>> {
        if daemon_agent_did.trim().is_empty() {
            bail!("daemon_agent_did must not be empty");
        }
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            r#"
SELECT
    agent_definition.agent_did,
    agent_definition.handle,
    agent_definition.agent_kind,
    agent_definition.controller_user_id,
    agent_definition.controller_full_handle,
    agent_definition.controller_scope_key,
    agent_definition.controller_did,
    agent_definition.runtime_plugin_id,
    agent_definition.runtime_profile_id,
    agent_definition.workspace_id,
    agent_definition.policy_id,
    agent_definition.local_agent_db_path,
    agent_definition.message_db_path,
    agent_definition.status
FROM agent_definition
INNER JOIN runtime_daemon_binding
    ON runtime_daemon_binding.runtime_agent_did = agent_definition.agent_did
WHERE agent_definition.agent_kind = 'runtime'
  AND agent_definition.status = 'active'
  AND runtime_daemon_binding.daemon_agent_did = ?1
ORDER BY agent_definition.updated_at DESC, agent_definition.agent_did ASC
"#,
        )?;
        let rows = statement.query_map([daemon_agent_did], agent_definition_from_row)?;
        let mut definitions = Vec::new();
        for row in rows {
            definitions.push(row?);
        }
        Ok(definitions)
    }

    pub fn should_emit_agent_status_query_snapshot(
        &self,
        daemon_agent_did: &str,
        controller_did: &str,
        min_interval_ms: i64,
    ) -> Result<bool> {
        if daemon_agent_did.trim().is_empty() {
            bail!("daemon_agent_did must not be empty");
        }
        if controller_did.trim().is_empty() {
            bail!("controller_did must not be empty");
        }
        if min_interval_ms < 0 {
            bail!("min_interval_ms must not be negative");
        }
        let connection = self.connection()?;
        let last_snapshot_at_ms = connection
            .query_row(
                r#"
SELECT last_snapshot_at_ms
FROM agent_status_query_throttle
WHERE daemon_agent_did = ?1
  AND controller_did = ?2
"#,
                rusqlite::params![daemon_agent_did, controller_did],
                |row| row.get::<_, i64>(0),
            )
            .optional()?;
        let now = current_time_millis()?;
        if last_snapshot_at_ms.is_some_and(|last| now.saturating_sub(last) < min_interval_ms) {
            return Ok(false);
        }
        connection.execute(
            r#"
INSERT INTO agent_status_query_throttle (
    daemon_agent_did,
    controller_did,
    last_snapshot_at_ms
) VALUES (?1, ?2, ?3)
ON CONFLICT(daemon_agent_did, controller_did) DO UPDATE SET
    last_snapshot_at_ms = excluded.last_snapshot_at_ms
"#,
            rusqlite::params![daemon_agent_did, controller_did, now],
        )?;
        Ok(true)
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
    controller_user_id,
    controller_full_handle,
    controller_scope_key,
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
    controller_user_id,
    controller_full_handle,
    controller_scope_key,
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
                        controller_user_id: definition.controller_user_id.clone(),
                        controller_full_handle: definition.controller_full_handle.clone(),
                        controller_scope_key: definition.controller_scope_key.clone(),
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
    controller_user_id,
    controller_full_handle,
    controller_scope_key,
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
  AND status = 'active'
ORDER BY updated_at DESC, agent_did ASC
"#
            }
            None => {
                r#"
SELECT
    agent_did,
    handle,
    agent_kind,
    controller_user_id,
    controller_full_handle,
    controller_scope_key,
    controller_did,
    runtime_plugin_id,
    runtime_profile_id,
    workspace_id,
    policy_id,
    local_agent_db_path,
    message_db_path,
    status
FROM agent_definition
WHERE status = 'active'
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
    controller_user_id,
    controller_full_handle,
    controller_scope_key,
    controller_did,
    sender_did,
    conversation_id,
    task_text,
    status,
    created_at_ms,
    updated_at_ms
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'created', ?10, ?10)
ON CONFLICT(task_id) DO UPDATE SET
    task_text = excluded.task_text,
    updated_at_ms = excluded.updated_at_ms
"#,
            rusqlite::params![
                task.task_id,
                task.agent_did,
                task.controller_user_id,
                task.controller_full_handle,
                task.controller_scope_key,
                task.controller_did,
                task.sender_did,
                task.conversation_id,
                task.text,
                now,
            ],
        )?;
        Ok(())
    }

    pub fn try_insert_runtime_run(&self, run: &RuntimeRun) -> Result<bool> {
        let connection = self.connection()?;
        let now = current_time_millis()?;
        let inserted = connection.execute(
            r#"
INSERT OR IGNORE INTO runtime_run (
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
        Ok(inserted > 0)
    }

    pub fn insert_runtime_run(&self, run: &RuntimeRun) -> Result<()> {
        if !self.try_insert_runtime_run(run)? {
            bail!("runtime run already exists: {}", run.run_id);
        }
        Ok(())
    }

    pub fn insert_runtime_retry_request(
        &self,
        original_run: &RuntimeRun,
        command_id: &str,
    ) -> Result<RuntimeRetryQueueRecord> {
        if original_run.status != RuntimeRunStatus::Failed {
            bail!("only failed runs can be retried");
        }
        let command_id = command_id.trim();
        if command_id.is_empty() {
            bail!("command_id must not be empty");
        }
        let now = current_time_millis()?;
        let retry_id = format!("retry_{}_{}", now, stable_id_suffix(&original_run.run_id));
        let record = RuntimeRetryQueueRecord {
            retry_id,
            original_run_id: original_run.run_id.clone(),
            task_id: original_run.task_id.clone(),
            agent_did: original_run.agent_did.clone(),
            runtime_profile_id: original_run.runtime_profile_id.clone(),
            runtime_plugin_id: original_run.runtime_plugin_id.clone(),
            workspace_id: original_run.workspace_id.clone(),
            status: "queued".to_string(),
            requested_by_command_id: command_id.to_string(),
            attempts: 0,
            created_at_ms: now,
            updated_at_ms: now,
        };
        record.validate()?;
        let connection = self.connection()?;
        connection.execute(
            r#"
INSERT INTO runtime_retry_queue (
    retry_id,
    original_run_id,
    task_id,
    agent_did,
    runtime_profile_id,
    runtime_plugin_id,
    workspace_id,
    status,
    requested_by_command_id,
    attempts,
    created_at_ms,
    updated_at_ms
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'queued', ?8, 0, ?9, ?9)
"#,
            rusqlite::params![
                record.retry_id,
                record.original_run_id,
                record.task_id,
                record.agent_did,
                record.runtime_profile_id,
                record.runtime_plugin_id,
                record.workspace_id,
                record.requested_by_command_id,
                now,
            ],
        )?;
        Ok(record)
    }

    pub fn load_runtime_retry_request(&self, retry_id: &str) -> Result<RuntimeRetryQueueRecord> {
        let connection = self.connection()?;
        connection
            .query_row(
                r#"
SELECT
    retry_id,
    original_run_id,
    task_id,
    agent_did,
    runtime_profile_id,
    runtime_plugin_id,
    workspace_id,
    status,
    requested_by_command_id,
    attempts,
    created_at_ms,
    updated_at_ms
FROM runtime_retry_queue
WHERE retry_id = ?1
"#,
                [retry_id],
                runtime_retry_queue_record_from_row,
            )
            .context("load runtime retry request")
    }

    pub fn list_queued_runtime_retries(
        &self,
        limit: usize,
    ) -> Result<Vec<RuntimeRetryQueueRecord>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            r#"
SELECT
    retry_id,
    original_run_id,
    task_id,
    agent_did,
    runtime_profile_id,
    runtime_plugin_id,
    workspace_id,
    status,
    requested_by_command_id,
    attempts,
    created_at_ms,
    updated_at_ms
FROM runtime_retry_queue
WHERE status = 'queued'
ORDER BY created_at_ms ASC, retry_id ASC
LIMIT ?1
"#,
        )?;
        let rows =
            statement.query_map([limit.max(1) as i64], runtime_retry_queue_record_from_row)?;
        let mut retries = Vec::new();
        for row in rows {
            retries.push(row?);
        }
        Ok(retries)
    }

    pub fn mark_runtime_retry_status(&self, retry_id: &str, status: &str) -> Result<()> {
        if retry_id.trim().is_empty() {
            bail!("retry_id must not be empty");
        }
        if status.trim().is_empty() {
            bail!("retry status must not be empty");
        }
        let connection = self.connection()?;
        let updated = connection.execute(
            r#"
UPDATE runtime_retry_queue
SET status = ?1,
    attempts = attempts + CASE WHEN ?1 = 'running' THEN 1 ELSE 0 END,
    updated_at_ms = ?2
WHERE retry_id = ?3
"#,
            rusqlite::params![status, current_time_millis()?, retry_id],
        )?;
        if updated == 0 {
            bail!("runtime retry request does not exist: {retry_id}");
        }
        Ok(())
    }

    pub fn upsert_runtime_final_outbox_pending(
        &self,
        record: &RuntimeFinalOutboxRecord,
    ) -> Result<()> {
        record.validate()?;
        if record.status != "pending" {
            bail!("runtime final outbox upsert requires pending status");
        }
        let now = current_time_millis()?;
        let connection = self.connection()?;
        connection.execute(
            r#"
INSERT INTO runtime_final_outbox (
    idempotency_key,
    run_id,
    agent_did,
    runtime_profile_id,
    controller_scope_key,
    controller_did,
    conversation_id,
    final_text,
    security,
    status,
    attempt_count,
    next_attempt_at_ms,
    last_error_code,
    last_error_summary,
    message_id,
    created_at_ms,
    updated_at_ms,
    sent_at_ms
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'pending', 0, ?10, NULL, NULL, NULL, ?11, ?11, NULL)
ON CONFLICT(idempotency_key) DO UPDATE SET
    final_text = CASE WHEN runtime_final_outbox.status = 'sent' THEN runtime_final_outbox.final_text ELSE excluded.final_text END,
    security = CASE WHEN runtime_final_outbox.status = 'sent' THEN runtime_final_outbox.security ELSE excluded.security END,
    status = CASE WHEN runtime_final_outbox.status = 'sent' THEN runtime_final_outbox.status ELSE 'pending' END,
    next_attempt_at_ms = CASE WHEN runtime_final_outbox.status = 'sent' THEN runtime_final_outbox.next_attempt_at_ms ELSE excluded.next_attempt_at_ms END,
    last_error_code = CASE WHEN runtime_final_outbox.status = 'sent' THEN runtime_final_outbox.last_error_code ELSE NULL END,
    last_error_summary = CASE WHEN runtime_final_outbox.status = 'sent' THEN runtime_final_outbox.last_error_summary ELSE NULL END,
    updated_at_ms = excluded.updated_at_ms
"#,
            rusqlite::params![
                record.idempotency_key,
                record.run_id,
                record.agent_did,
                record.runtime_profile_id,
                record.controller_scope_key,
                record.controller_did,
                record.conversation_id,
                record.final_text,
                record.security,
                record.next_attempt_at_ms,
                now,
            ],
        )?;
        Ok(())
    }

    pub fn load_runtime_final_outbox_by_run(
        &self,
        run_id: &str,
    ) -> Result<Option<RuntimeFinalOutboxRecord>> {
        let connection = self.connection()?;
        connection
            .query_row(
                r#"
SELECT
    idempotency_key,
    run_id,
    agent_did,
    runtime_profile_id,
    controller_scope_key,
    controller_did,
    conversation_id,
    final_text,
    security,
    status,
    attempt_count,
    next_attempt_at_ms,
    last_error_code,
    last_error_summary,
    message_id,
    created_at_ms,
    updated_at_ms,
    sent_at_ms
FROM runtime_final_outbox
WHERE run_id = ?1
"#,
                [run_id],
                runtime_final_outbox_record_from_row,
            )
            .optional()
            .context("load runtime final outbox by run")
    }

    pub fn list_due_runtime_final_outbox(
        &self,
        now_ms: i64,
        limit: usize,
    ) -> Result<Vec<RuntimeFinalOutboxRecord>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            r#"
SELECT
    idempotency_key,
    run_id,
    agent_did,
    runtime_profile_id,
    controller_scope_key,
    controller_did,
    conversation_id,
    final_text,
    security,
    status,
    attempt_count,
    next_attempt_at_ms,
    last_error_code,
    last_error_summary,
    message_id,
    created_at_ms,
    updated_at_ms,
    sent_at_ms
FROM runtime_final_outbox
WHERE status = 'pending'
  AND next_attempt_at_ms <= ?1
ORDER BY created_at_ms ASC, idempotency_key ASC
LIMIT ?2
"#,
        )?;
        let rows = statement.query_map(
            rusqlite::params![now_ms, limit.max(1) as i64],
            runtime_final_outbox_record_from_row,
        )?;
        let mut records = Vec::new();
        for row in rows {
            records.push(row?);
        }
        Ok(records)
    }

    pub fn mark_runtime_final_outbox_sending(&self, idempotency_key: &str) -> Result<bool> {
        let connection = self.connection()?;
        let updated = connection.execute(
            r#"
UPDATE runtime_final_outbox
SET status = 'sending',
    attempt_count = attempt_count + 1,
    updated_at_ms = ?1
WHERE idempotency_key = ?2
  AND status = 'pending'
"#,
            rusqlite::params![current_time_millis()?, idempotency_key],
        )?;
        Ok(updated > 0)
    }

    pub fn mark_runtime_final_outbox_sent(
        &self,
        idempotency_key: &str,
        message_id: Option<&str>,
    ) -> Result<()> {
        let now = current_time_millis()?;
        let connection = self.connection()?;
        let updated = connection.execute(
            r#"
UPDATE runtime_final_outbox
SET status = 'sent',
    message_id = ?1,
    sent_at_ms = ?2,
    updated_at_ms = ?2,
    last_error_code = NULL,
    last_error_summary = NULL
WHERE idempotency_key = ?3
"#,
            rusqlite::params![message_id, now, idempotency_key],
        )?;
        if updated == 0 {
            bail!("runtime final outbox does not exist: {idempotency_key}");
        }
        Ok(())
    }

    pub fn mark_runtime_final_outbox_retry(
        &self,
        idempotency_key: &str,
        next_attempt_at_ms: i64,
        error_code: &str,
        error_summary: &str,
    ) -> Result<()> {
        let connection = self.connection()?;
        let updated = connection.execute(
            r#"
UPDATE runtime_final_outbox
SET status = 'pending',
    next_attempt_at_ms = ?1,
    last_error_code = ?2,
    last_error_summary = ?3,
    updated_at_ms = ?4
WHERE idempotency_key = ?5
  AND status = 'sending'
"#,
            rusqlite::params![
                next_attempt_at_ms,
                error_code,
                error_summary,
                current_time_millis()?,
                idempotency_key,
            ],
        )?;
        if updated == 0 {
            bail!("runtime final outbox is not sending: {idempotency_key}");
        }
        Ok(())
    }

    pub fn recover_stale_runtime_final_outbox_sending(
        &self,
        stale_before_ms: i64,
        next_attempt_at_ms: i64,
    ) -> Result<usize> {
        let connection = self.connection()?;
        let updated = connection.execute(
            r#"
UPDATE runtime_final_outbox
SET status = 'pending',
    next_attempt_at_ms = ?1,
    last_error_code = COALESCE(last_error_code, 'final_delivery_recovered'),
    last_error_summary = COALESCE(last_error_summary, 'Recovered stale final delivery attempt'),
    updated_at_ms = ?2
WHERE status = 'sending'
  AND updated_at_ms <= ?3
"#,
            rusqlite::params![next_attempt_at_ms, current_time_millis()?, stale_before_ms],
        )?;
        Ok(updated)
    }

    pub fn mark_runtime_final_outbox_failed_terminal(
        &self,
        idempotency_key: &str,
        error_code: &str,
        error_summary: &str,
    ) -> Result<()> {
        let connection = self.connection()?;
        let updated = connection.execute(
            r#"
UPDATE runtime_final_outbox
SET status = 'failed_terminal',
    last_error_code = ?1,
    last_error_summary = ?2,
    updated_at_ms = ?3
WHERE idempotency_key = ?4
"#,
            rusqlite::params![
                error_code,
                error_summary,
                current_time_millis()?,
                idempotency_key,
            ],
        )?;
        if updated == 0 {
            bail!("runtime final outbox does not exist: {idempotency_key}");
        }
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
    controller_user_id,
    controller_full_handle,
    controller_scope_key,
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
                        controller_user_id: row.get(2)?,
                        controller_full_handle: row.get(3)?,
                        controller_scope_key: row.get(4)?,
                        controller_did: row.get(5)?,
                        sender_did: row.get(6)?,
                        conversation_id: row.get(7)?,
                        text: row.get(8)?,
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
    controller_user_id,
    controller_full_handle,
    controller_scope_key,
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
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?23)
ON CONFLICT(run_id) DO UPDATE SET
    agent_did = excluded.agent_did,
    runtime_profile_id = excluded.runtime_profile_id,
    driver_id = excluded.driver_id,
    controller_user_id = excluded.controller_user_id,
    controller_full_handle = excluded.controller_full_handle,
    controller_scope_key = excluded.controller_scope_key,
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
                record.controller_user_id,
                record.controller_full_handle,
                record.controller_scope_key,
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
    controller_user_id,
    controller_full_handle,
    controller_scope_key,
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

    pub fn audit_event_exists(
        &self,
        event_type: &str,
        agent_did: Option<&str>,
        detail_contains: Option<&str>,
    ) -> Result<bool> {
        if event_type.trim().is_empty() {
            bail!("event_type must not be empty");
        }
        let connection = self.connection()?;
        let mut sql = "SELECT COUNT(*) FROM audit_log WHERE event_type = ?1".to_string();
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(event_type.to_string())];
        if let Some(agent_did) = agent_did {
            sql.push_str(" AND agent_did = ?");
            params.push(Box::new(agent_did.to_string()));
        }
        if let Some(detail_contains) = detail_contains {
            sql.push_str(" AND COALESCE(detail_json, '') LIKE ?");
            params.push(Box::new(format!("%{detail_contains}%")));
        }
        let count: i64 = connection.query_row(
            &sql,
            rusqlite::params_from_iter(params.iter().map(|value| value.as_ref())),
            |row| row.get(0),
        )?;
        Ok(count > 0)
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
            controller_user_id TEXT NOT NULL DEFAULT '',
            controller_full_handle TEXT NOT NULL DEFAULT '',
            controller_scope_key TEXT NOT NULL DEFAULT '',
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
            controller_user_id TEXT NOT NULL DEFAULT '',
            controller_full_handle TEXT NOT NULL DEFAULT '',
            controller_scope_key TEXT NOT NULL DEFAULT '',
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
            controller_user_id TEXT NOT NULL DEFAULT '',
            controller_full_handle TEXT NOT NULL DEFAULT '',
            controller_scope_key TEXT NOT NULL DEFAULT '',
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
            controller_scope_key TEXT NOT NULL DEFAULT '',
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

        CREATE TABLE IF NOT EXISTS runtime_daemon_binding (
            runtime_agent_did TEXT PRIMARY KEY,
            daemon_agent_did TEXT NOT NULL,
            controller_user_id TEXT NOT NULL DEFAULT '',
            controller_full_handle TEXT NOT NULL DEFAULT '',
            controller_scope_key TEXT NOT NULL DEFAULT '',
            controller_did TEXT NOT NULL,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_runtime_daemon_binding_daemon
        ON runtime_daemon_binding(daemon_agent_did, controller_scope_key);

        CREATE TABLE IF NOT EXISTS agent_status_query_throttle (
            daemon_agent_did TEXT NOT NULL,
            controller_scope_key TEXT NOT NULL DEFAULT '',
            controller_did TEXT NOT NULL,
            last_snapshot_at_ms INTEGER NOT NULL,
            PRIMARY KEY (daemon_agent_did, controller_did)
        );

        CREATE TABLE IF NOT EXISTS runtime_retry_queue (
            retry_id TEXT PRIMARY KEY,
            original_run_id TEXT NOT NULL,
            task_id TEXT NOT NULL,
            agent_did TEXT NOT NULL,
            runtime_profile_id TEXT NOT NULL,
            runtime_plugin_id TEXT NOT NULL,
            workspace_id TEXT,
            status TEXT NOT NULL,
            requested_by_command_id TEXT NOT NULL,
            attempts INTEGER NOT NULL DEFAULT 0,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_runtime_retry_queue_status
        ON runtime_retry_queue(status, created_at_ms);

        CREATE TABLE IF NOT EXISTS runtime_agent_create_request (
            daemon_agent_did TEXT NOT NULL,
            controller_scope_key TEXT NOT NULL DEFAULT '',
            controller_did TEXT NOT NULL,
            client_request_id TEXT NOT NULL,
            runtime_agent_did TEXT NOT NULL,
            command_id TEXT NOT NULL,
            outcome_json TEXT NOT NULL,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL,
            PRIMARY KEY (daemon_agent_did, controller_scope_key, client_request_id)
        );

        CREATE INDEX IF NOT EXISTS idx_runtime_agent_create_request_runtime
        ON runtime_agent_create_request(runtime_agent_did);

        CREATE TABLE IF NOT EXISTS runtime_final_outbox (
            idempotency_key TEXT PRIMARY KEY,
            run_id TEXT NOT NULL UNIQUE,
            agent_did TEXT NOT NULL,
            runtime_profile_id TEXT NOT NULL,
            controller_scope_key TEXT NOT NULL DEFAULT '',
            controller_did TEXT NOT NULL,
            conversation_id TEXT,
            final_text TEXT NOT NULL,
            security TEXT NOT NULL DEFAULT 'default_plain',
            status TEXT NOT NULL,
            attempt_count INTEGER NOT NULL DEFAULT 0,
            next_attempt_at_ms INTEGER NOT NULL DEFAULT 0,
            last_error_code TEXT,
            last_error_summary TEXT,
            message_id TEXT,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL,
            sent_at_ms INTEGER
        );

        CREATE INDEX IF NOT EXISTS idx_runtime_final_outbox_due
        ON runtime_final_outbox(status, next_attempt_at_ms, created_at_ms);

        CREATE TABLE IF NOT EXISTS user_delegated_identity (
            verification_method TEXT PRIMARY KEY,
            user_did TEXT NOT NULL,
            app_instance_id TEXT NOT NULL,
            controller_did TEXT NOT NULL,
            daemon_agent_did TEXT NOT NULL,
            public_key_multibase TEXT NOT NULL,
            private_key_material TEXT NOT NULL,
            allowed_scopes_json TEXT NOT NULL,
            status TEXT NOT NULL,
            expires_at TEXT,
            bootstrap_id TEXT NOT NULL,
            idempotency_key TEXT NOT NULL,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_user_delegated_identity_user
        ON user_delegated_identity(user_did, app_instance_id, status);

        CREATE TABLE IF NOT EXISTS bootstrap_replay (
            bootstrap_id TEXT PRIMARY KEY,
            idempotency_key TEXT NOT NULL UNIQUE,
            payload_hash TEXT NOT NULL,
            user_did TEXT NOT NULL,
            verification_method TEXT NOT NULL,
            app_instance_id TEXT NOT NULL,
            daemon_agent_did TEXT NOT NULL,
            status TEXT NOT NULL,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_bootstrap_replay_identity
        ON bootstrap_replay(user_did, verification_method, app_instance_id);

        CREATE TABLE IF NOT EXISTS app_message_agent_binding (
            binding_id TEXT PRIMARY KEY,
            user_did TEXT NOT NULL,
            inbox_auth_verification_method TEXT NOT NULL,
            app_instance_id TEXT NOT NULL,
            bootstrap_id TEXT NOT NULL,
            idempotency_key TEXT NOT NULL,
            daemon_agent_did TEXT NOT NULL,
            runtime_agent_did TEXT NOT NULL,
            runtime_profile_id TEXT NOT NULL,
            role TEXT NOT NULL,
            desired_agent_json TEXT NOT NULL,
            capability_policy_json TEXT NOT NULL,
            status TEXT NOT NULL,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL,
            revoked_at_ms INTEGER
        );

        CREATE INDEX IF NOT EXISTS idx_app_message_agent_binding_active
        ON app_message_agent_binding(user_did, app_instance_id, role, status, revoked_at_ms);

        CREATE UNIQUE INDEX IF NOT EXISTS ux_app_message_agent_binding_active_role
        ON app_message_agent_binding(user_did, app_instance_id, role)
        WHERE revoked_at_ms IS NULL
          AND status IN ('message_agent_ready', 'message_agent_active', 'message_agent_ensuring');

        CREATE TABLE IF NOT EXISTS inbox_cursor (
            owner_did TEXT NOT NULL,
            inbox_scope TEXT NOT NULL,
            cursor TEXT,
            updated_at_ms INTEGER NOT NULL,
            PRIMARY KEY (owner_did, inbox_scope)
        );

        CREATE TABLE IF NOT EXISTS processed_message (
            owner_did TEXT NOT NULL,
            message_id TEXT NOT NULL,
            schema TEXT NOT NULL,
            processed_at_ms INTEGER NOT NULL,
            status TEXT NOT NULL,
            PRIMARY KEY (owner_did, message_id)
        );

        CREATE INDEX IF NOT EXISTS idx_processed_message_status
        ON processed_message(owner_did, status, processed_at_ms);

        CREATE TABLE IF NOT EXISTS message_event (
            event_id TEXT PRIMARY KEY,
            owner_did TEXT NOT NULL,
            conversation_id TEXT,
            message_id TEXT NOT NULL,
            message_kind TEXT NOT NULL,
            sender_did TEXT NOT NULL,
            received_at TEXT,
            plain_text_ref_or_excerpt TEXT,
            content_hash TEXT NOT NULL,
            schema TEXT NOT NULL,
            processing_status TEXT NOT NULL,
            retention_class TEXT NOT NULL,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_message_event_owner_message
        ON message_event(owner_did, message_id);

        CREATE INDEX IF NOT EXISTS idx_message_event_processing
        ON message_event(owner_did, processing_status, created_at_ms);

        CREATE TABLE IF NOT EXISTS message_sync_outbox (
            idempotency_key TEXT PRIMARY KEY,
            owner_did TEXT NOT NULL,
            app_instance_id TEXT NOT NULL,
            payload_json TEXT NOT NULL,
            status TEXT NOT NULL,
            attempt_count INTEGER NOT NULL DEFAULT 0,
            next_attempt_at_ms INTEGER NOT NULL DEFAULT 0,
            last_error_code TEXT,
            last_error_summary TEXT,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL,
            sent_at_ms INTEGER
        );

        CREATE INDEX IF NOT EXISTS idx_message_sync_outbox_due
        ON message_sync_outbox(status, next_attempt_at_ms, created_at_ms);

        CREATE TABLE IF NOT EXISTS control_command_state (
            daemon_agent_did TEXT NOT NULL,
            controller_scope_key TEXT NOT NULL,
            command_id TEXT NOT NULL,
            command TEXT NOT NULL,
            message_id TEXT NOT NULL,
            status TEXT NOT NULL,
            target_version TEXT,
            result_json TEXT NOT NULL DEFAULT '{}',
            error_summary TEXT,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL,
            PRIMARY KEY (daemon_agent_did, controller_scope_key, command_id)
        );

        CREATE INDEX IF NOT EXISTS idx_control_command_state_message
        ON control_command_state(daemon_agent_did, message_id);

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
    migrate_agent_management_v11(connection)?;
    migrate_runtime_retry_queue_v12(connection)?;
    migrate_runtime_agent_create_request_v13(connection)?;
    migrate_runtime_final_outbox_v14(connection)?;
    migrate_runtime_final_plain_delivery_v15(connection)?;
    migrate_user_delegated_identity_v16(connection)?;
    migrate_app_message_agent_binding_v17(connection)?;
    migrate_user_delegated_inbox_sync_v18(connection)?;
    migrate_controller_scope_v19(connection)?;
    migrate_control_command_state_v20(connection)?;
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

fn migrate_user_delegated_identity_v16(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS user_delegated_identity (
            verification_method TEXT PRIMARY KEY,
            user_did TEXT NOT NULL,
            app_instance_id TEXT NOT NULL,
            controller_did TEXT NOT NULL,
            daemon_agent_did TEXT NOT NULL,
            public_key_multibase TEXT NOT NULL,
            private_key_material TEXT NOT NULL,
            allowed_scopes_json TEXT NOT NULL,
            status TEXT NOT NULL,
            expires_at TEXT,
            bootstrap_id TEXT NOT NULL,
            idempotency_key TEXT NOT NULL,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_user_delegated_identity_user
        ON user_delegated_identity(user_did, app_instance_id, status);

        CREATE TABLE IF NOT EXISTS bootstrap_replay (
            bootstrap_id TEXT PRIMARY KEY,
            idempotency_key TEXT NOT NULL UNIQUE,
            payload_hash TEXT NOT NULL,
            user_did TEXT NOT NULL,
            verification_method TEXT NOT NULL,
            app_instance_id TEXT NOT NULL,
            daemon_agent_did TEXT NOT NULL,
            status TEXT NOT NULL,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_bootstrap_replay_identity
        ON bootstrap_replay(user_did, verification_method, app_instance_id);
        "#,
    )?;
    Ok(())
}

fn migrate_app_message_agent_binding_v17(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS app_message_agent_binding (
            binding_id TEXT PRIMARY KEY,
            user_did TEXT NOT NULL,
            inbox_auth_verification_method TEXT NOT NULL,
            app_instance_id TEXT NOT NULL,
            bootstrap_id TEXT NOT NULL,
            idempotency_key TEXT NOT NULL,
            daemon_agent_did TEXT NOT NULL,
            runtime_agent_did TEXT NOT NULL,
            runtime_profile_id TEXT NOT NULL,
            role TEXT NOT NULL,
            desired_agent_json TEXT NOT NULL,
            capability_policy_json TEXT NOT NULL,
            status TEXT NOT NULL,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL,
            revoked_at_ms INTEGER
        );

        CREATE INDEX IF NOT EXISTS idx_app_message_agent_binding_active
        ON app_message_agent_binding(user_did, app_instance_id, role, status, revoked_at_ms);

        CREATE UNIQUE INDEX IF NOT EXISTS ux_app_message_agent_binding_active_role
        ON app_message_agent_binding(user_did, app_instance_id, role)
        WHERE revoked_at_ms IS NULL
          AND status IN ('message_agent_ready', 'message_agent_active', 'message_agent_ensuring');
        "#,
    )?;
    Ok(())
}

fn migrate_user_delegated_inbox_sync_v18(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS inbox_cursor (
            owner_did TEXT NOT NULL,
            inbox_scope TEXT NOT NULL,
            cursor TEXT,
            updated_at_ms INTEGER NOT NULL,
            PRIMARY KEY (owner_did, inbox_scope)
        );

        CREATE TABLE IF NOT EXISTS processed_message (
            owner_did TEXT NOT NULL,
            message_id TEXT NOT NULL,
            schema TEXT NOT NULL,
            processed_at_ms INTEGER NOT NULL,
            status TEXT NOT NULL,
            PRIMARY KEY (owner_did, message_id)
        );

        CREATE INDEX IF NOT EXISTS idx_processed_message_status
        ON processed_message(owner_did, status, processed_at_ms);

        CREATE TABLE IF NOT EXISTS message_event (
            event_id TEXT PRIMARY KEY,
            owner_did TEXT NOT NULL,
            conversation_id TEXT,
            message_id TEXT NOT NULL,
            message_kind TEXT NOT NULL,
            sender_did TEXT NOT NULL,
            received_at TEXT,
            plain_text_ref_or_excerpt TEXT,
            content_hash TEXT NOT NULL,
            schema TEXT NOT NULL,
            processing_status TEXT NOT NULL,
            retention_class TEXT NOT NULL,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_message_event_owner_message
        ON message_event(owner_did, message_id);

        CREATE INDEX IF NOT EXISTS idx_message_event_processing
        ON message_event(owner_did, processing_status, created_at_ms);

        CREATE TABLE IF NOT EXISTS message_sync_outbox (
            idempotency_key TEXT PRIMARY KEY,
            owner_did TEXT NOT NULL,
            app_instance_id TEXT NOT NULL,
            payload_json TEXT NOT NULL,
            status TEXT NOT NULL,
            attempt_count INTEGER NOT NULL DEFAULT 0,
            next_attempt_at_ms INTEGER NOT NULL DEFAULT 0,
            last_error_code TEXT,
            last_error_summary TEXT,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL,
            sent_at_ms INTEGER
        );

        CREATE INDEX IF NOT EXISTS idx_message_sync_outbox_due
        ON message_sync_outbox(status, next_attempt_at_ms, created_at_ms);
        "#,
    )?;
    Ok(())
}

fn migrate_control_command_state_v20(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS control_command_state (
            daemon_agent_did TEXT NOT NULL,
            controller_scope_key TEXT NOT NULL,
            command_id TEXT NOT NULL,
            command TEXT NOT NULL,
            message_id TEXT NOT NULL,
            status TEXT NOT NULL,
            target_version TEXT,
            result_json TEXT NOT NULL DEFAULT '{}',
            error_summary TEXT,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL,
            PRIMARY KEY (daemon_agent_did, controller_scope_key, command_id)
        );

        CREATE INDEX IF NOT EXISTS idx_control_command_state_message
        ON control_command_state(daemon_agent_did, message_id);
        "#,
    )?;
    Ok(())
}

fn migrate_runtime_retry_queue_v12(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS runtime_retry_queue (
            retry_id TEXT PRIMARY KEY,
            original_run_id TEXT NOT NULL,
            task_id TEXT NOT NULL,
            agent_did TEXT NOT NULL,
            runtime_profile_id TEXT NOT NULL,
            runtime_plugin_id TEXT NOT NULL,
            workspace_id TEXT,
            status TEXT NOT NULL,
            requested_by_command_id TEXT NOT NULL,
            attempts INTEGER NOT NULL DEFAULT 0,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_runtime_retry_queue_status
        ON runtime_retry_queue(status, created_at_ms);
        "#,
    )?;
    Ok(())
}

fn migrate_runtime_agent_create_request_v13(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS runtime_agent_create_request (
            daemon_agent_did TEXT NOT NULL,
            controller_did TEXT NOT NULL,
            client_request_id TEXT NOT NULL,
            runtime_agent_did TEXT NOT NULL,
            command_id TEXT NOT NULL,
            outcome_json TEXT NOT NULL,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL,
            PRIMARY KEY (daemon_agent_did, controller_did, client_request_id)
        );

        CREATE INDEX IF NOT EXISTS idx_runtime_agent_create_request_runtime
        ON runtime_agent_create_request(runtime_agent_did);
        "#,
    )?;
    Ok(())
}

fn migrate_runtime_final_outbox_v14(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS runtime_final_outbox (
            idempotency_key TEXT PRIMARY KEY,
            run_id TEXT NOT NULL UNIQUE,
            agent_did TEXT NOT NULL,
            runtime_profile_id TEXT NOT NULL,
            controller_did TEXT NOT NULL,
            conversation_id TEXT,
            final_text TEXT NOT NULL,
            security TEXT NOT NULL DEFAULT 'default_plain',
            status TEXT NOT NULL,
            attempt_count INTEGER NOT NULL DEFAULT 0,
            next_attempt_at_ms INTEGER NOT NULL DEFAULT 0,
            last_error_code TEXT,
            last_error_summary TEXT,
            message_id TEXT,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL,
            sent_at_ms INTEGER
        );

        CREATE INDEX IF NOT EXISTS idx_runtime_final_outbox_due
        ON runtime_final_outbox(status, next_attempt_at_ms, created_at_ms);
        "#,
    )?;
    Ok(())
}

fn migrate_runtime_final_plain_delivery_v15(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        r#"
        UPDATE runtime_final_outbox
        SET security = 'default_plain',
            status = CASE WHEN status = 'sent' THEN status ELSE 'pending' END,
            attempt_count = CASE WHEN status = 'sent' THEN attempt_count ELSE 0 END,
            next_attempt_at_ms = CASE WHEN status = 'sent' THEN next_attempt_at_ms ELSE 0 END,
            last_error_code = CASE WHEN status = 'sent' THEN last_error_code ELSE NULL END,
            last_error_summary = CASE WHEN status = 'sent' THEN last_error_summary ELSE NULL END,
            updated_at_ms = strftime('%s','now') * 1000
        WHERE security = 'direct_e2ee'
          AND status != 'sent';
        "#,
    )?;
    Ok(())
}

fn migrate_controller_scope_v19(connection: &Connection) -> Result<()> {
    for (table, column, definition) in [
        (
            "agent_definition",
            "controller_user_id",
            "TEXT NOT NULL DEFAULT ''",
        ),
        (
            "agent_definition",
            "controller_full_handle",
            "TEXT NOT NULL DEFAULT ''",
        ),
        (
            "agent_definition",
            "controller_scope_key",
            "TEXT NOT NULL DEFAULT ''",
        ),
        (
            "runtime_task",
            "controller_user_id",
            "TEXT NOT NULL DEFAULT ''",
        ),
        (
            "runtime_task",
            "controller_full_handle",
            "TEXT NOT NULL DEFAULT ''",
        ),
        (
            "runtime_task",
            "controller_scope_key",
            "TEXT NOT NULL DEFAULT ''",
        ),
        (
            "hermes_native_sessions",
            "controller_scope_key",
            "TEXT NOT NULL DEFAULT ''",
        ),
        (
            "runtime_daemon_binding",
            "controller_user_id",
            "TEXT NOT NULL DEFAULT ''",
        ),
        (
            "runtime_daemon_binding",
            "controller_full_handle",
            "TEXT NOT NULL DEFAULT ''",
        ),
        (
            "runtime_daemon_binding",
            "controller_scope_key",
            "TEXT NOT NULL DEFAULT ''",
        ),
        (
            "runtime_final_outbox",
            "controller_scope_key",
            "TEXT NOT NULL DEFAULT ''",
        ),
        (
            "cli_driver_run",
            "controller_user_id",
            "TEXT NOT NULL DEFAULT ''",
        ),
        (
            "cli_driver_run",
            "controller_full_handle",
            "TEXT NOT NULL DEFAULT ''",
        ),
        (
            "cli_driver_run",
            "controller_scope_key",
            "TEXT NOT NULL DEFAULT ''",
        ),
    ] {
        add_column_if_missing(connection, table, column, definition)?;
    }

    connection.execute_batch(
        r#"
        UPDATE agent_definition
        SET
            controller_user_id = CASE WHEN controller_user_id = '' THEN 'legacy-user:' || hex(controller_did) ELSE controller_user_id END,
            controller_full_handle = CASE WHEN controller_full_handle = '' THEN 'legacy-handle:' || hex(controller_did) ELSE controller_full_handle END,
            controller_scope_key = CASE WHEN controller_scope_key = '' THEN 'controller-scope:legacy-did:' || hex(controller_did) ELSE controller_scope_key END
        WHERE controller_did <> '';

        UPDATE runtime_task
        SET
            controller_user_id = CASE WHEN controller_user_id = '' THEN COALESCE((SELECT controller_user_id FROM agent_definition WHERE agent_definition.agent_did = runtime_task.agent_did), 'legacy-user:' || hex(controller_did)) ELSE controller_user_id END,
            controller_full_handle = CASE WHEN controller_full_handle = '' THEN COALESCE((SELECT controller_full_handle FROM agent_definition WHERE agent_definition.agent_did = runtime_task.agent_did), 'legacy-handle:' || hex(controller_did)) ELSE controller_full_handle END,
            controller_scope_key = CASE WHEN controller_scope_key = '' THEN COALESCE((SELECT controller_scope_key FROM agent_definition WHERE agent_definition.agent_did = runtime_task.agent_did), 'controller-scope:legacy-did:' || hex(controller_did)) ELSE controller_scope_key END
        WHERE controller_did <> '';

        UPDATE runtime_daemon_binding
        SET
            controller_user_id = CASE WHEN controller_user_id = '' THEN COALESCE((SELECT controller_user_id FROM agent_definition WHERE agent_definition.agent_did = runtime_daemon_binding.daemon_agent_did), 'legacy-user:' || hex(controller_did)) ELSE controller_user_id END,
            controller_full_handle = CASE WHEN controller_full_handle = '' THEN COALESCE((SELECT controller_full_handle FROM agent_definition WHERE agent_definition.agent_did = runtime_daemon_binding.daemon_agent_did), 'legacy-handle:' || hex(controller_did)) ELSE controller_full_handle END,
            controller_scope_key = CASE WHEN controller_scope_key = '' THEN COALESCE((SELECT controller_scope_key FROM agent_definition WHERE agent_definition.agent_did = runtime_daemon_binding.daemon_agent_did), 'controller-scope:legacy-did:' || hex(controller_did)) ELSE controller_scope_key END
        WHERE controller_did <> '';

        UPDATE hermes_native_sessions
        SET controller_scope_key = 'controller-scope:legacy-did:' || hex(controller_did)
        WHERE controller_scope_key = ''
          AND controller_did <> '';

        UPDATE runtime_final_outbox
        SET controller_scope_key = COALESCE(
            (SELECT controller_scope_key FROM agent_definition WHERE agent_definition.agent_did = runtime_final_outbox.agent_did),
            'controller-scope:legacy-did:' || hex(controller_did)
        )
        WHERE controller_scope_key = ''
          AND controller_did <> '';

        UPDATE cli_driver_run
        SET
            controller_user_id = CASE WHEN controller_user_id = '' THEN COALESCE((SELECT controller_user_id FROM agent_definition WHERE agent_definition.agent_did = cli_driver_run.agent_did), 'legacy-user:' || hex(controller_did)) ELSE controller_user_id END,
            controller_full_handle = CASE WHEN controller_full_handle = '' THEN COALESCE((SELECT controller_full_handle FROM agent_definition WHERE agent_definition.agent_did = cli_driver_run.agent_did), 'legacy-handle:' || hex(controller_did)) ELSE controller_full_handle END,
            controller_scope_key = CASE WHEN controller_scope_key = '' THEN COALESCE((SELECT controller_scope_key FROM agent_definition WHERE agent_definition.agent_did = cli_driver_run.agent_did), 'controller-scope:legacy-did:' || hex(controller_did)) ELSE controller_scope_key END
        WHERE controller_did <> '';
        "#,
    )?;

    rebuild_runtime_agent_create_request_for_scope(connection)?;
    connection.execute_batch(
        r#"
        CREATE INDEX IF NOT EXISTS idx_runtime_daemon_binding_daemon_scope
        ON runtime_daemon_binding(daemon_agent_did, controller_scope_key);
        "#,
    )?;
    Ok(())
}

fn rebuild_runtime_agent_create_request_for_scope(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS runtime_agent_create_request_v19 (
            daemon_agent_did TEXT NOT NULL,
            controller_scope_key TEXT NOT NULL DEFAULT '',
            controller_did TEXT NOT NULL,
            client_request_id TEXT NOT NULL,
            runtime_agent_did TEXT NOT NULL,
            command_id TEXT NOT NULL,
            outcome_json TEXT NOT NULL,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL,
            PRIMARY KEY (daemon_agent_did, controller_scope_key, client_request_id)
        );

        INSERT OR IGNORE INTO runtime_agent_create_request_v19 (
            daemon_agent_did,
            controller_scope_key,
            controller_did,
            client_request_id,
            runtime_agent_did,
            command_id,
            outcome_json,
            created_at_ms,
            updated_at_ms
        )
        SELECT
            daemon_agent_did,
            COALESCE(
                (SELECT controller_scope_key FROM agent_definition WHERE agent_definition.agent_did = runtime_agent_create_request.daemon_agent_did),
                'controller-scope:legacy-did:' || hex(controller_did)
            ),
            controller_did,
            client_request_id,
            runtime_agent_did,
            command_id,
            outcome_json,
            created_at_ms,
            updated_at_ms
        FROM runtime_agent_create_request;

        DROP TABLE runtime_agent_create_request;
        ALTER TABLE runtime_agent_create_request_v19 RENAME TO runtime_agent_create_request;

        CREATE INDEX IF NOT EXISTS idx_runtime_agent_create_request_runtime
        ON runtime_agent_create_request(runtime_agent_did);
        "#,
    )?;
    Ok(())
}

fn migrate_agent_management_v11(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS runtime_daemon_binding (
            runtime_agent_did TEXT PRIMARY KEY,
            daemon_agent_did TEXT NOT NULL,
            controller_did TEXT NOT NULL,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_runtime_daemon_binding_daemon
        ON runtime_daemon_binding(daemon_agent_did, controller_did);

        CREATE TABLE IF NOT EXISTS agent_status_query_throttle (
            daemon_agent_did TEXT NOT NULL,
            controller_did TEXT NOT NULL,
            last_snapshot_at_ms INTEGER NOT NULL,
            PRIMARY KEY (daemon_agent_did, controller_did)
        );

        INSERT OR IGNORE INTO runtime_daemon_binding (
            runtime_agent_did,
            daemon_agent_did,
            controller_did,
            created_at_ms,
            updated_at_ms
        )
        SELECT
            runtime.agent_did,
            daemon.agent_did,
            runtime.controller_did,
            0,
            0
        FROM agent_definition AS runtime
        INNER JOIN agent_definition AS daemon
            ON daemon.agent_kind = 'daemon'
           AND daemon.controller_did = runtime.controller_did
        WHERE runtime.agent_kind = 'runtime'
          AND (
              SELECT COUNT(*)
              FROM agent_definition AS daemon_count
              WHERE daemon_count.agent_kind = 'daemon'
                AND daemon_count.controller_did = runtime.controller_did
          ) = 1;
        "#,
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
    let workspace_mode_raw: Option<String> = row.get(13)?;
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
    let command_raw: String = row.get(15)?;
    let command_json = serde_json::from_str(&command_raw).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(
            command_raw.len(),
            rusqlite::types::Type::Text,
            Box::new(err),
        )
    })?;
    let output_raw: String = row.get(16)?;
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
        controller_user_id: row.get(4)?,
        controller_full_handle: row.get(5)?,
        controller_scope_key: row.get(6)?,
        controller_did: row.get(7)?,
        conversation_id: row.get(8)?,
        route_key: row.get(9)?,
        workspace_id: row.get(10)?,
        workspace_root: row.get::<_, Option<String>>(11)?.map(PathBuf::from),
        workspace_instance_path: row.get::<_, Option<String>>(12)?.map(PathBuf::from),
        workspace_mode,
        is_security_boundary: row.get::<_, i64>(14)? != 0,
        command_json,
        output_json,
        final_output_path: row.get::<_, Option<String>>(17)?.map(PathBuf::from),
        native_session_id: row.get(18)?,
        synthetic_session_id: row.get(19)?,
        status: row.get(20)?,
        fallback_final_source: row.get(21)?,
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
        controller_user_id: row.get(3)?,
        controller_full_handle: row.get(4)?,
        controller_scope_key: row.get(5)?,
        controller_did: row.get(6)?,
        runtime_plugin_id: row.get(7)?,
        runtime_profile_id: row.get(8)?,
        workspace_id: row.get(9)?,
        policy_id: row.get(10)?,
        local_agent_db_path: row.get(11)?,
        message_db_path: row.get(12)?,
        status: row.get(13)?,
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
        controller_scope_key: row.get(4)?,
        controller_did: row.get(5)?,
        conversation_id: row.get(6)?,
        route_key: row.get(7)?,
        hermes_profile: row.get(8)?,
        hermes_session_id: row.get(9)?,
        session_kind: row.get(10)?,
        status: row.get(11)?,
        created_at_ms: row.get(12)?,
        updated_at_ms: row.get(13)?,
    })
}

fn runtime_retry_queue_record_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<RuntimeRetryQueueRecord> {
    Ok(RuntimeRetryQueueRecord {
        retry_id: row.get(0)?,
        original_run_id: row.get(1)?,
        task_id: row.get(2)?,
        agent_did: row.get(3)?,
        runtime_profile_id: row.get(4)?,
        runtime_plugin_id: row.get(5)?,
        workspace_id: row.get(6)?,
        status: row.get(7)?,
        requested_by_command_id: row.get(8)?,
        attempts: row.get(9)?,
        created_at_ms: row.get(10)?,
        updated_at_ms: row.get(11)?,
    })
}

fn runtime_final_outbox_record_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<RuntimeFinalOutboxRecord> {
    Ok(RuntimeFinalOutboxRecord {
        idempotency_key: row.get(0)?,
        run_id: row.get(1)?,
        agent_did: row.get(2)?,
        runtime_profile_id: row.get(3)?,
        controller_scope_key: row.get(4)?,
        controller_did: row.get(5)?,
        conversation_id: row.get(6)?,
        final_text: row.get(7)?,
        security: row.get(8)?,
        status: row.get(9)?,
        attempt_count: row.get(10)?,
        next_attempt_at_ms: row.get(11)?,
        last_error_code: row.get(12)?,
        last_error_summary: row.get(13)?,
        message_id: row.get(14)?,
        created_at_ms: row.get(15)?,
        updated_at_ms: row.get(16)?,
        sent_at_ms: row.get(17)?,
    })
}

fn runtime_agent_create_request_record_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<RuntimeAgentCreateRequestRecord> {
    let outcome_json: String = row.get(6)?;
    let outcome_json = serde_json::from_str(&outcome_json).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(
            outcome_json.len(),
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                err.to_string(),
            )),
        )
    })?;
    Ok(RuntimeAgentCreateRequestRecord {
        daemon_agent_did: row.get(0)?,
        controller_scope_key: row.get(1)?,
        controller_did: row.get(2)?,
        client_request_id: row.get(3)?,
        runtime_agent_did: row.get(4)?,
        command_id: row.get(5)?,
        outcome_json,
        created_at_ms: row.get(7)?,
        updated_at_ms: row.get(8)?,
    })
}

fn control_command_state_record_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<ControlCommandStateRecord> {
    let result_json_raw: String = row.get(7)?;
    let result_json = serde_json::from_str(&result_json_raw).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(
            result_json_raw.len(),
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                err.to_string(),
            )),
        )
    })?;
    Ok(ControlCommandStateRecord {
        daemon_agent_did: row.get(0)?,
        controller_scope_key: row.get(1)?,
        command_id: row.get(2)?,
        command: row.get(3)?,
        message_id: row.get(4)?,
        status: row.get(5)?,
        target_version: row.get(6)?,
        result_json,
        error_summary: row.get(8)?,
        created_at_ms: row.get(9)?,
        updated_at_ms: row.get(10)?,
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
            "runtime_daemon_binding",
            "agent_status_query_throttle",
            "runtime_retry_queue",
            "runtime_agent_create_request",
            "runtime_final_outbox",
            "user_delegated_identity",
            "bootstrap_replay",
            "app_message_agent_binding",
            "inbox_cursor",
            "processed_message",
            "message_event",
            "message_sync_outbox",
            "control_command_state",
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

    fn delegated_identity_fixture() -> (UserDelegatedIdentityRecord, BootstrapReplayRecord) {
        let identity = UserDelegatedIdentityRecord {
            user_did: "did:wba:example.com:user:alice:e1_user".to_string(),
            verification_method: "did:wba:example.com:user:alice:e1_user#daemon-key-1".to_string(),
            app_instance_id: "app_1".to_string(),
            controller_did: "did:wba:example.com:user:alice:e1_user".to_string(),
            daemon_agent_did: "did:agent:daemon".to_string(),
            public_key_multibase: "z-public".to_string(),
            private_key_material: "z-private-secret".to_string(),
            allowed_scopes_json: serde_json::json!([
                "message.inbox.read.plain",
                "message.history.read.plain",
                "message.send.plain"
            ]),
            status: "paired_key_received".to_string(),
            expires_at: Some("2026-09-09T00:00:00Z".to_string()),
            bootstrap_id: "boot_1".to_string(),
            idempotency_key: "message-agent-bootstrap:did:wba:example.com:user:alice:e1_user:app_1"
                .to_string(),
            created_at_ms: 0,
            updated_at_ms: 0,
        };
        let replay = BootstrapReplayRecord {
            bootstrap_id: identity.bootstrap_id.clone(),
            idempotency_key: identity.idempotency_key.clone(),
            payload_hash: "payload-hash-1".to_string(),
            user_did: identity.user_did.clone(),
            verification_method: identity.verification_method.clone(),
            app_instance_id: identity.app_instance_id.clone(),
            daemon_agent_did: identity.daemon_agent_did.clone(),
            status: identity.status.clone(),
            created_at_ms: 0,
            updated_at_ms: 0,
        };
        (identity, replay)
    }

    #[test]
    fn user_delegated_identity_roundtrips_and_replays_idempotently() {
        let root = tempfile::tempdir().unwrap();
        let config = DaemonConfig::for_state_root(root.path()).unwrap();
        let state = DaemonState::open(&config).unwrap();
        state.initialize().unwrap();
        let (identity, replay) = delegated_identity_fixture();

        assert_eq!(
            state.store_bootstrap_state(&identity, &replay).unwrap(),
            BootstrapStoreOutcome::Inserted
        );
        assert_eq!(
            state.store_bootstrap_state(&identity, &replay).unwrap(),
            BootstrapStoreOutcome::Duplicate
        );

        let loaded = state
            .load_user_delegated_identity(&identity.verification_method)
            .unwrap()
            .unwrap();
        assert_eq!(loaded.user_did, identity.user_did);
        assert_eq!(loaded.private_key_material, "z-private-secret");
        assert_eq!(loaded.status, "paired_key_received");
        assert!(!format!("{loaded:?}").contains("z-private-secret"));

        let replay_loaded = state.load_bootstrap_replay("boot_1").unwrap().unwrap();
        assert_eq!(replay_loaded.payload_hash, "payload-hash-1");

        let reopened = DaemonState::open(&config).unwrap();
        let recovered = reopened
            .load_user_delegated_identity(&identity.verification_method)
            .unwrap()
            .unwrap();
        assert_eq!(recovered.status, "paired_key_received");
    }

    #[test]
    fn user_delegated_identity_rejects_conflicting_replay() {
        let root = tempfile::tempdir().unwrap();
        let config = DaemonConfig::for_state_root(root.path()).unwrap();
        let state = DaemonState::open(&config).unwrap();
        state.initialize().unwrap();
        let (identity, mut replay) = delegated_identity_fixture();
        state.store_bootstrap_state(&identity, &replay).unwrap();
        replay.payload_hash = "payload-hash-2".to_string();

        let error = state
            .store_bootstrap_state(&identity, &replay)
            .unwrap_err()
            .to_string();
        assert!(error.contains("replay conflict"));
    }

    #[test]
    fn app_message_agent_binding_roundtrips_and_restores_active_record() {
        let root = tempfile::tempdir().unwrap();
        let config = DaemonConfig::for_state_root(root.path()).unwrap();
        let state = DaemonState::open(&config).unwrap();
        state.initialize().unwrap();
        let record = AppMessageAgentBindingRecord {
            binding_id: "app-message-agent:did:human:alice:app_1".to_string(),
            user_did: "did:human:alice".to_string(),
            inbox_auth_verification_method: "did:human:alice#daemon-key-1".to_string(),
            app_instance_id: "app_1".to_string(),
            bootstrap_id: "boot_1".to_string(),
            idempotency_key: "message-agent-bootstrap:did:human:alice:app_1".to_string(),
            daemon_agent_did: "did:agent:daemon".to_string(),
            runtime_agent_did: "did:agent:runtime-hermes".to_string(),
            runtime_profile_id: "profile_hermes_app_message".to_string(),
            role: "app_message_handler".to_string(),
            desired_agent_json: serde_json::json!({
                "role": "app_message_handler",
                "runtime": "hermes"
            }),
            capability_policy_json: serde_json::json!({
                "allowed_actions": ["message.summarize_plain"]
            }),
            status: "message_agent_ready".to_string(),
            created_at_ms: 0,
            updated_at_ms: 0,
            revoked_at_ms: None,
        };

        state.upsert_app_message_agent_binding(&record).unwrap();
        let loaded = state
            .load_active_app_message_agent_binding(
                "did:human:alice",
                "app_1",
                "app_message_handler",
            )
            .unwrap()
            .unwrap();
        assert_eq!(loaded.binding_id, record.binding_id);
        assert_eq!(loaded.runtime_agent_did, "did:agent:runtime-hermes");

        let reopened = DaemonState::open(&config).unwrap();
        let restored = reopened
            .load_app_message_agent_binding(&record.binding_id)
            .unwrap()
            .unwrap();
        assert_eq!(restored.status, "message_agent_ready");
    }

    #[test]
    fn app_message_agent_binding_revokes_superseded_records_for_same_user_role() {
        let root = tempfile::tempdir().unwrap();
        let config = DaemonConfig::for_state_root(root.path()).unwrap();
        let state = DaemonState::open(&config).unwrap();
        state.initialize().unwrap();
        let mut first = AppMessageAgentBindingRecord {
            binding_id: "app-message-agent:did:human:alice:app_1".to_string(),
            user_did: "did:human:alice".to_string(),
            inbox_auth_verification_method: "did:human:alice#daemon-key-1".to_string(),
            app_instance_id: "app_1".to_string(),
            bootstrap_id: "boot_1".to_string(),
            idempotency_key: "message-agent-bootstrap:did:human:alice:app_1".to_string(),
            daemon_agent_did: "did:agent:daemon".to_string(),
            runtime_agent_did: "did:agent:runtime-hermes-1".to_string(),
            runtime_profile_id: "profile_hermes_app_message_1".to_string(),
            role: "app_message_handler".to_string(),
            desired_agent_json: serde_json::json!({
                "role": "app_message_handler",
                "runtime": "hermes"
            }),
            capability_policy_json: serde_json::json!({
                "allowed_actions": ["message.summarize_plain"]
            }),
            status: "message_agent_ready".to_string(),
            created_at_ms: 0,
            updated_at_ms: 0,
            revoked_at_ms: None,
        };
        let mut second = first.clone();
        second.binding_id = "app-message-agent:did:human:alice:app_2".to_string();
        second.app_instance_id = "app_2".to_string();
        second.bootstrap_id = "boot_2".to_string();
        second.idempotency_key = "message-agent-bootstrap:did:human:alice:app_2".to_string();
        second.runtime_agent_did = "did:agent:runtime-hermes-2".to_string();
        second.runtime_profile_id = "profile_hermes_app_message_2".to_string();
        let mut other_user = first.clone();
        other_user.binding_id = "app-message-agent:did:human:bob:app_1".to_string();
        other_user.user_did = "did:human:bob".to_string();
        other_user.inbox_auth_verification_method = "did:human:bob#daemon-key-1".to_string();
        other_user.runtime_agent_did = "did:agent:runtime-hermes-bob".to_string();
        other_user.runtime_profile_id = "profile_hermes_bob".to_string();

        state.upsert_app_message_agent_binding(&first).unwrap();
        state.upsert_app_message_agent_binding(&second).unwrap();
        state.upsert_app_message_agent_binding(&other_user).unwrap();

        let revoked = state
            .revoke_other_active_app_message_agent_bindings(
                "did:human:alice",
                "app_message_handler",
                &second.binding_id,
            )
            .unwrap();
        assert_eq!(revoked, 1);
        assert!(state
            .load_active_app_message_agent_binding(
                "did:human:alice",
                "app_1",
                "app_message_handler",
            )
            .unwrap()
            .is_none());
        assert_eq!(
            state
                .load_active_app_message_agent_binding(
                    "did:human:alice",
                    "app_2",
                    "app_message_handler",
                )
                .unwrap()
                .unwrap()
                .binding_id,
            second.binding_id
        );
        assert!(state
            .load_active_app_message_agent_binding("did:human:bob", "app_1", "app_message_handler",)
            .unwrap()
            .is_some());
        first.revoked_at_ms = state
            .load_app_message_agent_binding(&first.binding_id)
            .unwrap()
            .unwrap()
            .revoked_at_ms;
        assert!(first.revoked_at_ms.is_some());
    }

    #[test]
    fn delegated_inbox_sync_state_roundtrips_and_deduplicates_messages() {
        let root = tempfile::tempdir().unwrap();
        let config = DaemonConfig::for_state_root(root.path()).unwrap();
        let state = DaemonState::open(&config).unwrap();
        state.initialize().unwrap();

        let cursor = InboxCursorRecord {
            owner_did: "did:human:alice".to_string(),
            inbox_scope: "default_plain".to_string(),
            cursor: Some("cursor_10".to_string()),
            updated_at_ms: 0,
        };
        state.upsert_inbox_cursor(&cursor).unwrap();
        let loaded_cursor = state
            .load_inbox_cursor("did:human:alice", "default_plain")
            .unwrap()
            .unwrap();
        assert_eq!(loaded_cursor.cursor.as_deref(), Some("cursor_10"));

        let processed = ProcessedMessageRecord {
            owner_did: "did:human:alice".to_string(),
            message_id: "msg_1".to_string(),
            schema: "awiki.user_message.default_plain.v1".to_string(),
            processed_at_ms: 0,
            status: "dispatched".to_string(),
        };
        assert!(state.try_insert_processed_message(&processed).unwrap());
        assert!(!state.try_insert_processed_message(&processed).unwrap());
        state
            .mark_processed_message_status("did:human:alice", "msg_1", "done")
            .unwrap();
        let loaded_processed = state
            .load_processed_message("did:human:alice", "msg_1")
            .unwrap()
            .unwrap();
        assert_eq!(loaded_processed.status, "done");

        let event = MessageEventRecord {
            event_id: "evt_msg_1".to_string(),
            owner_did: "did:human:alice".to_string(),
            conversation_id: Some("direct:did:human:bob".to_string()),
            message_id: "msg_1".to_string(),
            message_kind: "text".to_string(),
            sender_did: "did:human:bob".to_string(),
            received_at: Some("2026-06-09T00:00:00Z".to_string()),
            plain_text_ref_or_excerpt: Some("hello".to_string()),
            content_hash: "hash_1".to_string(),
            schema: "awiki.user_message.default_plain.v1".to_string(),
            processing_status: "agent_dispatched".to_string(),
            retention_class: "short_excerpt".to_string(),
            created_at_ms: 0,
            updated_at_ms: 0,
        };
        state.upsert_message_event(&event).unwrap();
        let loaded_event = state.load_message_event("evt_msg_1").unwrap().unwrap();
        assert_eq!(
            loaded_event.plain_text_ref_or_excerpt.as_deref(),
            Some("hello")
        );

        let sync = MessageSyncOutboxRecord {
            idempotency_key: "message-sync:did:human:alice:msg_1".to_string(),
            owner_did: "did:human:alice".to_string(),
            app_instance_id: "app_1".to_string(),
            payload_json: serde_json::json!({
                "schema": "awiki.message.sync.v1",
                "message_id": "msg_1"
            }),
            status: "pending".to_string(),
            attempt_count: 0,
            next_attempt_at_ms: 0,
            last_error_code: None,
            last_error_summary: None,
            created_at_ms: 0,
            updated_at_ms: 0,
            sent_at_ms: None,
        };
        state.upsert_message_sync_outbox(&sync).unwrap();
        let loaded_sync = state
            .load_message_sync_outbox(&sync.idempotency_key)
            .unwrap()
            .unwrap();
        assert_eq!(loaded_sync.payload_json["message_id"], "msg_1");
        let due = state.list_due_message_sync_outbox(i64::MAX, 10).unwrap();
        assert_eq!(due.len(), 1);
        assert!(state
            .mark_message_sync_outbox_sending(&sync.idempotency_key)
            .unwrap());
        state
            .mark_message_sync_outbox_retry(
                &sync.idempotency_key,
                i64::MAX - 1,
                "retry",
                "retry later",
            )
            .unwrap();
        assert!(state
            .list_due_message_sync_outbox(0, 10)
            .unwrap()
            .is_empty());
        state
            .recover_stale_message_sync_outbox_sending(i64::MAX, 0)
            .unwrap();
        state
            .mark_message_sync_outbox_sending(&sync.idempotency_key)
            .unwrap();
        state
            .mark_message_sync_outbox_sent(&sync.idempotency_key)
            .unwrap();
        let sent_sync = state
            .load_message_sync_outbox(&sync.idempotency_key)
            .unwrap()
            .unwrap();
        assert_eq!(sent_sync.status, "sent");
        state
            .upsert_message_sync_outbox(&MessageSyncOutboxRecord {
                status: "pending".to_string(),
                payload_json: serde_json::json!({
                    "schema": "awiki.message.sync.v1",
                    "message_id": "msg_changed"
                }),
                ..sync
            })
            .unwrap();
        let still_sent = state
            .load_message_sync_outbox("message-sync:did:human:alice:msg_1")
            .unwrap()
            .unwrap();
        assert_eq!(still_sent.status, "sent");
        assert_eq!(still_sent.payload_json["message_id"], "msg_1");
    }

    #[test]
    fn control_command_state_roundtrips_and_deduplicates() {
        let root = tempfile::tempdir().unwrap();
        let config = DaemonConfig::for_state_root(root.path()).unwrap();
        let state = DaemonState::open(&config).unwrap();
        state.initialize().unwrap();

        let first = state
            .try_begin_control_command(
                "did:agent:daemon",
                "controller-scope:v1:test-alice",
                "cmd_upgrade_1",
                "daemon.upgrade",
                "msg_upgrade_1",
                Some("latest"),
            )
            .unwrap();
        assert!(first.is_none());

        let duplicate = state
            .try_begin_control_command(
                "did:agent:daemon",
                "controller-scope:v1:test-alice",
                "cmd_upgrade_1",
                "daemon.upgrade",
                "msg_upgrade_1",
                Some("latest"),
            )
            .unwrap()
            .unwrap();
        assert_eq!(duplicate.status, "in_progress");
        assert_eq!(duplicate.target_version.as_deref(), Some("latest"));

        state
            .mark_control_command_state(
                "did:agent:daemon",
                "controller-scope:v1:test-alice",
                "cmd_upgrade_1",
                "restart_scheduled",
                serde_json::json!({
                    "command": "daemon.upgrade",
                    "status": "ready",
                    "version": "0.2.0",
                    "restarted": true,
                }),
                None,
            )
            .unwrap();

        let stored = state
            .load_control_command_state(
                "did:agent:daemon",
                "controller-scope:v1:test-alice",
                "cmd_upgrade_1",
            )
            .unwrap()
            .unwrap();
        assert_eq!(stored.status, "restart_scheduled");
        assert_eq!(stored.result_json["version"], "0.2.0");
    }

    #[test]
    fn runtime_final_outbox_roundtrips_retry_and_sent_state() {
        let root = tempfile::tempdir().unwrap();
        let config = DaemonConfig::for_state_root(root.path()).unwrap();
        let state = DaemonState::open(&config).unwrap();
        state.initialize().unwrap();

        let now = current_time_millis().unwrap();
        let record = RuntimeFinalOutboxRecord {
            idempotency_key: "runtime-final:did:agent:hermes:run_1:controller-scope:v1:test-alice"
                .to_string(),
            run_id: "run_1".to_string(),
            agent_did: "did:agent:hermes".to_string(),
            runtime_profile_id: "profile_hermes".to_string(),
            controller_scope_key: "controller-scope:v1:test-alice".to_string(),
            controller_did: "did:human:alice".to_string(),
            conversation_id: Some("direct:did:human:alice".to_string()),
            final_text: "final text".to_string(),
            security: "default_plain".to_string(),
            status: "pending".to_string(),
            attempt_count: 0,
            next_attempt_at_ms: now,
            last_error_code: None,
            last_error_summary: None,
            message_id: None,
            created_at_ms: now,
            updated_at_ms: now,
            sent_at_ms: None,
        };

        state.upsert_runtime_final_outbox_pending(&record).unwrap();
        let due = state.list_due_runtime_final_outbox(now, 10).unwrap();
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].final_text, "final text");
        assert!(state
            .mark_runtime_final_outbox_sending(&record.idempotency_key)
            .unwrap());
        state
            .mark_runtime_final_outbox_retry(
                &record.idempotency_key,
                now + 10_000,
                "final_delivery_retry",
                "temporary unavailable",
            )
            .unwrap();
        let stored = state
            .load_runtime_final_outbox_by_run("run_1")
            .unwrap()
            .unwrap();
        assert_eq!(stored.status, "pending");
        assert_eq!(stored.attempt_count, 1);
        assert_eq!(
            stored.last_error_code.as_deref(),
            Some("final_delivery_retry")
        );
        assert!(state
            .list_due_runtime_final_outbox(now + 9_999, 10)
            .unwrap()
            .is_empty());

        assert!(state
            .mark_runtime_final_outbox_sending(&record.idempotency_key)
            .unwrap());
        let recovered = state
            .recover_stale_runtime_final_outbox_sending(now + 60_000, now)
            .unwrap();
        assert_eq!(recovered, 1);
        let due = state.list_due_runtime_final_outbox(now, 10).unwrap();
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].status, "pending");
        assert_eq!(due[0].attempt_count, 2);

        assert!(state
            .mark_runtime_final_outbox_sending(&record.idempotency_key)
            .unwrap());
        state
            .mark_runtime_final_outbox_sent(&record.idempotency_key, Some("msg_final_1"))
            .unwrap();
        let stored = state
            .load_runtime_final_outbox_by_run("run_1")
            .unwrap()
            .unwrap();
        assert_eq!(stored.status, "sent");
        assert_eq!(stored.attempt_count, 3);
        assert_eq!(stored.message_id.as_deref(), Some("msg_final_1"));
        assert!(stored.sent_at_ms.is_some());
        assert!(state
            .list_due_runtime_final_outbox(now + 60_000, 10)
            .unwrap()
            .is_empty());
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
            controller_user_id: "user-alice".to_string(),
            controller_full_handle: "alice.anpclaw.com".to_string(),
            controller_scope_key: "controller-scope:v1:test-alice".to_string(),
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
    fn runtime_daemon_binding_and_status_query_throttle_roundtrip() {
        let root = tempfile::tempdir().unwrap();
        let config = DaemonConfig::for_state_root(root.path()).unwrap();
        let state = DaemonState::open(&config).unwrap();
        state.initialize().unwrap();

        state
            .upsert_runtime_daemon_binding(
                "did:agent:runtime",
                "did:agent:daemon",
                "user-alice",
                "alice.anpclaw.com",
                "controller-scope:v1:test-alice",
                "did:human:alice",
            )
            .unwrap();

        assert!(state
            .runtime_agent_belongs_to_daemon_scope(
                "did:agent:runtime",
                "did:agent:daemon",
                "controller-scope:v1:test-alice",
            )
            .unwrap());
        assert!(!state
            .runtime_agent_belongs_to_daemon_scope(
                "did:agent:runtime",
                "did:agent:other-daemon",
                "controller-scope:v1:test-alice",
            )
            .unwrap());

        assert!(state
            .should_emit_agent_status_query_snapshot("did:agent:daemon", "did:human:alice", 10_000,)
            .unwrap());
        assert!(!state
            .should_emit_agent_status_query_snapshot("did:agent:daemon", "did:human:alice", 10_000,)
            .unwrap());
        assert!(state
            .should_emit_agent_status_query_snapshot("did:agent:daemon", "did:human:alice", 0,)
            .unwrap());
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
            "controller-scope:v1:test-alice",
            Some("direct:did:human:alice".to_string()),
            "conversation",
        );
        let session = HermesNativeSessionRecord::active(
            &route,
            "did:human:alice",
            "awiki_alice_hermes",
            "hermes-session-1",
        )
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

        let replacement = HermesNativeSessionRecord::active(
            &route,
            "did:human:alice",
            "awiki_alice_hermes",
            "hermes-session-2",
        )
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

fn user_delegated_identity_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<UserDelegatedIdentityRecord> {
    let allowed_scopes_json_raw: String = row.get(7)?;
    let allowed_scopes_json = serde_json::from_str(&allowed_scopes_json_raw).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(
            allowed_scopes_json_raw.len(),
            rusqlite::types::Type::Text,
            Box::new(err),
        )
    })?;
    Ok(UserDelegatedIdentityRecord {
        user_did: row.get(0)?,
        verification_method: row.get(1)?,
        app_instance_id: row.get(2)?,
        controller_did: row.get(3)?,
        daemon_agent_did: row.get(4)?,
        public_key_multibase: row.get(5)?,
        private_key_material: row.get(6)?,
        allowed_scopes_json,
        status: row.get(8)?,
        expires_at: row.get(9)?,
        bootstrap_id: row.get(10)?,
        idempotency_key: row.get(11)?,
        created_at_ms: row.get(12)?,
        updated_at_ms: row.get(13)?,
    })
}

fn bootstrap_replay_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<BootstrapReplayRecord> {
    Ok(BootstrapReplayRecord {
        bootstrap_id: row.get(0)?,
        idempotency_key: row.get(1)?,
        payload_hash: row.get(2)?,
        user_did: row.get(3)?,
        verification_method: row.get(4)?,
        app_instance_id: row.get(5)?,
        daemon_agent_did: row.get(6)?,
        status: row.get(7)?,
        created_at_ms: row.get(8)?,
        updated_at_ms: row.get(9)?,
    })
}

fn app_message_agent_binding_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<AppMessageAgentBindingRecord> {
    let desired_agent_json_raw: String = row.get(10)?;
    let desired_agent_json = serde_json::from_str(&desired_agent_json_raw).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(
            desired_agent_json_raw.len(),
            rusqlite::types::Type::Text,
            Box::new(err),
        )
    })?;
    let capability_policy_json_raw: String = row.get(11)?;
    let capability_policy_json =
        serde_json::from_str(&capability_policy_json_raw).map_err(|err| {
            rusqlite::Error::FromSqlConversionFailure(
                capability_policy_json_raw.len(),
                rusqlite::types::Type::Text,
                Box::new(err),
            )
        })?;
    Ok(AppMessageAgentBindingRecord {
        binding_id: row.get(0)?,
        user_did: row.get(1)?,
        inbox_auth_verification_method: row.get(2)?,
        app_instance_id: row.get(3)?,
        bootstrap_id: row.get(4)?,
        idempotency_key: row.get(5)?,
        daemon_agent_did: row.get(6)?,
        runtime_agent_did: row.get(7)?,
        runtime_profile_id: row.get(8)?,
        role: row.get(9)?,
        desired_agent_json,
        capability_policy_json,
        status: row.get(12)?,
        created_at_ms: row.get(13)?,
        updated_at_ms: row.get(14)?,
        revoked_at_ms: row.get(15)?,
    })
}

fn inbox_cursor_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<InboxCursorRecord> {
    Ok(InboxCursorRecord {
        owner_did: row.get(0)?,
        inbox_scope: row.get(1)?,
        cursor: row.get(2)?,
        updated_at_ms: row.get(3)?,
    })
}

fn processed_message_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ProcessedMessageRecord> {
    Ok(ProcessedMessageRecord {
        owner_did: row.get(0)?,
        message_id: row.get(1)?,
        schema: row.get(2)?,
        processed_at_ms: row.get(3)?,
        status: row.get(4)?,
    })
}

fn message_event_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<MessageEventRecord> {
    Ok(MessageEventRecord {
        event_id: row.get(0)?,
        owner_did: row.get(1)?,
        conversation_id: row.get(2)?,
        message_id: row.get(3)?,
        message_kind: row.get(4)?,
        sender_did: row.get(5)?,
        received_at: row.get(6)?,
        plain_text_ref_or_excerpt: row.get(7)?,
        content_hash: row.get(8)?,
        schema: row.get(9)?,
        processing_status: row.get(10)?,
        retention_class: row.get(11)?,
        created_at_ms: row.get(12)?,
        updated_at_ms: row.get(13)?,
    })
}

fn message_sync_outbox_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<MessageSyncOutboxRecord> {
    let payload_json_raw: String = row.get(3)?;
    let payload_json = serde_json::from_str(&payload_json_raw).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(
            payload_json_raw.len(),
            rusqlite::types::Type::Text,
            Box::new(err),
        )
    })?;
    Ok(MessageSyncOutboxRecord {
        idempotency_key: row.get(0)?,
        owner_did: row.get(1)?,
        app_instance_id: row.get(2)?,
        payload_json,
        status: row.get(4)?,
        attempt_count: row.get(5)?,
        next_attempt_at_ms: row.get(6)?,
        last_error_code: row.get(7)?,
        last_error_summary: row.get(8)?,
        created_at_ms: row.get(9)?,
        updated_at_ms: row.get(10)?,
        sent_at_ms: row.get(11)?,
    })
}

fn load_bootstrap_replay_by_id_or_key(
    connection: &Connection,
    bootstrap_id: &str,
    idempotency_key: &str,
) -> Result<Option<BootstrapReplayRecord>> {
    connection
        .query_row(
            r#"
SELECT
    bootstrap_id,
    idempotency_key,
    payload_hash,
    user_did,
    verification_method,
    app_instance_id,
    daemon_agent_did,
    status,
    created_at_ms,
    updated_at_ms
FROM bootstrap_replay
WHERE bootstrap_id = ?1 OR idempotency_key = ?2
ORDER BY created_at_ms ASC
LIMIT 1
"#,
            rusqlite::params![bootstrap_id, idempotency_key],
            bootstrap_replay_from_row,
        )
        .optional()
        .context("load bootstrap replay by id or idempotency key")
}
