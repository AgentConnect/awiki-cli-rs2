use super::*;

#[cfg(feature = "group-e2ee")]
fn change_fixture(db: &Connection) {
    db.execute_batch(SCHEMA).unwrap();
    db.execute_batch("CREATE TABLE groups (owner_identity_id TEXT, group_id TEXT, my_role TEXT, membership_status TEXT, metadata TEXT, PRIMARY KEY(owner_identity_id,group_id));").unwrap();
    for (owner, role, status, profile) in [
        ("owner", "owner", "active", "group-e2ee"),
        ("member", "member", "active", "group-e2ee"),
        ("left", "owner", "left", "group-e2ee"),
        ("plain", "owner", "active", "transport-protected"),
        ("unknown", "owner", "active", ""),
    ] {
        db.execute(
            "INSERT INTO groups VALUES (?1,'did:example:group',?2,?3,?4)",
            (
                owner,
                role,
                status,
                serde_json::json!({"required_security_profile":profile}).to_string(),
            ),
        )
        .unwrap();
    }
}

#[cfg(feature = "group-e2ee")]
fn change_progress(db: &Connection, device: &str) -> Value {
    let raw: String = db.query_row(
        "SELECT intent_json FROM group_operation_journal WHERE owner_identity_id='owner' AND local_device_id=?1 AND operation_key='recovery:repair:did:example:group'",
        [device], |row| row.get(0),
    ).unwrap();
    serde_json::from_str(&raw).unwrap()
}

#[cfg(feature = "group-e2ee")]
#[test]
fn group_change_recovery_commits_with_projection_and_survives_restart() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("changes.sqlite");
    let mut db = Connection::open(&path).unwrap();
    change_fixture(&db);
    {
        let tx = db.transaction().unwrap();
        assert!(schedule_group_state_change_on(
            &tx,
            "owner",
            "device",
            "did:example:group",
            "event-1"
        )
        .unwrap());
        tx.rollback().unwrap();
    }
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM group_operation_journal", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    let tx = db.transaction().unwrap();
    assert!(
        schedule_group_state_change_on(&tx, "owner", "device", "did:example:group", "event-1")
            .unwrap()
    );
    tx.commit().unwrap();
    drop(db);
    let db = Connection::open(path).unwrap();
    let observed = change_progress(&db, "device");
    assert_eq!(observed["phase"], "preparing");
    assert_eq!(observed["next_attempt_at"], 0);
    assert!(!schedule_group_state_change_on(
        &db,
        "owner",
        "device",
        "did:example:group",
        "event-1"
    )
    .unwrap());
    assert_eq!(change_progress(&db, "device"), observed);
}

#[cfg(feature = "group-e2ee")]
#[test]
fn older_group_repair_cannot_complete_or_delay_a_new_change() {
    let db = Connection::open_in_memory().unwrap();
    change_fixture(&db);
    let key = "recovery:repair:did:example:group";
    schedule_group_state_change_on(&db, "owner", "device", "did:example:group", "event-1").unwrap();
    let observed = change_progress(&db, "device");
    schedule_group_state_change_on(&db, "owner", "device", "did:example:group", "event-2").unwrap();
    let current = change_progress(&db, "device");
    for phase in ["completed", "retry"] {
        let mut stale_completion = observed.clone();
        stale_completion["phase"] = serde_json::json!(phase);
        stale_completion["next_attempt_at"] = serde_json::json!(99999);
        assert!(
            !replace_if_unchanged_on(&db, "owner", "device", key, &observed, stale_completion)
                .unwrap()
        );
        assert_eq!(change_progress(&db, "device"), current);
    }
    let mut completed = current.clone();
    completed["phase"] = serde_json::json!("completed");
    assert!(
        replace_if_unchanged_on(&db, "owner", "device", key, &current, completed.clone()).unwrap()
    );
    assert!(!schedule_group_state_change_on(
        &db,
        "owner",
        "device",
        "did:example:group",
        "event-2"
    )
    .unwrap());
    assert_eq!(change_progress(&db, "device"), completed);
}

