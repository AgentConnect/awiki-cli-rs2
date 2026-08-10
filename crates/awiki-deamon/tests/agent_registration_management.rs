use std::sync::{Arc, Mutex};

use awiki_deamon::agent::{workspace_id, AgentKind};
use awiki_deamon::commands::{
    handle_agent_payload_message, handle_agent_payload_message_with_readiness, setup_daemon_agent,
    AgentCommandOutcome, IncomingAgentPayloadMessage, RuntimeAgentCreateOutcome,
    RuntimeAgentMessageReadiness,
};
use awiki_deamon::outbox::MemoryRuntimeOutbox;
use awiki_deamon::plugins::hermes::{AWIKI_SKILLS_VERSION, HERMES_RUNTIME_PLUGIN_ID};
use awiki_deamon::registration::{
    AgentInventoryClient, AgentInvocationAuthorization, AgentLatestStatusUpdateItem,
    AgentRegistrationClient, AgentRegistrationExchangeRequest, AgentRegistrationExchangeResult,
    ControllerSenderScope, DidAuthMaterial, RegistrationToken, RegistrationTokenMetadata,
};
use awiki_deamon::state::{AppPersonalAgentBindingRecord, CreateCliRouteSession};
use awiki_deamon::workspace::WorkspaceMode;
use awiki_deamon::{
    daemon_cli::{setup_daemon_agent_from_token, SetupDaemonAgentOptions},
    run_command_json, DaemonCommand, DaemonConfig, DaemonState,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use rusqlite::Connection;
use serde_json::json;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Default)]
struct MockRegistrationClient {
    requests: Arc<Mutex<Vec<AgentRegistrationExchangeRequest>>>,
    archive_requests: Arc<Mutex<Vec<(String, String)>>>,
    fail_reason: Option<String>,
}

#[derive(Clone)]
struct RecordingRuntimeReadiness {
    calls: Arc<Mutex<Vec<String>>>,
    outbox: MemoryRuntimeOutbox,
    fail: bool,
}

impl RuntimeAgentMessageReadiness for RecordingRuntimeReadiness {
    fn ensure_message_ready(&self, outcome: &RuntimeAgentCreateOutcome) -> anyhow::Result<()> {
        assert!(
            self.outbox
                .agent_statuses()
                .iter()
                .all(|status| status.payload["state"] != "ready"),
            "ready status must not be emitted before the message baseline"
        );
        self.calls.lock().unwrap().push(outcome.agent_did.clone());
        if self.fail {
            anyhow::bail!("initial message baseline unavailable");
        }
        Ok(())
    }
}

impl MockRegistrationClient {
    fn requests(&self) -> Vec<AgentRegistrationExchangeRequest> {
        self.requests.lock().unwrap().clone()
    }

    fn archive_requests(&self) -> Vec<(String, String)> {
        self.archive_requests.lock().unwrap().clone()
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
        let account_id = format!("user_{}", request.handle);
        let manifest = anp::authentication::validate_device_manifest(&request.did_document)
            .unwrap()
            .unwrap();
        let device = manifest.devices.first().unwrap();
        let response_handle = request.handle.clone();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let claims = json!({
            "iss": "user-service",
            "aud": ["awiki-user-service", "awiki-message-service"],
            "sub": did,
            "type": "access",
            "purpose": "awiki.device.access.v1",
            "did": did,
            "user_id": account_id,
            "device_id": device.device_id,
            "key_id": device.signing_key_id,
            "auth_generation": 1,
            "scopes": ["device:manage", "device:read", "message:connect"],
            "iat": now,
            "nbf": now,
            "exp": now + 3600,
            "jti": format!("mock-device-{}", device.device_id),
        });
        let access_token = format!(
            "e30.{}.test-signature",
            URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).unwrap())
        );
        Ok(AgentRegistrationExchangeResult {
            token_id: format!("agtok_{}_{}", request.agent_kind.as_str(), request.handle),
            did,
            user_id: Some(account_id),
            agent_kind: request.agent_kind,
            controller_user_id: "user-alice".to_string(),
            controller_full_handle: "alice.anpclaw.com".to_string(),
            controller_did: request.controller_did,
            handle: response_handle,
            binding_generation: Some("1".to_string()),
            status: "registered".to_string(),
            access_token: Some(access_token),
        })
    }
}

impl AgentInventoryClient for MockRegistrationClient {
    fn verify_token(
        &self,
        _token: &RegistrationToken,
    ) -> anyhow::Result<RegistrationTokenMetadata> {
        anyhow::bail!("verify_token is not used in agent registration management tests")
    }

    fn sync_controller_scope(
        &self,
        daemon_agent_did: &str,
        _auth: &DidAuthMaterial,
    ) -> anyhow::Result<serde_json::Value> {
        Ok(json!({
            "agent_did": daemon_agent_did,
            "controller_user_id": "user-alice",
            "controller_full_handle": "alice.anpclaw.com",
            "controller_did": "did:human:alice",
            "updated_count": 1,
        }))
    }

    fn verify_controller_sender(
        &self,
        _daemon_agent_did: &str,
        sender_did: &str,
        _auth: &DidAuthMaterial,
    ) -> anyhow::Result<ControllerSenderScope> {
        if sender_did == "did:human:alice" || sender_did == "did:human:alice-new" {
            Ok(ControllerSenderScope {
                controller_user_id: "user-alice".to_string(),
                controller_full_handle: "alice.anpclaw.com".to_string(),
                controller_did: sender_did.to_string(),
                sender_did: sender_did.to_string(),
            })
        } else {
            anyhow::bail!("controller_scope_mismatch")
        }
    }

    fn authorize_agent_invocation(
        &self,
        _daemon_agent_did: &str,
        _agent_did: &str,
        _sender_did: &str,
        _source_conversation_id: Option<&str>,
        _source_message_id: Option<&str>,
        _auth: &DidAuthMaterial,
    ) -> anyhow::Result<AgentInvocationAuthorization> {
        anyhow::bail!(
            "authorize_agent_invocation is not used in agent registration management tests"
        )
    }

    fn update_latest_status(
        &self,
        _daemon_agent_did: &str,
        _statuses: Vec<AgentLatestStatusUpdateItem>,
        _auth: &DidAuthMaterial,
    ) -> anyhow::Result<serde_json::Value> {
        anyhow::bail!("update_latest_status is not used in agent registration management tests")
    }

    fn archive_agent(
        &self,
        daemon_agent_did: &str,
        agent_did: &str,
        _auth: &DidAuthMaterial,
    ) -> anyhow::Result<serde_json::Value> {
        self.archive_requests
            .lock()
            .unwrap()
            .push((daemon_agent_did.to_string(), agent_did.to_string()));
        Ok(json!({ "archived": [] }))
    }
}

fn fixture() -> (tempfile::TempDir, DaemonConfig, DaemonState) {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    config.ensure_state_layout().unwrap();
    let state = DaemonState::open_with_root_key_bytes(&config, [21_u8; 32]);
    state.initialize().unwrap();
    (root, config, state)
}

fn write_release_status_manifest(root: &std::path::Path, latest: &str) {
    let releases = root.join("releases");
    std::fs::create_dir_all(&releases).unwrap();
    std::fs::write(
        releases.join("manifest.json"),
        serde_json::to_vec_pretty(&json!({
            "latest": latest,
            "min_supported": "0.1.0",
            "packages": []
        }))
        .unwrap(),
    )
    .unwrap();
}

fn expect_created(outcome: AgentCommandOutcome) -> RuntimeAgentCreateOutcome {
    match outcome {
        AgentCommandOutcome::RuntimeAgentCreated(created) => created,
        other => panic!("expected runtime agent create outcome, got {other:?}"),
    }
}

fn assert_codex_profile_home(
    config: &DaemonConfig,
    runtime_profile_id: &str,
) -> std::path::PathBuf {
    let expected = config
        .state_root
        .join("runtime")
        .join("profiles")
        .join(runtime_profile_id)
        .join("codex-home");
    assert!(expected.is_dir(), "missing {}", expected.display());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let profile_dir = expected.parent().unwrap();
        let profile_mode = std::fs::metadata(profile_dir).unwrap().permissions().mode() & 0o777;
        assert_eq!(profile_mode, 0o700);
        let mode = std::fs::metadata(&expected).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);
    }
    expected
}

