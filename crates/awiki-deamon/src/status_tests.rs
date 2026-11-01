use super::*;

#[tokio::test]
async fn initialized_status_does_not_compete_with_active_database_writers() {
    let root = tempfile::tempdir().unwrap();
    let initial = initialize_and_report(root.path().to_path_buf())
        .await
        .unwrap();
    let config = DaemonConfig::for_state_root(root.path()).unwrap();
    let daemon_writer = rusqlite::Connection::open(&config.daemon_db_path).unwrap();
    daemon_writer.execute_batch("BEGIN IMMEDIATE").unwrap();
    let core_writer = rusqlite::Connection::open(&config.im_core_sqlite_path).unwrap();
    core_writer.execute_batch("BEGIN IMMEDIATE").unwrap();

    let inspected = run_command_async(DaemonCommand::Status {
        state_root: root.path().to_path_buf(),
    })
    .await;
    core_writer.execute_batch("ROLLBACK").unwrap();
    daemon_writer.execute_batch("ROLLBACK").unwrap();

    let DaemonCommandOutput::Status(status) = inspected
        .expect("status must only inspect initialized stores while foreground owns the writer")
    else {
        panic!("status returned a different command result");
    };
    assert_eq!(status.daemon_schema_version, initial.daemon_schema_version);
    assert_eq!(
        status.im_core_schema_version,
        initial.im_core_schema_version
    );
    assert_eq!(status.state_root, initial.state_root);
    assert!(!status.sync_probe.v2_bootstrap_completed);
}
