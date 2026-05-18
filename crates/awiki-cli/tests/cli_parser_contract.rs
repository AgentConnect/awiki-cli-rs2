use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn unknown_local_flags_fail_like_go_cobra_before_handler_execution() {
    for args in [
        &["status", "--bogus"][..],
        &["version", "--bogus=value"][..],
        &[
            "config",
            "set",
            "--did-domain",
            "awiki.info",
            "--bogus",
            "value",
        ][..],
    ] {
        let output = awiki_cmd(args);
        assert_code(&output, 1);
        assert_stdout_empty(&output);
        let envelope = error_json(&output);
        assert_eq!(envelope["error"]["code"], "internal_error");
        assert_eq!(envelope["error"]["message"], "unknown flag: --bogus");
        assert_eq!(envelope["error"]["hint"], Value::Null);
    }

    let output = awiki_cmd(&["status", "--format", "json"]);
    assert_success(&output);
    let envelope = success_json(&output);
    assert_eq!(envelope["command"], "awiki-cli status");
}

#[test]
fn unknown_shorthand_flags_fail_like_go_cobra_before_handler_execution() {
    for (args, message) in [
        (&["status", "-v"][..], "unknown shorthand flag: 'v' in -v"),
        (
            &[
                "group",
                "get",
                "-g",
                "did:wba:awiki.ai:groups:demo:e1_group",
            ][..],
            "unknown shorthand flag: 'g' in -g",
        ),
        (&["status", "-vh"][..], "unknown shorthand flag: 'v' in -vh"),
    ] {
        let output = awiki_cmd(args);
        assert_code(&output, 1);
        assert_stdout_empty(&output);
        let envelope = error_json(&output);
        assert_eq!(envelope["error"]["code"], "internal_error");
        assert_eq!(envelope["error"]["message"], message);
        assert_eq!(envelope["error"]["hint"], Value::Null);
    }

    let output = awiki_cmd(&["status", "-"]);
    assert_success(&output);
    let envelope = success_json(&output);
    assert_eq!(envelope["command"], "awiki-cli status");
}

fn awiki_cmd(args: &[&str]) -> Output {
    let workspace = TempDir::new().expect("temp workspace");
    let mut command = Command::new(env!("CARGO_BIN_EXE_awiki-cli"));
    command
        .args(args)
        .env("AWIKI_CLI_WORKSPACE_HOME_DIR", workspace.path())
        .env("AWIKI_CLI_UPDATE_CACHE_ONLY", "1")
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

fn assert_stdout_empty(output: &Output) {
    assert!(
        output.stdout.is_empty(),
        "stdout should be empty, got {}",
        String::from_utf8_lossy(&output.stdout)
    );
}

fn success_json(output: &Output) -> Value {
    assert!(
        output.stderr.is_empty(),
        "stderr should be empty, got {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let envelope: Value =
        serde_json::from_slice(&output.stdout).expect("stdout should be a JSON success envelope");
    assert_eq!(envelope["ok"], true, "success envelope should set ok=true");
    envelope
}

fn error_json(output: &Output) -> Value {
    let envelope: Value =
        serde_json::from_slice(&output.stderr).expect("stderr should be a JSON error envelope");
    assert_eq!(envelope["ok"], false, "error envelope should set ok=false");
    assert_eq!(envelope["error"]["retryable"], false);
    envelope
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
