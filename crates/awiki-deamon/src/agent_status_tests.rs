const HERMES_RUNTIME_PLUGIN_ID: &str = "runtime.hermes";
const GENERIC_CLI_RUNTIME_PLUGIN_ID: &str = "generic-cli";

use super::*;
use crate::agent::AgentDefinition;
use crate::outbox::MemoryRuntimeOutbox;
use crate::runtime::{
    RuntimeConversationScope, RuntimeInvocationAuthority, RuntimeTask, RuntimeTaskTriggerKind,
};
use crate::state::{BootstrapReplayRecord, CliRuntimeProfileRecord, UserDelegatedIdentityRecord};
use std::collections::BTreeSet;
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
    let acp = &payload["daemon"]["diagnostics_summary"]["config_summary"]["acp"];
    assert_eq!(
        acp["supported_drivers"],
        json!(crate::acp::SUPPORTED_DRIVERS)
    );
    assert_eq!(acp["protocol_version"], 1);
    let dump = payload.to_string();
    assert!(!dump.contains("token"));
    assert!(!dump.contains("private"));
}

#[test]
fn delegated_subkey_status_proposal_is_public_only_v3() {
    let value = delegated_subkey_proposal_value(&crate::identity_custody::PreparedDaemonSubkey {
        user_did: "did:wba:awiki.test:user:alice:e1_user".to_string(),
        verification_method: "did:wba:awiki.test:user:alice:e1_user#daemon-key-1".to_string(),
        public_key_multibase: "zPublic".to_string(),
    });

    assert_eq!(
        value["schema"],
        crate::app_bridge::bootstrap::USER_SUBKEY_PACKAGE_SCHEMA_V3
    );
    assert_eq!(value["key_algorithm"], "Ed25519");
    assert!(!value.to_string().contains("private"));
}

#[test]
fn daemon_snapshot_payload_includes_daemon_diagnostics_summary() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    config.ensure_state_layout().unwrap();
    let state = DaemonState::open_with_root_key_bytes(&config, [24_u8; 32]);
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
        payload["daemon"]["diagnostics_summary"]["config_summary"]["acp"]["supported_drivers"],
        json!(crate::acp::SUPPORTED_DRIVERS)
    );
}

