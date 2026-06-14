use awiki_deamon::inbox::ControllerTextMessage;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use awiki_deamon::outbox::{
    MemoryRuntimeOutbox, OutboxRecordKind, RuntimeAttachmentSend, RuntimeAttachmentSendResult,
    RuntimeMessageSecurity, RuntimeMessageSend, RuntimeMessageSendResult, RuntimeOutbox,
};
use awiki_deamon::plugins::hermes::{
    reset_hermes_session_by_route, FakeHermesBehavior, FakeHermesGateway, HermesPromptWrapper,
    HermesRuntimePlugin, AWIKI_SKILLS_VERSION, HERMES_RUNTIME_PLUGIN_ID,
};
use awiki_deamon::runtime::host::{
    flush_runtime_final_outbox, run_controller_text_task, run_existing_runtime_task_with_config,
};
use awiki_deamon::runtime::{
    RuntimeAgentProfile, RuntimeInstallStatus, RuntimeLaunchContext, RuntimeLaunchOutcome,
    RuntimePlugin, RuntimeRun, RuntimeRunStatus, RuntimeTask,
};
use awiki_deamon::state::{HermesProfileRecord, HermesSessionRoute};
use awiki_deamon::workspace::WorkspaceMode;
use awiki_deamon::{DaemonConfig, DaemonState};
use rusqlite::Connection;

#[derive(Debug, Clone)]
struct FlakyFinalOutbox {
    inner: MemoryRuntimeOutbox,
    fail_next_message: Arc<AtomicBool>,
}

impl FlakyFinalOutbox {
    fn new() -> Self {
        Self {
            inner: MemoryRuntimeOutbox::default(),
            fail_next_message: Arc::new(AtomicBool::new(true)),
        }
    }

    fn records(&self) -> Vec<awiki_deamon::outbox::OutboxRecord> {
        self.inner.records()
    }
}

impl RuntimeOutbox for FlakyFinalOutbox {
    fn resolve_recipient_did(
        &self,
        context: &awiki_deamon::state::AuthorizedRuntimeContext,
        recipient: &str,
    ) -> anyhow::Result<Option<String>> {
        self.inner.resolve_recipient_did(context, recipient)
    }

    fn send_status(
        &self,
        context: &awiki_deamon::state::AuthorizedRuntimeContext,
        state: &str,
        text: Option<&str>,
    ) -> anyhow::Result<()> {
        self.inner.send_status(context, state, text)
    }

    fn send_status_with_detail(
        &self,
        context: &awiki_deamon::state::AuthorizedRuntimeContext,
        state: &str,
        text: Option<&str>,
        last_error_code: Option<&str>,
        last_error_summary: Option<&str>,
    ) -> anyhow::Result<()> {
        self.inner.send_status_with_detail(
            context,
            state,
            text,
            last_error_code,
            last_error_summary,
        )
    }

    fn send_final(
        &self,
        context: &awiki_deamon::state::AuthorizedRuntimeContext,
        text: Option<&str>,
    ) -> anyhow::Result<()> {
        self.inner.send_final(context, text)
    }

    fn send_message(
        &self,
        context: &awiki_deamon::state::AuthorizedRuntimeContext,
        message: &RuntimeMessageSend,
    ) -> anyhow::Result<RuntimeMessageSendResult> {
        if self.fail_next_message.swap(false, Ordering::SeqCst) {
            anyhow::bail!("temporary message service unavailable");
        }
        self.inner.send_message(context, message)
    }

    fn send_attachment(
        &self,
        context: &awiki_deamon::state::AuthorizedRuntimeContext,
        attachment: &RuntimeAttachmentSend,
    ) -> anyhow::Result<RuntimeAttachmentSendResult> {
        self.inner.send_attachment(context, attachment)
    }
}

fn fixture() -> (tempfile::TempDir, DaemonState) {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    config.ensure_state_layout().unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();
    (root, state)
}

fn profile(workspace_root: std::path::PathBuf) -> RuntimeAgentProfile {
    RuntimeAgentProfile {
        agent_did: "did:agent:hermes".to_string(),
        controller_user_id: "user-alice".to_string(),
        controller_full_handle: "alice.anpclaw.com".to_string(),
        controller_scope_key: "controller-scope:v1:test-alice-anpclaw-com".to_string(),
        controller_did: "did:human:alice".to_string(),
        runtime_profile_id: "profile_hermes_alice".to_string(),
        runtime_plugin_id: HERMES_RUNTIME_PLUGIN_ID.to_string(),
        display_name: Some("Alice Hermes".to_string()),
        workspace_id: Some("workspace_hermes".to_string()),
        workspace_root: Some(workspace_root),
        workspace_mode: Some(WorkspaceMode::SharedRoot),
    }
}

fn hermes_record(home: std::path::PathBuf) -> HermesProfileRecord {
    HermesProfileRecord {
        agent_did: "did:agent:hermes".to_string(),
        runtime_profile_id: "profile_hermes_alice".to_string(),
        hermes_profile: "awiki_alice_hermes".to_string(),
        hermes_home: home,
        hermes_version: None,
        awiki_skills_version: AWIKI_SKILLS_VERSION.to_string(),
        status: "ready".to_string(),
    }
}

