use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use awiki_deamon::agent::ACP_RUNTIME_PLUGIN_ID;
use awiki_deamon::inbox::ControllerTextMessage;
use awiki_deamon::outbox::{MemoryRuntimeOutbox, OutboxRecordKind};
use awiki_deamon::plugins::acp::connection::{AcpConnectionTimeouts, AcpProcessPool};
use awiki_deamon::plugins::acp::runner::{acp_session_route_key, AcpRuntimePlugin};
use awiki_deamon::plugins::hermes::StdioHermesGateway;
use awiki_deamon::runtime::dispatch::with_runtime_plugin;
use awiki_deamon::runtime::host::run_controller_text_task;
use awiki_deamon::runtime::{
    RuntimeAgentProfile, RuntimeConversationScope, RuntimeInvocationAuthority,
    RuntimeLaunchContext, RuntimePlugin, RuntimeRun, RuntimeRunStatus, RuntimeTask,
    RuntimeTaskTriggerKind,
};
use awiki_deamon::security::runtime_token::RuntimeRpcToken;
use awiki_deamon::state::AcpRuntimeProfileRecord;
use awiki_deamon::{DaemonConfig, DaemonState};
use serde_json::json;

const STUB_ACP_SERVER: &str = r#"
import json
import os
import pathlib
import sys

prompt_log = pathlib.Path(sys.argv[1])
if len(sys.argv) == 4 and os.getenv(sys.argv[2]) != sys.argv[3]:
    sys.stderr.write("missing expected daemon environment fallback\n")
    sys.stderr.flush()
    raise SystemExit(21)

def read_frame():
    line = sys.stdin.readline()
    if not line:
        raise SystemExit(0)
    return json.loads(line)

def send_frame(frame):
    sys.stdout.write(json.dumps(frame, separators=(",", ":")) + "\n")
    sys.stdout.flush()

initialize = read_frame()
send_frame({
    "jsonrpc": "2.0",
    "id": initialize["id"],
    "result": {
        "protocolVersion": 1,
        "agentInfo": {"name": "stub-acp", "version": "1"},
        "agentCapabilities": {
            "promptCapabilities": {"image": False, "audio": False, "embeddedContext": False}
        },
        "authMethods": [],
    },
})

new_session = read_frame()
send_frame({
    "jsonrpc": "2.0",
    "id": new_session["id"],
    "result": {"sessionId": "stub-session"},
})

session_index = 1
while True:
    prompt = read_frame()
    if prompt["method"] == "session/new":
        session_index += 1
        send_frame({
            "jsonrpc": "2.0",
            "id": prompt["id"],
            "result": {"sessionId": f"stub-session-{session_index}"},
        })
        continue
    if prompt["method"] == "session/cancel":
        continue
    session_id = prompt["params"]["sessionId"]
    prompt_log.write_text(prompt["params"]["prompt"][0]["text"], encoding="utf-8")
    send_frame({
        "jsonrpc": "2.0",
        "method": "session/update",
        "params": {
            "sessionId": session_id,
            "update": {
                "sessionUpdate": "agent_message_chunk",
                "content": {"type": "text", "text": "ACP final answer"},
            },
        },
    })
    send_frame({
        "jsonrpc": "2.0",
        "id": prompt["id"],
        "result": {"stopReason": "end_turn"},
    })
"#;

fn fixture() -> Result<(tempfile::TempDir, DaemonState, AcpRuntimeProfileRecord)> {
    let root = tempfile::tempdir()?;
    let config = DaemonConfig::for_state_root(root.path())?;
    config.ensure_state_layout()?;
    let state = DaemonState::open_with_root_key_bytes(&config, [42_u8; 32]);
    state.initialize()?;

    let profile_root = root.path().join("runtime/acp/profile_acp_alice");
    let workspace = profile_root.join("workspace");
    std::fs::create_dir_all(&workspace)?;
    let script = profile_root.join("stub_acp.py");
    std::fs::write(&script, STUB_ACP_SERVER)?;
    let config_path = profile_root.join("cordis.yml");
    std::fs::write(&config_path, "stub: true\n")?;
    let dotenv_path = profile_root.join(".env");
    std::fs::write(&dotenv_path, "DEEPSEEK_API_KEY=\"fixture-only\"\n")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dotenv_path, std::fs::Permissions::from_mode(0o600))?;
    }
    let prompt_log = profile_root.join("prompt.txt");
    let profile = AcpRuntimeProfileRecord {
        runtime_profile_id: "profile_acp_alice".to_string(),
        agent_did: "did:agent:acp-alice".to_string(),
        acp_agent_id: "deepseek-harness".to_string(),
        install_mode: "local".to_string(),
        install_root: profile_root.clone(),
        entry_command_json: json!({
            "program": "python3",
            "args": [script.display().to_string(), prompt_log.display().to_string()]
        }),
        config_path,
        cwd_root: workspace,
        credential_env_names: vec!["DEEPSEEK_API_KEY".to_string()],
        permission_policy: "allow-once".to_string(),
        installed_version: Some("stub".to_string()),
        status: "ready".to_string(),
    };
    state.upsert_acp_runtime_profile(&profile)?;
    Ok((root, state, profile))
}

