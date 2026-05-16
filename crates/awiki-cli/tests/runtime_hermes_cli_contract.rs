use serde_json::Value;
use std::path::Path;
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn hermes_guide_default_matches_go_contract() {
    let workspace = TempDir::new("guide-default").expect("temp workspace");

    let output = awiki_cmd(
        &["runtime", "host-notify", "hermes", "guide"],
        workspace.path(),
        &[],
    );
    assert_success(&output);
    let envelope = success_json(&output);

    assert_eq!(
        envelope["command"],
        "awiki-cli runtime host-notify hermes guide"
    );
    assert_eq!(envelope["summary"], "Hermes host notify guide generated");

    let guide = &envelope["data"]["hermes_guide"];
    assert!(guide.is_object(), "hermes_guide should be an object");
    assert_eq!(
        guide["delivery_model"],
        "awiki-cli only forwards host notify events to the Hermes adapter. Final delivery targets are configured in Hermes."
    );
    assert_eq!(guide["awiki_cli"]["current"]["sink"], "log");
    assert_eq!(
        guide["awiki_cli"]["current"]["notify_url"],
        "http://127.0.0.1:8765/notify/host-event"
    );
    assert_eq!(guide["awiki_cli"]["current"]["deliver"], "feishu");
    assert_eq!(guide["awiki_cli"]["current"]["secret_configured"], false);
    assert_eq!(guide["awiki_cli"]["current"]["secret_source"], "unset");
    assert_eq!(
        guide["awiki_cli"]["recommended_setup_command"],
        "awiki-cli runtime host-notify hermes setup"
    );
    assert_json_array_contains(
        &guide["awiki_cli"]["verify_commands"],
        "awiki-cli runtime host-notify config show",
    );
    assert_json_array_contains(
        &guide["awiki_cli"]["verify_commands"],
        "awiki-cli runtime host-notify hermes status",
    );
    assert_eq!(guide["hermes"]["notify_route_name"], "notify");
    assert_eq!(guide["hermes"]["webhook_port"], 8644);
    assert_eq!(guide["hermes"]["deliver_target"], "feishu");
    assert_eq!(
        guide["hermes"]["adapter_notify_url"],
        "http://127.0.0.1:8765/notify/host-event"
    );
    assert_contains(&guide["hermes"]["recommended_route"], "deliver: \"feishu\"");

    assert_warning_contains(
        &envelope,
        "Current host notify sink is \"log\". Run `awiki-cli runtime host-notify hermes setup` to switch awiki-cli over to the fully managed local Hermes flow.",
    );
    assert_warning_contains(
        &envelope,
        "awiki-cli does not have a Hermes notify secret yet. `awiki-cli runtime host-notify hermes setup` will generate and persist one automatically.",
    );
}

#[test]
fn hermes_guide_accepts_log_deliver_with_probe_warning() {
    let workspace = TempDir::new("guide-log").expect("temp workspace");

    let output = awiki_cmd(
        &[
            "runtime",
            "host-notify",
            "hermes",
            "guide",
            "--deliver",
            "log",
        ],
        workspace.path(),
        &[],
    );
    assert_success(&output);
    let envelope = success_json(&output);

    assert_eq!(
        envelope["command"],
        "awiki-cli runtime host-notify hermes guide"
    );
    assert_eq!(envelope["summary"], "Hermes host notify guide generated");
    assert_eq!(
        envelope["data"]["hermes_guide"]["hermes"]["deliver_target"],
        "log"
    );
    assert_contains(
        &envelope["data"]["hermes_guide"]["hermes"]["recommended_route"],
        "deliver: \"log\"",
    );
    assert_warning_contains(
        &envelope,
        "This guide is using `deliver: \"log\"` for probe-only verification. Switch to a real messaging platform such as `feishu` or `telegram` for end-user delivery.",
    );
}

#[test]
fn hermes_guide_rejects_invalid_deliver_like_go_contract() {
    let workspace = TempDir::new("guide-invalid").expect("temp workspace");

    let output = awiki_cmd(
        &[
            "runtime",
            "host-notify",
            "hermes",
            "guide",
            "--deliver",
            "invalid",
        ],
        workspace.path(),
        &[],
    );
    assert_code(&output, 2);
    let envelope = error_json(&output);

    assert_eq!(envelope["error"]["code"], "invalid_argument");
    assert_eq!(
        envelope["error"]["message"],
        "unsupported Hermes deliver target \"invalid\""
    );
    assert_contains(&envelope["error"]["hint"], "Use --deliver with one of:");
    assert_contains(&envelope["error"]["hint"], "feishu");
    assert_contains(&envelope["error"]["hint"], "telegram");
}

#[test]
fn hermes_status_default_matches_go_readiness_contract() {
    let workspace = TempDir::new("status-default").expect("temp workspace");

    let output = awiki_cmd(
        &["runtime", "host-notify", "hermes", "status"],
        workspace.path(),
        &[],
    );
    assert_success(&output);
    let envelope = success_json(&output);

    assert_eq!(
        envelope["command"],
        "awiki-cli runtime host-notify hermes status"
    );
    assert_eq!(envelope["summary"], "Hermes host notify readiness loaded");
    assert!(envelope["data"]["host_notify"].is_object());
    assert!(envelope["data"]["readiness"].is_object());
    assert_eq!(envelope["data"]["ready"], false);
    assert_eq!(envelope["data"]["readiness"]["awiki_sink_is_hermes"], false);
    assert_eq!(
        envelope["data"]["readiness"]["awiki_secret_configured"],
        false
    );
    assert_eq!(envelope["data"]["readiness"]["bridge_running"], false);
    assert_eq!(envelope["data"]["readiness"]["bridge_available"], false);

    assert_eq!(envelope["data"]["local_hermes"]["route_configured"], false);
    assert_warning_contains(
        &envelope,
        "Hermes host notify secret is not configured in awiki-cli",
    );
}

