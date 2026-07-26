use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use rusqlite::{Connection, Transaction};

use super::{upgrade_failed, CanonicalUpgradeReport, RELEASE_0710_SCHEMA_VERSION};

#[derive(Debug, Clone)]
struct RouteRow {
    owner_identity_id: String,
    owner_did: String,
    old_conversation_id: String,
    peer_user_id: String,
    full_handle: String,
    current_did: String,
    updated_at: String,
}

#[derive(Debug, Clone)]
struct GroupRow {
    owner_identity_id: String,
    owner_did: String,
    group_id: String,
    group_did: String,
    membership_status: String,
    activity_at: String,
}

#[derive(Debug, Clone)]
struct HandleDidBinding {
    did: String,
    is_current: bool,
    binding_generation: Option<String>,
}

#[derive(Debug, Clone)]
struct MessageIdentityRow {
    msg_id: String,
    owner_identity_id: String,
    owner_did: String,
    old_conversation_id: String,
    old_thread_id: String,
    sender_did: String,
    receiver_did: String,
    group_id: String,
    group_did: String,
}

pub(super) fn migrate_shadow(path: &Path) -> crate::ImResult<CanonicalUpgradeReport> {
    let mut connection = Connection::open(path)
        .map_err(|_| upgrade_failed("shadow_migration", "shadow_open_failed"))?;
    let source_version = super::super::schema::current_schema_version(&connection)?;
    if source_version != RELEASE_0710_SCHEMA_VERSION {
        return Err(upgrade_failed(
            "shadow_migration",
            "shadow_source_version_changed",
        ));
    }
    let source_snapshot = super::validate::SourceConservationSnapshot::capture(&connection)?;
    let transaction = connection
        .transaction()
        .map_err(|_| upgrade_failed("shadow_migration", "transaction_start_failed"))?;
    super::super::schema::create_schema(&transaction, false)?;

    let mut report = CanonicalUpgradeReport {
        source_schema_version: source_version,
        target_schema_version: super::super::schema::SCHEMA_VERSION,
        ..CanonicalUpgradeReport::default()
    };
    let direct_routes = migrate_verified_routes(&transaction)?;
    report.migrated_personas =
        u64::try_from(direct_routes.values().collect::<BTreeSet<&String>>().len())
            .unwrap_or(u64::MAX);
    let group_routes = migrate_groups(&transaction)?;
    let migrated = migrate_messages(&transaction, &direct_routes, &group_routes)?;
    report.migrated_conversations = migrated.migrated_conversations;
    report.unresolved_messages = migrated.unresolved_messages;
    migrate_read_state_aliases(&transaction)?;
    super::super::conversation_summaries::rebuild_all(&transaction)?;
    report.alias_count = transaction
        .query_row("SELECT COUNT(*) FROM conversation_aliases", [], |row| {
            row.get::<_, i64>(0)
        })
        .map_err(super::super::local_state_unavailable)
        .map(|value| u64::try_from(value).unwrap_or_default())?;

    super::validate::validate_migrated_shadow(&transaction, &source_snapshot)?;
    super::super::schema::set_schema_version(&transaction, super::super::schema::SCHEMA_VERSION)?;
    transaction
        .commit()
        .map_err(|_| upgrade_failed("shadow_migration", "transaction_commit_failed"))?;
    Ok(report)
}

