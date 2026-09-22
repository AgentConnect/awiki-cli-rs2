//! Bounded, non-interactive probe. Never retain arbitrary client output in reports.
use std::{
    io::Read,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

struct ProbeChild(Child);
impl Drop for ProbeChild {
    fn drop(&mut self) {
        // Also clean descendants when the launcher has already exited.
        #[cfg(unix)]
        unsafe {
            libc::killpg(self.0.id() as i32, libc::SIGKILL);
        }
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn read_bounded(mut pipe: impl Read) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut buffer = [0; 4096];
    while let Ok(count) = pipe.read(&mut buffer) {
        if count == 0 {
            break;
        }
        let keep = count.min(32768_usize.saturating_sub(bytes.len()));
        bytes.extend_from_slice(&buffer[..keep]);
    }
    bytes
}

pub(crate) fn run(command: &mut Command, deadline: Instant) -> Result<String, &'static str> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let child = command.spawn().map_err(|e| match e.kind() {
        std::io::ErrorKind::PermissionDenied => "not_executable",
        _ => "launch_failed",
    })?;
    let mut child = ProbeChild(child);
    let stdout = child.0.stdout.take().unwrap();
    let stderr = child.0.stderr.take().unwrap();
    let out = std::thread::spawn(move || read_bounded(stdout));
    let err = std::thread::spawn(move || read_bounded(stderr));
    let result = loop {
        match child.0.try_wait() {
            Ok(Some(status)) => {
                break if status.success() {
                    Ok(())
                } else {
                    Err("version_failed")
                }
            }
            Err(_) => break Err("launch_failed"),
            _ if Instant::now() >= deadline => break Err("timeout"),
            _ => std::thread::sleep(Duration::from_millis(10)),
        }
    };
    drop(child);
    let stdout = out.join().unwrap_or_default();
    let stderr = err.join().unwrap_or_default();
    result?;
    let mut text = String::from_utf8_lossy(&stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&stderr));
    Ok(text)
}
