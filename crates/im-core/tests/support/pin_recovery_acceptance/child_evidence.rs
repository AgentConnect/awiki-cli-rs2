//! One evidence sink for native and Python child processes; raw streams never print.
use super::*;
use std::{
    io::Write,
    process::{Command, Output, Stdio},
};
use zeroize::Zeroizing;

pub(super) fn run(
    _input: &Input,
    label: &str,
    command: &mut Command,
    expected_exit: i32,
) -> Output {
    let output = command.output().expect("spawn exact acceptance child");
    let python = std::env::var_os("AWIKI_REALM_SYSTEM_AUDIT_PYTHON")
        .expect("System-owned evidence Python required");
    let root = std::env::var_os("AWIKI_REALM_SYSTEM_AUDIT_ROOT")
        .expect("System-owned evidence root required");
    let payload = serde_json::json!({"exitCode":output.status.code(),"stdout":String::from_utf8_lossy(&output.stdout),"stderr":String::from_utf8_lossy(&output.stderr)});
    let bytes = Zeroizing::new(serde_json::to_vec(&payload).expect("child evidence envelope"));
    let mut sink = Command::new(python)
        .current_dir(root)
        .args([
            "-m",
            "tests_v2.key_recovery.core_driver",
            "--record-child",
            label,
            &expected_exit.to_string(),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn redacted child evidence sink");
    sink.stdin
        .take()
        .expect("evidence sink stdin")
        .write_all(&bytes)
        .expect("child evidence pipe");
    assert!(
        sink.wait().expect("evidence sink completion").success(),
        "child evidence sink failed; preserve exact scope and parent failure"
    );
    assert_eq!(
        output.status.code(),
        Some(expected_exit),
        "acceptance child failed: {label}; inspect the retained redacted child failure evidence"
    );
    output
}
