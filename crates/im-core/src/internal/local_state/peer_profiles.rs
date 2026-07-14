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
