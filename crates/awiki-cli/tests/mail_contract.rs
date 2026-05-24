use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn mail_schema_is_on_default_im_core_surface() {
    let workspace = TempDir::new().expect("workspace");
    let output = awiki_cmd(&["schema", "mail"], workspace.path());
    assert_success(&output);
    let envelope = success_json(&output);

    assert_eq!(envelope["data"]["command"]["name"], "mail");
    assert_eq!(envelope["data"]["command"]["cutover"]["status"], "im_core");
    assert_eq!(
        envelope["data"]["command"]["cutover"]["default_surface"],
        true
    );

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
fn mail_dry_run_commands_plan_im_core_email_calls_without_side_effects() {
    let workspace = TempDir::new().expect("workspace");
    let attachment_path = workspace.path().join("planned").join("attachment.txt");
    let attachment_path_string = attachment_path.to_string_lossy().into_owned();

    for (args, command, action) in [
        (
            &[
                "--dry-run",
                "mail",
                "inbox",
                "--folder",
                "archive",
                "--limit",
                "2",
                "--offset",
                "1",
                "--unread",
            ][..],
            "awiki-cli mail inbox",
            "mail.getInbox",
        ),
        (
            &["--dry-run", "mail", "notify", "--limit", "3"][..],
            "awiki-cli mail notify",
            "mail.notifications",
        ),
        (
            &["--dry-run", "mail", "read", "--id", "msg-1"][..],
            "awiki-cli mail read",
            "mail.getMessage",
        ),
        (
            &["--dry-run", "mail", "mark-read", "msg-1"][..],
            "awiki-cli mail mark-read",
            "mail.markRead",
        ),
        (
            &["--dry-run", "mail", "account"][..],
            "awiki-cli mail account",
            "mail.getMailbox",
        ),
        (
            &[
                "--dry-run",
                "mail",
                "send",
                "--to",
                "bob@example.com",
                "--cc",
                "copy@example.com",
                "--subject",
                "Hello",
                "--body",
                "Body",
                "--html",
                "<p>Body</p>",
            ][..],
            "awiki-cli mail send",
            "mail.send",
        ),
        (
            &[
                "--dry-run",
                "mail",
                "attachment",
                "download",
                "--message-id",
                "msg-1",
                "--attachment-index",
                "0",
                "--output",
                &attachment_path_string,
            ][..],
            "awiki-cli mail attachment download",
            "mail.getAttachment",
        ),
    ] {
        let output = awiki_cmd(args, workspace.path());
        assert_success(&output);
        let envelope = success_json(&output);
        assert_eq!(envelope["command"], command);
        assert_eq!(envelope["meta"]["dry_run"], true);
        assert_eq!(envelope["data"]["plan"]["action"], action);
    }

    assert!(
        !attachment_path.exists(),
        "dry-run mail attachment download must not write output files"
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
        .env_remove("AVIKI_FORMAT")
        .env_remove("AWIKI_CLI_TRACE_TIMING");
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
    assert!(
        output.stderr.is_empty(),
        "stderr should be empty: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn success_json(output: &Output) -> Value {
    let envelope: Value =
        serde_json::from_slice(&output.stdout).expect("stdout should be a JSON success envelope");
    assert_eq!(envelope["ok"], true);
    envelope
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
            "awiki-cli-rs2-mail-cutover-test-{}-{nanos}",
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