fn migrate_verified_routes(
    connection: &Transaction<'_>,
) -> crate::ImResult<BTreeMap<(String, String), String>> {
    let rows = route_rows(connection)?;
    let mut did_routes = BTreeMap::new();
    for row in rows {
        let Some(authority) = handle_authority(&row.full_handle) else {
            continue;
        };
        if crate::ids::Did::parse(&row.current_did).is_err() || row.owner_did.trim().is_empty() {
            continue;
        }
        let persona = match crate::internal::canonical_identity::PeerPersona::from_verified_handle(
            authority,
            &row.peer_user_id,
            &row.full_handle,
            Some("verified"),
        ) {
            Ok(persona) => persona,
            Err(_) => continue,
        };
        let verified_at = non_empty_or(&row.updated_at, "release_0710_migration");
        let did_bindings = handle_did_bindings(connection, &row)?;
        let current_generation = did_bindings
            .iter()
            .find(|binding| binding.did == row.current_did)
            .and_then(|binding| binding.binding_generation.clone());
        super::super::peer_personas::upsert(
            connection,
            &super::super::peer_personas::PeerPersonaRecord {
                owner_identity_id: row.owner_identity_id.clone(),
                persona: persona.clone(),
                binding_generation: current_generation.clone(),
                subject_type: "human".to_owned(),
                source: "release_0710_verified_route".to_owned(),
                authority_revision: None,
                verified_at: verified_at.to_owned(),
            },
        )?;
        super::super::peer_identifiers::bind(
            connection,
            &super::super::peer_identifiers::PeerIdentifierRecord {
                owner_identity_id: row.owner_identity_id.clone(),
                peer_persona_id: persona.peer_persona_id.clone(),
                identifier_kind: "handle".to_owned(),
                identifier_value: persona.full_handle.clone(),
                is_current: true,
                binding_generation: current_generation,
                source: "release_0710_verified_route".to_owned(),
                verified_at: verified_at.to_owned(),
            },
        )?;
        for binding in &did_bindings {
            super::super::peer_identifiers::bind(
                connection,
                &super::super::peer_identifiers::PeerIdentifierRecord {
                    owner_identity_id: row.owner_identity_id.clone(),
                    peer_persona_id: persona.peer_persona_id.clone(),
                    identifier_kind: "did".to_owned(),
                    identifier_value: binding.did.clone(),
                    is_current: binding.is_current,
                    binding_generation: binding.binding_generation.clone(),
                    source: "release_0710_verified_handle_binding".to_owned(),
                    verified_at: verified_at.to_owned(),
                },
            )?;
        }
        let canonical_id = persona.direct_conversation_id();
        connection
            .execute(
                r#"UPDATE direct_peer_routes
SET conversation_id = ?1, peer_persona_id = ?2, authority_namespace = ?3
WHERE owner_identity_id = ?4 AND conversation_id = ?5"#,
                rusqlite::params![
                    canonical_id,
                    persona.peer_persona_id,
                    persona.authority_namespace,
                    row.owner_identity_id,
                    row.old_conversation_id,
                ],
            )
            .map_err(super::super::local_state_unavailable)?;
        super::super::conversation_registry::ensure(
            connection,
            &super::super::conversation_registry::ConversationRegistryRecord {
                owner_identity_id: row.owner_identity_id.clone(),
                owner_did: row.owner_did.clone(),
                conversation_id: canonical_id.clone(),
                thread_kind: "direct".to_owned(),
                thread_id: canonical_id.clone(),
                activity_at: verified_at.to_owned(),
            },
        )?;
        connection
            .execute(
                r#"UPDATE contacts SET peer_persona_id = ?1
WHERE owner_identity_id = ?2
  AND (did = ?3 OR LOWER(TRIM(COALESCE(handle, ''))) = ?4)"#,
                (
                    persona.peer_persona_id.as_str(),
                    row.owner_identity_id.as_str(),
                    row.current_did.as_str(),
                    persona.full_handle.as_str(),
                ),
            )
            .map_err(super::super::local_state_unavailable)?;
        for binding in &did_bindings {
            insert_alias(
                connection,
                &row.owner_identity_id,
                "verified_did",
                &super::super::owner_scope::direct_conversation_id(&binding.did),
                &canonical_id,
            )?;
            did_routes.insert(
                (row.owner_identity_id.clone(), binding.did.clone()),
                canonical_id.clone(),
            );
        }
        if row.old_conversation_id != canonical_id {
            insert_alias(
                connection,
                &row.owner_identity_id,
                "release_0710_route",
                &row.old_conversation_id,
                &canonical_id,
            )?;
            mark_legacy_merged_if_present(
                connection,
                &row.owner_identity_id,
                &row.old_conversation_id,
                &canonical_id,
            )?;
        }
    }
    Ok(did_routes)
}

