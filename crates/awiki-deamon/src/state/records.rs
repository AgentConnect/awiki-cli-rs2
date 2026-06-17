use std::path::PathBuf;

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::security::runtime_token::current_time_millis;
use crate::workspace::WorkspaceMode;

pub(super) const DEFAULT_CLI_RECIPIENT_POLICY_JSON: &str = r#"{"mode":"controller-only"}"#;
const DEFAULT_CLI_DRIVER_CONFIG_JSON: &str = "{}";

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

pub(super) fn validate_control_command_status(status: &str) -> Result<()> {
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

pub(super) fn stable_id_suffix(input: &str) -> String {
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
