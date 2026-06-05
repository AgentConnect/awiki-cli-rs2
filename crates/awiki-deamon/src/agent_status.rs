use anyhow::Result;
use serde_json::{json, Value};
use time::format_description::well_known::Rfc3339;

use crate::agent::{AgentDefinition, AgentKind};
use crate::outbox::{AgentManagementOutbox, AgentStatusResponse};
use crate::plugins::hermes::HermesGatewayCommandStatus;
use crate::registration::{
    AgentInventoryClient, AgentLatestStatusUpdateItem, DidAuthMaterial,
    UserServiceAgentRegistrationClient,
};
use crate::security::runtime_token::current_time_millis;
use crate::service::{manage_service, ServiceAction, ServicePlatform, ServiceStatus};
use crate::state::DaemonState;
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
            if let Err(error) = emit_daemon_heartbeat(config, state, im_core, outbox, &daemon) {
                record_status_error(
                    state,
                    &daemon,
                    "daemon.status.heartbeat.control_failed",
                    &error.to_string(),
                )?;
            } else {
                emitted = true;
            }

            let latest_items = latest_status_items(config, state, &daemon, now)?;
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
    let runtimes = state
        .list_runtime_agent_definitions_for_daemon(&daemon.agent_did)?
        .into_iter()
        .map(|agent| runtime_status_payload(config, state, daemon, agent, &now))
        .collect::<Result<Vec<_>>>()?;
    Ok(json!({
        "command": "agent.status.query",
        "daemon_agent_did": daemon.agent_did,
        "daemon": daemon_status_payload(daemon, &service, &now),
        "runtimes": runtimes,
        "runs": [],
    }))
}

