#[cfg(feature = "sqlite")]
pub(crate) mod projection;
#[cfg(feature = "sqlite")]
pub(crate) mod records;
#[cfg(feature = "sqlite")]
pub(crate) mod relationships;

#[cfg(feature = "sqlite")]
pub(crate) fn open_writable(
    client: &crate::core::ImClient,
) -> crate::ImResult<rusqlite::Connection> {
    let sqlite_path = &client.core_inner().sdk_paths().local_state.sqlite_path;
    if let Some(parent) = sqlite_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let connection = rusqlite::Connection::open(sqlite_path).map_err(local_state_unavailable)?;
    configure(&connection)?;
    records::ensure_schema(&connection)?;
    Ok(connection)
}

#[cfg(feature = "sqlite")]
fn configure(connection: &rusqlite::Connection) -> crate::ImResult<()> {
    connection
        .pragma_update(None, "journal_mode", "WAL")
        .map_err(local_state_unavailable)?;
    connection
        .pragma_update(None, "foreign_keys", "ON")
        .map_err(local_state_unavailable)?;
    connection
        .pragma_update(None, "busy_timeout", 5000)
        .map_err(local_state_unavailable)?;
    Ok(())
}

#[cfg(feature = "sqlite")]
pub(crate) fn local_state_unavailable(err: impl std::fmt::Display) -> crate::ImError {
    crate::ImError::LocalStateUnavailable {
        detail: err.to_string(),
    }
}
