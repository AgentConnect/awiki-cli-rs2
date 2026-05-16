use serde_json::Value;
use std::path::Path;
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

const NOTIFY_URL: &str = "http://127.0.0.1:8765/notify/host-event";
const SECRET: &str = "secret-contract-value";

#[test]
fn hermes_set_requires_at_least_one_changed_flag_like_go_contract() {
    let workspace = TempDir::new("set-missing-flags").expect("temp workspace");

    let output = awiki_cmd(
        &["runtime", "host-notify", "hermes", "set"],
        workspace.path(),
    );
    assert_code(&output, 2);
    let envelope = error_json(&output);

    assert_eq!(envelope["error"]["code"], "invalid_argument");
    assert_eq!(
        envelope["error"]["message"],
        "hermes set requires at least one changed flag."
    );
    assert_eq!(envelope["error"]["hint"], "Use --notify-url or --deliver.");
}

#[test]
fn hermes_set_rejects_unsupported_deliver_like_go_contract() {
    let workspace = TempDir::new("set-invalid-deliver").expect("temp workspace");

    let output = awiki_cmd(
        &[
            "runtime",
            "host-notify",
            "hermes",
            "set",
            "--deliver",
            "invalid",
        ],
        workspace.path(),
    );
    assert_code(&output, 2);
    let envelope = error_json(&output);

    assert_eq!(envelope["error"]["code"], "invalid_argument");
    assert_eq!(
        envelope["error"]["message"],
        "unsupported Hermes deliver target \"invalid\""
    );
}

#[test]
fn hermes_set_dry_run_reports_plan_like_go_contract() {
    let workspace = TempDir::new("set-dry-run").expect("temp workspace");

    let output = awiki_cmd(
        &[
            "--dry-run",
            "runtime",
            "host-notify",
            "hermes",
            "set",
            "--notify-url",
            NOTIFY_URL,
            "--deliver",
            "telegram",
        ],
        workspace.path(),
    );
    assert_success(&output);
    let envelope = success_json(&output);

    assert_eq!(
        envelope["command"],
        "awiki-cli runtime host-notify hermes set"
    );
    assert_eq!(
        envelope["summary"],
        "Dry run: Hermes host notify config change planned"
    );
    assert_eq!(envelope["data"]["plan"]["action"], "host_notify_hermes_set");
    assert_eq!(envelope["data"]["plan"]["notify_url"], NOTIFY_URL);
    assert_eq!(envelope["data"]["plan"]["deliver"], "telegram");
    assert_eq!(
        envelope["data"]["plan"]["config_file"],
        workspace
            .path()
            .join("config.yaml")
            .to_string_lossy()
            .as_ref()
    );
}

#[test]
fn hermes_set_dry_run_allows_unsupported_deliver_like_go_contract() {
    let workspace = TempDir::new("set-dry-run-invalid-deliver").expect("temp workspace");

    let output = awiki_cmd(
        &[
            "--dry-run",
            "runtime",
            "host-notify",
            "hermes",
            "set",
            "--deliver",
            "invalid",
        ],
        workspace.path(),
    );
    assert_success(&output);
    let envelope = success_json(&output);

    assert_eq!(
        envelope["summary"],
        "Dry run: Hermes host notify config change planned"
    );
    assert_eq!(envelope["data"]["plan"]["action"], "host_notify_hermes_set");
    assert_eq!(envelope["data"]["plan"]["deliver"], "invalid");
}

#[test]
fn hermes_set_live_writes_config_without_requiring_secret_like_go_contract() {
    let workspace = TempDir::new("set-live").expect("temp workspace");

    let output = awiki_cmd(
        &[
            "runtime",
            "host-notify",
            "hermes",
            "set",
            "--notify-url",
            NOTIFY_URL,
            "--deliver",
            "telegram",
        ],
        workspace.path(),
    );
    assert_success(&output);
    let envelope = success_json(&output);

    assert_eq!(
        envelope["command"],
        "awiki-cli runtime host-notify hermes set"
    );
    assert_eq!(envelope["summary"], "Hermes host notify config updated");
    assert!(
        envelope["data"]["hermes"]
            .as_object()
            .is_some_and(|value| value.is_empty()),
        "Go returns an empty Hermes runtime object until the host-notify sink is hermes"
    );
    assert!(
        envelope["data"]["listener"].is_object(),
        "Hermes config changes should report listener apply/status context"
    );

    let show = awiki_cmd(
        &["runtime", "host-notify", "config", "show"],
        workspace.path(),
    );
    assert_success(&show);
    let envelope = success_json(&show);
    assert_eq!(envelope["data"]["host_notify"]["sink"], "log");
    assert_eq!(envelope["data"]["host_notify"]["hermes"]["notify_url"], "");
    assert_eq!(
        envelope["data"]["host_notify"]["hermes"]["deliver"],
        "telegram"
    );
    assert_eq!(
        envelope["data"]["host_notify"]["hermes"]["secret_configured"],
        false
    );

    let config_file = std::fs::read_to_string(workspace.path().join("config.yaml"))
        .expect("config file should be written");
    assert!(config_file.contains("notify_url: http://127.0.0.1:8765/notify/host-event"));
    assert!(config_file.contains("deliver: telegram"));
}

