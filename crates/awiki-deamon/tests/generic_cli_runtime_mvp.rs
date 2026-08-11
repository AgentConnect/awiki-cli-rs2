use awiki_deamon::controller_scope::VerifiedControllerSender;
use awiki_deamon::inbox::{route_controller_text_task_with_verified_sender, ControllerTextMessage};
use awiki_deamon::outbox::{
    MemoryRuntimeOutbox, OutboxRecordKind, RuntimeAttachmentSend, RuntimeAttachmentSendResult,
    RuntimeMessageSend, RuntimeMessageSendResult, RuntimeOutbox,
};
use awiki_deamon::plugins::generic_cli::{
    claude_code::{
        build_prompt_envelope as build_claude_code_prompt_envelope,
        claude_code_final_text_from_stream_json, claude_code_native_session_id_from_stream_json,
        ClaudeCodeDriver, ClaudeCodeDriverConfig, ClaudeCodeSessionMode,
    },
    codex::{
        build_prompt_envelope as build_codex_prompt_envelope,
        codex_native_session_id_from_stdout_jsonl, CodexDriver, CodexDriverConfig, CodexResumeMode,
    },
    sanitize_cli_output_bytes, validate_native_session_id, validate_native_session_source,
    CommandGenericCliDriver, GenericCliDriver, GenericCliDriverRegistry, GenericCliExit,
    GenericCliInvocation, GenericCliInvocationContext, GenericCliRuntimePlugin,
    TestGenericCliDriver,
};
use awiki_deamon::runtime::host::{
    run_controller_text_task, run_controller_text_task_with_config,
    run_controller_text_task_with_verified_sender_config, run_existing_runtime_task_with_config,
};
use awiki_deamon::runtime::{
    RuntimeAgentProfile, RuntimeConversationScope, RuntimeInstallStatus,
    RuntimeInvocationAuthority, RuntimeLaunchContext, RuntimeLaunchOutcome, RuntimePlugin,
    RuntimeRunStatus, RuntimeTask, RuntimeTaskTriggerKind,
};
use awiki_deamon::state::{
    AuthorizedRuntimeContext, CliRuntimeProfileRecord, CreateCliRouteSession,
};
use awiki_deamon::workspace::WorkspaceMode;
use awiki_deamon::{DaemonConfig, DaemonState};
use rusqlite::Connection;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

fn fixture() -> (tempfile::TempDir, DaemonState) {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    config.ensure_state_layout().unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();
    upsert_test_cli_profile(&state, "profile_generic_cli_1", "codex");
    (root, state)
}

fn upsert_test_cli_profile(
    state: &DaemonState,
    runtime_profile_id: &str,
    driver_id: &str,
) -> CliRuntimeProfileRecord {
    let cli_profile = CliRuntimeProfileRecord::for_driver(runtime_profile_id, driver_id).unwrap();
    state.upsert_cli_runtime_profile(&cli_profile).unwrap();
    cli_profile
}

fn assert_busy_retry_metadata(record: &awiki_deamon::outbox::OutboxRecord, reason: &str) {
    let metadata = record
        .metadata
        .as_ref()
        .expect("busy status should include metadata");
    assert_eq!(metadata["retryable"].as_bool(), Some(true));
    assert_eq!(metadata["deferred"].as_bool(), Some(true));
    assert_eq!(metadata["next_action"].as_str(), Some("retry_later"));
    assert_eq!(metadata["retry_after_ms"].as_i64(), Some(10_000));
    assert_eq!(metadata["retry_after_seconds"].as_i64(), Some(10));
    assert_eq!(metadata["busy_reason"].as_str(), Some(reason));
}

#[derive(Clone)]
struct FailingStatusOutbox {
    inner: MemoryRuntimeOutbox,
}

impl FailingStatusOutbox {
    fn new(inner: MemoryRuntimeOutbox) -> Self {
        Self { inner }
    }

    fn records(&self) -> Vec<awiki_deamon::outbox::OutboxRecord> {
        self.inner.records()
    }
}

impl RuntimeOutbox for FailingStatusOutbox {
    fn resolve_recipient_did(
        &self,
        context: &AuthorizedRuntimeContext,
        recipient: &str,
    ) -> anyhow::Result<Option<String>> {
        self.inner.resolve_recipient_did(context, recipient)
    }

    fn send_status(
        &self,
        context: &AuthorizedRuntimeContext,
        state: &str,
        text: Option<&str>,
    ) -> anyhow::Result<()> {
        self.send_status_with_detail(context, state, text, None, None)
    }

    fn send_status_with_detail(
        &self,
        context: &AuthorizedRuntimeContext,
        state: &str,
        text: Option<&str>,
        last_error_code: Option<&str>,
        last_error_summary: Option<&str>,
    ) -> anyhow::Result<()> {
        if matches!(state, "running" | "succeeded") {
            anyhow::bail!("synthetic status delivery failure: {state}");
        }
        self.inner.send_status_with_detail(
            context,
            state,
            text,
            last_error_code,
            last_error_summary,
        )
    }

    fn send_status_with_metadata(
        &self,
        context: &AuthorizedRuntimeContext,
        state: &str,
        text: Option<&str>,
        last_error_code: Option<&str>,
        last_error_summary: Option<&str>,
        metadata: Option<&serde_json::Value>,
    ) -> anyhow::Result<()> {
        if matches!(state, "running" | "succeeded") {
            anyhow::bail!("synthetic status delivery failure: {state}");
        }
        self.inner.send_status_with_metadata(
            context,
            state,
            text,
            last_error_code,
            last_error_summary,
            metadata,
        )
    }

    fn send_final(
        &self,
        context: &AuthorizedRuntimeContext,
        text: Option<&str>,
    ) -> anyhow::Result<()> {
        self.inner.send_final(context, text)
    }

    fn send_message(
        &self,
        context: &AuthorizedRuntimeContext,
        message: &RuntimeMessageSend,
    ) -> anyhow::Result<RuntimeMessageSendResult> {
        self.inner.send_message(context, message)
    }

    fn send_attachment(
        &self,
        context: &AuthorizedRuntimeContext,
        attachment: &RuntimeAttachmentSend,
    ) -> anyhow::Result<RuntimeAttachmentSendResult> {
        self.inner.send_attachment(context, attachment)
    }
}

#[test]
fn generic_cli_output_sanitizer_redacts_controls_non_utf8_and_truncates() {
    let dirty_output = b"alpha rtok_secret \x1b[31mred\x1b[0m \xff beta";
    let sanitized = sanitize_cli_output_bytes(dirty_output, "rtok_secret", 24);

    assert_eq!(sanitized.text, "alpha <redacted-runtime-");
    assert_eq!(sanitized.raw_bytes, dirty_output.len());
    assert_eq!(sanitized.text_bytes, 24);
    assert!(sanitized.output_sanitized);
    assert!(sanitized.output_truncated);
    assert!(sanitized.non_utf8_replaced);
    assert!(sanitized.token_redacted);
    assert_eq!(
        sanitized.metadata_json()["sanitizer_version"].as_str(),
        Some("generic-cli-output-sanitizer-v1")
    );

    let split_token =
        sanitize_cli_output_bytes(b"token rtok_\x1b[31msecret done", "rtok_secret", 256);
    assert_eq!(split_token.text, "token <redacted-runtime-rpc-token> done");
    assert!(split_token.output_sanitized);
    assert!(split_token.token_redacted);
}

fn assert_busy_route_message_queue_item(
    state: &DaemonState,
    runtime_profile_id: &str,
    route_key: &str,
    source_message_id: &str,
    run_id: &str,
    reason: &str,
    forbidden_prompt_text: &str,
) {
    let queue = state
        .list_cli_route_message_queue_for_route(runtime_profile_id, route_key)
        .unwrap();
    assert_eq!(queue.len(), 1);
    let item = &queue[0];
    let expected_task_id = format!("task_{source_message_id}");
    assert_eq!(item.status, "queued");
    assert_eq!(item.source_message_id, source_message_id);
    assert_eq!(item.task_id.as_deref(), Some(expected_task_id.as_str()));
    assert_eq!(item.run_id.as_deref(), Some(run_id));
    assert_eq!(item.enqueue_reason, reason);
    assert_eq!(item.last_error_code.as_deref(), Some(reason));
    assert!(
        item.next_attempt_at_ms
            > awiki_deamon::security::runtime_token::current_time_millis().unwrap()
    );
    assert!(!format!("{item:?}").contains(forbidden_prompt_text));
}

fn profile(workspace_root: std::path::PathBuf) -> RuntimeAgentProfile {
    RuntimeAgentProfile {
        agent_did: "did:agent:alice-coder".to_string(),
        agent_handle: "alice-coder".to_string(),
        controller_user_id: "user-alice".to_string(),
        controller_full_handle: "alice.anpclaw.com".to_string(),
        controller_scope_key: "controller-scope:v1:test-alice-anpclaw-com".to_string(),
        controller_did: "did:human:alice".to_string(),
        runtime_profile_id: "profile_generic_cli_1".to_string(),
        runtime_plugin_id: "generic-cli".to_string(),
        display_name: Some("Alice Coder".to_string()),
        preferred_language: "zh-Hans".to_string(),
        workspace_id: Some("workspace_awiki".to_string()),
        workspace_root: Some(workspace_root),
        workspace_mode: Some(WorkspaceMode::SharedRoot),
    }
}

fn runtime_task_for_invocation(
    trigger_kind: RuntimeTaskTriggerKind,
    conversation_scope: RuntimeConversationScope,
    invocation_authority: RuntimeInvocationAuthority,
    sender_did: &str,
    requester_user_id: Option<&str>,
    requester_full_handle: Option<&str>,
    conversation_id: Option<&str>,
) -> RuntimeTask {
    let requester_did = sender_did.to_string();
    let reply_recipient_did = if trigger_kind == RuntimeTaskTriggerKind::DelegatedDirect {
        "did:human:alice".to_string()
    } else {
        requester_did.clone()
    };
    let task = RuntimeTask {
        task_id: "task_prompt_context".to_string(),
        agent_did: "did:agent:alice-coder".to_string(),
        agent_handle: "alice-coder".to_string(),
        controller_user_id: "user-alice".to_string(),
        controller_full_handle: "alice.anpclaw.com".to_string(),
        controller_scope_key: "controller-scope:v1:test-alice-anpclaw-com".to_string(),
        controller_did: "did:human:alice".to_string(),
        sender_did: sender_did.to_string(),
        requester_did,
        requester_user_id: requester_user_id.map(str::to_string),
        requester_full_handle: requester_full_handle.map(str::to_string),
        trigger_kind,
        conversation_scope,
        invocation_authority,
        reply_recipient_did,
        conversation_id: conversation_id.map(str::to_string),
        text: "检查当前上下文".to_string(),
    };
    task.validate().unwrap();
    task
}

fn generic_cli_invocation_from_task(
    task: RuntimeTask,
) -> awiki_deamon::plugins::generic_cli::GenericCliInvocation {
    generic_cli_invocation_from_task_with_language(task, "zh-Hans")
}

fn generic_cli_invocation_from_task_with_language(
    task: RuntimeTask,
    preferred_language: &str,
) -> awiki_deamon::plugins::generic_cli::GenericCliInvocation {
    awiki_deamon::plugins::generic_cli::GenericCliInvocation {
        run_id: "run_prompt_context".to_string(),
        task_id: task.task_id.clone(),
        message_id: "prompt_context".to_string(),
        conversation_id: task.conversation_id.clone(),
        preferred_language: preferred_language.to_string(),
        context: GenericCliInvocationContext::from_task(&task),
        task_text: task.text,
        agent_did: task.agent_did,
        runtime_profile_id: "profile_generic_cli_1".to_string(),
        workspace_root: None,
        workspace_instance: None,
        route_session: None,
        runtime_temp_dir: None,
        runtime_rpc_token: "rtok_prompt_context_secret".to_string(),
        local_socket_path: None,
        callbacks: Vec::new(),
    }
}

fn controller_private_route_key(profile: &RuntimeAgentProfile) -> String {
    awiki_deamon::state::cli_route_session_key(
        &profile.agent_did,
        &profile.controller_scope_key,
        &format!("direct:controller:{}", profile.controller_scope_key),
    )
    .unwrap()
}

struct EnvVarGuard {
    saved: Vec<(&'static str, Option<std::ffi::OsString>)>,
}

impl EnvVarGuard {
    fn set(vars: &[(&'static str, &'static str)]) -> Self {
        let saved = vars
            .iter()
            .map(|(key, _)| (*key, std::env::var_os(key)))
            .collect::<Vec<_>>();
        for (key, value) in vars {
            std::env::set_var(key, value);
        }
        Self { saved }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        for (key, value) in self.saved.iter().rev() {
            if let Some(value) = value {
                std::env::set_var(key, value);
            } else {
                std::env::remove_var(key);
            }
        }
    }
}

#[test]
fn controller_text_task_runs_generic_cli_and_records_callbacks() {
    let (root, state) = fixture();
    let outbox = MemoryRuntimeOutbox::default();
    let plugin = GenericCliRuntimePlugin::new(TestGenericCliDriver::default());
    let profile = profile(root.path().join("workspace"));
    let cli_profile =
        CliRuntimeProfileRecord::for_driver(&profile.runtime_profile_id, "codex").unwrap();
    state.upsert_cli_runtime_profile(&cli_profile).unwrap();

    let result = run_controller_text_task(
        &state,
        &profile,
        &plugin,
        &outbox,
        ControllerTextMessage {
            message_id: "msg_001".to_string(),
            conversation_id: Some("conv_001".to_string()),
            sender_did: "did:human:alice".to_string(),
            requester_user_id: None,
            requester_full_handle: None,
            trigger_kind: awiki_deamon::runtime::RuntimeTaskTriggerKind::ControllerDirect,
            invocation_authority: awiki_deamon::runtime::RuntimeInvocationAuthority::Controller,
            target_agent_did: "did:agent:alice-coder".to_string(),
            text: "分析这个仓库".to_string(),
        },
    )
    .unwrap();

    assert_eq!(result.run.status, RuntimeRunStatus::Finished);
    assert_eq!(result.launch_outcome.status, RuntimeRunStatus::Finished);
    assert_eq!(result.launch_outcome.callbacks.len(), 2);

    let records = outbox.records();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].kind, OutboxRecordKind::Status);
    assert_eq!(records[0].state.as_deref(), Some("running"));
    assert_eq!(records[1].kind, OutboxRecordKind::Final);
    assert_eq!(records[1].state.as_deref(), Some("finished"));
    assert!(records
        .iter()
        .all(|record| record.run_id == "run_task_msg_001"));

    let connection = Connection::open(root.path().join("daemon.db")).unwrap();
    let run_status: String = connection
        .query_row(
            "SELECT status FROM runtime_run WHERE run_id = 'run_task_msg_001'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(run_status, "finished");

    let task_count: i64 = connection
        .query_row("SELECT COUNT(*) FROM runtime_task", [], |row| row.get(0))
        .unwrap();
    assert_eq!(task_count, 1);

    let audit_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM audit_log WHERE run_id = 'run_task_msg_001'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(audit_count, 2);
}

#[derive(Debug, Clone)]
struct ExecutionBoundaryObservingDriver {
    state: DaemonState,
    outbox: MemoryRuntimeOutbox,
    observed_running_before_dispatch: Arc<AtomicBool>,
}

impl GenericCliDriver for ExecutionBoundaryObservingDriver {
    fn check_install_status(&self) -> anyhow::Result<RuntimeInstallStatus> {
        Ok(RuntimeInstallStatus {
            installed: true,
            detail: Some("execution boundary observing driver".to_string()),
        })
    }

    fn run(&self, invocation: GenericCliInvocation) -> anyhow::Result<GenericCliExit> {
        let persisted = self.state.load_runtime_run(&invocation.run_id)?;
        let running_was_published = self.outbox.records().iter().any(|record| {
            record.run_id == invocation.run_id
                && record.kind == OutboxRecordKind::Status
                && record.state.as_deref() == Some("running")
        });
        self.observed_running_before_dispatch.store(
            persisted.status == RuntimeRunStatus::Running && running_was_published,
            Ordering::SeqCst,
        );
        Ok(GenericCliExit {
            exit_code: 0,
            status: RuntimeRunStatus::Finished,
            callbacks: invocation.callbacks,
            metadata: json!({}),
        })
    }
}

#[test]
fn generic_cli_persists_and_publishes_running_before_blocking_driver_dispatch() {
    let (root, state) = fixture();
    let outbox = MemoryRuntimeOutbox::default();
    let observed_running_before_dispatch = Arc::new(AtomicBool::new(false));
    let plugin = GenericCliRuntimePlugin::new(ExecutionBoundaryObservingDriver {
        state: state.clone(),
        outbox: outbox.clone(),
        observed_running_before_dispatch: Arc::clone(&observed_running_before_dispatch),
    });
    let profile = profile(root.path().join("workspace"));

    let result = run_controller_text_task(
        &state,
        &profile,
        &plugin,
        &outbox,
        ControllerTextMessage {
            message_id: "msg_running_before_dispatch".to_string(),
            conversation_id: Some("conv_running_before_dispatch".to_string()),
            sender_did: "did:human:alice".to_string(),
            requester_user_id: None,
            requester_full_handle: None,
            trigger_kind: RuntimeTaskTriggerKind::ControllerDirect,
            invocation_authority: RuntimeInvocationAuthority::Controller,
            target_agent_did: profile.agent_did.clone(),
            text: "observe lifecycle boundary".to_string(),
        },
    )
    .unwrap();

    assert!(observed_running_before_dispatch.load(Ordering::SeqCst));
    assert_eq!(result.run.status, RuntimeRunStatus::Finished);
    let records = outbox.records();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].kind, OutboxRecordKind::Status);
    assert_eq!(records[0].state.as_deref(), Some("running"));
    assert_eq!(records[1].kind, OutboxRecordKind::Final);
    assert_eq!(records[1].state.as_deref(), Some("finished"));
}

#[test]
fn failed_generic_cli_marks_run_failed_without_final_callback() {
    let (root, state) = fixture();
    let outbox = MemoryRuntimeOutbox::default();
    let plugin = GenericCliRuntimePlugin::new(TestGenericCliDriver { exit_code: 7 });
    let profile = profile(root.path().join("workspace"));
    let cli_profile =
        CliRuntimeProfileRecord::for_driver(&profile.runtime_profile_id, "codex").unwrap();
    state.upsert_cli_runtime_profile(&cli_profile).unwrap();

    let result = run_controller_text_task(
        &state,
        &profile,
        &plugin,
        &outbox,
        ControllerTextMessage {
            message_id: "msg_failed".to_string(),
            conversation_id: Some("conv_001".to_string()),
            sender_did: "did:human:alice".to_string(),
            requester_user_id: None,
            requester_full_handle: None,
            trigger_kind: awiki_deamon::runtime::RuntimeTaskTriggerKind::ControllerDirect,
            invocation_authority: awiki_deamon::runtime::RuntimeInvocationAuthority::Controller,
            target_agent_did: "did:agent:alice-coder".to_string(),
            text: "执行一个会失败的任务".to_string(),
        },
    )
    .unwrap();

    assert_eq!(result.run.status, RuntimeRunStatus::Failed);
    assert_eq!(result.launch_outcome.status, RuntimeRunStatus::Failed);
    assert_eq!(result.launch_outcome.exit_code, Some(7));

    let records = outbox.records();
    assert_eq!(records.len(), 2);
    let failed_status = records
        .iter()
        .find(|record| {
            record.kind == OutboxRecordKind::Status && record.state.as_deref() == Some("failed")
        })
        .expect("failed status record");
    assert_eq!(
        failed_status.last_error_code.as_deref(),
        Some("generic_cli_failed")
    );
    assert_eq!(
        failed_status.last_error_summary.as_deref(),
        Some("generic CLI test driver exited with status 7")
    );
    assert_eq!(
        failed_status.metadata.as_ref().unwrap()["next_action"].as_str(),
        Some("manual_review_required")
    );
    assert_eq!(
        failed_status.metadata.as_ref().unwrap()["failed_message_recovery"].as_str(),
        Some("unsupported")
    );
    let run_record = state.load_cli_driver_run(&result.run.run_id).unwrap();
    assert_eq!(run_record.status, "failed");
    assert_eq!(run_record.output_json, json!({}));
}

#[test]
fn route_root_controller_private_uses_stable_route_across_controller_did_rotation() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    config.ensure_state_layout().unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();
    let outbox = MemoryRuntimeOutbox::default();
    let plugin = GenericCliRuntimePlugin::new(TestGenericCliDriver::default());
    let mut profile = profile(
        config
            .state_root
            .join("runtime")
            .join("workspaces")
            .join("profile_generic_cli_1"),
    );
    profile.workspace_mode = Some(WorkspaceMode::RouteRoot);
    let cli_profile =
        CliRuntimeProfileRecord::for_driver(&profile.runtime_profile_id, "codex").unwrap();
    state.upsert_cli_runtime_profile(&cli_profile).unwrap();

    let first = run_controller_text_task_with_config(
        &config,
        &state,
        &profile,
        &plugin,
        &outbox,
        ControllerTextMessage {
            message_id: "msg_route_bob".to_string(),
            conversation_id: Some("dm:did:human:bob".to_string()),
            sender_did: "did:human:alice".to_string(),
            requester_user_id: None,
            requester_full_handle: None,
            trigger_kind: awiki_deamon::runtime::RuntimeTaskTriggerKind::ControllerDirect,
            invocation_authority: awiki_deamon::runtime::RuntimeInvocationAuthority::Controller,
            target_agent_did: profile.agent_did.clone(),
            text: "hello bob".to_string(),
        },
    )
    .unwrap();
    let rotated_sender = VerifiedControllerSender {
        controller_user_id: profile.controller_user_id.clone(),
        controller_full_handle: profile.controller_full_handle.clone(),
        controller_scope_key: profile.controller_scope_key.clone(),
        controller_did: "did:human:alice:rotated".to_string(),
        sender_did: "did:human:alice:rotated".to_string(),
    };
    let second = run_controller_text_task_with_verified_sender_config(
        &config,
        &state,
        &profile,
        &rotated_sender,
        &plugin,
        &outbox,
        ControllerTextMessage {
            message_id: "msg_route_carol".to_string(),
            conversation_id: Some("direct:did:human:carol".to_string()),
            sender_did: "did:human:alice:rotated".to_string(),
            requester_user_id: None,
            requester_full_handle: None,
            trigger_kind: awiki_deamon::runtime::RuntimeTaskTriggerKind::ControllerDirect,
            invocation_authority: awiki_deamon::runtime::RuntimeInvocationAuthority::Controller,
            target_agent_did: profile.agent_did.clone(),
            text: "hello carol".to_string(),
        },
    )
    .unwrap();

    assert_eq!(first.run.status, RuntimeRunStatus::Finished);
    assert_eq!(second.run.status, RuntimeRunStatus::Finished);
    let first_run = state.load_cli_driver_run(&first.run.run_id).unwrap();
    let second_run = state.load_cli_driver_run(&second.run.run_id).unwrap();
    assert_eq!(first_run.workspace_mode, Some(WorkspaceMode::RouteRoot));
    assert_eq!(second_run.workspace_mode, Some(WorkspaceMode::RouteRoot));
    assert_eq!(first_run.route_key, second_run.route_key);
    assert_eq!(
        first_run.workspace_instance_path,
        second_run.workspace_instance_path
    );
    let first_route = state
        .load_cli_route_session(&first_run.route_key)
        .unwrap()
        .unwrap();
    let second_route = state
        .load_cli_route_session(&second_run.route_key)
        .unwrap()
        .unwrap();
    assert_eq!(
        first_route.route_key_hash,
        state.cli_route_key_hash(&first_route.route_key).unwrap()
    );
    assert_ne!(
        first_route.route_key_hash,
        awiki_deamon::state::cli_route_key_hash(&first_route.route_key).unwrap()
    );
    assert_eq!(
        first_route.conversation_id,
        "direct:controller:controller-scope:v1:test-alice-anpclaw-com"
    );
    assert_eq!(second_route.conversation_id, first_route.conversation_id);
    assert!(first_route.workspace_path.exists());
    assert!(first_route.session_dir.exists());
    assert_eq!(first_route.status, "active");
    assert_eq!(second_route.status, "active");
    assert!(first_route
        .workspace_path
        .starts_with(profile.workspace_root.as_ref().unwrap()));
    assert!(first_route
        .workspace_path
        .file_name()
        .unwrap()
        .to_string_lossy()
        .starts_with("route_"));
    assert_eq!(
        first_route.workspace_path.file_name().unwrap(),
        first_route.route_key_hash.as_str()
    );
    assert_eq!(
        state
            .count_cli_route_sessions_for_runtime_profile(
                &profile.runtime_profile_id,
                &profile.controller_scope_key,
                Some("active"),
            )
            .unwrap(),
        1
    );
}

