use serde_json::Value;
use std::path::Path;
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

const DEFAULT_NOTIFY_URL: &str = "http://127.0.0.1:8765/notify/host-event";
const EXPLICIT_SECRET: &str = "explicit-secret";

#[test]
fn hermes_setup_default_dry_run_reports_managed_plan_like_go_contract() {
    let workspace = TempDir::new("setup-default-dry-run").expect("temp workspace");

    let output = awiki_cmd(
        &["--dry-run", "runtime", "host-notify", "hermes", "setup"],
        workspace.path(),
    );
    assert_success(&output);
    let envelope = success_json(&output);

    assert_eq!(
        envelope["command"],
        "awiki-cli runtime host-notify hermes setup"
    );
    assert_eq!(
        envelope["summary"],
        "Dry run: Hermes host notify setup planned"
    );
    let plan = &envelope["data"]["plan"];
    assert_eq!(plan["action"], "host_notify_hermes_setup");
    assert_eq!(plan["notify_url"], DEFAULT_NOTIFY_URL);
    assert_eq!(plan["deliver"], "feishu");
    assert_eq!(plan["previous_sink"], "log");
    assert_eq!(plan["host_notify_enabled"], true);
    assert_eq!(plan["manages_local_hermes"], true);
    assert_eq!(plan["starts_local_bridge"], true);
    assert_eq!(plan["route_uses_home_channel"], true);
    assert_eq!(plan["secret_source"], "generated");
    assert_eq!(
        plan["awiki_config_file"],
        tenant_config_path(workspace.path())
            .to_string_lossy()
            .as_ref()
    );
    assert_eq!(
        plan["hermes_config_file"],
        workspace
            .path()
            .join("hermes-home")
            .join("config.yaml")
            .to_string_lossy()
            .as_ref()
    );
    assert!(tenant_config_path(workspace.path()).exists());
    assert!(!workspace.path().join("config.yaml").exists());
    assert!(!workspace.path().join("hermes-home/config.yaml").exists());
    assert!(!tenant_runtime_path(workspace.path(), "listener.pid").exists());
    assert!(!tenant_runtime_path(workspace.path(), "listener.status.json").exists());
    assert!(!tenant_runtime_path(workspace.path(), "listener.expected-boot-id").exists());
    assert!(!tenant_runtime_path(workspace.path(), "message-daemon.sock").exists());
}

#[test]
fn hermes_setup_dry_run_accepts_explicit_flags_and_redacts_secret_like_go_contract() {
    let workspace = TempDir::new("setup-explicit-dry-run").expect("temp workspace");

    let output = awiki_cmd(
        &[
            "--dry-run",
            "runtime",
            "host-notify",
            "hermes",
            "setup",
            "--notify-url",
            "http://127.0.0.1:9999/hook",
            "--deliver",
            "telegram",
            "--secret",
            EXPLICIT_SECRET,
        ],
        workspace.path(),
    );
    assert_success(&output);
    assert_not_contains(&String::from_utf8_lossy(&output.stdout), EXPLICIT_SECRET);
    assert_not_contains(&String::from_utf8_lossy(&output.stderr), EXPLICIT_SECRET);
    let envelope = success_json(&output);

    let plan = &envelope["data"]["plan"];
    assert_eq!(plan["notify_url"], "http://127.0.0.1:9999/hook");
    assert_eq!(plan["deliver"], "telegram");
    assert_eq!(plan["secret_source"], "flag");
    assert!(tenant_config_path(workspace.path()).exists());
    assert!(!workspace.path().join("config.yaml").exists());
    assert!(!workspace.path().join("hermes-home/config.yaml").exists());
}

#[test]
fn webhook_alias_setup_dry_run_uses_canonical_command_like_go_contract() {
    let workspace = TempDir::new("setup-webhook-alias-dry-run").expect("temp workspace");

    let output = awiki_cmd(
        &[
            "--dry-run",
            "runtime",
            "host-notify",
            "webhook",
            "setup",
            "--deliver",
            "log",
        ],
        workspace.path(),
    );
    assert_success(&output);
    let envelope = success_json(&output);

    assert_eq!(
        envelope["command"],
        "awiki-cli runtime host-notify hermes setup"
    );
    assert_eq!(envelope["data"]["plan"]["deliver"], "log");
}

