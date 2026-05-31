use std::sync::{Arc, Mutex};

use awiki_deamon::agent::AgentKind;
use awiki_deamon::commands::{
    handle_agent_payload_message, setup_daemon_agent, AgentCommandOutcome,
    IncomingAgentPayloadMessage,
};
use awiki_deamon::outbox::MemoryRuntimeOutbox;
use awiki_deamon::plugins::hermes::{AWIKI_SKILLS_VERSION, HERMES_RUNTIME_PLUGIN_ID};
use awiki_deamon::registration::{
    AgentRegistrationClient, AgentRegistrationExchangeRequest, AgentRegistrationExchangeResult,
    RegistrationToken,
};
use awiki_deamon::{
    daemon_cli::{setup_daemon_agent_from_token, SetupDaemonAgentOptions},
    run_command_json, DaemonCommand, DaemonConfig, DaemonState,
};
use rusqlite::Connection;
use serde_json::json;

#[derive(Debug, Clone, Default)]
struct MockRegistrationClient {
    requests: Arc<Mutex<Vec<AgentRegistrationExchangeRequest>>>,
    fail_reason: Option<String>,
}

impl MockRegistrationClient {
    fn requests(&self) -> Vec<AgentRegistrationExchangeRequest> {
        self.requests.lock().unwrap().clone()
    }
}

impl AgentRegistrationClient for MockRegistrationClient {
    fn exchange_token(
        &self,
        request: AgentRegistrationExchangeRequest,
    ) -> anyhow::Result<AgentRegistrationExchangeResult> {
        if let Some(reason) = self.fail_reason.as_deref() {
            anyhow::bail!("agent registration token exchange failed: {reason}");
        }
        self.requests.lock().unwrap().push(request.clone());
        let did = request
            .did_document
            .get("id")
            .and_then(serde_json::Value::as_str)
            .unwrap()
            .to_string();
        Ok(AgentRegistrationExchangeResult {
            token_id: format!("agtok_{}_{}", request.agent_kind.as_str(), request.handle),
            did,
            user_id: Some(format!("user_{}", request.handle)),
            agent_kind: request.agent_kind,
            controller_did: request.controller_did,
            handle: request.handle,
            status: "registered".to_string(),
        })
    }
}

fn fixture() -> (tempfile::TempDir, DaemonConfig, DaemonState) {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    config.ensure_state_layout().unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();
    (root, config, state)
}

