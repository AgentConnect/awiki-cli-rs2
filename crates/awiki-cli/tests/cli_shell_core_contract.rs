use rusqlite::Connection;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
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
    assert_eq!(
        envelope["data"]["version"],
        option_env!("AWIKI_CLI_VERSION").unwrap_or("dev")
    );
    assert_eq!(
        envelope["data"]["commit"],
        option_env!("AWIKI_CLI_COMMIT").unwrap_or("unknown")
    );
    assert_eq!(
        envelope["data"]["build_date"],
        option_env!("AWIKI_CLI_BUILD_DATE").unwrap_or("unknown")
    );
    assert_eq!(
        envelope["data"]["cgo_enabled"],
        option_env!("AWIKI_CLI_CGO_ENABLED").unwrap_or("unknown")
    );
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
    assert_eq!(envelope["data"]["config_exists"], true);
    assert_eq!(envelope["data"]["tenant"]["active"], "default");
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
        4
    );
    assert_ne!(
        envelope["data"]["workspace_upgrade"]["detection"]["current_version"],
        "dev"
    );
}

#[test]
fn tenant_commands_create_switch_and_validate_new_tenant_boundary() {
    let workspace = TempDir::new().expect("temp workspace");

    let dry_run = awiki_cmd_with_workspace(
        &[
            "--dry-run",
            "tenant",
            "create",
            "acme",
            "--backend-base-url",
            "https://api.acme.test/",
            "--did-host",
            "Acme.Test.",
        ],
        workspace.path().to_str().unwrap(),
    );
    assert_success(&dry_run);
    let envelope = success_json(&dry_run);
    assert_eq!(envelope["command"], "awiki-cli tenant create");
    assert_eq!(envelope["summary"], "Dry run: tenant creation planned");
    assert_eq!(envelope["meta"]["dry_run"], true);
    assert_eq!(envelope["data"]["plan"]["action"], "tenant_create");
    assert_eq!(
        envelope["data"]["plan"]["tenant"]["profile"]["backend_base_url"],
        "https://api.acme.test"
    );
    assert_eq!(
        envelope["data"]["plan"]["tenant"]["profile"]["did_host"],
        "acme.test"
    );
    assert!(
        !workspace.path().join("tenants").join("acme").exists(),
        "dry-run tenant create must not write tenant state"
    );

    let create = awiki_cmd_with_workspace(
        &[
            "tenant",
            "create",
            "acme",
            "--backend-base-url",
            "https://api.acme.test/",
            "--did-host",
            "Acme.Test.",
        ],
        workspace.path().to_str().unwrap(),
    );
    assert_success(&create);
    let envelope = success_json(&create);
    assert_eq!(envelope["summary"], "Tenant created");
    assert_eq!(envelope["data"]["tenant"]["active"], "acme");
    let acme_dir = workspace.path().join("tenants").join("acme");
    assert!(acme_dir.join("config.yaml").is_file());

    let switch = awiki_cmd_with_workspace(
        &["tenant", "use", "acme"],
        workspace.path().to_str().unwrap(),
    );
    assert_success(&switch);
    let envelope = success_json(&switch);
    assert_eq!(envelope["summary"], "Tenant switched");
    assert_eq!(envelope["data"]["tenant"]["active"], "acme");

    let show = awiki_cmd_with_workspace(&["config", "show"], workspace.path().to_str().unwrap());
    assert_success(&show);
    let envelope = success_json(&show);
    assert_eq!(
        envelope["data"]["service_base_url"],
        "https://api.acme.test"
    );
    assert_eq!(envelope["data"]["did_domain"], "acme.test");
    assert_eq!(
        envelope["data"]["paths"]["workspace_home_dir"],
        acme_dir.to_string_lossy().as_ref()
    );

    let missing_name = awiki_cmd_with_workspace(
        &[
            "tenant",
            "create",
            "--backend-base-url",
            "https://api.example.test",
            "--did-host",
            "example.test",
        ],
        workspace.path().to_str().unwrap(),
    );
    assert_code(&missing_name, 2);
    let envelope = error_json(&missing_name);
    assert_eq!(envelope["error"]["code"], "invalid_argument");
    assert_contains(
        &envelope["error"]["message"],
        "tenant create requires <name>",
    );

    let bad_host = awiki_cmd_with_workspace(
        &[
            "tenant",
            "create",
            "bad",
            "--backend-base-url",
            "https://api.example.test",
            "--did-host",
            "https://tenant.example",
        ],
        workspace.path().to_str().unwrap(),
    );
    assert_code(&bad_host, 1);
    let envelope = error_json(&bad_host);
    assert_contains(
        &envelope["error"]["message"],
        "did_host must be a bare domain",
    );
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

    let output_topic = success_json(&awiki_cmd(&["docs", "output"]));
    let references = output_topic["data"]["topic"]["references"]
        .as_array()
        .expect("output topic references should be an array");
    assert!(
        references
            .iter()
            .any(|reference| reference == "docs/architecture/output-format.md"),
        "output topic should preserve Go's canonical output-format reference: {references:?}"
    );
    assert_docs_references_exist(references);

    let tenant_topic = success_json(&awiki_cmd(&["docs", "tenant"]));
    assert_eq!(tenant_topic["data"]["topic"]["name"], "tenant");
    assert!(
        tenant_topic["data"]["topic"]["references"]
            .as_array()
            .expect("tenant topic references")
            .iter()
            .any(|reference| reference == "docs/installation.md"),
        "tenant docs topic should point to installation docs: {tenant_topic:?}"
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

    let tenant_output = awiki_cmd(&["schema", "tenant"]);
    assert_success(&tenant_output);
    let tenant = success_json(&tenant_output);
    assert_eq!(schema_command(&tenant)["name"], "tenant");
    assert_text_contains(
        schema_command(&tenant)["long"].as_str().unwrap_or_default(),
        "backend_base_url + did_host",
    );
    assert!(schema_child(&tenant, "tenant.create").is_object());

    let tenant_create_help = awiki_cmd(&["tenant", "create", "--help"]);
    assert_success(&tenant_create_help);
    assert_stderr_empty(&tenant_create_help);
    let tenant_create = stdout_text(&tenant_create_help);
    assert_text_contains(&tenant_create, "awiki-cli tenant create <name>");
    assert_text_contains(&tenant_create, "does not make the new tenant active");
    assert_text_contains(&tenant_create, "--backend-base-url <string>");
}

#[test]
fn static_schema_and_docs_do_not_require_valid_workspace_config() {
    let workspace = TempDir::new().expect("temp workspace");
    let config = workspace
        .path()
        .join("tenants")
        .join("default")
        .join("config.yaml");
    std::fs::create_dir_all(config.parent().expect("config parent")).expect("tenant dir");
    std::fs::write(
        &config,
        "schema_version: 1\nservices:\n  service_base_url: https://legacy.example\n  did_domain: legacy.example\n",
    )
    .expect("write deprecated config");

    let schema =
        awiki_cmd_with_workspace(&["schema", "tenant"], workspace.path().to_str().unwrap());
    assert_success(&schema);
    let schema = success_json(&schema);
    assert_eq!(schema["data"]["command"]["name"], "tenant");

    let help = awiki_cmd_with_workspace(&["tenant", "--help"], workspace.path().to_str().unwrap());
    assert_success(&help);
    assert_stderr_empty(&help);
    let help = stdout_text(&help);
    assert_text_contains(&help, "awiki-cli tenant");
    assert_text_contains(&help, "Commands:");

    let docs = awiki_cmd_with_workspace(&["docs", "tenant"], workspace.path().to_str().unwrap());
    assert_success(&docs);
    let docs = success_json(&docs);
    assert_eq!(docs["data"]["topic"]["name"], "tenant");
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
    assert!(
        people["data"]["children"]
            .as_array()
            .expect("people children should be an array")
            .iter()
            .all(|command| command["name"] != "people.search"),
        "unsupported people.search should stay off the default schema surface: {people:?}"
    );
    let people_search = schema_all_command("people.search");
    assert_go_stub_schema(&people_search, "people.search", "phase8");

    let contacts = schema_for(&["people", "contacts"]);
    assert_eq!(schema_command(&contacts)["implemented"], true);
    let contacts_save = schema_child(&contacts, "people.contacts.save");
    assert_eq!(contacts_save["implemented"], true);
    assert_eq!(contacts_save["handler"], "people.contacts.save");
    assert_eq!(schema_flag(contacts_save, "did")["required"], true);
    assert_eq!(
        schema_flag(contacts_save, "display-name")["usage"],
        "Contact display name"
    );
    let contacts_save_all = schema_all_command("people.contacts.save");
    assert_eq!(schema_flag(&contacts_save_all, "name")["deprecated"], true);

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

    for (args, command, capability, required_phase) in [
        (
            &["runtime", "heartbeat", "status"][..],
            "runtime.heartbeat.status",
            "runtime-heartbeat",
            "outside current im-core cutover",
        ),
        (
            &["people", "search", "alice"][..],
            "people.search",
            "people-directory",
            "future people search API",
        ),
        (
            &["debug", "raw", "rpc"][..],
            "debug.raw.rpc",
            "raw-rpc",
            "outside current im-core cutover",
        ),
    ] {
        let output = awiki_cmd(args);
        assert_code(&output, 2);
        assert_stdout_empty(&output);
        let envelope = error_json(&output);

        if command == "debug.raw.rpc" {
            assert_eq!(envelope["error"]["code"], "removed_command");
            assert_eq!(envelope["error"]["details"]["command"], command);
        } else {
            assert_eq!(envelope["error"]["code"], "unsupported_capability");
            assert_eq!(envelope["error"]["details"]["command"], command);
            assert_eq!(envelope["error"]["details"]["capability"], capability);
            assert_eq!(
                envelope["error"]["details"]["required_phase"],
                required_phase
            );
            assert_eq!(
                envelope["error"]["details"]["cutover_status"],
                "unsupported"
            );
        }
    }

    let output = awiki_cmd(&["debug", "logs", "--follow"]);
    assert_code(&output, 2);
    assert_stdout_empty(&output);
    let envelope = error_json(&output);
    assert_eq!(envelope["error"]["code"], "diagnostic_gate_required");
    assert_eq!(envelope["error"]["details"]["command"], "debug.logs");

    let group_code = awiki_cmd(&["group", "code", "get", "--group", "did:group"]);
    assert_code(&group_code, 2);
    assert_stdout_empty(&group_code);
    let envelope = error_json(&group_code);
    assert_eq!(envelope["error"]["code"], "removed_command");
    assert_eq!(envelope["error"]["details"]["command"], "group.code.get");
}

#[test]
fn group_e2ee_unknown_subcommands_match_go_cobra_boundary() {
    for args in [
        &["group", "e2ee", "leave-requests", "--group", "did:group"][..],
        &[
            "group",
            "e2ee",
            "leave-request",
            "list",
            "--group",
            "did:group",
        ][..],
    ] {
        let output = awiki_cmd(args);
        assert_code(&output, 1);
        assert_stdout_empty(&output);
        let envelope = error_json(&output);

        assert_eq!(envelope["error"]["code"], "internal_error");
        assert_contains(&envelope["error"]["message"], "unknown command");
        assert_contains(&envelope["error"]["hint"], "awiki-cli group e2ee --help");
    }
}

#[test]
fn schema_metadata_matches_go_catalog_for_choices_and_grouping_nodes() {
    let upgrade = schema_for(&["upgrade"]);
    assert_eq!(
        schema_command(&upgrade)["short"],
        "Check for newer awiki-cli versions and show upgrade hints"
    );
    assert_eq!(schema_command(&upgrade)["side_effect"], false);

    let register = schema_for(&["id", "register"]);
    assert_eq!(
        schema_command(&register)["short"],
        "Register a handle-backed user identity"
    );
    assert_eq!(
        schema_flag(schema_command(&register), "handle")["usage"],
        "Handle local part"
    );
    assert_eq!(
        schema_flag(schema_command(&register), "wait")["usage"],
        "Wait for email verification before completing registration"
    );

    let profile = schema_for(&["id", "profile"]);
    assert_eq!(
        schema_command(&profile)["short"],
        "Read or update DID profile data"
    );
    let profile_set = schema_for(&["id", "profile", "set"]);
    assert_eq!(
        schema_command(&profile_set)["short"],
        "Update DID profile data"
    );
    assert_eq!(
        schema_flag(schema_command(&profile_set), "display-name")["usage"],
        "Profile display name"
    );
    assert_eq!(
        schema_flag(schema_command(&profile_set), "avatar-uri")["usage"],
        "Profile avatar URI"
    );
    let profile_set_all = schema_all_command("id.profile.set");
    assert_eq!(
        schema_flag(&profile_set_all, "avatar-url")["deprecated"],
        true
    );

    let msg_send = schema_for(&["msg", "send"]);
    assert_eq!(
        schema_flag(schema_command(&msg_send), "secure")["choices"],
        serde_json::json!(["off", "required"])
    );
    let msg_inbox = schema_for(&["msg", "inbox"]);
    assert_eq!(
        schema_flag(schema_command(&msg_inbox), "scope")["choices"],
        serde_json::json!(["all", "direct", "group"])
    );

    let group_create = schema_for(&["group", "create"]);
    assert!(
        schema_command(&group_create)["flags"]
            .as_array()
            .expect("group.create flags should be an array")
            .iter()
            .all(|flag| flag["name"] != "message-security-profile"),
        "deprecated message-security-profile should stay off the default schema surface: {group_create:?}"
    );
    let group_create_all = schema_all_command("group.create");
    assert_eq!(
        schema_flag(&group_create_all, "avatar-uri")["usage"],
        "Group avatar URI"
    );
    assert_eq!(
        schema_flag(&group_create_all, "message-security-profile")["choices"],
        serde_json::json!(["transport-protected", "group-e2ee"])
    );
    let publish = schema_for(&["group", "e2ee", "publish-key-package"]);
    assert_eq!(
        schema_flag(schema_command(&publish), "purpose")["choices"],
        serde_json::json!(["normal", "recovery", "update"])
    );
    let page_create = schema_for(&["page", "create"]);
    assert_eq!(
        schema_flag(schema_command(&page_create), "visibility")["choices"],
        serde_json::json!(["public", "draft", "unlisted"])
    );
    let page_update = schema_for(&["page", "update"]);
    assert_eq!(
        schema_flag(schema_command(&page_update), "visibility")["choices"],
        serde_json::json!(["public", "draft", "unlisted"])
    );

    let runtime_mode = schema_for(&["runtime", "mode"]);
    assert_eq!(schema_command(&runtime_mode)["implemented"], false);
    assert_eq!(
        schema_command(&runtime_mode)["short"],
        "Inspect or update runtime mode"
    );
    let runtime_setup = schema_for(&["runtime", "setup"]);
    assert_eq!(
        schema_flag(schema_command(&runtime_setup), "mode")["choices"],
        serde_json::json!(["http", "websocket"])
    );

    let listener = schema_for(&["runtime", "listener"]);
    assert_eq!(schema_command(&listener)["implemented"], false);
    let listener_install = schema_for(&["runtime", "listener", "install"]);
    assert_eq!(
        schema_command(&listener_install)["short"],
        "Install the listener service"
    );
    let listener_config = schema_for(&["runtime", "listener", "config"]);
    assert_eq!(schema_command(&listener_config)["implemented"], false);

    let host_notify = schema_for(&["runtime", "host-notify"]);
    assert_eq!(schema_command(&host_notify)["implemented"], false);
    assert_eq!(
        schema_command(&host_notify)["short"],
        "Inspect or update host notification settings"
    );
    let host_notify_set = schema_for(&["runtime", "host-notify", "config", "set"]);
    assert_eq!(
        schema_command(&host_notify_set)["short"],
        "Update host notification configuration"
    );
    assert_eq!(
        schema_flag(schema_command(&host_notify_set), "sink")["choices"],
        serde_json::json!(["noop", "log", "file", "openclaw", "hermes"])
    );
    let openclaw = schema_for(&["runtime", "host-notify", "openclaw"]);
    assert_eq!(schema_command(&openclaw)["implemented"], false);
    let openclaw_token = schema_for(&["runtime", "host-notify", "openclaw", "set-token"]);
    assert_eq!(
        schema_command(&openclaw_token)["short"],
        "Store the OpenClaw hook token in config"
    );
    assert_eq!(
        schema_flag(schema_command(&openclaw_token), "value")["required"],
        true
    );

    let debug = schema_for(&["debug"]);
    assert_eq!(
        schema_command(&debug)["short"],
        "Debugging and raw inspection commands"
    );
    let debug_db = schema_for(&["debug", "db"]);
    assert_eq!(schema_command(&debug_db)["phase"], "phase4");
    assert_eq!(
        schema_command(&debug_db)["short"],
        "Database inspection helpers"
    );
    let debug_query = schema_for(&["debug", "db", "query"]);
    assert_eq!(schema_command(&debug_query)["phase"], "phase4");
    assert_eq!(schema_command(&debug_query)["use"], "query <SQL>");
    let debug_import = schema_for(&["debug", "db", "import-v1"]);
    assert_eq!(schema_command(&debug_import)["phase"], "phase4");
    assert_eq!(
        schema_flag(schema_command(&debug_import), "path")["usage"],
        "Explicit legacy database path override"
    );
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
        workspace
            .path()
            .join("tenants")
            .join("default")
            .to_string_lossy()
            .as_ref()
    );
    assert_eq!(envelope["data"]["plan"]["config_exists"], true);
    assert!(
        !workspace.path().join("config.yaml").exists(),
        "dry-run init must not write legacy root config.yaml"
    );
    assert!(
        !workspace
            .path()
            .join("tenants")
            .join("default")
            .join("data")
            .join("awiki-cli.db")
            .exists(),
        "dry-run init must not initialize tenant SQLite state"
    );
}

