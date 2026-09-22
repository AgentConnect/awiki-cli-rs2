use std::{cell::RefCell, sync::mpsc, thread, time::Duration};

use rusqlite::TransactionBehavior;

use super::{load_lane_sync_states, reconcile_sync_lane_capability_v1a, LaneSyncState};
use crate::internal::wire::sync_v2::SyncLaneV3;

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

#[test]
fn lane_capability_reconcile_waits_for_concurrent_writer() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("local-state.sqlite");
    let mut writer = super::super::open_writable(&path).unwrap();
    writer
        .execute(
            "INSERT INTO identity_account_bindings(
                owner_identity_id, account_id, current_did, device_id,
                identity_generation, device_auth_generation, created_at, updated_at
             ) VALUES ('owner', 'account', 'did:example:owner', 'device', '1', '2', 1, 1)",
            [],
        )
        .unwrap();
    writer
        .execute(
            "INSERT INTO sync_installation_state(owner_identity_id, client_instance_id, created_at)
             VALUES ('owner', 'installation', 1)",
            [],
        )
        .unwrap();
    let receiver = super::super::open_writable(&path).unwrap();
    let state = LaneSyncState {
        owner_identity_id: "owner".to_owned(),
        lane: SyncLaneV3::P5Device,
        stream_epoch: "41".to_owned(),
        scan_seq: "3".to_owned(),
        committed_seq: "3".to_owned(),
    };

    let transaction = writer
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .unwrap();
    transaction
        .execute("UPDATE identity_account_bindings SET updated_at=2", [])
        .unwrap();
    let (signal, observed) = mpsc::channel();
    let worker_state = state.clone();
    let worker = thread::spawn(move || {
        WRITER_WAIT.with(|sender| *sender.borrow_mut() = Some(signal.clone()));
        receiver.busy_handler(Some(notify_writer_wait)).unwrap();
        let result = reconcile_sync_lane_capability_v1a(
            &receiver,
            "owner",
            &[worker_state],
            "2",
            "installation",
            r#"["lanes.p5_device.v1"]"#,
        );
        // A deferred read-to-write upgrade fails without calling the busy
        // handler. Signal completion too, so the regression needs no sleeps.
        let _ = signal.send(false);
        result
    });

    let waited_for_writer = observed.recv_timeout(Duration::from_secs(10)).unwrap();
    transaction.commit().unwrap();
    let result = worker.join().unwrap();
    assert!(
        waited_for_writer,
        "reconciliation must wait before reading its write snapshot: {result:?}"
    );
    result.unwrap();
    assert_eq!(load_lane_sync_states(&writer, "owner").unwrap(), [state]);
    assert_eq!(
        writer
            .query_row(
                "SELECT negotiated_device_auth_generation, client_instance_id,
                        negotiated_capabilities_json FROM sync_lane_capability_state
                 WHERE owner_identity_id='owner'",
                [],
                |row| Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?
                )),
            )
            .unwrap(),
        (
            "2".to_owned(),
            "installation".to_owned(),
            r#"["lanes.p5_device.v1"]"#.to_owned()
        )
    );
}
