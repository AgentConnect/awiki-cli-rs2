use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn revoke_confirmation_injection_is_not_public_api() {
    let source = include_str!("../src/m_core_cli_adapter/device_revoke.rs");
    assert!(source.contains("pub(crate) async fn revoke_via_im_core_async<F>"));
    assert!(!source.contains("pub async fn revoke_via_im_core_async<F>"));
}

#[test]
fn explicit_zero_disables_device_revoke_before_workspace_open() {
    let workspace = TempDir::new("device-revoke-gate").unwrap();
    let output = awiki_cmd(
        &["id", "device", "revoke", "--device", "dev-target"],
        workspace.path(),
        Some("0"),
    );
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        error_json(&output)["error"]["code"],
        "unsupported_capability"
    );
    assert!(!workspace.path().join("tenants").exists());
}

#[test]
fn device_revoke_requires_foreground_tty_before_workspace_open() {
    let workspace = TempDir::new("device-revoke-tty").unwrap();
    let output = awiki_cmd(
        &["id", "device", "revoke", "--device", "dev-target"],
        workspace.path(),
        None,
    );
    assert_eq!(output.status.code(), Some(3));
    assert_eq!(
        error_json(&output)["error"]["code"],
        "user_presence_required"
    );
    assert!(!workspace.path().join("tenants").exists());
}

#[test]
fn device_revoke_rejects_dry_run_before_workspace_open() {
    let workspace = TempDir::new("device-revoke-dry-run").unwrap();
    let output = awiki_cmd(
        &[
            "--dry-run",
            "id",
            "device",
            "revoke",
            "--device",
            "dev-target",
        ],
        workspace.path(),
        Some("1"),
    );
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(error_json(&output)["error"]["code"], "invalid_argument");
    assert!(!workspace.path().join("tenants").exists());
}

#[test]
fn device_revoke_schema_exposes_only_target_device() {
    let workspace = TempDir::new("device-revoke-schema").unwrap();
    let output = awiki_cmd(
        &["schema", "id", "device", "revoke"],
        workspace.path(),
        None,
    );
    assert_eq!(output.status.code(), Some(0));
    let schema: Value = serde_json::from_slice(&output.stdout).unwrap();
    let names = schema["data"]["command"]["flags"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|flag| flag["name"].as_str())
        .collect::<Vec<_>>();
    assert_eq!(names, vec!["device"]);
    let output = String::from_utf8_lossy(&output.stdout);
    for forbidden in [
        "auth_generation",
        "document_version",
        "document_hash",
        "registry_version",
        "root_proof",
        "admin_proof",
        "user-presence-confirmed",
    ] {
        assert!(!output.contains(forbidden), "schema leaked {forbidden}");
    }
}

fn awiki_cmd(args: &[&str], workspace: &Path, gate: Option<&str>) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_awiki-cli"));
    command
        .args(args)
        .env("AWIKI_CLI_WORKSPACE_HOME_DIR", workspace)
        .env("HOME", workspace.join("home"))
        .env("USERPROFILE", workspace.join("home"))
        .env("AWIKI_CLI_UPDATE_CACHE_ONLY", "1")
        .env_remove("AWIKI_MULTI_DEVICE_DEVICE_REVOKE_ENABLED")
        .env_remove("AWIKI_WORKSPACE")
        .env_remove("AWIKI_WORKSPACE_HOME")
        .env_remove("AWIKI_HOME")
        .env_remove("AVIKI_WORKSPACE_HOME");
    if let Some(gate) = gate {
        command.env("AWIKI_MULTI_DEVICE_DEVICE_REVOKE_ENABLED", gate);
    }
    command.output().unwrap()
}

fn error_json(output: &Output) -> Value {
    assert!(output.stdout.is_empty());
    serde_json::from_slice(&output.stderr).unwrap()
}

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(name: &str) -> std::io::Result<Self> {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("{name}-{}-{nanos}", std::process::id()));
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
