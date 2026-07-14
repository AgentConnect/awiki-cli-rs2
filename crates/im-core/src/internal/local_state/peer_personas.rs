//! Owner-scoped canonical Handle Persona projection.

use rusqlite::{Connection, OptionalExtension};

pub(crate) const TABLE_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS peer_personas (
    owner_identity_id      TEXT NOT NULL,
    peer_persona_id        TEXT NOT NULL,
    authority_namespace    TEXT NOT NULL,
    authority_subject_id   TEXT NOT NULL,
    full_handle            TEXT NOT NULL,
    binding_generation     TEXT,
    subject_type           TEXT NOT NULL DEFAULT 'human',
    source                 TEXT NOT NULL,
    authority_revision     TEXT,
    verified_at            TEXT NOT NULL,
    created_at             TEXT NOT NULL,
    updated_at             TEXT NOT NULL,
    PRIMARY KEY (owner_identity_id, peer_persona_id),
    UNIQUE (owner_identity_id, authority_namespace, authority_subject_id, full_handle)
);
"#;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PeerPersonaRecord {
    pub(crate) owner_identity_id: String,
    pub(crate) persona: crate::internal::canonical_identity::PeerPersona,
    pub(crate) binding_generation: Option<String>,
    pub(crate) subject_type: String,
    pub(crate) source: String,
    pub(crate) authority_revision: Option<String>,
    pub(crate) verified_at: String,
}

pub(crate) fn create_schema(connection: &Connection) -> crate::ImResult<()> {
    connection
        .execute_batch(TABLE_SQL)
        .map_err(super::local_state_unavailable)
}

pub(crate) fn upsert(connection: &Connection, record: &PeerPersonaRecord) -> crate::ImResult<()> {
    validate(record)?;
    let now = now();
    connection
        .execute(
            r#"
INSERT INTO peer_personas
    (owner_identity_id, peer_persona_id, authority_namespace, authority_subject_id,
     full_handle, binding_generation, subject_type, source, authority_revision,
     verified_at, created_at, updated_at)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11)
ON CONFLICT(owner_identity_id, peer_persona_id) DO UPDATE SET
    binding_generation = COALESCE(excluded.binding_generation, peer_personas.binding_generation),
    subject_type = excluded.subject_type,
    source = excluded.source,
    authority_revision = COALESCE(excluded.authority_revision, peer_personas.authority_revision),
    verified_at = excluded.verified_at,
    updated_at = excluded.updated_at
WHERE peer_personas.authority_namespace = excluded.authority_namespace
  AND peer_personas.authority_subject_id = excluded.authority_subject_id
  AND peer_personas.full_handle = excluded.full_handle"#,
            rusqlite::params![
                record.owner_identity_id.trim(),
                record.persona.peer_persona_id,
                record.persona.authority_namespace,
                record.persona.authority_subject_id,
                record.persona.full_handle,
                record.binding_generation.as_deref(),
                record.subject_type.trim(),
                record.source.trim(),
                record.authority_revision.as_deref(),
                record.verified_at.trim(),
                now,
            ],
        )
        .map_err(super::local_state_unavailable)?;

    let stored = connection
        .query_row(
            r#"SELECT authority_namespace, authority_subject_id, full_handle
FROM peer_personas WHERE owner_identity_id = ?1 AND peer_persona_id = ?2"#,
            (
                record.owner_identity_id.trim(),
                record.persona.peer_persona_id.as_str(),
            ),
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .optional()
        .map_err(super::local_state_unavailable)?;
    if stored.as_ref()
        != Some(&(
            record.persona.authority_namespace.clone(),
            record.persona.authority_subject_id.clone(),
            record.persona.full_handle.clone(),
        ))
    {
        return Err(crate::ImError::IdentityBindingConflict {
            detail: "peer Persona immutable authority identity does not match stored state"
                .to_owned(),
        });
    }
    Ok(())
}

