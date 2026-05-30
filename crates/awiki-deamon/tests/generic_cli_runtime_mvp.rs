use awiki_deamon::inbox::{route_controller_text_task, ControllerTextMessage};
use awiki_deamon::outbox::{MemoryRuntimeOutbox, OutboxRecordKind};
use awiki_deamon::plugins::generic_cli::{GenericCliRuntimePlugin, TestGenericCliDriver};
use awiki_deamon::runtime::host::run_controller_text_task;
use awiki_deamon::runtime::{RuntimeAgentProfile, RuntimeRunStatus};
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
