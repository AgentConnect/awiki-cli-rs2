use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn root_key_confirmation_injection_is_not_public_api() {
    let source = include_str!("../src/m_core_cli_adapter/root_key_transfer.rs");
    assert!(source.contains("pub(crate) async fn send_via_im_core_async<F>"));
    assert!(source.contains("pub(crate) async fn retry_via_im_core_async<F>"));
    assert!(!source.contains("pub async fn send_via_im_core_async<F>"));
    assert!(!source.contains("pub async fn retry_via_im_core_async<F>"));
}

#[test]
fn root_key_send_fails_closed_before_workspace_open() {
    let workspace = TempDir::new("root-key-gate").unwrap();
    let output = awiki_cmd(
        &[
            "id",
            "device",
            "root-key",
            "send",
            "--device",
            "dev-recipient",
            "--message-id",
            "root-message-1",
        ],
        workspace.path(),
        None,
    );
    assert_eq!(output.status.code(), Some(2));
    let envelope = error_json(&output);
    assert_eq!(envelope["error"]["code"], "unsupported_capability");
    assert!(!workspace.path().join("tenants").exists());
}

#[test]
fn root_key_send_rejects_dry_run_before_workspace_open() {
    let workspace = TempDir::new("root-key-dry-run").unwrap();
    let output = awiki_cmd(
        &[
            "--dry-run",
            "id",
            "device",
            "root-key",
            "send",
            "--device",
            "dev-recipient",
            "--message-id",
            "root-message-2",
        ],
        workspace.path(),
        Some("1"),
    );
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(error_json(&output)["error"]["code"], "invalid_argument");
    assert!(!workspace.path().join("tenants").exists());
}

#[test]
fn root_key_list_fails_closed_before_workspace_open() {
    let workspace = TempDir::new("root-key-list-gate").unwrap();
    let output = awiki_cmd(
        &["id", "device", "root-key", "list"],
        workspace.path(),
        None,
    );
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        error_json(&output)["error"]["code"],
        "unsupported_capability"
    );
    assert!(!workspace.path().join("tenants").exists());
}

#[test]
fn root_key_retry_requires_foreground_tty_before_workspace_open() {
    let workspace = TempDir::new("root-key-retry-tty").unwrap();
    let output = awiki_cmd(
        &[
            "id",
            "device",
            "root-key",
            "retry",
            "--message-id",
            "root-message-1",
        ],
        workspace.path(),
        Some("1"),
    );
    assert_eq!(output.status.code(), Some(3));
    assert_eq!(
        error_json(&output)["error"]["code"],
        "user_presence_required"
    );
    assert!(!workspace.path().join("tenants").exists());
}

#[test]
fn root_key_retry_rejects_dry_run_before_workspace_open() {
    let workspace = TempDir::new("root-key-retry-dry-run").unwrap();
    let output = awiki_cmd(
        &[
            "--dry-run",
            "id",
            "device",
            "root-key",
            "retry",
            "--message-id",
            "root-message-1",
        ],
        workspace.path(),
        Some("1"),
    );
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(error_json(&output)["error"]["code"], "invalid_argument");
    assert!(!workspace.path().join("tenants").exists());
}

#[test]
fn root_key_schema_exposes_only_device_and_message_id() {
    let workspace = TempDir::new("root-key-schema").unwrap();
    let output = awiki_cmd(
        &["schema", "id", "device", "root-key", "send"],
        workspace.path(),
        None,
    );
    assert_eq!(output.status.code(), Some(0));
    let envelope: Value = serde_json::from_slice(&output.stdout).unwrap();
    let names = envelope["data"]["command"]["flags"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|flag| flag["name"].as_str())
        .collect::<Vec<_>>();
    assert_eq!(names, vec!["device", "message-id"]);
    let output = String::from_utf8_lossy(&output.stdout);
    for forbidden in [
        "root_private_key",
        "document_hash",
        "transport_context",
        "completion",
        "transfer-id",
        "user-presence-confirmed",
        "--ttl",
    ] {
        assert!(!output.contains(forbidden), "schema leaked {forbidden}");
    }
}

#[test]
fn root_key_list_and_retry_schemas_expose_no_secret_or_route_override() {
    let workspace = TempDir::new("root-key-status-schema").unwrap();
    let list = awiki_cmd(
        &["schema", "id", "device", "root-key", "list"],
        workspace.path(),
        None,
    );
    assert_eq!(list.status.code(), Some(0));
    assert_eq!(
        schema_flag_names(&list),
        vec!["include-completed".to_owned()]
    );

    let retry = awiki_cmd(
        &["schema", "id", "device", "root-key", "retry"],
        workspace.path(),
        None,
    );
    assert_eq!(retry.status.code(), Some(0));
    assert_eq!(schema_flag_names(&retry), vec!["message-id".to_owned()]);
    let retry_schema: Value = serde_json::from_slice(&retry.stdout).unwrap();
    let flags = retry_schema["data"]["command"]["flags"].as_array().unwrap();
    for forbidden in [
        "device",
        "recipient",
        "root-private-key",
        "transport-context",
        "sidecar",
        "inner-json",
        "user-presence-confirmed",
        "transfer-id",
    ] {
        assert!(
            flags.iter().all(|flag| flag["name"] != forbidden),
            "retry schema exposed forbidden flag {forbidden}"
        );
    }
}

fn schema_flag_names(output: &Output) -> Vec<String> {
    let envelope: Value = serde_json::from_slice(&output.stdout).unwrap();
    envelope["data"]["command"]["flags"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|flag| flag["name"].as_str().map(ToOwned::to_owned))
        .collect()
}

fn awiki_cmd(args: &[&str], workspace: &Path, gate: Option<&str>) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_awiki-cli"));
    command
        .args(args)
        .env("AWIKI_CLI_WORKSPACE_HOME_DIR", workspace)
        .env("HOME", workspace.join("home"))
        .env("USERPROFILE", workspace.join("home"))
        .env("AWIKI_CLI_UPDATE_CACHE_ONLY", "1")
        .env_remove("AWIKI_MULTI_DEVICE_ROOT_TRANSFER_ENABLED")
        .env_remove("AWIKI_WORKSPACE")
        .env_remove("AWIKI_WORKSPACE_HOME")
        .env_remove("AWIKI_HOME")
        .env_remove("AVIKI_WORKSPACE_HOME");
    if let Some(gate) = gate {
        command.env("AWIKI_MULTI_DEVICE_ROOT_TRANSFER_ENABLED", gate);
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
