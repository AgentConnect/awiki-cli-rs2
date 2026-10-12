use std::process::Command;

#[test]
fn package_self_check_keeps_full_version_identity() {
    let output = Command::new(env!("CARGO_BIN_EXE_awiki-deamon"))
        .args([
            "__self-check",
            "--expected-version",
            env!("CARGO_PKG_VERSION"),
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let receipt: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(receipt["ok"], true);
    assert_eq!(receipt["version"], env!("CARGO_PKG_VERSION"));
    let rejected = Command::new(env!("CARGO_BIN_EXE_awiki-deamon"))
        .args(["__self-check", "--expected-version", "0.0.0-unmatched"])
        .output()
        .unwrap();
    assert!(!rejected.status.success());
}