#[test]
fn hermes_setup_dry_run_uses_raw_hermes_config_when_sink_is_not_hermes_like_go_contract() {
    let workspace = TempDir::new("setup-raw-hermes-config").expect("temp workspace");
    write_tenant_config(
        workspace.path(),
        "runtime:\n  host_notify:\n    sink: log\n    hermes:\n      notify_url: http://127.0.0.1:9999/config-hook\n      deliver: telegram\n      secret: config-secret\n",
    );

    let output = awiki_cmd(
        &["--dry-run", "runtime", "host-notify", "hermes", "setup"],
        workspace.path(),
    );
    assert_success(&output);
    assert_not_contains(&String::from_utf8_lossy(&output.stdout), "config-secret");
    assert_not_contains(&String::from_utf8_lossy(&output.stderr), "config-secret");
    let envelope = success_json(&output);

    let plan = &envelope["data"]["plan"];
    assert_eq!(plan["notify_url"], "http://127.0.0.1:9999/config-hook");
    assert_eq!(plan["deliver"], "telegram");
    assert_eq!(plan["secret_source"], "config_file");
    assert_eq!(plan["previous_sink"], "log");
    assert_config_contains(
        workspace.path(),
        "notify_url: http://127.0.0.1:9999/config-hook",
    );
    assert!(!workspace.path().join("hermes-home/config.yaml").exists());
}

#[test]
fn hermes_setup_dry_run_uses_raw_legacy_webhook_config_like_go_contract() {
    let workspace = TempDir::new("setup-raw-webhook-config").expect("temp workspace");
    write_tenant_config(
        workspace.path(),
        "runtime:\n  host_notify:\n    sink: log\n    webhook:\n      notify_url: http://127.0.0.1:9998/legacy-hook\n      secret: legacy-config-secret\n",
    );

    let output = awiki_cmd(
        &["--dry-run", "runtime", "host-notify", "hermes", "setup"],
        workspace.path(),
    );
    assert_success(&output);
    assert_not_contains(
        &String::from_utf8_lossy(&output.stdout),
        "legacy-config-secret",
    );
    assert_not_contains(
        &String::from_utf8_lossy(&output.stderr),
        "legacy-config-secret",
    );
    let envelope = success_json(&output);

    let plan = &envelope["data"]["plan"];
    assert_eq!(plan["notify_url"], "http://127.0.0.1:9998/legacy-hook");
    assert_eq!(plan["deliver"], "feishu");
    assert_eq!(plan["secret_source"], "config_file");
}

#[test]
fn hermes_setup_dry_run_reports_environment_secret_sources_like_go_contract() {
    let workspace = TempDir::new("setup-env-secret").expect("temp workspace");

    let output = awiki_cmd_with_env(
        &["--dry-run", "runtime", "host-notify", "hermes", "setup"],
        workspace.path(),
        &[("AWIKI_HOST_NOTIFY_HERMES_SECRET", "  hermes-env-secret  ")],
    );
    assert_success(&output);
    assert_not_contains(
        &String::from_utf8_lossy(&output.stdout),
        "hermes-env-secret",
    );
    let envelope = success_json(&output);
    assert_eq!(envelope["data"]["plan"]["secret_source"], "environment");

    let legacy_workspace = TempDir::new("setup-legacy-env").expect("temp workspace");
    let legacy_output = awiki_cmd_with_env(
        &["--dry-run", "runtime", "host-notify", "hermes", "setup"],
        legacy_workspace.path(),
        &[("AWIKI_HOST_NOTIFY_WEBHOOK_SECRET", "legacy-env-secret")],
    );
    assert_success(&legacy_output);
    assert_not_contains(
        &String::from_utf8_lossy(&legacy_output.stdout),
        "legacy-env-secret",
    );
    let legacy_envelope = success_json(&legacy_output);
    assert_eq!(
        legacy_envelope["data"]["plan"]["secret_source"],
        "environment"
    );
}

