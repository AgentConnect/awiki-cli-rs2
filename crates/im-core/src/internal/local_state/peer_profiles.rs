//! Persona-keyed display profile projection.

use rusqlite::Connection;

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
    display_name = COALESCE(excluded.display_name, peer_profiles.display_name),
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
