use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn page_schema_remains_exactly_queryable_but_outside_default_cutover_surface() {
    let workspace = TempDir::new().expect("workspace");
    let output = awiki_cmd(&["schema", "page"], workspace.path());
    assert_success(&output);
    let envelope = success_json(&output);

    assert_eq!(envelope["data"]["command"]["name"], "page");
    assert_eq!(
        envelope["data"]["command"]["cutover"]["status"],
        "unsupported"
    );
    assert_eq!(
        envelope["data"]["command"]["cutover"]["capability"],
        "page-site"
    );
    assert_eq!(
        envelope["data"]["command"]["cutover"]["default_surface"],
        false
    );

    let children: Vec<_> = envelope["data"]["children"]
        .as_array()
        .expect("children should be an array")
        .iter()
        .map(|child| child["name"].as_str().unwrap())
        .collect();
    for expected in [
        "page.create",
        "page.delete",
        "page.get",
        "page.list",
        "page.rename",
        "page.update",
    ] {
        assert!(
            children.contains(&expected),
            "page schema children should include {expected}: {children:?}"
        );
    }
}

#[test]
fn page_commands_return_cutover_unsupported_instead_of_legacy_plans() {
    let workspace = TempDir::new().expect("workspace");

    for (args, command) in [
        (&["page", "list"][..], "page.list"),
        (
            &["--dry-run", "page", "get", "--slug", "hello"][..],
            "page.get",
        ),
        (
            &[
                "--dry-run",
                "page",
                "create",
                "--slug",
                "hello",
                "--title",
                "Hello",
                "--markdown",
                "Body",
            ][..],
            "page.create",
        ),
        (
            &["page", "update", "--slug", "hello", "--title", "New"][..],
            "page.update",
        ),
        (
            &["page", "rename", "--slug", "hello", "--to", "new"][..],
            "page.rename",
        ),
        (&["page", "delete", "--slug", "hello"][..], "page.delete"),
    ] {
        let output = awiki_cmd(args, workspace.path());
        assert_cutover_unsupported(&output, command);
    }
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

fn assert_cutover_unsupported(output: &Output, command: &str) {
    assert_eq!(
        output.status.code(),
        Some(2),
        "unexpected exit status; stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stdout.is_empty(),
        "stdout should be empty: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let envelope: Value =
        serde_json::from_slice(&output.stderr).expect("stderr should be a JSON error envelope");
    assert_eq!(envelope["ok"], false);
    assert_eq!(envelope["error"]["code"], "unsupported_capability");
    assert_eq!(envelope["error"]["details"]["command"], command);
    assert_eq!(envelope["error"]["details"]["capability"], "page-site");
    assert_eq!(
        envelope["error"]["details"]["required_phase"],
        "outside current im-core cutover"
    );
    assert_eq!(
        envelope["error"]["details"]["cutover_status"],
        "unsupported"
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
            "awiki-cli-rs2-page-cutover-test-{}-{nanos}",
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
