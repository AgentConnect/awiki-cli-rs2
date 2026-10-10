use super::*;
use std::{cell::RefCell, sync::mpsc, thread, time::Duration};

thread_local! {
    static WRITER_WAIT: RefCell<Option<mpsc::Sender<bool>>> = const { RefCell::new(None) };
}

fn notify_writer_wait(attempt: i32) -> bool {
    if attempt == 0 {
        WRITER_WAIT.with(|sender| {
            if let Some(sender) = sender.borrow().as_ref() {
                let _ = sender.send(true);
            }
        });
    }
    thread::sleep(Duration::from_millis(1));
    attempt < 5_000
}

#[test]
fn direct_ratchet_write_waits_before_reading_concurrent_sync_snapshot() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("state.sqlite");
    let writer = crate::internal::local_state::open_writable(&path).unwrap();
    writer.execute_batch("CREATE TABLE concurrency_probe (value INTEGER); INSERT INTO concurrency_probe VALUES (0);").unwrap();
    let reader = crate::internal::local_state::open_writable(&path).unwrap();
    let vault_root = root.path().join("vault");
    let (ready_tx, ready_rx) = mpsc::channel();
    let (start_tx, start_rx) = mpsc::channel();
    let (signal, observed) = mpsc::channel();
    let worker = thread::spawn(move || {
        let store = super::tests::store(&reader, &vault_root);
        ready_tx.send(()).unwrap();
        start_rx.recv().unwrap();
        WRITER_WAIT.with(|sender| *sender.borrow_mut() = Some(signal.clone()));
        reader.busy_handler(Some(notify_writer_wait)).unwrap();
        let result = (|| -> crate::ImResult<i64> {
            let tx = store.transaction()?;
            let value: i64 = tx
                .query_row("SELECT value FROM concurrency_probe", [], |row| row.get(0))
                .map_err(crate::internal::local_state::local_state_unavailable)?;
            tx.execute("UPDATE concurrency_probe SET value = ?1", [value + 1])
                .map_err(crate::internal::local_state::local_state_unavailable)?;
            tx.commit()
                .map_err(crate::internal::local_state::local_state_unavailable)?;
            Ok(value)
        })();
        let _ = signal.send(false);
        result
    });
    ready_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    let tx = Transaction::new_unchecked(&writer, rusqlite::TransactionBehavior::Immediate).unwrap();
    tx.execute("UPDATE concurrency_probe SET value = 1", [])
        .unwrap();
    start_tx.send(()).unwrap();
    let waited = observed.recv_timeout(Duration::from_secs(10)).unwrap();
    tx.commit().unwrap();
    let result = worker.join().unwrap();
    assert!(
        waited,
        "ratchet transaction must wait before reading: {result:?}"
    );
    assert_eq!(result.unwrap(), 1);
    assert_eq!(
        writer
            .query_row("SELECT value FROM concurrency_probe", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        2
    );
}