#[test]
fn hermes_message_controller_text_runs_status_and_final_callbacks() {
    let (root, state) = fixture();
    let outbox = MemoryRuntimeOutbox::default();
    let gateway = FakeHermesGateway::default();
    let plugin = HermesRuntimePlugin::new(
        gateway.clone(),
        hermes_record(root.path().join("runtime/hermes/profile")),
    );
    let profile = profile(root.path().join("workspace"));

    let result = run_controller_text_task(
        &state,
        &profile,
        &plugin,
        &outbox,
        ControllerTextMessage {
            message_id: "msg_001".to_string(),
            conversation_id: Some("direct:did:human:alice".to_string()),
            sender_did: "did:human:alice".to_string(),
            target_agent_did: "did:agent:hermes".to_string(),
            text: "请处理这条消息".to_string(),
        },
    )
    .unwrap();

    assert_eq!(result.launch_outcome.status, RuntimeRunStatus::Running);
    assert!(result.launch_outcome.callbacks.is_empty());
    assert_eq!(result.run.status, RuntimeRunStatus::Finished);

    let records = outbox.records();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].kind, OutboxRecordKind::Message);
    assert_eq!(records[0].recipient.as_deref(), Some("did:human:alice"));
    assert_eq!(records[0].text.as_deref(), Some("fake complete"));
    assert_eq!(
        records[0].security,
        Some(RuntimeMessageSecurity::DefaultPlain)
    );
    assert_eq!(records[1].kind, OutboxRecordKind::Status);
    assert_eq!(records[1].state.as_deref(), Some("succeeded"));
    assert_eq!(records[1].text.as_deref(), Some("Hermes response sent"));

    let prompts = gateway.submitted_prompts();
    assert_eq!(prompts.len(), 1);
    assert_eq!(prompts[0].run_id, "run_task_msg_001");
    assert!(prompts[0].prompt.contains("controller_verified: true"));
    assert!(prompts[0].prompt.contains("run_id: run_task_msg_001"));
    assert!(prompts[0].prompt.contains("outbound-send"));
    assert!(prompts[0]
        .prompt
        .contains("daemon sends it back to the APP automatically"));
    assert!(prompts[0].prompt.contains("output_language_policy:"));
    assert!(prompts[0]
        .prompt
        .contains("If the language cannot be inferred, use Simplified Chinese"));
    assert!(prompts[0]
        .prompt
        .contains("Do not let the English labels or technical wrapper text"));
    assert!(prompts[0]
        .prompt
        .contains("Do not mention the controller wrapper"));
    assert!(!prompts[0].prompt.contains("finish-message"));
    assert!(!prompts[0].prompt.contains("send-attachment"));
    assert!(prompts[0].prompt.contains("请处理这条消息"));
    assert!(!prompts[0].prompt.contains("auth_private_key"));
    assert!(!prompts[0].prompt.contains("jwt_token"));
    assert!(!prompts[0].prompt.contains("runtime_rpc_token"));
}

#[test]
fn hermes_message_duplicate_controller_text_reuses_existing_run_without_relaunch() {
    let (root, state) = fixture();
    let first_outbox = MemoryRuntimeOutbox::default();
    let gateway = FakeHermesGateway::default();
    let plugin = HermesRuntimePlugin::new(
        gateway.clone(),
        hermes_record(root.path().join("runtime/hermes/profile")),
    );
    let profile = profile(root.path().join("workspace"));
    let message = ControllerTextMessage {
        message_id: "msg_duplicate_inbox".to_string(),
        conversation_id: Some("direct:did:human:alice".to_string()),
        sender_did: "did:human:alice".to_string(),
        target_agent_did: "did:agent:hermes".to_string(),
        text: "重复投递只处理一次".to_string(),
    };

    let first = run_controller_text_task(&state, &profile, &plugin, &first_outbox, message.clone())
        .unwrap();
    assert_eq!(first.run.status, RuntimeRunStatus::Finished);
    assert_eq!(gateway.submitted_prompts().len(), 1);
    assert_eq!(first_outbox.records().len(), 2);

    let second_outbox = MemoryRuntimeOutbox::default();
    let second =
        run_controller_text_task(&state, &profile, &plugin, &second_outbox, message).unwrap();

    assert_eq!(second.run.run_id, "run_task_msg_duplicate_inbox");
    assert_eq!(second.run.status, RuntimeRunStatus::Finished);
    assert_eq!(second.launch_outcome.status, RuntimeRunStatus::Finished);
    assert_eq!(second.launch_outcome.metadata["deduplicated"], true);
    assert_eq!(second.token_id, "");
    assert_eq!(gateway.submitted_prompts().len(), 1);
    assert!(second_outbox.records().is_empty());
}

