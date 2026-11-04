use super::*;
use rusqlite::TransactionBehavior;
use std::{cell::RefCell, sync::mpsc, thread, time::Duration};

thread_local! {
    static WRITER_WAIT: RefCell<Option<mpsc::Sender<bool>>> = const { RefCell::new(None) };
}

fn notify_writer_wait(attempt: i32) -> bool {
    WRITER_WAIT.with(|sender| {
        if let Some(sender) = sender.borrow_mut().take() {
            let _ = sender.send(true);
        }
    });
    thread::sleep(Duration::from_millis(1));
    attempt < 5_000
}

fn lookup(did: &str, generation: &str) -> crate::directory::HandleLookupResult {
    crate::directory::HandleLookupResult {
        handle: crate::ids::Handle::parse("alice.awiki.info", "").unwrap(),
        did: crate::ids::Did::parse(did).unwrap(),
        user_id: "user-alice".to_owned(),
        domain: Some("awiki.info".to_owned()),
        status: Some("active".to_owned()),
        binding_generation: Some(generation.to_owned()),
        profile: None,
        warnings: Vec::new(),
    }
}

#[test]
fn verified_projection_waits_for_writer_before_reading_and_writing() {
    assert_projection_after_writer(false);
}

#[test]
fn verified_projection_rechecks_committed_generation_after_writer() {
    assert_projection_after_writer(true);
}

fn assert_projection_after_writer(stale_lookup: bool) {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("peer-projection.sqlite");
    let mut writer = super::super::open_writable(&path).unwrap();
    let canonical = project_verified_handle(
        &mut writer,
        "owner",
        "did:example:owner",
        &lookup("did:example:alice", "1"),
    )
    .unwrap();
    let mut receiver = super::super::open_writable(&path).unwrap();
    let transaction = writer
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .unwrap();
    if stale_lookup {
        transaction
            .execute(
                "UPDATE peer_personas SET binding_generation='2' WHERE owner_identity_id='owner'",
                [],
            )
            .unwrap();
    } else {
        transaction.execute(
            "UPDATE peer_personas SET verified_at='writer-committed' WHERE owner_identity_id='owner'", [],
        ).unwrap();
    }
    let (signal, observed) = mpsc::channel();
    let worker = thread::spawn(move || {
        WRITER_WAIT.with(|sender| *sender.borrow_mut() = Some(signal.clone()));
        receiver.busy_handler(Some(notify_writer_wait)).unwrap();
        let next = if stale_lookup {
            lookup("did:example:alice", "1")
        } else {
            lookup("did:example:alice-next", "2")
        };
        let result = project_verified_handle(&mut receiver, "owner", "did:example:owner", &next);
        // A deferred WAL snapshot upgrade fails without invoking busy_handler.
        let _ = signal.send(false);
        result
    });
    let waited = observed.recv_timeout(Duration::from_secs(10)).unwrap();
    transaction.commit().unwrap();
    let result = worker.join().unwrap();
    assert!(
        waited,
        "projection must acquire its writer before reading: {result:?}"
    );
    if stale_lookup {
        assert!(matches!(
            result,
            Err(crate::ImError::IdentityBindingConflict { .. })
        ));
    } else {
        assert_eq!(result.unwrap(), canonical);
        assert_eq!(
            super::super::direct_peer_routes::get(&writer, "owner", &canonical)
                .unwrap()
                .unwrap()
                .current_did,
            "did:example:alice-next",
        );
    }
    let (generation, count): (String, i64) = writer.query_row(
        "SELECT binding_generation, (SELECT count(*) FROM peer_personas WHERE owner_identity_id='owner') FROM peer_personas WHERE owner_identity_id='owner'", [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    ).unwrap();
    assert_eq!(generation, "2");
    assert_eq!(count, 1);
}
