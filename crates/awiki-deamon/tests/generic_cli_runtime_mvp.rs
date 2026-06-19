use awiki_deamon::controller_scope::VerifiedControllerSender;
use awiki_deamon::inbox::{route_controller_text_task_with_verified_sender, ControllerTextMessage};
use awiki_deamon::outbox::{MemoryRuntimeOutbox, OutboxRecordKind};
use awiki_deamon::plugins::generic_cli::{
    codex::{CodexDriver, CodexDriverConfig},
    CommandGenericCliDriver, GenericCliDriver, GenericCliDriverRegistry, GenericCliRuntimePlugin,
    TestGenericCliDriver,
};
use awiki_deamon::runtime::host::{run_controller_text_task, run_controller_text_task_with_config};
use awiki_deamon::runtime::{
    RuntimeAgentProfile, RuntimeInstallStatus, RuntimeLaunchContext, RuntimeLaunchOutcome,
    RuntimePlugin, RuntimeRunStatus,
};
use awiki_deamon::state::CliRuntimeProfileRecord;
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
        controller_user_id: "user-alice".to_string(),
        controller_full_handle: "alice.anpclaw.com".to_string(),
        controller_scope_key: "controller-scope:v1:test-alice-anpclaw-com".to_string(),
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
            requester_full_handle: None,
            trigger_kind: awiki_deamon::runtime::RuntimeTaskTriggerKind::ControllerDirect,
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
            requester_full_handle: None,
            trigger_kind: awiki_deamon::runtime::RuntimeTaskTriggerKind::ControllerDirect,
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

#[derive(Debug, Clone)]
struct MsgSendCallbackPlugin;

impl RuntimePlugin for MsgSendCallbackPlugin {
    fn plugin_id(&self) -> &str {
        "generic-cli"
    }

    fn check_install_status(&self) -> anyhow::Result<RuntimeInstallStatus> {
        Ok(RuntimeInstallStatus {
            installed: true,
            detail: Some("msg.send callback test plugin".to_string()),
        })
    }

    fn launch_run(&self, context: RuntimeLaunchContext) -> anyhow::Result<RuntimeLaunchOutcome> {
        let token = context.runtime_rpc_token.as_str().to_string();
        Ok(RuntimeLaunchOutcome {
            run_id: context.run.run_id,
            status: RuntimeRunStatus::Running,
            exit_code: Some(0),
            callbacks: vec![
                awiki_deamon::cli_wrapper::CliWrapperRequest::msg_send(
                    token.clone(),
                    "did:human:bob",
                    "hello from generic-cli",
                )
                .into_rpc_request(),
                awiki_deamon::cli_wrapper::CliWrapperRequest::task_finish(
                    token,
                    context.task.task_id,
                    "done",
                )
                .into_rpc_request(),
            ],
            metadata: json!({"driver_id": "codex"}),
        })
    }
}

#[derive(Debug, Clone)]
struct AttachmentSendCallbackPlugin;

impl RuntimePlugin for AttachmentSendCallbackPlugin {
    fn plugin_id(&self) -> &str {
        "generic-cli"
    }

    fn check_install_status(&self) -> anyhow::Result<RuntimeInstallStatus> {
        Ok(RuntimeInstallStatus {
            installed: true,
            detail: Some("attachment.send callback test plugin".to_string()),
        })
    }

    fn launch_run(&self, context: RuntimeLaunchContext) -> anyhow::Result<RuntimeLaunchOutcome> {
        let workspace = context
            .workspace_root
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("workspace_root is required"))?;
        std::fs::create_dir_all(workspace)?;
        let report_path = workspace.join("report.txt");
        std::fs::write(&report_path, "small report")?;
        let token = context.runtime_rpc_token.as_str().to_string();
        Ok(RuntimeLaunchOutcome {
            run_id: context.run.run_id,
            status: RuntimeRunStatus::Running,
            exit_code: Some(0),
            callbacks: vec![
                awiki_deamon::cli_wrapper::CliWrapperRequest::attachment_send(
                    token.clone(),
                    report_path.to_string_lossy().to_string(),
                    Some("report.txt"),
                    Some("report ready"),
                )
                .into_rpc_request(),
                awiki_deamon::cli_wrapper::CliWrapperRequest::task_finish(
                    token,
                    context.task.task_id,
                    "done",
                )
                .into_rpc_request(),
            ],
            metadata: json!({"driver_id": "codex"}),
        })
    }
}

#[derive(Debug, Clone)]
struct DuplicateFinishCallbackPlugin;

impl RuntimePlugin for DuplicateFinishCallbackPlugin {
    fn plugin_id(&self) -> &str {
        "generic-cli"
    }

    fn check_install_status(&self) -> anyhow::Result<RuntimeInstallStatus> {
        Ok(RuntimeInstallStatus {
            installed: true,
            detail: Some("duplicate finish callback test plugin".to_string()),
        })
    }

    fn launch_run(&self, context: RuntimeLaunchContext) -> anyhow::Result<RuntimeLaunchOutcome> {
        let token = context.runtime_rpc_token.as_str().to_string();
        let task_id = context.task.task_id;
        Ok(RuntimeLaunchOutcome {
            run_id: context.run.run_id,
            status: RuntimeRunStatus::Running,
            exit_code: Some(0),
            callbacks: vec![
                awiki_deamon::cli_wrapper::CliWrapperRequest::task_finish(
                    token.clone(),
                    task_id.clone(),
                    "first done",
                )
                .into_rpc_request(),
                awiki_deamon::cli_wrapper::CliWrapperRequest::task_finish(
                    token,
                    task_id,
                    "second done",
                )
                .into_rpc_request(),
            ],
            metadata: json!({"driver_id": "codex"}),
        })
    }
}

#[derive(Debug, Clone)]
struct UnauthorizedMsgSendCallbackPlugin;

impl RuntimePlugin for UnauthorizedMsgSendCallbackPlugin {
    fn plugin_id(&self) -> &str {
        "generic-cli"
    }

    fn check_install_status(&self) -> anyhow::Result<RuntimeInstallStatus> {
        Ok(RuntimeInstallStatus {
            installed: true,
            detail: Some("unauthorized msg.send callback test plugin".to_string()),
        })
    }

