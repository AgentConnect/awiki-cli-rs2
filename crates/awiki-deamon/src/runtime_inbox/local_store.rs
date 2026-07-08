use std::path::Path;

use anyhow::{Context, Result};

use super::projection::normalize_runtime_inbox_conversation_records;
use super::{RuntimeInboxScope, MAX_LIMIT};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct LocalConversationRecord {
    pub(super) conversation_id: String,
    pub(super) message_count: u32,
    pub(super) unread_count: u32,
    pub(super) last_message_at: String,
    pub(super) last_message: Option<LocalMessageRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct LocalMessageRecord {
    pub(super) msg_id: String,
    pub(super) direction: i64,
    pub(super) sender_did: String,
    pub(super) receiver_did: String,
    pub(super) group_id: String,
    pub(super) group_did: String,
    pub(super) content_type: String,
    pub(super) content: String,
    pub(super) sent_at: String,
    pub(super) stored_at: String,
    pub(super) is_read: bool,
    pub(super) metadata: String,
}

pub(super) fn load_local_conversations(
    sqlite_path: &Path,
    owner_identity_id: &str,
    runtime_agent_did: &str,
    scope: RuntimeInboxScope,
    limit: u32,
    offset: usize,
) -> Result<Vec<LocalConversationRecord>> {
    let connection = rusqlite::Connection::open(sqlite_path)?;
    let records = load_local_conversations_from_messages(&connection, owner_identity_id, scope)?;
    let records = normalize_runtime_inbox_conversation_records(records, runtime_agent_did)?;
    Ok(records
        .into_iter()
        .skip(offset)
        .take(limit.min(MAX_LIMIT) as usize + 1)
        .collect())
}

pub(super) fn load_local_messages(
    sqlite_path: &Path,
    owner_identity_id: &str,
    conversation_id: &str,
    limit: u32,
    offset: usize,
) -> Result<Vec<LocalMessageRecord>> {
    let connection = rusqlite::Connection::open(sqlite_path)?;
    let row_limit = i64::from(limit.min(MAX_LIMIT)) + 1;
    let row_offset = i64::try_from(offset).context("runtime inbox thread offset too large")?;
    let mut statement = connection.prepare(
        r#"
SELECT
    msg_id,
    direction,
    sender_did,
    receiver_did,
    group_id,
    group_did,
    content_type,
    content,
    sent_at,
    stored_at,
    is_read,
    metadata
FROM messages
WHERE owner_identity_id = ?1
  AND COALESCE(NULLIF(conversation_id, ''), thread_id) = ?2
ORDER BY COALESCE(sent_at, stored_at) DESC, msg_id DESC
LIMIT ?3 OFFSET ?4"#,
    )?;
    let rows = statement.query_map(
        (owner_identity_id, conversation_id, row_limit, row_offset),
        |row| {
            Ok(LocalMessageRecord {
                msg_id: optional_string(row, "msg_id")?,
                direction: row.get::<_, Option<i64>>("direction")?.unwrap_or_default(),
                sender_did: optional_string(row, "sender_did")?,
                receiver_did: optional_string(row, "receiver_did")?,
                group_id: optional_string(row, "group_id")?,
                group_did: optional_string(row, "group_did")?,
                content_type: optional_string(row, "content_type")?,
                content: optional_string(row, "content")?,
                sent_at: optional_string(row, "sent_at")?,
                stored_at: optional_string(row, "stored_at")?,
                is_read: row.get::<_, Option<i64>>("is_read")?.unwrap_or_default() != 0,
                metadata: optional_string(row, "metadata")?,
            })
        },
    )?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

pub(super) fn mark_local_conversation_read(
    sqlite_path: &Path,
    owner_identity_id: &str,
    conversation_id: &str,
) -> Result<usize> {
    let conversation_id = conversation_id.trim();
    if conversation_id.is_empty() {
        return Ok(0);
    }
    let connection = rusqlite::Connection::open(sqlite_path)?;
    let updated = connection.execute(
        r#"
UPDATE messages
SET is_read = 1
WHERE owner_identity_id = ?1
  AND COALESCE(NULLIF(conversation_id, ''), thread_id) = ?2
  AND direction = 0
  AND COALESCE(is_read, 0) = 0"#,
        rusqlite::params![owner_identity_id, conversation_id],
    )?;
    Ok(updated)
}

pub(super) fn load_local_conversations_from_messages(
    connection: &rusqlite::Connection,
    owner_identity_id: &str,
    scope: RuntimeInboxScope,
) -> Result<Vec<LocalConversationRecord>> {
    let mut statement = String::from(
        r#"
WITH latest AS (
    SELECT
        COALESCE(NULLIF(conversation_id, ''), thread_id) AS conversation_id,
        COUNT(*) AS message_count,
        SUM(CASE WHEN is_read = 0 AND direction = 0 THEN 1 ELSE 0 END) AS unread_count,
        MAX(COALESCE(NULLIF(sent_at, ''), stored_at)) AS last_message_at
    FROM messages
    WHERE owner_identity_id = ?1
      AND COALESCE(NULLIF(conversation_id, ''), thread_id) <> ''
"#,
    );
    match scope {
        RuntimeInboxScope::All => {}
        RuntimeInboxScope::Direct => statement.push_str(
            "      AND COALESCE(NULLIF(conversation_id, ''), thread_id) NOT LIKE 'group:%'\n",
        ),
        RuntimeInboxScope::Group => statement.push_str(
            "      AND COALESCE(NULLIF(conversation_id, ''), thread_id) LIKE 'group:%'\n",
        ),
    }
    statement.push_str(
        r#"
    GROUP BY COALESCE(NULLIF(conversation_id, ''), thread_id)
)
SELECT
    l.conversation_id,
    l.message_count,
    l.unread_count,
    l.last_message_at,
    m.msg_id,
    m.direction,
    m.sender_did,
    m.receiver_did,
    m.group_id,
    m.group_did,
    m.content_type,
    m.content,
    m.sent_at,
    m.stored_at,
    m.is_read,
    m.metadata
FROM latest l
JOIN messages m
  ON m.owner_identity_id = ?1
 AND COALESCE(NULLIF(m.conversation_id, ''), m.thread_id) = l.conversation_id
 AND COALESCE(NULLIF(m.sent_at, ''), m.stored_at) = l.last_message_at
 AND m.msg_id = (
     SELECT m2.msg_id
     FROM messages m2
     WHERE m2.owner_identity_id = ?1
       AND COALESCE(NULLIF(m2.conversation_id, ''), m2.thread_id) = l.conversation_id
       AND COALESCE(NULLIF(m2.sent_at, ''), m2.stored_at) = l.last_message_at
     ORDER BY m2.msg_id DESC
     LIMIT 1
)
ORDER BY l.last_message_at DESC, l.conversation_id ASC"#,
    );
    let mut statement = connection.prepare(&statement)?;
    let rows = statement.query_map((owner_identity_id,), |row| {
        Ok(LocalConversationRecord {
            conversation_id: optional_string(row, "conversation_id")?,
            message_count: u32_from_i64(
                row.get::<_, Option<i64>>("message_count")?
                    .unwrap_or_default(),
            ),
            unread_count: u32_from_i64(
                row.get::<_, Option<i64>>("unread_count")?
                    .unwrap_or_default(),
            ),
            last_message_at: optional_string(row, "last_message_at")?,
            last_message: Some(LocalMessageRecord {
                msg_id: optional_string(row, "msg_id")?,
                direction: row.get::<_, Option<i64>>("direction")?.unwrap_or_default(),
                sender_did: optional_string(row, "sender_did")?,
                receiver_did: optional_string(row, "receiver_did")?,
                group_id: optional_string(row, "group_id")?,
                group_did: optional_string(row, "group_did")?,
                content_type: optional_string(row, "content_type")?,
                content: optional_string(row, "content")?,
                sent_at: optional_string(row, "sent_at")?,
                stored_at: optional_string(row, "stored_at")?,
                is_read: row.get::<_, Option<i64>>("is_read")?.unwrap_or_default() != 0,
                metadata: optional_string(row, "metadata")?,
            }),
        })
    })?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

pub(super) fn optional_string(row: &rusqlite::Row<'_>, name: &str) -> rusqlite::Result<String> {
    row.get::<_, Option<String>>(name)
        .map(|value| value.unwrap_or_default())
}

pub(super) fn u32_from_i64(value: i64) -> u32 {
    u32::try_from(value.max(0)).unwrap_or(u32::MAX)
}
