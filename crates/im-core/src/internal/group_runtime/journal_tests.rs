use super::*;
#[test]
fn retries_reuse_exact_intent_and_cannot_change_security_or_cross_owner_device() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("journal.sqlite");
    let mut db = Connection::open(&path).unwrap();
    db.execute_batch(SCHEMA).unwrap();
    let first = serde_json::json!({"operation_id":"first", "security":"group-e2ee"});
    freeze_on(
        &mut db,
        "alice",
        "a1",
        "create:one",
        "encrypted",
        first.clone(),
    )
    .unwrap();
    drop(db);
    let mut db = Connection::open(&path).unwrap();
    assert_eq!(
        freeze_on(
            &mut db,
            "alice",
            "a1",
            "create:one",
            "encrypted",
            Value::Null
        )
        .unwrap(),
        first
    );
    assert!(freeze_on(&mut db, "alice", "a1", "create:one", "plain", Value::Null).is_err());
    assert_eq!(
        freeze_on(
            &mut db,
            "bob",
            "a1",
            "create:one",
            "plain",
            Value::Bool(false)
        )
        .unwrap(),
        Value::Bool(false)
    );
    assert_eq!(
        freeze_on(
            &mut db,
            "alice",
            "a2",
            "create:one",
            "encrypted",
            Value::Bool(true)
        )
        .unwrap(),
        Value::Bool(true)
    );
}