#[cfg(feature = "group-e2ee")]
#[test]
fn group_change_does_not_grant_owner_authority_or_cross_device_scope() {
    let db = Connection::open_in_memory().unwrap();
    change_fixture(&db);
    // Partial group projections legitimately have no known role. They must not
    // stop the enclosing sync transaction or acquire owner maintenance rights.
    db.execute(
        "INSERT INTO groups VALUES ('unknown-role','did:example:group',NULL,'active',?1)",
        [serde_json::json!({"required_security_profile":"group-e2ee"}).to_string()],
    )
    .unwrap();
    db.execute(
        "INSERT INTO groups VALUES ('unknown-policy','did:example:group','owner','active',NULL)",
        [],
    )
    .unwrap();
    for owner in [
        "member",
        "left",
        "plain",
        "unknown",
        "missing",
        "unknown-role",
        "unknown-policy",
    ] {
        assert!(!schedule_group_state_change_on(
            &db,
            owner,
            "device",
            "did:example:group",
            "event-1"
        )
        .unwrap());
    }
    let scheduled: i64 = db
        .query_row("SELECT count(*) FROM group_operation_journal", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(scheduled, 0);
    for device in ["a", "b"] {
        assert!(schedule_group_state_change_on(
            &db,
            "owner",
            device,
            "did:example:group",
            "event-1"
        )
        .unwrap());
    }
    let before_b = change_progress(&db, "b");
    schedule_group_state_change_on(&db, "owner", "a", "did:example:group", "event-2").unwrap();
    assert_eq!(change_progress(&db, "b"), before_b);
    assert_eq!(change_progress(&db, "a")["change_generation"], 2);
}

#[test]
fn lazy_group_intent_produces_once_across_connections_and_reopen() {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Barrier,
    };
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("send.sqlite");
    Connection::open(&path)
        .unwrap()
        .execute_batch(SCHEMA)
        .unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let barrier = Arc::new(Barrier::new(2));
    let handles: Vec<_> = (0..2)
        .map(|_| {
            let (path, calls, barrier) = (path.clone(), calls.clone(), barrier.clone());
            std::thread::spawn(move || {
                let mut db = Connection::open(path).unwrap();
                db.busy_timeout(std::time::Duration::from_secs(5)).unwrap();
                barrier.wait();
                freeze_on_with(&mut db, "owner", "device", "send:op", "binding", || {
                    calls.fetch_add(1, Ordering::SeqCst);
                    std::thread::sleep(std::time::Duration::from_millis(20));
                    Ok(serde_json::json!({"cipher":"fixed","created_at":"original"}))
                })
                .unwrap()
            })
        })
        .collect();
    let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    assert_eq!(results[0], results[1]);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let mut db = Connection::open(path).unwrap();
    let resumed = freeze_on_with(&mut db, "owner", "device", "send:op", "binding", || {
        panic!("must not encrypt again")
    })
    .unwrap();
    assert_eq!(resumed, results[0]);
    assert!(
        freeze_on_with(&mut db, "owner", "device", "send:op", "changed", || panic!(
            "conflict must precede encryption"
        ))
        .is_err()
    );
}

#[test]
fn lazy_group_intent_never_exposes_ciphertext_before_persistence() {
    let mut db = Connection::open_in_memory().unwrap();
    db.execute_batch(SCHEMA).unwrap();
    db.execute_batch("CREATE TRIGGER fail_send BEFORE INSERT ON group_operation_journal BEGIN SELECT RAISE(ABORT,'synthetic disk failure'); END;").unwrap();
    let mut encrypted = 0;
    assert!(
        freeze_on_with(&mut db, "owner", "device", "send:op", "binding", || {
            encrypted += 1;
            Ok(serde_json::json!({"cipher":"never submitted"}))
        })
        .is_err()
    );
    assert_eq!(encrypted, 1);
    let rows: i64 = db
        .query_row("SELECT count(*) FROM group_operation_journal", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(rows, 0);
    db.execute_batch("DROP TRIGGER fail_send;").unwrap();
    assert!(
        freeze_on_with(&mut db, "owner", "device", "send:op", "binding", || Err(
            crate::ImError::PermissionDenied
        ))
        .is_err()
    );
    let result = freeze_on_with(&mut db, "owner", "device", "send:op", "binding", || {
        Ok(serde_json::json!({"cipher":"first eligible submission"}))
    })
    .unwrap();
    assert_eq!(result["cipher"], "first eligible submission");
}

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
        let tx = db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        schedule_key_package_replenishment_on(&tx, "alice", "a1", "welcome-1").unwrap();
        tx.rollback().unwrap();
    }
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM group_operation_journal", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    let tx = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .unwrap();
    schedule_key_package_replenishment_on(&tx, "alice", "a1", "welcome-1").unwrap();
    save_on(
        &tx,
        "alice",
        "a1",
        "key-package-replenish:welcome-1",
        serde_json::json!({"completed":true}),
    )
    .unwrap();
    save_on(
        &tx,
        "alice",
        "a1",
        "device-preparation-status",
        serde_json::json!({"next_attempt_at":300}),
    )
    .unwrap();
    schedule_key_package_replenishment_on(&tx, "alice", "a1", "welcome-1").unwrap();
    tx.commit().unwrap();
    let records: Vec<String> = db
        .prepare("SELECT intent_json FROM group_operation_journal ORDER BY operation_key")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(records.len(), 2);
    assert_eq!(
        serde_json::from_str::<Value>(&records[0]).unwrap()["next_attempt_at"],
        300
    );
    assert_eq!(
        serde_json::from_str::<Value>(&records[1]).unwrap()["completed"],
        true
    );
    schedule_key_package_replenishment_on(&db, "alice", "a2", "welcome-1").unwrap();
    assert_eq!(
        db.query_row(
            "SELECT COUNT(*) FROM group_operation_journal WHERE local_device_id='a2'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        2
    );
}
