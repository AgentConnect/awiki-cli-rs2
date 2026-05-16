use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn msg_schema_exposes_go_command_surface() {
    let workspace = TempDir::new().expect("workspace");
    let output = awiki_cmd(&["schema", "msg"], workspace.path());
    assert_success(&output);
    let envelope = success_json(&output);

    assert_eq!(envelope["data"]["command"]["name"], "msg");
    let children: Vec<_> = envelope["data"]["children"]
        .as_array()
        .expect("children should be an array")
        .iter()
        .map(|child| child["name"].as_str().unwrap())
        .collect();
    for expected in [
        "msg.attachment",
        "msg.history",
        "msg.inbox",
        "msg.mark-read",
        "msg.secure",
        "msg.send",
    ] {
        assert!(
            children.contains(&expected),
            "msg schema children should include {expected}: {children:?}"
        );
    }

    let secure = success_json(&awiki_cmd(&["schema", "msg", "secure"], workspace.path()));
    let secure_children: Vec<_> = secure["data"]["children"]
        .as_array()
        .expect("secure children should be an array")
        .iter()
        .map(|child| child["name"].as_str().unwrap())
        .collect();
    for expected in [
        "msg.secure.drop",
        "msg.secure.failed",
        "msg.secure.init",
        "msg.secure.repair",
        "msg.secure.retry",
        "msg.secure.status",
    ] {
        assert!(
            secure_children.contains(&expected),
            "msg secure children should include {expected}: {secure_children:?}"
        );
    }
}

