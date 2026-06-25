use std::fmt;
use std::io::{Read, Write};
use std::process::{Child, Command, Output};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use serde_json::Value;

pub const DEFAULT_GENERIC_CLI_RUN_TIMEOUT: Duration = Duration::from_secs(10 * 60);
pub const DEFAULT_GENERIC_CLI_PROBE_TIMEOUT: Duration = Duration::from_secs(5);
const MANAGED_CHILD_POLL_INTERVAL: Duration = Duration::from_millis(20);

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

    #[cfg(test)]
    pub(crate) fn new_for_test(context: &'static str, timeout: Duration) -> Self {
        Self {
            context,
            timeout,
            metadata: test_process_management_metadata(),
        }
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

    pub fn wait_timeout(
        mut self,
        wait_context: &'static str,
        timeout: Duration,
    ) -> Result<ManagedChildOutput> {
        let stdout_reader = self.child.as_mut().and_then(|child| child.stdout.take());
        let stderr_reader = self.child.as_mut().and_then(|child| child.stderr.take());
        let stdout_handle = stdout_reader.map(spawn_pipe_reader);
        let stderr_handle = stderr_reader.map(spawn_pipe_reader);
        let deadline = Instant::now() + timeout;
        loop {
            let status = self
                .child
                .as_mut()
                .expect("managed child is always present before timeout wait")
                .try_wait()
                .context(wait_context)?;
            if let Some(status) = status {
                let stdout = join_pipe_reader(stdout_handle, "join managed child stdout reader")?;
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

#[cfg(test)]
fn test_process_management_metadata() -> ProcessManagementMetadata {
    ProcessManagementMetadata {
        process_group_isolated: cfg!(unix),
        process_tree_cleanup_supported: cfg!(unix),
        process_tree_cleanup_strategy: if cfg!(unix) {
            "unix_process_group"
        } else {
            "unsupported"
        },
    }
}

#[cfg(test)]
pub(crate) fn test_process_management_strategy() -> &'static str {
    test_process_management_metadata().process_tree_cleanup_strategy
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