    fn launch_run(&self, context: RuntimeLaunchContext) -> anyhow::Result<RuntimeLaunchOutcome> {
        Ok(RuntimeLaunchOutcome {
            run_id: context.run.run_id,
            status: RuntimeRunStatus::Running,
            exit_code: Some(0),
            callbacks: vec![awiki_deamon::cli_wrapper::CliWrapperRequest::msg_send(
                context.runtime_rpc_token.as_str().to_string(),
                "did:human:mallory",
                "blocked",
            )
            .into_rpc_request()],
            metadata: json!({"driver_id": "codex"}),
        })
    }
}

#[test]
fn duplicate_task_finish_callback_is_idempotent() {
    let (root, state) = fixture();
    let outbox = MemoryRuntimeOutbox::default();
    let plugin = DuplicateFinishCallbackPlugin;
    let profile = profile(root.path().join("workspace"));

    let result = run_controller_text_task(
        &state,
        &profile,
        &plugin,
        &outbox,
        ControllerTextMessage {
            message_id: "msg_duplicate_finish".to_string(),
            conversation_id: Some("conv_duplicate_finish".to_string()),
            sender_did: "did:human:alice".to_string(),
            requester_full_handle: None,
            trigger_kind: awiki_deamon::runtime::RuntimeTaskTriggerKind::ControllerDirect,
            target_agent_did: "did:agent:alice-coder".to_string(),
            text: "finish once".to_string(),
        },
    )
    .unwrap();

    assert_eq!(result.run.status, RuntimeRunStatus::Finished);
    let records = outbox.records();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].kind, OutboxRecordKind::Final);
    assert_eq!(records[0].text.as_deref(), Some("first done"));
}

#[test]
fn generic_cli_runtime_profile_policy_allows_non_controller_msg_send() {
    let (root, state) = fixture();
    let outbox = MemoryRuntimeOutbox::default();
    let plugin = MsgSendCallbackPlugin;
    let profile = profile(root.path().join("workspace"));
    let mut cli_profile =
        CliRuntimeProfileRecord::for_driver(&profile.runtime_profile_id, "codex").unwrap();
    cli_profile.recipient_policy_json = json!({
        "allow_controller": true,
        "allowed_dids": ["did:human:bob"],
        "allowed_security": ["default_plain"]
    });
    state.upsert_cli_runtime_profile(&cli_profile).unwrap();

    let result = run_controller_text_task(
        &state,
        &profile,
        &plugin,
        &outbox,
        ControllerTextMessage {
            message_id: "msg_policy".to_string(),
            conversation_id: Some("conv_policy".to_string()),
            sender_did: "did:human:alice".to_string(),
            requester_full_handle: None,
            trigger_kind: awiki_deamon::runtime::RuntimeTaskTriggerKind::ControllerDirect,
            target_agent_did: "did:agent:alice-coder".to_string(),
            text: "send to bob".to_string(),
        },
    )
    .unwrap();

    assert_eq!(result.run.status, RuntimeRunStatus::Finished);
    let records = outbox.records();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].kind, OutboxRecordKind::Message);
    assert_eq!(records[0].recipient.as_deref(), Some("did:human:bob"));
    assert_eq!(records[0].text.as_deref(), Some("hello from generic-cli"));
    assert_eq!(records[1].kind, OutboxRecordKind::Final);

    let connection = Connection::open(root.path().join("daemon.db")).unwrap();
    let allowed_recipients_json: String = connection
        .query_row(
            "SELECT allowed_recipients_json FROM runtime_rpc_tokens WHERE token_id = ?1",
            [&result.token_id],
            |row| row.get(0),
        )
        .unwrap();
    assert!(allowed_recipients_json.contains("did:human:bob"));
    let sent_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM audit_log WHERE event_type = 'runtime.msg_send.sent'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(sent_count, 1);
}

#[test]
fn runtime_run_token_allows_current_conversation_attachment_send() {
    let (root, state) = fixture();
    let outbox = MemoryRuntimeOutbox::default();
    let plugin = AttachmentSendCallbackPlugin;
    let profile = profile(root.path().join("workspace"));

    let result = run_controller_text_task(
        &state,
        &profile,
        &plugin,
        &outbox,
        ControllerTextMessage {
            message_id: "msg_attachment_policy".to_string(),
            conversation_id: Some("conv_attachment_policy".to_string()),
            sender_did: "did:human:alice".to_string(),
            requester_full_handle: None,
            trigger_kind: awiki_deamon::runtime::RuntimeTaskTriggerKind::ControllerDirect,
            target_agent_did: "did:agent:alice-coder".to_string(),
            text: "send current conversation attachment".to_string(),
        },
    )
    .unwrap();

    assert_eq!(result.run.status, RuntimeRunStatus::Finished);
    let records = outbox.records();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].kind, OutboxRecordKind::Message);
    assert_eq!(records[0].recipient.as_deref(), Some("did:human:alice"));
    assert_eq!(
        records[0].raw_recipient.as_deref(),
        Some("current_conversation")
    );
    assert_eq!(records[0].text.as_deref(), Some("report ready"));
    assert_eq!(records[1].kind, OutboxRecordKind::Final);

    let connection = Connection::open(root.path().join("daemon.db")).unwrap();
    let allowed_methods_json: String = connection
        .query_row(
            "SELECT allowed_methods_json FROM runtime_rpc_tokens WHERE token_id = ?1",
            [&result.token_id],
            |row| row.get(0),
        )
        .unwrap();
    assert!(allowed_methods_json.contains("send_attachment"));
    let audit_dump: String = connection
        .query_row(
            "SELECT COALESCE(detail_json, '') FROM audit_log WHERE event_type = 'runtime.attachment_send.sent' ORDER BY created_at_ms DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(audit_dump.contains("report.txt"));
    assert!(!audit_dump.contains(root.path().to_string_lossy().as_ref()));
}

#[test]
fn generic_cli_runtime_profile_policy_rejects_unlisted_msg_send() {
    let (root, state) = fixture();
    let outbox = MemoryRuntimeOutbox::default();
    let plugin = UnauthorizedMsgSendCallbackPlugin;
    let profile = profile(root.path().join("workspace"));
    let mut cli_profile =
        CliRuntimeProfileRecord::for_driver(&profile.runtime_profile_id, "codex").unwrap();
    cli_profile.recipient_policy_json = json!({
        "allow_controller": true,
        "allowed_dids": ["did:human:bob"],
        "allowed_security": ["default_plain"]
    });
    state.upsert_cli_runtime_profile(&cli_profile).unwrap();

    let error = run_controller_text_task(
        &state,
        &profile,
        &plugin,
        &outbox,
        ControllerTextMessage {
            message_id: "msg_policy_blocked".to_string(),
            conversation_id: Some("conv_policy_blocked".to_string()),
            sender_did: "did:human:alice".to_string(),
            requester_full_handle: None,
            trigger_kind: awiki_deamon::runtime::RuntimeTaskTriggerKind::ControllerDirect,
            target_agent_did: "did:agent:alice-coder".to_string(),
            text: "send to mallory".to_string(),
        },
    )
    .unwrap_err();

    assert!(error.to_string().contains("runtime callback"));
    assert!(outbox.records().is_empty());
}

