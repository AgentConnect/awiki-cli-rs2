#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ConversationRecord {
    pub(crate) owner_identity_id: String,
    pub(crate) owner_did: String,
    pub(crate) conversation_id: String,
    pub(crate) peer_persona_id: String,
    pub(crate) canonical_group_did: String,
    pub(crate) resolution_state: String,
    pub(crate) thread_kind: String,
    pub(crate) thread_id: String,
    pub(crate) title: String,
    pub(crate) direct_peer_did: String,
    pub(crate) activity_at: String,
    pub(crate) message_count: i64,
    pub(crate) unread_count: i64,
    pub(crate) unread_mention_count: i64,
    pub(crate) first_unread_mention_message_id: Option<String>,
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
    let owner_identity_id = required_owner_identity_id(owner_identity_id)?;
    super::conversation_summaries::ensure_owner_backfilled(connection, &owner_identity_id)?;
    let limit = page_limit(query.limit, 50) + 1;
    let cursor = decode_conversation_cursor(query.cursor.as_ref().map(crate::ids::Cursor::as_str))?;
    let mut statement = String::from(
        r#"
SELECT
    r.owner_identity_id,
    r.owner_did,
    r.conversation_id,
    COALESCE(r.peer_persona_id, '') AS peer_persona_id,
    COALESCE(r.canonical_group_did, '') AS canonical_group_did,
    r.resolution_state,
    r.thread_kind,
    r.thread_id,
    COALESCE(
      (
        SELECT g.name
        FROM groups g
        WHERE g.owner_identity_id = r.owner_identity_id
          AND r.thread_kind = 'group'
          AND (
            g.group_did = r.canonical_group_did
            OR g.group_id = r.canonical_group_did
          )
        ORDER BY g.stored_at DESC,
        g.group_id DESC
        LIMIT 1
      ),
      ''
    ) AS conversation_title,
    COALESCE(d.current_did, '') AS direct_peer_did,
    r.activity_at,
    COALESCE(t.message_count, 0) AS message_count,
    COALESCE(t.unread_count, 0) AS unread_count,
    COALESCE(t.unread_mention_count, 0) AS unread_mention_count,
    t.first_unread_mention_message_id,
    COALESCE(t.last_message_at, '') AS last_message_at,
    COALESCE(t.last_content, '') AS last_content,
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
    m.mentions_current_user,
    m.credential_name
FROM conversation_registry r
LEFT JOIN conversation_summaries t
  ON t.owner_identity_id = r.owner_identity_id
 AND t.conversation_id = r.conversation_id
LEFT JOIN messages m
  ON m.owner_identity_id = r.owner_identity_id
 AND m.msg_id = t.last_message_id
LEFT JOIN direct_peer_routes d
  ON d.owner_identity_id = r.owner_identity_id
 AND d.conversation_id = r.conversation_id
WHERE r.owner_identity_id = ?1 AND r.is_active = 1"#,
    );
    if cursor.is_some() {
        statement.push_str(
            r#"
 AND (r.activity_at < ?3 OR (r.activity_at = ?3 AND r.conversation_id < ?4))"#,
        );
    }
    if query.unread_only {
        statement.push_str(" AND COALESCE(t.unread_count, 0) > 0");
    }
    match (query.include_direct, query.include_groups) {
        (true, true) => {}
        (true, false) => statement.push_str(" AND r.thread_kind <> 'group'"),
        (false, true) => statement.push_str(" AND r.thread_kind = 'group'"),
        (false, false) => return Ok(Vec::new()),
    }
    statement.push_str(
        r#"
ORDER BY r.activity_at DESC, r.conversation_id DESC
LIMIT ?2"#,
    );
    let _owner = normalize_owner_did(owner_did);
    let mut statement = connection
        .prepare(&statement)
        .map_err(super::local_state_unavailable)?;
    let mut rows = if let Some((cursor_activity_at, cursor_conversation_id)) = cursor {
        statement
            .query_map(
                (
                    &owner_identity_id,
                    limit,
                    cursor_activity_at,
                    cursor_conversation_id,
                ),
                conversation_record_from_row,
            )
            .map_err(super::local_state_unavailable)?
    } else {
        statement
            .query_map((&owner_identity_id, limit), conversation_record_from_row)
            .map_err(super::local_state_unavailable)?
    };
    let mut result = Vec::new();
    for row in &mut rows {
        let record = row.map_err(super::local_state_unavailable)?;
        if is_self_direct_record(&record) {
            continue;
        }
        result.push(record);
    }
    Ok(result)
}