#[test]
fn hermes_status_reports_configured_sink_and_env_secret_when_available() {
    let workspace = TempDir::new("status-configured").expect("temp workspace");
    let config_path = workspace.path().join("config.yaml");
    std::fs::write(
        &config_path,
        "runtime:\n  host_notify:\n    enabled: true\n    sink: hermes\n    hermes:\n      notify_url: http://127.0.0.1:8765/notify/host-event\n      deliver: log\n",
    )
    .expect("write config");

    let output = awiki_cmd(
        &["runtime", "host-notify", "hermes", "status"],
        workspace.path(),
        &[("AWIKI_HOST_NOTIFY_HERMES_SECRET", "env-secret")],
    );
    assert_success(&output);
    let envelope = success_json(&output);

    assert_eq!(envelope["summary"], "Hermes host notify readiness loaded");
    assert_eq!(envelope["data"]["host_notify"]["enabled"], true);
    assert_eq!(envelope["data"]["host_notify"]["sink"], "hermes");
    assert_eq!(
        envelope["data"]["host_notify"]["hermes"]["notify_url"],
        "http://127.0.0.1:8765/notify/host-event"
    );
    assert_eq!(envelope["data"]["host_notify"]["hermes"]["deliver"], "log");
    assert_eq!(
        envelope["data"]["host_notify"]["hermes"]["secret_configured"],
        true
    );
    assert_eq!(envelope["data"]["readiness"]["awiki_sink_is_hermes"], true);
    assert_eq!(
        envelope["data"]["readiness"]["awiki_host_notify_enabled"],
        true
    );
    assert_eq!(
        envelope["data"]["readiness"]["awiki_secret_configured"],
        true
    );
    assert_eq!(envelope["data"]["ready"], false);
}

fn awiki_cmd(args: &[&str], workspace: &Path, envs: &[(&str, &str)]) -> Output {
    let home = workspace.join("home");
    let hermes_home = workspace.join("hermes-home");
    std::fs::create_dir_all(&home).expect("home dir");
    std::fs::create_dir_all(&hermes_home).expect("Hermes home dir");

    let mut command = Command::new(env!("CARGO_BIN_EXE_awiki-cli"));
    command
        .args(args)
        .env("AWIKI_CLI_WORKSPACE_HOME_DIR", workspace)
        .env("AWIKI_CLI_UPDATE_CACHE_ONLY", "1")
        .env("HOME", &home)
        .env("HERMES_HOME", &hermes_home)
        .env_remove("OPENCLAW_CONFIG_PATH")
        .env_remove("OPENCLAW_GATEWAY_PORT")
        .env_remove("OPENCLAW_HOOK_TOKEN")
        .env_remove("AWIKI_HOST_NOTIFY_HERMES_SECRET")
        .env_remove("AWIKI_HOST_NOTIFY_WEBHOOK_SECRET")
        .env_remove("AWIKI_WORKSPACE")
        .env_remove("AWIKI_WORKSPACE_HOME")
        .env_remove("AWIKI_HOME")
        .env_remove("AVIKI_WORKSPACE_HOME")
        .env_remove("AWIKI_FORMAT")
        .env_remove("AVIKI_FORMAT");
    for (key, value) in envs {
        command.env(key, value);
    }
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
    assert!(
        output.stderr.is_empty(),
        "stderr should be empty:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let envelope: Value =
        serde_json::from_slice(&output.stdout).expect("stdout should be a JSON success envelope");
    assert_eq!(envelope["ok"], true);
    envelope
}

fn error_json(output: &Output) -> Value {
    assert!(
        output.stdout.is_empty(),
        "stdout should be empty:\n{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let envelope: Value =
        serde_json::from_slice(&output.stderr).expect("stderr should be a JSON error envelope");
    assert_eq!(envelope["ok"], false);
    envelope
}

fn assert_contains(value: &Value, needle: &str) {
    let haystack = value.as_str().expect("value should be a string");
    assert!(
        haystack.contains(needle),
        "{haystack:?} should contain {needle:?}"
    );
}

fn assert_json_array_contains(value: &Value, needle: &str) {
    let values = value.as_array().expect("value should be an array");
    assert!(
        values
            .iter()
            .any(|value| value.as_str().is_some_and(|item| item == needle)),
        "{values:?} should contain {needle:?}"
    );
}

fn assert_warning_contains(envelope: &Value, needle: &str) {
    let warnings = envelope["warnings"].as_array().expect("warnings array");
    assert!(
        warnings
            .iter()
            .any(|warning| warning.as_str().unwrap_or_default().contains(needle)),
        "warnings should contain {needle:?}; got {warnings:?}"
    );
}

struct TempDir {
    path: std::path::PathBuf,
}

impl TempDir {
    fn new(name: &str) -> std::io::Result<Self> {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "awiki-cli-rs2-hermes-cli-{name}-{}-{nonce}",
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
