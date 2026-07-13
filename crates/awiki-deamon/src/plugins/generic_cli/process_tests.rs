use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::ManagedChild;

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