#[cfg(feature = "sqlite")]
fn is_self_direct_record(record: &ConversationRecord) -> bool {
    let Some(last_message) = record.last_message.as_ref() else {
        return false;
    };
    let owner = record.owner_did.trim();
    !owner.is_empty()
        && last_message.group_id.trim().is_empty()
        && last_message.group_did.trim().is_empty()
        && last_message.sender_did.trim() == owner
        && last_message.receiver_did.trim() == owner
}

#[cfg(feature = "sqlite")]
fn conversation_record_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ConversationRecord> {
    let msg_id = row.get::<_, Option<String>>("msg_id")?.unwrap_or_default();
    let owner_identity_id = row
        .get::<_, Option<String>>("owner_identity_id")?
        .unwrap_or_default();
    let owner_did = row
        .get::<_, Option<String>>("owner_did")?
        .unwrap_or_default();
    let conversation_id = row
        .get::<_, Option<String>>("conversation_id")?
        .unwrap_or_default();
    let peer_persona_id = row
        .get::<_, Option<String>>("peer_persona_id")?
        .unwrap_or_default();
    let canonical_group_did = row
        .get::<_, Option<String>>("canonical_group_did")?
        .unwrap_or_default();
    let resolution_state = row
        .get::<_, Option<String>>("resolution_state")?
        .unwrap_or_default();
    let thread_kind = row
        .get::<_, Option<String>>("thread_kind")?
        .unwrap_or_default();
    let thread_id = row
        .get::<_, Option<String>>("thread_id")?
        .unwrap_or_default();
    let title = row
        .get::<_, Option<String>>("conversation_title")?
        .unwrap_or_default();
    let direct_peer_did = row
        .get::<_, Option<String>>("direct_peer_did")?
        .unwrap_or_default();
    let activity_at = row
        .get::<_, Option<String>>("activity_at")?
        .unwrap_or_default();
    let last_message = if msg_id.trim().is_empty() {
        None
    } else {
        Some(super::messages::MessageRecord {
            msg_id,
            owner_identity_id: owner_identity_id.clone(),
            owner_did: owner_did.clone(),
            conversation_id: conversation_id.clone(),
            wire_thread_kind: String::new(),
            wire_thread_ref: String::new(),
            wire_identity_resolution_state: String::new(),
            thread_id: thread_id.clone(),
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
            hydration_state: super::messages::MessageHydrationState::Hydrated,
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
            mentions_current_user: row
                .get::<_, Option<i64>>("mentions_current_user")?
                .unwrap_or_default()
                != 0,
            credential_name: row
                .get::<_, Option<String>>("credential_name")?
                .unwrap_or_default(),
        })
    };
    Ok(ConversationRecord {
        owner_identity_id,
        owner_did,
        conversation_id,
        peer_persona_id,
        canonical_group_did,
        resolution_state,
        thread_kind,
        thread_id,
        title,
        direct_peer_did,
        activity_at,
        message_count: row
            .get::<_, Option<i64>>("message_count")?
            .unwrap_or_default(),
        unread_count: row
            .get::<_, Option<i64>>("unread_count")?
            .unwrap_or_default(),
        unread_mention_count: row
            .get::<_, Option<i64>>("unread_mention_count")?
            .unwrap_or_default(),
        first_unread_mention_message_id: row
            .get::<_, Option<String>>("first_unread_mention_message_id")?
            .filter(|value| !value.trim().is_empty()),
        last_message_at: row
            .get::<_, Option<String>>("last_message_at")?
            .unwrap_or_default(),
        last_content: row
            .get::<_, Option<String>>("last_content")?
            .unwrap_or_default(),
        last_message,
    })
}

