use std::time::Duration;

use awiki_deamon::acp::PLUGIN_ID;
use awiki_deamon::agent::resolve_runtime;
use awiki_deamon::controller_scope::VerifiedControllerSender;
use awiki_deamon::inbox::{
    route_controller_text_task, route_controller_text_task_with_verified_sender,
    ControllerTextMessage,
};
use awiki_deamon::runtime::RuntimeAgentProfile;
use awiki_deamon::security::runtime_token::{RpcMethod, RuntimeTokenScope};

fn profile() -> RuntimeAgentProfile {
    RuntimeAgentProfile {
        agent_did: "did:agent:hermes".to_string(),
        agent_handle: "alice-hermes".to_string(),
        controller_user_id: "user-alice".to_string(),
        controller_full_handle: "alice.anpclaw.com".to_string(),
        controller_scope_key: "controller-scope:v1:test-alice-anpclaw-com".to_string(),
        controller_did: "did:human:alice".to_string(),
        runtime_profile_id: "profile_hermes_alice".to_string(),
        runtime_plugin_id: PLUGIN_ID.to_string(),
        display_name: Some("Alice Hermes".to_string()),
        preferred_language: "zh-Hans".to_string(),
        workspace_id: None,
        workspace_root: None,
        workspace_mode: None,
    }
}

#[test]
fn all_product_types_resolve_to_acp_and_legacy_plugin_ids_cannot_create() {
    for kind in awiki_deamon::acp::SUPPORTED_DRIVERS {
        let resolution = resolve_runtime(kind, None).unwrap();
        assert_eq!(resolution.runtime_plugin_id, PLUGIN_ID);
        assert_eq!(resolution.driver_id.as_deref(), Some(kind));
        assert!(!resolution.defaulted_driver_id);
    }
    for legacy in [
        "runtime.hermes",
        "generic-cli",
        "runtime.cli.codex",
        "runtime.cli.claude-code",
    ] {
        assert!(resolve_runtime(legacy, None)
            .unwrap_err()
            .to_string()
            .contains("legacy_runtime_disabled"));
    }
}

#[test]
fn current_rpc_methods_keep_compatibility_names_without_new_message_aliases() {
    assert_eq!(
        RpcMethod::parse("task.status").unwrap().as_str(),
        "task.status"
    );
    assert_eq!(
        RpcMethod::parse("task.finish").unwrap().as_str(),
        "task.finish"
    );
    assert_eq!(RpcMethod::parse("msg.send").unwrap().as_str(), "msg.send");

    assert!(RpcMethod::parse("message.status").is_err());
    assert!(RpcMethod::parse("message.finish").is_err());
    assert!(RpcMethod::parse("task.result").is_err());
}

#[test]
fn acp_msg_send_recipient_scope_is_controlled_by_runtime_token_scope() {
    let scoped = RuntimeTokenScope::new(
        "did:agent:hermes",
        "profile_hermes_alice",
        "run_msg_001",
        vec![RpcMethod::MsgSend],
        Some(vec!["did:human:alice".to_string()]),
        Duration::from_secs(60),
    )
    .unwrap();

    assert!(scoped.allows_method(&RpcMethod::MsgSend));
    assert!(scoped.allows_recipient(Some("did:human:alice")));
    assert!(!scoped.allows_recipient(Some("did:human:bob")));
    assert!(!scoped.allows_recipient(None));

    let unrestricted = RuntimeTokenScope::new(
        "did:agent:hermes",
        "profile_hermes_alice",
        "run_msg_002",
        vec![RpcMethod::MsgSend],
        None,
        Duration::from_secs(60),
    )
    .unwrap();
    assert!(unrestricted.allows_recipient(Some("did:human:bob")));
}

#[test]
fn acp_controller_text_route_preserves_verified_sender_did() {
    let profile = profile();
    let routed = route_controller_text_task(
        &profile,
        ControllerTextMessage {
            message_id: "msg_001".to_string(),
            conversation_id: Some("direct:did:human:alice".to_string()),
            sender_did: "did:human:alice".to_string(),
            requester_user_id: None,
            requester_full_handle: None,
            trigger_kind: awiki_deamon::runtime::RuntimeTaskTriggerKind::ControllerDirect,
            invocation_authority: awiki_deamon::runtime::RuntimeInvocationAuthority::Controller,
            target_agent_did: "did:agent:hermes".to_string(),
            text: "请处理这条消息".to_string(),
        },
    )
    .unwrap();

    assert_eq!(routed.task_id, "task_msg_001");
    assert_eq!(routed.controller_did, "did:human:alice");
    assert_eq!(routed.agent_did, "did:agent:hermes");

    let verified_sender = VerifiedControllerSender {
        controller_user_id: profile.controller_user_id.clone(),
        controller_full_handle: profile.controller_full_handle.clone(),
        controller_scope_key: profile.controller_scope_key.clone(),
        controller_did: "did:human:alice-new".to_string(),
        sender_did: "did:human:alice-new".to_string(),
    };
    let rotated = route_controller_text_task_with_verified_sender(
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
            target_agent_did: "did:agent:hermes".to_string(),
            text: "恢复身份后的控制者消息".to_string(),
        },
    )
    .unwrap();
    assert_eq!(rotated.controller_did, "did:human:alice-new");
    assert_eq!(rotated.controller_scope_key, profile.controller_scope_key);

    let wrong_target = route_controller_text_task(
        &profile,
        ControllerTextMessage {
            message_id: "msg_003".to_string(),
            conversation_id: None,
            sender_did: "did:human:alice".to_string(),
            requester_user_id: None,
            requester_full_handle: None,
            trigger_kind: awiki_deamon::runtime::RuntimeTaskTriggerKind::ControllerDirect,
            invocation_authority: awiki_deamon::runtime::RuntimeInvocationAuthority::Controller,
            target_agent_did: "did:agent:other".to_string(),
            text: "错误目标".to_string(),
        },
    )
    .unwrap_err();
    assert!(wrong_target.to_string().contains("target"));
}

#[test]
fn workspace_modes_document_security_boundary() {
    use awiki_deamon::workspace::WorkspaceMode;
    assert!(!WorkspaceMode::SharedRoot.is_security_boundary());
    assert!(!WorkspaceMode::WorktreePerTask.is_security_boundary());
    assert!(WorkspaceMode::Container.is_security_boundary());
    assert!(WorkspaceMode::Sandbox.is_security_boundary());
}

#[test]
fn runtime_callback_debug_redacts_runtime_rpc_token() {
    let request = awiki_deamon::local_rpc::RuntimeRpcRequest {
        runtime_rpc_token: "rtok_debug_secret_value_123456789".to_string(),
        method: "task.status".to_string(),
        params: serde_json::json!({ "state": "running" }),
        debug: None,
    };
    assert!(!format!("{request:?}").contains("rtok_debug_secret_value_123456789"));
}
