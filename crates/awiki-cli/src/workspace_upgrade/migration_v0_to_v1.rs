use crate::workspace_config;
use crate::workspace_upgrade::fsutil;
use crate::workspace_upgrade::legacy_identity as identity;
use crate::workspace_upgrade::legacy_sqlite as store;
use crate::workspace_upgrade::upgrader::{Context, MigrationError};
use rusqlite::Connection;
use std::fmt;

#[derive(Debug)]
pub enum RefreshResolvedConfigError {
    Required,
    ReadConfig(String),
}

impl fmt::Display for RefreshResolvedConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Required => f.write_str("resolved config is required"),
            Self::ReadConfig(err) => f.write_str(err),
        }
    }
}

impl std::error::Error for RefreshResolvedConfigError {}

pub fn refresh_resolved_config(
    current: &workspace_config::Resolved,
) -> Result<workspace_config::Resolved, RefreshResolvedConfigError> {
    refresh_resolved_config_optional(Some(current))
}

pub fn refresh_resolved_config_optional(
    current: Option<&workspace_config::Resolved>,
) -> Result<workspace_config::Resolved, RefreshResolvedConfigError> {
    let current = current.ok_or(RefreshResolvedConfigError::Required)?;
    let mut refreshed = current.clone();
    let (file_config, exists, error) =
        workspace_config::read_file_config(&current.paths.config_file);
    if !error.is_empty() {
        return Err(RefreshResolvedConfigError::ReadConfig(error));
    }
    refreshed.config_exists = exists;
    refreshed.config_schema_version = file_config.schema_version;
    if !file_config.runtime.mode.trim().is_empty() {
        refreshed.runtime_mode = file_config.runtime.mode.trim().to_string();
    }
    if !file_config.runtime.socket_path.trim().is_empty() {
        refreshed.runtime_socket_path = file_config.runtime.socket_path.trim().to_string();
    }
    if !file_config.output.format.trim().is_empty() {
        refreshed.output_format = file_config.output.format.trim().to_string();
    }
    if let Some(no_color) = file_config.output.no_color {
        refreshed.no_color = no_color;
    }
    if !file_config.services.service_base_url.trim().is_empty() {
        refreshed.service_base_url =
            workspace_config::normalize_base_url(file_config.services.service_base_url.trim());
    }
    if !file_config.services.did_domain.trim().is_empty() {
        refreshed.did_domain = file_config.services.did_domain.trim().to_string();
    }
    if !file_config.services.anp_service_endpoint.trim().is_empty() {
        refreshed.anp_service_endpoint =
            file_config.services.anp_service_endpoint.trim().to_string();
    } else if refreshed.anp_service_endpoint.trim().is_empty() {
        refreshed.anp_service_endpoint =
            workspace_config::derive_anp_service_endpoint(&refreshed.service_base_url);
    }
    if !file_config.services.anp_service_did.trim().is_empty() {
        refreshed.anp_service_did = file_config.services.anp_service_did.trim().to_string();
    } else if refreshed.anp_service_did.trim().is_empty() {
        refreshed.anp_service_did =
            workspace_config::derive_anp_service_did(&refreshed.service_base_url);
    }
    if !file_config.services.mail_service_url.trim().is_empty() {
        refreshed.mail_service_url =
            workspace_config::normalize_base_url(file_config.services.mail_service_url.trim());
    } else if refreshed.mail_service_url.trim().is_empty() {
        refreshed.mail_service_url = refreshed.service_base_url.clone();
    }
    if !file_config.services.ca_bundle.trim().is_empty() {
        refreshed.ca_bundle = file_config.services.ca_bundle.trim().to_string();
    }
    Ok(refreshed)
}

pub fn ensure_target_store_schema(paths: &workspace_config::Paths) -> store::StoreResult<()> {
    let connection = store::open(paths)?;
    store::ensure_schema(&connection)
}

pub fn apply_workspace_v0_to_v1_config(context: &Context) -> Result<(), MigrationError> {
    apply_workspace_v0_to_v1_config_optional(Some(context))
}

