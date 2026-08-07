//! Append-only, conflict-visible mapping from verified legacy references to canonical conversations.

use rusqlite::{Connection, OptionalExtension};

pub(crate) const TABLE_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS conversation_aliases (
    owner_identity_id       TEXT NOT NULL,
    alias_kind              TEXT NOT NULL,
    alias_conversation_id   TEXT NOT NULL,
    canonical_conversation_id TEXT NOT NULL,
    source                  TEXT NOT NULL,
    created_at              TEXT NOT NULL,
    verified_at             TEXT NOT NULL,
    PRIMARY KEY (owner_identity_id, alias_kind, alias_conversation_id),
    FOREIGN KEY (owner_identity_id, canonical_conversation_id)
      REFERENCES conversation_registry(owner_identity_id, conversation_id)
);
CREATE INDEX IF NOT EXISTS idx_conversation_aliases_owner_target
ON conversation_aliases(owner_identity_id, canonical_conversation_id);
"#;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ConversationAliasRecord {
    pub(crate) owner_identity_id: String,
    pub(crate) alias_kind: String,
    pub(crate) alias_conversation_id: String,
    pub(crate) canonical_conversation_id: String,
    pub(crate) source: String,
    pub(crate) verified_at: String,
}

pub(crate) fn create_schema(connection: &Connection) -> crate::ImResult<()> {
    connection
        .execute_batch(TABLE_SQL)
        .map_err(super::local_state_unavailable)
}

pub(crate) fn insert(
    connection: &Connection,
    record: &ConversationAliasRecord,
) -> crate::ImResult<()> {
    validate(record)?;
    let target_is_alias = connection
        .query_row(
            r#"SELECT 1 FROM conversation_aliases
WHERE owner_identity_id = ?1 AND alias_conversation_id = ?2 LIMIT 1"#,
            (
                record.owner_identity_id.trim(),
                record.canonical_conversation_id.trim(),
            ),
            |_| Ok(()),
        )
        .optional()
        .map_err(super::local_state_unavailable)?
        .is_some();
    if target_is_alias {
        return Err(crate::ImError::IdentityBindingConflict {
            detail: "conversation alias target must be a canonical registry id, not another alias"
                .to_owned(),
        });
    }
    let target_is_canonical_resolved = connection
        .query_row(
            r#"SELECT 1 FROM conversation_registry
WHERE owner_identity_id = ?1 AND conversation_id = ?2
  AND lifecycle_state <> 'merged' AND resolution_state = 'resolved'
LIMIT 1"#,
            (
                record.owner_identity_id.trim(),
                record.canonical_conversation_id.trim(),
            ),
            |_| Ok(()),
        )
        .optional()
        .map_err(super::local_state_unavailable)?
        .is_some();
    if !target_is_canonical_resolved {
        return Err(crate::ImError::IdentityUnresolved {
            detail: "conversation alias target must be a resolved canonical registry row"
                .to_owned(),
        });
    }
    let created_at = time::OffsetDateTime::now_utc().unix_timestamp().to_string();
    connection
        .execute(
            r#"INSERT OR IGNORE INTO conversation_aliases
    (owner_identity_id, alias_kind, alias_conversation_id,
     canonical_conversation_id, source, created_at, verified_at)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)"#,
            rusqlite::params![
                record.owner_identity_id.trim(),
                record.alias_kind.trim(),
                record.alias_conversation_id.trim(),
                record.canonical_conversation_id.trim(),
                record.source.trim(),
                created_at,
                record.verified_at.trim(),
            ],
        )
        .map_err(super::local_state_unavailable)?;
    let existing_target = connection
        .query_row(
            r#"SELECT canonical_conversation_id FROM conversation_aliases
WHERE owner_identity_id = ?1 AND alias_kind = ?2 AND alias_conversation_id = ?3"#,
            (
                record.owner_identity_id.trim(),
                record.alias_kind.trim(),
                record.alias_conversation_id.trim(),
            ),
            |row| row.get::<_, String>(0),
        )
        .map_err(super::local_state_unavailable)?;
    if existing_target != record.canonical_conversation_id.trim() {
        return Err(crate::ImError::ConversationAliasConflict {
            alias: record.alias_conversation_id.trim().to_owned(),
            existing_target,
            requested_target: record.canonical_conversation_id.trim().to_owned(),
        });
    }
    Ok(())
}

