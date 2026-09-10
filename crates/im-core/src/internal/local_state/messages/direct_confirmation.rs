//! Narrow confirmation of a provisional ordinary Direct target. No writes here:
//! the caller applies the decision in the same SQL upsert/outer sync transaction.

use rusqlite::{Connection, OptionalExtension};
use serde_json::Value;

use super::{MessageHydrationState, MessageRecord, WireThreadIdentity};

pub(super) fn can_confirm_target(
    db: &Connection,
    owner: &str,
    message_id: &str,
    conversation: &str,
    incoming: &MessageRecord,
    wire: &WireThreadIdentity,
) -> crate::ImResult<bool> {
    if wire.kind != "direct"
        || wire.resolution_state != "resolved"
        || incoming.direction != 1
        || incoming.is_e2ee
        || incoming.hydration_state != MessageHydrationState::Hydrated
        || incoming.sender_did != incoming.owner_did
        || incoming.receiver_did != wire.reference
        || !incoming.group_id.is_empty()
        || !incoming.group_did.is_empty()
    {
        return Ok(false);
    }
    let existing: Option<(String, String)> = db
        .query_row(
            r#"SELECT COALESCE(metadata, ''), receiver_did FROM messages
WHERE owner_identity_id = ?1 AND msg_id = ?2 AND owner_did = ?3
  AND conversation_id = ?4 AND direction = 1 AND sender_did = ?3
  AND wire_thread_kind = 'direct' AND wire_identity_resolution_state = 'resolved'
  AND wire_thread_ref = receiver_did AND receiver_did <> ?5
  AND server_seq IS NULL AND is_e2ee = 0 AND hydration_state = 'hydrated'
  AND COALESCE(group_id, '') = '' AND COALESCE(group_did, '') = ''
  AND content_type = ?6 AND COALESCE(content, '') = ?7"#,
            rusqlite::params![
                owner,
                message_id,
                incoming.owner_did,
                conversation,
                incoming.receiver_did,
                incoming.content_type,
                incoming.content
            ],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(super::super::local_state_unavailable)?;
    let Some((metadata, old_target)) = existing else {
        return Ok(false);
    };
    let (Ok(previous), Ok(next)) = (
        serde_json::from_str::<Value>(&metadata),
        serde_json::from_str::<Value>(&incoming.metadata),
    ) else {
        return Ok(false);
    };
    let operation = previous.get("operation_id").and_then(Value::as_str);
    if operation.is_none_or(|value| value.trim().is_empty())
        || operation != next.get("operation_id").and_then(Value::as_str)
    {
        return Ok(false);
    }
    let remote_confirmation = incoming.server_seq.is_some_and(|seq| seq > 0);
    // Exact legacy signature: a successful send already resolved this target,
    // but the old client persisted its original local echo as wire truth.
    let legacy_confirmation = remote_confirmation
        && matches!(
            previous["delivery_state"].as_str(),
            Some("accepted" | "sent")
        )
        && previous["resolved_target_did"] == incoming.receiver_did
        && previous["peer_current_did"] == incoming.receiver_did;
    let accepted_result = matches!(next["delivery_state"].as_str(), Some("accepted" | "sent"))
        && next["resolved_target_did"] == incoming.receiver_did
        && next["peer_current_did"] == incoming.receiver_did;
    // Reliable sync may beat the HTTP send response back to local storage.
    let provisional = matches!(
        previous["delivery_state"].as_str(),
        Some("pending" | "stored_locally")
    );
    let fresh_confirmation = (provisional
        && (remote_confirmation || (incoming.server_seq.is_none() && accepted_result)))
        || (previous["delivery_state"] == "failed" && remote_confirmation);
    if !legacy_confirmation && !fresh_confirmation {
        return Ok(false);
    }
    // A Handle string/metadata is not proof. Both DID bindings must belong to
    // the SAME verified owner-scoped Persona and canonical Direct registry.
    // Historic repair may refer to a since-rotated successor; fresh acceptance
    // additionally requires the currently verified route.
    db.query_row(
        r#"SELECT EXISTS (
SELECT 1 FROM direct_peer_routes r
JOIN conversation_registry c ON c.owner_identity_id = r.owner_identity_id
 AND c.conversation_id = r.conversation_id AND c.peer_persona_id = r.peer_persona_id
JOIN peer_identifiers old ON old.owner_identity_id = r.owner_identity_id
 AND old.peer_persona_id = r.peer_persona_id AND old.identifier_kind = 'did'
 AND old.identifier_value = ?3 AND old.is_current = 0
JOIN peer_identifiers new ON new.owner_identity_id = r.owner_identity_id
 AND new.peer_persona_id = r.peer_persona_id AND new.identifier_kind = 'did'
 AND new.identifier_value = ?4
WHERE r.owner_identity_id = ?1 AND r.conversation_id = ?2
 AND c.thread_kind = 'direct' AND c.resolution_state = 'resolved'
 AND c.lifecycle_state = 'active'
 AND old.source IN ('handle_authority', 'verified_did_transition')
 AND new.source IN ('handle_authority', 'verified_did_transition')
 AND (?5 OR (r.current_did = ?4 AND new.is_current = 1)))"#,
        rusqlite::params![
            owner,
            conversation,
            old_target,
            incoming.receiver_did,
            legacy_confirmation
        ],
        |row| row.get(0),
    )
    .map_err(super::super::local_state_unavailable)
}

#[cfg(test)]
#[path = "direct_confirmation_tests.rs"]
mod tests;
