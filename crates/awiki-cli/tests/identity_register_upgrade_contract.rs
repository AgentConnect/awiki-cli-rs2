use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

mod support;

use support::{tenant_config_path, tenant_workspace};

#[test]
fn register_archives_legacy_config_json_before_contact_validation() {
    let workspace = TempDir::new().expect("workspace");
    let workspace_home = workspace.path().join(".awiki-cli");
    std::fs::create_dir_all(&workspace_home).expect("create workspace home");
    let (legacy_config, legacy_text) = write_legacy_config_json(
        &workspace_home,
        json!({
            "schema_version": 1,
            "services": {
                "service_base_url": "https://legacy-id-register.example",
                "did_domain": "legacy-id-register.example",
            },
            "runtime": {
                "mode": "http",
            },
        }),
    );

    let register = awiki_cmd(&["id", "register", "--handle", "legacy"], workspace.path());
    assert_code(&register, 2);
    let register = error_json(&register);
    assert_eq!(register["error"]["code"], "invalid_argument");
    assert!(register["error"]["message"]
        .as_str()
        .unwrap()
        .contains("exactly one of phone or email is required"));

    assert!(!legacy_config.exists());
    assert_default_tenant_config(&workspace_home);
    assert_legacy_archived(&workspace_home, "config.json", &legacy_text);
    assert_no_runtime_state(&workspace_home);
}

fn awiki_cmd(args: &[&str], workspace: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_awiki-cli"))
        .args(args)
        .env("AWIKI_CLI_WORKSPACE_HOME_DIR", workspace.join(".awiki-cli"))
        .env("HOME", workspace)
        .env("USERPROFILE", workspace)
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

fn assert_default_tenant_config(workspace_home: &Path) {
    let config_text =
        std::fs::read_to_string(tenant_config_path(workspace_home)).expect("read tenant config");
    assert!(
        config_text.contains("schema_version: 1\n"),
        "default tenant config: {config_text:?}"
    );
    assert!(!config_text.contains("service_base_url:"));
    assert!(!config_text.contains("did_domain:"));
}

fn assert_legacy_archived(workspace_home: &Path, relative: &str, expected_text: &str) {
    let archive_root = workspace_home.join("legacy-archive");
    let entries = std::fs::read_dir(&archive_root)
        .unwrap_or_else(|err| panic!("read legacy archive {}: {err}", archive_root.display()))
        .map(|entry| entry.expect("legacy archive entry").path())
        .collect::<Vec<_>>();
    assert_eq!(entries.len(), 1);
    assert_eq!(
        std::fs::read_to_string(entries[0].join(relative)).expect("read archived legacy file"),
        expected_text
    )
}

fn assert_no_runtime_state(workspace_home: &Path) {
    let tenant = tenant_workspace(workspace_home);
    assert!(!tenant.join("data").join("awiki-cli.db").exists());
    assert!(!tenant.join("runtime").join("message-daemon.sock").exists());
    assert!(!tenant.join("runtime").join("listener.pid").exists());
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
            "awiki-cli-register-upgrade-{}-{unique}",
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