#[test]
fn hermes_message_failed_status_does_not_send_success_final() {
    let (root, state) = fixture();
    let outbox = MemoryRuntimeOutbox::default();
    let gateway = FakeHermesGateway::with_behavior(FakeHermesBehavior::FailWithStatus);
    let plugin = HermesRuntimePlugin::new(
        gateway,
        hermes_record(root.path().join("runtime/hermes/profile")),
    );
    let profile = profile(root.path().join("workspace"));

    let result = run_controller_text_task(
        &state,
        &profile,
        &plugin,
        &outbox,
        ControllerTextMessage {
            message_id: "msg_failed".to_string(),
            conversation_id: Some("direct:did:human:alice".to_string()),
            sender_did: "did:human:alice".to_string(),
            target_agent_did: "did:agent:hermes".to_string(),
            text: "模拟失败".to_string(),
        },
    )
    .unwrap();

    assert_eq!(result.run.status, RuntimeRunStatus::Failed);
    let records = outbox.records();
    assert_eq!(records.len(), 3);
    assert_eq!(records[0].kind, OutboxRecordKind::Status);
    assert_eq!(records[0].state.as_deref(), Some("failed"));
    assert_eq!(records[1].kind, OutboxRecordKind::Message);
    assert_eq!(records[1].recipient.as_deref(), Some("did:human:alice"));
    assert_eq!(
        records[1].text.as_deref(),
        Some("Hermes 运行失败：fake failure")
    );
    assert_eq!(
        records[1].security,
        Some(RuntimeMessageSecurity::DefaultPlain)
    );
    assert_eq!(records[2].kind, OutboxRecordKind::Status);
    assert_eq!(records[2].state.as_deref(), Some("failed"));
    assert!(!records
        .iter()
        .any(|record| record.kind == OutboxRecordKind::Final));
}

#[test]
fn hermes_message_final_outbox_retries_pending_final_and_finishes_run_once_sent() {
    let (root, state) = fixture();
    let outbox = FlakyFinalOutbox::new();
    let gateway = FakeHermesGateway::default();
    let plugin = HermesRuntimePlugin::new(
        gateway,
        hermes_record(root.path().join("runtime/hermes/profile")),
    );
    let profile = profile(root.path().join("workspace"));

    let result = run_controller_text_task(
        &state,
        &profile,
        &plugin,
        &outbox,
        ControllerTextMessage {
            message_id: "msg_retry_final".to_string(),
            conversation_id: Some("direct:did:human:alice".to_string()),
            sender_did: "did:human:alice".to_string(),
            target_agent_did: "did:agent:hermes".to_string(),
            text: "请处理这条消息".to_string(),
        },
    )
    .unwrap();

    assert_eq!(result.run.status, RuntimeRunStatus::Running);
    let pending = state
        .load_runtime_final_outbox_by_run("run_task_msg_retry_final")
        .unwrap()
        .unwrap();
    assert_eq!(pending.status, "pending");
    assert_eq!(pending.attempt_count, 1);
    assert_eq!(
        pending.conversation_id.as_deref(),
        Some("direct:did:human:alice")
    );
    assert_eq!(
        pending.last_error_code.as_deref(),
        Some("final_delivery_retry")
    );
    assert!(pending
        .idempotency_key
        .contains("runtime-final:did:agent:hermes"));
    assert_eq!(
        state
            .load_runtime_run("run_task_msg_retry_final")
            .unwrap()
            .status,
        RuntimeRunStatus::Running
    );

    rusqlite::Connection::open(root.path().join("daemon.db"))
        .unwrap()
        .execute(
            "UPDATE runtime_final_outbox SET next_attempt_at_ms = 0 WHERE run_id = ?1",
            ["run_task_msg_retry_final"],
        )
        .unwrap();

    let sent = flush_runtime_final_outbox(&state, &outbox, 10).unwrap();
    assert_eq!(sent, 1);
    let stored = state
        .load_runtime_final_outbox_by_run("run_task_msg_retry_final")
        .unwrap()
        .unwrap();
    assert_eq!(stored.status, "sent");
    assert_eq!(stored.attempt_count, 2);
    assert!(stored.sent_at_ms.is_some());
    assert_eq!(
        state
            .load_runtime_run("run_task_msg_retry_final")
            .unwrap()
            .status,
        RuntimeRunStatus::Finished
    );

    let records = outbox.records();
    let final_messages = records
        .iter()
        .filter(|record| {
            record.kind == OutboxRecordKind::Message
                && record.text.as_deref() == Some("fake complete")
        })
        .collect::<Vec<_>>();
    assert_eq!(final_messages.len(), 1);
    assert_eq!(
        final_messages[0].idempotency_key.as_deref(),
        Some(stored.idempotency_key.as_str())
    );
    assert!(records.iter().any(|record| {
        record.kind == OutboxRecordKind::Status
            && record.state.as_deref() == Some("running")
            && record.text.as_deref() == Some("Hermes response is ready; delivery is retrying")
    }));
    assert!(records.iter().any(|record| {
        record.kind == OutboxRecordKind::Status
            && record.state.as_deref() == Some("succeeded")
            && record.text.as_deref() == Some("Hermes response sent")
    }));
}

