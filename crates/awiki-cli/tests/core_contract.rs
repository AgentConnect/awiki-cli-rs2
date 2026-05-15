use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn status_reports_phase_version_paths_state_and_config() {
    let output = awiki_cmd(&["status"]);
    assert_success(&output);
    let envelope = success_json(&output);

    assert_eq!(envelope["command"], "awiki-cli status");
    assert_non_empty_string(&envelope["summary"], "summary");
    assert_eq!(envelope["data"]["cli"]["phase"], "phase1-shell");
    assert_has_keys(&envelope["data"]["cli"]["version"], &build_info_keys());
    assert_has_keys(
        &envelope["data"]["paths"],
        &[
            "workspace_home_dir",
            "root_dir",
            "config_dir",
            "data_dir",
            "state_dir",
            "cache_dir",
            "logs_dir",
            "config_file",
            "identity_dir",
            "database_file",
        ],
    );
    assert_has_keys(
        &envelope["data"]["state"],
        &["active_identity", "identity_count", "legacy_scan"],
    );
    assert_has_keys(
        &envelope["data"]["config"],
        &["config_exists", "config_error", "env_hits", "sources"],
    );
}

#[test]
fn version_reports_current_build_info() {
    let output = awiki_cmd(&["version"]);
    assert_success(&output);
    let envelope = success_json(&output);

    assert_eq!(envelope["command"], "awiki-cli version");
    assert_eq!(envelope["summary"], "Build information");
    assert_has_keys(&envelope["data"], &build_info_keys());
    assert_eq!(envelope["data"]["version"], "dev");
    assert_eq!(envelope["data"]["commit"], "unknown");
    assert_eq!(envelope["data"]["build_date"], "unknown");
    assert_eq!(envelope["data"]["cgo_enabled"], "unknown");
    assert_eq!(envelope["data"]["compiler"], "rustc");
    assert_non_empty_string(&envelope["data"]["go_version"], "go_version");
    assert_non_empty_string(&envelope["data"]["goos"], "goos");
    assert_non_empty_string(&envelope["data"]["goarch"], "goarch");
    assert_ne!(envelope["data"]["goos"], "macos");
    assert_ne!(envelope["data"]["goarch"], "x86_64");
}

#[test]
fn config_show_reports_resolved_configuration_snapshot() {
    let output = awiki_cmd(&["config", "show"]);
    assert_success(&output);
    let envelope = success_json(&output);

    assert_eq!(envelope["command"], "awiki-cli config show");
    assert_eq!(envelope["summary"], "Resolved configuration");
    assert_has_keys(
        &envelope["data"],
        &[
            "paths",
            "config_schema_version",
            "runtime_mode",
            "output_format",
            "service_base_url",
            "did_domain",
            "config_exists",
            "config_error",
            "env_hits",
            "sources",
            "identity_store",
            "database",
            "workspace_upgrade",
        ],
    );
    assert_eq!(envelope["data"]["output_format"], "json");
    assert_eq!(envelope["data"]["did_domain"], "awiki.ai");
    assert_eq!(envelope["data"]["config_exists"], false);
    assert_has_keys(
        &envelope["data"]["identity_store"],
        &[
            "identity_dir",
            "index_file",
            "default_identity",
            "legacy_scan",
        ],
    );
    assert_has_keys(&envelope["data"]["database"], &["database_file", "exists"]);
    assert_has_keys(
        &envelope["data"]["workspace_upgrade"],
        &["paths", "detection"],
    );
    assert_eq!(
        envelope["data"]["workspace_upgrade"]["detection"]["latest_version"],
        3
    );
    assert_ne!(
        envelope["data"]["workspace_upgrade"]["detection"]["current_version"],
        "dev"
    );
}

