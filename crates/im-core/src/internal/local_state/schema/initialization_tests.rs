use super::ensure_schema;
use rusqlite::Connection;

fn snapshot(db: &Connection) -> Vec<(String, String, String)> {
    db.prepare("SELECT type, name, COALESCE(sql, '') FROM sqlite_master ORDER BY type, name")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
}

#[test]
fn fresh_schema_failure_rolls_back_new_objects_and_preserves_existing_state() {
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch("CREATE VIEW direct_peer_routes AS SELECT 'preserved' AS marker")
        .unwrap();
    let before = snapshot(&db);
    assert!(
        ensure_schema(&db).is_err(),
        "a view cannot receive the route indexes"
    );
    assert!(db.is_autocommit());
    assert_eq!(
        db.query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        snapshot(&db),
        before,
        "failed initialization must not retain partial tables"
    );
    db.execute_batch("DROP VIEW direct_peer_routes").unwrap();
    ensure_schema(&db).unwrap();
    assert!(
        db.query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
            .unwrap()
            > 0
    );
}

#[test]
fn fresh_schema_survives_reopen_and_repeated_ensure() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("state.sqlite");
    let before;
    {
        let db = Connection::open(&path).unwrap();
        ensure_schema(&db).unwrap();
        assert!(db.is_autocommit());
        before = snapshot(&db);
    }
    let db = Connection::open(&path).unwrap();
    ensure_schema(&db).unwrap();
    ensure_schema(&db).unwrap();
    assert_eq!(snapshot(&db), before);
}

#[test]
fn fresh_schema_respects_the_callers_existing_transaction() {
    let mut db = Connection::open_in_memory().unwrap();
    let transaction = db.transaction().unwrap();
    ensure_schema(&transaction).unwrap();
    assert!(!transaction.is_autocommit());
    assert!(!snapshot(&transaction).is_empty());
    transaction.rollback().unwrap();
    assert!(snapshot(&db).is_empty());
    assert_eq!(
        db.query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        0
    );
}
