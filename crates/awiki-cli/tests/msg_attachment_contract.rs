use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn msg_attachment_send_dry_run_keeps_cli_owned_plan_contract() {
    let workspace = TempDir::new().expect("workspace");
    let attachment = workspace.path().join("payload.txt");
    std::fs::write(&attachment, "attachment body").expect("write attachment");

    let direct = success_json(&awiki_cmd(
        &[
            "--dry-run",
            "--identity",
            "alice",
            "msg",
            "send",
            "--to",
            "bob",
            "--text",
            "caption",
            "--file",
            attachment.to_str().expect("attachment path"),
            "--mime-type",
            "text/plain",
        ],
        workspace.path(),
    ));

    assert_eq!(direct["summary"], "Dry run: message send planned");
    assert_eq!(direct["data"]["plan"]["action"], "attachment.send");
    assert_eq!(direct["data"]["plan"]["identity"], "alice");
    assert_eq!(
        direct["data"]["plan"]["target"],
        json!({ "did": "bob", "handle": "bob.awiki.ai", "kind": "direct" })
    );
    assert_eq!(
        direct["data"]["plan"]["message_type"],
        "attachment_manifest"
    );
    assert_eq!(direct["data"]["plan"]["transport"], "http");
    assert_eq!(direct["data"]["plan"]["local_writes"], json!(["messages"]));
    assert_eq!(
        direct["data"]["plan"]["attachment"],
        json!({
            "path": attachment.to_str().expect("attachment path"),
            "mime_type": "text/plain",
            "caption": "caption",
        })
    );

    let group = success_json(&awiki_cmd(
        &[
            "--dry-run",
            "--identity",
            "alice",
            "msg",
            "send",
            "--group",
            "did:wba:awiki.ai:groups:demo:e1_group",
            "--file",
            attachment.to_str().expect("attachment path"),
        ],
        workspace.path(),
    ));

    assert_eq!(group["data"]["plan"]["action"], "attachment.send");
    assert_eq!(
        group["data"]["plan"]["target"],
        json!({ "did": "did:wba:awiki.ai:groups:demo:e1_group", "kind": "group" })
    );
    assert_eq!(group["data"]["plan"]["attachment"]["caption"], "");
}

#[test]
fn msg_attachment_download_dry_run_exposes_cli_overwrite_policy() {
    let workspace = TempDir::new().expect("workspace");

    let direct = success_json(&awiki_cmd(
        &[
            "--dry-run",
            "--identity",
            "alice",
            "msg",
            "attachment",
            "download",
            "--with",
            "bob",
            "--message-id",
            "msg-1",
            "--attachment-id",
            "att-1",
            "--output",
            "out.bin",
        ],
        workspace.path(),
    ));

    assert_eq!(direct["summary"], "Dry run: attachment download planned");
    assert_eq!(direct["data"]["plan"]["action"], "attachment.download");
    assert_eq!(
        direct["data"]["plan"]["target"],
        json!({ "did": "bob", "handle": "bob.awiki.ai", "kind": "direct" })
    );
    assert_eq!(direct["data"]["plan"]["message_id"], "msg-1");
    assert_eq!(direct["data"]["plan"]["attachment_id"], "att-1");
    assert_eq!(direct["data"]["plan"]["output_path"], "out.bin");
    assert_eq!(direct["data"]["plan"]["overwrite"], true);
    assert_eq!(direct["data"]["plan"]["transport"], "http");
    assert_eq!(
        direct["data"]["plan"]["local_writes"],
        json!(["output_file"])
    );

    let group = success_json(&awiki_cmd(
        &[
            "--dry-run",
            "--identity",
            "alice",
            "msg",
            "attachment",
            "download",
            "--group",
            "did:wba:awiki.ai:groups:demo:e1_group",
            "--message-id",
            "group-msg-1",
            "--output",
            "group.bin",
        ],
        workspace.path(),
    ));

    assert_eq!(
        group["data"]["plan"]["target"],
        json!({ "did": "did:wba:awiki.ai:groups:demo:e1_group", "kind": "group" })
    );
    assert_eq!(group["data"]["plan"]["attachment_id"], "");
    assert_eq!(group["data"]["plan"]["overwrite"], true);
}