pub fn apply_workspace_v0_to_v1_config_optional(
    context: Option<&Context>,
) -> Result<(), MigrationError> {
    let context = context.ok_or_else(|| {
        MigrationError::Message("workspace upgrade requires a resolved config".to_string())
    })?;
    let inspection = context.inspection.as_ref().ok_or_else(|| {
        MigrationError::Message("workspace upgrade inspection is required".to_string())
    })?;
    let detection = &inspection.detection;
    if detection.config_exists {
        workspace_config::ensure_config_schema_version(&context.paths.config_file)
            .map_err(|err| MigrationError::Message(err.to_string()))?;
    } else if detection.legacy_config_exists {
        let (mut legacy_file_config, _, error) =
            workspace_config::read_file_config(&context.paths.legacy_config_file);
        if !error.is_empty() {
            return Err(MigrationError::Message(error));
        }
        legacy_file_config.schema_version = workspace_config::CONFIG_SCHEMA_VERSION;
        workspace_config::write_file_config_raw(&context.paths.config_file, legacy_file_config)
            .map_err(|err| MigrationError::Message(err.to_string()))?;
        match std::fs::remove_file(&context.paths.legacy_config_file) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => {
                return Err(MigrationError::Message(format!(
                    "remove legacy config: {err}"
                )));
            }
        }
    } else if !detection.has_workspace && detection.legacy_settings_exists {
        let legacy_config =
            super::settings::load_legacy_settings(&context.paths.legacy_settings_path)
                .map_err(|err| MigrationError::Message(err.to_string()))?;
        let mut file_config = workspace_config::FileConfig {
            schema_version: workspace_config::CONFIG_SCHEMA_VERSION,
            ..Default::default()
        };
        file_config.runtime.mode = legacy_config.runtime_mode;
        file_config.services.service_base_url = legacy_config.service_base_url;
        file_config.services.did_domain = legacy_config.did_domain;
        workspace_config::write_file_config_raw(&context.paths.config_file, file_config)
            .map_err(|err| MigrationError::Message(err.to_string()))?;
    }
    Ok(())
}

pub fn apply_workspace_v0_to_v1_legacy_imports(
    context: &Context,
) -> Result<identity::ImportResult, MigrationError> {
    apply_workspace_v0_to_v1_legacy_imports_optional(Some(context))
}

pub fn apply_workspace_v0_to_v1_legacy_imports_optional(
    context: Option<&Context>,
) -> Result<identity::ImportResult, MigrationError> {
    let context = context.ok_or_else(|| {
        MigrationError::Message("workspace upgrade requires a resolved config".to_string())
    })?;
    let inspection = context.inspection.as_ref().ok_or_else(|| {
        MigrationError::Message("workspace upgrade inspection is required".to_string())
    })?;
    let detection = &inspection.detection;
    let mut imported_legacy = identity::ImportResult::default();

    if !detection.has_workspace && detection.has_legacy {
        let manager = identity::Manager::new(context.resolved.paths.clone());
        let legacy_scan = manager.scan_legacy()?;
        if legacy_scan.has_legacy {
            imported_legacy = manager.import_all_legacy()?;
        }

        let legacy_db = store::scan_legacy_database(&context.resolved.paths)?;
        if legacy_db.exists {
            let mut db = store::open(&context.resolved.paths)?;
            store::ensure_schema(&db)?;
            let owners = legacy_owner_lookup(&manager);
            store::import_legacy_database(&mut db, &context.resolved.paths, &owners)?;
        }
    }

    Ok(imported_legacy)
}

pub fn apply_workspace_v0_to_v1_local_state(
    context: &mut Context,
) -> Result<identity::ImportResult, MigrationError> {
    apply_workspace_v0_to_v1_local_state_optional(Some(context))
}

pub fn apply_workspace_v0_to_v1_local_state_optional(
    context: Option<&mut Context>,
) -> Result<identity::ImportResult, MigrationError> {
    let context = context.ok_or_else(|| {
        MigrationError::Message("workspace upgrade requires a resolved config".to_string())
    })?;
    apply_workspace_v0_to_v1_config(context)?;
    let imported_legacy = apply_workspace_v0_to_v1_legacy_imports(context)?;
    if fsutil::file_exists(&context.paths.database_file) {
        ensure_target_store_schema(&context.resolved.paths)?;
    }
    if !imported_legacy.imported.is_empty() {
        let refreshed = refresh_resolved_config(&context.resolved)
            .map_err(|err| MigrationError::Message(err.to_string()))?;
        context.resolved = refreshed;
        context.paths = super::resolve_paths(&context.resolved);
    }
    Ok(imported_legacy)
}

pub fn validate_workspace_v0_to_v1(context: &Context) -> Result<(), MigrationError> {
    validate_workspace_v0_to_v1_optional(Some(context))
}

pub fn validate_workspace_v0_to_v1_optional(
    context: Option<&Context>,
) -> Result<(), MigrationError> {
    let context = context.ok_or_else(|| {
        MigrationError::Message("workspace upgrade requires a resolved config".to_string())
    })?;
    if fsutil::file_exists(&context.paths.config_file) {
        let (file_config, _, error) =
            workspace_config::read_file_config(&context.paths.config_file);
        if !error.is_empty() {
            return Err(MigrationError::Message(error));
        }
        if file_config.schema_version != workspace_config::CONFIG_SCHEMA_VERSION {
            return Err(MigrationError::Message(format!(
                "config schema version = {}, want {}",
                file_config.schema_version,
                workspace_config::CONFIG_SCHEMA_VERSION
            )));
        }
    }
    if fsutil::file_exists(&context.paths.database_file) {
        let connection = store::open_read_only(&context.paths.database_file)?;
        let version = store::current_schema_version(&connection)?;
        if version != store::SCHEMA_VERSION {
            return Err(MigrationError::Message(format!(
                "sqlite schema version = {version}, want {}",
                store::SCHEMA_VERSION
            )));
        }
        validate_sqlite_health(&connection)?;
    }
    if context.inspection.as_ref().is_some_and(|inspection| {
        !inspection.detection.has_workspace && inspection.detection.legacy_identity_exists
    }) {
        let manager = identity::Manager::new(context.resolved.paths.clone());
        if manager.list()?.is_empty() {
            return Err(MigrationError::Message(
                "expected at least one imported identity after legacy upgrade".to_string(),
            ));
        }
    }
    Ok(())
}

