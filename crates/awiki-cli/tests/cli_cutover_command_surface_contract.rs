use awiki_cli::command_catalog::{
    self, CliShellRole, CommandAudience, CommandOwner, CutoverStatus, DirectInvocationPolicy,
};
use serde_json::Value;
use std::process::{Command, Output};

#[test]
fn every_command_spec_has_cutover_classification() {
    let missing: Vec<_> = command_catalog::specs()
        .into_iter()
        .filter(|spec| command_catalog::try_cutover_status(spec.name).is_none())
        .map(|spec| spec.name)
        .collect();

    assert!(
        missing.is_empty(),
        "every current command spec must be classified for CLI cutover: {missing:?}"
    );
}

#[test]
fn every_command_spec_has_required_policy_metadata() {
    for spec in command_catalog::specs() {
        assert_eq!(spec.canonical_name(), spec.name);
        assert!(
            !spec.audience().as_str().is_empty(),
            "{} must classify audience",
            spec.name
        );
        assert!(
            !spec.primary_owner().as_str().is_empty(),
            "{} must classify primary owner",
            spec.name
        );
        assert!(
            !spec.cli_shell_role().as_str().is_empty(),
            "{} must classify CLI shell role",
            spec.name
        );
        assert!(
            !spec.direct_invocation().kind().is_empty(),
            "{} must classify direct invocation policy",
            spec.name
        );
    }
}

#[test]
fn cutover_classifier_marks_supported_im_core_commands() {
    for command in [
        "id.list",
        "id.current",
        "id.status",
        "id.use",
        "id.register",
        "id.refresh-token",
        "id.resolve",
        "id.bind",
        "id.recover",
        "id.recovery",
        "id.recovery.sessions",
        "id.recovery.begin",
        "id.recovery.status",
        "id.recovery.cancel",
        "id.recovery.finalize",
        "id.recovery.activate",
        "id.profile.get",
        "id.profile.set",
        "msg.send",
        "msg.inbox",
        "msg.history",
        "msg.mark-read",
        "msg.attachment.download",
        "msg.secure",
        "msg.secure.status",
        "msg.secure.repair",
        "mail",
        "mail.account",
        "mail.attachment",
        "mail.attachment.download",
        "mail.inbox",
        "mail.mark-read",
        "mail.notify",
        "mail.read",
        "mail.send",
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
        "group.secure",
        "group.secure.status",
        "group.secure.repair",
        "group.e2ee.status",
        "group.e2ee.repair",
        "people.follow",
        "people.unfollow",
        "people.status",
        "people.followers",
        "people.following",
        "people.contacts.list",
        "people.contacts.save",
        "page",
        "page.create",
        "page.list",
        "page.get",
        "page.update",
        "page.rename",
        "page.delete",
        "site",
        "site.root",
        "site.root.get",
        "site.root.set",
        "site.page",
        "site.page.list",
        "site.page.get",
        "site.page.create",
        "site.page.update",
        "site.page.rename",
        "site.page.delete",
    ] {
        assert_eq!(
            command_catalog::cutover_status(command),
            CutoverStatus::ImCore,
            "{command} should be on the im-core cutover path"
        );
    }
}

#[test]
fn cutover_classifier_keeps_cli_owned_host_commands() {
    for command in [
        "status",
        "docs",
        "schema",
        "doctor",
        "version",
        "upgrade",
        "init",
        "completion.bash",
        "config.show",
        "runtime.status",
        "runtime.listener.status",
        "runtime.listener.enable",
        "runtime.listener.disable",
        "runtime.host-notify.enable",
        "runtime.host-notify.disable",
    ] {
        assert_eq!(
            command_catalog::cutover_status(command),
            CutoverStatus::CliOwned,
            "{command} should remain CLI-owned"
        );
    }
}