fn create_cli_route_session(
    config: &DaemonConfig,
    state: &DaemonState,
    created: &RuntimeAgentCreateOutcome,
    daemon: &awiki_deamon::agent::AgentDefinition,
    conversation_id: &str,
    native_session_id: &str,
) -> awiki_deamon::state::CliRouteSessionRecord {
    let route = state
        .get_or_create_cli_route_session(CreateCliRouteSession {
            agent_did: created.agent_did.clone(),
            runtime_profile_id: created.runtime_profile_id.clone(),
            driver_id: created
                .driver_id
                .clone()
                .unwrap_or_else(|| "codex".to_string()),
            controller_user_id: daemon.controller_user_id.clone(),
            controller_full_handle: daemon.controller_full_handle.clone(),
            controller_scope_key: daemon.controller_scope_key.clone(),
            controller_did: daemon.controller_did.clone(),
            conversation_id: conversation_id.to_string(),
            workspace_path: config
                .state_root
                .join("runtime")
                .join("workspaces")
                .join(&created.runtime_profile_id)
                .join("conversations")
                .join(conversation_id.replace(':', "_")),
            session_dir: config
                .state_root
                .join("runtime")
                .join("sessions")
                .join(&created.runtime_profile_id)
                .join(conversation_id.replace(':', "_")),
        })
        .unwrap();
    state
        .update_cli_route_session_native_id(
            &route.route_key,
            Some(native_session_id),
            Some("json_event"),
            Some(&route.route_key),
        )
        .unwrap();
    state
        .load_cli_route_session(&route.route_key)
        .unwrap()
        .unwrap()
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
        r#"{"peer_user_id":"user-bob","peer_full_handle":"bob.anpclaw.com","peer_current_did":"did:human:bob"}"#,
    );
    insert_projected_message(
        &connection,
        &owner_identity_id,
        runtime_agent_did,
        "msg-direct-bob-new-did",
        "dm:did:human:bob-new",
        0,
        "did:human:bob-new",
        runtime_agent_did,
        "",
        "",
        "text/plain",
        "hello from new did",
        "2026-06-04T10:01:00Z",
        0,
        r#"{"peer_user_id":"user-bob","peer_full_handle":"bob.anpclaw.com","peer_current_did":"did:human:bob-new"}"#,
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
        "{}",
    );
    insert_projected_message(
        &connection,
        &owner_identity_id,
        runtime_agent_did,
        "msg-group-attachment-summary",
        "group:did:group:summary",
        0,
        "did:human:dana",
        runtime_agent_did,
        "did:group:summary",
        "did:group:summary",
        "application/anp-attachment-manifest+json",
        r#"{"attachments":[]}"#,
        "2026-06-04T10:06:00Z",
        1,
        r#"{"attachment_summary":{"attachment_id":"att-summary","filename":"summary.md","mime_type":"text/markdown","size_bytes":2048},"has_attachments":true}"#,
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
    metadata: &str,
) {
    connection
        .execute(
            r#"
INSERT INTO messages
    (msg_id, owner_identity_id, owner_did, conversation_id, thread_id, direction,
     sender_did, receiver_did, group_id, group_did, content_type, content,
     sent_at, stored_at, is_read, metadata)
VALUES (?1, ?2, ?3, ?4, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?12, ?13, ?14)"#,
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
                metadata,
            ],
        )
        .unwrap();
}

