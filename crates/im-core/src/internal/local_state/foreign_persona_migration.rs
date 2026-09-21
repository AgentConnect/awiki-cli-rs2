//! Narrow repair of the former home-Directory projection for foreign Handles.
//! The caller has freshly verified public WNS. Local authorities never enter
//! this path; neither Handle equality nor a private subject alone proves continuity.
use rusqlite::{Connection, OptionalExtension};

pub(crate) struct LegacyPersona {
    persona_id: String,
    conversation_id: String,
}

fn conflict(detail: &str) -> crate::ImError {
    crate::ImError::IdentityBindingConflict {
        detail: detail.to_owned(),
    }
}

pub(crate) fn prepare(
    db: &Connection,
    owner: &str,
    lookup: &crate::directory::HandleLookupResult,
    home_domain: Option<&str>,
) -> crate::ImResult<Option<LegacyPersona>> {
    let next = lookup.peer_persona()?;
    let Some(home) = home_domain else {
        return Ok(None);
    };
    if next.authority_namespace
        == crate::internal::canonical_identity::normalize_authority_namespace(home)?
        || next.authority_subject_id != next.full_handle
    {
        return Ok(None);
    }
    let existing = db.query_row(
        "SELECT peer_persona_id FROM peer_identifiers WHERE owner_identity_id=?1 AND identifier_kind='handle' AND identifier_value=?2",
        (owner, &next.full_handle), |row| row.get::<_, String>(0),
    ).optional().map_err(super::local_state_unavailable)?;
    let Some(old_id) = existing.filter(|id| id != &next.peer_persona_id) else {
        return Ok(None);
    };
    let old = db.query_row(
        "SELECT authority_namespace, authority_subject_id, full_handle, source FROM peer_personas WHERE owner_identity_id=?1 AND peer_persona_id=?2",
        (owner, &old_id), |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?)),
    ).map_err(super::local_state_unavailable)?;
    let persona = crate::internal::canonical_identity::PeerPersona::from_verified_handle(
        &old.0,
        &old.1,
        &old.2,
        Some("active"),
    )?;
    if persona.peer_persona_id != old_id
        || old.0 != next.authority_namespace
        || old.2 != next.full_handle
        || old.1 == next.full_handle
        || old.3 != "handle_authority"
    {
        return Err(conflict(
            "foreign Handle cache does not match the known Directory projection",
        ));
    }
    let resolved = super::peer_personas::resolve_by_handle(db, owner, &next.full_handle)?
        .ok_or_else(|| conflict("foreign Handle cache has no unambiguous verified route"))?;
    if resolved.peer_persona_id != old_id
        || resolved.conversation_id != persona.direct_conversation_id()
    {
        return Err(conflict("foreign Handle cache has an inconsistent route"));
    }
    // No many-to-one merge: an independently established target needs its own
    // conflict review, even if its Handle string looks the same.
    let target_exists: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM peer_personas WHERE owner_identity_id=?1 AND peer_persona_id=?2)",
        (owner, &next.peer_persona_id), |r| r.get(0),
    ).map_err(super::local_state_unavailable)?;
    if target_exists {
        return Err(conflict(
            "foreign Handle repair has an independently established target Persona",
        ));
    }
    super::peer_personas::validate_projection_generation(
        db,
        owner,
        &old_id,
        lookup.did.as_str(),
        lookup.binding_generation.as_deref(),
    )?;
    let mut statement = db.prepare(
        "SELECT identifier_kind, identifier_value, source FROM peer_identifiers WHERE owner_identity_id=?1 AND peer_persona_id=?2",
    ).map_err(super::local_state_unavailable)?;
    let identifiers = statement
        .query_map((owner, &old_id), |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })
        .map_err(super::local_state_unavailable)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(super::local_state_unavailable)?;
    for (kind, value, source) in identifiers {
        if !(source == "handle_authority" || (kind == "did" && source == "verified_did_transition"))
            || (kind == "handle" && value != next.full_handle)
        {
            return Err(conflict(
                "foreign Handle cache contains an unrelated identifier",
            ));
        }
        if kind == "did" {
            let mut current = value;
            let mut visited = std::collections::BTreeSet::new();
            while current != lookup.did.as_str() {
                if !visited.insert(current.clone()) || visited.len() > 64 {
                    return Err(conflict(
                        "foreign Handle cache continuity chain is cyclic or too long",
                    ));
                }
                current = super::did_transition_edges::get_successor(db, owner, &current)?
                    .ok_or_else(|| {
                        conflict("foreign Handle cache has no proof-verified DID continuity")
                    })?;
            }
        } else if kind != "handle" {
            return Err(conflict(
                "foreign Handle cache contains an unsupported identifier",
            ));
        }
    }
    Ok(Some(LegacyPersona {
        persona_id: old_id,
        conversation_id: resolved.conversation_id,
    }))
}

