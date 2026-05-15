use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn site_schema_exposes_go_command_surface() {
    let workspace = TempDir::new().expect("workspace");
    let output = awiki_cmd(&["schema", "site"], workspace.path());
    assert_success(&output);
    let envelope = success_json(&output);

    assert_eq!(envelope["data"]["command"]["name"], "site");
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

    let page = success_json(&awiki_cmd(&["schema", "site", "page"], workspace.path()));
    let page_children: Vec<_> = page["data"]["children"]
        .as_array()
        .expect("site page children should be an array")
        .iter()
        .map(|child| child["name"].as_str().unwrap())
        .collect();
    for expected in [
        "site.page.create",
        "site.page.delete",
        "site.page.get",
        "site.page.list",
        "site.page.rename",
        "site.page.update",
    ] {
        assert!(
            page_children.contains(&expected),
            "site page schema children should include {expected}: {page_children:?}"
        );
    }

    let root = success_json(&awiki_cmd(&["schema", "site", "root"], workspace.path()));
    let root_children: Vec<_> = root["data"]["children"]
        .as_array()
        .expect("site root children should be an array")
        .iter()
        .map(|child| child["name"].as_str().unwrap())
        .collect();
    for expected in ["site.root.get", "site.root.set"] {
        assert!(
            root_children.contains(&expected),
            "site root schema children should include {expected}: {root_children:?}"
        );
    }
}

#[test]
fn site_root_dry_run_plans_match_go_contracts() {
    let workspace = TempDir::new().expect("workspace");

    let get = success_json(&awiki_cmd(
        &[
            "--identity",
            "alice",
            "--dry-run",
            "site",
            "root",
            "get",
            "--domain",
            " Tenant.Example ",
        ],
        workspace.path(),
    ));
    assert_eq!(get["summary"], "Dry run: site root get planned");
    assert_eq!(get["data"]["plan"]["action"], "site.root.get");
    assert_eq!(get["data"]["plan"]["identity"], "alice");
    assert_eq!(get["data"]["plan"]["rpc_endpoint"], "/site/rpc");
    assert_eq!(get["data"]["plan"]["rpc_method"], "get_root");
    assert_eq!(
        get["data"]["plan"]["request"],
        json!({ "domain": "Tenant.Example" })
    );

    let set = success_json(&awiki_cmd(
        &[
            "--identity",
            "alice",
            "--dry-run",
            "site",
            "root",
            "set",
            "--domain",
            "tenant.example",
            "--markdown",
            "body",
        ],
        workspace.path(),
    ));
    assert_eq!(set["summary"], "Dry run: site root set planned");
    assert_eq!(set["data"]["plan"]["action"], "site.root.set");
    assert_eq!(set["data"]["plan"]["rpc_method"], "set_root");
    assert_eq!(
        set["data"]["plan"]["request"],
        json!({ "domain": "tenant.example", "body_bytes": 4 })
    );
}

