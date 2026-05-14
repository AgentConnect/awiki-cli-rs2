use crate::config;
use crate::store;
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
    current: &config::Resolved,
) -> Result<config::Resolved, RefreshResolvedConfigError> {
    refresh_resolved_config_optional(Some(current))
}

pub fn refresh_resolved_config_optional(
    current: Option<&config::Resolved>,
) -> Result<config::Resolved, RefreshResolvedConfigError> {
    let current = current.ok_or(RefreshResolvedConfigError::Required)?;
    let mut refreshed = current.clone();
    let (file_config, exists, error) = config::read_file_config(&current.paths.config_file);
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
            config::normalize_base_url(file_config.services.service_base_url.trim());
    }
    if !file_config.services.did_domain.trim().is_empty() {
        refreshed.did_domain = file_config.services.did_domain.trim().to_string();
    }
    if !file_config.services.anp_service_endpoint.trim().is_empty() {
        refreshed.anp_service_endpoint =
            file_config.services.anp_service_endpoint.trim().to_string();
    } else if refreshed.anp_service_endpoint.trim().is_empty() {
        refreshed.anp_service_endpoint =
            config::derive_anp_service_endpoint(&refreshed.service_base_url);
    }
    if !file_config.services.anp_service_did.trim().is_empty() {
        refreshed.anp_service_did = file_config.services.anp_service_did.trim().to_string();
    } else if refreshed.anp_service_did.trim().is_empty() {
        refreshed.anp_service_did = config::derive_anp_service_did(&refreshed.service_base_url);
    }
    if !file_config.services.mail_service_url.trim().is_empty() {
        refreshed.mail_service_url =
            config::normalize_base_url(file_config.services.mail_service_url.trim());
    } else if refreshed.mail_service_url.trim().is_empty() {
        refreshed.mail_service_url = refreshed.service_base_url.clone();
    }
    if !file_config.services.ca_bundle.trim().is_empty() {
        refreshed.ca_bundle = file_config.services.ca_bundle.trim().to_string();
    }
    Ok(refreshed)
}

pub fn ensure_target_store_schema(paths: &config::Paths) -> store::StoreResult<()> {
    let connection = store::open(paths)?;
    store::ensure_schema(&connection)
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
}