fn test_identity_alias(did: &str) -> String {
    did.rsplit(':').next().unwrap().to_string()
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
    let readiness = RecordingRuntimeReadiness {
        calls: Arc::new(Mutex::new(Vec::new())),
        outbox: outbox.clone(),
        fail: false,
    };
    let outcome = handle_agent_payload_message_with_readiness(
        &config,
        &state,
        &registration,
        &outbox,
        &readiness,
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
                    "registration_token": "tok_runtime_secret_value",
                    "display_name": "Alice Coder"
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
    assert_eq!(
        readiness.calls.lock().unwrap().as_slice(),
        &[created.agent_did.clone()]
    );
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
        Some("workspace_claude_code_alice_awiki_coder")
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
    let runtime_profile = state
        .load_runtime_agent_profile(&created.agent_did)
        .unwrap();
    assert_eq!(
        runtime_profile.workspace_mode,
        Some(WorkspaceMode::RouteRoot)
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
    assert!(state
        .load_agent_auth_token(&daemon.agent_did)
        .unwrap()
        .is_none());
    assert!(state
        .load_agent_auth_token(&created.agent_did)
        .unwrap()
        .is_none());
    assert!(state
        .load_agent_device_identity(&daemon.agent_did)
        .unwrap()
        .is_some());
    assert!(state
        .load_agent_device_identity(&created.agent_did)
        .unwrap()
        .is_some());

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
fn runtime_create_reports_ready_only_after_retryable_message_readiness_succeeds() {
    let (root, config, state) = fixture();
    let registration = MockRegistrationClient::default();
    let daemon = setup_daemon_agent(
        &config,
        &state,
        &registration,
        "readiness-daemon",
        "did:human:alice",
        RegistrationToken::new("tok_daemon_readiness").unwrap(),
    )
    .unwrap();
    let outbox = MemoryRuntimeOutbox::default();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let message = || IncomingAgentPayloadMessage {
        message_id: "msg_runtime_readiness".to_owned(),
        conversation_id: Some("conv_runtime_readiness".to_owned()),
        sender_did: "did:human:alice".to_owned(),
        target_agent_did: daemon.agent_did.clone(),
        content_type: "application/json".to_owned(),
        payload: json!({
            "schema": "awiki.agent.command.v1",
            "command_id": "cmd_runtime_readiness",
            "command": "runtime.agent.create",
            "target_agent_kind": "runtime",
            "args": {
                "handle": "runtime-readiness",
                "runtime": "claude-code",
                "display_name": "Runtime Readiness",
                "workspace": root.path().join("readiness-workspace").display().to_string(),
                "controller_did": "did:human:alice",
                "registration_token": "tok_runtime_readiness",
                "client_request_id": "app_req_runtime_readiness"
            }
        }),
    };
    let failing = RecordingRuntimeReadiness {
        calls: calls.clone(),
        outbox: outbox.clone(),
        fail: true,
    };

    let error = handle_agent_payload_message_with_readiness(
        &config,
        &state,
        &registration,
        &outbox,
        &failing,
        message(),
    )
    .unwrap_err();
    assert!(error.to_string().contains("message readiness"));
    let statuses = outbox.agent_statuses();
    assert_eq!(statuses.len(), 1);
    assert_eq!(statuses[0].payload["state"], "failed");
    assert_eq!(statuses[0].payload["result"]["phase"], "message_readiness");

    let succeeding = RecordingRuntimeReadiness {
        calls: calls.clone(),
        outbox: outbox.clone(),
        fail: false,
    };
    let outcome = handle_agent_payload_message_with_readiness(
        &config,
        &state,
        &registration,
        &outbox,
        &succeeding,
        message(),
    )
    .unwrap();
    let created = expect_created(outcome);
    assert_eq!(
        calls.lock().unwrap().as_slice(),
        &[created.agent_did.clone(), created.agent_did]
    );
    let statuses = outbox.agent_statuses();
    assert_eq!(statuses.len(), 2);
    assert_eq!(statuses[1].payload["state"], "ready");
    assert_eq!(registration.requests().len(), 2);
}

#[test]
fn runtime_agent_create_rejects_missing_handle() {
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
    .unwrap_err();

    assert!(error
        .to_string()
        .contains("runtime.agent.create handle is required"));
    assert_eq!(registration.requests().len(), 1);
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
                "handle": "@alice-hermes-once",
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
    assert_eq!(
        second.client_request_id.as_deref(),
        Some("app_req_create_hermes_once")
    );
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
        statuses[0].payload["result"]["client_request_id"],
        "app_req_create_hermes_once"
    );
    assert_eq!(
        statuses[1].payload["result"]["client_request_id"],
        "app_req_create_hermes_once"
    );
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
    let (root, mut config, state) = fixture();
    let release_root = root.path().join("daemon-release");
    write_release_status_manifest(&release_root, awiki_deamon::upgrade::CURRENT_DAEMON_VERSION);
    config.download_base_url = format!("file://{}", release_root.display());
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
    assert!(status.payload["daemon"].get("display_name").is_none());
    assert_eq!(status.payload["runs"], json!([]));
    for runtime in status.payload["runtimes"].as_array().unwrap() {
        assert!(runtime.get("display_name").is_none());
    }
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
fn runtime_session_list_returns_redacted_generic_cli_routes_for_selected_profile() {
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
    let codex = expect_created(
        handle_agent_payload_message(
            &config,
            &state,
            &registration,
            &outbox,
            IncomingAgentPayloadMessage {
                message_id: "msg_create_codex_for_session_list".to_string(),
                conversation_id: Some("conv_create_codex_for_session_list".to_string()),
                sender_did: "did:human:alice".to_string(),
                target_agent_did: daemon.agent_did.clone(),
                content_type: "application/json".to_string(),
                payload: json!({
                    "schema": "awiki.agent.command.v1",
                    "command_id": "cmd_create_codex_for_session_list",
                    "command": "runtime.agent.create",
                    "target_agent_kind": "runtime",
                    "args": {
                        "handle": "@alice-codex-session-list",
                        "runtime": "codex",
                        "driver_id": "codex",
                        "controller_did": "did:human:alice",
                        "registration_token": "tok_runtime_secret_value",
                        "display_name": "Codex Session List"
                    }
                }),
            },
        )
        .unwrap(),
    );
    let claude = expect_created(
        handle_agent_payload_message(
            &config,
            &state,
            &registration,
            &outbox,
            IncomingAgentPayloadMessage {
                message_id: "msg_create_claude_for_session_list".to_string(),
                conversation_id: Some("conv_create_claude_for_session_list".to_string()),
                sender_did: "did:human:alice".to_string(),
                target_agent_did: daemon.agent_did.clone(),
                content_type: "application/json".to_string(),
                payload: json!({
                    "schema": "awiki.agent.command.v1",
                    "command_id": "cmd_create_claude_for_session_list",
                    "command": "runtime.agent.create",
                    "target_agent_kind": "runtime",
                    "args": {
                        "handle": "@alice-claude-session-list",
                        "runtime": "claude-code",
                        "driver_id": "claude-code",
                        "controller_did": "did:human:alice",
                        "registration_token": "tok_runtime_secret_value",
                        "display_name": "Claude Session List"
                    }
                }),
            },
        )
        .unwrap(),
    );
    let bob_route = create_cli_route_session(
        &config,
        &state,
        &codex,
        &daemon,
        "direct:did:human:bob",
        "codex-native-bob",
    );
    let charlie_route = create_cli_route_session(
        &config,
        &state,
        &codex,
        &daemon,
        "direct:did:human:charlie",
        "codex-native-charlie",
    );
    let claude_bob_route = create_cli_route_session(
        &config,
        &state,
        &claude,
        &daemon,
        "direct:did:human:bob",
        "claude-native-bob",
    );

    handle_agent_payload_message(
        &config,
        &state,
        &registration,
        &outbox,
        IncomingAgentPayloadMessage {
            message_id: "msg_generic_cli_session_list".to_string(),
            conversation_id: Some("conv_generic_cli_session_list".to_string()),
            sender_did: "did:human:alice".to_string(),
            target_agent_did: daemon.agent_did.clone(),
            content_type: "application/json".to_string(),
            payload: json!({
                "schema": "awiki.agent.command.v1",
                "command_id": "cmd_generic_cli_session_list",
                "command": "runtime.session.list",
                "target_agent_kind": "runtime",
                "args": {
                    "runtime_agent_did": codex.agent_did,
                    "limit": 10
                }
            }),
        },
    )
    .unwrap();

    let status = outbox.agent_statuses().pop().unwrap();
    assert_eq!(status.payload["state"], "ready");
    assert_eq!(status.payload["result"]["command"], "runtime.session.list");
    assert_eq!(status.payload["result"]["runtime_plugin_id"], "generic-cli");
    assert_eq!(status.payload["result"]["driver_id"], "codex");
    assert_eq!(status.payload["result"]["runtime_profile_id_present"], true);
    assert_eq!(
        status.payload["result"]["controller_scope_key_present"],
        true
    );
    assert_eq!(status.payload["result"]["page"]["limit"], 10);
    let items = status.payload["result"]["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    let route_hashes = items
        .iter()
        .map(|item| item["route_key_hash"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert!(route_hashes.contains(&bob_route.route_key_hash.as_str()));
    assert!(route_hashes.contains(&charlie_route.route_key_hash.as_str()));
    assert!(items
        .iter()
        .all(|item| item["conversation_kind"] == "direct"));
    assert!(items
        .iter()
        .all(|item| item["native_session_present"] == true));

    let public_payload = status.payload.to_string();
    assert!(!public_payload.contains(&bob_route.route_key));
    assert!(!public_payload.contains(&charlie_route.route_key));
    assert!(!public_payload.contains(&claude_bob_route.route_key));
    assert!(!public_payload.contains("direct:did:human:bob"));
    assert!(!public_payload.contains("did:human:charlie"));
    assert!(!public_payload.contains(bob_route.workspace_path.to_string_lossy().as_ref()));
    assert!(!public_payload.contains(bob_route.session_dir.to_string_lossy().as_ref()));
    assert!(!public_payload.contains("codex-native-bob"));
    assert!(!public_payload.contains("claude-native-bob"));

    let audit_dump: String = state
        .connection()
        .unwrap()
        .query_row(
            "SELECT COALESCE(detail_json, '') FROM audit_log WHERE event_type = 'runtime.session.list' ORDER BY created_at_ms DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(audit_dump.contains("\"runtime_plugin_id\":\"generic-cli\""));
    assert!(audit_dump.contains("\"driver_id\":\"codex\""));
    assert!(audit_dump.contains("\"returned_count\":2"));
    assert!(!audit_dump.contains(&bob_route.route_key));
    assert!(!audit_dump.contains(bob_route.workspace_path.to_string_lossy().as_ref()));
    assert!(!audit_dump.contains("codex-native-bob"));
}

#[test]
fn runtime_session_status_filters_generic_cli_route_by_hash_without_cross_profile_leakage() {
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
    let codex = expect_created(
        handle_agent_payload_message(
            &config,
            &state,
            &registration,
            &outbox,
            IncomingAgentPayloadMessage {
                message_id: "msg_create_codex_for_session_status".to_string(),
                conversation_id: Some("conv_create_codex_for_session_status".to_string()),
                sender_did: "did:human:alice".to_string(),
                target_agent_did: daemon.agent_did.clone(),
                content_type: "application/json".to_string(),
                payload: json!({
                    "schema": "awiki.agent.command.v1",
                    "command_id": "cmd_create_codex_for_session_status",
                    "command": "runtime.agent.create",
                    "target_agent_kind": "runtime",
                    "args": {
                        "handle": "@alice-codex-session-status",
                        "runtime": "codex",
                        "driver_id": "codex",
                        "controller_did": "did:human:alice",
                        "registration_token": "tok_runtime_secret_value",
                        "display_name": "Codex Session Status"
                    }
                }),
            },
        )
        .unwrap(),
    );
    let claude = expect_created(
        handle_agent_payload_message(
            &config,
            &state,
            &registration,
            &outbox,
            IncomingAgentPayloadMessage {
                message_id: "msg_create_claude_for_session_status".to_string(),
                conversation_id: Some("conv_create_claude_for_session_status".to_string()),
                sender_did: "did:human:alice".to_string(),
                target_agent_did: daemon.agent_did.clone(),
                content_type: "application/json".to_string(),
                payload: json!({
                    "schema": "awiki.agent.command.v1",
                    "command_id": "cmd_create_claude_for_session_status",
                    "command": "runtime.agent.create",
                    "target_agent_kind": "runtime",
                    "args": {
                        "handle": "@alice-claude-session-status",
                        "runtime": "claude-code",
                        "driver_id": "claude-code",
                        "controller_did": "did:human:alice",
                        "registration_token": "tok_runtime_secret_value",
                        "display_name": "Claude Session Status"
                    }
                }),
            },
        )
        .unwrap(),
    );
    let bob_route = create_cli_route_session(
        &config,
        &state,
        &codex,
        &daemon,
        "direct:did:human:bob",
        "codex-native-bob",
    );
    let charlie_route = create_cli_route_session(
        &config,
        &state,
        &codex,
        &daemon,
        "direct:did:human:charlie",
        "codex-native-charlie",
    );
    let claude_bob_route = create_cli_route_session(
        &config,
        &state,
        &claude,
        &daemon,
        "direct:did:human:bob",
        "claude-native-bob",
    );

    handle_agent_payload_message(
        &config,
        &state,
        &registration,
        &outbox,
        IncomingAgentPayloadMessage {
            message_id: "msg_generic_cli_session_status".to_string(),
            conversation_id: Some("conv_generic_cli_session_status".to_string()),
            sender_did: "did:human:alice".to_string(),
            target_agent_did: daemon.agent_did.clone(),
            content_type: "application/json".to_string(),
            payload: json!({
                "schema": "awiki.agent.command.v1",
                "command_id": "cmd_generic_cli_session_status",
                "command": "runtime.session.status",
                "target_agent_kind": "runtime",
                "args": {
                    "runtime_agent_did": codex.agent_did.clone(),
                    "route_key_hash": bob_route.route_key_hash.clone()
                }
            }),
        },
    )
    .unwrap();

    let status = outbox.agent_statuses().pop().unwrap();
    assert_eq!(status.payload["state"], "ready");
    assert_eq!(
        status.payload["result"]["command"],
        "runtime.session.status"
    );
    let items = status.payload["result"]["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["route_key_hash"], bob_route.route_key_hash);
    assert_eq!(items[0]["native_session_present"], true);

    let public_payload = status.payload.to_string();
    assert!(!public_payload.contains(&charlie_route.route_key_hash));
    assert!(!public_payload.contains(&claude_bob_route.route_key_hash));
    assert!(!public_payload.contains(&bob_route.route_key));
    assert!(!public_payload.contains("direct:did:human:bob"));
    assert!(!public_payload.contains("codex-native-bob"));
    assert!(!public_payload.contains("claude-native-bob"));

    handle_agent_payload_message(
        &config,
        &state,
        &registration,
        &outbox,
        IncomingAgentPayloadMessage {
            message_id: "msg_missing_generic_cli_session_status".to_string(),
            conversation_id: Some("conv_missing_generic_cli_session_status".to_string()),
            sender_did: "did:human:alice".to_string(),
            target_agent_did: daemon.agent_did.clone(),
            content_type: "application/json".to_string(),
            payload: json!({
                "schema": "awiki.agent.command.v1",
                "command_id": "cmd_missing_generic_cli_session_status",
                "command": "runtime.session.status",
                "target_agent_kind": "runtime",
                "args": {
                    "runtime_agent_did": codex.agent_did,
                    "route_key_hash": "route_000000000000000000000000"
                }
            }),
        },
    )
    .unwrap();
    let status = outbox.agent_statuses().pop().unwrap();
    assert_eq!(status.payload["state"], "failed");
    assert_eq!(
        status.payload["result"]["error_code"],
        "route_session_not_found"
    );
    assert_eq!(status.payload["result"].get("items"), None);
}

#[test]
fn runtime_session_list_rejects_hermes_and_missing_generic_cli_profile() {
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
    let hermes = expect_created(
        handle_agent_payload_message(
            &config,
            &state,
            &registration,
            &outbox,
            IncomingAgentPayloadMessage {
                message_id: "msg_create_hermes_for_session_list".to_string(),
                conversation_id: Some("conv_create_hermes_for_session_list".to_string()),
                sender_did: "did:human:alice".to_string(),
                target_agent_did: daemon.agent_did.clone(),
                content_type: "application/json".to_string(),
                payload: json!({
                    "schema": "awiki.agent.command.v1",
                    "command_id": "cmd_create_hermes_for_session_list",
                    "command": "runtime.agent.create",
                    "target_agent_kind": "runtime",
                    "args": {
                        "handle": "@alice-hermes-session-list",
                        "runtime": "hermes",
                        "controller_did": "did:human:alice",
                        "registration_token": "tok_runtime_secret_value",
                        "display_name": "Hermes Session List"
                    }
                }),
            },
        )
        .unwrap(),
    );
    handle_agent_payload_message(
        &config,
        &state,
        &registration,
        &outbox,
        IncomingAgentPayloadMessage {
            message_id: "msg_hermes_session_list".to_string(),
            conversation_id: Some("conv_hermes_session_list".to_string()),
            sender_did: "did:human:alice".to_string(),
            target_agent_did: daemon.agent_did.clone(),
            content_type: "application/json".to_string(),
            payload: json!({
                "schema": "awiki.agent.command.v1",
                "command_id": "cmd_hermes_session_list",
                "command": "runtime.session.list",
                "target_agent_kind": "runtime",
                "args": {
                    "runtime_agent_did": hermes.agent_did
                }
            }),
        },
    )
    .unwrap();
    let status = outbox.agent_statuses().pop().unwrap();
    assert_eq!(status.payload["state"], "failed");
    assert_eq!(
        status.payload["result"]["error_code"],
        "unsupported_for_runtime"
    );
    assert_eq!(status.payload["result"].get("items"), None);

    let codex = expect_created(
        handle_agent_payload_message(
            &config,
            &state,
            &registration,
            &outbox,
            IncomingAgentPayloadMessage {
                message_id: "msg_create_codex_missing_profile_session_list".to_string(),
                conversation_id: Some("conv_create_codex_missing_profile_session_list".to_string()),
                sender_did: "did:human:alice".to_string(),
                target_agent_did: daemon.agent_did.clone(),
                content_type: "application/json".to_string(),
                payload: json!({
                    "schema": "awiki.agent.command.v1",
                    "command_id": "cmd_create_codex_missing_profile_session_list",
                    "command": "runtime.agent.create",
                    "target_agent_kind": "runtime",
                    "args": {
                        "handle": "@alice-codex-missing-profile-session-list",
                        "runtime": "codex",
                        "driver_id": "codex",
                        "controller_did": "did:human:alice",
                        "registration_token": "tok_runtime_secret_value",
                        "display_name": "Codex Missing Profile Session List"
                    }
                }),
            },
        )
        .unwrap(),
    );
    state
        .connection()
        .unwrap()
        .execute(
            "DELETE FROM cli_runtime_profile WHERE runtime_profile_id = ?1",
            [&codex.runtime_profile_id],
        )
        .unwrap();

    handle_agent_payload_message(
        &config,
        &state,
        &registration,
        &outbox,
        IncomingAgentPayloadMessage {
            message_id: "msg_missing_profile_session_list".to_string(),
            conversation_id: Some("conv_missing_profile_session_list".to_string()),
            sender_did: "did:human:alice".to_string(),
            target_agent_did: daemon.agent_did,
            content_type: "application/json".to_string(),
            payload: json!({
                "schema": "awiki.agent.command.v1",
                "command_id": "cmd_missing_profile_session_list",
                "command": "runtime.session.list",
                "target_agent_kind": "runtime",
                "args": {
                    "runtime_agent_did": codex.agent_did
                }
            }),
        },
    )
    .unwrap();
    let status = outbox.agent_statuses().pop().unwrap();
    assert_eq!(status.payload["state"], "failed");
    assert_eq!(
        status.payload["result"]["error_code"],
        "runtime_profile_unavailable"
    );
    assert_eq!(status.payload["result"].get("items"), None);
}

#[test]
fn runtime_session_list_rejects_invalid_filters_and_unowned_runtime() {
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
    let codex = expect_created(
        handle_agent_payload_message(
            &config,
            &state,
            &registration,
            &outbox,
            IncomingAgentPayloadMessage {
                message_id: "msg_create_codex_for_invalid_session_list".to_string(),
                conversation_id: Some("conv_create_codex_for_invalid_session_list".to_string()),
                sender_did: "did:human:alice".to_string(),
                target_agent_did: daemon_one.agent_did.clone(),
                content_type: "application/json".to_string(),
                payload: json!({
                    "schema": "awiki.agent.command.v1",
                    "command_id": "cmd_create_codex_for_invalid_session_list",
                    "command": "runtime.agent.create",
                    "target_agent_kind": "runtime",
                    "args": {
                        "handle": "@alice-codex-invalid-session-list",
                        "runtime": "codex",
                        "driver_id": "codex",
                        "controller_did": "did:human:alice",
                        "registration_token": "tok_runtime_secret_value",
                        "display_name": "Codex Invalid Session List"
                    }
                }),
            },
        )
        .unwrap(),
    );

    handle_agent_payload_message(
        &config,
        &state,
        &registration,
        &outbox,
        IncomingAgentPayloadMessage {
            message_id: "msg_invalid_session_list_limit".to_string(),
            conversation_id: Some("conv_invalid_session_list_limit".to_string()),
            sender_did: "did:human:alice".to_string(),
            target_agent_did: daemon_one.agent_did.clone(),
            content_type: "application/json".to_string(),
            payload: json!({
                "schema": "awiki.agent.command.v1",
                "command_id": "cmd_invalid_session_list_limit",
                "command": "runtime.session.list",
                "target_agent_kind": "runtime",
                "args": {
                    "runtime_agent_did": codex.agent_did.clone(),
                    "limit": 101
                }
            }),
        },
    )
    .unwrap();
    let status = outbox.agent_statuses().pop().unwrap();
    assert_eq!(status.payload["state"], "failed");
    assert_eq!(status.payload["result"]["error_code"], "invalid_filter");
    assert_eq!(status.payload["result"]["filter"], "limit");

    handle_agent_payload_message(
        &config,
        &state,
        &registration,
        &outbox,
        IncomingAgentPayloadMessage {
            message_id: "msg_malformed_session_list_limit".to_string(),
            conversation_id: Some("conv_malformed_session_list_limit".to_string()),
            sender_did: "did:human:alice".to_string(),
            target_agent_did: daemon_one.agent_did.clone(),
            content_type: "application/json".to_string(),
            payload: json!({
                "schema": "awiki.agent.command.v1",
                "command_id": "cmd_malformed_session_list_limit",
                "command": "runtime.session.list",
                "target_agent_kind": "runtime",
                "args": {
                    "runtime_agent_did": codex.agent_did.clone(),
                    "limit": "not-a-number"
                }
            }),
        },
    )
    .unwrap();
    let status = outbox.agent_statuses().pop().unwrap();
    assert_eq!(status.payload["state"], "failed");
    assert_eq!(status.payload["result"]["error_code"], "invalid_filter");
    assert_eq!(status.payload["result"]["filter"], "limit");

    handle_agent_payload_message(
        &config,
        &state,
        &registration,
        &outbox,
        IncomingAgentPayloadMessage {
            message_id: "msg_unowned_session_list".to_string(),
            conversation_id: Some("conv_unowned_session_list".to_string()),
            sender_did: "did:human:alice".to_string(),
            target_agent_did: daemon_two.agent_did,
            content_type: "application/json".to_string(),
            payload: json!({
                "schema": "awiki.agent.command.v1",
                "command_id": "cmd_unowned_session_list",
                "command": "runtime.session.list",
                "target_agent_kind": "runtime",
                "args": {
                    "runtime_agent_did": codex.agent_did
                }
            }),
        },
    )
    .unwrap();
    let status = outbox.agent_statuses().pop().unwrap();
    assert_eq!(status.payload["state"], "failed");
    assert_eq!(status.payload["result"]["command"], "runtime.session.list");
    assert_eq!(status.payload["result"]["error_code"], "runtime_not_owned");
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
fn daemon_upgrade_cancel_reports_not_running_without_running_download() {
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
            message_id: "msg_upgrade_cancel_idle".to_string(),
            conversation_id: Some("conv_upgrade_cancel_idle".to_string()),
            sender_did: "did:human:alice".to_string(),
            target_agent_did: daemon.agent_did,
            content_type: "application/json".to_string(),
            payload: json!({
                "schema": "awiki.agent.command.v1",
                "command_id": "cmd_upgrade_cancel_idle",
                "command": "daemon.upgrade.cancel",
                "target_agent_kind": "daemon",
                "args": {}
            }),
        },
    )
    .unwrap();

    let status = outbox.agent_statuses().pop().unwrap();
    assert_eq!(status.payload["state"], "ready");
    assert_eq!(status.payload["result"]["command"], "daemon.upgrade.cancel");
    assert_eq!(status.payload["result"]["status"], "not_running");
}

#[test]
fn daemon_upgrade_cancel_rejects_restart_scheduled_upgrade() {
    let (root, mut config, state) = fixture();
    let release_root = root.path().join("daemon-release");
    write_release_status_manifest(&release_root, "9999.0.0");
    config.download_base_url = format!("file://{}", release_root.display());
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
    state
        .try_begin_control_command(
            &daemon.agent_did,
            &daemon.controller_scope_key,
            "cmd_upgrade_restarting",
            "daemon.upgrade",
            "msg_upgrade_restarting",
            Some("latest"),
        )
        .unwrap();
    state
        .mark_control_command_state(
            &daemon.agent_did,
            &daemon.controller_scope_key,
            "cmd_upgrade_restarting",
            "restart_scheduled",
            json!({
                "command": "daemon.upgrade",
                "daemon_agent_did": daemon.agent_did,
                "status": "restart_scheduled",
            }),
            None,
        )
        .unwrap();
    let outbox = MemoryRuntimeOutbox::default();

    handle_agent_payload_message(
        &config,
        &state,
        &registration,
        &outbox,
        IncomingAgentPayloadMessage {
            message_id: "msg_upgrade_cancel_restarting".to_string(),
            conversation_id: Some("conv_upgrade_cancel_restarting".to_string()),
            sender_did: "did:human:alice".to_string(),
            target_agent_did: daemon.agent_did,
            content_type: "application/json".to_string(),
            payload: json!({
                "schema": "awiki.agent.command.v1",
                "command_id": "cmd_upgrade_cancel_restarting",
                "command": "daemon.upgrade.cancel",
                "target_agent_kind": "daemon",
                "args": {}
            }),
        },
    )
    .unwrap();

    let status = outbox.agent_statuses().pop().unwrap();
    assert_eq!(status.payload["state"], "failed");
    assert_eq!(
        status.payload["result"]["error_code"],
        "upgrade_not_cancellable"
    );
    assert_eq!(
        status.payload["result"]["upgrade_command_id"],
        "cmd_upgrade_restarting"
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
                    "registration_token": "tok_runtime_secret_value",
                    "display_name": "Generic CLI"
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
    let codex_home = assert_codex_profile_home(&config, &created.runtime_profile_id);
    assert_eq!(cli_profile.config_home, Some(codex_home));
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
                        "registration_token": "tok_runtime_secret_value",
                        "display_name": format!("{runtime} Runtime")
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
        let runtime_profile = state
            .load_runtime_agent_profile(&created.agent_did)
            .unwrap();
        assert_eq!(
            runtime_profile.workspace_mode,
            Some(WorkspaceMode::RouteRoot)
        );
        assert_eq!(
            runtime_profile.workspace_id.as_deref(),
            Some(
                workspace_id(&format!("{expected_driver_id}-alice-{runtime}"))
                    .unwrap()
                    .as_str()
            )
        );
        assert_eq!(
            runtime_profile.workspace_root,
            Some(
                config
                    .state_root
                    .join("runtime")
                    .join("workspaces")
                    .join(expected_profile_id)
            )
        );
        let cli_profile = state
            .load_cli_runtime_profile(&created.runtime_profile_id)
            .unwrap();
        assert_eq!(cli_profile.driver_id, expected_driver_id);
        assert_eq!(cli_profile.default_workspace_mode, WorkspaceMode::RouteRoot);
        if expected_driver_id == "codex" {
            let codex_home = assert_codex_profile_home(&config, &created.runtime_profile_id);
            assert_eq!(cli_profile.config_home, Some(codex_home));
        } else {
            assert_eq!(cli_profile.config_home, None);
        }
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
                    "registration_token": "tok_runtime_secret_value",
                    "display_name": "Generic Default"
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
    let codex_home = assert_codex_profile_home(&config, &created.runtime_profile_id);
    assert_eq!(cli_profile.config_home, Some(codex_home));
}

#[test]
fn runtime_agent_create_accepts_workspace_strategy_alias_for_generic_cli() {
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
            message_id: "msg_create_workspace_strategy_alias".to_string(),
            conversation_id: Some("conv_create_workspace_strategy_alias".to_string()),
            sender_did: "did:human:alice".to_string(),
            target_agent_did: daemon.agent_did,
            content_type: "application/json".to_string(),
            payload: json!({
                "schema": "awiki.agent.command.v1",
                "command_id": "cmd_create_workspace_strategy_alias",
                "command": "runtime.agent.create",
                "target_agent_kind": "runtime",
                "args": {
                    "handle": "@alice-codex-strategy",
                    "runtime": "codex",
                    "driver_id": "codex",
                    "workspace_strategy": "route-root",
                    "controller_did": "did:human:alice",
                    "registration_token": "tok_runtime_secret_value",
                    "display_name": "Codex Strategy Alias"
                }
            }),
        },
    )
    .unwrap();

    let created = expect_created(outcome);
    let runtime_profile = state
        .load_runtime_agent_profile(&created.agent_did)
        .unwrap();
    assert_eq!(
        runtime_profile.workspace_mode,
        Some(WorkspaceMode::RouteRoot)
    );
    let cli_profile = state
        .load_cli_runtime_profile(&created.runtime_profile_id)
        .unwrap();
    assert_eq!(cli_profile.default_workspace_mode, WorkspaceMode::RouteRoot);
}

#[test]
fn runtime_agent_create_rejects_conflicting_workspace_mode_and_strategy() {
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
            message_id: "msg_create_workspace_strategy_conflict".to_string(),
            conversation_id: Some("conv_create_workspace_strategy_conflict".to_string()),
            sender_did: "did:human:alice".to_string(),
            target_agent_did: daemon.agent_did,
            content_type: "application/json".to_string(),
            payload: json!({
                "schema": "awiki.agent.command.v1",
                "command_id": "cmd_create_workspace_strategy_conflict",
                "command": "runtime.agent.create",
                "target_agent_kind": "runtime",
                "args": {
                    "handle": "@alice-codex-conflict",
                    "runtime": "codex",
                    "driver_id": "codex",
                    "workspace_mode": "route-root",
                    "workspace_strategy": "shared-root",
                    "controller_did": "did:human:alice",
                    "registration_token": "tok_runtime_secret_value",
                    "display_name": "Codex Strategy Conflict",
                    "client_request_id": "app_req_workspace_strategy_conflict"
                }
            }),
        },
    )
    .unwrap_err();

    assert!(error
        .to_string()
        .contains("workspace_mode and workspace_strategy"));
    assert_eq!(state.list_runtime_agent_definitions().unwrap().len(), 0);
    assert_eq!(outbox.agent_statuses().len(), 1);
    assert_eq!(outbox.agent_statuses()[0].payload["state"], "failed");
    assert_eq!(
        outbox.agent_statuses()[0].payload["result"]["client_request_id"],
        "app_req_workspace_strategy_conflict"
    );
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
                    "registration_token": "tok_runtime_secret_value",
                    "display_name": "Invalid Generic"
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
                    "registration_token": "tok_runtime_secret_value",
                    "display_name": "Unauthorized Runtime"
                }
            }),
        },
    )
    .unwrap_err();

    assert!(error
        .chain()
        .any(|cause| cause.to_string().contains("controller_scope_mismatch")));
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
                    "registration_token": "tok_runtime_secret_value",
                    "display_name": "Failed Runtime"
                }
            }),
        },
    )
    .unwrap_err();

    assert!(format!("{error:#}").contains("scope_mismatch"));
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
                    "registration_token": "tok_runtime_secret_value",
                    "display_name": "Inbox Runtime"
                }
            }),
        },
    )
    .unwrap();
    let created = expect_created(outcome);
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
    assert_eq!(items.len(), 3);
    assert_eq!(items[0]["kind"], "group");
    assert_eq!(items[0]["group_did"], "did:group:summary");
    assert_eq!(items[0]["peer_did"], serde_json::Value::Null);
    assert_eq!(items[0]["title"], "did:group:summary");
    assert_eq!(items[0]["display"]["title"], "did:group:summary");
    assert_eq!(items[0]["display"]["source"], "did_fallback");
    assert_eq!(items[0]["last_message_preview"], "附件: summary.md");
    assert_eq!(items[0]["has_attachments"], true);
    assert_eq!(items[0]["last_content_type"], "attachment");
    assert_eq!(items[1]["kind"], "group");
    assert_eq!(items[1]["group_did"], "did:group:team");
    assert_eq!(items[1]["peer_did"], serde_json::Value::Null);
    assert_eq!(items[1]["display"]["title"], "did:group:team");
    assert_eq!(items[1]["display"]["source"], "did_fallback");
    assert_eq!(items[1]["has_attachments"], true);
    assert_eq!(items[1]["last_content_type"], "attachment");
    assert_eq!(items[2]["kind"], "direct");
    let direct_thread_id = items[2]["thread_id"].as_str().unwrap().to_string();
    assert!(direct_thread_id.starts_with("dm:peer-scope:v1:"));
    assert_eq!(items[2]["title"], "bob.anpclaw.com");
    assert_eq!(items[2]["peer_user_id"], "user-bob");
    assert_eq!(items[2]["peer_handle"], "bob.anpclaw.com");
    assert_eq!(items[2]["peer_did"], "did:human:bob-new");
    assert_eq!(items[2]["group_did"], serde_json::Value::Null);
    assert_eq!(items[2]["display"]["title"], "bob.anpclaw.com");
    assert_eq!(items[2]["display"]["source"], "did_fallback");
    assert_eq!(items[2]["last_message_preview"], "hello from new did");

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
    assert_eq!(payload["result"]["kind"], "group");
    assert_eq!(payload["result"]["peer_did"], serde_json::Value::Null);
    assert_eq!(payload["result"]["group_did"], "did:group:team");
    assert_eq!(payload["result"]["title"], "did:group:team");
    assert_eq!(payload["result"]["display"]["title"], "did:group:team");
    assert_eq!(payload["result"]["display"]["source"], "did_fallback");
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

    let summary_thread_outbox = MemoryRuntimeOutbox::default();
    handle_agent_payload_message(
        &config,
        &state,
        &registration,
        &summary_thread_outbox,
        IncomingAgentPayloadMessage {
            message_id: "msg_query_runtime_inbox_summary_thread".to_string(),
            conversation_id: Some("conv_daemon_inbox".to_string()),
            sender_did: "did:human:alice".to_string(),
            target_agent_did: daemon.agent_did.clone(),
            content_type: "application/json".to_string(),
            payload: json!({
                "schema": "awiki.agent.command.v1",
                "command_id": "cmd_runtime_inbox_summary_thread_query",
                "command": "runtime.inbox.thread.query",
                "target_agent_kind": "daemon",
                "args": {
                    "runtime_agent_did": created.agent_did,
                    "thread_id": "group:did:group:summary",
                    "kind": "group",
                    "group_did": "did:group:summary",
                    "limit": 20
                }
            }),
        },
    )
    .unwrap();
    let statuses = summary_thread_outbox.agent_statuses();
    assert_eq!(statuses.len(), 1);
    let payload = &statuses[0].payload;
    assert_eq!(payload["status_scope"], "runtime_inbox_thread");
    assert_eq!(payload["state"], "succeeded");
    let messages = payload["result"]["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0]["content_type"], "attachment");
    assert_eq!(
        messages[0]["attachments"][0]["attachment_id"],
        "att-summary"
    );
    assert_eq!(messages[0]["attachments"][0]["filename"], "summary.md");
    assert_eq!(messages[0]["attachments"][0]["mime_type"], "text/markdown");
    assert_eq!(messages[0]["attachments"][0]["size_bytes"], 2048);

    let direct_thread_outbox = MemoryRuntimeOutbox::default();
    handle_agent_payload_message(
        &config,
        &state,
        &registration,
        &direct_thread_outbox,
        IncomingAgentPayloadMessage {
            message_id: "msg_query_runtime_inbox_direct_thread".to_string(),
            conversation_id: Some("conv_daemon_inbox".to_string()),
            sender_did: "did:human:alice".to_string(),
            target_agent_did: daemon.agent_did.clone(),
            content_type: "application/json".to_string(),
            payload: json!({
                "schema": "awiki.agent.command.v1",
                "command_id": "cmd_runtime_inbox_direct_thread_query",
                "command": "runtime.inbox.thread.query",
                "target_agent_kind": "daemon",
                "args": {
                    "runtime_agent_did": created.agent_did,
                    "thread_id": direct_thread_id,
                    "kind": "direct",
                    "peer_handle": "bob.anpclaw.com",
                    "peer_did": "did:human:bob-new",
                    "limit": 20
                }
            }),
        },
    )
    .unwrap();
    let statuses = direct_thread_outbox.agent_statuses();
    assert_eq!(statuses.len(), 1);
    let payload = &statuses[0].payload;
    assert_eq!(payload["status_scope"], "runtime_inbox_thread");
    assert_eq!(payload["state"], "succeeded");
    assert_eq!(payload["result"]["title"], "bob.anpclaw.com");
    let messages = payload["result"]["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0]["sender_handle"], "bob.anpclaw.com");
    assert_eq!(messages[1]["sender_handle"], "bob.anpclaw.com");
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
fn runtime_inbox_query_repairs_controller_direct_messages_from_runtime_scope() {
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
    let created = expect_created(
        handle_agent_payload_message(
            &config,
            &state,
            &registration,
            &outbox,
            IncomingAgentPayloadMessage {
                message_id: "msg_create_runtime_controller_inbox".to_string(),
                conversation_id: Some("conv_daemon_inbox".to_string()),
                sender_did: "did:human:alice".to_string(),
                target_agent_did: daemon.agent_did.clone(),
                content_type: "application/json".to_string(),
                payload: json!({
                    "schema": "awiki.agent.command.v1",
                    "command_id": "cmd_create_runtime_controller_inbox",
                    "command": "runtime.agent.create",
                    "target_agent_kind": "runtime",
                    "args": {
                        "handle": "@alice-controller-inbox-runtime",
                        "runtime": "claude-code",
                        "controller_did": "did:human:alice",
                        "registration_token": "tok_runtime_secret_value",
                        "display_name": "Controller Inbox Runtime"
                    }
                }),
            },
        )
        .unwrap(),
    );
    if let Some(parent) = config.im_core_sqlite_path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    let connection = Connection::open(&config.im_core_sqlite_path).unwrap();
    im_core::compat::local_state::ensure_schema(&connection).unwrap();
    let owner_identity_id = test_identity_alias(&created.agent_did);
    insert_projected_message(
        &connection,
        &owner_identity_id,
        &created.agent_did,
        "msg-controller-direct",
        "dm:did:human:alice",
        0,
        "did:human:alice",
        &created.agent_did,
        "",
        "",
        "text/plain",
        "hello runtime",
        "2026-06-04T10:00:00Z",
        0,
        "{}",
    );

    let inbox_outbox = MemoryRuntimeOutbox::default();
    handle_agent_payload_message(
        &config,
        &state,
        &registration,
        &inbox_outbox,
        IncomingAgentPayloadMessage {
            message_id: "msg_query_runtime_controller_inbox".to_string(),
            conversation_id: Some("conv_daemon_inbox".to_string()),
            sender_did: "did:human:alice".to_string(),
            target_agent_did: daemon.agent_did.clone(),
            content_type: "application/json".to_string(),
            payload: json!({
                "schema": "awiki.agent.command.v1",
                "command_id": "cmd_runtime_controller_inbox_query",
                "command": "runtime.inbox.query",
                "target_agent_kind": "daemon",
                "args": {
                    "runtime_agent_did": created.agent_did,
                    "scope": "direct",
                    "limit": 10
                }
            }),
        },
    )
    .unwrap();

    let statuses = inbox_outbox.agent_statuses();
    assert_eq!(statuses.len(), 1);
    let payload = &statuses[0].payload;
    assert_eq!(payload["status_scope"], "runtime_inbox");
    assert_eq!(payload["state"], "succeeded");
    let items = payload["result"]["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert!(items[0]["thread_id"]
        .as_str()
        .unwrap()
        .starts_with("dm:peer-scope:v1:"));
    assert_eq!(items[0]["title"], "alice.anpclaw.com");
    assert_eq!(items[0]["peer_user_id"], "user-alice");
    assert_eq!(items[0]["peer_handle"], "alice.anpclaw.com");
    assert_eq!(items[0]["peer_did"], "did:human:alice");
    assert_eq!(items[0]["last_message_preview"], "hello runtime");

    let repaired: (String, String) = connection
        .query_row(
            "SELECT conversation_id, metadata FROM messages WHERE msg_id = 'msg-controller-direct'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert!(repaired.0.starts_with("dm:peer-scope:v1:"));
    assert!(repaired.1.contains(r#""peer_user_id":"user-alice""#));
    assert!(repaired
        .1
        .contains(r#""peer_full_handle":"alice.anpclaw.com""#));
}

#[test]
fn runtime_inbox_query_keeps_scoped_thread_when_latest_outgoing_lacks_scope() {
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
    let created = expect_created(
        handle_agent_payload_message(
            &config,
            &state,
            &registration,
            &outbox,
            IncomingAgentPayloadMessage {
                message_id: "msg_create_runtime_scoped_inbox".to_string(),
                conversation_id: Some("conv_daemon_inbox".to_string()),
                sender_did: "did:human:alice".to_string(),
                target_agent_did: daemon.agent_did.clone(),
                content_type: "application/json".to_string(),
                payload: json!({
                    "schema": "awiki.agent.command.v1",
                    "command_id": "cmd_create_runtime_scoped_inbox",
                    "command": "runtime.agent.create",
                    "target_agent_kind": "runtime",
                    "args": {
                        "handle": "@alice-scoped-inbox-runtime",
                        "runtime": "hermes",
                        "controller_did": "did:human:alice",
                        "registration_token": "tok_runtime_secret_value",
                        "display_name": "Scoped Inbox Runtime"
                    }
                }),
            },
        )
        .unwrap(),
    );
    if let Some(parent) = config.im_core_sqlite_path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    let connection = Connection::open(&config.im_core_sqlite_path).unwrap();
    im_core::compat::local_state::ensure_schema(&connection).unwrap();
    let owner_identity_id = test_identity_alias(&created.agent_did);
    let scoped_thread_id =
        im_core::messages::direct_peer_scope_thread_id("user-alice", "alice.anpclaw.com")
            .unwrap()
            .as_str()
            .to_string();
    insert_projected_message(
        &connection,
        &owner_identity_id,
        &created.agent_did,
        "msg-controller-incoming",
        &scoped_thread_id,
        0,
        "did:human:alice",
        &created.agent_did,
        "",
        "",
        "text/plain",
        "现在几点了",
        "2026-06-04T10:00:00Z",
        0,
        r#"{
            "peer_user_id": "user-alice",
            "peer_full_handle": "alice.anpclaw.com",
            "peer_current_did": "did:human:alice"
        }"#,
    );
    insert_projected_message(
        &connection,
        &owner_identity_id,
        &created.agent_did,
        "msg-runtime-outgoing",
        &scoped_thread_id,
        1,
        &created.agent_did,
        "did:human:alice",
        "",
        "",
        "text/plain",
        "现在是 2026-06-04 10:00 UTC。",
        "2026-06-04T10:00:10Z",
        1,
        r#"{"content_type":"text/plain","delivery_state":"accepted"}"#,
    );

    let inbox_outbox = MemoryRuntimeOutbox::default();
    handle_agent_payload_message(
        &config,
        &state,
        &registration,
        &inbox_outbox,
        IncomingAgentPayloadMessage {
            message_id: "msg_query_runtime_scoped_inbox".to_string(),
            conversation_id: Some("conv_daemon_inbox".to_string()),
            sender_did: "did:human:alice".to_string(),
            target_agent_did: daemon.agent_did.clone(),
            content_type: "application/json".to_string(),
            payload: json!({
                "schema": "awiki.agent.command.v1",
                "command_id": "cmd_runtime_scoped_inbox_query",
                "command": "runtime.inbox.query",
                "target_agent_kind": "daemon",
                "args": {
                    "runtime_agent_did": created.agent_did,
                    "scope": "direct",
                    "limit": 10
                }
            }),
        },
    )
    .unwrap();

    let statuses = inbox_outbox.agent_statuses();
    assert_eq!(statuses.len(), 1);
    let payload = &statuses[0].payload;
    assert_eq!(payload["status_scope"], "runtime_inbox");
    assert_eq!(payload["state"], "succeeded");
    let items = payload["result"]["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["thread_id"], scoped_thread_id);
    assert_eq!(items[0]["title"], "alice.anpclaw.com");
    assert_eq!(
        items[0]["last_message_preview"],
        "现在是 2026-06-04 10:00 UTC。"
    );
    assert_eq!(items[0]["peer_user_id"], "user-alice");
    assert_eq!(items[0]["peer_handle"], "alice.anpclaw.com");
    assert_eq!(items[0]["peer_did"], "did:human:alice");

    let repaired_metadata: String = connection
        .query_row(
            "SELECT metadata FROM messages WHERE msg_id = 'msg-runtime-outgoing'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(repaired_metadata.contains(r#""peer_user_id":"user-alice""#));
    assert!(repaired_metadata.contains(r#""peer_full_handle":"alice.anpclaw.com""#));
}

#[test]
fn runtime_agent_delete_archives_owned_runtime_and_reports_status() {
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
    let created = expect_created(
        handle_agent_payload_message(
            &config,
            &state,
            &registration,
            &outbox,
            IncomingAgentPayloadMessage {
                message_id: "msg_create_for_delete".to_string(),
                conversation_id: Some("conv_delete".to_string()),
                sender_did: "did:human:alice".to_string(),
                target_agent_did: daemon.agent_did.clone(),
                content_type: "application/json".to_string(),
                payload: json!({
                    "schema": "awiki.agent.command.v1",
                    "command_id": "cmd_create_for_delete",
                    "command": "runtime.agent.create",
                    "target_agent_kind": "runtime",
                    "args": {
                        "handle": "@alice-delete-runtime",
                        "runtime": "hermes",
                        "controller_did": "did:human:alice",
                        "registration_token": "tok_runtime_secret_value",
                        "display_name": "Delete Runtime"
                    }
                }),
            },
        )
        .unwrap(),
    );
    let runtime = state.load_agent_definition(&created.agent_did).unwrap();
    let agent_db = config.state_root.join(&runtime.local_agent_db_path);
    let message_db = config.state_root.join(&runtime.message_db_path);
    std::fs::create_dir_all(agent_db.parent().unwrap()).unwrap();
    std::fs::write(&agent_db, b"agent").unwrap();
    std::fs::write(&message_db, b"message").unwrap();

    handle_agent_payload_message(
        &config,
        &state,
        &registration,
        &outbox,
        IncomingAgentPayloadMessage {
            message_id: "msg_delete_runtime".to_string(),
            conversation_id: Some("conv_delete".to_string()),
            sender_did: "did:human:alice".to_string(),
            target_agent_did: daemon.agent_did.clone(),
            content_type: "application/json".to_string(),
            payload: json!({
                "schema": "awiki.agent.command.v1",
                "command_id": "cmd_delete_runtime",
                "command": "runtime.agent.delete",
                "target_agent_kind": "runtime",
                "args": {
                    "runtime_agent_did": created.agent_did
                }
            }),
        },
    )
    .unwrap();

    assert_eq!(
        registration.archive_requests(),
        vec![(daemon.agent_did.clone(), created.agent_did.clone())]
    );
    assert_eq!(
        state
            .load_agent_definition(&created.agent_did)
            .unwrap()
            .status,
        "archived"
    );
    assert!(state
        .list_runtime_agent_definitions_for_daemon(&daemon.agent_did)
        .unwrap()
        .is_empty());
    let latest_items =
        awiki_deamon::agent_status::latest_status_items(&config, &state, &daemon, 1_700_000)
            .unwrap();
    assert_eq!(latest_items.len(), 1);
    assert_eq!(latest_items[0].agent_did, daemon.agent_did);
    let snapshot =
        awiki_deamon::agent_status::daemon_snapshot_payload(&config, &state, &daemon).unwrap();
    assert_eq!(snapshot["runtimes"].as_array().map(Vec::len), Some(0));
    assert!(!agent_db.exists());
    assert!(!message_db.exists());
    let archived = outbox.agent_statuses().last().unwrap().clone();
    assert_eq!(archived.payload["state"], "archived");
    assert_eq!(
        archived.payload["result"]["command"],
        "runtime.agent.delete"
    );
    assert_eq!(
        archived.payload["result"]["runtime_agent_did"],
        created.agent_did
    );
}

#[test]
fn personal_agent_binding_disable_command_stops_active_binding() {
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
    let created = expect_created(
        handle_agent_payload_message(
            &config,
            &state,
            &registration,
            &outbox,
            IncomingAgentPayloadMessage {
                message_id: "msg_create_personal_agent".to_string(),
                conversation_id: Some("conv_personal_agent".to_string()),
                sender_did: "did:human:alice".to_string(),
                target_agent_did: daemon.agent_did.clone(),
                content_type: "application/json".to_string(),
                payload: json!({
                    "schema": "awiki.agent.command.v1",
                    "command_id": "cmd_create_personal_agent",
                    "command": "runtime.agent.create",
                    "target_agent_kind": "runtime",
                    "args": {
                        "handle": "@hermes-personal-app-1",
                        "runtime": "hermes",
                        "controller_did": "did:human:alice",
                        "registration_token": "tok_runtime_secret_value",
                        "display_name": "Hermes Personal Agent"
                    }
                }),
            },
        )
        .unwrap(),
    );
    state
        .upsert_app_personal_agent_binding(&AppPersonalAgentBindingRecord {
            binding_id: "app-personal-agent:did:human:alice:app_1".to_string(),
            user_did: "did:human:alice".to_string(),
            inbox_auth_verification_method: "did:human:alice#daemon-key-1".to_string(),
            app_instance_id: "app_1".to_string(),
            bootstrap_id: "boot_1".to_string(),
            idempotency_key: "personal-agent-bootstrap:did:human:alice:app_1".to_string(),
            daemon_agent_did: daemon.agent_did.clone(),
            runtime_agent_did: created.agent_did.clone(),
            runtime_profile_id: created.runtime_profile_id.clone(),
            role: "app_message_handler".to_string(),
            desired_agent_json: json!({"role": "app_message_handler"}),
            capability_policy_json: json!({"allowed_actions": []}),
            status: "personal_agent_ready".to_string(),
            created_at_ms: 0,
            updated_at_ms: 0,
            revoked_at_ms: None,
        })
        .unwrap();

    handle_agent_payload_message(
        &config,
        &state,
        &registration,
        &outbox,
        IncomingAgentPayloadMessage {
            message_id: "msg_pause_personal_agent".to_string(),
            conversation_id: Some("conv_personal_agent".to_string()),
            sender_did: "did:human:alice".to_string(),
            target_agent_did: daemon.agent_did.clone(),
            content_type: "application/json".to_string(),
            payload: json!({
                "schema": "awiki.agent.command.v1",
                "command_id": "cmd_pause_personal_agent",
                "command": "personal_agent.binding.disable",
                "target_agent_kind": "runtime",
                "args": {
                    "personal_agent_did": created.agent_did,
                    "lifecycle_action": "pause"
                }
            }),
        },
    )
    .unwrap();

    assert!(state
        .load_active_app_personal_agent_binding_by_runtime(&created.agent_did)
        .unwrap()
        .is_none());
    let binding = state
        .load_active_or_inactive_app_personal_agent_binding_by_runtime(&created.agent_did)
        .unwrap()
        .unwrap();
    assert_eq!(binding.status, "personal_agent_disabled");
    assert!(binding.revoked_at_ms.is_none());
    let disabled = outbox.agent_statuses().last().unwrap().clone();
    assert_eq!(disabled.payload["state"], "disabled");
    assert_eq!(
        disabled.payload["result"]["command"],
        "personal_agent.binding.disable"
    );
    assert_eq!(
        disabled.payload["result"]["runtime_agent_did"],
        created.agent_did
    );
}

#[test]
fn legacy_message_agent_disable_command_emits_canonical_status() {
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
    let runtime_agent_did = "did:agent:legacy-personal";
    let legacy_binding_id = "app-message-agent:did:human:alice:app_1";
    let legacy_idempotency_key = "message-agent-bootstrap:did:human:alice:app_1";
    state
        .upsert_app_personal_agent_binding(&AppPersonalAgentBindingRecord {
            binding_id: legacy_binding_id.to_string(),
            user_did: "did:human:alice".to_string(),
            inbox_auth_verification_method: "did:human:alice#daemon-key-1".to_string(),
            app_instance_id: "app_1".to_string(),
            bootstrap_id: "boot_legacy".to_string(),
            idempotency_key: legacy_idempotency_key.to_string(),
            daemon_agent_did: daemon.agent_did.clone(),
            runtime_agent_did: runtime_agent_did.to_string(),
            runtime_profile_id: "profile_legacy_personal".to_string(),
            role: "app_message_handler".to_string(),
            desired_agent_json: json!({
                "role": "app_message_handler",
                "runtime_profile": "message_agent"
            }),
            capability_policy_json: json!({"allowed_actions": []}),
            status: "personal_agent_ready".to_string(),
            created_at_ms: 0,
            updated_at_ms: 0,
            revoked_at_ms: None,
        })
        .unwrap();

    handle_agent_payload_message(
        &config,
        &state,
        &registration,
        &outbox,
        IncomingAgentPayloadMessage {
            message_id: "msg_pause_legacy_personal_agent".to_string(),
            conversation_id: Some("conv_personal_agent".to_string()),
            sender_did: "did:human:alice".to_string(),
            target_agent_did: daemon.agent_did.clone(),
            content_type: "application/json".to_string(),
            payload: json!({
                "schema": "awiki.agent.command.v1",
                "command_id": "cmd_pause_legacy_personal_agent",
                "command": "message_agent.binding.disable",
                "target_agent_kind": "runtime",
                "args": {
                    "message_agent_did": runtime_agent_did,
                    "lifecycle_action": "pause"
                }
            }),
        },
    )
    .unwrap();

    let binding = state
        .load_active_or_inactive_app_personal_agent_binding_by_runtime(runtime_agent_did)
        .unwrap()
        .unwrap();
    assert_eq!(binding.binding_id, legacy_binding_id);
    assert_eq!(binding.idempotency_key, legacy_idempotency_key);
    assert_eq!(binding.status, "personal_agent_disabled");
    let disabled = outbox.agent_statuses().last().unwrap().clone();
    assert_eq!(
        disabled.payload["result"]["command"],
        "personal_agent.binding.disable"
    );
    assert_eq!(
        disabled.payload["result"]["personal_agent_did"],
        runtime_agent_did
    );
    assert!(disabled.payload["result"]
        .get("message_agent_did")
        .is_none());
}

#[test]
fn daemon_delete_archives_daemon_family_and_reports_status() {
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
    let created = expect_created(
        handle_agent_payload_message(
            &config,
            &state,
            &registration,
            &outbox,
            IncomingAgentPayloadMessage {
                message_id: "msg_create_for_daemon_delete".to_string(),
                conversation_id: Some("conv_daemon_delete".to_string()),
                sender_did: "did:human:alice".to_string(),
                target_agent_did: daemon.agent_did.clone(),
                content_type: "application/json".to_string(),
                payload: json!({
                    "schema": "awiki.agent.command.v1",
                    "command_id": "cmd_create_for_daemon_delete",
                    "command": "runtime.agent.create",
                    "target_agent_kind": "runtime",
                    "args": {
                        "handle": "@alice-daemon-delete-runtime",
                        "runtime": "hermes",
                        "controller_did": "did:human:alice",
                        "registration_token": "tok_runtime_secret_value",
                        "display_name": "Daemon Delete Runtime"
                    }
                }),
            },
        )
        .unwrap(),
    );

    let delete_outcome = handle_agent_payload_message(
        &config,
        &state,
        &registration,
        &outbox,
        IncomingAgentPayloadMessage {
            message_id: "msg_delete_daemon".to_string(),
            conversation_id: Some("conv_daemon_delete".to_string()),
            sender_did: "did:human:alice".to_string(),
            target_agent_did: daemon.agent_did.clone(),
            content_type: "application/json".to_string(),
            payload: json!({
                "schema": "awiki.agent.command.v1",
                "command_id": "cmd_delete_daemon",
                "command": "daemon.delete",
                "target_agent_kind": "daemon",
                "args": {
                    "daemon_agent_did": daemon.agent_did
                }
            }),
        },
    )
    .unwrap();

    assert_eq!(
        registration.archive_requests(),
        vec![(daemon.agent_did.clone(), daemon.agent_did.clone())]
    );
    assert_eq!(
        state
            .load_agent_definition(&daemon.agent_did)
            .unwrap()
            .status,
        "archived"
    );
    assert_eq!(
        state
            .load_agent_definition(&created.agent_did)
            .unwrap()
            .status,
        "archived"
    );
    let archived = outbox.agent_statuses().last().unwrap().clone();
    assert_eq!(archived.payload["state"], "archived");
    assert_eq!(archived.payload["result"]["command"], "daemon.delete");
    assert_eq!(
        archived.payload["result"]["daemon_agent_did"],
        daemon.agent_did
    );
    assert!(matches!(
        delete_outcome,
        AgentCommandOutcome::StatusReported { .. }
    ));
    assert_eq!(
        awiki_deamon::archive::pending_daemon_archive_finalizer(&config)
            .unwrap()
            .as_deref(),
        archived.payload["result"]["archive_id"].as_str()
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
                    "registration_token": "tok_runtime_secret_value",
                    "display_name": "Hermes Status"
                }
            }),
        },
    )
    .unwrap();
    let created = expect_created(outcome);
    assert_eq!(created.runtime_plugin_id, HERMES_RUNTIME_PLUGIN_ID);
    let route = awiki_deamon::state::HermesSessionRoute::new(
        created.agent_did.clone(),
        created.handle.clone(),
        created.runtime_profile_id.clone(),
        "controller-scope:v1:test-alice-anpclaw-com",
        "controller_private",
        "controller:controller-scope:v1:test-alice-anpclaw-com",
        Some("direct:did:human:alice".to_string()),
        "conversation",
    );
    let session = awiki_deamon::state::HermesNativeSessionRecord::active(
        &route,
        "did:human:alice",
        "awiki_alice_hermes_status",
        "hermes-session-status",
        Some("live-hermes-session-status".to_string()),
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
