//! Owner-scoped routing projection for canonical Direct conversations.
//!
//! A peer-scope conversation id is intentionally non-reversible. Directory
//! resolution records the current DID here so conversation-first operations
//! can route an empty conversation without an App-constructed `dm:<DID>`
//! alias. Messages remain the durable conversation truth.

use rusqlite::{Connection, OptionalExtension};

use super::owner_scope::{direct_conversation_id_for_peer_scope, DirectPeerScope, OwnerScope};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DirectPeerRouteRecord {
    pub(crate) owner_identity_id: String,
    pub(crate) conversation_id: String,
    pub(crate) peer_persona_id: Option<String>,
    pub(crate) authority_namespace: Option<String>,
    pub(crate) peer_user_id: String,
    pub(crate) full_handle: String,
    pub(crate) current_did: String,
    pub(crate) updated_at: String,
}

impl DirectPeerRouteRecord {
    pub(crate) fn for_client(
        client: &crate::core::ImClient,
        conversation_id: &str,
        peer_scope: &DirectPeerScope,
        current_did: &str,
    ) -> crate::ImResult<Self> {
        let owner = OwnerScope::for_client(client)?;
        Self::new(
            owner.owner_identity_id,
            conversation_id,
            peer_scope.user_id.clone(),
            peer_scope.full_handle.clone(),
            current_did,
        )
    }

    pub(crate) fn new(
        owner_identity_id: impl Into<String>,
        conversation_id: impl Into<String>,
        peer_user_id: impl Into<String>,
        full_handle: impl Into<String>,
        current_did: impl Into<String>,
    ) -> crate::ImResult<Self> {
        let owner_identity_id = required("owner_identity_id", owner_identity_id.into())?;
        let conversation_id = required("conversation_id", conversation_id.into())?;
        let peer_scope = DirectPeerScope::new(peer_user_id, full_handle)?;
        if conversation_id != direct_conversation_id_for_peer_scope(&peer_scope) {
            return Err(crate::ImError::invalid_input(
                Some("conversation_id".to_owned()),
                "conversation_id does not match the resolved Direct peer scope",
            ));
        }
        let current_did = required("current_did", current_did.into())?;
        crate::ids::Did::parse(&current_did)?;
        Ok(Self {
            owner_identity_id,
            conversation_id,
            peer_persona_id: None,
            authority_namespace: None,
            peer_user_id: peer_scope.user_id,
            full_handle: peer_scope.full_handle,
            current_did,
            updated_at: time::OffsetDateTime::now_utc().unix_timestamp().to_string(),
        })
    }

    pub(crate) fn from_verified_persona(
        owner_identity_id: impl Into<String>,
        persona: &crate::internal::canonical_identity::PeerPersona,
        current_did: impl Into<String>,
    ) -> crate::ImResult<Self> {
        let current_did = required("current_did", current_did.into())?;
        crate::ids::Did::parse(&current_did)?;
        Ok(Self {
            owner_identity_id: required("owner_identity_id", owner_identity_id.into())?,
            conversation_id: persona.direct_conversation_id(),
            peer_persona_id: Some(persona.peer_persona_id.clone()),
            authority_namespace: Some(persona.authority_namespace.clone()),
            peer_user_id: persona.authority_subject_id.clone(),
            full_handle: persona.full_handle.clone(),
            current_did,
            updated_at: time::OffsetDateTime::now_utc().unix_timestamp().to_string(),
        })
    }

    pub(crate) fn peer_scope(&self) -> DirectPeerScope {
        DirectPeerScope {
            user_id: self.peer_user_id.clone(),
            full_handle: self.full_handle.clone(),
        }
    }
}

pub(crate) fn upsert(
    connection: &Connection,
    record: &DirectPeerRouteRecord,
) -> crate::ImResult<()> {
    validate_record(record)?;
    connection
        .execute(
            r#"
INSERT INTO direct_peer_routes
    (owner_identity_id, conversation_id, peer_persona_id, authority_namespace,
     peer_user_id, full_handle, current_did, updated_at)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
ON CONFLICT(owner_identity_id, conversation_id)
DO UPDATE SET
    peer_persona_id = COALESCE(excluded.peer_persona_id, direct_peer_routes.peer_persona_id),
    authority_namespace = COALESCE(excluded.authority_namespace, direct_peer_routes.authority_namespace),
    peer_user_id = excluded.peer_user_id,
    full_handle = excluded.full_handle,
    current_did = excluded.current_did,
    updated_at = excluded.updated_at
"#,
            rusqlite::params![
                record.owner_identity_id,
                record.conversation_id,
                record.peer_persona_id,
                record.authority_namespace,
                record.peer_user_id,
                record.full_handle,
                record.current_did,
                record.updated_at,
            ],
        )
        .map_err(super::local_state_unavailable)?;
    Ok(())
}

