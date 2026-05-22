use awiki_cli::cmdmeta::{self, CutoverStatus};
use serde_json::Value;

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
        cmdmeta::cutover_status("msg.attachment.download"),
        CutoverStatus::Unsupported {
            capability: "attachments",
            phase: "Phase 4",
        }
    );
    assert_eq!(
        cmdmeta::cutover_status("msg.secure.status"),
        CutoverStatus::Unsupported {
            capability: "secure-direct",
            phase: "Phase 6",
        }
    );
    assert_eq!(
        cmdmeta::cutover_status("mail.inbox"),
        CutoverStatus::Unsupported {
            capability: "mail",
            phase: "outside current im-core cutover",
        }
    );
    assert_eq!(
        cmdmeta::cutover_status("people.search"),
        CutoverStatus::Unsupported {
            capability: "people-directory",
            phase: "future directory/relation API",
        }
    );
    assert_eq!(
        cmdmeta::cutover_status("page.list"),
        CutoverStatus::Unsupported {
            capability: "page-site",
            phase: "outside current im-core cutover",
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
        CutoverStatus::DiagnosticOnly
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
    assert_eq!(attachment["cutover"]["status"], "unsupported");
    assert_eq!(attachment["cutover"]["capability"], "attachments");
    assert_eq!(attachment["cutover"]["required_phase"], "Phase 4");
    assert_eq!(attachment["cutover"]["default_surface"], false);

    let runtime = schema_value("runtime.listener");
    let run = schema_child("runtime.listener", "runtime.listener.run");
    assert_eq!(runtime["cutover"]["status"], "cli_owned");
    assert_eq!(run["cutover"]["status"], "hidden");
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
