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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CliRouteSessionRecord {
    pub route_key: String,
    pub route_key_hash: String,
    pub agent_did: String,
    pub runtime_profile_id: String,
    pub driver_id: String,
    pub controller_user_id: String,
    pub controller_full_handle: String,
    pub controller_scope_key: String,
    pub controller_did: String,
    pub conversation_id: String,
    pub workspace_path: PathBuf,
    pub session_dir: PathBuf,
    pub native_session_id: Option<String>,
    pub native_session_source: Option<String>,
    pub synthetic_session_id: Option<String>,
    pub status: String,
    pub last_run_id: Option<String>,
    pub last_message_id: Option<String>,
    pub lock_run_id: Option<String>,
    pub lock_owner: Option<String>,
    pub lock_expires_at_ms: Option<i64>,
    pub last_error_code: Option<String>,
    pub last_error_summary: Option<String>,
    pub version: i64,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CliRuntimeLockRecord {
    pub lock_key: String,
    pub lock_kind: String,
    pub runtime_profile_id: Option<String>,
    pub driver_id: Option<String>,
    pub run_id: String,
    pub lock_owner: String,
    pub lock_expires_at_ms: i64,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CliRouteMessageQueueRecord {
    pub queue_id: String,
    pub agent_did: String,
    pub runtime_profile_id: String,
    pub driver_id: String,
    pub controller_user_id: String,
    pub controller_full_handle: String,
    pub controller_scope_key: String,
    pub controller_did: String,
    pub conversation_id: String,
    pub route_key: String,
    pub route_key_hash: String,
    pub source_message_id: String,
    pub task_id: Option<String>,
    pub run_id: Option<String>,
    pub status: String,
    pub enqueue_reason: String,
    pub attempts: i64,
    pub next_attempt_at_ms: i64,
    pub route_sequence: i64,
    pub last_error_code: Option<String>,
    pub last_error_summary: Option<String>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliRouteMessageQueueSummary {
    pub queued_count: i64,
    pub running_count: i64,
    pub succeeded_count: i64,
    pub failed_count: i64,
    pub cancelled_count: i64,
    pub dead_letter_count: i64,
    pub due_queued_count: i64,
    pub due_route_count: i64,
    pub oldest_queued_age_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateCliRouteMessageQueueReference {
    pub agent_did: String,
    pub runtime_profile_id: String,
    pub driver_id: String,
    pub controller_user_id: String,
    pub controller_full_handle: String,
    pub controller_scope_key: String,
    pub controller_did: String,
    pub conversation_id: String,
    pub source_message_id: String,
    pub task_id: Option<String>,
    pub run_id: Option<String>,
    pub enqueue_reason: String,
    pub next_attempt_at_ms: i64,
    pub last_error_code: Option<String>,
    pub last_error_summary: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateCliRouteSession {
    pub agent_did: String,
    pub runtime_profile_id: String,
    pub driver_id: String,
    pub controller_user_id: String,
    pub controller_full_handle: String,
    pub controller_scope_key: String,
    pub controller_did: String,
    pub conversation_id: String,
    pub workspace_path: PathBuf,
    pub session_dir: PathBuf,
}

impl CliRuntimeLockRecord {
    pub fn validate(&self) -> Result<()> {
        for (field_name, value) in [
            ("lock_key", self.lock_key.as_str()),
            ("lock_kind", self.lock_kind.as_str()),
            ("run_id", self.run_id.as_str()),
            ("lock_owner", self.lock_owner.as_str()),
        ] {
            if value.trim().is_empty() {
                bail!("{field_name} must not be empty");
            }
        }
        if self
            .runtime_profile_id
            .as_deref()
            .is_some_and(|value| value.trim().is_empty())
        {
            bail!("runtime_profile_id must not be empty when present");
        }
        if self
            .driver_id
            .as_deref()
            .is_some_and(|value| value.trim().is_empty())
        {
            bail!("driver_id must not be empty when present");
        }
        Ok(())
    }
}

impl CliRouteMessageQueueRecord {
    pub fn validate(&self) -> Result<()> {
        validate_cli_route_message_queue_fields(
            &self.queue_id,
            &self.agent_did,
            &self.runtime_profile_id,
            &self.driver_id,
            &self.controller_user_id,
            &self.controller_full_handle,
            &self.controller_scope_key,
            &self.controller_did,
            &self.conversation_id,
            &self.route_key,
            &self.route_key_hash,
            &self.source_message_id,
            &self.status,
            &self.enqueue_reason,
            self.attempts,
            self.next_attempt_at_ms,
            self.route_sequence,
        )?;
        for (field_name, value) in [
            ("task_id", self.task_id.as_deref()),
            ("run_id", self.run_id.as_deref()),
            ("last_error_code", self.last_error_code.as_deref()),
            ("last_error_summary", self.last_error_summary.as_deref()),
        ] {
            if value.is_some_and(|value| value.trim().is_empty()) {
                bail!("{field_name} must not be empty when present");
            }
        }
        Ok(())
    }
}

impl CreateCliRouteMessageQueueReference {
    pub fn route_key(&self) -> Result<String> {
        cli_route_session_key(
            &self.agent_did,
            &self.controller_scope_key,
            &self.conversation_id,
        )
    }

    pub fn validate(&self) -> Result<()> {
        let conversation_id = canonical_cli_conversation_id(&self.conversation_id)?;
        let route_key = cli_route_session_key(
            &self.agent_did,
            &self.controller_scope_key,
            &conversation_id,
        )?;
        validate_cli_route_message_queue_fields(
            "queue_dry_run",
            &self.agent_did,
            &self.runtime_profile_id,
            &self.driver_id,
            &self.controller_user_id,
            &self.controller_full_handle,
            &self.controller_scope_key,
            &self.controller_did,
            &conversation_id,
            &route_key,
            "route_000000000000000000000000",
            &self.source_message_id,
            "queued",
            &self.enqueue_reason,
            0,
            self.next_attempt_at_ms,
            1,
        )?;
        for (field_name, value) in [
            ("task_id", self.task_id.as_deref()),
            ("run_id", self.run_id.as_deref()),
            ("last_error_code", self.last_error_code.as_deref()),
            ("last_error_summary", self.last_error_summary.as_deref()),
        ] {
            if value.is_some_and(|value| value.trim().is_empty()) {
                bail!("{field_name} must not be empty when present");
            }
        }
        Ok(())
    }
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

impl CliRouteSessionRecord {
    pub fn validate(&self) -> Result<()> {
        validate_cli_route_key(&self.route_key)?;
        validate_cli_route_key_hash(&self.route_key_hash)?;
        validate_cli_route_session_fields(
            &self.agent_did,
            &self.runtime_profile_id,
            &self.driver_id,
            &self.controller_user_id,
            &self.controller_full_handle,
            &self.controller_scope_key,
            &self.controller_did,
            &self.conversation_id,
            &self.workspace_path,
            &self.session_dir,
            &self.status,
        )?;
        for (field_name, value) in [
            ("native_session_id", self.native_session_id.as_deref()),
            (
                "native_session_source",
                self.native_session_source.as_deref(),
            ),
            ("synthetic_session_id", self.synthetic_session_id.as_deref()),
            ("last_run_id", self.last_run_id.as_deref()),
            ("last_message_id", self.last_message_id.as_deref()),
            ("lock_run_id", self.lock_run_id.as_deref()),
            ("lock_owner", self.lock_owner.as_deref()),
            ("last_error_code", self.last_error_code.as_deref()),
            ("last_error_summary", self.last_error_summary.as_deref()),
        ] {
            if value.is_some_and(|value| value.trim().is_empty()) {
                bail!("{field_name} must not be empty when present");
            }
        }
        Ok(())
    }
}

impl CreateCliRouteSession {
    pub fn route_key(&self) -> Result<String> {
        cli_route_session_key(
            &self.agent_did,
            &self.controller_scope_key,
            &self.conversation_id,
        )
    }

    pub fn into_record(self, route_key_hash: String) -> Result<CliRouteSessionRecord> {
        let route_key = self.route_key()?;
        validate_cli_route_key_hash(&route_key_hash)?;
        let conversation_id = canonical_cli_conversation_id(&self.conversation_id)?;
        let now = current_time_millis()?;
        let record = CliRouteSessionRecord {
            route_key,
            route_key_hash,
            agent_did: self.agent_did,
            runtime_profile_id: self.runtime_profile_id,
            driver_id: self.driver_id,
            controller_user_id: self.controller_user_id,
            controller_full_handle: self.controller_full_handle,
            controller_scope_key: self.controller_scope_key,
            controller_did: self.controller_did,
            conversation_id,
            workspace_path: self.workspace_path,
            session_dir: self.session_dir,
            native_session_id: None,
            native_session_source: None,
            synthetic_session_id: None,
            status: "active".to_string(),
            last_run_id: None,
            last_message_id: None,
            lock_run_id: None,
            lock_owner: None,
            lock_expires_at_ms: None,
            last_error_code: None,
            last_error_summary: None,
            version: 0,
            created_at_ms: now,
            updated_at_ms: now,
        };
        record.validate()?;
        Ok(record)
    }
}

pub fn cli_route_session_key(
    agent_did: &str,
    controller_scope_key: &str,
    conversation_id: &str,
) -> Result<String> {
    for (field_name, value) in [
        ("agent_did", agent_did),
        ("controller_scope_key", controller_scope_key),
        ("conversation_id", conversation_id),
    ] {
        if value.trim().is_empty() {
            bail!("{field_name} must not be empty");
        }
    }
    let conversation_id = canonical_cli_conversation_id(conversation_id)?;
    Ok(format!(
        "cli:{agent_did}:{controller_scope_key}:{conversation_id}:message-run"
    ))
}

pub fn canonical_cli_conversation_id(input: &str) -> Result<String> {
    let value = input.trim();
    if value.is_empty() {
        bail!("conversation_id must not be empty for a generic-cli route session");
    }
    if value == "no-conversation" {
        bail!("no-conversation cannot be used for a generic-cli route session");
    }
    if let Some(peer) = value
        .strip_prefix("direct:")
        .or_else(|| value.strip_prefix("dm:"))
    {
        let peer = peer.trim();
        if peer.is_empty() {
            bail!("direct conversation peer must not be empty");
        }
        return Ok(format!("direct:{peer}"));
    }
    if let Some(group) = value.strip_prefix("group:") {
        let group = group.trim();
        if group.is_empty() {
            bail!("group conversation id must not be empty");
        }
        return Ok(format!("group:{group}"));
    }
    if let Some(thread) = value.strip_prefix("thread:") {
        let thread = thread.trim();
        if thread.is_empty() {
            bail!("thread conversation id must not be empty");
        }
        return Ok(format!("thread:{thread}"));
    }
    if value.contains(':') {
        bail!("unsupported generic-cli conversation_id prefix");
    }
    Ok(format!("thread:{value}"))
}

pub fn cli_route_key_hash(route_key: &str) -> Result<String> {
    validate_cli_route_key(route_key)?;
    let digest = Sha256::digest(route_key.as_bytes());
    let short = digest
        .iter()
        .take(12)
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    Ok(format!("route_{short}"))
}

pub fn cli_route_key_hash_with_salt(route_key: &str, salt_hex: &str) -> Result<String> {
    validate_cli_route_key(route_key)?;
    let salt = decode_cli_route_hash_salt_hex(salt_hex)?;
    let digest = hmac_sha256(&salt, b"awiki-cli-route-key-v2", route_key.as_bytes());
    let short = digest
        .iter()
        .take(12)
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    Ok(format!("route_{short}"))
}

fn hmac_sha256(key: &[u8], domain: &[u8], message: &[u8]) -> [u8; 32] {
    const SHA256_BLOCK_SIZE: usize = 64;
    let mut block_key = [0u8; SHA256_BLOCK_SIZE];
    if key.len() > SHA256_BLOCK_SIZE {
        let digest = Sha256::digest(key);
        block_key[..32].copy_from_slice(&digest);
    } else {
        block_key[..key.len()].copy_from_slice(key);
    }

    let mut ipad = [0x36u8; SHA256_BLOCK_SIZE];
    let mut opad = [0x5cu8; SHA256_BLOCK_SIZE];
    for index in 0..SHA256_BLOCK_SIZE {
        ipad[index] ^= block_key[index];
        opad[index] ^= block_key[index];
    }

    let mut inner = Sha256::new();
    inner.update(ipad);
    inner.update(domain);
    inner.update([0u8]);
    inner.update(message);
    let inner_digest = inner.finalize();

    let mut outer = Sha256::new();
    outer.update(opad);
    outer.update(inner_digest);
    let digest = outer.finalize();
    let mut output = [0u8; 32];
    output.copy_from_slice(&digest);
    output
}

pub(crate) fn encode_cli_route_hash_salt_hex(salt: &[u8; 32]) -> String {
    salt.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub(crate) fn decode_cli_route_hash_salt_hex(input: &str) -> Result<[u8; 32]> {
    let value = input.trim();
    if value.len() != 64
        || !value
            .chars()
            .all(|ch| ch.is_ascii_digit() || ('a'..='f').contains(&ch))
    {
        bail!("route hash salt must use 32-byte lowercase hex format");
    }
    let mut salt = [0u8; 32];
    for index in 0..32 {
        let start = index * 2;
        salt[index] = u8::from_str_radix(&value[start..start + 2], 16)
            .map_err(|error| anyhow::anyhow!("invalid route hash salt hex: {error}"))?;
    }
    Ok(salt)
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecureBootstrapReplayRecord {
    pub operation_id: String,
    pub nonce: String,
    pub envelope_hash: String,
    pub recipient_daemon_did: String,
    pub recipient_key_id: String,
    pub sender_human_did: String,
    pub bootstrap_id: String,
    pub idempotency_key: String,
    pub payload_sha256: Option<String>,
    pub expires_at: String,
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

impl SecureBootstrapReplayRecord {
    pub fn validate(&self) -> Result<()> {
        for (field_name, value) in [
            ("operation_id", self.operation_id.as_str()),
            ("nonce", self.nonce.as_str()),
            ("envelope_hash", self.envelope_hash.as_str()),
            ("recipient_daemon_did", self.recipient_daemon_did.as_str()),
            ("recipient_key_id", self.recipient_key_id.as_str()),
            ("sender_human_did", self.sender_human_did.as_str()),
            ("bootstrap_id", self.bootstrap_id.as_str()),
            ("idempotency_key", self.idempotency_key.as_str()),
            ("expires_at", self.expires_at.as_str()),
            ("status", self.status.as_str()),
        ] {
            if value.trim().is_empty() {
                bail!("{field_name} must not be empty");
            }
        }
        if self.envelope_hash.len() != 64
            || !self.envelope_hash.chars().all(|ch| ch.is_ascii_hexdigit())
        {
            bail!("secure bootstrap envelope_hash must be a 64-character hex digest");
        }
        if let Some(payload_sha256) = self.payload_sha256.as_deref() {
            if payload_sha256.trim().is_empty() {
                bail!("payload_sha256 must not be empty when present");
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
    pub session_actor_did: String,
    pub conversation_id: Option<String>,
    pub session_kind: String,
}

impl HermesSessionRoute {
    pub fn new(
        agent_did: impl Into<String>,
        runtime_profile_id: impl Into<String>,
        controller_scope_key: impl Into<String>,
        session_actor_did: impl Into<String>,
        conversation_id: Option<String>,
        session_kind: impl Into<String>,
    ) -> Self {
        Self {
            agent_did: agent_did.into(),
            runtime_profile_id: runtime_profile_id.into(),
            controller_scope_key: controller_scope_key.into(),
            session_actor_did: session_actor_did.into(),
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
        if self.session_actor_did.trim().is_empty() {
            bail!("session_actor_did must not be empty");
        }
        if self.session_kind.trim().is_empty() {
            bail!("session_kind must not be empty");
        }
        Ok(())
    }

    pub fn route_key(&self) -> String {
        format!(
            "hermes:{}:{}:{}:{}:{}",
            self.agent_did,
            self.controller_scope_key,
            self.session_actor_did,
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
    pub session_actor_did: String,
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
    pub next_attempt_at_ms: i64,
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
    pub recipient_did: String,
    pub conversation_id: Option<String>,
    pub final_text: String,
    pub final_source: String,
    pub final_body_hash: String,
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
        if self.attempts < 0 {
            bail!("attempts must not be negative");
        }
        if self.next_attempt_at_ms < 0 {
            bail!("next_attempt_at_ms must not be negative");
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
            ("recipient_did", self.recipient_did.as_str()),
            ("final_text", self.final_text.as_str()),
            ("final_source", self.final_source.as_str()),
            ("security", self.security.as_str()),
            ("status", self.status.as_str()),
        ] {
            if value.trim().is_empty() {
                bail!("{field_name} must not be empty");
            }
        }
        if let Some(hash) = self.final_body_hash.strip_prefix("sha256:") {
            if hash.len() != 64
                || !hash
                    .chars()
                    .all(|ch| ch.is_ascii_hexdigit() && !ch.is_ascii_uppercase())
            {
                bail!("final_body_hash must use sha256:<64 lowercase hex> format");
            }
        } else if !self.final_body_hash.is_empty() {
            bail!("final_body_hash must use sha256:<64 lowercase hex> format");
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
            session_actor_did: route.session_actor_did.clone(),
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
        if self.controller_did.trim().is_empty() {
            bail!("controller_did must not be empty");
        }
        if self.session_actor_did.trim().is_empty() {
            bail!("session_actor_did must not be empty");
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

pub(crate) fn validate_cli_route_key(route_key: &str) -> Result<()> {
    let route_key = route_key.trim();
    if route_key.is_empty() {
        bail!("route_key must not be empty");
    }
    if route_key.contains(":no-conversation:") {
        bail!("route_key must not use no-conversation");
    }
    if !route_key.starts_with("cli:") || !route_key.ends_with(":message-run") {
        bail!("route_key must use generic-cli message-run format");
    }
    Ok(())
}

pub(crate) fn validate_cli_route_key_hash(route_key_hash: &str) -> Result<()> {
    let value = route_key_hash.trim();
    if !value.starts_with("route_") || value.len() != "route_".len() + 24 {
        bail!("route_key_hash must use route_<24 hex> format");
    }
    if !value["route_".len()..]
        .chars()
        .all(|ch| ch.is_ascii_hexdigit())
    {
        bail!("route_key_hash contains unsupported characters");
    }
    Ok(())
}

pub(crate) fn validate_cli_route_session_fields(
    agent_did: &str,
    runtime_profile_id: &str,
    driver_id: &str,
    controller_user_id: &str,
    controller_full_handle: &str,
    controller_scope_key: &str,
    controller_did: &str,
    conversation_id: &str,
    workspace_path: &std::path::Path,
    session_dir: &std::path::Path,
    status: &str,
) -> Result<()> {
    for (field_name, value) in [
        ("agent_did", agent_did),
        ("runtime_profile_id", runtime_profile_id),
        ("driver_id", driver_id),
        ("controller_user_id", controller_user_id),
        ("controller_full_handle", controller_full_handle),
        ("controller_scope_key", controller_scope_key),
        ("controller_did", controller_did),
        ("conversation_id", conversation_id),
        ("status", status),
    ] {
        if value.trim().is_empty() {
            bail!("{field_name} must not be empty");
        }
    }
    canonical_cli_conversation_id(conversation_id)?;
    if workspace_path.as_os_str().is_empty() {
        bail!("workspace_path must not be empty");
    }
    if session_dir.as_os_str().is_empty() {
        bail!("session_dir must not be empty");
    }
    Ok(())
}

pub(crate) fn validate_cli_route_message_queue_fields(
    queue_id: &str,
    agent_did: &str,
    runtime_profile_id: &str,
    driver_id: &str,
    controller_user_id: &str,
    controller_full_handle: &str,
    controller_scope_key: &str,
    controller_did: &str,
    conversation_id: &str,
    route_key: &str,
    route_key_hash: &str,
    source_message_id: &str,
    status: &str,
    enqueue_reason: &str,
    attempts: i64,
    next_attempt_at_ms: i64,
    route_sequence: i64,
) -> Result<()> {
    for (field_name, value) in [
        ("queue_id", queue_id),
        ("agent_did", agent_did),
        ("runtime_profile_id", runtime_profile_id),
        ("driver_id", driver_id),
        ("controller_user_id", controller_user_id),
        ("controller_full_handle", controller_full_handle),
        ("controller_scope_key", controller_scope_key),
        ("controller_did", controller_did),
        ("conversation_id", conversation_id),
        ("route_key", route_key),
        ("route_key_hash", route_key_hash),
        ("source_message_id", source_message_id),
        ("status", status),
        ("enqueue_reason", enqueue_reason),
    ] {
        if value.trim().is_empty() {
            bail!("{field_name} must not be empty");
        }
    }
    if !queue_id
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-'))
    {
        bail!("queue_id contains unsupported characters");
    }
    if !matches!(
        status,
        "queued" | "running" | "succeeded" | "failed" | "cancelled" | "dead_letter"
    ) {
        bail!("unsupported cli route message queue status: {status}");
    }
    if !matches!(driver_id, "codex" | "claude-code") {
        bail!("unsupported cli route message queue driver: {driver_id}");
    }
    canonical_cli_conversation_id(conversation_id)?;
    validate_cli_route_key(route_key)?;
    validate_cli_route_key_hash(route_key_hash)?;
    if attempts < 0 {
        bail!("attempts must not be negative");
    }
    if next_attempt_at_ms < 0 {
        bail!("next_attempt_at_ms must not be negative");
    }
    if route_sequence <= 0 {
        bail!("route_sequence must be positive");
    }
    Ok(())
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