#[test]
fn route_root_external_direct_uses_requester_scope_route() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    config.ensure_state_layout().unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();
    let outbox = MemoryRuntimeOutbox::default();
    let plugin = GenericCliRuntimePlugin::new(TestGenericCliDriver::default());
    let mut profile = profile(
        config
            .state_root
            .join("runtime")
            .join("workspaces")
            .join("profile_generic_cli_1"),
    );
    profile.workspace_mode = Some(WorkspaceMode::RouteRoot);
    let cli_profile =
        CliRuntimeProfileRecord::for_driver(&profile.runtime_profile_id, "codex").unwrap();
    state.upsert_cli_runtime_profile(&cli_profile).unwrap();

    let bob_first = runtime_task_for_invocation(
        RuntimeTaskTriggerKind::ExternalDirect,
        RuntimeConversationScope::direct("user-bob", "bob.anpclaw.com").unwrap(),
        RuntimeInvocationAuthority::Requester,
        "did:human:bob:first",
        Some("user-bob"),
        Some("bob.anpclaw.com"),
        Some("direct:did:human:bob:first"),
    );
    let mut bob_second = runtime_task_for_invocation(
        RuntimeTaskTriggerKind::ExternalDirect,
        RuntimeConversationScope::direct("user-bob", "bob.anpclaw.com").unwrap(),
        RuntimeInvocationAuthority::Requester,
        "did:human:bob:second",
        Some("user-bob"),
        Some("bob.anpclaw.com"),
        Some("direct:did:human:bob:second"),
    );
    bob_second.task_id = "task_prompt_context_bob_second".to_string();
    let mut carol = runtime_task_for_invocation(
        RuntimeTaskTriggerKind::ExternalDirect,
        RuntimeConversationScope::direct("user-carol", "carol.anpclaw.com").unwrap(),
        RuntimeInvocationAuthority::Requester,
        "did:human:carol",
        Some("user-carol"),
        Some("carol.anpclaw.com"),
        Some("direct:did:human:carol"),
    );
    carol.task_id = "task_prompt_context_carol".to_string();

    let first = run_existing_runtime_task_with_config(
        &config,
        &state,
        &profile,
        &plugin,
        &outbox,
        bob_first,
        "run_bob_first",
    )
    .unwrap();
    let second = run_existing_runtime_task_with_config(
        &config,
        &state,
        &profile,
        &plugin,
        &outbox,
        bob_second,
        "run_bob_second",
    )
    .unwrap();
    let third = run_existing_runtime_task_with_config(
        &config,
        &state,
        &profile,
        &plugin,
        &outbox,
        carol,
        "run_carol",
    )
    .unwrap();

    assert_eq!(first.run.status, RuntimeRunStatus::Finished);
    assert_eq!(second.run.status, RuntimeRunStatus::Finished);
    assert_eq!(third.run.status, RuntimeRunStatus::Finished);
    let first_run = state.load_cli_driver_run(&first.run.run_id).unwrap();
    let second_run = state.load_cli_driver_run(&second.run.run_id).unwrap();
    let third_run = state.load_cli_driver_run(&third.run.run_id).unwrap();
    assert_eq!(first_run.route_key, second_run.route_key);
    assert_ne!(first_run.route_key, third_run.route_key);
    let first_route = state
        .load_cli_route_session(&first_run.route_key)
        .unwrap()
        .unwrap();
    let third_route = state
        .load_cli_route_session(&third_run.route_key)
        .unwrap()
        .unwrap();
    assert_eq!(
        first_route.conversation_id,
        "direct:user:user-bob:handle:bob.anpclaw.com"
    );
    assert_eq!(
        third_route.conversation_id,
        "direct:user:user-carol:handle:carol.anpclaw.com"
    );
}

#[test]
fn route_root_requires_conversation_id_and_does_not_create_long_term_session() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    config.ensure_state_layout().unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();
    let outbox = MemoryRuntimeOutbox::default();
    let plugin = GenericCliRuntimePlugin::new(TestGenericCliDriver::default());
    let mut profile = profile(
        config
            .state_root
            .join("runtime")
            .join("workspaces")
            .join("profile_generic_cli_1"),
    );
    profile.workspace_mode = Some(WorkspaceMode::RouteRoot);
    let cli_profile =
        CliRuntimeProfileRecord::for_driver(&profile.runtime_profile_id, "codex").unwrap();
    state.upsert_cli_runtime_profile(&cli_profile).unwrap();

    let error = run_controller_text_task_with_config(
        &config,
        &state,
        &profile,
        &plugin,
        &outbox,
        ControllerTextMessage {
            message_id: "msg_route_missing_conversation".to_string(),
            conversation_id: None,
            sender_did: "did:human:alice".to_string(),
            requester_user_id: None,
            requester_full_handle: None,
            trigger_kind: awiki_deamon::runtime::RuntimeTaskTriggerKind::ControllerDirect,
            invocation_authority: awiki_deamon::runtime::RuntimeInvocationAuthority::Controller,
            target_agent_did: profile.agent_did.clone(),
            text: "missing conversation".to_string(),
        },
    )
    .unwrap_err();
    let error_text = format!("{error:#}");
    assert!(error_text.contains("requires conversation_id"));
    let run = state
        .load_runtime_run("run_task_msg_route_missing_conversation")
        .unwrap();
    assert_eq!(run.status, RuntimeRunStatus::Failed);
    assert_eq!(
        state
            .count_cli_route_sessions_for_runtime_profile(
                &profile.runtime_profile_id,
                &profile.controller_scope_key,
                None,
            )
            .unwrap(),
        0
    );
    let records = outbox.records();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].kind, OutboxRecordKind::Status);
    assert_eq!(records[0].state.as_deref(), Some("failed"));
    assert_eq!(
        records[0].last_error_code.as_deref(),
        Some("route_session_preparation_failed")
    );
    assert_eq!(
        records[0].metadata.as_ref().unwrap()["next_action"].as_str(),
        Some("manual_review_required")
    );
    assert_eq!(
        records[0].metadata.as_ref().unwrap()["failed_message_recovery"].as_str(),
        Some("unsupported")
    );
}

#[test]
fn route_root_profile_busy_releases_route_lease_without_launching_driver() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    config.ensure_state_layout().unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();
    let outbox = MemoryRuntimeOutbox::default();
    let plugin = GenericCliRuntimePlugin::new(TestGenericCliDriver::default());
    let mut profile = profile(
        config
            .state_root
            .join("runtime")
            .join("workspaces")
            .join("profile_generic_cli_1"),
    );
    profile.workspace_mode = Some(WorkspaceMode::RouteRoot);
    let cli_profile =
        CliRuntimeProfileRecord::for_driver(&profile.runtime_profile_id, "codex").unwrap();
    state.upsert_cli_runtime_profile(&cli_profile).unwrap();
    assert!(state
        .try_acquire_cli_runtime_profile_lock(
            &profile.runtime_profile_id,
            "codex",
            "run_existing",
            "test",
            awiki_deamon::security::runtime_token::current_time_millis().unwrap() + 60_000,
        )
        .unwrap());

    let error = run_controller_text_task_with_config(
        &config,
        &state,
        &profile,
        &plugin,
        &outbox,
        ControllerTextMessage {
            message_id: "msg_profile_busy".to_string(),
            conversation_id: Some("direct:did:human:bob".to_string()),
            sender_did: "did:human:alice".to_string(),
            requester_user_id: None,
            requester_full_handle: None,
            trigger_kind: awiki_deamon::runtime::RuntimeTaskTriggerKind::ControllerDirect,
            invocation_authority: awiki_deamon::runtime::RuntimeInvocationAuthority::Controller,
            target_agent_did: profile.agent_did.clone(),
            text: "profile busy".to_string(),
        },
    )
    .unwrap_err();
    let error_text = format!("{error:#}");
    assert!(error_text.contains("runtime profile is busy"));
    let run = state.load_runtime_run("run_task_msg_profile_busy").unwrap();
    assert_eq!(run.status, RuntimeRunStatus::Failed);
    let route_key = controller_private_route_key(&profile);
    let route = state.load_cli_route_session(&route_key).unwrap().unwrap();
    assert_eq!(route.status, "queued");
    assert_eq!(route.lock_run_id, None);
    assert_eq!(route.last_message_id, None);
    assert_eq!(route.last_error_code.as_deref(), Some("profile_busy"));
    assert!(state
        .load_cli_driver_run("run_task_msg_profile_busy")
        .unwrap_err()
        .to_string()
        .contains("load cli driver run"));
    assert_eq!(
        state
            .count_cli_runtime_locks(
                Some("profile"),
                Some(&profile.runtime_profile_id),
                Some("codex"),
                false,
            )
            .unwrap(),
        1
    );
    let records = outbox.records();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].kind, OutboxRecordKind::Status);
    assert_eq!(records[0].state.as_deref(), Some("queued"));
    assert_eq!(records[0].last_error_code.as_deref(), Some("profile_busy"));
    assert_busy_retry_metadata(&records[0], "profile_busy");
    let retries = state
        .list_runtime_retry_requests_for_original_run("run_task_msg_profile_busy")
        .unwrap();
    assert_eq!(retries.len(), 1);
    assert_eq!(retries[0].original_run_id, "run_task_msg_profile_busy");
    assert_eq!(retries[0].task_id, "task_msg_profile_busy");
    assert_eq!(
        retries[0].requested_by_command_id,
        "runtime.busy.auto-deferred"
    );
    assert!(
        retries[0].next_attempt_at_ms
            > awiki_deamon::security::runtime_token::current_time_millis().unwrap()
    );
    assert!(!format!("{:?}", retries[0]).contains("profile busy"));
    assert_busy_route_message_queue_item(
        &state,
        &profile.runtime_profile_id,
        &route_key,
        "msg_profile_busy",
        "run_task_msg_profile_busy",
        "profile_busy",
        "profile busy",
    );
}

#[test]
fn route_root_route_busy_does_not_launch_driver_or_release_existing_lease() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    config.ensure_state_layout().unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();
    let outbox = MemoryRuntimeOutbox::default();
    let plugin = GenericCliRuntimePlugin::new(TestGenericCliDriver::default());
    let mut profile = profile(
        config
            .state_root
            .join("runtime")
            .join("workspaces")
            .join("profile_generic_cli_1"),
    );
    profile.workspace_mode = Some(WorkspaceMode::RouteRoot);
    let cli_profile =
        CliRuntimeProfileRecord::for_driver(&profile.runtime_profile_id, "codex").unwrap();
    state.upsert_cli_runtime_profile(&cli_profile).unwrap();

    let conversation_id = "direct:did:human:bob";
    let route_key = controller_private_route_key(&profile);
    let route_hash = state.cli_route_key_hash(&route_key).unwrap();
    let workspace_root = profile.workspace_root.as_ref().unwrap();
    let session_root = workspace_root
        .parent()
        .and_then(|runtime_workspaces_root| runtime_workspaces_root.parent())
        .unwrap()
        .join("sessions")
        .join(&profile.runtime_profile_id);
    let paths =
        awiki_deamon::workspace::route_workspace_paths(workspace_root, &session_root, &route_hash)
            .unwrap();
    let route = state
        .get_or_create_cli_route_session(awiki_deamon::state::CreateCliRouteSession {
            agent_did: profile.agent_did.clone(),
            runtime_profile_id: profile.runtime_profile_id.clone(),
            driver_id: "codex".to_string(),
            controller_user_id: profile.controller_user_id.clone(),
            controller_full_handle: profile.controller_full_handle.clone(),
            controller_scope_key: profile.controller_scope_key.clone(),
            controller_did: profile.controller_did.clone(),
            conversation_id: format!("direct:controller:{}", profile.controller_scope_key),
            workspace_path: paths.workspace_path,
            session_dir: paths.session_dir,
        })
        .unwrap();
    assert!(state
        .try_acquire_cli_route_session_lease(
            &route.route_key,
            "run_existing",
            "test",
            awiki_deamon::security::runtime_token::current_time_millis().unwrap() + 60_000,
        )
        .unwrap());

    let error = run_controller_text_task_with_config(
        &config,
        &state,
        &profile,
        &plugin,
        &outbox,
        ControllerTextMessage {
            message_id: "msg_route_busy".to_string(),
            conversation_id: Some(conversation_id.to_string()),
            sender_did: "did:human:alice".to_string(),
            requester_user_id: None,
            requester_full_handle: None,
            trigger_kind: awiki_deamon::runtime::RuntimeTaskTriggerKind::ControllerDirect,
            invocation_authority: awiki_deamon::runtime::RuntimeInvocationAuthority::Controller,
            target_agent_did: profile.agent_did.clone(),
            text: "route busy".to_string(),
        },
    )
    .unwrap_err();
    let error_text = format!("{error:#}");
    assert!(error_text.contains("route session is busy"));
    let run = state.load_runtime_run("run_task_msg_route_busy").unwrap();
    assert_eq!(run.status, RuntimeRunStatus::Failed);
    let route = state.load_cli_route_session(&route_key).unwrap().unwrap();
    assert_eq!(route.status, "running");
    assert_eq!(route.lock_run_id.as_deref(), Some("run_existing"));
    assert_eq!(route.last_error_code, None);
    assert!(state
        .load_cli_driver_run("run_task_msg_route_busy")
        .unwrap_err()
        .to_string()
        .contains("load cli driver run"));
    let records = outbox.records();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].kind, OutboxRecordKind::Status);
    assert_eq!(records[0].state.as_deref(), Some("queued"));
    assert_eq!(records[0].last_error_code.as_deref(), Some("route_busy"));
    assert_busy_retry_metadata(&records[0], "route_busy");
    let retries = state
        .list_runtime_retry_requests_for_original_run("run_task_msg_route_busy")
        .unwrap();
    assert_eq!(retries.len(), 1);
    assert_eq!(retries[0].original_run_id, "run_task_msg_route_busy");
    assert_eq!(retries[0].task_id, "task_msg_route_busy");
    assert_eq!(
        retries[0].requested_by_command_id,
        "runtime.busy.auto-deferred"
    );
    assert!(!format!("{:?}", retries[0]).contains("route busy"));
    assert_busy_route_message_queue_item(
        &state,
        &profile.runtime_profile_id,
        &route_key,
        "msg_route_busy",
        "run_task_msg_route_busy",
        "route_busy",
        "route busy",
    );
}

#[test]
fn route_root_claude_host_home_busy_releases_profile_and_route_lease() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    config.ensure_state_layout().unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();
    let outbox = MemoryRuntimeOutbox::default();
    let plugin = GenericCliRuntimePlugin::new(TestGenericCliDriver::default());
    let mut profile = profile(
        config
            .state_root
            .join("runtime")
            .join("workspaces")
            .join("profile_generic_cli_1"),
    );
    profile.workspace_mode = Some(WorkspaceMode::RouteRoot);
    let cli_profile =
        CliRuntimeProfileRecord::for_driver(&profile.runtime_profile_id, "claude-code").unwrap();
    state.upsert_cli_runtime_profile(&cli_profile).unwrap();
    assert!(state
        .try_acquire_cli_host_home_lock(
            "claude-code",
            "run_existing",
            "test",
            awiki_deamon::security::runtime_token::current_time_millis().unwrap() + 60_000,
        )
        .unwrap());

    let error = run_controller_text_task_with_config(
        &config,
        &state,
        &profile,
        &plugin,
        &outbox,
        ControllerTextMessage {
            message_id: "msg_host_home_busy".to_string(),
            conversation_id: Some("direct:did:human:bob".to_string()),
            sender_did: "did:human:alice".to_string(),
            requester_user_id: None,
            requester_full_handle: None,
            trigger_kind: awiki_deamon::runtime::RuntimeTaskTriggerKind::ControllerDirect,
            invocation_authority: awiki_deamon::runtime::RuntimeInvocationAuthority::Controller,
            target_agent_did: profile.agent_did.clone(),
            text: "host home busy".to_string(),
        },
    )
    .unwrap_err();
    let error_text = format!("{error:#}");
    assert!(error_text.contains("host home is busy"));
    let run = state
        .load_runtime_run("run_task_msg_host_home_busy")
        .unwrap();
    assert_eq!(run.status, RuntimeRunStatus::Failed);
    let route_key = controller_private_route_key(&profile);
    let route = state.load_cli_route_session(&route_key).unwrap().unwrap();
    assert_eq!(route.status, "queued");
    assert_eq!(route.lock_run_id, None);
    assert_eq!(route.last_message_id, None);
    assert_eq!(route.last_error_code.as_deref(), Some("host_home_busy"));
    assert!(state
        .load_cli_driver_run("run_task_msg_host_home_busy")
        .unwrap_err()
        .to_string()
        .contains("load cli driver run"));
    assert_eq!(
        state
            .count_cli_runtime_locks(
                Some("profile"),
                Some(&profile.runtime_profile_id),
                Some("claude-code"),
                false,
            )
            .unwrap(),
        0
    );
    assert_eq!(
        state
            .count_cli_runtime_locks(Some("host-home"), None, Some("claude-code"), false)
            .unwrap(),
        1
    );
    let records = outbox.records();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].kind, OutboxRecordKind::Status);
    assert_eq!(records[0].state.as_deref(), Some("queued"));
    assert_eq!(
        records[0].last_error_code.as_deref(),
        Some("host_home_busy")
    );
    assert_busy_retry_metadata(&records[0], "host_home_busy");
    let retries = state
        .list_runtime_retry_requests_for_original_run("run_task_msg_host_home_busy")
        .unwrap();
    assert_eq!(retries.len(), 1);
    assert_eq!(retries[0].original_run_id, "run_task_msg_host_home_busy");
    assert_eq!(retries[0].task_id, "task_msg_host_home_busy");
    assert_eq!(
        retries[0].requested_by_command_id,
        "runtime.busy.auto-deferred"
    );
    assert!(!format!("{:?}", retries[0]).contains("host home busy"));
    assert_busy_route_message_queue_item(
        &state,
        &profile.runtime_profile_id,
        &route_key,
        "msg_host_home_busy",
        "run_task_msg_host_home_busy",
        "host_home_busy",
        "host home busy",
    );
}

#[test]
fn route_root_session_dir_failure_marks_run_and_route_failed_before_launch() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    config.ensure_state_layout().unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();
    let outbox = MemoryRuntimeOutbox::default();
    let plugin = GenericCliRuntimePlugin::new(TestGenericCliDriver::default());
    let mut profile = profile(
        config
            .state_root
            .join("runtime")
            .join("workspaces")
            .join("profile_generic_cli_1"),
    );
    profile.workspace_mode = Some(WorkspaceMode::RouteRoot);
    let cli_profile =
        CliRuntimeProfileRecord::for_driver(&profile.runtime_profile_id, "codex").unwrap();
    state.upsert_cli_runtime_profile(&cli_profile).unwrap();
    std::fs::create_dir_all(config.state_root.join("runtime")).unwrap();
    std::fs::write(
        config.state_root.join("runtime").join("sessions"),
        "not a dir",
    )
    .unwrap();

    let error = run_controller_text_task_with_config(
        &config,
        &state,
        &profile,
        &plugin,
        &outbox,
        ControllerTextMessage {
            message_id: "msg_route_bad_session_dir".to_string(),
            conversation_id: Some("direct:did:human:bob".to_string()),
            sender_did: "did:human:alice".to_string(),
            requester_user_id: None,
            requester_full_handle: None,
            trigger_kind: awiki_deamon::runtime::RuntimeTaskTriggerKind::ControllerDirect,
            invocation_authority: awiki_deamon::runtime::RuntimeInvocationAuthority::Controller,
            target_agent_did: profile.agent_did.clone(),
            text: "bad session dir".to_string(),
        },
    )
    .unwrap_err();
    let error_text = format!("{error:#}");
    assert!(error_text.contains("create generic-cli route session dir"));
    let run = state
        .load_runtime_run("run_task_msg_route_bad_session_dir")
        .unwrap();
    assert_eq!(run.status, RuntimeRunStatus::Failed);
    let run_record_missing = state
        .load_cli_driver_run("run_task_msg_route_bad_session_dir")
        .unwrap_err();
    assert!(run_record_missing
        .to_string()
        .contains("load cli driver run"));
    let route_key = controller_private_route_key(&profile);
    let route = state.load_cli_route_session(&route_key).unwrap().unwrap();
    assert_eq!(route.status, "failed");
    assert_eq!(route.last_run_id.as_deref(), Some(run.run_id.as_str()));
    assert_eq!(
        route.last_error_code.as_deref(),
        Some("route_session_dir_create_failed")
    );
    assert_eq!(outbox.records().len(), 1);
    assert_eq!(
        outbox.records()[0].last_error_code.as_deref(),
        Some("route_session_preparation_failed")
    );
    assert_eq!(
        outbox.records()[0].metadata.as_ref().unwrap()["next_action"].as_str(),
        Some("manual_review_required")
    );
    assert_eq!(
        outbox.records()[0].metadata.as_ref().unwrap()["failed_message_recovery"].as_str(),
        Some("unsupported")
    );
}

#[derive(Debug, Clone)]
struct InstallMissingPlugin;

impl RuntimePlugin for InstallMissingPlugin {
    fn plugin_id(&self) -> &str {
        "generic-cli"
    }

    fn check_install_status(&self) -> anyhow::Result<RuntimeInstallStatus> {
        Ok(RuntimeInstallStatus {
            installed: false,
            detail: Some("codex binary missing at /tmp/secret/path".to_string()),
        })
    }

    fn launch_run(&self, _context: RuntimeLaunchContext) -> anyhow::Result<RuntimeLaunchOutcome> {
        unreachable!("install missing plugin should not launch")
    }
}

#[test]
fn generic_cli_missing_install_emits_failed_setup_required_without_route_session() {
    let (root, state) = fixture();
    let outbox = MemoryRuntimeOutbox::default();
    let plugin = InstallMissingPlugin;
    let mut profile = profile(root.path().join("workspace"));
    profile.workspace_mode = Some(WorkspaceMode::RouteRoot);

    let error = run_controller_text_task(
        &state,
        &profile,
        &plugin,
        &outbox,
        ControllerTextMessage {
            message_id: "msg_install_missing".to_string(),
            conversation_id: Some("direct:did:human:bob".to_string()),
            sender_did: "did:human:alice".to_string(),
            requester_user_id: None,
            requester_full_handle: None,
            trigger_kind: awiki_deamon::runtime::RuntimeTaskTriggerKind::ControllerDirect,
            invocation_authority: awiki_deamon::runtime::RuntimeInvocationAuthority::Controller,
            target_agent_did: profile.agent_did.clone(),
            text: "install missing".to_string(),
        },
    )
    .unwrap_err();
    assert!(error.to_string().contains("not installed"));

    let run = state
        .load_runtime_run("run_task_msg_install_missing")
        .unwrap();
    assert_eq!(run.status, RuntimeRunStatus::Failed);
    assert_eq!(
        state
            .count_cli_route_sessions_for_runtime_profile(
                &profile.runtime_profile_id,
                &profile.controller_scope_key,
                None,
            )
            .unwrap(),
        0
    );
    assert!(state
        .load_cli_driver_run("run_task_msg_install_missing")
        .unwrap_err()
        .to_string()
        .contains("load cli driver run"));

    let records = outbox.records();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].kind, OutboxRecordKind::Status);
    assert_eq!(records[0].state.as_deref(), Some("failed"));
    assert_eq!(
        records[0].last_error_code.as_deref(),
        Some("runtime_not_installed")
    );
    assert_eq!(
        records[0].metadata.as_ref().unwrap()["next_action"].as_str(),
        Some("setup_required")
    );
    assert_eq!(
        records[0].metadata.as_ref().unwrap()["failed_message_recovery"].as_str(),
        Some("unsupported")
    );
    assert!(!records
        .iter()
        .any(|record| record.kind == OutboxRecordKind::Final));
}

#[derive(Debug, Clone)]
struct LaunchErrorPlugin;

impl RuntimePlugin for LaunchErrorPlugin {
    fn plugin_id(&self) -> &str {
        "generic-cli"
    }

    fn check_install_status(&self) -> anyhow::Result<RuntimeInstallStatus> {
        Ok(RuntimeInstallStatus {
            installed: true,
            detail: Some("launch error test plugin".to_string()),
        })
    }

    fn launch_run(&self, _context: RuntimeLaunchContext) -> anyhow::Result<RuntimeLaunchOutcome> {
        anyhow::bail!("synthetic launch failure")
    }
}

#[test]
fn generic_cli_launch_error_emits_failed_status_and_does_not_advance_message_waterline() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    config.ensure_state_layout().unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();
    let outbox = MemoryRuntimeOutbox::default();
    let plugin = LaunchErrorPlugin;
    let mut profile = profile(
        config
            .state_root
            .join("runtime")
            .join("workspaces")
            .join("profile_generic_cli_1"),
    );
    profile.workspace_mode = Some(WorkspaceMode::RouteRoot);
    let cli_profile =
        CliRuntimeProfileRecord::for_driver(&profile.runtime_profile_id, "codex").unwrap();
    state.upsert_cli_runtime_profile(&cli_profile).unwrap();

    let error = run_controller_text_task_with_config(
        &config,
        &state,
        &profile,
        &plugin,
        &outbox,
        ControllerTextMessage {
            message_id: "msg_launch_error".to_string(),
            conversation_id: Some("direct:did:human:bob".to_string()),
            sender_did: "did:human:alice".to_string(),
            requester_user_id: None,
            requester_full_handle: None,
            trigger_kind: awiki_deamon::runtime::RuntimeTaskTriggerKind::ControllerDirect,
            invocation_authority: awiki_deamon::runtime::RuntimeInvocationAuthority::Controller,
            target_agent_did: profile.agent_did.clone(),
            text: "launch error".to_string(),
        },
    )
    .unwrap_err();
    let error_text = format!("{error:#}");
    assert!(error_text.contains("synthetic launch failure"));

    let run = state.load_runtime_run("run_task_msg_launch_error").unwrap();
    assert_eq!(run.status, RuntimeRunStatus::Failed);
    let route_key = controller_private_route_key(&profile);
    let route = state.load_cli_route_session(&route_key).unwrap().unwrap();
    assert_eq!(route.status, "failed");
    assert_eq!(route.lock_run_id, None);
    assert_eq!(route.last_message_id, None);
    assert_eq!(route.last_error_code.as_deref(), Some("launch_failed"));
    assert!(state
        .load_cli_driver_run("run_task_msg_launch_error")
        .unwrap_err()
        .to_string()
        .contains("load cli driver run"));

    let records = outbox.records();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].kind, OutboxRecordKind::Status);
    assert_eq!(records[0].state.as_deref(), Some("running"));
    assert_eq!(records[1].kind, OutboxRecordKind::Status);
    assert_eq!(records[1].state.as_deref(), Some("failed"));
    assert_eq!(records[1].last_error_code.as_deref(), Some("launch_failed"));
    assert_eq!(
        records[1].metadata.as_ref().unwrap()["next_action"].as_str(),
        Some("manual_review_required")
    );
    assert_eq!(
        records[1].metadata.as_ref().unwrap()["failed_message_recovery"].as_str(),
        Some("unsupported")
    );
    assert!(!records
        .iter()
        .any(|record| record.kind == OutboxRecordKind::Final));
}

#[derive(Debug, Clone)]
struct MsgSendCallbackPlugin;

impl RuntimePlugin for MsgSendCallbackPlugin {
    fn plugin_id(&self) -> &str {
        "generic-cli"
    }

    fn check_install_status(&self) -> anyhow::Result<RuntimeInstallStatus> {
        Ok(RuntimeInstallStatus {
            installed: true,
            detail: Some("msg.send callback test plugin".to_string()),
        })
    }