pub fn daemon_lightweight_payload(config: &DaemonConfig, daemon: &AgentDefinition) -> Value {
    let now = rfc3339_now();
    let service = service_status(config);
    json!({
        "schema": "awiki.agent.status.v1",
        "event_id": format!("evt_{}", current_time_millis().unwrap_or(0)),
        "sent_at": now,
        "daemon_agent_did": daemon.agent_did,
        "status_scope": "daemon",
        "command_id": null,
        "state": "ready",
        "message": "daemon heartbeat",
        "daemon": daemon_status_payload(daemon, &service, &now),
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
    let last_seen_at = Some(rfc3339_from_millis(now_ms));
    let service = service_status(config);
    let mut items = vec![AgentLatestStatusUpdateItem {
        agent_did: daemon.agent_did.clone(),
        agent_kind: AgentKind::Daemon,
        status: "ready".to_string(),
        last_seen_at: last_seen_at.clone(),
        version: Some(env!("CARGO_PKG_VERSION").to_string()),
        min_supported_version: Some("0.1.0".to_string()),
        platform: Some(crate::service::current_platform_label()),
        service: Some(service_label(service.platform).to_string()),
        needs_upgrade: false,
        needs_config: false,
        last_error_code: None,
        last_error_summary: None,
        diagnostics_summary: json!({
            "installation_status": if service.installed { "installed" } else { "not_installed" },
            "runner_status": if service.running { "running" } else { "not_running" },
            "config_summary": {
                "service_installed": service.installed,
            },
        }),
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
            version: Some(env!("CARGO_PKG_VERSION").to_string()),
            min_supported_version: Some("0.1.0".to_string()),
            platform: Some(crate::service::current_platform_label()),
            service: Some(service_label(service.platform).to_string()),
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
    sync_controller_did_from_latest_response(state, &daemon.agent_did, &response)?;
    Ok(())
}

pub fn sync_controller_did_from_latest_response(
    state: &DaemonState,
    daemon_agent_did: &str,
    response: &Value,
) -> Result<()> {
    let Some(controller_did) = controller_did_from_latest_response(daemon_agent_did, response)
    else {
        return Ok(());
    };
    let local = state.load_agent_definition(daemon_agent_did)?;
    if local.controller_did == controller_did {
        return Ok(());
    }
    state.update_controller_did_for_agent_family(daemon_agent_did, &controller_did)?;
    state.insert_audit_event_json(
        "daemon.controller_did.synced",
        Some(daemon_agent_did),
        None,
        None,
        None,
        json!({
            "old_controller_did": local.controller_did,
            "new_controller_did": controller_did,
        }),
    )?;
    Ok(())
}

fn controller_did_from_latest_response(daemon_agent_did: &str, response: &Value) -> Option<String> {
    response
        .get("updated")
        .and_then(Value::as_array)?
        .iter()
        .filter(|item| {
            item.get("agent_did")
                .and_then(Value::as_str)
                .map(|did| did == daemon_agent_did)
                .unwrap_or(false)
        })
        .find_map(|item| item.get("controller_did").and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn emit_daemon_heartbeat<O>(
    config: &DaemonConfig,
    state: &DaemonState,
    im_core: &ImCoreAdapter,
    outbox: &O,
    daemon: &AgentDefinition,
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
        payload: daemon_lightweight_payload(config, daemon),
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
        "display_name": runtime.handle,
        "runtime": runtime_name_from_plugin(runtime.runtime_plugin_id.as_deref()),
        "runtime_profile_id": runtime.runtime_profile_id,
        "status": if runtime_status.needs_config { "needs_config" } else { "ready" },
        "last_seen_at": now,
        "needs_config": runtime_status.needs_config,
        "needs_upgrade": false,
        "last_error_code": runtime_status.last_error_code,
        "last_error_summary": null,
        "diagnostics_summary": runtime_diagnostics_summary(state, &runtime, &runtime_status),
    }))
}

fn daemon_status_payload(daemon: &AgentDefinition, service: &ServiceStatus, now: &str) -> Value {
    json!({
        "agent_did": daemon.agent_did,
        "display_name": daemon.handle,
        "status": "ready",
        "last_seen_at": now,
        "version": env!("CARGO_PKG_VERSION"),
        "min_supported_version": "0.1.0",
        "platform": crate::service::current_platform_label(),
        "service": service_label(service.platform),
        "needs_upgrade": false,
        "last_error_code": null,
        "last_error_summary": null,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RuntimeStatusSummary {
    is_hermes: bool,
    needs_config: bool,
    last_error_code: Option<String>,
    gateway_command_status: Option<HermesGatewayCommandStatus>,
}

fn runtime_status_summary(
    config: &DaemonConfig,
    state: &DaemonState,
    runtime: &AgentDefinition,
) -> RuntimeStatusSummary {
    runtime_status_summary_with_gateway_status(state, runtime, || {
        crate::plugins::hermes::StdioHermesGateway::from_config_without_detection(config)
            .gateway_command_status()
    })
}

fn runtime_status_summary_with_gateway_status(
    state: &DaemonState,
    runtime: &AgentDefinition,
    gateway_status: impl FnOnce() -> HermesGatewayCommandStatus,
) -> RuntimeStatusSummary {
    let is_hermes = runtime.runtime_plugin_id.as_deref()
        == Some(crate::plugins::hermes::HERMES_RUNTIME_PLUGIN_ID);
    if !is_hermes {
        return RuntimeStatusSummary {
            is_hermes,
            needs_config: false,
            last_error_code: None,
            gateway_command_status: None,
        };
    }
    let profile_needs_config = state
        .load_hermes_profile(&runtime.agent_did)
        .map(|profile| profile.status != "ready")
        .unwrap_or(true);
    let gateway_command_status = gateway_status();
    RuntimeStatusSummary {
        is_hermes,
        needs_config: profile_needs_config || gateway_command_status.needs_config(),
        last_error_code: gateway_command_status.error_code().map(str::to_string),
        gateway_command_status: Some(gateway_command_status),
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
            },
        }),
        Err(_) => json!({
            "profile_status": "missing",
            "config_summary": {
                "gateway_command": runtime_status
                    .gateway_command_status
                    .map(HermesGatewayCommandStatus::as_str)
                    .unwrap_or("unknown"),
            },
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
                "{}:{}:{}:{}:{}",
                item.agent_did,
                item.status,
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
    use crate::plugins::hermes::{AWIKI_SKILLS_VERSION, HERMES_RUNTIME_PLUGIN_ID};
    use crate::state::HermesProfileRecord;
    use std::collections::BTreeSet;

    fn daemon() -> AgentDefinition {
        AgentDefinition {
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
        }
    }

    fn hermes_runtime() -> AgentDefinition {
        AgentDefinition {
            agent_did: "did:agent:hermes".to_string(),
            handle: "alice-hermes".to_string(),
            agent_kind: AgentKind::Runtime,
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

    fn allowed_latest_diagnostics_keys() -> BTreeSet<&'static str> {
        [
            "installation_status",
            "profile_status",
            "runner_status",
            "active_session_count",
            "runtime_version",
            "config_summary",
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
        let dump = payload.to_string();
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
        let root = tempfile::tempdir().unwrap();
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

        let missing_gateway = runtime_status_summary_with_gateway_status(&state, &runtime, || {
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

        let configured_gateway =
            runtime_status_summary_with_gateway_status(&state, &runtime, || {
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