#[derive(Debug, Clone)]
struct SendMessageDriver;

impl awiki_deamon::plugins::generic_cli::GenericCliDriver for SendMessageDriver {
    fn check_install_status(&self) -> anyhow::Result<RuntimeInstallStatus> {
        Ok(RuntimeInstallStatus {
            installed: true,
            detail: Some("send message test driver".to_string()),
        })
    }

    fn run(
        &self,
        invocation: awiki_deamon::plugins::generic_cli::GenericCliInvocation,
    ) -> anyhow::Result<awiki_deamon::plugins::generic_cli::GenericCliExit> {
        let _callback = awiki_deamon::cli_wrapper::CliWrapperRequest::msg_send(
            invocation.runtime_rpc_token,
            "did:human:bob",
            "hello from generic-cli",
        )
        .into_rpc_request();
        assert_eq!(invocation.callbacks.len(), 2);
        Ok(awiki_deamon::plugins::generic_cli::GenericCliExit {
            exit_code: 0,
            status: RuntimeRunStatus::Finished,
            callbacks: invocation.callbacks,
            metadata: json!({"driver_id": "codex"}),
        })
    }
}

#[test]
fn generic_cli_runtime_profile_policy_persists_allowed_recipients_in_token_scope() {
    let (root, state) = fixture();
    let outbox = MemoryRuntimeOutbox::default();
    let plugin = GenericCliRuntimePlugin::new(SendMessageDriver);
    let profile = profile(root.path().join("workspace"));
    let mut cli_profile =
        CliRuntimeProfileRecord::for_driver(&profile.runtime_profile_id, "codex").unwrap();
    cli_profile.recipient_policy_json = json!({
        "allow_controller": true,
        "allowed_dids": ["did:human:bob"],
        "allowed_security": ["default_plain"]
    });
    state.upsert_cli_runtime_profile(&cli_profile).unwrap();

    let result = run_controller_text_task(
        &state,
        &profile,
        &plugin,
        &outbox,
        ControllerTextMessage {
            message_id: "msg_policy".to_string(),
            conversation_id: Some("conv_policy".to_string()),
            sender_did: "did:human:alice".to_string(),
            requester_full_handle: None,
            trigger_kind: awiki_deamon::runtime::RuntimeTaskTriggerKind::ControllerDirect,
            target_agent_did: "did:agent:alice-coder".to_string(),
            text: "send to bob".to_string(),
        },
    )
    .unwrap();

    assert_eq!(result.run.status, RuntimeRunStatus::Finished);
    let connection = Connection::open(root.path().join("daemon.db")).unwrap();
    let allowed_recipients_json: String = connection
        .query_row(
            "SELECT allowed_recipients_json FROM runtime_rpc_tokens WHERE token_id = ?1",
            [&result.token_id],
            |row| row.get(0),
        )
        .unwrap();
    assert!(allowed_recipients_json.contains("did:human:bob"));
}

#[cfg(unix)]
#[test]
fn command_driver_uses_local_rpc_without_task_text_env_or_callbacks() {
    use awiki_deamon::local_rpc::{bind_uds_listener, handle_uds_stream_with_outbox};
    use std::os::unix::fs::PermissionsExt;
    use std::time::{Duration, Instant};

    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    config.ensure_state_layout().unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();
    let profile = profile(root.path().join("workspace"));
    std::fs::create_dir_all(profile.workspace_root.as_ref().unwrap()).unwrap();

    let script_path = root.path().join("generic-cli-driver.sh");
    let env_capture = root.path().join("driver-env.jsonl");
    std::fs::write(
        &script_path,
        format!(
            r#"#!/bin/sh
set -eu
cat > "{env_capture}" <<EOF
TASK_TEXT=${{AWIKI_DAEMON_TASK_TEXT-}}
SOCKET=${{AWIKI_DAEMON_SOCKET-}}
RUN_ID=${{AWIKI_DAEMON_RUN_ID-}}
TASK_ID=${{AWIKI_DAEMON_TASK_ID-}}
AGENT_DID=${{AWIKI_DAEMON_AGENT_DID-}}
PROFILE_ID=${{AWIKI_DAEMON_RUNTIME_PROFILE_ID-}}
WRAPPER=${{AWIKI_DAEMON_CLI_WRAPPER-}}
EOF
python3 - "$AWIKI_DAEMON_SOCKET" "$AWIKI_DAEMON_RUNTIME_RPC_TOKEN" "$AWIKI_DAEMON_TASK_ID" <<'PY'
import json
import socket
import sys

socket_path, token, task_id = sys.argv[1:4]
for request in [
    {{
        "runtime_rpc_token": token,
        "method": "task.status",
        "params": {{"task_id": task_id, "state": "running", "text": "command started"}},
    }},
    {{
        "runtime_rpc_token": token,
        "method": "task.finish",
        "params": {{"task_id": task_id, "text": "command finished"}},
    }},
]:
    client = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    client.connect(socket_path)
    client.sendall((json.dumps(request) + "\n").encode())
    response = b""
    while not response.endswith(b"\n"):
        chunk = client.recv(4096)
        if not chunk:
            break
        response += chunk
    client.close()
    decoded = json.loads(response.decode())
    if not decoded.get("ok"):
        raise SystemExit(decoded)
PY
"#,
            env_capture = env_capture.display()
        ),
    )
    .unwrap();
    let mut permissions = std::fs::metadata(&script_path).unwrap().permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(&script_path, permissions).unwrap();

    let outbox = MemoryRuntimeOutbox::default();
    let listener = bind_uds_listener(&config.local_socket_path).unwrap();
    let worker_state = state.clone();
    let worker_outbox = outbox.clone();
    let worker = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut handled = 0;
        while handled < 2 {
            match listener.accept() {
                Ok((stream, _)) => {
                    handle_uds_stream_with_outbox(&worker_state, &worker_outbox, stream).unwrap();
                    handled += 1;
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if Instant::now() > deadline {
                        panic!("timed out waiting for command driver RPC");
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("accept command driver RPC: {error}"),
            }
        }
    });

    let plugin = GenericCliRuntimePlugin::new(CommandGenericCliDriver::new(&script_path, vec![]));
    let result = run_controller_text_task_with_config(
        &config,
        &state,
        &profile,
        &plugin,
        &outbox,
        ControllerTextMessage {
            message_id: "msg_command_driver".to_string(),
            conversation_id: Some("conv_command_driver".to_string()),
            sender_did: "did:human:alice".to_string(),
            requester_full_handle: None,
            trigger_kind: awiki_deamon::runtime::RuntimeTaskTriggerKind::ControllerDirect,
            target_agent_did: "did:agent:alice-coder".to_string(),
            text: "run command driver".to_string(),
        },
    )
    .unwrap();
    worker.join().unwrap();

    assert_eq!(result.launch_outcome.status, RuntimeRunStatus::Finished);
    assert!(result.launch_outcome.callbacks.is_empty());
    let records = outbox.records();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].kind, OutboxRecordKind::Status);
    assert_eq!(records[1].kind, OutboxRecordKind::Final);

    let env_dump = std::fs::read_to_string(env_capture).unwrap();
    assert!(env_dump.contains("TASK_TEXT=\n"));
    assert!(env_dump.contains("SOCKET="));
    assert!(env_dump.contains("RUN_ID=run_task_msg_command_driver"));
    assert!(env_dump.contains("TASK_ID=task_msg_command_driver"));
    assert!(env_dump.contains("AGENT_DID=did:agent:alice-coder"));
    assert!(env_dump.contains("PROFILE_ID=profile_generic_cli_1"));
    assert!(env_dump.contains("WRAPPER=library:awiki_deamon::cli_wrapper"));
}

