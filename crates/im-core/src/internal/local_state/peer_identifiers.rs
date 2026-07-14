//! Conflict-visible identifier bindings for canonical peer Personas.

use rusqlite::{Connection, OptionalExtension};

pub(crate) const TABLE_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS peer_identifiers (
    owner_identity_id    TEXT NOT NULL,
    peer_persona_id      TEXT NOT NULL,
    identifier_kind     TEXT NOT NULL,
    identifier_value    TEXT NOT NULL,
    is_current          INTEGER NOT NULL DEFAULT 1,
    binding_generation  TEXT,
    source              TEXT NOT NULL,
    verified_at         TEXT NOT NULL,
    first_seen_at       TEXT NOT NULL,
    last_seen_at        TEXT NOT NULL,
    PRIMARY KEY (owner_identity_id, identifier_kind, identifier_value),
    FOREIGN KEY (owner_identity_id, peer_persona_id)
      REFERENCES peer_personas(owner_identity_id, peer_persona_id)
);
CREATE INDEX IF NOT EXISTS idx_peer_identifiers_owner_persona
ON peer_identifiers(owner_identity_id, peer_persona_id, identifier_kind, is_current);
"#;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PeerIdentifierRecord {
    pub(crate) owner_identity_id: String,
    pub(crate) peer_persona_id: String,
    pub(crate) identifier_kind: String,
    pub(crate) identifier_value: String,
    pub(crate) is_current: bool,
    pub(crate) binding_generation: Option<String>,
    pub(crate) source: String,
    pub(crate) verified_at: String,
}

pub(crate) fn create_schema(connection: &Connection) -> crate::ImResult<()> {
    connection
        .execute_batch(TABLE_SQL)
        .map_err(super::local_state_unavailable)
}

pub(crate) fn bind(connection: &Connection, record: &PeerIdentifierRecord) -> crate::ImResult<()> {
    for (field, value) in [
        ("owner_identity_id", record.owner_identity_id.as_str()),
        ("peer_persona_id", record.peer_persona_id.as_str()),
        ("identifier_kind", record.identifier_kind.as_str()),
        ("identifier_value", record.identifier_value.as_str()),
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
    if !matches!(record.identifier_kind.trim(), "did" | "handle") {
        return Err(crate::ImError::invalid_input(
            Some("identifier_kind".to_owned()),
            "identifier_kind must be did or handle",
        ));
    }
    let now = time::OffsetDateTime::now_utc().unix_timestamp().to_string();
    connection
        .execute(
            r#"INSERT OR IGNORE INTO peer_identifiers
    (owner_identity_id, peer_persona_id, identifier_kind, identifier_value,
     is_current, binding_generation, source, verified_at, first_seen_at, last_seen_at)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9)"#,
            rusqlite::params![
                record.owner_identity_id.trim(),
                record.peer_persona_id.trim(),
                record.identifier_kind.trim(),
                record.identifier_value.trim(),
                i64::from(record.is_current),
                record.binding_generation.as_deref(),
                record.source.trim(),
                record.verified_at.trim(),
                now,
            ],
        )
        .map_err(super::local_state_unavailable)?;
    let existing = connection
        .query_row(
            r#"SELECT peer_persona_id FROM peer_identifiers
WHERE owner_identity_id = ?1 AND identifier_kind = ?2 AND identifier_value = ?3"#,
            (
                record.owner_identity_id.trim(),
                record.identifier_kind.trim(),
                record.identifier_value.trim(),
            ),
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(super::local_state_unavailable)?;
    if existing.as_deref() != Some(record.peer_persona_id.trim()) {
        return Err(crate::ImError::IdentityBindingConflict {
            detail: format!(
                "{} identifier is already bound to another Persona",
                record.identifier_kind.trim()
            ),
        });
    }
    connection
        .execute(
            r#"UPDATE peer_identifiers SET
    is_current = ?1,
    binding_generation = COALESCE(?2, binding_generation),
    source = ?3,
    verified_at = ?4,
    last_seen_at = ?5
WHERE owner_identity_id = ?6 AND identifier_kind = ?7 AND identifier_value = ?8"#,
            rusqlite::params![
                i64::from(record.is_current),
                record.binding_generation.as_deref(),
                record.source.trim(),
                record.verified_at.trim(),
                now,
                record.owner_identity_id.trim(),
                record.identifier_kind.trim(),
                record.identifier_value.trim(),
            ],
        )
        .map_err(super::local_state_unavailable)?;
    Ok(())
}
