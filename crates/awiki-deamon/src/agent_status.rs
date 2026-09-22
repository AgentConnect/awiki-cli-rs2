use std::collections::BTreeMap;

use anyhow::{bail, Context, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde_json::{json, Value};
use time::format_description::well_known::Rfc3339;

use crate::agent::{AgentDefinition, AgentKind};
use crate::outbox::{AgentManagementOutbox, AgentStatusResponse};
use crate::registration::{
    AgentInventoryClient, AgentLatestStatusUpdateItem, UserServiceAgentRegistrationClient,
};
use crate::security::runtime_token::current_time_millis;
use crate::service::{manage_service, ServiceAction, ServicePlatform, ServiceStatus};
use crate::state::DaemonState;
use crate::upgrade::{check_release_status, DaemonReleaseStatus};
use crate::{DaemonConfig, ImCoreAdapter};

pub const IDLE_HEARTBEAT_MS: i64 = 5 * 60 * 1000;
pub const ACTIVE_HEARTBEAT_MS: i64 = 30 * 1000;
pub const APP_ATTENTION_WINDOW_MS: i64 = 2 * 60 * 1000;
pub const LATEST_STATUS_CHECK_MS: i64 = 10 * 1000;
pub const RELEASE_STATUS_CHECK_MS: i64 = 5 * 60 * 1000;
pub const CONTROLLER_IDENTITY_CHANGED_EVENT: &str = "daemon.controller_identity_changed";
pub const CONTROLLER_IDENTITY_CHANGED_ERROR: &str = "controller_identity_changed";

pub fn controller_identity_change_observed(
    state: &DaemonState,
    daemon_agent_did: &str,
) -> Result<bool> {
    state.audit_event_exists(
        CONTROLLER_IDENTITY_CHANGED_EVENT,
        Some(daemon_agent_did),
        Some(CONTROLLER_IDENTITY_CHANGED_ERROR),
    )
}

pub fn ensure_controller_identity_active(
    _state: &DaemonState,
    _daemon_agent_did: &str,
) -> Result<()> {
    Ok(())
}

pub fn record_controller_identity_changed(
    state: &DaemonState,
    daemon_agent_did: &str,
    source: &'static str,
) -> Result<()> {
    state.insert_agent_audit_event_json_once(
        CONTROLLER_IDENTITY_CHANGED_EVENT,
        daemon_agent_did,
        json!({
            "reason": CONTROLLER_IDENTITY_CHANGED_ERROR,
            "source": source,
        }),
    )?;
    bail!(CONTROLLER_IDENTITY_CHANGED_ERROR)
}

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
                || !self
                    .last_user_service_write_at_ms_by_daemon
                    .contains_key(&daemon.agent_did)
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
    let delegated_subkey = daemon_delegated_subkey_proposal(config, state, daemon);
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
            delegated_subkey.as_ref(),
        ),
        "runtimes": runtimes,
        "runs": [],
    }))
}

pub fn daemon_lightweight_payload(config: &DaemonConfig, daemon: &AgentDefinition) -> Value {
    let now = rfc3339_now();
    let service = service_status(config);
    let release = check_release_status(config);
    daemon_lightweight_payload_with_release(config, daemon, &service, &now, &release, None, None)
}

fn daemon_lightweight_payload_with_release(
    config: &DaemonConfig,
    daemon: &AgentDefinition,
    service: &ServiceStatus,
    now: &str,
    release: &DaemonReleaseStatus,
    bootstrap_key: Option<&DaemonBootstrapKeySummary>,
    delegated_subkey: Option<&crate::identity_custody::PreparedDaemonSubkey>,
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
        "daemon": daemon_status_payload(
            config,
            daemon,
            service,
            now,
            release,
            bootstrap_key,
            delegated_subkey,
        ),
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
        min_supported_version: release.minimum_supported_version.clone(),
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
            None,
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
        None,
    )
}