fn handle_did_bindings(
    connection: &Transaction<'_>,
    route: &RouteRow,
) -> crate::ImResult<Vec<HandleDidBinding>> {
    let mut statement = connection
        .prepare(
            r#"SELECT did, is_current, COALESCE(metadata, '')
FROM contact_handle_bindings
WHERE owner_identity_id = ?1 AND LOWER(TRIM(handle)) = LOWER(TRIM(?2))
ORDER BY is_current DESC, last_seen_at DESC, did"#,
        )
        .map_err(super::super::local_state_unavailable)?;
    let rows = statement
        .query_map(
            (route.owner_identity_id.as_str(), route.full_handle.as_str()),
            |row| {
                let metadata = row.get::<_, String>(2)?;
                Ok(HandleDidBinding {
                    did: row.get(0)?,
                    is_current: row.get::<_, i64>(1)? != 0,
                    binding_generation: binding_generation(&metadata),
                })
            },
        )
        .map_err(super::super::local_state_unavailable)?;
    let mut bindings = BTreeMap::<String, HandleDidBinding>::new();
    for binding in collect_rows(rows)? {
        if crate::ids::Did::parse(&binding.did).is_ok() {
            bindings.insert(binding.did.clone(), binding);
        }
    }
    bindings
        .entry(route.current_did.clone())
        .and_modify(|binding| binding.is_current = true)
        .or_insert_with(|| HandleDidBinding {
            did: route.current_did.clone(),
            is_current: true,
            binding_generation: None,
        });
    Ok(bindings.into_values().collect())
}

fn binding_generation(metadata: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(metadata)
        .ok()
        .and_then(|value| value.get("binding_generation").cloned())
        .and_then(|value| match value {
            serde_json::Value::String(value) => non_empty(&value).map(ToOwned::to_owned),
            serde_json::Value::Number(value) => Some(value.to_string()),
            _ => None,
        })
}

fn migrate_groups(
    connection: &Transaction<'_>,
) -> crate::ImResult<BTreeMap<(String, String), String>> {
    let rows = group_rows(connection)?;
    let mut routes = BTreeMap::new();
    for row in rows {
        if crate::ids::Did::parse(&row.group_did).is_err() || row.owner_did.trim().is_empty() {
            continue;
        }
        let canonical_id = super::super::owner_scope::group_conversation_id(&row.group_did);
        super::super::conversation_registry::ensure(
            connection,
            &super::super::conversation_registry::ConversationRegistryRecord {
                owner_identity_id: row.owner_identity_id.clone(),
                owner_did: row.owner_did.clone(),
                conversation_id: canonical_id.clone(),
                thread_kind: "group".to_owned(),
                thread_id: row.group_did.clone(),
                activity_at: row.activity_at.clone(),
            },
        )?;
        if inactive_group_membership(&row.membership_status) {
            connection
                .execute(
                    r#"UPDATE conversation_registry
SET is_active = 0, lifecycle_state = 'left', resolution_state = 'resolved'
WHERE owner_identity_id = ?1 AND conversation_id = ?2"#,
                    (row.owner_identity_id.as_str(), canonical_id.as_str()),
                )
                .map_err(super::super::local_state_unavailable)?;
        }
        let legacy_group_id = super::super::owner_scope::group_conversation_id(&row.group_id);
        if legacy_group_id != canonical_id {
            insert_alias(
                connection,
                &row.owner_identity_id,
                "release_0710_group_id",
                &legacy_group_id,
                &canonical_id,
            )?;
            mark_legacy_merged_if_present(
                connection,
                &row.owner_identity_id,
                &legacy_group_id,
                &canonical_id,
            )?;
        }
        routes.insert(
            (row.owner_identity_id.clone(), row.group_id.clone()),
            canonical_id.clone(),
        );
        routes.insert(
            (row.owner_identity_id.clone(), row.group_did.clone()),
            canonical_id,
        );
    }
    Ok(routes)
}

#[derive(Debug, Default)]
struct MessageMigrationOutcome {
    migrated_conversations: u64,
    unresolved_messages: u64,
}

