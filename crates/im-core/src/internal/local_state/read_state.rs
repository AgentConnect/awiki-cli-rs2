#[cfg(feature = "sqlite")]
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ThreadReadStateRecord {
    pub(crate) owner_identity_id: String,
    pub(crate) owner_did: String,
    pub(crate) thread_scope: String,
    pub(crate) thread_id: String,
    pub(crate) conversation_id: String,
    pub(crate) read_watermark_message_id: Option<String>,
    pub(crate) read_watermark_seq: Option<String>,
    pub(crate) read_watermark_at: Option<String>,
    pub(crate) pending_remote_ack: bool,
    pub(crate) remote_ack_at: Option<String>,
    pub(crate) updated_at: String,
}

#[cfg(feature = "sqlite")]
pub(crate) fn upsert_thread_read_state(
    connection: &rusqlite::Connection,
    record: &ThreadReadStateRecord,
) -> crate::ImResult<()> {
    crate::internal::local_state::schema::ensure_schema(connection)?;
    let owner_identity_id = required("owner_identity_id", &record.owner_identity_id)?;
    let thread_scope = required("thread_scope", &record.thread_scope)?;
    let thread_id = required("thread_id", &record.thread_id)?;
    let updated_at = required("updated_at", &record.updated_at)?;
    connection
        .execute(
            r#"
INSERT INTO thread_read_state
    (owner_identity_id, owner_did, thread_scope, thread_id, conversation_id,
     read_watermark_message_id, read_watermark_seq, read_watermark_at,
     pending_remote_ack, remote_ack_at, updated_at)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
ON CONFLICT(owner_identity_id, thread_scope, thread_id) DO UPDATE SET
    owner_did = excluded.owner_did,
    conversation_id = CASE
        WHEN TRIM(excluded.conversation_id) <> '' THEN excluded.conversation_id
        ELSE thread_read_state.conversation_id
    END,
    read_watermark_message_id = CASE
        WHEN thread_read_state.read_watermark_seq IS NULL
          OR (
            excluded.read_watermark_seq IS NOT NULL
            AND CAST(excluded.read_watermark_seq AS INTEGER) >= CAST(thread_read_state.read_watermark_seq AS INTEGER)
          )
        THEN excluded.read_watermark_message_id
        ELSE thread_read_state.read_watermark_message_id
    END,
    read_watermark_seq = CASE
        WHEN thread_read_state.read_watermark_seq IS NULL THEN excluded.read_watermark_seq
        WHEN excluded.read_watermark_seq IS NULL THEN thread_read_state.read_watermark_seq
        WHEN CAST(excluded.read_watermark_seq AS INTEGER) > CAST(thread_read_state.read_watermark_seq AS INTEGER)
        THEN excluded.read_watermark_seq
        ELSE thread_read_state.read_watermark_seq
    END,
    read_watermark_at = CASE
        WHEN thread_read_state.read_watermark_seq IS NULL
          OR (
            excluded.read_watermark_seq IS NOT NULL
            AND CAST(excluded.read_watermark_seq AS INTEGER) >= CAST(thread_read_state.read_watermark_seq AS INTEGER)
          )
        THEN excluded.read_watermark_at
        ELSE thread_read_state.read_watermark_at
    END,
    pending_remote_ack = CASE
        WHEN thread_read_state.read_watermark_seq IS NULL THEN excluded.pending_remote_ack
        WHEN excluded.read_watermark_seq IS NULL THEN thread_read_state.pending_remote_ack
        WHEN CAST(excluded.read_watermark_seq AS INTEGER) >= CAST(thread_read_state.read_watermark_seq AS INTEGER)
        THEN excluded.pending_remote_ack
        ELSE thread_read_state.pending_remote_ack
    END,
    remote_ack_at = CASE
        WHEN thread_read_state.read_watermark_seq IS NULL THEN excluded.remote_ack_at
        WHEN excluded.read_watermark_seq IS NULL THEN thread_read_state.remote_ack_at
        WHEN CAST(excluded.read_watermark_seq AS INTEGER) >= CAST(thread_read_state.read_watermark_seq AS INTEGER)
        THEN excluded.remote_ack_at
        ELSE thread_read_state.remote_ack_at
    END,
    updated_at = CASE
        WHEN thread_read_state.read_watermark_seq IS NULL THEN excluded.updated_at
        WHEN excluded.read_watermark_seq IS NULL THEN thread_read_state.updated_at
        WHEN CAST(excluded.read_watermark_seq AS INTEGER) >= CAST(thread_read_state.read_watermark_seq AS INTEGER)
        THEN excluded.updated_at
        ELSE thread_read_state.updated_at
    END"#,
            rusqlite::params![
                owner_identity_id,
                record.owner_did.trim(),
                thread_scope,
                thread_id,
                record.conversation_id.trim(),
                nullable_option(record.read_watermark_message_id.as_deref()),
                nullable_option(record.read_watermark_seq.as_deref()),
                nullable_option(record.read_watermark_at.as_deref()),
                record.pending_remote_ack,
                nullable_option(record.remote_ack_at.as_deref()),
                updated_at,
            ],
        )
        .map_err(super::local_state_unavailable)?;
    Ok(())
}