pub fn update_user_service_latest(
    config: &DaemonConfig,
    state: &DaemonState,
    daemon: &AgentDefinition,
    items: Vec<AgentLatestStatusUpdateItem>,
) -> Result<()> {
    let client = UserServiceAgentRegistrationClient::new(&config.user_service_base_url)?;
    crate::controller_scope::with_controller_reconcile_singleflight(daemon, || {
        let daemon = state.load_agent_definition(&daemon.agent_did)?;
        let auth = crate::controller_scope::daemon_auth_material(config, state, &daemon)?;
        let response = client.update_latest_status(&daemon.agent_did, items, &auth)?;
        sync_controller_scope_from_response(state, &daemon.agent_did, &response)
    })
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
    crate::controller_scope::reconcile_authoritative_controller_scope(
        state,
        &local,
        controller.controller_user_id.as_deref(),
        controller.controller_full_handle.as_deref(),
        None,
        &controller.controller_did,
        "authoritative_status",
        "daemon.controller_scope_mismatch",
    )
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
    ensure_controller_identity_active(state, &daemon.agent_did)?;
    let _client = im_core.client_for_agent(config, state, &daemon.agent_did)?;
    outbox.send_agent_status(&AgentStatusResponse {
        conversation_id: None,
        agent_did: daemon.agent_did.clone(),
        recipient_did: daemon.controller_did.clone(),
        payload: {
            let service = service_status(config);
            let now = rfc3339_now();
            daemon_lightweight_payload_with_release(
                config,
                daemon,
                &service,
                &now,
                release,
                daemon_bootstrap_key_summary(state, daemon).as_ref(),
                None,
            )
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
    delegated_subkey: Option<&crate::identity_custody::PreparedDaemonSubkey>,
) -> Value {
    json!({
        "agent_did": daemon.agent_did,
        "status": if release.needs_upgrade { "needs_upgrade" } else { "ready" },
        "last_seen_at": now,
        "version": release.current_version.clone(),
        "latest_version": release.latest_version.clone(),
        "min_supported_version": release.minimum_supported_version.clone(),
        "platform": crate::service::current_platform_label(),
        "service": service_label(service.platform),
        "needs_upgrade": release.needs_upgrade,
        "last_error_code": null,
        "last_error_summary": null,
        "diagnostics_summary": daemon_diagnostics_summary(
            service,
            release,
            bootstrap_key,
            delegated_subkey,
        ),
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
    delegated_subkey: Option<&crate::identity_custody::PreparedDaemonSubkey>,
) -> Value {
    let mut config_summary = json!({
        "service_installed": service.installed,
        "release_manifest_url": release.manifest_url.clone(),
        "release_policy_url": release.policy_url.clone(),
        "release_policy_origin": release.policy_origin.clone(),
        "release_policy_revision": release.policy_revision,
        "release_policy_source": release.policy_source.clone(),
        "release_status": if release.latest_version.is_some() || release.policy_revision.is_some() { "ok" } else { "unavailable" },
        "release_error": release.error.clone(),
        "bootstrap_key_status": if bootstrap_key.is_some() { "ready" } else { "missing" },
        "runtime_client_detection": {"schema_version":1},
        "acp": {"capability_schema_version":1,"supported_drivers":crate::acp::SUPPORTED_DRIVERS,"protocol_version":1},
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
    if let Some(delegated_subkey) = delegated_subkey {
        if let Some(object) = config_summary.as_object_mut() {
            object.insert(
                "delegated_subkey_proposal".to_string(),
                delegated_subkey_proposal_value(delegated_subkey),
            );
        }
    }
    json!({
        "installation_status": if service.installed { "installed" } else { "not_installed" },
        "runner_status": if service.running { "running" } else { "not_running" },
        "config_summary": config_summary,
    })
}

fn delegated_subkey_proposal_value(
    delegated_subkey: &crate::identity_custody::PreparedDaemonSubkey,
) -> Value {
    json!({
        "schema": crate::app_bridge::bootstrap::USER_SUBKEY_PACKAGE_SCHEMA_V3,
        "user_did": delegated_subkey.user_did,
        "verification_method": delegated_subkey.verification_method,
        "key_type": "Multikey/Ed25519",
        "key_algorithm": "Ed25519",
        "public_key_multibase": delegated_subkey.public_key_multibase,
    })
}

fn daemon_delegated_subkey_proposal(
    config: &DaemonConfig,
    state: &DaemonState,
    daemon: &AgentDefinition,
) -> Option<crate::identity_custody::PreparedDaemonSubkey> {
    use crate::app_bridge::bootstrap::BootstrapDidDocumentResolver;

    let resolver = crate::app_bridge::bootstrap::DefaultBootstrapDidDocumentResolver::new(config);
    let document = resolver
        .resolve_user_did_document(daemon.controller_did.trim())
        .ok()?;
    crate::identity_custody::prepare_daemon_subkey(state, &document).ok()
}

pub fn reconcile_daemon_upgrade_state(
    state: &DaemonState,
    daemon: &AgentDefinition,
    release: &DaemonReleaseStatus,
) -> Result<()> {
    if release.latest_version.is_none() {
        return Ok(());
    }
    let active_command_ids = crate::commands::active_daemon_upgrade_command_ids(
        &daemon.agent_did,
        &daemon.controller_scope_key,
    );
    state.reconcile_daemon_upgrade_commands_with_active(
        &daemon.agent_did,
        &daemon.controller_scope_key,
        &release.current_version,
        release.latest_version.as_deref(),
        release.needs_upgrade,
        &active_command_ids,
    )
}

fn daemon_bootstrap_key_summary(
    state: &DaemonState,
    daemon: &AgentDefinition,
) -> Option<DaemonBootstrapKeySummary> {
    let (did_document, expected_key_id) = match state.load_agent_device_identity(&daemon.agent_did)
    {
        Ok(Some(identity)) => (identity.did_document, identity.device_e2ee_key_id),
        _ => (
            state
                .load_agent_identity(&daemon.agent_did)
                .ok()?
                .did_document,
            format!(
                "{}#{}",
                daemon.agent_did.trim(),
                anp::authentication::VM_KEY_E2EE_AGREEMENT
            ),
        ),
    };
    daemon_bootstrap_key_summary_from_did_document(&did_document, &expected_key_id)
        .ok()
        .flatten()
}

fn daemon_bootstrap_key_summary_from_did_document(
    did_document: &Value,
    expected_key_id: &str,
) -> Result<Option<DaemonBootstrapKeySummary>> {
    let Some(methods) = did_document
        .get("verificationMethod")
        .and_then(Value::as_array)
    else {
        return Ok(None);
    };
    let Some(method) = methods.iter().find(|method| {
        method.get("id").and_then(Value::as_str).map(str::trim) == Some(expected_key_id)
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
        key_id: expected_key_id.to_owned(),
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
    needs_config: bool,
    last_error_code: Option<String>,
}

fn runtime_status_summary(
    _config: &DaemonConfig,
    state: &DaemonState,
    runtime: &AgentDefinition,
) -> RuntimeStatusSummary {
    let code = if runtime.status == "retired"
        || runtime
            .runtime_plugin_id
            .as_deref()
            .is_some_and(crate::state::runtime_retirement::is_legacy_runtime)
    {
        Some(crate::state::runtime_retirement::LEGACY_RUNTIME_DISABLED)
    } else if runtime.runtime_plugin_id.as_deref() != Some(crate::acp::PLUGIN_ID) {
        Some("runtime_protocol_unsupported")
    } else if runtime
        .runtime_profile_id
        .as_deref()
        .and_then(|id| state.load_cli_runtime_profile(id).ok())
        .is_none()
    {
        Some("acp_profile_missing")
    } else {
        None
    };
    // Heartbeats are read-only: installation checks belong to explicit
    // detection and task/session preparation, never account repair or probing.
    RuntimeStatusSummary {
        needs_config: code.is_some(),
        last_error_code: code.map(str::to_owned),
    }
}

pub(crate) fn runtime_diagnostics(
    state: &DaemonState,
    runtime: &AgentDefinition,
    config: &DaemonConfig,
) -> Value {
    runtime_diagnostics_summary(
        state,
        runtime,
        &runtime_status_summary(config, state, runtime),
    )
}

fn runtime_diagnostics_summary(
    state: &DaemonState,
    runtime: &AgentDefinition,
    runtime_status: &RuntimeStatusSummary,
) -> Value {
    if runtime_status.last_error_code.as_deref()
        == Some(crate::state::runtime_retirement::LEGACY_RUNTIME_DISABLED)
    {
        return json!({"profile_status":"retired","config_summary":{
            "protocol":"legacy","available":false,
            "retired_reason":crate::state::runtime_retirement::LEGACY_RUNTIME_DISABLED
        }});
    }
    if runtime.runtime_plugin_id.as_deref() == Some(crate::acp::PLUGIN_ID) {
        let Some(id) = runtime.runtime_profile_id.as_deref() else {
            return json!({"profile_status":"missing","config_summary":{"protocol":"acp"}});
        };
        let driver = state.load_cli_runtime_profile(id).ok().map(|p| p.driver_id);
        let probe = state
            .connection()
            .ok()
            .and_then(|db| {
                db.query_row(
                    "SELECT report FROM acp_probes WHERE profile_id=?1",
                    [id],
                    |r| r.get::<_, String>(0),
                )
                .ok()
            })
            .and_then(|s| serde_json::from_str::<Value>(&s).ok())
            .unwrap_or(json!({}));
        return json!({"profile_status":"ready","runtime_version":probe["binaryVersion"],"config_summary":{"protocol":"acp","driver_id":driver,"capabilities":probe["agentCapabilities"],"auth_status":"unknown"}});
    }
    json!({"profile_status":"unavailable","config_summary":{"protocol":"unsupported"}})
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
        Some("runtime.hermes") => "hermes",
        Some(crate::agent::GENERIC_CLI_RUNTIME_PLUGIN_ID) => "generic-cli",
        Some(crate::acp::PLUGIN_ID) => "acp",
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
#[path = "agent_status_tests.rs"]
mod tests;
