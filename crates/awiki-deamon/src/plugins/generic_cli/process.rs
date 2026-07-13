use std::fmt;
use std::io::{Read, Write};
use std::process::{Child, Command, Output};
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use serde_json::Value;

pub const DEFAULT_GENERIC_CLI_RUN_TIMEOUT: Duration = Duration::from_secs(10 * 60);
pub const DEFAULT_GENERIC_CLI_PROBE_TIMEOUT: Duration = Duration::from_secs(5);
const MANAGED_CHILD_POLL_INTERVAL: Duration = Duration::from_millis(20);
const MANAGED_CHILD_OBSERVER_TICK_INTERVAL: Duration = Duration::from_millis(250);

#[derive(Debug)]
pub struct ManagedChild {
    child: Option<Child>,
    metadata: ProcessManagementMetadata,
}

#[derive(Debug)]
pub struct ManagedChildOutput {
    pub output: Output,
    metadata: ProcessManagementMetadata,
}

#[derive(Debug, Clone, Copy)]
struct ProcessManagementMetadata {
    process_group_isolated: bool,
    process_tree_cleanup_supported: bool,
    process_tree_cleanup_strategy: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub struct ManagedChildTimeoutError {
    context: &'static str,
    timeout: Duration,
    metadata: ProcessManagementMetadata,
}

impl ManagedChildTimeoutError {
    pub fn timeout_ms(&self) -> u128 {
        self.timeout.as_millis()
    }

    pub fn metadata_json(&self) -> Value {
        process_metadata_json(self.metadata)
    }
}

impl fmt::Display for ManagedChildTimeoutError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} timed out after {} ms",
            self.context,
            self.timeout.as_millis()
        )
    }
}

impl std::error::Error for ManagedChildTimeoutError {}

impl ManagedChild {
    pub fn spawn(command: &mut Command, context: &'static str) -> Result<Self> {
        let metadata = configure_managed_command(command);
        let child = command.spawn().context(context)?;
        Ok(Self {
            child: Some(child),
            metadata,
        })
    }

    pub fn write_stdin_and_wait_timeout(
        mut self,
        input: &[u8],
        stdin_context: &'static str,
        wait_context: &'static str,
        timeout: Duration,
    ) -> Result<ManagedChildOutput> {
        if let Some(child) = self.child.as_mut() {
            let write_result = child
                .stdin
                .as_mut()
                .context("open managed child stdin")
                .and_then(|stdin| stdin.write_all(input).context(stdin_context));
            if let Err(error) = write_result {
                self.kill_process_tree_best_effort();
                return Err(error);
            }
            drop(child.stdin.take());
        }
        self.wait_timeout(wait_context, timeout)
    }

    pub fn write_stdin_and_wait_timeout_observed<OnStdoutLine, OnTick>(
        mut self,
        input: &[u8],
        stdin_context: &'static str,
        wait_context: &'static str,
        timeout: Duration,
        on_stdout_line: OnStdoutLine,
        on_tick: OnTick,
    ) -> Result<ManagedChildOutput>
    where
        OnStdoutLine: FnMut(&[u8], Duration),
        OnTick: FnMut(Duration),
    {
        if let Some(child) = self.child.as_mut() {
            let write_result = child
                .stdin
                .as_mut()
                .context("open managed child stdin")
                .and_then(|stdin| stdin.write_all(input).context(stdin_context));
            if let Err(error) = write_result {
                self.kill_process_tree_best_effort();
                return Err(error);
            }
            drop(child.stdin.take());
        }
        self.wait_timeout_observed(wait_context, timeout, on_stdout_line, on_tick)
    }

    pub fn wait_timeout(
        self,
        wait_context: &'static str,
        timeout: Duration,
    ) -> Result<ManagedChildOutput> {
        self.wait_timeout_observed(wait_context, timeout, |_, _| {}, |_| {})
    }

    fn wait_timeout_observed<OnStdoutLine, OnTick>(
        mut self,
        wait_context: &'static str,
        timeout: Duration,
        mut on_stdout_line: OnStdoutLine,
        mut on_tick: OnTick,
    ) -> Result<ManagedChildOutput>
    where
        OnStdoutLine: FnMut(&[u8], Duration),
        OnTick: FnMut(Duration),
    {
        let stdout_reader = self.child.as_mut().and_then(|child| child.stdout.take());
        let stderr_reader = self.child.as_mut().and_then(|child| child.stderr.take());
        let (stdout_line_tx, stdout_line_rx) = mpsc::channel();
        let stdout_handle =
            stdout_reader.map(|reader| spawn_observed_stdout_reader(reader, stdout_line_tx));
        let stderr_handle = stderr_reader.map(spawn_pipe_reader);
        let started_at = Instant::now();
        let mut next_tick_at = started_at + MANAGED_CHILD_OBSERVER_TICK_INTERVAL;
        let deadline = Instant::now() + timeout;
        loop {
            drain_stdout_lines(&stdout_line_rx, started_at.elapsed(), &mut on_stdout_line);
            let now = Instant::now();
            if now >= next_tick_at {
                on_tick(started_at.elapsed());
                next_tick_at = now + MANAGED_CHILD_OBSERVER_TICK_INTERVAL;
            }
            let status = self
                .child
                .as_mut()
                .expect("managed child is always present before timeout wait")
                .try_wait()
                .context(wait_context)?;
            if let Some(status) = status {
                let stdout = join_pipe_reader(stdout_handle, "join managed child stdout reader")?;
                drain_stdout_lines(&stdout_line_rx, started_at.elapsed(), &mut on_stdout_line);
                let stderr = join_pipe_reader(stderr_handle, "join managed child stderr reader")?;
                self.child.take();
                return Ok(ManagedChildOutput {
                    output: Output {
                        status,
                        stdout,
                        stderr,
                    },
                    metadata: self.metadata,
                });
            }
            if Instant::now() >= deadline {
                self.kill_process_tree_best_effort();
                let stdout =
                    join_pipe_reader(stdout_handle, "join timed out managed child stdout reader")?;
                let stderr =
                    join_pipe_reader(stderr_handle, "join timed out managed child stderr reader")?;
                drop(stdout);
                drop(stderr);
                self.child.take();
                return Err(ManagedChildTimeoutError {
                    context: wait_context,
                    timeout,
                    metadata: self.metadata,
                }
                .into());
            }
            std::thread::sleep(MANAGED_CHILD_POLL_INTERVAL);
        }
    }

