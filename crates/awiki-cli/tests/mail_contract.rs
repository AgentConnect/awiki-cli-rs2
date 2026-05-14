use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn mail_schema_exposes_go_command_surface() {
    let workspace = TempDir::new().expect("workspace");
    let output = awiki_cmd(&["schema", "mail"], workspace.path());
    assert_success(&output);
    let envelope = success_json(&output);

    assert_eq!(envelope["data"]["command"]["name"], "mail");
    let children: Vec<_> = envelope["data"]["children"]
        .as_array()
        .expect("children should be an array")
        .iter()
        .map(|child| child["name"].as_str().unwrap())
        .collect();
    for expected in [
        "mail.account",
        "mail.attachment",
        "mail.inbox",
        "mail.mark-read",
        "mail.notify",
        "mail.read",
        "mail.send",
    ] {
        assert!(
            children.contains(&expected),
            "mail schema children should include {expected}: {children:?}"
        );
    }
}

#[test]
fn mail_dry_run_plans_match_go_contracts() {
    let workspace = TempDir::new().expect("workspace");

    let inbox = success_json(&awiki_cmd(
        &[
            "--identity",
            "alice",
            "--dry-run",
            "mail",
            "inbox",
            "--folder",
            "archive",
            "--unread",
            "--limit",
            "7",
            "--offset",
            "2",
        ],
        workspace.path(),
    ));
    assert_eq!(inbox["summary"], "Dry run: mail inbox planned");
    assert_eq!(inbox["data"]["plan"]["action"], "mail.getInbox");
    assert_eq!(inbox["data"]["plan"]["identity"], "alice");
    assert_eq!(inbox["data"]["plan"]["folder"], "archive");
    assert_eq!(inbox["data"]["plan"]["unread_only"], true);
    assert_eq!(
        inbox["data"]["plan"]["remote_calls"][0],
        "POST /mail/rpc mail.getInbox"
    );

    let send = success_json(&awiki_cmd(
        &[
            "--identity",
            "alice",
            "--dry-run",
            "mail",
            "send",
            "--to",
            "bob@example.com,carol@example.com dave@example.com",
            "--cc",
            "copy@example.com;mirror@example.com",
            "--subject",
            "Hello",
            "--body",
            "Body",
            "--html",
            "<p>Body</p>",
        ],
        workspace.path(),
    ));
    assert_eq!(send["summary"], "Dry run: mail send planned");
    assert_eq!(send["data"]["plan"]["action"], "mail.send");
    assert_eq!(
        send["data"]["plan"]["to"],
        json!(["bob@example.com", "carol@example.com", "dave@example.com"])
    );
    assert_eq!(
        send["data"]["plan"]["cc"],
        json!(["copy@example.com", "mirror@example.com"])
    );
    assert_eq!(send["data"]["plan"]["has_html"], true);

    let read = success_json(&awiki_cmd(
        &["--dry-run", "mail", "read", "--id", "msg-1"],
        workspace.path(),
    ));
    assert_eq!(read["data"]["plan"]["action"], "mail.getMessage");

    let mark_read = success_json(&awiki_cmd(
        &["--dry-run", "mail", "mark-read", "msg-1", "msg-2"],
        workspace.path(),
    ));
    assert_eq!(mark_read["data"]["plan"]["action"], "mail.markRead");
    assert_eq!(
        mark_read["data"]["plan"]["message_ids"],
        json!(["msg-1", "msg-2"])
    );

    let account = success_json(&awiki_cmd(
        &["--dry-run", "mail", "account"],
        workspace.path(),
    ));
    assert_eq!(account["data"]["plan"]["action"], "mail.getMailbox");

    let attachment = success_json(&awiki_cmd(
        &[
            "--dry-run",
            "mail",
            "attachment",
            "download",
            "--message-id",
            "msg-1",
            "--attachment-index",
            "1",
            "--output",
            "out.txt",
        ],
        workspace.path(),
    ));
    assert_eq!(attachment["data"]["plan"]["action"], "mail.getAttachment");
    assert_eq!(attachment["data"]["plan"]["attachment_index"], 1);
    assert_eq!(attachment["data"]["plan"]["output"], "out.txt");

    let notify = success_json(&awiki_cmd(
        &["--dry-run", "mail", "notify"],
        workspace.path(),
    ));
    assert_eq!(notify["data"]["plan"]["action"], "mail.notifications");
    assert_eq!(notify["data"]["plan"]["remote_calls"], json!([]));
}

