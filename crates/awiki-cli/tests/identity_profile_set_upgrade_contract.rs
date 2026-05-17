use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn profile_set_validates_body_sources_before_workspace_upgrade_like_go() {
    let workspace = TempDir::new().expect("workspace");
    let workspace_home = workspace.path().join(".awiki-cli");
    std::fs::create_dir_all(&workspace_home).expect("create workspace home");
    let markdown_file = workspace.path().join("profile.md");
    std::fs::write(&markdown_file, "# Profile\n").expect("write profile");
    let legacy_config = write_legacy_config_json(
        &workspace_home,
        json!({
            "schema_version": 1,
            "services": {
                "service_base_url": "https://legacy-id-profile-set-invalid.example",
                "did_domain": "legacy-id-profile-set-invalid.example",
            },
            "runtime": {
                "mode": "http",
            },
        }),
    )
    .0;

    let profile = awiki_cmd(
        &[
            "id",
            "profile",
            "set",
            "--markdown",
            "inline",
            "--markdown-file",
            markdown_file.to_str().unwrap(),
        ],
        workspace.path(),
    );
    assert_code(&profile, 2);
    let profile = error_json(&profile);
    assert_eq!(profile["error"]["code"], "invalid_argument");
    assert!(profile["error"]["message"]
        .as_str()
        .unwrap()
        .contains("Use either --markdown or --markdown-file"));
    assert_no_workspace_upgrade(&workspace_home, &legacy_config);
}

#[test]
fn profile_set_migrates_legacy_config_json_before_active_identity_boundary_like_go() {
    let workspace = TempDir::new().expect("workspace");
    let workspace_home = workspace.path().join(".awiki-cli");
    std::fs::create_dir_all(&workspace_home).expect("create workspace home");
    let (legacy_config, legacy_text) = write_legacy_config_json(
        &workspace_home,
        json!({
            "schema_version": 1,
            "services": {
                "service_base_url": "https://legacy-id-profile-set.example",
                "did_domain": "legacy-id-profile-set.example",
            },
            "runtime": {
                "mode": "http",
            },
        }),
    );

    let profile = awiki_cmd(
        &["id", "profile", "set", "--display-name", "Legacy Profile"],
        workspace.path(),
    );
    assert_code(&profile, 5);
    let profile = error_json(&profile);
    assert_eq!(profile["error"]["code"], "not_found");
    assert!(profile["error"]["message"]
        .as_str()
        .unwrap()
        .contains("identity not found: no active identity is configured"));

    assert!(!legacy_config.exists());
    assert_migrated_config(
        &workspace_home,
        "https://legacy-id-profile-set.example",
        "legacy-id-profile-set.example",
    );
    assert_workspace_upgrade_meta(&workspace_home, &legacy_text);
    assert_no_runtime_state(&workspace_home);
}

fn awiki_cmd(args: &[&str], workspace: &Path) -> Output {
    let bin = env!("CARGO_BIN_EXE_awiki-cli");
    Command::new(bin)
        .args(args)
        .env("AWIKI_CLI_WORKSPACE_HOME_DIR", workspace.join(".awiki-cli"))
        .env("AWIKI_CLI_UPDATE_CACHE_ONLY", "1")
        .env_remove("AWIKI_WORKSPACE")
        .env_remove("AWIKI_WORKSPACE_HOME")
        .env_remove("AWIKI_HOME")
        .env_remove("AVIKI_WORKSPACE_HOME")
        .env_remove("AWIKI_FORMAT")
        .env_remove("AVIKI_FORMAT")
        .output()
        .expect("run awiki-cli")
}

fn assert_code(output: &Output, expected: i32) {
    assert_eq!(
        output.status.code(),
        Some(expected),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn error_json(output: &Output) -> Value {
    serde_json::from_slice(&output.stderr).expect("parse error JSON")
}

fn write_legacy_config_json(workspace_home: &Path, payload: Value) -> (PathBuf, String) {
    let legacy_config = workspace_home.join("config.json");
    let legacy_text = serde_json::to_string(&payload).expect("serialize legacy config");
    std::fs::write(&legacy_config, &legacy_text).expect("write legacy config");
    (legacy_config, legacy_text)
}

fn assert_no_workspace_upgrade(workspace_home: &Path, legacy_config: &Path) {
    assert!(legacy_config.exists());
    assert!(!workspace_home.join("config.yaml").exists());
    assert!(!workspace_home.join("upgrade").join("meta.json").exists());
}

fn assert_migrated_config(workspace_home: &Path, service_base_url: &str, did_domain: &str) {
    let config_text =
        std::fs::read_to_string(workspace_home.join("config.yaml")).expect("read migrated config");
    for needle in [
        "schema_version: 1\n".to_string(),
        "  mode: http\n".to_string(),
        format!("  service_base_url: {service_base_url}\n"),
        format!("  did_domain: {did_domain}\n"),
    ] {
        assert!(
            config_text.contains(&needle),
            "migrated config: {config_text:?}"
        );
    }
}

fn assert_workspace_upgrade_meta(workspace_home: &Path, legacy_text: &str) {
    let meta_path = workspace_home.join("upgrade").join("meta.json");
    let meta: Value =
        serde_json::from_slice(&std::fs::read(&meta_path).expect("read upgrade meta"))
            .expect("upgrade meta JSON");
    assert_eq!(meta["workspace_schema_version"], 3);
    let backup_dir = PathBuf::from(meta["last_backup_dir"].as_str().unwrap());
    assert_eq!(
        std::fs::read_to_string(backup_dir.join("config.json.bak"))
            .expect("read legacy config backup"),
        legacy_text
    );
    assert!(!workspace_home
        .join("upgrade")
        .join("upgrade_journal.json")
        .exists());
}

fn assert_no_runtime_state(workspace_home: &Path) {
    assert!(!workspace_home.join("data").join("awiki-cli.db").exists());
    assert!(!workspace_home
        .join("runtime")
        .join("message-daemon.sock")
        .exists());
    assert!(!workspace_home.join("runtime").join("listener.pid").exists());
}

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new() -> std::io::Result<Self> {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "awiki-cli-profile-set-upgrade-{}-{unique}",
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
