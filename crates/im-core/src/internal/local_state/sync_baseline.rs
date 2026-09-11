//! Durable local baseline inputs share the receive inbox. They are not ANP lanes.
//! A baseline only performs local replacement; each message remains independent.
use super::{
    local_state_unavailable,
    sync_inbox::{self, InboxEvent, InputClaim, ReceiveOutcome},
    sync_v2,
};
use rusqlite::{Connection, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Baseline {
    pub(crate) server_time: String,
    pub(crate) server_cutoff: Option<String>,
    pub(crate) groups: Vec<Value>,
    pub(crate) read_states: Vec<Value>,
    pub(crate) message_ids: BTreeSet<String>,
    pub(crate) notification_ids: BTreeSet<String>,
}

pub(crate) fn receive_snapshot(
    db: &Connection,
    checkpoint: sync_v2::SnapshotApplyInputV2,
    binding: sync_v2::IdentityAccountBinding,
    installation: String,
    baseline: Baseline,
    events: Vec<InboxEvent>,
    now: i64,
) -> crate::ImResult<ReceiveOutcome> {
    let tx = Transaction::new_unchecked(db, TransactionBehavior::Immediate)
        .map_err(local_state_unavailable)?;
    sync_v2::validate_snapshot_receive(&tx, &checkpoint)?;
    if binding.owner_identity_id != checkpoint.owner_identity_id
        || binding.current_did != checkpoint.owner_did
    {
        return Err(sync_inbox::error(
            "SYNC_ACCOUNT_BINDING_MISMATCH",
            "snapshot queue binding does not match checkpoint",
        ));
    }
    let outcome = insert_baseline_and_events(
        &tx,
        &binding,
        &installation,
        &checkpoint.stream_epoch,
        &checkpoint.snapshot_scan_seq,
        baseline,
        events,
        now,
    )?;
    sync_v2::commit_snapshot_receive_state(&tx, &checkpoint)?;
    tx.commit().map_err(local_state_unavailable)?;
    Ok(outcome)
}

pub(crate) fn receive_bootstrap(
    db: &Connection,
    input: sync_v2::BootstrapApplyInputV2,
    baseline: Baseline,
    expected_run_generation: i64,
    now: i64,
) -> crate::ImResult<ReceiveOutcome> {
    let tx = Transaction::new_unchecked(db, TransactionBehavior::Immediate)
        .map_err(local_state_unavailable)?;
    sync_v2::require_message_sync_run_generation(
        &tx,
        &input.binding.owner_identity_id,
        Some(expected_run_generation),
    )?;
    let binding = input.binding.clone();
    let installation = input.client_instance_id.clone();
    let epoch = input.state.stream_epoch.clone();
    let anchor = input.state.scan_seq.clone();
    // This header has no domain projections; only binding, negotiation and cursor.
    if !input.groups.is_empty() || !input.read_states.is_empty() {
        return Err(sync_inbox::error(
            "SYNC_INPUT_INVALID",
            "bootstrap receive header contains business projections",
        ));
    }
    sync_v2::apply_bootstrap_in_transaction(&tx, input)?;
    let outcome = insert_baseline_and_events(
        &tx,
        &binding,
        &installation,
        &epoch,
        &anchor,
        baseline,
        vec![],
        now,
    )?;
    tx.commit().map_err(local_state_unavailable)?;
    Ok(outcome)
}

fn insert_baseline_and_events(
    db: &Connection,
    binding: &sync_v2::IdentityAccountBinding,
    installation: &str,
    epoch: &str,
    anchor: &str,
    baseline: Baseline,
    events: Vec<InboxEvent>,
    now: i64,
) -> crate::ImResult<ReceiveOutcome> {
    let installed: String = db
        .query_row(
            "SELECT client_instance_id FROM sync_installation_state WHERE owner_identity_id=?1",
            [&binding.owner_identity_id],
            |row| row.get(0),
        )
        .map_err(local_state_unavailable)?;
    if installed != installation {
        return Err(sync_inbox::error(
            "SYNC_INSTALLATION_CHANGED",
            "baseline receive installation is no longer current",
        ));
    }
    let mut payload = serde_json::to_value(baseline)
        .map_err(|_| sync_inbox::error("SYNC_INPUT_INVALID", "baseline cannot be encoded"))?;
    payload["snapshot_scan_seq"] = serde_json::json!(anchor);
    // Bootstrap and snapshot responses are fresh observations, not immutable
    // stream events. Several can share an anchor while their state differs.
    // Give each local receipt its own positive position; processing uses inbox
    // receive order, and the remote anchor remains explicit in the payload.
    let position = rand::random::<u128>().max(1).to_string();
    let marker = InboxEvent {
        event_id: format!("local-sync-baseline:{position}"),
        position,
        event_type: "sync.baseline".into(),
        payload,
        processing_scope: "baseline".into(),
        group_did: None,
    };
    let mut pending = Vec::new();
    let mut outcome = ReceiveOutcome::default();
    for (lane, event) in std::iter::once(("baseline", marker))
        .chain(events.into_iter().map(|event| ("ordinary", event)))
    {
        if sync_inbox::input_is_duplicate(db, &binding.owner_identity_id, lane, epoch, &event)? {
            outcome.duplicates += usize::from(lane == "ordinary");
        } else {
            pending.push((lane, event));
        }
    }
    outcome.removed = sync_inbox::make_room(db, pending.len(), now)?;
    for (lane, event) in pending {
        sync_inbox::insert_input(db, binding, installation, lane, epoch, &event, now)?;
        outcome.received += usize::from(lane == "ordinary");
    }
    Ok(outcome)
}

pub(crate) fn apply_claim(
    db: &Connection,
    claim: &InputClaim,
    baseline: Baseline,
    groups: Vec<super::groups::GroupRecord>,
    read_states: Vec<sync_v2::ReadStateApplyV2>,
    now: i64,
) -> crate::ImResult<super::sync_state::SyncDeltaInvalidation> {
    let tx = Transaction::new_unchecked(db, TransactionBehavior::Immediate)
        .map_err(local_state_unavailable)?;
    sync_inbox::require_claim(&tx, claim, now)?;
    let removed = if let Some(cutoff) = baseline.server_cutoff.as_deref() {
        let group_ids = groups.iter().map(|group| group.group_id.clone()).collect();
        sync_v2::replace_snapshot_ordinary_projection(
            &tx,
            &claim.owner_identity_id,
            &claim.device_id,
            &baseline.server_time,
            cutoff,
            &baseline.message_ids,
            &group_ids,
            &baseline.notification_ids,
        )?
    } else {
        BTreeSet::new()
    };
    let mut invalidation = sync_v2::v2_invalidation(
        &tx,
        &claim.owner_identity_id,
        &claim.owner_did,
        claim
            .payload
            .get("snapshot_scan_seq")
            .and_then(Value::as_str)
            .unwrap_or(&claim.position),
        &[],
        &groups,
        &read_states,
    )?;
    for group in groups {
        if !sync_v2::group_state_is_stale(&tx, &group)? {
            super::groups::upsert_group(&tx, group)?;
        }
    }
    for read_state in read_states {
        sync_v2::apply_remote_read_state(
            &tx,
            &claim.owner_identity_id,
            &claim.owner_did,
            &read_state,
        )?;
    }
    sync_v2::record_applied_event(
        &tx,
        &sync_v2::AppliedEventReceipt {
            owner_identity_id: claim.owner_identity_id.clone(),
            event_id: claim.event_id.clone(),
            stream_epoch: claim.lane_epoch.clone(),
            event_seq: claim.position.clone(),
            applied_at: now,
        },
    )?;
    sync_inbox::complete_claim(&tx, claim, now)?;
    tx.commit().map_err(local_state_unavailable)?;
    invalidation
        .conversation_ids
        .extend(removed.iter().cloned());
    invalidation.thread_ids.extend(removed);
    invalidation.conversation_ids.sort();
    invalidation.conversation_ids.dedup();
    invalidation.thread_ids.sort();
    invalidation.thread_ids.dedup();
    Ok(invalidation)
}

#[cfg(test)]
#[path = "sync_baseline_tests.rs"]
mod tests;
