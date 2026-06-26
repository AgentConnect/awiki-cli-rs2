use std::collections::BTreeMap;

use anyhow::{bail, Context, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde_json::{json, Value};
use time::format_description::well_known::Rfc3339;

use crate::agent::{AgentDefinition, AgentKind};
use crate::outbox::{AgentManagementOutbox, AgentStatusResponse};
use crate::plugins::generic_cli::{GenericCliDriverRegistry, GENERIC_CLI_RUNTIME_PLUGIN_ID};
use crate::plugins::hermes::{
    ensure_runtime_model_config, hermes_runtime_model_config_status,
    repair_hermes_profile_if_needed, HermesGatewayCommandStatus, HermesRuntimeModelConfigStatus,
    StdioHermesGateway,
};
use crate::registration::{
    AgentInventoryClient, AgentLatestStatusUpdateItem, DidAuthMaterial,
    UserServiceAgentRegistrationClient,
};
use crate::runtime::RuntimePlugin;
use crate::security::runtime_token::current_time_millis;
use crate::service::{manage_service, ServiceAction, ServicePlatform, ServiceStatus};
use crate::state::{CliRuntimeProfileRecord, DaemonState};
use crate::upgrade::{check_release_status, DaemonReleaseStatus};
use crate::{DaemonConfig, ImCoreAdapter};

pub const IDLE_HEARTBEAT_MS: i64 = 5 * 60 * 1000;
pub const ACTIVE_HEARTBEAT_MS: i64 = 30 * 1000;
pub const APP_ATTENTION_WINDOW_MS: i64 = 2 * 60 * 1000;
pub const LATEST_STATUS_CHECK_MS: i64 = 10 * 1000;
pub const RELEASE_STATUS_CHECK_MS: i64 = 5 * 60 * 1000;
const GENERIC_CLI_SETUP_PROBE_TTL_MS: i64 = 5 * 60 * 1000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeartbeatScheduler {
    last_control_emit_at_ms: Option<i64>,
    last_latest_check_at_ms: Option<i64>,
    last_release_check_at_ms: Option<i64>,
    last_release_status: Option<DaemonReleaseStatus>,
    last_user_service_write_at_ms_by_daemon: BTreeMap<String, i64>,
    last_status_signature_by_daemon: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeartbeatOutcome {
    pub emitted_control: bool,
    pub wrote_user_service: bool,
    pub active: bool,
}

impl HeartbeatScheduler {
    pub fn new() -> Self {
        Self {
            last_control_emit_at_ms: None,
            last_latest_check_at_ms: None,
            last_release_check_at_ms: None,
            last_release_status: None,
            last_user_service_write_at_ms_by_daemon: BTreeMap::new(),
            last_status_signature_by_daemon: BTreeMap::new(),
        }
    }

    fn release_status(&mut self, config: &DaemonConfig, now: i64) -> DaemonReleaseStatus {
        let release_due = self
            .last_release_check_at_ms
            .map(|last| now.saturating_sub(last) >= RELEASE_STATUS_CHECK_MS)
            .unwrap_or(true);
        if release_due || self.last_release_status.is_none() {
            let release = check_release_status(config);
            self.last_release_check_at_ms = Some(now);
            self.last_release_status = Some(release.clone());
            release
        } else {
            self.last_release_status
                .clone()
                .unwrap_or_else(|| check_release_status(config))
        }
    }

    pub fn tick<O>(
        &mut self,
        config: &DaemonConfig,
        state: &DaemonState,
        im_core: &ImCoreAdapter,
        outbox: &O,
    ) -> Result<HeartbeatOutcome>
    where
        O: AgentManagementOutbox,
    {
        let now = current_time_millis()?;
        let active = has_running_runs(state)? || app_attention_active(state, now)?;
        let interval = if active {
            ACTIVE_HEARTBEAT_MS
        } else {
            IDLE_HEARTBEAT_MS
        };
        let control_due = self
            .last_control_emit_at_ms
            .map(|last| now.saturating_sub(last) >= interval)
            .unwrap_or(true);
        let latest_check_due = self
            .last_latest_check_at_ms
            .map(|last| now.saturating_sub(last) >= LATEST_STATUS_CHECK_MS)
            .unwrap_or(true);
        if !control_due && !latest_check_due {
            return Ok(HeartbeatOutcome {
                emitted_control: false,
                wrote_user_service: false,
                active,
            });
        }

        let daemon_agents = state
            .list_agent_definitions()?
            .into_iter()
            .filter(|agent| agent.agent_kind == AgentKind::Daemon)
            .collect::<Vec<_>>();
        let mut emitted = false;
        let mut wrote_latest = false;
        for daemon in daemon_agents {
            let release = self.release_status(config, now);
            reconcile_daemon_upgrade_state(state, &daemon, &release)?;
            if control_due {
                if let Err(error) =
                    emit_daemon_heartbeat(config, state, im_core, outbox, &daemon, &release)
                {
                    record_status_error(
                        state,
                        &daemon,
                        "daemon.status.heartbeat.control_failed",
                        &error.to_string(),
                    )?;
                } else {
                    emitted = true;
                }
            }

            if !latest_check_due {
                continue;
            }
            let latest_items =
                latest_status_items_with_release(config, state, &daemon, now, &release)?;
            let signature = latest_signature(&latest_items);
            let signature_changed = self
                .last_status_signature_by_daemon
                .get(&daemon.agent_did)
                .map(|last| last != &signature)
                .unwrap_or(true);
            let write_interval_due = self
                .last_user_service_write_at_ms_by_daemon
                .get(&daemon.agent_did)
                .map(|last| now.saturating_sub(*last) >= interval)
                .unwrap_or(true);
            let should_write_latest = signature_changed
                || self
                    .last_user_service_write_at_ms_by_daemon
                    .get(&daemon.agent_did)
                    .is_none()
                || write_interval_due;
            if should_write_latest {
                match update_user_service_latest(config, state, &daemon, latest_items) {
                    Ok(()) => {
                        self.last_user_service_write_at_ms_by_daemon
                            .insert(daemon.agent_did.clone(), now);
                        self.last_status_signature_by_daemon
                            .insert(daemon.agent_did.clone(), signature);
                        wrote_latest = true;
                    }
                    Err(error) => {
                        record_status_error(
                            state,
                            &daemon,
                            "daemon.status.heartbeat.latest_failed",
                            &error.to_string(),
                        )?;
                    }
                }
            }
        }
        if control_due {
            self.last_control_emit_at_ms = Some(now);
        }
        if latest_check_due {
            self.last_latest_check_at_ms = Some(now);
        }
        Ok(HeartbeatOutcome {
            emitted_control: emitted,
            wrote_user_service: wrote_latest,
            active,
        })
    }
}

impl Default for HeartbeatScheduler {
    fn default() -> Self {
        Self::new()
    }
}

pub fn daemon_snapshot_payload(
    config: &DaemonConfig,
    state: &DaemonState,
    daemon: &AgentDefinition,
) -> Result<Value> {
    let now = rfc3339_now();
    let service = service_status(config);
    let release = check_release_status(config);
    reconcile_daemon_upgrade_state(state, daemon, &release)?;
    let runtimes = state
        .list_runtime_agent_definitions_for_daemon(&daemon.agent_did)?
        .into_iter()
        .map(|agent| runtime_status_payload(config, state, daemon, agent, &now))
        .collect::<Result<Vec<_>>>()?;
    Ok(json!({
        "command": "agent.status.query",
        "daemon_agent_did": daemon.agent_did,
        "daemon": daemon_status_payload(
            config,
            daemon,
            &service,
            &now,
            &release,
            daemon_bootstrap_key_summary(state, daemon).as_ref(),
        ),
        "runtimes": runtimes,
        "runs": [],
    }))
}

pub fn daemon_lightweight_payload(config: &DaemonConfig, daemon: &AgentDefinition) -> Value {
    let now = rfc3339_now();
    let service = service_status(config);
    let release = check_release_status(config);
    daemon_lightweight_payload_with_release(config, daemon, &service, &now, &release)
}

fn daemon_lightweight_payload_with_release(
    config: &DaemonConfig,
    daemon: &AgentDefinition,
    service: &ServiceStatus,
    now: &str,
    release: &DaemonReleaseStatus,
) -> Value {
    json!({
        "schema": "awiki.agent.status.v1",
        "event_id": format!("evt_{}", current_time_millis().unwrap_or(0)),
        "sent_at": now,
        "daemon_agent_did": daemon.agent_did,
        "status_scope": "daemon",
        "command_id": null,
        "state": "ready",
        "message": "daemon heartbeat",
        "daemon": daemon_status_payload(config, daemon, service, now, release, None),
        "runtimes": [],
        "runs": [],
        "details": {
            "daemon_agent_did": daemon.agent_did,
            "status": "ready",
        },
        "result": {
            "daemon_agent_did": daemon.agent_did,
            "status": "ready",
        },
    })
}

pub fn latest_status_items(
    config: &DaemonConfig,
    state: &DaemonState,
    daemon: &AgentDefinition,
    now_ms: i64,
) -> Result<Vec<AgentLatestStatusUpdateItem>> {
    let release = check_release_status(config);
    reconcile_daemon_upgrade_state(state, daemon, &release)?;
    latest_status_items_with_release(config, state, daemon, now_ms, &release)
}

fn latest_status_items_with_release(
    config: &DaemonConfig,
    state: &DaemonState,
    daemon: &AgentDefinition,
    now_ms: i64,
    release: &DaemonReleaseStatus,
) -> Result<Vec<AgentLatestStatusUpdateItem>> {
    let last_seen_at = Some(rfc3339_from_millis(now_ms));
    let service = service_status(config);
    let mut items = vec![AgentLatestStatusUpdateItem {
        agent_did: daemon.agent_did.clone(),
        agent_kind: AgentKind::Daemon,
        status: if release.needs_upgrade {
            "needs_upgrade"
        } else {
            "ready"
        }
        .to_string(),
        last_seen_at: last_seen_at.clone(),
        version: Some(release.current_version.clone()),
        latest_version: release.latest_version.clone(),
        min_supported_version: None,
        platform: Some(crate::service::current_platform_label()),
        service: Some(service_label(service.platform).to_string()),
        needs_upgrade: release.needs_upgrade,
        needs_config: false,
        last_error_code: None,
        last_error_summary: None,
        diagnostics_summary: daemon_diagnostics_summary(
            &service,
            release,
            daemon_bootstrap_key_summary(state, daemon).as_ref(),
        ),
    }];
    for runtime in state.list_runtime_agent_definitions_for_daemon(&daemon.agent_did)? {
        let runtime_status = runtime_status_summary(config, state, &runtime);
        items.push(AgentLatestStatusUpdateItem {
            agent_did: runtime.agent_did.clone(),
            agent_kind: AgentKind::Runtime,
            status: if runtime_status.needs_config {
                "needs_config"
            } else {
                "ready"
            }
            .to_string(),
            last_seen_at: last_seen_at.clone(),
            version: None,
            latest_version: None,
            min_supported_version: None,
            platform: None,
            service: None,
            needs_upgrade: false,
            needs_config: runtime_status.needs_config,
            last_error_code: runtime_status.last_error_code.clone(),
            last_error_summary: None,
            diagnostics_summary: runtime_diagnostics_summary(state, &runtime, &runtime_status),
        });
    }
    Ok(items)
}

pub fn daemon_latest_diagnostics_summary(
    _config: &DaemonConfig,
    state: &DaemonState,
    daemon: &AgentDefinition,
    service: &ServiceStatus,
    release: &DaemonReleaseStatus,
) -> Value {
    daemon_diagnostics_summary(
        service,
        release,
        daemon_bootstrap_key_summary(state, daemon).as_ref(),
    )
}

pub fn update_user_service_latest(
    config: &DaemonConfig,
    state: &DaemonState,
    daemon: &AgentDefinition,
    items: Vec<AgentLatestStatusUpdateItem>,
) -> Result<()> {
    let client = UserServiceAgentRegistrationClient::new(&config.user_service_base_url)?;
    let auth_paths = crate::im_core_adapter::agent_identity_auth_paths(config, &daemon.agent_did);
    let auth = DidAuthMaterial {
        did_document_path: auth_paths.0,
        private_key_path: auth_paths.1,
        bearer_token: state.load_agent_auth_token(&daemon.agent_did)?,
    };
    let response = client.update_latest_status(&daemon.agent_did, items, &auth)?;
    sync_controller_scope_from_response(state, &daemon.agent_did, &response)?;
    Ok(())
}

pub fn sync_controller_scope_from_response(
    state: &DaemonState,
    daemon_agent_did: &str,
    response: &Value,
) -> Result<()> {
    let Some(controller) = controller_scope_from_response(daemon_agent_did, response) else {
        return Ok(());
    };
    let local = state.load_agent_definition(daemon_agent_did)?;
    if controller
        .controller_user_id
        .as_deref()
        .is_some_and(|value| value != local.controller_user_id)
        || controller
            .controller_full_handle
            .as_deref()
            .is_some_and(|value| value != local.controller_full_handle)
    {
        state.insert_audit_event_json(
            "daemon.controller_scope_mismatch",
            Some(daemon_agent_did),
            None,
            None,
            None,
            json!({
                "local_controller_user_id": local.controller_user_id,
                "local_controller_full_handle": local.controller_full_handle,
                "remote_controller_user_id": controller.controller_user_id,
                "remote_controller_full_handle": controller.controller_full_handle,
            }),
        )?;
        bail!("controller_scope_mismatch");
    }
    if local.controller_did == controller.controller_did {
        return Ok(());
    }
    state.update_controller_did_for_agent_family(daemon_agent_did, &controller.controller_did)?;
    state.insert_audit_event_json(
        "daemon.controller_did.synced",
        Some(daemon_agent_did),
        None,
        None,
        None,
        json!({
            "old_controller_did": local.controller_did,
            "new_controller_did": controller.controller_did,
        }),
    )?;
    Ok(())
}

pub fn sync_controller_did_from_latest_response(
    state: &DaemonState,
    daemon_agent_did: &str,
    response: &Value,
) -> Result<()> {
    sync_controller_scope_from_response(state, daemon_agent_did, response)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ControllerScopeSyncPayload {
    controller_user_id: Option<String>,
    controller_full_handle: Option<String>,
    controller_did: String,
}

fn controller_scope_from_response(
    daemon_agent_did: &str,
    response: &Value,
) -> Option<ControllerScopeSyncPayload> {
    let item = response
        .get("updated")
        .and_then(Value::as_array)
        .and_then(|items| {
            items.iter().find(|item| {
                item.get("agent_did")
                    .and_then(Value::as_str)
                    .map(|did| did == daemon_agent_did)
                    .unwrap_or(false)
            })
        })
        .unwrap_or(response);
    let controller_did = item
        .get("controller_did")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)?;
    Some(ControllerScopeSyncPayload {
        controller_user_id: optional_nonempty_string(item, "controller_user_id"),
        controller_full_handle: optional_nonempty_string(item, "controller_full_handle"),
        controller_did,
    })
}

fn optional_nonempty_string(value: &Value, field: &str) -> Option<String> {
    value
        .get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
}

fn emit_daemon_heartbeat<O>(
    config: &DaemonConfig,
    state: &DaemonState,
    im_core: &ImCoreAdapter,
    outbox: &O,
    daemon: &AgentDefinition,
    release: &DaemonReleaseStatus,
) -> Result<()>
where
    O: AgentManagementOutbox,
{
    let identity = state.load_agent_identity(&daemon.agent_did)?;
    let jwt_token = state.load_agent_auth_token(&daemon.agent_did)?;
    let _client = im_core.client_for_agent_identity(config, &identity, jwt_token.as_deref())?;
    outbox.send_agent_status(&AgentStatusResponse {
        conversation_id: None,
        agent_did: daemon.agent_did.clone(),
        recipient_did: daemon.controller_did.clone(),
        payload: {
            let service = service_status(config);
            let now = rfc3339_now();
            daemon_lightweight_payload_with_release(config, daemon, &service, &now, release)
        },
    })
}

fn runtime_status_payload(
    config: &DaemonConfig,
    state: &DaemonState,
    daemon: &AgentDefinition,
    runtime: AgentDefinition,
    now: &str,
) -> Result<Value> {
    let runtime_status = runtime_status_summary(config, state, &runtime);
    Ok(json!({
        "agent_did": runtime.agent_did,
        "daemon_agent_did": daemon.agent_did,
        "runtime": runtime_name_from_plugin(runtime.runtime_plugin_id.as_deref()),
        "runtime_profile_id": runtime.runtime_profile_id,
        "status": if runtime_status.needs_config { "needs_config" } else { "ready" },
        "last_seen_at": now,
        "needs_config": runtime_status.needs_config,
        "last_error_code": runtime_status.last_error_code,
        "last_error_summary": null,
        "diagnostics_summary": runtime_diagnostics_summary(state, &runtime, &runtime_status),
    }))
}

fn daemon_status_payload(
    _config: &DaemonConfig,
    daemon: &AgentDefinition,
    service: &ServiceStatus,
    now: &str,
    release: &DaemonReleaseStatus,
    bootstrap_key: Option<&DaemonBootstrapKeySummary>,
) -> Value {
    json!({
        "agent_did": daemon.agent_did,
        "status": if release.needs_upgrade { "needs_upgrade" } else { "ready" },
        "last_seen_at": now,
        "version": release.current_version.clone(),
        "latest_version": release.latest_version.clone(),
        "min_supported_version": null,
        "platform": crate::service::current_platform_label(),
        "service": service_label(service.platform),
        "needs_upgrade": release.needs_upgrade,
        "last_error_code": null,
        "last_error_summary": null,
        "diagnostics_summary": daemon_diagnostics_summary(service, release, bootstrap_key),
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DaemonBootstrapKeySummary {
    key_id: String,
    public_key_multibase: String,
    public_key_b64u: String,
    key_algorithm: String,
}

fn daemon_diagnostics_summary(
    service: &ServiceStatus,
    release: &DaemonReleaseStatus,
    bootstrap_key: Option<&DaemonBootstrapKeySummary>,
) -> Value {
    let mut config_summary = json!({
        "service_installed": service.installed,
        "release_manifest_url": release.manifest_url.clone(),
        "release_status": if release.error.is_some() { "unavailable" } else { "ok" },
        "release_error": release.error.clone(),
        "bootstrap_key_status": if bootstrap_key.is_some() { "ready" } else { "missing" },
        "generic_cli": generic_cli_daemon_capability_summary(),
    });
    if let Some(bootstrap_key) = bootstrap_key {
        if let Some(object) = config_summary.as_object_mut() {
            object.insert(
                "bootstrap_key_id".to_string(),
                Value::String(bootstrap_key.key_id.clone()),
            );
            object.insert(
                "bootstrap_public_key_multibase".to_string(),
                Value::String(bootstrap_key.public_key_multibase.clone()),
            );
            object.insert(
                "bootstrap_public_key_b64u".to_string(),
                Value::String(bootstrap_key.public_key_b64u.clone()),
            );
            object.insert(
                "bootstrap_key_algorithm".to_string(),
                Value::String(bootstrap_key.key_algorithm.clone()),
            );
        }
    }
    json!({
        "installation_status": if service.installed { "installed" } else { "not_installed" },
        "runner_status": if service.running { "running" } else { "not_running" },
        "config_summary": config_summary,
    })
}

pub fn reconcile_daemon_upgrade_state(
    state: &DaemonState,
    daemon: &AgentDefinition,
    release: &DaemonReleaseStatus,
) -> Result<()> {
    if release.error.is_some() || release.latest_version.is_none() {
        return Ok(());
    }
    state.reconcile_daemon_upgrade_commands(
        &daemon.agent_did,
        &daemon.controller_scope_key,
        &release.current_version,
        release.latest_version.as_deref(),
        release.needs_upgrade,
    )
}

fn daemon_bootstrap_key_summary(
    state: &DaemonState,
    daemon: &AgentDefinition,
) -> Option<DaemonBootstrapKeySummary> {
    let identity = state.load_agent_identity(&daemon.agent_did).ok()?;
    daemon_bootstrap_key_summary_from_did_document(&identity.did_document, &daemon.agent_did)
        .ok()
        .flatten()
}

fn daemon_bootstrap_key_summary_from_did_document(
    did_document: &Value,
    daemon_agent_did: &str,
) -> Result<Option<DaemonBootstrapKeySummary>> {
    let expected_key_id = format!(
        "{}#{}",
        daemon_agent_did.trim(),
        anp::authentication::VM_KEY_E2EE_AGREEMENT
    );
    let Some(methods) = did_document
        .get("verificationMethod")
        .and_then(Value::as_array)
    else {
        return Ok(None);
    };
    let Some(method) = methods.iter().find(|method| {
        method.get("id").and_then(Value::as_str).map(str::trim) == Some(expected_key_id.as_str())
    }) else {
        return Ok(None);
    };
    let public_key_multibase = method
        .get("publicKeyMultibase")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .context("daemon bootstrap key is missing publicKeyMultibase")?
        .to_string();
    let bytes = x25519_public_key_bytes_from_multibase(&public_key_multibase)
        .context("extract daemon bootstrap public key")?;
    Ok(Some(DaemonBootstrapKeySummary {
        key_id: expected_key_id,
        public_key_multibase,
        public_key_b64u: URL_SAFE_NO_PAD.encode(bytes),
        key_algorithm: "x25519".to_string(),
    }))
}

fn x25519_public_key_bytes_from_multibase(value: &str) -> Result<[u8; 32]> {
    let encoded = value
        .trim()
        .strip_prefix('z')
        .context("daemon bootstrap key must use base58btc multibase")?;
    let mut bytes = bs58::decode(encoded)
        .into_vec()
        .context("decode daemon bootstrap public key multibase")?;
    if bytes.len() == 34 && bytes.starts_with(&[0xec, 0x01]) {
        bytes = bytes[2..].to_vec();
    }
    let bytes: [u8; 32] = bytes
        .try_into()
        .map_err(|_| anyhow::anyhow!("daemon bootstrap public key must be 32 bytes"))?;
    Ok(bytes)
}

pub fn reconcile_daemon_upgrade_state_from_release_status(
    config: &DaemonConfig,
    state: &DaemonState,
    daemon: &AgentDefinition,
) -> Result<()> {
    let release = check_release_status(config);
    reconcile_daemon_upgrade_state(state, daemon, &release)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RuntimeStatusSummary {
    is_hermes: bool,
    needs_config: bool,
    last_error_code: Option<String>,
    gateway_command_status: Option<HermesGatewayCommandStatus>,
    model_config_status: Option<HermesRuntimeModelConfigStatus>,
}

fn runtime_status_summary(
    config: &DaemonConfig,
    state: &DaemonState,
    runtime: &AgentDefinition,
) -> RuntimeStatusSummary {
    runtime_status_summary_with_gateway_status(config, state, runtime, || {
        hermes_gateway_command_status(config)
    })
}

fn hermes_gateway_command_status(config: &DaemonConfig) -> HermesGatewayCommandStatus {
    let status = StdioHermesGateway::from_config_without_detection(config).gateway_command_status();
    if status != HermesGatewayCommandStatus::Missing {
        return status;
    }
    let Ok(Some(_detected)) = StdioHermesGateway::ensure_detected_config(config) else {
        return status;
    };
    StdioHermesGateway::from_config_without_detection(config).gateway_command_status()
}

fn runtime_status_summary_with_gateway_status(
    config: &DaemonConfig,
    state: &DaemonState,
    runtime: &AgentDefinition,
    gateway_status: impl FnOnce() -> HermesGatewayCommandStatus,
) -> RuntimeStatusSummary {
    let is_hermes = runtime.runtime_plugin_id.as_deref()
        == Some(crate::plugins::hermes::HERMES_RUNTIME_PLUGIN_ID);
    if !is_hermes {
        if runtime.runtime_plugin_id.as_deref() == Some(GENERIC_CLI_RUNTIME_PLUGIN_ID) {
            let (needs_config, last_error_code) = generic_cli_runtime_status(state, runtime);
            return RuntimeStatusSummary {
                is_hermes,
                needs_config,
                last_error_code,
                gateway_command_status: None,
                model_config_status: None,
            };
        }
        return RuntimeStatusSummary {
            is_hermes,
            needs_config: false,
            last_error_code: None,
            gateway_command_status: None,
            model_config_status: None,
        };
    }
    let (profile_needs_config, model_config_status) = match state
        .load_hermes_profile(&runtime.agent_did)
    {
        Ok(profile) => {
            let profile = if let Ok(runtime_profile) =
                state.load_runtime_agent_profile(&runtime.agent_did)
            {
                repair_hermes_profile_if_needed(config, state, &runtime_profile, &runtime.handle)
                    .ok()
                    .flatten()
                    .map(|result| result.record)
                    .unwrap_or(profile)
            } else {
                profile
            };
            let _ = ensure_runtime_model_config(&profile.hermes_home);
            let model_config_status = hermes_runtime_model_config_status(&profile.hermes_home);
            (
                profile.status != "ready" || model_config_status.needs_config(),
                Some(model_config_status),
            )
        }
        Err(_) => (true, None),
    };
    let gateway_command_status = gateway_status();
    let last_error_code = gateway_command_status
        .error_code()
        .or_else(|| model_config_status.and_then(HermesRuntimeModelConfigStatus::error_code))
        .map(str::to_string);
    RuntimeStatusSummary {
        is_hermes,
        needs_config: profile_needs_config || gateway_command_status.needs_config(),
        last_error_code,
        gateway_command_status: Some(gateway_command_status),
        model_config_status,
    }
}

fn runtime_diagnostics_summary(
    state: &DaemonState,
    runtime: &AgentDefinition,
    runtime_status: &RuntimeStatusSummary,
) -> Value {
    if runtime.runtime_plugin_id.as_deref()
        != Some(crate::plugins::hermes::HERMES_RUNTIME_PLUGIN_ID)
    {
        if runtime.runtime_plugin_id.as_deref() == Some(GENERIC_CLI_RUNTIME_PLUGIN_ID) {
            return generic_cli_diagnostics_summary(state, runtime);
        }
        return json!({});
    }
    match state.load_hermes_profile(&runtime.agent_did) {
        Ok(profile) => json!({
            "profile_status": profile.status,
            "runtime_version": profile.hermes_version,
            "config_summary": {
                "awiki_skills_version": profile.awiki_skills_version,
                "gateway_command": runtime_status
                    .gateway_command_status
                    .map(HermesGatewayCommandStatus::as_str)
                    .unwrap_or("unknown"),
                "model_config": runtime_status
                    .model_config_status
                    .map(HermesRuntimeModelConfigStatus::as_str)
                    .unwrap_or("unknown"),
            },
        }),
        Err(_) => json!({
            "profile_status": "missing",
            "config_summary": {
                "gateway_command": runtime_status
                    .gateway_command_status
                    .map(HermesGatewayCommandStatus::as_str)
                    .unwrap_or("unknown"),
                "model_config": runtime_status
                    .model_config_status
                    .map(HermesRuntimeModelConfigStatus::as_str)
                    .unwrap_or("unknown"),
            },
        }),
    }
}

fn generic_cli_runtime_status(
    state: &DaemonState,
    runtime: &AgentDefinition,
) -> (bool, Option<String>) {
    let Some(runtime_profile_id) = runtime.runtime_profile_id.as_deref() else {
        return (true, Some("generic_cli_profile_missing".to_string()));
    };
    let Ok(profile) = state.load_cli_runtime_profile(runtime_profile_id) else {
        return (true, Some("generic_cli_profile_missing".to_string()));
    };
    let missing_config_home = profile.driver_id == "codex"
        && !profile
            .config_home
            .as_ref()
            .is_some_and(|path| path.is_dir());
    let install_status = GenericCliDriverRegistry::new(profile.clone()).check_install_status();
    let missing_binary = install_status
        .as_ref()
        .map(|status| !status.installed)
        .unwrap_or(true);
    let auth_status = generic_cli_auth_status(&profile);
    let last_error_code = if missing_config_home {
        Some("generic_cli_config_home_missing".to_string())
    } else if missing_binary {
        Some("generic_cli_driver_missing".to_string())
    } else if install_status.is_err() {
        Some("generic_cli_driver_probe_failed".to_string())
    } else if auth_status == "missing" {
        Some("generic_cli_auth_missing".to_string())
    } else if auth_status == "unknown" {
        Some("generic_cli_auth_unknown".to_string())
    } else {
        None
    };
    (
        missing_config_home || missing_binary || matches!(auth_status, "missing" | "unknown"),
        last_error_code,
    )
}

fn generic_cli_diagnostics_summary(state: &DaemonState, runtime: &AgentDefinition) -> Value {
    let Some(runtime_profile_id) = runtime.runtime_profile_id.as_deref() else {
        return json!({
            "profile_status": "missing",
            "driver_id": null,
            "active_session_count": 0,
            "config_summary": {
                "driver_id": null,
                "capability_schema_version": 1,
                "route_session_supported": true,
                "native_resume_supported": true,
                "profile_concurrency_cap_supported": true,
                "supported_drivers": supported_generic_cli_drivers(),
                "supported_workspace_modes": supported_generic_cli_workspace_modes(),
                "supported_sandbox_modes": supported_generic_cli_sandbox_modes(),
                "supported_runtime_create_args": supported_generic_cli_runtime_create_args(),
                "binary_installed": false,
                "binary_detail": "missing runtime_profile_id",
                "driver_version": null,
                "driver_status_code": "profile_missing",
                "create_supported": false,
                "setup_ready": false,
                "setup_status": "profile_missing",
                "probe_status": "failed",
                "probe_ttl_ms": GENERIC_CLI_SETUP_PROBE_TTL_MS,
                "next_action": "manual_review_required",
                "auth_status": "unknown",
                "home_isolation": "unknown",
                "host_home_shared_lock": false,
                "runtime_locks": empty_generic_cli_runtime_lock_summary(),
                "config_home": null,
                "config_home_exists": false,
                "default_workspace_mode": null,
                "default_sandbox": null,
                "route_hash": generic_cli_route_hash_summary(state),
                "route_session_counts": empty_route_session_counts(),
                "route_message_queue": unsupported_route_message_queue_summary("profile_missing"),
                "runtime_card": generic_cli_missing_runtime_card_summary(
                    "profile_missing",
                    "contact_admin",
                ),
                "max_parallel_runs_per_profile": 1,
                "runtime_target_required": true,
                "setup": generic_cli_setup_summary(
                    None,
                    "profile_missing",
                    "manual_review_required",
                    "unknown",
                    "unknown",
                    false,
                    "profile_missing",
                    "failed",
                    None,
                ),
                "driver_args_schema_version": null,
                "driver_capabilities": generic_cli_driver_capabilities(None),
            },
        });
    };
    let Ok(profile) = state.load_cli_runtime_profile(runtime_profile_id) else {
        return json!({
            "profile_status": "missing",
            "driver_id": null,
            "active_session_count": 0,
            "config_summary": {
                "driver_id": null,
                "capability_schema_version": 1,
                "route_session_supported": true,
                "native_resume_supported": true,
                "profile_concurrency_cap_supported": true,
                "supported_drivers": supported_generic_cli_drivers(),
                "supported_workspace_modes": supported_generic_cli_workspace_modes(),
                "supported_sandbox_modes": supported_generic_cli_sandbox_modes(),
                "supported_runtime_create_args": supported_generic_cli_runtime_create_args(),
                "binary_installed": false,
                "binary_detail": "missing cli runtime profile",
                "driver_version": null,
                "driver_status_code": "profile_missing",
                "create_supported": false,
                "setup_ready": false,
                "setup_status": "profile_missing",
                "probe_status": "failed",
                "probe_ttl_ms": GENERIC_CLI_SETUP_PROBE_TTL_MS,
                "next_action": "manual_review_required",
                "auth_status": "unknown",
                "home_isolation": "unknown",
                "host_home_shared_lock": false,
                "runtime_locks": empty_generic_cli_runtime_lock_summary(),
                "config_home": null,
                "config_home_exists": false,
                "default_workspace_mode": null,
                "default_sandbox": null,
                "route_hash": generic_cli_route_hash_summary(state),
                "route_session_counts": empty_route_session_counts(),
                "route_message_queue": unsupported_route_message_queue_summary("profile_missing"),
                "runtime_card": generic_cli_missing_runtime_card_summary(
                    "profile_missing",
                    "contact_admin",
                ),
                "max_parallel_runs_per_profile": 1,
                "runtime_target_required": true,
                "setup": generic_cli_setup_summary(
                    None,
                    "profile_missing",
                    "manual_review_required",
                    "unknown",
                    "unknown",
                    false,
                    "profile_missing",
                    "failed",
                    None,
                ),
                "driver_args_schema_version": null,
                "driver_capabilities": generic_cli_driver_capabilities(None),
            },
        });
    };
    let config_home_exists = profile
        .config_home
        .as_ref()
        .is_some_and(|path| path.is_dir());
    let install_probe = GenericCliDriverRegistry::new(profile.clone()).check_install_status();
    let install_probe_failed = install_probe.is_err();
    let install_status =
        install_probe.unwrap_or_else(|error| crate::runtime::RuntimeInstallStatus {
            installed: false,
            detail: Some(sanitize_public_error(&error.to_string())),
        });
    let missing_config_home = profile.driver_id == "codex"
        && !profile
            .config_home
            .as_ref()
            .is_some_and(|path| path.is_dir());
    let driver_status_code = generic_cli_driver_status_code(
        &profile,
        missing_config_home,
        &install_status,
        install_probe_failed,
    );
    let create_supported = generic_cli_create_supported(&profile.driver_id);
    let auth_status = generic_cli_auth_status(&profile);
    let setup_ready = generic_cli_setup_ready(driver_status_code, create_supported, auth_status);
    let setup_status = generic_cli_setup_status(driver_status_code, setup_ready, auth_status);
    let probe_status = generic_cli_probe_status(driver_status_code, install_probe_failed);
    let driver_version = if driver_status_code == "ok" {
        generic_cli_driver_version(install_status.detail.as_deref())
    } else {
        None
    };
    let driver_version_for_setup = driver_version.clone();
    let next_action = generic_cli_next_action(driver_status_code, auth_status);
    let home_isolation = generic_cli_home_isolation(&profile, config_home_exists);
    let host_home_shared_lock =
        profile.driver_id == "claude-code" && home_isolation == "host_default";
    let route_session_counts =
        generic_cli_route_session_counts(state, runtime_profile_id, &runtime.controller_scope_key);
    let route_message_queue = generic_cli_route_message_queue_summary(
        state,
        &runtime.agent_did,
        runtime_profile_id,
        &profile.driver_id,
        &runtime.controller_scope_key,
    );
    let runtime_card = generic_cli_runtime_card_summary(
        &profile.driver_id,
        setup_ready,
        setup_status,
        driver_status_code,
        next_action,
        &route_session_counts,
        &route_message_queue,
    );
    let active_session_count = route_session_counts
        .get("active")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    json!({
        "profile_status": profile.status,
        "active_session_count": active_session_count,
        "config_summary": {
            "driver_id": profile.driver_id,
            "capability_schema_version": 1,
            "route_session_supported": true,
            "native_resume_supported": true,
            "profile_concurrency_cap_supported": true,
            "supported_drivers": supported_generic_cli_drivers(),
            "supported_workspace_modes": supported_generic_cli_workspace_modes(),
            "supported_sandbox_modes": supported_generic_cli_sandbox_modes(),
            "supported_runtime_create_args": supported_generic_cli_runtime_create_args(),
            "binary_installed": install_status.installed,
            "binary_detail": install_status.detail.map(|detail| sanitize_public_error(&detail)),
            "driver_version": driver_version,
            "driver_status_code": driver_status_code,
            "create_supported": create_supported,
            "setup_ready": setup_ready,
            "setup_status": setup_status,
            "probe_status": probe_status,
            "probe_ttl_ms": GENERIC_CLI_SETUP_PROBE_TTL_MS,
            "next_action": next_action,
            "auth_status": auth_status,
            "home_isolation": home_isolation,
            "host_home_shared_lock": host_home_shared_lock,
            "runtime_locks": generic_cli_runtime_lock_summary(
                state,
                runtime_profile_id,
                &profile.driver_id,
                host_home_shared_lock,
            ),
            "config_home": if profile.config_home.is_some() { "configured" } else { "missing" },
            "config_home_exists": config_home_exists,
            "default_workspace_mode": profile.default_workspace_mode.as_str(),
            "default_sandbox": profile.default_sandbox,
            "route_hash": generic_cli_route_hash_summary(state),
            "route_session_counts": route_session_counts,
            "route_message_queue": route_message_queue,
            "runtime_card": runtime_card,
            "max_parallel_runs_per_profile": 1,
            "runtime_target_required": true,
            "setup": generic_cli_setup_summary(
                Some(&profile.driver_id),
                driver_status_code,
                next_action,
                home_isolation,
                auth_status,
                setup_ready,
                setup_status,
                probe_status,
                driver_version_for_setup.as_deref(),
            ),
            "driver_args_schema_version": generic_cli_driver_args_schema_version(&profile.driver_id),
            "driver_capabilities": generic_cli_driver_capabilities(Some(&profile.driver_id)),
        },
    })
}

fn generic_cli_create_supported(driver_id: &str) -> bool {
    matches!(driver_id, "codex" | "claude-code" | "command")
}

fn generic_cli_auth_status(profile: &CliRuntimeProfileRecord) -> &'static str {
    match profile.driver_id.as_str() {
        "codex" => {
            if profile
                .config_home
                .as_ref()
                .is_some_and(|path| path.join("auth.json").is_file())
            {
                "ok"
            } else {
                "missing"
            }
        }
        "claude-code" => "not_applicable",
        "command" => "not_applicable",
        _ => "unknown",
    }
}

fn generic_cli_setup_ready(
    driver_status_code: &str,
    create_supported: bool,
    auth_status: &str,
) -> bool {
    create_supported && driver_status_code == "ok" && matches!(auth_status, "ok" | "not_applicable")
}

fn generic_cli_setup_status(
    driver_status_code: &str,
    setup_ready: bool,
    auth_status: &str,
) -> &'static str {
    if setup_ready {
        "ready"
    } else {
        match driver_status_code {
            "missing_binary" | "config_home_missing" | "profile_missing" => "needs_setup",
            "probe_failed" => "probe_failed",
            "not_implemented" | "unsupported_driver" => "unsupported",
            "ok" if auth_status == "missing" => "needs_setup",
            _ => "unknown",
        }
    }
}

fn generic_cli_probe_status(driver_status_code: &str, install_probe_failed: bool) -> &'static str {
    if install_probe_failed {
        "failed"
    } else {
        match driver_status_code {
            "not_implemented" | "unsupported_driver" | "profile_missing" => "unsupported",
            _ => "fresh",
        }
    }
}

fn supported_generic_cli_drivers() -> Value {
    json!(["codex", "claude-code", "command"])
}

fn supported_generic_cli_workspace_modes() -> Value {
    json!(["route-root", "shared-root", "worktree-per-task"])
}

fn supported_generic_cli_sandbox_modes() -> Value {
    json!(["read-only", "workspace-write"])
}

fn supported_generic_cli_runtime_create_args() -> Value {
    json!([
        "runtime",
        "driver_id",
        "workspace_mode",
        "workspace_strategy",
        "default_sandbox",
        "default_model",
        "driver_config",
        "recipient_policy",
        "client_request_id"
    ])
}

fn generic_cli_daemon_capability_summary() -> Value {
    json!({
        "capability_schema_version": 1,
        "supported_drivers": supported_generic_cli_drivers(),
        "supported_workspace_modes": supported_generic_cli_workspace_modes(),
        "supported_sandbox_modes": supported_generic_cli_sandbox_modes(),
        "supported_runtime_create_args": supported_generic_cli_runtime_create_args(),
        "route_session_supported": true,
        "native_resume_supported": true,
        "profile_concurrency_cap_supported": true,
        "max_parallel_runs_per_profile": 1,
        "runtime_target_required": true,
    })
}

fn empty_route_session_counts() -> Value {
    json!({
        "total": 0,
        "active": 0,
        "running": 0,
        "failed": 0,
        "reset": 0,
    })
}

fn empty_route_message_queue_summary() -> Value {
    json!({
        "supported": true,
        "dispatch_source": "cli_route_message_queue",
        "runtime_retry_coordination": "auto_deferred_superseded",
        "contains_user_content": false,
        "last_message_id_watermark_policy": "final_only",
        "queued_count": 0,
        "running_count": 0,
        "succeeded_count": 0,
        "failed_count": 0,
        "cancelled_count": 0,
        "dead_letter_count": 0,
        "due_queued_count": 0,
        "due_route_count": 0,
        "oldest_queued_age_ms": null,
        "next_action": "none",
    })
}

fn unsupported_route_message_queue_summary(reason: &str) -> Value {
    let mut summary = empty_route_message_queue_summary();
    summary["supported"] = json!(false);
    summary["unsupported_reason"] = json!(reason);
    summary
}

fn generic_cli_missing_runtime_card_summary(setup_state: &str, next_action: &str) -> Value {
    json!({
        "supported": false,
        "status_schema_version": 1,
        "runtime_family": "generic-cli",
        "driver_id": null,
        "lifecycle_state": "manual_review_required",
        "setup_ready": false,
        "setup_state": setup_state,
        "queue_state": "idle",
        "active_run_state": "idle",
        "route_session_state": "none",
        "queued_count": 0,
        "running_count": 0,
        "dead_letter_count": 0,
        "failed_count": 0,
        "oldest_queued_age_ms": null,
        "next_action": next_action,
        "contains_user_content": false,
        "contains_provider_auth_material": false,
        "last_message_id_watermark_policy": "final_only",
    })
}

fn generic_cli_runtime_card_summary(
    driver_id: &str,
    setup_ready: bool,
    setup_status: &str,
    driver_status_code: &str,
    setup_next_action: &str,
    route_session_counts: &Value,
    route_message_queue: &Value,
) -> Value {
    let queue_supported = route_message_queue
        .get("supported")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let queued_count = route_message_queue_i64(route_message_queue, "queued_count");
    let running_count = route_message_queue_i64(route_message_queue, "running_count");
    let dead_letter_count = route_message_queue_i64(route_message_queue, "dead_letter_count");
    let failed_count = route_message_queue_i64(route_message_queue, "failed_count");
    let oldest_queued_age_ms = route_message_queue
        .get("oldest_queued_age_ms")
        .cloned()
        .unwrap_or(Value::Null);
    let route_session_state = generic_cli_route_session_card_state(route_session_counts);
    let queue_state = generic_cli_queue_card_state(
        queue_supported,
        queued_count,
        running_count,
        dead_letter_count,
    );
    let active_run_state =
        generic_cli_active_run_card_state(queue_state, queued_count, running_count, failed_count);
    let lifecycle_state = generic_cli_runtime_card_lifecycle_state(
        setup_ready,
        setup_status,
        queue_state,
        queued_count,
        running_count,
        dead_letter_count,
        route_session_state,
    );
    let next_action = generic_cli_runtime_card_next_action(
        lifecycle_state,
        setup_next_action,
        dead_letter_count,
        queued_count,
        running_count,
        driver_status_code,
    );
    json!({
        "supported": true,
        "status_schema_version": 1,
        "runtime_family": "generic-cli",
        "driver_id": driver_id,
        "lifecycle_state": lifecycle_state,
        "setup_ready": setup_ready,
        "setup_state": generic_cli_runtime_card_setup_state(setup_status, driver_status_code),
        "queue_state": queue_state,
        "active_run_state": active_run_state,
        "route_session_state": route_session_state,
        "queued_count": queued_count,
        "running_count": running_count,
        "dead_letter_count": dead_letter_count,
        "failed_count": failed_count,
        "oldest_queued_age_ms": oldest_queued_age_ms,
        "next_action": next_action,
        "contains_user_content": false,
        "contains_provider_auth_material": false,
        "last_message_id_watermark_policy": "final_only",
    })
}

fn route_message_queue_i64(summary: &Value, field: &str) -> i64 {
    summary.get(field).and_then(Value::as_i64).unwrap_or(0)
}

fn route_session_count_u64(counts: &Value, field: &str) -> u64 {
    counts.get(field).and_then(Value::as_u64).unwrap_or(0)
}

fn generic_cli_route_session_card_state(route_session_counts: &Value) -> &'static str {
    if route_session_count_u64(route_session_counts, "running") > 0 {
        "active"
    } else if route_session_count_u64(route_session_counts, "failed") > 0 {
        "failed"
    } else if route_session_count_u64(route_session_counts, "active") > 0 {
        "active"
    } else if route_session_count_u64(route_session_counts, "reset") > 0 {
        "reset"
    } else if route_session_count_u64(route_session_counts, "total") > 0 {
        "active"
    } else {
        "none"
    }
}

fn generic_cli_queue_card_state(
    queue_supported: bool,
    queued_count: i64,
    running_count: i64,
    dead_letter_count: i64,
) -> &'static str {
    if !queue_supported {
        "blocked"
    } else if dead_letter_count > 0 {
        "dead_letter"
    } else if running_count > 0 {
        "running"
    } else if queued_count > 0 {
        "queued"
    } else {
        "idle"
    }
}

fn generic_cli_active_run_card_state(
    queue_state: &str,
    queued_count: i64,
    running_count: i64,
    failed_count: i64,
) -> &'static str {
    if running_count > 0 || queue_state == "running" {
        "running"
    } else if queued_count > 0 || queue_state == "queued" {
        "queued"
    } else if failed_count > 0 {
        "failed"
    } else {
        "idle"
    }
}

