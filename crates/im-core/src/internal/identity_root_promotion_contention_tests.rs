use super::*;
use std::sync::mpsc;
use std::time::Duration;

fn promotion_with_writer(changed_binding: bool) -> (crate::ImResult<()>, String, String) {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("promotion.sqlite");
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .pragma_update(None, "journal_mode", "WAL")
        .unwrap();
    connection.busy_timeout(Duration::from_secs(5)).unwrap();
    connection
        .execute_batch(
            r#"
CREATE TABLE identity_root_import_completion_v1 (
    owner_identity_id TEXT, local_device_id TEXT, message_id TEXT,
    pending_root_ref_json TEXT, phase TEXT, updated_at TEXT, last_error_code TEXT
);
INSERT INTO identity_root_import_completion_v1 VALUES
    ('owner-a', 'device-a', 'message-a', 'pending-ref', 'registry_confirmed', 'before', NULL);
CREATE TABLE identity_account_bindings (
    owner_identity_id TEXT PRIMARY KEY, device_id TEXT,
    device_auth_generation TEXT, updated_at INTEGER
);
INSERT INTO identity_account_bindings VALUES ('owner-a', 'device-a', '1', 1);
"#,
        )
        .unwrap();
    let (locked_tx, locked_rx) = mpsc::channel();
    let writer = std::thread::spawn(move || {
        let db = rusqlite::Connection::open(path).unwrap();
        db.busy_timeout(Duration::from_secs(5)).unwrap();
        db.execute_batch("BEGIN IMMEDIATE").unwrap();
        if changed_binding {
            db.execute_batch("UPDATE identity_account_bindings SET device_auth_generation='3'")
                .unwrap();
        } else {
            db.execute_batch("UPDATE identity_account_bindings SET updated_at=2")
                .unwrap();
        }
        locked_tx.send(()).unwrap();
        std::thread::sleep(Duration::from_millis(75));
        db.execute_batch("COMMIT").unwrap();
    });
    locked_rx.recv().unwrap();
    let result = mark_coordinator_promoted(
        &connection,
        "owner-a",
        "device-a",
        "message-a",
        "pending-ref",
        2,
    );
    writer.join().unwrap();
    let generation = connection
        .query_row(
            "SELECT device_auth_generation FROM identity_account_bindings",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let phase = connection
        .query_row(
            "SELECT phase FROM identity_root_import_completion_v1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    (result, generation, phase)
}

#[test]
fn promotion_waits_for_existing_writer_before_reading_binding() {
    let (result, generation, phase) = promotion_with_writer(false);
    result.unwrap();
    assert_eq!(generation, "2");
    assert_eq!(phase, "promoted");
}

#[test]
fn promotion_rechecks_committed_generation_without_rolling_it_back() {
    let (result, generation, phase) = promotion_with_writer(true);
    assert!(matches!(
        result,
        Err(crate::ImError::IdentityBindingConflict { .. })
    ));
    assert_eq!(generation, "3");
    assert_eq!(phase, "registry_confirmed");
}