#[test]
fn cutover_classifier_marks_unsupported_and_internal_commands() {
    assert_eq!(
        command_catalog::cutover_status("msg.secure.init"),
        CutoverStatus::Unsupported {
            capability: "secure-direct",
            phase: "Phase 6",
        }
    );
    assert_eq!(
        command_catalog::cutover_status("msg.secure.outbox.list"),
        CutoverStatus::Unsupported {
            capability: "secure-direct",
            phase: "Phase 6",
        }
    );
    assert_eq!(
        command_catalog::cutover_status("people.search"),
        CutoverStatus::Unsupported {
            capability: "people-directory",
            phase: "future people search API",
        }
    );
    assert_eq!(
        command_catalog::cutover_status("runtime.heartbeat.status"),
        CutoverStatus::Unsupported {
            capability: "runtime-heartbeat",
            phase: "outside current im-core cutover",
        }
    );
    assert_eq!(
        command_catalog::cutover_status("group.e2ee.publish-key-package"),
        CutoverStatus::DiagnosticOnly
    );
    assert_eq!(
        command_catalog::cutover_status("group.secure.diagnostics"),
        CutoverStatus::Unsupported {
            capability: "group secure diagnostics",
            phase: "future diagnostics plan",
        }
    );
    assert_eq!(
        command_catalog::cutover_status("debug.db.query"),
        CutoverStatus::Unsupported {
            capability: "raw-sql",
            phase: "outside current im-core cutover",
        }
    );
    assert_eq!(
        command_catalog::cutover_status("runtime.host-notify.openclaw.set-token"),
        CutoverStatus::DiagnosticOnly
    );
    assert_eq!(
        command_catalog::cutover_status("runtime.listener.run"),
        CutoverStatus::Hidden
    );
    assert_eq!(
        command_catalog::direct_invocation_policy("runtime.listener.run"),
        DirectInvocationPolicy::RequireInternalServiceGate
    );
    assert_eq!(
        command_catalog::cutover_status("group.code.get"),
        CutoverStatus::Removed
    );
    assert_eq!(
        command_catalog::cutover_status("debug.raw.rpc"),
        CutoverStatus::Removed
    );
}

#[test]
fn command_policy_maps_final_cutover_surfaces() {
    let msg_send = command_catalog::lookup("msg.send").unwrap();
    assert_eq!(msg_send.audience(), CommandAudience::DefaultUser);
    assert_eq!(msg_send.primary_owner(), CommandOwner::ImCoreMessages);
    assert_eq!(msg_send.cli_shell_role(), CliShellRole::ReadsUserInputFile);
    assert_eq!(msg_send.direct_invocation(), DirectInvocationPolicy::Allow);

    let listener_start = command_catalog::lookup("runtime.listener.start").unwrap();
    assert_eq!(listener_start.audience(), CommandAudience::Operator);
    assert_eq!(
        listener_start.cli_shell_role(),
        CliShellRole::ManagesLocalService
    );
    assert_eq!(
        listener_start.direct_invocation(),
        DirectInvocationPolicy::Allow
    );

    let service_run = command_catalog::lookup("runtime.listener.service-run").unwrap();
    assert_eq!(service_run.audience(), CommandAudience::InternalService);
    assert_eq!(
        service_run.direct_invocation(),
        DirectInvocationPolicy::RequireInternalServiceGate
    );

    let diagnostic = command_catalog::lookup("debug.db.handle-history").unwrap();
    assert_eq!(diagnostic.audience(), CommandAudience::Diagnostic);
    assert_eq!(diagnostic.primary_owner(), CommandOwner::CliDiagnostic);
    assert_eq!(
        diagnostic.direct_invocation(),
        DirectInvocationPolicy::RequireDiagnosticGate
    );

    let migration = command_catalog::lookup("id.import-v1").unwrap();
    assert_eq!(migration.audience(), CommandAudience::MigrationOnly);
    assert_eq!(migration.primary_owner(), CommandOwner::CliMigration);
    assert_eq!(
        migration.direct_invocation(),
        DirectInvocationPolicy::RequireMigrationGate
    );

    let secure_status = command_catalog::lookup("msg.secure.status").unwrap();
    assert_eq!(secure_status.audience(), CommandAudience::DefaultUser);
    assert_eq!(secure_status.primary_owner(), CommandOwner::ImCoreSecure);
    assert_eq!(
        secure_status.direct_invocation(),
        DirectInvocationPolicy::Allow
    );

    let group_secure_repair = command_catalog::lookup("group.secure.repair").unwrap();
    assert_eq!(group_secure_repair.audience(), CommandAudience::DefaultUser);
    assert_eq!(
        group_secure_repair.primary_owner(),
        CommandOwner::ImCoreSecure
    );
    assert_eq!(
        group_secure_repair.secondary_owners(),
        &[CommandOwner::ImCoreGroups]
    );
    assert_eq!(
        group_secure_repair.direct_invocation(),
        DirectInvocationPolicy::Allow
    );

    let group_e2ee_status = command_catalog::lookup("group.e2ee.status").unwrap();
    assert_eq!(group_e2ee_status.audience(), CommandAudience::DefaultUser);
    assert_eq!(
        group_e2ee_status.direct_invocation(),
        DirectInvocationPolicy::DeprecatedAlias {
            replacement: "group secure status",
            until: "next-major",
        }
    );
}

