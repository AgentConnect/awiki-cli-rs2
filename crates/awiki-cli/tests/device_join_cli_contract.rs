use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

const SECRET_SENTINEL: &str = "otp-grant-must-never-appear";

#[test]
fn approval_confirmation_injection_is_not_public_api() {
    let source = include_str!("../src/m_core_cli_adapter/device_join.rs");

    assert!(source.contains("pub(crate) async fn approve_via_im_core_async<F>"));
    assert!(!source.contains("pub async fn approve_via_im_core_async<F>"));
}

#[test]
fn device_join_commands_fail_closed_before_workspace_open() {
    let workspace = TempDir::new("device-join-gate").expect("workspace");
    let output = awiki_cmd(
        &[
            "id",
            "device",
            "join",
            "start",
            "--did",
            "did:wba:example.test:alice",
            "--operation-id",
            "join-test-1",
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
fn account_verification_grant_is_not_a_cli_flag_or_error_value() {
    let workspace = TempDir::new("device-join-no-token-argv").expect("workspace");
    let output = awiki_cmd(
        &[
            "id",
            "device",
            "join",
            "start",
            "--did",
            "did:wba:example.test:alice",
            "--operation-id",
            "join-test-2",
            "--account-verification-token",
            SECRET_SENTINEL,
        ],
        workspace.path(),
        Some("1"),
    );

    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("unknown flag: --account-verification-token"));
    assert!(!stderr.contains(SECRET_SENTINEL));
    assert!(!String::from_utf8_lossy(&output.stdout).contains(SECRET_SENTINEL));
}

#[test]
fn state_advancing_poll_rejects_dry_run_before_workspace_open() {
    let workspace = TempDir::new("device-join-poll-dry-run").expect("workspace");
    let output = awiki_cmd(
        &[
            "--dry-run",
            "id",
            "device",
            "join",
            "poll",
            "--session",
            "join-test-3",
        ],
        workspace.path(),
        Some("1"),
    );

    assert_eq!(output.status.code(), Some(2));
    let envelope = error_json(&output);
    assert_eq!(envelope["error"]["code"], "invalid_argument");
    assert!(!workspace.path().join("tenants").exists());
}

#[test]
fn device_join_schema_exposes_only_safe_inputs() {
    let workspace = TempDir::new("device-join-schema").expect("workspace");
    let output = awiki_cmd(
        &["schema", "id", "device", "join", "start"],
        workspace.path(),
        None,
    );

    assert_eq!(output.status.code(), Some(0));
    let envelope: Value = serde_json::from_slice(&output.stdout).expect("schema JSON");
    let flags = envelope["data"]["command"]["flags"]
        .as_array()
        .expect("flags");
    let names: Vec<_> = flags
        .iter()
        .filter_map(|flag| flag["name"].as_str())
        .collect();
    assert_eq!(names, vec!["did", "operation-id", "ttl-seconds"]);
    let output_text = String::from_utf8_lossy(&output.stdout);
    assert!(!output_text.contains("join_session_token"));
    assert!(!output_text.contains("pairing_secret"));
    assert!(!output_text.contains("private_key"));
}

fn awiki_cmd(args: &[&str], workspace: &Path, gate: Option<&str>) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_awiki-cli"));
    command
        .args(args)
        .env("AWIKI_CLI_WORKSPACE_HOME_DIR", workspace)
        .env("HOME", workspace.join("home"))
        .env("USERPROFILE", workspace.join("home"))
        .env("AWIKI_CLI_UPDATE_CACHE_ONLY", "1")
        .env_remove("AWIKI_MULTI_DEVICE_JOIN_ENABLED")
        .env_remove("AWIKI_ACCOUNT_VERIFICATION_TOKEN")
        .env_remove("AWIKI_WORKSPACE")
        .env_remove("AWIKI_WORKSPACE_HOME")
        .env_remove("AWIKI_HOME")
        .env_remove("AVIKI_WORKSPACE_HOME");
    if let Some(gate) = gate {
        command.env("AWIKI_MULTI_DEVICE_JOIN_ENABLED", gate);
    }
    command.output().expect("run awiki-cli binary")
}

fn error_json(output: &Output) -> Value {
    assert!(output.stdout.is_empty());
    serde_json::from_slice(&output.stderr).expect("error JSON")
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