    fn launch_run(&self, context: RuntimeLaunchContext) -> anyhow::Result<RuntimeLaunchOutcome> {
        let token = context.runtime_rpc_token.as_str().to_string();
        Ok(RuntimeLaunchOutcome {
            run_id: context.run.run_id,
            status: RuntimeRunStatus::Running,
            exit_code: Some(0),
            callbacks: vec![
                awiki_deamon::cli_wrapper::CliWrapperRequest::msg_send(
                    token.clone(),
                    "did:human:bob",
                    "hello from generic-cli",
                )
                .into_rpc_request(),
                awiki_deamon::cli_wrapper::CliWrapperRequest::task_finish(
                    token,
                    context.task.task_id,
                    "done",
                )
                .into_rpc_request(),
            ],
            metadata: json!({"driver_id": "codex"}),
        })
    }
}

#[derive(Debug, Clone)]
struct AttachmentSendCallbackPlugin;

impl RuntimePlugin for AttachmentSendCallbackPlugin {
    fn plugin_id(&self) -> &str {
        "generic-cli"
    }

    fn check_install_status(&self) -> anyhow::Result<RuntimeInstallStatus> {
        Ok(RuntimeInstallStatus {
            installed: true,
            detail: Some("attachment.send callback test plugin".to_string()),
        })
    }

    fn launch_run(&self, context: RuntimeLaunchContext) -> anyhow::Result<RuntimeLaunchOutcome> {
        let workspace = context
            .workspace_root
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("workspace_root is required"))?;
        std::fs::create_dir_all(workspace)?;
        let report_path = workspace.join("report.txt");
        std::fs::write(&report_path, "small report")?;
        let token = context.runtime_rpc_token.as_str().to_string();
        Ok(RuntimeLaunchOutcome {
            run_id: context.run.run_id,
            status: RuntimeRunStatus::Running,
            exit_code: Some(0),
            callbacks: vec![
                awiki_deamon::cli_wrapper::CliWrapperRequest::attachment_send(
                    token.clone(),
                    report_path.to_string_lossy().to_string(),
                    Some("report.txt"),
                    Some("report ready"),
                )
                .into_rpc_request(),
                awiki_deamon::cli_wrapper::CliWrapperRequest::task_finish(
                    token,
                    context.task.task_id,
                    "done",
                )
                .into_rpc_request(),
            ],
            metadata: json!({"driver_id": "codex"}),
        })
    }
}

#[derive(Debug, Clone)]
struct DuplicateFinishCallbackPlugin;

impl RuntimePlugin for DuplicateFinishCallbackPlugin {
    fn plugin_id(&self) -> &str {
        "generic-cli"
    }

    fn check_install_status(&self) -> anyhow::Result<RuntimeInstallStatus> {
        Ok(RuntimeInstallStatus {
            installed: true,
            detail: Some("duplicate finish callback test plugin".to_string()),
        })
    }

    fn launch_run(&self, context: RuntimeLaunchContext) -> anyhow::Result<RuntimeLaunchOutcome> {
        let token = context.runtime_rpc_token.as_str().to_string();
        let task_id = context.task.task_id;
        Ok(RuntimeLaunchOutcome {
            run_id: context.run.run_id,
            status: RuntimeRunStatus::Running,
            exit_code: Some(0),
            callbacks: vec![
                awiki_deamon::cli_wrapper::CliWrapperRequest::task_finish(
                    token.clone(),
                    task_id.clone(),
                    "first done",
                )
                .into_rpc_request(),
                awiki_deamon::cli_wrapper::CliWrapperRequest::task_finish(
                    token,
                    task_id,
                    "second done",
                )
                .into_rpc_request(),
            ],
            metadata: json!({"driver_id": "codex"}),
        })
    }
}

#[derive(Debug, Clone)]
struct ResetBeforeFinishCallbackPlugin {
    state: DaemonState,
}

impl RuntimePlugin for ResetBeforeFinishCallbackPlugin {
    fn plugin_id(&self) -> &str {
        "generic-cli"
    }

    fn check_install_status(&self) -> anyhow::Result<RuntimeInstallStatus> {
        Ok(RuntimeInstallStatus {
            installed: true,
            detail: Some("reset before finish callback test plugin".to_string()),
        })
    }

    fn launch_run(&self, context: RuntimeLaunchContext) -> anyhow::Result<RuntimeLaunchOutcome> {
        let route_session = context.cli_route_session.as_ref().expect("route session");
        let route_key = route_session.route_key.clone();
        self.state
            .reset_cli_route_session_by_route(&route_key)
            .unwrap();
        let reactivated = self
            .state
            .get_or_create_cli_route_session(CreateCliRouteSession {
                agent_did: route_session.agent_did.clone(),
                runtime_profile_id: route_session.runtime_profile_id.clone(),
                driver_id: route_session.driver_id.clone(),
                controller_user_id: route_session.controller_user_id.clone(),
                controller_full_handle: route_session.controller_full_handle.clone(),
                controller_scope_key: route_session.controller_scope_key.clone(),
                controller_did: route_session.controller_did.clone(),
                conversation_id: route_session.conversation_id.clone(),
                workspace_path: route_session.workspace_path.clone(),
                session_dir: route_session.session_dir.clone(),
            })
            .unwrap();
        assert!(self
            .state
            .try_acquire_cli_route_session_lease(
                &reactivated.route_key,
                "run_after_reset",
                "test-new-run",
                awiki_deamon::security::runtime_token::current_time_millis().unwrap() + 60_000,
            )
            .unwrap());
        let token = context.runtime_rpc_token.as_str().to_string();
        Ok(RuntimeLaunchOutcome {
            run_id: context.run.run_id,
            status: RuntimeRunStatus::Running,
            exit_code: Some(0),
            callbacks: vec![awiki_deamon::cli_wrapper::CliWrapperRequest::task_finish(
                token,
                context.task.task_id,
                "late final after reset",
            )
            .into_rpc_request()],
            metadata: json!({"driver_id": "codex"}),
        })
    }
}

#[derive(Debug, Clone)]
struct ResetBeforeFallbackFinalPlugin {
    state: DaemonState,
}

impl RuntimePlugin for ResetBeforeFallbackFinalPlugin {
    fn plugin_id(&self) -> &str {
        "generic-cli"
    }

    fn check_install_status(&self) -> anyhow::Result<RuntimeInstallStatus> {
        Ok(RuntimeInstallStatus {
            installed: true,
            detail: Some("reset before fallback final test plugin".to_string()),
        })
    }

    fn launch_run(&self, context: RuntimeLaunchContext) -> anyhow::Result<RuntimeLaunchOutcome> {
        let route_key = context
            .cli_route_session
            .as_ref()
            .expect("route session")
            .route_key
            .clone();
        let session_dir = context
            .cli_route_session
            .as_ref()
            .expect("route session")
            .session_dir
            .clone();
        std::fs::create_dir_all(&session_dir).unwrap();
        let final_output_path = session_dir.join("late-output.md");
        std::fs::write(&final_output_path, "late fallback final after reset").unwrap();
        self.state
            .reset_cli_route_session_by_route(&route_key)
            .unwrap();
        Ok(RuntimeLaunchOutcome {
            run_id: context.run.run_id,
            status: RuntimeRunStatus::Finished,
            exit_code: Some(0),
            callbacks: Vec::new(),
            metadata: json!({
                "driver_id": "codex",
                "native_session_id": "codex-native-late-after-reset",
                "native_session_source": "json_event",
                "final_output_path": final_output_path,
            }),
        })
    }
}

#[derive(Debug, Clone)]
struct NativeSessionMetadataPlugin {
    driver_id: &'static str,
    native_session_id: &'static str,
    native_session_source: &'static str,
}

impl RuntimePlugin for NativeSessionMetadataPlugin {
    fn plugin_id(&self) -> &str {
        "generic-cli"
    }

    fn check_install_status(&self) -> anyhow::Result<RuntimeInstallStatus> {
        Ok(RuntimeInstallStatus {
            installed: true,
            detail: Some("native session metadata test plugin".to_string()),
        })
    }

    fn launch_run(&self, context: RuntimeLaunchContext) -> anyhow::Result<RuntimeLaunchOutcome> {
        let token = context.runtime_rpc_token.as_str().to_string();
        Ok(RuntimeLaunchOutcome {
            run_id: context.run.run_id,
            status: RuntimeRunStatus::Finished,
            exit_code: Some(0),
            callbacks: vec![awiki_deamon::cli_wrapper::CliWrapperRequest::task_finish(
                token,
                context.task.task_id,
                "done without trusted native id",
            )
            .into_rpc_request()],
            metadata: json!({
                "driver_id": self.driver_id,
                "native_session_id": self.native_session_id,
                "native_session_source": self.native_session_source,
            }),
        })
    }
}

#[derive(Debug, Clone)]
struct UnauthorizedMsgSendCallbackPlugin;

impl RuntimePlugin for UnauthorizedMsgSendCallbackPlugin {
    fn plugin_id(&self) -> &str {
        "generic-cli"
    }

    fn check_install_status(&self) -> anyhow::Result<RuntimeInstallStatus> {
        Ok(RuntimeInstallStatus {
            installed: true,
            detail: Some("unauthorized msg.send callback test plugin".to_string()),
        })
    }

    fn launch_run(&self, context: RuntimeLaunchContext) -> anyhow::Result<RuntimeLaunchOutcome> {
        Ok(RuntimeLaunchOutcome {
            run_id: context.run.run_id,
            status: RuntimeRunStatus::Running,
            exit_code: Some(0),
            callbacks: vec![awiki_deamon::cli_wrapper::CliWrapperRequest::msg_send(
                context.runtime_rpc_token.as_str().to_string(),
                "did:human:mallory",
                "blocked",
            )
            .into_rpc_request()],
            metadata: json!({"driver_id": "codex"}),
        })
    }
}

#[test]
fn duplicate_task_finish_callback_is_idempotent() {
    let (root, state) = fixture();
    let outbox = MemoryRuntimeOutbox::default();
    let plugin = DuplicateFinishCallbackPlugin;
    let profile = profile(root.path().join("workspace"));

    let result = run_controller_text_task(
        &state,
        &profile,
        &plugin,
        &outbox,
        ControllerTextMessage {
            message_id: "msg_duplicate_finish".to_string(),
            conversation_id: Some("conv_duplicate_finish".to_string()),
            sender_did: "did:human:alice".to_string(),
            requester_user_id: None,
            requester_full_handle: None,
            trigger_kind: awiki_deamon::runtime::RuntimeTaskTriggerKind::ControllerDirect,
            invocation_authority: awiki_deamon::runtime::RuntimeInvocationAuthority::Controller,
            target_agent_did: "did:agent:alice-coder".to_string(),
            text: "finish once".to_string(),
        },
    )
    .unwrap();

    assert_eq!(result.run.status, RuntimeRunStatus::Finished);
    let records = outbox.records();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].kind, OutboxRecordKind::Status);
    assert_eq!(records[0].state.as_deref(), Some("running"));
    assert_eq!(records[1].kind, OutboxRecordKind::Final);
    assert_eq!(records[1].text.as_deref(), Some("first done"));
    let metadata = records[1]
        .metadata
        .as_ref()
        .expect("final callback metadata");
    assert_eq!(metadata["final_source"], "task_finish_callback");
    assert_eq!(
        metadata["final_body_hash"],
        test_final_body_hash("first done")
    );
    assert_eq!(metadata["final_text_bytes"], 10);
}

#[test]
fn reset_rejects_late_task_finish_callback_without_polluting_new_route_lock() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    config.ensure_state_layout().unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();
    let outbox = MemoryRuntimeOutbox::default();
    let mut profile = profile(
        config
            .state_root
            .join("runtime")
            .join("workspaces")
            .join("profile_generic_cli_1"),
    );
    profile.workspace_mode = Some(WorkspaceMode::RouteRoot);
    let cli_profile =
        CliRuntimeProfileRecord::for_driver(&profile.runtime_profile_id, "codex").unwrap();
    state.upsert_cli_runtime_profile(&cli_profile).unwrap();
    let plugin = ResetBeforeFinishCallbackPlugin {
        state: state.clone(),
    };

    let result = run_controller_text_task_with_config(
        &config,
        &state,
        &profile,
        &plugin,
        &outbox,
        ControllerTextMessage {
            message_id: "msg_late_finish_after_reset".to_string(),
            conversation_id: Some("direct:did:human:bob".to_string()),
            sender_did: "did:human:alice".to_string(),
            requester_user_id: None,
            requester_full_handle: None,
            trigger_kind: awiki_deamon::runtime::RuntimeTaskTriggerKind::ControllerDirect,
            invocation_authority: awiki_deamon::runtime::RuntimeInvocationAuthority::Controller,
            target_agent_did: profile.agent_did.clone(),
            text: "reset before callback".to_string(),
        },
    )
    .unwrap();

    assert_eq!(result.run.status, RuntimeRunStatus::Failed);
    let run = state
        .load_runtime_run("run_task_msg_late_finish_after_reset")
        .unwrap();
    assert_eq!(run.status, RuntimeRunStatus::Failed);
    let route_key = controller_private_route_key(&profile);
    let route = state.load_cli_route_session(&route_key).unwrap().unwrap();
    assert_eq!(route.status, "running");
    assert_eq!(route.lock_run_id.as_deref(), Some("run_after_reset"));
    assert_eq!(route.last_message_id, None);
    assert_eq!(route.native_session_id, None);
    let cli_run = state
        .load_cli_driver_run("run_task_msg_late_finish_after_reset")
        .unwrap();
    assert_eq!(cli_run.status, "failed");

    let records = outbox.records();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].kind, OutboxRecordKind::Status);
    assert_eq!(records[0].state.as_deref(), Some("running"));
    assert_eq!(records[1].kind, OutboxRecordKind::Status);
    assert_eq!(records[1].state.as_deref(), Some("failed"));
    assert_eq!(
        records[1].last_error_code.as_deref(),
        Some("late_callback_rejected")
    );
    assert_eq!(
        records[1].metadata.as_ref().unwrap()["source"].as_str(),
        Some("route_lease_guard")
    );
    assert!(!records
        .iter()
        .any(|record| record.kind == OutboxRecordKind::Final));
    let connection = Connection::open(config.daemon_db_path).unwrap();
    let rejected_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM audit_log WHERE event_type = 'runtime_rpc.side_effect_rejected'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(rejected_count, 1);
}

#[test]
fn reset_rejects_late_fallback_final_and_native_id_writeback() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    config.ensure_state_layout().unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();
    let outbox = MemoryRuntimeOutbox::default();
    let mut profile = profile(
        config
            .state_root
            .join("runtime")
            .join("workspaces")
            .join("profile_generic_cli_1"),
    );
    profile.workspace_mode = Some(WorkspaceMode::RouteRoot);
    let cli_profile =
        CliRuntimeProfileRecord::for_driver(&profile.runtime_profile_id, "codex").unwrap();
    state.upsert_cli_runtime_profile(&cli_profile).unwrap();
    let plugin = ResetBeforeFallbackFinalPlugin {
        state: state.clone(),
    };

    let result = run_controller_text_task_with_config(
        &config,
        &state,
        &profile,
        &plugin,
        &outbox,
        ControllerTextMessage {
            message_id: "msg_late_fallback_after_reset".to_string(),
            conversation_id: Some("direct:did:human:bob".to_string()),
            sender_did: "did:human:alice".to_string(),
            requester_user_id: None,
            requester_full_handle: None,
            trigger_kind: awiki_deamon::runtime::RuntimeTaskTriggerKind::ControllerDirect,
            invocation_authority: awiki_deamon::runtime::RuntimeInvocationAuthority::Controller,
            target_agent_did: profile.agent_did.clone(),
            text: "reset before fallback".to_string(),
        },
    )
    .unwrap();

    assert_eq!(result.run.status, RuntimeRunStatus::Failed);
    let run = state
        .load_runtime_run("run_task_msg_late_fallback_after_reset")
        .unwrap();
    assert_eq!(run.status, RuntimeRunStatus::Failed);
    let cli_run = state
        .load_cli_driver_run("run_task_msg_late_fallback_after_reset")
        .unwrap();
    assert_eq!(cli_run.status, "failed");
    assert_eq!(cli_run.native_session_id, None);
    assert_eq!(cli_run.fallback_final_source, None);
    let route = state
        .load_cli_route_session(&cli_run.route_key)
        .unwrap()
        .unwrap();
    assert_eq!(route.status, "reset");
    assert_eq!(route.lock_run_id, None);
    assert_eq!(route.native_session_id, None);
    assert_eq!(route.native_session_source, None);
    assert_eq!(route.last_message_id, None);

    let records = outbox.records();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].kind, OutboxRecordKind::Status);
    assert_eq!(records[0].state.as_deref(), Some("running"));
    assert_eq!(records[1].kind, OutboxRecordKind::Status);
    assert_eq!(records[1].state.as_deref(), Some("failed"));
    assert_eq!(
        records[1].last_error_code.as_deref(),
        Some("late_callback_rejected")
    );
    assert!(!records
        .iter()
        .any(|record| record.kind == OutboxRecordKind::Final));
    let final_outbox = state
        .load_runtime_final_outbox_by_run("run_task_msg_late_fallback_after_reset")
        .unwrap();
    assert!(final_outbox.is_none());
}

fn test_final_body_hash(text: &str) -> String {
    let digest = Sha256::digest(text.as_bytes());
    format!(
        "sha256:{}",
        digest
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    )
}

#[test]
fn generic_cli_host_does_not_persist_untrusted_native_session_metadata() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    config.ensure_state_layout().unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();
    let outbox = MemoryRuntimeOutbox::default();
    let mut profile = profile(
        config
            .state_root
            .join("runtime")
            .join("workspaces")
            .join("profile_generic_cli_1"),
    );
    profile.workspace_mode = Some(WorkspaceMode::RouteRoot);
    let cli_profile =
        CliRuntimeProfileRecord::for_driver(&profile.runtime_profile_id, "codex").unwrap();
    state.upsert_cli_runtime_profile(&cli_profile).unwrap();

    let result = run_controller_text_task_with_config(
        &config,
        &state,
        &profile,
        &NativeSessionMetadataPlugin {
            driver_id: "codex",
            native_session_id: " codex-valid-looking-id ",
            native_session_source: "json_event",
        },
        &outbox,
        ControllerTextMessage {
            message_id: "msg_invalid_native".to_string(),
            conversation_id: Some("direct:did:human:bob".to_string()),
            sender_did: "did:human:alice".to_string(),
            requester_user_id: None,
            requester_full_handle: None,
            trigger_kind: awiki_deamon::runtime::RuntimeTaskTriggerKind::ControllerDirect,
            invocation_authority: awiki_deamon::runtime::RuntimeInvocationAuthority::Controller,
            target_agent_did: profile.agent_did.clone(),
            text: "return untrusted native id".to_string(),
        },
    )
    .unwrap();

    assert_eq!(result.run.status, RuntimeRunStatus::Finished);
    let run_record = state.load_cli_driver_run(&result.run.run_id).unwrap();
    assert_eq!(run_record.native_session_id, None);
    let route = state
        .load_cli_route_session(&run_record.route_key)
        .unwrap()
        .unwrap();
    assert_eq!(route.native_session_id, None);
    assert_eq!(route.native_session_source, None);
    assert!(outbox
        .records()
        .iter()
        .any(|record| record.kind == OutboxRecordKind::Final));
}

#[test]
fn generic_cli_host_requires_matching_native_session_source() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    config.ensure_state_layout().unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();
    let outbox = MemoryRuntimeOutbox::default();
    let mut profile = profile(
        config
            .state_root
            .join("runtime")
            .join("workspaces")
            .join("profile_generic_cli_1"),
    );
    profile.workspace_mode = Some(WorkspaceMode::RouteRoot);
    let cli_profile =
        CliRuntimeProfileRecord::for_driver(&profile.runtime_profile_id, "codex").unwrap();
    state.upsert_cli_runtime_profile(&cli_profile).unwrap();

    let result = run_controller_text_task_with_config(
        &config,
        &state,
        &profile,
        &NativeSessionMetadataPlugin {
            driver_id: "codex",
            native_session_id: "codex-valid-looking-id",
            native_session_source: " json_event ",
        },
        &outbox,
        ControllerTextMessage {
            message_id: "msg_invalid_native_source".to_string(),
            conversation_id: Some("direct:did:human:bob".to_string()),
            sender_did: "did:human:alice".to_string(),
            requester_user_id: None,
            requester_full_handle: None,
            trigger_kind: awiki_deamon::runtime::RuntimeTaskTriggerKind::ControllerDirect,
            invocation_authority: awiki_deamon::runtime::RuntimeInvocationAuthority::Controller,
            target_agent_did: profile.agent_did.clone(),
            text: "return invalid native source".to_string(),
        },
    )
    .unwrap();

    let run_record = state.load_cli_driver_run(&result.run.run_id).unwrap();
    assert_eq!(run_record.native_session_id, None);
    let route = state
        .load_cli_route_session(&run_record.route_key)
        .unwrap()
        .unwrap();
    assert_eq!(route.native_session_id, None);
    assert_eq!(route.native_session_source, None);
}

#[test]
fn generic_cli_runtime_profile_policy_allows_non_controller_msg_send() {
    let (root, state) = fixture();
    let outbox = MemoryRuntimeOutbox::default();
    let plugin = MsgSendCallbackPlugin;
    let profile = profile(root.path().join("workspace"));
    let mut cli_profile =
        CliRuntimeProfileRecord::for_driver(&profile.runtime_profile_id, "codex").unwrap();
    cli_profile.recipient_policy_json = json!({
        "allow_controller": true,
        "allowed_dids": ["did:human:bob"],
        "allowed_security": ["default_plain"]
    });
    state.upsert_cli_runtime_profile(&cli_profile).unwrap();

    let result = run_controller_text_task(
        &state,
        &profile,
        &plugin,
        &outbox,
        ControllerTextMessage {
            message_id: "msg_policy".to_string(),
            conversation_id: Some("conv_policy".to_string()),
            sender_did: "did:human:alice".to_string(),
            requester_user_id: None,
            requester_full_handle: None,
            trigger_kind: awiki_deamon::runtime::RuntimeTaskTriggerKind::ControllerDirect,
            invocation_authority: awiki_deamon::runtime::RuntimeInvocationAuthority::Controller,
            target_agent_did: "did:agent:alice-coder".to_string(),
            text: "send to bob".to_string(),
        },
    )
    .unwrap();

    assert_eq!(result.run.status, RuntimeRunStatus::Finished);
    let records = outbox.records();
    assert_eq!(records.len(), 3);
    assert_eq!(records[0].kind, OutboxRecordKind::Status);
    assert_eq!(records[0].state.as_deref(), Some("running"));
    assert_eq!(records[1].kind, OutboxRecordKind::Message);
    assert_eq!(records[1].recipient.as_deref(), Some("did:human:bob"));
    assert_eq!(records[1].text.as_deref(), Some("hello from generic-cli"));
    assert_eq!(records[2].kind, OutboxRecordKind::Final);

    let connection = Connection::open(root.path().join("daemon.db")).unwrap();
    let allowed_recipients_json: String = connection
        .query_row(
            "SELECT allowed_recipients_json FROM runtime_rpc_tokens WHERE token_id = ?1",
            [&result.token_id],
            |row| row.get(0),
        )
        .unwrap();
    assert!(allowed_recipients_json.contains("did:human:bob"));
    let sent_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM audit_log WHERE event_type = 'runtime.msg_send.sent'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(sent_count, 1);
}

#[test]
fn runtime_run_token_allows_current_conversation_attachment_send() {
    let (root, state) = fixture();
    let outbox = MemoryRuntimeOutbox::default();
    let plugin = AttachmentSendCallbackPlugin;
    let profile = profile(root.path().join("workspace"));

    let result = run_controller_text_task(
        &state,
        &profile,
        &plugin,
        &outbox,
        ControllerTextMessage {
            message_id: "msg_attachment_policy".to_string(),
            conversation_id: Some("conv_attachment_policy".to_string()),
            sender_did: "did:human:alice".to_string(),
            requester_user_id: None,
            requester_full_handle: None,
            trigger_kind: awiki_deamon::runtime::RuntimeTaskTriggerKind::ControllerDirect,
            invocation_authority: awiki_deamon::runtime::RuntimeInvocationAuthority::Controller,
            target_agent_did: "did:agent:alice-coder".to_string(),
            text: "send current conversation attachment".to_string(),
        },
    )
    .unwrap();

    assert_eq!(result.run.status, RuntimeRunStatus::Finished);
    let records = outbox.records();
    assert_eq!(records.len(), 3);
    assert_eq!(records[0].kind, OutboxRecordKind::Status);
    assert_eq!(records[0].state.as_deref(), Some("running"));
    assert_eq!(records[1].kind, OutboxRecordKind::Message);
    assert_eq!(records[1].recipient.as_deref(), Some("did:human:alice"));
    assert_eq!(
        records[1].raw_recipient.as_deref(),
        Some("current_conversation")
    );
    assert_eq!(records[1].text.as_deref(), Some("report ready"));
    assert_eq!(records[2].kind, OutboxRecordKind::Final);

    let connection = Connection::open(root.path().join("daemon.db")).unwrap();
    let allowed_methods_json: String = connection
        .query_row(
            "SELECT allowed_methods_json FROM runtime_rpc_tokens WHERE token_id = ?1",
            [&result.token_id],
            |row| row.get(0),
        )
        .unwrap();
    assert!(allowed_methods_json.contains("send_attachment"));
    let audit_dump: String = connection
        .query_row(
            "SELECT COALESCE(detail_json, '') FROM audit_log WHERE event_type = 'runtime.attachment_send.sent' ORDER BY created_at_ms DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(audit_dump.contains("report.txt"));
    assert!(!audit_dump.contains(root.path().to_string_lossy().as_ref()));
}

#[test]
fn generic_cli_runtime_profile_policy_rejects_unlisted_msg_send() {
    let (root, state) = fixture();
    let outbox = MemoryRuntimeOutbox::default();
    let plugin = UnauthorizedMsgSendCallbackPlugin;
    let profile = profile(root.path().join("workspace"));
    let mut cli_profile =
        CliRuntimeProfileRecord::for_driver(&profile.runtime_profile_id, "codex").unwrap();
    cli_profile.recipient_policy_json = json!({
        "allow_controller": true,
        "allowed_dids": ["did:human:bob"],
        "allowed_security": ["default_plain"]
    });
    state.upsert_cli_runtime_profile(&cli_profile).unwrap();

    let error = run_controller_text_task(
        &state,
        &profile,
        &plugin,
        &outbox,
        ControllerTextMessage {
            message_id: "msg_policy_blocked".to_string(),
            conversation_id: Some("conv_policy_blocked".to_string()),
            sender_did: "did:human:alice".to_string(),
            requester_user_id: None,
            requester_full_handle: None,
            trigger_kind: awiki_deamon::runtime::RuntimeTaskTriggerKind::ControllerDirect,
            invocation_authority: awiki_deamon::runtime::RuntimeInvocationAuthority::Controller,
            target_agent_did: "did:agent:alice-coder".to_string(),
            text: "send to mallory".to_string(),
        },
    )
    .unwrap_err();

    assert!(error.to_string().contains("runtime callback"));
    let records = outbox.records();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].kind, OutboxRecordKind::Status);
    assert_eq!(records[0].state.as_deref(), Some("running"));
}

