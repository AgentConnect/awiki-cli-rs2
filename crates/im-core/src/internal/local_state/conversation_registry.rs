use rusqlite::{Connection, OptionalExtension};

const TABLE_ONLY_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS conversation_registry (
    owner_identity_id TEXT NOT NULL,
    owner_did         TEXT NOT NULL DEFAULT '',
    conversation_id   TEXT NOT NULL,
    thread_kind       TEXT NOT NULL,
    thread_id         TEXT NOT NULL,
    activity_at       TEXT NOT NULL,
    created_at        TEXT NOT NULL,
    updated_at        TEXT NOT NULL,
    is_active         INTEGER NOT NULL DEFAULT 1,
    peer_persona_id   TEXT,
    canonical_group_did TEXT,
    lifecycle_state   TEXT NOT NULL DEFAULT 'active',
    resolution_state  TEXT NOT NULL DEFAULT 'legacy_unresolved',
    merged_into_conversation_id TEXT,
    PRIMARY KEY (owner_identity_id, conversation_id)
);
"#;

const INDEX_SQL: &str = r#"
CREATE INDEX IF NOT EXISTS idx_conversation_registry_owner_activity
ON conversation_registry(owner_identity_id, is_active, activity_at DESC, conversation_id DESC);
CREATE UNIQUE INDEX IF NOT EXISTS idx_conversation_registry_active_direct_persona
ON conversation_registry(owner_identity_id, peer_persona_id)
WHERE thread_kind = 'direct'
  AND lifecycle_state = 'active'
  AND resolution_state = 'resolved';
CREATE UNIQUE INDEX IF NOT EXISTS idx_conversation_registry_active_group_did
ON conversation_registry(owner_identity_id, canonical_group_did)
WHERE thread_kind = 'group'
  AND lifecycle_state = 'active'
  AND resolution_state = 'resolved';
"#;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ConversationRegistryRecord {
    pub(crate) owner_identity_id: String,
    pub(crate) owner_did: String,
    pub(crate) conversation_id: String,
    pub(crate) thread_kind: String,
    pub(crate) thread_id: String,
    pub(crate) activity_at: String,
}

pub(crate) fn create_schema(connection: &Connection) -> crate::ImResult<()> {
    connection
        .execute_batch(TABLE_ONLY_SQL)
        .map_err(super::local_state_unavailable)?;
    for (column, definition) in [
        ("peer_persona_id", "TEXT"),
        ("canonical_group_did", "TEXT"),
        ("lifecycle_state", "TEXT NOT NULL DEFAULT 'active'"),
        (
            "resolution_state",
            "TEXT NOT NULL DEFAULT 'legacy_unresolved'",
        ),
        ("merged_into_conversation_id", "TEXT"),
    ] {
        ensure_column(connection, column, definition)?;
    }
    connection
        .execute_batch(INDEX_SQL)
        .map_err(super::local_state_unavailable)?;
    Ok(())
}

fn ensure_column(connection: &Connection, column: &str, definition: &str) -> crate::ImResult<()> {
    let exists = {
        let mut statement = connection
            .prepare("PRAGMA table_info(conversation_registry)")
            .map_err(super::local_state_unavailable)?;
        let rows = statement
            .query_map([], |row| row.get::<_, String>(1))
            .map_err(super::local_state_unavailable)?;
        let mut exists = false;
        for row in rows {
            if row.map_err(super::local_state_unavailable)? == column {
                exists = true;
                break;
            }
        }
        exists
    };
    if !exists {
        connection
            .execute(
                &format!("ALTER TABLE conversation_registry ADD COLUMN {column} {definition}"),
                [],
            )
            .map_err(super::local_state_unavailable)?;
    }
    Ok(())
}

