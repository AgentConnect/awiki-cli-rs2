use std::sync::{Arc, Mutex};

use awiki_deamon::agent::AgentKind;
use awiki_deamon::commands::{
    handle_agent_payload_message, setup_daemon_agent, AgentCommandOutcome,
    IncomingAgentPayloadMessage, RuntimeAgentCreateOutcome,
};
use awiki_deamon::outbox::MemoryRuntimeOutbox;
use awiki_deamon::plugins::hermes::{AWIKI_SKILLS_VERSION, HERMES_RUNTIME_PLUGIN_ID};
use awiki_deamon::registration::{
    AgentRegistrationClient, AgentRegistrationExchangeRequest, AgentRegistrationExchangeResult,
    RegistrationToken,
};
use awiki_deamon::runtime::{RuntimeRun, RuntimeRunStatus};
use awiki_deamon::state::{HermesNativeSessionRecord, HermesSessionRoute};
use awiki_deamon::{
    daemon_cli::{setup_daemon_agent_from_token, SetupDaemonAgentOptions},
    im_core_adapter::sync_agent_identity_to_im_core,
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

fn expect_created(outcome: AgentCommandOutcome) -> RuntimeAgentCreateOutcome {
    match outcome {
        AgentCommandOutcome::RuntimeAgentCreated(created) => created,
        other => panic!("expected runtime agent create outcome, got {other:?}"),
    }
}

fn seed_runtime_inbox_projection(config: &DaemonConfig, runtime_agent_did: &str) {
    if let Some(parent) = config.im_core_sqlite_path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    let connection = Connection::open(&config.im_core_sqlite_path).unwrap();
    im_core::compat::local_state::ensure_schema(&connection).unwrap();
    let owner_identity_id = test_identity_alias(runtime_agent_did);
    insert_projected_message(
        &connection,
        &owner_identity_id,
        runtime_agent_did,
        "msg-direct-bob",
        "dm:did:human:bob",
        0,
        "did:human:bob",
        runtime_agent_did,
        "",
        "",
        "text/plain",
        "hello runtime",
        "2026-06-04T10:00:00Z",
        0,
    );
    insert_projected_message(
        &connection,
        &owner_identity_id,
        runtime_agent_did,
        "msg-group-attachment",
        "group:did:group:team",
        0,
        "did:human:carol",
        runtime_agent_did,
        "did:group:team",
        "did:group:team",
        "application/anp-attachment-manifest+json",
        r#"{
          "attachments": [{
            "attachment_id": "att-report",
            "filename": "report.pdf",
            "mime_type": "application/pdf",
            "size_bytes": 102400,
            "object_uri": "https://object.example/report"
          }],
          "caption": "group report"
        }"#,
        "2026-06-04T10:05:00Z",
        1,
    );
}

#[allow(clippy::too_many_arguments)]
fn insert_projected_message(
    connection: &Connection,
    owner_identity_id: &str,
    owner_did: &str,
    msg_id: &str,
    conversation_id: &str,
    direction: i64,
    sender_did: &str,
    receiver_did: &str,
    group_id: &str,
    group_did: &str,
    content_type: &str,
    content: &str,
    sent_at: &str,
    is_read: i64,
) {
    connection
        .execute(
            r#"
INSERT INTO messages
    (msg_id, owner_identity_id, owner_did, conversation_id, thread_id, direction,
     sender_did, receiver_did, group_id, group_did, content_type, content,
     sent_at, stored_at, is_read)
VALUES (?1, ?2, ?3, ?4, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?12, ?13)"#,
            rusqlite::params![
                msg_id,
                owner_identity_id,
                owner_did,
                conversation_id,
                direction,
                sender_did,
                receiver_did,
                group_id,
                group_did,
                content_type,
                content,
                sent_at,
                is_read,
            ],
        )
        .unwrap();
}

