use super::fsutil;
use super::legacy_identity as identity;
use super::legacy_sqlite as store;
use super::migration_v0_to_v1::validate_sqlite_health;
use super::upgrader::{Context, MigrationError};
use crate::workspace_config;
use std::path::PathBuf;

pub(crate) fn apply_workspace_v3_to_v4_owner_identity_local_state(
    context: &mut Context,
) -> Result<(), MigrationError> {
    let mut connection = store::open(&context.resolved.paths)?;
    let version = store::current_schema_version(&connection)?;
    if version == 0 || version == im_core::compat::local_state::SCHEMA_VERSION {
        im_core::compat::local_state::ensure_identity_owned_schema(&connection)
            .map_err(store_error_from_im_core)?;
        record_identity_did_history_for_workspace(&mut connection, &context.resolved.paths)?;
        validate_identity_owned_invariants(&connection)?;
        return Ok(());
    }
    if version > im_core::compat::local_state::SCHEMA_VERSION {
        return Err(MigrationError::Message(format!(
            "workspace v3 -> v4 遇到较新的本地 SQLite schema {version}，当前仅支持 {}，拒绝重建数据库",
            im_core::compat::local_state::SCHEMA_VERSION
        )));
    }

    drop(connection);
    rebuild_clean_identity_owned_database(context, version)
}

pub(crate) fn validate_workspace_v3_to_v4_owner_identity_local_state(
    context: &Context,
) -> Result<(), MigrationError> {
    if !fsutil::file_exists(&context.paths.database_file) {
        return Ok(());
    }
    let connection = store::open_read_only(&context.paths.database_file)?;
    let version = store::current_schema_version(&connection)?;
    if version != im_core::compat::local_state::SCHEMA_VERSION {
        return Err(MigrationError::Message(format!(
            "sqlite schema version = {version}, want {}",
            im_core::compat::local_state::SCHEMA_VERSION
        )));
    }
    validate_sqlite_health(&connection)?;
    validate_identity_owned_invariants(&connection)
}

fn record_identity_did_history_for_workspace(
    connection: &mut rusqlite::Connection,
    paths: &workspace_config::Paths,
) -> Result<(), MigrationError> {
    let manager = identity::Manager::new(paths.clone());
    let identities = manager.list()?;
    for summary in identities {
        if summary.unique_id.trim().is_empty() || summary.did.trim().is_empty() {
            continue;
        }
        im_core::compat::local_state::record_identity_did_history_transition::<String>(
            connection,
            &summary.unique_id,
            &summary.did,
            &[],
        )
        .map_err(store_error_from_im_core)?;
    }
    Ok(())
}

fn validate_identity_owned_invariants(
    connection: &rusqlite::Connection,
) -> Result<(), MigrationError> {
    let violations = im_core::compat::local_state::identity_owned_owner_invariants(connection)
        .map_err(store_error_from_im_core)?;
    if violations.is_empty() {
        return Ok(());
    }
    let summary = violations
        .into_iter()
        .map(|violation| {
            format!(
                "{}:{}:{}",
                violation.table, violation.invariant, violation.row_count
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    Err(MigrationError::Message(format!(
        "local state owner identity invariant violation: {summary}"
    )))
}

fn rebuild_clean_identity_owned_database(
    context: &mut Context,
    source_version: i64,
) -> Result<(), MigrationError> {
    if context.backup_dir.trim().is_empty() {
        return Err(MigrationError::Message(
            "workspace v3 -> v4 需要先创建 workspace backup 才能重建本地 SQLite".to_string(),
        ));
    }
    let sqlite_backup = PathBuf::from(&context.backup_dir).join("awiki-cli.db.bak");
    if !sqlite_backup.is_file() {
        return Err(MigrationError::Message(
            "workspace v3 -> v4 找不到重建所需的 SQLite backup，拒绝删除旧数据库".to_string(),
        ));
    }
    remove_sqlite_file_set(&context.paths.database_file)?;
    let mut connection = store::open(&context.resolved.paths)?;
    im_core::compat::local_state::ensure_identity_owned_schema(&connection)
        .map_err(store_error_from_im_core)?;
    record_identity_did_history_for_workspace(&mut connection, &context.resolved.paths)?;
    validate_identity_owned_invariants(&connection)?;
    context.warnings.push(format!(
        "workspace v3 -> v4 已在备份后将旧本地 SQLite schema {source_version} 重建为干净 schema {}；旧业务行未按 DID/credential/path 静默迁移",
        im_core::compat::local_state::SCHEMA_VERSION
    ));
    Ok(())
}

fn remove_sqlite_file_set(database_file: &str) -> Result<(), MigrationError> {
    for path in sqlite_file_set(database_file) {
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => {
                return Err(MigrationError::Store(store::StoreError::Io(
                    std::io::Error::new(err.kind(), "remove old local SQLite file"),
                )));
            }
        }
    }
    Ok(())
}

fn sqlite_file_set(database_file: &str) -> Vec<PathBuf> {
    let database = PathBuf::from(database_file);
    vec![
        database.clone(),
        PathBuf::from(format!("{database_file}-wal")),
        PathBuf::from(format!("{database_file}-shm")),
        PathBuf::from(format!("{database_file}-journal")),
    ]
}

fn store_error_from_im_core(err: im_core::ImError) -> MigrationError {
    match err {
        im_core::ImError::LocalStateUnavailable { detail } => {
            MigrationError::Store(store::StoreError::Invalid(detail))
        }
        other => MigrationError::Store(store::StoreError::Invalid(other.to_string())),
    }
}
