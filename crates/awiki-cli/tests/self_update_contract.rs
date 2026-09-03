use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn upgrade_schema_exposes_current_installing_contract() {
    let output = awiki_cmd(&["schema", "upgrade"]);
    assert_success(&output);
    let envelope = success_json(&output);

    assert_eq!(envelope["data"]["command"]["name"], "upgrade");
    assert_eq!(envelope["data"]["command"]["handler"], "upgrade");
    assert_eq!(envelope["data"]["command"]["phase"], "phase2");
    assert_eq!(envelope["data"]["command"]["side_effect"], true);
    assert_eq!(
        envelope["data"]["command"]["outputs"],
        json!(["json", "pretty", "table"])
    );
}

#[test]
fn upgrade_cache_only_uses_seeded_metadata_for_dev_builds() {
    let workspace = TempDir::new().expect("temp workspace");
    seed_metadata(workspace.path(), "10.0.2", "10.0.1");

    let output = awiki_cmd_with_workspace(
        &["upgrade", "--format", "json"],
        workspace.path(),
        &[
            ("AWIKI_CLI_UPDATE_CACHE_ONLY", "1"),
            ("AWIKI_CLI_UPDATE_CACHE_TTL", "3153600000"),
        ],
    );
    assert_success(&output);
    let envelope = success_json(&output);

    assert_eq!(envelope["command"], "awiki-cli upgrade");
    assert_eq!(envelope["data"]["min_supported_version"], "10.0.1");
    assert_eq!(envelope["data"]["latest_version"], "10.0.2");
    assert_eq!(envelope["data"]["dev_build"], true);
    assert_eq!(envelope["data"]["blocked"], false);
    assert_eq!(envelope["data"]["strict_disabled"], false);
    assert_eq!(envelope["data"]["update_metadata_source"], "cache");
    assert_eq!(envelope["data"]["update_check_status"], "ok");
    assert_eq!(envelope["data"]["upgrade_attempted"], false);
}

#[test]
fn upgrade_strict_disable_follows_config_and_env_override() {
    let config_workspace = TempDir::new().expect("temp workspace");
    seed_metadata(config_workspace.path(), "10.0.2", "10.0.1");
    let config_path = config_workspace
        .path()
        .join("tenants")
        .join("builtin-primary")
        .join("config.yaml");
    fs::create_dir_all(config_path.parent().expect("tenant config parent"))
        .expect("create tenant config parent");
    fs::write(config_path, "update:\n  disable_strict_version: true\n").expect("write config");

    let output = awiki_cmd_with_workspace(
        &["upgrade", "--format", "json"],
        config_workspace.path(),
        &[
            ("AWIKI_CLI_UPDATE_CACHE_ONLY", "1"),
            ("AWIKI_CLI_UPDATE_CACHE_TTL", "3153600000"),
        ],
    );
    assert_success(&output);
    let envelope = success_json(&output);
    assert_eq!(envelope["data"]["strict_disabled"], true);
    assert_eq!(envelope["data"]["blocked"], false);

    let env_workspace = TempDir::new().expect("temp workspace");
    seed_metadata(env_workspace.path(), "10.0.2", "10.0.1");
    let output = awiki_cmd_with_workspace(
        &["upgrade", "--format", "json"],
        env_workspace.path(),
        &[
            ("AWIKI_CLI_UPDATE_CACHE_ONLY", "1"),
            ("AWIKI_CLI_UPDATE_CACHE_TTL", "3153600000"),
            ("AWIKI_CLI_DISABLE_STRICT_VERSION", "1"),
        ],
    );
    assert_success(&output);
    let envelope = success_json(&output);
    assert_eq!(envelope["data"]["strict_disabled"], true);
    assert_eq!(envelope["data"]["blocked"], false);
}

#[test]
fn root_preflight_soft_fails_update_check_for_non_exempt_commands() {
    let workspace = TempDir::new().expect("temp workspace");

    let output = awiki_cmd_with_workspace(
        &["status", "--format", "json"],
        workspace.path(),
        &[("AWIKI_CLI_UPDATE_CACHE_ONLY", "1")],
    );
    assert_success(&output);
    let envelope = success_json(&output);

    assert_eq!(envelope["command"], "awiki-cli status");
    assert_eq!(envelope["data"]["cli"]["phase"], "phase1-shell");
}

#[test]
fn root_preflight_verbose_logs_soft_update_check_failures() {
    let workspace = TempDir::new().expect("temp workspace");

    let output = awiki_cmd_with_workspace(
        &["--verbose", "status", "--format", "json"],
        workspace.path(),
        &[("AWIKI_CLI_UPDATE_CACHE_ONLY", "1")],
    );
    assert_success_code(&output);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("[awiki-cli] update check failed:"),
        "stderr should include verbose update check failure, got {stderr}"
    );
    assert!(
        stderr.contains("cache-only mode"),
        "stderr should include cache-only failure cause, got {stderr}"
    );
    let envelope: Value =
        serde_json::from_slice(&output.stdout).expect("stdout should be a JSON success envelope");
    assert_eq!(envelope["ok"], true);
    assert_eq!(envelope["command"], "awiki-cli status");
}