#[derive(Debug, Clone)]
struct SendMessageDriver;

impl awiki_deamon::plugins::generic_cli::GenericCliDriver for SendMessageDriver {
    fn check_install_status(&self) -> anyhow::Result<RuntimeInstallStatus> {
        Ok(RuntimeInstallStatus {
            installed: true,
            detail: Some("send message test driver".to_string()),
        })
    }

    fn run(
        &self,
        invocation: awiki_deamon::plugins::generic_cli::GenericCliInvocation,
    ) -> anyhow::Result<awiki_deamon::plugins::generic_cli::GenericCliExit> {
        let _callback = awiki_deamon::cli_wrapper::CliWrapperRequest::msg_send(
            invocation.runtime_rpc_token,
            "did:human:bob",
            "hello from generic-cli",
        )
        .into_rpc_request();
        assert_eq!(invocation.callbacks.len(), 2);
        Ok(awiki_deamon::plugins::generic_cli::GenericCliExit {
            exit_code: 0,
            status: RuntimeRunStatus::Finished,
            callbacks: invocation.callbacks,
            metadata: json!({"driver_id": "codex"}),
        })
    }
}

#[derive(Debug, Clone)]
struct DirtyFallbackFinalDriver {
    final_output_path: std::path::PathBuf,
}

impl awiki_deamon::plugins::generic_cli::GenericCliDriver for DirtyFallbackFinalDriver {
    fn check_install_status(&self) -> anyhow::Result<RuntimeInstallStatus> {
        Ok(RuntimeInstallStatus {
            installed: true,
            detail: Some("dirty fallback final test driver".to_string()),
        })
    }

    fn run(
        &self,
        invocation: awiki_deamon::plugins::generic_cli::GenericCliInvocation,
    ) -> anyhow::Result<awiki_deamon::plugins::generic_cli::GenericCliExit> {
        std::fs::write(
            &self.final_output_path,
            format!(
                "\u{1b}[35mhost fallback dirty {}\u{1b}[0m\u{7}\n",
                invocation.runtime_rpc_token
            ),
        )?;
        Ok(awiki_deamon::plugins::generic_cli::GenericCliExit {
            exit_code: 0,
            status: RuntimeRunStatus::Finished,
            callbacks: Vec::new(),
            metadata: json!({
                "driver_id": "codex",
                "final_output_path": self.final_output_path,
                "output": {
                    "final_output_path": self.final_output_path,
                },
            }),
        })
    }
}

#[test]
fn generic_cli_host_sanitizes_untrusted_fallback_final_text() {
    let (root, state) = fixture();
    let outbox = MemoryRuntimeOutbox::default();
    let final_output_path = root.path().join("dirty-fallback-final.txt");
    let plugin = GenericCliRuntimePlugin::new(DirtyFallbackFinalDriver {
        final_output_path: final_output_path.clone(),
    });
    let profile = profile(root.path().join("workspace"));

    let result = run_controller_text_task(
        &state,
        &profile,
        &plugin,
        &outbox,
        ControllerTextMessage {
            message_id: "msg_dirty_fallback".to_string(),
            conversation_id: Some("conv_dirty_fallback".to_string()),
            sender_did: "did:human:alice".to_string(),
            requester_user_id: None,
            requester_full_handle: None,
            trigger_kind: awiki_deamon::runtime::RuntimeTaskTriggerKind::ControllerDirect,
            invocation_authority: awiki_deamon::runtime::RuntimeInvocationAuthority::Controller,
            target_agent_did: "did:agent:alice-coder".to_string(),
            text: "run dirty fallback".to_string(),
        },
    )
    .unwrap();

    assert_eq!(result.run.status, RuntimeRunStatus::Finished);
    let records = outbox.records();
    assert_eq!(records.len(), 3);
    assert_eq!(records[1].kind, OutboxRecordKind::Message);
    assert_eq!(
        records[1].text.as_deref(),
        Some("host fallback dirty <redacted-runtime-rpc-token>")
    );
    assert_eq!(records[1].recipient.as_deref(), Some("did:human:alice"));
    assert_eq!(records[2].kind, OutboxRecordKind::Status);
    assert_eq!(records[2].state.as_deref(), Some("succeeded"));
    assert_eq!(records[2].text.as_deref(), Some("Runtime response sent"));
    assert!(!records[1]
        .text
        .as_deref()
        .unwrap_or_default()
        .contains("\u{1b}"));
    assert!(!records[1]
        .text
        .as_deref()
        .unwrap_or_default()
        .contains("\u{7}"));
    let persisted_final = std::fs::read_to_string(&final_output_path).unwrap();
    assert_eq!(
        persisted_final.trim(),
        "host fallback dirty <redacted-runtime-rpc-token>"
    );
    assert!(!persisted_final.contains("rtok_"));
    assert!(!persisted_final.contains("\u{1b}"));
    assert!(!persisted_final.contains("\u{7}"));
    let run_record = state.load_cli_driver_run(&result.run.run_id).unwrap();
    assert_eq!(
        run_record.fallback_final_source.as_deref(),
        Some("codex_output_last_message")
    );
    assert_eq!(run_record.final_output_path, Some(final_output_path));
    assert_eq!(
        run_record.output_json["fallback_final_sanitizer"]["output_sanitized"].as_bool(),
        Some(true)
    );
    assert_eq!(
        run_record.output_json["fallback_final_sanitizer"]["token_redacted"].as_bool(),
        Some(true)
    );
    let final_outbox = state
        .load_runtime_final_outbox_by_run(&result.run.run_id)
        .unwrap()
        .unwrap();
    assert_eq!(final_outbox.status, "sent");
    assert_eq!(
        final_outbox.final_text,
        "host fallback dirty <redacted-runtime-rpc-token>"
    );
    assert_eq!(final_outbox.final_source, "codex_output_last_message");
}

#[test]
fn generic_cli_runtime_profile_policy_persists_allowed_recipients_in_token_scope() {
    let (root, state) = fixture();
    let outbox = MemoryRuntimeOutbox::default();
    let plugin = GenericCliRuntimePlugin::new(SendMessageDriver);
    let profile = profile(root.path().join("workspace"));
    let mut cli_profile =
        CliRuntimeProfileRecord::for_driver(&profile.runtime_profile_id, "codex").unwrap();
    cli_profile.recipient_policy_json = json!({
        "allow_controller": true,
        "allowed_dids": ["did:human:bob"],
        "allowed_security": ["default_plain"]
    });
    state.upsert_cli_runtime_profile(&cli_profile).unwrap();

    let result = run_controller_text_task(
        &state,
        &profile,
        &plugin,
        &outbox,
        ControllerTextMessage {
            message_id: "msg_policy".to_string(),
            conversation_id: Some("conv_policy".to_string()),
            sender_did: "did:human:alice".to_string(),
            requester_user_id: None,
            requester_full_handle: None,
            trigger_kind: awiki_deamon::runtime::RuntimeTaskTriggerKind::ControllerDirect,
            invocation_authority: awiki_deamon::runtime::RuntimeInvocationAuthority::Controller,
            target_agent_did: "did:agent:alice-coder".to_string(),
            text: "send to bob".to_string(),
        },
    )
    .unwrap();

    assert_eq!(result.run.status, RuntimeRunStatus::Finished);
    let connection = Connection::open(root.path().join("daemon.db")).unwrap();
    let allowed_recipients_json: String = connection
        .query_row(
            "SELECT allowed_recipients_json FROM runtime_rpc_tokens WHERE token_id = ?1",
            [&result.token_id],
            |row| row.get(0),
        )
        .unwrap();
    assert!(allowed_recipients_json.contains("did:human:bob"));
}

#[cfg(unix)]
#[test]
fn command_driver_uses_local_rpc_without_task_text_env_or_callbacks() {
    use awiki_deamon::local_rpc::{bind_uds_listener, handle_uds_stream_with_outbox};
    use std::os::unix::fs::PermissionsExt;
    use std::time::{Duration, Instant};

    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    config.ensure_state_layout().unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();
    let profile = profile(root.path().join("workspace"));
    upsert_test_cli_profile(&state, &profile.runtime_profile_id, "command");
    std::fs::create_dir_all(profile.workspace_root.as_ref().unwrap()).unwrap();

    let script_path = root.path().join("generic-cli-driver.sh");
    let env_capture = root.path().join("driver-env.jsonl");
    std::fs::write(
        &script_path,
        format!(
            r#"#!/bin/sh
set -eu
cat > "{env_capture}" <<EOF
TASK_TEXT=${{AWIKI_DAEMON_TASK_TEXT-}}
SOCKET=${{AWIKI_DAEMON_SOCKET-}}
RUN_ID=${{AWIKI_DAEMON_RUN_ID-}}
TASK_ID=${{AWIKI_DAEMON_TASK_ID-}}
AGENT_DID=${{AWIKI_DAEMON_AGENT_DID-}}
PROFILE_ID=${{AWIKI_DAEMON_RUNTIME_PROFILE_ID-}}
WRAPPER=${{AWIKI_DAEMON_CLI_WRAPPER-}}
EOF
python3 - "$AWIKI_DAEMON_SOCKET" "$AWIKI_DAEMON_RUNTIME_RPC_TOKEN" "$AWIKI_DAEMON_TASK_ID" <<'PY'
import json
import socket
import sys

socket_path, token, task_id = sys.argv[1:4]
for request in [
    {{
        "runtime_rpc_token": token,
        "method": "task.status",
        "params": {{"task_id": task_id, "state": "running", "text": "command started"}},
    }},
    {{
        "runtime_rpc_token": token,
        "method": "task.finish",
        "params": {{"task_id": task_id, "text": "command finished"}},
    }},
]:
    client = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    client.connect(socket_path)
    client.sendall((json.dumps(request) + "\n").encode())
    response = b""
    while not response.endswith(b"\n"):
        chunk = client.recv(4096)
        if not chunk:
            break
        response += chunk
    client.close()
    decoded = json.loads(response.decode())
    if not decoded.get("ok"):
        raise SystemExit(decoded)
PY
"#,
            env_capture = env_capture.display()
        ),
    )
    .unwrap();
    let mut permissions = std::fs::metadata(&script_path).unwrap().permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(&script_path, permissions).unwrap();

    let outbox = MemoryRuntimeOutbox::default();
    let listener = bind_uds_listener(&config.local_socket_path).unwrap();
    let worker_state = state.clone();
    let worker_outbox = outbox.clone();
    let worker = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut handled = 0;
        while handled < 2 {
            match listener.accept() {
                Ok((stream, _)) => {
                    handle_uds_stream_with_outbox(&worker_state, &worker_outbox, stream).unwrap();
                    handled += 1;
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if Instant::now() > deadline {
                        panic!("timed out waiting for command driver RPC");
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("accept command driver RPC: {error}"),
            }
        }
    });

    let plugin = GenericCliRuntimePlugin::new(CommandGenericCliDriver::new(&script_path, vec![]));
    let result = run_controller_text_task_with_config(
        &config,
        &state,
        &profile,
        &plugin,
        &outbox,
        ControllerTextMessage {
            message_id: "msg_command_driver".to_string(),
            conversation_id: Some("conv_command_driver".to_string()),
            sender_did: "did:human:alice".to_string(),
            requester_user_id: None,
            requester_full_handle: None,
            trigger_kind: awiki_deamon::runtime::RuntimeTaskTriggerKind::ControllerDirect,
            invocation_authority: awiki_deamon::runtime::RuntimeInvocationAuthority::Controller,
            target_agent_did: "did:agent:alice-coder".to_string(),
            text: "run command driver".to_string(),
        },
    )
    .unwrap();
    worker.join().unwrap();

    assert_eq!(result.launch_outcome.status, RuntimeRunStatus::Finished);
    assert!(result.launch_outcome.callbacks.is_empty());
    let records = outbox.records();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].kind, OutboxRecordKind::Status);
    assert_eq!(records[1].kind, OutboxRecordKind::Final);

    let env_dump = std::fs::read_to_string(env_capture).unwrap();
    assert!(env_dump.contains("TASK_TEXT=\n"));
    assert!(env_dump.contains("SOCKET="));
    assert!(env_dump.contains("RUN_ID=run_task_msg_command_driver"));
    assert!(env_dump.contains("TASK_ID=task_msg_command_driver"));
    assert!(env_dump.contains("AGENT_DID=did:agent:alice-coder"));
    assert!(env_dump.contains("PROFILE_ID=profile_generic_cli_1"));
    assert!(env_dump.contains("WRAPPER=library:awiki_deamon::cli_wrapper"));
}

#[cfg(unix)]
#[test]
fn generic_cli_driver_registry_runs_command_profile() {
    use awiki_deamon::local_rpc::{bind_uds_listener, handle_uds_stream_with_outbox};
    use std::os::unix::fs::PermissionsExt;
    use std::time::{Duration, Instant};

    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    config.ensure_state_layout().unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();
    let profile = profile(root.path().join("workspace"));
    upsert_test_cli_profile(&state, &profile.runtime_profile_id, "codex");
    std::fs::create_dir_all(profile.workspace_root.as_ref().unwrap()).unwrap();

    let script_path = root.path().join("registry-driver.sh");
    std::fs::write(
        &script_path,
        r#"#!/bin/sh
set -eu
python3 - "$AWIKI_DAEMON_SOCKET" "$AWIKI_DAEMON_RUNTIME_RPC_TOKEN" "$AWIKI_DAEMON_TASK_ID" <<'PY'
import json
import socket
import sys

socket_path, token, task_id = sys.argv[1:4]
request = {
    "runtime_rpc_token": token,
    "method": "task.finish",
    "params": {"task_id": task_id, "text": "registry finished"},
}
client = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
client.connect(socket_path)
client.sendall((json.dumps(request) + "\n").encode())
response = b""
while not response.endswith(b"\n"):
    chunk = client.recv(4096)
    if not chunk:
        break
    response += chunk
client.close()
decoded = json.loads(response.decode())
if not decoded.get("ok"):
    raise SystemExit(decoded)
PY
"#,
    )
    .unwrap();
    let mut permissions = std::fs::metadata(&script_path).unwrap().permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(&script_path, permissions).unwrap();

    let mut cli_profile =
        CliRuntimeProfileRecord::for_driver(&profile.runtime_profile_id, "command").unwrap();
    cli_profile.binary_path = Some(script_path);
    state.upsert_cli_runtime_profile(&cli_profile).unwrap();

    let outbox = MemoryRuntimeOutbox::default();
    let listener = bind_uds_listener(&config.local_socket_path).unwrap();
    let worker_state = state.clone();
    let worker_outbox = outbox.clone();
    let worker = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match listener.accept() {
                Ok((stream, _)) => {
                    handle_uds_stream_with_outbox(&worker_state, &worker_outbox, stream).unwrap();
                    break;
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if Instant::now() > deadline {
                        panic!("timed out waiting for registry driver RPC");
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("accept registry driver RPC: {error}"),
            }
        }
    });

    let loaded_cli_profile = state
        .load_cli_runtime_profile(&profile.runtime_profile_id)
        .unwrap();
    let plugin = GenericCliDriverRegistry::new(loaded_cli_profile);
    let result = run_controller_text_task_with_config(
        &config,
        &state,
        &profile,
        &plugin,
        &outbox,
        ControllerTextMessage {
            message_id: "msg_registry_driver".to_string(),
            conversation_id: Some("conv_registry_driver".to_string()),
            sender_did: "did:human:alice".to_string(),
            requester_user_id: None,
            requester_full_handle: None,
            trigger_kind: awiki_deamon::runtime::RuntimeTaskTriggerKind::ControllerDirect,
            invocation_authority: awiki_deamon::runtime::RuntimeInvocationAuthority::Controller,
            target_agent_did: "did:agent:alice-coder".to_string(),
            text: "run registry driver".to_string(),
        },
    )
    .unwrap();
    worker.join().unwrap();

    assert_eq!(result.run.status, RuntimeRunStatus::Finished);
    let records = outbox.records();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].kind, OutboxRecordKind::Status);
    assert_eq!(records[0].state.as_deref(), Some("running"));
    assert_eq!(records[1].kind, OutboxRecordKind::Final);
    assert_eq!(records[1].text.as_deref(), Some("registry finished"));
}

#[cfg(unix)]
#[test]
fn command_driver_timeout_marks_failed_and_releases_route_without_final() {
    use std::os::unix::fs::PermissionsExt;
    use std::time::Duration;

    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    config.ensure_state_layout().unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();
    let mut profile = profile(root.path().join("workspace"));
    profile.workspace_mode = Some(WorkspaceMode::RouteRoot);
    upsert_test_cli_profile(&state, &profile.runtime_profile_id, "command");
    std::fs::create_dir_all(profile.workspace_root.as_ref().unwrap()).unwrap();

    let script_path = root.path().join("hanging-command-driver.sh");
    std::fs::write(
        &script_path,
        r#"#!/bin/sh
set -eu
sleep 10
"#,
    )
    .unwrap();
    let mut permissions = std::fs::metadata(&script_path).unwrap().permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(&script_path, permissions).unwrap();

    let plugin = GenericCliRuntimePlugin::new(
        CommandGenericCliDriver::new(&script_path, vec![])
            .with_run_timeout(Duration::from_millis(50)),
    );
    let outbox = MemoryRuntimeOutbox::default();
    let result = run_controller_text_task_with_config(
        &config,
        &state,
        &profile,
        &plugin,
        &outbox,
        ControllerTextMessage {
            message_id: "msg_command_timeout".to_string(),
            conversation_id: Some("direct:did:human:bob".to_string()),
            sender_did: "did:human:alice".to_string(),
            requester_user_id: None,
            requester_full_handle: None,
            trigger_kind: awiki_deamon::runtime::RuntimeTaskTriggerKind::ControllerDirect,
            invocation_authority: awiki_deamon::runtime::RuntimeInvocationAuthority::Controller,
            target_agent_did: profile.agent_did.clone(),
            text: "hang command driver".to_string(),
        },
    )
    .unwrap();

    assert_eq!(result.run.status, RuntimeRunStatus::Failed);
    assert_eq!(result.launch_outcome.status, RuntimeRunStatus::Failed);
    assert_eq!(result.launch_outcome.exit_code, Some(124));
    assert_eq!(
        result.launch_outcome.metadata["error_code"].as_str(),
        Some("generic_cli_timeout")
    );
    assert_eq!(
        result.launch_outcome.metadata["process"]["timed_out"].as_bool(),
        Some(true)
    );
    assert_eq!(
        result.launch_outcome.metadata["process"]["management"]["process_tree_cleanup_supported"]
            .as_bool(),
        Some(true)
    );

    let records = outbox.records();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].kind, OutboxRecordKind::Status);
    assert_eq!(records[0].state.as_deref(), Some("running"));
    assert_eq!(records[1].kind, OutboxRecordKind::Status);
    assert_eq!(records[1].state.as_deref(), Some("failed"));
    assert_eq!(
        records[1].last_error_code.as_deref(),
        Some("generic_cli_timeout")
    );
    assert_eq!(
        records[1].metadata.as_ref().unwrap()["next_action"].as_str(),
        Some("manual_review_required")
    );
    assert!(!records
        .iter()
        .any(|record| record.kind == OutboxRecordKind::Final));

    let run_record = state.load_cli_driver_run(&result.run.run_id).unwrap();
    assert_eq!(run_record.status, "failed");
    let route = state
        .load_cli_route_session(&run_record.route_key)
        .unwrap()
        .unwrap();
    assert_eq!(route.status, "failed");
    assert_eq!(route.lock_run_id, None);
    assert_eq!(route.last_message_id, None);
    assert_eq!(
        route.last_error_code.as_deref(),
        Some("generic_cli_timeout")
    );
    assert_eq!(
        state
            .count_cli_runtime_locks(None, Some(&profile.runtime_profile_id), None, false)
            .unwrap(),
        0
    );
}

fn codex_config(binary_path: std::path::PathBuf) -> CodexDriverConfig {
    let config_home = binary_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join("codex-home");
    std::fs::create_dir_all(&config_home).unwrap();
    std::fs::write(config_home.join("auth.json"), "{}").unwrap();
    CodexDriverConfig {
        binary_path,
        config_home,
        profile: Some("awiki".to_string()),
        model: Some("gpt-test".to_string()),
        sandbox: "danger-full-access".to_string(),
        ignore_user_config: true,
        ignore_rules: true,
        ephemeral: false,
        output_dir: None,
        cli_wrapper: "library:awiki_deamon::cli_wrapper".to_string(),
        run_timeout: std::time::Duration::from_secs(10 * 60),
    }
}

#[test]
fn codex_driver_command_builder_uses_trusted_host_exec_contract() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    let final_output = root.path().join("final.txt");
    let driver = CodexDriver::new(codex_config(root.path().join("codex"))).unwrap();

    let args = driver.command_args(&workspace, &final_output);

    assert_eq!(args[0], "exec");
    assert!(args
        .windows(2)
        .any(|pair| pair == ["--cd", workspace.to_str().unwrap()]));
    assert!(args
        .windows(2)
        .any(|pair| pair == ["--sandbox", "danger-full-access"]));
    assert!(args.contains(&"--dangerously-bypass-approvals-and-sandbox".to_string()));
    assert!(args.windows(2).any(|pair| pair == ["--model", "gpt-test"]));
    assert!(args.windows(2).any(|pair| pair == ["--profile", "awiki"]));
    assert!(args.contains(&"--ignore-user-config".to_string()));
    assert!(args.contains(&"--ignore-rules".to_string()));
    assert!(!args.contains(&"--ephemeral".to_string()));
    assert!(args.contains(&"--skip-git-repo-check".to_string()));
    assert!(args.contains(&"--json".to_string()));
    assert!(args
        .windows(2)
        .any(|pair| pair == ["--output-last-message", final_output.to_str().unwrap()]));
    assert_eq!(args.last().map(String::as_str), Some("-"));
    assert!(!args.contains(&"--all".to_string()));
}

#[test]
fn codex_driver_command_builder_uses_explicit_resume_id_and_never_all() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    let final_output = root.path().join("final.txt");
    let driver = CodexDriver::new(codex_config(root.path().join("codex"))).unwrap();

    let args = driver.command_args_for_mode(
        &workspace,
        &final_output,
        CodexResumeMode::ResumeId("codex-session-123".to_string()),
    );

    assert_eq!(args[0], "exec");
    let resume_index = args.iter().position(|arg| arg == "resume").unwrap();
    let cd_index = args.iter().position(|arg| arg == "--cd").unwrap();
    assert!(cd_index < resume_index);
    assert_eq!(args[resume_index + 1], "codex-session-123");
    assert!(args
        .windows(2)
        .any(|pair| pair == ["--cd", workspace.to_str().unwrap()]));
    assert!(args
        .windows(2)
        .any(|pair| pair == ["--output-last-message", final_output.to_str().unwrap()]));
    assert_eq!(args.last().map(String::as_str), Some("-"));
    assert!(!args.contains(&"--last".to_string()));
    assert!(!args.contains(&"--all".to_string()));
}

#[test]
fn codex_driver_command_builder_uses_fresh_when_route_has_no_native_session() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    let final_output = root.path().join("final.txt");
    let driver = CodexDriver::new(codex_config(root.path().join("codex"))).unwrap();

    let args = driver.command_args_for_mode(&workspace, &final_output, CodexResumeMode::Fresh);

    assert_eq!(args[0], "exec");
    assert_eq!(args.last().map(String::as_str), Some("-"));
    assert!(args
        .windows(2)
        .any(|pair| pair == ["--cd", workspace.to_str().unwrap()]));
    assert!(!args.contains(&"resume".to_string()));
    assert!(!args.contains(&"--last".to_string()));
    assert!(!args.contains(&"--all".to_string()));
}

