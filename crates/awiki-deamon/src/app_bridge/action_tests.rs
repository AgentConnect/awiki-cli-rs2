use tempfile::TempDir;

use super::*;
use crate::config::DaemonConfig;

#[test]
fn allowed_contact_write_action_queues_confirmation_request() {
    let fixture = fixture(json!({
        "schema": APP_CAPABILITIES_SCHEMA,
        "capabilities": MVP_ALLOWED_ACTIONS,
        "require_confirmation_for_write_actions": true
    }));
    let outcome = queue_runtime_app_action_request(
        &fixture.state,
        &fixture.context,
        &json!({
            "action_id": "act_contact_note_1",
            "action": "contact.update_note",
            "source_message_id": "msg_1",
            "conversation_id": "direct:did:human:bob",
            "args": {
                "contact_did": "did:human:bob",
                "note": "Follow up about the launch"
            }
        }),
    )
    .unwrap();

    assert_eq!(outcome.state, "requires_confirmation");
    assert!(outcome.requires_confirmation);
    let record = fixture
        .state
        .load_message_sync_outbox(&outcome.idempotency_key)
        .unwrap()
        .unwrap();
    assert_eq!(record.payload_json["schema"], APP_ACTION_SCHEMA);
    assert_eq!(record.payload_json["action"], "contact.update_note");
    assert_eq!(record.payload_json["state"], "requires_confirmation");
    assert_eq!(record.payload_json["requires_confirmation"], true);
    assert_eq!(record.payload_json["daemon_agent_did"], "did:agent:daemon");
    assert_eq!(record.payload_json["runtime_agent_did"], "did:agent:hermes");
    assert_eq!(record.payload_json["args"]["contact_did"], "did:human:bob");
}

#[test]
fn high_risk_action_is_rejected_and_result_is_queued() {
    let fixture = fixture(json!({
        "schema": APP_CAPABILITIES_SCHEMA,
        "capabilities": MVP_ALLOWED_ACTIONS
    }));
    let error = queue_runtime_app_action_request(
        &fixture.state,
        &fixture.context,
        &json!({
            "action_id": "act_send_1",
            "action": "message.send",
            "args": {
                "to": "did:human:bob",
                "text": "send this"
            }
        }),
    )
    .unwrap_err();

    assert!(error.to_string().contains("MVP allowlist"));
    let record = fixture
        .state
        .load_message_sync_outbox("app-action:did:human:alice:run_user_msg_1:act_send_1:rejected")
        .unwrap()
        .unwrap();
    assert_eq!(record.payload_json["schema"], APP_ACTION_RESULT_SCHEMA);
    assert_eq!(record.payload_json["action"], "message.send");
    assert_eq!(record.payload_json["state"], "rejected");
    assert_eq!(record.payload_json["daemon_agent_did"], "did:agent:daemon");
    assert_eq!(record.payload_json["runtime_agent_did"], "did:agent:hermes");
    assert_eq!(record.payload_json["error_code"], "action_not_allowed");
}

#[test]
fn binding_capability_policy_restricts_mvp_action_subset() {
    let fixture = fixture(json!({
        "schema": APP_CAPABILITIES_SCHEMA,
        "capabilities": ["message.summarize_plain"]
    }));
    let error = queue_runtime_app_action_request(
        &fixture.state,
        &fixture.context,
        &json!({
            "action_id": "act_read_contact_1",
            "action": "contact.read",
            "args": {
                "contact_did": "did:human:bob"
            }
        }),
    )
    .unwrap_err();

    assert!(error.to_string().contains("not enabled"));
}

#[test]
fn empty_explicit_capabilities_disable_app_actions() {
    let fixture = fixture(json!({
        "schema": APP_CAPABILITIES_SCHEMA,
        "capabilities": []
    }));
    let error = queue_runtime_app_action_request(
        &fixture.state,
        &fixture.context,
        &json!({
            "action_id": "act_summary_1",
            "action": "message.summarize_plain",
            "args": {"message_id": "msg_1"}
        }),
    )
    .unwrap_err();

    assert!(error.to_string().contains("not enabled"));
}