pub(crate) fn backfill_from_summaries(connection: &Connection) -> crate::ImResult<usize> {
    let changed = connection
        .execute(
            r#"
INSERT OR IGNORE INTO conversation_registry
    (owner_identity_id, owner_did, conversation_id, thread_kind, thread_id,
     activity_at, created_at, updated_at, is_active, canonical_group_did,
     lifecycle_state, resolution_state)
SELECT owner_identity_id,
       owner_did,
       conversation_id,
       CASE WHEN conversation_id LIKE 'group:%' THEN 'group' ELSE 'thread' END,
       thread_id,
       COALESCE(NULLIF(last_message_at, ''), updated_at),
       updated_at,
       updated_at,
       1
       ,CASE WHEN conversation_id LIKE 'group:%' THEN SUBSTR(conversation_id, 7) ELSE NULL END
       ,'active'
       ,CASE WHEN conversation_id LIKE 'group:%' THEN 'resolved' ELSE 'legacy_unresolved' END
FROM conversation_summaries"#,
            [],
        )
        .map_err(super::local_state_unavailable)?;
    Ok(changed)
}

pub(crate) fn ensure(
    connection: &Connection,
    record: &ConversationRegistryRecord,
) -> crate::ImResult<()> {
    for (field, value) in [
        ("owner_identity_id", record.owner_identity_id.as_str()),
        ("owner_did", record.owner_did.as_str()),
        ("conversation_id", record.conversation_id.as_str()),
        ("thread_kind", record.thread_kind.as_str()),
        ("thread_id", record.thread_id.as_str()),
        ("activity_at", record.activity_at.as_str()),
    ] {
        if value.trim().is_empty() {
            return Err(crate::ImError::invalid_input(
                Some(field.to_owned()),
                format!("{field} is required"),
            ));
        }
    }
    let (peer_persona_id, canonical_group_did, resolution_state) =
        canonical_identity_for_record(connection, record)?;
    connection
        .execute(
            r#"
INSERT INTO conversation_registry
    (owner_identity_id, owner_did, conversation_id, thread_kind, thread_id,
     activity_at, created_at, updated_at, is_active, peer_persona_id,
     canonical_group_did, lifecycle_state, resolution_state)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6, ?6, 1, ?7, ?8, 'active', ?9)
ON CONFLICT(owner_identity_id, conversation_id) DO UPDATE SET
    owner_did = excluded.owner_did,
    thread_kind = excluded.thread_kind,
    thread_id = excluded.thread_id,
    activity_at = CASE
        WHEN excluded.activity_at > conversation_registry.activity_at THEN excluded.activity_at
        ELSE conversation_registry.activity_at
    END,
    updated_at = excluded.updated_at,
    is_active = CASE
        WHEN conversation_registry.lifecycle_state = 'merged' THEN 0
        ELSE 1
    END,
    peer_persona_id = COALESCE(excluded.peer_persona_id, conversation_registry.peer_persona_id),
    canonical_group_did = COALESCE(excluded.canonical_group_did, conversation_registry.canonical_group_did),
    lifecycle_state = CASE
        WHEN conversation_registry.lifecycle_state = 'merged' THEN 'merged'
        ELSE 'active'
    END,
    resolution_state = CASE
        WHEN conversation_registry.resolution_state = 'blocked_conflict' THEN 'blocked_conflict'
        WHEN conversation_registry.lifecycle_state = 'merged' THEN conversation_registry.resolution_state
        ELSE excluded.resolution_state
    END"#,
            rusqlite::params![
                record.owner_identity_id,
                record.owner_did,
                record.conversation_id,
                record.thread_kind,
                record.thread_id,
                record.activity_at,
                peer_persona_id,
                canonical_group_did,
                resolution_state,
            ],
        )
        .map_err(super::local_state_unavailable)?;
    Ok(())
}

