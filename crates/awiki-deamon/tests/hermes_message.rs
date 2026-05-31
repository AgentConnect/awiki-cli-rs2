use awiki_deamon::inbox::ControllerTextMessage;
use awiki_deamon::outbox::{MemoryRuntimeOutbox, OutboxRecordKind};
use awiki_deamon::plugins::hermes::{
    FakeHermesBehavior, FakeHermesGateway, HermesPromptWrapper, HermesRuntimePlugin,
    HERMES_RUNTIME_PLUGIN_ID,
};
use awiki_deamon::runtime::host::run_controller_text_task;
use awiki_deamon::runtime::{RuntimeAgentProfile, RuntimeRun, RuntimeRunStatus, RuntimeTask};
use awiki_deamon::state::HermesProfileRecord;
use awiki_deamon::workspace::WorkspaceMode;
use awiki_deamon::{DaemonConfig, DaemonState};

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
        awiki_skills_version: "awiki-hermes-skills-v1".to_string(),
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
    assert_eq!(result.launch_outcome.callbacks.len(), 2);
    assert_eq!(result.run.status, RuntimeRunStatus::Finished);

    let records = outbox.records();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].kind, OutboxRecordKind::Status);
    assert_eq!(records[0].state.as_deref(), Some("running"));
    assert_eq!(records[1].kind, OutboxRecordKind::Final);
    assert_eq!(records[1].state.as_deref(), Some("finished"));

    let prompts = gateway.submitted_prompts();
    assert_eq!(prompts.len(), 1);
    assert_eq!(prompts[0].run_id, "run_task_msg_001");
    assert!(prompts[0].prompt.contains("controller_verified: true"));
    assert!(prompts[0].prompt.contains("run_id: run_task_msg_001"));
    assert!(prompts[0].prompt.contains("finish-message"));
    assert!(prompts[0].prompt.contains("send-message"));
    assert!(prompts[0].prompt.contains("请处理这条消息"));
    assert!(!prompts[0].prompt.contains("auth_private_key"));
    assert!(!prompts[0].prompt.contains("jwt_token"));
    assert!(!prompts[0].prompt.contains("runtime_rpc_token"));
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
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].kind, OutboxRecordKind::Status);
    assert_eq!(records[0].state.as_deref(), Some("failed"));
}

#[test]
fn hermes_message_non_controller_text_is_rejected_before_gateway() {
    let (root, state) = fixture();
    let outbox = MemoryRuntimeOutbox::default();
    let gateway = FakeHermesGateway::default();
    let plugin = HermesRuntimePlugin::new(
        gateway.clone(),
        hermes_record(root.path().join("runtime/hermes/profile")),
    );
    let profile = profile(root.path().join("workspace"));

    let error = run_controller_text_task(
        &state,
        &profile,
        &plugin,
        &outbox,
        ControllerTextMessage {
            message_id: "msg_unauthorized".to_string(),
            conversation_id: None,
            sender_did: "did:human:bob".to_string(),
            target_agent_did: "did:agent:hermes".to_string(),
            text: "越权执行".to_string(),
        },
    )
    .unwrap_err();

    assert!(error.to_string().contains("controller_did"));
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
