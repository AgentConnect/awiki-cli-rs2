use awiki_cli::host_runtime::listener_shutdown_signal::{
    request_test_shutdown, reset_test_shutdown_request, wait_for_foreground_shutdown_with_interval,
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

static TEST_SIGNAL_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn wait_returns_immediately_when_shutdown_flag_is_already_set() {
    let _guard = TEST_SIGNAL_LOCK.lock().expect("test signal lock");
    reset_test_shutdown_request();
    let shutdown = AtomicBool::new(true);
    let started = Instant::now();

    wait_for_foreground_shutdown_with_interval(&shutdown, Duration::from_millis(50));

    assert!(started.elapsed() < Duration::from_millis(100));
    assert!(shutdown.load(Ordering::SeqCst));
}

#[test]
fn wait_promotes_platform_signal_request_to_shutdown_flag_like_go_context_cancel() {
    let _guard = TEST_SIGNAL_LOCK.lock().expect("test signal lock");
    reset_test_shutdown_request();
    let shutdown = AtomicBool::new(false);

    thread::scope(|scope| {
        scope.spawn(|| {
            thread::sleep(Duration::from_millis(25));
            request_test_shutdown();
        });
        wait_for_foreground_shutdown_with_interval(&shutdown, Duration::from_millis(5));
    });

    assert!(shutdown.load(Ordering::SeqCst));
    reset_test_shutdown_request();
}