#[test]
fn init_creates_real_sqlite_schema() {
    let workspace = TempDir::new().expect("temp workspace");
    let init_output = awiki_cmd_with_workspace(&["init"], workspace.path().to_str().unwrap());
    assert_success(&init_output);
    let envelope = success_json(&init_output);
    assert_eq!(
        envelope["data"]["listener"]["managed_by"],
        "awiki-cli runtime listener"
    );
    assert_eq!(
        envelope["data"]["listener"]["status_command"],
        "awiki-cli runtime listener status"
    );
    assert!(
        !String::from_utf8_lossy(&init_output.stdout).contains("not_managed_in_rust_slice"),
        "init output must not expose old slice-era implementation wording"
    );

    let connection = Connection::open(
        workspace
            .path()
            .join("tenants")
            .join("default")
            .join("data")
            .join("awiki-cli.db"),
    )
    .expect("open db");
    let table_name: String = connection
        .query_row(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'messages'",
            [],
            |row| row.get(0),
        )
        .expect("messages table exists");
    assert_eq!(table_name, "messages");
}

#[test]
fn debug_db_query_returns_stable_unsupported_capability() {
    let workspace = TempDir::new().expect("temp workspace");

    let output = awiki_cmd_with_workspace(
        &["--diagnostic", "debug", "db", "query", "SELECT 1 AS value"],
        workspace.path().to_str().unwrap(),
    );
    assert_code(&output, 2);
    assert_stdout_empty(&output);
    let envelope = error_json(&output);

    assert_eq!(envelope["error"]["code"], "unsupported_capability");
    assert_eq!(envelope["error"]["details"]["command"], "debug.db.query");
    assert_eq!(envelope["error"]["details"]["capability"], "raw-sql");
    assert_eq!(
        envelope["error"]["details"]["required_phase"],
        "outside current im-core cutover"
    );
    assert_eq!(
        envelope["error"]["details"]["cutover_status"],
        "unsupported"
    );
    assert!(
        !workspace.path().join("data").join("awiki-cli.db").exists(),
        "unsupported debug db query must not create the local SQLite store"
    );
}

