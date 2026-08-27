use awiki_cli::command_catalog::{CutoverStatus, DirectInvocationPolicy};
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static TEMP_DIR_COUNTER: AtomicU64 = AtomicU64::new(0);

#[test]
fn schema_flags_omit_empty_fields_like_go_catalog() {
    let tenant_create = schema_for(&["tenant", "create"]);
    let backend_base_url = schema_flag(schema_command(&tenant_create), "backend-base-url");

    assert_eq!(backend_base_url["name"], "backend-base-url");
    assert_eq!(backend_base_url["type"], "string");
    assert_eq!(backend_base_url["required"], true);
    assert!(backend_base_url.get("default").is_none());
    assert!(backend_base_url.get("choices").is_none());
    assert!(backend_base_url.get("deprecated").is_none());

    let create = schema_for(&["id", "create"]);
    let name = schema_flag(schema_command(&create), "name");
    assert_eq!(name["required"], true);
    assert!(name.get("default").is_none());
    assert!(name.get("choices").is_none());
    assert!(name.get("deprecated").is_none());

    let msg_send = schema_for(&["msg", "send"]);
    let secure = schema_flag(schema_command(&msg_send), "secure");
    assert_eq!(secure["default"], "off");
    assert_eq!(secure["choices"], serde_json::json!(["off", "required"]));
    assert!(secure.get("required").is_none());
    assert!(secure.get("deprecated").is_none());
    assert_eq!(
        schema_flag(schema_command(&msg_send), "payload")["type"],
        "string"
    );
    assert_eq!(
        schema_flag(schema_command(&msg_send), "payload-file")["type"],
        "string"
    );
}

#[test]
fn onboarding_claim_schema_accepts_only_stdin_secret_input() {
    let onboarding = schema_for(&["onboarding", "claim"]);
    let command = schema_command(&onboarding);
    let flags = schema_flag_names(command);

    assert_eq!(command["name"], "onboarding.claim");
    assert_eq!(command["primary_owner"], "im_core_onboarding");
    assert_eq!(command["side_effect"], true);
    assert!(flags.contains(&"service-base-url"));
    assert!(flags.contains(&"expected-controller-handle"));
    assert!(flags.contains(&"expected-agent-handle"));
    assert!(flags.contains(&"token-stdin"));
    assert!(!flags.contains(&"token"));
    assert_eq!(schema_flag(command, "token-stdin")["type"], "bool");
    assert_eq!(schema_flag(command, "token-stdin")["required"], true);
    assert!(schema_flag(command, "token-stdin")["usage"]
        .as_str()
        .unwrap()
        .contains("EOF"));
    assert!(!serde_json::to_string(command).unwrap().contains("awsk1_"));
}

#[test]
fn onboarding_legacy_migration_is_an_explicit_same_did_product_command() {
    let onboarding = schema_for(&["onboarding", "migrate-legacy"]);
    let command = schema_command(&onboarding);

    assert_eq!(command["name"], "onboarding.migrate-legacy");
    assert_eq!(command["primary_owner"], "im_core_onboarding");
    assert_eq!(command["side_effect"], true);
    assert!(
        command["flags"].is_null()
            || command["flags"]
                .as_array()
                .is_some_and(|flags| flags.is_empty())
    );
    let raw = serde_json::to_string(command).unwrap();
    assert!(raw.contains("same-DID Legacy upgrade"));
    assert!(!raw.contains("private key"));
    assert!(!raw.contains("access_token"));
}

#[test]
fn onboarding_legacy_claim_recovery_reuses_original_did_and_stdin_secret_only() {
    let onboarding = schema_for(&["onboarding", "recover-legacy-claim"]);
    let command = schema_command(&onboarding);

    assert_eq!(command["name"], "onboarding.recover-legacy-claim");
    assert_eq!(command["primary_owner"], "im_core_onboarding");
    assert_eq!(command["side_effect"], true);
    assert_eq!(
        schema_flag_names(command),
        vec![
            "service-base-url",
            "expected-controller-handle",
            "expected-agent-handle",
            "token-stdin",
        ]
    );
    let raw = serde_json::to_string(command).unwrap();
    assert!(raw.contains("exact pending DID and key material"));
    assert!(raw.contains("never deletes recovery artifacts"));
    assert!(!raw.contains("token-value"));
    assert!(!raw.contains("access_token"));
}

