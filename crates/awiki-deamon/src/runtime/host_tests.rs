use super::*;
use crate::agent::{AgentDefinition, AgentKind};
use crate::outbox::MemoryRuntimeOutbox;
use crate::runtime::{RuntimeInvocationAuthority, RuntimeRunStatus, RuntimeTaskTriggerKind};

#[test]
fn runtime_final_outbox_is_fenced_after_controller_identity_change() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();
    let daemon = AgentDefinition {
        agent_did: "did:agent:daemon".to_string(),
        handle: "alice-daemon".to_string(),
        agent_kind: AgentKind::Daemon,
        controller_user_id: "user-alice".to_string(),
        controller_full_handle: "alice.anpclaw.com".to_string(),
        controller_scope_key: "controller-scope:v1:test-alice".to_string(),
        controller_did: "did:human:alice".to_string(),
        runtime_plugin_id: None,
        runtime_profile_id: None,
        workspace_id: None,
        policy_id: "default".to_string(),
        local_agent_db_path: "agents/daemon/agent.db".to_string(),
        message_db_path: "agents/daemon/messages.db".to_string(),
        status: "active".to_string(),
    };
    let profile = RuntimeAgentProfile {
        agent_did: "did:agent:hermes".to_string(),
        agent_handle: "alice-hermes".to_string(),
        controller_user_id: daemon.controller_user_id.clone(),
        controller_full_handle: daemon.controller_full_handle.clone(),
        controller_scope_key: daemon.controller_scope_key.clone(),
        controller_did: daemon.controller_did.clone(),
        runtime_profile_id: "profile-hermes".to_string(),
        runtime_plugin_id: "hermes".to_string(),
        display_name: None,
        preferred_language: "en".to_string(),
        workspace_id: None,
        workspace_root: None,
        workspace_mode: None,
    };
    let runtime = AgentDefinition {
        agent_did: profile.agent_did.clone(),
        handle: profile.agent_handle.clone(),
        agent_kind: AgentKind::Runtime,
        controller_user_id: profile.controller_user_id.clone(),
        controller_full_handle: profile.controller_full_handle.clone(),
        controller_scope_key: profile.controller_scope_key.clone(),
        controller_did: profile.controller_did.clone(),
        runtime_plugin_id: Some(profile.runtime_plugin_id.clone()),
        runtime_profile_id: Some(profile.runtime_profile_id.clone()),
        workspace_id: None,
        policy_id: "default".to_string(),
        local_agent_db_path: "agents/hermes/agent.db".to_string(),
        message_db_path: "agents/hermes/messages.db".to_string(),
        status: "active".to_string(),
    };
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
    let run = RuntimeRun {
        run_id: "run-before-controller-change".to_string(),
        task_id: "task-before-controller-change".to_string(),
        agent_did: runtime.agent_did.clone(),
        runtime_profile_id: profile.runtime_profile_id.clone(),
        runtime_plugin_id: profile.runtime_plugin_id.clone(),
        workspace_id: None,
        status: RuntimeRunStatus::Running,
    };
    state
        .insert_runtime_task(&RuntimeTask {
            task_id: run.task_id.clone(),
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
            conversation_scope: crate::runtime::RuntimeConversationScope::ControllerPrivate {
                controller_scope_key: daemon.controller_scope_key.clone(),
            },
            invocation_authority: RuntimeInvocationAuthority::Controller,
            reply_recipient_did: daemon.controller_did.clone(),
            conversation_id: Some("direct:did:human:alice".to_string()),
            text: "must remain isolated".to_string(),
        })
        .unwrap();
    state.insert_runtime_run(&run).unwrap();
    let pending = runtime_final_outbox_record(
        &profile,
        &daemon.controller_did,
        &daemon.controller_did,
        &run,
        Some("direct:did:human:alice"),
        "must remain isolated",
        "test",
    )
    .unwrap();
    let payload = runtime_final_payload(&state, &pending)
        .unwrap()
        .expect("direct Runtime final must carry reply correlation");
    assert_eq!(payload["text"], "must remain isolated");
    assert_eq!(
        payload["annotations"]["awiki_reply_to_message_id"],
        "task-before-controller-change"
    );
    assert_eq!(payload["mentions"], serde_json::json!([]));
    state.upsert_runtime_final_outbox_pending(&pending).unwrap();
    crate::agent_status::record_controller_identity_changed(
        &state,
        &daemon.agent_did,
        "test_authoritative_status",
    )
    .unwrap_err();
    let outbox = MemoryRuntimeOutbox::default();

    let sent = flush_runtime_final_outbox(&state, &outbox, 8).unwrap();

    assert_eq!(sent, 0);
    assert!(outbox.records().is_empty());
    let stored = state
        .load_runtime_final_outbox_by_run(&run.run_id)
        .unwrap()
        .unwrap();
    assert_eq!(stored.status, "failed_terminal");
    assert_eq!(
        stored.last_error_code.as_deref(),
        Some("controller_identity_changed")
    );
    assert!(state
        .list_due_runtime_final_outbox(i64::MAX, 8)
        .unwrap()
        .is_empty());
    assert_eq!(flush_runtime_final_outbox(&state, &outbox, 8).unwrap(), 0);
    assert!(outbox.records().is_empty());
    assert_eq!(stored.controller_did, "did:human:alice");
    assert_eq!(stored.recipient_did, "did:human:alice");
}