#[test]
fn people_contacts_save_dry_run_uses_im_core_handler() {
    let workspace = TempDir::new().expect("temp workspace");

    let output = awiki_cmd_with_workspace(
        &[
            "--dry-run",
            "people",
            "contacts",
            "save",
            "--did",
            "did:example:alice",
            "--handle",
            "alice",
            "--display-name",
            "Alice",
            "--reason",
            "migration smoke",
        ],
        workspace.path().to_str().unwrap(),
    );
    assert_success(&output);
    let envelope = success_json(&output);

    assert_eq!(envelope["command"], "awiki-cli people contacts save");
    assert_eq!(envelope["meta"]["dry_run"], true);
    assert_eq!(envelope["data"]["plan"]["action"], "contacts.save");
    assert_eq!(envelope["data"]["plan"]["service"], "im-core.directory");
    assert_eq!(
        envelope["data"]["plan"]["operation"],
        "people.contacts.save"
    );
    assert_eq!(envelope["data"]["plan"]["did"], "did:example:alice");
    assert_eq!(envelope["data"]["plan"]["handle"], "alice.awiki.ai");
    assert_eq!(envelope["data"]["plan"]["display_name"], "Alice");
    assert_eq!(envelope["data"]["plan"]["name"], "Alice");
    assert_eq!(envelope["data"]["plan"]["note"], "migration smoke");
    assert_ne!(
        envelope["error"]["code"], "unsupported_capability",
        "people.contacts.save should route through the im-core handler"
    );
}

