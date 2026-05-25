use awiki_cli::cmdmeta::{self, CutoverStatus};
use serde_json::Value;
use std::process::{Command, Output};

#[test]
fn every_command_spec_has_cutover_classification() {
    let missing: Vec<_> = cmdmeta::specs()
        .into_iter()
        .filter(|spec| cmdmeta::try_cutover_status(spec.name).is_none())
        .map(|spec| spec.name)
        .collect();

    assert!(
        missing.is_empty(),
        "every current command spec must be classified for CLI cutover: {missing:?}"
    );
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
        "id.profile.get",
        "id.profile.set",
        "msg.send",
        "msg.inbox",
        "msg.history",
        "msg.mark-read",
        "msg.attachment.download",
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
            cmdmeta::cutover_status(command),
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
        "config.set",
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
        "runtime.listener.config.show",
        "runtime.listener.config.set",
        "runtime.listener.enable",
        "runtime.listener.disable",
        "runtime.host-notify.config.show",
        "runtime.host-notify.config.set",
        "runtime.host-notify.enable",
        "runtime.host-notify.disable",
        "runtime.host-notify.hermes.guide",
        "runtime.host-notify.hermes.status",
        "runtime.host-notify.hermes.setup",
    ] {
        assert_eq!(
            cmdmeta::cutover_status(command),
            CutoverStatus::CliOwned,
            "{command} should remain CLI-owned"
        );
    }
}

#[test]
fn cutover_classifier_marks_unsupported_and_internal_commands() {
    assert_eq!(
        cmdmeta::cutover_status("msg.secure.status"),
        CutoverStatus::Unsupported {
            capability: "secure-direct",
            phase: "Phase 6",
        }
    );
    assert_eq!(
        cmdmeta::cutover_status("people.search"),
        CutoverStatus::Unsupported {
            capability: "people-directory",
            phase: "future people search API",
        }
    );
    assert_eq!(
        cmdmeta::cutover_status("runtime.heartbeat.status"),
        CutoverStatus::Unsupported {
            capability: "runtime-heartbeat",
            phase: "outside current im-core cutover",
        }
    );
    assert_eq!(
        cmdmeta::cutover_status("group.e2ee.publish-key-package"),
        CutoverStatus::DiagnosticOnly
    );
    assert_eq!(
        cmdmeta::cutover_status("debug.db.query"),
        CutoverStatus::Unsupported {
            capability: "raw-sql",
            phase: "outside current im-core cutover",
        }
    );
    assert_eq!(
        cmdmeta::cutover_status("runtime.host-notify.openclaw.set-token"),
        CutoverStatus::DiagnosticOnly
    );
    assert_eq!(
        cmdmeta::cutover_status("runtime.listener.run"),
        CutoverStatus::Hidden
    );
    assert_eq!(
        cmdmeta::cutover_status("group.code.get"),
        CutoverStatus::Removed
    );
    assert_eq!(
        cmdmeta::cutover_status("debug.raw.rpc"),
        CutoverStatus::Removed
    );
}

#[test]
fn schema_serializes_cutover_status_for_commands_and_children() {
    let msg_send = schema_value("msg.send");
    assert_eq!(msg_send["cutover"]["status"], "im_core");
    assert_eq!(msg_send["cutover"]["default_surface"], true);

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
}

#[test]
fn default_schema_surface_includes_only_cli_owned_and_im_core_commands() {
    let names: Vec<_> = cmdmeta::default_surface_specs()
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
        "mail.inbox",
        "mail.attachment.download",
        "group.create",
        "people.follow",
        "people.contacts.list",
        "page.list",
        "page.create",
        "site.root.get",
        "site.page.list",
        "runtime.listener.start",
        "runtime.host-notify.hermes.setup",
    ] {
        assert!(
            names.contains(&command),
            "{command} should remain on the default schema/help surface"
        );
    }

    for command in [
        "msg.secure.status",
        "people.search",
        "runtime.heartbeat.status",
        "group.e2ee.publish-key-package",
        "debug.db.query",
        "runtime.host-notify.openclaw.set-token",
        "runtime.listener.run",
        "group.code.get",
        "debug.raw.rpc",
    ] {
        assert!(
            !names.contains(&command),
            "{command} should not be advertised on the default schema/help surface"
        );
        assert!(
            cmdmeta::lookup(command).is_some(),
            "{command} should remain queryable by exact schema target"
        );
    }
}

#[test]
fn unsupported_cutover_error_has_stable_contract() {
    let err = awiki_cli::app::unsupported::unsupported_cutover_command(
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

#[test]
fn unsupported_non_im_domains_do_not_enter_legacy_handlers() {
    for (args, command, capability) in [(
        &["debug", "db", "query", "SELECT 1"][..],
        "debug.db.query",
        "raw-sql",
    )] {
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
}

fn schema_value(command: &str) -> Value {
    serde_json::to_value(cmdmeta::lookup(command).expect("command schema exists"))
        .expect("schema serializes")
}

fn schema_child(parent: &str, child: &str) -> Value {
    cmdmeta::children_of(parent)
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
