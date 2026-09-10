use super::*;
use std::io::{BufRead, Write};
use std::os::unix::{fs::PermissionsExt, net::UnixListener};
use std::time::Duration;

#[test]
fn codex_late_recovery_after_task_finish_does_not_emit_a_second_final() {
    assert_late_recovery_keeps_single_final(false);
}

#[test]
fn codex_group_late_recovery_after_task_finish_does_not_emit_a_second_final() {
    assert_late_recovery_keeps_single_final(true);
}

fn assert_late_recovery_keeps_single_final(group: bool) {
    use awiki_deamon::local_rpc::{bind_uds_listener, handle_uds_stream_with_outbox};

    let _guard = generic_cli_process_test_guard();
    let _env = EnvVarGuard::set(&[("AWIKI_DAEMON_AGENT_PROXY_MODE", "inherit")]);
    let root = tempfile::tempdir_in("/tmp").unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    config.ensure_state_layout().unwrap();
    let state = DaemonState::open(&config).unwrap();
    state.initialize().unwrap();
    let profile = profile(root.path().join("workspace"));
    upsert_test_cli_profile(&state, &profile.runtime_profile_id, "codex");
    std::fs::create_dir_all(profile.workspace_root.as_ref().unwrap()).unwrap();
    let fake = root.path().join("codex");
    std::fs::write(
        &fake,
        r#"#!/usr/bin/env python3
import json, os, pathlib, socket, sys, time
if sys.argv[1:] == ["--version"]:
    print("fake-codex 1.0")
    sys.exit(0)
sys.stdin.read()
root = pathlib.Path(__file__).parent
with (root / "executions").open("a") as f:
    f.write("run\n")
pathlib.Path(sys.argv[sys.argv.index("--output-last-message") + 1]).write_text("duplicate fallback")
def wait_for(name):
    deadline = time.monotonic() + 5
    while not (root / name).exists():
        if time.monotonic() > deadline:
            raise RuntimeError("missing RPC barrier " + name)
        time.sleep(0.005)
print(json.dumps({"type": "error", "message": "Reconnecting... 1/5"}), flush=True)
wait_for("rpc-1")
with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as client:
    client.settimeout(5)
    client.connect(os.environ["AWIKI_DAEMON_SOCKET"])
    client.sendall((json.dumps({
        "runtime_rpc_token": os.environ["AWIKI_DAEMON_RUNTIME_RPC_TOKEN"],
        "method": "task.finish",
        "params": {"task_id": os.environ["AWIKI_DAEMON_TASK_ID"], "text": "single callback final"}
    }) + "\n").encode())
    assert json.loads(client.makefile().readline())["ok"]
print(json.dumps({"type": "turn.completed"}), flush=True)
wait_for("rpc-3")
"#,
    )
    .unwrap();
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o700)).unwrap();
    let outbox = MemoryRuntimeOutbox::default();
    let listener = bind_uds_listener(&config.local_socket_path).unwrap();
    let worker_state = state.clone();
    let worker_outbox = outbox.clone();
    let barrier_root = root.path().to_path_buf();
    let worker = std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        for index in 1..=3 {
            let stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(std::time::Instant::now() < deadline, "missing Codex RPC");
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => panic!("{error}"),
                }
            };
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            handle_uds_stream_with_outbox(&worker_state, &worker_outbox, stream).unwrap();
            std::fs::write(barrier_root.join(format!("rpc-{index}")), "accepted").unwrap();
        }
    });
    let plugin = GenericCliRuntimePlugin::new(CodexDriver::new(codex_config(fake)).unwrap());
    let result = run_controller_text_task_with_config(
        &config,
        &state,
        &profile,
        &plugin,
        &outbox,
        ControllerTextMessage {
            message_id: "msg_codex_late_recovery".to_string(),
            conversation_id: Some(
                if group {
                    "group:did:example:codex-late-recovery"
                } else {
                    "conv_codex_late_recovery"
                }
                .to_string(),
            ),
            sender_did: "did:human:alice".to_string(),
            requester_user_id: group.then(|| "user-alice".to_string()),
            requester_full_handle: group.then(|| "alice.anpclaw.com".to_string()),
            trigger_kind: if group {
                RuntimeTaskTriggerKind::GroupMention
            } else {
                RuntimeTaskTriggerKind::ControllerDirect
            },
            invocation_authority: RuntimeInvocationAuthority::Controller,
            target_agent_did: profile.agent_did.clone(),
            text: "finish before transport recovery".to_string(),
        },
    )
    .unwrap();
    worker.join().unwrap();
    assert_eq!(result.run.status, RuntimeRunStatus::Finished);
    assert_eq!(
        result.launch_outcome.metadata["progress_observation"]["report_successes"],
        2
    );
    assert_eq!(
        std::fs::read_to_string(root.path().join("executions")).unwrap(),
        "run\n"
    );
    let records = outbox.records();
    assert_eq!(
        records
            .iter()
            .map(|record| &record.kind)
            .collect::<Vec<_>>(),
        vec![
            &OutboxRecordKind::Status,
            &OutboxRecordKind::Status,
            &OutboxRecordKind::Final
        ]
    );
    assert_eq!(
        records[1].metadata.as_ref().unwrap()["progress"]["code"],
        "external_service_delayed"
    );
    assert_eq!(records[2].text.as_deref(), Some("single callback final"));
    assert_eq!(
        state
            .load_cli_driver_run(&result.run.run_id)
            .unwrap()
            .fallback_final_source,
        None
    );
}