fn generic_cli_runtime_card_lifecycle_state(
    setup_ready: bool,
    setup_status: &str,
    queue_state: &str,
    queued_count: i64,
    running_count: i64,
    dead_letter_count: i64,
    route_session_state: &str,
) -> &'static str {
    if !setup_ready {
        if matches!(setup_status, "unsupported" | "profile_missing") {
            return "manual_review_required";
        }
        return "needs_setup";
    }
    if dead_letter_count > 0 || queue_state == "dead_letter" {
        "dead_letter"
    } else if running_count > 0 || queue_state == "running" {
        "running"
    } else if queued_count > 0 || queue_state == "queued" {
        "queued"
    } else if route_session_state == "failed" {
        "failed"
    } else {
        "created"
    }
}

fn generic_cli_runtime_card_next_action(
    lifecycle_state: &str,
    setup_next_action: &str,
    dead_letter_count: i64,
    queued_count: i64,
    running_count: i64,
    driver_status_code: &str,
) -> &'static str {
    if lifecycle_state == "manual_review_required" {
        return "contact_admin";
    }
    if !matches!(driver_status_code, "ok") {
        return match setup_next_action {
            "install_driver" => "setup_required",
            "upgrade_daemon" => "upgrade_required",
            "manual_review_required" => "manual_review_required",
            _ => "setup_required",
        };
    }
    if dead_letter_count > 0 {
        "manual_review_required"
    } else if queued_count > 0 || running_count > 0 {
        "retry_later"
    } else if setup_next_action == "manual_review_required" {
        "manual_review_required"
    } else {
        "none"
    }
}