    fn kill_process_tree_best_effort(&mut self) {
        if let Some(child) = self.child.as_mut() {
            kill_child_process_tree_best_effort(child, self.metadata);
        }
    }
}

impl Drop for ManagedChild {
    fn drop(&mut self) {
        self.kill_process_tree_best_effort();
    }
}

impl ManagedChildOutput {
    pub fn process_metadata(&self) -> Value {
        self.metadata_json()
    }

    pub fn metadata_json(&self) -> Value {
        process_metadata_json(self.metadata)
    }
}

fn process_metadata_json(metadata: ProcessManagementMetadata) -> Value {
    serde_json::json!({
        "process_group_isolated": metadata.process_group_isolated,
        "process_tree_cleanup_supported": metadata.process_tree_cleanup_supported,
        "process_tree_cleanup_strategy": metadata.process_tree_cleanup_strategy,
    })
}

fn spawn_pipe_reader<R>(mut reader: R) -> JoinHandle<std::io::Result<Vec<u8>>>
where
    R: Read + Send + 'static,
{
    std::thread::spawn(move || {
        let mut buffer = Vec::new();
        reader.read_to_end(&mut buffer)?;
        Ok(buffer)
    })
}

fn spawn_observed_stdout_reader<R>(
    reader: R,
    line_sender: mpsc::Sender<Vec<u8>>,
) -> JoinHandle<std::io::Result<Vec<u8>>>
where
    R: Read + Send + 'static,
{
    std::thread::spawn(move || {
        let mut reader = std::io::BufReader::new(reader);
        let mut output = Vec::new();
        loop {
            let mut line = Vec::new();
            let read = std::io::BufRead::read_until(&mut reader, b'\n', &mut line)?;
            if read == 0 {
                break;
            }
            output.extend_from_slice(&line);
            let _ = line_sender.send(line);
        }
        Ok(output)
    })
}

fn drain_stdout_lines<OnStdoutLine>(
    receiver: &mpsc::Receiver<Vec<u8>>,
    elapsed: Duration,
    on_stdout_line: &mut OnStdoutLine,
) where
    OnStdoutLine: FnMut(&[u8], Duration),
{
    while let Ok(line) = receiver.try_recv() {
        on_stdout_line(&line, elapsed);
    }
}

fn join_pipe_reader(
    handle: Option<JoinHandle<std::io::Result<Vec<u8>>>>,
    context: &'static str,
) -> Result<Vec<u8>> {
    let Some(handle) = handle else {
        return Ok(Vec::new());
    };
    match handle.join() {
        Ok(Ok(output)) => Ok(output),
        Ok(Err(error)) => Err(error).context(context),
        Err(_) => anyhow::bail!("{context}: reader thread panicked"),
    }
}

#[cfg(unix)]
fn configure_managed_command(command: &mut Command) -> ProcessManagementMetadata {
    use std::os::unix::process::CommandExt;

    unsafe {
        command.pre_exec(|| {
            if libc::setpgid(0, 0) == 0 {
                Ok(())
            } else {
                Err(std::io::Error::last_os_error())
            }
        });
    }
    ProcessManagementMetadata {
        process_group_isolated: true,
        process_tree_cleanup_supported: true,
        process_tree_cleanup_strategy: "unix_process_group",
    }
}

#[cfg(not(unix))]
fn configure_managed_command(_command: &mut Command) -> ProcessManagementMetadata {
    ProcessManagementMetadata {
        process_group_isolated: false,
        process_tree_cleanup_supported: false,
        process_tree_cleanup_strategy: "unsupported",
    }
}

#[cfg(unix)]
fn kill_child_process_tree_best_effort(child: &mut Child, metadata: ProcessManagementMetadata) {
    if let Ok(None) = child.try_wait() {
        if metadata.process_tree_cleanup_supported {
            kill_process_group_by_id(child.id(), libc::SIGTERM);
            std::thread::sleep(std::time::Duration::from_millis(50));
            if let Ok(None) = child.try_wait() {
                kill_process_group_by_id(child.id(), libc::SIGKILL);
            }
        } else {
            let _ = child.kill();
        }
        let _ = child.wait();
    }
}

#[cfg(unix)]
fn kill_process_group_by_id(child_id: u32, signal: libc::c_int) {
    unsafe {
        libc::killpg(child_id as libc::pid_t, signal);
    }
}

#[cfg(not(unix))]
fn kill_child_process_tree_best_effort(child: &mut Child, _metadata: ProcessManagementMetadata) {
    if let Ok(None) = child.try_wait() {
        let _ = child.kill();
        let _ = child.wait();
    }
}

#[cfg(not(unix))]
fn kill_process_tree_by_id_best_effort(_child_id: u32, _metadata: ProcessManagementMetadata) {}

#[cfg(test)]
#[path = "process_tests.rs"]
mod process_tests;
