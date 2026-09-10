//! One durable receive queue for ordinary, P5 and P6 events.
//!
//! Receive checkpoints and business completion are independent. This module
//! owns queue claims and retention; domain reducers own business facts.

use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use super::{local_state_unavailable, sync_v2};

/// One full compact snapshot admits up to 10,000 items. Leave room for its
/// local baseline marker while keeping one finite database-wide item budget.
pub(crate) const MAX_INBOX_RECORDS: i64 = 16_384;
pub(crate) const RETENTION_SECONDS: i64 = 48 * 60 * 60;
pub(crate) const CLAIM_SECONDS: i64 = 60;
pub(crate) const MAX_ACTIVE_INPUTS: usize = 8;

#[derive(Debug, Clone)]
pub(crate) struct InboxEvent {
    pub(crate) event_id: String,
    pub(crate) position: String,
    pub(crate) event_type: String,
    pub(crate) payload: Value,
    pub(crate) processing_scope: String,
    pub(crate) group_did: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct OrdinaryReceiveBatch {
    pub(crate) binding: sync_v2::IdentityAccountBinding,
    pub(crate) client_instance_id: String,
    pub(crate) expected_run_generation: Option<i64>,
    pub(crate) stream_epoch: String,
    pub(crate) expected_scan_seq: String,
    pub(crate) next_scan_seq: String,
    pub(crate) server_time: String,
    pub(crate) events: Vec<InboxEvent>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ReceiveOutcome {
    pub(crate) received: usize,
    pub(crate) duplicates: usize,
    pub(crate) removed: Vec<RemovedInput>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ProcessingSummary {
    pub(crate) pending: usize,
    pub(crate) active: usize,
    pub(crate) blocked: usize,
    pub(crate) next_retry_at: Option<i64>,
    pub(crate) error_code: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct InputClaim {
    pub(crate) input_id: String,
    pub(crate) owner_identity_id: String,
    pub(crate) owner_did: String,
    pub(crate) account_id: String,
    pub(crate) device_id: String,
    pub(crate) auth_generation: String,
    pub(crate) lane: String,
    pub(crate) lane_epoch: String,
    pub(crate) position: String,
    pub(crate) event_id: String,
    pub(crate) event_type: String,
    pub(crate) payload: Value,
    pub(crate) group_did: Option<String>,
    pub(crate) logical_event_seq: Option<String>,
    pub(crate) received_at: String,
    pub(crate) source_created_at: Option<String>,
    pub(crate) source_expires_at: Option<String>,
    pub(crate) token: String,
    pub(crate) attempt_count: i64,
}

impl InputClaim {
    pub(crate) fn secure_input(&self) -> crate::ImResult<sync_v2::SyncLaneInboxRecord> {
        let lane = match self.lane.as_str() {
            "p5_device" => crate::internal::wire::sync_v2::SyncLaneV3::P5Device,
            "p6_group" => crate::internal::wire::sync_v2::SyncLaneV3::P6Group,
            _ => {
                return Err(error(
                    "SYNC_INPUT_TYPE_INVALID",
                    "ordinary input is not a secure lane",
                ))
            }
        };
        Ok(sync_v2::SyncLaneInboxRecord {
            input_id: self.input_id.clone(),
            owner_identity_id: self.owner_identity_id.clone(),
            lane,
            lane_epoch: self.lane_epoch.clone(),
            position: self.position.clone(),
            event_id: self.event_id.clone(),
            event_type: self.event_type.clone(),
            raw_payload: self.payload.clone(),
            group_did: self.group_did.clone(),
            received_at: self.received_at.clone(),
            source_created_at: self.source_created_at.clone(),
            source_expires_at: self.source_expires_at.clone(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RemovedInput {
    pub(crate) input_id: String,
    pub(crate) owner_identity_id: String,
    pub(crate) event_id: String,
}

/// Called inside the owning schema transaction. Child outcome rows are
/// preserved explicitly so rebuilding the constrained inbox cannot cascade
/// away pending secure-domain responsibilities.
pub(crate) fn ensure_schema(db: &Connection) -> crate::ImResult<()> {
    let has_attempts = has_column(db, "sync_lane_inbox", "attempt_token")?;
    let ddl: String = db
        .query_row(
            "SELECT sql FROM sqlite_master WHERE name='sync_lane_inbox'",
            [],
            |row| row.get(0),
        )
        .map_err(local_state_unavailable)?;
    if !has_attempts || !ddl.contains("'baseline'") {
        let table_ddl = sync_v2::SYNC_V2_SCHEMA_SQL
            .split("CREATE TABLE IF NOT EXISTS sync_lane_inbox (")
            .nth(1)
            .and_then(|tail| tail.split_once("\n);"))
            .map(|(body, _)| format!("CREATE TABLE sync_lane_inbox_v45 ({body}\n);"))
            .ok_or_else(|| error("SYNC_INBOX_SCHEMA_INVALID", "inbox DDL is missing"))?;
        db.execute_batch(&table_ddl)
            .map_err(local_state_unavailable)?;
        if has_attempts {
            if !has_column(db, "sync_lane_inbox", "logical_event_seq")? {
                db.execute_batch("ALTER TABLE sync_lane_inbox ADD COLUMN logical_event_seq TEXT")
                    .map_err(local_state_unavailable)?;
            }
            db.execute_batch("CREATE TEMP TABLE sync_inbox_attempts_v45 AS SELECT logical_event_seq, input_id, processing_scope, attempt_count, attempt_token, attempt_deadline, last_attempt_at, next_attempt_at, processing_error_code FROM sync_lane_inbox").map_err(local_state_unavailable)?;
        }
        db.execute_batch(
            "INSERT INTO sync_lane_inbox_v45(
                input_id, owner_identity_id, lane, lane_epoch, position,
                event_id, event_type, raw_payload_json, payload_bytes,
                account_id_snapshot, device_id_snapshot, auth_generation_snapshot,
                client_instance_id_snapshot, group_did, received_at,
                source_created_at, source_expires_at, closed_at, created_at
             ) SELECT input_id, owner_identity_id, lane, lane_epoch, position,
                event_id, event_type, raw_payload_json, payload_bytes,
                account_id_snapshot, device_id_snapshot, auth_generation_snapshot,
                client_instance_id_snapshot, group_did, received_at,
                source_created_at, source_expires_at, closed_at, created_at
             FROM sync_lane_inbox;
             CREATE TEMP TABLE sync_p5_outcomes_v45 AS SELECT * FROM sync_p5_input_outcomes;
             CREATE TEMP TABLE sync_p6_outcomes_v45 AS SELECT * FROM sync_p6_input_outcomes;
             DROP TABLE sync_p5_input_outcomes;
             DROP TABLE sync_p6_input_outcomes;
             DROP TABLE sync_lane_inbox;
             ALTER TABLE sync_lane_inbox_v45 RENAME TO sync_lane_inbox;",
        )
        .map_err(local_state_unavailable)?;
        db.execute_batch(sync_v2::SYNC_V2_SCHEMA_SQL)
            .map_err(local_state_unavailable)?;
        db.execute_batch(
            "INSERT INTO sync_p5_input_outcomes SELECT * FROM sync_p5_outcomes_v45;
             INSERT INTO sync_p6_input_outcomes SELECT * FROM sync_p6_outcomes_v45;
             DROP TABLE sync_p5_outcomes_v45;
             DROP TABLE sync_p6_outcomes_v45;
             UPDATE sync_lane_inbox SET
                processing_scope = COALESCE(group_did, json_extract(raw_payload_json, '$.meta.sender_did'), event_id),
                attempt_count = COALESCE(
                    (SELECT attempt_count FROM sync_p5_input_outcomes WHERE input_id=sync_lane_inbox.input_id),
                    (SELECT attempt_count FROM sync_p6_input_outcomes WHERE input_id=sync_lane_inbox.input_id), 0),
                next_attempt_at = COALESCE(
                    (SELECT next_retry_at FROM sync_p5_input_outcomes WHERE input_id=sync_lane_inbox.input_id),
                    (SELECT next_retry_at FROM sync_p6_input_outcomes WHERE input_id=sync_lane_inbox.input_id), 0);",
        )
        .map_err(local_state_unavailable)?;
    }
    if has_attempts && !ddl.contains("'baseline'") {
        db.execute_batch("UPDATE sync_lane_inbox SET (logical_event_seq, processing_scope, attempt_count, attempt_token, attempt_deadline, last_attempt_at, next_attempt_at, processing_error_code) = (SELECT logical_event_seq, processing_scope, attempt_count, attempt_token, attempt_deadline, last_attempt_at, next_attempt_at, processing_error_code FROM sync_inbox_attempts_v45 WHERE input_id=sync_lane_inbox.input_id); DROP TABLE sync_inbox_attempts_v45;").map_err(local_state_unavailable)?;
    }
    db.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_sync_inbox_due
         ON sync_lane_inbox(owner_identity_id, closed_at, next_attempt_at, attempt_deadline);
         CREATE INDEX IF NOT EXISTS idx_sync_inbox_retention
         ON sync_lane_inbox(created_at, input_id);",
    )
    .map_err(local_state_unavailable)?;
    if !has_column(db, "sync_lane_inbox", "logical_event_seq")? {
        db.execute_batch("ALTER TABLE sync_lane_inbox ADD COLUMN logical_event_seq TEXT;
            UPDATE sync_lane_inbox SET logical_event_seq=CAST(json_extract(raw_payload_json,'$.body.group_event_seq') AS TEXT)
            WHERE lane='p6_group';").map_err(local_state_unavailable)?;
    }
    // Physical attempts can outlive eviction of their payload. Keep their
    // bounded occupancy separately, without payload or a second task queue.
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS sync_input_leases (
        attempt_token TEXT PRIMARY KEY, deadline INTEGER NOT NULL
    );
    INSERT OR IGNORE INTO sync_input_leases(attempt_token,deadline)
    SELECT attempt_token,attempt_deadline FROM sync_lane_inbox WHERE attempt_token IS NOT NULL;",
    )
    .map_err(local_state_unavailable)?;
    for table in ["sync_applied_events", "sync_lane_applied_events"] {
        if !has_column(db, table, "payload_hash")? {
            db.execute_batch(&format!("ALTER TABLE {table} ADD COLUMN payload_hash TEXT"))
                .map_err(local_state_unavailable)?;
        }
    }
    Ok(())
}

pub(crate) fn receive_ordinary(
    db: &Connection,
    input: OrdinaryReceiveBatch,
    now: i64,
) -> crate::ImResult<ReceiveOutcome> {
    use std::collections::BTreeSet;
    let transaction = Transaction::new_unchecked(db, TransactionBehavior::Immediate)
        .map_err(local_state_unavailable)?;
    require_binding(&transaction, &input.binding)?;
    sync_v2::require_message_sync_run_generation(
        &transaction,
        &input.binding.owner_identity_id,
        input.expected_run_generation,
    )?;
    let installed: Option<String> = transaction
        .query_row(
            "SELECT client_instance_id FROM sync_installation_state WHERE owner_identity_id=?1",
            [&input.binding.owner_identity_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(local_state_unavailable)?;
    if installed.as_deref() != Some(&input.client_instance_id) {
        return Err(error(
            "SYNC_INSTALLATION_CHANGED",
            "receive installation is no longer current",
        ));
    }
    let current =
        match sync_v2::load_message_sync_state(&transaction, &input.binding.owner_identity_id)? {
            sync_v2::MessageSyncStateAccess::Ready(state) => state,
            _ => {
                return Err(error(
                    "SYNC_BOOTSTRAP_REQUIRED",
                    "receive requires a bootstrapped cursor",
                ))
            }
        };
    if current.stream_epoch != input.stream_epoch || current.scan_seq != input.expected_scan_seq {
        return Err(error(
            "SYNC_RECEIVE_SUPERSEDED",
            "receive cursor changed before commit",
        ));
    }
    if sync_v2::compare_decimal(&input.next_scan_seq, &current.scan_seq)?.is_lt() {
        return Err(error(
            "SYNC_CURSOR_REGRESSION",
            "receive cursor cannot move backwards",
        ));
    }
    let mut event_ids = BTreeSet::new();
    let mut positions = BTreeSet::new();
    let mut pending = Vec::new();
    let mut outcome = ReceiveOutcome::default();
    for event in &input.events {
        if event.event_id.trim().is_empty()
            || event.event_id.trim() != event.event_id
            || event.event_type.trim().is_empty()
            || !event.payload.is_object()
            || !event_ids.insert(&event.event_id)
            || !positions.insert(&event.position)
        {
            return Err(error(
                "SYNC_INVALID_PAGE",
                "receive events have invalid or duplicate identities",
            ));
        }
        sync_v2::validate_positive_decimal("event_seq", &event.position)?;
        if sync_v2::compare_decimal(&event.position, &input.next_scan_seq)?.is_gt() {
            return Err(error(
                "SYNC_INVALID_PAGE",
                "received event is ahead of the page cursor",
            ));
        }
        let envelope: crate::internal::wire::sync_v2::SyncEventV2 =
            serde_json::from_value(event.payload.get("event").cloned().ok_or_else(|| {
                error(
                    "SYNC_INPUT_INCOMPLETE",
                    "ordinary receive envelope is missing",
                )
            })?)
            .map_err(|_| {
                error(
                    "SYNC_INPUT_INCOMPLETE",
                    "ordinary receive envelope is incomplete",
                )
            })?;
        if envelope.event_id != event.event_id
            || envelope.event_seq != event.position
            || envelope.event_type != event.event_type
            || envelope.stream_epoch != input.stream_epoch
            || envelope.account_id != input.binding.account_id
            || envelope
                .recipient_device_id
                .as_deref()
                .is_some_and(|device| device != input.binding.protocol_device_id)
            || (matches!(
                event.event_type.as_str(),
                "message.created" | "system.notification"
            ) && !event.payload.get("hydrated").is_some_and(Value::is_object))
        {
            return Err(error(
                "SYNC_INPUT_INCOMPLETE",
                "ordinary receive data does not match its exact envelope",
            ));
        }
        if input_is_duplicate(
            &transaction,
            &input.binding.owner_identity_id,
            "ordinary",
            &input.stream_epoch,
            event,
        )? {
            outcome.duplicates += 1;
        } else {
            pending.push(event);
        }
    }
    outcome.removed = make_room(&transaction, pending.len(), now)?;
    for event in pending {
        insert_input(
            &transaction,
            &input.binding,
            &input.client_instance_id,
            "ordinary",
            &input.stream_epoch,
            event,
            now,
        )?;
        outcome.received += 1;
    }
    let next = sync_v2::MessageSyncState {
        scan_seq: input.next_scan_seq,
        last_server_time: Some(input.server_time),
        last_success_at: Some(now),
        last_error_code: None,
        updated_at: now,
        ..current
    };
    if !matches!(
        sync_v2::advance_message_sync_state(&transaction, &next)?,
        sync_v2::MessageSyncStateAccess::Ready(_)
    ) {
        return Err(error(
            "SYNC_RECEIVE_SUPERSEDED",
            "receive binding changed before commit",
        ));
    }
    transaction.commit().map_err(local_state_unavailable)?;
    Ok(outcome)
}

pub(crate) fn input_is_duplicate(
    db: &Connection,
    owner: &str,
    lane: &str,
    epoch: &str,
    event: &InboxEvent,
) -> crate::ImResult<bool> {
    let existing = db
        .query_row(
            "SELECT position, event_type, raw_payload_json FROM sync_lane_inbox
         WHERE owner_identity_id=?1 AND lane=?2 AND lane_epoch=?3 AND event_id=?4",
            params![owner, lane, epoch, event.event_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .optional()
        .map_err(local_state_unavailable)?;
    if let Some((position, kind, payload)) = existing {
        let payload: Value = serde_json::from_str(&payload)
            .map_err(|_| error("SYNC_INPUT_CONFLICT", "stored receive payload is invalid"))?;
        if position != event.position
            || kind != event.event_type
            || payload_hash(lane, &payload)? != payload_hash(lane, &event.payload)?
        {
            return Err(error(
                "SYNC_INPUT_CONFLICT",
                "event identity conflicts with an existing receive payload",
            ));
        }
        return Ok(true);
    }
    let stored_position: Option<String> = db.query_row(
        "SELECT event_id FROM sync_lane_inbox WHERE owner_identity_id=?1 AND lane=?2 AND lane_epoch=?3 AND position=?4",
        params![owner, lane, epoch, event.position], |row| row.get(0),
    ).optional().map_err(local_state_unavailable)?;
    if stored_position.is_some() {
        return Err(error(
            "SYNC_INPUT_CONFLICT",
            "receive position belongs to another event",
        ));
    }
    let stored_hash: Option<Option<String>> = if matches!(lane, "ordinary" | "baseline") {
        db.query_row(
            "SELECT payload_hash FROM sync_applied_events WHERE owner_identity_id=?1 AND event_id=?2 AND stream_epoch=?3",
            params![owner, event.event_id, epoch], |row| row.get(0),
        ).optional().map_err(local_state_unavailable)?
    } else {
        db.query_row(
            "SELECT payload_hash FROM sync_lane_applied_events WHERE owner_identity_id=?1 AND lane=?2 AND event_id=?3 AND stream_epoch=?4",
            params![owner, lane, event.event_id, epoch], |row| row.get(0),
        ).optional().map_err(local_state_unavailable)?
    };
    match stored_hash.flatten() {
        Some(hash) if hash == payload_hash(lane, &event.payload)? => Ok(true),
        Some(_) => Err(error(
            "SYNC_INPUT_CONFLICT",
            "completed event was redelivered with a different payload",
        )),
        // Older receipts have no receive digest. Preserve input for the existing
        // idempotent reducer instead of inventing a historical payload hash.
        None => Ok(false),
    }
}

pub(crate) fn insert_input(
    db: &Connection,
    binding: &sync_v2::IdentityAccountBinding,
    installation: &str,
    lane: &str,
    epoch: &str,
    event: &InboxEvent,
    now: i64,
) -> crate::ImResult<()> {
    let payload = serde_json::to_string(&event.payload)
        .map_err(|_| error("SYNC_INPUT_INVALID", "receive payload cannot be encoded"))?;
    let input_id = serde_json::to_string(&json!([
        binding.owner_identity_id,
        lane,
        epoch,
        event.event_id
    ]))
    .map_err(|_| error("SYNC_INPUT_INVALID", "receive identity cannot be encoded"))?;
    let received_at = chrono::DateTime::from_timestamp(now, 0)
        .ok_or_else(|| error("SYNC_INPUT_INVALID", "receive time is invalid"))?
        .to_rfc3339();
    db.execute(
        "INSERT INTO sync_lane_inbox(
            input_id, owner_identity_id, lane, lane_epoch, position, event_id, event_type,
            raw_payload_json, payload_bytes, account_id_snapshot, device_id_snapshot,
            auth_generation_snapshot, client_instance_id_snapshot, group_did,
            received_at, created_at, processing_scope
         ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)",
        params![
            input_id,
            binding.owner_identity_id,
            lane,
            epoch,
            event.position,
            event.event_id,
            event.event_type,
            payload,
            payload.len() as i64,
            binding.account_id,
            binding.protocol_device_id,
            binding.device_auth_generation,
            installation,
            event.group_did,
            received_at,
            now,
            event.processing_scope
        ],
    )
    .map_err(local_state_unavailable)?;
    Ok(())
}

pub(crate) fn payload_hash(lane: &str, payload: &Value) -> crate::ImResult<String> {
    // Server time and the local replay source are not immutable event content.
    let immutable = if lane == "ordinary" && payload.get("event").is_some() {
        json!([payload.get("event"), payload.get("hydrated")])
    } else {
        payload.clone()
    };
    let bytes = serde_json_canonicalizer::to_vec(&immutable).map_err(|_| {
        error(
            "SYNC_INPUT_INVALID",
            "receive payload cannot be canonicalized",
        )
    })?;
    Ok(Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

pub(crate) fn claim_inputs(
    db: &Connection,
    binding: &sync_v2::IdentityAccountBinding,
    now: i64,
    limit: u32,
) -> crate::ImResult<Vec<InputClaim>> {
    if limit == 0 || limit > 64 {
        return Err(crate::ImError::invalid_input(
            Some("limit".to_owned()),
            "claim limit must be between 1 and 64",
        ));
    }
    let transaction = Transaction::new_unchecked(db, TransactionBehavior::Immediate)
        .map_err(local_state_unavailable)?;
    require_binding(&transaction, binding)?;
    transaction
        .execute("DELETE FROM sync_input_leases WHERE deadline<=?1", [now])
        .map_err(local_state_unavailable)?;
    let leased: usize = transaction
        .query_row(
            "SELECT COUNT(*) FROM sync_input_leases WHERE deadline>?1",
            [now],
            |row| row.get(0),
        )
        .map_err(local_state_unavailable)?;
    let limit = limit.min(MAX_ACTIVE_INPUTS.saturating_sub(leased) as u32);
    if limit == 0 {
        return Ok(Vec::new());
    }
    // Round-robin lanes first and then independent conversation scopes. Retry
    // attempts sort behind untried work within a scope and never sleep here.
    let mut statement = transaction.prepare(
        "WITH due AS (
            SELECT *, ROW_NUMBER() OVER (
                PARTITION BY lane, processing_scope
                ORDER BY attempt_count, COALESCE(last_attempt_at,0), created_at, input_id
            ) AS scope_rank
            FROM sync_lane_inbox AS candidate
            WHERE owner_identity_id=?1 AND account_id_snapshot=?2 AND device_id_snapshot=?3
              AND closed_at IS NULL AND created_at>?4
              AND next_attempt_at IS NOT NULL AND next_attempt_at<=?5
              AND (attempt_token IS NULL OR attempt_deadline<=?5)
              AND (lane NOT IN ('ordinary','baseline') OR NOT EXISTS (
                SELECT 1 FROM sync_lane_inbox AS baseline
                WHERE baseline.owner_identity_id=candidate.owner_identity_id
                  AND baseline.lane='baseline' AND baseline.closed_at IS NULL AND baseline.created_at>?4
                  AND (candidate.lane='ordinary' OR baseline.rowid<candidate.rowid)
              ))
         ), fair AS (
            SELECT *, ROW_NUMBER() OVER (
                PARTITION BY lane ORDER BY scope_rank, attempt_count, COALESCE(last_attempt_at,0), created_at, input_id
            ) AS lane_rank FROM due
         )
         SELECT input_id, lane, lane_epoch, position, event_id, event_type,
                raw_payload_json, group_did, received_at, source_created_at,
                source_expires_at, attempt_count, logical_event_seq
         FROM fair ORDER BY lane_rank, lane LIMIT ?6",
    ).map_err(local_state_unavailable)?;
    let rows = statement
        .query_map(
            params![
                binding.owner_identity_id,
                binding.account_id,
                binding.protocol_device_id,
                now.saturating_sub(RETENTION_SECONDS),
                now,
                limit
            ],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, Option<String>>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, Option<String>>(9)?,
                    row.get::<_, Option<String>>(10)?,
                    row.get::<_, i64>(11)?,
                    row.get::<_, Option<String>>(12)?,
                ))
            },
        )
        .map_err(local_state_unavailable)?;
    let candidates = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(local_state_unavailable)?;
    drop(statement);
    let mut claims = Vec::new();
    for (
        input_id,
        lane,
        lane_epoch,
        position,
        event_id,
        event_type,
        raw,
        group_did,
        received_at,
        source_created_at,
        source_expires_at,
        attempts,
        logical_event_seq,
    ) in candidates
    {
        let payload = serde_json::from_str(&raw)
            .map_err(|_| error("SYNC_INPUT_INVALID", "stored receive payload is invalid"))?;
        let token = crate::internal::wire::common::generate_operation_id();
        transaction
            .execute(
                "INSERT INTO sync_input_leases(attempt_token,deadline) VALUES(?1,?2)",
                params![token, now.saturating_add(CLAIM_SECONDS)],
            )
            .map_err(local_state_unavailable)?;
        transaction
            .execute(
            "UPDATE sync_lane_inbox SET attempt_token=?2, attempt_deadline=?3,
             attempt_count=attempt_count+1, last_attempt_at=?4, processing_error_code=NULL WHERE input_id=?1",
                params![input_id, token, now.saturating_add(CLAIM_SECONDS), now],
            )
            .map_err(local_state_unavailable)?;
        claims.push(InputClaim {
            input_id,
            lane,
            lane_epoch,
            position,
            event_id,
            event_type,
            payload,
            group_did,
            received_at,
            logical_event_seq,
            source_created_at,
            source_expires_at,
            token,
            owner_identity_id: binding.owner_identity_id.clone(),
            owner_did: binding.current_did.clone(),
            account_id: binding.account_id.clone(),
            device_id: binding.protocol_device_id.clone(),
            auth_generation: binding.device_auth_generation.clone(),
            attempt_count: attempts.saturating_add(1),
        });
    }
    transaction.commit().map_err(local_state_unavailable)?;
    Ok(claims)
}

pub(crate) fn require_claim(db: &Connection, claim: &InputClaim, now: i64) -> crate::ImResult<()> {
    let live: bool = db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sync_lane_inbox AS input
            JOIN identity_account_bindings AS binding USING(owner_identity_id)
            JOIN sync_input_leases AS lease ON lease.attempt_token=input.attempt_token
            WHERE lease.deadline>?4 AND input.input_id=?1 AND input.owner_identity_id=?2 AND input.attempt_token=?3
              AND input.attempt_deadline>?4
              AND (input.processing_error_code IS NULL OR input.processing_error_code<>'sync.processing_timeout')
              AND binding.account_id=?5 AND binding.device_id=?6
              AND binding.device_auth_generation=?7 AND binding.current_did=?8)",
            params![
                claim.input_id,
                claim.owner_identity_id,
                claim.token,
                now,
                claim.account_id,
                claim.device_id,
                claim.auth_generation,
                claim.owner_did
            ],
            |row| row.get(0),
        )
        .map_err(local_state_unavailable)?;
    if !live {
        return Err(error(
            "SYNC_INPUT_SUPERSEDED",
            "receive input was removed or its processing attempt is stale",
        ));
    }
    Ok(())
}

pub(crate) fn fail_claim(
    db: &Connection,
    claim: &InputClaim,
    code: &str,
    retry_at: Option<i64>,
    now: i64,
) -> crate::ImResult<()> {
    let transaction = Transaction::new_unchecked(db, TransactionBehavior::Immediate)
        .map_err(local_state_unavailable)?;
    require_claim(&transaction, claim, now)?;
    transaction
        .execute(
            "UPDATE sync_lane_inbox SET attempt_token=NULL, attempt_deadline=NULL, closed_at=NULL,
            next_attempt_at=?2, processing_error_code=?3 WHERE input_id=?1",
            params![claim.input_id, retry_at, code],
        )
        .map_err(local_state_unavailable)?;
    release_claim_lease(&transaction, claim)?;
    transaction.commit().map_err(local_state_unavailable)
}

/// Park the logical attempt without freeing its actual executing task. The
/// coordinator continues to renew this lease until the underlying call ends.
pub(crate) fn park_timeout(db: &Connection, claim: &InputClaim, now: i64) -> crate::ImResult<bool> {
    let changed = db.execute(
        "UPDATE sync_lane_inbox SET next_attempt_at=NULL, processing_error_code='sync.processing_timeout',
             attempt_deadline=?4, closed_at=NULL
         WHERE input_id=?1 AND owner_identity_id=?2 AND attempt_token=?3",
        params![claim.input_id, claim.owner_identity_id, claim.token, now.saturating_add(CLAIM_SECONDS)],
    ).map_err(local_state_unavailable)?;
    Ok(changed == 1)
}

pub(crate) fn release_timeout(
    db: &Connection,
    claim: &InputClaim,
    retry_at: i64,
) -> crate::ImResult<bool> {
    let changed = db.execute(
        "UPDATE sync_lane_inbox SET attempt_token=NULL, attempt_deadline=NULL, next_attempt_at=?4
         WHERE input_id=?1 AND owner_identity_id=?2 AND attempt_token=?3
           AND processing_error_code='sync.processing_timeout'",
        params![claim.input_id, claim.owner_identity_id, claim.token, retry_at],
    ).map_err(local_state_unavailable)?;
    Ok(changed == 1)
}

pub(crate) fn renew_claims(
    db: &Connection,
    claims: &[InputClaim],
    now: i64,
) -> crate::ImResult<()> {
    let transaction = Transaction::new_unchecked(db, TransactionBehavior::Immediate)
        .map_err(local_state_unavailable)?;
    for claim in claims {
        transaction
            .execute(
                "UPDATE sync_input_leases SET deadline=?2 WHERE attempt_token=?1",
                params![claim.token, now.saturating_add(CLAIM_SECONDS)],
            )
            .map_err(local_state_unavailable)?;
        transaction
            .execute(
                "UPDATE sync_lane_inbox SET attempt_deadline=?4
             WHERE input_id=?1 AND owner_identity_id=?2 AND attempt_token=?3",
                params![
                    claim.input_id,
                    claim.owner_identity_id,
                    claim.token,
                    now.saturating_add(CLAIM_SECONDS)
                ],
            )
            .map_err(local_state_unavailable)?;
    }
    transaction.commit().map_err(local_state_unavailable)
}

pub(crate) fn release_claim_lease(db: &Connection, claim: &InputClaim) -> crate::ImResult<()> {
    release_processing_slot(db, &claim.token)
}

pub(crate) fn release_processing_slot(db: &Connection, token: &str) -> crate::ImResult<()> {
    db.execute(
        "DELETE FROM sync_input_leases WHERE attempt_token=?1",
        [token],
    )
    .map_err(local_state_unavailable)?;
    Ok(())
}

pub(crate) fn reserve_processing_slot(
    db: &Connection,
    now: i64,
) -> crate::ImResult<Option<String>> {
    let tx = Transaction::new_unchecked(db, TransactionBehavior::Immediate)
        .map_err(local_state_unavailable)?;
    tx.execute("DELETE FROM sync_input_leases WHERE deadline<=?1", [now])
        .map_err(local_state_unavailable)?;
    let count: usize = tx
        .query_row("SELECT COUNT(*) FROM sync_input_leases", [], |row| {
            row.get(0)
        })
        .map_err(local_state_unavailable)?;
    if count >= MAX_ACTIVE_INPUTS {
        return Ok(None);
    }
    let token = crate::internal::wire::common::generate_operation_id();
    tx.execute(
        "INSERT INTO sync_input_leases(attempt_token,deadline) VALUES(?1,?2)",
        params![token, now.saturating_add(CLAIM_SECONDS)],
    )
    .map_err(local_state_unavailable)?;
    tx.commit().map_err(local_state_unavailable)?;
    Ok(Some(token))
}

pub(crate) fn renew_processing_slots(
    db: &Connection,
    tokens: &[String],
    now: i64,
) -> crate::ImResult<()> {
    for token in tokens {
        db.execute(
            "UPDATE sync_input_leases SET deadline=?2 WHERE attempt_token=?1",
            params![token, now.saturating_add(CLAIM_SECONDS)],
        )
        .map_err(local_state_unavailable)?;
    }
    Ok(())
}

pub(crate) fn processing_summary(
    db: &Connection,
    owner: &str,
    now: i64,
) -> crate::ImResult<ProcessingSummary> {
    db.query_row(
        "SELECT COUNT(*), COALESCE(SUM(attempt_token IS NOT NULL),0),
            COALESCE(SUM(next_attempt_at IS NULL AND attempt_token IS NULL),0),
            MIN(CASE WHEN next_attempt_at>?2 THEN next_attempt_at END),
            MIN(CASE WHEN next_attempt_at IS NULL THEN processing_error_code END)
         FROM sync_lane_inbox WHERE owner_identity_id=?1 AND closed_at IS NULL",
        params![owner, now],
        |row| {
            Ok(ProcessingSummary {
                pending: row.get(0)?,
                active: row.get(1)?,
                blocked: row.get(2)?,
                next_retry_at: row.get(3)?,
                error_code: row.get(4)?,
            })
        },
    )
    .map_err(local_state_unavailable)
}

pub(crate) fn complete_claim(db: &Connection, claim: &InputClaim, now: i64) -> crate::ImResult<()> {
    require_claim(db, claim, now)?;
    let hash = payload_hash(&claim.lane, &claim.payload)?;
    if matches!(claim.lane.as_str(), "ordinary" | "baseline") {
        db.execute("UPDATE sync_applied_events SET payload_hash=?3 WHERE owner_identity_id=?1 AND event_id=?2",
            params![claim.owner_identity_id, claim.event_id, hash]).map_err(local_state_unavailable)?;
    } else {
        let logical_sequence = claim
            .payload
            .pointer("/body/group_event_seq")
            .and_then(|value| {
                value
                    .as_str()
                    .map(str::to_owned)
                    .or_else(|| value.as_u64().map(|sequence| sequence.to_string()))
            })
            .or_else(|| claim.logical_event_seq.clone());
        db.execute(
            "INSERT INTO sync_lane_applied_events(owner_identity_id, lane, event_id, stream_epoch,
                event_seq, group_did, group_event_seq, applied_at, payload_hash)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)
             ON CONFLICT(owner_identity_id,lane,event_id) DO UPDATE SET payload_hash=excluded.payload_hash",
            params![claim.owner_identity_id, claim.lane, claim.event_id, claim.lane_epoch, claim.position,
                claim.group_did, logical_sequence, now, hash],
        ).map_err(local_state_unavailable)?;
    }
    db.execute("DELETE FROM sync_lane_inbox WHERE input_id=?1 AND owner_identity_id=?2 AND attempt_token=?3",
        params![claim.input_id, claim.owner_identity_id, claim.token]).map_err(local_state_unavailable)?;
    Ok(())
}

pub(crate) fn apply_ordinary_claim(
    db: &Connection,
    claim: &InputClaim,
    input: sync_v2::DeltaApplyInputV2,
    now: i64,
) -> crate::ImResult<sync_v2::DeltaApplyOutcomeV2> {
    apply_ordinary_claim_with(db, claim, input, now, |_| Ok(()))
}

pub(crate) fn apply_ordinary_claim_with<C>(
    db: &Connection,
    claim: &InputClaim,
    input: sync_v2::DeltaApplyInputV2,
    now: i64,
    commit_domain: C,
) -> crate::ImResult<sync_v2::DeltaApplyOutcomeV2>
where
    C: FnOnce(&Transaction<'_>) -> crate::ImResult<()>,
{
    if claim.lane != "ordinary"
        || input.owner_identity_id != claim.owner_identity_id
        || input.events.len() != 1
        || input.events[0].event_id != claim.event_id
        || input.events[0].event_seq != claim.position
        || input.events[0].event_type != claim.event_type
        || input.stream_epoch != claim.lane_epoch
        || input.owner_did != claim.owner_did
        || input.account_id != claim.account_id
        || input.protocol_device_id != claim.device_id
        || input.device_auth_generation != claim.auth_generation
    {
        return Err(error(
            "SYNC_INPUT_CONFLICT",
            "business result does not match its claimed input",
        ));
    }
    let transaction = Transaction::new_unchecked(db, TransactionBehavior::Immediate)
        .map_err(local_state_unavailable)?;
    require_claim(&transaction, claim, now)?;
    if claim.lane == "ordinary" {
        let baseline_pending: bool = transaction.query_row("SELECT EXISTS(SELECT 1 FROM sync_lane_inbox WHERE owner_identity_id=?1 AND lane='baseline' AND closed_at IS NULL AND created_at>?2)", params![claim.owner_identity_id, now.saturating_sub(RETENTION_SECONDS)], |row| row.get(0)).map_err(local_state_unavailable)?;
        if baseline_pending {
            return Err(error(
                "sync.baseline_pending",
                "ordinary projection awaits local baseline replacement",
            ));
        }
    }
    let outcome = sync_v2::apply_delta_events_in_transaction(&transaction, input)?;
    commit_domain(&transaction)?;
    complete_claim(&transaction, claim, now)?;
    transaction.commit().map_err(local_state_unavailable)?;
    Ok(outcome)
}

pub(crate) fn claim_was_completed(db: &Connection, claim: &InputClaim) -> crate::ImResult<bool> {
    let hash: Option<Option<String>> = if matches!(claim.lane.as_str(), "ordinary" | "baseline") {
        db.query_row("SELECT payload_hash FROM sync_applied_events WHERE owner_identity_id=?1 AND event_id=?2 AND stream_epoch=?3",
            params![claim.owner_identity_id, claim.event_id, claim.lane_epoch], |row| row.get(0))
            .optional().map_err(local_state_unavailable)?
    } else {
        db.query_row("SELECT payload_hash FROM sync_lane_applied_events WHERE owner_identity_id=?1 AND lane=?2 AND event_id=?3 AND stream_epoch=?4",
            params![claim.owner_identity_id, claim.lane, claim.event_id, claim.lane_epoch], |row| row.get(0))
            .optional().map_err(local_state_unavailable)?
    };
    Ok(hash.flatten().as_deref() == Some(payload_hash(&claim.lane, &claim.payload)?.as_str()))
}

fn require_binding(
    db: &Connection,
    binding: &sync_v2::IdentityAccountBinding,
) -> crate::ImResult<()> {
    let current = sync_v2::load_identity_account_binding(db, &binding.owner_identity_id)?;
    if !current.is_some_and(|current| {
        current.account_id == binding.account_id
            && current.protocol_device_id == binding.protocol_device_id
            && current.current_did == binding.current_did
            && current.device_auth_generation == binding.device_auth_generation
    }) {
        return Err(error(
            "SYNC_ACCOUNT_BINDING_MISMATCH",
            "processing or receive binding is no longer current",
        ));
    }
    Ok(())
}

fn has_column(db: &Connection, table: &str, column: &str) -> crate::ImResult<bool> {
    let mut statement = db
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(local_state_unavailable)?;
    let names = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(local_state_unavailable)?;
    for name in names {
        if name.map_err(local_state_unavailable)? == column {
            return Ok(true);
        }
    }
    Ok(false)
}

pub(crate) fn purge_expired(
    db: &Connection,
    now: i64,
    limit: u32,
) -> crate::ImResult<Vec<RemovedInput>> {
    if limit == 0 || limit > 4096 {
        return Err(crate::ImError::invalid_input(
            Some("limit".to_owned()),
            "inbox cleanup limit must be between 1 and 4096",
        ));
    }
    let transaction = db
        .unchecked_transaction()
        .map_err(local_state_unavailable)?;
    let removed = delete_oldest(
        &transaction,
        Some(now.saturating_sub(RETENTION_SECONDS)),
        i64::from(limit),
    )?;
    transaction.commit().map_err(local_state_unavailable)?;
    Ok(removed)
}

/// The caller's receive transaction also contains new rows and cursor writes.
/// Any later failure rolls back these capacity evictions with the whole batch.
pub(crate) fn make_room(
    db: &Connection,
    additional: usize,
    now: i64,
) -> crate::ImResult<Vec<RemovedInput>> {
    make_room_with_limit(db, additional, now, MAX_INBOX_RECORDS)
}

pub(super) fn make_room_with_limit(
    db: &Connection,
    additional: usize,
    now: i64,
    max_records: i64,
) -> crate::ImResult<Vec<RemovedInput>> {
    let additional = i64::try_from(additional).map_err(|_| {
        error(
            "SYNC_INBOX_BATCH_TOO_LARGE",
            "receive batch exceeds inbox capacity",
        )
    })?;
    if additional > max_records || max_records <= 0 {
        return Err(error(
            "SYNC_INBOX_BATCH_TOO_LARGE",
            "receive batch exceeds inbox capacity",
        ));
    }
    // A duplicate-only page does not evict live records to make space.
    let mut removed = delete_oldest(
        db,
        Some(now.saturating_sub(RETENTION_SECONDS)),
        i64::from(sync_v2::SYNC_CLEANUP_BATCH_SIZE),
    )?;
    let count: i64 = db
        .query_row("SELECT COUNT(*) FROM sync_lane_inbox", [], |row| row.get(0))
        .map_err(local_state_unavailable)?;
    let excess = count
        .saturating_add(additional)
        .saturating_sub(max_records)
        .max(0);
    if additional > 0 && excess > 0 {
        removed.extend(delete_oldest(db, None, excess)?);
    }
    Ok(removed)
}

fn delete_oldest(
    db: &Connection,
    cutoff: Option<i64>,
    limit: i64,
) -> crate::ImResult<Vec<RemovedInput>> {
    let mut statement = db
        .prepare(
            "SELECT input_id, owner_identity_id, event_id FROM sync_lane_inbox
         WHERE (?1 IS NULL OR created_at <= ?1)
         ORDER BY created_at, input_id LIMIT ?2",
        )
        .map_err(local_state_unavailable)?;
    let rows = statement
        .query_map(params![cutoff, limit], |row| {
            Ok(RemovedInput {
                input_id: row.get(0)?,
                owner_identity_id: row.get(1)?,
                event_id: row.get(2)?,
            })
        })
        .map_err(local_state_unavailable)?;
    let removed = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(local_state_unavailable)?;
    drop(statement);
    for item in &removed {
        db.execute(
            "DELETE FROM sync_lane_inbox WHERE input_id=?1 AND owner_identity_id=?2",
            params![item.input_id, item.owner_identity_id],
        )
        .map_err(local_state_unavailable)?;
    }
    Ok(removed)
}

pub(crate) fn error(code: &str, detail: &str) -> crate::ImError {
    crate::ImError::Service {
        status_code: None,
        code: Some(code.to_owned()),
        message: detail.to_owned(),
        data: None,
    }
}

pub(crate) fn retry_event(
    db: &Connection,
    owner: &str,
    event_id: &str,
    now: i64,
) -> crate::ImResult<u32> {
    if event_id.is_empty() || event_id.trim() != event_id {
        return Err(crate::ImError::invalid_input(
            Some("event_id".into()),
            "processing event id must be a non-empty canonical string",
        ));
    }
    let changed = db.execute("UPDATE sync_lane_inbox SET next_attempt_at=?3, processing_error_code=NULL
        WHERE owner_identity_id=?1 AND event_id=?2 AND closed_at IS NULL AND attempt_token IS NULL AND created_at>?4",
        params![owner, event_id, now, now.saturating_sub(RETENTION_SECONDS)]).map_err(local_state_unavailable)?;
    Ok(changed.min(u32::MAX as usize) as u32)
}

pub(crate) fn pending_updates(
    db: &Connection,
    owner: &str,
    limit: u32,
    now: i64,
) -> crate::ImResult<Vec<crate::messages::MessageProcessingUpdate>> {
    if limit == 0 || limit > 256 {
        return Err(crate::ImError::invalid_input(
            Some("limit".into()),
            "pending processing limit must be between 1 and 256",
        ));
    }
    let mut stmt = db
        .prepare(
            "SELECT event_id, next_attempt_at, processing_error_code FROM sync_lane_inbox
        WHERE owner_identity_id=?1 AND closed_at IS NULL AND created_at>?2
        ORDER BY (next_attempt_at IS NOT NULL), created_at, input_id LIMIT ?3",
        )
        .map_err(local_state_unavailable)?;
    let rows = stmt
        .query_map(
            params![owner, now.saturating_sub(RETENTION_SECONDS), limit],
            |row| {
                let retry: Option<i64> = row.get(1)?;
                Ok(crate::messages::MessageProcessingUpdate {
                    event_id: row.get(0)?,
                    status: if retry.is_none() {
                        crate::messages::MessageProcessingStatus::Blocked
                    } else {
                        crate::messages::MessageProcessingStatus::Retrying
                    },
                    changed_conversation_ids: vec![],
                    committed_incoming_messages: vec![],
                    error_code: row.get(2)?,
                })
            },
        )
        .map_err(local_state_unavailable)?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(local_state_unavailable)
}
