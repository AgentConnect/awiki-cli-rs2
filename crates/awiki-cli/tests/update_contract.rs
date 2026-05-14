use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn upgrade_schema_exposes_go_contract() {
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
    seed_metadata(workspace.path(), "0.0.1-beta.5", "10.0.1");

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
    assert_eq!(envelope["data"]["latest_version"], "0.0.1-beta.5");
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
    seed_metadata(config_workspace.path(), "0.0.1-beta.5", "10.0.1");
    fs::write(
        config_workspace.path().join("config.yaml"),
        "update:\n  disable_strict_version: true\n",
    )
    .expect("write config");

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
    seed_metadata(env_workspace.path(), "0.0.1-beta.5", "10.0.1");
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

fn seed_metadata(workspace: &Path, latest: &str, minimum: &str) {
    let path = workspace.join("cache").join("update").join("metadata.json");
    fs::create_dir_all(path.parent().unwrap()).expect("create cache dir");
    let payload = json!({
        "latest_version": latest,
        "min_supported_version": minimum,
        "retrieved_at": "2020-01-01T00:00:00Z",
        "source": "network",
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
    assert_eq!(
        output.status.code(),
        Some(0),
        "unexpected exit status; stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
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

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new() -> std::io::Result<Self> {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "awiki-cli-rs2-update-test-{}-{nanos}",
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
