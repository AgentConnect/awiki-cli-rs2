use anyhow::{bail, Result};
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeartbeatScheduler {
    last_control_emit_at_ms: Option<i64>,
    last_user_service_write_at_ms: Option<i64>,
    last_status_signature: Option<String>,
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
            last_user_service_write_at_ms: None,
            last_status_signature: None,
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
        let due = self
            .last_control_emit_at_ms
            .map(|last| now.saturating_sub(last) >= interval)
            .unwrap_or(true);
        if !due {
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
            let release = check_release_status(config);
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

            let latest_items =
                latest_status_items_with_release(config, state, &daemon, now, &release)?;
            let signature = latest_signature(&latest_items);
            let should_write_latest = !active
                || self
                    .last_status_signature
                    .as_deref()
                    .map(|last| last != signature)
                    .unwrap_or(true)
                || self.last_user_service_write_at_ms.is_none();
            if should_write_latest {
                match update_user_service_latest(config, state, &daemon, latest_items) {
                    Ok(()) => {
                        self.last_user_service_write_at_ms = Some(now);
                        self.last_status_signature = Some(signature);
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
        self.last_control_emit_at_ms = Some(now);
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
    let runtimes = state
        .list_runtime_agent_definitions_for_daemon(&daemon.agent_did)?
        .into_iter()
        .map(|agent| runtime_status_payload(config, state, daemon, agent, &now, &release))
        .collect::<Result<Vec<_>>>()?;
    Ok(json!({
        "command": "agent.status.query",
        "daemon_agent_did": daemon.agent_did,
        "daemon": daemon_status_payload(config, daemon, &service, &now, &release),
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
        "daemon": daemon_status_payload(config, daemon, service, now, release),
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
        diagnostics_summary: daemon_diagnostics_summary(&service, release),
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
            version: Some(release.current_version.clone()),
            latest_version: release.latest_version.clone(),
            min_supported_version: None,
            platform: Some(crate::service::current_platform_label()),
            service: Some(service_label(service.platform).to_string()),
            needs_upgrade: release.needs_upgrade,
            needs_config: runtime_status.needs_config,
            last_error_code: runtime_status.last_error_code.clone(),
            last_error_summary: None,
            diagnostics_summary: runtime_diagnostics_summary(state, &runtime, &runtime_status),
        });
    }
    Ok(items)
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
    release: &DaemonReleaseStatus,
) -> Result<Value> {
    let runtime_status = runtime_status_summary(config, state, &runtime);
    Ok(json!({
        "agent_did": runtime.agent_did,
        "daemon_agent_did": daemon.agent_did,
        "runtime": runtime_name_from_plugin(runtime.runtime_plugin_id.as_deref()),
        "runtime_profile_id": runtime.runtime_profile_id,
        "status": if runtime_status.needs_config { "needs_config" } else { "ready" },
        "last_seen_at": now,
        "version": release.current_version.clone(),
        "latest_version": release.latest_version.clone(),
        "needs_config": runtime_status.needs_config,
        "needs_upgrade": release.needs_upgrade,
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
        "diagnostics_summary": daemon_diagnostics_summary(service, release),
    })
}

fn daemon_diagnostics_summary(service: &ServiceStatus, release: &DaemonReleaseStatus) -> Value {
    json!({
        "installation_status": if service.installed { "installed" } else { "not_installed" },
        "runner_status": if service.running { "running" } else { "not_running" },
        "config_summary": {
            "service_installed": service.installed,
            "release_manifest_url": release.manifest_url.clone(),
            "release_status": if release.error.is_some() { "unavailable" } else { "ok" },
            "release_error": release.error.clone(),
        },
    })
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
    let install_status = GenericCliDriverRegistry::new(profile).check_install_status();
    let missing_binary = install_status
        .as_ref()
        .map(|status| !status.installed)
        .unwrap_or(true);
    let last_error_code = if missing_config_home {
        Some("generic_cli_config_home_missing".to_string())
    } else if missing_binary {
        Some("generic_cli_driver_missing".to_string())
    } else if install_status.is_err() {
        Some("generic_cli_driver_probe_failed".to_string())
    } else {
        None
    };
    (missing_config_home || missing_binary, last_error_code)
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
                "profile_concurrency_cap_supported": false,
                "supported_drivers": supported_generic_cli_drivers(),
                "supported_workspace_modes": supported_generic_cli_workspace_modes(),
                "supported_sandbox_modes": supported_generic_cli_sandbox_modes(),
                "supported_runtime_create_args": supported_generic_cli_runtime_create_args(),
                "binary_installed": false,
                "binary_detail": "missing runtime_profile_id",
                "driver_status_code": "profile_missing",
                "next_action": "manual_review_required",
                "auth_status": "unknown",
                "home_isolation": "unknown",
                "host_home_shared_lock": false,
                "config_home": null,
                "config_home_exists": false,
                "default_workspace_mode": null,
                "default_sandbox": null,
                "route_hash": generic_cli_route_hash_summary(),
                "route_session_counts": empty_route_session_counts(),
                "max_parallel_runs_per_profile": 1,
                "runtime_target_required": true,
                "setup": generic_cli_setup_summary(
                    None,
                    "profile_missing",
                    "manual_review_required",
                    "unknown",
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
                "profile_concurrency_cap_supported": false,
                "supported_drivers": supported_generic_cli_drivers(),
                "supported_workspace_modes": supported_generic_cli_workspace_modes(),
                "supported_sandbox_modes": supported_generic_cli_sandbox_modes(),
                "supported_runtime_create_args": supported_generic_cli_runtime_create_args(),
                "binary_installed": false,
                "binary_detail": "missing cli runtime profile",
                "driver_status_code": "profile_missing",
                "next_action": "manual_review_required",
                "auth_status": "unknown",
                "home_isolation": "unknown",
                "host_home_shared_lock": false,
                "config_home": null,
                "config_home_exists": false,
                "default_workspace_mode": null,
                "default_sandbox": null,
                "route_hash": generic_cli_route_hash_summary(),
                "route_session_counts": empty_route_session_counts(),
                "max_parallel_runs_per_profile": 1,
                "runtime_target_required": true,
                "setup": generic_cli_setup_summary(
                    None,
                    "profile_missing",
                    "manual_review_required",
                    "unknown",
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
    let next_action = generic_cli_next_action(driver_status_code);
    let home_isolation = generic_cli_home_isolation(&profile, config_home_exists);
    let host_home_shared_lock =
        profile.driver_id == "claude-code" && home_isolation == "host_default";
    let route_session_counts =
        generic_cli_route_session_counts(state, runtime_profile_id, &runtime.controller_scope_key);
    let active_session_count = route_session_counts
        .get("active")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    json!({
        "profile_status": profile.status,
        "driver_id": profile.driver_id,
        "active_session_count": active_session_count,
        "config_summary": {
            "driver_id": profile.driver_id,
            "capability_schema_version": 1,
            "route_session_supported": true,
            "native_resume_supported": true,
            "profile_concurrency_cap_supported": false,
            "supported_drivers": supported_generic_cli_drivers(),
            "supported_workspace_modes": supported_generic_cli_workspace_modes(),
            "supported_sandbox_modes": supported_generic_cli_sandbox_modes(),
            "supported_runtime_create_args": supported_generic_cli_runtime_create_args(),
            "binary_installed": install_status.installed,
            "binary_detail": install_status.detail.map(|detail| sanitize_public_error(&detail)),
            "driver_status_code": driver_status_code,
            "next_action": next_action,
            "auth_status": "unknown",
            "home_isolation": home_isolation,
            "host_home_shared_lock": host_home_shared_lock,
            "config_home": if profile.config_home.is_some() { "configured" } else { "missing" },
            "config_home_exists": config_home_exists,
            "default_workspace_mode": profile.default_workspace_mode.as_str(),
            "default_sandbox": profile.default_sandbox,
            "route_hash": generic_cli_route_hash_summary(),
            "route_session_counts": route_session_counts,
            "max_parallel_runs_per_profile": 1,
            "runtime_target_required": true,
            "setup": generic_cli_setup_summary(
                Some(&profile.driver_id),
                driver_status_code,
                next_action,
                home_isolation,
            ),
            "driver_args_schema_version": generic_cli_driver_args_schema_version(&profile.driver_id),
            "driver_capabilities": generic_cli_driver_capabilities(Some(&profile.driver_id)),
        },
    })
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

fn empty_route_session_counts() -> Value {
    json!({
        "total": 0,
        "active": 0,
        "running": 0,
        "failed": 0,
        "reset": 0,
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

fn generic_cli_route_hash_summary() -> Value {
    json!({
        "algorithm": "sha256",
        "version": "v1",
        "keyed": false,
        "salt_disclosed": false,
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

fn generic_cli_next_action(driver_status_code: &str) -> &'static str {
    match driver_status_code {
        "ok" => "none",
        "missing_binary" => "install_driver",
        "config_home_missing" => "manual_review_required",
        "probe_failed" => "manual_review_required",
        "not_implemented" | "unsupported_driver" => "upgrade_daemon",
        "profile_missing" => "manual_review_required",
        _ => "manual_review_required",
    }
}

fn generic_cli_setup_summary(
    driver_id: Option<&str>,
    driver_status_code: &str,
    next_action: &str,
    home_isolation: &str,
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
        "auth_status": "unknown",
        "home_isolation": home_isolation,
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
            format!(
                "{}:{}:{}:{}:{}:{}:{}",
                item.agent_did,
                item.status,
                item.version.as_deref().unwrap_or_default(),
                item.latest_version.as_deref().unwrap_or_default(),
                item.needs_upgrade,
                item.needs_config,
                item.last_error_code.as_deref().unwrap_or_default()
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
    use crate::agent::AgentDefinition;
    use crate::plugins::generic_cli::GENERIC_CLI_RUNTIME_PLUGIN_ID;
    use crate::plugins::hermes::{AWIKI_SKILLS_VERSION, HERMES_RUNTIME_PLUGIN_ID};
    use crate::state::{CliRuntimeProfileRecord, CreateCliRouteSession, HermesProfileRecord};
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

    fn allowed_latest_diagnostics_keys() -> BTreeSet<&'static str> {
        [
            "installation_status",
            "profile_status",
            "runner_status",
            "active_session_count",
            "runtime_version",
            "driver_id",
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

        let runtime_item = items
            .iter()
            .find(|item| item.agent_kind == AgentKind::Runtime)
            .unwrap();
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
        cli_profile.config_home = Some(config_home);
        state.upsert_cli_runtime_profile(&cli_profile).unwrap();

        let status = runtime_status_summary(&config, &state, &runtime);
        let diagnostics = runtime_diagnostics_summary(&state, &runtime, &status);

        assert!(status.needs_config);
        assert_eq!(
            status.last_error_code.as_deref(),
            Some("generic_cli_driver_missing")
        );
        assert_eq!(diagnostics["profile_status"], "active");
        assert_eq!(diagnostics["driver_id"], "codex");
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
            false
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
        assert_eq!(
            diagnostics["config_summary"]["next_action"],
            "install_driver"
        );
        assert_eq!(diagnostics["config_summary"]["auth_status"], "unknown");
        assert_eq!(
            diagnostics["config_summary"]["home_isolation"],
            "profile_home"
        );
        assert_eq!(
            diagnostics["config_summary"]["host_home_shared_lock"],
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
            "sha256"
        );
        assert_eq!(diagnostics["config_summary"]["route_hash"]["keyed"], false);
        assert_eq!(
            diagnostics["config_summary"]["route_hash"]["salt_disclosed"],
            false
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
        assert_eq!(diagnostics["driver_id"], "claude-code");
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