pub(crate) fn ensure_validated(
    connection: &Connection,
    owner_identity_id: &str,
    owner_did: &str,
    conversation_id: &str,
) -> crate::ImResult<()> {
    let conversation_id = conversation_id.trim();
    let (thread_kind, thread_id) = if let Some(group_ref) = conversation_id.strip_prefix("group:") {
        let group_ref = group_ref.trim();
        crate::ids::Did::parse(group_ref).map_err(|_| {
            crate::ImError::CanonicalGroupIdentityMissing {
                group: group_ref.to_owned(),
            }
        })?;
        let active = connection
            .query_row(
                r#"
SELECT COALESCE(NULLIF(TRIM(group_id), ''), TRIM(group_did))
FROM groups
WHERE owner_identity_id = ?1
  AND group_did = ?2
  AND COALESCE(NULLIF(TRIM(membership_status), ''), 'active')
      NOT IN ('left', 'removed', 'inactive', 'non_member')
LIMIT 1"#,
                (owner_identity_id, group_ref),
                |row| row.get::<_, String>(0),
            )
            .map_err(|err| match err {
                rusqlite::Error::QueryReturnedNoRows => crate::ImError::invalid_input(
                    Some("conversation_id".to_owned()),
                    "group conversation requires an active local membership projection",
                ),
                other => super::local_state_unavailable(other),
            })?;
        ("group".to_owned(), active)
    } else if conversation_id.starts_with("dm:peer-scope:v1:") {
        let route = super::direct_peer_routes::get(connection, owner_identity_id, conversation_id)?
            .ok_or_else(|| {
                crate::ImError::invalid_input(
                    Some("conversation_id".to_owned()),
                    "Direct conversation requires an owner-scoped canonical peer route",
                )
            })?;
        if route
            .peer_persona_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .is_none()
        {
            return Err(crate::ImError::IdentityUnresolved {
                detail: "Direct conversation route is not bound to a verified Persona".to_owned(),
            });
        }
        ("direct".to_owned(), route.conversation_id)
    } else {
        return Err(crate::ImError::invalid_input(
            Some("conversation_id".to_owned()),
            "conversation must use a canonical group: or dm:peer-scope:v1: id",
        ));
    };
    ensure(
        connection,
        &ConversationRegistryRecord {
            owner_identity_id: owner_identity_id.to_owned(),
            owner_did: owner_did.to_owned(),
            conversation_id: conversation_id.to_owned(),
            thread_kind,
            thread_id,
            activity_at: now_utc_like(),
        },
    )?;
    require_active_resolved(connection, owner_identity_id, conversation_id)
}

pub(crate) fn require_active_resolved(
    connection: &Connection,
    owner_identity_id: &str,
    conversation_id: &str,
) -> crate::ImResult<()> {
    let state = connection
        .query_row(
            r#"SELECT lifecycle_state, resolution_state
FROM conversation_registry
WHERE owner_identity_id = ?1 AND conversation_id = ?2"#,
            (owner_identity_id.trim(), conversation_id.trim()),
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()
        .map_err(super::local_state_unavailable)?;
    match state {
        Some((lifecycle, resolution)) if lifecycle == "active" && resolution == "resolved" => {
            Ok(())
        }
        Some((_, resolution)) if resolution == "blocked_conflict" => {
            Err(crate::ImError::IdentityBindingConflict {
                detail: "conversation canonical identity is blocked by a binding conflict"
                    .to_owned(),
            })
        }
        Some(_) | None => Err(crate::ImError::IdentityUnresolved {
            detail: "conversation is not an active resolved canonical conversation".to_owned(),
        }),
    }
}

pub(crate) fn ensure_from_summary(
    connection: &Connection,
    owner_identity_id: &str,
    conversation_id: &str,
) -> crate::ImResult<()> {
    let row = connection.query_row(
        r#"
SELECT owner_did, thread_id, last_message_at, updated_at
FROM conversation_summaries
WHERE owner_identity_id = ?1 AND conversation_id = ?2"#,
        (owner_identity_id, conversation_id),
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        },
    );
    let (owner_did, summary_thread_id, last_message_at, updated_at) = match row {
        Ok(row) => row,
        Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(()),
        Err(err) => return Err(super::local_state_unavailable(err)),
    };
    let (thread_kind, thread_id) = if let Some(group_ref) = conversation_id.strip_prefix("group:") {
        ("group", group_ref.to_owned())
    } else if conversation_id.starts_with("dm:peer-scope:v1:") {
        ("direct", summary_thread_id)
    } else {
        ("thread", summary_thread_id)
    };
    ensure(
        connection,
        &ConversationRegistryRecord {
            owner_identity_id: owner_identity_id.to_owned(),
            owner_did,
            conversation_id: conversation_id.to_owned(),
            thread_kind: thread_kind.to_owned(),
            thread_id,
            activity_at: if last_message_at.trim().is_empty() {
                updated_at
            } else {
                last_message_at
            },
        },
    )
}

