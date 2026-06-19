use std::io::Write;
use std::process::{Child, Command, Output};

use anyhow::{Context, Result};
use serde_json::Value;

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

impl ManagedChild {
    pub fn spawn(command: &mut Command, context: &'static str) -> Result<Self> {
        let metadata = configure_managed_command(command);
        let child = command.spawn().context(context)?;
        Ok(Self {
            child: Some(child),
            metadata,
        })
    }

    pub fn write_stdin_and_wait(
        mut self,
        input: &[u8],
        stdin_context: &'static str,
        wait_context: &'static str,
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
        self.wait(wait_context)
    }

    pub fn wait(mut self, wait_context: &'static str) -> Result<ManagedChildOutput> {
        let child = self
            .child
            .take()
            .expect("managed child is always present before wait");
        let child_id = child.id();
        match child.wait_with_output().context(wait_context) {
            Ok(output) => Ok(ManagedChildOutput {
                output,
                metadata: self.metadata,
            }),
            Err(error) => {
                kill_process_tree_by_id_best_effort(child_id, self.metadata);
                Err(error)
            }
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
        serde_json::json!({
            "process_group_isolated": self.metadata.process_group_isolated,
            "process_tree_cleanup_supported": self.metadata.process_tree_cleanup_supported,
            "process_tree_cleanup_strategy": self.metadata.process_tree_cleanup_strategy,
        })
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
fn kill_process_tree_by_id_best_effort(child_id: u32, metadata: ProcessManagementMetadata) {
    if metadata.process_tree_cleanup_supported {
        kill_process_group_by_id(child_id, libc::SIGTERM);
        std::thread::sleep(std::time::Duration::from_millis(50));
        kill_process_group_by_id(child_id, libc::SIGKILL);
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