fn migrate_messages(
    connection: &Transaction<'_>,
    direct_routes: &BTreeMap<(String, String), String>,
    group_routes: &BTreeMap<(String, String), String>,
) -> crate::ImResult<MessageMigrationOutcome> {
    let rows = message_identity_rows(connection)?;
    let mut migrated_conversations = BTreeSet::new();
    let mut unresolved_messages = 0u64;
    for row in rows {
        let mut wire_kind = String::new();
        let mut wire_ref = String::new();
        let mut resolution_state = "legacy_unresolved";
        let mut canonical_id = None;
        if let Some(group_ref) = non_empty(&row.group_did).or_else(|| non_empty(&row.group_id)) {
            wire_kind = "group".to_owned();
            wire_ref = non_empty(&row.group_did)
                .map(ToOwned::to_owned)
                .or_else(|| {
                    group_routes
                        .get(&(row.owner_identity_id.clone(), group_ref.to_owned()))
                        .map(|mapped| mapped.trim_start_matches("group:").to_owned())
                })
                .unwrap_or_else(|| group_ref.to_owned());
            resolution_state = "resolved";
            if let Some(mapped) =
                group_routes.get(&(row.owner_identity_id.clone(), group_ref.to_owned()))
            {
                canonical_id = Some(mapped.clone());
            }
        } else if let Some(peer_did) = direct_peer_did(&row) {
            wire_kind = "direct".to_owned();
            wire_ref = peer_did.to_owned();
            resolution_state = "resolved";
            if let Some(mapped) =
                direct_routes.get(&(row.owner_identity_id.clone(), peer_did.to_owned()))
            {
                canonical_id = Some(mapped.clone());
            }
        } else if row.old_thread_id.trim().starts_with("mail:") {
            wire_kind = "mail".to_owned();
            wire_ref = row.old_thread_id.trim().to_owned();
            resolution_state = "resolved";
            canonical_id = Some(row.old_conversation_id.clone());
        }

        connection
            .execute(
                r#"UPDATE messages
SET wire_thread_kind = ?1, wire_thread_ref = ?2,
    wire_identity_resolution_state = ?3,
    conversation_id = COALESCE(?4, conversation_id),
    thread_id = COALESCE(?4, thread_id)
WHERE owner_identity_id = ?5 AND msg_id = ?6"#,
                rusqlite::params![
                    wire_kind,
                    wire_ref,
                    resolution_state,
                    canonical_id.as_deref(),
                    row.owner_identity_id,
                    row.msg_id,
                ],
            )
            .map_err(super::super::local_state_unavailable)?;
        if let Some(canonical_id) = canonical_id {
            if row.old_conversation_id != canonical_id && !row.old_conversation_id.trim().is_empty()
            {
                insert_alias(
                    connection,
                    &row.owner_identity_id,
                    if wire_kind == "group" {
                        "release_0710_group_thread"
                    } else {
                        "release_0710_direct_thread"
                    },
                    &row.old_conversation_id,
                    &canonical_id,
                )?;
                mark_legacy_merged_if_present(
                    connection,
                    &row.owner_identity_id,
                    &row.old_conversation_id,
                    &canonical_id,
                )?;
            }
            migrated_conversations.insert((row.owner_identity_id, canonical_id));
        } else {
            unresolved_messages = unresolved_messages.saturating_add(1);
            ensure_legacy_registry(connection, &row)?;
        }
    }
    Ok(MessageMigrationOutcome {
        migrated_conversations: u64::try_from(migrated_conversations.len()).unwrap_or(u64::MAX),
        unresolved_messages,
    })
}

fn migrate_read_state_aliases(connection: &Transaction<'_>) -> crate::ImResult<()> {
    connection
        .execute(
            r#"UPDATE thread_read_state
SET conversation_id = (
    SELECT alias.canonical_conversation_id
    FROM conversation_aliases alias
    WHERE alias.owner_identity_id = thread_read_state.owner_identity_id
      AND alias.alias_conversation_id = thread_read_state.conversation_id
    ORDER BY alias.alias_kind LIMIT 1
)
WHERE EXISTS (
    SELECT 1 FROM conversation_aliases alias
    WHERE alias.owner_identity_id = thread_read_state.owner_identity_id
      AND alias.alias_conversation_id = thread_read_state.conversation_id
)"#,
            [],
        )
        .map_err(super::super::local_state_unavailable)?;
    Ok(())
}

