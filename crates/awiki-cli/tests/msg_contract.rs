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

    let secure_direct = awiki_cmd(
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
    );
    assert_code(&secure_direct, 2);
    let secure_direct = error_json(&secure_direct);
    assert_eq!(secure_direct["error"]["code"], "unsupported_capability");
    assert_eq!(secure_direct["error"]["details"]["command"], "msg.send");
    assert_eq!(
        secure_direct["error"]["details"]["capability"],
        "secure-direct"
    );
    assert_eq!(
        secure_direct["error"]["details"]["required_phase"],
        "Phase 6"
    );

    let secure_direct_equals = awiki_cmd(
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
    );
    assert_code(&secure_direct_equals, 2);
    let secure_direct_equals = error_json(&secure_direct_equals);
    assert_eq!(
        secure_direct_equals["error"]["message"],
        secure_direct["error"]["message"]
    );

    let attachment_send = awiki_cmd(
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
        ],
        workspace.path(),
    );
    assert_success(&attachment_send);
    let attachment_send = success_json(&attachment_send);
    assert_eq!(attachment_send["summary"], "Dry run: message send planned");
    assert_eq!(attachment_send["data"]["plan"]["action"], "attachment.send");
    assert_eq!(
        attachment_send["data"]["plan"]["target"],
        json!({ "did": "bob", "handle": "bob.awiki.ai", "kind": "direct" })
    );
    assert_eq!(
        attachment_send["data"]["plan"]["message_type"],
        "attachment_manifest"
    );
    assert_eq!(attachment_send["data"]["plan"]["transport"], "http");
    assert_eq!(
        attachment_send["data"]["plan"]["attachment"],
        json!({
            "path": "/tmp/demo.txt",
            "mime_type": "",
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

    let download = awiki_cmd(
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
    );
    assert_success(&download);
    let download = success_json(&download);
    assert_eq!(download["summary"], "Dry run: attachment download planned");
    assert_eq!(download["data"]["plan"]["action"], "attachment.download");
    assert_eq!(
        download["data"]["plan"]["target"],
        json!({ "did": "bob", "handle": "bob.awiki.ai", "kind": "direct" })
    );
    assert_eq!(download["data"]["plan"]["message_id"], "msg-1");
    assert_eq!(download["data"]["plan"]["attachment_id"], "att-1");
    assert_eq!(download["data"]["plan"]["output_path"], "out.bin");
    assert_eq!(download["data"]["plan"]["overwrite"], true);

    let inbox = success_json(&awiki_cmd(
        &[
            "--dry-run",
            "--identity",
            "alice",
            "msg",
            "inbox",
            "--scope",
            "all",
            "--unread",
            "--limit",
            "7",
        ],
        workspace.path(),
    ));
    assert_eq!(inbox["summary"], "Dry run: inbox read planned");
    assert_eq!(inbox["data"]["plan"]["action"], "inbox.get");
    assert_eq!(inbox["data"]["plan"]["scope"], "all");
    assert_eq!(inbox["data"]["plan"]["with"], "");
    assert_eq!(inbox["data"]["plan"].get("with_handle"), None);
    assert_eq!(inbox["data"]["plan"]["group"], "");
    assert_eq!(inbox["data"]["plan"]["limit"], 7);
    assert_eq!(inbox["data"]["plan"]["mark_read"], false);
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
fn msg_send_default_cutover_dry_run_routes_direct_and_group_text() {
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
            "--secure",
            "plain",
        ],
        workspace.path(),
    ));
    assert_eq!(direct["summary"], "Dry run: message send planned");
    assert_eq!(direct["data"]["plan"]["action"], "direct.send");
    assert_eq!(direct["data"]["plan"]["identity"], "alice");
    assert_eq!(
        direct["data"]["plan"]["target"],
        json!({ "did": "bob", "handle": "bob.awiki.ai", "kind": "direct" })
    );

    let text_path = workspace.path().join("body.txt");
    std::fs::write(&text_path, "hello from file").expect("write text file");
    let text_file = success_json(&awiki_cmd(
        &[
            "--dry-run",
            "--identity",
            "alice",
            "msg",
            "send",
            "--to",
            "bob",
            "--text-file",
            text_path.to_str().unwrap(),
        ],
        workspace.path(),
    ));
    assert_eq!(text_file["data"]["plan"]["action"], "direct.send");

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
    assert_eq!(group["data"]["plan"]["identity"], "alice");
    assert_eq!(group["data"]["plan"]["message_type"], "text");
    assert_eq!(
        group["data"]["plan"]["target"],
        json!({ "did": "did:wba:awiki.ai:groups:demo:e1_group", "kind": "group" })
    );

    let group_text_path = workspace.path().join("group-body.txt");
    std::fs::write(&group_text_path, "hello group from file").expect("write group text file");
    let group_text_file = success_json(&awiki_cmd(
        &[
            "--dry-run",
            "--identity",
            "alice",
            "msg",
            "send",
            "--group",
            "did:wba:awiki.ai:groups:demo:e1_group",
            "--text-file",
            group_text_path.to_str().unwrap(),
        ],
        workspace.path(),
    ));
    assert_eq!(group_text_file["data"]["plan"]["action"], "group.send");
    assert_eq!(
        group_text_file["data"]["plan"]["target"],
        json!({ "did": "did:wba:awiki.ai:groups:demo:e1_group", "kind": "group" })
    );
}

