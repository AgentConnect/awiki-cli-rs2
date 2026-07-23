use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn root_key_adapter_uses_one_identity_scoped_prepare_confirm_flow() {
    let source = include_str!("../src/m_core_cli_adapter/root_key_transfer.rs");
    let vault = include_str!("../src/m_core_cli_adapter/vault.rs");
    assert!(source.contains("pub(crate) async fn send_via_im_core_async<F>"));
    assert!(!source.contains("pub async fn send_via_im_core_async<F>"));
    let prepare = source.find(".prepare(").expect("Core prepare call");
    let confirmation = source.find("confirm(").expect("host confirmation");
    let send = source
        .find(".confirm_and_send(")
        .expect("Core confirm-and-send call");
    assert!(prepare < confirmation && confirmation < send);
    for removed in [
        "list_via_im_core_async",
        "retry_via_im_core_async",
        "RootKeyTransferListRequest",
        "RootKeyTransferRetryRequest",
        "MessageId::parse",
        "require_rollout_enabled",
        "imported the root key",
    ] {
        assert!(!source.contains(removed), "adapter retained {removed}");
    }
    for removed in [
        "AWIKI_MULTI_DEVICE_ROOT_TRANSFER_ENABLED",
        "multi_device_root_transfer_enabled",
        "with_multi_device_root_transfer_enabled",
    ] {
        assert!(!vault.contains(removed), "vault retained {removed}");
    }
}

#[test]
fn root_key_confirmation_displays_safe_summary_and_reads_transfer_once() {
    let source = include_str!("../src/cli_shell/root_key_transfer_handlers.rs");
    for expected in [
        "DID: {did}",
        "Target device: {recipient_device_id}",
        "Signing key ID: {signing_key_id}",
        "E2EE key ID: {e2ee_key_id}",
        "Preparation expires at: {expires_at}",
        "Type TRANSFER to send the root key:",
    ] {
        assert!(
            source.contains(expected),
            "missing safe summary: {expected}"
        );
    }
    assert_eq!(
        source.matches("read_line()?").count(),
        1,
        "CLI must request exactly one confirmation input"
    );
    for removed in [
        "Re-enter the target device ID",
        "Type RETRY",
        "run_id_device_root_key_list_async",
        "run_id_device_root_key_retry_async",
        "root-key control",
        "root-control",
    ] {
        assert!(!source.contains(removed), "handler retained {removed}");
    }
}

#[test]
fn root_key_send_rejects_dry_run_without_message_id_or_workspace_writes() {
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
        ],
        workspace.path(),
    );
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(error_json(&output)["error"]["code"], "invalid_argument");
    assert!(!workspace.path().join("tenants").exists());
}

#[test]
fn root_key_schema_exposes_only_recipient_device() {
    let workspace = TempDir::new("root-key-schema").unwrap();
    let output = awiki_cmd(
        &["schema", "id", "device", "root-key", "send"],
        workspace.path(),
    );
    assert_eq!(output.status.code(), Some(0));
    let envelope: Value = serde_json::from_slice(&output.stdout).unwrap();
    let names = envelope["data"]["command"]["flags"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|flag| flag["name"].as_str())
        .collect::<Vec<_>>();
    assert_eq!(names, vec!["device"]);
    let schema = String::from_utf8_lossy(&output.stdout);
    for forbidden in [
        "message-id",
        "authorization-handle",
        "root-private-key",
        "ciphertext",
        "prekey",
        "proof",
        "nonce",
        "checkpoint",
        "sidecar",
        "completion",
        "user-presence-confirmed",
        "transfer-id",
        "--ttl",
    ] {
        assert!(
            !schema.to_ascii_lowercase().contains(forbidden),
            "schema leaked {forbidden}"
        );
    }
}

#[test]
fn root_key_list_and_retry_are_not_commands() {
    let workspace = TempDir::new("root-key-removed-commands").unwrap();
    for removed in ["list", "retry"] {
        let output = awiki_cmd(
            &["schema", "id", "device", "root-key", removed],
            workspace.path(),
        );
        assert!(!output.status.success(), "{removed} remained in schema");
        assert!(output.stdout.is_empty());
    }
}

#[test]
fn root_key_catalog_and_result_copy_are_safe_and_final() {
    let catalog = include_str!("../src/command_catalog/mod.rs");
    let root_key_catalog = catalog
        .lines()
        .filter(|line| line.contains("id.device.root-key"))
        .collect::<Vec<_>>()
        .join("\n");
    let adapter = include_str!("../src/m_core_cli_adapter/root_key_transfer.rs");
    assert!(root_key_catalog.contains("Core generates the message ID"));
    assert!(adapter.contains("\"根密钥已发送\""));
    assert!(adapter.contains("Vec::new()"));
    for forbidden in [
        "id.device.root-key.list",
        "id.device.root-key.retry",
        "--message-id",
        "root-control",
        "root control",
        "wait for",
        "awaiting import",
        "import completed",
    ] {
        assert!(
            !root_key_catalog.to_ascii_lowercase().contains(forbidden)
                && !adapter.to_ascii_lowercase().contains(forbidden),
            "product surface retained {forbidden}"
        );
    }
}

fn awiki_cmd(args: &[&str], workspace: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_awiki-cli"))
        .args(args)
        .env("AWIKI_CLI_WORKSPACE_HOME_DIR", workspace)
        .env("HOME", workspace.join("home"))
        .env("USERPROFILE", workspace.join("home"))
        .env("AWIKI_CLI_UPDATE_CACHE_ONLY", "1")
        .env_remove("AWIKI_WORKSPACE")
        .env_remove("AWIKI_WORKSPACE_HOME")
        .env_remove("AWIKI_HOME")
        .env_remove("AVIKI_WORKSPACE_HOME")
        .output()
        .unwrap()
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
