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
fn notification_driven_adapter_surface_is_present() {
    let adapter = include_str!("../src/m_core_cli_adapter/device_join.rs");

    assert!(adapter.contains(".local_device_join_requests(selector)"));
    assert!(adapter.contains(".start_device_join_verification("));
    assert!(adapter.contains(".reject_device_join(selector, &join_session_id, reason)"));
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

#[test]
fn device_join_schema_is_notification_driven_and_member_only() {
    let workspace = TempDir::new("device-join-notification-schema").expect("workspace");

    let requests = command_schema(&workspace, "requests");
    assert!(flag_names(&requests).is_empty());

    let verify = command_schema(&workspace, "verify");
    assert_eq!(
        flag_names(&verify),
        vec!["session", "operation-id", "challenge-ttl-seconds"]
    );

    let poll = command_schema(&workspace, "poll");
    assert_eq!(flag_names(&poll), vec!["session"]);

    let approve = command_schema(&workspace, "approve");
    assert_eq!(flag_names(&approve), vec!["session"]);

    let reject = command_schema(&workspace, "reject");
    assert_eq!(flag_names(&reject), vec!["session", "reason"]);
    assert_eq!(
        flag_choices(&reject, "reason"),
        vec!["user-rejected", "sas-mismatch"]
    );

    let cancel = command_schema(&workspace, "cancel");
    assert_eq!(flag_names(&cancel), vec!["session"]);
}

fn command_schema(workspace: &TempDir, command: &str) -> Value {
    let output = awiki_cmd(
        &["schema", "id", "device", "join", command],
        workspace.path(),
    );
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("schema JSON")
}

fn flag_names(schema: &Value) -> Vec<&str> {
    schema["data"]["command"]["flags"]
        .as_array()
        .map(|flags| {
            flags
                .iter()
                .filter_map(|flag| flag["name"].as_str())
                .collect()
        })
        .unwrap_or_default()
}

fn flag_choices<'a>(schema: &'a Value, name: &str) -> Vec<&'a str> {
    schema["data"]["command"]["flags"]
        .as_array()
        .expect("flags")
        .iter()
        .find(|flag| flag["name"] == name)
        .expect("named flag")["choices"]
        .as_array()
        .expect("choices")
        .iter()
        .filter_map(Value::as_str)
        .collect()
}

fn awiki_cmd(args: &[&str], workspace: &Path) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_awiki-cli"));
    command
        .args(args)
        .env("AWIKI_CLI_WORKSPACE_HOME_DIR", workspace)
        .env("HOME", workspace.join("home"))
        .env("USERPROFILE", workspace.join("home"))
        .env("AWIKI_CLI_UPDATE_CACHE_ONLY", "1")
        .env_remove("AWIKI_ACCOUNT_VERIFICATION_TOKEN")
        .env_remove("AWIKI_WORKSPACE")
        .env_remove("AWIKI_WORKSPACE_HOME")
        .env_remove("AWIKI_HOME")
        .env_remove("AVIKI_WORKSPACE_HOME");
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