#[test]
fn hermes_message_empty_final_fails_without_success_outbox() {
    let (root, state) = fixture();
    let outbox = MemoryRuntimeOutbox::default();
    let gateway = FakeHermesGateway::with_behavior(FakeHermesBehavior::CompleteWithoutText);
    let plugin = HermesRuntimePlugin::new(
        gateway,
        hermes_record(root.path().join("runtime/hermes/profile")),
    );
    let profile = profile(root.path().join("workspace"));

    let error = run_controller_text_task(
        &state,
        &profile,
        &plugin,
        &outbox,
        ControllerTextMessage {
            message_id: "msg_empty_final".to_string(),
            conversation_id: Some("direct:did:human:alice".to_string()),
            sender_did: "did:human:alice".to_string(),
            target_agent_did: "did:agent:hermes".to_string(),
            text: "返回空结果".to_string(),
        },
    )
    .unwrap_err();

    assert!(error
        .to_string()
        .contains("Hermes run completed without final text"));
    assert_eq!(
        state
            .load_runtime_run("run_task_msg_empty_final")
            .unwrap()
            .status,
        RuntimeRunStatus::Failed
    );
    assert!(state
        .load_runtime_final_outbox_by_run("run_task_msg_empty_final")
        .unwrap()
        .is_none());

    let records = outbox.records();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].kind, OutboxRecordKind::Message);
    assert_eq!(records[0].recipient.as_deref(), Some("did:human:alice"));
    assert_eq!(
        records[0].text.as_deref(),
        Some("Hermes 运行失败：Hermes run completed without final text")
    );
    assert_eq!(
        records[0].security,
        Some(RuntimeMessageSecurity::DefaultPlain)
    );
    assert_eq!(records[1].kind, OutboxRecordKind::Status);
    assert_eq!(records[1].state.as_deref(), Some("failed"));
    assert_eq!(
        records[1].last_error_code.as_deref(),
        Some("final_text_missing")
    );
    assert_eq!(
        records[1].last_error_summary.as_deref(),
        Some("Hermes run completed without final text")
    );
}

#[test]
fn hermes_message_auto_approved_approval_still_returns_final_text() {
    let (root, state) = fixture();
    let outbox = MemoryRuntimeOutbox::default();
    let gateway = FakeHermesGateway::with_behavior(FakeHermesBehavior::ApprovalRequest);
    let plugin = HermesRuntimePlugin::new(
        gateway.clone(),
        hermes_record(root.path().join("runtime/hermes/profile")),
    );
    let profile = profile(root.path().join("workspace"));

    let result = run_controller_text_task(
        &state,
        &profile,
        &plugin,
        &outbox,
        ControllerTextMessage {
            message_id: "msg_approval".to_string(),
            conversation_id: Some("direct:did:human:alice".to_string()),
            sender_did: "did:human:alice".to_string(),
            target_agent_did: "did:agent:hermes".to_string(),
            text: "触发 approval.request".to_string(),
        },
    )
    .unwrap();

    assert_eq!(result.run.status, RuntimeRunStatus::Finished);
    let records = outbox.records();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].kind, OutboxRecordKind::Message);
    assert_eq!(records[0].recipient.as_deref(), Some("did:human:alice"));
    assert_eq!(
        records[0].text.as_deref(),
        Some("fake complete after approval approved")
    );
    assert_eq!(
        records[0].security,
        Some(RuntimeMessageSecurity::DefaultPlain)
    );
    assert_eq!(records[1].kind, OutboxRecordKind::Status);
    assert_eq!(records[1].state.as_deref(), Some("succeeded"));
    assert_eq!(records[1].text.as_deref(), Some("Hermes response sent"));
    assert!(records[1].last_error_code.is_none());
    assert!(records[1].last_error_summary.is_none());
    assert!(gateway.observed_events().iter().any(|event| {
        event.kind == awiki_deamon::plugins::hermes::HermesRuntimeEventKind::ToolCallObserved
            && event.code.as_deref() == Some("approval_auto_approved")
    }));
}

#[derive(Debug)]
struct MissingGatewayHermesPlugin;

impl RuntimePlugin for MissingGatewayHermesPlugin {
    fn plugin_id(&self) -> &str {
        HERMES_RUNTIME_PLUGIN_ID
    }

    fn check_install_status(&self) -> anyhow::Result<RuntimeInstallStatus> {
        Ok(RuntimeInstallStatus {
            installed: false,
            detail: Some("AWIKI_HERMES_BIN is not set".to_string()),
        })
    }

    fn launch_run(&self, _context: RuntimeLaunchContext) -> anyhow::Result<RuntimeLaunchOutcome> {
        unreachable!("install status should fail before launch")
    }
}

#[test]
fn hermes_message_missing_gateway_uses_documented_error_code() {
    let (root, state) = fixture();
    let outbox = MemoryRuntimeOutbox::default();
    let profile = profile(root.path().join("workspace"));

    let error = run_controller_text_task(
        &state,
        &profile,
        &MissingGatewayHermesPlugin,
        &outbox,
        ControllerTextMessage {
            message_id: "msg_missing_gateway".to_string(),
            conversation_id: Some("direct:did:human:alice".to_string()),
            sender_did: "did:human:alice".to_string(),
            target_agent_did: "did:agent:hermes".to_string(),
            text: "触发 missing gateway".to_string(),
        },
    )
    .unwrap_err();

    assert!(error.to_string().contains("not installed"));
    let run = state
        .load_runtime_run("run_task_msg_missing_gateway")
        .unwrap();
    assert_eq!(run.status, RuntimeRunStatus::Failed);
    let records = outbox.records();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].kind, OutboxRecordKind::Message);
    assert_eq!(
        records[0].text.as_deref(),
        Some("Hermes 运行失败：Hermes gateway command is not configured")
    );
    assert_eq!(records[1].kind, OutboxRecordKind::Status);
    assert_eq!(records[1].state.as_deref(), Some("failed"));
    assert_eq!(
        records[1].last_error_code.as_deref(),
        Some("gateway_command_missing")
    );
    assert_eq!(
        records[1].last_error_summary.as_deref(),
        Some("Hermes gateway command is not configured")
    );
}

