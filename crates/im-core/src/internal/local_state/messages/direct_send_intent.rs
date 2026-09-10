//! Freeze a target-first plain Direct request before its first network write.
use rusqlite::{Connection, OptionalExtension, TransactionBehavior};
use serde_json::Value;

use super::MessageRecord;

#[derive(Debug, Clone)]
pub(crate) struct DirectSendIntent {
    pub target_did: String,
    pub created_at: String,
}

pub(crate) fn prepare(
    connection: &mut Connection,
    record: MessageRecord,
) -> crate::ImResult<DirectSendIntent> {
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(super::super::local_state_unavailable)?;
    let metadata: Value =
        serde_json::from_str(&record.metadata).map_err(|_| conflict(&record.msg_id))?;
    let operation = metadata["operation_id"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| conflict(&record.msg_id))?;
    // One operation cannot acquire a second message ID, even in another process.
    let other_message: Option<String> = transaction
        .query_row(
            "SELECT msg_id FROM messages WHERE owner_identity_id = ?1 AND direction = 1
         AND wire_thread_kind = 'direct' AND is_e2ee = 0
         AND json_valid(metadata) AND json_extract(metadata, '$.operation_id') = ?2
         AND msg_id <> ?3 LIMIT 1",
            (&record.owner_identity_id, operation, &record.msg_id),
            |row| row.get(0),
        )
        .optional()
        .map_err(super::super::local_state_unavailable)?;
    if other_message.is_some() {
        return Err(conflict(&record.msg_id));
    }
    let existing = super::existing_outgoing_direct_wire_snapshot(
        &transaction,
        &record.owner_identity_id,
        &record.owner_did,
        &record.conversation_id,
        &record.msg_id,
    )?;
    let intent = if let Some(existing) = existing {
        let (content_type, content, e2ee, old_metadata): (String, String, bool, String) =
            transaction
                .query_row(
                    "SELECT content_type, content, is_e2ee, metadata FROM messages
             WHERE owner_identity_id = ?1 AND msg_id = ?2",
                    (&record.owner_identity_id, &record.msg_id),
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                )
                .map_err(super::super::local_state_unavailable)?;
        let old_metadata: Value =
            serde_json::from_str(&old_metadata).map_err(|_| conflict(&record.msg_id))?;
        if content_type != record.content_type
            || content != record.content
            || e2ee
            || old_metadata["operation_id"] != metadata["operation_id"]
        {
            return Err(conflict(&record.msg_id));
        }
        DirectSendIntent {
            target_did: existing.target_did,
            created_at: existing
                .created_at
                .filter(|s| !s.is_empty())
                .ok_or_else(|| conflict(&record.msg_id))?,
        }
    } else {
        let created_at = metadata["wire_created_at"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| conflict(&record.msg_id))?
            .to_owned();
        super::upsert_message(&transaction, &record)?;
        DirectSendIntent {
            target_did: record.receiver_did,
            created_at,
        }
    };
    transaction
        .commit()
        .map_err(super::super::local_state_unavailable)?;
    Ok(intent)
}

fn conflict(message_id: &str) -> crate::ImError {
    crate::ImError::MessageWireIdentityConflict {
        message_id: message_id.to_owned(),
    }
}

#[cfg(test)]
#[path = "direct_send_intent_tests.rs"]
mod tests;