#[test]
fn codex_native_session_id_parser_handles_known_jsonl_shapes() {
    let stdout = br#"
not json
{"type":"session.created","session_id":"codex-session-top"}
{"thread":{"id":"thread-nested"}}
{"msg":{"session":{"id":"session-msg"}}}
"#;

    assert_eq!(
        codex_native_session_id_from_stdout_jsonl(stdout).as_deref(),
        Some("codex-session-top")
    );
    assert_eq!(
        codex_native_session_id_from_stdout_jsonl(br#"{"thread":{"id":"thread-nested"}}"#)
            .as_deref(),
        Some("thread-nested")
    );
    assert_eq!(
        codex_native_session_id_from_stdout_jsonl(br#"{"msg":{"session":{"id":"session-msg"}}}"#)
            .as_deref(),
        Some("session-msg")
    );
    assert_eq!(
        codex_native_session_id_from_stdout_jsonl(br#"{"conversation_id":"awiki-route"}"#),
        None
    );
}

#[test]
fn native_session_id_validation_rejects_path_control_and_overlong_values() {
    assert!(validate_native_session_id("codex", "codex-session-123"));
    assert!(validate_native_session_id(
        "claude-code",
        "11111111-2222-4333-8444-555555555555"
    ));
    assert!(validate_native_session_source("codex", "json_event"));
    assert!(validate_native_session_source(
        "claude-code",
        "generated_session_id"
    ));
    assert!(!validate_native_session_id("codex", " codex-session-123 "));
    assert!(!validate_native_session_id(
        "claude-code",
        " 11111111-2222-4333-8444-555555555555 "
    ));

    let bad_ids = vec![
        "../codex".to_string(),
        "codex/session".to_string(),
        "codex\\session".to_string(),
        "codex session".to_string(),
        "codex\nsession".to_string(),
        "..".to_string(),
        ".".to_string(),
        "a".repeat(129),
    ];
    for bad in bad_ids {
        assert!(
            !validate_native_session_id("codex", &bad),
            "unexpected valid codex native id: {bad:?}"
        );
        assert!(
            !validate_native_session_id("claude-code", &bad),
            "unexpected valid claude native id: {bad:?}"
        );
    }
    assert!(!validate_native_session_source("codex", "stream_json"));
    assert!(!validate_native_session_source("codex", " json_event "));
    assert!(!validate_native_session_source("claude-code", "json_event"));
    assert!(!validate_native_session_source(
        "claude-code",
        " stream_json "
    ));
    assert!(!validate_native_session_id("gemini", "session-123"));
}

#[test]
fn native_session_id_parsers_ignore_untrusted_values() {
    for stdout in [
        br#"{"session_id":"../codex"}"#.as_slice(),
        br#"{"session_id":"codex/session"}"#.as_slice(),
        br#"{"thread":{"id":"codex session"}}"#.as_slice(),
        br#"{"msg":{"session":{"id":".."}}}"#.as_slice(),
    ] {
        assert_eq!(codex_native_session_id_from_stdout_jsonl(stdout), None);
    }
    for stdout in [
        br#"{"type":"system","session_id":"../claude"}"#.as_slice(),
        br#"{"type":"system","session_id":"claude/session"}"#.as_slice(),
        br#"{"message":{"session":{"id":"claude session"}}}"#.as_slice(),
        br#"{"conversation":{"id":"..evil"}}"#.as_slice(),
    ] {
        assert_eq!(claude_code_native_session_id_from_stream_json(stdout), None);
    }
}

#[test]
fn codex_driver_command_builder_allows_explicit_ephemeral_debug_mode() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    let final_output = root.path().join("final.txt");
    let mut config = codex_config(root.path().join("codex"));
    config.ephemeral = true;
    let driver = CodexDriver::new(config).unwrap();

    let args = driver.command_args(&workspace, &final_output);

    assert!(args.contains(&"--ephemeral".to_string()));
}

#[test]
fn codex_driver_accepts_trusted_host_full_access_sandbox() {
    let root = tempfile::tempdir().unwrap();
    let mut config = codex_config(root.path().join("codex"));
    config.sandbox = "danger-full-access".to_string();

    let driver = CodexDriver::new(config).unwrap();

    let args = driver.command_args(
        &root.path().join("workspace"),
        &root.path().join("final.txt"),
    );
    assert!(args
        .windows(2)
        .any(|pair| pair == ["--sandbox", "danger-full-access"]));
    assert!(args.contains(&"--dangerously-bypass-approvals-and-sandbox".to_string()));
}

#[test]
fn codex_driver_config_prefers_driver_config_sandbox_over_profile_default() {
    let root = tempfile::tempdir().unwrap();
    let mut cli_profile =
        CliRuntimeProfileRecord::for_driver("profile_generic_cli_1", "codex").unwrap();
    cli_profile.binary_path = Some(root.path().join("codex-from-profile"));
    let config_home = root.path().join("codex-home-from-profile");
    std::fs::create_dir_all(&config_home).unwrap();
    cli_profile.config_home = Some(config_home.clone());
    cli_profile.default_model = Some("model-from-profile".to_string());
    cli_profile.default_sandbox = Some("read-only".to_string());
    cli_profile.driver_config_json = json!({
        "sandbox": "workspace-write",
        "profile": "codex-profile",
        "ignore_user_config": true,
        "ignore_rules": true,
        "ephemeral": false
    });

    let config = CodexDriverConfig::from_profile(&cli_profile).unwrap();

    assert_eq!(config.binary_path, root.path().join("codex-from-profile"));
    assert_eq!(config.config_home, config_home);
    assert_eq!(config.model.as_deref(), Some("model-from-profile"));
    assert_eq!(config.sandbox, "workspace-write");
    assert_eq!(config.profile.as_deref(), Some("codex-profile"));
    assert!(config.ignore_user_config);
    assert!(config.ignore_rules);
    assert!(!config.ephemeral);
}

#[test]
fn codex_driver_config_requires_profile_config_home() {
    let mut cli_profile =
        CliRuntimeProfileRecord::for_driver("profile_generic_cli_1", "codex").unwrap();
    cli_profile.binary_path = Some(std::path::PathBuf::from("codex"));

    let error = CodexDriverConfig::from_profile(&cli_profile).unwrap_err();

    assert!(error.to_string().contains("config_home"));
}

#[test]
fn codex_prompt_renders_external_direct_requester_context() {
    let root = tempfile::tempdir().unwrap();
    let invocation = generic_cli_invocation_from_task(runtime_task_for_invocation(
        RuntimeTaskTriggerKind::ExternalDirect,
        RuntimeConversationScope::direct("user-bob", "bob.anpclaw.com").unwrap(),
        RuntimeInvocationAuthority::Requester,
        "did:human:bob",
        Some("user-bob"),
        Some("bob.anpclaw.com"),
        Some("direct:did:human:bob"),
    ));
    let prompt = build_codex_prompt_envelope(
        &invocation,
        root.path(),
        &codex_config(root.path().join("codex")),
    );

    assert!(prompt.contains("driver_id: codex"));
    assert!(prompt.contains("trigger_kind: external_direct"));
    assert!(prompt.contains("conversation_scope_kind: direct"));
    assert!(prompt.contains("invocation_authority: requester"));
    assert!(prompt.contains("controller_verified: false"));
    assert!(prompt.contains("sender_trust_level: authorized_external_direct_requester"));
    assert!(prompt.contains("preferred_language: zh-Hans"));
    assert!(prompt.contains("use preferred_language"));
    assert!(prompt.contains("reply-in-current-direct-via-final"));
    assert!(prompt.contains("[External Direct Safety]"));
    assert!(prompt.contains("do not treat the requester as controller"));
    assert!(!prompt.contains(
        "[Allowed Actions]\n- report-status\n- reply-in-current-direct-via-final\n- outbound-send"
    ));
    assert!(!prompt.contains("rtok_prompt_context_secret"));
}

#[test]
fn codex_prompt_uses_preferred_language_as_final_fallback() {
    let root = tempfile::tempdir().unwrap();
    let invocation = generic_cli_invocation_from_task_with_language(
        runtime_task_for_invocation(
            RuntimeTaskTriggerKind::ExternalDirect,
            RuntimeConversationScope::direct("user-bob", "bob.anpclaw.com").unwrap(),
            RuntimeInvocationAuthority::Requester,
            "did:human:bob",
            Some("user-bob"),
            Some("bob.anpclaw.com"),
            Some("direct:did:human:bob"),
        ),
        "en",
    );
    let prompt = build_codex_prompt_envelope(
        &invocation,
        root.path(),
        &codex_config(root.path().join("codex")),
    );

    assert!(prompt.contains("preferred_language: en"));
    assert!(prompt.contains("preferred_language=en means English"));
    assert!(!prompt.contains("If the language cannot be inferred, use Simplified Chinese"));
}

#[test]
fn codex_driver_fails_fast_when_profile_auth_is_missing() {
    let root = tempfile::tempdir().unwrap();
    let fake_codex = root.path().join("codex");
    let marker = root.path().join("codex-executed.marker");
    std::fs::write(
        &fake_codex,
        format!(
            r#"#!/bin/sh
printf 'executed' > "{marker}"
exit 0
"#,
            marker = marker.display(),
        ),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(&fake_codex).unwrap().permissions();
        permissions.set_mode(0o700);
        std::fs::set_permissions(&fake_codex, permissions).unwrap();
    }
    let mut config = codex_config(fake_codex);
    std::fs::remove_file(config.config_home.join("auth.json")).unwrap();
    config.output_dir = Some(root.path().join("codex-output"));
    let workspace_root = root.path().join("workspace");
    std::fs::create_dir_all(&workspace_root).unwrap();
    let driver = CodexDriver::new(config).unwrap();

    let exit = driver
        .run(generic_cli_invocation_for_process_test(
            root.path(),
            &workspace_root,
        ))
        .unwrap();

    assert_eq!(exit.status, RuntimeRunStatus::Failed);
    assert_eq!(exit.exit_code, 78);
    assert_eq!(
        exit.metadata["error_code"].as_str(),
        Some("generic_cli_auth_missing")
    );
    assert_eq!(exit.metadata["auth_status"].as_str(), Some("missing"));
    assert_eq!(exit.metadata["process"]["spawned"].as_bool(), Some(false));
    assert!(
        !marker.exists(),
        "Codex binary should not be spawned when auth.json is missing"
    );
}

#[cfg(unix)]
#[test]
fn codex_driver_runs_in_isolated_process_group() {
    use std::os::unix::fs::PermissionsExt;

    let root = tempfile::tempdir().unwrap();
    let fake_codex = root.path().join("codex-process-group");
    let process_capture = root.path().join("codex-process.txt");
    std::fs::write(
        &fake_codex,
        format!(
            r#"#!/bin/sh
set -eu
if [ "${{1-}}" = "--version" ]; then
  echo "codex-cli 9.9.9"
  exit 0
fi
cat >/dev/null
FINAL_OUTPUT=""
PREV=""
for ARG in "$@"; do
  if [ "$PREV" = "--output-last-message" ]; then
    FINAL_OUTPUT="$ARG"
  fi
  PREV="$ARG"
done
printf 'PID=%s\n' "$$" > "{process_capture}"
ps -o pgid= -p "$$" | tr -d ' ' | sed 's/^/PGID=/' >> "{process_capture}"
printf 'codex process group final\n' > "$FINAL_OUTPUT"
printf '{{"session_id":"codex-process-group-session"}}\n'
"#,
            process_capture = process_capture.display(),
        ),
    )
    .unwrap();
    let mut permissions = std::fs::metadata(&fake_codex).unwrap().permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(&fake_codex, permissions).unwrap();

    let workspace_root = root.path().join("workspace");
    std::fs::create_dir_all(&workspace_root).unwrap();
    let output_dir = root.path().join("codex-output");
    let mut config = codex_config(fake_codex);
    config.output_dir = Some(output_dir);
    let driver = CodexDriver::new(config).unwrap();
    let invocation = generic_cli_invocation_for_process_test(root.path(), &workspace_root);

    let exit = driver.run(invocation).unwrap();

    assert_eq!(exit.status, RuntimeRunStatus::Finished);
    assert_eq!(
        exit.metadata["process"]["process_group_isolated"].as_bool(),
        Some(true)
    );
    assert_eq!(
        exit.metadata["process"]["process_tree_cleanup_supported"].as_bool(),
        Some(true)
    );
    assert_eq!(
        exit.metadata["process"]["process_tree_cleanup_strategy"].as_str(),
        Some("unix_process_group")
    );
    let capture = std::fs::read_to_string(process_capture).unwrap();
    let child_pgid = captured_value(&capture, "PGID").parse::<i32>().unwrap();
    let parent_pgid = unsafe { libc::getpgrp() };
    assert_ne!(child_pgid, parent_pgid);
}

fn claude_code_config(binary_path: std::path::PathBuf) -> ClaudeCodeDriverConfig {
    ClaudeCodeDriverConfig {
        binary_path,
        model: Some("sonnet-test".to_string()),
        sandbox: "danger-full-access".to_string(),
        permission_mode: "bypassPermissions".to_string(),
        tools: Some("default".to_string()),
        allowed_tools: Some("Bash,WebSearch,WebFetch".to_string()),
        setting_sources: Some("user".to_string()),
        strict_mcp_config: true,
        bare: false,
        no_session_persistence: false,
        output_dir: None,
        cli_wrapper: "library:awiki_deamon::cli_wrapper".to_string(),
        run_timeout: std::time::Duration::from_secs(10 * 60),
    }
}

#[cfg(unix)]
#[test]
fn claude_code_driver_check_install_status_uses_probe_env_allowlist() {
    use std::os::unix::fs::PermissionsExt;

    let root = tempfile::tempdir().unwrap();
    let fake_claude = root.path().join("claude");
    let env_capture = root.path().join("claude-version-env.txt");
    std::fs::write(
        &fake_claude,
        format!(
            r#"#!/bin/sh
if [ "${{1-}}" = "--version" ]; then
  cat > "{env_capture}" <<EOF
HOME=${{HOME-}}
CODEX_HOME=${{CODEX_HOME-}}
OPENAI_API_KEY=${{OPENAI_API_KEY-}}
ANTHROPIC_API_KEY=${{ANTHROPIC_API_KEY-}}
AWS_SECRET_ACCESS_KEY=${{AWS_SECRET_ACCESS_KEY-}}
GITHUB_TOKEN=${{GITHUB_TOKEN-}}
NPM_TOKEN=${{NPM_TOKEN-}}
AWIKI_DAEMON_SOCKET=${{AWIKI_DAEMON_SOCKET-}}
AWIKI_DAEMON_RUNTIME_RPC_TOKEN=${{AWIKI_DAEMON_RUNTIME_RPC_TOKEN-}}
AWIKI_DAEMON_RUN_ID=${{AWIKI_DAEMON_RUN_ID-}}
AWIKI_DAEMON_TASK_ID=${{AWIKI_DAEMON_TASK_ID-}}
EOF
  echo "1.2.3 (Claude Code)"
  exit 0
fi
exit 0
"#,
            env_capture = env_capture.display(),
        ),
    )
    .unwrap();
    let mut permissions = std::fs::metadata(&fake_claude).unwrap().permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(&fake_claude, permissions).unwrap();

    let _secret_env = EnvVarGuard::set(&[
        ("OPENAI_API_KEY", "openai-secret"),
        ("ANTHROPIC_API_KEY", "anthropic-secret"),
        ("AWS_SECRET_ACCESS_KEY", "aws-secret"),
        ("GITHUB_TOKEN", "github-secret"),
        ("NPM_TOKEN", "npm-secret"),
        ("CODEX_HOME", "/tmp/host-codex-home"),
        ("AWIKI_DAEMON_SOCKET", "/tmp/awiki.sock"),
        ("AWIKI_DAEMON_RUNTIME_RPC_TOKEN", "rtok_secret"),
        ("AWIKI_DAEMON_RUN_ID", "run_secret"),
        ("AWIKI_DAEMON_TASK_ID", "task_secret"),
    ]);

    let installed = ClaudeCodeDriver::new(claude_code_config(fake_claude))
        .unwrap()
        .check_install_status()
        .unwrap();

    assert!(installed.installed);
    assert!(installed.detail.unwrap().contains("Claude Code"));
    let env_dump = std::fs::read_to_string(env_capture).unwrap();
    assert!(env_dump.contains("HOME="));
    assert!(env_dump.contains("CODEX_HOME=/tmp/host-codex-home\n"));
    assert!(env_dump.contains("OPENAI_API_KEY=openai-secret\n"));
    assert!(env_dump.contains("ANTHROPIC_API_KEY=anthropic-secret\n"));
    assert!(env_dump.contains("AWS_SECRET_ACCESS_KEY=\n"));
    assert!(env_dump.contains("GITHUB_TOKEN=\n"));
    assert!(env_dump.contains("NPM_TOKEN=\n"));
    assert!(env_dump.contains("AWIKI_DAEMON_SOCKET=\n"));
    assert!(env_dump.contains("AWIKI_DAEMON_RUNTIME_RPC_TOKEN=\n"));
    assert!(env_dump.contains("AWIKI_DAEMON_RUN_ID=\n"));
    assert!(env_dump.contains("AWIKI_DAEMON_TASK_ID=\n"));
}

#[cfg(unix)]
#[test]
fn claude_code_driver_runs_in_isolated_process_group() {
    use std::os::unix::fs::PermissionsExt;

    let root = tempfile::tempdir().unwrap();
    let fake_claude = root.path().join("claude-process-group");
    let process_capture = root.path().join("claude-process.txt");
    std::fs::write(
        &fake_claude,
        format!(
            r#"#!/bin/sh
set -eu
if [ "${{1-}}" = "--version" ]; then
  echo "1.2.3 (Claude Code)"
  exit 0
fi
cat >/dev/null
printf 'PID=%s\n' "$$" > "{process_capture}"
ps -o pgid= -p "$$" | tr -d ' ' | sed 's/^/PGID=/' >> "{process_capture}"
printf '{{"type":"system","session_id":"claude-process-group-session"}}\n'
printf '{{"type":"result","result":"claude process group final"}}\n'
"#,
            process_capture = process_capture.display(),
        ),
    )
    .unwrap();
    let mut permissions = std::fs::metadata(&fake_claude).unwrap().permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(&fake_claude, permissions).unwrap();

    let workspace_root = root.path().join("workspace");
    std::fs::create_dir_all(&workspace_root).unwrap();
    let output_dir = root.path().join("claude-output");
    let mut config = claude_code_config(fake_claude);
    config.output_dir = Some(output_dir);
    let driver = ClaudeCodeDriver::new(config).unwrap();
    let invocation = generic_cli_invocation_for_process_test(root.path(), &workspace_root);

    let exit = driver.run(invocation).unwrap();

    assert_eq!(exit.status, RuntimeRunStatus::Finished);
    assert_eq!(
        exit.metadata["process"]["process_group_isolated"].as_bool(),
        Some(true)
    );
    assert_eq!(
        exit.metadata["process"]["process_tree_cleanup_supported"].as_bool(),
        Some(true)
    );
    assert_eq!(
        exit.metadata["process"]["process_tree_cleanup_strategy"].as_str(),
        Some("unix_process_group")
    );
    let capture = std::fs::read_to_string(process_capture).unwrap();
    let child_pgid = captured_value(&capture, "PGID").parse::<i32>().unwrap();
    let parent_pgid = unsafe { libc::getpgrp() };
    assert_ne!(child_pgid, parent_pgid);
}

#[test]
fn claude_code_driver_command_builder_uses_trusted_host_print_contract() {
    let root = tempfile::tempdir().unwrap();
    let driver = ClaudeCodeDriver::new(claude_code_config(root.path().join("claude"))).unwrap();

    let args = driver.command_args_for_mode(ClaudeCodeSessionMode::New {
        session_id: "11111111-2222-4333-8444-555555555555".to_string(),
    });

    assert_eq!(args[0], "-p");
    assert!(args.contains(&"--verbose".to_string()));
    assert!(args
        .windows(2)
        .any(|pair| pair == ["--output-format", "stream-json"]));
    assert!(args
        .windows(2)
        .any(|pair| pair == ["--permission-mode", "bypassPermissions"]));
    assert!(args.windows(2).any(|pair| pair == ["--tools", "default"]));
    assert!(args
        .windows(2)
        .any(|pair| pair == ["--allowed-tools", "Bash,WebSearch,WebFetch"]));
    assert!(args.contains(&"--dangerously-skip-permissions".to_string()));
    assert!(args
        .windows(2)
        .any(|pair| pair == ["--model", "sonnet-test"]));
    assert!(args
        .windows(2)
        .any(|pair| pair == ["--setting-sources", "user"]));
    assert!(args.contains(&"--strict-mcp-config".to_string()));
    assert!(args
        .windows(2)
        .any(|pair| pair == ["--session-id", "11111111-2222-4333-8444-555555555555"]));
    assert!(!args.contains(&"--resume".to_string()));
    assert!(!args.contains(&"--continue".to_string()));
    assert!(!args.contains(&"--no-session-persistence".to_string()));
}

#[test]
fn claude_code_driver_command_builder_uses_resume_without_continue() {
    let root = tempfile::tempdir().unwrap();
    let driver = ClaudeCodeDriver::new(claude_code_config(root.path().join("claude"))).unwrap();

    let args = driver.command_args_for_mode(ClaudeCodeSessionMode::ResumeId(
        "claude-session-123".to_string(),
    ));

    assert_eq!(args[0], "-p");
    assert!(args.contains(&"--verbose".to_string()));
    assert!(args
        .windows(2)
        .any(|pair| pair == ["--resume", "claude-session-123"]));
    assert!(!args.contains(&"--session-id".to_string()));
    assert!(!args.contains(&"--continue".to_string()));
    assert!(!args.contains(&"--no-session-persistence".to_string()));
}

#[test]
fn claude_code_prompt_renders_controller_group_mention_context() {
    let root = tempfile::tempdir().unwrap();
    let invocation = generic_cli_invocation_from_task(runtime_task_for_invocation(
        RuntimeTaskTriggerKind::GroupMention,
        RuntimeConversationScope::group_visible("did:group:team"),
        RuntimeInvocationAuthority::Controller,
        "did:human:alice",
        Some("user-alice"),
        Some("alice.anpclaw.com"),
        Some("group:did:group:team"),
    ));
    let prompt = build_claude_code_prompt_envelope(
        &invocation,
        root.path(),
        &claude_code_config(root.path().join("claude")),
    );

    assert!(prompt.contains("driver_id: claude-code"));
    assert!(prompt.contains("trigger_kind: group_mention"));
    assert!(prompt.contains("conversation_scope_kind: group_visible"));
    assert!(prompt.contains("invocation_authority: controller"));
    assert!(prompt.contains("controller_verified: true"));
    assert!(prompt.contains("sender_trust_level: verified_controller_group_visible"));
    assert!(prompt.contains("reply-in-current-group-via-final"));
    assert!(prompt.contains("outbound-send"));
    assert!(prompt.contains("[Group Mention Safety]"));
    assert!(prompt.contains("ordinary final reply still goes to the current group"));
    assert!(!prompt.contains("rtok_prompt_context_secret"));
}

#[test]
fn claude_code_stream_json_parser_handles_session_and_result_shapes() {
    let stdout = br#"
not json
{"type":"system","session_id":"claude-session-top"}
{"message":{"session":{"id":"claude-session-nested"}}}
{"type":"result","result":"final from result"}
"#;

    assert_eq!(
        claude_code_native_session_id_from_stream_json(stdout).as_deref(),
        Some("claude-session-top")
    );
    assert_eq!(
        claude_code_native_session_id_from_stream_json(
            br#"{"message":{"session":{"id":"claude-session-nested"}}}"#
        )
        .as_deref(),
        Some("claude-session-nested")
    );
    assert_eq!(
        claude_code_native_session_id_from_stream_json(br#"{"conversation_id":"awiki-route"}"#),
        None
    );
    assert_eq!(
        claude_code_final_text_from_stream_json(stdout).as_deref(),
        Some("final from result")
    );
    assert_eq!(
        claude_code_final_text_from_stream_json(
            br#"{"message":{"content":[{"type":"text","text":"hello "},{"type":"text","text":"world"}]}}"#
        )
        .as_deref(),
        Some("hello world")
    );
}

#[test]
fn claude_code_driver_still_rejects_non_plan_permission_mode_for_read_only() {
    let root = tempfile::tempdir().unwrap();
    let mut config = claude_code_config(root.path().join("claude"));
    config.permission_mode = "default".to_string();
    config.sandbox = "read-only".to_string();

    let error = ClaudeCodeDriver::new(config).unwrap_err();

    assert!(error.to_string().contains("permission_mode"));
}

#[test]
fn claude_code_driver_config_defaults_to_host_home_diagnostic() {
    let root = tempfile::tempdir().unwrap();
    let mut cli_profile =
        CliRuntimeProfileRecord::for_driver("profile_generic_cli_1", "claude-code").unwrap();
    cli_profile.binary_path = Some(root.path().join("claude-from-profile"));
    cli_profile.default_model = Some("model-from-profile".to_string());
    cli_profile.default_sandbox = Some("danger-full-access".to_string());
    cli_profile.driver_config_json = json!({
        "setting_sources": "user",
        "strict_mcp_config": true
    });

    let config = ClaudeCodeDriverConfig::from_profile(&cli_profile).unwrap();

    assert_eq!(config.binary_path, root.path().join("claude-from-profile"));
    assert_eq!(config.model.as_deref(), Some("model-from-profile"));
    assert_eq!(config.sandbox, "danger-full-access");
    assert_eq!(config.permission_mode, "bypassPermissions");
    assert_eq!(config.tools.as_deref(), Some("default"));
    assert_eq!(
        config.allowed_tools.as_deref(),
        Some("Bash,WebSearch,WebFetch")
    );
    assert_eq!(config.setting_sources.as_deref(), Some("user"));
    assert!(config.strict_mcp_config);
    assert!(!config.no_session_persistence);
}

#[test]
fn claude_code_driver_config_reads_driver_config_binary_path_when_profile_path_missing() {
    let mut cli_profile =
        CliRuntimeProfileRecord::for_driver("profile_generic_cli_1", "claude-code").unwrap();
    cli_profile.driver_config_json = json!({
        "binary_path": "/tmp/configured-claude",
        "setting_sources": "user"
    });

    let config = ClaudeCodeDriverConfig::from_profile(&cli_profile).unwrap();

    assert_eq!(
        config.binary_path,
        std::path::PathBuf::from("/tmp/configured-claude")
    );
}

#[cfg(unix)]
#[test]
fn claude_code_registry_runs_profile_instead_of_not_implemented() {
    use std::os::unix::fs::PermissionsExt;

    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    config.ensure_state_layout().unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();
    let profile = profile(root.path().join("workspace"));
    upsert_test_cli_profile(&state, &profile.runtime_profile_id, "claude-code");
    std::fs::create_dir_all(profile.workspace_root.as_ref().unwrap()).unwrap();

    let fake_claude = root.path().join("claude-registry");
    std::fs::write(
        &fake_claude,
        r#"#!/bin/sh
if [ "${1-}" = "--version" ]; then
  echo "1.2.3 (Claude Code)"
  exit 0
fi
cat >/dev/null
printf '{"type":"system","session_id":"claude-registry-session"}\n'
printf '{"type":"result","result":"registry claude finished"}\n'
"#,
    )
    .unwrap();
    let mut permissions = std::fs::metadata(&fake_claude).unwrap().permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(&fake_claude, permissions).unwrap();

    let mut cli_profile =
        CliRuntimeProfileRecord::for_driver(&profile.runtime_profile_id, "claude-code").unwrap();
    cli_profile.binary_path = Some(fake_claude);
    cli_profile.driver_config_json = json!({"setting_sources": "user"});
    state.upsert_cli_runtime_profile(&cli_profile).unwrap();

    let plugin = GenericCliDriverRegistry::new(cli_profile);
    let installed = plugin.check_install_status().unwrap();
    assert!(installed.installed);
    assert!(!installed
        .detail
        .unwrap_or_default()
        .contains("not implemented"));

    let outbox = MemoryRuntimeOutbox::default();
    let result = run_controller_text_task_with_config(
        &config,
        &state,
        &profile,
        &plugin,
        &outbox,
        ControllerTextMessage {
            message_id: "msg_claude_registry".to_string(),
            conversation_id: Some("conv_claude_registry".to_string()),
            sender_did: "did:human:alice".to_string(),
            requester_user_id: None,
            requester_full_handle: None,
            trigger_kind: awiki_deamon::runtime::RuntimeTaskTriggerKind::ControllerDirect,
            invocation_authority: awiki_deamon::runtime::RuntimeInvocationAuthority::Controller,
            target_agent_did: "did:agent:alice-coder".to_string(),
            text: "run registry claude".to_string(),
        },
    )
    .unwrap();

    assert_eq!(result.run.status, RuntimeRunStatus::Finished);
    assert_eq!(
        result.launch_outcome.metadata["driver_id"].as_str(),
        Some("claude-code")
    );
    assert_eq!(
        result.launch_outcome.metadata["native_session_id"].as_str(),
        Some("claude-registry-session")
    );
    let records = outbox.records();
    assert!(records
        .iter()
        .any(|record| record.kind == OutboxRecordKind::Message
            && record.text.as_deref() == Some("registry claude finished")
            && record.recipient.as_deref() == Some("did:human:alice")));
    let final_outbox = state
        .load_runtime_final_outbox_by_run(&result.run.run_id)
        .unwrap()
        .unwrap();
    assert_eq!(final_outbox.status, "sent");
    assert_eq!(final_outbox.final_text, "registry claude finished");
    assert_eq!(final_outbox.final_source, "claude-code_output_last_message");
}

#[cfg(unix)]
#[test]
fn codex_driver_check_install_status_handles_fake_binary_and_missing_binary() {
    use std::os::unix::fs::PermissionsExt;

    let root = tempfile::tempdir().unwrap();
    let fake_codex = root.path().join("codex");
    let env_capture = root.path().join("codex-version-env.txt");
    std::fs::write(
        &fake_codex,
        format!(
            r#"#!/bin/sh
if [ "${{1-}}" = "--version" ]; then
  cat > "{env_capture}" <<EOF
CODEX_HOME=${{CODEX_HOME-}}
OPENAI_API_KEY=${{OPENAI_API_KEY-}}
ANTHROPIC_API_KEY=${{ANTHROPIC_API_KEY-}}
AWS_SECRET_ACCESS_KEY=${{AWS_SECRET_ACCESS_KEY-}}
GITHUB_TOKEN=${{GITHUB_TOKEN-}}
NPM_TOKEN=${{NPM_TOKEN-}}
AWIKI_DAEMON_SOCKET=${{AWIKI_DAEMON_SOCKET-}}
AWIKI_DAEMON_RUNTIME_RPC_TOKEN=${{AWIKI_DAEMON_RUNTIME_RPC_TOKEN-}}
AWIKI_DAEMON_RUN_ID=${{AWIKI_DAEMON_RUN_ID-}}
AWIKI_DAEMON_TASK_ID=${{AWIKI_DAEMON_TASK_ID-}}
EOF
  echo "codex-cli 9.9.9"
  exit 0
fi
exit 0
"#,
            env_capture = env_capture.display(),
        ),
    )
    .unwrap();
    let mut permissions = std::fs::metadata(&fake_codex).unwrap().permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(&fake_codex, permissions).unwrap();

    let _secret_env = EnvVarGuard::set(&[
        ("OPENAI_API_KEY", "openai-secret"),
        ("ANTHROPIC_API_KEY", "anthropic-secret"),
        ("AWS_SECRET_ACCESS_KEY", "aws-secret"),
        ("GITHUB_TOKEN", "github-secret"),
        ("NPM_TOKEN", "npm-secret"),
        ("AWIKI_DAEMON_SOCKET", "/tmp/awiki.sock"),
        ("AWIKI_DAEMON_RUNTIME_RPC_TOKEN", "rtok_secret"),
        ("AWIKI_DAEMON_RUN_ID", "run_secret"),
        ("AWIKI_DAEMON_TASK_ID", "task_secret"),
    ]);
    let driver = CodexDriver::new(codex_config(fake_codex.clone())).unwrap();
    let codex_home = driver.config().config_home.clone();
    let installed = driver.check_install_status().unwrap();
    assert!(installed.installed);
    assert!(installed.detail.unwrap().contains("codex-cli 9.9.9"));
    let env_dump = std::fs::read_to_string(env_capture).unwrap();
    assert!(env_dump.contains(&format!("CODEX_HOME={}", codex_home.display())));
    assert!(env_dump.contains("OPENAI_API_KEY=\n"));
    assert!(env_dump.contains("ANTHROPIC_API_KEY=\n"));
    assert!(env_dump.contains("AWS_SECRET_ACCESS_KEY=\n"));
    assert!(env_dump.contains("GITHUB_TOKEN=\n"));
    assert!(env_dump.contains("NPM_TOKEN=\n"));
    assert!(env_dump.contains("AWIKI_DAEMON_SOCKET=\n"));
    assert!(env_dump.contains("AWIKI_DAEMON_RUNTIME_RPC_TOKEN=\n"));
    assert!(env_dump.contains("AWIKI_DAEMON_RUN_ID=\n"));
    assert!(env_dump.contains("AWIKI_DAEMON_TASK_ID=\n"));

    let missing = CodexDriver::new(codex_config(root.path().join("missing-codex")))
        .unwrap()
        .check_install_status()
        .unwrap();
    assert!(!missing.installed);
}

#[cfg(unix)]
#[test]
fn codex_driver_fake_binary_uses_stdin_env_outputs_and_local_rpc() {
    use awiki_deamon::local_rpc::{bind_uds_listener, handle_uds_stream_with_outbox};
    use std::os::unix::fs::PermissionsExt;
    use std::time::{Duration, Instant};

    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    config.ensure_state_layout().unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();
    let profile = profile(root.path().join("workspace"));
    upsert_test_cli_profile(&state, &profile.runtime_profile_id, "codex");
    std::fs::create_dir_all(profile.workspace_root.as_ref().unwrap()).unwrap();

    let fake_codex = root.path().join("codex");
    let args_capture = root.path().join("codex-args.txt");
    let env_capture = root.path().join("codex-env.txt");
    let prompt_capture = root.path().join("codex-prompt.txt");
    let output_dir = root.path().join("codex-output");
    std::fs::write(
        &fake_codex,
        format!(
            r#"#!/bin/sh
set -eu
if [ "${{1-}}" = "--version" ]; then
  echo "codex-cli 9.9.9"
  exit 0
fi
printf '%s\n' "$@" > "{args_capture}"
cat > "{prompt_capture}"
cat > "{env_capture}" <<EOF
TASK_TEXT=${{AWIKI_DAEMON_TASK_TEXT-}}
SOCKET=${{AWIKI_DAEMON_SOCKET-}}
RUN_ID=${{AWIKI_DAEMON_RUN_ID-}}
TASK_ID=${{AWIKI_DAEMON_TASK_ID-}}
AGENT_DID=${{AWIKI_DAEMON_AGENT_DID-}}
PROFILE_ID=${{AWIKI_DAEMON_RUNTIME_PROFILE_ID-}}
WRAPPER=${{AWIKI_DAEMON_CLI_WRAPPER-}}
CODEX_HOME=${{CODEX_HOME-}}
OPENAI_API_KEY=${{OPENAI_API_KEY-}}
ANTHROPIC_API_KEY=${{ANTHROPIC_API_KEY-}}
AWS_SECRET_ACCESS_KEY=${{AWS_SECRET_ACCESS_KEY-}}
GITHUB_TOKEN=${{GITHUB_TOKEN-}}
NPM_TOKEN=${{NPM_TOKEN-}}
EOF
FINAL_OUTPUT=""
PREV=""
for ARG in "$@"; do
  if [ "$PREV" = "--output-last-message" ]; then
    FINAL_OUTPUT="$ARG"
  fi
  PREV="$ARG"
done
printf 'fake codex final %s\n' "$AWIKI_DAEMON_RUNTIME_RPC_TOKEN" > "$FINAL_OUTPUT"
printf '{{"session_id":"codex-session-shared-root"}}\n'
printf '{{"type":"codex-event","message":"stdout jsonl %s"}}\n' "$AWIKI_DAEMON_RUNTIME_RPC_TOKEN"
printf '\033[31mstdout ansi %s\033[0m\007\n' "$AWIKI_DAEMON_RUNTIME_RPC_TOKEN"
printf 'stderr diagnostic %s\n' "$AWIKI_DAEMON_RUNTIME_RPC_TOKEN" >&2
printf '\033]8;;https://example.invalid\007stderr osc %s\033]8;;\007\n' "$AWIKI_DAEMON_RUNTIME_RPC_TOKEN" >&2
python3 - "$AWIKI_DAEMON_SOCKET" "$AWIKI_DAEMON_RUNTIME_RPC_TOKEN" "$AWIKI_DAEMON_TASK_ID" <<'PY'
import json
import socket
import sys

socket_path, token, task_id = sys.argv[1:4]
for request in [
    {{
        "runtime_rpc_token": token,
        "method": "task.status",
        "params": {{"task_id": task_id, "state": "running", "text": "codex started"}},
    }},
    {{
        "runtime_rpc_token": token,
        "method": "task.finish",
        "params": {{"task_id": task_id, "text": "codex finished"}},
    }},
]:
    client = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    client.connect(socket_path)
    client.sendall((json.dumps(request) + "\n").encode())
    response = b""
    while not response.endswith(b"\n"):
        chunk = client.recv(4096)
        if not chunk:
            break
        response += chunk
    client.close()
    decoded = json.loads(response.decode())
    if not decoded.get("ok"):
        raise SystemExit(decoded)
PY
"#,
            args_capture = args_capture.display(),
            prompt_capture = prompt_capture.display(),
            env_capture = env_capture.display(),
        ),
    )
    .unwrap();
    let mut permissions = std::fs::metadata(&fake_codex).unwrap().permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(&fake_codex, permissions).unwrap();

    let outbox = MemoryRuntimeOutbox::default();
    let listener = bind_uds_listener(&config.local_socket_path).unwrap();
    let worker_state = state.clone();
    let worker_outbox = outbox.clone();
    let worker = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut handled = 0;
        while handled < 2 {
            match listener.accept() {
                Ok((stream, _)) => {
                    handle_uds_stream_with_outbox(&worker_state, &worker_outbox, stream).unwrap();
                    handled += 1;
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if Instant::now() > deadline {
                        panic!("timed out waiting for codex driver RPC");
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("accept codex driver RPC: {error}"),
            }
        }
    });

    let mut driver_config = codex_config(fake_codex);
    let binary_path = driver_config.binary_path.clone();
    let codex_home = driver_config.config_home.clone();
    driver_config.output_dir = Some(output_dir.clone());
    let plugin = GenericCliRuntimePlugin::new(CodexDriver::new(driver_config).unwrap());
    let result = run_controller_text_task_with_config(
        &config,
        &state,
        &profile,
        &plugin,
        &outbox,
        ControllerTextMessage {
            message_id: "msg_codex_driver".to_string(),
            conversation_id: Some("conv_codex_driver".to_string()),
            sender_did: "did:human:alice".to_string(),
            requester_user_id: None,
            requester_full_handle: None,
            trigger_kind: awiki_deamon::runtime::RuntimeTaskTriggerKind::ControllerDirect,
            invocation_authority: awiki_deamon::runtime::RuntimeInvocationAuthority::Controller,
            target_agent_did: "did:agent:alice-coder".to_string(),
            text: "run codex driver".to_string(),
        },
    )
    .unwrap();
    worker.join().unwrap();

    assert_eq!(result.run.status, RuntimeRunStatus::Finished);
    assert_eq!(result.launch_outcome.status, RuntimeRunStatus::Finished);
    assert!(result.launch_outcome.callbacks.is_empty());

    let records = outbox.records();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].kind, OutboxRecordKind::Status);
    assert_eq!(records[1].kind, OutboxRecordKind::Final);
    assert_eq!(records[1].text.as_deref(), Some("codex finished"));

    let args_dump = std::fs::read_to_string(args_capture).unwrap();
    assert!(args_dump.contains("exec\n"));
    assert!(args_dump.contains("--cd\n"));
    assert!(args_dump.contains("--sandbox\ndanger-full-access\n"));
    assert!(args_dump.contains("--dangerously-bypass-approvals-and-sandbox\n"));
    assert!(args_dump.contains("--json\n"));
    assert!(args_dump.contains("--output-last-message\n"));
    assert!(args_dump.ends_with("-\n"));
    let prompt = std::fs::read_to_string(prompt_capture).unwrap();
    assert!(prompt.contains("[Awiki Runtime Context]"));
    assert!(prompt.contains("driver_id: codex"));
    assert!(prompt.contains("permission_policy: trusted-host-full-access"));
    assert!(prompt.contains("message_id: msg_codex_driver"));
    assert!(prompt.contains("conversation_id: conv_codex_driver"));
    assert!(prompt.contains("user_message:\nrun codex driver"));
    assert!(!prompt.contains("rtok_"));
    assert!(!prompt.contains(config.local_socket_path.to_str().unwrap()));

    let env_dump = std::fs::read_to_string(env_capture).unwrap();
    assert!(env_dump.contains("TASK_TEXT=\n"));
    assert!(env_dump.contains("SOCKET="));
    assert!(env_dump.contains("RUN_ID=run_task_msg_codex_driver"));
    assert!(env_dump.contains("TASK_ID=task_msg_codex_driver"));
    assert!(env_dump.contains("AGENT_DID=did:agent:alice-coder"));
    assert!(env_dump.contains("PROFILE_ID=profile_generic_cli_1"));
    assert!(env_dump.contains("WRAPPER=library:awiki_deamon::cli_wrapper"));
    assert!(env_dump.contains(&format!("CODEX_HOME={}", codex_home.display())));
    assert!(env_dump.contains("OPENAI_API_KEY=\n"));
    assert!(env_dump.contains("ANTHROPIC_API_KEY=\n"));
    assert!(env_dump.contains("AWS_SECRET_ACCESS_KEY=\n"));
    assert!(env_dump.contains("GITHUB_TOKEN=\n"));
    assert!(env_dump.contains("NPM_TOKEN=\n"));

    assert_eq!(
        std::fs::read_to_string(output_dir.join("final-output.txt")).unwrap(),
        "fake codex final <redacted-runtime-rpc-token>\n"
    );
    let stdout_dump = std::fs::read_to_string(output_dir.join("codex-stdout.jsonl")).unwrap();
    assert!(stdout_dump.contains("stdout jsonl"));
    assert!(stdout_dump.contains("stdout ansi"));
    assert!(!stdout_dump.contains("rtok_"));
    assert!(!stdout_dump.contains("\u{1b}"));
    assert!(!stdout_dump.contains("\u{7}"));
    let stderr_dump = std::fs::read_to_string(output_dir.join("codex-stderr.log")).unwrap();
    assert!(stderr_dump.contains("stderr diagnostic"));
    assert!(stderr_dump.contains("stderr osc"));
    assert!(!stderr_dump.contains("rtok_"));
    assert!(!stderr_dump.contains("\u{1b}"));
    assert!(!stderr_dump.contains("\u{7}"));
    assert!(
        std::fs::read_to_string(output_dir.join("codex-observation.jsonl"))
            .unwrap()
            .contains("codex.exec.observation")
    );
    assert!(
        !std::fs::read_to_string(output_dir.join("final-output.txt"))
            .unwrap()
            .contains("rtok_")
    );

    let run_record = state.load_cli_driver_run(&result.run.run_id).unwrap();
    assert_eq!(run_record.driver_id, "codex");
    assert_eq!(run_record.controller_did, "did:human:alice");
    assert_eq!(
        run_record.conversation_id.as_deref(),
        Some("conv_codex_driver")
    );
    assert_eq!(
        run_record.route_key,
        "cli:did:agent:alice-coder:controller-scope:v1:test-alice-anpclaw-com:conv_codex_driver:message-run"
    );
    assert_eq!(run_record.workspace_mode, Some(WorkspaceMode::SharedRoot));
    assert!(!run_record.is_security_boundary);
    assert_eq!(
        run_record.workspace_instance_path.as_deref(),
        profile
            .workspace_root
            .as_ref()
            .map(|path| path.canonicalize().unwrap())
            .as_deref()
    );
    assert_eq!(
        run_record.final_output_path,
        Some(output_dir.join("final-output.txt"))
    );
    assert_eq!(run_record.fallback_final_source, None);
    assert_eq!(
        run_record.native_session_id.as_deref(),
        Some("codex-session-shared-root")
    );
    assert_eq!(
        run_record.command_json["program"].as_str(),
        Some(binary_path.to_str().unwrap_or_default())
    );
    assert_eq!(
        run_record.output_json["sanitizer"]["stdout"]["output_sanitized"].as_bool(),
        Some(true)
    );
    assert_eq!(
        run_record.output_json["sanitizer"]["stdout"]["token_redacted"].as_bool(),
        Some(true)
    );
    assert_eq!(
        run_record.output_json["sanitizer"]["stderr"]["output_sanitized"].as_bool(),
        Some(true)
    );
    assert_eq!(
        run_record.output_json["sanitizer"]["final_output"]["token_redacted"].as_bool(),
        Some(true)
    );
}

#[cfg(unix)]
#[test]
fn codex_route_root_records_native_id_and_resumes_same_route() {
    use std::os::unix::fs::PermissionsExt;

    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    config.ensure_state_layout().unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();
    let outbox = MemoryRuntimeOutbox::default();
    let mut profile = profile(
        config
            .state_root
            .join("runtime")
            .join("workspaces")
            .join("profile_generic_cli_1"),
    );
    profile.workspace_mode = Some(WorkspaceMode::RouteRoot);

    let fake_codex = root.path().join("codex-route-resume");
    let args_capture_dir = root.path().join("args");
    std::fs::create_dir_all(&args_capture_dir).unwrap();
    std::fs::write(
        &fake_codex,
        format!(
            r#"#!/bin/sh
set -eu
if [ "${{1-}}" = "--version" ]; then
  echo "codex-cli 9.9.9"
  exit 0
fi
RUN="${{AWIKI_DAEMON_RUN_ID}}"
	printf '%s\n' "$@" > "{args_capture_dir}/$RUN.args"
	COUNT_FILE="{args_capture_dir}/$RUN.count"
	COUNT=0
	if [ -f "$COUNT_FILE" ]; then
	  COUNT="$(cat "$COUNT_FILE")"
	fi
	COUNT="$((COUNT + 1))"
	printf '%s\n' "$COUNT" > "$COUNT_FILE"
	cat >/dev/null
FINAL_OUTPUT=""
PREV=""
for ARG in "$@"; do
  if [ "$PREV" = "--output-last-message" ]; then
    FINAL_OUTPUT="$ARG"
  fi
  PREV="$ARG"
done
printf 'route final %s\n' "$RUN" > "$FINAL_OUTPUT"
case "$RUN" in
  run_task_msg_route_resume_1)
    printf '{{"session_id":"codex-native-bob"}}\n'
    ;;
	  run_task_msg_route_resume_2)
	    if [ "$COUNT" = "1" ]; then
	      printf 'session not found: codex-native-bob\n' >&2
	      exit 1
	    fi
	    printf '{{"session_id":"codex-native-bob-recreated"}}\n'
	    ;;
  run_task_msg_route_resume_3)
    printf '{{"session_id":"codex-native-bob-after-reset"}}\n'
    ;;
  *)
    printf '{{"session_id":"codex-native-other"}}\n'
    ;;
