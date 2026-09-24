use super::*;

#[test]
fn history_roundtrip_and_exact_version_hook() {
    // Source/registry builders remove their temporary checkout before executing
    // the compiled tests. Materialize the exact compiled fixture in owned storage.
    let root = tempfile::tempdir().unwrap();
    for (relative, source) in [
        (
            "tests/gemini_history.test.mjs",
            include_str!("../../../tests/gemini_history.test.mjs"),
        ),
        (
            "tests/fixtures/gemini_060_converter.js",
            include_str!("../../../tests/fixtures/gemini_060_converter.js"),
        ),
        (
            "tests/fixtures/gemini_060_history.json",
            include_str!("../../../tests/fixtures/gemini_060_history.json"),
        ),
        ("src/acp/gemini_replay.mjs", include_str!("../gemini_replay.mjs")),
    ] {
        let path = root.path().join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, source).unwrap();
    }
    let output = std::process::Command::new("node")
        .args(["--test", "tests/gemini_history.test.mjs"])
        .current_dir(root.path())
        .output()
        .expect("Node is required for Gemini compatibility tests");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn only_exact_unrecoverable_history_enters_reset_flow() {
    for (details, expected) in [
        ("awiki_gemini_history_unrecoverable", true),
        ("awiki_gemini_history_compatibility_mismatch", false),
        ("prefix awiki_gemini_history_unrecoverable", false),
    ] {
        let error = acp::Error::internal_error().data(json!({"details":details}));
        assert_eq!(
            missing_native_context(&error, Brand::Gemini, "session"),
            expected
        );
        assert!(!missing_native_context(&error, Brand::Kimi, "session"));
    }
    assert!(gemini_startup_missing(
        "Error resuming session: awiki_gemini_history_unrecoverable",
        "session"
    ));
    assert!(!gemini_startup_missing(
        "Error resuming session: awiki_gemini_history_compatibility_mismatch",
        "session"
    ));
}
