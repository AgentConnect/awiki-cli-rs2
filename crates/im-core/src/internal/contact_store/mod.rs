#[cfg(feature = "sqlite")]
pub(crate) mod projection;
#[cfg(feature = "sqlite")]
pub(crate) mod records;
#[cfg(all(feature = "sqlite", any(feature = "blocking", test)))]
pub(crate) mod relationships;

#[cfg(all(feature = "sqlite", any(feature = "blocking", test)))]
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

#[cfg(all(feature = "sqlite", any(feature = "blocking", test)))]
fn configure(connection: &rusqlite::Connection) -> crate::ImResult<()> {
    crate::internal::local_state::configure(connection)
}

#[cfg(feature = "sqlite")]
pub(crate) fn local_state_unavailable(err: impl std::fmt::Display) -> crate::ImError {
    crate::ImError::LocalStateUnavailable {
        detail: err.to_string(),
    }
}