#[test]
fn site_page_dry_run_plans_match_go_contracts() {
    let workspace = TempDir::new().expect("workspace");

    let list = success_json(&awiki_cmd(
        &[
            "--dry-run",
            "site",
            "page",
            "list",
            "--domain",
            " tenant.example ",
        ],
        workspace.path(),
    ));
    assert_eq!(list["summary"], "Dry run: site page list planned");
    assert_eq!(list["data"]["plan"]["action"], "site.page.list");
    assert_eq!(list["data"]["plan"]["rpc_method"], "list_pages");
    assert_eq!(list["data"]["plan"]["request"]["domain"], "tenant.example");

    let get = success_json(&awiki_cmd(
        &[
            "--dry-run",
            "site",
            "page",
            "get",
            "--domain",
            "tenant.example",
            "--slug",
            " hello ",
        ],
        workspace.path(),
    ));
    assert_eq!(get["summary"], "Dry run: site page get planned");
    assert_eq!(get["data"]["plan"]["action"], "site.page.get");
    assert_eq!(get["data"]["plan"]["rpc_method"], "get_page");
    assert_eq!(
        get["data"]["plan"]["request"],
        json!({ "domain": "tenant.example", "slug": "hello" })
    );

    let body_file = workspace.path().join("site-page.md");
    std::fs::write(&body_file, "# File Body\n").expect("write markdown file");
    let create = success_json(&awiki_cmd(
        &[
            "--identity",
            "alice",
            "--dry-run",
            "site",
            "page",
            "create",
            "--domain",
            "tenant.example",
            "--slug",
            "from-file",
            "--markdown-file",
            body_file.to_str().expect("path is utf8"),
        ],
        workspace.path(),
    ));
    assert_eq!(create["summary"], "Dry run: site page create planned");
    assert_eq!(create["data"]["plan"]["action"], "site.page.create");
    assert_eq!(create["data"]["plan"]["identity"], "alice");
    assert_eq!(create["data"]["plan"]["rpc_method"], "create_page");
    assert_eq!(
        create["data"]["plan"]["request"],
        json!({
            "domain": "tenant.example",
            "slug": "from-file",
            "body_bytes": "# File Body\n".len(),
        })
    );

    let update = success_json(&awiki_cmd(
        &[
            "--dry-run",
            "site",
            "page",
            "update",
            "--domain",
            "tenant.example",
            "--slug",
            "hello",
            "--markdown",
            "updated",
        ],
        workspace.path(),
    ));
    assert_eq!(update["summary"], "Dry run: site page update planned");
    assert_eq!(update["data"]["plan"]["action"], "site.page.update");
    assert_eq!(update["data"]["plan"]["rpc_method"], "update_page");
    assert_eq!(
        update["data"]["plan"]["request"],
        json!({ "domain": "tenant.example", "slug": "hello", "body_bytes": 7 })
    );

    let rename = success_json(&awiki_cmd(
        &[
            "--dry-run",
            "site",
            "page",
            "rename",
            "--domain",
            "tenant.example",
            "--slug",
            " old ",
            "--to",
            " new ",
        ],
        workspace.path(),
    ));
    assert_eq!(rename["summary"], "Dry run: site page rename planned");
    assert_eq!(rename["data"]["plan"]["action"], "site.page.rename");
    assert_eq!(
        rename["data"]["plan"]["request"],
        json!({ "domain": "tenant.example", "old_slug": "old", "new_slug": "new" })
    );

    let delete = success_json(&awiki_cmd(
        &[
            "--dry-run",
            "site",
            "page",
            "delete",
            "--domain",
            "tenant.example",
            "--slug",
            " old ",
        ],
        workspace.path(),
    ));
    assert_eq!(delete["summary"], "Dry run: site page delete planned");
    assert_eq!(delete["data"]["plan"]["action"], "site.page.delete");
    assert_eq!(
        delete["data"]["plan"]["request"],
        json!({ "domain": "tenant.example", "slug": "old" })
    );
}

#[test]
fn site_validation_errors_match_go_cli_boundary() {
    let workspace = TempDir::new().expect("workspace");

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
    assert_code(&missing_body, 2);
    let envelope = error_json(&missing_body);
    assert_eq!(envelope["error"]["code"], "invalid_argument");
    assert_contains(
        &envelope["error"]["message"],
        "provide either inline markdown or markdown file",
    );
    assert_eq!(
        envelope["error"]["hint"],
        "Provide --markdown or --markdown-file."
    );

    let body_file = workspace.path().join("conflict.md");
    std::fs::write(&body_file, "# Conflict\n").expect("write markdown file");
    let conflict = awiki_cmd(
        &[
            "--dry-run",
            "site",
            "page",
            "create",
            "--domain",
            "tenant.example",
            "--slug",
            "conflict",
            "--markdown",
            "inline",
            "--markdown-file",
            body_file.to_str().expect("path is utf8"),
        ],
        workspace.path(),
    );
    assert_code(&conflict, 2);
    let envelope = error_json(&conflict);
    assert_eq!(envelope["error"]["code"], "invalid_argument");
    assert_contains(
        &envelope["error"]["message"],
        "use either inline markdown or markdown file, not both",
    );
    assert_contains(&envelope["error"]["hint"], "Choose one content body source");

    let missing_required = awiki_cmd(
        &[
            "--dry-run",
            "site",
            "page",
            "get",
            "--domain",
            "tenant.example",
        ],
        workspace.path(),
    );
    assert_code(&missing_required, 1);
    let envelope = error_json(&missing_required);
    assert_eq!(envelope["error"]["code"], "internal_error");
    assert_contains(
        &envelope["error"]["message"],
        "required flag(s) \"slug\" not set",
    );
}

#[test]
fn site_non_dry_run_is_deferred_until_site_rpc_slice() {
    let workspace = TempDir::new().expect("workspace");

    let get = awiki_cmd(
        &["site", "root", "get", "--domain", "tenant.example"],
        workspace.path(),
    );
    assert_code(&get, 1);
    let envelope = error_json(&get);
    assert_eq!(envelope["error"]["code"], "not_implemented");
    assert_contains(
        &envelope["error"]["message"],
        "site root get requires non-dry-run implementation",
    );

    let create = awiki_cmd(
        &[
            "site",
            "page",
            "create",
            "--domain",
            "tenant.example",
            "--slug",
            "deferred",
            "--markdown",
            "body",
        ],
        workspace.path(),
    );
    assert_code(&create, 1);
    let envelope = error_json(&create);
    assert_eq!(envelope["error"]["code"], "not_implemented");
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
            "awiki-cli-rs2-site-test-{}-{nanos}",
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