fn generic_cli_runtime_card_setup_state(
    setup_status: &str,
    driver_status_code: &str,
) -> &'static str {
    match driver_status_code {
        "profile_missing" => "profile_missing",
        "config_home_missing" => "profile_incomplete",
        "missing_binary" => "binary_missing",
        "probe_failed" => "binary_probe_failed",
        "not_implemented" | "unsupported_driver" => "unsupported_driver_version",
        "ok" if setup_status == "ready" => "ready",
        "ok" => "auth_unknown",
        _ => "unknown",
    }
}

fn empty_generic_cli_runtime_lock_summary() -> Value {
    json!({
        "profile_lock_supported": true,
        "host_home_lock_supported": true,
        "profile_lock_active": false,
        "host_home_lock_active": false,
        "profile_lock_count": 0,
        "host_home_lock_count": 0,
        "max_parallel_runs_per_profile": 1,
        "host_home_shared_lock": false,
    })
}

fn generic_cli_route_message_queue_summary(
    state: &DaemonState,
    agent_did: &str,
    runtime_profile_id: &str,
    driver_id: &str,
    controller_scope_key: &str,
) -> Value {
    if !matches!(driver_id, "codex" | "claude-code") {
        return unsupported_route_message_queue_summary("unsupported_driver");
    }
    let Ok(summary) = state.summarize_cli_route_message_queue_for_runtime(
        agent_did,
        runtime_profile_id,
        driver_id,
        controller_scope_key,
        current_time_millis().unwrap_or(0),
    ) else {
        return unsupported_route_message_queue_summary("summary_unavailable");
    };
    json!({
        "supported": true,
        "dispatch_source": "cli_route_message_queue",
        "runtime_retry_coordination": "auto_deferred_superseded",
        "contains_user_content": false,
        "last_message_id_watermark_policy": "final_only",
        "queued_count": summary.queued_count,
        "running_count": summary.running_count,
        "succeeded_count": summary.succeeded_count,
        "failed_count": summary.failed_count,
        "cancelled_count": summary.cancelled_count,
        "dead_letter_count": summary.dead_letter_count,
        "due_queued_count": summary.due_queued_count,
        "due_route_count": summary.due_route_count,
        "oldest_queued_age_ms": summary.oldest_queued_age_ms,
        "next_action": generic_cli_route_message_queue_next_action(
            summary.dead_letter_count,
            summary.queued_count,
            summary.running_count,
        ),
    })
}