#[cfg(feature = "sqlite")]
pub(crate) fn get_thread_read_state(
    connection: &rusqlite::Connection,
    owner_identity_id: &str,
    thread_scope: &str,
    thread_id: &str,
) -> crate::ImResult<Option<ThreadReadStateRecord>> {
    let owner_identity_id = required("owner_identity_id", owner_identity_id)?;
    let thread_scope = required("thread_scope", thread_scope)?;
    let thread_id = required("thread_id", thread_id)?;
    let mut statement = connection
        .prepare(
            r#"
SELECT owner_identity_id,
       owner_did,
       thread_scope,
       thread_id,
       conversation_id,
       read_watermark_message_id,
       read_watermark_seq,
       read_watermark_at,
       pending_remote_ack,
       remote_ack_at,
       updated_at
FROM thread_read_state
WHERE owner_identity_id = ?1
  AND thread_scope = ?2
  AND thread_id = ?3
LIMIT 1"#,
        )
        .map_err(super::local_state_unavailable)?;
    let result = statement
        .query_row(
            rusqlite::params![owner_identity_id, thread_scope, thread_id],
            thread_read_state_from_row,
        )
        .map(Some)
        .or_else(|error| match error {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            other => Err(other),
        })
        .map_err(super::local_state_unavailable)?;
    Ok(result)
}

#[cfg(feature = "sqlite")]
fn thread_read_state_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ThreadReadStateRecord> {
    Ok(ThreadReadStateRecord {
        owner_identity_id: row
            .get::<_, Option<String>>("owner_identity_id")?
            .unwrap_or_default(),
        owner_did: row
            .get::<_, Option<String>>("owner_did")?
            .unwrap_or_default(),
        thread_scope: row
            .get::<_, Option<String>>("thread_scope")?
            .unwrap_or_default(),
        thread_id: row
            .get::<_, Option<String>>("thread_id")?
            .unwrap_or_default(),
        conversation_id: row
            .get::<_, Option<String>>("conversation_id")?
            .unwrap_or_default(),
        read_watermark_message_id: row.get("read_watermark_message_id")?,
        read_watermark_seq: row.get("read_watermark_seq")?,
        read_watermark_at: row.get("read_watermark_at")?,
        pending_remote_ack: row
            .get::<_, Option<bool>>("pending_remote_ack")?
            .unwrap_or(false),
        remote_ack_at: row.get("remote_ack_at")?,
        updated_at: row
            .get::<_, Option<String>>("updated_at")?
            .unwrap_or_default(),
    })
}

#[cfg(feature = "sqlite")]
fn required(field: &'static str, value: &str) -> crate::ImResult<String> {
    let value = value.trim();
    if value.is_empty() {
        return Err(crate::ImError::invalid_input(
            Some(field.to_owned()),
            format!("{field} is required"),
        ));
    }
    Ok(value.to_owned())
}

#[cfg(feature = "sqlite")]
fn nullable_option(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}