#[cfg(feature = "sqlite")]
pub(crate) fn encode_conversation_cursor(record: &ConversationRecord) -> Option<String> {
    let conversation_id = non_empty(&record.conversation_id)?;
    Some(format!(
        "conversation-list:v2:{}:{}",
        base64_url_encode(record.activity_at.as_str()),
        base64_url_encode(conversation_id)
    ))
}

#[cfg(feature = "sqlite")]
fn decode_conversation_cursor(cursor: Option<&str>) -> crate::ImResult<Option<(String, String)>> {
    let Some(cursor) = cursor.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    let Some(rest) = cursor
        .strip_prefix("conversation-list:v2:")
        .or_else(|| cursor.strip_prefix("conversation-list:v1:"))
    else {
        return Err(crate::ImError::invalid_input(
            Some("cursor".to_owned()),
            "conversation cursor must be produced by conversations",
        ));
    };
    let Some((last_message_at, conversation_id)) = rest.split_once(':') else {
        return Err(crate::ImError::invalid_input(
            Some("cursor".to_owned()),
            "conversation cursor is malformed",
        ));
    };
    let last_message_at = base64_url_decode(last_message_at, "cursor")?;
    let conversation_id = base64_url_decode(conversation_id, "cursor")?;
    required("cursor.conversation_id", &conversation_id)?;
    Ok(Some((last_message_at, conversation_id)))
}

#[cfg(feature = "sqlite")]
fn non_empty(value: &str) -> Option<&str> {
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

#[cfg(feature = "sqlite")]
fn required(field: &'static str, value: &str) -> crate::ImResult<()> {
    if value.trim().is_empty() {
        return Err(crate::ImError::invalid_input(
            Some(field.to_owned()),
            format!("{field} is required"),
        ));
    }
    Ok(())
}

#[cfg(feature = "sqlite")]
fn base64_url_encode(value: &str) -> String {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};

    URL_SAFE_NO_PAD.encode(value.as_bytes())
}

