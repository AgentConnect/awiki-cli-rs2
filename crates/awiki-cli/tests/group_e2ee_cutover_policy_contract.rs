use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

const GROUP_DID: &str = "did:wba:awiki.ai:groups:demo:e1_group";
const MEMBER_DID: &str = "did:wba:awiki.ai:bob:e1_bob";

#[test]
fn high_level_group_e2ee_options_are_stable_unsupported() {
    let workspace = TempDir::new("group-e2ee-high-level").expect("workspace");
    let cases = [
        (
            &["group", "create", "--name", "Encrypted Group", "--e2ee"][..],
            "group.create",
        ),
        (
            &[
                "group",
                "create",
                "--name",
                "Encrypted Group",
                "--message-security-profile",
                "group-e2ee",
            ][..],
            "group.create",
        ),
        (
            &[
                "group", "add", "--group", GROUP_DID, "--member", MEMBER_DID, "--e2ee",
            ][..],
            "group.add",
        ),
        (
            &[
                "group", "remove", "--group", GROUP_DID, "--member", MEMBER_DID, "--e2ee",
            ][..],
            "group.remove",
        ),
        (
            &["group", "leave", "--group", GROUP_DID, "--e2ee"][..],
            "group.leave",
        ),
    ];

    for (args, command) in cases {
        let output = awiki_cmd(args, workspace.path());
        assert_unsupported_capability(&output, command, "group e2ee", "Phase 6");
    }
}

#[test]
fn group_e2ee_direct_commands_require_internal_gate() {
    let workspace = TempDir::new("group-e2ee-internal-gate").expect("workspace");
    for (args, command) in group_e2ee_commands(false) {
        let output = awiki_cmd(&args, workspace.path());
        assert_internal_command(&output, command);
    }
}

#[test]
fn group_e2ee_internal_dry_run_plans_remain_queryable() {
    let workspace = TempDir::new("group-e2ee-dry-run").expect("workspace");
    let cases = [
        ("group.e2ee.status", "group.e2ee.status"),
        (
            "group.e2ee.publish-key-package",
            "group.e2ee.publish_key_package",
        ),
        ("group.e2ee.pending", "group.e2ee.pending"),
        ("group.e2ee.repair", "group.e2ee.repair"),
        ("group.e2ee.update-key", "group.e2ee.update_key"),
        ("group.e2ee.rejoin", "group.e2ee.rejoin"),
        ("group.e2ee.recover-member", "group.e2ee.recover_member"),
        (
            "group.e2ee.process-leave-request",
            "group.e2ee.process_leave_request",
        ),
    ];

    for (command, action) in cases {
        let args = group_e2ee_args(command, true);
        let output = awiki_internal_cmd(&args, workspace.path());
        assert_success(&output);
        let envelope = success_json(&output);
        assert_eq!(envelope["data"]["plan"]["action"], action);
    }
}

#[test]
fn group_e2ee_internal_live_commands_stay_unsupported() {
    let workspace = TempDir::new("group-e2ee-live-unsupported").expect("workspace");
    for (args, command) in group_e2ee_commands(false) {
        let output = awiki_internal_cmd(&args, workspace.path());
        assert_unsupported_capability(&output, command, "group e2ee", "Phase 6");
    }
}

#[test]
fn msg_group_secure_send_is_stable_unsupported_without_legacy_e2ee_send() {
    let workspace = TempDir::new("group-e2ee-send-unsupported").expect("workspace");
    let output = awiki_cmd(
        &[
            "msg",
            "send",
            "--group",
            GROUP_DID,
            "--text",
            "hello group e2ee",
            "--secure",
            "group-e2ee",
        ],
        workspace.path(),
    );

    assert_unsupported_capability(&output, "msg.send", "secure-direct", "Phase 6");
}

fn group_e2ee_commands(dry_run: bool) -> Vec<(Vec<&'static str>, &'static str)> {
    [
        "group.e2ee.status",
        "group.e2ee.publish-key-package",
        "group.e2ee.pending",
        "group.e2ee.repair",
        "group.e2ee.update-key",
        "group.e2ee.rejoin",
        "group.e2ee.recover-member",
        "group.e2ee.process-leave-request",
    ]
    .into_iter()
    .map(|command| (group_e2ee_args(command, dry_run), command))
    .collect()
}

