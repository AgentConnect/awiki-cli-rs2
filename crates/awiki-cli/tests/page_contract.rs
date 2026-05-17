use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn page_schema_exposes_go_command_surface() {
    let workspace = TempDir::new().expect("workspace");
    let output = awiki_cmd(&["schema", "page"], workspace.path());
    assert_success(&output);
    let envelope = success_json(&output);

    assert_eq!(envelope["data"]["command"]["name"], "page");
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

    let update = success_json(&awiki_cmd(&["schema", "page", "update"], workspace.path()));
    let flags: Vec<_> = update["data"]["command"]["flags"]
        .as_array()
        .expect("flags should be an array")
        .iter()
        .map(|flag| flag["name"].as_str().unwrap())
        .collect();
    for expected in ["slug", "title", "markdown", "markdown-file", "visibility"] {
        assert!(
            flags.contains(&expected),
            "page update flags should include {expected}: {flags:?}"
        );
    }
}

#[test]
fn page_dry_run_plans_match_go_contracts() {
    let workspace = TempDir::new().expect("workspace");

    let create = success_json(&awiki_cmd(
        &[
            "--identity",
            "alice",
            "--dry-run",
            "page",
            "create",
            "--slug",
            "hello",
            "--title",
            "Hello",
            "--markdown",
            "body",
            "--visibility",
            "draft",
        ],
        workspace.path(),
    ));
    assert_eq!(create["summary"], "Dry run: page create planned");
    assert_eq!(create["data"]["plan"]["action"], "page.create");
    assert_eq!(create["data"]["plan"]["identity"], "alice");
    assert_eq!(create["data"]["plan"]["rpc_endpoint"], "/content/rpc");
    assert_eq!(create["data"]["plan"]["rpc_method"], "create");
    assert_eq!(
        create["data"]["plan"]["request"],
        json!({
            "slug": "hello",
            "title": "Hello",
            "body_bytes": 4,
            "visibility": "draft",
        })
    );

    let body_file = workspace.path().join("page.md");
    std::fs::write(&body_file, "# File Body\n").expect("write markdown file");
    let file_create = success_json(&awiki_cmd(
        &[
            "--dry-run",
            "page",
            "create",
            "--slug",
            "from-file",
            "--title",
            "From File",
            "--markdown-file",
            body_file.to_str().expect("path is utf8"),
        ],
        workspace.path(),
    ));
    assert_eq!(
        file_create["data"]["plan"]["request"]["body_bytes"],
        "# File Body\n".len()
    );
    assert_eq!(
        file_create["data"]["plan"]["request"]["visibility"],
        "public"
    );

    let list = success_json(&awiki_cmd(&["--dry-run", "page", "list"], workspace.path()));
    assert_eq!(list["summary"], "Dry run: page list planned");
    assert_eq!(list["data"]["plan"]["action"], "page.list");
    assert_eq!(list["data"]["plan"]["rpc_method"], "list");

    let get = success_json(&awiki_cmd(
        &["--dry-run", "page", "get", "--slug", " hello "],
        workspace.path(),
    ));
    assert_eq!(get["summary"], "Dry run: page get planned");
    assert_eq!(get["data"]["plan"]["request"]["slug"], "hello");

    let update = success_json(&awiki_cmd(
        &[
            "--identity",
            "alice",
            "--dry-run",
            "page",
            "update",
            "--slug",
            "hello",
            "--title",
            "New Title",
            "--markdown",
            "updated",
            "--visibility",
            "public",
        ],
        workspace.path(),
    ));
    assert_eq!(update["summary"], "Dry run: page update planned");
    assert_eq!(update["data"]["plan"]["action"], "page.update");
    assert_eq!(
        update["data"]["plan"]["changed_fields"],
        json!(["title", "body", "visibility"])
    );
    assert_eq!(update["data"]["plan"]["request"]["body_bytes"], 7);

    let empty_update = success_json(&awiki_cmd(
        &["--dry-run", "page", "update", "--slug", "hello"],
        workspace.path(),
    ));
    assert_eq!(empty_update["data"]["plan"]["changed_fields"], json!([]));

    let rename = success_json(&awiki_cmd(
        &[
            "--dry-run",
            "page",
            "rename",
            "--slug",
            " old ",
            "--to",
            " new ",
        ],
        workspace.path(),
    ));
    assert_eq!(rename["summary"], "Dry run: page rename planned");
    assert_eq!(rename["data"]["plan"]["action"], "page.rename");
    assert_eq!(
        rename["data"]["plan"]["request"],
        json!({ "old_slug": "old", "new_slug": "new" })
    );

    let delete = success_json(&awiki_cmd(
        &["--dry-run", "page", "delete", "--slug", " old "],
        workspace.path(),
    ));
    assert_eq!(delete["summary"], "Dry run: page delete planned");
    assert_eq!(delete["data"]["plan"]["action"], "page.delete");
    assert_eq!(delete["data"]["plan"]["request"]["slug"], "old");
}