#[test]
fn missing_capability_policy_does_not_default_to_all_actions_for_new_binding() {
    let fixture = fixture(json!({}));
    let error = queue_runtime_app_action_request(
        &fixture.state,
        &fixture.context,
        &json!({
            "action_id": "act_summary_1",
            "action": "message.summarize_plain",
            "args": {"message_id": "msg_1"}
        }),
    )
    .unwrap_err();

    assert!(error.to_string().contains("not enabled"));
}

#[test]
fn legacy_binding_without_capability_schema_can_use_desired_allowed_actions() {
    let fixture = fixture_with_desired_actions(json!({}), json!(["message.summarize_plain"]));
    let outcome = queue_runtime_app_action_request(
        &fixture.state,
        &fixture.context,
        &json!({
            "action_id": "act_summary_1",
            "action": "message.summarize_plain",
            "args": {"message_id": "msg_1"}
        }),
    )
    .unwrap();

    assert_eq!(outcome.state, "requested");
    assert!(!outcome.requires_confirmation);
}

#[test]
fn app_capabilities_and_result_payloads_parse_and_reject_private_state() {
    let capabilities = parse_app_capabilities_payload(json!({
        "schema": APP_CAPABILITIES_SCHEMA,
        "capabilities": ["message.summarize_plain", "contact.update_note"],
        "require_confirmation_for_write_actions": true
    }))
    .unwrap();
    assert_eq!(capabilities.capabilities.len(), 2);

    let result = parse_app_action_result_payload(json!({
        "schema": APP_ACTION_RESULT_SCHEMA,
        "action_id": "act_1",
        "action": "message.create_draft",
        "state": "succeeded",
        "result": {"draft_text": "Looks good"}
    }))
    .unwrap();
    assert_eq!(result.state, "succeeded");

    let private = parse_app_action_result_payload(json!({
        "schema": APP_ACTION_RESULT_SCHEMA,
        "action_id": "act_2",
        "action": "message.create_draft",
        "state": "succeeded",
        "result": {"private_key": "secret"}
    }))
    .unwrap_err();
    assert!(private.to_string().contains("forbidden private state"));
}

struct TestFixture {
    _root: TempDir,
    state: DaemonState,
    context: AuthorizedRuntimeContext,
}

fn fixture(capability_policy_json: Value) -> TestFixture {
    fixture_with_desired_actions(capability_policy_json, Value::Null)
}