esac
"#,
            args_capture_dir = args_capture_dir.display(),
        ),
    )
    .unwrap();
    let mut permissions = std::fs::metadata(&fake_codex).unwrap().permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(&fake_codex, permissions).unwrap();

    let mut cli_profile =
        CliRuntimeProfileRecord::for_driver(&profile.runtime_profile_id, "codex").unwrap();
    cli_profile.binary_path = Some(fake_codex);
    cli_profile.config_home = Some(root.path().join("codex-home-route"));
    std::fs::create_dir_all(cli_profile.config_home.as_ref().unwrap()).unwrap();
    std::fs::write(
        cli_profile.config_home.as_ref().unwrap().join("auth.json"),
        "{}",
    )
    .unwrap();
    cli_profile.default_sandbox = Some("read-only".to_string());
    state.upsert_cli_runtime_profile(&cli_profile).unwrap();
    let plugin = GenericCliDriverRegistry::new(cli_profile);

    let first = run_controller_text_task_with_config(
        &config,
        &state,
        &profile,
        &plugin,
        &outbox,
        ControllerTextMessage {
            message_id: "msg_route_resume_1".to_string(),
            conversation_id: Some("direct:did:human:bob".to_string()),
            sender_did: "did:human:alice".to_string(),
            requester_user_id: None,
            requester_full_handle: None,
            trigger_kind: awiki_deamon::runtime::RuntimeTaskTriggerKind::ControllerDirect,
            invocation_authority: awiki_deamon::runtime::RuntimeInvocationAuthority::Controller,
            target_agent_did: profile.agent_did.clone(),
            text: "first route message".to_string(),
        },
    )
    .unwrap();
    let first_run = state.load_cli_driver_run(&first.run.run_id).unwrap();
    let route = state
        .load_cli_route_session(&first_run.route_key)
        .unwrap()
        .unwrap();
    assert_eq!(first.run.status, RuntimeRunStatus::Finished);
    assert_eq!(route.native_session_id.as_deref(), Some("codex-native-bob"));
    assert_eq!(route.native_session_source.as_deref(), Some("json_event"));
    assert_eq!(
        first_run.native_session_id.as_deref(),
        Some("codex-native-bob")
    );
    assert_eq!(
        first_run.final_output_path,
        Some(route.session_dir.join("last-output.md"))
    );
    let first_args =
        std::fs::read_to_string(args_capture_dir.join("run_task_msg_route_resume_1.args")).unwrap();
    assert!(first_args.starts_with("exec\n--cd\n"));
    assert!(!first_args.contains("resume\n"));
    assert!(!first_args.contains("--all"));

    let second = run_controller_text_task_with_config(
        &config,
        &state,
        &profile,
        &plugin,
        &outbox,
        ControllerTextMessage {
            message_id: "msg_route_resume_2".to_string(),
            conversation_id: Some("direct:did:human:bob".to_string()),
            sender_did: "did:human:alice".to_string(),
            requester_user_id: None,
            requester_full_handle: None,
            trigger_kind: awiki_deamon::runtime::RuntimeTaskTriggerKind::ControllerDirect,
            invocation_authority: awiki_deamon::runtime::RuntimeInvocationAuthority::Controller,
            target_agent_did: profile.agent_did.clone(),
            text: "second route message".to_string(),
        },
    )
    .unwrap();
    let second_run = state.load_cli_driver_run(&second.run.run_id).unwrap();
    assert_eq!(
        second_run.native_session_id.as_deref(),
        Some("codex-native-bob-recreated")
    );
    assert_eq!(
        second.launch_outcome.metadata["resume"]["fallback"]["reason"].as_str(),
        Some("native_session_missing")
    );
    let second_args =
        std::fs::read_to_string(args_capture_dir.join("run_task_msg_route_resume_2.args")).unwrap();
    let second_arg_lines = second_args.lines().collect::<Vec<_>>();
    assert!(!second_arg_lines.contains(&"resume"));
    assert!(!second_args.contains("--last"));
    assert!(!second_args.contains("--all"));
    assert_eq!(
        std::fs::read_to_string(args_capture_dir.join("run_task_msg_route_resume_2.count"))
            .unwrap()
            .trim(),
        "2"
    );
    let route_after_fallback = state
        .load_cli_route_session(&first_run.route_key)
        .unwrap()
        .unwrap();
    assert_eq!(
        route_after_fallback.native_session_id.as_deref(),
        Some("codex-native-bob-recreated")
    );

    state
        .reset_cli_route_session_by_route(&first_run.route_key)
        .unwrap();
    let reset_route = state
        .load_cli_route_session(&first_run.route_key)
        .unwrap()
        .unwrap();
    assert_eq!(reset_route.native_session_id, None);

    let third = run_controller_text_task_with_config(
        &config,
        &state,
        &profile,
        &plugin,
        &outbox,
        ControllerTextMessage {
            message_id: "msg_route_resume_3".to_string(),
            conversation_id: Some("direct:did:human:bob".to_string()),
            sender_did: "did:human:alice".to_string(),
            requester_user_id: None,
            requester_full_handle: None,
            trigger_kind: awiki_deamon::runtime::RuntimeTaskTriggerKind::ControllerDirect,
            invocation_authority: awiki_deamon::runtime::RuntimeInvocationAuthority::Controller,
            target_agent_did: profile.agent_did.clone(),
            text: "after reset".to_string(),
        },
    )
    .unwrap();
    let third_run = state.load_cli_driver_run(&third.run.run_id).unwrap();
    assert_eq!(
        third_run.native_session_id.as_deref(),
        Some("codex-native-bob-after-reset")
    );
    let third_args =
        std::fs::read_to_string(args_capture_dir.join("run_task_msg_route_resume_3.args")).unwrap();
    assert!(third_args.starts_with("exec\n--cd\n"));
    assert!(!third_args.contains("codex-native-bob\n"));
    assert!(!third_args.contains("--all"));
}

