use serde::Serialize;
use std::fmt;

pub const SCHEMA_VERSION: i64 = 12;

pub type StoreResult<T> = Result<T, StoreError>;

#[derive(Debug)]
pub enum StoreError {
    Sqlite(rusqlite::Error),
    Io(std::io::Error),
    UnsafeSql(String),
    UnsupportedLegacySchema(String),
    LegacyDatabaseNotFound,
    NotFound(String),
    Invalid(String),
}

impl StoreError {
    pub fn unsafe_sql(message: impl Into<String>) -> Self {
        Self::UnsafeSql(format!("unsafe sql statement: {}", message.into()))
    }

    pub fn invalid(message: impl Into<String>) -> Self {
        Self::Invalid(message.into())
    }
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sqlite(err) => write!(f, "{err}"),
            Self::Io(err) => write!(f, "{err}"),
            Self::UnsafeSql(message) => f.write_str(message),
            Self::UnsupportedLegacySchema(message) => f.write_str(message),
            Self::LegacyDatabaseNotFound => f.write_str("legacy sqlite database not found"),
            Self::NotFound(message) => f.write_str(message),
            Self::Invalid(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for StoreError {}

impl From<rusqlite::Error> for StoreError {
    fn from(value: rusqlite::Error) -> Self {
        match value {
            rusqlite::Error::QueryReturnedNoRows => {
                Self::NotFound("query returned no rows".to_string())
            }
            other => Self::Sqlite(other),
        }
    }
}

impl From<std::io::Error> for StoreError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct LegacyScan {
    pub path: String,
    pub exists: bool,
    pub schema_version: i64,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tables: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImportReport {
    pub source_path: String,
    pub source_schema_version: i64,
    pub imported_rows: std::collections::BTreeMap<String, usize>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub skipped_tables: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}
