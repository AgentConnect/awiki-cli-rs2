#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[test]
fn foreground_listener_exits_and_cleans_runtime_artifacts_on_sigterm_like_go() {
    foreground_listener_exits_and_cleans_runtime_artifacts_on_signal(SIGTERM);
}

#[test]
fn foreground_listener_exits_and_cleans_runtime_artifacts_on_sigint_like_go() {
    foreground_listener_exits_and_cleans_runtime_artifacts_on_signal(SIGINT);
}

fn foreground_listener_exits_and_cleans_runtime_artifacts_on_signal(signal: std::os::raw::c_int) {
    let workspace = TempDir::new("listener-signal").expect("temp workspace");
    write_runtime_config(workspace.path());
    let mut child = awiki_command(workspace.path())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn foreground listener");

    let runtime_dir = workspace.path().join("runtime");
    let pid_file = runtime_dir.join("listener.pid");
    let status_file = runtime_dir.join("listener.status.json");
    let socket_path = runtime_dir.join("message-daemon.sock");

    wait_for_path(&pid_file);
    wait_for_path(&status_file);
    wait_for_path(&socket_path);
    terminate_with_signal(&mut child, signal);
    let output = wait_for_child_output(child, Duration::from_secs(10));

    assert_success(&output);
    wait_for_removed(&pid_file);
    wait_for_removed(&status_file);
    wait_for_removed(&socket_path);
    assert!(
        output.stdout.is_empty(),
        "foreground listener should not render stdout on clean signal shutdown:\n{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        output.stderr.is_empty(),
        "foreground listener should not render stderr on clean signal shutdown:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn awiki_command(workspace: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_awiki-cli"));
    command
        .args(["runtime", "listener", "run"])
        .env("AWIKI_CLI_WORKSPACE_HOME_DIR", workspace)
        .env("AWIKI_CLI_UPDATE_CACHE_ONLY", "1")
        .env_remove("AWIKI_WORKSPACE")
        .env_remove("AWIKI_WORKSPACE_HOME")
        .env_remove("AWIKI_HOME")
        .env_remove("AVIKI_WORKSPACE_HOME")
        .env_remove("AWIKI_FORMAT")
        .env_remove("AVIKI_FORMAT");
    command
}

fn write_runtime_config(workspace: &Path) {
    std::fs::create_dir_all(workspace).expect("workspace dir");
    std::fs::write(
        workspace.join("config.yaml"),
        "runtime:\n  mode: websocket\n  listener:\n    enabled: true\n  host_notify:\n    enabled: true\n    sink: log\n",
    )
    .expect("write config");
}

fn wait_for_path(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if path.exists() {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("timed out waiting for {}", path.display());
}

fn wait_for_removed(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if !path.exists() {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("timed out waiting for {} removal", path.display());
}

fn wait_for_child_output(mut child: Child, timeout: Duration) -> Output {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if let Some(_status) = child.try_wait().expect("poll foreground listener") {
            return child.wait_with_output().expect("wait foreground listener");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let _ = child.kill();
    let output = child.wait_with_output().expect("kill foreground listener");
    panic!(
        "foreground listener did not exit after signal; stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn assert_success(output: &Output) {
    assert_eq!(
        output.status.code(),
        Some(0),
        "unexpected exit status; stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn terminate_with_signal(child: &mut Child, signal: std::os::raw::c_int) {
    let result = unsafe { kill(child.id() as std::os::raw::c_int, signal) };
    assert_eq!(result, 0, "kill({signal}) failed");
}

const SIGINT: std::os::raw::c_int = 2;
const SIGTERM: std::os::raw::c_int = 15;

extern "C" {
    fn kill(pid: std::os::raw::c_int, sig: std::os::raw::c_int) -> std::os::raw::c_int;
}

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(name: &str) -> std::io::Result<Self> {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "awiki-cli-rs2-{name}-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path)?;
        Ok(Self { path })
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
