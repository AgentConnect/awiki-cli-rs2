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

#[test]
fn partial_invites_and_backoff_survive_restart_without_overwriting_other_members_or_devices() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("progress.sqlite");
    let db = Connection::open(&path).unwrap();
    db.execute_batch(SCHEMA).unwrap();
    for (device, member) in [("a1", "bob"), ("a1", "carol"), ("a2", "bob")] {
        save_on(&db, "alice", device, &format!("recovery:member:g:{member}"),
            serde_json::json!({"phase":"submitted", "request":{"member":member}, "next_attempt_at":123})).unwrap();
    }
    save_on(
        &db,
        "alice",
        "a1",
        "recovery:member:g:bob",
        serde_json::json!({"phase":"preparing", "group_did":"g", "business_committed":true,
            "encryption_ready":false, "attempts":2, "next_attempt_at":456}),
    )
    .unwrap();
    drop(db);
    let db = Connection::open(path).unwrap();
    let values: Vec<String> = db.prepare("SELECT intent_json FROM group_operation_journal ORDER BY local_device_id, operation_key").unwrap()
        .query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect();
    assert_eq!(values.len(), 3);
    let partial: Value = serde_json::from_str(&values[0]).unwrap();
    assert_eq!(partial["group_did"], "g");
    assert_eq!(partial["business_committed"], true);
    assert_eq!(partial["encryption_ready"], false);
    assert_eq!(partial["next_attempt_at"], 456);
    for untouched in &values[1..] {
        assert_eq!(
            serde_json::from_str::<Value>(untouched).unwrap()["phase"],
            "submitted"
        );
    }
}

#[test]
fn welcome_replenishment_commits_with_sync_and_replay_keeps_completed_progress() {
    let root = tempfile::tempdir().unwrap();
    let mut db = Connection::open(root.path().join("welcome.sqlite")).unwrap();
    db.execute_batch(SCHEMA).unwrap();
    {
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate).unwrap();
        schedule_key_package_replenishment_on(&tx, "alice", "a1", "welcome-1").unwrap();
        tx.rollback().unwrap();
    }
    assert_eq!(db.query_row("SELECT COUNT(*) FROM group_operation_journal", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate).unwrap();
    schedule_key_package_replenishment_on(&tx, "alice", "a1", "welcome-1").unwrap();
    save_on(&tx, "alice", "a1", "key-package-replenish:welcome-1", serde_json::json!({"completed":true})).unwrap();
    save_on(&tx, "alice", "a1", "device-preparation-status", serde_json::json!({"next_attempt_at":300})).unwrap();
    schedule_key_package_replenishment_on(&tx, "alice", "a1", "welcome-1").unwrap();
    tx.commit().unwrap();
    let records: Vec<String> = db.prepare("SELECT intent_json FROM group_operation_journal ORDER BY operation_key").unwrap()
        .query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect();
    assert_eq!(records.len(), 2);
    assert_eq!(serde_json::from_str::<Value>(&records[0]).unwrap()["next_attempt_at"], 300);
    assert_eq!(serde_json::from_str::<Value>(&records[1]).unwrap()["completed"], true);
    schedule_key_package_replenishment_on(&db, "alice", "a2", "welcome-1").unwrap();
    assert_eq!(db.query_row("SELECT COUNT(*) FROM group_operation_journal WHERE local_device_id='a2'", [], |r| r.get::<_, i64>(0)).unwrap(), 2);
}