#[test]
fn hermes_message_send_message_callback_records_direct_message() {
    let (root, state) = fixture();
    let outbox = MemoryRuntimeOutbox::default();
    let gateway = FakeHermesGateway::with_behavior(FakeHermesBehavior::SendMessage);
    let plugin = HermesRuntimePlugin::new(
        gateway,
        hermes_record(root.path().join("runtime/hermes/profile")),
    );
    let profile = profile(root.path().join("workspace"));

    let result = run_controller_text_task(
        &state,
        &profile,
        &plugin,
        &outbox,
        ControllerTextMessage {
            message_id: "msg_send".to_string(),
            conversation_id: Some("direct:did:human:alice".to_string()),
            sender_did: "did:human:alice".to_string(),
            target_agent_did: "did:agent:hermes".to_string(),
            text: "请给 controller 发消息".to_string(),
        },
    )
    .unwrap();

    assert_eq!(result.launch_outcome.status, RuntimeRunStatus::Running);
    assert_eq!(result.run.status, RuntimeRunStatus::Finished);
    let records = outbox.records();
    assert_eq!(records.len(), 3);
    assert_eq!(records[0].kind, OutboxRecordKind::Message);
    assert_eq!(records[0].recipient.as_deref(), Some("did:human:alice"));
    assert_eq!(records[0].text.as_deref(), Some("Hermes says hello"));
    assert_eq!(
        records[0].security,
        Some(RuntimeMessageSecurity::DefaultPlain)
    );
    assert_eq!(records[1].kind, OutboxRecordKind::Message);
    assert_eq!(records[1].text.as_deref(), Some("fake complete"));
    assert_eq!(
        records[1].security,
        Some(RuntimeMessageSecurity::DefaultPlain)
    );
    assert_eq!(records[2].kind, OutboxRecordKind::Status);
    assert_eq!(records[2].state.as_deref(), Some("succeeded"));
}

#[test]
fn hermes_message_send_message_callback_allows_active_handle_lookup() {
    let (root, state) = fixture();
    let outbox =
        MemoryRuntimeOutbox::default().with_handle_resolution("bob", "did:human:bob-resolved");
    let gateway = FakeHermesGateway::with_behavior(FakeHermesBehavior::SendHandleMessage);
    let plugin = HermesRuntimePlugin::new(
        gateway,
        hermes_record(root.path().join("runtime/hermes/profile")),
    );
    let profile = profile(root.path().join("workspace"));

    let result = run_controller_text_task(
        &state,
        &profile,
        &plugin,
        &outbox,
        ControllerTextMessage {
            message_id: "msg_send_handle".to_string(),
            conversation_id: Some("direct:did:human:alice".to_string()),
            sender_did: "did:human:alice".to_string(),
            target_agent_did: "did:agent:hermes".to_string(),
            text: "请给 bob 发消息".to_string(),
        },
    )
    .unwrap();

    assert_eq!(result.run.status, RuntimeRunStatus::Finished);
    let records = outbox.records();
    assert_eq!(records.len(), 3);
    assert_eq!(records[0].kind, OutboxRecordKind::Message);
    assert_eq!(
        records[0].recipient.as_deref(),
        Some("did:human:bob-resolved")
    );
    assert_eq!(records[0].raw_recipient.as_deref(), Some("bob"));
    assert_eq!(
        records[0].resolved_did.as_deref(),
        Some("did:human:bob-resolved")
    );
    assert_eq!(records[0].text.as_deref(), Some("Hermes says hello Bob"));
    assert_eq!(
        records[0].security,
        Some(RuntimeMessageSecurity::DefaultPlain)
    );
    assert_eq!(records[1].kind, OutboxRecordKind::Message);
    assert_eq!(records[1].recipient.as_deref(), Some("did:human:alice"));
    assert_eq!(records[1].text.as_deref(), Some("fake complete"));
    assert_eq!(records[2].kind, OutboxRecordKind::Status);
    assert_eq!(records[2].state.as_deref(), Some("succeeded"));
}

#[test]
fn hermes_message_run_token_allows_direct_and_group_outbound_targets() {
    let (root, state) = fixture();
    let outbox = MemoryRuntimeOutbox::default();
    let gateway = FakeHermesGateway::with_behavior(FakeHermesBehavior::ObserveOnly);
    let plugin = HermesRuntimePlugin::new(
        gateway,
        hermes_record(root.path().join("runtime/hermes/profile")),
    );
    let profile = profile(root.path().join("workspace"));

    let result = run_controller_text_task(
        &state,
        &profile,
        &plugin,
        &outbox,
        ControllerTextMessage {
            message_id: "msg_outbound_scope".to_string(),
            conversation_id: Some("direct:did:human:alice".to_string()),
            sender_did: "did:human:alice".to_string(),
            target_agent_did: "did:agent:hermes".to_string(),
            text: "帮我给别人或群发消息".to_string(),
        },
    )
    .unwrap();

    let connection = Connection::open(root.path().join("daemon.db")).unwrap();
    let allowed_recipients_json: String = connection
        .query_row(
            "SELECT allowed_recipients_json FROM runtime_rpc_tokens WHERE token_id = ?1",
            [&result.token_id],
            |row| row.get(0),
        )
        .unwrap();

    assert!(allowed_recipients_json.contains("did:human:alice"));
    assert!(allowed_recipients_json.contains("@active_handle_lookup"));
    assert!(allowed_recipients_json.contains("@any_direct"));
    assert!(allowed_recipients_json.contains("@any_group"));
}

