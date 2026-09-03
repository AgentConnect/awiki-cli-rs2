use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn group_gate_defaults_on_and_accepts_explicit_zero_or_one() {
    for gate in [None, Some("0"), Some("1")] {
        let workspace = TempDir::new("group-rollout-valid").unwrap();
        write_file_compat_config(workspace.path());
        let output = awiki_cmd(workspace.path(), gate, None);
        assert_eq!(output.status.code(), Some(0), "stderr={:?}", output.stderr);
    }
}

#[test]
fn invalid_group_gate_fails_closed() {
    for gate in ["", "true", "yes", "2"] {
        let workspace = TempDir::new("group-rollout-invalid").unwrap();
        write_file_compat_config(workspace.path());
        let output = awiki_cmd(workspace.path(), Some(gate), None);

        assert_eq!(output.status.code(), Some(2), "gate={gate:?}");
        let error: Value = serde_json::from_slice(&output.stderr).unwrap();
        assert_eq!(error["error"]["code"], "invalid_config");
        assert!(error["error"]["message"]
            .as_str()
            .unwrap()
            .contains("AWIKI_MULTI_DEVICE_GROUP_E2EE_ENABLED must be 0 or 1"));
    }
}

#[test]
fn did_transition_hidden_gate_accepts_zero_or_one_and_rejects_other_values() {
    for gate in [None, Some("0"), Some("1")] {
        let workspace = TempDir::new("did-transition-rollout-valid").unwrap();
        write_file_compat_config(workspace.path());
        let output = awiki_cmd(workspace.path(), None, gate);
        assert_eq!(output.status.code(), Some(0), "stderr={:?}", output.stderr);
    }

    for gate in ["", "true", "yes", "2"] {
        let workspace = TempDir::new("did-transition-rollout-invalid").unwrap();
        write_file_compat_config(workspace.path());
        let output = awiki_cmd(workspace.path(), None, Some(gate));
        assert_eq!(output.status.code(), Some(2), "gate={gate:?}");
        let error: Value = serde_json::from_slice(&output.stderr).unwrap();
        assert_eq!(error["error"]["code"], "invalid_config");
        assert!(error["error"]["message"]
            .as_str()
            .unwrap()
            .contains("AWIKI_DID_TRANSITION_VNEXT_HIDDEN_ROLLOUT_ENABLED must be 0 or 1"));
    }
}

fn awiki_cmd(
    workspace: &Path,
    group_gate: Option<&str>,
    did_transition_gate: Option<&str>,
) -> Output {
    let home = workspace.join("home");
    let product_home = workspace.join(".awiki-cli");
    let mut command = Command::new(env!("CARGO_BIN_EXE_awiki-cli"));
    command
        .args(["id", "list"])
        .env("AWIKI_CLI_WORKSPACE_HOME_DIR", &product_home)
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("AWIKI_CLI_UPDATE_CACHE_ONLY", "1")
        .env_remove("AWIKI_MULTI_DEVICE_DEVICE_REVOKE_ENABLED")
        .env_remove("AWIKI_MULTI_DEVICE_DIRECT_E2EE_ENABLED")
        .env_remove("AWIKI_MULTI_DEVICE_HANDLE_RECOVERY_ENABLED")
        .env_remove("AWIKI_MULTI_DEVICE_GROUP_E2EE_ENABLED")
        .env_remove("AWIKI_DID_TRANSITION_VNEXT_HIDDEN_ROLLOUT_ENABLED")
        .env_remove("AWIKI_WORKSPACE")
        .env_remove("AWIKI_WORKSPACE_HOME")
        .env_remove("AWIKI_HOME")
        .env_remove("AVIKI_WORKSPACE_HOME");
    if let Some(gate) = group_gate {
        command.env("AWIKI_MULTI_DEVICE_GROUP_E2EE_ENABLED", gate);
    }
    if let Some(gate) = did_transition_gate {
        command.env("AWIKI_DID_TRANSITION_VNEXT_HIDDEN_ROLLOUT_ENABLED", gate);
    }
    command.output().unwrap()
}

fn write_file_compat_config(workspace: &Path) {
    let config = workspace
        .join(".awiki-cli")
        .join("tenants")
        .join("builtin-primary")
        .join("config.yaml");
    std::fs::create_dir_all(config.parent().unwrap()).unwrap();
    std::fs::write(config, "secret_storage:\n  mode: file_compat\n").unwrap();
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