fn test_identity_alias(did: &str) -> String {
    did.chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
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

    let created = expect_created(outcome);
    assert_eq!(created.handle, "alice-awiki-coder");
    assert_eq!(created.runtime_plugin_id, "generic-cli");
    assert_eq!(created.driver_id.as_deref(), Some("claude-code"));
    assert!(!created.defaulted_driver_id);
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
        .starts_with(&format!("did:wba:{}:agent:runtime:", config.did_domain)));

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
        "generic-cli"
    );
    assert_eq!(statuses[0].payload["result"]["driver_id"], "claude-code");

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
    let cli_driver_id: String = connection
        .query_row(
            "SELECT driver_id FROM cli_runtime_profile WHERE runtime_profile_id = ?1",
            [&created.runtime_profile_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(cli_driver_id, "claude-code");
    let audit_dump: String = connection
        .query_row(
            "SELECT COALESCE(token_id, '') || ' ' || COALESCE(detail_json, '') FROM audit_log WHERE event_type = 'agent.registration.exchange' LIMIT 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(audit_dump.contains(&created.registration_token_id));
    assert!(audit_dump.contains("\"runtime_plugin_id\":\"generic-cli\""));
    assert!(audit_dump.contains("\"driver_id\":\"claude-code\""));
    assert!(audit_dump.contains("\"legacy_runtime_plugin_id\":\"runtime.cli.claude-code\""));
    assert!(!audit_dump.contains("tok_runtime_secret_value"));
}

#[test]
fn runtime_agent_create_generates_product_handle_when_app_omits_handle() {
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

    let outcome = handle_agent_payload_message(
        &config,
        &state,
        &registration,
        &outbox,
        IncomingAgentPayloadMessage {
            message_id: "msg_product_create_hermes".to_string(),
            conversation_id: Some("conv_daemon_product".to_string()),
            sender_did: "did:human:alice".to_string(),
            target_agent_did: daemon.agent_did.clone(),
            content_type: "application/json".to_string(),
            payload: json!({
                "schema": "awiki.agent.command.v1",
                "command_id": "cmd_product_create_hermes",
                "command": "runtime.agent.create",
                "target_agent_kind": "runtime",
                "args": {
                    "runtime": "hermes",
                    "controller_did": "did:human:alice",
                    "registration_token": "tok_runtime_secret_value",
                    "display_name": "Hermes"
                },
                "reply_policy": {
                    "progress": true,
                    "final": true
                }
            }),
        },
    )
    .unwrap();

    let created = expect_created(outcome);
    assert!(created.handle.starts_with("awiki-agent-"));
    assert_eq!(created.handle.len(), "awiki-agent-".len() + 16);
    assert_eq!(created.runtime_plugin_id, HERMES_RUNTIME_PLUGIN_ID);

    let requests = registration.requests();
    assert_eq!(requests[1].handle, created.handle);
    let status = outbox.agent_statuses().pop().unwrap();
    assert_eq!(
        status.payload["result"]["runtime_agent_did"],
        created.agent_did
    );
    assert_eq!(
        status.payload["result"]["daemon_agent_did"],
        daemon.agent_did
    );
    assert_eq!(status.payload["result"]["runtime"], "hermes");
    assert_eq!(status.payload["result"]["display_name"], "Hermes");
}

#[test]
fn runtime_agent_create_reuses_client_request_id_without_second_exchange() {
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

    let create_message = |command_id: &str| IncomingAgentPayloadMessage {
        message_id: format!("msg_{command_id}"),
        conversation_id: Some("conv_create_idempotent".to_string()),
        sender_did: "did:human:alice".to_string(),
        target_agent_did: daemon.agent_did.clone(),
        content_type: "application/json".to_string(),
        payload: json!({
            "schema": "awiki.agent.command.v1",
            "command_id": command_id,
            "command": "runtime.agent.create",
            "target_agent_kind": "runtime",
            "args": {
                "runtime": "hermes",
                "controller_did": "did:human:alice",
                "registration_token": "tok_runtime_secret_value",
                "display_name": "Hermes",
                "client_request_id": "app_req_create_hermes_once"
            }
        }),
    };

    let first = expect_created(
        handle_agent_payload_message(
            &config,
            &state,
            &registration,
            &outbox,
            create_message("cmd_create_once"),
        )
        .unwrap(),
    );
    let second = expect_created(
        handle_agent_payload_message(
            &config,
            &state,
            &registration,
            &outbox,
            create_message("cmd_create_once_retry"),
        )
        .unwrap(),
    );

    assert_eq!(second.command_id, "cmd_create_once_retry");
    assert_eq!(second.agent_did, first.agent_did);
    assert_eq!(second.handle, first.handle);
    assert_eq!(second.runtime_profile_id, first.runtime_profile_id);
    assert_eq!(registration.requests().len(), 2);
    let runtimes = state
        .list_runtime_agent_definitions_for_daemon(&daemon.agent_did)
        .unwrap();
    assert_eq!(runtimes.len(), 1);
    assert_eq!(runtimes[0].agent_did, first.agent_did);

    let statuses = outbox.agent_statuses();
    assert_eq!(statuses.len(), 2);
    assert_eq!(
        statuses[0].payload["result"]["runtime_agent_did"],
        first.agent_did
    );
    assert_eq!(
        statuses[1].payload["result"]["runtime_agent_did"],
        first.agent_did
    );
    assert_eq!(statuses[1].payload["command_id"], "cmd_create_once_retry");
}

#[test]
fn agent_status_query_returns_snapshot_payload_without_chat_content() {
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

    let outcome = handle_agent_payload_message(
        &config,
        &state,
        &registration,
        &outbox,
        IncomingAgentPayloadMessage {
            message_id: "msg_status_query".to_string(),
            conversation_id: Some("conv_daemon_status".to_string()),
            sender_did: "did:human:alice".to_string(),
            target_agent_did: daemon.agent_did.clone(),
            content_type: "application/json".to_string(),
            payload: json!({
                "schema": "awiki.agent.command.v1",
                "command_id": "cmd_status_query",
                "command": "agent.status.query",
                "target_agent_kind": "daemon",
                "args": {
                    "include_runtimes": true,
                    "include_diagnostics": true
                }
            }),
        },
    )
    .unwrap();

    assert_eq!(
        outcome,
        AgentCommandOutcome::StatusReported {
            command_id: "cmd_status_query".to_string()
        }
    );
    let status = outbox.agent_statuses().pop().unwrap();
    assert_eq!(status.payload["schema"], "awiki.agent.status.v1");
    assert_eq!(status.payload["status_scope"], "snapshot");
    assert_eq!(status.payload["daemon_agent_did"], daemon.agent_did);
    assert_eq!(status.payload["daemon"]["status"], "ready");
    assert_eq!(status.payload["runs"], json!([]));
    let dump = status.payload.to_string();
    assert!(!dump.contains("tok_daemon_secret_value"));
    assert!(!dump.contains("prompt"));
}

#[test]
fn repeated_agent_status_query_is_throttled_by_daemon_and_controller() {
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

    for command_id in ["cmd_status_query_1", "cmd_status_query_2"] {
        handle_agent_payload_message(
            &config,
            &state,
            &registration,
            &outbox,
            IncomingAgentPayloadMessage {
                message_id: format!("msg_{command_id}"),
                conversation_id: Some("conv_daemon_status".to_string()),
                sender_did: "did:human:alice".to_string(),
                target_agent_did: daemon.agent_did.clone(),
                content_type: "application/json".to_string(),
                payload: json!({
                    "schema": "awiki.agent.command.v1",
                    "command_id": command_id,
                    "command": "agent.status.query",
                    "target_agent_kind": "daemon",
                    "args": {
                        "include_runtimes": true,
                        "include_diagnostics": true
                    }
                }),
            },
        )
        .unwrap();
    }

    let statuses = outbox.agent_statuses();
    assert_eq!(statuses.len(), 2);
    assert_eq!(statuses[0].payload["status_scope"], "snapshot");
    assert_eq!(statuses[0].payload["result"]["throttled"], json!(null));
    assert_eq!(statuses[1].payload["status_scope"], "daemon");
    assert_eq!(statuses[1].payload["result"]["throttled"], true);
    assert_eq!(statuses[1].payload["result"]["retry_after_seconds"], 10);
}

#[test]
fn runtime_session_reset_archives_active_hermes_route() {
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
    let outcome = handle_agent_payload_message(
        &config,
        &state,
        &registration,
        &outbox,
        IncomingAgentPayloadMessage {
            message_id: "msg_product_create_hermes_reset".to_string(),
            conversation_id: Some("conv_daemon_product_reset".to_string()),
            sender_did: "did:human:alice".to_string(),
            target_agent_did: daemon.agent_did.clone(),
            content_type: "application/json".to_string(),
            payload: json!({
                "schema": "awiki.agent.command.v1",
                "command_id": "cmd_product_create_hermes_reset",
                "command": "runtime.agent.create",
                "target_agent_kind": "runtime",
                "args": {
                    "handle": "@alice-hermes-reset",
                    "runtime": "hermes",
                    "controller_did": "did:human:alice",
                    "registration_token": "tok_runtime_secret_value",
                    "display_name": "Hermes"
                }
            }),
        },
    )
    .unwrap();
    let created = expect_created(outcome);
    let route = HermesSessionRoute::new(
        created.agent_did.clone(),
        created.runtime_profile_id.clone(),
        "did:human:alice",
        Some("dm:alice:hermes".to_string()),
        "conversation",
    );
    let record =
        HermesNativeSessionRecord::active(&route, "awiki_alice_hermes", "hsession-1").unwrap();
    state.store_hermes_native_session(&record).unwrap();
    assert!(state
        .load_active_hermes_session_by_route(&route)
        .unwrap()
        .is_some());

    handle_agent_payload_message(
        &config,
        &state,
        &registration,
        &outbox,
        IncomingAgentPayloadMessage {
            message_id: "msg_session_reset".to_string(),
            conversation_id: Some("conv_daemon_reset".to_string()),
            sender_did: "did:human:alice".to_string(),
            target_agent_did: daemon.agent_did,
            content_type: "application/json".to_string(),
            payload: json!({
                "schema": "awiki.agent.command.v1",
                "command_id": "cmd_session_reset",
                "command": "runtime.session.reset",
                "target_agent_kind": "runtime",
                "args": {
                    "runtime_agent_did": created.agent_did,
                    "conversation_id": "dm:alice:hermes"
                }
            }),
        },
    )
    .unwrap();

    assert!(state
        .load_active_hermes_session_by_route(&route)
        .unwrap()
        .is_none());
    let status = outbox.agent_statuses().pop().unwrap();
    assert_eq!(status.payload["result"]["command"], "runtime.session.reset");
    assert_eq!(status.payload["result"]["reset_count"], 1);
}

#[test]
fn runtime_session_reset_rejects_runtime_owned_by_another_daemon() {
    let (_root, config, state) = fixture();
    let registration = MockRegistrationClient::default();
    let daemon_one = setup_daemon_agent(
        &config,
        &state,
        &registration,
        "alice-mac-daemon-one",
        "did:human:alice",
        RegistrationToken::new("tok_daemon_secret_value_one").unwrap(),
    )
    .unwrap();
    let daemon_two = setup_daemon_agent(
        &config,
        &state,
        &registration,
        "alice-mac-daemon-two",
        "did:human:alice",
        RegistrationToken::new("tok_daemon_secret_value_two").unwrap(),
    )
    .unwrap();
    let outbox = MemoryRuntimeOutbox::default();
    let outcome = handle_agent_payload_message(
        &config,
        &state,
        &registration,
        &outbox,
        IncomingAgentPayloadMessage {
            message_id: "msg_create_daemon_one_runtime".to_string(),
            conversation_id: Some("conv_daemon_one".to_string()),
            sender_did: "did:human:alice".to_string(),
            target_agent_did: daemon_one.agent_did,
            content_type: "application/json".to_string(),
            payload: json!({
                "schema": "awiki.agent.command.v1",
                "command_id": "cmd_create_daemon_one_runtime",
                "command": "runtime.agent.create",
                "target_agent_kind": "runtime",
                "args": {
                    "handle": "@alice-hermes-daemon-one",
                    "runtime": "generic-cli",
                    "controller_did": "did:human:alice",
                    "registration_token": "tok_runtime_secret_value"
                }
            }),
        },
    )
    .unwrap();
    let created = expect_created(outcome);

    handle_agent_payload_message(
        &config,
        &state,
        &registration,
        &outbox,
        IncomingAgentPayloadMessage {
            message_id: "msg_wrong_daemon_session_reset".to_string(),
            conversation_id: Some("conv_daemon_two".to_string()),
            sender_did: "did:human:alice".to_string(),
            target_agent_did: daemon_two.agent_did,
            content_type: "application/json".to_string(),
            payload: json!({
                "schema": "awiki.agent.command.v1",
                "command_id": "cmd_wrong_daemon_session_reset",
                "command": "runtime.session.reset",
                "target_agent_kind": "runtime",
                "args": {
                    "runtime_agent_did": created.agent_did
                }
            }),
        },
    )
    .unwrap();

    let status = outbox.agent_statuses().pop().unwrap();
    assert_eq!(status.payload["result"]["command"], "runtime.session.reset");
    assert_eq!(status.payload["result"]["error_code"], "runtime_not_owned");
}

#[test]
fn runtime_run_retry_validates_failed_run_state_without_prompt_leakage() {
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
    let created = match handle_agent_payload_message(
        &config,
        &state,
        &registration,
        &outbox,
        IncomingAgentPayloadMessage {
            message_id: "msg_create_retry_runtime".to_string(),
            conversation_id: Some("conv_create_retry_runtime".to_string()),
            sender_did: "did:human:alice".to_string(),
            target_agent_did: daemon.agent_did.clone(),
            content_type: "application/json".to_string(),
            payload: json!({
                "schema": "awiki.agent.command.v1",
                "command_id": "cmd_create_retry_runtime",
                "command": "runtime.agent.create",
                "target_agent_kind": "runtime",
                "args": {
                    "handle": "@alice-retry-runtime",
                    "runtime": "generic-cli",
                    "controller_did": "did:human:alice",
                    "registration_token": "tok_runtime_secret_value"
                }
            }),
        },
    )
    .unwrap()
    {
        AgentCommandOutcome::RuntimeAgentCreated(created) => created,
        other => panic!("unexpected outcome: {other:?}"),
    };
    state
        .insert_runtime_task(&awiki_deamon::runtime::RuntimeTask {
            task_id: "task_failed_retry".to_string(),
            agent_did: created.agent_did.clone(),
            controller_did: "did:human:alice".to_string(),
            sender_did: "did:human:alice".to_string(),
            conversation_id: Some("dm:alice:retry".to_string()),
            text: "super secret prompt".to_string(),
        })
        .unwrap();
    state
        .insert_runtime_run(&RuntimeRun {
            run_id: "run_failed_retry".to_string(),
            task_id: "task_failed_retry".to_string(),
            agent_did: created.agent_did.clone(),
            runtime_profile_id: created.runtime_profile_id.clone(),
            runtime_plugin_id: created.runtime_plugin_id.clone(),
            workspace_id: None,
            status: RuntimeRunStatus::Failed,
        })
        .unwrap();

    handle_agent_payload_message(
        &config,
        &state,
        &registration,
        &outbox,
        IncomingAgentPayloadMessage {
            message_id: "msg_retry_run".to_string(),
            conversation_id: Some("conv_retry_run".to_string()),
            sender_did: "did:human:alice".to_string(),
            target_agent_did: daemon.agent_did,
            content_type: "application/json".to_string(),
            payload: json!({
                "schema": "awiki.agent.command.v1",
                "command_id": "cmd_retry_run",
                "command": "runtime.run.retry",
                "target_agent_kind": "runtime",
                "args": {
                    "runtime_agent_did": created.agent_did,
                    "run_id": "run_failed_retry"
                }
            }),
        },
    )
    .unwrap();

    let status = outbox.agent_statuses().pop().unwrap();
    assert_eq!(status.payload["state"], "queued");
    assert_eq!(status.payload["status_scope"], "run");
    assert_eq!(status.payload["result"]["retry_status"], "queued");
    assert_eq!(status.payload["result"]["run_id"], "run_failed_retry");
    let retry_id = status.payload["result"]["retry_id"].as_str().unwrap();
    assert!(retry_id.starts_with("retry_"));
    assert_eq!(
        status.payload["result"]["retry_run_id"],
        format!("run_{retry_id}")
    );
    assert_eq!(
        status.payload["runs"][0]["run_id"],
        format!("run_{retry_id}")
    );
    assert_eq!(status.payload["runs"][0]["message_id"], "task_failed_retry");
    assert_eq!(
        status.payload["runs"][0]["runtime_agent_did"],
        created.agent_did
    );
    assert_eq!(
        status.payload["runs"][0]["conversation_id"],
        "dm:alice:retry"
    );
    assert_eq!(status.payload["runs"][0]["status"], "queued");
    assert!(status.payload["runs"][0]["updated_at"].is_string());
    let retry = state.load_runtime_retry_request(retry_id).unwrap();
    assert_eq!(retry.status, "queued");
    assert_eq!(retry.original_run_id, "run_failed_retry");
    assert_eq!(retry.task_id, "task_failed_retry");
    assert_eq!(retry.requested_by_command_id, "cmd_retry_run");
    let dump = status.payload.to_string();
    assert!(!dump.contains("super secret prompt"));
    assert!(!format!("{retry:?}").contains("super secret prompt"));
}

#[test]
fn runtime_rebuild_returns_unsupported_command_status() {
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

    handle_agent_payload_message(
        &config,
        &state,
        &registration,
        &outbox,
        IncomingAgentPayloadMessage {
            message_id: "msg_rebuild".to_string(),
            conversation_id: Some("conv_rebuild".to_string()),
            sender_did: "did:human:alice".to_string(),
            target_agent_did: daemon.agent_did,
            content_type: "application/json".to_string(),
            payload: json!({
                "schema": "awiki.agent.command.v1",
                "command_id": "cmd_rebuild",
                "command": "runtime.agent.rebuild",
                "target_agent_kind": "runtime",
                "args": {}
            }),
        },
    )
    .unwrap();

    let status = outbox.agent_statuses().pop().unwrap();
    assert_eq!(
        status.payload["result"]["error_code"],
        "unsupported_command"
    );
}

#[test]
fn daemon_upgrade_rejects_other_daemon_target_without_running_download() {
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

    handle_agent_payload_message(
        &config,
        &state,
        &registration,
        &outbox,
        IncomingAgentPayloadMessage {
            message_id: "msg_upgrade_other_daemon".to_string(),
            conversation_id: Some("conv_upgrade_other_daemon".to_string()),
            sender_did: "did:human:alice".to_string(),
            target_agent_did: daemon.agent_did,
            content_type: "application/json".to_string(),
            payload: json!({
                "schema": "awiki.agent.command.v1",
                "command_id": "cmd_upgrade_other_daemon",
                "command": "daemon.upgrade",
                "target_agent_kind": "daemon",
                "args": {
                    "daemon_agent_did": "did:agent:other-daemon",
                    "target_version": "latest"
                }
            }),
        },
    )
    .unwrap();

    let status = outbox.agent_statuses().pop().unwrap();
    assert_eq!(status.payload["state"], "failed");
    assert_eq!(
        status.payload["result"]["error_code"],
        "daemon_target_mismatch"
    );
}

#[test]
fn runtime_agent_create_accepts_generic_cli_driver_contract_fields() {
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
    let outcome = handle_agent_payload_message(
        &config,
        &state,
        &registration,
        &outbox,
        IncomingAgentPayloadMessage {
            message_id: "msg_create_generic_cli".to_string(),
            conversation_id: Some("conv_daemon_generic_cli".to_string()),
            sender_did: "did:human:alice".to_string(),
            target_agent_did: daemon.agent_did,
            content_type: "application/json".to_string(),
            payload: json!({
                "schema": "awiki.agent.command.v1",
                "command_id": "cmd_create_generic_cli",
                "command": "runtime.agent.create",
                "target_agent_kind": "runtime",
                "args": {
                    "handle": "@alice-generic-cli",
                    "runtime": "generic-cli",
                    "driver_id": "codex",
                    "driver_config": {
                        "profile": "awiki"
                    },
                    "recipient_policy": {
                        "allow": [
                            "did:human:alice",
                            "@bob"
                        ]
                    },
                    "controller_did": "did:human:alice",
                    "registration_token": "tok_runtime_secret_value"
                }
            }),
        },
    )
    .unwrap();

    let created = expect_created(outcome);
    assert_eq!(created.runtime_plugin_id, "generic-cli");
    assert_eq!(created.driver_id.as_deref(), Some("codex"));
    assert!(!created.defaulted_driver_id);
    assert_eq!(
        created.runtime_profile_id,
        "profile_generic_cli_alice_generic_cli"
    );
    assert_eq!(
        outbox.agent_statuses()[0].payload["result"]["runtime_plugin_id"],
        "generic-cli"
    );
    assert_eq!(
        outbox.agent_statuses()[0].payload["result"]["driver_id"],
        "codex"
    );
    let cli_profile = state
        .load_cli_runtime_profile(&created.runtime_profile_id)
        .unwrap();
    assert_eq!(cli_profile.driver_id, "codex");
    assert_eq!(
        cli_profile.driver_config_json,
        json!({ "profile": "awiki" })
    );
    assert_eq!(
        cli_profile.recipient_policy_json,
        json!({ "allow": ["did:human:alice", "@bob"] })
    );
}

#[test]
fn runtime_agent_create_maps_codex_and_gemini_aliases_to_generic_cli_profiles() {
    for (runtime, expected_driver_id, expected_profile_id) in [
        ("codex", "codex", "profile_codex_alice_codex"),
        ("codex-cli", "codex", "profile_codex_cli_alice_codex_cli"),
        ("gemini", "gemini", "profile_gemini_alice_gemini"),
        (
            "gemini-cli",
            "gemini",
            "profile_gemini_cli_alice_gemini_cli",
        ),
    ] {
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
        let outcome = handle_agent_payload_message(
            &config,
            &state,
            &registration,
            &outbox,
            IncomingAgentPayloadMessage {
                message_id: format!("msg_create_{runtime}"),
                conversation_id: Some(format!("conv_create_{runtime}")),
                sender_did: "did:human:alice".to_string(),
                target_agent_did: daemon.agent_did,
                content_type: "application/json".to_string(),
                payload: json!({
                    "schema": "awiki.agent.command.v1",
                    "command_id": format!("cmd_create_{runtime}"),
                    "command": "runtime.agent.create",
                    "target_agent_kind": "runtime",
                    "args": {
                        "handle": format!("@alice-{runtime}"),
                        "runtime": runtime,
                        "controller_did": "did:human:alice",
                        "registration_token": "tok_runtime_secret_value"
                    }
                }),
            },
        )
        .unwrap();

        let created = expect_created(outcome);
        assert_eq!(created.runtime_plugin_id, "generic-cli");
        assert_eq!(created.driver_id.as_deref(), Some(expected_driver_id));
        assert_eq!(created.runtime_profile_id, expected_profile_id);
        assert!(!created.defaulted_driver_id);
        let runtime_agent = state.load_agent_definition(&created.agent_did).unwrap();
        assert_eq!(
            runtime_agent.runtime_plugin_id.as_deref(),
            Some("generic-cli")
        );
        let cli_profile = state
            .load_cli_runtime_profile(&created.runtime_profile_id)
            .unwrap();
        assert_eq!(cli_profile.driver_id, expected_driver_id);
        assert_eq!(
            cli_profile.recipient_policy_json,
            json!({ "mode": "controller-only" })
        );
        assert_eq!(
            outbox.agent_statuses()[0].payload["result"]["driver_id"],
            expected_driver_id
        );
    }
}

#[test]
fn runtime_agent_create_defaults_generic_cli_driver_to_codex() {
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
    let outcome = handle_agent_payload_message(
        &config,
        &state,
        &registration,
        &outbox,
        IncomingAgentPayloadMessage {
            message_id: "msg_create_generic_cli_default".to_string(),
            conversation_id: Some("conv_create_generic_cli_default".to_string()),
            sender_did: "did:human:alice".to_string(),
            target_agent_did: daemon.agent_did,
            content_type: "application/json".to_string(),
            payload: json!({
                "schema": "awiki.agent.command.v1",
                "command_id": "cmd_create_generic_cli_default",
                "command": "runtime.agent.create",
                "target_agent_kind": "runtime",
                "args": {
                    "handle": "@alice-generic-default",
                    "runtime": "generic-cli",
                    "controller_did": "did:human:alice",
                    "registration_token": "tok_runtime_secret_value"
                }
            }),
        },
    )
    .unwrap();

    let created = expect_created(outcome);
    assert_eq!(created.runtime_plugin_id, "generic-cli");
    assert_eq!(created.driver_id.as_deref(), Some("codex"));
    assert!(created.defaulted_driver_id);
    assert_eq!(
        outbox.agent_statuses()[0].payload["result"]["defaulted_driver_id"],
        true
    );
    let cli_profile = state
        .load_cli_runtime_profile(&created.runtime_profile_id)
        .unwrap();
    assert_eq!(cli_profile.driver_id, "codex");
}

#[test]
fn runtime_agent_create_rejects_invalid_generic_cli_contract_fields() {
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
            message_id: "msg_create_generic_cli_invalid".to_string(),
            conversation_id: Some("conv_daemon_generic_cli_invalid".to_string()),
            sender_did: "did:human:alice".to_string(),
            target_agent_did: daemon.agent_did,
            content_type: "application/json".to_string(),
            payload: json!({
                "schema": "awiki.agent.command.v1",
                "command_id": "cmd_create_generic_cli_invalid",
                "command": "runtime.agent.create",
                "target_agent_kind": "runtime",
                "args": {
                    "handle": "@alice-generic-cli-invalid",
                    "runtime": "generic-cli",
                    "driver_id": "codex",
                    "driver_config": ["not", "an", "object"],
                    "controller_did": "did:human:alice",
                    "registration_token": "tok_runtime_secret_value"
                }
            }),
        },
    )
    .unwrap_err();

    assert!(error.to_string().contains("driver_config"));
    assert_eq!(state.list_runtime_agent_definitions().unwrap().len(), 0);
    assert_eq!(outbox.agent_statuses().len(), 1);
    assert_eq!(outbox.agent_statuses()[0].payload["state"], "failed");
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
fn runtime_inbox_commands_read_owned_runtime_local_projection() {
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
    let outcome = handle_agent_payload_message(
        &config,
        &state,
        &registration,
        &outbox,
        IncomingAgentPayloadMessage {
            message_id: "msg_create_runtime_inbox".to_string(),
            conversation_id: Some("conv_daemon_inbox".to_string()),
            sender_did: "did:human:alice".to_string(),
            target_agent_did: daemon.agent_did.clone(),
            content_type: "application/json".to_string(),
            payload: json!({
                "schema": "awiki.agent.command.v1",
                "command_id": "cmd_create_runtime_inbox",
                "command": "runtime.agent.create",
                "target_agent_kind": "runtime",
                "args": {
                    "handle": "@alice-inbox-runtime",
                    "runtime": "claude-code",
                    "controller_did": "did:human:alice",
                    "registration_token": "tok_runtime_secret_value"
                }
            }),
        },
    )
    .unwrap();
    let created = expect_created(outcome);
    let runtime_identity = state.load_agent_identity(&created.agent_did).unwrap();
    sync_agent_identity_to_im_core(&config, &runtime_identity, None).unwrap();
    seed_runtime_inbox_projection(&config, &created.agent_did);

    let inbox_outbox = MemoryRuntimeOutbox::default();
    handle_agent_payload_message(
        &config,
        &state,
        &registration,
        &inbox_outbox,
        IncomingAgentPayloadMessage {
            message_id: "msg_query_runtime_inbox".to_string(),
            conversation_id: Some("conv_daemon_inbox".to_string()),
            sender_did: "did:human:alice".to_string(),
            target_agent_did: daemon.agent_did.clone(),
            content_type: "application/json".to_string(),
            payload: json!({
                "schema": "awiki.agent.command.v1",
                "command_id": "cmd_runtime_inbox_query",
                "command": "runtime.inbox.query",
                "target_agent_kind": "daemon",
                "args": {
                    "runtime_agent_did": created.agent_did,
                    "scope": "all",
                    "limit": 10
                }
            }),
        },
    )
    .unwrap();
    let statuses = inbox_outbox.agent_statuses();
    assert_eq!(statuses.len(), 1);
    let payload = &statuses[0].payload;
    assert_eq!(payload["schema"], "awiki.agent.status.v1");
    assert_eq!(payload["status_scope"], "runtime_inbox");
    assert_eq!(payload["command"], "runtime.inbox.query");
    assert_eq!(payload["command_id"], "cmd_runtime_inbox_query");
    assert_eq!(payload["request_id"], "cmd_runtime_inbox_query");
    assert_eq!(payload["state"], "succeeded");
    let items = payload["result"]["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0]["kind"], "group");
    assert_eq!(items[0]["group_did"], "did:group:team");
    assert_eq!(items[0]["has_attachments"], true);
    assert_eq!(items[0]["last_content_type"], "attachment");
    assert_eq!(items[1]["kind"], "direct");
    assert_eq!(items[1]["peer_did"], "did:human:bob");
    assert_eq!(items[1]["last_message_preview"], "hello runtime");

    let thread_outbox = MemoryRuntimeOutbox::default();
    handle_agent_payload_message(
        &config,
        &state,
        &registration,
        &thread_outbox,
        IncomingAgentPayloadMessage {
            message_id: "msg_query_runtime_inbox_thread".to_string(),
            conversation_id: Some("conv_daemon_inbox".to_string()),
            sender_did: "did:human:alice".to_string(),
            target_agent_did: daemon.agent_did.clone(),
            content_type: "application/json".to_string(),
            payload: json!({
                "schema": "awiki.agent.command.v1",
                "command_id": "cmd_runtime_inbox_thread_query",
                "command": "runtime.inbox.thread.query",
                "target_agent_kind": "daemon",
                "args": {
                    "runtime_agent_did": created.agent_did,
                    "thread_id": "group:did:group:team",
                    "kind": "group",
                    "limit": 20
                }
            }),
        },
    )
    .unwrap();
    let statuses = thread_outbox.agent_statuses();
    assert_eq!(statuses.len(), 1);
    let payload = &statuses[0].payload;
    assert_eq!(payload["status_scope"], "runtime_inbox_thread");
    assert_eq!(payload["state"], "succeeded");
    let messages = payload["result"]["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0]["content_type"], "attachment");
    assert_eq!(messages[0]["attachments"][0]["attachment_id"], "att-report");
    assert_eq!(messages[0]["attachments"][0]["filename"], "report.pdf");
    assert_eq!(
        messages[0]["attachments"][0]["mime_type"],
        "application/pdf"
    );
    assert_eq!(messages[0]["attachments"][0]["size_bytes"], 102400);
    assert!(!payload
        .to_string()
        .contains(&config.state_root.display().to_string()));
}

#[test]
fn runtime_inbox_query_rejects_unowned_runtime_without_reading_messages() {
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
    handle_agent_payload_message(
        &config,
        &state,
        &registration,
        &outbox,
        IncomingAgentPayloadMessage {
            message_id: "msg_query_runtime_inbox_unowned".to_string(),
            conversation_id: Some("conv_daemon_inbox".to_string()),
            sender_did: "did:human:alice".to_string(),
            target_agent_did: daemon.agent_did,
            content_type: "application/json".to_string(),
            payload: json!({
                "schema": "awiki.agent.command.v1",
                "command_id": "cmd_runtime_inbox_unowned",
                "command": "runtime.inbox.query",
                "target_agent_kind": "daemon",
                "args": {
                    "runtime_agent_did": "did:agent:other-runtime",
                    "scope": "all"
                }
            }),
        },
    )
    .unwrap();

    let statuses = outbox.agent_statuses();
    assert_eq!(statuses.len(), 1);
    assert_eq!(statuses[0].payload["status_scope"], "runtime_inbox");
    assert_eq!(statuses[0].payload["state"], "failed");
    assert_eq!(
        statuses[0].payload["result"]["error_code"],
        "runtime_not_owned"
    );
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
    let created = expect_created(outcome);
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
