use std::future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Notify;

use super::PatchStreamLifecycle;

struct DropSignal(Arc<AtomicBool>);

impl Drop for DropSignal {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

#[tokio::test]
async fn stop_wakes_and_joins_an_idle_patch_worker() {
    let lifecycle = PatchStreamLifecycle::new("test patch");
    let entered = Arc::new(Notify::new());
    let dropped = Arc::new(AtomicBool::new(false));
    let worker_entered = Arc::clone(&entered);
    let worker_dropped = Arc::clone(&dropped);

    lifecycle
        .spawn(async move {
            let _drop_signal = DropSignal(worker_dropped);
            worker_entered.notify_one();
            future::pending::<()>().await;
        })
        .expect("idle patch worker should attach");
    entered.notified().await;

    tokio::time::timeout(Duration::from_secs(1), lifecycle.stop())
        .await
        .expect("stop must wake an idle patch worker")
        .expect("stop should join the patch worker");

    assert!(
        dropped.load(Ordering::SeqCst),
        "stop returned before the patch worker was dropped"
    );
}

#[tokio::test]
async fn stop_is_idempotent_after_the_patch_worker_exits() {
    let lifecycle = PatchStreamLifecycle::new("test patch");
    lifecycle
        .spawn(async {})
        .expect("patch worker should attach");

    lifecycle.stop().await.expect("first stop should succeed");
    lifecycle.stop().await.expect("second stop should succeed");
}

#[tokio::test]
async fn stopped_lifecycle_rejects_a_late_patch_worker() {
    let lifecycle = PatchStreamLifecycle::new("test patch");
    lifecycle.stop().await.expect("stop should succeed");

    let error = lifecycle
        .spawn(future::pending::<()>())
        .expect_err("a worker must not attach after stop");

    assert_eq!(error.code, "object_closed");
}