fn group_e2ee_args(command: &str, dry_run: bool) -> Vec<&'static str> {
    let mut args = match command {
        "group.e2ee.status" => vec!["group", "e2ee", "status", "--group", GROUP_DID],
        "group.e2ee.publish-key-package" => vec![
            "group",
            "e2ee",
            "publish-key-package",
            "--group",
            GROUP_DID,
            "--purpose",
            "update",
            "--contract-test",
        ],
        "group.e2ee.pending" => vec!["group", "e2ee", "pending", "--group", GROUP_DID],
        "group.e2ee.repair" => vec!["group", "e2ee", "repair", "--group", GROUP_DID],
        "group.e2ee.update-key" => vec![
            "group",
            "e2ee",
            "update-key",
            "--group",
            GROUP_DID,
            "--member",
            MEMBER_DID,
        ],
        "group.e2ee.rejoin" => vec![
            "group", "e2ee", "rejoin", "--group", GROUP_DID, "--member", MEMBER_DID,
        ],
        "group.e2ee.recover-member" => vec![
            "group",
            "e2ee",
            "recover-member",
            "--group",
            GROUP_DID,
            "--member",
            MEMBER_DID,
        ],
        "group.e2ee.process-leave-request" => vec![
            "group",
            "e2ee",
            "process-leave-request",
            "--group",
            GROUP_DID,
            "--member",
            MEMBER_DID,
        ],
        _ => panic!("unknown group e2ee command: {command}"),
    };
    if dry_run {
        args.push("--dry-run");
    }
    args
}

fn awiki_cmd(args: &[&str], workspace: &Path) -> Output {
    let mut command = base_awiki_command(args, workspace);
    command.output().expect("run awiki-cli binary")
}

fn awiki_internal_cmd(args: &[&str], workspace: &Path) -> Output {
    let mut command = base_awiki_command(args, workspace);
    command.env("AWIKI_CLI_INTERNAL_ENTRY", "1");
    command.output().expect("run awiki-cli binary")
}

fn base_awiki_command(args: &[&str], workspace: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_awiki-cli"));
    command
        .args(args)
        .env("AWIKI_CLI_WORKSPACE_HOME_DIR", workspace)
        .env("AWIKI_CLI_UPDATE_CACHE_ONLY", "1")
        .env_remove("AWIKI_WORKSPACE")
        .env_remove("AWIKI_WORKSPACE_HOME")
        .env_remove("AWIKI_HOME")
        .env_remove("AVIKI_WORKSPACE_HOME")
        .env_remove("AWIKI_FORMAT")
        .env_remove("AVIKI_FORMAT")
        .env_remove("AWIKI_ANP_MLS_BINARY");
    command
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

fn assert_internal_command(output: &Output, command: &str) {
    assert_eq!(
        output.status.code(),
        Some(2),
        "unexpected exit status; stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let envelope = error_json(output);
    assert_eq!(envelope["error"]["code"], "internal_command");
    assert_eq!(envelope["error"]["details"]["command"], command);
    assert_eq!(
        envelope["error"]["details"]["required_gate"],
        "AWIKI_CLI_INTERNAL_ENTRY=1"
    );
}

fn assert_unsupported_capability(
    output: &Output,
    command: &str,
    capability: &str,
    required_phase: &str,
) {
    assert_eq!(
        output.status.code(),
        Some(2),
        "unexpected exit status; stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let envelope = error_json(output);
    assert_eq!(envelope["error"]["code"], "unsupported_capability");
    assert_eq!(envelope["error"]["details"]["command"], command);
    assert_eq!(envelope["error"]["details"]["capability"], capability);
    assert_eq!(
        envelope["error"]["details"]["required_phase"],
        required_phase
    );
    assert_eq!(
        envelope["error"]["details"]["cutover_status"],
        "unsupported"
    );
}

fn success_json(output: &Output) -> Value {
    assert!(
        output.stderr.is_empty(),
        "stderr should be empty: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let envelope: Value =
        serde_json::from_slice(&output.stdout).expect("stdout should be a JSON success envelope");
    assert_eq!(envelope["ok"], true);
    envelope
}

fn error_json(output: &Output) -> Value {
    assert!(
        !output.status.success(),
        "command should fail\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stderr).expect("stderr should be a JSON error envelope")
}

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(prefix: &str) -> std::io::Result<Self> {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "awiki-cli-rs2-{prefix}-{}-{nanos}",
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
