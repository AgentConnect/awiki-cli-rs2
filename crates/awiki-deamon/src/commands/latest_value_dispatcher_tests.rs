use super::*;
use std::sync::mpsc;
use std::time::{Duration, Instant};

#[test]
fn slow_delivery_does_not_block_publishers_and_stale_values_are_coalesced() {
    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let delivered = Arc::new(Mutex::new(Vec::new()));
    let worker_delivered = Arc::clone(&delivered);
    let dispatcher = LatestValueDispatcher::spawn("latest-value-test", move |value| {
        worker_delivered.lock().unwrap().push(value);
        if value == 1 {
            started_tx.send(()).unwrap();
            release_rx.recv().unwrap();
        }
    })
    .unwrap();

    dispatcher.publish(1);
    started_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    let started = Instant::now();
    for value in 2..=1_000 {
        dispatcher.publish(value);
    }
    assert!(started.elapsed() < Duration::from_millis(100));

    release_tx.send(()).unwrap();
    for _ in 0..100 {
        if delivered.lock().unwrap().last() == Some(&1_000) {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    dispatcher.close();

    assert_eq!(*delivered.lock().unwrap(), vec![1, 1_000]);
}

#[test]
fn close_discards_queued_progress_and_waits_for_inflight_delivery() {
    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let delivered = Arc::new(Mutex::new(Vec::new()));
    let worker_delivered = Arc::clone(&delivered);
    let dispatcher = LatestValueDispatcher::spawn("latest-value-close-test", move |value| {
        worker_delivered.lock().unwrap().push(value);
        started_tx.send(()).unwrap();
        release_rx.recv().unwrap();
    })
    .unwrap();

    dispatcher.publish(1);
    started_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    dispatcher.publish(2);
    let closer = std::thread::spawn(move || dispatcher.close());
    std::thread::sleep(Duration::from_millis(20));
    release_tx.send(()).unwrap();
    closer.join().unwrap();

    assert_eq!(*delivered.lock().unwrap(), vec![1]);
}