#[test]
fn hermes_message_scope_mismatch_task_is_rejected_before_gateway() {
    let (root, state) = fixture();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    config.ensure_state_layout().unwrap();
    let outbox = MemoryRuntimeOutbox::default();
    let gateway = FakeHermesGateway::default();
    let plugin = HermesRuntimePlugin::new(
        gateway.clone(),
        hermes_record(root.path().join("runtime/hermes/profile")),
    );
    let profile = profile(root.path().join("workspace"));
    let task = RuntimeTask {
        task_id: "task_msg_unauthorized".to_string(),
        agent_did: "did:agent:hermes".to_string(),
        controller_user_id: "user-bob".to_string(),
        controller_full_handle: "bob.anpclaw.com".to_string(),
        controller_scope_key: "controller-scope:v1:test-bob-anpclaw-com".to_string(),
        controller_did: "did:human:bob".to_string(),
        sender_did: "did:human:bob".to_string(),
        conversation_id: None,
        text: "越权执行".to_string(),
    };
    task.validate().unwrap();

    let error = run_existing_runtime_task_with_config(
        &config,
        &state,
        &profile,
        &plugin,
        &outbox,
        task,
        "run_task_msg_unauthorized",
    )
    .unwrap_err();

    assert!(error.to_string().contains("controller scope"));
    assert!(gateway.submitted_prompts().is_empty());
    assert!(outbox.records().is_empty());
}

#[test]
fn hermes_message_prompt_wrapper_debug_redacts_user_message() {
    let hermes = hermes_record(std::env::temp_dir().join("runtime/hermes/profile"));
    let run = RuntimeRun {
        run_id: "run_task_msg_debug".to_string(),
        task_id: "task_msg_debug".to_string(),
        agent_did: "did:agent:hermes".to_string(),
        runtime_profile_id: "profile_hermes_alice".to_string(),
        runtime_plugin_id: HERMES_RUNTIME_PLUGIN_ID.to_string(),
        workspace_id: None,
        status: RuntimeRunStatus::Pending,
    };
    let task = RuntimeTask {
        task_id: "task_msg_debug".to_string(),
        agent_did: "did:agent:hermes".to_string(),
        controller_user_id: "user-alice".to_string(),
        controller_full_handle: "alice.anpclaw.com".to_string(),
        controller_scope_key: "controller-scope:v1:test-alice-anpclaw-com".to_string(),
        controller_did: "did:human:alice".to_string(),
        sender_did: "did:human:alice".to_string(),
        conversation_id: None,
        text: "secret rtok_debug_secret_value_123456789 jwt_token auth_private_key".to_string(),
    };
    let wrapper = HermesPromptWrapper::new(&hermes, &run, &task);
    let debug = format!("{wrapper:?}");

    assert!(debug.contains("<redacted>"));
    assert!(!debug.contains("rtok_debug_secret_value_123456789"));
    assert!(!debug.contains("jwt_token"));
    assert!(!debug.contains("auth_private_key"));
}

#[test]
fn hermes_message_prompt_disables_interactive_requests() {
    let (root, state) = fixture();
    let outbox = MemoryRuntimeOutbox::default();
    let gateway = FakeHermesGateway::with_behavior(FakeHermesBehavior::ObserveOnly);
    let plugin = HermesRuntimePlugin::new(
        gateway.clone(),
        hermes_record(root.path().join("runtime/hermes/profile")),
    );
    let profile = profile(root.path().join("workspace"));

    run_controller_text_task(
        &state,
        &profile,
        &plugin,
        &outbox,
        ControllerTextMessage {
            message_id: "msg_no_clarify_request".to_string(),
            conversation_id: Some("direct:did:human:alice".to_string()),
            sender_did: "did:human:alice".to_string(),
            target_agent_did: "did:agent:hermes".to_string(),
            text: "信息不够时也不要走 Hermes interactive request".to_string(),
        },
    )
    .unwrap();

    let prompts = gateway.submitted_prompts();
    assert_eq!(prompts.len(), 1);
    assert!(prompts[0]
        .prompt
        .contains("Do not use Hermes interactive requests"));
    assert!(prompts[0].prompt.contains("clarify.request"));
    assert!(prompts[0]
        .prompt
        .contains("ask for it in your ordinary final answer"));
}

