//! Owner-scoped cache for proof-verified did:wba transition edges.
//!
//! This table is deliberately not a route or Persona projection. Writers may
//! only persist edges returned with a strong assurance by the ANP resolver.

use std::collections::HashMap;

use anp::authentication::{TransitionAssurance, TransitionCache};
use rusqlite::{Connection, OptionalExtension};

pub(crate) const TABLE_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS did_transition_edges (
    owner_identity_id TEXT NOT NULL,
    predecessor_did   TEXT NOT NULL,
    successor_did     TEXT NOT NULL,
    assurance         TEXT NOT NULL,
    verified_at       TEXT NOT NULL,
    PRIMARY KEY (owner_identity_id, predecessor_did),
    CHECK (assurance IN ('verified', 'recovery_verified')),
    CHECK (length(trim(owner_identity_id)) > 0),
    CHECK (length(trim(predecessor_did)) > 0),
    CHECK (length(trim(successor_did)) > 0),
    CHECK (predecessor_did <> successor_did)
);
CREATE INDEX IF NOT EXISTS idx_did_transition_edges_owner_successor
ON did_transition_edges(owner_identity_id, successor_did);
"#;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct VerifiedDidTransitionEdge {
    pub(crate) predecessor_did: String,
    pub(crate) successor_did: String,
    pub(crate) assurance: TransitionAssurance,
}

#[derive(Debug, Default)]
pub(crate) struct VerifiedDidTransitionCache {
    edges: HashMap<String, String>,
}