pub(crate) fn deactivate(
    connection: &Connection,
    owner_identity_id: &str,
    conversation_id: &str,
) -> crate::ImResult<()> {
    connection
        .execute(
            "UPDATE conversation_registry SET is_active = 0, lifecycle_state = 'archived', updated_at = ?1 WHERE owner_identity_id = ?2 AND conversation_id = ?3",
            rusqlite::params![now_utc_like(), owner_identity_id.trim(), conversation_id.trim()],
        )
        .map_err(super::local_state_unavailable)?;
    Ok(())
}

pub(crate) fn mark_merged(
    connection: &Connection,
    owner_identity_id: &str,
    conversation_id: &str,
    canonical_conversation_id: &str,
) -> crate::ImResult<()> {
    let target_exists = connection
        .query_row(
            r#"SELECT 1 FROM conversation_registry
WHERE owner_identity_id = ?1 AND conversation_id = ?2
  AND lifecycle_state = 'active' AND resolution_state = 'resolved'"#,
            (owner_identity_id.trim(), canonical_conversation_id.trim()),
            |_| Ok(()),
        )
        .optional()
        .map_err(super::local_state_unavailable)?
        .is_some();
    if !target_exists {
        return Err(crate::ImError::IdentityUnresolved {
            detail: "canonical merge target is not an active resolved conversation".to_owned(),
        });
    }
    connection
        .execute(
            r#"UPDATE conversation_registry
SET is_active = 0,
    lifecycle_state = 'merged',
    resolution_state = 'resolved',
    merged_into_conversation_id = ?1,
    updated_at = ?2
WHERE owner_identity_id = ?3 AND conversation_id = ?4"#,
            rusqlite::params![
                canonical_conversation_id.trim(),
                now_utc_like(),
                owner_identity_id.trim(),
                conversation_id.trim(),
            ],
        )
        .map_err(super::local_state_unavailable)?;
    Ok(())
}

fn canonical_identity_for_record(
    connection: &Connection,
    record: &ConversationRegistryRecord,
) -> crate::ImResult<(Option<String>, Option<String>, &'static str)> {
    match record.thread_kind.as_str() {
        "group" => {
            let group_did = record
                .conversation_id
                .strip_prefix("group:")
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_owned);
            Ok((None, group_did, "resolved"))
        }
        "direct" => {
            let persona = connection
                .query_row(
                    r#"SELECT peer_persona_id FROM direct_peer_routes
WHERE owner_identity_id = ?1 AND conversation_id = ?2
  AND TRIM(COALESCE(peer_persona_id, '')) <> ''"#,
                    (
                        record.owner_identity_id.as_str(),
                        record.conversation_id.as_str(),
                    ),
                    |row| row.get::<_, String>(0),
                )
                .optional()
                .map_err(super::local_state_unavailable)?;
            let resolution = if persona.is_some() {
                "resolved"
            } else {
                "legacy_unresolved"
            };
            Ok((persona, None, resolution))
        }
        _ => Ok((None, None, "legacy_unresolved")),
    }
}

