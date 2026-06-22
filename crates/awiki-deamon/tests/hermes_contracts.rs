use std::time::Duration;

use awiki_deamon::agent::resolve_runtime;
use awiki_deamon::agent::runtime_plugin_id;
use awiki_deamon::controller_scope::VerifiedControllerSender;
use awiki_deamon::inbox::{
    route_controller_text_task, route_controller_text_task_with_verified_sender,
    ControllerTextMessage,
};
use awiki_deamon::plugins::hermes::{HERMES_RUNTIME_NAME, HERMES_RUNTIME_PLUGIN_ID};
use awiki_deamon::runtime::RuntimeAgentProfile;
use awiki_deamon::security::runtime_token::{RpcMethod, RuntimeTokenScope};

fn hermes_profile() -> RuntimeAgentProfile {
    RuntimeAgentProfile {
        agent_did: "did:agent:hermes".to_string(),
        agent_handle: "alice-hermes".to_string(),
        controller_user_id: "user-alice".to_string(),
        controller_full_handle: "alice.anpclaw.com".to_string(),
        controller_scope_key: "controller-scope:v1:test-alice-anpclaw-com".to_string(),
        controller_did: "did:human:alice".to_string(),
        runtime_profile_id: "profile_hermes_alice".to_string(),
        runtime_plugin_id: HERMES_RUNTIME_PLUGIN_ID.to_string(),
        display_name: Some("Alice Hermes".to_string()),
        workspace_id: None,
        workspace_root: None,
        workspace_mode: None,
    }
}

#[test]
fn hermes_runtime_plugin_id_is_stable() {
    assert_eq!(
        runtime_plugin_id(HERMES_RUNTIME_NAME).unwrap(),
        HERMES_RUNTIME_PLUGIN_ID
    );
    assert_eq!(
        runtime_plugin_id(" hermes ").unwrap(),
        HERMES_RUNTIME_PLUGIN_ID
    );

    let resolution = resolve_runtime(HERMES_RUNTIME_NAME, None).unwrap();
    assert_eq!(resolution.runtime_plugin_id, HERMES_RUNTIME_PLUGIN_ID);
    assert_eq!(resolution.driver_id, None);
    assert_eq!(resolution.legacy_runtime_plugin_id, None);
    assert!(!resolution.defaulted_driver_id);
    assert!(resolve_runtime(HERMES_RUNTIME_NAME, Some("codex")).is_err());
}

#[test]
fn hermes_current_rpc_methods_keep_compatibility_names_without_new_message_aliases() {
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
fn hermes_msg_send_recipient_scope_is_controlled_by_runtime_token_scope() {
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
fn hermes_controller_text_route_preserves_verified_sender_did() {
    let profile = hermes_profile();
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
fn hermes_implementation_contract_records_mvp_non_goals() {
    let contract = include_str!("../docs/hermes-plugin/implementation-contract.md");

    assert!(contract.contains("不安装 Hermes Python plugin"));
    assert!(contract.contains("不写 `plugin.yaml`"));
    assert!(contract.contains("不持有 DID 私钥"));
    assert!(contract.contains("`msg.send` 的目标契约是真实 ANP direct/group 普通消息"));
    assert!(contract.contains("不新增 `task.result`"));
    assert!(contract.contains("不新增 `application/vnd.awiki...`"));
}