pub(crate) fn project_verified_handle(
    connection: &mut Connection,
    owner_identity_id: &str,
    owner_did: &str,
    lookup: &crate::directory::HandleLookupResult,
) -> crate::ImResult<String> {
    let persona = lookup.peer_persona()?;
    let verified_at = now();
    let transaction = connection
        .transaction()
        .map_err(super::local_state_unavailable)?;
    upsert(
        &transaction,
        &PeerPersonaRecord {
            owner_identity_id: owner_identity_id.trim().to_owned(),
            persona: persona.clone(),
            binding_generation: lookup.binding_generation.clone(),
            subject_type: "human".to_owned(),
            source: "handle_authority".to_owned(),
            authority_revision: None,
            verified_at: verified_at.clone(),
        },
    )?;
    for (kind, value) in [
        ("handle", persona.full_handle.as_str()),
        ("did", lookup.did.as_str()),
    ] {
        if kind == "did" {
            transaction
                .execute(
                    r#"UPDATE peer_identifiers SET is_current = 0
WHERE owner_identity_id = ?1 AND peer_persona_id = ?2
  AND identifier_kind = 'did' AND identifier_value <> ?3"#,
                    (
                        owner_identity_id.trim(),
                        persona.peer_persona_id.as_str(),
                        value,
                    ),
                )
                .map_err(super::local_state_unavailable)?;
        }
        super::peer_identifiers::bind(
            &transaction,
            &super::peer_identifiers::PeerIdentifierRecord {
                owner_identity_id: owner_identity_id.trim().to_owned(),
                peer_persona_id: persona.peer_persona_id.clone(),
                identifier_kind: kind.to_owned(),
                identifier_value: value.to_owned(),
                is_current: true,
                binding_generation: lookup.binding_generation.clone(),
                source: "handle_authority".to_owned(),
                verified_at: verified_at.clone(),
            },
        )?;
    }
    if let Some(profile) = lookup.profile.as_ref() {
        super::peer_profiles::upsert_from_verified_lookup(
            &transaction,
            owner_identity_id,
            &persona.peer_persona_id,
            &persona.full_handle,
            profile,
        )?;
    }
    let conflicting_contacts: i64 = transaction
        .query_row(
            r#"SELECT COUNT(*) FROM contacts
WHERE owner_identity_id = ?1
  AND TRIM(COALESCE(peer_persona_id, '')) <> ''
  AND peer_persona_id <> ?2
  AND (
      did IN (
          SELECT identifier_value FROM peer_identifiers
          WHERE owner_identity_id = ?1 AND peer_persona_id = ?2
            AND identifier_kind = 'did'
      )
      OR LOWER(TRIM(COALESCE(handle, ''))) = ?3
  )"#,
            (
                owner_identity_id.trim(),
                persona.peer_persona_id.as_str(),
                persona.full_handle.as_str(),
            ),
            |row| row.get(0),
        )
        .map_err(super::local_state_unavailable)?;
    if conflicting_contacts > 0 {
        return Err(crate::ImError::IdentityBindingConflict {
            detail: "contact projection is already bound to another Persona".to_owned(),
        });
    }
    transaction
        .execute(
            r#"UPDATE contacts SET peer_persona_id = ?1
WHERE owner_identity_id = ?2
  AND (
      did IN (
          SELECT identifier_value FROM peer_identifiers
          WHERE owner_identity_id = ?2 AND peer_persona_id = ?1
            AND identifier_kind = 'did'
      )
      OR LOWER(TRIM(COALESCE(handle, ''))) = ?3
  )"#,
            (
                persona.peer_persona_id.as_str(),
                owner_identity_id.trim(),
                persona.full_handle.as_str(),
            ),
        )
        .map_err(super::local_state_unavailable)?;
    let route = super::direct_peer_routes::DirectPeerRouteRecord::from_verified_persona(
        owner_identity_id,
        &persona,
        lookup.did.as_str(),
    )?;
    super::direct_peer_routes::upsert(&transaction, &route)?;
    super::conversation_registry::ensure(
        &transaction,
        &super::conversation_registry::ConversationRegistryRecord {
            owner_identity_id: owner_identity_id.trim().to_owned(),
            owner_did: owner_did.trim().to_owned(),
            conversation_id: route.conversation_id.clone(),
            thread_kind: "direct".to_owned(),
            thread_id: route.conversation_id.clone(),
            activity_at: verified_at.clone(),
        },
    )?;
    super::conversation_aliases::insert(
        &transaction,
        &super::conversation_aliases::ConversationAliasRecord {
            owner_identity_id: owner_identity_id.trim().to_owned(),
            alias_kind: "verified_did".to_owned(),
            alias_conversation_id: super::owner_scope::direct_conversation_id(lookup.did.as_str()),
            canonical_conversation_id: route.conversation_id.clone(),
            source: "handle_authority".to_owned(),
            verified_at,
        },
    )?;
    transaction
        .commit()
        .map_err(super::local_state_unavailable)?;
    Ok(route.conversation_id)
}