#[test]
fn page_dry_run_preserves_go_visibility_and_update_validation_boundaries() {
    let workspace = TempDir::new().expect("workspace");

    let create = success_json(&awiki_cmd(
        &[
            "--dry-run",
            "page",
            "create",
            "--slug",
            "raw-visibility",
            "--title",
            "Raw Visibility",
            "--markdown",
            "body",
            "--visibility",
            "private",
        ],
        workspace.path(),
    ));
    assert_eq!(create["data"]["plan"]["request"]["visibility"], "private");

    let update = success_json(&awiki_cmd(
        &[
            "--dry-run",
            "page",
            "update",
            "--slug",
            "raw-visibility",
            "--visibility",
            "private",
        ],
        workspace.path(),
    ));
    assert_eq!(
        update["data"]["plan"]["changed_fields"],
        json!(["visibility"])
    );
    assert_eq!(update["data"]["plan"]["request"]["visibility"], "private");
}

#[test]
fn page_validation_errors_match_go_cli_boundary() {
    let workspace = TempDir::new().expect("workspace");

    let missing_slug = awiki_cmd(
        &[
            "--dry-run",
            "page",
            "create",
            "--title",
            "Missing Slug",
            "--markdown",
            "body",
        ],
        workspace.path(),
    );
    assert_code(&missing_slug, 2);
    let envelope = error_json(&missing_slug);
    assert_contains(&envelope["error"]["message"], "slug is required");

    let missing_title = awiki_cmd(
        &[
            "--dry-run",
            "page",
            "create",
            "--slug",
            "missing-title",
            "--markdown",
            "body",
        ],
        workspace.path(),
    );
    assert_code(&missing_title, 2);
    let envelope = error_json(&missing_title);
    assert_contains(&envelope["error"]["message"], "title is required");

    let body_file = workspace.path().join("conflict.md");
    std::fs::write(&body_file, "# Conflict\n").expect("write markdown file");
    let conflict = awiki_cmd(
        &[
            "--dry-run",
            "page",
            "create",
            "--slug",
            "conflict",
            "--title",
            "Conflict",
            "--markdown",
            "inline",
            "--markdown-file",
            body_file.to_str().expect("path is utf8"),
        ],
        workspace.path(),
    );
    assert_code(&conflict, 2);
    let envelope = error_json(&conflict);
    assert_contains(
        &envelope["error"]["message"],
        "use either inline markdown or markdown file, not both",
    );
    assert_contains(&envelope["error"]["hint"], "Choose one content body source");
}

#[test]
fn page_required_flag_errors_match_go_cobra_boundary() {
    let workspace = TempDir::new().expect("workspace");

    let get = awiki_cmd(&["--dry-run", "page", "get"], workspace.path());
    assert_code(&get, 1);
    let envelope = error_json(&get);
    assert_eq!(envelope["error"]["code"], "internal_error");
    assert_contains(
        &envelope["error"]["message"],
        "required flag(s) \"slug\" not set",
    );

    let update = awiki_cmd(
        &["--dry-run", "page", "update", "--title", "New Title"],
        workspace.path(),
    );
    assert_code(&update, 1);
    let envelope = error_json(&update);
    assert_eq!(envelope["error"]["code"], "internal_error");
    assert_contains(
        &envelope["error"]["message"],
        "required flag(s) \"slug\" not set",
    );

    let rename_missing_to = awiki_cmd(
        &["--dry-run", "page", "rename", "--slug", "old"],
        workspace.path(),
    );
    assert_code(&rename_missing_to, 1);
    let envelope = error_json(&rename_missing_to);
    assert_eq!(envelope["error"]["code"], "internal_error");
    assert_contains(
        &envelope["error"]["message"],
        "required flag(s) \"to\" not set",
    );

    let rename_missing_both = awiki_cmd(&["--dry-run", "page", "rename"], workspace.path());
    assert_code(&rename_missing_both, 1);
    let envelope = error_json(&rename_missing_both);
    assert_eq!(envelope["error"]["code"], "internal_error");
    assert_contains(
        &envelope["error"]["message"],
        "required flag(s) \"slug\", \"to\" not set",
    );

    let delete = awiki_cmd(&["--dry-run", "page", "delete"], workspace.path());
    assert_code(&delete, 1);
    let envelope = error_json(&delete);
    assert_eq!(envelope["error"]["code"], "internal_error");
    assert_contains(
        &envelope["error"]["message"],
        "required flag(s) \"slug\" not set",
    );

    let blank_slug = success_json(&awiki_cmd(
        &["--dry-run", "page", "get", "--slug", ""],
        workspace.path(),
    ));
    assert_eq!(blank_slug["summary"], "Dry run: page get planned");
    assert_eq!(blank_slug["data"]["plan"]["request"]["slug"], "");
}