#[test]
fn msg_send_default_cutover_supports_attachment_dry_run_and_rejects_secure_direct() {
    let workspace = TempDir::new().expect("workspace");

    let attachment = awiki_cmd(
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
        ],
        workspace.path(),
    );
    assert_success(&attachment);
    let attachment = success_json(&attachment);
    assert_eq!(attachment["data"]["plan"]["action"], "attachment.send");
    assert_eq!(
        attachment["data"]["plan"]["attachment"]["path"],
        "/tmp/demo.txt"
    );

    let secure = awiki_cmd(
        &[
            "--dry-run",
            "--identity",
            "alice",
            "msg",
            "send",
            "--to",
            "bob",
            "--text",
            "secret",
            "--secure",
            "on",
        ],
        workspace.path(),
    );
    assert_code(&secure, 2);
    let secure = error_json(&secure);
    assert_eq!(secure["error"]["code"], "unsupported_capability");
    assert_eq!(secure["error"]["details"]["command"], "msg.send");
    assert_eq!(secure["error"]["details"]["capability"], "secure-direct");
    assert_eq!(secure["error"]["details"]["required_phase"], "Phase 6");
}

#[test]
fn msg_read_default_cutover_dry_run_routes_inbox_and_history_subset() {
    let workspace = TempDir::new().expect("workspace");

    let inbox = success_json(&awiki_cmd(
        &[
            "--dry-run",
            "--identity",
            "alice",
            "msg",
            "inbox",
            "--scope",
            "group",
            "--unread",
            "--limit",
            "5",
        ],
        workspace.path(),
    ));
    assert_eq!(inbox["summary"], "Dry run: inbox read planned");
    assert_eq!(inbox["data"]["plan"]["action"], "inbox.get");
    assert_eq!(inbox["data"]["plan"]["identity"], "alice");
    assert_eq!(inbox["data"]["plan"]["scope"], "group");
    assert_eq!(inbox["data"]["plan"]["with"], "");
    assert_eq!(inbox["data"]["plan"]["group"], "");
    assert_eq!(inbox["data"]["plan"]["limit"], 5);
    assert_eq!(inbox["data"]["plan"]["mark_read"], false);

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
            "5",
            "--cursor",
            "seq-2",
        ],
        workspace.path(),
    ));
    assert_eq!(history["summary"], "Dry run: direct history read planned");
    assert_eq!(history["data"]["plan"]["action"], "direct.get_history");
    assert_eq!(history["data"]["plan"]["identity"], "alice");
    assert_eq!(history["data"]["plan"]["with"], "bob");
    assert_eq!(history["data"]["plan"]["with_handle"], "bob.awiki.ai");
    assert_eq!(history["data"]["plan"]["limit"], 5);
    assert_eq!(history["data"]["plan"]["cursor"], "seq-2");
}