#[test]
fn schema_command_list_preserves_go_catalog_order_for_drift_prone_groups() {
    let output = awiki_cmd(&["schema"]);
    assert_success(&output);
    let envelope = success_json(&output);
    let commands = envelope["data"]["commands"]
        .as_array()
        .expect("data.commands should be an array");
    let names: Vec<&str> = commands
        .iter()
        .map(|command| {
            command["name"]
                .as_str()
                .expect("schema command should have a string name")
        })
        .collect();

    assert_subsequence(
        &names,
        &[
            "id.use",
            "id.profile",
            "id.profile.get",
            "id.profile.set",
            "msg",
        ],
    );
    assert_subsequence(
        &names,
        &[
            "runtime.listener",
            "runtime.listener.status",
            "runtime.listener.enable",
            "runtime.listener.disable",
            "runtime.host-notify",
            "runtime.host-notify.enable",
            "runtime.host-notify.disable",
        ],
    );
}

#[test]
fn schema_audience_views_expose_non_default_surfaces() {
    let operator = success_json(&awiki_cmd(&["schema", "--audience", "operator"]));
    let operator_names = schema_names(&operator);
    assert!(operator_names.contains(&"runtime.listener.start"));
    assert!(operator_names.contains(&"runtime.host-notify.hermes.setup"));
    assert!(!operator_names.contains(&"msg.send"));

    let migration = success_json(&awiki_cmd(&["schema", "--audience", "migration"]));
    let migration_names = schema_names(&migration);
    assert!(migration_names.contains(&"id.vault.migrate"));
    assert!(migration_names.contains(&"id.vault.cleanup-plaintext"));

    let diagnostic = success_json(&awiki_cmd(&["schema", "--audience", "diagnostic"]));
    let diagnostic_names = schema_names(&diagnostic);
    assert!(diagnostic_names.contains(&"debug.db.handle-history"));
    assert!(diagnostic_names.contains(&"runtime.host-notify.hermes.set-secret"));

    let all = success_json(&awiki_cmd(&["schema", "--all"]));
    let all_names = schema_names(&all);
    assert!(all_names.contains(&"runtime.listener.service-run"));
    assert!(all_names.contains(&"debug.raw.rpc"));

    let default = success_json(&awiki_cmd(&["schema"]));
    let default_names = schema_names(&default);
    assert!(default_names.contains(&"id.vault.status"));
    assert!(!default_names.contains(&"id.vault.migrate"));
}

#[test]
fn default_schema_hides_deprecated_e2ee_alias_flags_but_all_keeps_metadata() {
    let default = success_json(&awiki_cmd(&["schema"]));
    let create = default["data"]["commands"]
        .as_array()
        .expect("default commands")
        .iter()
        .find(|command| command["name"] == "group.create")
        .expect("group.create default schema entry");
    let default_flags = schema_flag_names(create);
    assert!(default_flags.contains(&"secure"));
    assert!(!default_flags.contains(&"e2ee"));
    assert!(!default_flags.contains(&"message-security-profile"));

    let group = success_json(&awiki_cmd(&["schema", "group"]));
    let child_create = group["data"]["children"]
        .as_array()
        .expect("group children")
        .iter()
        .find(|command| command["name"] == "group.create")
        .expect("group.create child schema entry");
    let child_flags = schema_flag_names(child_create);
    assert!(child_flags.contains(&"secure"));
    assert!(!child_flags.contains(&"e2ee"));
    assert!(!child_flags.contains(&"message-security-profile"));

    let exact_create = schema_for(&["group", "create"]);
    let exact_flags = schema_flag_names(schema_command(&exact_create));
    assert!(exact_flags.contains(&"secure"));
    assert!(!exact_flags.contains(&"e2ee"));
    assert!(!exact_flags.contains(&"message-security-profile"));

    let all = success_json(&awiki_cmd(&["schema", "--all"]));
    let all_create = all["data"]["commands"]
        .as_array()
        .expect("all commands")
        .iter()
        .find(|command| command["name"] == "group.create")
        .expect("group.create all schema entry");
    let e2ee = schema_flag(all_create, "e2ee");
    let profile = schema_flag(all_create, "message-security-profile");
    assert_eq!(e2ee["deprecated"], true);
    assert_eq!(profile["deprecated"], true);
    assert_eq!(
        profile["choices"],
        serde_json::json!(["transport-protected", "group-e2ee"])
    );
}

