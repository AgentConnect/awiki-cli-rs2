use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn onboarding_claim_requires_stdin_and_rejects_token_argument() {
    let missing_stdin = awiki_cmd(&[
        "onboarding",
        "claim",
        "--service-base-url",
        "https://awiki.info",
        "--expected-controller-handle",
        "alice.awiki.info",
        "--expected-agent-handle",
        "skill-test.awiki.info",
    ]);
    assert_code(&missing_stdin, 2);
    assert_stdout_empty(&missing_stdin);
    let error = error_json(&missing_stdin);
    assert_eq!(error["error"]["code"], "invalid_argument");
    assert_text_contains(
        error["error"]["message"].as_str().unwrap_or_default(),
        "--token-stdin",
    );

    let raw = "awsk1_do-not-render-this-value";
    let token_argument = awiki_cmd(&[
        "onboarding",
        "claim",
        "--token",
        raw,
        "--service-base-url",
        "https://awiki.info",
        "--expected-controller-handle",
        "alice.awiki.info",
        "--expected-agent-handle",
        "skill-test.awiki.info",
    ]);
    assert_code(&token_argument, 2);
    assert_stdout_empty(&token_argument);
    assert!(!String::from_utf8_lossy(&token_argument.stderr).contains(raw));
    assert_eq!(
        error_json(&token_argument)["error"]["code"],
        "invalid_argument"
    );
}

#[test]
fn onboarding_legacy_recovery_requires_stdin_and_never_accepts_token_argv() {
    let missing_stdin = awiki_cmd(&[
        "onboarding",
        "recover-legacy-claim",
        "--service-base-url",
        "https://awiki.info",
        "--expected-controller-handle",
        "alice.awiki.info",
        "--expected-agent-handle",
        "skill-test.awiki.info",
    ]);
    assert_code(&missing_stdin, 2);
    assert_stdout_empty(&missing_stdin);
    assert_text_contains(
        error_json(&missing_stdin)["error"]["message"]
            .as_str()
            .unwrap_or_default(),
        "--token-stdin",
    );

    let raw = "awsk1_legacy-recovery-secret";
    let token_argument = awiki_cmd(&[
        "onboarding",
        "recover-legacy-claim",
        "--token",
        raw,
        "--service-base-url",
        "https://awiki.info",
        "--expected-controller-handle",
        "alice.awiki.info",
        "--expected-agent-handle",
        "skill-test.awiki.info",
    ]);
    assert_code(&token_argument, 2);
    assert_stdout_empty(&token_argument);
    assert!(!String::from_utf8_lossy(&token_argument.stderr).contains(raw));
    assert_eq!(
        error_json(&token_argument)["error"]["code"],
        "invalid_argument"
    );
}

#[test]
fn unknown_local_flags_are_reported_as_invalid_arguments_before_handler_execution() {
    for args in [
        &["status", "--bogus"][..],
        &["version", "--bogus=value"][..],
        &[
            "tenant",
            "create",
            "acme",
            "--backend-base-url",
            "https://api.acme.test",
            "--did-host",
            "acme.test",
            "--bogus",
            "value",
        ][..],
    ] {
        let output = awiki_cmd(args);
        assert_code(&output, 2);
        assert_stdout_empty(&output);
        let envelope = error_json(&output);
        assert_eq!(envelope["error"]["code"], "invalid_argument");
        assert_eq!(envelope["error"]["message"], "unknown flag: --bogus");
        assert_eq!(envelope["error"]["hint"], Value::Null);
    }

    let output = awiki_cmd(&["status", "--format", "json"]);
    assert_success(&output);
    let envelope = success_json(&output);
    assert_eq!(envelope["command"], "awiki-cli status");
}

#[test]
fn unknown_shorthand_flags_are_reported_as_invalid_arguments_before_handler_execution() {
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
        assert_code(&output, 2);
        assert_stdout_empty(&output);
        let envelope = error_json(&output);
        assert_eq!(envelope["error"]["code"], "invalid_argument");
        assert_eq!(envelope["error"]["message"], message);
        assert_eq!(envelope["error"]["hint"], Value::Null);
    }

    let output = awiki_cmd(&["status", "-"]);
    assert_success(&output);
    let envelope = success_json(&output);
    assert_eq!(envelope["command"], "awiki-cli status");
}

#[test]
fn unknown_global_long_flags_are_reported_as_invalid_arguments_before_missing_command() {
    for args in [
        &["--bogus", "status"][..],
        &["--bogus"][..],
        &["--format", "json", "--bogus", "status"][..],
    ] {
        let output = awiki_cmd(args);
        assert_code(&output, 2);
        assert_stdout_empty(&output);
        let envelope = error_json(&output);
        assert_eq!(envelope["error"]["code"], "invalid_argument");
        assert_eq!(envelope["error"]["message"], "unknown flag: --bogus");
        assert_eq!(envelope["error"]["hint"], Value::Null);
    }
}

#[test]
fn root_help_prints_default_command_surface() {
    for args in [&["--help"][..], &["-h"][..]] {
        let output = awiki_cmd(args);
        assert_success(&output);
        assert_stderr_empty(&output);
        let text = stdout_text(&output);
        assert_text_contains(&text, "AWiki CLI");
        assert_text_contains(&text, "Usage:");
        assert_text_contains(&text, "Commands:");
        assert_text_contains(&text, "tenant");
        assert_text_contains(&text, "schema [COMMAND]");
    }
}

#[test]
fn command_help_prints_human_readable_text() {
    for (args, expected_usage, expected_text) in [
        (
            &["tenant", "--help"][..],
            "awiki-cli tenant",
            "Manage backend and DID host tenants",
        ),
        (
            &["tenant", "create", "--help"][..],
            "awiki-cli tenant create <name>",
            "--backend-base-url <string>",
        ),
        (
            &["tenant", "current", "-h"][..],
            "awiki-cli tenant current",
            "Show the current tenant",
        ),
    ] {
        let output = awiki_cmd(args);
        assert_success(&output);
        assert_stderr_empty(&output);
        let text = stdout_text(&output);
        assert_text_contains(&text, expected_usage);
        assert_text_contains(&text, expected_text);
        assert_text_contains(&text, "machine-readable command metadata");
        assert!(
            serde_json::from_str::<Value>(&text).is_err(),
            "help should be human-readable text, not a JSON envelope: {text}"
        );
    }
}

fn awiki_cmd(args: &[&str]) -> Output {
    let workspace = TempDir::new().expect("temp workspace");
    let mut command = Command::new(env!("CARGO_BIN_EXE_awiki-cli"));
    command
        .args(args)
        .env("AWIKI_CLI_WORKSPACE_HOME_DIR", workspace.path())
        .env("HOME", workspace.path().join("home"))
        .env("USERPROFILE", workspace.path().join("home"))
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

fn assert_stderr_empty(output: &Output) {
    assert!(
        output.stderr.is_empty(),
        "stderr should be empty, got {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn stdout_text(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn assert_text_contains(haystack: &str, needle: &str) {
    assert!(
        haystack.contains(needle),
        "expected {haystack:?} to contain {needle:?}"
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
        static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let thread_id = format!("{:?}", std::thread::current().id())
            .chars()
            .filter(|ch| ch.is_ascii_alphanumeric())
            .collect::<String>();
        let path = std::env::temp_dir().join(format!(
            "awiki-cli-rs2-test-{}-{nanos}-{thread_id}-{counter}",
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
