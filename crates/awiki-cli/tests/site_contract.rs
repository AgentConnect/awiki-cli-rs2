use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn site_schema_is_supported_on_default_cutover_surface() {
    let workspace = TempDir::new().expect("workspace");
    let output = awiki_cmd(&["schema", "site"], workspace.path());
    assert_success(&output);
    let envelope = success_json(&output);

    assert_eq!(envelope["data"]["command"]["name"], "site");
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
    for expected in ["site.page", "site.root"] {
        assert!(
            children.contains(&expected),
            "site schema children should include {expected}: {children:?}"
        );
    }
}

#[test]
fn site_dry_run_plans_keep_cli_envelope_contract() {
    let workspace = TempDir::new().expect("workspace");
    let cases = [
        (
            vec![
                "--dry-run",
                "site",
                "root",
                "get",
                "--domain",
                "tenant.example",
            ],
            "site.root.get",
            "get_root",
            "Dry run: site root get planned",
        ),
        (
            vec![
                "--dry-run",
                "site",
                "root",
                "set",
                "--domain",
                "tenant.example",
                "--markdown",
                "Body",
            ],
            "site.root.set",
            "set_root",
            "Dry run: site root set planned",
        ),
        (
            vec![
                "--dry-run",
                "site",
                "page",
                "list",
                "--domain",
                "tenant.example",
            ],
            "site.page.list",
            "list_pages",
            "Dry run: site page list planned",
        ),
        (
            vec![
                "--dry-run",
                "site",
                "page",
                "get",
                "--domain",
                "tenant.example",
                "--slug",
                "hello",
            ],
            "site.page.get",
            "get_page",
            "Dry run: site page get planned",
        ),
        (
            vec![
                "--dry-run",
                "site",
                "page",
                "create",
                "--domain",
                "tenant.example",
                "--slug",
                "hello",
                "--markdown",
                "Body",
            ],
            "site.page.create",
            "create_page",
            "Dry run: site page create planned",
        ),
        (
            vec![
                "--dry-run",
                "site",
                "page",
                "update",
                "--domain",
                "tenant.example",
                "--slug",
                "hello",
                "--markdown",
                "Body",
            ],
            "site.page.update",
            "update_page",
            "Dry run: site page update planned",
        ),
        (
            vec![
                "--dry-run",
                "site",
                "page",
                "rename",
                "--domain",
                "tenant.example",
                "--slug",
                "hello",
                "--to",
                "new",
            ],
            "site.page.rename",
            "rename_page",
            "Dry run: site page rename planned",
        ),
        (
            vec![
                "--dry-run",
                "site",
                "page",
                "delete",
                "--domain",
                "tenant.example",
                "--slug",
                "hello",
            ],
            "site.page.delete",
            "delete_page",
            "Dry run: site page delete planned",
        ),
    ];

    for (args, action, rpc_method, summary) in cases {
        let output = awiki_cmd(&args, workspace.path());
        assert_success(&output);
        let envelope = success_json(&output);
        assert_eq!(envelope["summary"], summary);
        assert_eq!(envelope["data"]["plan"]["action"], action);
        assert_eq!(envelope["data"]["plan"]["rpc_endpoint"], "/site/rpc");
        assert_eq!(envelope["data"]["plan"]["rpc_method"], rpc_method);
    }
}

#[test]
fn site_argument_errors_stay_cli_owned() {
    let workspace = TempDir::new().expect("workspace");

    let missing_domain = awiki_cmd(&["--dry-run", "site", "root", "get"], workspace.path());
    assert_error_code(&missing_domain, "invalid_argument");

    let missing_body = awiki_cmd(
        &[
            "--dry-run",
            "site",
            "root",
            "set",
            "--domain",
            "tenant.example",
        ],
        workspace.path(),
    );
    assert_error_code(&missing_body, "invalid_argument");

    let conflicting_body = awiki_cmd(
        &[
            "--dry-run",
            "site",
            "page",
            "create",
            "--domain",
            "tenant.example",
            "--slug",
            "hello",
            "--markdown",
            "Body",
            "--markdown-file",
            "body.md",
        ],
        workspace.path(),
    );
    assert_error_code(&conflicting_body, "invalid_argument");
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

fn assert_error_code(output: &Output, code: &str) {
    assert_eq!(
        output.status.code(),
        Some(2),
        "unexpected exit status; stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let envelope: Value =
        serde_json::from_slice(&output.stderr).expect("stderr should be a JSON error envelope");
    assert_eq!(envelope["ok"], false);
    assert_eq!(envelope["error"]["code"], code);
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
            "awiki-cli-rs2-site-cutover-test-{}-{nanos}",
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
