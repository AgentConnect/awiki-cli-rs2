use std::collections::BTreeSet;

use rusqlite::Connection;

pub(crate) const TABLE_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS conversation_summaries (
    owner_identity_id TEXT NOT NULL,
    owner_did         TEXT NOT NULL DEFAULT '',
    conversation_id   TEXT NOT NULL,
    thread_id         TEXT NOT NULL,
    message_count     INTEGER NOT NULL DEFAULT 0,
    unread_count      INTEGER NOT NULL DEFAULT 0,
    unread_mention_count INTEGER NOT NULL DEFAULT 0,
    first_unread_mention_message_id TEXT,
    last_message_id   TEXT,
    last_message_at   TEXT NOT NULL DEFAULT '',
    last_content      TEXT,
    last_content_type TEXT,
    last_sender_did   TEXT,
    last_sender_name  TEXT,
    last_payload_json TEXT,
    group_id          TEXT,
    group_did         TEXT,
    updated_at        TEXT NOT NULL,
    PRIMARY KEY (owner_identity_id, conversation_id)
);
"#;

pub(crate) const INDEX_STATEMENTS: &[&str] = &[
    "CREATE INDEX IF NOT EXISTS idx_conversation_summaries_owner_last ON conversation_summaries(owner_identity_id, last_message_at DESC, conversation_id)",
    "CREATE INDEX IF NOT EXISTS idx_conversation_summaries_owner_unread_last ON conversation_summaries(owner_identity_id, unread_count, last_message_at DESC)",
];

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct SummaryMessageRow {
    msg_id: String,
    owner_did: String,
    direction: i64,
    sender_did: String,
    group_id: String,
    group_did: String,
    content_type: String,
    content: String,
    is_read: bool,
    sender_name: String,
    mentions_current_user: bool,
    sort_at: String,
}

pub(crate) fn create_schema(connection: &Connection) -> crate::ImResult<()> {
    connection
        .execute_batch(TABLE_SQL)
        .map_err(super::local_state_unavailable)?;
    for statement in INDEX_STATEMENTS {
        connection
            .execute(statement, [])
            .map_err(super::local_state_unavailable)?;
    }
    Ok(())
}

pub(crate) fn ensure_owner_backfilled(
    connection: &Connection,
    owner_identity_id: &str,
) -> crate::ImResult<()> {
    let owner_identity_id = required("owner_identity_id", owner_identity_id)?;
    let summary_count = connection
        .query_row(
            "SELECT COUNT(*) FROM conversation_summaries WHERE owner_identity_id = ?1",
            [&owner_identity_id],
            |row| row.get::<_, i64>(0),
        )
        .map_err(super::local_state_unavailable)?;
    if summary_count > 0 {
        return Ok(());
    }
    let message_count = connection
        .query_row(
            "SELECT COUNT(*) FROM messages WHERE owner_identity_id = ?1",
            [&owner_identity_id],
            |row| row.get::<_, i64>(0),
        )
        .map_err(super::local_state_unavailable)?;
    if message_count > 0 {
        rebuild_owner(connection, &owner_identity_id)?;
    }
    Ok(())
}

pub(crate) fn rebuild_all(connection: &Connection) -> crate::ImResult<usize> {
    connection
        .execute("DELETE FROM conversation_summaries", [])
        .map_err(super::local_state_unavailable)?;
    let owners = distinct_owner_identity_ids(connection)?;
    let mut rebuilt = 0;
    for owner_identity_id in owners {
        rebuilt += rebuild_owner(connection, &owner_identity_id)?;
    }
    Ok(rebuilt)
}

pub(crate) fn rebuild_owner(
    connection: &Connection,
    owner_identity_id: &str,
) -> crate::ImResult<usize> {
    let owner_identity_id = required("owner_identity_id", owner_identity_id)?;
    connection
        .execute(
            "DELETE FROM conversation_summaries WHERE owner_identity_id = ?1",
            [&owner_identity_id],
        )
        .map_err(super::local_state_unavailable)?;
    let conversation_ids = distinct_conversation_ids_for_owner(connection, &owner_identity_id)?;
    let mut rebuilt = 0;
    for conversation_id in conversation_ids {
        if rebuild_conversation(connection, &owner_identity_id, &conversation_id)? {
            rebuilt += 1;
        }
    }
    Ok(rebuilt)
}

