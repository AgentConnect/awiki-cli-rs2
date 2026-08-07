use std::fs;
use std::path::{Path, PathBuf};

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
fn im_core_adapter_has_no_legacy_bridge_needles() {
    let offenders: Vec<_> = adapter_rs_files()
        .into_iter()
        .filter(|path| path.file_name().is_some_and(|name| name != "tests.rs"))
        .filter_map(|path| {
            let text = fs::read_to_string(&path).expect("read adapter source");
            let has_bridge = LEGACY_BRIDGE_NEEDLES
                .iter()
                .any(|needle| text.contains(needle));
            has_bridge.then_some(path)
        })
        .collect();

    assert!(
        offenders.is_empty(),
        "legacy bridge references must not remain in im_core_adapter default cutover boundary: {offenders:?}"
    );
}

#[test]
fn stable_boundary_modules_do_not_use_legacy_bridge_needles() {
    for relative in [
        "core_config.rs",
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
    let err = awiki_cli::m_core_cli_adapter::unsupported_cutover_command(
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
        .join("host_runtime")
        .join("listener_supervisor_run.rs");
    let text = fs::read_to_string(&path).expect("read runtime listener supervisor");
    for forbidden in [
        concat!("im_core::", "compat::realtime::run_realtime_transport"),
        concat!("im_core::", "compat::realtime::RealtimeRunnerEventSink"),
        concat!("im_core::", "compat::realtime::RealtimeRunnerTransport"),
    ] {
        assert!(
            !text.contains(forbidden),
        "host runtime listener should use im_core::prelude realtime runner API, not {forbidden}"
        );
    }
    assert!(
        text.contains(".realtime()")
            && text.contains(".start_async(")
            && text.contains("im_core_realtime_adapter::listener_realtime_options()"),
        "host runtime listener should call the public async realtime service API outside compat"
    );
    assert!(
        text.contains(".sync_now_async(") && text.contains("im_core::messages::MessageSyncRequest"),
        "host runtime listener should converge through the public v2 reliable-sync API"
    );
    assert!(
        !text.contains("sync_delta"),
        "the first-party CLI listener must not call the v1 compatibility reader"
    );
}

const LEGACY_BRIDGE_NEEDLES: &[&str] = &[
    concat!("im_core::", "compat"),
    concat!("use crate::", "message"),
    concat!("crate::", "message::"),
    concat!("message", "::"),
    "identity::register(",
    "identity::register_plan(",
    "identity::refresh_token(",
    "host_runtime::listener_",
];

fn adapter_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("m_core_cli_adapter")
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