fn validate(record: &PeerPersonaRecord) -> crate::ImResult<()> {
    for (field, value) in [
        ("owner_identity_id", record.owner_identity_id.as_str()),
        ("peer_persona_id", record.persona.peer_persona_id.as_str()),
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
    Ok(())
}

fn now() -> String {
    time::OffsetDateTime::now_utc().unix_timestamp().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verified_handle_projection_is_owner_scoped_and_rotation_keeps_canonical_ids() {
        let mut db = Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        let lookup = |did: &str, generation: &str| crate::directory::HandleLookupResult {
            handle: crate::ids::Handle::parse("Alice.AWiki.Info", "").unwrap(),
            did: crate::ids::Did::parse(did).unwrap(),
            user_id: "user-alice".to_owned(),
            domain: Some("AWIKI.INFO.".to_owned()),
            status: Some("active".to_owned()),
            binding_generation: Some(generation.to_owned()),
            profile: None,
            warnings: Vec::new(),
        };
        let first = project_verified_handle(
            &mut db,
            "owner-a",
            "did:example:owner",
            &lookup("did:example:alice-old", "1"),
        )
        .unwrap();
        let rotated = project_verified_handle(
            &mut db,
            "owner-a",
            "did:example:owner",
            &lookup("did:example:alice-new", "2"),
        )
        .unwrap();
        assert_eq!(first, rotated);
        assert_eq!(
            db.query_row(
                "SELECT COUNT(*) FROM peer_personas WHERE owner_identity_id = 'owner-a'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
            1
        );
        assert_eq!(
            db.query_row(
                "SELECT COUNT(*) FROM conversation_registry WHERE owner_identity_id = 'owner-a' AND lifecycle_state = 'active' AND resolution_state = 'resolved'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
            1
        );
        assert_eq!(
            db.query_row(
                "SELECT COUNT(*) FROM peer_identifiers WHERE owner_identity_id = 'owner-a' AND identifier_kind = 'did'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
            2
        );
    }

    #[test]
    fn verified_handle_projection_persists_persona_keyed_profile_without_degrading_it() {
        let mut db = Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        let mut profile =
            crate::identity::Profile::new(crate::ids::Did::parse("did:example:alice").unwrap());
        profile.display_name = Some("Alice".to_owned());
        profile.avatar_uri = Some("https://example.test/alice.png".to_owned());
        profile.version_id = Some("profile-v1".to_owned());
        let lookup = crate::directory::HandleLookupResult {
            handle: crate::ids::Handle::parse("alice.awiki.info", "").unwrap(),
            did: crate::ids::Did::parse("did:example:alice").unwrap(),
            user_id: "user-alice".to_owned(),
            domain: Some("awiki.info".to_owned()),
            status: Some("active".to_owned()),
            binding_generation: Some("1".to_owned()),
            profile: Some(profile),
            warnings: Vec::new(),
        };
        let conversation_id =
            project_verified_handle(&mut db, "owner-a", "did:example:owner", &lookup).unwrap();
        let stored: (String, String, String) = db
            .query_row(
                r#"SELECT display_name, full_handle, profile_version
FROM peer_profiles WHERE owner_identity_id = 'owner-a'"#,
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(stored.0, "Alice");
        assert_eq!(stored.1, "alice.awiki.info");
        assert_eq!(stored.2, "profile-v1");
        assert!(conversation_id.starts_with("dm:peer-scope:v1:"));
    }
}
