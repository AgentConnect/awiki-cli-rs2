pub(crate) mod contacts;
pub(crate) mod conversations;
pub(crate) mod groups;
pub(crate) mod messages;

#[cfg(feature = "sqlite")]
pub(crate) mod schema;

#[cfg(feature = "sqlite")]
#[allow(dead_code)]
pub(crate) trait LocalStateStore {
    fn ensure_schema(&self) -> crate::ImResult<()>;

    fn current_schema_version(&self) -> crate::ImResult<i64>;

    fn store_message(&mut self, _record: messages::MessageRecord) -> crate::ImResult<()> {
        Err(unsupported_operation("store_message"))
    }

    fn store_messages(&mut self, _records: &[messages::MessageRecord]) -> crate::ImResult<()> {
        Err(unsupported_operation("store_messages"))
    }

    fn upsert_contact(&mut self, _record: contacts::ContactRecord) -> crate::ImResult<()> {
        Err(unsupported_operation("upsert_contact"))
    }

    fn upsert_group(&mut self, _record: groups::GroupRecord) -> crate::ImResult<()> {
        Err(unsupported_operation("upsert_group"))
    }

    fn upsert_group_member(&mut self, _record: groups::GroupMemberRecord) -> crate::ImResult<()> {
        Err(unsupported_operation("upsert_group_member"))
    }

    fn list_conversations(
        &self,
        _owner_did: &str,
        _limit: i64,
    ) -> crate::ImResult<Vec<conversations::ConversationRecord>> {
        Err(unsupported_operation("list_conversations"))
    }
}

#[cfg(feature = "sqlite")]
impl LocalStateStore for rusqlite::Connection {
    fn ensure_schema(&self) -> crate::ImResult<()> {
        schema::ensure_schema(self)
    }

    fn current_schema_version(&self) -> crate::ImResult<i64> {
        schema::current_schema_version(self)
    }
}

#[cfg(feature = "sqlite")]
pub(crate) fn open_writable(path: &std::path::Path) -> crate::ImResult<rusqlite::Connection> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let connection = rusqlite::Connection::open(path).map_err(local_state_unavailable)?;
    configure(&connection)?;
    schema::ensure_schema(&connection)?;
    Ok(connection)
}

#[cfg(feature = "sqlite")]
pub(crate) fn configure(connection: &rusqlite::Connection) -> crate::ImResult<()> {
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

#[cfg(feature = "sqlite")]
#[allow(dead_code)]
fn unsupported_operation(name: &str) -> crate::ImError {
    crate::ImError::unsupported(format!("local_state.{name}"))
}