#[test]
fn page_non_dry_run_requires_active_identity_for_content_rpc_slice() {
    let workspace = TempDir::new().expect("workspace");

    let list = awiki_cmd(&["page", "list"], workspace.path());
    assert_code(&list, 5);
    let envelope = error_json(&list);
    assert_eq!(envelope["error"]["code"], "not_found");
    assert_contains(
        &envelope["error"]["message"],
        "identity not found: no active identity is configured",
    );

    let create = awiki_cmd(
        &[
            "page",
            "create",
            "--slug",
            "deferred",
            "--title",
            "Deferred",
            "--markdown",
            "body",
        ],
        workspace.path(),
    );
    assert_code(&create, 5);
    let envelope = error_json(&create);
    assert_eq!(envelope["error"]["code"], "not_found");
}

#[test]
fn page_list_migrates_legacy_config_json_before_identity_boundary_like_go() {
    let workspace = TempDir::new().expect("workspace");
    let legacy_config = workspace.path().join("config.json");
    std::fs::write(
        &legacy_config,
        r#"{"schema_version":1,"services":{"service_base_url":"https://legacy.example","did_domain":"legacy.example"},"runtime":{"mode":"http"}}"#,
    )
    .expect("write legacy config");

    let list = awiki_cmd(&["page", "list"], workspace.path());
    assert_code(&list, 5);
    let envelope = error_json(&list);
    assert_eq!(envelope["error"]["code"], "not_found");
    assert_contains(
        &envelope["error"]["message"],
        "identity not found: no active identity is configured",
    );

    assert!(
        !legacy_config.exists(),
        "legacy config.json should be removed after workspace upgrade"
    );
    let config_yaml = workspace.path().join("config.yaml");
    let config_text = std::fs::read_to_string(&config_yaml).expect("read migrated config");
    assert!(
        config_text.contains("  mode: http\n"),
        "migrated config should keep runtime mode, got {config_text:?}"
    );
    assert!(
        config_text.contains("  service_base_url: https://legacy.example\n"),
        "migrated config should keep service URL, got {config_text:?}"
    );
    assert!(
        config_text.contains("  did_domain: legacy.example\n"),
        "migrated config should keep DID domain, got {config_text:?}"
    );

    let meta_path = workspace.path().join("upgrade").join("meta.json");
    let meta: Value =
        serde_json::from_slice(&std::fs::read(&meta_path).expect("read upgrade meta"))
            .expect("upgrade meta JSON");
    assert_eq!(meta["workspace_schema_version"], 3);
    assert_non_empty_string(&meta["last_upgrade_id"], "last_upgrade_id");
    assert_non_empty_string(&meta["last_backup_dir"], "last_backup_dir");
    let backup_dir = PathBuf::from(meta["last_backup_dir"].as_str().unwrap());
    assert_eq!(
        backup_dir.parent(),
        Some(workspace.path().join("upgrade").join("backups").as_path())
    );
    assert!(backup_dir.join("config.json.bak").is_file());
    assert!(
        !workspace
            .path()
            .join("upgrade")
            .join("upgrade_journal.json")
            .exists(),
        "journal should be cleared after successful upgrade"
    );
    assert!(
        !workspace.path().join("data").join("awiki-cli.db").exists(),
        "page list should fail at identity boundary before creating SQLite state"
    );
    assert!(
        !workspace
            .path()
            .join("runtime")
            .join("message-daemon.sock")
            .exists(),
        "page list must not create runtime socket artifacts"
    );
    assert!(
        !workspace
            .path()
            .join("runtime")
            .join("listener.pid")
            .exists(),
        "page list must not create listener pid artifacts"
    );
}

fn awiki_cmd(args: &[&str], workspace: &Path) -> Output {
    let home = workspace.join("home");
    std::fs::create_dir_all(&home).expect("create isolated HOME");
    let mut command = Command::new(env!("CARGO_BIN_EXE_awiki-cli"));
    command
        .args(args)
        .env("HOME", &home)
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

fn assert_non_empty_string(value: &Value, field: &str) {
    assert!(
        value.as_str().is_some_and(|text| !text.trim().is_empty()),
        "{field} should be a non-empty string: {value:?}"
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
            "awiki-cli-rs2-page-test-{}-{nanos}",
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
