use std::fs;
use std::path::{Path, PathBuf};

const TEMPORARY_BRIDGE_MARKER: &str = "Temporary migration-only legacy bridge exception.";
const DELETE_MARKER: &str = "Delete in PR";

#[test]
fn im_core_adapter_documents_thin_boundary_policy() {
    let policy = fs::read_to_string(adapter_root().join("mod.rs")).expect("adapter mod policy");

    assert!(
        policy.contains("CLI boundary adapter for `im-core`"),
        "adapter module should document its boundary role"
    );
    assert!(
        policy.contains("No new legacy business bridge may be added here"),
        "adapter module should reject new legacy bridges"
    );
    assert!(
        policy.contains("C2, C3, C4, C5, and C7"),
        "adapter module should name cleanup slices"
    );
}

#[test]
fn legacy_bridge_exceptions_are_marked_temporary_with_cleanup_prs() {
    let offenders: Vec<_> = adapter_rs_files()
        .into_iter()
        .filter(|path| path.file_name().is_some_and(|name| name != "tests.rs"))
        .filter_map(|path| {
            let text = fs::read_to_string(&path).expect("read adapter source");
            let has_bridge = LEGACY_BRIDGE_NEEDLES
                .iter()
                .any(|needle| text.contains(needle));
            let has_marker = text.contains(TEMPORARY_BRIDGE_MARKER) && text.contains(DELETE_MARKER);
            (has_bridge && !has_marker).then_some(path)
        })
        .collect();

    assert!(
        offenders.is_empty(),
        "legacy bridge references in im_core_adapter must be temporary migration-only exceptions with cleanup PRs: {offenders:?}"
    );
}

#[test]
fn stable_boundary_modules_do_not_use_legacy_bridge_needles() {
    for relative in [
        "config.rs",
        "core.rs",
        "error.rs",
        "paths.rs",
        "render.rs",
        "unsupported.rs",
    ] {
        let path = adapter_root().join(relative);
        let text = fs::read_to_string(&path).expect("read stable adapter source");
        let found: Vec<_> = LEGACY_BRIDGE_NEEDLES
            .iter()
            .copied()
            .filter(|needle| text.contains(needle))
            .collect();
        assert!(
            found.is_empty(),
            "stable boundary module {relative} should not contain legacy bridge references: {found:?}"
        );
    }
}

#[test]
fn adapter_unsupported_cutover_error_has_stable_contract() {
    let err = awiki_cli::im_core_adapter::unsupported_cutover_command(
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
    assert!(err.detail.message.contains("attachments"));
    assert!(err.detail.hint.contains("Phase 4"));
}

#[test]
fn runtime_listener_host_uses_public_realtime_runner_api() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("runtime")
        .join("listener_supervisor_run.rs");
    let text = fs::read_to_string(&path).expect("read runtime listener supervisor");
    for forbidden in [
        "im_core::compat::realtime::run_realtime_transport",
        "im_core::compat::realtime::RealtimeRunnerEventSink",
        "im_core::compat::realtime::RealtimeRunnerTransport",
    ] {
        assert!(
            !text.contains(forbidden),
            "runtime listener host should use im_core::prelude realtime runner API, not {forbidden}"
        );
    }
    assert!(
        text.contains("im_core::realtime::run_realtime_transport_with_event_sink_until_shutdown"),
        "runtime listener host should call the public realtime runner API outside compat"
    );
}

const LEGACY_BRIDGE_NEEDLES: &[&str] = &[
    "im_core::compat",
    "use crate::message",
    "crate::message::",
    "message::",
    "identity::register(",
    "identity::register_plan(",
    "identity::refresh_token(",
    "runtime::listener_",
];

fn adapter_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("im_core_adapter")
}

fn adapter_rs_files() -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_rs_files(&adapter_root(), &mut files);
    files
}

fn collect_rs_files(dir: &Path, files: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("read adapter dir") {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            collect_rs_files(&path, files);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            files.push(path);
        }
    }
}