#[test]
fn codex_network_progress_arrives_via_rpc_during_a_single_execution() {
    let _guard = generic_cli_process_test_guard();
    let _env = EnvVarGuard::set(&[
        ("HTTPS_PROXY", "http://remote.example:46321"),
        ("NO_PROXY", "private.example"),
        ("AWIKI_DAEMON_AGENT_PROXY_MODE", "inherit"),
    ]);
    let root = tempfile::tempdir().unwrap();
    let fake = root.path().join("codex");
    std::fs::write(
        &fake,
        r#"#!/bin/sh
set -eu
if [ "${1-}" = "--version" ]; then
  printf '%s\n%s\n' "$HTTPS_PROXY" "$NO_PROXY" > "$(dirname "$0")/probe-env"
  echo 'fake-cli 1.0'
  exit 0
fi
cat >/dev/null
printf '%s\n%s\n' "$HTTPS_PROXY" "$NO_PROXY" > run-env
printf 'run\n' >> executions
printf '{"type":"error","message":"Reconnecting... 1/5"}\n'
sleep 0.2
printf 'Falling back from WebSockets to HTTPS transport.\n' >&2
sleep 0.2
printf '{"type":"item.completed","item":{"type":"agent_message","text":"done"}}\n'
sleep 0.2
touch finished
"#,
    )
    .unwrap();
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o700)).unwrap();
    let workspace = root.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    let invocation = generic_cli_invocation_for_process_test(root.path(), &workspace);
    let socket = invocation.local_socket_path.as_ref().unwrap();
    let listener = UnixListener::bind(socket).unwrap();
    std::fs::set_permissions(socket, std::fs::Permissions::from_mode(0o600)).unwrap();
    listener.set_nonblocking(true).unwrap();
    let finished = workspace.join("finished");
    let worker = std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let mut requests = Vec::new();
        while requests.len() < 2 {
            let (mut stream, _) = match listener.accept() {
                Ok(stream) => stream,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(
                        std::time::Instant::now() < deadline,
                        "missing live progress"
                    );
                    std::thread::sleep(Duration::from_millis(5));
                    continue;
                }
                Err(error) => panic!("{error}"),
            };
            stream
                .set_read_timeout(Some(Duration::from_secs(1)))
                .unwrap();
            let mut line = String::new();
            std::io::BufReader::new(stream.try_clone().unwrap())
                .read_line(&mut line)
                .unwrap();
            assert!(!finished.exists());
            requests.push(serde_json::from_str::<serde_json::Value>(&line).unwrap());
            stream.write_all(b"{\"ok\":true,\"result\":{}}\n").unwrap();
        }
        requests
    });
    let driver = CodexDriver::new(codex_config(fake)).unwrap();
    assert!(driver.check_install_status().unwrap().installed);
    let outcome = driver.run(invocation).unwrap();
    let requests = worker.join().unwrap();
    assert_eq!(outcome.status, RuntimeRunStatus::Finished);
    assert_eq!(
        outcome.metadata["progress_observation"]["report_successes"],
        2
    );
    assert_eq!(
        std::fs::read_to_string(workspace.join("executions")).unwrap(),
        "run\n"
    );
    assert_eq!(
        std::fs::read_to_string(root.path().join("probe-env")).unwrap(),
        std::fs::read_to_string(workspace.join("run-env")).unwrap()
    );
    for (request, code) in requests
        .iter()
        .zip(["external_service_delayed", "external_service_resumed"])
    {
        assert_eq!(request["method"], "task.status");
        assert_eq!(request["params"]["state"], "running");
        assert_eq!(request["params"]["progress"]["code"], code);
    }
    assert_eq!(
        std::env::var("NO_PROXY").unwrap(),
        "private.example",
        "daemon environment must not change"
    );
    let child = std::fs::read_to_string(workspace.join("run-env")).unwrap();
    assert!(child.contains("http://remote.example:46321"));
    assert!(child.contains("localhost") && child.contains("private.example"));
}