#[cfg(unix)]
#[test]
fn codex_route_root_existing_route_without_native_id_starts_fresh_session() {
    use std::os::unix::fs::PermissionsExt;

    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    config.ensure_state_layout().unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();
    let outbox = MemoryRuntimeOutbox::default();
    let mut profile = profile(
        config
            .state_root
            .join("runtime")
            .join("workspaces")
            .join("profile_generic_cli_1"),
    );
    profile.workspace_mode = Some(WorkspaceMode::RouteRoot);

    let fake_codex = root.path().join("codex-route-last");
    let args_capture_dir = root.path().join("args-last");
    std::fs::create_dir_all(&args_capture_dir).unwrap();
    std::fs::write(
        &fake_codex,
        format!(
            r#"#!/bin/sh
set -eu
if [ "${{1-}}" = "--version" ]; then
  echo "codex-cli 9.9.9"
  exit 0
fi
RUN="${{AWIKI_DAEMON_RUN_ID}}"
printf '%s\n' "$@" > "{args_capture_dir}/$RUN.args"
cat >/dev/null
FINAL_OUTPUT=""
PREV=""
for ARG in "$@"; do
  if [ "$PREV" = "--output-last-message" ]; then
    FINAL_OUTPUT="$ARG"
  fi
  PREV="$ARG"
done
printf 'route final %s\n' "$RUN" > "$FINAL_OUTPUT"
if [ "$RUN" = "run_task_msg_route_last_1" ]; then
  printf '{{"type":"missing-session-id"}}\n'
else
  printf '{{"session_id":"codex-native-from-fresh"}}\n'
fi
"#,
            args_capture_dir = args_capture_dir.display(),
        ),
    )
    .unwrap();
    let mut permissions = std::fs::metadata(&fake_codex).unwrap().permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(&fake_codex, permissions).unwrap();

    let mut cli_profile =
        CliRuntimeProfileRecord::for_driver(&profile.runtime_profile_id, "codex").unwrap();
    cli_profile.binary_path = Some(fake_codex);
    cli_profile.config_home = Some(root.path().join("codex-home-route-last"));
    std::fs::create_dir_all(cli_profile.config_home.as_ref().unwrap()).unwrap();
    std::fs::write(
        cli_profile.config_home.as_ref().unwrap().join("auth.json"),
        "{}",
    )
    .unwrap();
    state.upsert_cli_runtime_profile(&cli_profile).unwrap();
    let plugin = GenericCliDriverRegistry::new(cli_profile);

    let first = run_controller_text_task_with_config(
        &config,
        &state,
        &profile,
        &plugin,
        &outbox,
        ControllerTextMessage {
            message_id: "msg_route_last_1".to_string(),
            conversation_id: Some("direct:did:human:bob".to_string()),
            sender_did: "did:human:alice".to_string(),
            requester_user_id: None,
            requester_full_handle: None,
            trigger_kind: awiki_deamon::runtime::RuntimeTaskTriggerKind::ControllerDirect,
            invocation_authority: awiki_deamon::runtime::RuntimeInvocationAuthority::Controller,
            target_agent_did: profile.agent_did.clone(),
            text: "first route message".to_string(),
        },
    )
    .unwrap();
    let first_run = state.load_cli_driver_run(&first.run.run_id).unwrap();
    let route = state
        .load_cli_route_session(&first_run.route_key)
        .unwrap()
        .unwrap();
    assert_eq!(route.native_session_id, None);
    assert_eq!(route.last_message_id.as_deref(), Some("msg_route_last_1"));

    let second = run_controller_text_task_with_config(
        &config,
        &state,
        &profile,
        &plugin,
        &outbox,
        ControllerTextMessage {
            message_id: "msg_route_last_2".to_string(),
            conversation_id: Some("direct:did:human:bob".to_string()),
            sender_did: "did:human:alice".to_string(),
            requester_user_id: None,
            requester_full_handle: None,
            trigger_kind: awiki_deamon::runtime::RuntimeTaskTriggerKind::ControllerDirect,
            invocation_authority: awiki_deamon::runtime::RuntimeInvocationAuthority::Controller,
            target_agent_did: profile.agent_did.clone(),
            text: "second route message".to_string(),
        },
    )
    .unwrap();
    assert_eq!(
        second.launch_outcome.metadata["route_session"]["last_message_id_present"].as_bool(),
        Some(true)
    );
    assert_eq!(
        second.launch_outcome.metadata["resume"]["mode"].as_str(),
        Some("fresh")
    );
    let second_run = state.load_cli_driver_run(&second.run.run_id).unwrap();
    assert_eq!(
        second_run.native_session_id.as_deref(),
        Some("codex-native-from-fresh")
    );
    let second_args =
        std::fs::read_to_string(args_capture_dir.join("run_task_msg_route_last_2.args")).unwrap();
    let second_arg_lines = second_args.lines().collect::<Vec<_>>();
    assert!(!second_arg_lines.contains(&"resume"));
    assert!(!second_arg_lines.contains(&"--last"));
    assert!(!second_args.contains("--all"));
}

#[cfg(unix)]
#[test]
fn claude_code_route_root_records_generated_session_and_resumes_same_route() {
    use std::os::unix::fs::PermissionsExt;

    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    config.ensure_state_layout().unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();
    let outbox = MemoryRuntimeOutbox::default();
    let mut profile = profile(
        config
            .state_root
            .join("runtime")
            .join("workspaces")
            .join("profile_generic_cli_1"),
    );
    profile.workspace_mode = Some(WorkspaceMode::RouteRoot);

    let fake_claude = root.path().join("claude-route-resume");
    let args_capture_dir = root.path().join("claude-args");
    let prompt_capture_dir = root.path().join("claude-prompts");
    let env_capture_dir = root.path().join("claude-env");
    std::fs::create_dir_all(&args_capture_dir).unwrap();
    std::fs::create_dir_all(&prompt_capture_dir).unwrap();
    std::fs::create_dir_all(&env_capture_dir).unwrap();
    std::fs::write(
        &fake_claude,
        format!(
            r#"#!/bin/sh
set -eu
if [ "${{1-}}" = "--version" ]; then
  echo "1.2.3 (Claude Code)"
  exit 0
fi
	RUN="${{AWIKI_DAEMON_RUN_ID}}"
	printf '%s\n' "$@" > "{args_capture_dir}/$RUN.args"
	COUNT_FILE="{args_capture_dir}/$RUN.count"
	COUNT=0
	if [ -f "$COUNT_FILE" ]; then
	  COUNT="$(cat "$COUNT_FILE")"
	fi
	COUNT="$((COUNT + 1))"
	printf '%s\n' "$COUNT" > "$COUNT_FILE"
	cat > "{prompt_capture_dir}/$RUN.prompt"
cat > "{env_capture_dir}/$RUN.env" <<ENV
SOCKET=${{AWIKI_DAEMON_SOCKET-}}
RUN_ID=${{AWIKI_DAEMON_RUN_ID-}}
TASK_ID=${{AWIKI_DAEMON_TASK_ID-}}
AGENT_DID=${{AWIKI_DAEMON_AGENT_DID-}}
PROFILE_ID=${{AWIKI_DAEMON_RUNTIME_PROFILE_ID-}}
WRAPPER=${{AWIKI_DAEMON_CLI_WRAPPER-}}
OPENAI_API_KEY=${{OPENAI_API_KEY-}}
ANTHROPIC_API_KEY=${{ANTHROPIC_API_KEY-}}
AWS_SECRET_ACCESS_KEY=${{AWS_SECRET_ACCESS_KEY-}}
GITHUB_TOKEN=${{GITHUB_TOKEN-}}
NPM_TOKEN=${{NPM_TOKEN-}}
ENV
SESSION=""
PREV=""
	for ARG in "$@"; do
	  if [ "$PREV" = "--session-id" ] || [ "$PREV" = "--resume" ]; then
	    SESSION="$ARG"
	  fi
	  PREV="$ARG"
	done
	if [ "$RUN" = "run_task_msg_claude_route_2" ] && [ "$COUNT" = "1" ]; then
	  printf 'No conversation found with session ID: %s\n' "$SESSION" >&2
	  exit 1
	fi
	printf '{{"type":"system","session_id":"%s"}}\n' "$SESSION"
printf '{{"type":"result","result":"claude final %s"}}\n' "$RUN"
"#,
            args_capture_dir = args_capture_dir.display(),
            prompt_capture_dir = prompt_capture_dir.display(),
            env_capture_dir = env_capture_dir.display(),
        ),
    )
    .unwrap();
    let mut permissions = std::fs::metadata(&fake_claude).unwrap().permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(&fake_claude, permissions).unwrap();

    let mut cli_profile =
        CliRuntimeProfileRecord::for_driver(&profile.runtime_profile_id, "claude-code").unwrap();
    cli_profile.binary_path = Some(fake_claude);
    cli_profile.default_sandbox = Some("danger-full-access".to_string());
    cli_profile.driver_config_json = json!({
        "setting_sources": "user",
        "strict_mcp_config": true
    });
    state.upsert_cli_runtime_profile(&cli_profile).unwrap();
    let plugin = GenericCliDriverRegistry::new(cli_profile);

    let _secret_env = EnvVarGuard::set(&[
        ("OPENAI_API_KEY", "openai-secret"),
        ("ANTHROPIC_API_KEY", "anthropic-secret"),
        ("AWS_SECRET_ACCESS_KEY", "aws-secret"),
        ("GITHUB_TOKEN", "github-secret"),
        ("NPM_TOKEN", "npm-secret"),
    ]);

    let first = run_controller_text_task_with_config(
        &config,
        &state,
        &profile,
        &plugin,
        &outbox,
        ControllerTextMessage {
            message_id: "msg_claude_route_1".to_string(),
            conversation_id: Some("direct:did:human:bob".to_string()),
            sender_did: "did:human:alice".to_string(),
            requester_user_id: None,
            requester_full_handle: None,
            trigger_kind: awiki_deamon::runtime::RuntimeTaskTriggerKind::ControllerDirect,
            invocation_authority: awiki_deamon::runtime::RuntimeInvocationAuthority::Controller,
            target_agent_did: profile.agent_did.clone(),
            text: "first claude route message".to_string(),
        },
    )
    .unwrap();
    let first_run = state.load_cli_driver_run(&first.run.run_id).unwrap();
    let route = state
        .load_cli_route_session(&first_run.route_key)
        .unwrap()
        .unwrap();
    assert_eq!(first.run.status, RuntimeRunStatus::Finished);
    assert_eq!(route.native_session_source.as_deref(), Some("stream_json"));
    let first_native_id = route.native_session_id.clone().unwrap();
    assert_eq!(
        first_run.native_session_id.as_deref(),
        Some(first_native_id.as_str())
    );
    assert_eq!(
        first_run.fallback_final_source.as_deref(),
        Some("claude-code_output_last_message")
    );
    assert_eq!(
        first_run.final_output_path,
        Some(route.session_dir.join("last-output.md"))
    );
    let first_args =
        std::fs::read_to_string(args_capture_dir.join("run_task_msg_claude_route_1.args")).unwrap();
    assert!(first_args.starts_with("-p\n"));
    assert!(first_args.contains("--verbose\n"));
    assert!(first_args.contains("--output-format\nstream-json\n"));
    assert!(first_args.contains("--permission-mode\nbypassPermissions\n"));
    assert!(first_args.contains("--tools\ndefault\n"));
    assert!(first_args.contains("--allowed-tools\nBash,WebSearch,WebFetch\n"));
    assert!(first_args.contains("--dangerously-skip-permissions\n"));
    assert!(first_args.contains("--session-id\n"));
    assert!(first_args.contains(&format!("{first_native_id}\n")));
    assert!(!first_args.contains("--resume\n"));
    assert!(!first_args.contains("--continue"));
    assert!(!first_args.contains("--no-session-persistence"));
    let first_prompt =
        std::fs::read_to_string(prompt_capture_dir.join("run_task_msg_claude_route_1.prompt"))
            .unwrap();
    assert!(first_prompt.contains("driver_id: claude-code"));
    assert!(first_prompt.contains("permission_policy: trusted-host-full-access"));
    assert!(first_prompt.contains("tools: default"));
    assert!(first_prompt.contains("allowed_tools: Bash,WebSearch,WebFetch"));
    assert!(first_prompt.contains("message_id: msg_claude_route_1"));
    assert!(first_prompt.contains("user_message:\nfirst claude route message"));
    assert!(!first_prompt.contains("rtok_"));
    assert!(!first_prompt.contains(config.local_socket_path.to_str().unwrap()));

    let first_env =
        std::fs::read_to_string(env_capture_dir.join("run_task_msg_claude_route_1.env")).unwrap();
    assert!(first_env.contains("SOCKET="));
    assert!(first_env.contains("RUN_ID=run_task_msg_claude_route_1"));
    assert!(first_env.contains("TASK_ID=task_msg_claude_route_1"));
    assert!(first_env.contains("AGENT_DID=did:agent:alice-coder"));
    assert!(first_env.contains("PROFILE_ID=profile_generic_cli_1"));
    assert!(first_env.contains("WRAPPER=library:awiki_deamon::cli_wrapper"));
    assert!(first_env.contains("OPENAI_API_KEY=openai-secret\n"));
    assert!(first_env.contains("ANTHROPIC_API_KEY=anthropic-secret\n"));
    assert!(first_env.contains("AWS_SECRET_ACCESS_KEY=\n"));
    assert!(first_env.contains("GITHUB_TOKEN=\n"));
    assert!(first_env.contains("NPM_TOKEN=\n"));

    let second = run_controller_text_task_with_config(
        &config,
        &state,
        &profile,
        &plugin,
        &outbox,
        ControllerTextMessage {
            message_id: "msg_claude_route_2".to_string(),
            conversation_id: Some("direct:did:human:bob".to_string()),
            sender_did: "did:human:alice".to_string(),
            requester_user_id: None,
            requester_full_handle: None,
            trigger_kind: awiki_deamon::runtime::RuntimeTaskTriggerKind::ControllerDirect,
            invocation_authority: awiki_deamon::runtime::RuntimeInvocationAuthority::Controller,
            target_agent_did: profile.agent_did.clone(),
            text: "second claude route message".to_string(),
        },
    )
    .unwrap();
    let second_run = state.load_cli_driver_run(&second.run.run_id).unwrap();
    let second_native_id = second_run.native_session_id.clone().unwrap();
    assert_ne!(second_native_id, first_native_id);
    assert_eq!(
        second.launch_outcome.metadata["session"]["fallback"]["reason"].as_str(),
        Some("native_session_missing")
    );
    let second_args =
        std::fs::read_to_string(args_capture_dir.join("run_task_msg_claude_route_2.args")).unwrap();
    assert!(second_args.contains("--session-id\n"));
    assert!(second_args.contains(&format!("{second_native_id}\n")));
    assert!(!second_args.contains("--resume\n"));
    assert!(!second_args.contains("--continue"));
    assert!(!second_args.contains("--no-session-persistence"));
    assert_eq!(
        std::fs::read_to_string(args_capture_dir.join("run_task_msg_claude_route_2.count"))
            .unwrap()
            .trim(),
        "2"
    );
    let route_after_fallback = state
        .load_cli_route_session(&first_run.route_key)
        .unwrap()
        .unwrap();
    assert_eq!(
        route_after_fallback.native_session_id.as_deref(),
        Some(second_native_id.as_str())
    );

    state
        .reset_cli_route_session_by_route(&first_run.route_key)
        .unwrap();
    let third = run_controller_text_task_with_config(
        &config,
        &state,
        &profile,
        &plugin,
        &outbox,
        ControllerTextMessage {
            message_id: "msg_claude_route_3".to_string(),
            conversation_id: Some("direct:did:human:bob".to_string()),
            sender_did: "did:human:alice".to_string(),
            requester_user_id: None,
            requester_full_handle: None,
            trigger_kind: awiki_deamon::runtime::RuntimeTaskTriggerKind::ControllerDirect,
            invocation_authority: awiki_deamon::runtime::RuntimeInvocationAuthority::Controller,
            target_agent_did: profile.agent_did.clone(),
            text: "after reset".to_string(),
        },
    )
    .unwrap();
    let third_run = state.load_cli_driver_run(&third.run.run_id).unwrap();
    assert_ne!(
        third_run.native_session_id.as_deref(),
        Some(first_native_id.as_str())
    );
    let third_args =
        std::fs::read_to_string(args_capture_dir.join("run_task_msg_claude_route_3.args")).unwrap();
    assert!(third_args.contains("--session-id\n"));
    assert!(!third_args.contains(&format!("--resume\n{first_native_id}\n")));
}

#[cfg(unix)]
#[test]
fn claude_code_driver_nonzero_exit_does_not_record_generated_session_or_final() {
    use std::os::unix::fs::PermissionsExt;

    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    config.ensure_state_layout().unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();
    let profile = profile(root.path().join("workspace"));
    upsert_test_cli_profile(&state, &profile.runtime_profile_id, "claude-code");
    std::fs::create_dir_all(profile.workspace_root.as_ref().unwrap()).unwrap();

    let fake_claude = root.path().join("claude-fail");
    std::fs::write(
        &fake_claude,
        r#"#!/bin/sh
if [ "${1-}" = "--version" ]; then
  echo "1.2.3 (Claude Code)"
  exit 0
fi
cat >/dev/null
printf '{"type":"error","message":"auth missing"}\n'
exit 9
"#,
    )
    .unwrap();
    let mut permissions = std::fs::metadata(&fake_claude).unwrap().permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(&fake_claude, permissions).unwrap();

    let plugin = GenericCliRuntimePlugin::new(
        ClaudeCodeDriver::new(claude_code_config(fake_claude)).unwrap(),
    );
    let outbox = MemoryRuntimeOutbox::default();
    let result = run_controller_text_task_with_config(
        &config,
        &state,
        &profile,
        &plugin,
        &outbox,
        ControllerTextMessage {
            message_id: "msg_claude_fail".to_string(),
            conversation_id: Some("conv_claude_fail".to_string()),
            sender_did: "did:human:alice".to_string(),
            requester_user_id: None,
            requester_full_handle: None,
            trigger_kind: awiki_deamon::runtime::RuntimeTaskTriggerKind::ControllerDirect,
            invocation_authority: awiki_deamon::runtime::RuntimeInvocationAuthority::Controller,
            target_agent_did: "did:agent:alice-coder".to_string(),
            text: "run failing claude".to_string(),
        },
    )
    .unwrap();

    assert_eq!(result.run.status, RuntimeRunStatus::Failed);
    assert_eq!(result.launch_outcome.status, RuntimeRunStatus::Failed);
    assert_eq!(result.launch_outcome.exit_code, Some(9));
    assert_eq!(
        result.launch_outcome.metadata["native_session_id"],
        serde_json::Value::Null
    );
    let records = outbox.records();
    assert!(records
        .iter()
        .any(|record| record.kind == OutboxRecordKind::Status
            && record.state.as_deref() == Some("failed")));
    assert!(!records
        .iter()
        .any(|record| record.kind == OutboxRecordKind::Final));
    let run_record = state.load_cli_driver_run(&result.run.run_id).unwrap();
    assert_eq!(run_record.driver_id, "claude-code");
    assert_eq!(run_record.native_session_id, None);
    assert_eq!(run_record.fallback_final_source, None);
}

#[cfg(unix)]
#[test]
fn claude_code_route_session_rejects_no_session_persistence() {
    use std::os::unix::fs::PermissionsExt;

    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    config.ensure_state_layout().unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();
    let outbox = MemoryRuntimeOutbox::default();
    let mut profile = profile(
        config
            .state_root
            .join("runtime")
            .join("workspaces")
            .join("profile_generic_cli_1"),
    );
    profile.workspace_mode = Some(WorkspaceMode::RouteRoot);

    let fake_claude = root.path().join("claude-no-persist");
    std::fs::write(
        &fake_claude,
        r#"#!/bin/sh
if [ "${1-}" = "--version" ]; then
  echo "1.2.3 (Claude Code)"
  exit 0
fi
echo "unexpected claude invocation" >&2
exit 12
"#,
    )
    .unwrap();
    let mut permissions = std::fs::metadata(&fake_claude).unwrap().permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(&fake_claude, permissions).unwrap();

    let mut cli_profile =
        CliRuntimeProfileRecord::for_driver(&profile.runtime_profile_id, "claude-code").unwrap();
    cli_profile.binary_path = Some(fake_claude);
    cli_profile.default_sandbox = Some("read-only".to_string());
    cli_profile.driver_config_json = json!({
        "setting_sources": "user",
        "no_session_persistence": true
    });
    state.upsert_cli_runtime_profile(&cli_profile).unwrap();
    let plugin = GenericCliDriverRegistry::new(cli_profile);

    let result = run_controller_text_task_with_config(
        &config,
        &state,
        &profile,
        &plugin,
        &outbox,
        ControllerTextMessage {
            message_id: "msg_claude_no_persist".to_string(),
            conversation_id: Some("direct:did:human:bob".to_string()),
            sender_did: "did:human:alice".to_string(),
            requester_user_id: None,
            requester_full_handle: None,
            trigger_kind: awiki_deamon::runtime::RuntimeTaskTriggerKind::ControllerDirect,
            invocation_authority: awiki_deamon::runtime::RuntimeInvocationAuthority::Controller,
            target_agent_did: profile.agent_did.clone(),
            text: "route session should not use no persistence".to_string(),
        },
    )
    .unwrap();

    assert_eq!(result.run.status, RuntimeRunStatus::Failed);
    assert_eq!(result.launch_outcome.status, RuntimeRunStatus::Failed);
    assert_eq!(result.launch_outcome.exit_code, Some(2));
    assert_eq!(
        result.launch_outcome.metadata["error_code"].as_str(),
        Some("session_persistence_disabled")
    );
    assert!(result.launch_outcome.metadata["error_summary"]
        .as_str()
        .unwrap_or_default()
        .contains("no_session_persistence"));
    let run = state
        .load_runtime_run("run_task_msg_claude_no_persist")
        .unwrap();
    assert_eq!(run.status, RuntimeRunStatus::Failed);
    let cli_run = state
        .load_cli_driver_run("run_task_msg_claude_no_persist")
        .unwrap();
    assert_eq!(cli_run.driver_id, "claude-code");
    assert_eq!(cli_run.native_session_id, None);
    assert_eq!(cli_run.fallback_final_source, None);
    let route = state
        .load_cli_route_session(&cli_run.route_key)
        .unwrap()
        .unwrap();
    assert_eq!(route.status, "failed");
    assert_eq!(route.native_session_id, None);
    assert_eq!(
        route.last_error_code.as_deref(),
        Some("session_persistence_disabled")
    );
    assert!(route
        .last_error_summary
        .as_deref()
        .unwrap_or_default()
        .contains("no_session_persistence"));
    let records = outbox.records();
    let failed_status = records
        .iter()
        .find(|record| {
            record.kind == OutboxRecordKind::Status && record.state.as_deref() == Some("failed")
        })
        .expect("failed status record");
    assert_eq!(
        failed_status.last_error_code.as_deref(),
        Some("session_persistence_disabled")
    );
    assert_eq!(
        failed_status.metadata.as_ref().unwrap()["next_action"].as_str(),
        Some("manual_review_required")
    );
    assert_eq!(
        failed_status.metadata.as_ref().unwrap()["failed_message_recovery"].as_str(),
        Some("unsupported")
    );
    assert!(!records
        .iter()
        .any(|record| record.kind == OutboxRecordKind::Final));
}

#[cfg(unix)]
#[test]
fn claude_code_driver_timeout_marks_failed_and_releases_route_without_final() {
    use std::os::unix::fs::PermissionsExt;
    use std::time::Duration;

    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    config.ensure_state_layout().unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();
    let mut profile = profile(root.path().join("workspace"));
    profile.workspace_mode = Some(WorkspaceMode::RouteRoot);
    upsert_test_cli_profile(&state, &profile.runtime_profile_id, "claude-code");
    std::fs::create_dir_all(profile.workspace_root.as_ref().unwrap()).unwrap();

    let fake_claude = root.path().join("claude-timeout");
    std::fs::write(
        &fake_claude,
        r#"#!/bin/sh
if [ "${1-}" = "--version" ]; then
  echo "1.2.3 (Claude Code)"
  exit 0
fi
cat >/dev/null
sleep 10
"#,
    )
    .unwrap();
    let mut permissions = std::fs::metadata(&fake_claude).unwrap().permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(&fake_claude, permissions).unwrap();

    let mut driver_config = claude_code_config(fake_claude);
    driver_config.output_dir = Some(root.path().join("claude-timeout-output"));
    driver_config.run_timeout = Duration::from_millis(50);
    let plugin = GenericCliRuntimePlugin::new(ClaudeCodeDriver::new(driver_config).unwrap());
    let outbox = MemoryRuntimeOutbox::default();
    let result = run_controller_text_task_with_config(
        &config,
        &state,
        &profile,
        &plugin,
        &outbox,
        ControllerTextMessage {
            message_id: "msg_claude_timeout".to_string(),
            conversation_id: Some("direct:did:human:bob".to_string()),
            sender_did: "did:human:alice".to_string(),
            requester_user_id: None,
            requester_full_handle: None,
            trigger_kind: awiki_deamon::runtime::RuntimeTaskTriggerKind::ControllerDirect,
            invocation_authority: awiki_deamon::runtime::RuntimeInvocationAuthority::Controller,
            target_agent_did: profile.agent_did.clone(),
            text: "run hanging claude driver".to_string(),
        },
    )
    .unwrap();

    assert_eq!(result.run.status, RuntimeRunStatus::Failed);
    assert_eq!(result.launch_outcome.status, RuntimeRunStatus::Failed);
    assert_eq!(
        result.launch_outcome.metadata["error_code"].as_str(),
        Some("claude_code_cli_timeout")
    );
    assert_eq!(
        result.launch_outcome.metadata["process"]["timed_out"].as_bool(),
        Some(true)
    );
    let records = outbox.records();
    assert_eq!(records[1].kind, OutboxRecordKind::Status);
    assert_eq!(records[1].state.as_deref(), Some("failed"));
    assert_eq!(
        records[1].last_error_code.as_deref(),
        Some("claude_code_cli_timeout")
    );
    assert!(!records
        .iter()
        .any(|record| record.kind == OutboxRecordKind::Final));

    let run_record = state.load_cli_driver_run(&result.run.run_id).unwrap();
    assert_eq!(run_record.native_session_id, None);
    assert_eq!(run_record.status, "failed");
    let route = state
        .load_cli_route_session(&run_record.route_key)
        .unwrap()
        .unwrap();
    assert_eq!(route.status, "failed");
    assert_eq!(route.lock_run_id, None);
    assert_eq!(route.native_session_id, None);
    assert_eq!(route.last_message_id, None);
    assert_eq!(
        route.last_error_code.as_deref(),
        Some("claude_code_cli_timeout")
    );
    assert_eq!(
        state
            .count_cli_runtime_locks(None, Some(&profile.runtime_profile_id), None, false)
            .unwrap(),
        0
    );
}

