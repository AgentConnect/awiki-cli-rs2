use serde_json::Value;
use std::path::Path;
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn host_notify_enable_disable_dry_run_matches_go_contract() {
    let workspace = TempDir::new().expect("temp workspace");

    let enable = awiki_cmd(
        &["--dry-run", "runtime", "host-notify", "enable"],
        workspace.path(),
    );
    assert_success(&enable);
    let envelope = success_json(&enable);
    assert_eq!(envelope["command"], "awiki-cli runtime host-notify enable");
    assert_eq!(
        envelope["summary"],
        "Dry run: host notify enablement change planned"
    );
    assert_eq!(
        envelope["data"]["plan"]["action"],
        "host_notify_enable_toggle"
    );
    assert_eq!(envelope["data"]["plan"]["enabled"], true);
    assert_eq!(
        envelope["data"]["plan"]["config_file"],
        workspace
            .path()
            .join("config.yaml")
            .to_string_lossy()
            .as_ref()
    );

    let disable = awiki_cmd(
        &["--dry-run", "runtime", "host-notify", "disable"],
        workspace.path(),
    );
    assert_success(&disable);
    let envelope = success_json(&disable);
    assert_eq!(envelope["command"], "awiki-cli runtime host-notify disable");
    assert_eq!(
        envelope["summary"],
        "Dry run: host notify enablement change planned"
    );
    assert_eq!(
        envelope["data"]["plan"]["action"],
        "host_notify_enable_toggle"
    );
    assert_eq!(envelope["data"]["plan"]["enabled"], false);
    assert_eq!(
        envelope["data"]["plan"]["config_file"],
        workspace
            .path()
            .join("config.yaml")
            .to_string_lossy()
            .as_ref()
    );
}

#[test]
fn host_notify_disable_and_enable_toggle_config_and_preserve_sink() {
    let workspace = TempDir::new().expect("temp workspace");

    let set_sink = awiki_cmd(
        &["runtime", "host-notify", "config", "set", "--sink", "file"],
        workspace.path(),
    );
    assert_success(&set_sink);
    let envelope = success_json(&set_sink);
    assert_eq!(envelope["data"]["host_notify"]["enabled"], true);
    assert_eq!(envelope["data"]["host_notify"]["sink"], "file");

    let disable = awiki_cmd(&["runtime", "host-notify", "disable"], workspace.path());
    assert_success(&disable);
    let envelope = success_json(&disable);
    assert_eq!(envelope["command"], "awiki-cli runtime host-notify disable");
    assert_eq!(envelope["summary"], "Host notify disabled");
    assert_eq!(envelope["data"]["host_notify"]["enabled"], false);
    assert_eq!(envelope["data"]["host_notify"]["sink"], "file");
    assert!(
        envelope["data"]["listener"].is_object(),
        "host-notify changes should report listener apply/status context"
    );

    let show = awiki_cmd(
        &["runtime", "host-notify", "config", "show"],
        workspace.path(),
    );
    assert_success(&show);
    let envelope = success_json(&show);
    assert_eq!(envelope["data"]["host_notify"]["enabled"], false);
    assert_eq!(envelope["data"]["host_notify"]["sink"], "file");

    let enable = awiki_cmd(&["runtime", "host-notify", "enable"], workspace.path());
    assert_success(&enable);
    let envelope = success_json(&enable);
    assert_eq!(envelope["command"], "awiki-cli runtime host-notify enable");
    assert_eq!(envelope["summary"], "Host notify enabled");
    assert_eq!(envelope["data"]["host_notify"]["enabled"], true);
    assert_eq!(envelope["data"]["host_notify"]["sink"], "file");
    assert!(
        envelope["data"]["listener"].is_object(),
        "host-notify changes should report listener apply/status context"
    );

    let show = awiki_cmd(
        &["runtime", "host-notify", "config", "show"],
        workspace.path(),
    );
    assert_success(&show);
    let envelope = success_json(&show);
    assert_eq!(envelope["data"]["host_notify"]["enabled"], true);
    assert_eq!(envelope["data"]["host_notify"]["sink"], "file");
}

fn awiki_cmd(args: &[&str], workspace: &Path) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_awiki-cli"));
    command
        .args(args)
        .env("AWIKI_CLI_WORKSPACE_HOME_DIR", workspace)
        .env("AWIKI_CLI_UPDATE_CACHE_ONLY", "1")
        .env_remove("OPENCLAW_CONFIG_PATH")
        .env_remove("OPENCLAW_GATEWAY_PORT")
        .env_remove("OPENCLAW_HOOK_TOKEN")
        .env_remove("AWIKI_HOST_NOTIFY_HERMES_SECRET")
        .env_remove("AWIKI_HOST_NOTIFY_WEBHOOK_SECRET")
        .env_remove("AWIKI_WORKSPACE")
        .env_remove("AWIKI_WORKSPACE_HOME")
        .env_remove("AWIKI_HOME")
        .env_remove("AVIKI_WORKSPACE_HOME")
        .env_remove("AWIKI_FORMAT")
        .env_remove("AVIKI_FORMAT");
    command.output().expect("run awiki-cli binary")
}

fn assert_success(output: &Output) {
    assert_eq!(
        output.status.code(),
        Some(0),
        "unexpected exit status; stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn success_json(output: &Output) -> Value {
    assert!(
        output.stderr.is_empty(),
        "stderr should be empty:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let envelope: Value =
        serde_json::from_slice(&output.stdout).expect("stdout should be a JSON success envelope");
    assert_eq!(envelope["ok"], true);
    envelope
}

struct TempDir {
    path: std::path::PathBuf,
}

impl TempDir {
    fn new() -> std::io::Result<Self> {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "awiki-cli-rs2-host-notify-enable-disable-test-{}-{nonce}",
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