#[test]
fn hermes_message_prompt_constrains_outbound_send_to_daemon_runtime_wrapper() {
    let (root, state) = fixture();
    let outbox = MemoryRuntimeOutbox::default();
    let gateway = FakeHermesGateway::with_behavior(FakeHermesBehavior::ObserveOnly);
    let plugin = HermesRuntimePlugin::new(
        gateway.clone(),
        hermes_record(root.path().join("runtime/hermes/profile")),
    );
    let profile = profile(root.path().join("workspace"));

    run_controller_text_task(
        &state,
        &profile,
        &plugin,
        &outbox,
        ControllerTextMessage {
            message_id: "msg_outbound_wrapper_rules".to_string(),
            conversation_id: Some("direct:did:human:alice".to_string()),
            sender_did: "did:human:alice".to_string(),
            target_agent_did: "did:agent:hermes".to_string(),
            text: "帮我给 did:human:bob 发消息".to_string(),
        },
    )
    .unwrap();

    let prompts = gateway.submitted_prompts();
    assert_eq!(prompts.len(), 1);
    assert!(prompts[0].prompt.contains("awiki-deamon-runtime send"));
    assert!(prompts[0].prompt.contains("Do not call `awiki-cli`"));
    assert!(prompts[0].prompt.contains("do not switch local identities"));
    assert!(prompts[0]
        .prompt
        .contains("Never add, infer, or override a sender identity"));
    assert!(prompts[0]
        .prompt
        .contains("Do not retry with another local identity"));
}

#[test]
fn hermes_session_mapping_reuses_session_for_same_conversation_after_restart() {
    let (root, state) = fixture();
    let outbox = MemoryRuntimeOutbox::default();
    let gateway = FakeHermesGateway::with_behavior(FakeHermesBehavior::ObserveOnly);
    let hermes = hermes_record(root.path().join("runtime/hermes/profile"));
    let plugin = HermesRuntimePlugin::with_state(gateway.clone(), hermes.clone(), state.clone());
    let profile = profile(root.path().join("workspace"));

    for message_id in ["msg_session_1", "msg_session_2"] {
        run_controller_text_task(
            &state,
            &profile,
            &plugin,
            &outbox,
            ControllerTextMessage {
                message_id: message_id.to_string(),
                conversation_id: Some("direct:did:human:alice".to_string()),
                sender_did: "did:human:alice".to_string(),
                target_agent_did: "did:agent:hermes".to_string(),
                text: "继续同一个 Hermes session".to_string(),
            },
        )
        .unwrap();
    }

    assert_eq!(gateway.created_sessions().len(), 1);
    assert_eq!(gateway.submitted_prompts().len(), 2);

    run_controller_text_task(
        &state,
        &profile,
        &plugin,
        &outbox,
        ControllerTextMessage {
            message_id: "msg_session_other_conversation".to_string(),
            conversation_id: Some("direct:did:human:bob".to_string()),
            sender_did: "did:human:alice".to_string(),
            target_agent_did: "did:agent:hermes".to_string(),
            text: "另一个 conversation 应创建独立 session".to_string(),
        },
    )
    .unwrap();
    assert_eq!(gateway.created_sessions().len(), 2);

    let route = HermesSessionRoute::new(
        "did:agent:hermes",
        "profile_hermes_alice",
        "controller-scope:v1:test-alice-anpclaw-com",
        Some("direct:did:human:alice".to_string()),
        "conversation",
    );
    let active = state
        .load_active_hermes_session_by_route(&route)
        .unwrap()
        .unwrap();
    assert_eq!(
        active.hermes_session_id,
        "fake-session-hermes:did:agent:hermes:controller-scope:v1:test-alice-anpclaw-com:direct:did:human:alice:conversation"
    );

    let reopened_state =
        DaemonState::open(&DaemonConfig::for_state_root(root.path()).unwrap()).unwrap();
    let restarted_gateway = FakeHermesGateway::with_behavior(FakeHermesBehavior::ObserveOnly);
    let restarted_plugin =
        HermesRuntimePlugin::with_state(restarted_gateway.clone(), hermes, reopened_state.clone());
    run_controller_text_task(
        &reopened_state,
        &profile,
        &restarted_plugin,
        &outbox,
        ControllerTextMessage {
            message_id: "msg_session_3".to_string(),
            conversation_id: Some("direct:did:human:alice".to_string()),
            sender_did: "did:human:alice".to_string(),
            target_agent_did: "did:agent:hermes".to_string(),
            text: "daemon restart 后继续同一个 session".to_string(),
        },
    )
    .unwrap();
    assert!(restarted_gateway.created_sessions().is_empty());
    assert_eq!(restarted_gateway.submitted_prompts().len(), 1);
}