pub(crate) fn resolve(
    connection: &Connection,
    owner_identity_id: &str,
    alias_kind: &str,
    alias_conversation_id: &str,
) -> crate::ImResult<Option<String>> {
    connection
        .query_row(
            r#"SELECT canonical_conversation_id
FROM conversation_aliases
WHERE owner_identity_id = ?1 AND alias_kind = ?2 AND alias_conversation_id = ?3"#,
            (
                owner_identity_id.trim(),
                alias_kind.trim(),
                alias_conversation_id.trim(),
            ),
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(super::local_state_unavailable)
}

fn validate(record: &ConversationAliasRecord) -> crate::ImResult<()> {
    for (field, value) in [
        ("owner_identity_id", record.owner_identity_id.as_str()),
        ("alias_kind", record.alias_kind.as_str()),
        (
            "alias_conversation_id",
            record.alias_conversation_id.as_str(),
        ),
        (
            "canonical_conversation_id",
            record.canonical_conversation_id.as_str(),
        ),
        ("source", record.source.as_str()),
        ("verified_at", record.verified_at.as_str()),
    ] {
        if value.trim().is_empty() {
            return Err(crate::ImError::invalid_input(
                Some(field.to_owned()),
                format!("{field} is required"),
            ));
        }
    }
    if record.alias_conversation_id.trim() == record.canonical_conversation_id.trim() {
        return Err(crate::ImError::invalid_input(
            Some("alias_conversation_id".to_owned()),
            "alias must not equal its canonical target",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alias_insert_is_idempotent_but_never_last_write_wins() {
        let db = Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        for conversation_id in ["group:did:example:first", "group:did:example:second"] {
            crate::internal::local_state::conversation_registry::ensure(
                &db,
                &crate::internal::local_state::conversation_registry::ConversationRegistryRecord {
                    owner_identity_id: "owner".to_owned(),
                    owner_did: "did:example:owner".to_owned(),
                    conversation_id: conversation_id.to_owned(),
                    thread_kind: "group".to_owned(),
                    thread_id: conversation_id.trim_start_matches("group:").to_owned(),
                    activity_at: "2026-07-14T00:00:00Z".to_owned(),
                },
            )
            .unwrap();
        }
        let record = ConversationAliasRecord {
            owner_identity_id: "owner".to_owned(),
            alias_kind: "legacy_direct_did".to_owned(),
            alias_conversation_id: "dm:did:example:peer".to_owned(),
            canonical_conversation_id: "group:did:example:first".to_owned(),
            source: "verified_route".to_owned(),
            verified_at: "2026-07-14T00:00:00Z".to_owned(),
        };
        insert(&db, &record).unwrap();
        insert(&db, &record).unwrap();
        let conflict = insert(
            &db,
            &ConversationAliasRecord {
                canonical_conversation_id: "group:did:example:second".to_owned(),
                ..record
            },
        )
        .unwrap_err();
        assert!(matches!(
            conflict,
            crate::ImError::ConversationAliasConflict { .. }
        ));
    }

    #[test]
    fn alias_rejects_missing_or_unresolved_targets() {
        let db = Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        let record = ConversationAliasRecord {
            owner_identity_id: "owner".to_owned(),
            alias_kind: "legacy_direct_did".to_owned(),
            alias_conversation_id: "dm:did:example:peer".to_owned(),
            canonical_conversation_id: "dm:peer-scope:v1:missing".to_owned(),
            source: "verified_route".to_owned(),
            verified_at: "2026-07-14T00:00:00Z".to_owned(),
        };
        assert!(matches!(
            insert(&db, &record).unwrap_err(),
            crate::ImError::IdentityUnresolved { .. }
        ));

        crate::internal::local_state::conversation_registry::ensure(
            &db,
            &crate::internal::local_state::conversation_registry::ConversationRegistryRecord {
                owner_identity_id: "owner".to_owned(),
                owner_did: "did:example:owner".to_owned(),
                conversation_id: record.canonical_conversation_id.clone(),
                thread_kind: "direct".to_owned(),
                thread_id: record.canonical_conversation_id.clone(),
                activity_at: "2026-07-14T00:00:00Z".to_owned(),
            },
        )
        .unwrap();
        assert!(matches!(
            insert(&db, &record).unwrap_err(),
            crate::ImError::IdentityUnresolved { .. }
        ));
    }

    #[test]
    fn alias_survives_canonical_target_archive_but_rejects_merged_target() {
        let db = Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        let target = "group:did:example:archived";
        crate::internal::local_state::conversation_registry::ensure(
            &db,
            &crate::internal::local_state::conversation_registry::ConversationRegistryRecord {
                owner_identity_id: "owner".to_owned(),
                owner_did: "did:example:owner".to_owned(),
                conversation_id: target.to_owned(),
                thread_kind: "group".to_owned(),
                thread_id: "did:example:archived".to_owned(),
                activity_at: "2026-07-14T00:00:00Z".to_owned(),
            },
        )
        .unwrap();
        crate::internal::local_state::conversation_registry::deactivate(&db, "owner", target)
            .unwrap();
        let record = ConversationAliasRecord {
            owner_identity_id: "owner".to_owned(),
            alias_kind: "release_0710_group_id".to_owned(),
            alias_conversation_id: "group:legacy-archived".to_owned(),
            canonical_conversation_id: target.to_owned(),
            source: "test".to_owned(),
            verified_at: "2026-07-14T00:00:00Z".to_owned(),
        };
        insert(&db, &record).unwrap();
        assert_eq!(
            resolve(
                &db,
                "owner",
                "release_0710_group_id",
                "group:legacy-archived"
            )
            .unwrap()
            .as_deref(),
            Some(target)
        );
        assert!(
            crate::internal::local_state::canonical_invariants::check(&db, "owner")
                .unwrap()
                .is_empty()
        );

        db.execute(
            "UPDATE conversation_registry SET lifecycle_state = 'merged' WHERE owner_identity_id = 'owner' AND conversation_id = ?1",
            [target],
        )
        .unwrap();
        let rejected = ConversationAliasRecord {
            alias_conversation_id: "group:another-legacy".to_owned(),
            ..record
        };
        assert!(matches!(
            insert(&db, &rejected).unwrap_err(),
            crate::ImError::IdentityUnresolved { .. }
        ));
    }
}
