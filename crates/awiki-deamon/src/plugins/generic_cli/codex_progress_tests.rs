use super::*;
use std::sync::mpsc;
use std::time::{Duration, Instant};

#[test]
fn reconnect_fallback_recovery_and_unknown_events() {
    for line in [
        r#"{"type":"error","message":"Reconnecting... 1/5"}"#,
        r#"{"type":"warning","message":"Falling back from WebSockets to HTTPS transport."}"#,
    ] {
        assert_eq!(network_event(false, line.as_bytes()), Some(true));
    }
    assert_eq!(
        network_event(true, b"Falling back from WebSockets to HTTPS transport."),
        Some(true)
    );
    for line in [
        r#"{"type":"item.completed","item":{"type":"agent_message","text":"done"}}"#,
        r#"{"type":"turn.completed"}"#,
    ] {
        assert_eq!(network_event(false, line.as_bytes()), Some(false));
    }
    for line in [
        "not json",
        r#"{"type":"future.event","message":"Reconnecting..."}"#,
        r#"{"type":"item.completed","item":{"type":"command_execution","aggregated_output":"Reconnecting..."}}"#,
        r#"{"type":"turn.failed"}"#,
        r#"{"type":"error","message":"unauthorized"}"#,
    ] {
        assert_eq!(network_event(false, line.as_bytes()), None);
    }
    assert_eq!(network_event(true, b"tool output: Reconnecting..."), None);
}

#[test]
fn slow_or_failed_progress_never_blocks_observation_and_is_coalesced() {
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let mut reporter = CodexProgressReporter::with_delivery(move |_| {
        entered_tx.send(()).unwrap();
        release_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        false
    });
    reporter.on_line(
        false,
        br#"{"type":"error","message":"Reconnecting... 1/5"}"#,
    );
    entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let start = Instant::now();
    for _ in 0..1000 {
        reporter.on_line(
            false,
            br#"{"type":"error","message":"Reconnecting... 2/5"}"#,
        );
    }
    reporter.on_line(false, br#"{"type":"turn.completed"}"#);
    assert!(start.elapsed() < Duration::from_millis(200));
    assert_eq!(reporter.reports, 2);
    assert!(!reporter.delayed);
    // Stop queued progress before terminal handling, allow the in-flight delivery to finish.
    let finish = std::thread::spawn(move || reporter.finish());
    release_tx.send(()).unwrap();
    // If the worker already took the resumed update, release that bounded delivery too.
    let _ = release_tx.send(());
    let observation = finish.join().unwrap();
    assert!(observation["report_failures"].as_u64().unwrap() >= 1);
    assert_eq!(observation["delayed_reports"], 1);
}

#[test]
fn recovery_is_reported_only_after_network_delay() {
    let (tx, rx) = mpsc::channel();
    let mut reporter = CodexProgressReporter::with_delivery(move |value| {
        tx.send(value).unwrap();
        true
    });
    reporter.on_line(false, br#"{"type":"turn.completed"}"#);
    assert_eq!(reporter.reports, 0);
    reporter.on_line(true, b"Reconnecting... 1/5");
    assert!(rx.recv_timeout(Duration::from_secs(1)).unwrap());
    reporter.on_line(
        false,
        br#"{"type":"item.completed","item":{"type":"reasoning"}}"#,
    );
    assert!(!rx.recv_timeout(Duration::from_secs(1)).unwrap());
    assert_eq!(reporter.finish()["report_successes"], 2);
}
