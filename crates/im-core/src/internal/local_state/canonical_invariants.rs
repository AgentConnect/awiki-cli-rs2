//! Redacted diagnostics for canonical conversation projection invariants.

use rusqlite::Connection;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CanonicalInvariantViolation {
    pub(crate) table: &'static str,
    pub(crate) invariant: &'static str,
    pub(crate) row_count: i64,
}

pub(crate) fn check(
    connection: &Connection,
    owner_identity_id: &str,
) -> crate::ImResult<Vec<CanonicalInvariantViolation>> {
    let owner_identity_id = owner_identity_id.trim();
    if owner_identity_id.is_empty() {
        return Err(crate::ImError::invalid_input(
            Some("owner_identity_id".to_owned()),
            "owner_identity_id is required",
        ));
    }
    let checks = [
        (
            "conversation_registry",
            "one_active_direct_per_persona",
            r#"SELECT COUNT(*) FROM (
SELECT peer_persona_id FROM conversation_registry
WHERE owner_identity_id = ?1 AND thread_kind = 'direct'
  AND lifecycle_state = 'active' AND resolution_state = 'resolved'
GROUP BY peer_persona_id HAVING COUNT(*) > 1)"#,
        ),
        (
            "conversation_registry",
            "one_active_group_per_canonical_did",
            r#"SELECT COUNT(*) FROM (
SELECT canonical_group_did FROM conversation_registry
WHERE owner_identity_id = ?1 AND thread_kind = 'group'
  AND lifecycle_state = 'active' AND resolution_state = 'resolved'
GROUP BY canonical_group_did HAVING COUNT(*) > 1)"#,
        ),
        (
            "conversation_registry",
            "resolved_direct_requires_persona",
            r#"SELECT COUNT(*) FROM conversation_registry
WHERE owner_identity_id = ?1 AND thread_kind = 'direct'
  AND lifecycle_state = 'active' AND resolution_state = 'resolved'
  AND (peer_persona_id IS NULL OR TRIM(peer_persona_id) = '')"#,
        ),
        (
            "conversation_registry",
            "resolved_group_requires_canonical_did",
            r#"SELECT COUNT(*) FROM conversation_registry
WHERE owner_identity_id = ?1 AND thread_kind = 'group'
  AND lifecycle_state = 'active' AND resolution_state = 'resolved'
  AND (canonical_group_did IS NULL OR TRIM(canonical_group_did) = '')"#,
        ),
        (
            "conversation_registry",
            "merged_row_requires_target",
            r#"SELECT COUNT(*) FROM conversation_registry
WHERE owner_identity_id = ?1 AND lifecycle_state = 'merged'
  AND (merged_into_conversation_id IS NULL OR TRIM(merged_into_conversation_id) = '')"#,
        ),
        (
            "conversation_registry",
            "merged_target_must_exist",
            r#"SELECT COUNT(*) FROM conversation_registry source
LEFT JOIN conversation_registry target
  ON target.owner_identity_id = source.owner_identity_id
 AND target.conversation_id = source.merged_into_conversation_id
WHERE source.owner_identity_id = ?1 AND source.lifecycle_state = 'merged'
  AND TRIM(COALESCE(source.merged_into_conversation_id, '')) <> ''
  AND target.conversation_id IS NULL"#,
        ),
        (
            "conversation_aliases",
            "alias_target_must_be_active_resolved",
            r#"SELECT COUNT(*) FROM conversation_aliases alias
LEFT JOIN conversation_registry target
  ON target.owner_identity_id = alias.owner_identity_id
 AND target.conversation_id = alias.canonical_conversation_id
WHERE alias.owner_identity_id = ?1
  AND (target.conversation_id IS NULL OR target.lifecycle_state <> 'active'
       OR target.resolution_state <> 'resolved')"#,
        ),
        (
            "conversation_aliases",
            "alias_target_must_not_be_alias",
            r#"SELECT COUNT(*) FROM conversation_aliases source
JOIN conversation_aliases target
  ON target.owner_identity_id = source.owner_identity_id
 AND target.alias_conversation_id = source.canonical_conversation_id
WHERE source.owner_identity_id = ?1"#,
        ),
        (
            "direct_peer_routes",
            "route_persona_must_exist",
            r#"SELECT COUNT(*) FROM direct_peer_routes route
LEFT JOIN peer_personas persona
  ON persona.owner_identity_id = route.owner_identity_id
 AND persona.peer_persona_id = route.peer_persona_id
WHERE route.owner_identity_id = ?1 AND route.peer_persona_id IS NOT NULL
  AND persona.peer_persona_id IS NULL"#,
        ),
        (
            "direct_peer_routes",
            "route_registry_must_match_persona",
            r#"SELECT COUNT(*) FROM direct_peer_routes route
LEFT JOIN conversation_registry registry
  ON registry.owner_identity_id = route.owner_identity_id
 AND registry.conversation_id = route.conversation_id
 AND registry.peer_persona_id = route.peer_persona_id
 AND registry.thread_kind = 'direct'
 AND registry.lifecycle_state = 'active'
 AND registry.resolution_state = 'resolved'
WHERE route.owner_identity_id = ?1 AND route.peer_persona_id IS NOT NULL
  AND registry.conversation_id IS NULL"#,
        ),
        (
            "peer_profiles",
            "profile_persona_must_exist",
            r#"SELECT COUNT(*) FROM peer_profiles profile
LEFT JOIN peer_personas persona
  ON persona.owner_identity_id = profile.owner_identity_id
 AND persona.peer_persona_id = profile.peer_persona_id
WHERE profile.owner_identity_id = ?1 AND persona.peer_persona_id IS NULL"#,
        ),
        (
            "messages",
            "message_canonical_registry_must_exist",
            r#"SELECT COUNT(*) FROM messages message
LEFT JOIN conversation_registry registry
  ON registry.owner_identity_id = message.owner_identity_id
 AND registry.conversation_id = message.conversation_id
WHERE message.owner_identity_id = ?1
  AND TRIM(COALESCE(message.conversation_id, '')) <> ''
  AND registry.conversation_id IS NULL"#,
        ),
    ];

    let mut violations = Vec::new();
    for (table, invariant, sql) in checks {
        let row_count = connection
            .query_row(sql, [owner_identity_id], |row| row.get::<_, i64>(0))
            .map_err(super::local_state_unavailable)?;
        if row_count > 0 {
            violations.push(CanonicalInvariantViolation {
                table,
                invariant,
                row_count,
            });
        }
    }
    let row_count = route_factory_mismatch_count(connection, owner_identity_id)?;
    if row_count > 0 {
        violations.push(CanonicalInvariantViolation {
            table: "direct_peer_routes",
            invariant: "route_must_match_persona_factory",
            row_count,
        });
    }
    Ok(violations)
}

