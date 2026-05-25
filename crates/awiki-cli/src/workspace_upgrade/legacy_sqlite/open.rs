use super::{StoreResult, SCHEMA_VERSION};
use crate::workspace_config::Paths;
use rusqlite::{Connection, OpenFlags};
use std::fs;
use std::path::Path;

pub fn open(paths: &Paths) -> StoreResult<Connection> {
    open_database(&paths.database_file, false)
}

pub fn open_read_only(path: &str) -> StoreResult<Connection> {
    open_database(path, true)
}

fn open_database(path: &str, read_only: bool) -> StoreResult<Connection> {
    if path.trim().is_empty() {
        return Err(super::StoreError::invalid("sqlite path is required"));
    }
    if !read_only {
        if let Some(parent) = Path::new(path).parent() {
            fs::create_dir_all(parent)?;
        }
    }
    let connection = if read_only {
        Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?
    } else {
        Connection::open(path)?
    };
    if !read_only {
        configure_database(&connection)?;
    }
    Ok(connection)
}

fn configure_database(connection: &Connection) -> StoreResult<()> {
    connection.pragma_update(None, "journal_mode", "WAL")?;
    connection.pragma_update(None, "foreign_keys", "ON")?;
    connection.pragma_update(None, "busy_timeout", 5000)?;
    let _: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    let _ = SCHEMA_VERSION;
    Ok(())
}