fn launch_context(profile: &AcpRuntimeProfileRecord, suffix: &str) -> RuntimeLaunchContext {
    let task = RuntimeTask {
        task_id: format!("task_acp_{suffix}"),
        agent_did: profile.agent_did.clone(),
        agent_handle: "alice-acp".to_string(),
        controller_user_id: "user-alice".to_string(),
        controller_full_handle: "alice.anpclaw.com".to_string(),
        controller_scope_key: "controller-scope:v1:alice".to_string(),
        controller_did: "did:human:alice".to_string(),
        sender_did: "did:human:alice".to_string(),
        requester_did: "did:human:alice".to_string(),
        requester_user_id: None,
        requester_full_handle: None,
        trigger_kind: RuntimeTaskTriggerKind::ControllerDirect,
        conversation_scope: RuntimeConversationScope::controller_private(
            "controller-scope:v1:alice",
        ),
        invocation_authority: RuntimeInvocationAuthority::Controller,
        reply_recipient_did: "did:human:alice".to_string(),
        conversation_id: Some("direct:did:human:alice".to_string()),
        text: "请回答 ACP 测试消息".to_string(),
    };
    RuntimeLaunchContext {
        run: RuntimeRun {
            run_id: format!("run_acp_{suffix}"),
            task_id: task.task_id.clone(),
            agent_did: profile.agent_did.clone(),
            runtime_profile_id: profile.runtime_profile_id.clone(),
            runtime_plugin_id: ACP_RUNTIME_PLUGIN_ID.to_string(),
            workspace_id: None,
            status: RuntimeRunStatus::Pending,
        },
        task,
        preferred_language: "zh-Hans".to_string(),
        workspace_root: Some(profile.cwd_root.clone()),
        workspace_instance: None,
        cli_route_session: None,
        runtime_temp_dir: None,
        runtime_rpc_token: RuntimeRpcToken::generate(),
        local_socket_path: None,
    }
}

fn prompt_log(profile: &AcpRuntimeProfileRecord) -> PathBuf {
    profile.install_root.join("prompt.txt")
}

#[test]
fn acp_runtime_plugin_returns_native_outcome_and_persists_session() -> Result<()> {
    let (_root, state, profile) = fixture()?;
    let pool = AcpProcessPool::new(AcpConnectionTimeouts::default());
    let plugin = AcpRuntimePlugin::with_state(pool, profile.clone(), state.clone());
    assert!(plugin.check_install_status()?.installed);
    let context = launch_context(&profile, "first");
    let route_key = acp_session_route_key(&profile, &context.task)?;

    let outcome = plugin.launch_run(context)?;
    assert_eq!(outcome.status, RuntimeRunStatus::Running);
    assert_eq!(outcome.metadata["final_text"], "ACP final answer");
    assert_eq!(outcome.metadata["stop_reason"], "end_turn");
    assert_eq!(outcome.metadata["connection_epoch"], 1);
    assert_eq!(outcome.metadata["session_recreated"], false);
    assert!(outcome.metadata["error"].is_null());

    let session = state
        .load_active_acp_session_by_route(&route_key, 1)?
        .context("persisted ACP session")?;
    assert_eq!(session.acp_session_id, "stub-session");
    let prompt = std::fs::read_to_string(prompt_log(&profile))?;
    assert!(prompt.contains("[Controller]"));
    assert!(prompt.contains("invocation_authority: controller"));
    assert!(prompt.contains("[User Message]"));
    assert!(prompt.contains("请回答 ACP 测试消息"));
    Ok(())
}

