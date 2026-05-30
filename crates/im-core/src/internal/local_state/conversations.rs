#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ConversationRecord {
    pub(crate) owner_identity_id: String,
    pub(crate) owner_did: String,
    pub(crate) thread_id: String,
    pub(crate) message_count: i64,
    pub(crate) unread_count: i64,
    pub(crate) last_message_at: String,
    pub(crate) last_content: String,
    pub(crate) last_message: Option<super::messages::MessageRecord>,
}

#[cfg(feature = "sqlite")]
pub(crate) fn list_conversations(
    connection: &rusqlite::Connection,
    owner_did: &str,
    query: &crate::messages::ConversationQuery,
) -> crate::ImResult<Vec<ConversationRecord>> {
    list_conversations_for_owner_identity(connection, "", owner_did, query)
}

#[cfg(feature = "sqlite")]
pub(crate) fn list_conversations_for_owner_identity(
    connection: &rusqlite::Connection,
    owner_identity_id: &str,
    owner_did: &str,
    query: &crate::messages::ConversationQuery,
) -> crate::ImResult<Vec<ConversationRecord>> {
    let limit = page_limit(query.limit, 50) + 1;
    let mut statement = String::from(
        r#"
SELECT
    t.owner_identity_id,
    t.owner_did,
    t.thread_id,
    t.message_count,
    t.unread_count,
    t.last_message_at,
    t.last_content,
    m.msg_id,
    m.direction,
    m.sender_did,
    m.receiver_did,
    m.group_id,
    m.group_did,
    m.content_type,
    m.content,
    m.title,
    m.server_seq,
    m.sent_at,
    m.stored_at,
    m.is_e2ee,
    m.is_read,
    m.sender_name,
    m.metadata,
    m.credential_name
FROM threads t
LEFT JOIN messages m
  ON m.owner_identity_id = t.owner_identity_id
 AND m.thread_id = t.thread_id
 AND COALESCE(m.sent_at, m.stored_at) = t.last_message_at
 AND m.msg_id = (
     SELECT m2.msg_id
     FROM messages m2
     WHERE m2.owner_identity_id = t.owner_identity_id
       AND m2.thread_id = t.thread_id
       AND COALESCE(m2.sent_at, m2.stored_at) = t.last_message_at
     ORDER BY m2.msg_id DESC
     LIMIT 1
 )
WHERE t.owner_identity_id = ?1"#,
    );
    if query.unread_only {
        statement.push_str(" AND t.unread_count > 0");
    }
    match (query.include_direct, query.include_groups) {
        (true, true) => {}
        (true, false) => statement.push_str(" AND t.thread_id NOT LIKE 'group:%'"),
        (false, true) => statement.push_str(" AND t.thread_id LIKE 'group:%'"),
        (false, false) => return Ok(Vec::new()),
    }
    statement.push_str(
        r#"
ORDER BY t.last_message_at DESC, t.thread_id ASC
LIMIT ?2"#,
    );
    let owner_identity_id = required_owner_identity_id(owner_identity_id)?;
    let _owner = normalize_owner_did(owner_did);
    let mut statement = connection
        .prepare(&statement)
        .map_err(super::local_state_unavailable)?;
    let rows = statement
        .query_map((&owner_identity_id, limit), |row| {
            let msg_id = row.get::<_, Option<String>>("msg_id")?.unwrap_or_default();
            let last_message = if msg_id.trim().is_empty() {
                None
            } else {
                Some(super::messages::MessageRecord {
                    msg_id,
                    owner_identity_id: row
                        .get::<_, Option<String>>("owner_identity_id")?
                        .unwrap_or_default(),
                    owner_did: row
                        .get::<_, Option<String>>("owner_did")?
                        .unwrap_or_default(),
                    thread_id: row
                        .get::<_, Option<String>>("thread_id")?
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
                    title: row.get::<_, Option<String>>("title")?.unwrap_or_default(),
                    server_seq: row.get::<_, Option<i64>>("server_seq")?,
                    sent_at: row.get::<_, Option<String>>("sent_at")?.unwrap_or_default(),
                    stored_at: row
                        .get::<_, Option<String>>("stored_at")?
                        .unwrap_or_default(),
                    is_e2ee: row.get::<_, Option<i64>>("is_e2ee")?.unwrap_or_default() != 0,
                    is_read: row.get::<_, Option<i64>>("is_read")?.unwrap_or_default() != 0,
                    sender_name: row
                        .get::<_, Option<String>>("sender_name")?
                        .unwrap_or_default(),
                    metadata: row
                        .get::<_, Option<String>>("metadata")?
                        .unwrap_or_default(),
                    credential_name: row
                        .get::<_, Option<String>>("credential_name")?
                        .unwrap_or_default(),
                })
            };
            Ok(ConversationRecord {
                owner_identity_id: row
                    .get::<_, Option<String>>("owner_identity_id")?
                    .unwrap_or_default(),
                owner_did: row
                    .get::<_, Option<String>>("owner_did")?
                    .unwrap_or_default(),
                thread_id: row
                    .get::<_, Option<String>>("thread_id")?
                    .unwrap_or_default(),
                message_count: row
                    .get::<_, Option<i64>>("message_count")?
                    .unwrap_or_default(),
                unread_count: row
                    .get::<_, Option<i64>>("unread_count")?
                    .unwrap_or_default(),
                last_message_at: row
                    .get::<_, Option<String>>("last_message_at")?
                    .unwrap_or_default(),
                last_content: row
                    .get::<_, Option<String>>("last_content")?
                    .unwrap_or_default(),
                last_message,
            })
        })
        .map_err(super::local_state_unavailable)?;
    let mut result = Vec::new();
    for row in rows {
        result.push(row.map_err(super::local_state_unavailable)?);
    }
    Ok(result)
}