#[test]
fn msg_dry_run_plans_match_go_contracts() {
    let workspace = TempDir::new().expect("workspace");

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
            "hello",
        ],
        workspace.path(),
    ));
    assert_eq!(direct["summary"], "Dry run: message send planned");
    assert_eq!(direct["data"]["plan"]["action"], "direct.send");
    assert_eq!(direct["data"]["plan"]["identity"], "alice");
    assert_eq!(direct["data"]["plan"]["message_type"], "text");
    assert_eq!(direct["data"]["plan"]["runtime_mode"], "websocket");
    assert_eq!(direct["data"]["plan"]["transport"], "websocket");
    assert_eq!(direct["data"]["plan"]["local_writes"], json!(["messages"]));
    assert_eq!(
        direct["data"]["plan"]["target"],
        json!({ "did": "bob", "handle": "bob.awiki.ai", "kind": "direct" })
    );

    let secure_direct = success_json(&awiki_cmd(
        &[
            "--dry-run",
            "--identity",
            "alice",
            "msg",
            "send",
            "--to",
            "bob",
            "--text",
            "hello secure",
            "--secure",
            "on",
        ],
        workspace.path(),
    ));
    assert_eq!(secure_direct["summary"], "Dry run: message send planned");
    assert_eq!(secure_direct["data"]["plan"]["action"], "direct.send");
    assert_eq!(secure_direct["data"]["plan"]["message_type"], "text");
    assert_eq!(
        secure_direct["data"]["plan"]["target"]["handle"],
        "bob.awiki.ai"
    );

    let secure_direct_equals = success_json(&awiki_cmd(
        &[
            "--dry-run",
            "--identity",
            "alice",
            "msg",
            "send",
            "--to",
            "bob",
            "--text",
            "hello secure",
            "--secure=on",
        ],
        workspace.path(),
    ));
    assert_eq!(
        secure_direct_equals["data"]["plan"],
        secure_direct["data"]["plan"]
    );

    let attachment_send = success_json(&awiki_cmd(
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
            "/tmp/demo.txt",
            "--mime-type",
            "text/plain",
        ],
        workspace.path(),
    ));
    assert_eq!(attachment_send["data"]["plan"]["action"], "attachment.send");
    assert_eq!(
        attachment_send["data"]["plan"]["message_type"],
        "attachment_manifest"
    );
    assert_eq!(attachment_send["data"]["plan"]["transport"], "http");
    assert_eq!(
        attachment_send["data"]["plan"]["attachment"],
        json!({ "path": "/tmp/demo.txt", "mime_type": "text/plain", "caption": "caption" })
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
            "--text",
            "hello group",
        ],
        workspace.path(),
    ));
    assert_eq!(group["data"]["plan"]["action"], "group.send");
    assert_eq!(
        group["data"]["plan"]["target"],
        json!({ "did": "did:wba:awiki.ai:groups:demo:e1_group", "kind": "group" })
    );

    let download = success_json(&awiki_cmd(
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
    assert_eq!(download["summary"], "Dry run: attachment download planned");
    assert_eq!(download["data"]["plan"]["action"], "download_attachment");
    assert_eq!(download["data"]["plan"]["with_handle"], "bob.awiki.ai");
    assert_eq!(download["data"]["plan"]["transport"], "http");
    assert_eq!(download["data"]["plan"]["message_id"], "msg-1");
    assert_eq!(download["data"]["plan"]["attachment_id"], "att-1");
    assert_eq!(download["data"]["plan"]["output"], "out.bin");

    let inbox = success_json(&awiki_cmd(
        &[
            "--dry-run",
            "--identity",
            "alice",
            "msg",
            "inbox",
            "--scope",
            "all",
            "--with",
            "bob",
            "--unread",
            "--limit",
            "7",
            "--mark-read",
        ],
        workspace.path(),
    ));
    assert_eq!(inbox["summary"], "Dry run: inbox read planned");
    assert_eq!(inbox["data"]["plan"]["action"], "inbox.get");
    assert_eq!(inbox["data"]["plan"]["scope"], "all");
    assert_eq!(inbox["data"]["plan"]["with"], "bob");
    assert_eq!(inbox["data"]["plan"]["with_handle"], "bob.awiki.ai");
    assert_eq!(inbox["data"]["plan"]["group"], "");
    assert_eq!(inbox["data"]["plan"]["limit"], 7);
    assert_eq!(inbox["data"]["plan"]["mark_read"], true);
    assert_eq!(inbox["data"]["plan"].get("unread"), None);

    let history = success_json(&awiki_cmd(
        &[
            "--dry-run",
            "--identity",
            "alice",
            "msg",
            "history",
            "--with",
            "bob",
            "--limit",
            "15",
            "--cursor",
            "seq-2",
        ],
        workspace.path(),
    ));
    assert_eq!(history["summary"], "Dry run: direct history read planned");
    assert_eq!(history["data"]["plan"]["action"], "direct.get_history");
    assert_eq!(history["data"]["plan"]["with"], "bob");
    assert_eq!(history["data"]["plan"]["with_handle"], "bob.awiki.ai");
    assert_eq!(history["data"]["plan"]["limit"], 15);
    assert_eq!(history["data"]["plan"]["cursor"], "seq-2");

    let mark_read = success_json(&awiki_cmd(
        &[
            "--dry-run",
            "--identity",
            "alice",
            "msg",
            "mark-read",
            "msg-1",
            "msg-2",
        ],
        workspace.path(),
    ));
    assert_eq!(mark_read["summary"], "Dry run: mark-read planned");
    assert_eq!(mark_read["data"]["plan"]["action"], "inbox.mark_read");
    assert_eq!(
        mark_read["data"]["plan"]["message_ids"],
        json!(["msg-1", "msg-2"])
    );
}

#[test]
fn msg_secure_dry_run_plans_match_go_contracts() {
    let workspace = TempDir::new().expect("workspace");

    let status = success_json(&awiki_cmd(
        &[
            "--dry-run",
            "--identity",
            "alice",
            "msg",
            "secure",
            "status",
            "--with",
            "bob",
        ],
        workspace.path(),
    ));
    assert_eq!(status["summary"], "Dry run: secure status planned");
    assert_eq!(status["data"]["plan"]["action"], "msg.secure.status");
    assert_eq!(status["data"]["plan"]["identity"], "alice");
    assert_eq!(status["data"]["plan"]["with"], "bob");

    let init = success_json(&awiki_cmd(
        &[
            "--dry-run",
            "--identity",
            "alice",
            "msg",
            "secure",
            "init",
            "--with",
            "bob",
        ],
        workspace.path(),
    ));
    assert_eq!(init["summary"], "Dry run: secure init planned");
    assert_eq!(init["data"]["plan"]["action"], "msg.secure.init");
    assert_eq!(init["data"]["plan"]["with"], "bob");

    let repair = success_json(&awiki_cmd(
        &[
            "--dry-run",
            "--identity",
            "alice",
            "msg",
            "secure",
            "repair",
            "--with",
            "bob",
        ],
        workspace.path(),
    ));
    assert_eq!(repair["summary"], "Dry run: secure repair planned");
    assert_eq!(repair["data"]["plan"]["action"], "msg.secure.repair");
    assert_eq!(repair["data"]["plan"]["with"], "bob");

    let failed = success_json(&awiki_cmd(
        &[
            "--dry-run",
            "--identity",
            "alice",
            "msg",
            "secure",
            "failed",
        ],
        workspace.path(),
    ));
    assert_eq!(failed["summary"], "Dry run: secure failed listing planned");
    assert_eq!(failed["data"]["plan"]["action"], "msg.secure.failed");
    assert_eq!(failed["data"]["plan"]["identity"], "alice");

    let retry = success_json(&awiki_cmd(
        &[
            "--dry-run",
            "--identity",
            "alice",
            "msg",
            "secure",
            "retry",
            "outbox-1",
        ],
        workspace.path(),
    ));
    assert_eq!(retry["summary"], "Dry run: secure retry planned");
    assert_eq!(retry["data"]["plan"]["action"], "msg.secure.retry");
    assert_eq!(retry["data"]["plan"]["outbox_id"], "outbox-1");

    let drop = success_json(&awiki_cmd(
        &[
            "--dry-run",
            "--identity",
            "alice",
            "msg",
            "secure",
            "drop",
            "outbox-1",
        ],
        workspace.path(),
    ));
    assert_eq!(drop["summary"], "Dry run: secure drop planned");
    assert_eq!(drop["data"]["plan"]["action"], "msg.secure.drop");
    assert_eq!(drop["data"]["plan"]["outbox_id"], "outbox-1");
}

#[test]
fn msg_validation_errors_match_go_handler_boundary() {
    let workspace = TempDir::new().expect("workspace");

    let missing_target = awiki_cmd(
        &["--dry-run", "msg", "send", "--text", "hello"],
        workspace.path(),
    );
    assert_code(&missing_target, 2);
    let envelope = error_json(&missing_target);
    assert_contains(
        &envelope["error"]["message"],
        "msg send requires either --to or --group.",
    );

    let target_conflict = awiki_cmd(
        &[
            "--dry-run",
            "msg",
            "send",
            "--to",
            "bob",
            "--group",
            "did:group",
            "--text",
            "hello",
        ],
        workspace.path(),
    );
    assert_code(&target_conflict, 2);
    let envelope = error_json(&target_conflict);
    assert_contains(
        &envelope["error"]["message"],
        "msg send accepts either --to or --group, but not both.",
    );

    let missing_text = awiki_cmd(
        &["--dry-run", "msg", "send", "--to", "bob"],
        workspace.path(),
    );
    assert_code(&missing_text, 2);
    let envelope = error_json(&missing_text);
    assert_contains(
        &envelope["error"]["message"],
        "msg send requires --text or --text-file.",
    );

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
    assert_contains(
        &envelope["error"]["message"],
        "mime_type requires an attachment file",
    );

    let secure_retry_missing =
        awiki_cmd(&["--dry-run", "msg", "secure", "retry"], workspace.path());
    assert_code(&secure_retry_missing, 2);
    let envelope = error_json(&secure_retry_missing);
    assert_contains(
        &envelope["error"]["message"],
        "msg secure retry requires one outbox id.",
    );

    let secure_drop_missing = awiki_cmd(&["--dry-run", "msg", "secure", "drop"], workspace.path());
    assert_code(&secure_drop_missing, 2);
    let envelope = error_json(&secure_drop_missing);
    assert_contains(
        &envelope["error"]["message"],
        "msg secure drop requires one outbox id.",
    );

    let download_missing_target = awiki_cmd(
        &[
            "msg",
            "attachment",
            "download",
            "--message-id",
            "msg-1",
            "--output",
            "out.bin",
        ],
        workspace.path(),
    );
    assert_code(&download_missing_target, 2);
    let envelope = error_json(&download_missing_target);
    assert_eq!(envelope["error"]["code"], "invalid_argument");
    assert_contains(
        &envelope["error"]["message"],
        "attachment download requires either --with or --group",
    );

    let download_target_conflict = awiki_cmd(
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
    assert_code(&download_target_conflict, 2);
    let envelope = error_json(&download_target_conflict);
    assert_eq!(envelope["error"]["code"], "invalid_argument");
    assert_contains(
        &envelope["error"]["message"],
        "attachment download accepts either --with or --group, but not both",
    );
}

#[test]
fn msg_required_flag_errors_match_go_cobra_boundary() {
    let workspace = TempDir::new().expect("workspace");

    let history = awiki_cmd(&["--dry-run", "msg", "history"], workspace.path());
    assert_code(&history, 1);
    let envelope = error_json(&history);
    assert_eq!(envelope["error"]["code"], "internal_error");
    assert_contains(
        &envelope["error"]["message"],
        "required flag(s) \"with\" not set",
    );

    let attachment = awiki_cmd(
        &["--dry-run", "msg", "attachment", "download"],
        workspace.path(),
    );
    assert_code(&attachment, 1);
    let envelope = error_json(&attachment);
    assert_eq!(envelope["error"]["code"], "internal_error");
    assert_contains(
        &envelope["error"]["message"],
        "required flag(s) \"message-id\", \"output\" not set",
    );

    let init = awiki_cmd(&["--dry-run", "msg", "secure", "init"], workspace.path());
    assert_code(&init, 1);
    let envelope = error_json(&init);
    assert_eq!(envelope["error"]["code"], "internal_error");
    assert_contains(
        &envelope["error"]["message"],
        "required flag(s) \"with\" not set",
    );

    let repair = awiki_cmd(&["--dry-run", "msg", "secure", "repair"], workspace.path());
    assert_code(&repair, 1);
    let envelope = error_json(&repair);
    assert_eq!(envelope["error"]["code"], "internal_error");
    assert_contains(
        &envelope["error"]["message"],
        "required flag(s) \"with\" not set",
    );
}

fn awiki_cmd(args: &[&str], workspace: &Path) -> Output {
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
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "awiki-cli-rs2-msg-test-{}-{nanos}",
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