#[test]
fn config_set_did_domain_matches_go_contract() {
    let workspace = TempDir::new().expect("temp workspace");

    let dry_run = awiki_cmd_with_workspace(
        &[
            "--dry-run",
            "config",
            "set",
            "--did-domain",
            "Tenant.Example.",
        ],
        workspace.path().to_str().unwrap(),
    );
    assert_success(&dry_run);
    let envelope = success_json(&dry_run);
    assert_eq!(envelope["command"], "awiki-cli config set");
    assert_eq!(envelope["summary"], "Dry run: DID domain update planned");
    assert_eq!(envelope["meta"]["dry_run"], true);
    assert_eq!(envelope["data"]["plan"]["action"], "config_set_did_domain");
    assert_eq!(envelope["data"]["plan"]["did_domain"], "tenant.example");
    assert_eq!(
        envelope["data"]["plan"]["config_file"],
        workspace
            .path()
            .join("config.yaml")
            .to_string_lossy()
            .as_ref()
    );
    assert!(
        !workspace.path().join("config.yaml").exists(),
        "dry-run config set must not write config.yaml"
    );

    let update = awiki_cmd_with_workspace(
        &["config", "set", "--did-domain", " Tenant.Example. "],
        workspace.path().to_str().unwrap(),
    );
    assert_success(&update);
    let envelope = success_json(&update);
    assert_eq!(envelope["summary"], "DID domain updated");
    assert_eq!(envelope["data"]["did_domain"], "tenant.example");
    assert_eq!(
        envelope["data"]["config_file"],
        workspace
            .path()
            .join("config.yaml")
            .to_string_lossy()
            .as_ref()
    );
    let config_text = std::fs::read_to_string(workspace.path().join("config.yaml"))
        .expect("config.yaml should be written");
    assert!(
        config_text.contains("  did_domain: tenant.example"),
        "config.yaml should contain normalized did_domain, got {config_text:?}"
    );
    assert!(
        !workspace
            .path()
            .join("runtime")
            .join("message-daemon.sock")
            .exists(),
        "config set must not create runtime socket artifacts"
    );
    assert!(
        !workspace
            .path()
            .join("runtime")
            .join("listener.service.pid")
            .exists(),
        "config set must not create listener pid artifacts"
    );
}

#[test]
fn config_set_validates_did_domain_like_go() {
    let workspace = TempDir::new().expect("temp workspace");

    let missing = awiki_cmd_with_workspace(&["config", "set"], workspace.path().to_str().unwrap());
    assert_code(&missing, 2);
    let envelope = error_json(&missing);
    assert_eq!(envelope["error"]["code"], "invalid_argument");
    assert_contains(
        &envelope["error"]["message"],
        "config set requires --did-domain",
    );

    let url_like = awiki_cmd_with_workspace(
        &["config", "set", "--did-domain", "https://tenant.example"],
        workspace.path().to_str().unwrap(),
    );
    assert_code(&url_like, 2);
    let envelope = error_json(&url_like);
    assert_eq!(envelope["error"]["code"], "invalid_argument");
    assert_contains(&envelope["error"]["message"], "bare domain");
    assert_contains(&envelope["error"]["hint"], "tenant.example");

    let with_path = awiki_cmd_with_workspace(
        &["config", "set", "--did-domain", "tenant.example/path"],
        workspace.path().to_str().unwrap(),
    );
    assert_code(&with_path, 2);
    let envelope = error_json(&with_path);
    assert_contains(&envelope["error"]["message"], "path, query, or fragment");

    let go_accepted_host_shapes = awiki_cmd_with_workspace(
        &[
            "--dry-run",
            "config",
            "set",
            "--did-domain",
            "Tenant..Example",
        ],
        workspace.path().to_str().unwrap(),
    );
    assert_success(&go_accepted_host_shapes);
    let envelope = success_json(&go_accepted_host_shapes);
    assert_eq!(envelope["data"]["plan"]["did_domain"], "tenant..example");
}

#[test]
fn docs_list_and_topic_lookup_preserve_go_topic_contracts() {
    let list_output = awiki_cmd(&["docs"]);
    assert_success(&list_output);
    let list = success_json(&list_output);

    assert_eq!(list["command"], "awiki-cli docs");
    assert_eq!(list["summary"], "Available documentation topics");
    let topics = list["data"]["topics"]
        .as_array()
        .expect("data.topics should be an array");
    assert!(
        topics
            .iter()
            .any(|topic| topic["name"] == "overview" && topic["references"].is_array()),
        "docs topic list should include overview with references: {topics:?}"
    );

    let topic_output = awiki_cmd(&["docs", "  OvErViEw  "]);
    assert_success(&topic_output);
    let topic = success_json(&topic_output);

    assert_eq!(topic["command"], "awiki-cli docs");
    assert_eq!(topic["summary"], "Documentation topic overview");
    assert_eq!(topic["data"]["topic"]["name"], "overview");
    assert_non_empty_array(
        &topic["data"]["topic"]["references"],
        "data.topic.references",
    );
}

