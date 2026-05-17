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
fn trace_timing_status_keeps_stdout_json_and_writes_trace_to_stderr() {
    let output = awiki_trace_cmd(&["status"]);
    assert_success(&output);
    let envelope = success_json_with_stderr(&output);

    assert_eq!(envelope["command"], "awiki-cli status");
    let trace = stderr_text(&output);
    assert_text_contains(&trace, "[awiki-cli 耗时追踪]");
    assert_text_contains(&trace, "命令: awiki-cli status");
    assert_text_contains(&trace, "阶段:");
    assert_text_contains(&trace, "解析配置");
}

#[test]
fn trace_timing_error_keeps_json_error_first_and_appends_trace() {
    let output = awiki_trace_cmd(&["docs", "missing-topic"]);
    assert_code(&output, 5);
    assert_stdout_empty(&output);
    let (envelope, trace) = error_json_prefix_with_trace(&output);

    assert_eq!(envelope["error"]["code"], "not_found");
    assert_contains(&envelope["error"]["message"], "Unknown docs topic");
    assert_text_contains(&trace, "[awiki-cli 耗时追踪]");
}

#[test]
fn trace_timing_completion_keeps_raw_stdout_without_trace_stderr() {
    let output = awiki_trace_cmd(&["completion", "bash"]);
    assert_success(&output);
    assert_stderr_empty(&output);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_text_contains(&stdout, "complete -F _awiki-cli awiki-cli");
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
fn schema_exposes_go_stub_command_families_and_stub_errors() {
    let group_code = schema_for(&["group", "code"]);
    assert_eq!(schema_command(&group_code)["name"], "group.code");
    assert_eq!(schema_command(&group_code)["implemented"], false);
    let group_get = schema_child(&group_code, "group.code.get");
    assert_go_stub_schema(group_get, "group.code.get", "phase5");
    assert_output_contains(group_get, "table");
    assert_eq!(schema_flag(group_get, "group")["required"], true);

    let heartbeat = schema_for(&["runtime", "heartbeat"]);
    assert_eq!(schema_command(&heartbeat)["name"], "runtime.heartbeat");
    assert_eq!(schema_command(&heartbeat)["implemented"], false);
    let heartbeat_install = schema_child(&heartbeat, "runtime.heartbeat.install");
    assert_go_stub_schema(heartbeat_install, "runtime.heartbeat.install", "phase7");
    assert_eq!(schema_flag(heartbeat_install, "every")["default"], "15m");

    let people = schema_for(&["people"]);
    assert_eq!(schema_command(&people)["name"], "people");
    assert_eq!(schema_command(&people)["implemented"], true);
    assert_go_stub_schema(
        schema_child(&people, "people.search"),
        "people.search",
        "phase8",
    );

    let contacts = schema_for(&["people", "contacts"]);
    assert_eq!(schema_command(&contacts)["implemented"], false);
    let contacts_save = schema_child(&contacts, "people.contacts.save");
    assert_go_stub_schema(contacts_save, "people.contacts.save", "phase8");
    assert_eq!(schema_flag(contacts_save, "did")["required"], true);

    let raw = schema_for(&["debug", "raw"]);
    assert_eq!(schema_command(&raw)["implemented"], false);
    assert_go_stub_schema(
        schema_child(&raw, "debug.raw.rpc"),
        "debug.raw.rpc",
        "phase7",
    );

    let logs = schema_for(&["debug", "logs"]);
    let logs_command = schema_command(&logs);
    assert_go_stub_schema(logs_command, "debug.logs", "phase7");
    assert_output_contains(logs_command, "ndjson");
    assert_eq!(schema_flag(logs_command, "follow")["type"], "bool");

    for (args, name, phase, command_path) in [
        (
            &["group", "code", "get", "--group", "did:group"][..],
            "group.code.get",
            "PHASE5",
            "awiki-cli group code get",
        ),
        (
            &["runtime", "heartbeat", "status"][..],
            "runtime.heartbeat.status",
            "PHASE7",
            "awiki-cli runtime heartbeat status",
        ),
        (
            &["people", "search", "alice"][..],
            "people.search",
            "PHASE8",
            "awiki-cli people search",
        ),
        (
            &[
                "people",
                "contacts",
                "save",
                "--did",
                "did:example:alice",
                "--handle",
                "alice",
            ][..],
            "people.contacts.save",
            "PHASE8",
            "awiki-cli people contacts save",
        ),
        (
            &["debug", "raw", "rpc"][..],
            "debug.raw.rpc",
            "PHASE7",
            "awiki-cli debug raw rpc",
        ),
        (
            &["debug", "logs", "--follow"][..],
            "debug.logs",
            "PHASE7",
            "awiki-cli debug logs",
        ),
    ] {
        let output = awiki_cmd(args);
        assert_code(&output, 1);
        assert_stdout_empty(&output);
        let envelope = error_json(&output);

        assert_eq!(envelope["error"]["code"], "internal_error");
        assert_contains(
            &envelope["error"]["message"],
            &format!("{command_path} is not implemented yet."),
        );
        assert_contains(&envelope["error"]["hint"], &format!("planned for {phase}"));
        assert_contains(
            &envelope["error"]["hint"],
            &format!("awiki-cli schema {name}"),
        );
    }
}

#[test]
fn schema_exposes_hidden_hermes_bridge_service_run_like_go_catalog() {
    let bridge_output = awiki_cmd(&["schema", "runtime", "host-notify", "hermes", "bridge"]);
    assert_success(&bridge_output);
    let bridge = success_json(&bridge_output);

    assert_eq!(
        bridge["summary"],
        "Static contract for runtime.host-notify.hermes.bridge"
    );
    assert_eq!(
        bridge["data"]["command"]["name"],
        "runtime.host-notify.hermes.bridge"
    );
    assert_eq!(bridge["data"]["command"]["use"], "bridge");
    assert_eq!(bridge["data"]["command"]["hidden"], true);
    assert_eq!(bridge["data"]["command"]["implemented"], false);
    let children = bridge["data"]["children"]
        .as_array()
        .expect("bridge children should be an array");
    assert!(
        children.iter().any(|command| {
            command["name"] == "runtime.host-notify.hermes.bridge.service-run"
                && command["hidden"] == true
                && command["implemented"] == true
                && command["handler"] == "runtime.host-notify.hermes.bridge.service-run"
        }),
        "schema bridge children should expose hidden service-run: {children:?}"
    );

    let service_output = awiki_cmd(&[
        "schema",
        "runtime",
        "host-notify",
        "hermes",
        "bridge",
        "service-run",
    ]);
    assert_success(&service_output);
    let service = success_json(&service_output);

    assert_eq!(
        service["summary"],
        "Static contract for runtime.host-notify.hermes.bridge.service-run"
    );
    assert_eq!(
        service["data"]["command"]["name"],
        "runtime.host-notify.hermes.bridge.service-run"
    );
    assert_eq!(service["data"]["command"]["use"], "service-run");
    assert_eq!(service["data"]["command"]["hidden"], true);
    assert_eq!(service["data"]["command"]["implemented"], true);
    assert_eq!(
        service["data"]["command"]["handler"],
        "runtime.host-notify.hermes.bridge.service-run"
    );
    assert!(service["data"]["children"].as_array().unwrap().is_empty());
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

fn awiki_trace_cmd(args: &[&str]) -> Output {
    let workspace = TempDir::new().expect("temp workspace");
    awiki_trace_cmd_with_workspace(args, workspace.path().to_str().unwrap())
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

fn awiki_trace_cmd_with_workspace(args: &[&str], workspace: &str) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_awiki-cli"));
    command
        .args(args)
        .env("AWIKI_CLI_WORKSPACE_HOME_DIR", workspace)
        .env("AWIKI_CLI_UPDATE_CACHE_ONLY", "1")
        .env("AWIKI_CLI_TRACE_TIMING", "1")
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
    success_json_with_stderr(output)
}

fn success_json_with_stderr(output: &Output) -> Value {
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

fn error_json_prefix_with_trace(output: &Output) -> (Value, String) {
    let mut stream = serde_json::Deserializer::from_slice(&output.stderr).into_iter::<Value>();
    let envelope = stream
        .next()
        .expect("stderr should start with a JSON error envelope")
        .expect("stderr should start with a valid JSON error envelope");
    assert_eq!(envelope["ok"], false, "error envelope should set ok=false");
    assert_eq!(envelope["error"]["retryable"], false);
    assert_success_meta(&envelope);
    let trace_offset = stream.byte_offset();
    let trace = String::from_utf8_lossy(&output.stderr[trace_offset..]).into_owned();
    assert!(
        !trace.trim().is_empty(),
        "stderr should contain trace text after JSON error envelope"
    );
    (envelope, trace)
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

fn schema_for(path: &[&str]) -> Value {
    let mut args = vec!["schema"];
    args.extend_from_slice(path);
    let output = awiki_cmd(&args);
    assert_success(&output);
    success_json(&output)
}

fn schema_command(schema: &Value) -> &Value {
    &schema["data"]["command"]
}

fn schema_child<'a>(schema: &'a Value, name: &str) -> &'a Value {
    schema["data"]["children"]
        .as_array()
        .unwrap_or_else(|| panic!("schema children should be an array: {schema:?}"))
        .iter()
        .find(|command| command["name"] == name)
        .unwrap_or_else(|| panic!("missing schema child {name:?}: {schema:?}"))
}

fn schema_flag<'a>(command: &'a Value, name: &str) -> &'a Value {
    command["flags"]
        .as_array()
        .unwrap_or_else(|| panic!("schema flags should be an array: {command:?}"))
        .iter()
        .find(|flag| flag["name"] == name)
        .unwrap_or_else(|| panic!("missing flag {name:?}: {command:?}"))
}

fn assert_go_stub_schema(command: &Value, name: &str, phase: &str) {
    assert_eq!(command["name"], name);
    assert_eq!(command["phase"], phase);
    assert_eq!(command["implemented"], false);
    assert_eq!(command["handler"], "stub");
}

fn assert_output_contains(command: &Value, output: &str) {
    let outputs = command["outputs"]
        .as_array()
        .unwrap_or_else(|| panic!("schema outputs should be an array: {command:?}"));
    assert!(
        outputs.iter().any(|value| value == output),
        "expected outputs {outputs:?} to contain {output:?}"
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

fn stderr_text(output: &Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(!stderr.is_empty(), "stderr should contain trace output");
    stderr
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

fn assert_text_contains(haystack: &str, needle: &str) {
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