#[cfg(unix)]
#[test]
fn generic_cli_driver_registry_runs_command_profile() {
    use awiki_deamon::local_rpc::{bind_uds_listener, handle_uds_stream_with_outbox};
    use std::os::unix::fs::PermissionsExt;
    use std::time::{Duration, Instant};

    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    config.ensure_state_layout().unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();
    let profile = profile(root.path().join("workspace"));
    std::fs::create_dir_all(profile.workspace_root.as_ref().unwrap()).unwrap();

    let script_path = root.path().join("registry-driver.sh");
    std::fs::write(
        &script_path,
        r#"#!/bin/sh
set -eu
python3 - "$AWIKI_DAEMON_SOCKET" "$AWIKI_DAEMON_RUNTIME_RPC_TOKEN" "$AWIKI_DAEMON_TASK_ID" <<'PY'
import json
import socket
import sys

socket_path, token, task_id = sys.argv[1:4]
request = {
    "runtime_rpc_token": token,
    "method": "task.finish",
    "params": {"task_id": task_id, "text": "registry finished"},
}
client = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
client.connect(socket_path)
client.sendall((json.dumps(request) + "\n").encode())
response = b""
while not response.endswith(b"\n"):
    chunk = client.recv(4096)
    if not chunk:
        break
    response += chunk
client.close()
decoded = json.loads(response.decode())
if not decoded.get("ok"):
    raise SystemExit(decoded)
PY
"#,
    )
    .unwrap();
    let mut permissions = std::fs::metadata(&script_path).unwrap().permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(&script_path, permissions).unwrap();

    let mut cli_profile =
        CliRuntimeProfileRecord::for_driver(&profile.runtime_profile_id, "command").unwrap();
    cli_profile.binary_path = Some(script_path);
    state.upsert_cli_runtime_profile(&cli_profile).unwrap();

    let outbox = MemoryRuntimeOutbox::default();
    let listener = bind_uds_listener(&config.local_socket_path).unwrap();
    let worker_state = state.clone();
    let worker_outbox = outbox.clone();
    let worker = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match listener.accept() {
                Ok((stream, _)) => {
                    handle_uds_stream_with_outbox(&worker_state, &worker_outbox, stream).unwrap();
                    break;
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if Instant::now() > deadline {
                        panic!("timed out waiting for registry driver RPC");
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("accept registry driver RPC: {error}"),
            }
        }
    });

    let loaded_cli_profile = state
        .load_cli_runtime_profile(&profile.runtime_profile_id)
        .unwrap();
    let plugin = GenericCliDriverRegistry::new(loaded_cli_profile);
    let result = run_controller_text_task_with_config(
        &config,
        &state,
        &profile,
        &plugin,
        &outbox,
        ControllerTextMessage {
            message_id: "msg_registry_driver".to_string(),
            conversation_id: Some("conv_registry_driver".to_string()),
            sender_did: "did:human:alice".to_string(),
            requester_full_handle: None,
            trigger_kind: awiki_deamon::runtime::RuntimeTaskTriggerKind::ControllerDirect,
            target_agent_did: "did:agent:alice-coder".to_string(),
            text: "run registry driver".to_string(),
        },
    )
    .unwrap();
    worker.join().unwrap();

    assert_eq!(result.run.status, RuntimeRunStatus::Finished);
    let records = outbox.records();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].kind, OutboxRecordKind::Final);
    assert_eq!(records[0].text.as_deref(), Some("registry finished"));
}

fn codex_config(binary_path: std::path::PathBuf) -> CodexDriverConfig {
    let config_home = binary_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join("codex-home");
    std::fs::create_dir_all(&config_home).unwrap();
    CodexDriverConfig {
        binary_path,
        config_home,
        profile: Some("awiki".to_string()),
        model: Some("gpt-test".to_string()),
        sandbox: "workspace-write".to_string(),
        ignore_user_config: true,
        ignore_rules: true,
        ephemeral: false,
        output_dir: None,
        cli_wrapper: "library:awiki_deamon::cli_wrapper".to_string(),
    }
}

