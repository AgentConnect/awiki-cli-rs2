use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::ManagedChild;

#[test]
fn observed_wait_reads_both_streams_before_exit_and_preserves_output() {
    let root = tempfile::tempdir().unwrap();
    let finished = root.path().join("finished");
    let mut command = Command::new("sh");
    command
        .args([
            "-c",
            "printf 'reconnect\\n' >&2; sleep 0.1; printf 'recovered\\n'; sleep 0.1; touch \"$1\"",
            "sh",
        ])
        .arg(&finished)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut observed = Vec::new();
    let output = ManagedChild::spawn(&mut command, "spawn two-stream child")
        .unwrap()
        .write_stdin_and_wait_timeout_streams_observed(
            b"",
            "write input",
            "wait for two-stream child",
            Duration::from_secs(2),
            |stderr, line, _| {
                assert!(
                    !finished.exists(),
                    "progress must arrive while the task is still running"
                );
                observed.push((stderr, line.to_vec()));
            },
            |_| {},
        )
        .unwrap();
    assert!(output.output.status.success());
    assert_eq!(
        observed,
        [
            (true, b"reconnect\n".to_vec()),
            (false, b"recovered\n".to_vec())
        ]
    );
    assert_eq!(output.output.stdout, b"recovered\n");
    assert_eq!(output.output.stderr, b"reconnect\n");
}

#[test]
fn observed_wait_streams_lines_and_preserves_complete_output() {
    let mut command = Command::new("sh");
    command
        .arg("-c")
        .arg("printf 'first\\n'; sleep 0.15; printf 'second\\n'")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let observed = Arc::new(Mutex::new(Vec::<String>::new()));
    let observed_for_callback = observed.clone();
    let ticks = Arc::new(Mutex::new(0usize));
    let ticks_for_callback = ticks.clone();

    let output = ManagedChild::spawn(&mut command, "spawn observed test child")
        .unwrap()
        .write_stdin_and_wait_timeout_observed(
            b"",
            "write observed test stdin",
            "wait for observed test child",
            Duration::from_secs(2),
            move |line, _| {
                observed_for_callback
                    .lock()
                    .unwrap()
                    .push(String::from_utf8_lossy(line).trim().to_string());
            },
            move |_| {
                *ticks_for_callback.lock().unwrap() += 1;
            },
        )
        .unwrap();

    assert!(output.output.status.success());
    assert_eq!(output.output.stdout, b"first\nsecond\n");
    assert_eq!(observed.lock().unwrap().as_slice(), ["first", "second"]);
    assert_eq!(*ticks.lock().unwrap(), 0);
}

#[test]
fn observed_wait_ticks_while_child_is_silent() {
    let mut command = Command::new("sh");
    command
        .arg("-c")
        .arg("sleep 0.35")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let ticks = Arc::new(Mutex::new(0usize));
    let ticks_for_callback = ticks.clone();

    ManagedChild::spawn(&mut command, "spawn ticking test child")
        .unwrap()
        .write_stdin_and_wait_timeout_observed(
            b"",
            "write ticking test stdin",
            "wait for ticking test child",
            Duration::from_secs(2),
            |_, _| {},
            move |_| *ticks_for_callback.lock().unwrap() += 1,
        )
        .unwrap();

    assert!(*ticks.lock().unwrap() >= 1);
}