#[cfg(feature = "sqlite")]
fn page_limit(limit: crate::ids::PageLimit, fallback: i64) -> i64 {
    if limit.0 == 0 {
        fallback
    } else {
        i64::from(limit.0)
    }
}

#[cfg(feature = "sqlite")]
fn normalize_owner_did(value: &str) -> String {
    value.trim().to_string()
}

#[cfg(feature = "sqlite")]
fn normalize_owner_identity_id(value: &str) -> String {
    value.trim().to_string()
}

#[cfg(feature = "sqlite")]
fn required_owner_identity_id(value: &str) -> crate::ImResult<String> {
    let value = normalize_owner_identity_id(value);
    if value.is_empty() {
        return Err(crate::ImError::invalid_input(
            Some("owner_identity_id".to_owned()),
            "owner_identity_id is required",
        ));
    }
    Ok(value)
}

#[cfg(all(test, feature = "sqlite"))]
mod tests {
    use super::*;
    use rusqlite::Connection;

    #[test]
    fn local_state_conversations_projects_threads_with_filters() {
        let db = Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        seed_message(
            &db,
            "alice-id",
            "did:example:alice",
            "direct-old",
            "dm:alice:bob",
            0,
            "did:example:bob",
            "did:example:alice",
            "",
            "old",
            "2026-05-21T00:00:01Z",
            1,
        );
        seed_message(
            &db,
            "alice-id",
            "did:example:alice",
            "direct-new",
            "dm:alice:bob",
            0,
            "did:example:bob",
            "did:example:alice",
            "",
            "new",
            "2026-05-21T00:00:03Z",
            0,
        );
        seed_message(
            &db,
            "alice-id",
            "did:example:alice",
            "group-new",
            "group:group-1",
            0,
            "did:example:bob",
            "",
            "did:example:group-1",
            "group",
            "2026-05-21T00:00:04Z",
            0,
        );
        seed_message(
            &db,
            "other-id",
            "did:example:other",
            "other-msg",
            "dm:other:bob",
            0,
            "did:example:bob",
            "did:example:other",
            "",
            "other",
            "2026-05-21T00:00:05Z",
            0,
        );

        let all = list_conversations_for_owner_identity(
            &db,
            "alice-id",
            "did:example:alice",
            &crate::messages::ConversationQuery {
                limit: crate::ids::PageLimit(10),
                include_groups: true,
                include_direct: true,
                unread_only: false,
            },
        )
        .unwrap();

        assert_eq!(
            all.iter()
                .map(|record| record.thread_id.as_str())
                .collect::<Vec<_>>(),
            vec!["group:group-1", "dm:alice:bob"]
        );
        assert_eq!(all[0].message_count, 1);
        assert_eq!(all[0].unread_count, 1);
        assert_eq!(all[1].message_count, 2);
        assert_eq!(all[1].last_content, "new");
        assert_eq!(all[1].last_message.as_ref().unwrap().msg_id, "direct-new");

        let direct_unread = list_conversations_for_owner_identity(
            &db,
            "alice-id",
            "did:example:alice",
            &crate::messages::ConversationQuery {
                limit: crate::ids::PageLimit(10),
                include_groups: false,
                include_direct: true,
                unread_only: true,
            },
        )
        .unwrap();
        assert_eq!(direct_unread.len(), 1);
        assert_eq!(direct_unread[0].thread_id, "dm:alice:bob");

        let none = list_conversations_for_owner_identity(
            &db,
            "alice-id",
            "did:example:alice",
            &crate::messages::ConversationQuery {
                limit: crate::ids::PageLimit(10),
                include_groups: false,
                include_direct: false,
                unread_only: false,
            },
        )
        .unwrap();
        assert!(none.is_empty());
    }

