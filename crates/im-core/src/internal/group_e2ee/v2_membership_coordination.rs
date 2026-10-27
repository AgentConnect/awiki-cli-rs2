//! Coordinate membership preparation with received control across Core processes.
//! Only SDK typed inspection is used; SDK tables and epoch rollback stay private.
use super::*;
use fs2::FileExt;
use std::fs::{File, OpenOptions};

fn busy() -> crate::ImError {
    crate::ImError::Service {
        status_code: None,
        code: Some("group.e2ee.state_not_ready".into()),
        message: "group control must settle before another membership operation".into(),
        data: None,
    }
}
fn unavailable(_: impl std::fmt::Display) -> crate::ImError {
    crate::ImError::LocalStateUnavailable {
        detail: "group membership coordination is unavailable".into(),
    }
}

impl GroupE2eeV2Runtime {
    pub(crate) fn with_control_inbox(mut self, path: std::path::PathBuf) -> Self {
        self.control_inbox = Some(path);
        self
    }

    // A short, nonblocking companion to the SDK operation lock. Both a fresh
    // prepare and notice inspection/application take it, so the two separate
    // typed SDK calls cannot race a new prepare in another process. Never held
    // over network IO, and closing the File releases it on cancellation/crash.
    pub(super) fn membership_guard(&self) -> crate::ImResult<File> {
        let directory = self.store.lock_dir().join("core-membership");
        std::fs::create_dir_all(&directory).map_err(unavailable)?;
        let mut options = OpenOptions::new();
        options.create(true).read(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options
            .open(directory.join("state.lock"))
            .map_err(unavailable)?;
        file.try_lock_exclusive().map_err(|error| {
            if error.kind() == std::io::ErrorKind::WouldBlock {
                busy()
            } else {
                unavailable(error)
            }
        })?;
        Ok(file)
    }

    pub(super) fn require_control_inbox_drained(&self, group: &str) -> crate::ImResult<()> {
        let Some(path) = self.control_inbox.as_ref().filter(|path| path.is_file()) else {
            return Ok(());
        };
        let connection =
            rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
                .map_err(unavailable)?;
        let exists: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='sync_lane_inbox')", [], |row|row.get(0),
        ).map_err(unavailable)?;
        if !exists {
            return Ok(());
        }
        let owner = self.owner_scope()?;
        let pending: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM sync_lane_inbox WHERE owner_identity_id=?1 AND group_did=?2 AND event_type='p6.control.notice' AND closed_at IS NULL)",
            (&owner.owner_identity_id, group), |row|row.get(0),
        ).map_err(unavailable)?;
        if pending {
            Err(busy())
        } else {
            Ok(())
        }
    }

    pub(super) fn require_membership_settled(&self, group: &str) -> crate::ImResult<()> {
        let owner = self.owner_scope()?;
        let status = self.inspect_local_group(V2InspectLocalGroupInput {
            owner_did: owner.owner_did,
            owner_device_id: owner.device_id,
            group_did: group.into(),
            request_id: "im-core-control-before-membership-settled".into(),
        })?;
        if status.host_recheck_pending_count != 0 || status.auto_reconcile_pending_count != 0 {
            return Err(busy());
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "v2_membership_coordination_tests.rs"]
mod tests;