fn generic_cli_route_message_queue_next_action(
    dead_letter_count: i64,
    queued_count: i64,
    running_count: i64,
) -> &'static str {
    if dead_letter_count > 0 {
        "manual_review_required"
    } else if queued_count > 0 || running_count > 0 {
        "retry_later"
    } else {
        "none"
    }
}

fn generic_cli_runtime_lock_summary(
    state: &DaemonState,
    runtime_profile_id: &str,
    driver_id: &str,
    host_home_shared_lock: bool,
) -> Value {
    let profile_lock_count = state
        .count_cli_runtime_locks(
            Some("profile"),
            Some(runtime_profile_id),
            Some(driver_id),
            false,
        )
        .unwrap_or(0);
    let host_home_lock_count = if host_home_shared_lock {
        state
            .count_cli_runtime_locks(Some("host-home"), None, Some(driver_id), false)
            .unwrap_or(0)
    } else {
        0
    };
    json!({
        "profile_lock_supported": true,
        "host_home_lock_supported": true,
        "profile_lock_active": profile_lock_count > 0,
        "host_home_lock_active": host_home_lock_count > 0,
        "profile_lock_count": profile_lock_count,
        "host_home_lock_count": host_home_lock_count,
        "max_parallel_runs_per_profile": 1,
        "host_home_shared_lock": host_home_shared_lock,
    })
}

fn generic_cli_route_session_counts(
    state: &DaemonState,
    runtime_profile_id: &str,
    controller_scope_key: &str,
) -> Value {
    let count = |status: Option<&str>| {
        state
            .count_cli_route_sessions_for_runtime_profile(
                runtime_profile_id,
                controller_scope_key,
                status,
            )
            .unwrap_or(0)
    };
    json!({
        "total": count(None),
        "active": count(Some("active")),
        "running": count(Some("running")),
        "failed": count(Some("failed")),
        "reset": count(Some("reset")),
    })
}

fn generic_cli_route_hash_summary(state: &DaemonState) -> Value {
    json!({
        "algorithm": "hmac-sha256",
        "version": "v2",
        "keyed": true,
        "salt_disclosed": false,
        "salt_present": state.generic_cli_route_hash_salt_present(),
        "path_component_prefix": "route_",
        "authorization": false,
    })
}

fn generic_cli_home_isolation(
    profile: &CliRuntimeProfileRecord,
    config_home_exists: bool,
) -> &'static str {
    match profile.driver_id.as_str() {
        "codex" if config_home_exists => "profile_home",
        "codex" => "missing",
        "claude-code" if std::env::var_os("HOME").is_some() => "host_default",
        "claude-code" => "unknown",
        _ => "not_applicable",
    }
}

fn generic_cli_driver_status_code(
    profile: &CliRuntimeProfileRecord,
    missing_config_home: bool,
    install_status: &crate::runtime::RuntimeInstallStatus,
    install_probe_failed: bool,
) -> &'static str {
    if matches!(profile.driver_id.as_str(), "gemini") {
        return "not_implemented";
    }
    if !matches!(
        profile.driver_id.as_str(),
        "codex" | "claude-code" | "command"
    ) {
        return "unsupported_driver";
    }
    if missing_config_home {
        return "config_home_missing";
    }
    if install_probe_failed {
        return "probe_failed";
    }
    if !install_status.installed {
        return "missing_binary";
    }
    "ok"
}

fn generic_cli_next_action(driver_status_code: &str, auth_status: &str) -> &'static str {
    match (driver_status_code, auth_status) {
        ("ok", "unknown" | "missing") => "manual_review_required",
        ("ok", _) => "none",
        ("missing_binary", _) => "install_driver",
        ("config_home_missing", _) => "manual_review_required",
        ("probe_failed", _) => "manual_review_required",
        ("not_implemented" | "unsupported_driver", _) => "upgrade_daemon",
        ("profile_missing", _) => "manual_review_required",
        _ => "manual_review_required",
    }
}

fn generic_cli_setup_summary(
    driver_id: Option<&str>,
    driver_status_code: &str,
    next_action: &str,
    home_isolation: &str,
    auth_status: &str,
    setup_ready: bool,
    setup_status: &str,
    probe_status: &str,
    driver_version: Option<&str>,
) -> Value {
    json!({
        "driver_id": driver_id,
        "binary_status": match driver_status_code {
            "ok" => "installed",
            "missing_binary" => "missing",
            "probe_failed" => "probe_failed",
            "not_implemented" | "unsupported_driver" => "unsupported",
            "profile_missing" => "unknown",
            _ => "unknown",
        },
        "auth_status": auth_status,
        "home_isolation": home_isolation,
        "setup_ready": setup_ready,
        "setup_status": setup_status,
        "probe_status": probe_status,
        "probe_ttl_ms": GENERIC_CLI_SETUP_PROBE_TTL_MS,
        "driver_version": driver_version,
        "next_action": next_action,
        "local_setup_hint_id": driver_id.map(generic_cli_setup_hint_id),
        "checked_at_ms": current_time_millis().unwrap_or(0),
    })
}

fn generic_cli_setup_hint_id(driver_id: &str) -> &'static str {
    match driver_id {
        "codex" => "codex-login-profile-home-v1",
        "claude-code" => "claude-code-login-host-home-v1",
        "command" => "generic-cli-command-config-v1",
        _ => "generic-cli-setup-v1",
    }
}

fn generic_cli_driver_args_schema_version(driver_id: &str) -> Option<&'static str> {
    match driver_id {
        "codex" => Some("codex-exec-v1"),
        "claude-code" => Some("claude-code-print-v1"),
        "command" => Some("generic-command-v1"),
        _ => None,
    }
}

fn generic_cli_driver_capabilities(driver_id: Option<&str>) -> Value {
    match driver_id {
        Some("codex") => json!({
            "json_stream": true,
            "output_last_message": true,
            "explicit_session_resume": true,
            "explicit_session_create": false,
            "resume_last_scoped_by_cwd": true,
            "permission_mode_flag": false,
        }),
        Some("claude-code") => json!({
            "json_stream": true,
            "output_last_message": false,
            "explicit_session_resume": true,
            "explicit_session_create": true,
            "resume_last_scoped_by_cwd": true,
            "permission_mode_flag": true,
        }),
        Some("command") => json!({
            "json_stream": false,
            "output_last_message": false,
            "explicit_session_resume": false,
            "explicit_session_create": false,
            "resume_last_scoped_by_cwd": false,
            "permission_mode_flag": false,
        }),
        _ => json!({
            "json_stream": false,
            "output_last_message": false,
            "explicit_session_resume": false,
            "explicit_session_create": false,
            "resume_last_scoped_by_cwd": false,
            "permission_mode_flag": false,
        }),
    }
}

fn generic_cli_driver_version(detail: Option<&str>) -> Option<String> {
    let detail = detail?;
    let version = detail
        .rsplit_once('(')
        .and_then(|(_, tail)| tail.strip_suffix(')'))
        .unwrap_or(detail)
        .trim();
    if version.is_empty() {
        None
    } else {
        Some(sanitize_public_error(version))
    }
}

fn service_status(config: &DaemonConfig) -> ServiceStatus {
    if DaemonConfig::default_product_state_root()
        .map(|root| root != config.state_root)
        .unwrap_or(true)
    {
        return ServiceStatus {
            platform: ServicePlatform::Foreground,
            installed: false,
            running: false,
            unit_path: None,
            detail: Some("foreground/dev state root".to_string()),
        };
    }
    let executable = crate::service::default_executable_path().ok();
    executable
        .as_deref()
        .and_then(|executable| manage_service(config, executable, ServiceAction::Status).ok())
        .unwrap_or(ServiceStatus {
            platform: ServicePlatform::Foreground,
            installed: false,
            running: false,
            unit_path: None,
            detail: Some("service status unavailable".to_string()),
        })
}