#[test]
fn hermes_group_conversation_uses_group_session_and_final_message_target() {
    let (root, state) = fixture();
    let outbox = MemoryRuntimeOutbox::default();
    let gateway = FakeHermesGateway::default();
    let hermes = hermes_record(root.path().join("runtime/hermes/profile"));
    let plugin = HermesRuntimePlugin::with_state(gateway.clone(), hermes, state.clone());
    let profile = profile(root.path().join("workspace"));

    run_controller_text_task(
        &state,
        &profile,
        &plugin,
        &outbox,
        ControllerTextMessage {
            message_id: "did:example:group:9".to_string(),
            conversation_id: Some("group:did:example:group".to_string()),
            sender_did: "did:human:bob".to_string(),
            target_agent_did: "did:agent:hermes".to_string(),
            text: "群里请 Hermes 处理一下".to_string(),
        },
    )
    .unwrap();

    let created_sessions = gateway.created_sessions();
    assert_eq!(created_sessions.len(), 1);
    assert_eq!(
        created_sessions[0].conversation_id.as_deref(),
        Some("group:did:example:group")
    );

    let route = HermesSessionRoute::new(
        "did:agent:hermes",
        "profile_hermes_alice",
        "controller-scope:v1:test-alice-anpclaw-com",
        Some("group:did:example:group".to_string()),
        "conversation",
    );
    assert!(state
        .load_active_hermes_session_by_route(&route)
        .unwrap()
        .is_some());

    let stored = state
        .load_runtime_final_outbox_by_run("run_task_did:example:group:9")
        .unwrap()
        .unwrap();
    assert_eq!(stored.status, "sent");
    assert_eq!(
        stored.conversation_id.as_deref(),
        Some("group:did:example:group")
    );
    assert_eq!(stored.controller_did, "did:human:alice");

    let records = outbox.records();
    let final_message = records
        .iter()
        .find(|record| {
            record.kind == OutboxRecordKind::Message
                && record.text.as_deref() == Some("fake complete")
        })
        .expect("final message should be sent");
    assert_eq!(
        final_message.recipient.as_deref(),
        Some("did:example:group")
    );
    assert_eq!(
        final_message.raw_recipient.as_deref(),
        Some("did:example:group")
    );
    assert_eq!(final_message.resolved_did, None);
    assert_eq!(
        final_message.idempotency_key.as_deref(),
        Some(stored.idempotency_key.as_str())
    );
}

#[test]
fn hermes_session_mapping_reset_archives_old_session_and_creates_replacement() {
    let (root, state) = fixture();
    let outbox = MemoryRuntimeOutbox::default();
    let gateway = FakeHermesGateway::with_behavior(FakeHermesBehavior::ObserveOnly);
    let hermes = hermes_record(root.path().join("runtime/hermes/profile"));
    let plugin = HermesRuntimePlugin::with_state(gateway.clone(), hermes, state.clone());
    let profile = profile(root.path().join("workspace"));
    let route = HermesSessionRoute::new(
        "did:agent:hermes",
        "profile_hermes_alice",
        "controller-scope:v1:test-alice-anpclaw-com",
        Some("direct:did:human:alice".to_string()),
        "conversation",
    );

    run_controller_text_task(
        &state,
        &profile,
        &plugin,
        &outbox,
        ControllerTextMessage {
            message_id: "msg_session_reset_1".to_string(),
            conversation_id: Some("direct:did:human:alice".to_string()),
            sender_did: "did:human:alice".to_string(),
            target_agent_did: "did:agent:hermes".to_string(),
            text: "创建 session".to_string(),
        },
    )
    .unwrap();
    assert_eq!(gateway.created_sessions().len(), 1);

    assert_eq!(reset_hermes_session_by_route(&state, &route).unwrap(), 1);
    assert!(state
        .load_active_hermes_session_by_route(&route)
        .unwrap()
        .is_none());

    run_controller_text_task(
        &state,
        &profile,
        &plugin,
        &outbox,
        ControllerTextMessage {
            message_id: "msg_session_reset_2".to_string(),
            conversation_id: Some("direct:did:human:alice".to_string()),
            sender_did: "did:human:alice".to_string(),
            target_agent_did: "did:agent:hermes".to_string(),
            text: "reset 后创建新 session".to_string(),
        },
    )
    .unwrap();
    assert_eq!(gateway.created_sessions().len(), 2);
    assert!(state
        .load_active_hermes_session_by_route(&route)
        .unwrap()
        .is_some());
}

#[test]
fn hermes_session_missing_recreates_session_and_retries_prompt_once() {
    let (root, state) = fixture();
    let outbox = MemoryRuntimeOutbox::default();
    let gateway = FakeHermesGateway::with_behavior(FakeHermesBehavior::FailOnceWithMissingSession);
    let hermes = hermes_record(root.path().join("runtime/hermes/profile"));
    let plugin = HermesRuntimePlugin::with_state(gateway.clone(), hermes, state.clone());
    let profile = profile(root.path().join("workspace"));
    let route = HermesSessionRoute::new(
        "did:agent:hermes",
        "profile_hermes_alice",
        "controller-scope:v1:test-alice-anpclaw-com",
        Some("direct:did:human:alice".to_string()),
        "conversation",
    );

    run_controller_text_task(
        &state,
        &profile,
        &plugin,
        &outbox,
        ControllerTextMessage {
            message_id: "msg_session_missing".to_string(),
            conversation_id: Some("direct:did:human:alice".to_string()),
            sender_did: "did:human:alice".to_string(),
            target_agent_did: "did:agent:hermes".to_string(),
            text: "旧 Hermes session 失效后自动恢复".to_string(),
        },
    )
    .unwrap();

    assert_eq!(gateway.created_sessions().len(), 2);
    assert_eq!(gateway.submitted_prompts().len(), 2);
    let active = state
        .load_active_hermes_session_by_route(&route)
        .unwrap()
        .unwrap();
    assert!(active.hermes_session_id.ends_with("-2"));
    let records = outbox.records();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].kind, OutboxRecordKind::Message);
    assert_eq!(records[0].text.as_deref(), Some("fake complete"));
    assert_eq!(records[1].kind, OutboxRecordKind::Status);
    assert_eq!(records[1].state.as_deref(), Some("succeeded"));
}
