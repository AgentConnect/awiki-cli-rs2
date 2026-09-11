use anyhow::{Context, Result};
use awiki_deamon::plugins::acp::connection::AcpProcessPool;
use awiki_deamon::plugins::acp::runner::AcpRuntimePlugin;
use awiki_deamon::plugins::acp::{
    initialize_acp_profile, AcpProfileInitRequest, ACP_RUNTIME_PLUGIN_ID,
};
use awiki_deamon::runtime::{
    RuntimeAgentProfile, RuntimeConversationScope, RuntimeInvocationAuthority,
    RuntimeLaunchContext, RuntimePlugin, RuntimeRun, RuntimeRunStatus, RuntimeTask,
    RuntimeTaskTriggerKind,
};
use awiki_deamon::security::runtime_token::RuntimeRpcToken;
use awiki_deamon::{DaemonConfig, DaemonState};
use serde_json::json;

#[test]
fn real_local_deepseek_harness_accepts_generated_config_and_initialize() -> Result<()> {
    let checkout = std::path::Path::new("/home/ecs-user/deepseek-harness");
    if !checkout
        .join("packages/examples/acp-demo/lib/bin.js")
        .is_file()
    {
        return Ok(());
    }
    let root = tempfile::tempdir()?;
    let config = DaemonConfig::for_state_root(root.path())?;
    config.ensure_state_layout()?;
    let state = DaemonState::open_with_root_key_bytes(&config, [55_u8; 32]);
    state.initialize()?;
    let profile = RuntimeAgentProfile {
        agent_did: "did:agent:live-acp".to_string(),
        agent_handle: "live-acp".to_string(),
        controller_user_id: "user-alice".to_string(),
        controller_full_handle: "alice.anpclaw.com".to_string(),
        controller_scope_key: "controller-scope:v1:alice".to_string(),
        controller_did: "did:human:alice".to_string(),
        runtime_profile_id: "profile_live_acp".to_string(),
        runtime_plugin_id: "runtime.acp".to_string(),
        display_name: None,
        preferred_language: "zh-Hans".to_string(),
        workspace_id: None,
        workspace_root: None,
        workspace_mode: None,
    };
    let installed = initialize_acp_profile(
        &config,
        &state,
        &profile,
        AcpProfileInitRequest {
            acp_agent_id: "deepseek-harness",
            driver_config: Some(&json!({
                "install_mode": "local",
                "local_checkout": checkout
            })),
            secrets: Some(&json!({"DEEPSEEK_API_KEY": "smoke-only-not-used"})),
        },
    )?;
    assert_eq!(installed.record.status, "ready");
    Ok(())
}

#[test]
#[ignore = "requires a real DeepSeek credential and network access"]
fn real_local_deepseek_harness_returns_prompt_final_text() -> Result<()> {
    let checkout = std::path::Path::new("/home/ecs-user/deepseek-harness");
    if !checkout
        .join("packages/examples/acp-demo/lib/bin.js")
        .is_file()
    {
        return Ok(());
    }
    let api_key = std::env::var("DEEPSEEK_API_KEY")
        .context("DEEPSEEK_API_KEY is required for the live ACP prompt smoke")?;
    let root = tempfile::tempdir()?;
    let config = DaemonConfig::for_state_root(root.path())?;
    config.ensure_state_layout()?;
    let state = DaemonState::open_with_root_key_bytes(&config, [56_u8; 32]);
    state.initialize()?;
    let profile = RuntimeAgentProfile {
        agent_did: "did:agent:live-acp-prompt".to_string(),
        agent_handle: "live-acp-prompt".to_string(),
        controller_user_id: "user-alice".to_string(),
        controller_full_handle: "alice.anpclaw.com".to_string(),
        controller_scope_key: "controller-scope:v1:alice".to_string(),
        controller_did: "did:human:alice".to_string(),
        runtime_profile_id: "profile_live_acp_prompt".to_string(),
        runtime_plugin_id: ACP_RUNTIME_PLUGIN_ID.to_string(),
        display_name: None,
        preferred_language: "zh-Hans".to_string(),
        workspace_id: None,
        workspace_root: None,
        workspace_mode: None,
    };
    let installed = initialize_acp_profile(
        &config,
        &state,
        &profile,
        AcpProfileInitRequest {
            acp_agent_id: "deepseek-harness",
            driver_config: Some(&json!({
                "install_mode": "local",
                "local_checkout": checkout
            })),
            secrets: Some(&json!({"DEEPSEEK_API_KEY": api_key})),
        },
    )?;
    let task = RuntimeTask {
        task_id: "task_live_acp_prompt".to_string(),
        agent_did: profile.agent_did.clone(),
        agent_handle: profile.agent_handle.clone(),
        controller_user_id: profile.controller_user_id.clone(),
        controller_full_handle: profile.controller_full_handle.clone(),
        controller_scope_key: profile.controller_scope_key.clone(),
        controller_did: profile.controller_did.clone(),
        sender_did: profile.controller_did.clone(),
        requester_did: profile.controller_did.clone(),
        requester_user_id: None,
        requester_full_handle: None,
        trigger_kind: RuntimeTaskTriggerKind::ControllerDirect,
        conversation_scope: RuntimeConversationScope::controller_private(
            &profile.controller_scope_key,
        ),
        invocation_authority: RuntimeInvocationAuthority::Controller,
        reply_recipient_did: profile.controller_did.clone(),
        conversation_id: Some("direct:did:human:alice".to_string()),
        text: "只回复 ACP_LIVE_OK".to_string(),
    };
    let context = RuntimeLaunchContext {
        run: RuntimeRun {
            run_id: "run_live_acp_prompt".to_string(),
            task_id: task.task_id.clone(),
            agent_did: profile.agent_did.clone(),
            runtime_profile_id: profile.runtime_profile_id.clone(),
            runtime_plugin_id: ACP_RUNTIME_PLUGIN_ID.to_string(),
            workspace_id: None,
            status: RuntimeRunStatus::Pending,
        },
        task,
        preferred_language: profile.preferred_language.clone(),
        workspace_root: Some(installed.record.cwd_root.clone()),
        workspace_instance: None,
        cli_route_session: None,
        runtime_temp_dir: None,
        runtime_rpc_token: RuntimeRpcToken::generate(),
        local_socket_path: None,
    };
    let outcome = AcpRuntimePlugin::with_state(AcpProcessPool::default(), installed.record, state)
        .launch_run(context)?;
    assert_eq!(outcome.status, RuntimeRunStatus::Running);
    assert!(outcome.metadata["final_text"]
        .as_str()
        .is_some_and(|text| !text.trim().is_empty()));
    assert!(outcome.metadata["stop_reason"]
        .as_str()
        .is_some_and(|reason| !reason.trim().is_empty()));
    assert!(outcome.metadata["error"].is_null());
    Ok(())
}