pub(crate) fn get(
    connection: &Connection,
    owner_identity_id: &str,
    conversation_id: &str,
) -> crate::ImResult<Option<DirectPeerRouteRecord>> {
    let owner_identity_id = required("owner_identity_id", owner_identity_id.to_owned())?;
    let conversation_id = required("conversation_id", conversation_id.to_owned())?;
    let record = connection
        .query_row(
            r#"
SELECT owner_identity_id, conversation_id, peer_persona_id, authority_namespace,
       peer_user_id, full_handle, current_did, updated_at
FROM direct_peer_routes
WHERE owner_identity_id = ?1 AND conversation_id = ?2
"#,
            rusqlite::params![owner_identity_id, conversation_id],
            |row| {
                Ok(DirectPeerRouteRecord {
                    owner_identity_id: row.get(0)?,
                    conversation_id: row.get(1)?,
                    peer_persona_id: row.get(2)?,
                    authority_namespace: row.get(3)?,
                    peer_user_id: row.get(4)?,
                    full_handle: row.get(5)?,
                    current_did: row.get(6)?,
                    updated_at: row.get(7)?,
                })
            },
        )
        .optional()
        .map_err(super::local_state_unavailable)?;
    match record {
        Some(record) => {
            validate_record(&record).map_err(|_| crate::ImError::LocalStateUnavailable {
                detail: "Direct peer route integrity check failed".to_owned(),
            })?;
            Ok(Some(record))
        }
        None => Ok(None),
    }
}

fn validate_record(record: &DirectPeerRouteRecord) -> crate::ImResult<()> {
    if let (Some(peer_persona_id), Some(authority_namespace)) = (
        record.peer_persona_id.as_deref(),
        record.authority_namespace.as_deref(),
    ) {
        let persona = crate::internal::canonical_identity::PeerPersona::from_verified_handle(
            authority_namespace,
            &record.peer_user_id,
            &record.full_handle,
            Some("active"),
        )?;
        if peer_persona_id != persona.peer_persona_id
            || record.conversation_id != persona.direct_conversation_id()
        {
            return Err(crate::ImError::IdentityBindingConflict {
                detail: "Direct route does not match its canonical Persona".to_owned(),
            });
        }
        crate::ids::Did::parse(&record.current_did)?;
        return Ok(());
    }
    if record.peer_persona_id.is_some() || record.authority_namespace.is_some() {
        return Err(crate::ImError::IdentityBindingConflict {
            detail: "Direct route has a partial Persona binding".to_owned(),
        });
    }
    let validated = DirectPeerRouteRecord::new(
        record.owner_identity_id.clone(),
        record.conversation_id.clone(),
        record.peer_user_id.clone(),
        record.full_handle.clone(),
        record.current_did.clone(),
    )?;
    if validated.owner_identity_id != record.owner_identity_id
        || validated.conversation_id != record.conversation_id
        || validated.peer_persona_id != record.peer_persona_id
        || validated.authority_namespace != record.authority_namespace
        || validated.peer_user_id != record.peer_user_id
        || validated.full_handle != record.full_handle
        || validated.current_did != record.current_did
    {
        return Err(crate::ImError::LocalStateUnavailable {
            detail: "Direct peer route normalization mismatch".to_owned(),
        });
    }
    Ok(())
}

fn required(field: &'static str, value: String) -> crate::ImResult<String> {
    let value = value.trim();
    if value.is_empty() {
        return Err(crate::ImError::invalid_input(
            Some(field.to_owned()),
            format!("{field} must not be empty"),
        ));
    }
    Ok(value.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_is_owner_scoped_and_rotation_updates_current_did() {
        let connection = Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&connection).unwrap();
        let scope = DirectPeerScope::new("user-bob", "Bob.AWiki.Test").unwrap();
        let conversation_id = direct_conversation_id_for_peer_scope(&scope);
        let first = DirectPeerRouteRecord::new(
            "owner-a",
            &conversation_id,
            "user-bob",
            "bob.awiki.test",
            "did:example:bob-old",
        )
        .unwrap();
        upsert(&connection, &first).unwrap();
        assert!(get(&connection, "owner-b", &conversation_id)
            .unwrap()
            .is_none());

        let rotated = DirectPeerRouteRecord::new(
            "owner-a",
            &conversation_id,
            "user-bob",
            "BOB.AWIKI.TEST",
            "did:example:bob-current",
        )
        .unwrap();
        upsert(&connection, &rotated).unwrap();

        let stored = get(&connection, "owner-a", &conversation_id)
            .unwrap()
            .expect("route");
        assert_eq!(stored.current_did, "did:example:bob-current");
        assert_eq!(stored.full_handle, "bob.awiki.test");
        assert_eq!(stored.peer_scope(), scope);
    }

    #[test]
    fn route_rejects_conversation_id_mismatch() {
        let err = DirectPeerRouteRecord::new(
            "owner-a",
            "dm:peer-scope:v1:not-the-scope",
            "user-bob",
            "bob.awiki.test",
            "did:example:bob",
        )
        .unwrap_err();
        assert!(matches!(
            err,
            crate::ImError::InvalidInput {
                field: Some(ref field),
                ..
            } if field == "conversation_id"
        ));
    }
}
