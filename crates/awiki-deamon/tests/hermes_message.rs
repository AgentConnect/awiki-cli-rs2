use awiki_deamon::inbox::ControllerTextMessage;
use awiki_deamon::outbox::{MemoryRuntimeOutbox, OutboxRecordKind, RuntimeMessageSecurity};
use awiki_deamon::plugins::hermes::{
    reset_hermes_session_by_route, FakeHermesBehavior, FakeHermesGateway, HermesPromptWrapper,
    HermesRuntimePlugin, HERMES_RUNTIME_PLUGIN_ID,
};
use awiki_deamon::runtime::host::run_controller_text_task;
use awiki_deamon::runtime::{RuntimeAgentProfile, RuntimeRun, RuntimeRunStatus, RuntimeTask};
use awiki_deamon::state::{HermesProfileRecord, HermesSessionRoute};
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
    assert_eq!(result.run.status, RuntimeRunStatus::Pending);
    let records = outbox.records();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].kind, OutboxRecordKind::Message);
    assert_eq!(records[0].recipient.as_deref(), Some("did:human:alice"));
    assert_eq!(records[0].text.as_deref(), Some("Hermes says hello"));
    assert_eq!(
        records[0].security,
        Some(RuntimeMessageSecurity::DefaultPlain)
    );
}

#[test]
fn hermes_message_send_message_callback_respects_controller_recipient_scope() {
    let (root, state) = fixture();
    let outbox = MemoryRuntimeOutbox::default();
    let gateway = FakeHermesGateway::with_behavior(FakeHermesBehavior::SendMessage);
    let plugin = HermesRuntimePlugin::new(
        gateway,
        hermes_record(root.path().join("runtime/hermes/profile")),
    );
    let mut profile = profile(root.path().join("workspace"));
    profile.controller_did = "did:human:charlie".to_string();

    let error = run_controller_text_task(
        &state,
        &profile,
        &plugin,
        &outbox,
        ControllerTextMessage {
            message_id: "msg_send_blocked".to_string(),
            conversation_id: Some("direct:did:human:charlie".to_string()),
            sender_did: "did:human:charlie".to_string(),
            target_agent_did: "did:agent:hermes".to_string(),
            text: "尝试给非 controller 发消息".to_string(),
        },
    )
    .unwrap_err();
    let error_chain = error
        .chain()
        .map(|cause| cause.to_string())
        .collect::<Vec<_>>()
        .join(" | ");

    assert!(error_chain.contains("runtime callback"));
    assert!(error_chain.contains("recipient not allowed"));
    assert!(outbox.records().is_empty());
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
        "did:human:alice",
        Some("direct:did:human:alice".to_string()),
        "conversation",
    );
    let active = state
        .load_active_hermes_session_by_route(&route)
        .unwrap()
        .unwrap();
    assert_eq!(
        active.hermes_session_id,
        "fake-session-hermes:did:agent:hermes:did:human:alice:direct:did:human:alice:conversation"
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
        "did:human:alice",
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