#[test]
fn hermes_set_live_reports_runtime_config_when_sink_is_hermes_like_go_contract() {
    let workspace = TempDir::new("set-live-hermes-sink").expect("temp workspace");
    let sink = awiki_cmd(
        &[
            "runtime",
            "host-notify",
            "config",
            "set",
            "--sink",
            "hermes",
        ],
        workspace.path(),
    );
    assert_success(&sink);

    let output = awiki_cmd(
        &[
            "runtime",
            "host-notify",
            "hermes",
            "set",
            "--notify-url",
            NOTIFY_URL,
            "--deliver",
            "telegram",
        ],
        workspace.path(),
    );
    assert_success(&output);
    let envelope = success_json(&output);

    assert_eq!(
        envelope["command"],
        "awiki-cli runtime host-notify hermes set"
    );
    assert_eq!(envelope["summary"], "Hermes host notify config updated");
    assert_eq!(envelope["data"]["hermes"]["notify_url"], NOTIFY_URL);
    assert_eq!(envelope["data"]["hermes"]["deliver"], "telegram");
    assert!(
        envelope["data"]["listener"].is_object(),
        "Hermes config changes should report listener apply/status context"
    );

    let show = awiki_cmd(
        &["runtime", "host-notify", "config", "show"],
        workspace.path(),
    );
    assert_success(&show);
    let envelope = success_json(&show);
    assert_eq!(envelope["data"]["host_notify"]["sink"], "hermes");
    assert_eq!(
        envelope["data"]["host_notify"]["hermes"]["notify_url"],
        NOTIFY_URL
    );
    assert_eq!(
        envelope["data"]["host_notify"]["hermes"]["deliver"],
        "telegram"
    );
    assert_eq!(
        envelope["data"]["host_notify"]["hermes"]["secret_configured"],
        false
    );
}

#[test]
fn hermes_set_secret_dry_run_reports_redacted_plan_like_go_contract() {
    let workspace = TempDir::new("set-secret-dry-run").expect("temp workspace");

    let output = awiki_cmd(
        &[
            "--dry-run",
            "runtime",
            "host-notify",
            "hermes",
            "set-secret",
            "--value",
            SECRET,
        ],
        workspace.path(),
    );
    assert_success(&output);
    assert_not_contains(&String::from_utf8_lossy(&output.stdout), SECRET);
    let envelope = success_json(&output);

    assert_eq!(
        envelope["command"],
        "awiki-cli runtime host-notify hermes set-secret"
    );
    assert_eq!(envelope["summary"], "Dry run: Hermes secret update planned");
    assert_eq!(
        envelope["data"]["plan"]["action"],
        "host_notify_hermes_set_secret"
    );
    assert_eq!(envelope["data"]["plan"]["configured"], true);
    assert_eq!(
        envelope["data"]["plan"]["config_file"],
        workspace
            .path()
            .join("config.yaml")
            .to_string_lossy()
            .as_ref()
    );
}