fn ensure_legacy_registry(
    connection: &Transaction<'_>,
    row: &MessageIdentityRow,
) -> crate::ImResult<()> {
    let conversation_id = non_empty(&row.old_conversation_id)
        .or_else(|| non_empty(&row.old_thread_id))
        .ok_or_else(|| upgrade_failed("shadow_migration", "message_identity_missing"))?;
    connection
        .execute(
            r#"INSERT OR IGNORE INTO conversation_registry
(owner_identity_id, owner_did, conversation_id, thread_kind, thread_id,
 activity_at, created_at, updated_at, is_active, lifecycle_state, resolution_state)
VALUES (?1, ?2, ?3, 'thread', ?4, '0', '0', '0', 1, 'active', 'legacy_unresolved')"#,
            (
                row.owner_identity_id.as_str(),
                row.owner_did.as_str(),
                conversation_id,
                row.old_thread_id.as_str(),
            ),
        )
        .map_err(super::super::local_state_unavailable)?;
    Ok(())
}

fn mark_legacy_merged_if_present(
    connection: &Transaction<'_>,
    owner_identity_id: &str,
    old_conversation_id: &str,
    canonical_id: &str,
) -> crate::ImResult<()> {
    if old_conversation_id == canonical_id {
        return Ok(());
    }
    connection
        .execute(
            r#"UPDATE conversation_registry
SET is_active = 0, lifecycle_state = 'merged', resolution_state = 'resolved',
    merged_into_conversation_id = ?1
WHERE owner_identity_id = ?2 AND conversation_id = ?3"#,
            (canonical_id, owner_identity_id, old_conversation_id),
        )
        .map_err(super::super::local_state_unavailable)?;
    Ok(())
}

fn insert_alias(
    connection: &Transaction<'_>,
    owner_identity_id: &str,
    kind: &str,
    alias: &str,
    canonical_id: &str,
) -> crate::ImResult<()> {
    if alias.trim().is_empty() || alias == canonical_id {
        return Ok(());
    }
    super::super::conversation_aliases::insert(
        connection,
        &super::super::conversation_aliases::ConversationAliasRecord {
            owner_identity_id: owner_identity_id.to_owned(),
            alias_kind: kind.to_owned(),
            alias_conversation_id: alias.to_owned(),
            canonical_conversation_id: canonical_id.to_owned(),
            source: "release_0710_migration".to_owned(),
            verified_at: "release_0710_migration".to_owned(),
        },
    )
}

fn route_rows(connection: &Transaction<'_>) -> crate::ImResult<Vec<RouteRow>> {
    let mut statement = connection
        .prepare(
            r#"SELECT route.owner_identity_id,
       COALESCE(NULLIF(registry.owner_did, ''), history.did, ''),
       route.conversation_id, route.peer_user_id, route.full_handle,
       route.current_did, route.updated_at
FROM direct_peer_routes route
LEFT JOIN conversation_registry registry
  ON registry.owner_identity_id = route.owner_identity_id
 AND registry.conversation_id = route.conversation_id
LEFT JOIN identity_did_history history
  ON history.owner_identity_id = route.owner_identity_id AND history.status = 'current'
ORDER BY route.owner_identity_id, route.conversation_id"#,
        )
        .map_err(super::super::local_state_unavailable)?;
    let rows = statement
        .query_map([], |row| {
            Ok(RouteRow {
                owner_identity_id: row.get(0)?,
                owner_did: row.get(1)?,
                old_conversation_id: row.get(2)?,
                peer_user_id: row.get(3)?,
                full_handle: row.get(4)?,
                current_did: row.get(5)?,
                updated_at: row.get(6)?,
            })
        })
        .map_err(super::super::local_state_unavailable)?;
    collect_rows(rows)
}

fn group_rows(connection: &Transaction<'_>) -> crate::ImResult<Vec<GroupRow>> {
    let mut statement = connection
        .prepare(
            r#"SELECT groups.owner_identity_id, groups.owner_did, groups.group_id,
       COALESCE(groups.group_did, ''), COALESCE(groups.membership_status, 'active'),
       COALESCE(NULLIF(groups.last_message_at, ''), NULLIF(groups.remote_updated_at, ''), groups.stored_at)
FROM groups
WHERE TRIM(COALESCE(groups.group_did, '')) <> ''
ORDER BY groups.owner_identity_id, groups.group_id"#,
        )
        .map_err(super::super::local_state_unavailable)?;
    let rows = statement
        .query_map([], |row| {
            Ok(GroupRow {
                owner_identity_id: row.get(0)?,
                owner_did: row.get(1)?,
                group_id: row.get(2)?,
                group_did: row.get(3)?,
                membership_status: row.get(4)?,
                activity_at: row.get(5)?,
            })
        })
        .map_err(super::super::local_state_unavailable)?;
    collect_rows(rows)
}

