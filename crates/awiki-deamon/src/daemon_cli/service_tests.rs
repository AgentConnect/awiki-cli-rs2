use super::*;

#[test]
fn no_service_and_foreground_keep_compatible_output_without_service_management() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    for (foreground, no_service, detail) in [
        (false, true, "service installation skipped by --no-service"),
        (true, false, "foreground mode requested"),
        (true, true, "service installation skipped by --no-service"),
    ] {
        let service = install_daemon_service(&config, foreground, no_service).unwrap();
        assert_eq!(service.platform, ServicePlatform::Foreground);
        assert!(!service.installed && !service.running);
        assert!(service.unit_path.is_none());
        assert_eq!(service.detail.as_deref(), Some(detail));
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
    }
}