    fn seed_message(
        db: &Connection,
        owner_identity_id: &str,
        owner: &str,
        msg_id: &str,
        thread_id: &str,
        direction: i64,
        sender_did: &str,
        receiver_did: &str,
        group_did: &str,
        content: &str,
        sent_at: &str,
        is_read: i64,
    ) {
        db.execute(
            r#"
INSERT INTO messages
    (msg_id, owner_identity_id, owner_did, thread_id, direction, sender_did, receiver_did, group_id, group_did,
     content_type, content, sent_at, stored_at, is_read)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8, 'text/plain', ?9, ?10, ?10, ?11)"#,
            (
                msg_id,
                owner_identity_id,
                owner,
                thread_id,
                direction,
                sender_did,
                receiver_did,
                group_did,
                content,
                sent_at,
                is_read,
            ),
        )
        .unwrap();
    }

    #[test]
    fn local_state_owner_conversations_match_identity_without_legacy_fallback() {
        let db = Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        seed_identity_message(
            &db,
            "alice-id",
            "did:alice-old",
            "stable",
            "dm:alice:bob",
            "stable",
            "2026-05-21T00:00:02Z",
        );
        seed_identity_message(
            &db,
            "mallory-id",
            "did:alice-new",
            "same-did-other",
            "dm:alice:carol",
            "same-did-other",
            "2026-05-21T00:00:03Z",
        );
        seed_identity_message(
            &db,
            "bob-id",
            "did:alice-new",
            "other",
            "dm:alice:mallory",
            "other",
            "2026-05-21T00:00:04Z",
        );

        let records = list_conversations_for_owner_identity(
            &db,
            "alice-id",
            "did:alice-new",
            &crate::messages::ConversationQuery {
                limit: crate::ids::PageLimit(10),
                include_groups: true,
                include_direct: true,
                unread_only: false,
            },
        )
        .unwrap();

        assert_eq!(
            records
                .iter()
                .map(|record| record.thread_id.as_str())
                .collect::<Vec<_>>(),
            vec!["dm:alice:bob"]
        );
    }

    fn seed_identity_message(
        db: &Connection,
        owner_identity_id: &str,
        owner_did: &str,
        msg_id: &str,
        thread_id: &str,
        content: &str,
        sent_at: &str,
    ) {
        db.execute(
            r#"
INSERT INTO messages
    (msg_id, owner_identity_id, owner_did, thread_id, direction, sender_did, receiver_did,
     content_type, content, sent_at, stored_at, is_read)
VALUES (?1, ?2, ?3, ?4, 0, 'did:example:bob', ?3, 'text/plain', ?5, ?6, ?6, 0)"#,
            (
                msg_id,
                owner_identity_id,
                owner_did,
                thread_id,
                content,
                sent_at,
            ),
        )
        .unwrap();
    }
}