#[test]
fn mail_validation_errors_match_go_messages() {
    let workspace = TempDir::new().expect("workspace");

    let missing_id = awiki_cmd(&["mail", "read"], workspace.path());
    assert_code(&missing_id, 2);
    let envelope = error_json(&missing_id);
    assert_eq!(envelope["error"]["code"], "invalid_argument");
    assert_contains(&envelope["error"]["message"], "mail read requires --id.");

    let missing_mark = awiki_cmd(&["mail", "mark-read"], workspace.path());
    assert_code(&missing_mark, 2);
    let envelope = error_json(&missing_mark);
    assert_contains(
        &envelope["error"]["message"],
        "mail mark-read requires at least one message id.",
    );

    let missing_to = awiki_cmd(
        &["mail", "send", "--subject", "Hello", "--body", "Body"],
        workspace.path(),
    );
    assert_code(&missing_to, 2);
    let envelope = error_json(&missing_to);
    assert_contains(&envelope["error"]["message"], "mail send requires --to.");

    let missing_subject = awiki_cmd(
        &[
            "mail",
            "send",
            "--to",
            "alice@example.com",
            "--body",
            "Body",
        ],
        workspace.path(),
    );
    assert_code(&missing_subject, 2);
    let envelope = error_json(&missing_subject);
    assert_contains(
        &envelope["error"]["message"],
        "mail send requires --subject.",
    );

    let missing_body = awiki_cmd(
        &[
            "mail",
            "send",
            "--to",
            "alice@example.com",
            "--subject",
            "Hello",
        ],
        workspace.path(),
    );
    assert_code(&missing_body, 2);
    let envelope = error_json(&missing_body);
    assert_contains(&envelope["error"]["message"], "mail send requires --body.");

    let missing_message = awiki_cmd(&["mail", "attachment", "download"], workspace.path());
    assert_code(&missing_message, 2);
    let envelope = error_json(&missing_message);
    assert_contains(
        &envelope["error"]["message"],
        "mail attachment download requires --message-id.",
    );

    let negative_index = awiki_cmd(
        &[
            "mail",
            "attachment",
            "download",
            "--message-id",
            "msg-1",
            "--attachment-index",
            "-1",
        ],
        workspace.path(),
    );
    assert_code(&negative_index, 2);
    let envelope = error_json(&negative_index);
    assert_contains(
        &envelope["error"]["message"],
        "attachment index must be >= 0.",
    );
}

#[test]
fn mail_notify_reads_and_normalizes_local_sqlite_cache() {
    let workspace = TempDir::new().expect("workspace");

    let create = awiki_cmd(
        &[
            "id",
            "create",
            "--name",
            "Alice Mail",
            "--identity",
            "alice-mail",
        ],
        workspace.path(),
    );
    assert_success(&create);
    let owner_did =
        register_identity_for_mail(workspace.path(), "alice-mail", "alice", "user-alice");

    let init = awiki_cmd(&["init"], workspace.path());
    assert_success(&init);
    insert_mail_notification(workspace.path(), &owner_did);

    let output = awiki_cmd(
        &["--identity", "alice-mail", "mail", "notify"],
        workspace.path(),
    );
    assert_success(&output);
    let envelope = success_json(&output);
    assert_eq!(envelope["summary"], "Loaded 1 mail notification(s)");
    assert_eq!(envelope["data"]["total"], 1);
    let notification = &envelope["data"]["notifications"][0];
    assert_eq!(notification["source_kind"], "mail");
    assert_eq!(notification["title"], "[邮件] Mail subject");
    assert_contains(&notification["content"], "[邮件] 收件邮箱: alice@awiki.ai");
    assert_contains(&notification["content"], "发件人: sender@example.com");
    assert_contains(&notification["content"], "主题: Mail subject");
    assert_contains(&notification["content"], "Preview text");
    assert_contains(&notification["content"], "(这封邮件包含附件)");
}

