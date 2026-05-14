use super::fsutil;
use super::types::Paths;
use crate::{config, store};
use std::fmt;
use std::fs;
use std::path::Path;
use time::{OffsetDateTime, UtcOffset};

#[derive(Debug)]
pub enum BackupError {
    CreateBackupDir(std::io::Error),
    Copy(std::io::Error),
    Sync(std::io::Error),
    CreateSqliteBackupDir(std::io::Error),
    RemoveExistingSqliteBackup(std::io::Error),
    OpenSqliteSource(store::StoreError),
    VacuumSqliteIntoBackup(rusqlite::Error),
}

impl fmt::Display for BackupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CreateBackupDir(err) => write!(f, "create backup dir: {err}"),
            Self::Copy(err) | Self::Sync(err) => write!(f, "{err}"),
            Self::CreateSqliteBackupDir(err) => write!(f, "create sqlite backup dir: {err}"),
            Self::RemoveExistingSqliteBackup(err) => {
                write!(f, "remove existing sqlite backup: {err}")
            }
            Self::OpenSqliteSource(err) => write!(f, "open sqlite source for backup: {err}"),
            Self::VacuumSqliteIntoBackup(err) => write!(f, "vacuum sqlite into backup: {err}"),
        }
    }
}

impl std::error::Error for BackupError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::CreateBackupDir(err)
            | Self::Copy(err)
            | Self::Sync(err)
            | Self::CreateSqliteBackupDir(err)
            | Self::RemoveExistingSqliteBackup(err) => Some(err),
            Self::OpenSqliteSource(err) => Some(err),
            Self::VacuumSqliteIntoBackup(err) => Some(err),
        }
    }
}

pub fn create_backup(paths: &Paths, backup_id: &str) -> Result<String, BackupError> {
    let backup_id = if backup_id.trim().is_empty() {
        format_time_layout(OffsetDateTime::now_utc())
    } else {
        backup_id.to_string()
    };
    let backup_dir = Path::new(&paths.backup_root).join(backup_id);
    fs::create_dir_all(&backup_dir).map_err(BackupError::CreateBackupDir)?;

    if fsutil::file_exists(&paths.config_file) {
        fsutil::copy_file(
            &paths.config_file,
            backup_dir
                .join("config.yaml.bak")
                .to_string_lossy()
                .as_ref(),
            0o600,
        )
        .map_err(BackupError::Copy)?;
    }
    if fsutil::file_exists(&paths.legacy_config_file) {
        fsutil::copy_file(
            &paths.legacy_config_file,
            backup_dir
                .join("config.json.bak")
                .to_string_lossy()
                .as_ref(),
            0o600,
        )
        .map_err(BackupError::Copy)?;
    }
    if fsutil::dir_exists(&paths.identity_dir) {
        fsutil::copy_tree(
            &paths.identity_dir,
            backup_dir.join("identities").to_string_lossy().as_ref(),
        )
        .map_err(BackupError::Copy)?;
    }
    if fsutil::file_exists(&paths.database_file) {
        backup_sqlite_database(
            &paths.database_file,
            backup_dir
                .join("awiki-cli.db.bak")
                .to_string_lossy()
                .as_ref(),
        )?;
    }
    if fsutil::file_exists(&paths.meta_path) {
        fsutil::copy_file(
            &paths.meta_path,
            backup_dir.join("meta.json.bak").to_string_lossy().as_ref(),
            0o600,
        )
        .map_err(BackupError::Copy)?;
    }
    if fsutil::file_exists(&paths.journal_path) {
        fsutil::copy_file(
            &paths.journal_path,
            backup_dir
                .join("upgrade_journal.json.bak")
                .to_string_lossy()
                .as_ref(),
            0o600,
        )
        .map_err(BackupError::Copy)?;
    }

    let parent = backup_dir.parent().unwrap_or_else(|| Path::new("."));
    fsutil::sync_directory(parent).map_err(BackupError::Sync)?;
    Ok(backup_dir.to_string_lossy().into_owned())
}

pub fn backup_sqlite_database(
    source_path: &str,
    destination_path: &str,
) -> Result<(), BackupError> {
    let destination = Path::new(destination_path);
    let parent = destination.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(BackupError::CreateSqliteBackupDir)?;
    match fs::remove_file(destination) {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => return Err(BackupError::RemoveExistingSqliteBackup(err)),
    }
    let paths = config::Paths {
        database_file: source_path.to_string(),
        ..empty_config_paths()
    };
    let connection = store::open(&paths).map_err(BackupError::OpenSqliteSource)?;
    let statement = format!(
        "VACUUM INTO '{}'",
        escape_sqlite_string(destination.to_string_lossy().as_ref())
    );
    connection
        .execute_batch(&statement)
        .map_err(BackupError::VacuumSqliteIntoBackup)?;
    Ok(())
}

fn empty_config_paths() -> config::Paths {
    config::Paths {
        workspace_home_dir: String::new(),
        root_dir: String::new(),
        config_dir: String::new(),
        data_dir: String::new(),
        state_dir: String::new(),
        cache_dir: String::new(),
        logs_dir: String::new(),
        config_file: String::new(),
        identity_dir: String::new(),
        database_file: String::new(),
        legacy_credentials_dir: String::new(),
        legacy_data_dir: String::new(),
    }
}

fn escape_sqlite_string(value: &str) -> String {
    value.replace('\'', "''")
}

fn format_time_layout(value: OffsetDateTime) -> String {
    let value = value.to_offset(UtcOffset::UTC);
    let month: u8 = value.month().into();
    format!(
        "{:04}{:02}{:02}T{:02}{:02}{:02}Z",
        value.year(),
        month,
        value.day(),
        value.hour(),
        value.minute(),
        value.second()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_sqlite_string_doubles_single_quotes_like_go() {
        assert_eq!(escape_sqlite_string("plain"), "plain");
        assert_eq!(escape_sqlite_string("a'b''c"), "a''b''''c");
    }
}