#[test]
fn daemon_status_exposes_public_bootstrap_key_from_daemon_identity() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    config.ensure_state_layout().unwrap();
    let state = DaemonState::open_with_root_key_bytes(&config, [24_u8; 32]);
    state.initialize().unwrap();
    let identity = crate::registration::store_mock_vnext_device_identity(
        &config,
        &state,
        AgentKind::Daemon,
        "alice-mac-daemon",
    )
    .unwrap();
    let mut daemon = daemon();
    daemon.agent_did = identity.agent_did.clone();
    daemon.handle = identity.full_handle.clone();
    state.upsert_agent_definition(&daemon).unwrap();

    let payload = daemon_snapshot_payload(&config, &state, &daemon).unwrap();
    let diagnostics = &payload["daemon"]["diagnostics_summary"];

    let config_summary = &diagnostics["config_summary"];
    assert_eq!(
        config_summary["bootstrap_key_id"],
        identity.device_e2ee_key_id
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
    let state = DaemonState::open_with_root_key_bytes(&config, [24_u8; 32]);
    state.initialize().unwrap();
    let identity = crate::registration::store_mock_vnext_device_identity(
        &config,
        &state,
        AgentKind::Daemon,
        "alice-mac-daemon",
    )
    .unwrap();
    let mut daemon = daemon();
    daemon.agent_did = identity.agent_did.clone();
    state.upsert_agent_definition(&daemon).unwrap();

    let items = latest_status_items(&config, &state, &daemon, 1_700_000_000_000).unwrap();
    let daemon_item = items
        .iter()
        .find(|item| item.agent_kind == AgentKind::Daemon)
        .unwrap();

    assert_eq!(
        daemon_item.diagnostics_summary["config_summary"]["bootstrap_key_id"],
        identity.device_e2ee_key_id
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

#[tokio::test]
async fn daemon_heartbeat_includes_public_bootstrap_key_without_private_material() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    config.ensure_state_layout().unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();
    let identity = crate::registration::store_mock_vnext_device_identity(
        &config,
        &state,
        AgentKind::Daemon,
        "alice-mac-daemon",
    )
    .unwrap();
    let mut daemon = daemon();
    daemon.agent_did = identity.agent_did.clone();
    state.upsert_agent_definition(&daemon).unwrap();
    let im_core = ImCoreAdapter::open(&config).unwrap();
    im_core.initialize_local_state().await.unwrap();
    let outbox = MemoryRuntimeOutbox::default();
    let release = DaemonReleaseStatus {
        current_version: "test".to_string(),
        latest_version: None,
        minimum_supported_version: None,
        needs_upgrade: false,
        manifest_url: "test://release".to_string(),
        policy_url: "test://policy".to_string(),
        policy_origin: None,
        policy_revision: None,
        policy_source: None,
        error: Some("offline-test".to_string()),
    };

    emit_daemon_heartbeat(&config, &state, &im_core, &outbox, &daemon, &release).unwrap();

    let statuses = outbox.agent_statuses();
    assert_eq!(statuses.len(), 1);
    let payload = &statuses[0].payload;
    assert_eq!(payload["schema"], "awiki.agent.status.v1");
    let config_summary = &payload["daemon"]["diagnostics_summary"]["config_summary"];
    assert_eq!(config_summary["bootstrap_key_status"], "ready");
    assert_eq!(
        config_summary["bootstrap_key_id"],
        identity.device_e2ee_key_id
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

    record_controller_identity_changed(&state, &daemon.agent_did, "test_authoritative_status")
        .unwrap_err();
    emit_daemon_heartbeat(&config, &state, &im_core, &outbox, &daemon, &release).unwrap();
    assert_eq!(outbox.agent_statuses().len(), 2);
}

#[test]
fn latest_status_items_use_user_service_allowed_diagnostics_keys() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let state = DaemonState::open_with_root_key_bytes(&config, [7; 32]);
    state.initialize().unwrap();
    let daemon = daemon();
    state.upsert_agent_definition(&daemon).unwrap();
    for kind in crate::acp::SUPPORTED_DRIVERS {
        let mut runtime = hermes_runtime();
        runtime.agent_did = format!("did:agent:{kind}");
        runtime.runtime_profile_id = Some(format!("profile-{kind}"));
        runtime.runtime_plugin_id = Some(crate::acp::PLUGIN_ID.into());
        state.upsert_agent_definition(&runtime).unwrap();
        state
            .upsert_cli_runtime_profile(
                &CliRuntimeProfileRecord::for_driver(
                    runtime.runtime_profile_id.as_deref().unwrap(),
                    kind,
                )
                .unwrap(),
            )
            .unwrap();
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
    }
    let release = DaemonReleaseStatus {
        current_version: "test".to_string(),
        latest_version: None,
        minimum_supported_version: None,
        needs_upgrade: false,
        manifest_url: "test://release".to_string(),
        policy_url: "test://policy".to_string(),
        policy_origin: None,
        policy_revision: None,
        policy_source: None,
        error: Some("offline-test".to_string()),
    };
    let items = latest_status_items_with_release(&config, &state, &daemon, 1700000000000, &release)
        .unwrap();
    assert_eq!(items.len(), 8);
    let allowed = allowed_latest_diagnostics_keys();
    for item in &items {
        assert!(item
            .diagnostics_summary
            .as_object()
            .unwrap()
            .keys()
            .all(|key| allowed.contains(key.as_str())));
        if item.agent_kind == AgentKind::Runtime {
            assert_eq!(item.status, "ready");
            assert_eq!(
                item.diagnostics_summary["config_summary"]["protocol"],
                "acp"
            );
            assert!(item.diagnostics_summary["config_summary"]
                .get("gateway_command")
                .is_none());
        }
    }
    assert!(!serde_json::to_string(&items)
        .unwrap()
        .contains(root.path().to_str().unwrap()));
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

#[cfg(unix)]
#[test]
fn acp_runtime_diagnostics_fit_existing_inventory_contract() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    config.ensure_state_layout().unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();
    let mut runtime = generic_cli_runtime();
    runtime.runtime_plugin_id = Some(crate::acp::PLUGIN_ID.into());
    let id = runtime.runtime_profile_id.as_deref().unwrap();
    state
        .upsert_cli_runtime_profile(
            &crate::state::CliRuntimeProfileRecord::for_driver(id, "opencode").unwrap(),
        )
        .unwrap();
    let status = runtime_status_summary(&config, &state, &runtime);
    let diagnostics = runtime_diagnostics_summary(&state, &runtime, &status);
    let allowed = [
        "installation_status",
        "profile_status",
        "runner_status",
        "active_session_count",
        "runtime_version",
        "release_manifest_url",
        "release_status",
        "release_error",
        "config_summary",
    ];
    assert!(diagnostics
        .as_object()
        .unwrap()
        .keys()
        .all(|key| allowed.contains(&key.as_str())));
    assert_eq!(diagnostics["config_summary"]["protocol"], "acp");
    assert_eq!(diagnostics["config_summary"]["driver_id"], "opencode");
    assert!(!status.needs_config);
}

#[test]
fn controller_rebind_from_latest_status_fences_old_work_and_updates_live_binding() {
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

    let task = RuntimeTask {
        task_id: "task_before_controller_change".to_string(),
        agent_did: runtime.agent_did.clone(),
        agent_handle: runtime.handle.clone(),
        controller_user_id: daemon.controller_user_id.clone(),
        controller_full_handle: daemon.controller_full_handle.clone(),
        controller_scope_key: daemon.controller_scope_key.clone(),
        controller_did: daemon.controller_did.clone(),
        sender_did: daemon.controller_did.clone(),
        requester_did: daemon.controller_did.clone(),
        requester_user_id: Some(daemon.controller_user_id.clone()),
        requester_full_handle: Some(daemon.controller_full_handle.clone()),
        trigger_kind: RuntimeTaskTriggerKind::ControllerDirect,
        conversation_scope: RuntimeConversationScope::ControllerPrivate {
            controller_scope_key: daemon.controller_scope_key.clone(),
        },
        invocation_authority: RuntimeInvocationAuthority::Controller,
        reply_recipient_did: daemon.controller_did.clone(),
        conversation_id: Some(format!("direct:{}", daemon.controller_did)),
        text: "keep isolated".to_string(),
    };
    state.insert_runtime_task(&task).unwrap();
    state
        .connection()
        .unwrap()
        .execute(
            "UPDATE runtime_task SET status='running' WHERE task_id=?1",
            [&task.task_id],
        )
        .unwrap();
    let delegated = UserDelegatedIdentityRecord {
        user_did: daemon.controller_did.clone(),
        verification_method: format!("{}#daemon-key-1", daemon.controller_did),
        app_instance_id: "app-controller-change".to_string(),
        controller_did: daemon.controller_did.clone(),
        daemon_agent_did: daemon.agent_did.clone(),
        public_key_multibase: "z-public".to_string(),
        private_key_material: "z-private-secret".to_string(),
        private_key_ref_json: None,
        allowed_scopes_json: json!(["message.inbox.read.plain"]),
        status: "paired_key_received".to_string(),
        expires_at: Some("2026-09-09T00:00:00Z".to_string()),
        bootstrap_id: "boot-controller-change".to_string(),
        idempotency_key: "bootstrap-controller-change".to_string(),
        created_at_ms: 0,
        updated_at_ms: 0,
    };
    let replay = BootstrapReplayRecord {
        bootstrap_id: delegated.bootstrap_id.clone(),
        idempotency_key: delegated.idempotency_key.clone(),
        payload_hash: "payload-controller-change".to_string(),
        user_did: delegated.user_did.clone(),
        verification_method: delegated.verification_method.clone(),
        app_instance_id: delegated.app_instance_id.clone(),
        daemon_agent_did: delegated.daemon_agent_did.clone(),
        status: delegated.status.clone(),
        created_at_ms: 0,
        updated_at_ms: 0,
    };
    state.store_bootstrap_state(&delegated, &replay).unwrap();

    sync_controller_did_from_latest_response(
        &state,
        &daemon.agent_did,
        &json!({
            "updated": [{
                "agent_did": daemon.agent_did,
                "controller_user_id": daemon.controller_user_id,
                "controller_full_handle": daemon.controller_full_handle,
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
    let stored_task = state.load_runtime_task(&task.task_id).unwrap();
    let stored_task_status: String = state
        .connection()
        .unwrap()
        .query_row(
            "SELECT status FROM runtime_task WHERE task_id=?1",
            [&task.task_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(stored_task_status, "failed");
    assert_eq!(stored_task.controller_did, daemon.controller_did);
    assert_eq!(stored_task.reply_recipient_did, daemon.controller_did);
    assert_eq!(stored_task.conversation_id, task.conversation_id);
    assert_eq!(
        state
            .load_user_delegated_identity(&delegated.verification_method)
            .unwrap()
            .unwrap()
            .status,
        "recovery_fenced"
    );
    assert!(state
        .load_user_delegated_identity("did:human:alice-new#daemon-key-1")
        .unwrap()
        .is_none());
    assert!(state
        .audit_event_exists(
            "daemon.controller_rebound",
            Some(&daemon.agent_did),
            Some("controller_recovered"),
        )
        .unwrap());
    sync_controller_did_from_latest_response(
        &state,
        &daemon.agent_did,
        &json!({
            "updated": [{
                "agent_did": daemon.agent_did,
                "controller_user_id": daemon.controller_user_id,
                "controller_full_handle": daemon.controller_full_handle,
                "controller_did": "did:human:alice-new",
                "status": "ready",
            }]
        }),
    )
    .unwrap();
    let event_count: i64 = state
        .connection()
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM audit_log WHERE event_type=?1 AND agent_did=?2",
            ["daemon.controller_rebound", daemon.agent_did.as_str()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(event_count, 1);
    let detail_json: String = state
        .connection()
        .unwrap()
        .query_row(
            "SELECT detail_json FROM audit_log WHERE event_type=?1 AND agent_did=?2",
            ["daemon.controller_rebound", daemon.agent_did.as_str()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&detail_json).unwrap()["source"],
        "authoritative_status"
    );
    assert!(!detail_json.contains("did:human:alice"));
    assert!(!detail_json.contains("device-access"));
    assert!(!detail_json.contains("private"));
}

#[test]
fn controller_rebind_legacy_identity_change_audit_is_diagnostic_only() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    config.ensure_state_layout().unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();
    let daemon_agent_did = "did:agent:daemon-race";
    ensure_controller_identity_active(&state, daemon_agent_did).unwrap();

    record_controller_identity_changed(&state, daemon_agent_did, "legacy_authoritative_status")
        .unwrap_err();
    let event_count: i64 = state
        .connection()
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM audit_log WHERE event_type=?1 AND agent_did=?2",
            [CONTROLLER_IDENTITY_CHANGED_EVENT, daemon_agent_did],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(event_count, 1);

    let reopened = DaemonState::open(&config).unwrap();
    assert!(controller_identity_change_observed(&reopened, daemon_agent_did).unwrap());
    ensure_controller_identity_active(&reopened, daemon_agent_did).unwrap();
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