#[test]
fn msg_inbox_default_cutover_rejects_filters_and_mark_read_side_effect() {
    let workspace = TempDir::new().expect("workspace");

    let with_filter = awiki_cmd(
        &[
            "--dry-run",
            "--identity",
            "alice",
            "msg",
            "inbox",
            "--with",
            "bob",
            "--limit",
            "7",
        ],
        workspace.path(),
    );
    assert_code(&with_filter, 2);
    let with_filter = error_json(&with_filter);
    assert_eq!(with_filter["error"]["code"], "unsupported_capability");
    assert_eq!(with_filter["error"]["details"]["command"], "msg.inbox");
    assert_eq!(
        with_filter["error"]["details"]["capability"],
        "inbox-target-filters"
    );

    let group_filter = awiki_cmd(
        &[
            "--dry-run",
            "--identity",
            "alice",
            "msg",
            "inbox",
            "--group",
            "did:wba:awiki.ai:groups:demo:e1_group",
        ],
        workspace.path(),
    );
    assert_code(&group_filter, 2);
    let group_filter = error_json(&group_filter);
    assert_eq!(
        group_filter["error"]["details"]["capability"],
        "inbox-target-filters"
    );

    let mark_read = awiki_cmd(
        &[
            "--dry-run",
            "--identity",
            "alice",
            "msg",
            "inbox",
            "--mark-read",
        ],
        workspace.path(),
    );
    assert_code(&mark_read, 2);
    let mark_read = error_json(&mark_read);
    assert_eq!(
        mark_read["error"]["details"]["capability"],
        "inbox-mark-read-side-effect"
    );
}

#[test]
fn msg_secure_commands_are_cutover_unsupported() {
    let workspace = TempDir::new().expect("workspace");

    for args in [
        vec![
            "--dry-run",
            "--identity",
            "alice",
            "msg",
            "secure",
            "status",
            "--with",
            "bob",
        ],
        vec![
            "--dry-run",
            "--identity",
            "alice",
            "msg",
            "secure",
            "init",
            "--with",
            "bob",
        ],
        vec![
            "--dry-run",
            "--identity",
            "alice",
            "msg",
            "secure",
            "repair",
            "--with",
            "bob",
        ],
        vec![
            "--dry-run",
            "--identity",
            "alice",
            "msg",
            "secure",
            "failed",
        ],
        vec![
            "--dry-run",
            "--identity",
            "alice",
            "msg",
            "secure",
            "retry",
            "outbox-1",
        ],
        vec![
            "--dry-run",
            "--identity",
            "alice",
            "msg",
            "secure",
            "drop",
            "outbox-1",
        ],
    ] {
        let output = awiki_cmd(&args, workspace.path());
        assert_code(&output, 2);
        let envelope = error_json(&output);
        assert_eq!(envelope["error"]["code"], "unsupported_capability");
        assert_eq!(envelope["error"]["details"]["capability"], "secure-direct");
        assert_eq!(envelope["error"]["details"]["required_phase"], "Phase 6");
    }
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
        "attachment download accepts either --with or --group",
    );
}

#[test]
fn msg_unsupported_secure_mode_errors_match_cutover_boundary() {
    let workspace = TempDir::new().expect("workspace");
    let attachment = workspace.path().join("payload.txt");
    std::fs::write(&attachment, "attachment body").expect("write attachment");

    let secure_attachment = awiki_cmd(
        &[
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
            "--secure",
            "on",
        ],
        workspace.path(),
    );
    assert_code(&secure_attachment, 2);
    let envelope = error_json(&secure_attachment);
    assert_eq!(envelope["error"]["code"], "unsupported_capability");
    assert_contains(&envelope["error"]["message"], "secure attachment");
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
    assert_code(&attachment, 2);
    let envelope = error_json(&attachment);
    assert_eq!(envelope["error"]["code"], "invalid_argument");
    assert_contains(
        &envelope["error"]["message"],
        "attachment download requires either --with or --group",
    );

    let init = awiki_cmd(&["--dry-run", "msg", "secure", "init"], workspace.path());
    assert_code(&init, 2);
    let envelope = error_json(&init);
    assert_eq!(envelope["error"]["code"], "unsupported_capability");
    assert_eq!(envelope["error"]["details"]["command"], "msg.secure.init");
    assert_eq!(envelope["error"]["details"]["capability"], "secure-direct");

    let repair = awiki_cmd(&["--dry-run", "msg", "secure", "repair"], workspace.path());
    assert_code(&repair, 2);
    let envelope = error_json(&repair);
    assert_eq!(envelope["error"]["code"], "unsupported_capability");
    assert_eq!(envelope["error"]["details"]["command"], "msg.secure.repair");
    assert_eq!(envelope["error"]["details"]["capability"], "secure-direct");
}

fn awiki_cmd(args: &[&str], workspace: &Path) -> Output {
    awiki_cmd_with_env(args, workspace, &[])
}

fn awiki_cmd_with_env(args: &[&str], workspace: &Path, envs: &[(&str, &str)]) -> Output {
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
    for (key, value) in envs {
        command.env(key, value);
    }
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