#[test]
fn codex_driver_command_builder_uses_safe_exec_contract() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    let final_output = root.path().join("final.txt");
    let driver = CodexDriver::new(codex_config(root.path().join("codex"))).unwrap();

    let args = driver.command_args(&workspace, &final_output);

    assert_eq!(args[0], "exec");
    assert!(args
        .windows(2)
        .any(|pair| pair == ["--cd", workspace.to_str().unwrap()]));
    assert!(args
        .windows(2)
        .any(|pair| pair == ["--sandbox", "workspace-write"]));
    assert!(args.windows(2).any(|pair| pair == ["--model", "gpt-test"]));
    assert!(args.windows(2).any(|pair| pair == ["--profile", "awiki"]));
    assert!(args.contains(&"--ignore-user-config".to_string()));
    assert!(args.contains(&"--ignore-rules".to_string()));
    assert!(!args.contains(&"--ephemeral".to_string()));
    assert!(args.contains(&"--json".to_string()));
    assert!(args
        .windows(2)
        .any(|pair| pair == ["--output-last-message", final_output.to_str().unwrap()]));
    assert_eq!(args.last().map(String::as_str), Some("-"));
    assert!(!args.contains(&"danger-full-access".to_string()));
    assert!(!args.contains(&"--dangerously-bypass-approvals-and-sandbox".to_string()));
}

#[test]
fn codex_driver_command_builder_allows_explicit_ephemeral_debug_mode() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    let final_output = root.path().join("final.txt");
    let mut config = codex_config(root.path().join("codex"));
    config.ephemeral = true;
    let driver = CodexDriver::new(config).unwrap();

    let args = driver.command_args(&workspace, &final_output);

    assert!(args.contains(&"--ephemeral".to_string()));
}

#[test]
fn codex_driver_rejects_dangerous_default_sandbox() {
    let root = tempfile::tempdir().unwrap();
    let mut config = codex_config(root.path().join("codex"));
    config.sandbox = "danger-full-access".to_string();

    let error = CodexDriver::new(config).unwrap_err();

    assert!(error.to_string().contains("sandbox"));
}

#[test]
fn codex_driver_config_prefers_driver_config_sandbox_over_profile_default() {
    let root = tempfile::tempdir().unwrap();
    let mut cli_profile =
        CliRuntimeProfileRecord::for_driver("profile_generic_cli_1", "codex").unwrap();
    cli_profile.binary_path = Some(root.path().join("codex-from-profile"));
    let config_home = root.path().join("codex-home-from-profile");
    std::fs::create_dir_all(&config_home).unwrap();
    cli_profile.config_home = Some(config_home.clone());
    cli_profile.default_model = Some("model-from-profile".to_string());
    cli_profile.default_sandbox = Some("read-only".to_string());
    cli_profile.driver_config_json = json!({
        "sandbox": "workspace-write",
        "profile": "codex-profile",
        "ignore_user_config": true,
        "ignore_rules": true,
        "ephemeral": false
    });

    let config = CodexDriverConfig::from_profile(&cli_profile).unwrap();

    assert_eq!(config.binary_path, root.path().join("codex-from-profile"));
    assert_eq!(config.config_home, config_home);
    assert_eq!(config.model.as_deref(), Some("model-from-profile"));
    assert_eq!(config.sandbox, "workspace-write");
    assert_eq!(config.profile.as_deref(), Some("codex-profile"));
    assert!(config.ignore_user_config);
    assert!(config.ignore_rules);
    assert!(!config.ephemeral);
}

#[test]
fn codex_driver_config_requires_profile_config_home() {
    let mut cli_profile =
        CliRuntimeProfileRecord::for_driver("profile_generic_cli_1", "codex").unwrap();
    cli_profile.binary_path = Some(std::path::PathBuf::from("codex"));

    let error = CodexDriverConfig::from_profile(&cli_profile).unwrap_err();

    assert!(error.to_string().contains("config_home"));
}

#[cfg(unix)]
#[test]
fn codex_driver_check_install_status_handles_fake_binary_and_missing_binary() {
    use std::os::unix::fs::PermissionsExt;

    let root = tempfile::tempdir().unwrap();
    let fake_codex = root.path().join("codex");
    std::fs::write(
        &fake_codex,
        r#"#!/bin/sh
if [ "${1-}" = "--version" ]; then
  echo "codex-cli 9.9.9"
  exit 0
fi
exit 0
"#,
    )
    .unwrap();
    let mut permissions = std::fs::metadata(&fake_codex).unwrap().permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(&fake_codex, permissions).unwrap();

    let installed = CodexDriver::new(codex_config(fake_codex))
        .unwrap()
        .check_install_status()
        .unwrap();
    assert!(installed.installed);
    assert!(installed.detail.unwrap().contains("codex-cli 9.9.9"));

    let missing = CodexDriver::new(codex_config(root.path().join("missing-codex")))
        .unwrap()
        .check_install_status()
        .unwrap();
    assert!(!missing.installed);
}