/// Runs inside the projection transaction, after the new Persona is inserted.
/// Only local identity/index columns change. Wire IDs, ciphertext, ratchets,
/// signed outbox payloads and remote read-watermark domains remain untouched.
pub(crate) fn migrate(
    db: &Connection,
    owner: &str,
    owner_did: &str,
    lookup: &crate::directory::HandleLookupResult,
    old: &LegacyPersona,
    verified_at: &str,
) -> crate::ImResult<()> {
    let persona = lookup.peer_persona()?;
    let next_id = persona.direct_conversation_id();
    let route = super::direct_peer_routes::DirectPeerRouteRecord::from_verified_persona(
        owner,
        &persona,
        lookup.did.as_str(),
    )?;
    super::direct_peer_routes::upsert(db, &route)?;
    super::conversation_registry::ensure(
        db,
        &super::conversation_registry::ConversationRegistryRecord {
            owner_identity_id: owner.to_owned(),
            owner_did: owner_did.to_owned(),
            conversation_id: next_id.clone(),
            thread_kind: "direct".to_owned(),
            thread_id: next_id.clone(),
            activity_at: verified_at.to_owned(),
        },
    )?;
    for table in [
        "peer_identifiers",
        "peer_profiles",
        "contacts",
        "group_members",
    ] {
        db.execute(&format!("UPDATE {table} SET peer_persona_id=?1 WHERE owner_identity_id=?2 AND peer_persona_id=?3"),
            (&persona.peer_persona_id, owner, &old.persona_id)).map_err(super::local_state_unavailable)?;
    }
    // Alias insertion remains fail-closed everywhere else. This explicit,
    // proof-checked repair flattens the old target before publishing its alias.
    db.execute("UPDATE conversation_aliases SET canonical_conversation_id=?1 WHERE owner_identity_id=?2 AND canonical_conversation_id=?3",
        (&next_id, owner, &old.conversation_id)).map_err(super::local_state_unavailable)?;
    super::conversation_aliases::insert(
        db,
        &super::conversation_aliases::ConversationAliasRecord {
            owner_identity_id: owner.to_owned(),
            alias_kind: "verified_foreign_persona".to_owned(),
            alias_conversation_id: old.conversation_id.clone(),
            canonical_conversation_id: next_id.clone(),
            source: "public_wns_directory_repair".to_owned(),
            verified_at: verified_at.to_owned(),
        },
    )?;
    for table in ["messages", "thread_read_state", "sync_thread_bindings"] {
        db.execute(&format!("UPDATE {table} SET conversation_id=?1 WHERE owner_identity_id=?2 AND conversation_id=?3"),
            (&next_id, owner, &old.conversation_id)).map_err(super::local_state_unavailable)?;
    }
    db.execute(
        "UPDATE messages SET thread_id=?1 WHERE owner_identity_id=?2 AND thread_id=?3",
        (&next_id, owner, &old.conversation_id),
    )
    .map_err(super::local_state_unavailable)?;
    db.execute(
        "UPDATE thread_read_state SET thread_id=?1 WHERE owner_identity_id=?2 AND thread_id=?3",
        (&next_id, owner, &old.conversation_id),
    )
    .map_err(super::local_state_unavailable)?;
    super::conversation_registry::mark_merged(db, owner, &old.conversation_id, &next_id)?;
    db.execute(
        "DELETE FROM direct_peer_routes WHERE owner_identity_id=?1 AND conversation_id=?2",
        (owner, &old.conversation_id),
    )
    .map_err(super::local_state_unavailable)?;
    // Retain the old immutable Persona and merged registry as migration evidence.
    super::conversation_summaries::rebuild_conversation(db, owner, &old.conversation_id)?;
    super::conversation_summaries::rebuild_conversation(db, owner, &next_id)?;
    Ok(())
}

#[cfg(test)]
#[path = "foreign_persona_migration_tests.rs"]
mod tests;
