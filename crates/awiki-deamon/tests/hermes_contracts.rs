use std::time::Duration;

use awiki_deamon::agent::runtime_plugin_id;
use awiki_deamon::inbox::{route_controller_text_task, ControllerTextMessage};
use awiki_deamon::plugins::hermes::{HERMES_RUNTIME_NAME, HERMES_RUNTIME_PLUGIN_ID};
use awiki_deamon::runtime::RuntimeAgentProfile;
use awiki_deamon::security::runtime_token::{RpcMethod, RuntimeTokenScope};

fn hermes_profile() -> RuntimeAgentProfile {
    RuntimeAgentProfile {
        agent_did: "did:agent:hermes".to_string(),
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
fn hermes_only_controller_text_can_enter_runtime_execution() {
    let profile = hermes_profile();
    let routed = route_controller_text_task(
        &profile,
        ControllerTextMessage {
            message_id: "msg_001".to_string(),
            conversation_id: Some("direct:did:human:alice".to_string()),
            sender_did: "did:human:alice".to_string(),
            target_agent_did: "did:agent:hermes".to_string(),
            text: "请处理这条消息".to_string(),
        },
    )
    .unwrap();

    assert_eq!(routed.task_id, "task_msg_001");
    assert_eq!(routed.controller_did, "did:human:alice");
    assert_eq!(routed.agent_did, "did:agent:hermes");

    let non_controller = route_controller_text_task(
        &profile,
        ControllerTextMessage {
            message_id: "msg_002".to_string(),
            conversation_id: None,
            sender_did: "did:human:bob".to_string(),
            target_agent_did: "did:agent:hermes".to_string(),
            text: "越权执行".to_string(),
        },
    )
    .unwrap_err();
    assert!(non_controller.to_string().contains("controller_did"));

    let wrong_target = route_controller_text_task(
        &profile,
        ControllerTextMessage {
            message_id: "msg_003".to_string(),
            conversation_id: None,
            sender_did: "did:human:alice".to_string(),
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
    assert!(contract.contains("`msg.send` 的目标契约是真实 ANP direct/direct-e2ee"));
    assert!(contract.contains("不新增 `task.result`"));
    assert!(contract.contains("不新增 `application/vnd.awiki...`"));
}
