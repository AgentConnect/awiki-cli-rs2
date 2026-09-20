use super::*;
use std::sync::atomic::AtomicUsize;

#[tokio::test(flavor = "current_thread")]
async fn slow_acp_delivery_keeps_control_executor_responsive_and_stops_once() {
    let stop = Arc::new(AtomicBool::new(false));
    let calls = Arc::new(AtomicUsize::new(0));
    let finished = Arc::new(AtomicBool::new(false));
    let delivery_calls = calls.clone();
    let delivery_finished = finished.clone();
    let handle = tokio::spawn(run_acp_event_scheduler(stop.clone(), move || {
        delivery_calls.fetch_add(1, Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(200));
        delivery_finished.store(true, Ordering::SeqCst);
        Ok(1)
    }));
    tokio::time::timeout(Duration::from_secs(2), async {
        while calls.load(Ordering::SeqCst) == 0 {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .unwrap();
    assert!(
        !finished.load(Ordering::SeqCst),
        "a control task must run during delivery"
    );
    stop.store(true, Ordering::Relaxed);
    tokio::time::timeout(Duration::from_secs(2), handle)
        .await
        .unwrap()
        .unwrap();
    assert!(finished.load(Ordering::SeqCst));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}