#[test]
fn daemon_setup_and_runtime_agent_create_command_persist_records_and_status_payload() {
    let (root, config, state) = fixture();
    let registration = MockRegistrationClient::default();
    let daemon = setup_daemon_agent(
        &config,
        &state,
        &registration,
        "@alice-mac-daemon",
        "did:human:alice",
        RegistrationToken::new("tok_daemon_secret_value").unwrap(),
    )
    .unwrap();

    assert_eq!(daemon.agent_kind, AgentKind::Daemon);
    assert_eq!(daemon.controller_did, "did:human:alice");
    assert_eq!(daemon.handle, "alice-mac-daemon");

    let outbox = MemoryRuntimeOutbox::default();
    let outcome = handle_agent_payload_message(
        &config,
        &state,
        &registration,
        &outbox,
        IncomingAgentPayloadMessage {
            message_id: "msg_002".to_string(),
            conversation_id: Some("conv_daemon_001".to_string()),
            sender_did: "did:human:alice".to_string(),
            target_agent_did: daemon.agent_did.clone(),
            content_type: "application/json".to_string(),
            payload: json!({
                "schema": "awiki.agent.command.v1",
                "command_id": "cmd_create_agent_001",
                "command": "runtime.agent.create",
                "target_agent_kind": "runtime",
                "args": {
                    "handle": "@alice-awiki-coder",
                    "runtime": "claude-code",
                    "workspace": root.path().join("workspace").display().to_string(),
                    "controller_did": "did:human:alice",
                    "registration_token": "tok_runtime_secret_value"
                },
                "reply_policy": {
                    "progress": true,
                    "final": true
                }
            }),
        },
    )
    .unwrap();

    let AgentCommandOutcome::RuntimeAgentCreated(created) = outcome;
    assert_eq!(created.handle, "alice-awiki-coder");
    assert_eq!(created.runtime_plugin_id, "runtime.cli.claude-code");
    assert_eq!(
        created.runtime_profile_id,
        "profile_claude_code_alice_awiki_coder"
    );
    assert_eq!(
        created.workspace_id.as_deref(),
        Some("workspace_alice_awiki_coder")
    );
    assert!(created
        .agent_did
        .starts_with("did:wba:awiki.local:agent:runtime:"));

    let runtime_agent = state.load_agent_definition(&created.agent_did).unwrap();
    assert_eq!(runtime_agent.agent_kind, AgentKind::Runtime);
    assert_eq!(runtime_agent.handle, "alice-awiki-coder");
    assert_eq!(runtime_agent.controller_did, "did:human:alice");
    assert_eq!(
        runtime_agent.runtime_profile_id.as_deref(),
        Some("profile_claude_code_alice_awiki_coder")
    );

    let statuses = outbox.agent_statuses();
    assert_eq!(statuses.len(), 1);
    assert_eq!(statuses[0].recipient_did, "did:human:alice");
    assert_eq!(statuses[0].payload["schema"], "awiki.agent.status.v1");
    assert_eq!(statuses[0].payload["state"], "ready");
    assert_eq!(
        statuses[0].payload["result"]["runtime_plugin_id"],
        "runtime.cli.claude-code"
    );

    let requests = registration.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].agent_kind, AgentKind::Daemon);
    assert_eq!(requests[1].agent_kind, AgentKind::Runtime);
    assert!(!format!("{:?}", requests[1]).contains("tok_runtime_secret_value"));
    assert!(format!("{:?}", requests[1]).contains("<redacted>"));

    let connection = Connection::open(root.path().join("daemon.db")).unwrap();
    let agent_count: i64 = connection
        .query_row("SELECT COUNT(*) FROM agent_definition", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(agent_count, 2);
    let workspace_count: i64 = connection
        .query_row("SELECT COUNT(*) FROM workspace_binding", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(workspace_count, 1);
    let audit_dump: String = connection
        .query_row(
            "SELECT COALESCE(token_id, '') || ' ' || COALESCE(detail_json, '') FROM audit_log WHERE event_type = 'agent.registration.exchange' LIMIT 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(audit_dump.contains(&created.registration_token_id));
    assert!(!audit_dump.contains("tok_runtime_secret_value"));
}

#[test]
fn non_controller_runtime_create_command_is_rejected_without_creating_agent() {
    let (_root, config, state) = fixture();
    let registration = MockRegistrationClient::default();
    let daemon = setup_daemon_agent(
        &config,
        &state,
        &registration,
        "alice-mac-daemon",
        "did:human:alice",
        RegistrationToken::new("tok_daemon_secret_value").unwrap(),
    )
    .unwrap();
    let outbox = MemoryRuntimeOutbox::default();

    let error = handle_agent_payload_message(
        &config,
        &state,
        &registration,
        &outbox,
        IncomingAgentPayloadMessage {
            message_id: "msg_unauthorized".to_string(),
            conversation_id: None,
            sender_did: "did:human:bob".to_string(),
            target_agent_did: daemon.agent_did,
            content_type: "application/json".to_string(),
            payload: json!({
                "schema": "awiki.agent.command.v1",
                "command_id": "cmd_unauthorized",
                "command": "runtime.agent.create",
                "args": {
                    "handle": "alice-awiki-coder",
                    "runtime": "claude-code",
                    "controller_did": "did:human:alice",
                    "registration_token": "tok_runtime_secret_value"
                }
            }),
        },
    )
    .unwrap_err();

    assert!(error.to_string().contains("controller_did"));
    assert_eq!(state.list_runtime_agent_definitions().unwrap().len(), 0);
    assert!(outbox.agent_statuses().is_empty());
}

#[test]
fn registration_token_failure_sends_failed_status_without_persisting_runtime_agent() {
    let (_root, config, state) = fixture();
    let setup_registration = MockRegistrationClient::default();
    let daemon = setup_daemon_agent(
        &config,
        &state,
        &setup_registration,
        "alice-mac-daemon",
        "did:human:alice",
        RegistrationToken::new("tok_daemon_secret_value").unwrap(),
    )
    .unwrap();
    let failing_registration = MockRegistrationClient {
        fail_reason: Some("scope_mismatch".to_string()),
        ..MockRegistrationClient::default()
    };
    let outbox = MemoryRuntimeOutbox::default();

    let error = handle_agent_payload_message(
        &config,
        &state,
        &failing_registration,
        &outbox,
        IncomingAgentPayloadMessage {
            message_id: "msg_failed_registration".to_string(),
            conversation_id: Some("conv_daemon_001".to_string()),
            sender_did: "did:human:alice".to_string(),
            target_agent_did: daemon.agent_did,
            content_type: "application/json".to_string(),
            payload: json!({
                "schema": "awiki.agent.command.v1",
                "command_id": "cmd_failed_registration",
                "command": "runtime.agent.create",
                "args": {
                    "handle": "alice-awiki-coder",
                    "runtime": "claude-code",
                    "controller_did": "did:human:alice",
                    "registration_token": "tok_runtime_secret_value"
                }
            }),
        },
    )
    .unwrap_err();

    assert!(error.to_string().contains("scope_mismatch"));
    assert_eq!(state.list_runtime_agent_definitions().unwrap().len(), 0);
    let statuses = outbox.agent_statuses();
    assert_eq!(statuses.len(), 1);
    assert_eq!(statuses[0].payload["state"], "failed");
    assert!(!statuses[0]
        .payload
        .to_string()
        .contains("tok_runtime_secret_value"));
}

#[test]
fn daemon_management_commands_list_agents_without_cli_crate_dependency() {
    let (root, config, state) = fixture();
    let registration = MockRegistrationClient::default();
    setup_daemon_agent(
        &config,
        &state,
        &registration,
        "alice-mac-daemon",
        "did:human:alice",
        RegistrationToken::new("tok_daemon_secret_value").unwrap(),
    )
    .unwrap();

    let output = run_command_json(DaemonCommand::AgentList {
        state_root: root.path().to_path_buf(),
    })
    .unwrap();

    assert_eq!(output["agents"][0]["handle"], "alice-mac-daemon");
}

#[test]
fn hermes_status_reports_profile_installation_and_sessions_without_secrets() {
    let (root, config, state) = fixture();
    let registration = MockRegistrationClient::default();
    let daemon = setup_daemon_agent(
        &config,
        &state,
        &registration,
        "alice-mac-daemon",
        "did:human:alice",
        RegistrationToken::new("tok_daemon_secret_value").unwrap(),
    )
    .unwrap();
    let outbox = MemoryRuntimeOutbox::default();

    let outcome = handle_agent_payload_message(
        &config,
        &state,
        &registration,
        &outbox,
        IncomingAgentPayloadMessage {
            message_id: "msg_create_hermes_status".to_string(),
            conversation_id: Some("conv_daemon_hermes_status".to_string()),
            sender_did: "did:human:alice".to_string(),
            target_agent_did: daemon.agent_did,
            content_type: "application/json".to_string(),
            payload: json!({
                "schema": "awiki.agent.command.v1",
                "command_id": "cmd_create_hermes_status",
                "command": "runtime.agent.create",
                "target_agent_kind": "runtime",
                "args": {
                    "handle": "@alice-hermes-status",
                    "runtime": "hermes",
                    "controller_did": "did:human:alice",
                    "registration_token": "tok_runtime_secret_value"
                }
            }),
        },
    )
    .unwrap();
    let AgentCommandOutcome::RuntimeAgentCreated(created) = outcome;
    assert_eq!(created.runtime_plugin_id, HERMES_RUNTIME_PLUGIN_ID);
    let route = awiki_deamon::state::HermesSessionRoute::new(
        created.agent_did.clone(),
        created.runtime_profile_id.clone(),
        "did:human:alice",
        Some("direct:did:human:alice".to_string()),
        "conversation",
    );
    let session = awiki_deamon::state::HermesNativeSessionRecord::active(
        &route,
        "awiki_alice_hermes_status",
        "hermes-session-status",
    )
    .unwrap();
    state.store_hermes_native_session(&session).unwrap();
    state
        .insert_audit_event_json(
            "hermes.error",
            Some(&created.agent_did),
            Some(&created.runtime_profile_id),
            Some("run_hermes_status"),
            None,
            json!({
                "error": "gateway failed with rtok_secret_value jwt_token auth_private_key",
            }),
        )
        .unwrap();

    let output = run_command_json(DaemonCommand::AgentStatus {
        state_root: root.path().to_path_buf(),
        agent_did: created.agent_did.clone(),
    })
    .unwrap();

    assert_eq!(
        output["agent"]["runtime_plugin_id"],
        HERMES_RUNTIME_PLUGIN_ID
    );
    assert_eq!(output["hermes"]["agent_did"], created.agent_did);
    assert_eq!(
        output["hermes"]["awiki_skills_version"],
        AWIKI_SKILLS_VERSION
    );
    assert_eq!(output["hermes"]["active_session_count"], 1);
    assert_eq!(output["hermes"]["runner_status"], "lazy");
    assert!(output["hermes"]["installation"]["detail"].is_string());
    assert_eq!(output["hermes"]["last_error"], "hermes.error");
    let dump = output.to_string();
    assert!(!dump.contains("tok_runtime_secret_value"));
    assert!(!dump.contains("tok_daemon_secret_value"));
    assert!(!dump.contains("rtok_"));
    assert!(!dump.contains("jwt_token"));
    assert!(!dump.contains("auth_private_key"));
}

#[test]
fn setup_daemon_agent_options_validate_required_fields() {
    let (_root, config, state) = fixture();
    let error = setup_daemon_agent_from_token(
        &config,
        &state,
        SetupDaemonAgentOptions {
            handle: "".to_string(),
            controller_did: "did:human:alice".to_string(),
            registration_token: "tok_daemon_secret_value".to_string(),
        },
    )
    .unwrap_err();

    assert!(error.to_string().contains("--handle"));
}