#[test]
fn root_preflight_exempts_only_update_help_and_tenant_switch_commands() {
    let workspace = TempDir::new().expect("temp workspace");
    let successful_exempt_commands: &[&[&str]] = &[
        &["version", "--format", "json"],
        &["upgrade", "--format", "json"],
        &["--help"],
        &["tenant", "--help"],
        &["tenant", "list", "--format", "json"],
        &["tenant", "current", "--format", "json"],
    ];

    for args in successful_exempt_commands {
        let mut verbose_args = vec!["--verbose"];
        verbose_args.extend_from_slice(args);
        let output = awiki_cmd_with_workspace(
            &verbose_args,
            workspace.path(),
            &[("AWIKI_CLI_UPDATE_CACHE_ONLY", "1")],
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            !stderr.contains("[awiki-cli] update check failed:"),
            "exempt command {args:?} should not log update check failure, stderr = {stderr}"
        );
        assert_success_with_context(
            &output,
            &format!("expected exempt command {args:?} to skip update check"),
        );
    }

    assert_success(&awiki_cmd_with_workspace(
        &["init"],
        workspace.path(),
        &[("AWIKI_CLI_UPDATE_CACHE_ONLY", "1")],
    ));
    let switched = awiki_cmd_with_workspace(
        &[
            "--verbose",
            "tenant",
            "use",
            "builtin-primary",
            "--format",
            "json",
        ],
        workspace.path(),
        &[("AWIKI_CLI_UPDATE_CACHE_ONLY", "1")],
    );
    assert_success_with_context(&switched, "tenant use must remain exempt");
    assert!(
        !String::from_utf8_lossy(&switched.stderr).contains("[awiki-cli] update check failed:"),
        "tenant use must not run the update preflight"
    );
}

fn seed_metadata(workspace: &Path, latest: &str, minimum: &str) {
    let path = workspace
        .join("tenants")
        .join("builtin-primary")
        .join("cache")
        .join("update")
        .join("metadata.json");
    fs::create_dir_all(path.parent().unwrap()).expect("create cache dir");
    let payload = json!({
        "product": "awiki-cli",
        "channel": "stable",
        "policy_origin": "https://awiki.me",
        "policy_revision": 1,
        "published_at": "2026-09-03T00:00:00Z",
        "release_notes_url": "https://awiki.me/cli/releases/10.0.1",
        "latest_version": latest,
        "min_supported_version": minimum,
        "installer_url": "https://awiki.me/cli/stable/awiki-cli.tgz",
        "installer_mirrors": [],
        "installer_sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "installer_size": 123,
        "installer_integrity": "sha512-YWJjZA==",
        "retrieved_at": "2020-01-01T00:00:00Z",
    });
    fs::write(
        path,
        format!("{}\n", serde_json::to_string_pretty(&payload).unwrap()),
    )
    .expect("write metadata");
}

fn awiki_cmd(args: &[&str]) -> Output {
    let workspace = TempDir::new().expect("temp workspace");
    awiki_cmd_with_workspace(args, workspace.path(), &[])
}

fn awiki_cmd_with_workspace(args: &[&str], workspace: &Path, extra_env: &[(&str, &str)]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_awiki-cli"));
    command
        .args(args)
        .env("AWIKI_CLI_WORKSPACE_HOME_DIR", workspace)
        .env("HOME", workspace.join("home"))
        .env("USERPROFILE", workspace.join("home"))
        .env_remove("AWIKI_CLI_DISABLE_STRICT_VERSION")
        .env_remove("AWIKI_CLI_UPDATE_CACHE_ONLY")
        .env_remove("AWIKI_CLI_UPDATE_CACHE_TTL")
        .env_remove("AWIKI_WORKSPACE")
        .env_remove("AWIKI_WORKSPACE_HOME")
        .env_remove("AWIKI_HOME")
        .env_remove("AVIKI_WORKSPACE_HOME")
        .env_remove("AWIKI_FORMAT")
        .env_remove("AVIKI_FORMAT");
    for (key, value) in extra_env {
        command.env(key, value);
    }
    command.output().expect("run awiki-cli binary")
}

fn assert_success(output: &Output) {
    assert_success_with_context(output, "unexpected exit status");
}

fn assert_success_with_context(output: &Output, context: &str) {
    assert_eq!(
        output.status.code(),
        Some(0),
        "{context}; stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn assert_success_code(output: &Output) {
    assert_success_with_context(output, "unexpected exit status");
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
            "awiki-cli-rs2-update-test-{}-{nanos}-{thread_id}-{counter}",
            std::process::id()
        ));
        fs::create_dir_all(&path)?;
        Ok(Self { path })
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
