use std::collections::{BTreeMap, BTreeSet};

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
    "CREATE INDEX IF NOT EXISTS idx_conversation_summaries_owner_last_desc ON conversation_summaries(owner_identity_id, last_message_at DESC, conversation_id DESC)",
    "CREATE INDEX IF NOT EXISTS idx_conversation_summaries_owner_unread_last ON conversation_summaries(owner_identity_id, unread_count, last_message_at DESC)",
];

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct SummaryMessageRow {
    msg_id: String,
    owner_did: String,
    direction: i64,
    sender_did: String,
    receiver_did: String,
    group_id: String,
    group_did: String,
    content_type: String,
    content: String,
    is_read: bool,
    sender_name: String,
    mentions_current_user: bool,
    sort_at: String,
    server_seq: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SummaryMessageProjection {
    pub(crate) owner_identity_id: String,
    pub(crate) conversation_id: String,
    row: SummaryMessageRow,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MarkReadSummaryRow {
    owner_identity_id: String,
    conversation_id: String,
    mentions_current_user: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SummaryStateRow {
    message_count: i64,
    unread_count: i64,
    unread_mention_count: i64,
    first_unread_mention_message_id: String,
    last_message_id: String,
    last_message_at: String,
    last_server_seq: Option<i64>,
}

impl SummaryMessageRow {
    fn is_control_payload(&self) -> bool {
        super::messages::is_control_payload_for_projection(
            &self.content_type,
            &self.content,
            &self.sender_did,
        )
    }

    fn is_unread_incoming(&self) -> bool {
        self.direction == 0 && !self.is_read && !self.is_control_payload()
    }

    fn is_unread_mention(&self) -> bool {
        self.is_unread_incoming() && self.mentions_current_user
    }

    fn is_self_direct(&self) -> bool {
        let owner = self.owner_did.trim();
        !owner.is_empty()
            && self.group_id.trim().is_empty()
            && self.group_did.trim().is_empty()
            && self.sender_did.trim() == owner
            && self.receiver_did.trim() == owner
    }

    fn contributes_to_summary(&self) -> bool {
        !self.is_self_direct() && !self.is_control_payload()
    }

    fn unread_count_delta(&self) -> i64 {
        if self.is_unread_incoming() {
            1
        } else {
            0
        }
    }

    fn unread_mention_count_delta(&self) -> i64 {
        if self.is_unread_mention() {
            1
        } else {
            0
        }
    }
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
        super::conversation_registry::backfill_from_summaries(connection)?;
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
            super::conversation_registry::ensure_from_summary(
                connection,
                owner_identity_id,
                conversation_id,
            )?;
            rebuilt += 1;
        }
    }
    Ok(rebuilt)
}

pub(crate) fn message_projection_for_id(
    connection: &Connection,
    owner_identity_id: &str,
    msg_id: &str,
) -> crate::ImResult<Option<SummaryMessageProjection>> {
    let owner_identity_id = required("owner_identity_id", owner_identity_id)?;
    let msg_id = required("msg_id", msg_id)?;
    let result = connection.query_row(
        r#"
SELECT msg_id,
       owner_identity_id,
       owner_did,
       COALESCE(NULLIF(conversation_id, ''), thread_id) AS conversation_id,
       direction,
       sender_did,
       receiver_did,
       group_id,
       group_did,
       content_type,
       content,
       is_read,
       sender_name,
       mentions_current_user,
       COALESCE(NULLIF(sent_at, ''), stored_at) AS sort_at,
       server_seq
FROM messages
WHERE owner_identity_id = ?1 AND msg_id = ?2"#,
        (&owner_identity_id, &msg_id),
        |row| {
            Ok(SummaryMessageProjection {
                owner_identity_id: row
                    .get::<_, Option<String>>("owner_identity_id")?
                    .unwrap_or_default(),
                conversation_id: row
                    .get::<_, Option<String>>("conversation_id")?
                    .unwrap_or_default(),
                row: summary_message_row(row)?,
            })
        },
    );
    match result {
        Ok(projection) => Ok(Some(projection)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(err) => Err(super::local_state_unavailable(err)),
    }
}

pub(crate) fn apply_message_delta_or_rebuild(
    connection: &Connection,
    previous: Option<&SummaryMessageProjection>,
    next: &SummaryMessageProjection,
) -> crate::ImResult<usize> {
    if !next.row.contributes_to_summary()
        || previous
            .map(|projection| !projection.row.contributes_to_summary())
            .unwrap_or(false)
    {
        return rebuild_single(connection, &next.owner_identity_id, &next.conversation_id);
    }
    if let Some(previous) = previous {
        if previous.owner_identity_id != next.owner_identity_id
            || previous.conversation_id != next.conversation_id
        {
            let mut touched = BTreeSet::new();
            touched.insert((
                previous.owner_identity_id.clone(),
                previous.conversation_id.clone(),
            ));
            touched.insert((next.owner_identity_id.clone(), next.conversation_id.clone()));
            return rebuild_touched(connection, &touched);
        }
        if previous.row.is_unread_mention() || next.row.is_unread_mention() {
            return rebuild_single(connection, &next.owner_identity_id, &next.conversation_id);
        }
    }

    let Some(summary) = summary_state(connection, &next.owner_identity_id, &next.conversation_id)?
    else {
        let message_count = message_count_for_conversation(
            connection,
            &next.owner_identity_id,
            &next.conversation_id,
        )?;
        if previous.is_none() && message_count == 1 {
            upsert_summary_from_last(
                connection,
                &next.owner_identity_id,
                &next.conversation_id,
                1,
                next.row.unread_count_delta(),
                next.row.unread_mention_count_delta(),
                first_unread_mention_for_new_summary(&next.row),
                &next.row,
            )?;
            return Ok(1);
        }
        return rebuild_single(connection, &next.owner_identity_id, &next.conversation_id);
    };

    if previous.is_none() && next.row.is_unread_mention() {
        return rebuild_single(connection, &next.owner_identity_id, &next.conversation_id);
    }

    match previous {
        None => apply_insert_delta(connection, &summary, next),
        Some(previous) => apply_update_delta(connection, &summary, previous, next),
    }
}

pub(crate) fn mark_read_summary_rows_for_message_ids(
    connection: &Connection,
    owner_identity_id: &str,
    message_ids: &[&str],
) -> crate::ImResult<Vec<MarkReadSummaryRow>> {
    let owner_identity_id = required("owner_identity_id", owner_identity_id)?;
    if message_ids.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders = vec!["?"; message_ids.len()].join(",");
    let statement = format!(
        r#"
SELECT owner_identity_id,
       COALESCE(NULLIF(conversation_id, ''), thread_id) AS conversation_id,
       mentions_current_user
FROM messages
WHERE owner_identity_id = ?
  AND direction = 0
  AND is_read = 0
  AND msg_id IN ({placeholders})"#
    );
    let mut params: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(message_ids.len() + 1);
    params.push(&owner_identity_id);
    for message_id in message_ids {
        params.push(message_id);
    }
    let mut statement = connection
        .prepare(&statement)
        .map_err(super::local_state_unavailable)?;
    let rows = statement
        .query_map(params.as_slice(), |row| {
            Ok(MarkReadSummaryRow {
                owner_identity_id: row
                    .get::<_, Option<String>>("owner_identity_id")?
                    .unwrap_or_default(),
                conversation_id: row
                    .get::<_, Option<String>>("conversation_id")?
                    .unwrap_or_default(),
                mentions_current_user: row
                    .get::<_, Option<i64>>("mentions_current_user")?
                    .unwrap_or_default()
                    != 0,
            })
        })
        .map_err(super::local_state_unavailable)?;
    let mut result = Vec::new();
    for row in rows {
        let row = row.map_err(super::local_state_unavailable)?;
        if !row.owner_identity_id.trim().is_empty() && !row.conversation_id.trim().is_empty() {
            result.push(row);
        }
    }
    Ok(result)
}

pub(crate) fn apply_mark_read_delta_or_rebuild(
    connection: &Connection,
    rows: &[MarkReadSummaryRow],
) -> crate::ImResult<usize> {
    let mut by_conversation = BTreeMap::<(String, String), (i64, i64, bool)>::new();
    for row in rows {
        let entry = by_conversation
            .entry((row.owner_identity_id.clone(), row.conversation_id.clone()))
            .or_insert((0, 0, false));
        entry.0 += 1;
        if row.mentions_current_user {
            entry.1 += 1;
            entry.2 = true;
        }
    }

    let mut changed = 0;
    let mut rebuild = BTreeSet::new();
    for ((owner_identity_id, conversation_id), (unread_delta, mention_delta, needs_rebuild)) in
        by_conversation
    {
        if needs_rebuild {
            rebuild.insert((owner_identity_id, conversation_id));
            continue;
        }
        if let Some(summary) = summary_state(connection, &owner_identity_id, &conversation_id)? {
            update_summary_counts(
                connection,
                &owner_identity_id,
                &conversation_id,
                0,
                -unread_delta,
                -mention_delta,
                &summary.first_unread_mention_message_id,
            )?;
            changed += 1;
        } else {
            rebuild.insert((owner_identity_id, conversation_id));
        }
    }
    changed += rebuild_touched(connection, &rebuild)?;
    Ok(changed)
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
       receiver_did,
       group_id,
       group_did,
       content_type,
       content,
       is_read,
       sender_name,
       mentions_current_user,
       COALESCE(NULLIF(sent_at, ''), stored_at) AS sort_at,
       server_seq
FROM messages
WHERE owner_identity_id = ?1
  AND COALESCE(NULLIF(conversation_id, ''), thread_id) = ?2
ORDER BY sort_at ASC,
         CASE WHEN server_seq IS NULL THEN 0 ELSE 1 END ASC,
         server_seq ASC,
         msg_id ASC"#,
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
        if !row.contributes_to_summary() {
            continue;
        }
        message_count += 1;
        if row.is_unread_incoming() {
            unread_count += 1;
            if row.mentions_current_user {
                unread_mention_count += 1;
                if first_unread_mention_message_id.is_empty() {
                    first_unread_mention_message_id = row.msg_id.clone();
                }
            }
        }
        if last
            .as_ref()
            .map(|current| message_is_later_than_message(&row, current))
            .unwrap_or(true)
        {
            last = Some(row);
        }
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

fn rebuild_single(
    connection: &Connection,
    owner_identity_id: &str,
    conversation_id: &str,
) -> crate::ImResult<usize> {
    if rebuild_conversation(connection, owner_identity_id, conversation_id)? {
        super::conversation_registry::ensure_from_summary(
            connection,
            owner_identity_id,
            conversation_id,
        )?;
        Ok(1)
    } else {
        Ok(0)
    }
}

fn apply_insert_delta(
    connection: &Connection,
    summary: &SummaryStateRow,
    next: &SummaryMessageProjection,
) -> crate::ImResult<usize> {
    if next.row.is_self_direct() {
        return rebuild_single(connection, &next.owner_identity_id, &next.conversation_id);
    }
    let message_count = summary.message_count.saturating_add(1);
    let unread_count = summary
        .unread_count
        .saturating_add(next.row.unread_count_delta());
    let unread_mention_count = summary
        .unread_mention_count
        .saturating_add(next.row.unread_mention_count_delta());
    let first_unread_mention_message_id =
        if summary.first_unread_mention_message_id.is_empty() && next.row.is_unread_mention() {
            next.row.msg_id.as_str()
        } else {
            summary.first_unread_mention_message_id.as_str()
        };

    if message_is_later_than_summary(&next.row, summary) {
        upsert_summary_from_last(
            connection,
            &next.owner_identity_id,
            &next.conversation_id,
            message_count,
            unread_count,
            unread_mention_count,
            first_unread_mention_message_id,
            &next.row,
        )?;
    } else {
        update_summary_counts(
            connection,
            &next.owner_identity_id,
            &next.conversation_id,
            1,
            next.row.unread_count_delta(),
            next.row.unread_mention_count_delta(),
            first_unread_mention_message_id,
        )?;
    }
    Ok(1)
}

fn apply_update_delta(
    connection: &Connection,
    summary: &SummaryStateRow,
    previous: &SummaryMessageProjection,
    next: &SummaryMessageProjection,
) -> crate::ImResult<usize> {
    let unread_delta = next
        .row
        .unread_count_delta()
        .saturating_sub(previous.row.unread_count_delta());
    let mention_delta = next
        .row
        .unread_mention_count_delta()
        .saturating_sub(previous.row.unread_mention_count_delta());
    if summary.last_message_id == previous.row.msg_id
        || message_is_later_than_summary(&next.row, summary)
    {
        return rebuild_single(connection, &next.owner_identity_id, &next.conversation_id);
    }
    update_summary_counts(
        connection,
        &next.owner_identity_id,
        &next.conversation_id,
        0,
        unread_delta,
        mention_delta,
        &summary.first_unread_mention_message_id,
    )?;
    Ok(1)
}

fn summary_state(
    connection: &Connection,
    owner_identity_id: &str,
    conversation_id: &str,
) -> crate::ImResult<Option<SummaryStateRow>> {
    let result = connection.query_row(
        r#"
SELECT t.message_count,
       t.unread_count,
       t.unread_mention_count,
       t.first_unread_mention_message_id,
       t.last_message_id,
       t.last_message_at,
       m.server_seq AS last_server_seq
FROM conversation_summaries t
LEFT JOIN messages m
  ON m.owner_identity_id = t.owner_identity_id
 AND m.msg_id = t.last_message_id
WHERE t.owner_identity_id = ?1 AND t.conversation_id = ?2"#,
        (owner_identity_id, conversation_id),
        |row| {
            Ok(SummaryStateRow {
                message_count: row.get::<_, Option<i64>>("message_count")?.unwrap_or(0),
                unread_count: row.get::<_, Option<i64>>("unread_count")?.unwrap_or(0),
                unread_mention_count: row
                    .get::<_, Option<i64>>("unread_mention_count")?
                    .unwrap_or(0),
                first_unread_mention_message_id: row
                    .get::<_, Option<String>>("first_unread_mention_message_id")?
                    .unwrap_or_default(),
                last_message_id: row
                    .get::<_, Option<String>>("last_message_id")?
                    .unwrap_or_default(),
                last_message_at: row
                    .get::<_, Option<String>>("last_message_at")?
                    .unwrap_or_default(),
                last_server_seq: row.get::<_, Option<i64>>("last_server_seq")?,
            })
        },
    );
    match result {
        Ok(row) => Ok(Some(row)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(err) => Err(super::local_state_unavailable(err)),
    }
}

fn message_count_for_conversation(
    connection: &Connection,
    owner_identity_id: &str,
    conversation_id: &str,
) -> crate::ImResult<i64> {
    connection
        .query_row(
            r#"
SELECT COUNT(*)
FROM messages
WHERE owner_identity_id = ?1
  AND COALESCE(NULLIF(conversation_id, ''), thread_id) = ?2"#,
            (owner_identity_id, conversation_id),
            |row| row.get::<_, i64>(0),
        )
        .map_err(super::local_state_unavailable)
}

fn update_summary_counts(
    connection: &Connection,
    owner_identity_id: &str,
    conversation_id: &str,
    message_delta: i64,
    unread_delta: i64,
    unread_mention_delta: i64,
    first_unread_mention_message_id: &str,
) -> crate::ImResult<()> {
    connection
        .execute(
            r#"
UPDATE conversation_summaries
SET message_count = MAX(0, message_count + ?3),
    unread_count = MAX(0, unread_count + ?4),
    unread_mention_count = MAX(0, unread_mention_count + ?5),
    first_unread_mention_message_id = ?6,
    updated_at = ?7
WHERE owner_identity_id = ?1 AND conversation_id = ?2"#,
            rusqlite::params![
                owner_identity_id,
                conversation_id,
                message_delta,
                unread_delta,
                unread_mention_delta,
                nullable_text(first_unread_mention_message_id),
                now_utc_like(),
            ],
        )
        .map_err(super::local_state_unavailable)?;
    Ok(())
}

fn upsert_summary_from_last(
    connection: &Connection,
    owner_identity_id: &str,
    conversation_id: &str,
    message_count: i64,
    unread_count: i64,
    unread_mention_count: i64,
    first_unread_mention_message_id: &str,
    last: &SummaryMessageRow,
) -> crate::ImResult<()> {
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
                nullable_text(first_unread_mention_message_id),
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
    Ok(())
}

fn first_unread_mention_for_new_summary(row: &SummaryMessageRow) -> &str {
    if row.is_unread_mention() {
        row.msg_id.as_str()
    } else {
        ""
    }
}

fn message_is_later_than_summary(row: &SummaryMessageRow, summary: &SummaryStateRow) -> bool {
    match (row.server_seq, summary.last_server_seq) {
        (Some(server_seq), Some(last_server_seq)) if server_seq != last_server_seq => {
            return server_seq > last_server_seq;
        }
        _ => {}
    }
    if row.sort_at.as_str() != summary.last_message_at.as_str() {
        return row.sort_at.as_str() > summary.last_message_at.as_str();
    }
    row.msg_id.as_str() > summary.last_message_id.as_str()
}

fn message_is_later_than_message(row: &SummaryMessageRow, current: &SummaryMessageRow) -> bool {
    match (row.server_seq, current.server_seq) {
        (Some(server_seq), Some(current_server_seq)) if server_seq != current_server_seq => {
            return server_seq > current_server_seq;
        }
        _ => {}
    }
    if row.sort_at != current.sort_at {
        return row.sort_at > current.sort_at;
    }
    row.msg_id > current.msg_id
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
        receiver_did: row
            .get::<_, Option<String>>("receiver_did")?
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
        server_seq: row.get::<_, Option<i64>>("server_seq")?,
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