#[test]
fn default_schema_surface_does_not_advertise_non_product_commands() {
    let default = success_json(&awiki_cmd(&["schema"]));
    let commands = default["data"]["commands"]
        .as_array()
        .expect("default schema commands");
    let mut leaked = Vec::new();

    for command in commands {
        let name = command["name"].as_str().expect("schema name");
        let cutover = command["cutover"]["status"].as_str().unwrap_or("missing");
        let policy = command["direct_invocation"]["policy"]
            .as_str()
            .unwrap_or("missing");
        let hidden = command["hidden"].as_bool().unwrap_or(false);
        if matches!(
            cutover,
            "unsupported" | "removed" | "hidden" | "diagnostic_only"
        ) || matches!(
            policy,
            "require_diagnostic_gate"
                | "require_migration_gate"
                | "require_internal_service_gate"
                | "stable_unsupported"
                | "removed"
        ) || hidden
        {
            leaked.push(format!(
                "{name}: cutover={cutover}, policy={policy}, hidden={hidden}"
            ));
        }
    }

    assert!(
        leaked.is_empty(),
        "default schema must stay limited to current public product commands: {leaked:?}"
    );
}

#[test]
fn documented_current_user_commands_resolve_to_supported_current_surface() {
    for words in [
        &["status"][..],
        &["doctor"][..],
        &["version"][..],
        &["init"][..],
        &["onboarding", "claim"][..],
        &["config", "show"][..],
        &["id", "list"][..],
        &["id", "current"][..],
        &["id", "status"][..],
        &["id", "register"][..],
        &["id", "bind"][..],
        &["id", "resolve"][..],
        &["id", "profile", "get"][..],
        &["id", "profile", "set"][..],
        &["msg", "send"][..],
        &["msg", "inbox"][..],
        &["msg", "history"][..],
        &["msg", "mark-read"][..],
        &["msg", "attachment", "download"][..],
        &["tenant", "list"][..],
        &["tenant", "current"][..],
        &["tenant", "create"][..],
        &["tenant", "setup"][..],
        &["tenant", "use"][..],
        &["tenant", "reconfigure"][..],
        &["mail", "inbox"][..],
        &["mail", "read"][..],
        &["mail", "mark-read"][..],
        &["mail", "send"][..],
        &["mail", "attachment", "download"][..],
        &["group", "create"][..],
        &["group", "list"][..],
        &["group", "get"][..],
        &["group", "join"][..],
        &["group", "leave"][..],
        &["group", "add"][..],
        &["group", "remove"][..],
        &["group", "members"][..],
        &["group", "messages"][..],
        &["group", "secure", "status"][..],
        &["group", "secure", "repair"][..],
        &["people", "contacts", "list"][..],
        &["people", "contacts", "save"][..],
        &["page", "list"][..],
        &["page", "get"][..],
        &["page", "create"][..],
        &["page", "update"][..],
        &["page", "delete"][..],
        &["site", "root", "get"][..],
        &["site", "root", "set"][..],
        &["site", "page", "list"][..],
        &["site", "page", "create"][..],
        &["runtime", "status"][..],
        &["runtime", "listener", "status"][..],
        &["runtime", "host-notify", "config", "show"][..],
    ] {
        let resolved =
            awiki_cli::command_catalog::resolve_command(words).expect("documented command");
        assert_eq!(
            resolved.consumed_words,
            words.len(),
            "documented command should resolve exactly: {words:?}"
        );
        let spec = awiki_cli::command_catalog::lookup(&resolved.name)
            .expect("resolved documented command has schema");
        assert!(
            matches!(
                spec.cutover_status(),
                CutoverStatus::CliOwned | CutoverStatus::ImCore
            ),
            "{:?} resolves to non-current cutover status {:?}",
            words,
            spec.cutover_status()
        );
        assert!(
            matches!(
                spec.direct_invocation(),
                DirectInvocationPolicy::Allow | DirectInvocationPolicy::AllowWithWarning
            ),
            "{:?} resolves to gated or unsupported direct invocation {:?}",
            words,
            spec.direct_invocation()
        );
    }
}