#[test]
fn people_follow_write_dry_run_plans_hide_relationship_wire_names() {
    let workspace = TempDir::new().expect("temp workspace");
    let cases = [
        (
            &["--dry-run", "people", "follow", "alice"][..],
            "awiki-cli people follow",
            "follow",
            "people.follow",
            "directory.follow",
            "Dry run: follow planned",
        ),
        (
            &["--dry-run", "people", "unfollow", "alice"][..],
            "awiki-cli people unfollow",
            "unfollow",
            "people.unfollow",
            "directory.unfollow",
            "Dry run: unfollow planned",
        ),
    ];

    for (args, command, action, operation, remote_call, summary) in cases {
        let output = awiki_cmd_with_workspace(args, workspace.path().to_str().unwrap());
        assert_success(&output);
        let envelope = success_json(&output);
        assert_eq!(envelope["command"], command);
        assert_eq!(envelope["summary"], summary);
        assert_eq!(envelope["data"]["plan"]["action"], action);
        assert_eq!(envelope["data"]["plan"]["service"], "im-core.directory");
        assert_eq!(envelope["data"]["plan"]["operation"], operation);
        assert_eq!(envelope["data"]["plan"]["remote_call"], remote_call);
        assert_eq!(
            envelope["data"]["plan"]["status_refresh"],
            "directory.relationship_status"
        );
        assert!(envelope["data"]["plan"].get("remote_calls").is_none());
    }
}

