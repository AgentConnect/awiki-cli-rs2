use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn recovery_gate_defaults_off_and_invalid_values_fail_closed() {
    let workspace = TempDir::new("recovery-gate-off").unwrap();
    let output = awiki_cmd(&["id", "recovery", "sessions"], workspace.path(), None);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        error_json(&output)["error"]["code"],
        "unsupported_capability"
    );
    assert!(!workspace.path().join("tenants").exists());

    for invalid in ["", "true", "yes", "2"] {
        let workspace = TempDir::new("recovery-gate-invalid").unwrap();
        let output = awiki_cmd(
            &["id", "recovery", "sessions"],
            workspace.path(),
            Some(invalid),
        );
        assert_eq!(output.status.code(), Some(2), "gate={invalid:?}");
        assert_eq!(error_json(&output)["error"]["code"], "invalid_config");
        assert!(!workspace.path().join("tenants").exists());
    }
}

#[test]
fn destructive_recovery_requires_foreground_tty_before_workspace_open() {
    for command in ["cancel", "finalize"] {
        let workspace = TempDir::new("recovery-tty").unwrap();
        let output = awiki_cmd(
            &["id", "recovery", command, "--session", "recovery-session-1"],
            workspace.path(),
            Some("1"),
        );
        assert_eq!(output.status.code(), Some(3), "command={command}");
        assert_eq!(
            error_json(&output)["error"]["code"],
            "user_presence_required"
        );
        assert!(!workspace.path().join("tenants").exists());
    }
}

#[test]
fn recovery_schema_never_accepts_credentials_or_internal_control_fields() {
    let workspace = TempDir::new("recovery-schema").unwrap();
    let expected = [
        ("begin", vec!["handle"]),
        ("status", vec!["session"]),
        ("cancel", vec!["session"]),
        ("finalize", vec!["session"]),
        ("activate", vec!["session"]),
        ("sessions", vec![]),
    ];
    for (subcommand, expected_flags) in expected {
        let output = awiki_cmd(
            &["schema", "id", "recovery", subcommand],
            workspace.path(),
            None,
        );
        assert_eq!(output.status.code(), Some(0), "subcommand={subcommand}");
        let schema: Value = serde_json::from_slice(&output.stdout).unwrap();
        let flags = schema["data"]["command"]["flags"]
            .as_array()
            .map(|flags| {
                flags
                    .iter()
                    .filter_map(|flag| flag["name"].as_str())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        assert_eq!(flags, expected_flags, "subcommand={subcommand}");
        let output = String::from_utf8_lossy(&output.stdout);
        for forbidden in [
            "verification-token",
            "account-token",
            "reconfirmation-token",
            "root-private",
            "root_proof",
            "admin_proof",
            "document_version",
            "document_hash",
            "registry_version",
            "auth_generation",
            "user-presence-confirmed",
        ] {
            assert!(
                !output.to_ascii_lowercase().contains(forbidden),
                "{subcommand} schema leaked {forbidden}"
            );
        }
    }
}

#[test]
fn recovery_adapter_reads_grants_only_from_environment() {
    let source = include_str!("../src/m_core_cli_adapter/handle_recovery.rs");
    assert!(source.contains("AWIKI_HANDLE_RECOVERY_BEGIN_VERIFICATION_TOKEN"));
    assert!(source.contains("AWIKI_HANDLE_RECOVERY_FINALIZE_VERIFICATION_TOKEN"));
    assert!(source.contains("HandleRecoveryBeginGrant::from_bytes(token.into_bytes())"));
    assert!(source.contains("HandleRecoveryReconfirmationGrant::from_bytes(token.into_bytes())"));
    assert!(!source.contains("std::env::args"));
}

fn awiki_cmd(args: &[&str], workspace: &Path, gate: Option<&str>) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_awiki-cli"));
    command
        .args(args)
        .env("AWIKI_CLI_WORKSPACE_HOME_DIR", workspace)
        .env("HOME", workspace.join("home"))
        .env("USERPROFILE", workspace.join("home"))
        .env("AWIKI_CLI_UPDATE_CACHE_ONLY", "1")
        .env_remove("AWIKI_MULTI_DEVICE_HANDLE_RECOVERY_ENABLED")
        .env_remove("AWIKI_HANDLE_RECOVERY_BEGIN_VERIFICATION_TOKEN")
        .env_remove("AWIKI_HANDLE_RECOVERY_FINALIZE_VERIFICATION_TOKEN")
        .env_remove("AWIKI_WORKSPACE")
        .env_remove("AWIKI_WORKSPACE_HOME")
        .env_remove("AWIKI_HOME")
        .env_remove("AVIKI_WORKSPACE_HOME");
    if let Some(gate) = gate {
        command.env("AWIKI_MULTI_DEVICE_HANDLE_RECOVERY_ENABLED", gate);
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
