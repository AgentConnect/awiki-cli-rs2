use super::*;

#[test]
fn macos_management_is_rejected_without_touching_windows_registration() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let executable = root.path().join("awiki-deamon.exe");
    for action in [
        ServiceAction::Status,
        ServiceAction::Install,
        ServiceAction::Restart,
    ] {
        let error = manage(&config, &executable, action).unwrap_err();
        assert!(error.to_string().contains("unavailable on this platform"));
    }
    assert!(restart_after_upgrade(&config, &executable).is_err());
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
}