#[test]
fn people_read_dry_run_commands_use_im_core_plans_without_identity() {
    let workspace = TempDir::new().expect("temp workspace");
    let cases = [
        (
            &["--dry-run", "people", "status", "alice"][..],
            "awiki-cli people status",
            "relationship.status",
            "im-core.directory",
            "people.status",
            "directory.relationship_status",
            "Dry run: relationship status planned",
        ),
        (
            &[
                "--dry-run",
                "people",
                "followers",
                "--limit",
                "2",
                "--offset",
                "1",
                "--profile",
            ][..],
            "awiki-cli people followers",
            "relationships.followers",
            "im-core.directory",
            "people.followers",
            "directory.followers",
            "Dry run: followers list planned",
        ),
        (
            &["--dry-run", "people", "following", "--limit", "3"][..],
            "awiki-cli people following",
            "relationships.following",
            "im-core.directory",
            "people.following",
            "directory.following",
            "Dry run: following list planned",
        ),
        (
            &["--dry-run", "people", "contacts", "list", "--limit", "4"][..],
            "awiki-cli people contacts list",
            "contacts.list",
            "im-core.directory",
            "people.contacts.list",
            "",
            "Dry run: contacts list planned",
        ),
    ];

    for (args, command, action, service, operation, remote_call, summary) in cases {
        let output = awiki_cmd_with_workspace(args, workspace.path().to_str().unwrap());
        assert_success(&output);
        let envelope = success_json(&output);
        assert_eq!(envelope["command"], command);
        assert_eq!(envelope["summary"], summary);
        assert_eq!(envelope["meta"]["dry_run"], true);
        assert_eq!(envelope["data"]["plan"]["action"], action);
        assert_eq!(envelope["data"]["plan"]["service"], service);
        assert_eq!(envelope["data"]["plan"]["operation"], operation);
        if remote_call.is_empty() {
            assert!(envelope["data"]["plan"].get("remote_call").is_none());
        } else {
            assert_eq!(envelope["data"]["plan"]["remote_call"], remote_call);
        }
        assert!(
            !workspace.path().join("identities").exists(),
            "dry-run people read command should not require or create identity state"
        );
    }
}