#[test]
fn acp_runtime_uses_daemon_environment_when_profile_dotenv_has_no_value() -> Result<()> {
    const FALLBACK_NAME: &str = "AWIKI_ACP_ENV_FALLBACK_TEST";
    const FALLBACK_VALUE: &str = "fallback-test-value";

    let (_root, state, mut profile) = fixture()?;
    profile.credential_env_names = vec!["DEEPSEEK_API_KEY".to_string(), FALLBACK_NAME.to_string()];
    profile.entry_command_json["args"] = json!([
        profile
            .install_root
            .join("stub_acp.py")
            .display()
            .to_string(),
        prompt_log(&profile).display().to_string(),
        FALLBACK_NAME,
        FALLBACK_VALUE,
    ]);
    state.upsert_acp_runtime_profile(&profile)?;

    std::env::set_var(FALLBACK_NAME, FALLBACK_VALUE);
    let result = AcpRuntimePlugin::with_state(AcpProcessPool::default(), profile.clone(), state)
        .launch_run(launch_context(&profile, "daemon_env_fallback"));
    std::env::remove_var(FALLBACK_NAME);

    let outcome = result?;
    assert_eq!(outcome.metadata["final_text"], "ACP final answer");
    Ok(())
}

#[test]
fn acp_profile_dotenv_takes_precedence_over_daemon_environment() -> Result<()> {
    const CREDENTIAL_NAME: &str = "AWIKI_ACP_ENV_PRIORITY_TEST";
    const PROFILE_VALUE: &str = "profile-private-value";

    let (_root, state, mut profile) = fixture()?;
    profile.credential_env_names =
        vec!["DEEPSEEK_API_KEY".to_string(), CREDENTIAL_NAME.to_string()];
    profile.entry_command_json["args"] = json!([
        profile
            .install_root
            .join("stub_acp.py")
            .display()
            .to_string(),
        prompt_log(&profile).display().to_string(),
        CREDENTIAL_NAME,
        PROFILE_VALUE,
    ]);
    std::fs::write(
        profile
            .config_path
            .parent()
            .context("profile directory")?
            .join(".env"),
        format!("DEEPSEEK_API_KEY=\"fixture-only\"\n{CREDENTIAL_NAME}=\"{PROFILE_VALUE}\"\n"),
    )?;
    state.upsert_acp_runtime_profile(&profile)?;

    std::env::set_var(CREDENTIAL_NAME, "daemon-fallback-value");
    let result = AcpRuntimePlugin::with_state(AcpProcessPool::default(), profile.clone(), state)
        .launch_run(launch_context(&profile, "profile_env_priority"));
    std::env::remove_var(CREDENTIAL_NAME);

    let outcome = result?;
    assert_eq!(outcome.metadata["final_text"], "ACP final answer");
    Ok(())
}

#[cfg(unix)]
#[test]
fn acp_runtime_refuses_to_read_profile_dotenv_through_a_symlink() -> Result<()> {
    let (root, state, profile) = fixture()?;
    let victim = root.path().join("untrusted-acp-env");
    std::fs::write(&victim, "DEEPSEEK_API_KEY=\"must-not-be-loaded\"\n")?;
    std::fs::remove_file(
        profile
            .config_path
            .parent()
            .context("profile directory")?
            .join(".env"),
    )?;
    std::os::unix::fs::symlink(
        &victim,
        profile
            .config_path
            .parent()
            .context("profile directory")?
            .join(".env"),
    )?;

    let error = AcpRuntimePlugin::with_state(AcpProcessPool::default(), profile.clone(), state)
        .launch_run(launch_context(&profile, "dotenv_symlink"))
        .expect_err("ACP runtime must reject a credential symlink")
        .to_string();
    assert!(error.contains("symlink"));
    assert!(!error.contains("must-not-be-loaded"));
    Ok(())
}

#[test]
fn acp_runtime_plugin_new_pool_starts_new_epoch_and_session() -> Result<()> {
    let (_root, state, profile) = fixture()?;
    let first_context = launch_context(&profile, "epoch_1");
    let route_key = acp_session_route_key(&profile, &first_context.task)?;
    {
        let plugin =
            AcpRuntimePlugin::with_state(AcpProcessPool::default(), profile.clone(), state.clone());
        let outcome = plugin.launch_run(first_context)?;
        assert_eq!(outcome.metadata["connection_epoch"], 1);
    }

    let plugin =
        AcpRuntimePlugin::with_state(AcpProcessPool::default(), profile.clone(), state.clone());
    let outcome = plugin.launch_run(launch_context(&profile, "epoch_2"))?;
    assert_eq!(outcome.metadata["connection_epoch"], 2);
    assert_eq!(outcome.metadata["session_recreated"], true);
    assert_eq!(
        outcome.metadata["session_status"],
        "ACP subprocess restarted; created a new session"
    );
    assert_eq!(
        state
            .load_active_acp_session_by_route(&route_key, 2)?
            .context("new epoch session")?
            .connection_epoch,
        2
    );
    Ok(())
}