#[cfg(unix)]
#[test]
fn codex_driver_fake_binary_uses_stdin_env_outputs_and_local_rpc() {
    use awiki_deamon::local_rpc::{bind_uds_listener, handle_uds_stream_with_outbox};
    use std::os::unix::fs::PermissionsExt;
    use std::time::{Duration, Instant};

    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    config.ensure_state_layout().unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();
    let profile = profile(root.path().join("workspace"));
    std::fs::create_dir_all(profile.workspace_root.as_ref().unwrap()).unwrap();

    let fake_codex = root.path().join("codex");
    let args_capture = root.path().join("codex-args.txt");
    let env_capture = root.path().join("codex-env.txt");
    let prompt_capture = root.path().join("codex-prompt.txt");
    let output_dir = root.path().join("codex-output");
    std::fs::write(
        &fake_codex,
        format!(
            r#"#!/bin/sh
set -eu
if [ "${{1-}}" = "--version" ]; then
  echo "codex-cli 9.9.9"
  exit 0
fi
printf '%s\n' "$@" > "{args_capture}"
cat > "{prompt_capture}"
cat > "{env_capture}" <<EOF
TASK_TEXT=${{AWIKI_DAEMON_TASK_TEXT-}}
SOCKET=${{AWIKI_DAEMON_SOCKET-}}
RUN_ID=${{AWIKI_DAEMON_RUN_ID-}}
TASK_ID=${{AWIKI_DAEMON_TASK_ID-}}
AGENT_DID=${{AWIKI_DAEMON_AGENT_DID-}}
PROFILE_ID=${{AWIKI_DAEMON_RUNTIME_PROFILE_ID-}}
WRAPPER=${{AWIKI_DAEMON_CLI_WRAPPER-}}
CODEX_HOME=${{CODEX_HOME-}}
EOF
FINAL_OUTPUT=""
PREV=""
for ARG in "$@"; do
  if [ "$PREV" = "--output-last-message" ]; then
    FINAL_OUTPUT="$ARG"
  fi
  PREV="$ARG"
done
printf 'fake codex final %s\n' "$AWIKI_DAEMON_RUNTIME_RPC_TOKEN" > "$FINAL_OUTPUT"
printf '{{"type":"codex-event","message":"stdout jsonl %s"}}\n' "$AWIKI_DAEMON_RUNTIME_RPC_TOKEN"
printf 'stderr diagnostic %s\n' "$AWIKI_DAEMON_RUNTIME_RPC_TOKEN" >&2
python3 - "$AWIKI_DAEMON_SOCKET" "$AWIKI_DAEMON_RUNTIME_RPC_TOKEN" "$AWIKI_DAEMON_TASK_ID" <<'PY'
import json
import socket
import sys

socket_path, token, task_id = sys.argv[1:4]
for request in [
    {{
        "runtime_rpc_token": token,
        "method": "task.status",
        "params": {{"task_id": task_id, "state": "running", "text": "codex started"}},
    }},
    {{
        "runtime_rpc_token": token,
        "method": "task.finish",
        "params": {{"task_id": task_id, "text": "codex finished"}},
    }},
]:
    client = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    client.connect(socket_path)
    client.sendall((json.dumps(request) + "\n").encode())
    response = b""
    while not response.endswith(b"\n"):
        chunk = client.recv(4096)
        if not chunk:
            break
        response += chunk
    client.close()
    decoded = json.loads(response.decode())
    if not decoded.get("ok"):
        raise SystemExit(decoded)
PY
"#,
            args_capture = args_capture.display(),
            prompt_capture = prompt_capture.display(),
            env_capture = env_capture.display(),
        ),
    )
    .unwrap();
    let mut permissions = std::fs::metadata(&fake_codex).unwrap().permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(&fake_codex, permissions).unwrap();

    let outbox = MemoryRuntimeOutbox::default();
    let listener = bind_uds_listener(&config.local_socket_path).unwrap();
    let worker_state = state.clone();
    let worker_outbox = outbox.clone();
    let worker = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut handled = 0;
        while handled < 2 {
            match listener.accept() {
                Ok((stream, _)) => {
                    handle_uds_stream_with_outbox(&worker_state, &worker_outbox, stream).unwrap();
                    handled += 1;
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if Instant::now() > deadline {
                        panic!("timed out waiting for codex driver RPC");
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("accept codex driver RPC: {error}"),
            }
        }
    });

    let mut driver_config = codex_config(fake_codex);
    let binary_path = driver_config.binary_path.clone();
    let codex_home = driver_config.config_home.clone();
    driver_config.output_dir = Some(output_dir.clone());
    let plugin = GenericCliRuntimePlugin::new(CodexDriver::new(driver_config).unwrap());
    let result = run_controller_text_task_with_config(
        &config,
        &state,
        &profile,
        &plugin,
        &outbox,
        ControllerTextMessage {
            message_id: "msg_codex_driver".to_string(),
            conversation_id: Some("conv_codex_driver".to_string()),
            sender_did: "did:human:alice".to_string(),
            requester_full_handle: None,
            trigger_kind: awiki_deamon::runtime::RuntimeTaskTriggerKind::ControllerDirect,
            target_agent_did: "did:agent:alice-coder".to_string(),
            text: "run codex driver".to_string(),
        },
    )
    .unwrap();
    worker.join().unwrap();

    assert_eq!(result.run.status, RuntimeRunStatus::Finished);
    assert_eq!(result.launch_outcome.status, RuntimeRunStatus::Finished);
    assert!(result.launch_outcome.callbacks.is_empty());

    let records = outbox.records();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].kind, OutboxRecordKind::Status);
    assert_eq!(records[1].kind, OutboxRecordKind::Final);
    assert_eq!(records[1].text.as_deref(), Some("codex finished"));

    let args_dump = std::fs::read_to_string(args_capture).unwrap();
    assert!(args_dump.contains("exec\n"));
    assert!(args_dump.contains("--cd\n"));
    assert!(args_dump.contains("--sandbox\nworkspace-write\n"));
    assert!(args_dump.contains("--json\n"));
    assert!(args_dump.contains("--output-last-message\n"));
    assert!(args_dump.ends_with("-\n"));
    assert!(!args_dump.contains("danger-full-access"));
    assert!(!args_dump.contains("dangerously-bypass"));

    let prompt = std::fs::read_to_string(prompt_capture).unwrap();
    assert!(prompt.contains("[Awiki Runtime Context]"));
    assert!(prompt.contains("driver_id: codex"));
    assert!(prompt.contains("message_id: msg_codex_driver"));
    assert!(prompt.contains("conversation_id: conv_codex_driver"));
    assert!(prompt.contains("user_message:\nrun codex driver"));
    assert!(!prompt.contains("rtok_"));
    assert!(!prompt.contains(config.local_socket_path.to_str().unwrap()));

    let env_dump = std::fs::read_to_string(env_capture).unwrap();
    assert!(env_dump.contains("TASK_TEXT=\n"));
    assert!(env_dump.contains("SOCKET="));
    assert!(env_dump.contains("RUN_ID=run_task_msg_codex_driver"));
    assert!(env_dump.contains("TASK_ID=task_msg_codex_driver"));
    assert!(env_dump.contains("AGENT_DID=did:agent:alice-coder"));
    assert!(env_dump.contains("PROFILE_ID=profile_generic_cli_1"));
    assert!(env_dump.contains("WRAPPER=library:awiki_deamon::cli_wrapper"));
    assert!(env_dump.contains(&format!("CODEX_HOME={}", codex_home.display())));

    assert_eq!(
        std::fs::read_to_string(output_dir.join("final-output.txt")).unwrap(),
        "fake codex final <redacted-runtime-rpc-token>\n"
    );
    let stdout_dump = std::fs::read_to_string(output_dir.join("codex-stdout.jsonl")).unwrap();
    assert!(stdout_dump.contains("stdout jsonl"));
    assert!(!stdout_dump.contains("rtok_"));
    let stderr_dump = std::fs::read_to_string(output_dir.join("codex-stderr.log")).unwrap();
    assert!(stderr_dump.contains("stderr diagnostic"));
    assert!(!stderr_dump.contains("rtok_"));
    assert!(
        std::fs::read_to_string(output_dir.join("codex-observation.jsonl"))
            .unwrap()
            .contains("codex.exec.observation")
    );
    assert!(
        !std::fs::read_to_string(output_dir.join("final-output.txt"))
            .unwrap()
            .contains("rtok_")
    );

    let run_record = state.load_cli_driver_run(&result.run.run_id).unwrap();
    assert_eq!(run_record.driver_id, "codex");
    assert_eq!(run_record.controller_did, "did:human:alice");
    assert_eq!(
        run_record.conversation_id.as_deref(),
        Some("conv_codex_driver")
    );
    assert_eq!(
        run_record.route_key,
        "cli:did:agent:alice-coder:controller-scope:v1:test-alice-anpclaw-com:conv_codex_driver:message-run"
    );
    assert_eq!(run_record.workspace_mode, Some(WorkspaceMode::SharedRoot));
    assert!(!run_record.is_security_boundary);
    assert_eq!(
        run_record.workspace_instance_path.as_deref(),
        profile
            .workspace_root
            .as_ref()
            .map(|path| path.canonicalize().unwrap())
            .as_deref()
    );
    assert_eq!(
        run_record.final_output_path,
        Some(output_dir.join("final-output.txt"))
    );
    assert_eq!(run_record.fallback_final_source, None);
    assert_eq!(
        run_record.command_json["program"].as_str(),
        Some(binary_path.to_str().unwrap_or_default())
    );
}