pub(crate) fn rebuild_touched(
    connection: &Connection,
    touched: &BTreeSet<(String, String)>,
) -> crate::ImResult<usize> {
    let mut rebuilt = 0;
    for (owner_identity_id, conversation_id) in touched {
        if owner_identity_id.trim().is_empty() || conversation_id.trim().is_empty() {
            continue;
        }
        if rebuild_conversation(connection, owner_identity_id, conversation_id)? {
            rebuilt += 1;
        }
    }
    Ok(rebuilt)
}

pub(crate) fn rebuild_conversation(
    connection: &Connection,
    owner_identity_id: &str,
    conversation_id: &str,
) -> crate::ImResult<bool> {
    let owner_identity_id = required("owner_identity_id", owner_identity_id)?;
    let conversation_id = required("conversation_id", conversation_id)?;
    let mut statement = connection
        .prepare(
            r#"
SELECT msg_id,
       owner_did,
       direction,
       sender_did,
       group_id,
       group_did,
       content_type,
       content,
       is_read,
       sender_name,
       mentions_current_user,
       COALESCE(NULLIF(sent_at, ''), stored_at) AS sort_at
FROM messages
WHERE owner_identity_id = ?1
  AND COALESCE(NULLIF(conversation_id, ''), thread_id) = ?2
ORDER BY sort_at ASC, msg_id ASC"#,
        )
        .map_err(super::local_state_unavailable)?;
    let rows = statement
        .query_map((&owner_identity_id, &conversation_id), summary_message_row)
        .map_err(super::local_state_unavailable)?;

    let mut message_count = 0_i64;
    let mut unread_count = 0_i64;
    let mut unread_mention_count = 0_i64;
    let mut first_unread_mention_message_id = String::new();
    let mut last: Option<SummaryMessageRow> = None;

    for row in rows {
        let row = row.map_err(super::local_state_unavailable)?;
        message_count += 1;
        if row.direction == 0 && !row.is_read {
            unread_count += 1;
            if row.mentions_current_user {
                unread_mention_count += 1;
                if first_unread_mention_message_id.is_empty() {
                    first_unread_mention_message_id = row.msg_id.clone();
                }
            }
        }
        last = Some(row);
    }

    if message_count == 0 {
        connection
            .execute(
                r#"
DELETE FROM conversation_summaries
WHERE owner_identity_id = ?1 AND conversation_id = ?2"#,
                (&owner_identity_id, &conversation_id),
            )
            .map_err(super::local_state_unavailable)?;
        return Ok(false);
    }

    let last = last.expect("message_count > 0 must have a last row");
    let owner_did = default_string(&last.owner_did, "");
    let updated_at = now_utc_like();
    let last_payload_json = if last.content_type.trim() == "application/json" {
        nullable_text(&last.content)
    } else {
        None
    };
    connection
        .execute(
            r#"
INSERT INTO conversation_summaries
    (owner_identity_id, owner_did, conversation_id, thread_id,
     message_count, unread_count, unread_mention_count, first_unread_mention_message_id,
     last_message_id, last_message_at, last_content, last_content_type, last_sender_did,
     last_sender_name, last_payload_json, group_id, group_did, updated_at)
VALUES (?1, ?2, ?3, ?3,
        ?4, ?5, ?6, ?7,
        ?8, ?9, ?10, ?11, ?12,
        ?13, ?14, ?15, ?16, ?17)
ON CONFLICT(owner_identity_id, conversation_id) DO UPDATE SET
    owner_did = excluded.owner_did,
    thread_id = excluded.thread_id,
    message_count = excluded.message_count,
    unread_count = excluded.unread_count,
    unread_mention_count = excluded.unread_mention_count,
    first_unread_mention_message_id = excluded.first_unread_mention_message_id,
    last_message_id = excluded.last_message_id,
    last_message_at = excluded.last_message_at,
    last_content = excluded.last_content,
    last_content_type = excluded.last_content_type,
    last_sender_did = excluded.last_sender_did,
    last_sender_name = excluded.last_sender_name,
    last_payload_json = excluded.last_payload_json,
    group_id = excluded.group_id,
    group_did = excluded.group_did,
    updated_at = excluded.updated_at"#,
            rusqlite::params![
                owner_identity_id,
                owner_did,
                conversation_id,
                message_count,
                unread_count,
                unread_mention_count,
                nullable_text(&first_unread_mention_message_id),
                last.msg_id,
                last.sort_at,
                nullable_text(&last.content),
                nullable_text(&last.content_type),
                nullable_text(&last.sender_did),
                nullable_text(&last.sender_name),
                last_payload_json,
                nullable_text(&last.group_id),
                nullable_text(&last.group_did),
                updated_at,
            ],
        )
        .map_err(super::local_state_unavailable)?;
    Ok(true)
}