#[test]
fn schema_lists_contracts_and_supports_space_joined_targets() {
    let list_output = awiki_cmd(&["schema"]);
    assert_success(&list_output);
    let list = success_json(&list_output);

    assert_eq!(list["command"], "awiki-cli schema");
    assert_eq!(list["summary"], "Static command contract");
    assert_eq!(list["data"]["phase"], "phase1-shell");
    assert!(
        list["data"]["commands"]
            .as_array()
            .expect("data.commands should be an array")
            .iter()
            .any(|command| command["name"] == "config.show"
                && command["implemented"] == true
                && command["outputs"].as_array().is_some()),
        "schema should include implemented config.show contract: {list:?}"
    );

    let command_output = awiki_cmd(&["schema", "config", "show"]);
    assert_success(&command_output);
    let command = success_json(&command_output);

    assert_eq!(command["command"], "awiki-cli schema");
    assert_eq!(command["summary"], "Static contract for config.show");
    assert_eq!(command["data"]["command"]["name"], "config.show");
    assert_eq!(command["data"]["command"]["handler"], "config.show");
    assert_eq!(command["data"]["command"]["implemented"], true);
    assert!(command["data"]["children"].is_array());
}

#[test]
fn validation_errors_use_json_error_envelopes_on_stderr() {
    let output = awiki_cmd(&["docs", "overview", "extra"]);
    assert_code(&output, 2);
    let envelope = error_json(&output);

    assert_stdout_empty(&output);
    assert_eq!(envelope["error"]["code"], "invalid_argument");
    assert_contains(
        &envelope["error"]["message"],
        "docs accepts at most one topic",
    );
    assert_contains(&envelope["error"]["hint"], "awiki-cli docs");
    assert_eq!(envelope["meta"]["format"], "json");

    let output = awiki_cmd(&["docs", "missing-topic"]);
    assert_code(&output, 5);
    let envelope = error_json(&output);

    assert_stdout_empty(&output);
    assert_eq!(envelope["error"]["code"], "not_found");
    assert_contains(&envelope["error"]["message"], "Unknown docs topic");
    assert_contains(&envelope["error"]["hint"], "awiki-cli docs");
    assert_eq!(envelope["meta"]["format"], "json");
}

#[test]
fn dry_run_init_returns_plan_without_writing_workspace() {
    let workspace = TempDir::new().expect("temp workspace");
    let output =
        awiki_cmd_with_workspace(&["--dry-run", "init"], workspace.path().to_str().unwrap());
    assert_success(&output);
    let envelope = success_json(&output);

    assert_eq!(envelope["command"], "awiki-cli init");
    assert_eq!(
        envelope["summary"],
        "Dry run: workspace initialization planned"
    );
    assert_eq!(envelope["meta"]["dry_run"], true);
    assert_eq!(envelope["data"]["plan"]["action"], "init_workspace");
    assert_eq!(
        envelope["data"]["plan"]["root_dir"],
        workspace.path().to_string_lossy().as_ref()
    );
    assert_eq!(envelope["data"]["plan"]["config_exists"], false);
    assert!(
        !workspace.path().join("config.yaml").exists(),
        "dry-run init must not write config.yaml"
    );
}

#[test]
fn init_creates_real_sqlite_schema() {
    let workspace = TempDir::new().expect("temp workspace");
    let init_output = awiki_cmd_with_workspace(&["init"], workspace.path().to_str().unwrap());
    assert_success(&init_output);

    let query_output = awiki_cmd_with_workspace(
        &[
            "debug",
            "db",
            "query",
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'messages'",
        ],
        workspace.path().to_str().unwrap(),
    );
    assert_success(&query_output);
    let envelope = success_json(&query_output);
    assert_eq!(envelope["command"], "awiki-cli debug db query");
    assert_eq!(envelope["data"]["rows"][0]["name"], "messages");
}

#[test]
fn debug_db_query_rejects_unsafe_sql_and_supports_table_output() {
    let workspace = TempDir::new().expect("temp workspace");

    let empty = awiki_cmd_with_workspace(
        &["debug", "db", "query", "   "],
        workspace.path().to_str().unwrap(),
    );
    assert_code(&empty, 2);
    let envelope = error_json(&empty);
    assert_eq!(envelope["error"]["code"], "invalid_argument");
    assert_contains(&envelope["error"]["message"], "empty statement");

    let multiple = awiki_cmd_with_workspace(
        &["debug", "db", "query", "SELECT 1; SELECT 2"],
        workspace.path().to_str().unwrap(),
    );
    assert_code(&multiple, 2);
    let envelope = error_json(&multiple);
    assert_contains(
        &envelope["error"]["message"],
        "multiple statements are not allowed",
    );

    let delete = awiki_cmd_with_workspace(
        &["debug", "db", "query", "DELETE FROM messages"],
        workspace.path().to_str().unwrap(),
    );
    assert_code(&delete, 2);
    let envelope = error_json(&delete);
    assert_contains(
        &envelope["error"]["message"],
        "DELETE without WHERE clause is not allowed",
    );

    let table = awiki_cmd_with_workspace(
        &[
            "debug",
            "db",
            "query",
            "SELECT 1 AS value",
            "--format",
            "table",
        ],
        workspace.path().to_str().unwrap(),
    );
    assert_success(&table);
    let stdout = String::from_utf8_lossy(&table.stdout);
    assert!(stdout.contains("value"), "table stdout: {stdout}");
}

