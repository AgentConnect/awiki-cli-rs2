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
