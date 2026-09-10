use super::*;
use std::io::{BufRead, Write};
use std::os::unix::{fs::PermissionsExt, net::UnixListener};
use std::time::Duration;

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
