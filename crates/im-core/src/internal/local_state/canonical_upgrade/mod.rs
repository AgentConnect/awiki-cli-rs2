//! Explicit release/0710 schema 27 to canonical schema upgrade orchestration.
//!
//! Ordinary local-state open remains read-only with respect to older schemas.
//! This module owns detection, cross-process locking, consistent backup,
//! shadow migration, validation, cutover, and restore.

mod source;

use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};
use std::time::Duration;

use fs2::FileExt as _;
use serde::{Deserialize, Serialize};

pub(crate) const RELEASE_0710_SCHEMA_VERSION: i64 = 27;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CanonicalUpgradePhase {
    Detected,
    PreflightPassed,
    BackupVerified,
    ShadowMigrated,
    ValidationPassed,
    CutoverStarted,
    Completed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CanonicalUpgradeEligibility {
    NotRequired,
    Eligible,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CanonicalUpgradeDetection {
    pub(crate) eligibility: CanonicalUpgradeEligibility,
    pub(crate) source_schema_version: i64,
    pub(crate) target_schema_version: i64,
    pub(crate) source_fingerprint: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CanonicalUpgradeJournal {
    pub(crate) format_version: u32,
    pub(crate) upgrade_id: String,
    pub(crate) source_schema_version: i64,
    pub(crate) target_schema_version: i64,
    pub(crate) source_fingerprint: String,
    pub(crate) phase: CanonicalUpgradePhase,
    pub(crate) backup_file: String,
    pub(crate) shadow_file: String,
    pub(crate) rollback_file: String,
    pub(crate) updated_at_unix: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct VerifiedBackup {
    pub(crate) path: PathBuf,
    pub(crate) source_fingerprint: String,
}

pub(crate) struct CanonicalUpgradeLock {
    file: File,
}

impl Drop for CanonicalUpgradeLock {
    fn drop(&mut self) {
        let _ = fs2::FileExt::unlock(&self.file);
    }
}

pub(crate) fn detect(sqlite_path: &Path) -> crate::ImResult<CanonicalUpgradeDetection> {
    source::detect(sqlite_path)
}

pub(crate) fn acquire_lock(upgrade_dir: &Path) -> crate::ImResult<CanonicalUpgradeLock> {
    create_private_dir(upgrade_dir)?;
    let lock_path = upgrade_dir.join("canonical-conversation-upgrade.lock");
    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(&lock_path)
        .map_err(|_| upgrade_failed("lock", "lock_file_unavailable"))?;
    set_private_file_permissions(&lock_path)?;
    file.try_lock_exclusive()
        .map_err(|_| crate::ImError::LocalStateUpgradeInProgress)?;
    Ok(CanonicalUpgradeLock { file })
}

pub(crate) fn create_verified_backup(
    sqlite_path: &Path,
    upgrade_dir: &Path,
    expected_fingerprint: &str,
    upgrade_id: &str,
) -> crate::ImResult<VerifiedBackup> {
    let backup_dir = upgrade_dir.join("backups").join(upgrade_id);
    create_private_dir(&backup_dir)?;
    let backup_path = backup_dir.join("im.sqlite");
    remove_sqlite_file_set(&backup_path)?;
    online_backup(sqlite_path, &backup_path, "backup")?;
    set_private_file_permissions(&backup_path)?;
    source::verify_release_0710_source(&backup_path, Some(expected_fingerprint))?;
    Ok(VerifiedBackup {
        path: backup_path,
        source_fingerprint: expected_fingerprint.to_owned(),
    })
}

pub(crate) fn create_shadow_from_source(
    sqlite_path: &Path,
    upgrade_dir: &Path,
    upgrade_id: &str,
) -> crate::ImResult<PathBuf> {
    create_private_dir(upgrade_dir)?;
    let shadow_path = upgrade_dir.join(format!("canonical-{upgrade_id}.shadow.sqlite"));
    remove_sqlite_file_set(&shadow_path)?;
    online_backup(sqlite_path, &shadow_path, "shadow_backup")?;
    set_private_file_permissions(&shadow_path)?;
    Ok(shadow_path)
}

pub(crate) fn load_journal(upgrade_dir: &Path) -> crate::ImResult<Option<CanonicalUpgradeJournal>> {
    let path = journal_path(upgrade_dir);
    if !path.exists() {
        return Ok(None);
    }
    let bytes = fs::read(&path).map_err(|_| upgrade_failed("journal", "journal_unreadable"))?;
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|_| upgrade_failed("journal", "journal_invalid"))
}

pub(crate) fn write_journal(
    upgrade_dir: &Path,
    journal: &CanonicalUpgradeJournal,
) -> crate::ImResult<()> {
    create_private_dir(upgrade_dir)?;
    let path = journal_path(upgrade_dir);
    let temporary = upgrade_dir.join("canonical-conversation-upgrade.journal.tmp");
    let payload = serde_json::to_vec_pretty(journal)
        .map_err(|_| upgrade_failed("journal", "journal_encode_failed"))?;
    fs::write(&temporary, payload)
        .map_err(|_| upgrade_failed("journal", "journal_write_failed"))?;
    set_private_file_permissions(&temporary)?;
    fs::rename(&temporary, &path)
        .map_err(|_| upgrade_failed("journal", "journal_commit_failed"))?;
    Ok(())
}

pub(crate) fn new_upgrade_id() -> String {
    format!(
        "{}-{}",
        time::OffsetDateTime::now_utc().unix_timestamp_nanos(),
        std::process::id()
    )
}

pub(crate) fn now_unix() -> i64 {
    time::OffsetDateTime::now_utc().unix_timestamp()
}

fn online_backup(source_path: &Path, target_path: &Path, phase: &str) -> crate::ImResult<()> {
    let source = rusqlite::Connection::open_with_flags(
        source_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|_| upgrade_failed(phase, "source_open_failed"))?;
    let mut target = rusqlite::Connection::open(target_path)
        .map_err(|_| upgrade_failed(phase, "target_open_failed"))?;
    let backup = rusqlite::backup::Backup::new(&source, &mut target)
        .map_err(|_| upgrade_failed(phase, "sqlite_backup_start_failed"))?;
    backup
        .run_to_completion(64, Duration::from_millis(5), None)
        .map_err(|_| upgrade_failed(phase, "sqlite_backup_failed"))?;
    drop(backup);
    target
        .query_row("PRAGMA integrity_check", [], |row| row.get::<_, String>(0))
        .map_err(|_| upgrade_failed(phase, "backup_integrity_check_failed"))
        .and_then(|result| {
            if result == "ok" {
                Ok(())
            } else {
                Err(upgrade_failed(phase, "backup_integrity_check_failed"))
            }
        })
}

fn journal_path(upgrade_dir: &Path) -> PathBuf {
    upgrade_dir.join("canonical-conversation-upgrade.journal.json")
}

pub(super) fn upgrade_failed(phase: &str, code: &str) -> crate::ImError {
    crate::ImError::LocalStateUpgradeFailed {
        phase: phase.to_owned(),
        code: code.to_owned(),
    }
}

fn create_private_dir(path: &Path) -> crate::ImResult<()> {
    fs::create_dir_all(path).map_err(|_| upgrade_failed("filesystem", "directory_unavailable"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .map_err(|_| upgrade_failed("filesystem", "directory_permissions_failed"))?;
    }
    Ok(())
}

fn set_private_file_permissions(path: &Path) -> crate::ImResult<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .map_err(|_| upgrade_failed("filesystem", "file_permissions_failed"))?;
    }
    Ok(())
}

pub(super) fn remove_sqlite_file_set(path: &Path) -> crate::ImResult<()> {
    for candidate in [
        path.to_path_buf(),
        PathBuf::from(format!("{}-wal", path.display())),
        PathBuf::from(format!("{}-shm", path.display())),
    ] {
        if candidate.exists() {
            fs::remove_file(candidate)
                .map_err(|_| upgrade_failed("filesystem", "stale_file_remove_failed"))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lock_is_cross_handle_exclusive() {
        let directory = tempfile::tempdir().unwrap();
        let first = acquire_lock(directory.path()).unwrap();
        assert!(matches!(
            acquire_lock(directory.path()),
            Err(crate::ImError::LocalStateUpgradeInProgress)
        ));
        drop(first);
        acquire_lock(directory.path()).unwrap();
    }

    #[test]
    fn journal_round_trip_is_atomic_and_redacted() {
        let directory = tempfile::tempdir().unwrap();
        let journal = CanonicalUpgradeJournal {
            format_version: 1,
            upgrade_id: "upgrade-1".to_owned(),
            source_schema_version: 27,
            target_schema_version: 28,
            source_fingerprint: "sha256:test".to_owned(),
            phase: CanonicalUpgradePhase::Detected,
            backup_file: "backups/upgrade-1/im.sqlite".to_owned(),
            shadow_file: "canonical-upgrade-1.shadow.sqlite".to_owned(),
            rollback_file: "canonical-upgrade-1.rollback.sqlite".to_owned(),
            updated_at_unix: 1,
        };
        write_journal(directory.path(), &journal).unwrap();
        assert_eq!(load_journal(directory.path()).unwrap(), Some(journal));
    }

    #[test]
    fn online_backup_contains_committed_wal_rows_and_is_verified() {
        let directory = tempfile::tempdir().unwrap();
        let source_path = directory.path().join("im.sqlite");
        source::create_minimal_release_0710_fixture(&source_path);
        let source_db = rusqlite::Connection::open(&source_path).unwrap();
        source_db
            .pragma_update(None, "journal_mode", "WAL")
            .unwrap();
        source_db
            .execute(
                r#"INSERT INTO messages
(owner_identity_id, owner_did, msg_id, conversation_id, thread_id,
 sender_did, receiver_did, group_did)
VALUES ('owner', 'did:example:owner', 'message-1', 'dm:legacy', 'dm:legacy',
        'did:example:peer', 'did:example:owner', '')"#,
                [],
            )
            .unwrap();
        let detection = detect(&source_path).unwrap();
        let backup = create_verified_backup(
            &source_path,
            &directory.path().join("upgrade"),
            &detection.source_fingerprint,
            "upgrade-1",
        )
        .unwrap();

        let backup_db = rusqlite::Connection::open(backup.path).unwrap();
        assert_eq!(
            backup_db
                .query_row("SELECT COUNT(*) FROM messages", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
    }
}