impl VerifiedDidTransitionCache {
    pub(crate) fn load(connection: &Connection, owner_identity_id: &str) -> crate::ImResult<Self> {
        let owner_identity_id = required("owner_identity_id", owner_identity_id)?;
        let mut statement = connection
            .prepare(
                "SELECT predecessor_did, successor_did FROM did_transition_edges \
                 WHERE owner_identity_id=?1 ORDER BY predecessor_did",
            )
            .map_err(super::local_state_unavailable)?;
        let rows = statement
            .query_map([owner_identity_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(super::local_state_unavailable)?;
        let mut edges = HashMap::new();
        for row in rows {
            let (predecessor, successor) = row.map_err(super::local_state_unavailable)?;
            if edges.insert(predecessor, successor).is_some() {
                return Err(crate::ImError::IdentityBindingConflict {
                    detail: "DID transition cache contains duplicate predecessors".to_owned(),
                });
            }
        }
        Ok(Self { edges })
    }
}

impl TransitionCache for VerifiedDidTransitionCache {
    fn get_successor(&self, predecessor_did: &str) -> Option<&str> {
        self.edges.get(predecessor_did).map(String::as_str)
    }

    fn compare_and_set(&mut self, predecessor_did: &str, successor_did: &str) -> bool {
        match self.edges.get(predecessor_did) {
            Some(existing) => existing == successor_did,
            None => {
                self.edges
                    .insert(predecessor_did.to_owned(), successor_did.to_owned());
                true
            }
        }
    }
}

pub(crate) fn create_schema(connection: &Connection) -> crate::ImResult<()> {
    connection
        .execute_batch(TABLE_SQL)
        .map_err(super::local_state_unavailable)
}

pub(crate) fn get_successor(
    connection: &Connection,
    owner_identity_id: &str,
    predecessor_did: &str,
) -> crate::ImResult<Option<String>> {
    let owner_identity_id = required("owner_identity_id", owner_identity_id)?;
    let predecessor_did = canonical_did("predecessor_did", predecessor_did)?;
    connection
        .query_row(
            "SELECT successor_did FROM did_transition_edges \
             WHERE owner_identity_id=?1 AND predecessor_did=?2",
            (owner_identity_id, predecessor_did.as_str()),
            |row| row.get(0),
        )
        .optional()
        .map_err(super::local_state_unavailable)
}

pub(crate) fn compare_and_set_verified(
    connection: &Connection,
    owner_identity_id: &str,
    edge: &VerifiedDidTransitionEdge,
) -> crate::ImResult<()> {
    let owner_identity_id = required("owner_identity_id", owner_identity_id)?;
    let predecessor = canonical_did("predecessor_did", &edge.predecessor_did)?;
    let successor = canonical_did("successor_did", &edge.successor_did)?;
    if predecessor == successor {
        return Err(crate::ImError::IdentityBindingConflict {
            detail: "DID transition edge cannot point to itself".to_owned(),
        });
    }
    let assurance = match edge.assurance {
        TransitionAssurance::Verified => "verified",
        TransitionAssurance::RecoveryVerified => "recovery_verified",
        TransitionAssurance::ProviderAsserted | TransitionAssurance::Unverified => {
            return Err(crate::ImError::IdentityBindingConflict {
                detail: "weak DID transition assurance cannot enter verified cache".to_owned(),
            });
        }
    };

    let transaction = connection
        .unchecked_transaction()
        .map_err(super::local_state_unavailable)?;
    check_read_projection_consistency(&transaction, owner_identity_id, &predecessor, &successor)?;
    transaction
        .execute(
            r#"INSERT INTO did_transition_edges
               (owner_identity_id, predecessor_did, successor_did, assurance, verified_at)
               VALUES (?1, ?2, ?3, ?4, ?5)
               ON CONFLICT(owner_identity_id, predecessor_did) DO NOTHING"#,
            rusqlite::params![
                owner_identity_id,
                predecessor,
                successor,
                assurance,
                time::OffsetDateTime::now_utc().unix_timestamp().to_string(),
            ],
        )
        .map_err(super::local_state_unavailable)?;
    let stored: String = transaction
        .query_row(
            "SELECT successor_did FROM did_transition_edges \
             WHERE owner_identity_id=?1 AND predecessor_did=?2",
            (owner_identity_id, predecessor.as_str()),
            |row| row.get(0),
        )
        .map_err(super::local_state_unavailable)?;
    if stored != successor {
        return Err(crate::ImError::IdentityBindingConflict {
            detail: "verified DID transition cache has a conflicting successor".to_owned(),
        });
    }
    transaction.commit().map_err(super::local_state_unavailable)
}

fn check_read_projection_consistency(
    connection: &Connection,
    owner_identity_id: &str,
    predecessor_did: &str,
    successor_did: &str,
) -> crate::ImResult<()> {
    // A transition cache entry must never be used to manufacture a second
    // Persona or conversation. Existing projections are inspected only for
    // contradictions and are not updated here.
    let persona_count: i64 = connection
        .query_row(
            r#"SELECT COUNT(DISTINCT peer_persona_id) FROM peer_identifiers
               WHERE owner_identity_id=?1 AND identifier_kind='did'
                 AND identifier_value IN (?2, ?3)"#,
            (owner_identity_id, predecessor_did, successor_did),
            |row| row.get(0),
        )
        .map_err(super::local_state_unavailable)?;
    if persona_count > 1 {
        return Err(crate::ImError::IdentityBindingConflict {
            detail: "transition endpoints are bound to different immutable Personas".to_owned(),
        });
    }

    let route_count: i64 = connection
        .query_row(
            r#"SELECT COUNT(DISTINCT conversation_id) FROM direct_peer_routes
               WHERE owner_identity_id=?1 AND current_did IN (?2, ?3)"#,
            (owner_identity_id, predecessor_did, successor_did),
            |row| row.get(0),
        )
        .map_err(super::local_state_unavailable)?;
    if route_count > 1 {
        return Err(crate::ImError::IdentityBindingConflict {
            detail: "transition endpoints are projected to different conversations".to_owned(),
        });
    }

    let self_history_count: i64 = connection
        .query_row(
            r#"SELECT COUNT(*) FROM identity_did_history
               WHERE owner_identity_id=?1 AND did IN (?2, ?3)"#,
            (owner_identity_id, predecessor_did, successor_did),
            |row| row.get(0),
        )
        .map_err(super::local_state_unavailable)?;
    if self_history_count == 1 {
        return Err(crate::ImError::IdentityBindingConflict {
            detail: "partial local identity history cannot be extended by peer transition cache"
                .to_owned(),
        });
    }

    let unresolved_binding_count: i64 = connection
        .query_row(
            r#"SELECT COUNT(DISTINCT b.remote_thread_key)
               FROM inbound_resolution_backlog q
               JOIN inbound_resolution_thread_bindings b
                 ON b.owner_identity_id=q.owner_identity_id
                AND b.event_id=q.event_id AND b.message_id=q.message_id
               WHERE q.owner_identity_id=?1 AND q.peer_did IN (?2, ?3)
                 AND q.resolution_state='pending' AND b.thread_kind='direct'"#,
            (owner_identity_id, predecessor_did, successor_did),
            |row| row.get(0),
        )
        .map_err(super::local_state_unavailable)?;
    if unresolved_binding_count > 1 {
        return Err(crate::ImError::IdentityBindingConflict {
            detail: "transition endpoints have conflicting unresolved Direct bindings".to_owned(),
        });
    }
    Ok(())
}

fn required<'a>(field: &str, value: &'a str) -> crate::ImResult<&'a str> {
    let value = value.trim();
    if value.is_empty() {
        return Err(crate::ImError::invalid_input(
            Some(field.to_owned()),
            format!("{field} is required"),
        ));
    }
    Ok(value)
}

fn canonical_did(field: &str, value: &str) -> crate::ImResult<String> {
    let value = required(field, value)?;
    let did = crate::ids::Did::parse(value)?;
    if did.as_str() != value {
        return Err(crate::ImError::invalid_input(
            Some(field.to_owned()),
            format!("{field} must be canonical"),
        ));
    }
    Ok(value.to_owned())
}

#[cfg(test)]
mod tests;