#[test]
fn hermes_setup_non_dry_run_writes_local_awiki_and_hermes_files_without_bridge_side_effects() {
    let workspace = TempDir::new("setup-non-dry-run-local").expect("temp workspace");

    let output = awiki_cmd(
        &["runtime", "host-notify", "hermes", "setup"],
        workspace.path(),
    );
    assert_success(&output);
    let envelope = success_json(&output);

    assert_eq!(
        envelope["command"],
        "awiki-cli runtime host-notify hermes setup"
    );
    assert_eq!(envelope["summary"], "Hermes host notify setup completed");
    assert_eq!(
        envelope["data"]["host_notify"]["sink"], "hermes",
        "setup switches awiki-cli to the Hermes sink like Go"
    );
    assert_eq!(envelope["data"]["host_notify"]["enabled"], true);
    assert_eq!(
        envelope["data"]["host_notify"]["hermes"]["notify_url"],
        DEFAULT_NOTIFY_URL
    );
    assert_eq!(
        envelope["data"]["host_notify"]["hermes"]["deliver"],
        "feishu"
    );
    assert_eq!(
        envelope["data"]["host_notify"]["hermes"]["secret_configured"],
        true
    );
    assert_eq!(envelope["data"]["local_hermes"]["route_configured"], true);
    assert_eq!(envelope["data"]["local_hermes"]["deliver"], "feishu");
    assert_eq!(
        envelope["data"]["local_hermes"]["route_secret_configured"],
        true
    );
    assert_eq!(envelope["data"]["bridge"]["installed"], false);
    assert_eq!(envelope["data"]["bridge"]["running"], false);
    assert_eq!(envelope["data"]["bridge"]["bridge_available"], false);
    assert!(
        envelope["data"]["listener"].is_object(),
        "setup should report the current listener status without restarting it"
    );
    assert!(
        envelope["data"]["next_steps"]
            .as_array()
            .is_some_and(|values| values.iter().any(|value| value
                .as_str()
                .unwrap_or_default()
                .contains("awiki-cli runtime host-notify hermes status"))),
        "Go setup returns status verification guidance"
    );
    assert!(
        envelope["warnings"]
            .as_array()
            .is_some_and(|warnings| warnings.iter().any(|value| value
                .as_str()
                .unwrap_or_default()
                .contains("Hermes bridge service install/start requires"))),
        "gate-off setup must report passive bridge status instead of implying the bridge service was started"
    );

    let config = std::fs::read_to_string(tenant_config_path(workspace.path()))
        .expect("awiki config should be written");
    assert_contains(&config, "    enabled: true\n");
    assert_contains(&config, "    sink: hermes\n");
    assert_contains(&config, "      notify_url: \n");
    assert_contains(&config, "      deliver: \n");
    assert_contains(&config, "      secret: ");

    let hermes_config = read_hermes_config(workspace.path());
    assert_contains(&hermes_config, "platforms:\n");
    assert_contains(&hermes_config, "  webhook:\n");
    assert_contains(&hermes_config, "    enabled: true\n");
    assert_contains(&hermes_config, "      port: 8644\n");
    assert_contains(&hermes_config, "        notify:\n");
    assert_contains(&hermes_config, "          secret: ");
    assert_contains(&hermes_config, "          deliver: feishu\n");
    assert!(!tenant_runtime_path(workspace.path(), "listener.pid").exists());
    assert!(!tenant_runtime_path(workspace.path(), "listener.status.json").exists());
    assert!(!tenant_runtime_path(workspace.path(), "listener.expected-boot-id").exists());
    assert!(!tenant_runtime_path(workspace.path(), "message-daemon.sock").exists());
}

#[test]
fn hermes_setup_non_dry_run_accepts_explicit_flags_and_redacts_secret() {
    let workspace = TempDir::new("setup-non-dry-run-explicit").expect("temp workspace");

    let output = awiki_cmd(
        &[
            "runtime",
            "host-notify",
            "hermes",
            "setup",
            "--notify-url",
            "http://127.0.0.1:9999/hook",
            "--deliver",
            "telegram",
            "--secret",
            EXPLICIT_SECRET,
        ],
        workspace.path(),
    );
    assert_success(&output);
    assert_not_contains(&String::from_utf8_lossy(&output.stdout), EXPLICIT_SECRET);
    assert_not_contains(&String::from_utf8_lossy(&output.stderr), EXPLICIT_SECRET);
    let envelope = success_json(&output);

    assert_eq!(
        envelope["data"]["host_notify"]["hermes"]["notify_url"],
        "http://127.0.0.1:9999/hook"
    );
    assert_eq!(
        envelope["data"]["host_notify"]["hermes"]["deliver"],
        "telegram"
    );
    assert_eq!(
        envelope["data"]["host_notify"]["hermes"]["secret_configured"],
        true
    );
    assert_eq!(envelope["data"]["local_hermes"]["deliver"], "telegram");
    assert_eq!(
        envelope["data"]["local_hermes"]["home_channel_key"],
        "TELEGRAM_HOME_CHANNEL"
    );
    assert_eq!(envelope["data"]["bridge"]["running"], false);

    let config = std::fs::read_to_string(tenant_config_path(workspace.path()))
        .expect("awiki config should be written");
    assert_contains(&config, "      notify_url: http://127.0.0.1:9999/hook\n");
    assert_contains(&config, "      deliver: telegram\n");
    assert_contains(&config, "      secret: explicit-secret\n");

    let hermes_config = read_hermes_config(workspace.path());
    assert_contains(&hermes_config, "          deliver: telegram\n");
    assert_contains(&hermes_config, "          secret: ");
}

