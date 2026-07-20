use std::fs::{File, OpenOptions};
use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use serde_json::json;

use crate::security::runtime_token::current_time_millis;
use crate::DaemonConfig;

#[derive(Debug)]
pub(super) struct StateRootOwnerGuard {
    #[allow(dead_code)]
    file: File,
}

impl StateRootOwnerGuard {
    pub(super) fn acquire(config: &DaemonConfig) -> Result<Self> {
        acquire_state_root_owner(config)
    }
}

#[cfg(unix)]
fn acquire_state_root_owner(config: &DaemonConfig) -> Result<StateRootOwnerGuard> {
    use std::io::Write;
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::PermissionsExt;

    let lock_path = state_root_owner_lock_path(config);
    let parent = lock_path
        .parent()
        .context("state root owner lock path has no parent")?;
    std::fs::create_dir_all(parent).context("create daemon state owner lock directory")?;
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
        .context("open daemon state owner lock")?;
    let _ = std::fs::set_permissions(&lock_path, std::fs::Permissions::from_mode(0o600));

    let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if rc != 0 {
        let error = std::io::Error::last_os_error();
        if matches!(
            error.kind(),
            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::AlreadyExists
        ) {
            bail!("daemon_state_root_busy: another awiki-deamon process owns this state root");
        }
        return Err(error).context("lock daemon state root owner");
    }

    file.set_len(0)
        .context("truncate daemon state owner lock metadata")?;
    let metadata = json!({
        "schema": "awiki.daemon.state_root_owner.v1",
        "owner_status": "active",
        "process_id": std::process::id(),
        "acquired_at_ms": current_time_millis()?,
        "lock_strategy": "flock",
    });
    file.write_all(serde_json::to_string_pretty(&metadata)?.as_bytes())
        .context("write daemon state owner lock metadata")?;
    file.write_all(b"\n")
        .context("write daemon state owner lock metadata newline")?;
    file.sync_all()
        .context("sync daemon state owner lock metadata")?;

    Ok(StateRootOwnerGuard { file })
}

#[cfg(not(unix))]
fn acquire_state_root_owner(_config: &DaemonConfig) -> Result<StateRootOwnerGuard> {
    bail!("daemon_state_root_owner_unsupported: daemon state root owner lock requires Unix flock")
}

fn state_root_owner_lock_path(config: &DaemonConfig) -> PathBuf {
    config.state_root.join("run").join("state-root-owner.lock")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn state_root_owner_guard_rejects_second_owner() {
        let root = tempfile::tempdir().unwrap();
        let config = DaemonConfig::for_state_root(root.path()).unwrap();
        config.ensure_state_layout().unwrap();

        let _first = StateRootOwnerGuard::acquire(&config).unwrap();
        assert!(state_root_owner_lock_path(&config).exists());
        let error = StateRootOwnerGuard::acquire(&config).unwrap_err();

        assert!(error.to_string().contains("daemon_state_root_busy"));
        assert!(!error
            .to_string()
            .contains(&config.state_root.display().to_string()));
    }

    #[cfg(unix)]
    #[test]
    fn state_root_owner_guard_releases_on_drop() {
        let root = tempfile::tempdir().unwrap();
        let config = DaemonConfig::for_state_root(root.path()).unwrap();
        config.ensure_state_layout().unwrap();

        let first = StateRootOwnerGuard::acquire(&config).unwrap();
        let lock_path = state_root_owner_lock_path(&config);
        drop(first);
        let _second = StateRootOwnerGuard::acquire(&config).unwrap();

        assert!(lock_path.exists());
    }
}
