use awiki_deamon::{run_command, DaemonCommand, DaemonConfig, DaemonState};

const EXPECTED_DAEMON_SCHEMA_VERSION: i64 = 23;

#[test]
fn init_state_creates_daemon_and_im_core_databases() {
    let root = tempfile::tempdir().unwrap();
    let status = run_command(DaemonCommand::InitState {
        state_root: root.path().to_path_buf(),
    })
    .unwrap();

    assert_eq!(status.state_root, root.path());
    assert!(status.database_path.exists());
    assert!(status.im_core_sqlite_path.exists());
    assert!(root.path().join("identity").is_dir());
    assert!(root.path().join("runtime").join("cache").is_dir());
    assert!(root.path().join("runtime").join("tmp").is_dir());
    assert!(root.path().join("rpc").is_dir());
    assert!(root.path().join("audit").is_dir());
    assert_eq!(status.daemon_schema_version, EXPECTED_DAEMON_SCHEMA_VERSION);
    assert!(status.im_core_schema_version.is_some());
}

#[test]
fn status_initializes_existing_state_idempotently() {
    let root = tempfile::tempdir().unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let first = DaemonState::open(&config).unwrap().initialize().unwrap();
    let second = run_command(DaemonCommand::Status {
        state_root: root.path().to_path_buf(),
    })
    .unwrap();

    assert_eq!(first.database_path, second.database_path);
    assert_eq!(second.daemon_schema_version, EXPECTED_DAEMON_SCHEMA_VERSION);
}