#[test]
fn schema_serializes_cutover_status_for_commands_and_children() {
    let msg_send = schema_value("msg.send");
    assert_eq!(msg_send["cutover"]["status"], "im_core");
    assert_eq!(msg_send["cutover"]["default_surface"], true);
    assert_eq!(msg_send["audience"], "default");
    assert_eq!(msg_send["primary_owner"], "im_core_messages");
    assert_eq!(msg_send["cli_shell_role"], "reads_user_input_file");
    assert_eq!(msg_send["direct_invocation"]["policy"], "allow");

    let attachment = schema_value("msg.attachment.download");
    assert_eq!(attachment["cutover"]["status"], "im_core");
    assert_eq!(attachment["cutover"]["default_surface"], true);

    let page_list = schema_value("page.list");
    assert_eq!(page_list["cutover"]["status"], "im_core");
    assert_eq!(page_list["cutover"]["default_surface"], true);

    let site_page_list = schema_value("site.page.list");
    assert_eq!(site_page_list["cutover"]["status"], "im_core");
    assert_eq!(site_page_list["cutover"]["default_surface"], true);

    let runtime = schema_value("runtime.listener");
    let run = schema_child("runtime.listener", "runtime.listener.run");
    assert_eq!(runtime["cutover"]["status"], "cli_owned");
    assert_eq!(run["cutover"]["status"], "hidden");
    assert_eq!(run["audience"], "internal");
    assert_eq!(
        run["direct_invocation"]["policy"],
        "require_internal_service_gate"
    );
}

#[test]
fn default_schema_surface_includes_only_cli_owned_and_im_core_commands() {
    let names: Vec<_> = command_catalog::default_surface_specs()
        .into_iter()
        .map(|spec| spec.name)
        .collect();

    for command in [
        "status",
        "schema",
        "id.list",
        "msg.send",
        "msg.inbox",
        "msg.attachment.download",
        "msg.secure.status",
        "msg.secure.repair",
        "mail.inbox",
        "mail.attachment.download",
        "group.create",
        "group.secure.status",
        "group.secure.repair",
        "people.follow",
        "people.contacts.list",
        "page.list",
        "page.create",
        "site.root.get",
        "site.page.list",
        "runtime.listener.enable",
        "runtime.host-notify.enable",
    ] {
        assert!(
            names.contains(&command),
            "{command} should remain on the default schema/help surface"
        );
    }

    for command in [
        "msg.secure.init",
        "msg.secure.failed",
        "msg.secure.retry",
        "msg.secure.drop",
        "people.search",
        "runtime.heartbeat.status",
        "group.e2ee.status",
        "group.e2ee.repair",
        "group.e2ee.publish-key-package",
        "group.secure.diagnostics",
        "debug.db.query",
        "runtime.host-notify.openclaw.set-token",
        "runtime.listener.start",
        "runtime.host-notify.hermes.setup",
        "runtime.listener.run",
        "group.code.get",
        "debug.raw.rpc",
    ] {
        assert!(
            !names.contains(&command),
            "{command} should not be advertised on the default schema/help surface"
        );
        assert!(
            command_catalog::lookup(command).is_some(),
            "{command} should remain queryable by exact schema target"
        );
    }
}

#[test]
fn release_artifact_script_documents_e2ee_feature_gate() {
    let crate_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let manifest = std::fs::read_to_string(crate_root.join("Cargo.toml"))
        .expect("read awiki-cli Cargo manifest");
    let im_core_dependency = manifest
        .lines()
        .find(|line| line.starts_with("im-core = "))
        .expect("awiki-cli im-core dependency");
    assert!(im_core_dependency.contains("\"group-e2ee\""));
    assert!(im_core_dependency.contains("\"blocking\""));

    let group_service =
        std::fs::read_to_string(crate_root.join("../im-core/src/groups/service.rs"))
            .expect("read im-core Group service");
    let create_async = group_service
        .split("pub async fn create_async(")
        .nth(1)
        .and_then(|rest| rest.split("pub fn join(").next())
        .expect("GroupService::create_async source");
    let worker_start = create_async
        .find("run_blocking(move ||")
        .expect("P6 sync initialization should enter the blocking worker");
    let worker_end = create_async[worker_start..]
        .find(".map_err(|err| crate::ImError::Internal")
        .map(|offset| worker_start + offset)
        .expect("P6 blocking worker result mapping");
    let worker_call = &create_async[worker_start..worker_end];
    assert!(worker_call.contains("initialize_created_group("));
    assert!(worker_call.trim_end().ends_with(".await"));

    let script = std::fs::read_to_string(
        crate_root
            .join("../..")
            .join("scripts/release/build-release-artifact.sh"),
    )
    .expect("read release artifact script");

    assert!(script.contains("verify_e2ee_feature_graph"));
    assert!(script.contains("im-core feature \"group-e2ee\""));
    assert!(script.contains("anp feature \"mls\""));
    assert!(script.contains("cargo_cmd[@]}\" tree -p awiki-cli -e features --locked"));
    assert!(script.contains("Windows E2EE package/release validation is deferred"));
    assert!(script.contains("x86_64-unknown-linux-musl"));
    assert!(script.contains("Linux CLI release binary contains GLIBC symbol requirements"));
    assert!(script.contains("git rev-parse HEAD"));
    assert!(!script.contains("git rev-parse --short HEAD"));
}