#[test]
fn acp_new_route_on_the_same_process_is_not_reported_as_a_restart() -> Result<()> {
    let (_root, state, profile) = fixture()?;
    let plugin = AcpRuntimePlugin::with_state(AcpProcessPool::default(), profile.clone(), state);
    plugin.launch_run(launch_context(&profile, "first_route"))?;

    let mut second_context = launch_context(&profile, "second_route");
    second_context.task.conversation_scope =
        RuntimeConversationScope::group_visible("did:group:acp-audit");
    second_context.task.trigger_kind = RuntimeTaskTriggerKind::GroupMention;
    second_context.task.invocation_authority = RuntimeInvocationAuthority::Requester;
    second_context.task.requester_user_id = Some("user-bob".to_string());
    second_context.task.requester_full_handle = Some("bob.anpclaw.com".to_string());
    second_context.task.requester_did = "did:human:bob".to_string();
    second_context.task.sender_did = "did:human:bob".to_string();
    second_context.task.reply_recipient_did = "did:human:bob".to_string();
    second_context.task.conversation_id = Some("group:did:group:acp-audit".to_string());
    let outcome = plugin.launch_run(second_context)?;

    assert_eq!(outcome.metadata["connection_epoch"], 1);
    assert_eq!(outcome.metadata["session_recreated"], false);
    assert!(outcome.metadata["session_status"].is_null());
    Ok(())
}

#[test]
fn acp_runtime_plugin_rejects_mismatched_profile_binding() -> Result<()> {
    let (_root, state, profile) = fixture()?;
    let plugin = AcpRuntimePlugin::with_state(AcpProcessPool::default(), profile.clone(), state);
    let mut context = launch_context(&profile, "wrong");
    context.run.agent_did = "did:agent:other".to_string();
    let error = plugin
        .launch_run(context)
        .expect_err("binding mismatch must fail")
        .to_string();
    assert!(error.contains("profile binding"));
    assert!(!Path::new(&prompt_log(&profile)).exists());
    Ok(())
}

#[test]
fn runtime_dispatch_loads_acp_plugin_from_profile() -> Result<()> {
    let (root, state, acp_profile) = fixture()?;
    let config = DaemonConfig::for_state_root(root.path())?;
    let runtime_profile = RuntimeAgentProfile {
        agent_did: acp_profile.agent_did.clone(),
        agent_handle: "alice-acp".to_string(),
        controller_user_id: "user-alice".to_string(),
        controller_full_handle: "alice.anpclaw.com".to_string(),
        controller_scope_key: "controller-scope:v1:alice".to_string(),
        controller_did: "did:human:alice".to_string(),
        runtime_profile_id: acp_profile.runtime_profile_id.clone(),
        runtime_plugin_id: ACP_RUNTIME_PLUGIN_ID.to_string(),
        display_name: None,
        preferred_language: "zh-Hans".to_string(),
        workspace_id: None,
        workspace_root: None,
        workspace_mode: None,
    };

    let dispatched = with_runtime_plugin(
        &config,
        &state,
        &runtime_profile,
        StdioHermesGateway::from_config_without_detection(&config),
        |plugin| {
            Ok((
                plugin.plugin_id().to_string(),
                plugin.check_install_status()?.installed,
            ))
        },
    )?
    .context("runtime plugin must be supported")?;
    assert_eq!(dispatched, (ACP_RUNTIME_PLUGIN_ID.to_string(), true));
    Ok(())
}

#[test]
fn acp_missing_required_runtime_credential_reports_not_installed() -> Result<()> {
    let (_root, state, mut profile) = fixture()?;
    profile.credential_env_names.clear();
    std::fs::remove_file(
        profile
            .config_path
            .parent()
            .context("profile directory")?
            .join(".env"),
    )?;
    state.upsert_acp_runtime_profile(&profile)?;

    let status = AcpRuntimePlugin::with_state(AcpProcessPool::default(), profile, state)
        .check_install_status()?;

    assert!(!status.installed);
    assert!(status
        .detail
        .as_deref()
        .is_some_and(|detail| detail.contains("required credential")));
    Ok(())
}

