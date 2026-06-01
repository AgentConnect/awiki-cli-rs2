use awiki_deamon::inbox::{route_controller_text_task, ControllerTextMessage};
use awiki_deamon::outbox::{MemoryRuntimeOutbox, OutboxRecordKind};
use awiki_deamon::plugins::generic_cli::{GenericCliRuntimePlugin, TestGenericCliDriver};
use awiki_deamon::runtime::host::run_controller_text_task;
use awiki_deamon::runtime::{
    RuntimeAgentProfile, RuntimeInstallStatus, RuntimeLaunchContext, RuntimeLaunchOutcome,
    RuntimePlugin, RuntimeRunStatus,
};
use awiki_deamon::state::CliRuntimeProfileRecord;
use awiki_deamon::workspace::WorkspaceMode;
use awiki_deamon::{DaemonConfig, DaemonState};
use rusqlite::Connection;
use serde_json::json;

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
        agent_did: "did:agent:alice-coder".to_string(),
        controller_did: "did:human:alice".to_string(),
        runtime_profile_id: "profile_generic_cli_1".to_string(),
        runtime_plugin_id: "generic-cli".to_string(),
        display_name: Some("Alice Coder".to_string()),
        workspace_id: Some("workspace_awiki".to_string()),
        workspace_root: Some(workspace_root),
        workspace_mode: Some(WorkspaceMode::SharedRoot),
    }
}

#[test]
fn controller_text_task_runs_generic_cli_and_records_callbacks() {
    let (root, state) = fixture();
    let outbox = MemoryRuntimeOutbox::default();
    let plugin = GenericCliRuntimePlugin::new(TestGenericCliDriver::default());
    let profile = profile(root.path().join("workspace"));

    let result = run_controller_text_task(
        &state,
        &profile,
        &plugin,
        &outbox,
        ControllerTextMessage {
            message_id: "msg_001".to_string(),
            conversation_id: Some("conv_001".to_string()),
            sender_did: "did:human:alice".to_string(),
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

#[test]
fn failed_generic_cli_marks_run_failed_without_final_callback() {
    let (root, state) = fixture();
    let outbox = MemoryRuntimeOutbox::default();
    let plugin = GenericCliRuntimePlugin::new(TestGenericCliDriver { exit_code: 7 });
    let profile = profile(root.path().join("workspace"));

    let result = run_controller_text_task(
        &state,
        &profile,
        &plugin,
        &outbox,
        ControllerTextMessage {
            message_id: "msg_failed".to_string(),
            conversation_id: Some("conv_001".to_string()),
            sender_did: "did:human:alice".to_string(),
            target_agent_did: "did:agent:alice-coder".to_string(),
            text: "执行一个会失败的任务".to_string(),
        },
    )
    .unwrap();

    assert_eq!(result.run.status, RuntimeRunStatus::Failed);
    assert_eq!(result.launch_outcome.status, RuntimeRunStatus::Failed);
    assert_eq!(result.launch_outcome.exit_code, Some(7));

    let records = outbox.records();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].kind, OutboxRecordKind::Status);
    assert_eq!(records[0].state.as_deref(), Some("failed"));
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
            target_agent_did: "did:agent:alice-coder".to_string(),
            text: "finish once".to_string(),
        },
    )
    .unwrap();

    assert_eq!(result.run.status, RuntimeRunStatus::Finished);
    let records = outbox.records();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].kind, OutboxRecordKind::Final);
    assert_eq!(records[0].text.as_deref(), Some("first done"));
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
            target_agent_did: "did:agent:alice-coder".to_string(),
            text: "send to bob".to_string(),
        },
    )
    .unwrap();

    assert_eq!(result.run.status, RuntimeRunStatus::Finished);
    let records = outbox.records();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].kind, OutboxRecordKind::Message);
    assert_eq!(records[0].recipient.as_deref(), Some("did:human:bob"));
    assert_eq!(records[0].text.as_deref(), Some("hello from generic-cli"));
    assert_eq!(records[1].kind, OutboxRecordKind::Final);

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
            target_agent_did: "did:agent:alice-coder".to_string(),
            text: "send to mallory".to_string(),
        },
    )
    .unwrap_err();

    assert!(error.to_string().contains("runtime callback"));
    assert!(outbox.records().is_empty());
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
        Ok(awiki_deamon::plugins::generic_cli::GenericCliExit { exit_code: 0 })
    }
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

#[test]
fn non_controller_text_is_not_routed_to_runtime_task() {
    let profile = profile(std::env::temp_dir().join("awiki-workspace"));
    let error = route_controller_text_task(
        &profile,
        ControllerTextMessage {
            message_id: "msg_002".to_string(),
            conversation_id: None,
            sender_did: "did:human:bob".to_string(),
            target_agent_did: "did:agent:alice-coder".to_string(),
            text: "请执行".to_string(),
        },
    )
    .unwrap_err();

    assert!(error.to_string().contains("controller_did"));
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
