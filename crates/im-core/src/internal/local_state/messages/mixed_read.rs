use crate::messages::ReadWatermark;
use rusqlite::Connection;

pub(crate) struct Plan {
    pub ordinary: Option<ReadWatermark>,
    pub effective: ReadWatermark,
    pub local_ids: Vec<String>,
}

struct Row {
    id: String,
    seq: Option<i64>,
    at: String,
    encrypted: bool,
    p5: bool,
    incoming: bool,
    unread: bool,
}

impl Row {
    fn key(
        &self,
    ) -> (
        Option<chrono::DateTime<chrono::FixedOffset>>,
        bool,
        i64,
        &str,
    ) {
        super::super::conversation_summaries::order_key(
            &self.at,
            self.encrypted,
            "",
            self.seq,
            &self.id,
        )
    }

    fn watermark(&self) -> crate::ImResult<ReadWatermark> {
        Ok(ReadWatermark {
            last_read_message_id: Some(crate::ids::MessageId::parse(&self.id)?),
            last_read_thread_seq: (!self.p5)
                .then_some(self.seq)
                .flatten()
                .map(|n| n.to_string()),
            read_at: chrono::DateTime::parse_from_rfc3339(&self.at)
                .ok()
                .map(|t| t.to_utc()),
        })
    }
}

/// Select exact local IDs before network work; future arrivals are never read by this plan.
pub(crate) fn prepare(
    db: &Connection,
    owner: &str,
    conversation: &str,
    requested: Option<&ReadWatermark>,
) -> crate::ImResult<Option<Plan>> {
    if !conversation.starts_with("dm:") {
        return Ok(None);
    }
    // A sequence-only request belongs to the ordinary lane, never the P5 inbox.
    if requested
        .is_some_and(|w| w.last_read_message_id.is_none() && w.last_read_thread_seq.is_some())
    {
        return Ok(None);
    }
    let mut statement = db
        .prepare(
            "SELECT msg_id, server_seq, COALESCE(NULLIF(sent_at, ''), stored_at),
                COALESCE(is_e2ee, 0), metadata, direction, COALESCE(is_read, 0)
         FROM messages WHERE owner_identity_id = ?1
         AND COALESCE(NULLIF(conversation_id, ''), thread_id) = ?2
         AND hydration_state = 'hydrated'",
        )
        .map_err(super::super::local_state_unavailable)?;
    let rows = statement
        .query_map([owner, conversation], |r| {
            let metadata = r.get::<_, Option<String>>(4)?.unwrap_or_default();
            Ok(Row {
                id: r.get(0)?,
                seq: r.get(1)?,
                at: r.get(2)?,
                encrypted: r.get(3)?,
                p5: super::authenticated_p5_v2_raw_message_id(&metadata).is_some(),
                incoming: r.get::<_, i64>(5)? == 0,
                unread: r.get::<_, i64>(6)? == 0,
            })
        })
        .map_err(super::super::local_state_unavailable)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(super::super::local_state_unavailable)?;
    if !rows.iter().any(|r| r.p5) {
        return Ok(None);
    }
    let anchor = if let Some(id) =
        requested.and_then(|w| w.last_read_message_id.as_ref().map(|id| id.as_str()))
    {
        let canonical = super::canonical_message_id_for_request(db, owner, id)?;
        rows.iter()
            .find(|r| r.id == canonical)
            .ok_or_else(|| crate::ImError::MessageNotFound {
                message_id: id.to_owned(),
            })?
    } else {
        let Some(anchor) = rows.iter().max_by_key(|r| r.key()) else {
            return Ok(None);
        };
        anchor
    };
    let eligible = rows
        .iter()
        .filter(|r| r.incoming && r.key() <= anchor.key());
    let local_ids = eligible
        .clone()
        .filter(|r| r.p5 && r.unread)
        .map(|r| r.id.clone())
        .collect();
    let ordinary = eligible
        .filter(|r| !r.p5 && r.seq.is_some_and(|s| s > 0))
        .max_by_key(|r| r.seq)
        .map(Row::watermark)
        .transpose()?;
    Ok(Some(Plan {
        ordinary,
        effective: anchor.watermark()?,
        local_ids,
    }))
}