fn distinct_owner_identity_ids(connection: &Connection) -> crate::ImResult<Vec<String>> {
    let mut statement = connection
        .prepare(
            r#"
SELECT DISTINCT owner_identity_id
FROM messages
WHERE TRIM(COALESCE(owner_identity_id, '')) <> ''
ORDER BY owner_identity_id"#,
        )
        .map_err(super::local_state_unavailable)?;
    let rows = statement
        .query_map([], |row| {
            row.get::<_, Option<String>>("owner_identity_id")
                .map(|value| value.unwrap_or_default())
        })
        .map_err(super::local_state_unavailable)?;
    let mut owners = Vec::new();
    for row in rows {
        let owner = row.map_err(super::local_state_unavailable)?;
        push_unique(&mut owners, owner.trim().to_owned());
    }
    Ok(owners)
}

fn distinct_conversation_ids_for_owner(
    connection: &Connection,
    owner_identity_id: &str,
) -> crate::ImResult<Vec<String>> {
    let mut statement = connection
        .prepare(
            r#"
SELECT DISTINCT COALESCE(NULLIF(conversation_id, ''), thread_id) AS conversation_id
FROM messages
WHERE owner_identity_id = ?1
  AND TRIM(COALESCE(COALESCE(NULLIF(conversation_id, ''), thread_id), '')) <> ''
ORDER BY conversation_id"#,
        )
        .map_err(super::local_state_unavailable)?;
    let rows = statement
        .query_map([owner_identity_id], |row| {
            row.get::<_, Option<String>>("conversation_id")
                .map(|value| value.unwrap_or_default())
        })
        .map_err(super::local_state_unavailable)?;
    let mut ids = Vec::new();
    for row in rows {
        let id = row.map_err(super::local_state_unavailable)?;
        push_unique(&mut ids, id.trim().to_owned());
    }
    Ok(ids)
}

fn summary_message_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SummaryMessageRow> {
    Ok(SummaryMessageRow {
        msg_id: row.get::<_, Option<String>>("msg_id")?.unwrap_or_default(),
        owner_did: row
            .get::<_, Option<String>>("owner_did")?
            .unwrap_or_default(),
        direction: row.get::<_, Option<i64>>("direction")?.unwrap_or_default(),
        sender_did: row
            .get::<_, Option<String>>("sender_did")?
            .unwrap_or_default(),
        group_id: row
            .get::<_, Option<String>>("group_id")?
            .unwrap_or_default(),
        group_did: row
            .get::<_, Option<String>>("group_did")?
            .unwrap_or_default(),
        content_type: row
            .get::<_, Option<String>>("content_type")?
            .unwrap_or_default(),
        content: row.get::<_, Option<String>>("content")?.unwrap_or_default(),
        is_read: row.get::<_, Option<i64>>("is_read")?.unwrap_or_default() != 0,
        sender_name: row
            .get::<_, Option<String>>("sender_name")?
            .unwrap_or_default(),
        mentions_current_user: row
            .get::<_, Option<i64>>("mentions_current_user")?
            .unwrap_or_default()
            != 0,
        sort_at: row.get::<_, Option<String>>("sort_at")?.unwrap_or_default(),
    })
}

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

fn nullable_text(value: &str) -> Option<&str> {
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

fn default_string(value: &str, fallback: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        fallback.to_owned()
    } else {
        value.to_owned()
    }
}

fn push_unique(values: &mut Vec<String>, value: String) {
    if value.trim().is_empty() || values.iter().any(|known| known == &value) {
        return;
    }
    values.push(value);
}

fn now_utc_like() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_owned())
}