#[cfg(unix)]
#[test]
fn codex_driver_success_without_finish_uses_fallback_final_once() {
    use std::os::unix::fs::PermissionsExt;

    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    config.ensure_state_layout().unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();
    let profile = profile(root.path().join("workspace"));
    upsert_test_cli_profile(&state, &profile.runtime_profile_id, "codex");
    std::fs::create_dir_all(profile.workspace_root.as_ref().unwrap()).unwrap();

    let fake_codex = root.path().join("codex-no-final");
    let output_dir = root.path().join("codex-fallback-output");
    std::fs::write(
        &fake_codex,
        r#"#!/bin/sh
if [ "${1-}" = "--version" ]; then
  echo "codex-cli 9.9.9"
  exit 0
fi
FINAL_OUTPUT=""
PREV=""
for ARG in "$@"; do
  if [ "$PREV" = "--output-last-message" ]; then
    FINAL_OUTPUT="$ARG"
  fi
  PREV="$ARG"
done
cat >/dev/null
printf '\033[32mfallback final from codex %s\033[0m\007\n' "$AWIKI_DAEMON_RUNTIME_RPC_TOKEN" > "$FINAL_OUTPUT"
exit 0
"#,
    )
    .unwrap();
    let mut permissions = std::fs::metadata(&fake_codex).unwrap().permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(&fake_codex, permissions).unwrap();

    let mut driver_config = codex_config(fake_codex);
    driver_config.output_dir = Some(output_dir.clone());
    let plugin = GenericCliRuntimePlugin::new(CodexDriver::new(driver_config).unwrap());
    let outbox = MemoryRuntimeOutbox::default();
    let result = run_controller_text_task_with_config(
        &config,
        &state,
        &profile,
        &plugin,
        &outbox,
        ControllerTextMessage {
            message_id: "msg_codex_fallback".to_string(),
            conversation_id: Some("conv_codex_fallback".to_string()),
            sender_did: "did:human:alice".to_string(),
            requester_user_id: None,
            requester_full_handle: None,
            trigger_kind: awiki_deamon::runtime::RuntimeTaskTriggerKind::ControllerDirect,
            invocation_authority: awiki_deamon::runtime::RuntimeInvocationAuthority::Controller,
            target_agent_did: "did:agent:alice-coder".to_string(),
            text: "run codex without explicit final".to_string(),
        },
    )
    .unwrap();

    assert_eq!(result.run.status, RuntimeRunStatus::Finished);
    assert_eq!(result.launch_outcome.status, RuntimeRunStatus::Finished);
    assert!(result.launch_outcome.callbacks.is_empty());
    let records = outbox.records();
    assert_eq!(records.len(), 3);
    assert_eq!(records[0].kind, OutboxRecordKind::Status);
    assert_eq!(records[0].state.as_deref(), Some("running"));
    assert_eq!(records[1].kind, OutboxRecordKind::Message);
    assert_eq!(
        records[1].text.as_deref(),
        Some("fallback final from codex <redacted-runtime-rpc-token>")
    );
    assert_eq!(records[1].recipient.as_deref(), Some("did:human:alice"));
    assert_eq!(records[2].kind, OutboxRecordKind::Status);
    assert_eq!(records[2].state.as_deref(), Some("succeeded"));
    assert_eq!(records[2].text.as_deref(), Some("Runtime response sent"));
    assert!(!records[1]
        .text
        .as_deref()
        .unwrap_or_default()
        .contains("\u{1b}"));
    assert!(!records[1]
        .text
        .as_deref()
        .unwrap_or_default()
        .contains("\u{7}"));
    let run_record = state.load_cli_driver_run(&result.run.run_id).unwrap();
    assert_eq!(
        run_record.fallback_final_source.as_deref(),
        Some("codex_output_last_message")
    );
    assert_eq!(
        run_record.final_output_path,
        Some(output_dir.join("final-output.txt"))
    );
    assert_eq!(
        run_record.output_json["sanitizer"]["final_output"]["output_sanitized"].as_bool(),
        Some(true)
    );
    assert_eq!(
        run_record.output_json["sanitizer"]["final_output"]["token_redacted"].as_bool(),
        Some(true)
    );
    assert_eq!(
        run_record.output_json["fallback_final_sanitizer"]["output_sanitized"].as_bool(),
        Some(false)
    );
    assert_eq!(
        run_record.output_json["fallback_final_sanitizer"]["token_redacted"].as_bool(),
        Some(false)
    );
    let final_outbox = state
        .load_runtime_final_outbox_by_run(&result.run.run_id)
        .unwrap()
        .unwrap();
    assert_eq!(final_outbox.status, "sent");
    assert_eq!(
        final_outbox.final_text,
        "fallback final from codex <redacted-runtime-rpc-token>"
    );
    assert_eq!(final_outbox.final_source, "codex_output_last_message");
}

#[cfg(unix)]
#[test]
fn codex_fallback_final_survives_status_delivery_failure_and_releases_route() {
    use std::os::unix::fs::PermissionsExt;

    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    config.ensure_state_layout().unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();
    let mut profile = profile(root.path().join("workspace"));
    profile.workspace_mode = Some(WorkspaceMode::RouteRoot);
    upsert_test_cli_profile(&state, &profile.runtime_profile_id, "codex");
    std::fs::create_dir_all(profile.workspace_root.as_ref().unwrap()).unwrap();

    let fake_codex = root.path().join("codex-status-failure");
    std::fs::write(
        &fake_codex,
        r#"#!/bin/sh
if [ "${1-}" = "--version" ]; then
  echo "codex-cli 9.9.9"
  exit 0
fi
FINAL_OUTPUT=""
PREV=""
for ARG in "$@"; do
  if [ "$PREV" = "--output-last-message" ]; then
    FINAL_OUTPUT="$ARG"
  fi
  PREV="$ARG"
done
cat >/dev/null
printf 'final survives status failure\n' > "$FINAL_OUTPUT"
printf '{"session_id":"codex-status-failure-session"}\n'
exit 0
"#,
    )
    .unwrap();
    let mut permissions = std::fs::metadata(&fake_codex).unwrap().permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(&fake_codex, permissions).unwrap();

    let plugin = GenericCliRuntimePlugin::new(CodexDriver::new(codex_config(fake_codex)).unwrap());
    let outbox = FailingStatusOutbox::new(MemoryRuntimeOutbox::default());
    let result = run_controller_text_task_with_config(
        &config,
        &state,
        &profile,
        &plugin,
        &outbox,
        ControllerTextMessage {
            message_id: "msg_codex_status_failure".to_string(),
            conversation_id: Some("conv_codex_status_failure".to_string()),
            sender_did: "did:human:alice".to_string(),
            requester_user_id: None,
            requester_full_handle: None,
            trigger_kind: awiki_deamon::runtime::RuntimeTaskTriggerKind::ControllerDirect,
            invocation_authority: awiki_deamon::runtime::RuntimeInvocationAuthority::Controller,
            target_agent_did: profile.agent_did.clone(),
            text: "run codex while status delivery fails".to_string(),
        },
    )
    .unwrap();

    assert_eq!(result.run.status, RuntimeRunStatus::Finished);
    let records = outbox.records();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].kind, OutboxRecordKind::Message);
    assert_eq!(
        records[0].text.as_deref(),
        Some("final survives status failure")
    );

    let run_record = state.load_cli_driver_run(&result.run.run_id).unwrap();
    assert_eq!(run_record.status, "finished");
    assert_eq!(
        run_record.fallback_final_source.as_deref(),
        Some("codex_output_last_message")
    );
    assert_eq!(
        run_record.native_session_id.as_deref(),
        Some("codex-status-failure-session")
    );
    let route = state
        .load_cli_route_session(&run_record.route_key)
        .unwrap()
        .unwrap();
    assert_eq!(route.status, "active");
    assert_eq!(route.lock_run_id, None);
    assert_eq!(
        route.last_message_id.as_deref(),
        Some("msg_codex_status_failure")
    );
    assert_eq!(
        route.native_session_id.as_deref(),
        Some("codex-status-failure-session")
    );
    let final_outbox = state
        .load_runtime_final_outbox_by_run(&result.run.run_id)
        .unwrap()
        .unwrap();
    assert_eq!(final_outbox.status, "sent");
    assert_eq!(final_outbox.final_text, "final survives status failure");

    let connection = Connection::open(root.path().join("daemon.db")).unwrap();
    let best_effort_failures: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM audit_log WHERE event_type = 'runtime.status.best_effort.failed'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(best_effort_failures, 2);
}

#[cfg(unix)]
#[test]
fn worktree_per_task_uses_daemon_runtime_temp_and_records_metadata() {
    use std::os::unix::fs::PermissionsExt;

    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    config.ensure_state_layout().unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();
    let workspace = root.path().join("git-workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    assert!(std::process::Command::new("git")
        .args(["-c", "init.defaultBranch=main"])
        .args(["init"])
        .arg(&workspace)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap()
        .success());
    std::fs::write(workspace.join("README.md"), "workspace\n").unwrap();
    assert!(std::process::Command::new("git")
        .arg("-C")
        .arg(&workspace)
        .args(["add", "README.md"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap()
        .success());
    assert!(std::process::Command::new("git")
        .arg("-C")
        .arg(&workspace)
        .args([
            "-c",
            "user.name=Awiki Test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "-m",
            "init",
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap()
        .success());
    let mut profile = profile(workspace.clone());
    profile.workspace_mode = Some(WorkspaceMode::WorktreePerTask);
    upsert_test_cli_profile(&state, &profile.runtime_profile_id, "codex");

    let fake_codex = root.path().join("codex-worktree");
    let pwd_capture = root.path().join("codex-pwd.txt");
    std::fs::write(
        &fake_codex,
        format!(
            r#"#!/bin/sh
if [ "${{1-}}" = "--version" ]; then
  echo "codex-cli 9.9.9"
  exit 0
fi
pwd > "{pwd_capture}"
FINAL_OUTPUT=""
PREV=""
for ARG in "$@"; do
  if [ "$PREV" = "--output-last-message" ]; then
    FINAL_OUTPUT="$ARG"
  fi
  PREV="$ARG"
done
cat >/dev/null
printf 'worktree final\n' > "$FINAL_OUTPUT"
exit 0
"#,
            pwd_capture = pwd_capture.display(),
        ),
    )
    .unwrap();
    let mut permissions = std::fs::metadata(&fake_codex).unwrap().permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(&fake_codex, permissions).unwrap();

    let plugin = GenericCliRuntimePlugin::new(CodexDriver::new(codex_config(fake_codex)).unwrap());
    let outbox = MemoryRuntimeOutbox::default();
    let result = run_controller_text_task_with_config(
        &config,
        &state,
        &profile,
        &plugin,
        &outbox,
        ControllerTextMessage {
            message_id: "msg_codex_worktree".to_string(),
            conversation_id: None,
            sender_did: "did:human:alice".to_string(),
            requester_user_id: None,
            requester_full_handle: None,
            trigger_kind: awiki_deamon::runtime::RuntimeTaskTriggerKind::ControllerDirect,
            invocation_authority: awiki_deamon::runtime::RuntimeInvocationAuthority::Controller,
            target_agent_did: "did:agent:alice-coder".to_string(),
            text: "run codex in worktree".to_string(),
        },
    )
    .unwrap();

    assert_eq!(result.run.status, RuntimeRunStatus::Finished);
    let run_record = state.load_cli_driver_run(&result.run.run_id).unwrap();
    let instance_path = run_record.workspace_instance_path.unwrap();
    let worktrees_root = config
        .runtime_temp_dir
        .join("worktrees")
        .canonicalize()
        .unwrap();
    assert!(instance_path.starts_with(worktrees_root));
    assert_ne!(instance_path, workspace.canonicalize().unwrap());
    assert_eq!(
        run_record.workspace_mode,
        Some(WorkspaceMode::WorktreePerTask)
    );
    assert!(!run_record.is_security_boundary);
    assert_eq!(
        std::fs::read_to_string(pwd_capture).unwrap().trim(),
        instance_path.to_str().unwrap()
    );
    assert_eq!(
        run_record.route_key,
        "cli:did:agent:alice-coder:controller-scope:v1:test-alice-anpclaw-com:no-conversation:message-run"
    );
}

#[cfg(unix)]
#[test]
fn codex_driver_nonzero_exit_does_not_forge_success_final() {
    use std::os::unix::fs::PermissionsExt;

    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    config.ensure_state_layout().unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();
    let mut profile = profile(root.path().join("workspace"));
    profile.workspace_mode = Some(WorkspaceMode::RouteRoot);
    upsert_test_cli_profile(&state, &profile.runtime_profile_id, "codex");
    std::fs::create_dir_all(profile.workspace_root.as_ref().unwrap()).unwrap();

    let fake_codex = root.path().join("codex-fail");
    let output_dir = root.path().join("codex-fail-output");
    std::fs::write(
        &fake_codex,
        r#"#!/bin/sh
if [ "${1-}" = "--version" ]; then
  echo "codex-cli 9.9.9"
  exit 0
fi
cat >/dev/null
echo "codex failed" >&2
exit 42
"#,
    )
    .unwrap();
    let mut permissions = std::fs::metadata(&fake_codex).unwrap().permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(&fake_codex, permissions).unwrap();

    let mut driver_config = codex_config(fake_codex);
    driver_config.output_dir = Some(output_dir.clone());
    let plugin = GenericCliRuntimePlugin::new(CodexDriver::new(driver_config).unwrap());
    let outbox = MemoryRuntimeOutbox::default();
    let result = run_controller_text_task_with_config(
        &config,
        &state,
        &profile,
        &plugin,
        &outbox,
        ControllerTextMessage {
            message_id: "msg_codex_failed".to_string(),
            conversation_id: Some("conv_codex_failed".to_string()),
            sender_did: "did:human:alice".to_string(),
            requester_user_id: None,
            requester_full_handle: None,
            trigger_kind: awiki_deamon::runtime::RuntimeTaskTriggerKind::ControllerDirect,
            invocation_authority: awiki_deamon::runtime::RuntimeInvocationAuthority::Controller,
            target_agent_did: "did:agent:alice-coder".to_string(),
            text: "run failing codex driver".to_string(),
        },
    )
    .unwrap();

    assert_eq!(result.run.status, RuntimeRunStatus::Failed);
    assert_eq!(result.launch_outcome.status, RuntimeRunStatus::Failed);
    assert_eq!(result.launch_outcome.exit_code, Some(42));
    assert!(result.launch_outcome.callbacks.is_empty());
    let records = outbox.records();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].kind, OutboxRecordKind::Status);
    assert_eq!(records[0].state.as_deref(), Some("running"));
    assert_eq!(records[1].kind, OutboxRecordKind::Status);
    assert_eq!(records[1].state.as_deref(), Some("failed"));
    assert_eq!(
        records[1].last_error_code.as_deref(),
        Some("codex_cli_failed")
    );
    assert_eq!(
        records[1].last_error_summary.as_deref(),
        Some("Codex CLI exited with status 42")
    );
    assert_eq!(
        records[1].metadata.as_ref().unwrap()["next_action"].as_str(),
        Some("manual_review_required")
    );
    assert_eq!(
        records[1].metadata.as_ref().unwrap()["failed_message_recovery"].as_str(),
        Some("unsupported")
    );
    assert!(std::fs::read_to_string(output_dir.join("codex-stderr.log"))
        .unwrap()
        .contains("codex failed"));
    let run_record = state.load_cli_driver_run(&result.run.run_id).unwrap();
    let route = state
        .load_cli_route_session(&run_record.route_key)
        .unwrap()
        .unwrap();
    assert_eq!(route.status, "failed");
    assert_eq!(route.last_error_code.as_deref(), Some("codex_cli_failed"));
    assert_eq!(route.last_message_id, None);
}

#[cfg(unix)]
#[test]
fn codex_driver_timeout_marks_failed_and_releases_route_without_final() {
    use std::os::unix::fs::PermissionsExt;
    use std::time::Duration;

    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    config.ensure_state_layout().unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();
    let mut profile = profile(root.path().join("workspace"));
    profile.workspace_mode = Some(WorkspaceMode::RouteRoot);
    upsert_test_cli_profile(&state, &profile.runtime_profile_id, "codex");
    std::fs::create_dir_all(profile.workspace_root.as_ref().unwrap()).unwrap();

    let fake_codex = root.path().join("codex-timeout");
    std::fs::write(
        &fake_codex,
        r#"#!/bin/sh
if [ "${1-}" = "--version" ]; then
  echo "codex-cli 9.9.9"
  exit 0
fi
cat >/dev/null
sleep 10
"#,
    )
    .unwrap();
    let mut permissions = std::fs::metadata(&fake_codex).unwrap().permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(&fake_codex, permissions).unwrap();

    let mut driver_config = codex_config(fake_codex);
    driver_config.output_dir = Some(root.path().join("codex-timeout-output"));
    driver_config.run_timeout = Duration::from_millis(50);
    let plugin = GenericCliRuntimePlugin::new(CodexDriver::new(driver_config).unwrap());
    let outbox = MemoryRuntimeOutbox::default();
    let result = run_controller_text_task_with_config(
        &config,
        &state,
        &profile,
        &plugin,
        &outbox,
        ControllerTextMessage {
            message_id: "msg_codex_timeout".to_string(),
            conversation_id: Some("direct:did:human:bob".to_string()),
            sender_did: "did:human:alice".to_string(),
            requester_user_id: None,
            requester_full_handle: None,
            trigger_kind: awiki_deamon::runtime::RuntimeTaskTriggerKind::ControllerDirect,
            invocation_authority: awiki_deamon::runtime::RuntimeInvocationAuthority::Controller,
            target_agent_did: profile.agent_did.clone(),
            text: "run hanging codex driver".to_string(),
        },
    )
    .unwrap();

    assert_eq!(result.run.status, RuntimeRunStatus::Failed);
    assert_eq!(result.launch_outcome.status, RuntimeRunStatus::Failed);
    assert_eq!(
        result.launch_outcome.metadata["error_code"].as_str(),
        Some("codex_cli_timeout")
    );
    assert_eq!(
        result.launch_outcome.metadata["process"]["timed_out"].as_bool(),
        Some(true)
    );
    let records = outbox.records();
    assert_eq!(records[1].kind, OutboxRecordKind::Status);
    assert_eq!(records[1].state.as_deref(), Some("failed"));
    assert_eq!(
        records[1].last_error_code.as_deref(),
        Some("codex_cli_timeout")
    );
    assert!(!records
        .iter()
        .any(|record| record.kind == OutboxRecordKind::Final));

    let run_record = state.load_cli_driver_run(&result.run.run_id).unwrap();
    assert_eq!(run_record.native_session_id, None);
    assert_eq!(run_record.status, "failed");
    let route = state
        .load_cli_route_session(&run_record.route_key)
        .unwrap()
        .unwrap();
    assert_eq!(route.status, "failed");
    assert_eq!(route.lock_run_id, None);
    assert_eq!(route.native_session_id, None);
    assert_eq!(route.last_message_id, None);
    assert_eq!(route.last_error_code.as_deref(), Some("codex_cli_timeout"));
    assert_eq!(
        state
            .count_cli_runtime_locks(None, Some(&profile.runtime_profile_id), None, false)
            .unwrap(),
        0
    );
}

#[test]
fn controller_text_route_preserves_verified_current_sender_did() {
    let profile = profile(std::env::temp_dir().join("awiki-workspace"));
    let verified_sender = VerifiedControllerSender {
        controller_user_id: profile.controller_user_id.clone(),
        controller_full_handle: profile.controller_full_handle.clone(),
        controller_scope_key: profile.controller_scope_key.clone(),
        controller_did: "did:human:alice-new".to_string(),
        sender_did: "did:human:alice-new".to_string(),
    };
    let task = route_controller_text_task_with_verified_sender(
        &profile,
        &verified_sender,
        ControllerTextMessage {
            message_id: "msg_002".to_string(),
            conversation_id: None,
            sender_did: "did:human:alice-new".to_string(),
            requester_user_id: None,
            requester_full_handle: None,
            trigger_kind: awiki_deamon::runtime::RuntimeTaskTriggerKind::ControllerDirect,
            invocation_authority: awiki_deamon::runtime::RuntimeInvocationAuthority::Controller,
            target_agent_did: "did:agent:alice-coder".to_string(),
            text: "请执行".to_string(),
        },
    )
    .unwrap();

    assert_eq!(task.controller_did, "did:human:alice-new");
    assert_eq!(task.sender_did, "did:human:alice-new");
    assert_eq!(task.controller_scope_key, profile.controller_scope_key);
}

#[test]
fn workspace_modes_document_security_boundary() {
    assert!(!WorkspaceMode::SharedRoot.is_security_boundary());
    assert!(!WorkspaceMode::WorktreePerTask.is_security_boundary());
    assert!(WorkspaceMode::Container.is_security_boundary());
    assert!(WorkspaceMode::Sandbox.is_security_boundary());
    assert!(WorkspaceMode::SharedRoot
        .isolation_note()
        .contains("不是硬隔离"));
}

#[test]
fn runtime_profile_requires_complete_workspace_binding() {
    let mut profile = profile(std::env::temp_dir().join("awiki-workspace"));
    profile.workspace_mode = None;

    let error = profile.validate().unwrap_err();
    assert!(error.to_string().contains("provided together"));
}

#[test]
fn runtime_callback_debug_redacts_runtime_rpc_token() {
    let request = awiki_deamon::local_rpc::RuntimeRpcRequest {
        runtime_rpc_token: "rtok_debug_secret_value_123456789".to_string(),
        method: "task.status".to_string(),
        params: json!({ "state": "running" }),
        debug: None,
    };

    assert!(!format!("{request:?}").contains("rtok_debug_secret_value_123456789"));
}

#[test]
fn generic_cli_invocation_debug_redacts_task_text_and_token() {
    let invocation = awiki_deamon::plugins::generic_cli::GenericCliInvocation {
        run_id: "run_debug".to_string(),
        task_id: "task_debug".to_string(),
        message_id: "debug".to_string(),
        conversation_id: Some("conv_debug".to_string()),
        preferred_language: "zh-Hans".to_string(),
        context: GenericCliInvocationContext {
            controller_did: "did:human:alice".to_string(),
            sender_did: "did:human:alice".to_string(),
            requester_did: "did:human:alice".to_string(),
            requester_full_handle: Some("alice.anpclaw.com".to_string()),
            trigger_kind: "controller_direct".to_string(),
            conversation_scope_kind: "controller_private".to_string(),
            conversation_scope_key: "controller:controller-scope:v1:test-alice-anpclaw-com"
                .to_string(),
            invocation_authority: "controller".to_string(),
            controller_verified: true,
            sender_trust_level: "verified_controller".to_string(),
            allowed_actions: vec![
                "report-status".to_string(),
                "reply-in-current-direct-via-final".to_string(),
                "outbound-send".to_string(),
            ],
        },
        task_text: "secret prompt rtok_debug_secret_value_123456789".to_string(),
        agent_did: "did:agent:debug".to_string(),
        runtime_profile_id: "profile_debug".to_string(),
        workspace_root: None,
        workspace_instance: None,
        route_session: None,
        runtime_temp_dir: None,
        runtime_rpc_token: "rtok_debug_secret_value_123456789".to_string(),
        local_socket_path: None,
        callbacks: Vec::new(),
    };

    let debug = format!("{invocation:?}");
    assert!(debug.contains("<redacted-task-text>"));
    assert!(debug.contains("<redacted>"));
    assert!(!debug.contains("secret prompt"));
    assert!(!debug.contains("rtok_debug_secret_value_123456789"));
}

fn generic_cli_invocation_for_process_test(
    root: &std::path::Path,
    workspace_root: &std::path::Path,
) -> awiki_deamon::plugins::generic_cli::GenericCliInvocation {
    awiki_deamon::plugins::generic_cli::GenericCliInvocation {
        run_id: "run_process_group".to_string(),
        task_id: "task_process_group".to_string(),
        message_id: "msg_process_group".to_string(),
        conversation_id: Some("direct:did:human:bob".to_string()),
        preferred_language: "zh-Hans".to_string(),
        context: GenericCliInvocationContext {
            controller_did: "did:human:alice".to_string(),
            sender_did: "did:human:bob".to_string(),
            requester_did: "did:human:bob".to_string(),
            requester_full_handle: Some("bob.anpclaw.com".to_string()),
            trigger_kind: "external_direct".to_string(),
            conversation_scope_kind: "direct".to_string(),
            conversation_scope_key: "user:user-bob:handle:bob.anpclaw.com".to_string(),
            invocation_authority: "requester".to_string(),
            controller_verified: false,
            sender_trust_level: "authorized_external_direct_requester".to_string(),
            allowed_actions: vec![
                "report-status".to_string(),
                "reply-in-current-direct-via-final".to_string(),
            ],
        },
        task_text: "process group test".to_string(),
        agent_did: "did:agent:alice-coder".to_string(),
        runtime_profile_id: "profile_generic_cli_1".to_string(),
        workspace_root: Some(workspace_root.to_path_buf()),
        workspace_instance: None,
        route_session: None,
        runtime_temp_dir: Some(root.join("runtime-tmp")),
        runtime_rpc_token: "rtok_process_group_secret".to_string(),
        local_socket_path: Some(root.join("awiki-deamon.sock")),
        callbacks: Vec::new(),
    }
}

fn captured_value<'a>(capture: &'a str, key: &str) -> &'a str {
    capture
        .lines()
        .find_map(|line| line.strip_prefix(&format!("{key}=")))
        .unwrap_or_else(|| panic!("missing captured value for {key} in {capture:?}"))
        .trim()
}