pub(crate) fn now_utc_like() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_keeps_empty_conversation_independent_of_summary() {
        let db = Connection::open_in_memory().unwrap();
        create_schema(&db).unwrap();
        let record = ConversationRegistryRecord {
            owner_identity_id: "owner-1".into(),
            owner_did: "did:example:owner".into(),
            conversation_id: "group:g1".into(),
            thread_kind: "group".into(),
            thread_id: "g1".into(),
            activity_at: "2026-07-13T00:00:00Z".into(),
        };
        ensure(&db, &record).unwrap();
        ensure(&db, &record).unwrap();
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM conversation_registry", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            1
        );
    }

    #[test]
    fn ordinary_ensure_never_reactivates_a_merged_legacy_row() {
        let db = Connection::open_in_memory().unwrap();
        create_schema(&db).unwrap();
        let target = ConversationRegistryRecord {
            owner_identity_id: "owner-1".into(),
            owner_did: "did:example:owner".into(),
            conversation_id: "group:did:example:canonical".into(),
            thread_kind: "group".into(),
            thread_id: "did:example:canonical".into(),
            activity_at: "2026-07-13T00:00:00Z".into(),
        };
        let legacy = ConversationRegistryRecord {
            conversation_id: "group:legacy-local-id".into(),
            thread_kind: "thread".into(),
            thread_id: "legacy-local-id".into(),
            ..target.clone()
        };
        ensure(&db, &target).unwrap();
        ensure(&db, &legacy).unwrap();
        mark_merged(
            &db,
            "owner-1",
            &legacy.conversation_id,
            &target.conversation_id,
        )
        .unwrap();

        ensure(&db, &legacy).unwrap();

        assert_eq!(
            db.query_row(
                r#"SELECT is_active, lifecycle_state, resolution_state,
       merged_into_conversation_id
FROM conversation_registry
WHERE owner_identity_id = 'owner-1' AND conversation_id = 'group:legacy-local-id'"#,
                [],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                },
            )
            .unwrap(),
            (
                0,
                "merged".to_owned(),
                "resolved".to_owned(),
                target.conversation_id,
            )
        );
    }

    #[test]
    fn validated_creation_requires_route_or_active_membership() {
        let db = Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        assert!(ensure_validated(
            &db,
            "owner-1",
            "did:example:owner",
            "dm:peer-scope:v1:missing"
        )
        .is_err());
        assert!(ensure_validated(&db, "owner-1", "did:example:owner", "group:missing").is_err());

        db.execute(
            r#"INSERT INTO groups
               (owner_identity_id, owner_did, group_id, group_did, membership_status, stored_at)
               VALUES ('owner-1', 'did:example:owner', 'g1', 'did:example:group', 'active', '2026-07-13T00:00:00Z')"#,
            [],
        )
        .unwrap();
        ensure_validated(
            &db,
            "owner-1",
            "did:example:owner",
            "group:did:example:group",
        )
        .unwrap();
        ensure_validated(
            &db,
            "owner-1",
            "did:example:owner",
            "group:did:example:group",
        )
        .unwrap();
        assert_eq!(
            db.query_row(
                "SELECT COUNT(*) FROM conversation_registry WHERE conversation_id = 'group:did:example:group'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
            1
        );
    }

    #[test]
    fn summary_registration_keeps_unresolved_direct_message_visible() {
        let db = Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        let conversation_id = "dm:did:example:peer";
        db.execute(
            r#"
INSERT INTO conversation_summaries
    (owner_identity_id, owner_did, conversation_id, thread_id,
     message_count, unread_count, unread_mention_count,
     last_message_at, updated_at)
VALUES ('owner-1', 'did:example:owner', ?1, ?1, 1, 1, 0,
        '2026-07-13T00:00:00Z', '2026-07-13T00:00:00Z')"#,
            [conversation_id],
        )
        .unwrap();
        ensure_from_summary(&db, "owner-1", conversation_id).unwrap();

        let ids = db
            .prepare("SELECT conversation_id FROM conversation_registry ORDER BY conversation_id")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(ids, vec![conversation_id]);
    }
}