#[cfg(unix)]
#[test]
fn codex_driver_success_without_finish_uses_fallback_final_once() {
    use std::os::unix::fs::PermissionsExt;

    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    config.ensure_state_layout().unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();
    let profile = profile(root.path().join("workspace"));
    std::fs::create_dir_all(profile.workspace_root.as_ref().unwrap()).unwrap();

    let fake_codex = root.path().join("codex-no-final");
    let output_dir = root.path().join("codex-fallback-output");
    std::fs::write(
        &fake_codex,
        r#"#!/bin/sh
if [ "${1-}" = "--version" ]; then
  echo "codex-cli 9.9.9"
  exit 0
fi
FINAL_OUTPUT=""
PREV=""
for ARG in "$@"; do
  if [ "$PREV" = "--output-last-message" ]; then
    FINAL_OUTPUT="$ARG"
  fi
  PREV="$ARG"
done
cat >/dev/null
printf 'fallback final from codex\n' > "$FINAL_OUTPUT"
exit 0
"#,
    )
    .unwrap();
    let mut permissions = std::fs::metadata(&fake_codex).unwrap().permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(&fake_codex, permissions).unwrap();

    let mut driver_config = codex_config(fake_codex);
    driver_config.output_dir = Some(output_dir.clone());
    let plugin = GenericCliRuntimePlugin::new(CodexDriver::new(driver_config).unwrap());
    let outbox = MemoryRuntimeOutbox::default();
    let result = run_controller_text_task_with_config(
        &config,
        &state,
        &profile,
        &plugin,
        &outbox,
        ControllerTextMessage {
            message_id: "msg_codex_fallback".to_string(),
            conversation_id: Some("conv_codex_fallback".to_string()),
            sender_did: "did:human:alice".to_string(),
            requester_full_handle: None,
            trigger_kind: awiki_deamon::runtime::RuntimeTaskTriggerKind::ControllerDirect,
            target_agent_did: "did:agent:alice-coder".to_string(),
            text: "run codex without explicit final".to_string(),
        },
    )
    .unwrap();

    assert_eq!(result.run.status, RuntimeRunStatus::Finished);
    assert_eq!(result.launch_outcome.status, RuntimeRunStatus::Finished);
    assert!(result.launch_outcome.callbacks.is_empty());
    let records = outbox.records();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].kind, OutboxRecordKind::Status);
    assert_eq!(records[0].state.as_deref(), Some("running"));
    assert_eq!(records[1].kind, OutboxRecordKind::Final);
    assert_eq!(
        records[1].text.as_deref(),
        Some("fallback final from codex")
    );
    let run_record = state.load_cli_driver_run(&result.run.run_id).unwrap();
    assert_eq!(
        run_record.fallback_final_source.as_deref(),
        Some("codex_output_last_message")
    );
    assert_eq!(
        run_record.final_output_path,
        Some(output_dir.join("final-output.txt"))
    );
}