#[test]
fn debug_db_import_v1_supports_dry_run_and_missing_path_errors() {
    let workspace = TempDir::new().expect("temp workspace");

    let dry_run = awiki_cmd_with_workspace(
        &["--migration", "debug", "db", "import-v1", "--dry-run"],
        workspace.path().to_str().unwrap(),
    );
    assert_success(&dry_run);
    let envelope = success_json(&dry_run);
    assert_eq!(envelope["meta"]["dry_run"], true);
    assert_eq!(envelope["data"]["plan"]["action"], "import_v1_sqlite");

    let missing = workspace.path().join("missing-legacy-root");
    let missing = awiki_cmd_with_workspace(
        &[
            "--migration",
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
        .env("HOME", Path::new(workspace).join("home"))
        .env("USERPROFILE", Path::new(workspace).join("home"))
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
        .env("HOME", Path::new(workspace).join("home"))
        .env("USERPROFILE", Path::new(workspace).join("home"))
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

fn schema_all_command(name: &str) -> Value {
    let output = awiki_cmd(&["schema", "--all"]);
    assert_success(&output);
    let schema = success_json(&output);
    schema["data"]["commands"]
        .as_array()
        .unwrap_or_else(|| panic!("schema --all commands should be an array: {schema:?}"))
        .iter()
        .find(|command| command["name"] == name)
        .unwrap_or_else(|| panic!("missing schema --all command {name:?}: {schema:?}"))
        .clone()
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

fn stdout_text(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
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

fn assert_docs_references_exist(references: &[Value]) {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crate should live under workspace root");
    for reference in references {
        let reference = reference
            .as_str()
            .unwrap_or_else(|| panic!("docs reference should be a string: {reference:?}"));
        if reference.starts_with("../") {
            continue;
        }
        assert!(
            repo_root.join(reference).is_file(),
            "docs reference {reference:?} should exist under {repo_root:?}"
        );
    }
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
        static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let thread_id = format!("{:?}", std::thread::current().id())
            .chars()
            .filter(|ch| ch.is_ascii_alphanumeric())
            .collect::<String>();
        let path = std::env::temp_dir().join(format!(
            "awiki-cli-rs2-test-{}-{nanos}-{thread_id}-{counter}",
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