#[test]
fn cmdmeta_resolves_canonical_paths_and_aliases_as_single_command_tree() {
    for (words, name, consumed) in [
        (&["status"][..], "status", 1),
        (&["id", "vault", "status"][..], "id.vault.status", 3),
        (&["group", "get"][..], "group.get", 2),
        (&["group", "show"][..], "group.get", 2),
        (&["group", "remove"][..], "group.remove", 2),
        (&["group", "kick"][..], "group.remove", 2),
        (
            &["runtime", "host-notify", "hermes", "guide"][..],
            "runtime.host-notify.hermes.guide",
            4,
        ),
        (
            &["runtime", "host-notify", "webhook", "guide"][..],
            "runtime.host-notify.hermes.guide",
            4,
        ),
    ] {
        let resolved =
            awiki_cli::command_catalog::resolve_command(words).expect("command resolves");
        assert_eq!(resolved.name, name, "words: {words:?}");
        assert_eq!(resolved.consumed_words, consumed, "words: {words:?}");
    }
}

#[test]
fn cmdmeta_rejects_unknown_group_e2ee_subcommands_like_cli_boundary() {
    let err = awiki_cli::command_catalog::resolve_command(&["group", "e2ee", "leave-requests"])
        .expect_err("unknown group e2ee subcommand should fail");
    assert_eq!(
        err,
        awiki_cli::command_catalog::CommandResolveError::UnknownSubcommand {
            parent: "group e2ee",
            subcommand: "leave-requests".to_string(),
        }
    );
}

#[test]
fn cmdmeta_bool_flag_lookup_is_command_scoped() {
    assert!(awiki_cli::command_catalog::is_local_bool_flag(
        "runtime.listener.config.set",
        "enabled"
    ));
    assert!(awiki_cli::command_catalog::is_local_bool_flag(
        "debug.logs",
        "follow"
    ));
    assert!(!awiki_cli::command_catalog::is_local_bool_flag(
        "tenant.create",
        "backend-base-url"
    ));
    assert!(!awiki_cli::command_catalog::is_local_bool_flag(
        "status", "enabled"
    ));
}

#[test]
fn cmdmeta_handler_catalog_matches_cli_dispatch_table() {
    let dispatch_names = cli_dispatch_names();
    let metadata_names: BTreeSet<_> = awiki_cli::command_catalog::specs()
        .into_iter()
        .map(|spec| spec.name.to_string())
        .collect();

    for command in &dispatch_names {
        assert!(
            metadata_names.contains(*command),
            "dispatch command {command:?} must have a cmdmeta entry"
        );
    }

    let undispatched: Vec<_> = awiki_cli::command_catalog::specs()
        .into_iter()
        .filter(|spec| {
            spec.implemented
                && !spec.handler.is_empty()
                && spec.handler != "stub"
                && !dispatch_names.contains(spec.name)
        })
        .map(|spec| spec.name)
        .collect();
    assert!(
        undispatched.is_empty(),
        "implemented cmdmeta handlers must be dispatched: {undispatched:?}"
    );
}