#[test]
fn unsupported_cutover_error_has_stable_contract() {
    let err = awiki_cli::cli_shell::unsupported::unsupported_cutover_command(
        "msg.send",
        "attachments",
        "Phase 4",
    );

    assert_eq!(err.exit_code, 2);
    assert_eq!(err.detail.code, "unsupported_capability");
    assert_eq!(err.detail.details["command"], "msg.send");
    assert_eq!(err.detail.details["capability"], "attachments");
    assert_eq!(err.detail.details["required_phase"], "Phase 4");
    assert_eq!(err.detail.details["cutover_status"], "unsupported");
    assert!(
        err.detail.message.contains("attachments"),
        "message should name capability: {:?}",
        err.detail.message
    );
    assert!(
        err.detail.hint.contains("Phase 4"),
        "hint should name required phase: {:?}",
        err.detail.hint
    );
}

#[test]
fn unsupported_cutover_stub_commands_do_not_enter_legacy_stub_boundary() {
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
}

#[test]
fn direct_invocation_policy_is_enforced_before_handlers() {
    for (args, code) in [
        (
            &["debug", "db", "handle-history", "alice"][..],
            "diagnostic_gate_required",
        ),
        (&["id", "import-v1"][..], "migration_gate_required"),
        (
            &["runtime", "listener", "service-run"][..],
            "internal_command",
        ),
        (
            &["group", "e2ee", "publish-key-package"][..],
            "internal_command",
        ),
        (
            &[
                "group",
                "secure",
                "diagnostics",
                "--group",
                "did:wba:awiki.ai:groups:demo:e1",
            ][..],
            "unsupported_capability",
        ),
        (&["debug", "raw", "rpc"][..], "removed_command"),
    ] {
        let output = awiki_cmd(args);
        assert_code(&output, 2);
        let envelope = error_json(&output);
        assert_eq!(envelope["error"]["code"], code);
    }
}

#[test]
fn unsupported_non_im_domains_do_not_enter_legacy_handlers() {
    let args = &["debug", "db", "query", "SELECT 1"][..];
    let command = "debug.db.query";
    let capability = "raw-sql";
    let output = awiki_cmd(args);
    assert_code(&output, 2);
    let envelope = error_json(&output);

    assert_eq!(envelope["error"]["code"], "unsupported_capability");
    assert_eq!(envelope["error"]["details"]["command"], command);
    assert_eq!(envelope["error"]["details"]["capability"], capability);
    assert_eq!(
        envelope["error"]["details"]["required_phase"],
        "outside current im-core cutover"
    );
    assert_eq!(
        envelope["error"]["details"]["cutover_status"],
        "unsupported"
    );
}

fn schema_value(command: &str) -> Value {
    serde_json::to_value(command_catalog::lookup(command).expect("command schema exists"))
        .expect("schema serializes")
}

fn schema_child(parent: &str, child: &str) -> Value {
    command_catalog::children_of(parent)
        .into_iter()
        .find(|spec| spec.name == child)
        .map(|spec| serde_json::to_value(spec).expect("schema serializes"))
        .unwrap_or_else(|| panic!("missing child {child} under {parent}"))
}

fn awiki_cmd(args: &[&str]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_awiki-cli"));
    command
        .args(args)
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

fn assert_code(output: &Output, code: i32) {
    assert_eq!(
        output.status.code(),
        Some(code),
        "unexpected exit status; stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
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