#[test]
fn hermes_set_secret_live_writes_redacted_status_like_go_contract() {
    let workspace = TempDir::new("set-secret-live").expect("temp workspace");

    let output = awiki_cmd(
        &[
            "runtime",
            "host-notify",
            "hermes",
            "set-secret",
            "--value",
            SECRET,
        ],
        workspace.path(),
    );
    assert_success(&output);
    assert_not_contains(&String::from_utf8_lossy(&output.stdout), SECRET);
    let envelope = success_json(&output);

    assert_eq!(
        envelope["command"],
        "awiki-cli runtime host-notify hermes set-secret"
    );
    assert_eq!(envelope["summary"], "Hermes secret updated");
    assert_eq!(envelope["data"]["hermes"]["secret_configured"], true);
    assert!(
        envelope["data"]["listener"].is_object(),
        "Hermes secret changes should report listener apply/status context"
    );

    let show = awiki_cmd(
        &["runtime", "host-notify", "config", "show"],
        workspace.path(),
    );
    assert_success(&show);
    assert_not_contains(&String::from_utf8_lossy(&show.stdout), SECRET);
    let envelope = success_json(&show);
    assert_eq!(
        envelope["data"]["host_notify"]["hermes"]["secret_configured"],
        true
    );
}

#[test]
fn hermes_set_secret_requires_non_blank_value_like_go_contract() {
    let workspace = TempDir::new("set-secret-missing").expect("temp workspace");

    for args in [
        vec!["runtime", "host-notify", "hermes", "set-secret"],
        vec![
            "runtime",
            "host-notify",
            "hermes",
            "set-secret",
            "--value",
            "   ",
        ],
    ] {
        let output = awiki_cmd(&args, workspace.path());
        assert_code(&output, 2);
        let envelope = error_json(&output);

        assert_eq!(envelope["error"]["code"], "invalid_argument");
        assert_eq!(
            envelope["error"]["message"],
            "hermes set-secret requires --value."
        );
        assert_eq!(envelope["error"]["hint"], "Use --value <secret>.");
    }
}

#[test]
fn hermes_clear_secret_dry_run_reports_plan_like_go_contract() {
    let workspace = TempDir::new("clear-secret-dry-run").expect("temp workspace");

    let output = awiki_cmd(
        &[
            "--dry-run",
            "runtime",
            "host-notify",
            "hermes",
            "clear-secret",
        ],
        workspace.path(),
    );
    assert_success(&output);
    let envelope = success_json(&output);

    assert_eq!(
        envelope["command"],
        "awiki-cli runtime host-notify hermes clear-secret"
    );
    assert_eq!(envelope["summary"], "Dry run: Hermes secret clear planned");
    assert_eq!(
        envelope["data"]["plan"]["action"],
        "host_notify_hermes_clear_secret"
    );
    assert_eq!(
        envelope["data"]["plan"]["config_file"],
        workspace
            .path()
            .join("config.yaml")
            .to_string_lossy()
            .as_ref()
    );
}

#[test]
fn hermes_clear_secret_live_clears_secret_like_go_contract() {
    let workspace = TempDir::new("clear-secret-live").expect("temp workspace");

    let set_secret = awiki_cmd(
        &[
            "runtime",
            "host-notify",
            "hermes",
            "set-secret",
            "--value",
            SECRET,
        ],
        workspace.path(),
    );
    assert_success(&set_secret);
    assert_not_contains(&String::from_utf8_lossy(&set_secret.stdout), SECRET);

    let output = awiki_cmd(
        &["runtime", "host-notify", "hermes", "clear-secret"],
        workspace.path(),
    );
    assert_success(&output);
    assert_not_contains(&String::from_utf8_lossy(&output.stdout), SECRET);
    let envelope = success_json(&output);

    assert_eq!(
        envelope["command"],
        "awiki-cli runtime host-notify hermes clear-secret"
    );
    assert_eq!(envelope["summary"], "Hermes secret cleared");
    assert_eq!(envelope["data"]["hermes"]["secret_configured"], false);
    assert!(
        envelope["data"]["listener"].is_object(),
        "Hermes secret changes should report listener apply/status context"
    );

    let show = awiki_cmd(
        &["runtime", "host-notify", "config", "show"],
        workspace.path(),
    );
    assert_success(&show);
    assert_not_contains(&String::from_utf8_lossy(&show.stdout), SECRET);
    let envelope = success_json(&show);
    assert_eq!(
        envelope["data"]["host_notify"]["hermes"]["secret_configured"],
        false
    );
}

fn awiki_cmd(args: &[&str], workspace: &Path) -> Output {
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

fn assert_not_contains(haystack: &str, needle: &str) {
    assert!(
        !haystack.contains(needle),
        "{haystack:?} should not contain {needle:?}"
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
            "awiki-cli-rs2-hermes-config-write-{name}-{}-{nonce}",
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