#[cfg(unix)]
#[test]
fn worktree_per_task_uses_daemon_runtime_temp_and_records_metadata() {
    use std::os::unix::fs::PermissionsExt;

    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    config.ensure_state_layout().unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();
    let workspace = root.path().join("git-workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    assert!(std::process::Command::new("git")
        .args(["-c", "init.defaultBranch=main"])
        .args(["init"])
        .arg(&workspace)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap()
        .success());
    std::fs::write(workspace.join("README.md"), "workspace\n").unwrap();
    assert!(std::process::Command::new("git")
        .arg("-C")
        .arg(&workspace)
        .args(["add", "README.md"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap()
        .success());
    assert!(std::process::Command::new("git")
        .arg("-C")
        .arg(&workspace)
        .args([
            "-c",
            "user.name=Awiki Test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "-m",
            "init",
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap()
        .success());
    let mut profile = profile(workspace.clone());
    profile.workspace_mode = Some(WorkspaceMode::WorktreePerTask);

    let fake_codex = root.path().join("codex-worktree");
    let pwd_capture = root.path().join("codex-pwd.txt");
    std::fs::write(
        &fake_codex,
        format!(
            r#"#!/bin/sh
if [ "${{1-}}" = "--version" ]; then
  echo "codex-cli 9.9.9"
  exit 0
fi
pwd > "{pwd_capture}"
FINAL_OUTPUT=""
PREV=""
for ARG in "$@"; do
  if [ "$PREV" = "--output-last-message" ]; then
    FINAL_OUTPUT="$ARG"
  fi
  PREV="$ARG"
done
cat >/dev/null
printf 'worktree final\n' > "$FINAL_OUTPUT"
exit 0
"#,
            pwd_capture = pwd_capture.display(),
        ),
    )
    .unwrap();
    let mut permissions = std::fs::metadata(&fake_codex).unwrap().permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(&fake_codex, permissions).unwrap();

    let plugin = GenericCliRuntimePlugin::new(CodexDriver::new(codex_config(fake_codex)).unwrap());
    let outbox = MemoryRuntimeOutbox::default();
    let result = run_controller_text_task_with_config(
        &config,
        &state,
        &profile,
        &plugin,
        &outbox,
        ControllerTextMessage {
            message_id: "msg_codex_worktree".to_string(),
            conversation_id: None,
            sender_did: "did:human:alice".to_string(),
            requester_full_handle: None,
            trigger_kind: awiki_deamon::runtime::RuntimeTaskTriggerKind::ControllerDirect,
            target_agent_did: "did:agent:alice-coder".to_string(),
            text: "run codex in worktree".to_string(),
        },
    )
    .unwrap();

    assert_eq!(result.run.status, RuntimeRunStatus::Finished);
    let run_record = state.load_cli_driver_run(&result.run.run_id).unwrap();
    let instance_path = run_record.workspace_instance_path.unwrap();
    let worktrees_root = config
        .runtime_temp_dir
        .join("worktrees")
        .canonicalize()
        .unwrap();
    assert!(instance_path.starts_with(worktrees_root));
    assert_ne!(instance_path, workspace.canonicalize().unwrap());
    assert_eq!(
        run_record.workspace_mode,
        Some(WorkspaceMode::WorktreePerTask)
    );
    assert!(!run_record.is_security_boundary);
    assert_eq!(
        std::fs::read_to_string(pwd_capture).unwrap().trim(),
        instance_path.to_str().unwrap()
    );
    assert_eq!(
        run_record.route_key,
        "cli:did:agent:alice-coder:controller-scope:v1:test-alice-anpclaw-com:no-conversation:message-run"
    );
}

#[cfg(unix)]
#[test]
fn codex_driver_nonzero_exit_does_not_forge_success_final() {
    use std::os::unix::fs::PermissionsExt;

    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    config.ensure_state_layout().unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();
    let profile = profile(root.path().join("workspace"));
    std::fs::create_dir_all(profile.workspace_root.as_ref().unwrap()).unwrap();

    let fake_codex = root.path().join("codex-fail");
    let output_dir = root.path().join("codex-fail-output");
    std::fs::write(
        &fake_codex,
        r#"#!/bin/sh
if [ "${1-}" = "--version" ]; then
  echo "codex-cli 9.9.9"
  exit 0
fi
cat >/dev/null
echo "codex failed" >&2
exit 42
"#,
    )
    .unwrap();
    let mut permissions = std::fs::metadata(&fake_codex).unwrap().permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(&fake_codex, permissions).unwrap();

    let mut driver_config = codex_config(fake_codex);
    driver_config.output_dir = Some(output_dir.clone());
    let plugin = GenericCliRuntimePlugin::new(CodexDriver::new(driver_config).unwrap());
    let outbox = MemoryRuntimeOutbox::default();
    let result = run_controller_text_task_with_config(
        &config,
        &state,
        &profile,
        &plugin,
        &outbox,
        ControllerTextMessage {
            message_id: "msg_codex_failed".to_string(),
            conversation_id: Some("conv_codex_failed".to_string()),
            sender_did: "did:human:alice".to_string(),
            requester_full_handle: None,
            trigger_kind: awiki_deamon::runtime::RuntimeTaskTriggerKind::ControllerDirect,
            target_agent_did: "did:agent:alice-coder".to_string(),
            text: "run failing codex driver".to_string(),
        },
    )
    .unwrap();

    assert_eq!(result.run.status, RuntimeRunStatus::Failed);
    assert_eq!(result.launch_outcome.status, RuntimeRunStatus::Failed);
    assert_eq!(result.launch_outcome.exit_code, Some(42));
    assert!(result.launch_outcome.callbacks.is_empty());
    let records = outbox.records();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].kind, OutboxRecordKind::Status);
    assert_eq!(records[0].state.as_deref(), Some("running"));
    assert_eq!(records[1].kind, OutboxRecordKind::Status);
    assert_eq!(records[1].state.as_deref(), Some("failed"));
    assert_eq!(
        records[1].last_error_summary.as_deref(),
        Some("Runtime exited with status 42")
    );
    assert!(std::fs::read_to_string(output_dir.join("codex-stderr.log"))
        .unwrap()
        .contains("codex failed"));
}

#[test]
fn controller_text_route_preserves_verified_current_sender_did() {
    let profile = profile(std::env::temp_dir().join("awiki-workspace"));
    let verified_sender = VerifiedControllerSender {
        controller_user_id: profile.controller_user_id.clone(),
        controller_full_handle: profile.controller_full_handle.clone(),
        controller_scope_key: profile.controller_scope_key.clone(),
        controller_did: "did:human:alice-new".to_string(),
        sender_did: "did:human:alice-new".to_string(),
    };
    let task = route_controller_text_task_with_verified_sender(
        &profile,
        &verified_sender,
        ControllerTextMessage {
            message_id: "msg_002".to_string(),
            conversation_id: None,
            sender_did: "did:human:alice-new".to_string(),
            requester_full_handle: None,
            trigger_kind: awiki_deamon::runtime::RuntimeTaskTriggerKind::ControllerDirect,
            target_agent_did: "did:agent:alice-coder".to_string(),
            text: "请执行".to_string(),
        },
    )
    .unwrap();

    assert_eq!(task.controller_did, "did:human:alice-new");
    assert_eq!(task.sender_did, "did:human:alice-new");
    assert_eq!(task.controller_scope_key, profile.controller_scope_key);
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

#[test]
fn generic_cli_invocation_debug_redacts_task_text_and_token() {
    let invocation = awiki_deamon::plugins::generic_cli::GenericCliInvocation {
        run_id: "run_debug".to_string(),
        task_id: "task_debug".to_string(),
        message_id: "debug".to_string(),
        conversation_id: Some("conv_debug".to_string()),
        task_text: "secret prompt rtok_debug_secret_value_123456789".to_string(),
        agent_did: "did:agent:debug".to_string(),
        runtime_profile_id: "profile_debug".to_string(),
        workspace_root: None,
        workspace_instance: None,
        runtime_temp_dir: None,
        runtime_rpc_token: "rtok_debug_secret_value_123456789".to_string(),
        local_socket_path: None,
        callbacks: Vec::new(),
    };

    let debug = format!("{invocation:?}");
    assert!(debug.contains("<redacted-task-text>"));
    assert!(debug.contains("<redacted>"));
    assert!(!debug.contains("secret prompt"));
    assert!(!debug.contains("rtok_debug_secret_value_123456789"));
}