#[test]
fn hermes_setup_dry_run_rejects_non_local_notify_url_like_go_contract() {
    let workspace = TempDir::new("setup-non-local-url").expect("temp workspace");

    let output = awiki_cmd(
        &[
            "--dry-run",
            "runtime",
            "host-notify",
            "hermes",
            "setup",
            "--notify-url",
            "http://10.0.0.1:8765/notify/host-event",
        ],
        workspace.path(),
    );
    assert_code(&output, 2);
    let envelope = error_json(&output);

    assert_eq!(envelope["error"]["code"], "invalid_argument");
    assert_eq!(
        envelope["error"]["message"],
        "notify URL host \"10.0.0.1\" is not local; full Hermes setup only supports a local bridge"
    );
    assert_eq!(
        envelope["error"]["hint"],
        "Use a local notify URL such as http://127.0.0.1:8765/notify/host-event for the fully managed Hermes flow."
    );
}

#[test]
fn hermes_setup_dry_run_rejects_blank_explicit_secret_like_go_contract() {
    let workspace = TempDir::new("setup-blank-secret").expect("temp workspace");

    let output = awiki_cmd(
        &[
            "--dry-run",
            "runtime",
            "host-notify",
            "hermes",
            "setup",
            "--secret",
            "   ",
        ],
        workspace.path(),
    );
    assert_code(&output, 2);
    let envelope = error_json(&output);

    assert_eq!(envelope["error"]["code"], "invalid_argument");
    assert_eq!(
        envelope["error"]["message"],
        "hermes setup requires a non-empty --secret when the flag is provided."
    );
    assert_eq!(envelope["error"]["hint"], "Use --secret <secret>.");
}

#[test]
fn hermes_setup_dry_run_rejects_malformed_config_for_secret_resolution_like_go_contract() {
    let workspace = TempDir::new("setup-malformed-config").expect("temp workspace");
    let init = awiki_cmd(&["config", "show"], workspace.path());
    assert_success(&init);
    write_tenant_config(
        workspace.path(),
        "runtime:\n  host_notify:\n    sink: 'unterminated\n",
    );

    let output = awiki_cmd(
        &["--dry-run", "runtime", "host-notify", "hermes", "setup"],
        workspace.path(),
    );
    assert_code(&output, 1);
    let envelope = error_json(&output);

    assert_eq!(envelope["error"]["code"], "internal_error");
    assert!(
        envelope["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("quoted scalar"),
        "unexpected message: {envelope:#?}"
    );
    assert_eq!(
        envelope["error"]["hint"],
        "Check awiki-cli host notify secret sources."
    );
}

#[test]
fn hermes_setup_dry_run_rejects_unsupported_deliver_like_go_contract() {
    let workspace = TempDir::new("setup-invalid-deliver").expect("temp workspace");

    let output = awiki_cmd(
        &[
            "--dry-run",
            "runtime",
            "host-notify",
            "hermes",
            "setup",
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

fn awiki_cmd(args: &[&str], workspace: &Path) -> Output {
    awiki_cmd_with_env(args, workspace, &[])
}

fn awiki_cmd_with_env(args: &[&str], workspace: &Path, envs: &[(&str, &str)]) -> Output {
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

fn assert_not_contains(haystack: &str, needle: &str) {
    assert!(
        !haystack.contains(needle),
        "{haystack:?} should not contain {needle:?}"
    );
}

fn assert_config_contains(workspace: &Path, needle: &str) {
    let config =
        std::fs::read_to_string(tenant_config_path(workspace)).expect("config should exist");
    assert_contains(&config, needle);
}

fn write_tenant_config(product_home: &Path, text: &str) {
    let config_path = tenant_config_path(product_home);
    std::fs::create_dir_all(config_path.parent().expect("tenant config parent"))
        .expect("create tenant config dir");
    std::fs::write(&config_path, text).expect("write tenant config");
}

fn tenant_config_path(product_home: &Path) -> std::path::PathBuf {
    product_home
        .join("tenants")
        .join("china")
        .join("config.yaml")
}

fn tenant_runtime_path(product_home: &Path, file_name: &str) -> std::path::PathBuf {
    product_home
        .join("tenants")
        .join("china")
        .join("runtime")
        .join(file_name)
}

fn read_hermes_config(workspace: &Path) -> String {
    std::fs::read_to_string(workspace.join("hermes-home").join("config.yaml"))
        .expect("Hermes config should be written")
}

fn assert_contains(haystack: &str, needle: &str) {
    assert!(
        haystack.contains(needle),
        "{haystack:?} should contain {needle:?}"
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
            "awiki-cli-rs2-hermes-setup-dry-run-{name}-{}-{nonce}",
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