#[cfg(feature = "sqlite")]
fn base64_url_decode(value: &str, field: &'static str) -> crate::ImResult<String> {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};

    let bytes = URL_SAFE_NO_PAD
        .decode(value.as_bytes())
        .map_err(|err| crate::ImError::invalid_input(Some(field.to_owned()), err.to_string()))?;
    String::from_utf8(bytes)
        .map_err(|err| crate::ImError::invalid_input(Some(field.to_owned()), err.to_string()))
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
    fn empty_direct_and_group_conversations_are_listed_without_messages() {
        let db = Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        let route = crate::internal::local_state::direct_peer_routes::DirectPeerRouteRecord::new(
            "alice-id",
            crate::internal::local_state::owner_scope::direct_conversation_id_for_peer_scope(
                &crate::internal::local_state::owner_scope::DirectPeerScope::new(
                    "bob-user",
                    "bob.awiki.info",
                )
                .unwrap(),
            ),
            "bob-user",
            "bob.awiki.info",
            "did:example:bob",
        )
        .unwrap();
        crate::internal::local_state::direct_peer_routes::upsert(&db, &route).unwrap();
        db.execute(
            r#"INSERT INTO groups
               (owner_identity_id, owner_did, group_id, group_did, name, stored_at)
               VALUES (?1, ?2, ?3, ?3, ?4, ?5)"#,
            (
                "alice-id",
                "did:example:alice",
                "g1",
                "Group One",
                "2026-07-13T00:00:00Z",
            ),
        )
        .unwrap();
        db.execute(
            r#"INSERT INTO groups
               (owner_identity_id, owner_did, group_id, group_did, name, stored_at)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6)"#,
            (
                "alice-id",
                "did:example:alice",
                "legacy-g1-alias",
                "g1",
                "Legacy Group One",
                "2026-07-12T00:00:00Z",
            ),
        )
        .unwrap();
        for record in [
            crate::internal::local_state::conversation_registry::ConversationRegistryRecord {
                owner_identity_id: "alice-id".into(),
                owner_did: "did:example:alice".into(),
                conversation_id: route.conversation_id.clone(),
                thread_kind: "direct".into(),
                thread_id: route.conversation_id.clone(),
                activity_at: "2026-07-13T00:00:02Z".into(),
            },
            crate::internal::local_state::conversation_registry::ConversationRegistryRecord {
                owner_identity_id: "alice-id".into(),
                owner_did: "did:example:alice".into(),
                conversation_id: "group:g1".into(),
                thread_kind: "group".into(),
                thread_id: "g1".into(),
                activity_at: "2026-07-13T00:00:01Z".into(),
            },
        ] {
            crate::internal::local_state::conversation_registry::ensure(&db, &record).unwrap();
        }

        let rows = list_conversations_for_owner_identity(
            &db,
            "alice-id",
            "did:example:alice",
            &crate::messages::ConversationQuery {
                limit: crate::ids::PageLimit(10),
                cursor: None,
                include_groups: true,
                include_direct: true,
                unread_only: false,
            },
        )
        .unwrap();
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|row| row.message_count == 0));
        assert!(rows.iter().all(|row| row.last_message.is_none()));
        assert_eq!(rows[0].thread_kind, "direct");
        assert_eq!(rows[0].direct_peer_did, "did:example:bob");
        assert_eq!(rows[1].thread_kind, "group");
        assert_eq!(rows[1].title, "Group One");
    }

    #[test]
    fn local_state_conversations_projects_threads_with_filters() {
        let db = Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        seed_message(
            &db,
            "alice-id",
            "did:example:alice",
            "direct-old",
            "dm:did:example:bob",
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
            "dm:did:example:bob",
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
            "dm:did:example:bob",
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
                cursor: None,
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
            vec!["group:group-1", "dm:did:example:bob"]
        );
        assert_eq!(all[0].message_count, 1);
        assert_eq!(all[0].unread_count, 1);
        assert_eq!(all[0].unread_mention_count, 0);
        assert_eq!(all[1].message_count, 2);
        assert_eq!(all[1].last_content, "new");
        assert_eq!(all[1].last_message.as_ref().unwrap().msg_id, "direct-new");

        let direct_unread = list_conversations_for_owner_identity(
            &db,
            "alice-id",
            "did:example:alice",
            &crate::messages::ConversationQuery {
                limit: crate::ids::PageLimit(10),
                cursor: None,
                include_groups: false,
                include_direct: true,
                unread_only: true,
            },
        )
        .unwrap();
        assert_eq!(direct_unread.len(), 1);
        assert_eq!(direct_unread[0].thread_id, "dm:did:example:bob");

        let none = list_conversations_for_owner_identity(
            &db,
            "alice-id",
            "did:example:alice",
            &crate::messages::ConversationQuery {
                limit: crate::ids::PageLimit(10),
                cursor: None,
                include_groups: false,
                include_direct: false,
                unread_only: false,
            },
        )
        .unwrap();
        assert!(none.is_empty());
    }

    #[test]
    fn local_state_conversations_hide_self_direct_rows() {
        let db = Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        seed_message(
            &db,
            "alice-id",
            "did:example:alice",
            "self-direct",
            "dm:did:example:alice",
            1,
            "did:example:alice",
            "did:example:alice",
            "",
            "self direct",
            "2026-05-21T00:00:05Z",
            1,
        );
        seed_message(
            &db,
            "alice-id",
            "did:example:alice",
            "bob-direct",
            "dm:did:example:bob",
            0,
            "did:example:bob",
            "did:example:alice",
            "",
            "hello",
            "2026-05-21T00:00:04Z",
            0,
        );

        let records = list_conversations_for_owner_identity(
            &db,
            "alice-id",
            "did:example:alice",
            &crate::messages::ConversationQuery {
                limit: crate::ids::PageLimit(10),
                cursor: None,
                include_groups: false,
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
            vec!["dm:did:example:bob"]
        );
    }

    #[test]
    fn local_state_conversations_projects_unread_mentions_for_owner() {
        let db = Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        seed_payload_message(
            &db,
            "alice-mention",
            "did:example:alice",
            "group:mentions",
            "did:example:bob",
            human_mention_payload("did:example:alice"),
            "2026-05-21T00:00:01Z",
            0,
        );
        seed_payload_message(
            &db,
            "alice-read-mention",
            "did:example:alice",
            "group:mentions",
            "did:example:bob",
            human_mention_payload("did:example:alice"),
            "2026-05-21T00:00:02Z",
            1,
        );
        seed_payload_message(
            &db,
            "all-humans",
            "did:example:alice",
            "group:mentions",
            "did:example:bob",
            selector_mention_payload("humans"),
            "2026-05-21T00:00:03Z",
            0,
        );
        seed_payload_message(
            &db,
            "all-agents",
            "did:example:alice",
            "group:mentions",
            "did:example:bob",
            selector_mention_payload("agents"),
            "2026-05-21T00:00:04Z",
            0,
        );
        seed_message(
            &db,
            "alice-id",
            "did:example:alice",
            "plain-at",
            "group:mentions",
            0,
            "did:example:bob",
            "",
            "did:example:mentions",
            "@alice 不是结构化 mention",
            "2026-05-21T00:00:05Z",
            0,
        );

        let conversations = list_conversations_for_owner_identity(
            &db,
            "alice-id",
            "did:example:alice",
            &crate::messages::ConversationQuery {
                limit: crate::ids::PageLimit(10),
                cursor: None,
                include_groups: true,
                include_direct: false,
                unread_only: false,
            },
        )
        .unwrap();

        assert_eq!(conversations.len(), 1);
        assert_eq!(conversations[0].unread_count, 4);
        assert_eq!(conversations[0].unread_mention_count, 2);
        assert_eq!(
            conversations[0].first_unread_mention_message_id.as_deref(),
            Some("alice-mention")
        );
    }

    #[test]
    fn local_state_conversations_pages_with_stable_sort_cursor() {
        let db = Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        seed_message(
            &db,
            "alice-id",
            "did:example:alice",
            "conv-b",
            "dm:did:example:b",
            0,
            "did:example:b",
            "did:example:alice",
            "",
            "b",
            "2026-05-21T00:00:03Z",
            0,
        );
        seed_message(
            &db,
            "alice-id",
            "did:example:alice",
            "conv-a",
            "dm:did:example:a",
            0,
            "did:example:a",
            "did:example:alice",
            "",
            "a",
            "2026-05-21T00:00:03Z",
            0,
        );
        seed_message(
            &db,
            "alice-id",
            "did:example:alice",
            "conv-c",
            "dm:did:example:c",
            0,
            "did:example:c",
            "did:example:alice",
            "",
            "c",
            "2026-05-21T00:00:02Z",
            0,
        );

        let first_page = list_conversations_for_owner_identity(
            &db,
            "alice-id",
            "did:example:alice",
            &crate::messages::ConversationQuery {
                limit: crate::ids::PageLimit(1),
                cursor: None,
                include_groups: false,
                include_direct: true,
                unread_only: false,
            },
        )
        .unwrap();

        assert_eq!(
            first_page
                .iter()
                .map(|record| record.conversation_id.as_str())
                .collect::<Vec<_>>(),
            vec!["dm:did:example:b", "dm:did:example:a"]
        );

        let cursor =
            crate::ids::Cursor::parse(encode_conversation_cursor(&first_page[0]).unwrap()).unwrap();
        let second_page = list_conversations_for_owner_identity(
            &db,
            "alice-id",
            "did:example:alice",
            &crate::messages::ConversationQuery {
                limit: crate::ids::PageLimit(1),
                cursor: Some(cursor),
                include_groups: false,
                include_direct: true,
                unread_only: false,
            },
        )
        .unwrap();

        assert_eq!(
            second_page
                .iter()
                .map(|record| record.conversation_id.as_str())
                .collect::<Vec<_>>(),
            vec!["dm:did:example:a", "dm:did:example:c"]
        );
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
    (msg_id, owner_identity_id, owner_did, conversation_id, thread_id, direction, sender_did, receiver_did, group_id, group_did,
     content_type, content, sent_at, stored_at, is_read)
VALUES (?1, ?2, ?3, ?4, ?4, ?5, ?6, ?7, ?8, ?8, 'text/plain', ?9, ?10, ?10, ?11)"#,
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

    fn seed_payload_message(
        db: &Connection,
        msg_id: &str,
        owner: &str,
        thread_id: &str,
        sender_did: &str,
        payload: serde_json::Value,
        sent_at: &str,
        is_read: i64,
    ) {
        let content = payload.to_string();
        let mentions_current_user = super::super::messages::mentions_current_user_for_projection(
            owner,
            0,
            thread_id,
            "did:example:mentions",
            "did:example:mentions",
            "application/json",
            &content,
        );
        db.execute(
            r#"
INSERT INTO messages
    (msg_id, owner_identity_id, owner_did, conversation_id, thread_id, direction, sender_did, group_id, group_did,
     content_type, content, sent_at, stored_at, is_read, mentions_current_user)
VALUES (?1, 'alice-id', ?2, ?3, ?3, 0, ?4, 'did:example:mentions', 'did:example:mentions',
        'application/json', ?5, ?6, ?6, ?7, ?8)"#,
            (
                msg_id,
                owner,
                thread_id,
                sender_did,
                content,
                sent_at,
                is_read,
                mentions_current_user,
            ),
        )
        .unwrap();
    }

    fn human_mention_payload(did: &str) -> serde_json::Value {
        serde_json::json!({
            "text": "@Alice 请看",
            "mentions": [{
                "id": "mention-1",
                "range": {
                    "start": 0,
                    "end": 6,
                    "unit": "unicode_code_point"
                },
                "target": {
                    "kind": "human",
                    "did": did,
                    "display_name": "Alice"
                },
                "mention_role": "addressee"
            }]
        })
    }

    fn selector_mention_payload(selector: &str) -> serde_json::Value {
        serde_json::json!({
            "text": "@所有人 请看",
            "mentions": [{
                "id": "mention-selector",
                "range": {
                    "start": 0,
                    "end": 4,
                    "unit": "unicode_code_point"
                },
                "target": {
                    "kind": "group_selector",
                    "selector": selector
                },
                "mention_role": "addressee"
            }]
        })
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
            "dm:did:example:bob",
            "stable",
            "2026-05-21T00:00:02Z",
        );
        seed_identity_message(
            &db,
            "mallory-id",
            "did:alice-new",
            "same-did-other",
            "dm:did:example:carol",
            "same-did-other",
            "2026-05-21T00:00:03Z",
        );
        seed_identity_message(
            &db,
            "bob-id",
            "did:alice-new",
            "other",
            "dm:did:example:mallory",
            "other",
            "2026-05-21T00:00:04Z",
        );

        let records = list_conversations_for_owner_identity(
            &db,
            "alice-id",
            "did:alice-new",
            &crate::messages::ConversationQuery {
                limit: crate::ids::PageLimit(10),
                cursor: None,
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
            vec!["dm:did:example:bob"]
        );
    }

    #[test]
    fn local_state_conversations_group_direct_rows_by_stable_conversation_id() {
        let db = Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        seed_message(
            &db,
            "alice-id",
            "did:example:alice-old",
            "before-did-replace",
            "dm:did:example:bob",
            0,
            "did:example:bob",
            "did:example:alice-old",
            "",
            "old did",
            "2026-05-21T00:00:01Z",
            1,
        );
        seed_message(
            &db,
            "alice-id",
            "did:example:alice-new",
            "after-did-replace",
            "dm:did:example:bob",
            0,
            "did:example:bob",
            "did:example:alice-new",
            "",
            "new did",
            "2026-05-21T00:00:02Z",
            0,
        );

        let records = list_conversations_for_owner_identity(
            &db,
            "alice-id",
            "did:example:alice-new",
            &crate::messages::ConversationQuery {
                limit: crate::ids::PageLimit(10),
                cursor: None,
                include_groups: false,
                include_direct: true,
                unread_only: false,
            },
        )
        .unwrap();

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].conversation_id, "dm:did:example:bob");
        assert_eq!(records[0].thread_id, records[0].conversation_id);
        assert_eq!(records[0].message_count, 2);
        assert_eq!(
            records[0].last_message.as_ref().unwrap().msg_id,
            "after-did-replace"
        );
    }

    #[test]
    fn local_state_conversations_list_is_local_read_without_reconcile_side_effect() {
        let db = Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        seed_identity_message(
            &db,
            "alice-id",
            "did:wba:anpclaw.com:zhuocheng:e1_owner",
            "legacy",
            "dm:did:wba:anpclaw.com:zhuochengtest:e1_old",
            "legacy message",
            "2026-06-10T00:00:01Z",
        );
        let scope = crate::internal::local_state::owner_scope::DirectPeerScope::new(
            "peer-user-id",
            "zhuochengtest.anpclaw.com",
        )
        .unwrap();
        let scoped_conversation_id =
            crate::internal::local_state::owner_scope::direct_conversation_id_for_peer_scope(
                &scope,
            );
        db.execute(
            r#"
INSERT INTO messages
    (msg_id, owner_identity_id, owner_did, conversation_id, thread_id, direction, sender_did, receiver_did,
     content_type, content, sent_at, stored_at, is_read, metadata)
VALUES (?1, ?2, ?3, ?4, ?4, 1, ?3, ?5,
        'text/plain', 'scoped message', '2026-06-10T00:00:02Z', '2026-06-10T00:00:02Z', 1, ?6)"#,
            (
                "scoped",
                "alice-id",
                "did:wba:anpclaw.com:zhuocheng:e1_owner",
                scoped_conversation_id.as_str(),
                "did:wba:anpclaw.com:zhuochengtest:e1_new",
                r#"{"peer_user_id":"peer-user-id","peer_full_handle":"zhuochengtest.anpclaw.com","peer_current_did":"did:wba:anpclaw.com:zhuochengtest:e1_new"}"#,
            ),
        )
        .unwrap();

        let records = list_conversations_for_owner_identity(
            &db,
            "alice-id",
            "did:wba:anpclaw.com:zhuocheng:e1_owner",
            &crate::messages::ConversationQuery {
                limit: crate::ids::PageLimit(10),
                cursor: None,
                include_groups: false,
                include_direct: true,
                unread_only: false,
            },
        )
        .unwrap();

        assert_eq!(records.len(), 2);
        let scoped_record = records
            .iter()
            .find(|record| record.conversation_id == scoped_conversation_id)
            .unwrap();
        assert_eq!(scoped_record.message_count, 1);
        let legacy_conversation_id: String = db
            .query_row(
                "SELECT conversation_id FROM messages WHERE msg_id = 'legacy'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            legacy_conversation_id,
            "dm:did:wba:anpclaw.com:zhuochengtest:e1_old",
        );
    }

    #[test]
    fn local_state_conversations_query_uses_materialized_summary_index() {
        let db = Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        seed_message(
            &db,
            "alice-id",
            "did:example:alice",
            "direct-new",
            "dm:did:example:bob",
            0,
            "did:example:bob",
            "did:example:alice",
            "",
            "new",
            "2026-05-21T00:00:03Z",
            0,
        );
        super::super::conversation_summaries::rebuild_owner(&db, "alice-id").unwrap();

        let plan = db
            .prepare(
                r#"
EXPLAIN QUERY PLAN
SELECT t.owner_identity_id, t.conversation_id
FROM conversation_summaries t
WHERE t.owner_identity_id = ?1
ORDER BY t.last_message_at DESC, t.conversation_id DESC
LIMIT ?2"#,
            )
            .unwrap()
            .query_map(("alice-id", 10_i64), |row| row.get::<_, String>(3))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
            .join("\n");

        assert!(plan.contains("conversation_summaries"), "{plan}");
        assert!(
            plan.contains("idx_conversation_summaries_owner_last_desc"),
            "{plan}"
        );
        assert!(!plan.contains("threads"), "{plan}");
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
    (msg_id, owner_identity_id, owner_did, conversation_id, thread_id, direction, sender_did, receiver_did,
     content_type, content, sent_at, stored_at, is_read)
VALUES (?1, ?2, ?3, ?4, ?4, 0, 'did:example:bob', ?3, 'text/plain', ?5, ?6, ?6, 0)"#,
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