fn inactive_group_membership(status: &str) -> bool {
    matches!(
        status.trim().to_ascii_lowercase().as_str(),
        "left" | "removed" | "inactive" | "non_member"
    )
}

fn message_identity_rows(connection: &Transaction<'_>) -> crate::ImResult<Vec<MessageIdentityRow>> {
    let mut statement = connection
        .prepare(
            r#"SELECT msg_id, owner_identity_id, owner_did,
       COALESCE(conversation_id, ''), thread_id,
       COALESCE(sender_did, ''), COALESCE(receiver_did, ''),
       COALESCE(group_id, ''), COALESCE(group_did, '')
FROM messages ORDER BY owner_identity_id, msg_id"#,
        )
        .map_err(super::super::local_state_unavailable)?;
    let rows = statement
        .query_map([], |row| {
            Ok(MessageIdentityRow {
                msg_id: row.get(0)?,
                owner_identity_id: row.get(1)?,
                owner_did: row.get(2)?,
                old_conversation_id: row.get(3)?,
                old_thread_id: row.get(4)?,
                sender_did: row.get(5)?,
                receiver_did: row.get(6)?,
                group_id: row.get(7)?,
                group_did: row.get(8)?,
            })
        })
        .map_err(super::super::local_state_unavailable)?;
    collect_rows(rows)
}

fn direct_peer_did(row: &MessageIdentityRow) -> Option<&str> {
    let owner = row.owner_did.trim();
    let sender = row.sender_did.trim();
    let receiver = row.receiver_did.trim();
    let peer = if !sender.is_empty() && sender != owner {
        sender
    } else {
        receiver
    };
    crate::ids::Did::parse(peer).ok().map(|_| peer)
}

fn handle_authority(full_handle: &str) -> Option<&str> {
    full_handle
        .trim()
        .trim_start_matches('@')
        .split_once('.')
        .map(|(_, domain)| domain)
}

fn non_empty(value: &str) -> Option<&str> {
    (!value.trim().is_empty()).then(|| value.trim())
}

fn non_empty_or<'a>(value: &'a str, fallback: &'a str) -> &'a str {
    non_empty(value).unwrap_or(fallback)
}