fn route_factory_mismatch_count(
    connection: &Connection,
    owner_identity_id: &str,
) -> crate::ImResult<i64> {
    let mut statement = connection
        .prepare(
            r#"SELECT route.conversation_id, route.peer_persona_id,
       persona.authority_namespace, persona.authority_subject_id, persona.full_handle
FROM direct_peer_routes route
JOIN peer_personas persona
  ON persona.owner_identity_id = route.owner_identity_id
 AND persona.peer_persona_id = route.peer_persona_id
WHERE route.owner_identity_id = ?1"#,
        )
        .map_err(super::local_state_unavailable)?;
    let rows = statement
        .query_map([owner_identity_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
            ))
        })
        .map_err(super::local_state_unavailable)?;
    let mut mismatches = 0i64;
    for row in rows {
        let (conversation_id, stored_persona_id, authority, subject, handle) =
            row.map_err(super::local_state_unavailable)?;
        let valid = crate::internal::canonical_identity::PeerPersona::from_verified_handle(
            &authority,
            &subject,
            &handle,
            Some("verified"),
        )
        .map(|persona| {
            persona.peer_persona_id == stored_persona_id
                && persona.direct_conversation_id() == conversation_id
        })
        .unwrap_or(false);
        if !valid {
            mismatches = mismatches.saturating_add(1);
        }
    }
    Ok(mismatches)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_redacted_canonical_projection_violations() {
        let db = Connection::open_in_memory().unwrap();
        super::super::schema::ensure_schema(&db).unwrap();
        for (conversation_id, active, lifecycle, resolution) in [
            ("dm:broken", 1, "active", "resolved"),
            ("dm:merged", 0, "merged", "legacy_unresolved"),
        ] {
            db.execute(
                r#"INSERT INTO conversation_registry
(owner_identity_id, owner_did, conversation_id, thread_kind, thread_id,
 activity_at, created_at, updated_at, is_active, lifecycle_state, resolution_state)
VALUES ('owner', 'did:example:owner', ?1, 'direct', ?1,
        '1', '1', '1', ?2, ?3, ?4)"#,
                rusqlite::params![conversation_id, active, lifecycle, resolution],
            )
            .unwrap();
        }

        let violations = check(&db, "owner").unwrap();
        assert!(violations.iter().any(|item| {
            item.invariant == "resolved_direct_requires_persona" && item.row_count == 1
        }));
        assert!(violations
            .iter()
            .any(|item| item.invariant == "merged_row_requires_target" && item.row_count == 1));
    }

    #[test]
    fn verified_persona_projection_satisfies_canonical_invariants() {
        let mut db = Connection::open_in_memory().unwrap();
        super::super::schema::ensure_schema(&db).unwrap();
        super::super::peer_personas::project_verified_handle(
            &mut db,
            "owner",
            "did:example:owner",
            &crate::directory::HandleLookupResult {
                handle: crate::ids::Handle::parse("peer.awiki.info", "").unwrap(),
                did: crate::ids::Did::parse("did:example:peer").unwrap(),
                user_id: "peer-user".to_owned(),
                domain: Some("awiki.info".to_owned()),
                status: Some("active".to_owned()),
                binding_generation: Some("1".to_owned()),
                profile: None,
                warnings: Vec::new(),
            },
        )
        .unwrap();

        assert!(check(&db, "owner").unwrap().is_empty());
    }
}