#[test]
fn msg_attachment_validation_stays_at_cli_boundary() {
    let workspace = TempDir::new().expect("workspace");
    let attachment = workspace.path().join("payload.txt");
    std::fs::write(&attachment, "attachment body").expect("write attachment");

    let mime_without_file = awiki_cmd(
        &[
            "--dry-run",
            "msg",
            "send",
            "--to",
            "bob",
            "--mime-type",
            "text/plain",
        ],
        workspace.path(),
    );
    assert_code(&mime_without_file, 2);
    let envelope = error_json(&mime_without_file);
    assert_eq!(envelope["error"]["code"], "invalid_argument");
    assert_contains(
        &envelope["error"]["message"],
        "mime_type requires an attachment file",
    );

    let secure_attachment = awiki_cmd(
        &[
            "--identity",
            "alice",
            "msg",
            "send",
            "--to",
            "bob",
            "--file",
            attachment.to_str().expect("attachment path"),
            "--secure",
            "on",
        ],
        workspace.path(),
    );
    assert_code(&secure_attachment, 2);
    let envelope = error_json(&secure_attachment);
    assert_eq!(envelope["error"]["code"], "unsupported_capability");
    assert_contains(&envelope["error"]["message"], "secure attachment");

    let missing_file = awiki_cmd(
        &[
            "--identity",
            "alice",
            "msg",
            "send",
            "--to",
            "bob",
            "--file",
            workspace
                .path()
                .join("missing.bin")
                .to_str()
                .expect("missing path"),
        ],
        workspace.path(),
    );
    assert_code(&missing_file, 2);
    let envelope = error_json(&missing_file);
    assert_eq!(envelope["error"]["code"], "invalid_argument");
    assert_contains(
        &envelope["error"]["message"],
        "attachment file path is unavailable",
    );

    let missing_output = awiki_cmd(
        &[
            "msg",
            "attachment",
            "download",
            "--with",
            "bob",
            "--message-id",
            "msg-1",
        ],
        workspace.path(),
    );
    assert_code(&missing_output, 2);
    let envelope = error_json(&missing_output);
    assert_eq!(envelope["error"]["code"], "invalid_argument");
    assert_contains(
        &envelope["error"]["message"],
        "attachment output path is required",
    );

    let target_conflict = awiki_cmd(
        &[
            "msg",
            "attachment",
            "download",
            "--with",
            "bob",
            "--group",
            "did:group",
            "--message-id",
            "msg-1",
            "--output",
            "out.bin",
        ],
        workspace.path(),
    );
    assert_code(&target_conflict, 2);
    let envelope = error_json(&target_conflict);
    assert_eq!(envelope["error"]["code"], "invalid_argument");
    assert_contains(
        &envelope["error"]["message"],
        "attachment download accepts either --with or --group",
    );
}

fn awiki_cmd(args: &[&str], workspace: &Path) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_awiki-cli"));
    command
        .args(args)
        .env("AWIKI_CLI_WORKSPACE_HOME_DIR", workspace)
        .env("HOME", workspace.join("home"))
        .env("USERPROFILE", workspace.join("home"))
        .env("AWIKI_CLI_UPDATE_CACHE_ONLY", "1")
        .env_remove("AWIKI_WORKSPACE")
        .env_remove("AWIKI_WORKSPACE_HOME")
        .env_remove("AWIKI_HOME")
        .env_remove("AVIKI_WORKSPACE_HOME")
        .env_remove("AWIKI_FORMAT")
        .env_remove("AVIKI_FORMAT");
    command.output().expect("run awiki-cli")
}

fn success_json(output: &Output) -> Value {
    assert_success(output);
    serde_json::from_slice(&output.stdout).expect("success JSON")
}

fn error_json(output: &Output) -> Value {
    assert!(
        !output.status.success(),
        "command should fail\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stderr).expect("error JSON")
}

fn assert_success(output: &Output) {
    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stderr.is_empty(),
        "stderr should be empty: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn assert_code(output: &Output, expected: i32) {
    assert_eq!(
        output.status.code(),
        Some(expected),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn assert_contains(value: &Value, needle: &str) {
    let haystack = value.as_str().unwrap_or_default();
    assert!(
        haystack.contains(needle),
        "{haystack:?} should contain {needle:?}"
    );
}

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new() -> std::io::Result<Self> {
        let mut path = std::env::temp_dir();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        path.push(format!(
            "awiki-msg-attachment-contract-{}-{nanos}",
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
