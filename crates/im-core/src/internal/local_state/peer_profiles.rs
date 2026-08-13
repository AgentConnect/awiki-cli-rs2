//! Persona-keyed display profile projection.

use rusqlite::{Connection, OptionalExtension};

pub(crate) const TABLE_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS peer_profiles (
    owner_identity_id  TEXT NOT NULL,
    peer_persona_id    TEXT NOT NULL,
    display_name       TEXT,
    full_handle        TEXT NOT NULL,
    avatar_uri         TEXT,
    subject_type       TEXT,
    profile_version    TEXT,
    updated_at         TEXT,
    fetched_at         TEXT NOT NULL,
    expires_at         TEXT,
    PRIMARY KEY (owner_identity_id, peer_persona_id),
    FOREIGN KEY (owner_identity_id, peer_persona_id)
      REFERENCES peer_personas(owner_identity_id, peer_persona_id)
);
"#;

pub(crate) fn create_schema(connection: &Connection) -> crate::ImResult<()> {
    connection
        .execute_batch(TABLE_SQL)
        .map_err(super::local_state_unavailable)
}

pub(crate) fn display_profile_for_peer(
    connection: &Connection,
    owner_identity_id: &str,
    peer: &crate::ids::PeerRef,
) -> crate::ImResult<Option<crate::directory::DisplayProfile>> {
    let peer_value = peer.as_str().trim();
    let is_did = peer_value.starts_with("did:");
    let identifier_kind = if is_did { "did" } else { "handle" };
    let row = connection
        .query_row(
            r#"SELECT persona.full_handle, profile.display_name, profile.avatar_uri,
                      COALESCE(profile.subject_type, persona.subject_type),
                      current_did.identifier_value, profile.expires_at
FROM peer_identifiers requested
JOIN peer_personas persona
  ON persona.owner_identity_id = requested.owner_identity_id
 AND persona.peer_persona_id = requested.peer_persona_id
LEFT JOIN peer_profiles profile
  ON profile.owner_identity_id = persona.owner_identity_id
 AND profile.peer_persona_id = persona.peer_persona_id
LEFT JOIN peer_identifiers current_did
  ON current_did.owner_identity_id = persona.owner_identity_id
 AND current_did.peer_persona_id = persona.peer_persona_id
 AND current_did.identifier_kind = 'did'
 AND current_did.is_current = 1
WHERE requested.owner_identity_id = ?1
  AND requested.identifier_kind = ?2
  AND CASE WHEN ?2 = 'handle'
           THEN LOWER(TRIM(requested.identifier_value)) = LOWER(TRIM(?3))
           ELSE requested.identifier_value = ?3
      END
ORDER BY requested.is_current DESC, current_did.is_current DESC
LIMIT 1"#,
            (owner_identity_id.trim(), identifier_kind, peer_value),
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                ))
            },
        )
        .optional()
        .map_err(super::local_state_unavailable)?;
    let Some((full_handle, display_name, avatar_uri, subject_type, current_did, expires_at)) = row
    else {
        return Ok(None);
    };
    let now = time::OffsetDateTime::now_utc().unix_timestamp();
    let is_stale = expires_at
        .as_deref()
        .and_then(|value| value.parse::<i64>().ok())
        .is_some_and(|value| value <= now);
    let did = if is_did {
        Some(crate::ids::Did::parse(peer_value)?)
    } else {
        current_did
            .as_deref()
            .map(crate::ids::Did::parse)
            .transpose()?
    };
    Ok(Some(crate::directory::DisplayProfile {
        did,
        handle: Some(crate::ids::Handle::parse(&full_handle, "")?),
        display_name,
        avatar_uri,
        avatar_url: None,
        profile_uri: None,
        subject_type,
        cache_hit: true,
        is_stale,
        legacy_fallback: false,
        warnings: Vec::new(),
    }))
}

pub(crate) fn upsert_from_verified_lookup(
    connection: &Connection,
    owner_identity_id: &str,
    peer_persona_id: &str,
    full_handle: &str,
    profile: &crate::identity::Profile,
) -> crate::ImResult<()> {
    let fetched_at = time::OffsetDateTime::now_utc().unix_timestamp();
    let expires_at = profile
        .ttl
        .and_then(|ttl| i64::try_from(ttl).ok())
        .and_then(|ttl| fetched_at.checked_add(ttl))
        .map(|value| value.to_string());
    let avatar_uri = profile
        .avatar_uri
        .as_deref()
        .or(profile.avatar_url.as_deref());
    connection
        .execute(
            r#"INSERT INTO peer_profiles
    (owner_identity_id, peer_persona_id, display_name, full_handle, avatar_uri,
     subject_type, profile_version, updated_at, fetched_at, expires_at)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
ON CONFLICT(owner_identity_id, peer_persona_id) DO UPDATE SET
    display_name = excluded.display_name,
    full_handle = excluded.full_handle,
    avatar_uri = COALESCE(excluded.avatar_uri, peer_profiles.avatar_uri),
    subject_type = COALESCE(excluded.subject_type, peer_profiles.subject_type),
    profile_version = COALESCE(excluded.profile_version, peer_profiles.profile_version),
    updated_at = COALESCE(excluded.updated_at, peer_profiles.updated_at),
    fetched_at = excluded.fetched_at,
    expires_at = excluded.expires_at"#,
            rusqlite::params![
                owner_identity_id.trim(),
                peer_persona_id.trim(),
                profile.display_name.as_deref(),
                full_handle.trim(),
                avatar_uri,
                profile.subject_type.as_deref(),
                profile.version_id.as_deref(),
                profile.updated_at.as_deref(),
                fetched_at.to_string(),
                expires_at,
            ],
        )
        .map_err(super::local_state_unavailable)?;
    Ok(())
}

pub(crate) fn refresh_existing_from_public_profile(
    connection: &Connection,
    owner_identity_id: &str,
    did: &crate::ids::Did,
    profile: &crate::identity::Profile,
) -> crate::ImResult<bool> {
    let binding = connection
        .query_row(
            r#"SELECT persona.peer_persona_id, persona.full_handle
FROM peer_identifiers identifier
JOIN peer_personas persona
  ON persona.owner_identity_id = identifier.owner_identity_id
 AND persona.peer_persona_id = identifier.peer_persona_id
WHERE identifier.owner_identity_id = ?1
  AND identifier.identifier_kind = 'did'
  AND identifier.identifier_value = ?2
  AND identifier.is_current = 1
LIMIT 1"#,
            (owner_identity_id.trim(), did.as_str()),
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()
        .map_err(super::local_state_unavailable)?;
    let Some((peer_persona_id, full_handle)) = binding else {
        return Ok(false);
    };
    upsert_from_verified_lookup(
        connection,
        owner_identity_id,
        &peer_persona_id,
        &full_handle,
        profile,
    )?;
    Ok(true)
}
