use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

static SHUTDOWN_REQUESTED: AtomicBool = AtomicBool::new(false);

pub fn install_foreground_shutdown_handler(shutdown: Arc<AtomicBool>) -> anyhow::Result<()> {
    SHUTDOWN_REQUESTED.store(false, Ordering::SeqCst);
    install_platform_shutdown_handler()?;
    if SHUTDOWN_REQUESTED.load(Ordering::SeqCst) {
        shutdown.store(true, Ordering::SeqCst);
    }
    Ok(())
}

pub fn wait_for_foreground_shutdown(shutdown: &AtomicBool) {
    wait_for_foreground_shutdown_with_interval(shutdown, Duration::from_millis(250));
}

pub fn wait_for_foreground_shutdown_with_interval(shutdown: &AtomicBool, interval: Duration) {
    while !shutdown.load(Ordering::SeqCst) && !SHUTDOWN_REQUESTED.load(Ordering::SeqCst) {
        thread::sleep(interval);
    }
    if SHUTDOWN_REQUESTED.load(Ordering::SeqCst) {
        shutdown.store(true, Ordering::SeqCst);
    }
}

pub fn reset_test_shutdown_request() {
    SHUTDOWN_REQUESTED.store(false, Ordering::SeqCst);
}

pub fn request_test_shutdown() {
    SHUTDOWN_REQUESTED.store(true, Ordering::SeqCst);
}

#[cfg(unix)]
fn install_platform_shutdown_handler() -> anyhow::Result<()> {
    unsafe {
        signal(SIGINT, listener_shutdown_signal_handler);
        signal(SIGTERM, listener_shutdown_signal_handler);
    }
    Ok(())
}

#[cfg(windows)]
fn install_platform_shutdown_handler() -> anyhow::Result<()> {
    let ok = unsafe { SetConsoleCtrlHandler(Some(listener_shutdown_console_handler), 1) };
    if ok == 0 {
        anyhow::bail!("install listener foreground shutdown handler failed");
    }
    Ok(())
}

#[cfg(not(any(unix, windows)))]
fn install_platform_shutdown_handler() -> anyhow::Result<()> {
    Ok(())
}

#[cfg(unix)]
extern "C" fn listener_shutdown_signal_handler(_signal: std::os::raw::c_int) {
    SHUTDOWN_REQUESTED.store(true, Ordering::SeqCst);
}

#[cfg(windows)]
extern "system" fn listener_shutdown_console_handler(ctrl_type: u32) -> i32 {
    match ctrl_type {
        CTRL_C_EVENT | CTRL_BREAK_EVENT => {
            SHUTDOWN_REQUESTED.store(true, Ordering::SeqCst);
            1
        }
        _ => 0,
    }
}

#[cfg(unix)]
const SIGINT: std::os::raw::c_int = 2;
#[cfg(unix)]
const SIGTERM: std::os::raw::c_int = 15;

#[cfg(windows)]
const CTRL_C_EVENT: u32 = 0;
#[cfg(windows)]
const CTRL_BREAK_EVENT: u32 = 1;

#[cfg(unix)]
extern "C" {
    fn signal(
        signum: std::os::raw::c_int,
        handler: extern "C" fn(std::os::raw::c_int),
    ) -> extern "C" fn(std::os::raw::c_int);
}

#[cfg(windows)]
#[link(name = "kernel32")]
extern "system" {
    fn SetConsoleCtrlHandler(handler: Option<extern "system" fn(u32) -> i32>, add: i32) -> i32;
}
