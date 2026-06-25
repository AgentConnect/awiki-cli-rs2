use anyhow::{bail, Result};
use serde_json::{json, Value};
use time::format_description::well_known::Rfc3339;

use crate::agent::{AgentDefinition, AgentKind};
use crate::outbox::{AgentManagementOutbox, AgentStatusResponse};
use crate::plugins::generic_cli::status::GenericCliStatusSummary;
use crate::plugins::hermes::{
    ensure_runtime_model_config, hermes_runtime_model_config_status,
    repair_hermes_profile_if_needed, HermesGatewayCommandStatus, HermesRuntimeModelConfigStatus,
    StdioHermesGateway,
};
use crate::public_error::sanitize_public_error;
use crate::registration::{
    AgentInventoryClient, AgentLatestStatusUpdateItem, DidAuthMaterial,
    UserServiceAgentRegistrationClient,
};
use crate::runtime::{RuntimeRun, RuntimeTask};
use crate::security::runtime_token::current_time_millis;
use crate::service::{manage_service, ServiceAction, ServicePlatform, ServiceStatus};
use crate::state::DaemonState;
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
            reconcile_daemon_upgrade_state(state, &daemon, &release)?;
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
    reconcile_daemon_upgrade_state(state, daemon, &release)?;
    let runtimes = state
        .list_runtime_agent_definitions_for_daemon(&daemon.agent_did)?
        .into_iter()
        .map(|agent| runtime_status_payload(config, state, daemon, agent, &now))
        .collect::<Result<Vec<_>>>()?;
    let runs = active_runtime_run_payloads(state, daemon, &now)?;
    Ok(json!({
        "command": "agent.status.query",
        "schema": "awiki.agent.status.v1",
        "event_id": format!("evt_{}", current_time_millis().unwrap_or(0)),
        "sent_at": now,
        "daemon_agent_did": daemon.agent_did,
        "status_scope": "snapshot",
        "state": "ready",
        "daemon": daemon_status_payload(config, daemon, &service, &now, &release),
        "runtimes": runtimes,
        "runs": runs,
    }))
}

fn active_runtime_run_payloads(
    state: &DaemonState,
    daemon: &AgentDefinition,
    now: &str,
) -> Result<Vec<Value>> {
    state
        .list_active_runtime_runs_for_daemon(&daemon.agent_did)?
        .into_iter()
        .map(|(run, task)| Ok(runtime_run_status_payload(&run, &task, now)))
        .collect()
}

fn runtime_run_status_payload(run: &RuntimeRun, task: &RuntimeTask, now: &str) -> Value {
    json!({
        "run_id": run.run_id,
        "message_id": task.task_id.strip_prefix("task_").unwrap_or(&task.task_id),
        "task_id": task.task_id,
        "runtime_agent_did": run.agent_did,
        "runtime_agent_handle": task.agent_handle,
        "agent_did": run.agent_did,
        "conversation_id": task.conversation_id,
        "status": run.status.as_str(),
        "started_at": now,
        "updated_at": now,
        "requester_did": task.requester_did,
        "requester_full_handle": task.requester_full_handle,
        "trigger_kind": task.trigger_kind.as_str(),
        "last_error_code": null,
        "last_error_summary": null,
    })
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
    latest_status_items_with_runtime_status(
        config,
        state,
        daemon,
        now_ms,
        release,
        |config, state, runtime| runtime_status_summary(config, state, runtime),
    )
}

