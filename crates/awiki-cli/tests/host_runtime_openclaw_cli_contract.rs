use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn openclaw_set_dry_run_echoes_remote_hook_url_before_live_validation_like_go() {
    let workspace = TempDir::new().expect("temp workspace");

    let dry_run = awiki_cmd_with_workspace(
        &[
            "--dry-run",
            "runtime",
            "host-notify",
            "openclaw",
            "set",
            "--hook-url",
            "http://10.0.0.1:18789/hooks/agent",
        ],
        workspace.path(),
    );
    assert_success(&dry_run);
    let envelope = success_json(&dry_run);
    assert_eq!(
        envelope["summary"],
        "Dry run: OpenClaw host notify config change planned"
    );
    assert_eq!(
        envelope["data"]["plan"]["action"],
        "host_notify_openclaw_set"
    );
    assert_eq!(
        envelope["data"]["plan"]["hook_url"],
        "http://10.0.0.1:18789/hooks/agent"
    );
    assert!(
        !workspace.path().join("config.yaml").exists(),
        "dry-run openclaw set must not write config.yaml"
    );

    let live = awiki_cmd_with_workspace(
        &[
            "runtime",
            "host-notify",
            "openclaw",
            "set",
            "--hook-url",
            "http://10.0.0.1:18789/hooks/agent",
        ],
        workspace.path(),
    );
    assert_code(&live, 2);
    let envelope = error_json(&live);
    assert_eq!(envelope["error"]["code"], "invalid_argument");
    assert_contains(
        &envelope["error"]["message"],
        "runtime.host_notify.openclaw.hook_url must use a loopback host",
    );
}

fn awiki_cmd_with_workspace(args: &[&str], workspace: &Path) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_awiki-cli"));
    command
        .args(args)
        .env("AWIKI_CLI_WORKSPACE_HOME_DIR", workspace)
        .env("HOME", workspace.join("home"))
        .env("USERPROFILE", workspace.join("home"))
        .env("AWIKI_CLI_UPDATE_CACHE_ONLY", "1")
        .env_remove("OPENCLAW_CONFIG_PATH")
        .env_remove("OPENCLAW_GATEWAY_PORT")
        .env_remove("OPENCLAW_HOOK_TOKEN")
        .env_remove("AWIKI_WORKSPACE")
        .env_remove("AWIKI_WORKSPACE_HOME")
        .env_remove("AWIKI_HOME")
        .env_remove("AVIKI_WORKSPACE_HOME")
        .env_remove("AWIKI_FORMAT")
        .env_remove("AVIKI_FORMAT");
    command.output().expect("run awiki-cli binary")
}

fn assert_success(output: &Output) {
    assert_code(output, 0);
}

fn assert_code(output: &Output, code: i32) {
    assert_eq!(
        output.status.code(),
        Some(code),
        "unexpected exit status; stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn success_json(output: &Output) -> Value {
    assert_stderr_empty(output);
    let envelope: Value =
        serde_json::from_slice(&output.stdout).expect("stdout should be a JSON success envelope");
    assert_eq!(envelope["ok"], true);
    envelope
}

fn error_json(output: &Output) -> Value {
    assert_stdout_empty(output);
    let envelope: Value =
        serde_json::from_slice(&output.stderr).expect("stderr should be a JSON error envelope");
    assert_eq!(envelope["ok"], false);
    envelope
}

fn assert_stdout_empty(output: &Output) {
    assert!(
        output.stdout.is_empty(),
        "stdout should be empty:\n{}",
        String::from_utf8_lossy(&output.stdout)
    );
}

fn assert_stderr_empty(output: &Output) {
    assert!(
        output.stderr.is_empty(),
        "stderr should be empty:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn assert_contains(value: &Value, needle: &str) {
    let haystack = value.as_str().expect("value should be a string");
    assert!(
        haystack.contains(needle),
        "{haystack:?} should contain {needle:?}"
    );
}

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new() -> std::io::Result<Self> {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("awiki-cli-rs2-test-{}-{nanos}", std::process::id()));
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