fn has_running_runs(state: &DaemonState) -> Result<bool> {
    let connection = state.connection()?;
    let count: i64 = connection.query_row(
        "SELECT COUNT(*) FROM runtime_run WHERE status = 'running'",
        [],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

fn app_attention_active(state: &DaemonState, now_ms: i64) -> Result<bool> {
    let connection = state.connection()?;
    let cutoff = now_ms.saturating_sub(APP_ATTENTION_WINDOW_MS);
    let count: i64 = connection.query_row(
        "SELECT COUNT(*) FROM agent_status_query_throttle WHERE last_snapshot_at_ms >= ?1",
        [cutoff],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

fn record_status_error(
    state: &DaemonState,
    daemon: &AgentDefinition,
    event_type: &str,
    message: &str,
) -> Result<()> {
    state.insert_audit_event_json(
        event_type,
        Some(&daemon.agent_did),
        None,
        None,
        None,
        json!({
            "error": sanitize_public_error(message),
        }),
    )
}

fn latest_signature(items: &[AgentLatestStatusUpdateItem]) -> String {
    items
        .iter()
        .map(|item| {
            let diagnostics = serde_json::to_string(&item.diagnostics_summary)
                .unwrap_or_else(|_| "diagnostics_unavailable".to_string());
            format!(
                "{}:{}:{}:{}:{}:{}:{}:{}",
                item.agent_did,
                item.status,
                item.version.as_deref().unwrap_or_default(),
                item.latest_version.as_deref().unwrap_or_default(),
                item.needs_upgrade,
                item.needs_config,
                item.last_error_code.as_deref().unwrap_or_default(),
                diagnostics
            )
        })
        .collect::<Vec<_>>()
        .join("|")
}

fn runtime_name_from_plugin(plugin_id: Option<&str>) -> &'static str {
    match plugin_id {
        Some(crate::plugins::hermes::HERMES_RUNTIME_PLUGIN_ID) => "hermes",
        Some(crate::agent::GENERIC_CLI_RUNTIME_PLUGIN_ID) => "generic-cli",
        _ => "runtime",
    }
}

fn service_label(platform: ServicePlatform) -> &'static str {
    match platform {
        ServicePlatform::LaunchAgent => "launch_agent",
        ServicePlatform::SystemdUser => "systemd_user",
        ServicePlatform::Foreground => "foreground",
        ServicePlatform::Unsupported => "unsupported",
    }
}

fn rfc3339_now() -> String {
    time::OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}

fn rfc3339_from_millis(ms: i64) -> String {
    let seconds = ms.div_euclid(1000);
    let nanos = (ms.rem_euclid(1000) * 1_000_000) as i32;
    let Ok(value) = time::OffsetDateTime::from_unix_timestamp(seconds) else {
        return rfc3339_now();
    };
    let Ok(value) = value.replace_nanosecond(nanos as u32) else {
        return rfc3339_now();
    };
    value.format(&Rfc3339).unwrap_or_else(|_| rfc3339_now())
}

fn sanitize_public_error(message: &str) -> String {
    let mut sanitized = message
        .split_whitespace()
        .map(|part| {
            let lower = part.to_ascii_lowercase();
            if lower.contains("token")
                || lower.contains("secret")
                || lower.contains("jwt")
                || lower.contains("key")
            {
                "<redacted>"
            } else if part.starts_with('/') || part.starts_with("file://") {
                "<path>"
            } else {
                part
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    if sanitized.chars().count() > 240 {
        sanitized = sanitized.chars().take(240).collect();
    }
    sanitized
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{generate_agent_identity, AgentDefinition};
    use crate::plugins::generic_cli::GENERIC_CLI_RUNTIME_PLUGIN_ID;
    use crate::plugins::hermes::{AWIKI_SKILLS_VERSION, HERMES_RUNTIME_PLUGIN_ID};
    use crate::state::{
        CliRuntimeProfileRecord, CreateCliRouteMessageQueueReference, CreateCliRouteSession,
        HermesProfileRecord,
    };
    use crate::workspace::WorkspaceMode;
    use std::collections::BTreeSet;
    use std::sync::{Mutex, MutexGuard};

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    struct EnvGuard {
        _lock: MutexGuard<'static, ()>,
        values: Vec<(&'static str, Option<String>)>,
    }

    impl EnvGuard {
        fn clear(keys: &[&'static str]) -> Self {
            let lock = ENV_LOCK
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let values = keys
                .iter()
                .map(|key| {
                    let value = std::env::var(key).ok();
                    std::env::remove_var(key);
                    (*key, value)
                })
                .collect();
            Self {
                _lock: lock,
                values,
            }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            for (key, value) in &self.values {
                if let Some(value) = value {
                    std::env::set_var(key, value);
                } else {
                    std::env::remove_var(key);
                }
            }
        }
    }

    const TEST_CONTROLLER_USER_ID: &str = "user-alice";
    const TEST_CONTROLLER_FULL_HANDLE: &str = "alice.anpclaw.com";
    const TEST_CONTROLLER_SCOPE_KEY: &str = "controller-scope:v1:test-alice-anpclaw-com";

    fn daemon() -> AgentDefinition {
        AgentDefinition {
            agent_did: "did:agent:daemon".to_string(),
            handle: "alice-daemon".to_string(),
            agent_kind: AgentKind::Daemon,
            controller_user_id: TEST_CONTROLLER_USER_ID.to_string(),
            controller_full_handle: TEST_CONTROLLER_FULL_HANDLE.to_string(),
            controller_scope_key: TEST_CONTROLLER_SCOPE_KEY.to_string(),
            controller_did: "did:human:alice".to_string(),
            runtime_plugin_id: None,
            runtime_profile_id: None,
            workspace_id: None,
            policy_id: "default".to_string(),
            local_agent_db_path: "agents/daemon/agent.db".to_string(),
            message_db_path: "agents/daemon/messages.db".to_string(),
            status: "active".to_string(),
        }
    }

    fn hermes_runtime() -> AgentDefinition {
        AgentDefinition {
            agent_did: "did:agent:hermes".to_string(),
            handle: "alice-hermes".to_string(),
            agent_kind: AgentKind::Runtime,
            controller_user_id: TEST_CONTROLLER_USER_ID.to_string(),
            controller_full_handle: TEST_CONTROLLER_FULL_HANDLE.to_string(),
            controller_scope_key: TEST_CONTROLLER_SCOPE_KEY.to_string(),
            controller_did: "did:human:alice".to_string(),
            runtime_plugin_id: Some(HERMES_RUNTIME_PLUGIN_ID.to_string()),
            runtime_profile_id: Some("profile_hermes_alice".to_string()),
            workspace_id: None,
            policy_id: "default".to_string(),
            local_agent_db_path: "agents/hermes/agent.db".to_string(),
            message_db_path: "agents/hermes/messages.db".to_string(),
            status: "active".to_string(),
        }
    }

    fn generic_cli_runtime() -> AgentDefinition {
        AgentDefinition {
            agent_did: "did:agent:codex".to_string(),
            handle: "alice-codex".to_string(),
            agent_kind: AgentKind::Runtime,
            controller_user_id: TEST_CONTROLLER_USER_ID.to_string(),
            controller_full_handle: TEST_CONTROLLER_FULL_HANDLE.to_string(),
            controller_scope_key: TEST_CONTROLLER_SCOPE_KEY.to_string(),
            controller_did: "did:human:alice".to_string(),
            runtime_plugin_id: Some(GENERIC_CLI_RUNTIME_PLUGIN_ID.to_string()),
            runtime_profile_id: Some("profile_codex_alice".to_string()),
            workspace_id: None,
            policy_id: "default".to_string(),
            local_agent_db_path: "agents/codex/agent.db".to_string(),
            message_db_path: "agents/codex/messages.db".to_string(),
            status: "active".to_string(),
        }
    }

    fn command_generic_cli_runtime() -> AgentDefinition {
        AgentDefinition {
            agent_did: "did:agent:command".to_string(),
            handle: "alice-command".to_string(),
            agent_kind: AgentKind::Runtime,
            controller_user_id: TEST_CONTROLLER_USER_ID.to_string(),
            controller_full_handle: TEST_CONTROLLER_FULL_HANDLE.to_string(),
            controller_scope_key: TEST_CONTROLLER_SCOPE_KEY.to_string(),
            controller_did: "did:human:alice".to_string(),
            runtime_plugin_id: Some(GENERIC_CLI_RUNTIME_PLUGIN_ID.to_string()),
            runtime_profile_id: Some("profile_command_alice".to_string()),
            workspace_id: None,
            policy_id: "default".to_string(),
            local_agent_db_path: "agents/command/agent.db".to_string(),
            message_db_path: "agents/command/messages.db".to_string(),
            status: "active".to_string(),
        }
    }

    fn create_test_route_session(
        root: &std::path::Path,
        conversation_id: &str,
    ) -> CreateCliRouteSession {
        let route_segment = conversation_id
            .chars()
            .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
            .collect::<String>();
        CreateCliRouteSession {
            agent_did: "did:agent:codex".to_string(),
            runtime_profile_id: "profile_codex_alice".to_string(),
            driver_id: "codex".to_string(),
            controller_user_id: TEST_CONTROLLER_USER_ID.to_string(),
            controller_full_handle: TEST_CONTROLLER_FULL_HANDLE.to_string(),
            controller_scope_key: TEST_CONTROLLER_SCOPE_KEY.to_string(),
            controller_did: "did:human:alice".to_string(),
            conversation_id: conversation_id.to_string(),
            workspace_path: root
                .join("runtime/workspaces/profile_codex_alice/conversations")
                .join(&route_segment),
            session_dir: root
                .join("runtime/sessions/profile_codex_alice")
                .join(route_segment),
        }
    }

    fn create_test_queue_reference(
        route: &crate::state::CliRouteSessionRecord,
        source_message_id: &str,
        next_attempt_at_ms: i64,
    ) -> CreateCliRouteMessageQueueReference {
        CreateCliRouteMessageQueueReference {
            agent_did: route.agent_did.clone(),
            runtime_profile_id: route.runtime_profile_id.clone(),
            driver_id: route.driver_id.clone(),
            controller_user_id: route.controller_user_id.clone(),
            controller_full_handle: route.controller_full_handle.clone(),
            controller_scope_key: route.controller_scope_key.clone(),
            controller_did: route.controller_did.clone(),
            conversation_id: route.conversation_id.clone(),
            source_message_id: source_message_id.to_string(),
            task_id: Some(format!("task_{source_message_id}")),
            run_id: Some(format!("run_{source_message_id}")),
            enqueue_reason: "profile_busy".to_string(),
            next_attempt_at_ms,
            last_error_code: Some("profile_busy".to_string()),
            last_error_summary: Some("profile busy sanitized".to_string()),
        }
    }

    fn allowed_latest_diagnostics_keys() -> BTreeSet<&'static str> {
        [
            "installation_status",
            "profile_status",
            "runner_status",
            "active_session_count",
            "runtime_version",
            "config_summary",
            "release_manifest_url",
            "release_status",
            "release_error",
        ]
        .into_iter()
        .collect()
    }

    #[test]
    fn lightweight_payload_uses_status_schema_without_sensitive_fields() {
        let root = tempfile::tempdir().unwrap();
        let config = DaemonConfig::for_state_root(root.path()).unwrap();
        let payload = daemon_lightweight_payload(&config, &daemon());
        assert_eq!(payload["schema"], "awiki.agent.status.v1");
        assert_eq!(payload["status_scope"], "daemon");
        assert_eq!(
            payload["daemon"]["diagnostics_summary"]["installation_status"],
            "not_installed"
        );
        assert_eq!(
            payload["daemon"]["diagnostics_summary"]["runner_status"],
            "not_running"
        );
        assert!(payload["daemon"]["diagnostics_summary"]["config_summary"].is_object());
        let generic_cli =
            &payload["daemon"]["diagnostics_summary"]["config_summary"]["generic_cli"];
        assert_eq!(generic_cli["capability_schema_version"], 1);
        assert!(generic_cli["supported_drivers"]
            .as_array()
            .unwrap()
            .contains(&json!("claude-code")));
        assert!(generic_cli["supported_workspace_modes"]
            .as_array()
            .unwrap()
            .contains(&json!("route-root")));
        assert!(generic_cli["supported_sandbox_modes"]
            .as_array()
            .unwrap()
            .contains(&json!("workspace-write")));
        let dump = payload.to_string();
        assert!(!dump.contains("token"));
        assert!(!dump.contains("private"));
    }

    #[test]
    fn daemon_snapshot_payload_includes_daemon_diagnostics_summary() {
        let root = tempfile::tempdir().unwrap();
        let config = DaemonConfig::for_state_root(root.path()).unwrap();
        config.ensure_state_layout().unwrap();
        let state = DaemonState::open(&config).unwrap();
        state.initialize().unwrap();
        let daemon = daemon();
        state.upsert_agent_definition(&daemon).unwrap();

        let payload = daemon_snapshot_payload(&config, &state, &daemon).unwrap();

        assert_eq!(
            payload["daemon"]["diagnostics_summary"]["installation_status"],
            "not_installed"
        );
        assert_eq!(
            payload["daemon"]["diagnostics_summary"]["runner_status"],
            "not_running"
        );
        assert!(
            payload["daemon"]["diagnostics_summary"]["config_summary"]["service_installed"]
                .as_bool()
                .is_some()
        );
        assert_eq!(
            payload["daemon"]["diagnostics_summary"]["config_summary"]["generic_cli"]
                ["runtime_target_required"],
            true
        );
    }

    #[test]
    fn daemon_status_exposes_public_bootstrap_key_from_daemon_identity() {
        let root = tempfile::tempdir().unwrap();
        let config = DaemonConfig::for_state_root(root.path()).unwrap();
        config.ensure_state_layout().unwrap();
        let state = DaemonState::open(&config).unwrap();
        state.initialize().unwrap();
        let identity = generate_agent_identity(&config, AgentKind::Daemon, "alice-mac-daemon")
            .unwrap()
            .into_record("alice-mac-daemon".to_string(), AgentKind::Daemon);
        let mut daemon = daemon();
        daemon.agent_did = identity.agent_did.clone();
        state.store_agent_identity(&identity).unwrap();
        state.upsert_agent_definition(&daemon).unwrap();

        let payload = daemon_snapshot_payload(&config, &state, &daemon).unwrap();
        let diagnostics = &payload["daemon"]["diagnostics_summary"];

        let config_summary = &diagnostics["config_summary"];
        assert_eq!(
            config_summary["bootstrap_key_id"],
            format!(
                "{}#{}",
                daemon.agent_did,
                anp::authentication::VM_KEY_E2EE_AGREEMENT
            )
        );
        assert_eq!(
            diagnostics["config_summary"]["bootstrap_key_status"],
            "ready"
        );
        assert_eq!(config_summary["bootstrap_key_algorithm"], "x25519");
        assert!(config_summary["bootstrap_public_key_multibase"]
            .as_str()
            .unwrap()
            .starts_with('z'));
        let public_key = URL_SAFE_NO_PAD
            .decode(
                config_summary["bootstrap_public_key_b64u"]
                    .as_str()
                    .unwrap(),
            )
            .unwrap();
        assert_eq!(public_key.len(), 32);

        let dump = payload.to_string();
        assert!(!dump.contains("PRIVATE KEY"));
        assert!(!dump.contains("token"));
        assert!(!dump.contains("private"));
    }

    #[test]
    fn latest_status_items_include_bootstrap_public_key_without_private_material() {
        let root = tempfile::tempdir().unwrap();
        let config = DaemonConfig::for_state_root(root.path()).unwrap();
        config.ensure_state_layout().unwrap();
        let state = DaemonState::open(&config).unwrap();
        state.initialize().unwrap();
        let identity = generate_agent_identity(&config, AgentKind::Daemon, "alice-mac-daemon")
            .unwrap()
            .into_record("alice-mac-daemon".to_string(), AgentKind::Daemon);
        let mut daemon = daemon();
        daemon.agent_did = identity.agent_did.clone();
        state.store_agent_identity(&identity).unwrap();
        state.upsert_agent_definition(&daemon).unwrap();

        let items = latest_status_items(&config, &state, &daemon, 1_700_000_000_000).unwrap();
        let daemon_item = items
            .iter()
            .find(|item| item.agent_kind == AgentKind::Daemon)
            .unwrap();

        assert_eq!(
            daemon_item.diagnostics_summary["config_summary"]["bootstrap_key_id"],
            format!(
                "{}#{}",
                daemon.agent_did,
                anp::authentication::VM_KEY_E2EE_AGREEMENT
            )
        );
        assert_eq!(
            daemon_item.diagnostics_summary["config_summary"]["bootstrap_key_status"],
            "ready"
        );
        assert!(
            daemon_item.diagnostics_summary["config_summary"]["bootstrap_public_key_b64u"]
                .as_str()
                .is_some()
        );
        let dump = serde_json::to_string(&items).unwrap();
        assert!(!dump.contains("PRIVATE KEY"));
        assert!(!dump.contains("token"));
        assert!(!dump.contains("private"));
    }

    #[test]
    fn latest_status_items_use_user_service_allowed_diagnostics_keys() {
        let root = tempfile::tempdir().unwrap();
        let config = DaemonConfig::for_state_root(root.path()).unwrap();
        config.ensure_state_layout().unwrap();
        let state = DaemonState::open(&config).unwrap();
        state.initialize().unwrap();
        let daemon = daemon();
        let runtime = hermes_runtime();
        state.upsert_agent_definition(&daemon).unwrap();
        state.upsert_agent_definition(&runtime).unwrap();
        state
            .upsert_runtime_daemon_binding(
                &runtime.agent_did,
                &daemon.agent_did,
                &daemon.controller_user_id,
                &daemon.controller_full_handle,
                &daemon.controller_scope_key,
                &daemon.controller_did,
            )
            .unwrap();
        state
            .upsert_hermes_profile(&HermesProfileRecord {
                agent_did: runtime.agent_did.clone(),
                runtime_profile_id: runtime.runtime_profile_id.clone().unwrap(),
                hermes_profile: "awiki_alice_hermes".to_string(),
                hermes_home: root.path().join("runtime/hermes/profile"),
                hermes_version: Some("1.2.3".to_string()),
                awiki_skills_version: AWIKI_SKILLS_VERSION.to_string(),
                status: "ready".to_string(),
            })
            .unwrap();

        let items = latest_status_items(&config, &state, &daemon, 1_700_000_000_000).unwrap();
        assert_eq!(items.len(), 2);
        let allowed = allowed_latest_diagnostics_keys();
        for item in &items {
            let keys = item
                .diagnostics_summary
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect::<BTreeSet<_>>();
            assert!(
                keys.is_subset(&allowed),
                "unexpected diagnostics keys for {}: {:?}",
                item.agent_did,
                keys.difference(&allowed).collect::<Vec<_>>()
            );
        }
        let daemon_item = items
            .iter()
            .find(|item| item.agent_kind == AgentKind::Daemon)
            .unwrap();
        assert_eq!(
            daemon_item.diagnostics_summary["installation_status"],
            "not_installed"
        );
        assert_eq!(
            daemon_item.diagnostics_summary["runner_status"],
            "not_running"
        );
        assert!(daemon_item.diagnostics_summary["service_installed"].is_null());
        assert!(
            daemon_item.diagnostics_summary["config_summary"]["service_installed"]
                .as_bool()
                .is_some()
        );
        let generic_cli = &daemon_item.diagnostics_summary["config_summary"]["generic_cli"];
        assert_eq!(generic_cli["capability_schema_version"], 1);
        assert!(generic_cli["supported_drivers"]
            .as_array()
            .unwrap()
            .contains(&json!("codex")));
        assert!(generic_cli["supported_drivers"]
            .as_array()
            .unwrap()
            .contains(&json!("claude-code")));

        let runtime_item = items
            .iter()
            .find(|item| item.agent_kind == AgentKind::Runtime)
            .unwrap();
        assert_eq!(runtime_item.status, "ready");
        assert_eq!(runtime_item.version, None);
        assert_eq!(runtime_item.latest_version, None);
        assert_eq!(runtime_item.min_supported_version, None);
        assert_eq!(runtime_item.platform, None);
        assert_eq!(runtime_item.service, None);
        assert!(!runtime_item.needs_upgrade);
        assert_eq!(runtime_item.diagnostics_summary["profile_status"], "ready");
        assert_eq!(runtime_item.diagnostics_summary["runtime_version"], "1.2.3");
        assert!(runtime_item.diagnostics_summary["awiki_skills_version"].is_null());
        assert_eq!(
            runtime_item.diagnostics_summary["config_summary"]["awiki_skills_version"],
            AWIKI_SKILLS_VERSION
        );
        assert!(
            runtime_item.diagnostics_summary["config_summary"]["gateway_command"]
                .as_str()
                .is_some()
        );

        let dump = serde_json::to_string(&items).unwrap();
        assert!(!dump.contains("file://"));
        assert!(!dump.contains("/Users/"));
        assert!(!dump.contains("/home/"));
        assert!(!dump.contains("/tmp/"));
        assert!(!dump.contains(root.path().to_string_lossy().as_ref()));
    }

    #[test]
    fn latest_signature_changes_when_runtime_diagnostics_change() {
        let running = AgentLatestStatusUpdateItem {
            agent_did: "did:agent:codex".to_string(),
            agent_kind: AgentKind::Runtime,
            status: "ready".to_string(),
            last_seen_at: Some("2026-01-01T00:00:00Z".to_string()),
            version: None,
            latest_version: None,
            min_supported_version: None,
            platform: None,
            service: None,
            needs_upgrade: false,
            needs_config: false,
            last_error_code: None,
            last_error_summary: None,
            diagnostics_summary: json!({
                "config_summary": {
                    "runtime_card": {
                        "status_schema_version": 1,
                        "runtime_family": "generic-cli",
                        "lifecycle_state": "running",
                        "running_count": 1,
                        "contains_user_content": false,
                        "contains_provider_auth_material": false
                    }
                }
            }),
        };
        let mut created = running.clone();
        created.diagnostics_summary = json!({
            "config_summary": {
                "runtime_card": {
                    "status_schema_version": 1,
                    "runtime_family": "generic-cli",
                    "lifecycle_state": "created",
                    "running_count": 0,
                    "contains_user_content": false,
                    "contains_provider_auth_material": false
                }
            }
        });

        assert_ne!(latest_signature(&[running]), latest_signature(&[created]));
    }

    #[test]
    fn hermes_runtime_status_requires_gateway_command_even_when_profile_is_ready() {
        let _env = EnvGuard::clear(&["AWIKI_HERMES_BASE_CONFIG_PATH", "HOME"]);
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        let base_hermes_home = home.join(".hermes");
        std::fs::create_dir_all(&base_hermes_home).unwrap();
        std::fs::write(
            base_hermes_home.join("config.yaml"),
            "model:\n  provider: custom\n  default: gpt-5.2\n",
        )
        .unwrap();
        std::env::set_var("HOME", &home);
        let config = DaemonConfig::for_state_root(root.path()).unwrap();
        config.ensure_state_layout().unwrap();
        let state = DaemonState::open(&config).unwrap();
        state.initialize().unwrap();
        let runtime = hermes_runtime();
        state.upsert_agent_definition(&runtime).unwrap();
        state
            .upsert_hermes_profile(&HermesProfileRecord {
                agent_did: runtime.agent_did.clone(),
                runtime_profile_id: runtime.runtime_profile_id.clone().unwrap(),
                hermes_profile: "awiki_alice_hermes".to_string(),
                hermes_home: root.path().join("runtime/hermes/profile"),
                hermes_version: Some("1.2.3".to_string()),
                awiki_skills_version: AWIKI_SKILLS_VERSION.to_string(),
                status: "ready".to_string(),
            })
            .unwrap();
        let hermes_home = root.path().join("runtime/hermes/profile");
        assert!(!hermes_home.join("config.yaml").exists());

        let missing_gateway =
            runtime_status_summary_with_gateway_status(&config, &state, &runtime, || {
                HermesGatewayCommandStatus::Missing
            });
        assert!(missing_gateway.needs_config);
        assert_eq!(
            missing_gateway.last_error_code.as_deref(),
            Some("gateway_command_missing")
        );
        let missing_diagnostics = runtime_diagnostics_summary(&state, &runtime, &missing_gateway);
        assert_eq!(
            missing_diagnostics["config_summary"]["gateway_command"],
            "missing"
        );
        assert_eq!(
            missing_diagnostics["config_summary"]["model_config"],
            "configured"
        );
        assert!(hermes_home.join("config.yaml").exists());

        let configured_gateway =
            runtime_status_summary_with_gateway_status(&config, &state, &runtime, || {
                HermesGatewayCommandStatus::Configured
            });
        assert!(!configured_gateway.needs_config);
        assert!(configured_gateway.last_error_code.is_none());
        let configured_diagnostics =
            runtime_diagnostics_summary(&state, &runtime, &configured_gateway);
        assert_eq!(
            configured_diagnostics["config_summary"]["gateway_command"],
            "configured"
        );
        assert_eq!(
            configured_diagnostics["config_summary"]["model_config"],
            "configured"
        );
    }

    #[test]
    fn generic_cli_runtime_status_reports_profile_home_without_leaking_local_path() {
        let root = tempfile::tempdir().unwrap();
        let config = DaemonConfig::for_state_root(root.path()).unwrap();
        config.ensure_state_layout().unwrap();
        let state = DaemonState::open(&config).unwrap();
        state.initialize().unwrap();
        let runtime = generic_cli_runtime();
        state.upsert_agent_definition(&runtime).unwrap();
        let config_home = root
            .path()
            .join("runtime/profiles/profile_codex_alice/codex-home");
        std::fs::create_dir_all(&config_home).unwrap();
        let mut cli_profile =
            CliRuntimeProfileRecord::for_driver("profile_codex_alice", "codex").unwrap();
        cli_profile.binary_path = Some(root.path().join("missing-codex"));
        cli_profile.config_home = Some(config_home.clone());
        state.upsert_cli_runtime_profile(&cli_profile).unwrap();

        let status = runtime_status_summary(&config, &state, &runtime);
        let diagnostics = runtime_diagnostics_summary(&state, &runtime, &status);

        assert!(status.needs_config);
        assert_eq!(
            status.last_error_code.as_deref(),
            Some("generic_cli_driver_missing")
        );
        assert_eq!(diagnostics["profile_status"], "active");
        assert!(diagnostics.get("driver_id").is_none());
        assert_eq!(diagnostics["active_session_count"], 0);
        assert_eq!(diagnostics["config_summary"]["driver_id"], "codex");
        assert_eq!(
            diagnostics["config_summary"]["capability_schema_version"],
            1
        );
        assert_eq!(
            diagnostics["config_summary"]["route_session_supported"],
            true
        );
        assert_eq!(
            diagnostics["config_summary"]["native_resume_supported"],
            true
        );
        assert_eq!(
            diagnostics["config_summary"]["profile_concurrency_cap_supported"],
            true
        );
        assert!(diagnostics["config_summary"]["supported_drivers"]
            .as_array()
            .unwrap()
            .contains(&json!("codex")));
        assert!(diagnostics["config_summary"]["supported_drivers"]
            .as_array()
            .unwrap()
            .contains(&json!("claude-code")));
        assert_eq!(diagnostics["config_summary"]["config_home"], "configured");
        assert_eq!(diagnostics["config_summary"]["config_home_exists"], true);
        assert_eq!(
            diagnostics["config_summary"]["driver_status_code"],
            "missing_binary"
        );
        assert_eq!(diagnostics["config_summary"]["create_supported"], true);
        assert_eq!(diagnostics["config_summary"]["setup_ready"], false);
        assert_eq!(diagnostics["config_summary"]["setup_status"], "needs_setup");
        assert_eq!(diagnostics["config_summary"]["probe_status"], "fresh");
        assert_eq!(
            diagnostics["config_summary"]["probe_ttl_ms"],
            GENERIC_CLI_SETUP_PROBE_TTL_MS
        );
        assert!(diagnostics["config_summary"]["driver_version"].is_null());
        assert_eq!(
            diagnostics["config_summary"]["next_action"],
            "install_driver"
        );
        let card = &diagnostics["config_summary"]["runtime_card"];
        assert_eq!(card["supported"], true);
        assert_eq!(card["status_schema_version"], 1);
        assert_eq!(card["runtime_family"], "generic-cli");
        assert_eq!(card["driver_id"], "codex");
        assert_eq!(card["lifecycle_state"], "needs_setup");
        assert_eq!(card["setup_ready"], false);
        assert_eq!(card["setup_state"], "binary_missing");
        assert_eq!(card["queue_state"], "idle");
        assert_eq!(card["active_run_state"], "idle");
        assert_eq!(card["route_session_state"], "none");
        assert_eq!(card["queued_count"], 0);
        assert_eq!(card["running_count"], 0);
        assert_eq!(card["dead_letter_count"], 0);
        assert_eq!(card["failed_count"], 0);
        assert!(card["oldest_queued_age_ms"].is_null());
        assert_eq!(card["next_action"], "setup_required");
        assert_eq!(card["contains_user_content"], false);
        assert_eq!(card["contains_provider_auth_material"], false);
        assert_eq!(card["last_message_id_watermark_policy"], "final_only");
        assert_eq!(diagnostics["config_summary"]["auth_status"], "missing");
        assert_eq!(
            diagnostics["config_summary"]["home_isolation"],
            "profile_home"
        );
        assert_eq!(
            diagnostics["config_summary"]["host_home_shared_lock"],
            false
        );
        assert_eq!(
            diagnostics["config_summary"]["runtime_locks"]["profile_lock_supported"],
            true
        );
        assert_eq!(
            diagnostics["config_summary"]["runtime_locks"]["profile_lock_active"],
            false
        );
        assert_eq!(
            diagnostics["config_summary"]["default_workspace_mode"],
            "shared-root"
        );
        assert_eq!(
            diagnostics["config_summary"]["default_sandbox"],
            "read-only"
        );
        assert_eq!(
            diagnostics["config_summary"]["route_hash"]["algorithm"],
            "hmac-sha256"
        );
        assert_eq!(diagnostics["config_summary"]["route_hash"]["version"], "v2");
        assert_eq!(diagnostics["config_summary"]["route_hash"]["keyed"], true);
        assert_eq!(
            diagnostics["config_summary"]["route_hash"]["salt_disclosed"],
            false
        );
        assert_eq!(
            diagnostics["config_summary"]["route_hash"]["salt_present"],
            true
        );
        assert_eq!(
            diagnostics["config_summary"]["route_hash"]["authorization"],
            false
        );
        assert_eq!(
            diagnostics["config_summary"]["route_session_counts"]["total"],
            0
        );
        assert_eq!(
            diagnostics["config_summary"]["route_session_counts"]["active"],
            0
        );
        assert_eq!(
            diagnostics["config_summary"]["max_parallel_runs_per_profile"],
            1
        );
        assert_eq!(
            diagnostics["config_summary"]["runtime_target_required"],
            true
        );
        assert_eq!(
            diagnostics["config_summary"]["setup"]["local_setup_hint_id"],
            "codex-login-profile-home-v1"
        );
        assert_eq!(
            diagnostics["config_summary"]["setup"]["next_action"],
            "install_driver"
        );
        assert_eq!(diagnostics["config_summary"]["setup"]["setup_ready"], false);
        assert_eq!(
            diagnostics["config_summary"]["setup"]["setup_status"],
            "needs_setup"
        );
        assert_eq!(
            diagnostics["config_summary"]["setup"]["probe_status"],
            "fresh"
        );
        assert_eq!(
            diagnostics["config_summary"]["setup"]["probe_ttl_ms"],
            GENERIC_CLI_SETUP_PROBE_TTL_MS
        );
        assert!(diagnostics["config_summary"]["setup"]["driver_version"].is_null());
        assert_eq!(
            diagnostics["config_summary"]["driver_args_schema_version"],
            "codex-exec-v1"
        );
        assert_eq!(
            diagnostics["config_summary"]["driver_capabilities"]["explicit_session_resume"],
            true
        );
        let dump = diagnostics.to_string();
        assert!(!dump.contains(root.path().to_string_lossy().as_ref()));
        assert!(!dump.contains("route_key"));
        assert!(!dump.contains("did:human:bob"));
        assert!(!dump.contains("tok_"));
    }

    #[test]
    fn generic_cli_runtime_card_reports_created_when_setup_ready_without_routes() {
        let root = tempfile::tempdir().unwrap();
        let config = DaemonConfig::for_state_root(root.path()).unwrap();
        config.ensure_state_layout().unwrap();
        let state = DaemonState::open(&config).unwrap();
        state.initialize().unwrap();
        let runtime = command_generic_cli_runtime();
        state.upsert_agent_definition(&runtime).unwrap();

        let fake_program = root.path().join("fake-command");
        std::fs::write(&fake_program, "ready").unwrap();
        let mut cli_profile =
            CliRuntimeProfileRecord::for_driver("profile_command_alice", "command").unwrap();
        cli_profile.binary_path = Some(fake_program);
        cli_profile.default_workspace_mode = WorkspaceMode::RouteRoot;
        state.upsert_cli_runtime_profile(&cli_profile).unwrap();

        let status = runtime_status_summary(&config, &state, &runtime);
        let diagnostics = runtime_diagnostics_summary(&state, &runtime, &status);
        let card = &diagnostics["config_summary"]["runtime_card"];

        assert!(!status.needs_config);
        assert!(status.last_error_code.is_none());
        assert_eq!(diagnostics["config_summary"]["setup_ready"], true);
        assert_eq!(card["supported"], true);
        assert_eq!(card["status_schema_version"], 1);
        assert_eq!(card["runtime_family"], "generic-cli");
        assert_eq!(card["driver_id"], "command");
        assert_eq!(card["lifecycle_state"], "created");
        assert_eq!(card["setup_ready"], true);
        assert_eq!(card["setup_state"], "ready");
        assert_eq!(card["queue_state"], "blocked");
        assert_eq!(card["active_run_state"], "idle");
        assert_eq!(card["route_session_state"], "none");
        assert_eq!(card["queued_count"], 0);
        assert_eq!(card["running_count"], 0);
        assert_eq!(card["dead_letter_count"], 0);
        assert_eq!(card["failed_count"], 0);
        assert!(card["oldest_queued_age_ms"].is_null());
        assert_eq!(card["next_action"], "none");
        assert_eq!(card["contains_user_content"], false);
        assert_eq!(card["contains_provider_auth_material"], false);
        assert_eq!(card["last_message_id_watermark_policy"], "final_only");

        let dump = diagnostics.to_string();
        assert!(!dump.contains(root.path().to_string_lossy().as_ref()));
        assert!(!dump.contains("fake-command"));
        assert!(!dump.contains("route_key"));
        assert!(!dump.contains("native_session_id"));
        assert!(!dump.contains("tok_"));
    }

    #[test]
    fn generic_cli_runtime_card_reports_profile_missing_without_sensitive_fields() {
        let root = tempfile::tempdir().unwrap();
        let config = DaemonConfig::for_state_root(root.path()).unwrap();
        config.ensure_state_layout().unwrap();
        let state = DaemonState::open(&config).unwrap();
        state.initialize().unwrap();
        let mut runtime = generic_cli_runtime();
        runtime.runtime_profile_id = None;

        let status = runtime_status_summary(&config, &state, &runtime);
        let diagnostics = runtime_diagnostics_summary(&state, &runtime, &status);
        let card = &diagnostics["config_summary"]["runtime_card"];

        assert!(status.needs_config);
        assert_eq!(
            status.last_error_code.as_deref(),
            Some("generic_cli_profile_missing")
        );
        assert_eq!(
            diagnostics["config_summary"]["driver_status_code"],
            "profile_missing"
        );
        assert_eq!(card["supported"], false);
        assert_eq!(card["status_schema_version"], 1);
        assert_eq!(card["runtime_family"], "generic-cli");
        assert!(card["driver_id"].is_null());
        assert_eq!(card["lifecycle_state"], "manual_review_required");
        assert_eq!(card["setup_ready"], false);
        assert_eq!(card["setup_state"], "profile_missing");
        assert_eq!(card["queue_state"], "idle");
        assert_eq!(card["active_run_state"], "idle");
        assert_eq!(card["route_session_state"], "none");
        assert_eq!(card["queued_count"], 0);
        assert_eq!(card["running_count"], 0);
        assert_eq!(card["dead_letter_count"], 0);
        assert_eq!(card["failed_count"], 0);
        assert!(card["oldest_queued_age_ms"].is_null());
        assert_eq!(card["next_action"], "contact_admin");
        assert_eq!(card["contains_user_content"], false);
        assert_eq!(card["contains_provider_auth_material"], false);
        assert_eq!(card["last_message_id_watermark_policy"], "final_only");

        let dump = diagnostics.to_string();
        assert!(!dump.contains(root.path().to_string_lossy().as_ref()));
        assert!(!dump.contains("route_key"));
        assert!(!dump.contains("native_session_id"));
        assert!(!dump.contains("did:human:bob"));
        assert!(!dump.contains("tok_"));
    }

    #[test]
    fn generic_cli_runtime_card_prioritizes_setup_before_queue_states() {
        let route_counts = json!({
            "total": 1,
            "active": 1,
            "running": 0,
            "failed": 0,
            "reset": 0,
        });
        let queue = json!({
            "supported": true,
            "queued_count": 3,
            "running_count": 1,
            "failed_count": 1,
            "dead_letter_count": 1,
            "oldest_queued_age_ms": 42,
        });

        let card = generic_cli_runtime_card_summary(
            "codex",
            false,
            "needs_setup",
            "missing_binary",
            "install_driver",
            &route_counts,
            &queue,
        );

        assert_eq!(card["supported"], true);
        assert_eq!(card["driver_id"], "codex");
        assert_eq!(card["lifecycle_state"], "needs_setup");
        assert_eq!(card["setup_ready"], false);
        assert_eq!(card["setup_state"], "binary_missing");
        assert_eq!(card["queue_state"], "dead_letter");
        assert_eq!(card["active_run_state"], "running");
        assert_eq!(card["route_session_state"], "active");
        assert_eq!(card["queued_count"], 3);
        assert_eq!(card["running_count"], 1);
        assert_eq!(card["dead_letter_count"], 1);
        assert_eq!(card["failed_count"], 1);
        assert_eq!(card["oldest_queued_age_ms"], 42);
        assert_eq!(card["next_action"], "setup_required");
        assert_eq!(card["contains_user_content"], false);
        assert_eq!(card["contains_provider_auth_material"], false);
        assert_eq!(card["last_message_id_watermark_policy"], "final_only");
    }

    #[test]
    fn generic_cli_runtime_card_prioritizes_dead_letter_running_and_queued() {
        let no_routes = empty_route_session_counts();
        let dead = generic_cli_runtime_card_summary(
            "codex",
            true,
            "ready",
            "ok",
            "none",
            &no_routes,
            &json!({
                "supported": true,
                "queued_count": 0,
                "running_count": 1,
                "failed_count": 0,
                "dead_letter_count": 1,
                "oldest_queued_age_ms": null,
            }),
        );
        assert_eq!(dead["lifecycle_state"], "dead_letter");
        assert_eq!(dead["queue_state"], "dead_letter");
        assert_eq!(dead["active_run_state"], "running");
        assert_eq!(dead["next_action"], "manual_review_required");

        let running = generic_cli_runtime_card_summary(
            "codex",
            true,
            "ready",
            "ok",
            "none",
            &no_routes,
            &json!({
                "supported": true,
                "queued_count": 2,
                "running_count": 1,
                "failed_count": 0,
                "dead_letter_count": 0,
                "oldest_queued_age_ms": 7,
            }),
        );
        assert_eq!(running["lifecycle_state"], "running");
        assert_eq!(running["queue_state"], "running");
        assert_eq!(running["active_run_state"], "running");
        assert_eq!(running["next_action"], "retry_later");

        let queued = generic_cli_runtime_card_summary(
            "codex",
            true,
            "ready",
            "ok",
            "none",
            &no_routes,
            &json!({
                "supported": true,
                "queued_count": 2,
                "running_count": 0,
                "failed_count": 0,
                "dead_letter_count": 0,
                "oldest_queued_age_ms": 7,
            }),
        );
        assert_eq!(queued["lifecycle_state"], "queued");
        assert_eq!(queued["queue_state"], "queued");
        assert_eq!(queued["active_run_state"], "queued");
        assert_eq!(queued["next_action"], "retry_later");
    }

    #[cfg(unix)]
    #[test]
    fn generic_cli_runtime_status_reports_driver_version_and_setup_gate() {
        use std::os::unix::fs::PermissionsExt;

        let root = tempfile::tempdir().unwrap();
        let config = DaemonConfig::for_state_root(root.path()).unwrap();
        config.ensure_state_layout().unwrap();
        let state = DaemonState::open(&config).unwrap();
        state.initialize().unwrap();
        let runtime = generic_cli_runtime();
        state.upsert_agent_definition(&runtime).unwrap();

        let fake_codex = root.path().join("fake-codex");
        std::fs::write(
            &fake_codex,
            "#!/bin/sh\nif [ \"${1-}\" = \"--version\" ]; then echo 'codex-cli 9.9.9'; exit 0; fi\nexit 0\n",
        )
        .unwrap();
        let mut permissions = std::fs::metadata(&fake_codex).unwrap().permissions();
        permissions.set_mode(0o700);
        std::fs::set_permissions(&fake_codex, permissions).unwrap();

        let config_home = root
            .path()
            .join("runtime/profiles/profile_codex_alice/codex-home");
        std::fs::create_dir_all(&config_home).unwrap();
        let mut cli_profile =
            CliRuntimeProfileRecord::for_driver("profile_codex_alice", "codex").unwrap();
        cli_profile.binary_path = Some(fake_codex);
        cli_profile.config_home = Some(config_home.clone());
        cli_profile.default_workspace_mode = WorkspaceMode::RouteRoot;
        state.upsert_cli_runtime_profile(&cli_profile).unwrap();

        let status = runtime_status_summary(&config, &state, &runtime);
        let diagnostics = runtime_diagnostics_summary(&state, &runtime, &status);

        assert!(status.needs_config);
        assert_eq!(
            status.last_error_code.as_deref(),
            Some("generic_cli_auth_missing")
        );
        assert_eq!(diagnostics["config_summary"]["binary_installed"], true);
        assert_eq!(diagnostics["config_summary"]["driver_status_code"], "ok");
        assert_eq!(
            diagnostics["config_summary"]["driver_version"],
            "codex-cli 9.9.9"
        );
        assert_eq!(diagnostics["config_summary"]["create_supported"], true);
        assert_eq!(diagnostics["config_summary"]["auth_status"], "missing");
        assert_eq!(diagnostics["config_summary"]["setup_ready"], false);
        assert_eq!(diagnostics["config_summary"]["setup_status"], "needs_setup");
        assert_eq!(diagnostics["config_summary"]["probe_status"], "fresh");
        assert_eq!(
            diagnostics["config_summary"]["probe_ttl_ms"],
            GENERIC_CLI_SETUP_PROBE_TTL_MS
        );
        assert_eq!(
            diagnostics["config_summary"]["next_action"],
            "manual_review_required"
        );
        assert_eq!(
            diagnostics["config_summary"]["setup"]["driver_version"],
            "codex-cli 9.9.9"
        );
        assert_eq!(
            diagnostics["config_summary"]["setup"]["auth_status"],
            "missing"
        );
        assert_eq!(diagnostics["config_summary"]["setup"]["setup_ready"], false);
        assert_eq!(
            diagnostics["config_summary"]["setup"]["setup_status"],
            "needs_setup"
        );
        assert_eq!(
            diagnostics["config_summary"]["setup"]["probe_status"],
            "fresh"
        );
        assert_eq!(
            diagnostics["config_summary"]["setup"]["next_action"],
            "manual_review_required"
        );
        assert_eq!(
            diagnostics["config_summary"]["driver_args_schema_version"],
            "codex-exec-v1"
        );

        std::fs::write(config_home.join("auth.json"), "{}").unwrap();
        let status = runtime_status_summary(&config, &state, &runtime);
        let diagnostics = runtime_diagnostics_summary(&state, &runtime, &status);

        assert!(!status.needs_config);
        assert!(status.last_error_code.is_none());
        assert_eq!(diagnostics["config_summary"]["auth_status"], "ok");
        assert_eq!(diagnostics["config_summary"]["setup_ready"], true);
        assert_eq!(diagnostics["config_summary"]["setup_status"], "ready");
        assert_eq!(diagnostics["config_summary"]["next_action"], "none");
        assert_eq!(
            diagnostics["config_summary"]["runtime_card"]["lifecycle_state"],
            "created"
        );
        assert_eq!(
            diagnostics["config_summary"]["runtime_card"]["setup_state"],
            "ready"
        );

        let dump = diagnostics.to_string();
        assert!(!dump.contains(root.path().to_string_lossy().as_ref()));
        assert!(!dump.contains("fake-codex"));
        assert!(!dump.contains("route_key"));
        assert!(!dump.contains("tok_"));
    }

    #[test]
    fn generic_cli_runtime_status_reports_route_session_counts_without_route_key_leakage() {
        let root = tempfile::tempdir().unwrap();
        let config = DaemonConfig::for_state_root(root.path()).unwrap();
        config.ensure_state_layout().unwrap();
        let state = DaemonState::open(&config).unwrap();
        state.initialize().unwrap();
        let runtime = generic_cli_runtime();
        state.upsert_agent_definition(&runtime).unwrap();
        let config_home = root
            .path()
            .join("runtime/profiles/profile_codex_alice/codex-home");
        std::fs::create_dir_all(&config_home).unwrap();
        let mut cli_profile =
            CliRuntimeProfileRecord::for_driver("profile_codex_alice", "codex").unwrap();
        cli_profile.binary_path = Some(root.path().join("missing-codex"));
        cli_profile.config_home = Some(config_home);
        cli_profile.default_workspace_mode = WorkspaceMode::RouteRoot;
        state.upsert_cli_runtime_profile(&cli_profile).unwrap();

        let active = state
            .get_or_create_cli_route_session(create_test_route_session(
                root.path(),
                "direct:did:human:bob",
            ))
            .unwrap();
        let running = state
            .get_or_create_cli_route_session(create_test_route_session(
                root.path(),
                "direct:did:human:charlie",
            ))
            .unwrap();
        assert!(state
            .try_acquire_cli_route_session_lease(
                &running.route_key,
                "run_running",
                "agent_status_test",
                current_time_millis().unwrap() + 60_000,
            )
            .unwrap());
        let failed = state
            .get_or_create_cli_route_session(create_test_route_session(
                root.path(),
                "group:did:group:eng",
            ))
            .unwrap();
        state
            .mark_cli_route_session_failed(
                &failed.route_key,
                Some("run_failed"),
                "missing_binary",
                "codex missing at /tmp/secret-token",
            )
            .unwrap();
        let reset = state
            .get_or_create_cli_route_session(create_test_route_session(
                root.path(),
                "thread:thread-123",
            ))
            .unwrap();
        assert_eq!(
            state
                .reset_cli_route_session_by_route(&reset.route_key)
                .unwrap(),
            1
        );

        let status = runtime_status_summary(&config, &state, &runtime);
        let diagnostics = runtime_diagnostics_summary(&state, &runtime, &status);
        let counts = &diagnostics["config_summary"]["route_session_counts"];

        assert_eq!(active.status, "active");
        assert_eq!(diagnostics["active_session_count"], 1);
        assert_eq!(counts["total"], 4);
        assert_eq!(counts["active"], 1);
        assert_eq!(counts["running"], 1);
        assert_eq!(counts["failed"], 1);
        assert_eq!(counts["reset"], 1);
        assert_eq!(
            diagnostics["config_summary"]["default_workspace_mode"],
            "route-root"
        );

        let dump = diagnostics.to_string();
        assert!(!dump.contains(root.path().to_string_lossy().as_ref()));
        assert!(!dump.contains("did:human:bob"));
        assert!(!dump.contains("did:human:charlie"));
        assert!(!dump.contains("did:group:eng"));
        assert!(!dump.contains("thread-123"));
        assert!(!dump.contains("secret-token"));
        assert!(!dump.contains(&active.route_key));
    }

    #[test]
    fn generic_cli_runtime_status_reports_empty_route_message_queue_summary() {
        let root = tempfile::tempdir().unwrap();
        let config = DaemonConfig::for_state_root(root.path()).unwrap();
        config.ensure_state_layout().unwrap();
        let state = DaemonState::open(&config).unwrap();
        state.initialize().unwrap();
        let runtime = generic_cli_runtime();
        state.upsert_agent_definition(&runtime).unwrap();
        let config_home = root
            .path()
            .join("runtime/profiles/profile_codex_alice/codex-home");
        std::fs::create_dir_all(&config_home).unwrap();
        let mut cli_profile =
            CliRuntimeProfileRecord::for_driver("profile_codex_alice", "codex").unwrap();
        cli_profile.binary_path = Some(root.path().join("missing-codex"));
        cli_profile.config_home = Some(config_home);
        state.upsert_cli_runtime_profile(&cli_profile).unwrap();

        let status = runtime_status_summary(&config, &state, &runtime);
        let diagnostics = runtime_diagnostics_summary(&state, &runtime, &status);
        let queue = &diagnostics["config_summary"]["route_message_queue"];

        assert_eq!(queue["supported"], true);
        assert_eq!(queue["dispatch_source"], "cli_route_message_queue");
        assert_eq!(
            queue["runtime_retry_coordination"],
            "auto_deferred_superseded"
        );
        assert_eq!(queue["contains_user_content"], false);
        assert_eq!(queue["last_message_id_watermark_policy"], "final_only");
        assert_eq!(queue["queued_count"], 0);
        assert_eq!(queue["running_count"], 0);
        assert_eq!(queue["succeeded_count"], 0);
        assert_eq!(queue["failed_count"], 0);
        assert_eq!(queue["cancelled_count"], 0);
        assert_eq!(queue["dead_letter_count"], 0);
        assert_eq!(queue["due_queued_count"], 0);
        assert_eq!(queue["due_route_count"], 0);
        assert!(queue["oldest_queued_age_ms"].is_null());
        assert_eq!(queue["next_action"], "none");
    }

    #[test]
    fn generic_cli_runtime_status_reports_route_message_queue_summary_without_leakage() {
        let root = tempfile::tempdir().unwrap();
        let config = DaemonConfig::for_state_root(root.path()).unwrap();
        config.ensure_state_layout().unwrap();
        let state = DaemonState::open(&config).unwrap();
        state.initialize().unwrap();
        let runtime = generic_cli_runtime();
        state.upsert_agent_definition(&runtime).unwrap();
        let config_home = root
            .path()
            .join("runtime/profiles/profile_codex_alice/codex-home");
        std::fs::create_dir_all(&config_home).unwrap();
        let mut cli_profile =
            CliRuntimeProfileRecord::for_driver("profile_codex_alice", "codex").unwrap();
        cli_profile.binary_path = Some(root.path().join("missing-codex"));
        cli_profile.config_home = Some(config_home);
        cli_profile.default_workspace_mode = WorkspaceMode::RouteRoot;
        state.upsert_cli_runtime_profile(&cli_profile).unwrap();

        let now = current_time_millis().unwrap();
        let bob = state
            .get_or_create_cli_route_session(create_test_route_session(
                root.path(),
                "direct:did:human:bob",
            ))
            .unwrap();
        let charlie = state
            .get_or_create_cli_route_session(create_test_route_session(
                root.path(),
                "direct:did:human:charlie",
            ))
            .unwrap();
        let future = state
            .get_or_create_cli_route_session(create_test_route_session(
                root.path(),
                "group:did:group:future",
            ))
            .unwrap();
        let running_route = state
            .get_or_create_cli_route_session(create_test_route_session(
                root.path(),
                "thread:thread-running",
            ))
            .unwrap();
        let succeeded_route = state
            .get_or_create_cli_route_session(create_test_route_session(
                root.path(),
                "thread:thread-succeeded",
            ))
            .unwrap();
        let failed_route = state
            .get_or_create_cli_route_session(create_test_route_session(
                root.path(),
                "thread:thread-failed",
            ))
            .unwrap();
        let cancelled_route = state
            .get_or_create_cli_route_session(create_test_route_session(
                root.path(),
                "thread:thread-cancelled",
            ))
            .unwrap();
        let dead_route = state
            .get_or_create_cli_route_session(create_test_route_session(
                root.path(),
                "thread:thread-dead",
            ))
            .unwrap();

        let bob_due_1 = state
            .enqueue_cli_route_message_reference(create_test_queue_reference(
                &bob,
                "msg_due_bob_1",
                now.saturating_sub(60_000),
            ))
            .unwrap();
        let bob_due_2 = state
            .enqueue_cli_route_message_reference(create_test_queue_reference(
                &bob,
                "msg_due_bob_2",
                now.saturating_sub(30_000),
            ))
            .unwrap();
        let charlie_due = state
            .enqueue_cli_route_message_reference(create_test_queue_reference(
                &charlie,
                "msg_due_charlie_1",
                now.saturating_sub(10_000),
            ))
            .unwrap();
        state
            .enqueue_cli_route_message_reference(create_test_queue_reference(
                &future,
                "msg_future_1",
                now + 600_000,
            ))
            .unwrap();
        let running = state
            .enqueue_cli_route_message_reference(create_test_queue_reference(
                &running_route,
                "msg_running_1",
                now.saturating_sub(5_000),
            ))
            .unwrap();
        state
            .mark_cli_route_message_queue_running(&running.queue_id, "native-run-running")
            .unwrap();
        let succeeded = state
            .enqueue_cli_route_message_reference(create_test_queue_reference(
                &succeeded_route,
                "msg_succeeded_1",
                now.saturating_sub(5_000),
            ))
            .unwrap();
        state
            .mark_cli_route_message_queue_succeeded(&succeeded.queue_id, "native-run-succeeded")
            .unwrap();
        let failed = state
            .enqueue_cli_route_message_reference(create_test_queue_reference(
                &failed_route,
                "msg_failed_1",
                now.saturating_sub(5_000),
            ))
            .unwrap();
        state
            .mark_cli_route_message_queue_failed_or_queued(
                &failed.queue_id,
                "failed",
                None,
                "missing_binary",
                "codex missing at /tmp/secret-token",
            )
            .unwrap();
        state
            .enqueue_cli_route_message_reference(create_test_queue_reference(
                &cancelled_route,
                "msg_cancelled_1",
                now.saturating_sub(5_000),
            ))
            .unwrap();
        state
            .cancel_cli_route_message_queue_for_route(
                &cancelled_route.runtime_profile_id,
                &cancelled_route.route_key,
                "route_reset",
            )
            .unwrap();
        let dead = state
            .enqueue_cli_route_message_reference(create_test_queue_reference(
                &dead_route,
                "msg_dead_1",
                now.saturating_sub(5_000),
            ))
            .unwrap();
        state
            .claim_cli_route_message_queue_item(&dead.queue_id, "native-run-dead")
            .unwrap();
        state
            .retry_or_dead_letter_cli_route_message_queue_item(
                &dead.queue_id,
                1,
                now + 60_000,
                "provider_unavailable",
                "provider unavailable near /tmp/secret-token",
            )
            .unwrap();

        let status = runtime_status_summary(&config, &state, &runtime);
        let diagnostics = runtime_diagnostics_summary(&state, &runtime, &status);
        let queue = &diagnostics["config_summary"]["route_message_queue"];
        let card = &diagnostics["config_summary"]["runtime_card"];

        assert_eq!(queue["supported"], true);
        assert_eq!(queue["queued_count"], 4);
        assert_eq!(queue["running_count"], 1);
        assert_eq!(queue["succeeded_count"], 1);
        assert_eq!(queue["failed_count"], 1);
        assert_eq!(queue["cancelled_count"], 1);
        assert_eq!(queue["dead_letter_count"], 1);
        assert_eq!(queue["due_queued_count"], 3);
        assert_eq!(queue["due_route_count"], 2);
        assert!(queue["oldest_queued_age_ms"].as_i64().unwrap() >= 0);
        assert_eq!(queue["next_action"], "manual_review_required");
        assert_eq!(queue["last_message_id_watermark_policy"], "final_only");
        assert_eq!(card["supported"], true);
        assert_eq!(card["status_schema_version"], 1);
        assert_eq!(card["runtime_family"], "generic-cli");
        assert_eq!(card["driver_id"], "codex");
        assert_eq!(card["lifecycle_state"], "needs_setup");
        assert_eq!(card["setup_ready"], false);
        assert_eq!(card["setup_state"], "binary_missing");
        assert_eq!(card["queue_state"], "dead_letter");
        assert_eq!(card["active_run_state"], "running");
        assert_eq!(card["route_session_state"], "active");
        assert_eq!(card["queued_count"], 4);
        assert_eq!(card["running_count"], 1);
        assert_eq!(card["dead_letter_count"], 1);
        assert_eq!(card["failed_count"], 1);
        assert!(card["oldest_queued_age_ms"].as_i64().unwrap() >= 0);
        assert_eq!(card["next_action"], "setup_required");
        assert_eq!(card["contains_user_content"], false);
        assert_eq!(card["contains_provider_auth_material"], false);
        assert_eq!(card["last_message_id_watermark_policy"], "final_only");

        let dump = diagnostics.to_string();
        assert!(!dump.contains(root.path().to_string_lossy().as_ref()));
        assert!(!dump.contains("did:human:bob"));
        assert!(!dump.contains("did:human:charlie"));
        assert!(!dump.contains("did:group:future"));
        assert!(!dump.contains("thread-running"));
        assert!(!dump.contains("thread-succeeded"));
        assert!(!dump.contains("thread-failed"));
        assert!(!dump.contains("thread-cancelled"));
        assert!(!dump.contains("thread-dead"));
        assert!(!dump.contains("msg_due_bob_1"));
        assert!(!dump.contains("msg_due_bob_2"));
        assert!(!dump.contains("msg_due_charlie_1"));
        assert!(!dump.contains("msg_future_1"));
        assert!(!dump.contains("msg_running_1"));
        assert!(!dump.contains("msg_succeeded_1"));
        assert!(!dump.contains("msg_failed_1"));
        assert!(!dump.contains("msg_cancelled_1"));
        assert!(!dump.contains("msg_dead_1"));
        assert!(!dump.contains(&bob.route_key));
        assert!(!dump.contains(&bob_due_1.queue_id));
        assert!(!dump.contains(&bob_due_2.queue_id));
        assert!(!dump.contains(&charlie_due.queue_id));
        assert!(!dump.contains("native-run-running"));
        assert!(!dump.contains("native-run-succeeded"));
        assert!(!dump.contains("native-run-dead"));
        assert!(!dump.contains("secret-token"));
    }

    #[test]
    fn generic_cli_runtime_status_reports_claude_host_default_home_isolation() {
        let _env = EnvGuard::clear(&["HOME"]);
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("host-home");
        std::fs::create_dir_all(&home).unwrap();
        std::env::set_var("HOME", &home);
        let config = DaemonConfig::for_state_root(root.path()).unwrap();
        config.ensure_state_layout().unwrap();
        let state = DaemonState::open(&config).unwrap();
        state.initialize().unwrap();
        let mut runtime = generic_cli_runtime();
        runtime.agent_did = "did:agent:claude".to_string();
        runtime.handle = "alice-claude".to_string();
        runtime.runtime_profile_id = Some("profile_claude_alice".to_string());
        state.upsert_agent_definition(&runtime).unwrap();
        let mut cli_profile =
            CliRuntimeProfileRecord::for_driver("profile_claude_alice", "claude-code").unwrap();
        cli_profile.binary_path = Some(root.path().join("missing-claude"));
        cli_profile.default_workspace_mode = WorkspaceMode::RouteRoot;
        state.upsert_cli_runtime_profile(&cli_profile).unwrap();

        let status = runtime_status_summary(&config, &state, &runtime);
        let diagnostics = runtime_diagnostics_summary(&state, &runtime, &status);

        assert!(status.needs_config);
        assert_eq!(
            status.last_error_code.as_deref(),
            Some("generic_cli_driver_missing")
        );
        assert!(diagnostics.get("driver_id").is_none());
        assert_eq!(diagnostics["config_summary"]["driver_id"], "claude-code");
        assert_eq!(
            diagnostics["config_summary"]["driver_status_code"],
            "missing_binary"
        );
        assert_eq!(
            diagnostics["config_summary"]["home_isolation"],
            "host_default"
        );
        assert_eq!(diagnostics["config_summary"]["host_home_shared_lock"], true);
        assert_eq!(
            diagnostics["config_summary"]["runtime_locks"]["host_home_shared_lock"],
            true
        );
        assert_eq!(
            diagnostics["config_summary"]["runtime_locks"]["host_home_lock_active"],
            false
        );
        assert_eq!(
            diagnostics["config_summary"]["setup"]["local_setup_hint_id"],
            "claude-code-login-host-home-v1"
        );
        assert_eq!(
            diagnostics["config_summary"]["driver_args_schema_version"],
            "claude-code-print-v1"
        );
        assert_eq!(
            diagnostics["config_summary"]["driver_capabilities"]["explicit_session_create"],
            true
        );

        let dump = diagnostics.to_string();
        assert!(!dump.contains(root.path().to_string_lossy().as_ref()));
        assert!(!dump.contains(home.to_string_lossy().as_ref()));
    }

    #[test]
    fn generic_cli_runtime_status_treats_claude_host_auth_as_not_applicable() {
        use std::os::unix::fs::PermissionsExt;

        let _env = EnvGuard::clear(&["HOME"]);
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("host-home");
        std::fs::create_dir_all(&home).unwrap();
        std::env::set_var("HOME", &home);
        let fake_claude = root.path().join("fake-claude");
        std::fs::write(
            &fake_claude,
            "#!/bin/sh\nif [ \"${1-}\" = \"--version\" ]; then echo '2.1.185 (Claude Code)'; exit 0; fi\nexit 0\n",
        )
        .unwrap();
        let mut permissions = std::fs::metadata(&fake_claude).unwrap().permissions();
        permissions.set_mode(0o700);
        std::fs::set_permissions(&fake_claude, permissions).unwrap();

        let config = DaemonConfig::for_state_root(root.path()).unwrap();
        config.ensure_state_layout().unwrap();
        let state = DaemonState::open(&config).unwrap();
        state.initialize().unwrap();
        let mut runtime = generic_cli_runtime();
        runtime.agent_did = "did:agent:claude".to_string();
        runtime.handle = "alice-claude".to_string();
        runtime.runtime_profile_id = Some("profile_claude_alice".to_string());
        state.upsert_agent_definition(&runtime).unwrap();
        let mut cli_profile =
            CliRuntimeProfileRecord::for_driver("profile_claude_alice", "claude-code").unwrap();
        cli_profile.binary_path = Some(fake_claude);
        cli_profile.default_workspace_mode = WorkspaceMode::RouteRoot;
        state.upsert_cli_runtime_profile(&cli_profile).unwrap();

        let status = runtime_status_summary(&config, &state, &runtime);
        let diagnostics = runtime_diagnostics_summary(&state, &runtime, &status);

        assert!(!status.needs_config);
        assert!(status.last_error_code.is_none());
        assert_eq!(
            diagnostics["config_summary"]["auth_status"],
            "not_applicable"
        );
        assert_eq!(diagnostics["config_summary"]["setup_ready"], true);
        assert_eq!(diagnostics["config_summary"]["setup_status"], "ready");
        assert_eq!(diagnostics["config_summary"]["next_action"], "none");
        assert_eq!(
            diagnostics["config_summary"]["home_isolation"],
            "host_default"
        );
        assert_eq!(
            diagnostics["config_summary"]["runtime_card"]["lifecycle_state"],
            "created"
        );
    }

    #[test]
    fn latest_status_items_auto_detects_missing_hermes_gateway_command() {
        let _env = EnvGuard::clear(&["AWIKI_HERMES_GATEWAY_CMD", "AWIKI_HERMES_BIN", "HOME"]);
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        let fake_python = home
            .join(".hermes")
            .join("hermes-agent")
            .join("venv")
            .join("bin")
            .join("python");
        write_fake_ready_gateway_executable(&fake_python).unwrap();
        std::fs::write(
            home.join(".hermes").join("config.yaml"),
            "model:\n  provider: custom\n  default: gpt-5.2\n",
        )
        .unwrap();
        std::env::set_var("HOME", &home);

        let state_root = root.path().join("state");
        let config = DaemonConfig::for_state_root(&state_root).unwrap();
        config.ensure_state_layout().unwrap();
        let state = DaemonState::open(&config).unwrap();
        state.initialize().unwrap();
        let daemon = daemon();
        let runtime = hermes_runtime();
        state.upsert_agent_definition(&daemon).unwrap();
        state.upsert_agent_definition(&runtime).unwrap();
        state
            .upsert_runtime_daemon_binding(
                &runtime.agent_did,
                &daemon.agent_did,
                &daemon.controller_user_id,
                &daemon.controller_full_handle,
                &daemon.controller_scope_key,
                &daemon.controller_did,
            )
            .unwrap();
        state
            .upsert_hermes_profile(&HermesProfileRecord {
                agent_did: runtime.agent_did.clone(),
                runtime_profile_id: runtime.runtime_profile_id.clone().unwrap(),
                hermes_profile: "awiki_alice_hermes".to_string(),
                hermes_home: root.path().join("runtime/hermes/profile"),
                hermes_version: Some("1.2.3".to_string()),
                awiki_skills_version: AWIKI_SKILLS_VERSION.to_string(),
                status: "ready".to_string(),
            })
            .unwrap();

        let items = latest_status_items(&config, &state, &daemon, 1_700_000_000_000).unwrap();
        let runtime_item = items
            .iter()
            .find(|item| item.agent_kind == AgentKind::Runtime)
            .unwrap();

        assert_eq!(runtime_item.status, "ready");
        assert!(!runtime_item.needs_config);
        assert!(runtime_item.last_error_code.is_none());
        assert_eq!(
            runtime_item.diagnostics_summary["config_summary"]["gateway_command"],
            "configured"
        );
        let loaded = DaemonConfig::for_state_root(&state_root).unwrap();
        assert_eq!(
            loaded.hermes_gateway_cmd.as_deref(),
            Some(format!("{} -m tui_gateway.entry", fake_python.display()).as_str())
        );
    }

    #[test]
    fn latest_status_response_can_rotate_local_controller_did() {
        let root = tempfile::tempdir().unwrap();
        let config = DaemonConfig::for_state_root(root.path()).unwrap();
        config.ensure_state_layout().unwrap();
        let state = DaemonState::open(&config).unwrap();
        state.initialize().unwrap();
        let daemon = daemon();
        let runtime = hermes_runtime();
        state.upsert_agent_definition(&daemon).unwrap();
        state.upsert_agent_definition(&runtime).unwrap();
        state
            .upsert_runtime_daemon_binding(
                &runtime.agent_did,
                &daemon.agent_did,
                &daemon.controller_user_id,
                &daemon.controller_full_handle,
                &daemon.controller_scope_key,
                &daemon.controller_did,
            )
            .unwrap();

        sync_controller_did_from_latest_response(
            &state,
            &daemon.agent_did,
            &json!({
                "updated": [{
                    "agent_did": daemon.agent_did,
                    "controller_did": "did:human:alice-new",
                    "status": "ready",
                }]
            }),
        )
        .unwrap();

        assert_eq!(
            state
                .load_agent_definition(&daemon.agent_did)
                .unwrap()
                .controller_did,
            "did:human:alice-new"
        );
        assert_eq!(
            state
                .load_agent_definition(&runtime.agent_did)
                .unwrap()
                .controller_did,
            "did:human:alice-new"
        );
        assert_eq!(
            state
                .load_runtime_daemon_binding(&runtime.agent_did)
                .unwrap()
                .unwrap()
                .controller_did,
            "did:human:alice-new"
        );
    }

    #[test]
    fn heartbeat_scheduler_is_due_immediately_then_obeys_idle_interval() {
        let mut scheduler = HeartbeatScheduler::new();
        assert!(scheduler.last_control_emit_at_ms.is_none());
        scheduler.last_control_emit_at_ms = Some(1000);
        assert_eq!(
            scheduler
                .last_control_emit_at_ms
                .map(|last| 1000 + IDLE_HEARTBEAT_MS - last >= IDLE_HEARTBEAT_MS),
            Some(true)
        );
    }
}

#[cfg(test)]
fn write_fake_ready_gateway_executable(path: &std::path::Path) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(
        path,
        r#"#!/bin/sh
printf '{"method":"gateway.ready","params":{"version":"test"}}\n'
while IFS= read -r _line; do
  :
done
"#,
    )?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(path)?.permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(path, permissions)?;
    }
    Ok(())
}
