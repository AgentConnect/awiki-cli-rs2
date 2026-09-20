use super::*;

#[cfg(unix)]
fn fixture(script: &str) -> (tempfile::TempDir, PathBuf) {
    let root = tempfile::tempdir().unwrap();
    let binary = super::tests::executable(root.path(), "hermes-fixture", script);
    (root, binary)
}

#[cfg(unix)]
#[test]
fn native_hermes_inspection_uses_only_version_and_dependency_check() {
    let (root, binary) = fixture(
        r#"[ "$1" = acp ] || exit 90
case "$2" in
--version) echo 0.18.2 ;;
--check) echo 'Hermes ACP check OK' ;;
*) exit 91 ;;
esac
"#,
    );
    let item = inspect_client("hermes", Some(binary), Instant::now() + ITEM_TIMEOUT);
    assert_eq!(item.status, "ready");
    assert_eq!(item.version.as_deref(), Some("0.18.2"));
    assert_eq!(item.execution_protocol, "acp");
    assert!(item.adapter_version.is_none());
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
}

#[cfg(unix)]
#[test]
fn installation_without_native_acp_dependencies_is_not_advertised_as_ready() {
    let (_root, binary) = fixture(
        r#"[ "$2" = --version ] && { echo 0.18.2; exit 0; }
echo 'private details must not reach UI' >&2
exit 2
"#,
    );
    let item = inspect_client("hermes", Some(binary), Instant::now() + ITEM_TIMEOUT);
    assert_eq!(item.status, "unavailable");
    assert_eq!(item.reason_code, Some("acp_dependencies_missing"));
    assert!(!serde_json::to_string(&item)
        .unwrap()
        .contains("private details"));
}

#[cfg(unix)]
#[test]
fn native_hermes_probe_has_a_shared_deadline_and_no_login_or_setup() {
    let (_root, binary) = fixture(
        r#"[ "$2" = --version ] && { echo 0.18.2; exit 0; }
sleep 120
"#,
    );
    let start = Instant::now();
    let item = inspect_client("hermes", Some(binary), start + Duration::from_millis(100));
    assert_eq!(item.status, "unknown");
    assert_eq!(item.reason_code, Some("timeout"));
    assert!(start.elapsed() < Duration::from_secs(2));
}
