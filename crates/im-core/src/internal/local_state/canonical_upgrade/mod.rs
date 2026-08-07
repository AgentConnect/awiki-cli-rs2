//! Explicit release/0710 schema 27 to canonical schema upgrade orchestration.
//!
//! Ordinary local-state open remains read-only with respect to older schemas.
//! This module owns detection, cross-process locking, consistent backup,
//! shadow migration, validation, cutover, and restore.

mod migrate;
mod source;
mod validate;

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
    RestoreStarted,
    Restored,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CanonicalRestoreReport {
    pub(crate) restored_schema_version: i64,
    pub(crate) target_safety_copy: PathBuf,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct CanonicalUpgradeReport {
    pub(crate) source_schema_version: i64,
    pub(crate) target_schema_version: i64,
    pub(crate) migrated_personas: u64,
    pub(crate) migrated_conversations: u64,
    pub(crate) unresolved_messages: u64,
    pub(crate) alias_count: u64,
    pub(crate) backup_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CanonicalConversationAliasMapping {
    pub(crate) owner_identity_id: String,
    pub(crate) owner_did: String,
    pub(crate) legacy_conversation_id: String,
    pub(crate) canonical_conversation_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CanonicalUpgradeOutcome {
    NotRequired(CanonicalUpgradeDetection),
    Completed(CanonicalUpgradeReport),
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

#[cfg(test)]
pub(crate) fn copy_release_0710_fixture(path: &Path) {
    source::copy_release_0710_fixture(path);
}

/// Reads the stable owner-scoped alias projection used by the App overlay
/// migrator. This is intentionally available after cutover as well so an App
/// crash between the Core and overlay journals can resume without guessing.
pub(crate) fn list_alias_mappings(
    sqlite_path: &Path,
) -> crate::ImResult<Vec<CanonicalConversationAliasMapping>> {
    use rusqlite::{Connection, OpenFlags};

    let connection = Connection::open_with_flags(
        sqlite_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(super::local_state_unavailable)?;
    let mut statement = connection
        .prepare(
            r#"SELECT DISTINCT aliases.owner_identity_id,
       registry.owner_did,
       aliases.alias_conversation_id,
       aliases.canonical_conversation_id
FROM conversation_aliases aliases
JOIN conversation_registry registry
  ON registry.owner_identity_id = aliases.owner_identity_id
 AND registry.conversation_id = aliases.canonical_conversation_id
WHERE aliases.alias_conversation_id <> aliases.canonical_conversation_id
ORDER BY aliases.owner_identity_id, aliases.alias_conversation_id"#,
        )
        .map_err(super::local_state_unavailable)?;
    let rows = statement
        .query_map([], |row| {
            Ok(CanonicalConversationAliasMapping {
                owner_identity_id: row.get(0)?,
                owner_did: row.get(1)?,
                legacy_conversation_id: row.get(2)?,
                canonical_conversation_id: row.get(3)?,
            })
        })
        .map_err(super::local_state_unavailable)?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(super::local_state_unavailable)
}

pub(crate) fn run(
    sqlite_path: &Path,
    upgrade_dir: &Path,
) -> crate::ImResult<CanonicalUpgradeOutcome> {
    run_with_interrupt_after_phase(sqlite_path, upgrade_dir, None)
}

fn run_with_interrupt_after_phase(
    sqlite_path: &Path,
    upgrade_dir: &Path,
    interrupt_after: Option<CanonicalUpgradePhase>,
) -> crate::ImResult<CanonicalUpgradeOutcome> {
    let _lock = acquire_lock(upgrade_dir)?;
    recover_interrupted_cutover(sqlite_path, upgrade_dir)?;
    let detection = detect(sqlite_path)?;
    if detection.eligibility == CanonicalUpgradeEligibility::NotRequired {
        complete_existing_journal_if_needed(upgrade_dir)?;
        return Ok(CanonicalUpgradeOutcome::NotRequired(detection));
    }
    source::verify_release_0710_source(sqlite_path, Some(&detection.source_fingerprint))?;
    let existing = load_journal(upgrade_dir)?;
    let upgrade_id = existing
        .as_ref()
        .filter(|journal| {
            journal.source_schema_version == detection.source_schema_version
                && journal.target_schema_version == detection.target_schema_version
                && journal.source_fingerprint == detection.source_fingerprint
                && journal.phase != CanonicalUpgradePhase::Completed
        })
        .map(|journal| journal.upgrade_id.clone())
        .unwrap_or_else(new_upgrade_id);
    let mut journal = CanonicalUpgradeJournal {
        format_version: 1,
        upgrade_id: upgrade_id.clone(),
        source_schema_version: detection.source_schema_version,
        target_schema_version: detection.target_schema_version,
        source_fingerprint: detection.source_fingerprint.clone(),
        phase: CanonicalUpgradePhase::Detected,
        backup_file: format!("backups/{upgrade_id}/im.sqlite"),
        shadow_file: format!("canonical-{upgrade_id}.shadow.sqlite"),
        rollback_file: format!("rollbacks/{upgrade_id}/im.sqlite"),
        updated_at_unix: now_unix(),
    };
    write_journal(upgrade_dir, &journal)?;
    interrupt_if_requested(interrupt_after, journal.phase)?;
    journal.phase = CanonicalUpgradePhase::PreflightPassed;
    journal.updated_at_unix = now_unix();
    write_journal(upgrade_dir, &journal)?;
    interrupt_if_requested(interrupt_after, journal.phase)?;

    let backup_path = upgrade_dir.join(&journal.backup_file);
    let backup = if backup_path.exists() {
        source::verify_release_0710_source(&backup_path, Some(&detection.source_fingerprint))?;
        VerifiedBackup {
            path: backup_path,
            source_fingerprint: detection.source_fingerprint.clone(),
        }
    } else {
        create_verified_backup(
            sqlite_path,
            upgrade_dir,
            &detection.source_fingerprint,
            &upgrade_id,
        )?
    };
    journal.phase = CanonicalUpgradePhase::BackupVerified;
    journal.updated_at_unix = now_unix();
    write_journal(upgrade_dir, &journal)?;
    interrupt_if_requested(interrupt_after, journal.phase)?;

    let shadow_path = create_shadow_from_source(sqlite_path, upgrade_dir, &upgrade_id)?;
    let mut report = migrate::migrate_shadow(&shadow_path)?;
    report.backup_path = backup.path;
    journal.phase = CanonicalUpgradePhase::ShadowMigrated;
    journal.updated_at_unix = now_unix();
    write_journal(upgrade_dir, &journal)?;
    interrupt_if_requested(interrupt_after, journal.phase)?;
    validate_target_file(&shadow_path)?;
    journal.phase = CanonicalUpgradePhase::ValidationPassed;
    journal.updated_at_unix = now_unix();
    write_journal(upgrade_dir, &journal)?;
    interrupt_if_requested(interrupt_after, journal.phase)?;

    journal.phase = CanonicalUpgradePhase::CutoverStarted;
    journal.updated_at_unix = now_unix();
    write_journal(upgrade_dir, &journal)?;
    interrupt_if_requested(interrupt_after, journal.phase)?;
    cutover(sqlite_path, upgrade_dir, &journal)?;
    validate_target_file(sqlite_path)?;
    journal.phase = CanonicalUpgradePhase::Completed;
    journal.updated_at_unix = now_unix();
    write_journal(upgrade_dir, &journal)?;
    interrupt_if_requested(interrupt_after, journal.phase)?;
    Ok(CanonicalUpgradeOutcome::Completed(report))
}

fn interrupt_if_requested(
    requested: Option<CanonicalUpgradePhase>,
    durable_phase: CanonicalUpgradePhase,
) -> crate::ImResult<()> {
    if requested == Some(durable_phase) {
        return Err(upgrade_failed("test_interrupt", "simulated_process_exit"));
    }
    Ok(())
}

pub(crate) fn acquire_lock(upgrade_dir: &Path) -> crate::ImResult<CanonicalUpgradeLock> {
    create_private_dir(upgrade_dir)?;
    let lock_path = upgrade_dir.join("canonical-conversation-upgrade.lock");
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&lock_path)
        .map_err(|_| upgrade_failed("lock", "lock_file_unavailable"))?;
    set_private_file_permissions(&lock_path)?;
    file.try_lock_exclusive()
        .map_err(|_| crate::ImError::LocalStateUpgradeInProgress)?;
    Ok(CanonicalUpgradeLock { file })
}

pub(crate) fn restore_verified_backup(
    sqlite_path: &Path,
    upgrade_dir: &Path,
) -> crate::ImResult<CanonicalRestoreReport> {
    let _lock = acquire_lock(upgrade_dir)?;
    let mut journal = load_journal(upgrade_dir)?
        .ok_or_else(|| upgrade_failed("restore", "upgrade_journal_missing"))?;
    if !matches!(
        journal.phase,
        CanonicalUpgradePhase::Completed
            | CanonicalUpgradePhase::RestoreStarted
            | CanonicalUpgradePhase::Restored
    ) {
        return Err(upgrade_failed("restore", "completed_cutover_required"));
    }
    let backup_path = upgrade_dir.join(&journal.backup_file);
    source::verify_release_0710_source(&backup_path, Some(&journal.source_fingerprint))?;

    if sqlite_path.exists()
        && source::verify_release_0710_source(sqlite_path, Some(&journal.source_fingerprint))
            .is_ok()
    {
        journal.phase = CanonicalUpgradePhase::Restored;
        journal.updated_at_unix = now_unix();
        write_journal(upgrade_dir, &journal)?;
        return Ok(CanonicalRestoreReport {
            restored_schema_version: RELEASE_0710_SCHEMA_VERSION,
            target_safety_copy: restore_target_path(upgrade_dir, &journal),
        });
    }

    if sqlite_path.exists() {
        validate_target_file(sqlite_path)?;
    }
    let restore_shadow = upgrade_dir.join(format!("restore-{}.shadow.sqlite", journal.upgrade_id));
    remove_sqlite_file_set(&restore_shadow)?;
    online_backup(&backup_path, &restore_shadow, "restore_backup")?;
    set_private_file_permissions(&restore_shadow)?;
    source::verify_release_0710_source(&restore_shadow, Some(&journal.source_fingerprint))?;

    journal.phase = CanonicalUpgradePhase::RestoreStarted;
    journal.updated_at_unix = now_unix();
    write_journal(upgrade_dir, &journal)?;

    let target_safety_copy = restore_target_path(upgrade_dir, &journal);
    if sqlite_path.exists() && !target_safety_copy.exists() {
        move_sqlite_file_set(sqlite_path, &target_safety_copy, "restore_target_move")?;
    }
    if !sqlite_path.exists() {
        if let Err(error) =
            move_sqlite_file_set(&restore_shadow, sqlite_path, "restore_source_move")
        {
            if target_safety_copy.exists() {
                let _ = move_sqlite_file_set(
                    &target_safety_copy,
                    sqlite_path,
                    "restore_target_rollback",
                );
            }
            return Err(error);
        }
    }
    source::verify_release_0710_source(sqlite_path, Some(&journal.source_fingerprint))?;
    journal.phase = CanonicalUpgradePhase::Restored;
    journal.updated_at_unix = now_unix();
    write_journal(upgrade_dir, &journal)?;
    Ok(CanonicalRestoreReport {
        restored_schema_version: RELEASE_0710_SCHEMA_VERSION,
        target_safety_copy,
    })
}

fn restore_target_path(upgrade_dir: &Path, journal: &CanonicalUpgradeJournal) -> PathBuf {
    upgrade_dir
        .join("restores")
        .join(&journal.upgrade_id)
        .join("target-before-restore.sqlite")
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

fn cutover(
    sqlite_path: &Path,
    upgrade_dir: &Path,
    journal: &CanonicalUpgradeJournal,
) -> crate::ImResult<()> {
    let shadow_path = upgrade_dir.join(&journal.shadow_file);
    validate_target_file(&shadow_path)?;
    let rollback_path = upgrade_dir.join(&journal.rollback_file);
    let rollback_dir = rollback_path
        .parent()
        .ok_or_else(|| upgrade_failed("cutover", "rollback_directory_invalid"))?;
    create_private_dir(rollback_dir)?;
    if !rollback_path.exists() {
        move_sqlite_file_set(sqlite_path, &rollback_path, "cutover_source_move")?;
    }
    if !sqlite_path.exists() {
        move_sqlite_file_set(&shadow_path, sqlite_path, "cutover_shadow_move")?;
        set_private_file_permissions(sqlite_path)?;
    }
    Ok(())
}

fn recover_interrupted_cutover(sqlite_path: &Path, upgrade_dir: &Path) -> crate::ImResult<()> {
    let Some(mut journal) = load_journal(upgrade_dir)? else {
        return Ok(());
    };
    if journal.phase != CanonicalUpgradePhase::CutoverStarted {
        return Ok(());
    }
    if sqlite_path.exists() {
        if validate_target_file(sqlite_path).is_ok() {
            journal.phase = CanonicalUpgradePhase::Completed;
            journal.updated_at_unix = now_unix();
            write_journal(upgrade_dir, &journal)?;
        }
        return Ok(());
    }
    let shadow_path = upgrade_dir.join(&journal.shadow_file);
    if shadow_path.exists() && validate_target_file(&shadow_path).is_ok() {
        move_sqlite_file_set(&shadow_path, sqlite_path, "cutover_recovery")?;
        validate_target_file(sqlite_path)?;
        journal.phase = CanonicalUpgradePhase::Completed;
        journal.updated_at_unix = now_unix();
        write_journal(upgrade_dir, &journal)?;
        return Ok(());
    }
    let rollback_path = upgrade_dir.join(&journal.rollback_file);
    if rollback_path.exists() {
        move_sqlite_file_set(&rollback_path, sqlite_path, "cutover_rollback")?;
        return Ok(());
    }
    Err(upgrade_failed(
        "cutover",
        "cutover_recovery_artifact_missing",
    ))
}

fn complete_existing_journal_if_needed(upgrade_dir: &Path) -> crate::ImResult<()> {
    let Some(mut journal) = load_journal(upgrade_dir)? else {
        return Ok(());
    };
    if journal.phase != CanonicalUpgradePhase::Completed {
        journal.phase = CanonicalUpgradePhase::Completed;
        journal.updated_at_unix = now_unix();
        write_journal(upgrade_dir, &journal)?;
    }
    Ok(())
}

fn validate_target_file(path: &Path) -> crate::ImResult<()> {
    let connection = rusqlite::Connection::open(path)
        .map_err(|_| upgrade_failed("validation", "target_open_failed"))?;
    super::schema::ensure_schema(&connection)?;
    let version = super::schema::current_schema_version(&connection)?;
    if version != super::schema::SCHEMA_VERSION {
        return Err(upgrade_failed(
            "validation",
            "target_schema_version_invalid",
        ));
    }
    let integrity: String = connection
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .map_err(|_| upgrade_failed("validation", "target_integrity_check_failed"))?;
    if integrity != "ok" {
        return Err(upgrade_failed(
            "validation",
            "target_integrity_check_failed",
        ));
    }
    Ok(())
}

fn move_sqlite_file_set(source: &Path, target: &Path, phase: &str) -> crate::ImResult<()> {
    if let Some(parent) = target.parent() {
        create_private_dir(parent)?;
    }
    for (source_file, target_file) in [
        (source.to_path_buf(), target.to_path_buf()),
        (
            PathBuf::from(format!("{}-wal", source.display())),
            PathBuf::from(format!("{}-wal", target.display())),
        ),
        (
            PathBuf::from(format!("{}-shm", source.display())),
            PathBuf::from(format!("{}-shm", target.display())),
        ),
    ] {
        if source_file.exists() {
            if target_file.exists() {
                return Err(upgrade_failed(phase, "target_already_exists"));
            }
            fs::rename(source_file, target_file)
                .map_err(|_| upgrade_failed(phase, "file_move_failed"))?;
        }
    }
    Ok(())
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

fn set_private_file_permissions(_path: &Path) -> crate::ImResult<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(_path, fs::Permissions::from_mode(0o600))
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
            target_schema_version: super::super::schema::SCHEMA_VERSION,
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
        source::copy_release_0710_fixture(&source_path);
        let source_db = rusqlite::Connection::open(&source_path).unwrap();
        source_db
            .pragma_update(None, "journal_mode", "WAL")
            .unwrap();
        source_db
            .execute(
                r#"INSERT INTO messages
(owner_identity_id, owner_did, msg_id, conversation_id, thread_id,
 sender_did, receiver_did, group_did, stored_at)
VALUES ('owner', 'did:example:owner', 'message-1', 'dm:legacy', 'dm:legacy',
        'did:example:peer', 'did:example:owner', '', 'test')"#,
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
                .query_row(
                    "SELECT COUNT(*) FROM messages WHERE msg_id = 'message-1'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            1
        );
    }

    #[test]
    fn runner_upgrades_once_preserves_backup_and_is_idempotent() {
        let directory = tempfile::tempdir().unwrap();
        let sqlite_path = directory.path().join("im.sqlite");
        let upgrade_dir = directory.path().join("upgrade");
        source::copy_release_0710_fixture(&sqlite_path);

        let first = run(&sqlite_path, &upgrade_dir).unwrap();
        let CanonicalUpgradeOutcome::Completed(report) = first else {
            panic!("expected completed upgrade");
        };
        assert_eq!(report.migrated_personas, 1);
        assert!(report.backup_path.exists());
        assert_eq!(
            rusqlite::Connection::open(&report.backup_path)
                .unwrap()
                .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
                .unwrap(),
            27
        );
        let live = rusqlite::Connection::open(&sqlite_path).unwrap();
        assert_eq!(
            live.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
                .unwrap(),
            super::super::schema::SCHEMA_VERSION
        );
        let alias_count = live
            .query_row("SELECT COUNT(*) FROM conversation_aliases", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap();
        drop(live);

        assert!(matches!(
            run(&sqlite_path, &upgrade_dir).unwrap(),
            CanonicalUpgradeOutcome::NotRequired(_)
        ));
        assert_eq!(
            rusqlite::Connection::open(&sqlite_path)
                .unwrap()
                .query_row("SELECT COUNT(*) FROM conversation_aliases", [], |row| {
                    row.get::<_, i64>(0)
                })
                .unwrap(),
            alias_count
        );
        assert_eq!(
            load_journal(&upgrade_dir).unwrap().unwrap().phase,
            CanonicalUpgradePhase::Completed
        );
    }

    #[test]
    fn runner_recovers_cutover_gap_without_losing_source_or_shadow() {
        let directory = tempfile::tempdir().unwrap();
        let sqlite_path = directory.path().join("im.sqlite");
        let upgrade_dir = directory.path().join("upgrade");
        source::copy_release_0710_fixture(&sqlite_path);
        let detection = detect(&sqlite_path).unwrap();
        let upgrade_id = "recovery-1";
        let shadow_path =
            create_shadow_from_source(&sqlite_path, &upgrade_dir, upgrade_id).unwrap();
        migrate::migrate_shadow(&shadow_path).unwrap();
        let journal = CanonicalUpgradeJournal {
            format_version: 1,
            upgrade_id: upgrade_id.to_owned(),
            source_schema_version: 27,
            target_schema_version: super::super::schema::SCHEMA_VERSION,
            source_fingerprint: detection.source_fingerprint,
            phase: CanonicalUpgradePhase::CutoverStarted,
            backup_file: format!("backups/{upgrade_id}/im.sqlite"),
            shadow_file: format!("canonical-{upgrade_id}.shadow.sqlite"),
            rollback_file: format!("rollbacks/{upgrade_id}/im.sqlite"),
            updated_at_unix: now_unix(),
        };
        write_journal(&upgrade_dir, &journal).unwrap();
        let rollback_path = upgrade_dir.join(&journal.rollback_file);
        move_sqlite_file_set(&sqlite_path, &rollback_path, "test_cutover").unwrap();
        assert!(!sqlite_path.exists());

        assert!(matches!(
            run(&sqlite_path, &upgrade_dir).unwrap(),
            CanonicalUpgradeOutcome::NotRequired(_)
        ));
        assert!(sqlite_path.exists());
        assert!(rollback_path.exists());
        assert_eq!(
            rusqlite::Connection::open(&sqlite_path)
                .unwrap()
                .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
                .unwrap(),
            super::super::schema::SCHEMA_VERSION
        );
    }

    #[test]
    fn runner_resumes_after_every_durable_upgrade_phase() {
        for phase in [
            CanonicalUpgradePhase::Detected,
            CanonicalUpgradePhase::PreflightPassed,
            CanonicalUpgradePhase::BackupVerified,
            CanonicalUpgradePhase::ShadowMigrated,
            CanonicalUpgradePhase::ValidationPassed,
            CanonicalUpgradePhase::CutoverStarted,
            CanonicalUpgradePhase::Completed,
        ] {
            let directory = tempfile::tempdir().unwrap();
            let sqlite_path = directory.path().join("im.sqlite");
            let upgrade_dir = directory.path().join("upgrade");
            source::copy_release_0710_fixture(&sqlite_path);

            assert!(
                run_with_interrupt_after_phase(&sqlite_path, &upgrade_dir, Some(phase)).is_err()
            );
            assert_eq!(
                load_journal(&upgrade_dir).unwrap().unwrap().phase,
                phase,
                "phase {phase:?} must be durable before interruption"
            );

            let resumed = run(&sqlite_path, &upgrade_dir).unwrap();
            assert!(matches!(
                resumed,
                CanonicalUpgradeOutcome::Completed(_) | CanonicalUpgradeOutcome::NotRequired(_)
            ));
            let db = rusqlite::Connection::open(&sqlite_path).unwrap();
            assert_eq!(
                db.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
                    .unwrap(),
                super::super::schema::SCHEMA_VERSION
            );
            let counts = (
                db.query_row("SELECT COUNT(*) FROM messages", [], |row| {
                    row.get::<_, i64>(0)
                })
                .unwrap(),
                db.query_row("SELECT COUNT(*) FROM e2ee_outbox", [], |row| {
                    row.get::<_, i64>(0)
                })
                .unwrap(),
                db.query_row("SELECT COUNT(*) FROM group_members", [], |row| {
                    row.get::<_, i64>(0)
                })
                .unwrap(),
                db.query_row("SELECT COUNT(*) FROM conversation_aliases", [], |row| {
                    row.get::<_, i64>(0)
                })
                .unwrap(),
            );
            drop(db);

            assert!(matches!(
                run(&sqlite_path, &upgrade_dir).unwrap(),
                CanonicalUpgradeOutcome::NotRequired(_)
            ));
            let db = rusqlite::Connection::open(&sqlite_path).unwrap();
            let repeated_counts = (
                db.query_row("SELECT COUNT(*) FROM messages", [], |row| {
                    row.get::<_, i64>(0)
                })
                .unwrap(),
                db.query_row("SELECT COUNT(*) FROM e2ee_outbox", [], |row| {
                    row.get::<_, i64>(0)
                })
                .unwrap(),
                db.query_row("SELECT COUNT(*) FROM group_members", [], |row| {
                    row.get::<_, i64>(0)
                })
                .unwrap(),
                db.query_row("SELECT COUNT(*) FROM conversation_aliases", [], |row| {
                    row.get::<_, i64>(0)
                })
                .unwrap(),
            );
            assert_eq!(
                repeated_counts, counts,
                "phase {phase:?} resume must be idempotent"
            );
        }
    }

    #[test]
    fn restore_resumes_after_target_was_moved_to_safety_copy() {
        let directory = tempfile::tempdir().unwrap();
        let sqlite_path = directory.path().join("im.sqlite");
        let upgrade_dir = directory.path().join("upgrade");
        source::copy_release_0710_fixture(&sqlite_path);
        run(&sqlite_path, &upgrade_dir).unwrap();

        let mut journal = load_journal(&upgrade_dir).unwrap().unwrap();
        journal.phase = CanonicalUpgradePhase::RestoreStarted;
        write_journal(&upgrade_dir, &journal).unwrap();
        let target_safety_copy = restore_target_path(&upgrade_dir, &journal);
        move_sqlite_file_set(&sqlite_path, &target_safety_copy, "test_restore_gap").unwrap();
        assert!(!sqlite_path.exists());

        let report = restore_verified_backup(&sqlite_path, &upgrade_dir).unwrap();
        assert_eq!(report.target_safety_copy, target_safety_copy);
        assert!(report.target_safety_copy.exists());
        assert_eq!(
            rusqlite::Connection::open(&sqlite_path)
                .unwrap()
                .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
                .unwrap(),
            RELEASE_0710_SCHEMA_VERSION
        );
        assert_eq!(
            load_journal(&upgrade_dir).unwrap().unwrap().phase,
            CanonicalUpgradePhase::Restored
        );
    }
}
