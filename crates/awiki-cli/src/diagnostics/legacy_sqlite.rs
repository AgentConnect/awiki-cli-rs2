use crate::workspace_config::Paths;
use crate::workspace_upgrade;
use rusqlite::Connection;
use serde_json::Value;

pub(crate) use crate::workspace_upgrade::legacy_sqlite::{StoreError, SCHEMA_VERSION};

pub(crate) fn open_read_only(path: &str) -> Result<Connection, StoreError> {
    workspace_upgrade::legacy_sqlite::open_read_only(path)
}

pub(crate) fn current_schema_version(connection: &Connection) -> Result<i64, StoreError> {
    workspace_upgrade::legacy_sqlite::current_schema_version(connection)
}

pub(crate) fn scan_legacy_database(
    paths: &Paths,
) -> Result<workspace_upgrade::legacy_sqlite::LegacyScan, StoreError> {
    workspace_upgrade::legacy_sqlite::scan_legacy_database(paths)
}

pub(crate) fn list_contact_handle_history(
    connection: &Connection,
    handle: &str,
) -> Result<Vec<Value>, StoreError> {
    workspace_upgrade::legacy_sqlite::list_contact_handle_history(connection, handle)
}