fn fixture_with_desired_actions(
    capability_policy_json: Value,
    allowed_actions: Value,
) -> TestFixture {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let state = DaemonState::open_with_root_key_bytes(&config, [34; 32]);
    state.initialize().unwrap();
    let mut desired_agent_json = json!({
        "role": "app_message_handler"
    });
    if !allowed_actions.is_null() {
        desired_agent_json["allowed_actions"] = allowed_actions;
    }
    let binding = AppPersonalAgentBindingRecord {
        binding_id: "app-personal-agent:did:human:alice:app_1".to_string(),
        user_did: "did:human:alice".to_string(),
        inbox_auth_verification_method: "did:human:alice#daemon-key-1".to_string(),
        app_instance_id: "app_1".to_string(),
        bootstrap_id: "boot_1".to_string(),
        idempotency_key: "personal-agent-bootstrap:did:human:alice:app_1".to_string(),
        daemon_agent_did: "did:agent:daemon".to_string(),
        runtime_agent_did: "did:agent:hermes".to_string(),
        runtime_profile_id: "profile_hermes".to_string(),
        role: "app_message_handler".to_string(),
        desired_agent_json,
        capability_policy_json,
        status: "personal_agent_ready".to_string(),
        created_at_ms: 0,
        updated_at_ms: 0,
        revoked_at_ms: None,
    };
    use crate::runtime::{
        RuntimeAgentProfile, RuntimeConversationScope, RuntimeInvocationAuthority, RuntimeRun,
        RuntimeRunStatus, RuntimeTask, RuntimeTaskTriggerKind,
    };
    let profile = RuntimeAgentProfile {
        agent_did: binding.runtime_agent_did.clone(),
        agent_handle: "hermes".into(),
        runtime_profile_id: binding.runtime_profile_id.clone(),
        runtime_plugin_id: "acp".into(),
        controller_did: binding.user_did.clone(),
        controller_user_id: "alice".into(),
        controller_full_handle: "alice.example.com".into(),
        controller_scope_key: "controller-scope:v1:alice:alice.example.com".into(),
        display_name: None,
        preferred_language: "en".into(),
        workspace_id: None,
        workspace_root: None,
        workspace_mode: None,
    };
    state.upsert_runtime_agent_profile(&profile).unwrap();
    state
        .upsert_cli_runtime_profile(
            &crate::state::CliRuntimeProfileRecord::for_driver(
                &profile.runtime_profile_id,
                "hermes",
            )
            .unwrap(),
        )
        .unwrap();
    state
        .insert_runtime_task(&RuntimeTask {
            task_id: "task_1".into(),
            agent_did: profile.agent_did.clone(),
            agent_handle: profile.agent_handle.clone(),
            controller_did: profile.controller_did.clone(),
            controller_user_id: profile.controller_user_id.clone(),
            controller_full_handle: profile.controller_full_handle.clone(),
            controller_scope_key: profile.controller_scope_key.clone(),
            sender_did: "did:human:bob".into(),
            requester_did: "did:human:bob".into(),
            requester_user_id: Some("bob".into()),
            requester_full_handle: Some("bob.example.com".into()),
            trigger_kind: RuntimeTaskTriggerKind::DelegatedDirect,
            conversation_scope: RuntimeConversationScope::direct("bob", "bob.example.com").unwrap(),
            invocation_authority: RuntimeInvocationAuthority::Requester,
            reply_recipient_did: binding.user_did.clone(),
            conversation_id: Some("direct:did:human:bob".into()),
            text: json!({"source_message_id":"msg_1"}).to_string(),
        })
        .unwrap();
    state
        .insert_runtime_run(&RuntimeRun {
            run_id: "run_user_msg_1".into(),
            task_id: "task_1".into(),
            agent_did: profile.agent_did.clone(),
            runtime_profile_id: profile.runtime_profile_id.clone(),
            runtime_plugin_id: "acp".into(),
            workspace_id: None,
            status: RuntimeRunStatus::Running,
        })
        .unwrap();
    state.upsert_app_personal_agent_binding(&binding).unwrap();
    let context = AuthorizedRuntimeContext {
        token_id: "token_1".to_string(),
        agent_did: binding.runtime_agent_did.clone(),
        runtime_profile_id: binding.runtime_profile_id.clone(),
        run_id: "run_user_msg_1".to_string(),
        method: crate::security::runtime_token::RpcMethod::AppActionRequest,
    };
    TestFixture {
        _root: root,
        state,
        context,
    }
}
#[test]
fn app_action_uses_the_accepted_source_and_rejects_route_override_or_revocation() {
    let f =
        fixture(json!({"schema":APP_CAPABILITIES_SCHEMA,"capabilities":["message.create_draft"]}));
    let request =
        json!({"action":"message.create_draft","args":{"draft_text":"A suggested reply"}});
    let first = queue_runtime_app_action_request(&f.state, &f.context, &request).unwrap();
    let again = queue_runtime_app_action_request(&f.state, &f.context, &request).unwrap();
    assert_eq!(first.idempotency_key, again.idempotency_key);
    let record = f
        .state
        .load_message_sync_outbox(&first.idempotency_key)
        .unwrap()
        .unwrap();
    assert_eq!(record.payload_json["source_message_id"], "msg_1");
    assert_eq!(
        record.payload_json["conversation_id"],
        "direct:did:human:bob"
    );
    for field in ["source_message_id", "conversation_id"] {
        let mut wrong = request.clone();
        wrong[field] = json!("foreign");
        assert_eq!(
            queue_runtime_app_action_request(&f.state, &f.context, &wrong)
                .unwrap_err()
                .to_string(),
            "app_action_source_mismatch"
        );
    }
    f.state
        .update_app_personal_agent_binding_status_by_runtime(
            &f.context.agent_did,
            "personal_agent_disabled",
            true,
        )
        .unwrap();
    assert!(queue_runtime_app_action_request(&f.state, &f.context, &request).is_err());
}