#[test]
fn debug_db_import_v1_supports_dry_run_and_missing_path_errors() {
    let workspace = TempDir::new().expect("temp workspace");

    let dry_run = awiki_cmd_with_workspace(
        &["debug", "db", "import-v1", "--dry-run"],
        workspace.path().to_str().unwrap(),
    );
    assert_success(&dry_run);
    let envelope = success_json(&dry_run);
    assert_eq!(envelope["meta"]["dry_run"], true);
    assert_eq!(envelope["data"]["plan"]["action"], "import_v1_sqlite");

    let missing = workspace.path().join("missing-legacy-root");
    let missing = awiki_cmd_with_workspace(
        &[
            "debug",
            "db",
            "import-v1",
            "--path",
            missing.to_str().unwrap(),
        ],
        workspace.path().to_str().unwrap(),
    );
    assert_code(&missing, 5);
    let envelope = error_json(&missing);
    assert_eq!(envelope["error"]["code"], "not_found");
    assert_contains(
        &envelope["error"]["message"],
        "legacy sqlite database not found",
    );
}

fn awiki_cmd(args: &[&str]) -> Output {
    let workspace = TempDir::new().expect("temp workspace");
    awiki_cmd_with_workspace(args, workspace.path().to_str().unwrap())
}

fn awiki_cmd_with_workspace(args: &[&str], workspace: &str) -> Output {
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
    assert_stderr_empty(output);
    let envelope: Value =
        serde_json::from_slice(&output.stdout).expect("stdout should be a JSON success envelope");
    assert_eq!(envelope["ok"], true, "success envelope should set ok=true");
    assert_success_meta(&envelope);
    envelope
}

fn error_json(output: &Output) -> Value {
    let envelope: Value =
        serde_json::from_slice(&output.stderr).expect("stderr should be a JSON error envelope");
    assert_eq!(envelope["ok"], false, "error envelope should set ok=false");
    assert_eq!(envelope["error"]["retryable"], false);
    assert_success_meta(&envelope);
    envelope
}

fn assert_success_meta(envelope: &Value) {
    assert_has_keys(&envelope["meta"], &["version", "dry_run", "format"]);
    assert_eq!(envelope["meta"]["format"], "json");
}

fn assert_has_keys(value: &Value, keys: &[&str]) {
    let object = value
        .as_object()
        .unwrap_or_else(|| panic!("expected object with keys {keys:?}, got {value:?}"));
    for key in keys {
        assert!(
            object.contains_key(*key),
            "missing key {key:?} in {object:?}"
        );
    }
}

fn assert_non_empty_string(value: &Value, label: &str) {
    assert!(
        value.as_str().is_some_and(|value| !value.is_empty()),
        "{label} should be a non-empty string, got {value:?}"
    );
}

fn assert_non_empty_array(value: &Value, label: &str) {
    assert!(
        value.as_array().is_some_and(|value| !value.is_empty()),
        "{label} should be a non-empty array, got {value:?}"
    );
}

fn assert_stdout_empty(output: &Output) {
    assert!(
        output.stdout.is_empty(),
        "stdout should be empty, got {}",
        String::from_utf8_lossy(&output.stdout)
    );
}

fn assert_stderr_empty(output: &Output) {
    assert!(
        output.stderr.is_empty(),
        "stderr should be empty, got {}",
        String::from_utf8_lossy(&output.stderr)
    );
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

fn build_info_keys() -> [&'static str; 8] {
    [
        "version",
        "commit",
        "build_date",
        "go_version",
        "goos",
        "goarch",
        "compiler",
        "cgo_enabled",
    ]
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
        let path =
            std::env::temp_dir().join(format!("awiki-cli-rs2-test-{}-{nanos}", std::process::id()));
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
