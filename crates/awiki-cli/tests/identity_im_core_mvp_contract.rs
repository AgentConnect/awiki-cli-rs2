use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn identity_im_core_mvp_register_and_refresh_dry_run_keep_legacy_contract() {
    let workspace = TempDir::new().expect("workspace");

    let register = success_json(&awiki_cmd_with_env(
        &[
            "--dry-run",
            "--identity",
            "alice-local",
            "id",
            "register",
            "--handle",
            "Alice",
            "--phone",
            "+15551234567",
            "--otp",
            "123456",
            "--invite-code",
            "invite-1",
        ],
        workspace.path(),
        &[("AWIKI_USE_IM_CORE_MVP", "1")],
    ));
    assert_eq!(
        register["summary"],
        "Dry run: handle registration flow planned"
    );
    assert_eq!(register["data"]["plan"]["action"], "register_handle");
    assert_eq!(register["data"]["plan"]["identity_name"], "alice-local");
    assert_eq!(register["data"]["plan"]["full_handle"], "alice.awiki.ai");
    assert_eq!(register["data"]["plan"]["phone"], "+15551234567");
    assert!(register["data"]["plan"]["remote_calls"]
        .as_array()
        .unwrap()
        .contains(&json!("did-auth.register")));

    let refresh = success_json(&awiki_cmd_with_env(
        &[
            "--identity",
            "alice-local",
            "id",
            "refresh-token",
            "--dry-run",
        ],
        workspace.path(),
        &[("AWIKI_USE_IM_CORE_MVP", "1")],
    ));
    assert_eq!(refresh["data"]["plan"]["action"], "refresh_token");
    assert_eq!(refresh["data"]["plan"]["identity_name"], "alice-local");
    assert_eq!(
        refresh["data"]["plan"]["auth_flow"],
        "did_auth_get_me_without_stored_bearer"
    );
}

#[test]
fn identity_im_core_mvp_refresh_selects_identity_before_legacy_auth() {
    let workspace = TempDir::new().expect("workspace");
    let workspace_home = workspace.path().join(".awiki-cli");
    let manager = identity_manager(&workspace_home);
    manager
        .save(awiki_cli::identity::types::SaveInput {
            identity_name: "alice".to_string(),
            did: "did:wba:awiki.ai:user:e1_alice".to_string(),
            unique_id: "e1_alice".to_string(),
            display_name: "Alice".to_string(),
            ..Default::default()
        })
        .expect("save alice");
    manager
        .save(awiki_cli::identity::types::SaveInput {
            identity_name: "bob".to_string(),
            did: "did:wba:awiki.ai:user:e1_bob".to_string(),
            unique_id: "e1_bob".to_string(),
            display_name: "Bob".to_string(),
            ..Default::default()
        })
        .expect("save bob");

    let result = awiki_cmd_with_env(
        &["--identity", "bob", "id", "refresh-token"],
        workspace.path(),
        &[("AWIKI_USE_IM_CORE_MVP", "1")],
    );
    assert_code(&result, 3);
    let result = error_json(&result);
    assert_eq!(result["error"]["code"], "auth_required");
    assert!(result["error"]["message"].as_str().unwrap().contains("bob"));
}

fn awiki_cmd_with_env(args: &[&str], workspace: &Path, envs: &[(&str, &str)]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_awiki-cli"));
    command
        .args(args)
        .env("AWIKI_CLI_WORKSPACE_HOME_DIR", workspace.join(".awiki-cli"))
        .env("HOME", workspace)
        .env("AWIKI_CLI_UPDATE_CACHE_ONLY", "1")
        .env_remove("AWIKI_WORKSPACE")
        .env_remove("AWIKI_WORKSPACE_HOME")
        .env_remove("AWIKI_HOME")
        .env_remove("AVIKI_WORKSPACE_HOME")
        .env_remove("AWIKI_FORMAT")
        .env_remove("AVIKI_FORMAT");
    for (key, value) in envs {
        command.env(key, value);
    }
    command.output().expect("run awiki-cli")
}

fn identity_manager(workspace: &Path) -> awiki_cli::identity::Manager {
    awiki_cli::identity::Manager::new(awiki_cli::config::Paths {
        workspace_home_dir: path_string(workspace),
        root_dir: path_string(workspace),
        config_dir: path_string(&workspace.join("config")),
        data_dir: path_string(&workspace.join("data")),
        state_dir: path_string(&workspace.join("state")),
        cache_dir: path_string(&workspace.join("cache")),
        logs_dir: path_string(&workspace.join("logs")),
        config_file: path_string(&workspace.join("config").join("config.yaml")),
        identity_dir: path_string(&workspace.join("identities")),
        database_file: path_string(&workspace.join("data").join("awiki.db")),
        legacy_credentials_dir: path_string(&workspace.join("legacy")),
        legacy_data_dir: path_string(&workspace.join("legacy-data")),
    })
}

fn success_json(output: &Output) -> Value {
    assert_code(output, 0);
    assert!(
        output.stderr.is_empty(),
        "stderr should be empty: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("success JSON")
}

fn error_json(output: &Output) -> Value {
    assert!(
        !output.status.success(),
        "command should fail\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stderr).expect("error JSON")
}

fn assert_code(output: &Output, expected: i32) {
    assert_eq!(
        output.status.code(),
        Some(expected),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
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
            "awiki-cli-rs2-id-im-core-test-{}-{nanos}",
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