#[test]
fn acp_missing_install_emits_runtime_not_installed_with_setup_required() -> Result<()> {
    let (_root, state, mut acp_profile) = fixture()?;
    acp_profile.status = "failed".to_string();
    state.upsert_acp_runtime_profile(&acp_profile)?;
    let plugin = AcpRuntimePlugin::with_state(
        AcpProcessPool::default(),
        acp_profile.clone(),
        state.clone(),
    );
    let profile = RuntimeAgentProfile {
        agent_did: acp_profile.agent_did.clone(),
        agent_handle: "alice-acp".to_string(),
        controller_user_id: "user-alice".to_string(),
        controller_full_handle: "alice.anpclaw.com".to_string(),
        controller_scope_key: "controller-scope:v1:alice".to_string(),
        controller_did: "did:human:alice".to_string(),
        runtime_profile_id: acp_profile.runtime_profile_id.clone(),
        runtime_plugin_id: ACP_RUNTIME_PLUGIN_ID.to_string(),
        display_name: None,
        preferred_language: "zh-Hans".to_string(),
        workspace_id: None,
        workspace_root: None,
        workspace_mode: None,
    };
    let outbox = MemoryRuntimeOutbox::default();

    let error = run_controller_text_task(
        &state,
        &profile,
        &plugin,
        &outbox,
        ControllerTextMessage::controller_direct(
            "msg_acp_install_missing",
            Some("direct:did:human:alice".to_string()),
            "did:human:alice",
            &profile.agent_did,
            "请运行尚未安装的 ACP profile",
        ),
    )
    .expect_err("ACP profile that is not ready must not launch");
    assert!(error.to_string().contains("not installed"));

    let records = outbox.records();
    let status = records
        .iter()
        .find(|record| record.kind == OutboxRecordKind::Status)
        .context("ACP missing-install status")?;
    assert_eq!(status.state.as_deref(), Some("failed"));
    assert_eq!(
        status.last_error_code.as_deref(),
        Some("runtime_not_installed")
    );
    assert_eq!(
        status
            .metadata
            .as_ref()
            .and_then(|value| value["next_action"].as_str()),
        Some("setup_required")
    );
    assert_eq!(
        status
            .metadata
            .as_ref()
            .and_then(|value| value["runtime_family"].as_str()),
        Some("acp")
    );
    assert!(!records
        .iter()
        .any(|record| record.kind == OutboxRecordKind::Final));
    Ok(())
}

#[test]
fn acp_native_outcome_uses_runtime_final_outbox() -> Result<()> {
    let (_root, state, acp_profile) = fixture()?;
    let plugin = AcpRuntimePlugin::with_state(
        AcpProcessPool::default(),
        acp_profile.clone(),
        state.clone(),
    );
    let profile = RuntimeAgentProfile {
        agent_did: acp_profile.agent_did.clone(),
        agent_handle: "alice-acp".to_string(),
        controller_user_id: "user-alice".to_string(),
        controller_full_handle: "alice.anpclaw.com".to_string(),
        controller_scope_key: "controller-scope:v1:alice".to_string(),
        controller_did: "did:human:alice".to_string(),
        runtime_profile_id: acp_profile.runtime_profile_id.clone(),
        runtime_plugin_id: ACP_RUNTIME_PLUGIN_ID.to_string(),
        display_name: None,
        preferred_language: "zh-Hans".to_string(),
        workspace_id: None,
        workspace_root: None,
        workspace_mode: None,
    };
    let outbox = MemoryRuntimeOutbox::default();

    let result = run_controller_text_task(
        &state,
        &profile,
        &plugin,
        &outbox,
        ControllerTextMessage::controller_direct(
            "msg_acp_host",
            Some("direct:did:human:alice".to_string()),
            "did:human:alice",
            &profile.agent_did,
            "请通过 host 返回结果",
        ),
    )?;

    assert_eq!(result.run.status, RuntimeRunStatus::Finished);
    let final_record = state
        .load_runtime_final_outbox_by_run(&result.run.run_id)?
        .context("ACP runtime final outbox record")?;
    assert_eq!(final_record.status, "sent");
    assert_eq!(final_record.final_source, "acp_final_text");
    assert!(outbox.records().iter().any(|record| {
        record.kind == OutboxRecordKind::Message
            && record.text.as_deref() == Some("ACP final answer")
    }));

    drop(plugin);
    let restarted_plugin = AcpRuntimePlugin::with_state(
        AcpProcessPool::default(),
        acp_profile.clone(),
        state.clone(),
    );
    let restarted = run_controller_text_task(
        &state,
        &profile,
        &restarted_plugin,
        &outbox,
        ControllerTextMessage::controller_direct(
            "msg_acp_host_after_restart",
            Some("direct:did:human:alice".to_string()),
            "did:human:alice",
            &profile.agent_did,
            "请在重启后通过 host 返回结果",
        ),
    )?;
    assert_eq!(restarted.launch_outcome.metadata["session_recreated"], true);
    assert!(outbox.records().iter().any(|record| {
        record.kind == OutboxRecordKind::Status
            && record.text.as_deref() == Some("ACP subprocess restarted; created a new session")
    }));
    Ok(())
}