fn collect_rows<T>(
    rows: rusqlite::MappedRows<'_, impl FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<T>>,
) -> crate::ImResult<Vec<T>> {
    let mut values = Vec::new();
    for row in rows {
        values.push(row.map_err(super::super::local_state_unavailable)?);
    }
    Ok(values)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrates_release_0710_projection_without_losing_wire_or_empty_conversations() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("release-0710.sqlite");
        super::super::source::copy_release_0710_fixture(&source);
        let report = migrate_shadow(&source).unwrap();
        assert_eq!(report.source_schema_version, 27);
        assert_eq!(
            report.target_schema_version,
            super::super::super::schema::SCHEMA_VERSION
        );
        assert_eq!(report.migrated_personas, 1);
        assert_eq!(report.unresolved_messages, 0);

        let db = Connection::open(&source).unwrap();
        assert_eq!(
            super::super::super::schema::current_schema_version(&db).unwrap(),
            super::super::super::schema::SCHEMA_VERSION
        );
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM messages", [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            2
        );
        let direct: (String, String, String) = db
            .query_row(
                r#"SELECT conversation_id, wire_thread_kind, wire_thread_ref
FROM messages WHERE msg_id = 'fixture-direct-message-1'"#,
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert!(direct.0.starts_with("dm:peer-scope:v1:"));
        assert_eq!(direct.1, "direct");
        assert_eq!(
            direct.2,
            "did:wba:awiki.info:fixture-peer:e1_fixture_peer_old"
        );
        assert_eq!(
            db.query_row(
                r#"SELECT COUNT(*) FROM conversation_registry
WHERE canonical_group_did = 'did:wba:awiki.info:groups:fixture-empty:e1_fixture_empty'
  AND lifecycle_state = 'active' AND resolution_state = 'resolved'"#,
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
            1
        );
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM e2ee_outbox", [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM group_members", [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            2
        );
        let membership_ids: Vec<String> = {
            let mut statement = db
                .prepare("SELECT membership_id FROM group_members ORDER BY user_id")
                .unwrap();
            statement
                .query_map([], |row| row.get(0))
                .unwrap()
                .collect::<rusqlite::Result<Vec<_>>>()
                .unwrap()
        };
        assert_eq!(membership_ids.len(), 2);
        assert!(membership_ids
            .iter()
            .all(|membership_id| membership_id.starts_with("membership:v1:")));
        assert_ne!(membership_ids[0], membership_ids[1]);
        let identifiers: Vec<(String, String, i64, String)> = {
            let mut statement = db
                .prepare(
                    r#"SELECT peer_persona_id, identifier_value, is_current,
       COALESCE(binding_generation, '')
FROM peer_identifiers
WHERE identifier_kind = 'did'
ORDER BY is_current, identifier_value"#,
                )
                .unwrap();
            statement
                .query_map([], |row| {
                    Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
                })
                .unwrap()
                .collect::<rusqlite::Result<Vec<_>>>()
                .unwrap()
        };
        assert_eq!(identifiers.len(), 2);
        assert_eq!(identifiers[0].0, identifiers[1].0);
        assert_eq!(identifiers[0].1, direct.2);
        assert_eq!(identifiers[0].2, 0);
        assert_eq!(identifiers[0].3, "1");
        assert_eq!(identifiers[1].2, 1);
        assert_eq!(identifiers[1].3, "2");
        assert_eq!(
            db.query_row(
                r#"SELECT message_count, unread_count, unread_mention_count
FROM conversation_summaries WHERE conversation_id = ?1"#,
                [direct.0.as_str()],
                |row| Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?
                )),
            )
            .unwrap(),
            (1, 1, 0)
        );
        assert_eq!(
            db.query_row(
                r#"SELECT message_count, unread_count, unread_mention_count
FROM conversation_summaries
WHERE conversation_id = 'group:did:wba:awiki.info:groups:fixture-group:e1_fixture_group'"#,
                [],
                |row| Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?
                )),
            )
            .unwrap(),
            (1, 0, 0)
        );
        assert_eq!(
            db.query_row(
                r#"SELECT COUNT(*) FROM conversation_registry
WHERE peer_persona_id = ?1 AND thread_kind = 'direct'
  AND lifecycle_state = 'active' AND resolution_state = 'resolved'"#,
                [identifiers[0].0.as_str()],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
            1
        );
        assert_eq!(
            db.query_row(
                r#"SELECT COUNT(*) FROM conversation_registry
WHERE lifecycle_state = 'active' AND resolution_state = 'resolved'"#,
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
            3
        );
        assert_eq!(
            db.query_row(
                r#"SELECT COUNT(*) FROM conversation_registry
WHERE conversation_id IN ('group:fixture-group-local', 'group:fixture-empty-group-local')
  AND is_active = 0 AND lifecycle_state = 'merged'
  AND resolution_state = 'resolved'
  AND TRIM(COALESCE(merged_into_conversation_id, '')) <> ''"#,
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
            2
        );
    }

    #[test]
    fn empty_direct_route_merge_leaves_one_active_canonical_conversation() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("release-0710-empty-direct.sqlite");
        super::super::source::copy_release_0710_fixture(&source);
        let db = Connection::open(&source).unwrap();
        let legacy_id = "dm:did:wba:awiki.info:fixture-peer:e1_fixture_peer";
        db.execute(
            r#"UPDATE direct_peer_routes SET conversation_id = ?1"#,
            [legacy_id],
        )
        .unwrap();
        db.execute(
            r#"INSERT INTO conversation_registry
(owner_identity_id, owner_did, conversation_id, thread_kind, thread_id,
 activity_at, created_at, updated_at, is_active)
VALUES ('fixture-owner-id', 'did:wba:awiki.info:fixture-owner:e1_fixture_owner',
        ?1, 'direct', ?1, '2026-07-10T00:00:00Z',
        '2026-07-10T00:00:00Z', '2026-07-10T00:00:00Z', 1)"#,
            [legacy_id],
        )
        .unwrap();
        drop(db);

        migrate_shadow(&source).unwrap();

        let db = Connection::open(&source).unwrap();
        let canonical_id: String = db
            .query_row(
                "SELECT conversation_id FROM direct_peer_routes WHERE owner_identity_id = 'fixture-owner-id'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(canonical_id.starts_with("dm:peer-scope:v1:"));
        assert_eq!(
            db.query_row(
                r#"SELECT COUNT(*) FROM conversation_registry
WHERE owner_identity_id = 'fixture-owner-id' AND thread_kind = 'direct'
  AND lifecycle_state = 'active' AND resolution_state = 'resolved'
  AND peer_persona_id IS NOT NULL"#,
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
            1
        );
        assert_eq!(
            db.query_row(
                r#"SELECT COUNT(*) FROM conversation_registry
WHERE owner_identity_id = 'fixture-owner-id' AND conversation_id = ?1
  AND is_active = 0 AND lifecycle_state = 'merged'
  AND resolution_state = 'resolved' AND merged_into_conversation_id = ?2"#,
                (legacy_id, canonical_id.as_str()),
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
            1
        );
    }

    #[test]
    fn left_group_migrates_to_resolved_non_active_canonical_lifecycle() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("release-0710-left-group.sqlite");
        super::super::source::copy_release_0710_fixture(&source);
        let db = Connection::open(&source).unwrap();
        db.execute(
            "UPDATE groups SET membership_status = 'left' WHERE group_id = 'fixture-empty-group-local'",
            [],
        )
        .unwrap();
        drop(db);

        migrate_shadow(&source).unwrap();

        let db = Connection::open(&source).unwrap();
        let canonical_id = "group:did:wba:awiki.info:groups:fixture-empty:e1_fixture_empty";
        assert_eq!(
            db.query_row(
                r#"SELECT COUNT(*) FROM conversation_registry
WHERE owner_identity_id = 'fixture-owner-id' AND conversation_id = ?1
  AND is_active = 0 AND lifecycle_state = 'left'
  AND resolution_state = 'resolved'"#,
                [canonical_id],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
            1
        );
        assert_eq!(
            db.query_row(
                r#"SELECT COUNT(*) FROM conversation_registry
WHERE owner_identity_id = 'fixture-owner-id'
  AND canonical_group_did = 'did:wba:awiki.info:groups:fixture-empty:e1_fixture_empty'
  AND lifecycle_state = 'active'"#,
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
            0
        );
        assert_eq!(
            super::super::super::conversation_aliases::resolve(
                &db,
                "fixture-owner-id",
                "release_0710_group_id",
                "group:fixture-empty-group-local",
            )
            .unwrap()
            .as_deref(),
            Some(canonical_id)
        );
    }

    #[test]
    fn did_fallback_route_stays_canonical_unresolved_without_losing_wire_identity() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("release-0710-did-fallback.sqlite");
        super::super::source::copy_release_0710_fixture(&source);
        let db = Connection::open(&source).unwrap();
        db.execute(
            "UPDATE direct_peer_routes SET peer_user_id = current_did",
            [],
        )
        .unwrap();
        drop(db);

        let report = migrate_shadow(&source).unwrap();
        assert_eq!(report.migrated_personas, 0);
        assert_eq!(report.unresolved_messages, 1);

        let db = Connection::open(&source).unwrap();
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM peer_personas", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
            0
        );
        let message: (String, String, String, String) = db
            .query_row(
                r#"SELECT conversation_id, wire_thread_kind, wire_thread_ref,
                          wire_identity_resolution_state
FROM messages WHERE msg_id = 'fixture-direct-message-1'"#,
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(
            message.0,
            "dm:did:wba:awiki.info:fixture-peer:e1_fixture_peer_old"
        );
        assert_eq!(message.1, "direct");
        assert_eq!(
            message.2,
            "did:wba:awiki.info:fixture-peer:e1_fixture_peer_old"
        );
        assert_eq!(message.3, "resolved");
        assert_eq!(
            db.query_row(
                r#"SELECT COUNT(*) FROM conversation_registry
WHERE conversation_id = ?1 AND lifecycle_state = 'active'
  AND resolution_state = 'legacy_unresolved'"#,
                [message.0.as_str()],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
            1
        );
    }
}