fn register_identity_for_mail(
    workspace: &Path,
    identity_name: &str,
    handle: &str,
    user_id: &str,
) -> String {
    let index_path = workspace.join("identities").join("index.json");
    let mut index: Value = serde_json::from_slice(&std::fs::read(&index_path).unwrap()).unwrap();
    index["credentials"][identity_name]["handle"] = json!(handle);
    index["credentials"][identity_name]["full_handle"] = json!(format!("{handle}@awiki.ai"));
    index["credentials"][identity_name]["user_id"] = json!(user_id);
    std::fs::write(&index_path, serde_json::to_vec_pretty(&index).unwrap()).unwrap();

    let dir_name = index["credentials"][identity_name]["dir_name"]
        .as_str()
        .unwrap();
    let identity_path = workspace
        .join("identities")
        .join(dir_name)
        .join("identity.json");
    let mut identity: Value =
        serde_json::from_slice(&std::fs::read(&identity_path).unwrap()).unwrap();
    identity["handle"] = json!(handle);
    identity["full_handle"] = json!(format!("{handle}@awiki.ai"));
    identity["user_id"] = json!(user_id);
    let did = identity["did"].as_str().unwrap().to_string();
    std::fs::write(
        &identity_path,
        serde_json::to_vec_pretty(&identity).unwrap(),
    )
    .unwrap();
    did
}

fn insert_mail_notification(workspace: &Path, owner_did: &str) {
    let db_path = workspace.join("data").join("awiki-cli.db");
    let connection = rusqlite::Connection::open(db_path).unwrap();
    connection
        .execute(
            r#"
INSERT INTO messages (
    msg_id, owner_did, thread_id, direction, receiver_did, content_type, content,
    title, sent_at, stored_at, is_read, metadata, credential_name
) VALUES (
    ?1, ?2, ?3, 0, ?2, 'text/plain', ?4, ?5, ?6, ?6, 0, ?7, 'alice-mail'
)
"#,
            rusqlite::params![
                "mail-message-1",
                owner_did,
                "mail:alice@awiki.ai",
                "old content",
                "[邮件] Old title",
                "2026-05-14T00:00:00Z",
                r#"{"source_kind":"mail","mailbox_address":"alice@awiki.ai","from_addr":"sender@example.com","subject":"Mail subject","preview":"Preview text","has_attachments":"yes"}"#
            ],
        )
        .unwrap();
}

fn awiki_cmd(args: &[&str], workspace: &Path) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_awiki-cli"));
    command
        .args(args)
        .env("AWIKI_CLI_WORKSPACE_HOME_DIR", workspace)
        .env_remove("AWIKI_WORKSPACE")
        .env_remove("AWIKI_WORKSPACE_HOME")
        .env_remove("AWIKI_HOME")
        .env_remove("AVIKI_WORKSPACE_HOME")
        .env_remove("AWIKI_FORMAT")
        .env_remove("AVIKI_FORMAT");
    command.output().expect("run awiki-cli binary")
}

fn assert_success(output: &Output) {
    assert_code(output, 0);
}

fn assert_code(output: &Output, code: i32) {
    assert_eq!(
        output.status.code(),
        Some(code),
        "unexpected exit status; stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
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
        output.stdout.is_empty(),
        "stdout should be empty: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let envelope: Value =
        serde_json::from_slice(&output.stderr).expect("stderr should be a JSON error envelope");
    assert_eq!(envelope["ok"], false);
    envelope
}

fn assert_contains(value: &Value, needle: &str) {
    let haystack = value
        .as_str()
        .unwrap_or_else(|| panic!("expected string containing {needle:?}, got {value:?}"));
    assert!(
        haystack.contains(needle),
        "expected {haystack:?} to contain {needle:?}"
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
            "awiki-cli-rs2-mail-test-{}-{nanos}",
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