fn cli_dispatch_names() -> BTreeSet<&'static str> {
    [
        "status",
        "version",
        "upgrade",
        "config.show",
        "tenant.list",
        "tenant.current",
        "tenant.create",
        "tenant.setup",
        "tenant.use",
        "tenant.reconfigure",
        "doctor",
        "docs",
        "schema",
        "help",
        "init",
        "onboarding.claim",
        "onboarding.resume",
        "onboarding.recover-legacy-claim",
        "onboarding.migrate-legacy",
        "completion.bash",
        "completion.zsh",
        "completion.fish",
        "completion.powershell",
        "id.create",
        "id.register",
        "id.list",
        "id.current",
        "id.use",
        "id.status",
        "id.vault.status",
        "id.vault.migrate",
        "id.vault.cleanup-plaintext",
        "id.import-v1",
        "id.bind",
        "id.refresh-token",
        "id.resolve",
        "id.profile.get",
        "id.profile.set",
        "id.device.list",
        "id.device.join.sessions",
        "id.device.join.requests",
        "id.device.join.start",
        "id.device.join.poll",
        "id.device.join.verify",
        "id.device.join.approve",
        "id.device.join.reject",
        "id.device.join.cancel",
        "id.device.revoke",
        "id.device.root-key.send",
        "msg.send",
        "msg.attachment.download",
        "msg.inbox",
        "msg.history",
        "msg.mark-read",
        "msg.secure.status",
        "mail.inbox",
        "mail.read",
        "mail.mark-read",
        "mail.account",
        "mail.send",
        "mail.attachment.download",
        "mail.notify",
        "people.follow",
        "people.unfollow",
        "people.status",
        "people.followers",
        "people.following",
        "people.contacts.list",
        "people.contacts.save",
        "group.create",
        "group.get",
        "group.join",
        "group.add",
        "group.remove",
        "group.leave",
        "group.update",
        "group.list",
        "group.members",
        "group.messages",
        "group.secure.status",
        "group.secure.repair",
        "group.secure.diagnostics",
        "group.e2ee.status",
        "group.e2ee.publish-key-package",
        "group.e2ee.repair",
        "page.create",
        "page.list",
        "page.get",
        "page.update",
        "page.rename",
        "page.delete",
        "site.root.get",
        "site.root.set",
        "site.page.list",
        "site.page.get",
        "site.page.create",
        "site.page.update",
        "site.page.rename",
        "site.page.delete",
        "runtime.status",
        "runtime.apply",
        "runtime.setup",
        "runtime.mode.get",
        "runtime.mode.set",
        "runtime.listener.status",
        "runtime.listener.install",
        "runtime.listener.start",
        "runtime.listener.stop",
        "runtime.listener.restart",
        "runtime.listener.uninstall",
        "runtime.listener.run",
        "runtime.listener.service-run",
        "runtime.listener.config.show",
        "runtime.listener.config.set",
        "runtime.listener.enable",
        "runtime.listener.disable",
        "runtime.host-notify.enable",
        "runtime.host-notify.disable",
        "runtime.host-notify.config.show",
        "runtime.host-notify.config.set",
        "runtime.host-notify.openclaw.set",
        "runtime.host-notify.openclaw.set-token",
        "runtime.host-notify.openclaw.clear-token",
        "runtime.host-notify.openclaw.route.add",
        "runtime.host-notify.openclaw.route.list",
        "runtime.host-notify.openclaw.route.remove",
        "runtime.host-notify.hermes.guide",
        "runtime.host-notify.hermes.status",
        "runtime.host-notify.hermes.setup",
        "runtime.host-notify.hermes.bridge.service-run",
        "runtime.host-notify.hermes.set",
        "runtime.host-notify.hermes.set-secret",
        "runtime.host-notify.hermes.clear-secret",
        "debug.db.import-v1",
        "debug.db.handle-history",
    ]
    .into_iter()
    .collect()
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

fn schema_names(schema: &Value) -> Vec<&str> {
    schema["data"]["commands"]
        .as_array()
        .expect("data.commands should be an array")
        .iter()
        .map(|command| command["name"].as_str().expect("schema name"))
        .collect()
}

fn schema_flag<'a>(command: &'a Value, name: &str) -> &'a Value {
    command["flags"]
        .as_array()
        .expect("schema flags should be an array")
        .iter()
        .find(|flag| flag["name"] == name)
        .unwrap_or_else(|| panic!("missing schema flag {name:?}: {command:?}"))
}

fn schema_flag_names(command: &Value) -> Vec<&str> {
    command["flags"]
        .as_array()
        .expect("schema flags should be an array")
        .iter()
        .map(|flag| flag["name"].as_str().expect("flag name"))
        .collect()
}

fn assert_subsequence(actual: &[&str], expected: &[&str]) {
    let mut cursor = 0usize;
    for expected_name in expected {
        let offset = actual[cursor..]
            .iter()
            .position(|actual_name| actual_name == expected_name)
            .unwrap_or_else(|| panic!("missing {expected_name:?} after {cursor}: {actual:?}"));
        cursor += offset + 1;
    }
}

fn awiki_cmd(args: &[&str]) -> Output {
    let workspace = TempDir::new().expect("temp workspace");
    let mut command = Command::new(env!("CARGO_BIN_EXE_awiki-cli"));
    command
        .args(args)
        .env("AWIKI_CLI_WORKSPACE_HOME_DIR", workspace.path())
        .env("HOME", workspace.path().join("home"))
        .env("USERPROFILE", workspace.path().join("home"))
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

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new() -> std::io::Result<Self> {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let counter = TEMP_DIR_COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "awiki-cli-rs2-schema-test-{}-{nanos}-{counter}",
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

fn assert_success(output: &Output) {
    assert_eq!(
        output.status.code(),
        Some(0),
        "unexpected exit status; stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn success_json(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|err| panic!("stdout should be JSON: {err}; output={output:?}"))
}