fn latest_status_items_with_runtime_status(
    config: &DaemonConfig,
    state: &DaemonState,
    daemon: &AgentDefinition,
    now_ms: i64,
    release: &DaemonReleaseStatus,
    runtime_status: impl Fn(&DaemonConfig, &DaemonState, &AgentDefinition) -> RuntimeStatusSummary,
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
        let runtime_status = runtime_status(config, state, &runtime);
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
    is_generic_cli: bool,
    needs_config: bool,
    last_error_code: Option<String>,
    gateway_command_status: Option<HermesGatewayCommandStatus>,
    model_config_status: Option<HermesRuntimeModelConfigStatus>,
    generic_cli: Option<GenericCliStatusSummary>,
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
    let is_generic_cli =
        runtime.runtime_plugin_id.as_deref() == Some(crate::agent::GENERIC_CLI_RUNTIME_PLUGIN_ID);
    if is_generic_cli {
        let generic_cli = generic_cli_status_summary(state, runtime);
        return RuntimeStatusSummary {
            is_hermes: false,
            is_generic_cli: true,
            needs_config: !generic_cli.setup_ready,
            last_error_code: generic_cli.error_code().map(str::to_string),
            gateway_command_status: None,
            model_config_status: None,
            generic_cli: Some(generic_cli),
        };
    }
    if !is_hermes {
        return RuntimeStatusSummary {
            is_hermes,
            is_generic_cli,
            needs_config: false,
            last_error_code: None,
            gateway_command_status: None,
            model_config_status: None,
            generic_cli: None,
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
        is_generic_cli,
        needs_config: profile_needs_config || gateway_command_status.needs_config(),
        last_error_code,
        gateway_command_status: Some(gateway_command_status),
        model_config_status,
        generic_cli: None,
    }
}

fn runtime_diagnostics_summary(
    state: &DaemonState,
    runtime: &AgentDefinition,
    runtime_status: &RuntimeStatusSummary,
) -> Value {
    if runtime.runtime_plugin_id.as_deref() == Some(crate::agent::GENERIC_CLI_RUNTIME_PLUGIN_ID) {
        let generic_cli = runtime_status
            .generic_cli
            .clone()
            .unwrap_or_else(|| generic_cli_status_summary(state, runtime));
        return generic_cli.diagnostics_summary();
    }
    if runtime.runtime_plugin_id.as_deref()
        != Some(crate::plugins::hermes::HERMES_RUNTIME_PLUGIN_ID)
    {
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

fn generic_cli_status_summary(
    state: &DaemonState,
    runtime: &AgentDefinition,
) -> GenericCliStatusSummary {
    let Some(runtime_profile_id) = runtime.runtime_profile_id.as_deref() else {
        return GenericCliStatusSummary::missing("missing runtime_profile_id");
    };
    let Ok(profile) = state.load_cli_runtime_profile(runtime_profile_id) else {
        return GenericCliStatusSummary::missing("missing cli runtime profile");
    };
    GenericCliStatusSummary::from_profile(profile)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::AgentDefinition;
    use crate::plugins::hermes::{AWIKI_SKILLS_VERSION, HERMES_RUNTIME_PLUGIN_ID};
    use crate::runtime::{
        RuntimeConversationScope, RuntimeInvocationAuthority, RuntimeRunStatus,
        RuntimeTaskTriggerKind,
    };
    use crate::state::{CliRuntimeProfileRecord, HermesProfileRecord};
    use std::collections::BTreeSet;
    use std::sync::{Mutex, MutexGuard};

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    struct EnvGuard {
        _lock: MutexGuard<'static, ()>,
        values: Vec<(&'static str, Option<String>)>,
    }

    impl EnvGuard {
        fn clear(keys: &[&'static str]) -> Self {
            let lock = ENV_LOCK.lock().unwrap();
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

    fn generic_cli_runtime(driver_id: &str) -> AgentDefinition {
        AgentDefinition {
            agent_did: format!("did:agent:{driver_id}"),
            handle: format!("alice-{driver_id}"),
            agent_kind: AgentKind::Runtime,
            controller_user_id: TEST_CONTROLLER_USER_ID.to_string(),
            controller_full_handle: TEST_CONTROLLER_FULL_HANDLE.to_string(),
            controller_scope_key: TEST_CONTROLLER_SCOPE_KEY.to_string(),
            controller_did: "did:human:alice".to_string(),
            runtime_plugin_id: Some(crate::agent::GENERIC_CLI_RUNTIME_PLUGIN_ID.to_string()),
            runtime_profile_id: Some(format!("profile_{driver_id}_alice")),
            workspace_id: Some(format!("workspace_{driver_id}_alice")),
            policy_id: "default".to_string(),
            local_agent_db_path: format!("agents/{driver_id}/agent.db"),
            message_db_path: format!("agents/{driver_id}/messages.db"),
            status: "active".to_string(),
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
    fn daemon_snapshot_payload_includes_active_runtime_runs() {
        let root = tempfile::tempdir().unwrap();
        let config = DaemonConfig::for_state_root(root.path()).unwrap();
        config.ensure_state_layout().unwrap();
        let state = DaemonState::open(&config).unwrap();
        state.initialize().unwrap();
        let daemon = daemon();
        let runtime = generic_cli_runtime("codex");
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
            .insert_runtime_task(&RuntimeTask {
                task_id: "task_msg_codex_active".to_string(),
                agent_did: runtime.agent_did.clone(),
                agent_handle: runtime.handle.clone(),
                controller_user_id: daemon.controller_user_id.clone(),
                controller_full_handle: daemon.controller_full_handle.clone(),
                controller_scope_key: daemon.controller_scope_key.clone(),
                controller_did: daemon.controller_did.clone(),
                sender_did: daemon.controller_did.clone(),
                requester_did: daemon.controller_did.clone(),
                requester_user_id: None,
                requester_full_handle: None,
                trigger_kind: RuntimeTaskTriggerKind::ControllerDirect,
                conversation_scope: RuntimeConversationScope::controller_private(
                    daemon.controller_scope_key.clone(),
                ),
                invocation_authority: RuntimeInvocationAuthority::Controller,
                reply_recipient_did: daemon.controller_did.clone(),
                conversation_id: Some("conv_codex_active".to_string()),
                text: "still running".to_string(),
            })
            .unwrap();
        state
            .insert_runtime_run(&RuntimeRun {
                run_id: "run_task_msg_codex_active".to_string(),
                task_id: "task_msg_codex_active".to_string(),
                agent_did: runtime.agent_did.clone(),
                runtime_profile_id: runtime.runtime_profile_id.clone().unwrap(),
                runtime_plugin_id: runtime.runtime_plugin_id.clone().unwrap(),
                workspace_id: runtime.workspace_id.clone(),
                status: RuntimeRunStatus::Running,
            })
            .unwrap();

        let payload = daemon_snapshot_payload(&config, &state, &daemon).unwrap();

        assert_eq!(payload["schema"], "awiki.agent.status.v1");
        assert_eq!(payload["status_scope"], "snapshot");
        let runs = payload["runs"].as_array().unwrap();
        assert_eq!(runs.len(), 1);
        let run = &runs[0];
        assert_eq!(run["run_id"], "run_task_msg_codex_active");
        assert_eq!(run["message_id"], "msg_codex_active");
        assert_eq!(run["task_id"], "task_msg_codex_active");
        assert_eq!(run["runtime_agent_did"], runtime.agent_did);
        assert_eq!(run["runtime_agent_handle"], runtime.handle);
        assert_eq!(run["conversation_id"], "conv_codex_active");
        assert_eq!(run["status"], "running");
        assert_eq!(run["requester_did"], daemon.controller_did);
        assert_eq!(run["trigger_kind"], "controller_direct");
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

        let release = DaemonReleaseStatus {
            current_version: crate::upgrade::CURRENT_DAEMON_VERSION.to_string(),
            latest_version: None,
            needs_upgrade: false,
            manifest_url: "https://example.test/daemon/releases/latest.json".to_string(),
            error: None,
        };
        let items = latest_status_items_with_runtime_status(
            &config,
            &state,
            &daemon,
            1_700_000_000_000,
            &release,
            |config, state, runtime| {
                runtime_status_summary_with_gateway_status(config, state, runtime, || {
                    HermesGatewayCommandStatus::Configured
                })
            },
        )
        .unwrap();
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
    fn latest_status_items_reports_codex_setup_readiness() {
        let root = tempfile::tempdir().unwrap();
        let config = DaemonConfig::for_state_root(root.path()).unwrap();
        config.ensure_state_layout().unwrap();
        let state = DaemonState::open(&config).unwrap();
        state.initialize().unwrap();
        let daemon = daemon();
        let runtime = generic_cli_runtime("codex");
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

        let fake_codex = root.path().join("bin").join("codex");
        write_fake_codex_executable(&fake_codex).unwrap();
        let config_home = root.path().join("codex-home");
        std::fs::create_dir_all(&config_home).unwrap();
        let mut cli_profile = CliRuntimeProfileRecord::for_driver(
            runtime.runtime_profile_id.as_deref().unwrap(),
            "codex",
        )
        .unwrap();
        cli_profile.binary_path = Some(fake_codex);
        cli_profile.config_home = Some(config_home.clone());
        state.upsert_cli_runtime_profile(&cli_profile).unwrap();

        let release = DaemonReleaseStatus {
            current_version: crate::upgrade::CURRENT_DAEMON_VERSION.to_string(),
            latest_version: None,
            needs_upgrade: false,
            manifest_url: "https://example.test/daemon/releases/latest.json".to_string(),
            error: None,
        };
        let items =
            latest_status_items_with_release(&config, &state, &daemon, 1_700_000_000_000, &release)
                .unwrap();
        let runtime_item = items
            .iter()
            .find(|item| item.agent_did == runtime.agent_did)
            .unwrap();
        assert_eq!(runtime_item.status, "needs_config");
        assert!(runtime_item.needs_config);
        assert_eq!(
            runtime_item.last_error_code.as_deref(),
            Some("generic_cli_auth_missing")
        );
        assert_eq!(
            runtime_item.diagnostics_summary["config_summary"]["auth_status"],
            "missing"
        );
        assert_eq!(
            runtime_item.diagnostics_summary["config_summary"]["setup_status"],
            "needs_setup"
        );

        std::fs::write(config_home.join("auth.json"), "{}").unwrap();
        let items =
            latest_status_items_with_release(&config, &state, &daemon, 1_700_000_000_000, &release)
                .unwrap();
        let runtime_item = items
            .iter()
            .find(|item| item.agent_did == runtime.agent_did)
            .unwrap();
        assert_eq!(runtime_item.status, "ready");
        assert!(!runtime_item.needs_config);
        assert!(runtime_item.last_error_code.is_none());
        assert_eq!(
            runtime_item.diagnostics_summary["config_summary"]["auth_status"],
            "ok"
        );
        assert_eq!(
            runtime_item.diagnostics_summary["config_summary"]["setup_ready"],
            true
        );
        assert_eq!(
            runtime_item.diagnostics_summary["config_summary"]["setup_status"],
            "ready"
        );
        let dump = runtime_item.diagnostics_summary.to_string();
        assert!(!dump.contains(root.path().to_string_lossy().as_ref()));
        assert!(!dump.contains("auth.json"));
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

#[cfg(test)]
fn write_fake_codex_executable(path: &std::path::Path) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(
        path,
        r#"#!/bin/sh
if [ "${1-}" = "--version" ]; then
  echo "codex-cli 9.9.9"
  exit 0
fi
cat >/dev/null
exit 0
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