pub fn validate_sqlite_health(connection: &Connection) -> Result<(), SQLiteHealthError> {
    expect_single_sqlite_ok(connection, "PRAGMA integrity_check")?;
    expect_sqlite_no_rows(connection, "PRAGMA foreign_key_check")
}

#[derive(Debug)]
pub enum SQLiteHealthError {
    Execute {
        query: &'static str,
        source: rusqlite::Error,
    },
    NoRows {
        query: &'static str,
    },
    Scan {
        query: &'static str,
        source: rusqlite::Error,
    },
    Failed {
        query: &'static str,
        result: String,
    },
    ForeignKeyViolations {
        query: &'static str,
    },
}

impl fmt::Display for SQLiteHealthError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Execute { query, source } => write!(f, "execute {query}: {source}"),
            Self::NoRows { query } => write!(f, "{query} returned no rows"),
            Self::Scan { query, source } => write!(f, "scan {query} result: {source}"),
            Self::Failed { query, result } => write!(f, "{query} failed: {result}"),
            Self::ForeignKeyViolations { query } => {
                write!(f, "{query} returned foreign key violations")
            }
        }
    }
}

impl std::error::Error for SQLiteHealthError {}

fn expect_single_sqlite_ok(
    connection: &Connection,
    query: &'static str,
) -> Result<(), SQLiteHealthError> {
    let mut statement = connection
        .prepare(query)
        .map_err(|source| SQLiteHealthError::Execute { query, source })?;
    let mut rows = statement
        .query([])
        .map_err(|source| SQLiteHealthError::Execute { query, source })?;
    let Some(row) = rows
        .next()
        .map_err(|source| SQLiteHealthError::Execute { query, source })?
    else {
        return Err(SQLiteHealthError::NoRows { query });
    };
    let result: String = row
        .get(0)
        .map_err(|source| SQLiteHealthError::Scan { query, source })?;
    let trimmed = result.trim();
    if !trimmed.eq_ignore_ascii_case("ok") && !trimmed.is_empty() {
        return Err(SQLiteHealthError::Failed { query, result });
    }
    Ok(())
}

fn expect_sqlite_no_rows(
    connection: &Connection,
    query: &'static str,
) -> Result<(), SQLiteHealthError> {
    let mut statement = connection
        .prepare(query)
        .map_err(|source| SQLiteHealthError::Execute { query, source })?;
    let mut rows = statement
        .query([])
        .map_err(|source| SQLiteHealthError::Execute { query, source })?;
    if rows
        .next()
        .map_err(|source| SQLiteHealthError::Execute { query, source })?
        .is_some()
    {
        return Err(SQLiteHealthError::ForeignKeyViolations { query });
    }
    Ok(())
}

fn legacy_owner_lookup(manager: &identity::Manager) -> store::LegacyOwnerLookup {
    let entries = manager
        .list()
        .unwrap_or_default()
        .into_iter()
        .map(|summary| (summary.identity_name, summary.did, summary.is_default));
    store::LegacyOwnerLookup::from_entries(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expect_single_sqlite_ok_matches_go_result_rules() {
        let connection = Connection::open_in_memory().expect("open memory db");
        expect_single_sqlite_ok(&connection, "SELECT 'ok'").expect("ok result");
        expect_single_sqlite_ok(&connection, "SELECT ' OK '").expect("case-insensitive ok");
        expect_single_sqlite_ok(&connection, "SELECT ''").expect("empty result is accepted");

        let err = expect_single_sqlite_ok(&connection, "SELECT 'not ok'")
            .expect_err("non-ok should fail");
        assert_eq!(err.to_string(), "SELECT 'not ok' failed: not ok");

        let err = expect_single_sqlite_ok(&connection, "SELECT 1 WHERE 0")
            .expect_err("no rows should fail");
        assert_eq!(err.to_string(), "SELECT 1 WHERE 0 returned no rows");
    }

    #[test]
    fn validate_workspace_v0_to_v1_optional_keeps_go_required_guard() {
        let err = validate_workspace_v0_to_v1_optional(None)
            .expect_err("missing context should match Go guard");
        assert_eq!(
            err.to_string(),
            "workspace upgrade requires a resolved config"
        );
    }
}
