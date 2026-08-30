use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex as StdMutex, OnceLock};
use std::time::{Duration as StdDuration, Instant};

use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

use crate::internal::auth::session::AsyncSessionProvider;
use crate::internal::message_runtime::read::MESSAGE_RPC_ENDPOINT;
use crate::internal::transport::{AsyncAuthenticatedRpcTransport, AsyncRpcTransport};

const PENDING_PERSONA_RESOLUTION_LIMIT: u32 = 32;
const GROUP_SEQUENCE_ONLY_TARGET_NOT_FOUND_MAX_ATTEMPTS: i64 = 3;
const SYNC_RUN_MAX_PAGES: u32 = 20;
const SYNC_RUN_DEADLINE: StdDuration = StdDuration::from_secs(20);

pub(crate) struct MessageSyncRuntimeV2<'a, P, T, R> {
    client: &'a crate::core::ImClient,
    session_provider: P,
    transport: T,
    directory_transport: R,
    run_deadline: StdDuration,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum RealtimeInlineMessageApplyOutcome {
    NotApplicable,
    Applied {
        message: crate::messages::Message,
        local_scan_seq: Option<String>,
    },
    Deferred,
}

#[cfg(all(feature = "blocking", feature = "sqlite"))]
pub(crate) fn apply_realtime_inline_message_v3(
    client: &crate::core::ImClient,
    notification: &Value,
) -> crate::ImResult<RealtimeInlineMessageApplyOutcome> {
    if crate::internal::realtime::notification::parse_inline_sync_event_v3(notification)?
        .is_some_and(|inline| {
            inline.lane != crate::internal::realtime::notification::InlineSyncLaneV3::Ordinary
        })
    {
        return Ok(RealtimeInlineMessageApplyOutcome::Deferred);
    }
    let Some((message, input)) = prepare_realtime_inline_message_v3(client, notification)? else {
        return Ok(RealtimeInlineMessageApplyOutcome::NotApplicable);
    };
    let connection = crate::internal::local_state::open_writable(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    )?;
    finish_realtime_inline_message_v3(
        client,
        message,
        crate::internal::local_state::sync_v2::apply_realtime_message_v3(&connection, input)?,
    )
}

#[cfg(feature = "sqlite")]
pub(crate) async fn apply_realtime_inline_message_v3_async(
    client: &crate::core::ImClient,
    notification: &Value,
) -> crate::ImResult<RealtimeInlineMessageApplyOutcome> {
    let Some(inline) =
        crate::internal::realtime::notification::parse_inline_sync_event_v3(notification)?
    else {
        return Ok(RealtimeInlineMessageApplyOutcome::NotApplicable);
    };
    if inline.lane != crate::internal::realtime::notification::InlineSyncLaneV3::Ordinary {
        return apply_realtime_e2ee_lane_v3_async(client, inline).await;
    }
    let Some((message, input)) = prepare_realtime_inline_message_v3(client, notification)? else {
        return Ok(RealtimeInlineMessageApplyOutcome::NotApplicable);
    };
    let outcome = client
        .core_inner()
        .local_state_db()
        .await?
        .apply_realtime_message_v3(input)
        .await?;
    finish_realtime_inline_message_v3(client, message, outcome)
}

#[cfg(feature = "sqlite")]
async fn apply_realtime_e2ee_lane_v3_async(
    client: &crate::core::ImClient,
    inline: crate::internal::realtime::notification::InlineSyncEventV3,
) -> crate::ImResult<RealtimeInlineMessageApplyOutcome> {
    let mut directory_transport = crate::internal::transport::CoreHttpTransport::new(client);
    apply_realtime_e2ee_lane_v3_with_directory_async(client, inline, &mut directory_transport).await
}

#[cfg(feature = "sqlite")]
async fn apply_realtime_e2ee_lane_v3_with_directory_async<R>(
    client: &crate::core::ImClient,
    inline: crate::internal::realtime::notification::InlineSyncEventV3,
    directory_transport: &mut R,
) -> crate::ImResult<RealtimeInlineMessageApplyOutcome>
where
    R: AsyncRpcTransport,
{
    // V1-B keeps HTTP sync as the sole durable lane source. Inline P5/P6
    // hydration is explicitly deferred; the notification remains only a pull
    // hint and never advances crypto/domain state ahead of sync_lane_inbox.
    let _ = (client, inline, directory_transport);
    Ok(RealtimeInlineMessageApplyOutcome::Deferred)
}

#[cfg(feature = "sqlite")]
fn prepare_realtime_inline_message_v3(
    client: &crate::core::ImClient,
    notification: &Value,
) -> crate::ImResult<
    Option<(
        crate::messages::Message,
        crate::internal::local_state::sync_v2::RealtimeMessageApplyInputV3,
    )>,
> {
    let Some(inline) =
        crate::internal::realtime::notification::parse_inline_sync_event_v3(notification)?
    else {
        return Ok(None);
    };
    if inline.lane != crate::internal::realtime::notification::InlineSyncLaneV3::Ordinary {
        return Ok(None);
    }
    let event = inline
        .ordinary_event
        .as_ref()
        .expect("ordinary inline event carries its frozen event");
    let context = client.sync_account_context()?;
    if event.account_id != context.account_id
        || event
            .recipient_device_id
            .as_deref()
            .is_some_and(|device_id| device_id != context.protocol_device_id)
    {
        return Err(sync_error(
            "SYNC_ACCOUNT_BINDING_MISMATCH",
            "realtime sync event does not match the active account device",
        ));
    }
    let projection_id = inline
        .projection
        .get("id")
        .or_else(|| inline.projection.get("message_id"))
        .and_then(Value::as_str);
    if projection_id != Some(event.aggregate_id.as_str())
        || event.payload.get("message_id").and_then(Value::as_str)
            != Some(event.aggregate_id.as_str())
    {
        return Err(sync_error(
            "SYNC_INVALID_PAGE",
            "realtime message projection conflicts with its event aggregate",
        ));
    }
    let stream_epoch = event.stream_epoch.clone();
    let mut public_messages = BTreeMap::new();
    let event = reduce_event(
        client,
        event,
        Some(&inline.projection),
        None,
        &mut public_messages,
    )?;
    let message = public_messages.remove(&event.event_id).ok_or_else(|| {
        sync_error(
            "SYNC_INVALID_PAGE",
            "realtime message reducer did not produce a public projection",
        )
    })?;
    Ok(Some((
        message,
        crate::internal::local_state::sync_v2::RealtimeMessageApplyInputV3 {
            owner_identity_id: client.current_identity().id.as_str().to_owned(),
            owner_did: client.did().as_str().to_owned(),
            account_id: context.account_id,
            protocol_device_id: context.protocol_device_id,
            device_auth_generation: context.device_auth_generation,
            stream_epoch,
            event,
        },
    )))
}

#[cfg(feature = "sqlite")]
fn finish_realtime_inline_message_v3(
    client: &crate::core::ImClient,
    message: crate::messages::Message,
    outcome: crate::internal::local_state::sync_v2::RealtimeMessageApplyOutcomeV3,
) -> crate::ImResult<RealtimeInlineMessageApplyOutcome> {
    match outcome {
        crate::internal::local_state::sync_v2::RealtimeMessageApplyOutcomeV3::Applied {
            local_scan_seq,
            invalidation,
        } => {
            super::sync::emit_committed_sync_invalidation(client, &invalidation);
            client.emit_committed_local_message_projection("sync_v2_realtime_fast_path");
            Ok(RealtimeInlineMessageApplyOutcome::Applied {
                message,
                local_scan_seq: Some(local_scan_seq),
            })
        }
        crate::internal::local_state::sync_v2::RealtimeMessageApplyOutcomeV3::Duplicate {
            ..
        }
        | crate::internal::local_state::sync_v2::RealtimeMessageApplyOutcomeV3::HintOnly {
            ..
        } => Ok(RealtimeInlineMessageApplyOutcome::Deferred),
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct LaneConsumerRunSummary {
    applied: u32,
    closed: u32,
    retryable: u32,
}

async fn consume_p5_lane_input(
    client: &crate::core::ImClient,
    input: &crate::internal::local_state::sync_v2::SyncLaneInboxRecord,
    attempt_count: i64,
) -> crate::ImResult<crate::internal::local_state::sync_v2::SyncLaneDomainStatus> {
    use crate::internal::local_state::sync_v2::{SyncLaneDomainState, SyncLaneDomainStatus};
    use crate::internal::wire::sync_v2::SyncLaneV3;
    let lock_scope = input
        .raw_payload
        .pointer("/meta/sender_did")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("invalid");
    let scope_lock =
        lane_consumer_scope_lock(&input.owner_identity_id, SyncLaneV3::P5Device, lock_scope);
    let _scope_guard = scope_lock.lock().await;
    let db = client.core_inner().local_state_db().await?;
    if let Some(status) =
        closed_lane_domain_status(&db, &input.owner_identity_id, &input.input_id).await?
    {
        return Ok(status);
    }
    let (metadata, body) =
        match crate::internal::secure_direct::v2_product::parse_v2_wire_message(&input.raw_payload)
        {
            Ok(Some(value)) => value,
            Ok(None) | Err(_) => {
                let invalid_scope = input
                    .raw_payload
                    .pointer("/meta/sender_did")
                    .and_then(Value::as_str)
                    .filter(|value| !value.trim().is_empty())
                    .map(str::to_owned)
                    .unwrap_or_else(|| format!("invalid:{}", input.event_id));
                let state = SyncLaneDomainState {
                    input_id: input.input_id.clone(),
                    lane: SyncLaneV3::P5Device,
                    scope: invalid_scope,
                    status: SyncLaneDomainStatus::Terminal,
                    retryable: false,
                    attempt_count,
                    next_retry_at: None,
                    operation_ref: Some(input.event_id.clone()),
                    last_error_code: Some("p5.malformed_input".to_owned()),
                };
                client
                    .core_inner()
                    .local_state_db()
                    .await?
                    .write_sync_lane_domain_state(state, unix_time_i64())
                    .await?;
                return Ok(SyncLaneDomainStatus::Terminal);
            }
        };
    let peer_scope = metadata.sender_did.clone();
    let expected_peer_did =
        p5_expected_decryption_peer(&metadata.sender_did, &metadata.target.did).map(str::to_owned);
    let trusted_delivery = p5_reliable_delivery_context(&metadata, &input.raw_payload)?;
    let now = unix_time_i64();
    if metadata.target.did != client.did().as_str() {
        let cutover = db
            .load_p5_did_cutover(input.owner_identity_id.clone(), metadata.target.did.clone())
            .await?;
        let (status, code) = match cutover.as_ref() {
            Some(cutover)
                if cutover.new_did == client.did().as_str()
                    && cutover.status == "draining"
                    && now <= cutover.drain_deadline =>
            {
                (
                    SyncLaneDomainStatus::RepairRequired,
                    "p5.old_did_retained_session_unavailable",
                )
            }
            Some(_) => (
                SyncLaneDomainStatus::Terminal,
                "p5.old_did_retention_expired",
            ),
            None => (
                SyncLaneDomainStatus::RepairRequired,
                "p5.target_binding_mismatch",
            ),
        };
        db.write_sync_lane_domain_state(
            SyncLaneDomainState {
                input_id: input.input_id.clone(),
                lane: SyncLaneV3::P5Device,
                scope: peer_scope,
                status,
                retryable: false,
                attempt_count,
                next_retry_at: None,
                operation_ref: Some(input.event_id.clone()),
                last_error_code: Some(code.to_owned()),
            },
            now,
        )
        .await?;
        if cutover.is_some() {
            let _ = db
                .complete_p5_did_cutover_if_drained(
                    input.owner_identity_id.clone(),
                    metadata.target.did,
                    now,
                )
                .await?;
        }
        return Ok(status);
    }
    db.write_sync_lane_domain_state(
        SyncLaneDomainState {
            input_id: input.input_id.clone(),
            lane: SyncLaneV3::P5Device,
            scope: peer_scope.clone(),
            status: SyncLaneDomainStatus::Processing,
            retryable: true,
            attempt_count,
            next_retry_at: None,
            operation_ref: Some(input.event_id.clone()),
            last_error_code: None,
        },
        now,
    )
    .await?;
    let input_id = input.input_id.clone();
    let event_id = input.event_id.clone();
    let peer_for_commit = peer_scope.clone();
    let raw_payload = input.raw_payload.clone();
    let result = crate::internal::secure_direct::v2_product::receive_for_client_scoped_with_commit(
        &client.core_handle(),
        client,
        true,
        metadata,
        body,
        expected_peer_did.as_deref(),
        Some(&trusted_delivery),
        move |transaction, outcome| {
            let own_sync_target = match outcome {
                crate::internal::secure_direct::v2_product::V2InboundProductOutcome::OwnSync(
                    projection,
                ) => Some(projection.target_did.as_str()),
                _ => None,
            };
            if let Some((record, attachment_manifest_cache)) =
                super::read::p5_lane_projection_record(client, &raw_payload, outcome)?
            {
                crate::internal::local_state::messages::upsert_messages_with_touched(
                    transaction,
                    &[record],
                )?;
                if let Some(attachment_manifest_cache) = attachment_manifest_cache {
                    crate::internal::local_state::attachment_manifest_cache::upsert_attachment_manifest_cache(
                        transaction,
                        &attachment_manifest_cache,
                    )?;
                }
            }
            crate::internal::local_state::sync_v2::write_sync_lane_domain_state_in_transaction(
                transaction,
                &SyncLaneDomainState {
                    input_id,
                    lane: SyncLaneV3::P5Device,
                    scope: p5_committed_domain_scope(&peer_for_commit, own_sync_target),
                    status: SyncLaneDomainStatus::Applied,
                    retryable: false,
                    attempt_count,
                    next_retry_at: None,
                    operation_ref: Some(event_id),
                    last_error_code: None,
                },
                now,
            )
        },
    )
    .await;
    match result {
        Ok(crate::internal::secure_direct::v2_product::V2InboundProductOutcome::Replay) => {
            let applied = db
                .load_sync_lane_domain_states(input.owner_identity_id.clone())
                .await?
                .into_iter()
                .any(|state| {
                    state.input_id == input.input_id
                        && state.status == SyncLaneDomainStatus::Applied
                });
            if applied {
                Ok(SyncLaneDomainStatus::Applied)
            } else {
                let state = p5_failure_domain_state(
                    input,
                    &peer_scope,
                    attempt_count,
                    &crate::ImError::IdentityBindingConflict {
                        detail: "legacy P5 replay has no atomic projection marker".to_owned(),
                    },
                    now,
                );
                let status = state.status;
                db.write_sync_lane_domain_state(state, now).await?;
                Ok(status)
            }
        }
        Ok(_) => {
            let applied = db
                .load_sync_lane_domain_states(input.owner_identity_id.clone())
                .await?
                .into_iter()
                .any(|state| {
                    state.input_id == input.input_id
                        && state.status == SyncLaneDomainStatus::Applied
                });
            if !applied {
                db.write_sync_lane_domain_state(
                    SyncLaneDomainState {
                        input_id: input.input_id.clone(),
                        lane: SyncLaneV3::P5Device,
                        scope: peer_scope,
                        status: SyncLaneDomainStatus::Applied,
                        retryable: false,
                        attempt_count,
                        next_retry_at: None,
                        operation_ref: Some(input.event_id.clone()),
                        last_error_code: None,
                    },
                    now,
                )
                .await?;
            }
            Ok(SyncLaneDomainStatus::Applied)
        }
        Err(error) => {
            let state = p5_failure_domain_state(input, &peer_scope, attempt_count, &error, now);
            let status = state.status;
            db.write_sync_lane_domain_state(state, now).await?;
            Ok(status)
        }
    }
}

fn p5_expected_decryption_peer<'a>(sender_did: &'a str, target_did: &str) -> Option<&'a str> {
    (sender_did != target_did).then_some(sender_did)
}

fn p5_committed_domain_scope(fallback: &str, own_sync_target: Option<&str>) -> String {
    own_sync_target.unwrap_or(fallback).to_owned()
}

fn p5_reliable_delivery_context(
    metadata: &anp::direct_e2ee::V2DirectMetadata,
    raw_payload: &Value,
) -> crate::ImResult<crate::internal::identity_root_import_completion::TrustedDirectDeliveryContext>
{
    let accepted_at = raw_payload
        .get("accepted_at")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    crate::internal::identity_root_import_completion::TrustedDirectDeliveryContext::from_stored_message(
        metadata,
        accepted_at,
        crate::internal::identity_root_import_completion::TrustedDirectDeliverySource::ReliableSync,
    )
}

fn p5_failure_domain_state(
    input: &crate::internal::local_state::sync_v2::SyncLaneInboxRecord,
    peer_scope: &str,
    attempt_count: i64,
    error: &crate::ImError,
    now: i64,
) -> crate::internal::local_state::sync_v2::SyncLaneDomainState {
    use crate::internal::local_state::sync_v2::{SyncLaneDomainState, SyncLaneDomainStatus};
    let (status, retryable, next_retry_at, code) = match error {
        crate::ImError::TransportUnavailable { .. }
        | crate::ImError::LocalStateUnavailable { .. }
            if attempt_count < 3 =>
        {
            (
                SyncLaneDomainStatus::Pending,
                true,
                Some(now.saturating_add(1_i64 << attempt_count.clamp(0, 6))),
                "p5.transient",
            )
        }
        crate::ImError::UnsupportedCapability { .. } => (
            SyncLaneDomainStatus::UpgradeRequired,
            false,
            None,
            "p5.upgrade_required",
        ),
        crate::ImError::PeerNotFound { .. }
        | crate::ImError::IdentityBindingConflict { .. }
        | crate::ImError::LocalStateUnavailable { .. }
        | crate::ImError::TransportUnavailable { .. } => (
            SyncLaneDomainStatus::RepairRequired,
            false,
            None,
            "p5.repair_required",
        ),
        _ => (SyncLaneDomainStatus::Terminal, false, None, "p5.terminal"),
    };
    SyncLaneDomainState {
        input_id: input.input_id.clone(),
        lane: crate::internal::wire::sync_v2::SyncLaneV3::P5Device,
        scope: peer_scope.to_owned(),
        status,
        retryable,
        attempt_count,
        next_retry_at,
        operation_ref: Some(input.event_id.clone()),
        last_error_code: Some(code.to_owned()),
    }
}

async fn drain_p5_lane_inputs(
    client: &crate::core::ImClient,
    max_inputs: u32,
) -> crate::ImResult<LaneConsumerRunSummary> {
    use crate::internal::wire::sync_v2::SyncLaneV3;
    let db = client.core_inner().local_state_db().await?;
    let owner_identity_id = client.current_identity().id.as_str().to_owned();
    let existing = db
        .load_sync_lane_domain_states(owner_identity_id.clone())
        .await?
        .into_iter()
        .map(|state| (state.input_id.clone(), state))
        .collect::<BTreeMap<_, _>>();
    let inputs = db
        .list_pending_sync_lane_inputs(
            owner_identity_id,
            SyncLaneV3::P5Device,
            unix_time_i64(),
            max_inputs.min(256),
        )
        .await?;
    let mut by_peer = BTreeMap::<String, Vec<_>>::new();
    for input in inputs {
        let peer = input
            .raw_payload
            .pointer("/meta/sender_did")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("invalid")
            .to_owned();
        by_peer.entry(peer).or_default().push(input);
    }
    let mut summary = LaneConsumerRunSummary::default();
    for inputs in by_peer.values() {
        for input in inputs.iter().take(8) {
            let attempt_count = existing
                .get(&input.input_id)
                .map_or(1, |state| state.attempt_count.saturating_add(1));
            match consume_p5_lane_input(client, input, attempt_count).await? {
                crate::internal::local_state::sync_v2::SyncLaneDomainStatus::Applied => {
                    summary.applied += 1;
                }
                crate::internal::local_state::sync_v2::SyncLaneDomainStatus::Pending
                | crate::internal::local_state::sync_v2::SyncLaneDomainStatus::Processing => {
                    summary.retryable += 1;
                }
                _ => summary.closed += 1,
            }
        }
    }
    Ok(summary)
}

async fn consume_p6_lane_input(
    client: &crate::core::ImClient,
    input: &crate::internal::local_state::sync_v2::SyncLaneInboxRecord,
    attempt_count: i64,
) -> crate::ImResult<crate::internal::local_state::sync_v2::SyncLaneDomainStatus> {
    use crate::internal::local_state::sync_v2::{SyncLaneDomainState, SyncLaneDomainStatus};
    use crate::internal::wire::sync_v2::SyncLaneV3;
    let group_did = input
        .group_did
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("invalid")
        .to_owned();
    let scope_lock =
        lane_consumer_scope_lock(&input.owner_identity_id, SyncLaneV3::P6Group, &group_did);
    let _scope_guard = scope_lock.lock().await;
    let db = client.core_inner().local_state_db().await?;
    if let Some(status) =
        closed_lane_domain_status(&db, &input.owner_identity_id, &input.input_id).await?
    {
        return Ok(status);
    }
    let now = unix_time_i64();
    let operation_ref = format!("p6:{}", input.input_id);
    db.write_sync_lane_domain_state(
        SyncLaneDomainState {
            input_id: input.input_id.clone(),
            lane: SyncLaneV3::P6Group,
            scope: group_did.clone(),
            status: SyncLaneDomainStatus::Processing,
            retryable: true,
            attempt_count,
            next_retry_at: None,
            operation_ref: Some(operation_ref.clone()),
            last_error_code: None,
        },
        now,
    )
    .await?;
    let effect = match input.event_type.as_str() {
        "p6.delivery.created" => {
            let group_event_seq = input
                .raw_payload
                .pointer("/body/group_event_seq")
                .and_then(|value| {
                    value
                        .as_str()
                        .map(str::to_owned)
                        .or_else(|| value.as_u64().map(|value| value.to_string()))
                })
                .ok_or_else(|| {
                    sync_error(
                        "SYNC_P6_NONCONFORMANT",
                        "P6 inbox application is missing group_event_seq",
                    )
                });
            match group_event_seq {
                Ok(group_event_seq) => match validate_p6_delta_binding(
                    &group_did,
                    &group_event_seq,
                    &input.raw_payload,
                ) {
                    Ok(()) => apply_p6_lane_delivery_projection_async(client, &input.raw_payload)
                        .await
                        .map(|_| ()),
                    Err(error) => Err(error),
                },
                Err(error) => Err(error),
            }
        }
        "p6.control.notice" => {
            super::read::consume_group_e2ee_control_notice_from_reliable_sync_async(
                client,
                &input.raw_payload,
            )
            .await
        }
        _ => Err(sync_error(
            "SYNC_P6_NONCONFORMANT",
            "P6 inbox contains an unsupported production event type",
        )),
    };
    match effect {
        Ok(()) => {
            db.write_sync_lane_domain_state(
                SyncLaneDomainState {
                    input_id: input.input_id.clone(),
                    lane: SyncLaneV3::P6Group,
                    scope: group_did,
                    status: SyncLaneDomainStatus::Applied,
                    retryable: false,
                    attempt_count,
                    next_retry_at: None,
                    operation_ref: Some(operation_ref),
                    last_error_code: None,
                },
                now,
            )
            .await?;
            Ok(SyncLaneDomainStatus::Applied)
        }
        Err(error) => {
            let state = p6_failure_domain_state(
                input,
                &group_did,
                &operation_ref,
                attempt_count,
                &error,
                now,
            );
            let status = state.status;
            db.write_sync_lane_domain_state(state, now).await?;
            Ok(status)
        }
    }
}

fn p6_failure_domain_state(
    input: &crate::internal::local_state::sync_v2::SyncLaneInboxRecord,
    group_did: &str,
    operation_ref: &str,
    attempt_count: i64,
    error: &crate::ImError,
    now: i64,
) -> crate::internal::local_state::sync_v2::SyncLaneDomainState {
    use crate::internal::local_state::sync_v2::{SyncLaneDomainState, SyncLaneDomainStatus};
    let error_code = error_code(error).unwrap_or("p6.unknown");
    let notice_type = input
        .raw_payload
        .pointer("/body/notice_type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let owner_or_authority_unavailable = matches!(
        error_code,
        "group.e2ee.owner_unavailable"
            | "group.e2ee.member_removed"
            | "group.e2ee.authorization_required"
    );
    let (status, retryable, next_retry_at, code) = match error {
        crate::ImError::TransportUnavailable { .. }
        | crate::ImError::LocalStateUnavailable { .. }
            if attempt_count < 3 =>
        {
            (
                SyncLaneDomainStatus::Pending,
                true,
                Some(now.saturating_add(1_i64 << attempt_count.clamp(0, 6))),
                "p6.transient",
            )
        }
        _ if owner_or_authority_unavailable => (
            SyncLaneDomainStatus::ActionRequired,
            false,
            None,
            "p6.action_required",
        ),
        crate::ImError::UnsupportedCapability { .. } => (
            SyncLaneDomainStatus::UpgradeRequired,
            false,
            None,
            "p6.upgrade_required",
        ),
        crate::ImError::GroupNotFound { .. }
        | crate::ImError::IdentityBindingConflict { .. }
        | crate::ImError::PeerNotFound { .. }
        | crate::ImError::LocalStateUnavailable { .. }
        | crate::ImError::TransportUnavailable { .. } => (
            SyncLaneDomainStatus::RepairRequired,
            false,
            None,
            "p6.repair_required",
        ),
        _ if matches!(notice_type, "welcome" | "commit" | "commit-delivery") => (
            SyncLaneDomainStatus::RejoinRequired,
            false,
            None,
            "p6.rejoin_required",
        ),
        _ => (SyncLaneDomainStatus::Terminal, false, None, "p6.terminal"),
    };
    SyncLaneDomainState {
        input_id: input.input_id.clone(),
        lane: crate::internal::wire::sync_v2::SyncLaneV3::P6Group,
        scope: group_did.to_owned(),
        status,
        retryable,
        attempt_count,
        next_retry_at,
        operation_ref: Some(operation_ref.to_owned()),
        last_error_code: Some(code.to_owned()),
    }
}

async fn drain_p6_lane_inputs(
    client: &crate::core::ImClient,
    max_inputs: u32,
) -> crate::ImResult<LaneConsumerRunSummary> {
    use crate::internal::wire::sync_v2::SyncLaneV3;
    let db = client.core_inner().local_state_db().await?;
    let owner_identity_id = client.current_identity().id.as_str().to_owned();
    let existing = db
        .load_sync_lane_domain_states(owner_identity_id.clone())
        .await?
        .into_iter()
        .map(|state| (state.input_id.clone(), state))
        .collect::<BTreeMap<_, _>>();
    let inputs = db
        .list_pending_sync_lane_inputs(
            owner_identity_id,
            SyncLaneV3::P6Group,
            unix_time_i64(),
            max_inputs.min(256),
        )
        .await?;
    let mut by_group = BTreeMap::<String, Vec<_>>::new();
    for input in inputs {
        by_group
            .entry(
                input
                    .group_did
                    .clone()
                    .unwrap_or_else(|| "invalid".to_owned()),
            )
            .or_default()
            .push(input);
    }
    let mut summary = LaneConsumerRunSummary::default();
    for inputs in by_group.values() {
        for input in inputs.iter().take(8) {
            let attempt_count = existing
                .get(&input.input_id)
                .map_or(1, |state| state.attempt_count.saturating_add(1));
            match consume_p6_lane_input(client, input, attempt_count).await? {
                crate::internal::local_state::sync_v2::SyncLaneDomainStatus::Applied => {
                    summary.applied += 1;
                }
                crate::internal::local_state::sync_v2::SyncLaneDomainStatus::Pending
                | crate::internal::local_state::sync_v2::SyncLaneDomainStatus::Processing => {
                    summary.retryable += 1;
                }
                _ => summary.closed += 1,
            }
        }
    }
    Ok(summary)
}

fn wake_sync_lane_consumers(client: &crate::core::ImClient) {
    let client = client.clone();
    tokio::spawn(async move {
        let _ = drain_pending_secure_lane_consumers(&client, 64).await;
    });
}

pub(crate) async fn drain_pending_secure_lane_consumers(
    client: &crate::core::ImClient,
    max_inputs: u32,
) -> crate::ImResult<()> {
    if max_inputs == 0 || max_inputs > 256 {
        return Err(crate::ImError::invalid_input(
            Some("max_inputs".to_owned()),
            "secure lane drain limit must be between 1 and 256",
        ));
    }
    let mut first_error: Option<crate::ImError> = None;
    #[cfg(feature = "secure-direct")]
    if let Err(error) = drain_p5_lane_inputs(client, max_inputs).await {
        first_error = Some(error);
    }
    #[cfg(feature = "group-e2ee")]
    if let Err(error) = drain_p6_lane_inputs(client, max_inputs).await {
        if first_error.is_none() {
            first_error = Some(error);
        }
    }
    match client.core_inner().local_state_db().await {
        Ok(db) => {
            if let Err(error) = db
                .purge_closed_sync_lane_inputs(unix_time_i64(), max_inputs)
                .await
            {
                if first_error.is_none() {
                    first_error = Some(error);
                }
            }
        }
        Err(error) => {
            if first_error.is_none() {
                first_error = Some(error);
            }
        }
    }
    match first_error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

async fn closed_lane_domain_status(
    db: &crate::internal::local_state::actor::LocalStateDb,
    owner_identity_id: &str,
    input_id: &str,
) -> crate::ImResult<Option<crate::internal::local_state::sync_v2::SyncLaneDomainStatus>> {
    Ok(db
        .load_sync_lane_domain_states(owner_identity_id.to_owned())
        .await?
        .into_iter()
        .find(|state| state.input_id == input_id && state.status.is_closed())
        .map(|state| state.status))
}

#[cfg(all(feature = "sqlite", feature = "group-e2ee"))]
async fn apply_p6_lane_delivery_projection_async(
    client: &crate::core::ImClient,
    envelope: &Value,
) -> crate::ImResult<crate::messages::Message> {
    let mut projected = p6_projection_for_application(envelope)?;
    crate::internal::message_runtime::read::clear_untrusted_p6_projection_state(
        std::slice::from_mut(&mut projected),
    );
    let cached_indices =
        crate::internal::message_runtime::read::apply_cached_group_e2ee_messages_async(
            client,
            std::slice::from_mut(&mut projected),
        )
        .await;
    if !cached_indices.contains(&0) {
        crate::internal::message_runtime::read::project_p6_v2_incoming_message(
            client,
            &mut projected,
        )
        .await?;
    } else if let Some(object) = projected.as_object_mut() {
        object.remove("meta");
        object.remove("body");
        object.remove("auth");
    }
    crate::internal::message_runtime::read::cache_attachment_manifests_for_internal_download_async(
        client,
        std::slice::from_ref(&projected),
    )
    .await;
    crate::internal::message_runtime::read::redact_attachment_manifests_for_public_projection(
        std::slice::from_mut(&mut projected),
    );
    let raw = json!({"messages": [projected], "has_more": false});
    let page = super::read::page_from_raw(client, &raw, crate::ids::PageLimit::new(1)?)?;
    let [message] = page.items.as_slice() else {
        return Err(sync_error(
            "SYNC_P6_APPLICATION_INCOMPLETE",
            "P6 delivery did not produce one durable message projection",
        ));
    };
    let outcome = super::read::persist_projection_async(
        client,
        std::slice::from_ref(message),
        &super::read::DirectP5ProjectionProvenance::default(),
    )
    .await?;
    if outcome
        .stored_messages
        .saturating_add(outcome.backlogged_messages)
        != 1
    {
        return Err(sync_error(
            "SYNC_P6_APPLICATION_INCOMPLETE",
            "P6 delivery was not durably stored or backlogged",
        ));
    }
    Ok(message.clone())
}

#[cfg(all(feature = "sqlite", feature = "group-e2ee"))]
fn p6_projection_for_application(envelope: &Value) -> crate::ImResult<Value> {
    let mut projected = envelope.clone();
    let object = projected
        .as_object_mut()
        .ok_or_else(|| sync_error("SYNC_INVALID_PAGE", "P6 envelope must be an object"))?;
    let meta = object
        .get("meta")
        .and_then(Value::as_object)
        .cloned()
        .ok_or_else(|| sync_error("SYNC_INVALID_PAGE", "P6 envelope meta is missing"))?;
    let body = object
        .get("body")
        .and_then(Value::as_object)
        .cloned()
        .ok_or_else(|| sync_error("SYNC_INVALID_PAGE", "P6 envelope body is missing"))?;
    let group_did = body
        .get("group_did")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty() && value.trim() == *value)
        .ok_or_else(|| sync_error("SYNC_INVALID_PAGE", "P6 body.group_did is invalid"))?;
    let group_event_seq = body
        .get("group_event_seq")
        .and_then(|value| {
            value
                .as_str()
                .map(ToOwned::to_owned)
                .or_else(|| value.as_u64().map(|value| value.to_string()))
        })
        .ok_or_else(|| sync_error("SYNC_INVALID_PAGE", "P6 body.group_event_seq is invalid"))?;
    crate::internal::local_state::sync_v2::validate_positive_decimal(
        "group_event_seq",
        &group_event_seq,
    )?;
    let raw_message_id = meta
        .get("message_id")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty() && value.trim() == *value)
        .ok_or_else(|| sync_error("SYNC_INVALID_PAGE", "P6 meta.message_id is invalid"))?;
    for (key, value) in [
        (
            "id",
            Value::String(format!("{group_did}:{group_event_seq}")),
        ),
        ("message_id", Value::String(raw_message_id.to_owned())),
        ("raw_message_id", Value::String(raw_message_id.to_owned())),
        ("group_did", Value::String(group_did.to_owned())),
        ("group_event_seq", Value::String(group_event_seq)),
        ("direction", Value::from(0)),
    ] {
        object.insert(key.to_owned(), value);
    }
    for key in ["group_state_version", "accepted_at", "group_cipher_object"] {
        if let Some(value) = body.get(key).cloned() {
            object.insert(key.to_owned(), value);
        }
    }
    for key in ["sender_did", "sender_device_id", "content_type"] {
        if let Some(value) = meta.get(key).cloned() {
            object.insert(key.to_owned(), value);
        }
    }
    if let Some(receiver_did) = meta
        .get("target")
        .and_then(Value::as_object)
        .and_then(|target| target.get("did"))
        .cloned()
    {
        object.insert("receiver_did".to_owned(), receiver_did);
    }
    Ok(projected)
}

#[cfg(all(feature = "sqlite", not(feature = "group-e2ee")))]
async fn apply_p6_lane_delivery_projection_async(
    _client: &crate::core::ImClient,
    _envelope: &Value,
) -> crate::ImResult<crate::messages::Message> {
    Err(crate::ImError::unsupported("group-e2ee"))
}

impl<'a, P, T, R> MessageSyncRuntimeV2<'a, P, T, R> {
    pub(crate) fn new(
        client: &'a crate::core::ImClient,
        session_provider: P,
        transport: T,
        directory_transport: R,
    ) -> Self {
        Self {
            client,
            session_provider,
            transport,
            directory_transport,
            run_deadline: SYNC_RUN_DEADLINE,
        }
    }

    #[cfg(test)]
    fn with_run_deadline_for_test(mut self, run_deadline: StdDuration) -> Self {
        self.run_deadline = run_deadline;
        self
    }
}

impl<P, T, R> MessageSyncRuntimeV2<'_, P, T, R>
where
    P: AsyncSessionProvider,
    T: AsyncAuthenticatedRpcTransport,
    R: AsyncRpcTransport,
{
    pub(crate) async fn sync_now(
        mut self,
        request: crate::messages::MessageSyncRequest,
    ) -> crate::ImResult<crate::messages::MessageSyncOutcome> {
        self.session_provider
            .ensure_session(crate::auth::AuthScope::Messaging)
            .await?;
        let binding = self.client.active_sync_account_binding().await?;
        let db = self.client.core_inner().local_state_db().await?;
        let run = db
            .begin_message_sync_run(&binding.owner_identity_id, unix_time_i64())
            .await?;
        wake_sync_lane_consumers(self.client);
        let run_started = Instant::now();
        let run_deadline = self.run_deadline;
        let final_result = match tokio::time::timeout(run_deadline, async {
            let mut device_epoch_refresh_attempted = false;
            loop {
                match self
                    .sync_now_once(&request, run.run_generation, run_started)
                    .await
                {
                    Err(error)
                        if !device_epoch_refresh_attempted && is_device_epoch_rejection(&error) =>
                    {
                        device_epoch_refresh_attempted = true;
                        self.refresh_session_and_lane_epoch().await?;
                    }
                    Ok((mut outcome, Some(_error))) if !device_epoch_refresh_attempted => {
                        self.refresh_session_and_lane_epoch().await?;
                        let binding = self.client.active_sync_account_binding().await?;
                        let db = self.client.core_inner().local_state_db().await?;
                        match self.drain_read_outbox(&db, &binding).await {
                            Ok(None) => break Ok(outcome),
                            Ok(Some(error)) => break Err(error),
                            Err(_) => {
                                outcome
                                    .warnings
                                    .push("sync.read_state_writeback_deferred".to_owned());
                                break Ok(outcome);
                            }
                        }
                    }
                    Ok((_outcome, Some(error))) => break Err(error),
                    Ok((outcome, None)) => break Ok(outcome),
                    Err(error) => break Err(error),
                }
            }
        })
        .await
        {
            Ok(result) => result,
            Err(_) => {
                let mut outcome = empty_outcome();
                outcome.warnings.push("sync.budget_exhausted".to_owned());
                Ok(outcome)
            }
        };
        let budget_pending = final_result.as_ref().is_ok_and(|outcome| {
            outcome
                .warnings
                .iter()
                .any(|warning| warning == "sync.budget_exhausted")
        });
        let classified_failure = final_result.as_ref().err().and_then(failure_outcome);
        let error_retryable = final_result.is_err()
            && !classified_failure.as_ref().is_some_and(|outcome| {
                matches!(
                    outcome.status,
                    crate::messages::MessageSyncStatus::Blocked
                        | crate::messages::MessageSyncStatus::AuthRevoked
                )
            });
        let now = unix_time_i64();
        let last_result_json = match &final_result {
            Ok(outcome) => json!({
                "status": outcome.status,
                "pages_fetched": outcome.pages_fetched,
                "events_applied": outcome.events_applied,
                "budget_pending": budget_pending,
                "elapsed_ms": run_started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
            }),
            Err(error) => json!({
                "status": classified_failure
                    .as_ref()
                    .map(|outcome| outcome.status)
                    .unwrap_or(crate::messages::MessageSyncStatus::RetryableFailure),
                "error": error.to_string(),
                "elapsed_ms": run_started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
            }),
        }
        .to_string();
        let current_generation = db
            .finish_message_sync_run(crate::internal::local_state::sync_v2::MessageSyncRunState {
                owner_identity_id: run.owner_identity_id,
                sync_pending: budget_pending || error_retryable,
                run_generation: run.run_generation,
                next_retry_at: error_retryable.then(|| now.saturating_add(1)),
                last_result_json: Some(last_result_json),
                updated_at: now,
            })
            .await?;
        if !current_generation {
            return Err(sync_error(
                "SYNC_RUN_SUPERSEDED",
                "a newer sync run superseded this result",
            ));
        }
        final_result
    }

    async fn refresh_session_and_lane_epoch(&mut self) -> crate::ImResult<()> {
        self.session_provider.refresh_session().await?;
        self.transport.reload_authentication_state()?;
        let binding = self.client.active_sync_account_binding().await?;
        let db = self.client.core_inner().local_state_db().await?;
        if !db
            .load_lane_sync_states(binding.owner_identity_id.clone())
            .await?
            .is_empty()
        {
            let _ = self.refresh_lane_bootstrap(&db, &binding).await?;
        }
        Ok(())
    }

    async fn sync_now_once(
        &mut self,
        request: &crate::messages::MessageSyncRequest,
        run_generation: i64,
        run_started: Instant,
    ) -> crate::ImResult<(crate::messages::MessageSyncOutcome, Option<crate::ImError>)> {
        self.session_provider
            .ensure_session(crate::auth::AuthScope::Messaging)
            .await?;
        let limit = crate::internal::wire::sync::validate_limit(request.limit.unwrap_or(100))?;
        let reason = request.reason.trim();
        if reason.is_empty() || reason != request.reason {
            return Err(crate::ImError::invalid_input(
                Some("reason".to_owned()),
                "message sync reason must be a non-empty canonical string",
            ));
        }
        let binding = self.client.active_sync_account_binding().await?;
        let db = self.client.core_inner().local_state_db().await?;
        let owner_identity_id = binding.owner_identity_id.clone();
        let mut result = empty_outcome();
        let pending_peer_dids = db
            .list_pending_inbound_resolution_peer_dids(
                owner_identity_id.clone(),
                PENDING_PERSONA_RESOLUTION_LIMIT,
            )
            .await?;
        let replayed_conversations = self
            .resolve_unresolved_peer_dids(&db, &binding, pending_peer_dids, &mut result.warnings)
            .await?;
        if !replayed_conversations.is_empty() {
            result
                .changed_conversation_ids
                .extend(replayed_conversations);
            self.client
                .emit_committed_local_message_projection("sync_v2_identity_replay");
        }
        let mut state = match db
            .load_message_sync_state(owner_identity_id.clone())
            .await?
        {
            crate::internal::local_state::sync_v2::MessageSyncStateAccess::Ready(state) => state,
            crate::internal::local_state::sync_v2::MessageSyncStateAccess::BootstrapRequired(_) => {
                self.bootstrap(&db, &binding, run_generation, &mut result)
                    .await?
            }
        };
        if db
            .lane_capability_negotiation_required(
                owner_identity_id.clone(),
                binding.device_auth_generation.clone(),
            )
            .await?
        {
            self.refresh_lane_bootstrap(&db, &binding).await?;
        }
        let mut lane_states =
            lane_state_map(db.load_lane_sync_states(owner_identity_id.clone()).await?);
        let p6_client_instance_id = db
            .load_or_create_sync_client_instance_id(owner_identity_id)
            .await?;
        let mut recovery_token_retries = 0_u8;
        let mut failure_fingerprints = BTreeMap::new();
        let mut blocked_lanes = BTreeSet::new();
        let mut p5_lane_recovery_attempted = false;
        loop {
            let cursor = crate::internal::wire::sync_v2::SyncCursorV2 {
                stream_epoch: state.stream_epoch.clone(),
                scan_seq: state.scan_seq.clone(),
            };
            let requested_lanes = lane_states
                .iter()
                .filter(|(lane, _)| !blocked_lanes.contains(*lane))
                .map(|(lane, state)| {
                    (
                        *lane,
                        crate::internal::wire::sync_v2::SyncLaneCursorV3 {
                            cursor: crate::internal::wire::sync_v2::SyncCursorV2 {
                                stream_epoch: state.stream_epoch.clone(),
                                scan_seq: state.scan_seq.clone(),
                            },
                            committed_seq: state.committed_seq.clone(),
                        },
                    )
                })
                .collect::<BTreeMap<_, _>>();
            let params = if requested_lanes.is_empty() {
                crate::internal::wire::sync_v2::build_delta_params(
                    &wire_identity(self.client),
                    &cursor,
                    limit,
                    reason,
                    &p6_client_instance_id,
                )?
            } else {
                crate::internal::wire::sync_v2::build_delta_params_with_lanes(
                    &wire_identity(self.client),
                    &cursor,
                    limit,
                    reason,
                    &requested_lanes,
                    &p6_client_instance_id,
                )?
            };
            let raw = self
                .transport
                .authenticated_rpc(MESSAGE_RPC_ENDPOINT, "sync.delta", params)
                .await?;
            let page = match crate::internal::wire::sync_v2::parse_delta_response(&raw)? {
                crate::internal::wire::sync_v2::SyncDeltaResponseV2::Delta(page) => page,
                crate::internal::wire::sync_v2::SyncDeltaResponseV2::RecoveryRequired(recovery) => {
                    match self
                        .recover_snapshot(
                            &db,
                            &binding,
                            &state,
                            &recovery,
                            None,
                            run_generation,
                            &mut result,
                        )
                        .await
                    {
                        Ok(next) => {
                            state = next;
                            recovery_token_retries = 0;
                            continue;
                        }
                        Err(error)
                            if matches!(
                                error_code(&error),
                                Some("sync.recovery_token_invalid" | "sync.recovery_token_expired")
                            ) && recovery_token_retries < 1 =>
                        {
                            recovery_token_retries += 1;
                            self.persist_recovery_failure(
                                &db,
                                &binding.owner_identity_id,
                                &state,
                                error_code(&error).unwrap_or("SYNC_RECOVERY_RETRY"),
                            )
                            .await?;
                            continue;
                        }
                        Err(error) => {
                            self.persist_recovery_failure(
                                &db,
                                &binding.owner_identity_id,
                                &state,
                                error_code(&error).unwrap_or("SYNC_RECOVERY_FAILED"),
                            )
                            .await?;
                            return Err(error);
                        }
                    }
                }
            };
            validate_page_binding(&page, &binding, &state)?;
            result.pages_fetched = result.pages_fetched.saturating_add(1);
            result.warnings.extend(page.warnings.clone());
            let lane_has_more = self
                .apply_lane_delta_sections(
                    &db,
                    &binding,
                    &p6_client_instance_id,
                    &page.lanes,
                    page.lane_transport_invalid,
                    &requested_lanes,
                    &mut lane_states,
                    &mut blocked_lanes,
                    &mut p5_lane_recovery_attempted,
                    &mut result,
                )
                .await?;
            wake_sync_lane_consumers(self.client);

            for event in page
                .events
                .iter()
                .filter(|event| event.event_type == "system.notification")
            {
                validate_system_notification_event_contract(self.client, &binding, event)?;
            }
            let hydration_event_ids = page
                .events
                .iter()
                .filter(|event| {
                    matches!(
                        event.event_type.as_str(),
                        "message.created" | "system.notification"
                    )
                })
                .map(|event| event.event_id.clone())
                .collect::<Vec<_>>();
            let ordinary_hydration_count = page
                .events
                .iter()
                .filter(|event| event.event_type == "message.created")
                .count();
            let hydrated = if hydration_event_ids.is_empty() {
                BTreeMap::new()
            } else {
                let (hydrated, _) = hydrate_required_messages_with_budget(
                    &mut self.transport,
                    &wire_identity(self.client),
                    &hydration_event_ids,
                    &mut failure_fingerprints,
                )
                .await?;
                result.messages_hydrated = result
                    .messages_hydrated
                    .saturating_add(u32::try_from(ordinary_hydration_count).unwrap_or(u32::MAX));
                hydrated
            };

            let mut verified_system_notifications = BTreeMap::new();
            for event in page
                .events
                .iter()
                .filter(|event| event.event_type == "system.notification")
            {
                let hydrated_projection = hydrated.get(&event.event_id).ok_or_else(|| {
                    sync_error(
                        "SYNC_HYDRATION_INCOMPLETE",
                        "system.notification has no exact hydrated projection",
                    )
                })?;
                let input = prepare_system_notification(
                    self.client,
                    &binding,
                    event,
                    hydrated_projection,
                    &mut self.directory_transport,
                )
                .await?;
                verified_system_notifications.insert(event.event_id.clone(), input);
            }

            let mut public_messages = BTreeMap::new();
            let apply_events = page
                .events
                .iter()
                .map(|event| {
                    reduce_event(
                        self.client,
                        event,
                        hydrated.get(&event.event_id),
                        verified_system_notifications.remove(&event.event_id),
                        &mut public_messages,
                    )
                })
                .collect::<crate::ImResult<Vec<_>>>()?;
            let direct_peer_dids = direct_peer_dids_from_events(&apply_events);
            let resolved_conversations = self
                .resolve_unresolved_peer_dids(&db, &binding, direct_peer_dids, &mut result.warnings)
                .await?;
            result
                .changed_conversation_ids
                .extend(resolved_conversations);
            require_current_sync_run_generation(&db, &binding.owner_identity_id, run_generation)
                .await?;
            let outcome = db
                .apply_sync_delta_v2(crate::internal::local_state::sync_v2::DeltaApplyInputV2 {
                    owner_identity_id: binding.owner_identity_id.clone(),
                    expected_run_generation: Some(run_generation),
                    owner_did: binding.current_did.clone(),
                    account_id: binding.account_id.clone(),
                    protocol_device_id: binding.protocol_device_id.clone(),
                    device_auth_generation: binding.device_auth_generation.clone(),
                    stream_epoch: page.next_cursor.stream_epoch.clone(),
                    next_scan_seq: page.next_cursor.scan_seq.clone(),
                    server_time: page.server_time.clone(),
                    events: apply_events,
                })
                .await?;
            apply_p4_terminal_events(self.client, &page.events)?;
            result.events_applied = result
                .events_applied
                .saturating_add(u32::try_from(outcome.applied_event_ids.len()).unwrap_or(u32::MAX));
            result.duplicates_skipped = result
                .duplicates_skipped
                .saturating_add(u32::try_from(outcome.duplicate_events).unwrap_or(u32::MAX));
            append_backlog_warning(&mut result.warnings, outcome.backlogged_messages);
            result
                .changed_conversation_ids
                .extend(outcome.invalidation.conversation_ids.clone());
            for event_id in &outcome.projected_message_event_ids {
                if let Some(message) = public_messages.get(event_id) {
                    if message.direction == crate::messages::MessageDirection::Incoming {
                        result.committed_incoming_messages.push(
                            crate::messages::CommittedIncomingMessage {
                                event_id: event_id.clone(),
                                logical_message_id: message.id.as_str().to_owned(),
                                source: "live_delta".to_owned(),
                                direction: crate::messages::MessageDirection::Incoming,
                                message: message.clone(),
                            },
                        );
                    }
                }
            }
            for notification in &outcome.committed_system_notifications {
                self.client
                    .emit_committed_system_notification(notification.clone());
            }
            super::sync::emit_committed_sync_invalidation(self.client, &outcome.invalidation);
            state.scan_seq = page.next_cursor.scan_seq;
            state.stream_epoch = page.next_cursor.stream_epoch;
            state.bootstrap_state = "active".to_owned();
            state.last_server_time = Some(page.server_time);
            state.last_success_at = Some(unix_time_i64());
            let has_continuation = page.has_more || lane_has_more;
            if has_continuation
                && (result.pages_fetched >= SYNC_RUN_MAX_PAGES
                    || run_started.elapsed() >= SYNC_RUN_DEADLINE)
            {
                result.warnings.push("sync.budget_exhausted".to_owned());
                result.changed_conversation_ids.sort();
                result.changed_conversation_ids.dedup();
                result.status =
                    if result.events_applied == 0 && result.changed_conversation_ids.is_empty() {
                        crate::messages::MessageSyncStatus::Idle
                    } else {
                        crate::messages::MessageSyncStatus::Changed
                    };
                return Ok((best_effort_cleanup(&db, &state, result).await, None));
            }
            if page.has_more && state.scan_seq == cursor.scan_seq {
                return Err(sync_error(
                    "SYNC_INVALID_PAGE",
                    "sync.delta returned has_more without cursor progress",
                ));
            }
            if !page.has_more && !lane_has_more {
                result.changed_conversation_ids.sort();
                result.changed_conversation_ids.dedup();
                result.status =
                    if result.events_applied == 0 && result.changed_conversation_ids.is_empty() {
                        crate::messages::MessageSyncStatus::Idle
                    } else {
                        crate::messages::MessageSyncStatus::Changed
                    };
                let device_epoch_rejection = match self.drain_read_outbox(&db, &binding).await {
                    Ok(error) => error,
                    Err(_) => {
                        result
                            .warnings
                            .push("sync.read_state_writeback_deferred".to_owned());
                        None
                    }
                };
                return Ok((
                    best_effort_cleanup(&db, &state, result).await,
                    device_epoch_rejection,
                ));
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn apply_lane_delta_sections(
        &mut self,
        db: &crate::internal::local_state::actor::LocalStateDb,
        binding: &crate::identity::ActiveSyncAccountBinding,
        client_instance_id: &str,
        sections: &BTreeMap<
            crate::internal::wire::sync_v2::SyncLaneV3,
            crate::internal::wire::sync_v2::SyncLaneDeltaSectionV3,
        >,
        transport_invalid: bool,
        requested: &BTreeMap<
            crate::internal::wire::sync_v2::SyncLaneV3,
            crate::internal::wire::sync_v2::SyncLaneCursorV3,
        >,
        lane_states: &mut BTreeMap<
            crate::internal::wire::sync_v2::SyncLaneV3,
            crate::internal::local_state::sync_v2::LaneSyncState,
        >,
        blocked_lanes: &mut BTreeSet<crate::internal::wire::sync_v2::SyncLaneV3>,
        p5_lane_recovery_attempted: &mut bool,
        result: &mut crate::messages::MessageSyncOutcome,
    ) -> crate::ImResult<bool> {
        use crate::internal::wire::sync_v2::{SyncLaneDeltaSectionV3, SyncLaneEventV3, SyncLaneV3};

        if transport_invalid {
            result
                .warnings
                .push("sync.lane.transport_invalid".to_owned());
        }
        for lane in requested.keys().filter(|lane| !sections.contains_key(lane)) {
            result
                .warnings
                .push(format!("sync.lane.{}.missing", lane.as_str()));
            blocked_lanes.insert(*lane);
        }
        let mut any_has_more = false;
        for lane in [SyncLaneV3::P5Device, SyncLaneV3::P6Group] {
            let Some(section) = sections.get(&lane) else {
                continue;
            };
            if !requested.contains_key(&lane) {
                result
                    .warnings
                    .push(format!("sync.lane.{}.unrequested", lane.as_str()));
                continue;
            }
            match section {
                SyncLaneDeltaSectionV3::TransportInvalid => {
                    result
                        .warnings
                        .push(format!("sync.lane.{}.transport_invalid", lane.as_str()));
                    blocked_lanes.insert(lane);
                }
                SyncLaneDeltaSectionV3::Error(error) => {
                    result
                        .warnings
                        .push(format!("sync.lane.{}.{}", lane.as_str(), error.anp_code));
                    if lane == SyncLaneV3::P5Device
                        && error.anp_code == "p5_device_recovery_required"
                        && !*p5_lane_recovery_attempted
                    {
                        *p5_lane_recovery_attempted = true;
                        match self.refresh_lane_bootstrap(db, binding).await {
                            Ok(refreshed) => {
                                *lane_states = refreshed;
                                return Ok(true);
                            }
                            Err(_) => result
                                .warnings
                                .push("sync.lane.p5_device.recovery_deferred".to_owned()),
                        }
                    }
                    blocked_lanes.insert(lane);
                }
                SyncLaneDeltaSectionV3::Page {
                    events,
                    next_cursor,
                    has_more,
                } => {
                    let current = lane_states.get(&lane).cloned().ok_or_else(|| {
                        sync_error(
                            "SYNC_LANE_BOOTSTRAP_REQUIRED",
                            "requested lane has no local bootstrap state",
                        )
                    })?;
                    if current.stream_epoch != next_cursor.stream_epoch
                        || crate::internal::local_state::sync_v2::compare_decimal(
                            &next_cursor.scan_seq,
                            &current.scan_seq,
                        )? == std::cmp::Ordering::Less
                    {
                        return Err(sync_error(
                            "SYNC_INVALID_PAGE",
                            "lane next cursor conflicts with the local checkpoint",
                        ));
                    }
                    validate_lane_page_progress(&current, events, next_cursor)?;
                    let received_at = chrono::Utc::now().to_rfc3339();
                    let mut handoff_failed = false;
                    for event in events {
                        let event_matches_lane = matches!(
                            (lane, event),
                            (SyncLaneV3::P5Device, SyncLaneEventV3::P5Delivery { .. })
                                | (
                                    SyncLaneV3::P6Group,
                                    SyncLaneEventV3::P6Delivery { .. }
                                        | SyncLaneEventV3::P6ControlNotice { .. }
                                )
                        );
                        if !event_matches_lane
                            || (lane == SyncLaneV3::P6Group
                                && validate_strict_p6_lane_wire(event).is_err())
                        {
                            result
                                .warnings
                                .push(format!("sync.lane.{}.transport_invalid", lane.as_str()));
                            blocked_lanes.insert(lane);
                            handoff_failed = true;
                            break;
                        }
                        let input = lane_handoff_input_from_wire(
                            binding,
                            client_instance_id,
                            &current.stream_epoch,
                            event,
                            &received_at,
                        )?;
                        match db.commit_sync_lane_handoff(input).await {
                            Ok(
                                crate::internal::local_state::sync_v2::SyncLaneHandoffOutcome::Inserted,
                            ) => {}
                            Ok(
                                crate::internal::local_state::sync_v2::SyncLaneHandoffOutcome::Duplicate,
                            ) => {
                                result.duplicates_skipped =
                                    result.duplicates_skipped.saturating_add(1);
                            }
                            Err(crate::ImError::Service { code, .. })
                                if matches!(
                                    code.as_deref(),
                                    Some("LANE_STORAGE_PRESSURE" | "LANE_INPUT_CONFLICT")
                                ) =>
                            {
                                let warning = if code.as_deref()
                                    == Some("LANE_STORAGE_PRESSURE")
                                {
                                    "lane_storage_pressure"
                                } else {
                                    "lane_input_conflict"
                                };
                                result.warnings.push(format!(
                                    "sync.lane.{}.{}",
                                    lane.as_str(),
                                    warning
                                ));
                                blocked_lanes.insert(lane);
                                handoff_failed = true;
                                break;
                            }
                            Err(error) => return Err(error),
                        }
                    }
                    if handoff_failed {
                        continue;
                    }
                    let previous_scan_seq = lane_states
                        .get(&lane)
                        .map(|state| state.scan_seq.clone())
                        .ok_or_else(|| {
                            sync_error(
                                "SYNC_LANE_BOOTSTRAP_REQUIRED",
                                "lane state disappeared during handoff",
                            )
                        })?;
                    let next = crate::internal::local_state::sync_v2::LaneSyncState {
                        owner_identity_id: binding.owner_identity_id.clone(),
                        lane,
                        stream_epoch: next_cursor.stream_epoch.clone(),
                        scan_seq: next_cursor.scan_seq.clone(),
                        committed_seq: next_cursor.scan_seq.clone(),
                    };
                    // Each complete event has already advanced the durable lane
                    // checkpoint in the same transaction as its inbox insert.
                    // `validate_lane_page_progress` guarantees that the last
                    // event position equals `next_cursor`, so a second page-level
                    // database writer would only add a redundant CAS window.
                    lane_states.insert(lane, next);
                    if *has_more && previous_scan_seq == next_cursor.scan_seq {
                        return Err(sync_error(
                            "SYNC_INVALID_PAGE",
                            "lane returned has_more without cursor progress",
                        ));
                    }
                    any_has_more |= *has_more;
                }
            }
        }
        Ok(any_has_more)
    }

    async fn refresh_lane_bootstrap(
        &mut self,
        db: &crate::internal::local_state::actor::LocalStateDb,
        binding: &crate::identity::ActiveSyncAccountBinding,
    ) -> crate::ImResult<
        BTreeMap<
            crate::internal::wire::sync_v2::SyncLaneV3,
            crate::internal::local_state::sync_v2::LaneSyncState,
        >,
    > {
        refresh_lane_bootstrap_with_transport_async(self.client, &mut self.transport, db, binding)
            .await
    }

    async fn resolve_unresolved_peer_dids(
        &mut self,
        db: &crate::internal::local_state::actor::LocalStateDb,
        binding: &crate::identity::ActiveSyncAccountBinding,
        dids: Vec<String>,
        warnings: &mut Vec<String>,
    ) -> crate::ImResult<Vec<String>> {
        let dids = db
            .filter_unresolved_peer_dids(binding.owner_identity_id.clone(), dids)
            .await?;
        let mut conversations = Vec::new();
        for did in dids {
            let did = crate::ids::Did::parse(&did)?;
            let lookup =
                crate::internal::directory_runtime::lookup_handle_by_did_for_projection_async(
                    self.client,
                    &mut self.directory_transport,
                    &did,
                )
                .await;
            let Ok(lookup) = lookup else {
                push_identity_resolution_deferred(warnings);
                continue;
            };
            match crate::directory::project_handle_lookup_async(self.client, &lookup).await {
                Ok(()) => conversations.push(lookup.direct_conversation_id()),
                Err(_) => push_identity_resolution_deferred(warnings),
            }
        }
        conversations.sort();
        conversations.dedup();
        Ok(conversations)
    }

    async fn drain_read_outbox(
        &mut self,
        db: &crate::internal::local_state::actor::LocalStateDb,
        binding: &crate::identity::ActiveSyncAccountBinding,
    ) -> crate::ImResult<Option<crate::ImError>> {
        for _ in 0..16 {
            let now = unix_time_i64();
            let Some(record) = db
                .claim_next_read_mutation(&binding.owner_identity_id, now)
                .await?
            else {
                break;
            };
            if let Err(error) = self.send_claimed_read_mutation(db, binding, &record).await {
                if error_code(&error) == Some("SYNC_LOCAL_OUTBOX_CORRUPT") {
                    db.permanently_fail_local_mutation(
                        &binding.owner_identity_id,
                        &record.mutation_id,
                        "SYNC_LOCAL_OUTBOX_CORRUPT",
                        now,
                    )
                    .await?;
                    continue;
                }
                let device_epoch_rejection = is_device_epoch_rejection(&error);
                let target_not_found = is_read_target_not_found(&record, &error);
                let sequence_only_group_target_not_found =
                    target_not_found && is_sequence_only_group_read(&record);
                if sequence_only_group_target_not_found
                    && record.attempt_count >= GROUP_SEQUENCE_ONLY_TARGET_NOT_FOUND_MAX_ATTEMPTS
                {
                    db.permanently_fail_local_mutation(
                        &binding.owner_identity_id,
                        &record.mutation_id,
                        error_code(&error).unwrap_or("READ_STATE_TARGET_NOT_FOUND"),
                        now,
                    )
                    .await?;
                    continue;
                }
                let retry_delay = if sequence_only_group_target_not_found {
                    sequence_only_group_read_retry_delay(record.attempt_count)
                } else if target_not_found && read_mutation_thread_kind(&record) == Some("direct") {
                    300
                } else {
                    5
                };
                let retry_at = if device_epoch_rejection {
                    now
                } else {
                    now.saturating_add(retry_delay)
                };
                db.retry_local_mutation(
                    &binding.owner_identity_id,
                    &record.mutation_id,
                    error_code(&error).unwrap_or("READ_STATE_RETRY"),
                    retry_at,
                )
                .await?;
                if device_epoch_rejection {
                    return Ok(Some(error));
                }
            }
        }
        Ok(None)
    }

    async fn send_claimed_read_mutation(
        &mut self,
        db: &crate::internal::local_state::actor::LocalStateDb,
        binding: &crate::identity::ActiveSyncAccountBinding,
        record: &crate::internal::local_state::sync_v2::LocalMutationRecord,
    ) -> crate::ImResult<()> {
        let payload: Value = serde_json::from_str(&record.payload_json).map_err(|error| {
            sync_error(
                "SYNC_LOCAL_OUTBOX_CORRUPT",
                format!("read outbox payload is invalid: {error}"),
            )
        })?;
        let field = |name: &str| {
            payload
                .get(name)
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .map(ToOwned::to_owned)
                .ok_or_else(|| {
                    sync_error(
                        "SYNC_LOCAL_OUTBOX_CORRUPT",
                        format!("read outbox payload is missing {name}"),
                    )
                })
        };
        let thread_kind = field("thread_kind")?;
        let thread_id = field("thread_id")?;
        let remote_thread_key =
            canonical_read_remote_thread_key(&thread_kind, &field("remote_thread_key")?);
        let requested_seq = field("read_watermark_seq")?;
        crate::internal::local_state::sync_v2::validate_decimal(
            "read_watermark_seq",
            &requested_seq,
        )?;
        let thread = match thread_kind.as_str() {
            "group" => crate::messages::ThreadRef::Group(crate::ids::GroupRef::parse(&thread_id)?),
            "direct" => {
                crate::messages::ThreadRef::Thread(crate::ids::ThreadId::parse(&thread_id)?)
            }
            _ => {
                return Err(sync_error(
                    "SYNC_LOCAL_OUTBOX_CORRUPT",
                    "read outbox thread_kind must be direct or group",
                ))
            }
        };
        let expected_thread = crate::internal::wire::read_state::read_state_thread_to_wire(
            &thread,
            Some(&remote_thread_key),
        )?;
        let params = crate::internal::wire::read_state::build_mark_read_state_rpc_params(
            &wire_identity(self.client),
            crate::internal::wire::read_state::MarkReadStateWireRequest {
                thread: thread.clone(),
                read_up_to_server_seq: Some(requested_seq.clone()),
                read_up_to_message_id: payload
                    .get("read_watermark_message_id")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned),
                client_observed_at: payload
                    .get("read_watermark_at")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned),
                fallback_max_message_ids: None,
                device_id: Some(binding.protocol_device_id.clone()),
                operation_id: Some(record.operation_id.clone()),
                remote_thread_key: Some(remote_thread_key),
            },
        )?;
        let raw = self
            .transport
            .authenticated_rpc(MESSAGE_RPC_ENDPOINT, "read_state.mark_read", params)
            .await?;
        let response = crate::internal::wire::read_state::parse_mark_read_state_response(
            &raw,
            &binding.current_did,
            &expected_thread,
        )?;
        let acknowledged_seq = response
            .read_watermark_server_seq
            .as_deref()
            .ok_or_else(|| incomplete_read_ack("read ACK has no server watermark"))?;
        if !response.remote_acknowledged
            || response.pending_remote_ack
            || response.partial
            || crate::internal::local_state::sync_v2::compare_decimal(
                acknowledged_seq,
                &requested_seq,
            )? == std::cmp::Ordering::Less
        {
            return Err(incomplete_read_ack(
                "read ACK is not final or is below the sent watermark",
            ));
        }
        db.mark_thread_read_watermark(
            &binding.owner_identity_id,
            &binding.current_did,
            crate::internal::local_state::messages::MarkThreadReadWatermarkInput {
                thread,
                read_watermark_message_id: response.read_watermark_message_id,
                read_watermark_seq: Some(acknowledged_seq.to_owned()),
                read_watermark_at: Some(response.read_at),
                pending_remote_ack: false,
            },
        )
        .await?;
        Ok(())
    }

    async fn recover_snapshot(
        &mut self,
        db: &crate::internal::local_state::actor::LocalStateDb,
        binding: &crate::identity::ActiveSyncAccountBinding,
        previous: &crate::internal::local_state::sync_v2::MessageSyncState,
        recovery: &crate::internal::wire::sync_v2::SyncRecoveryV2,
        lane_capability_reconcile: Option<
            crate::internal::local_state::sync_v2::LaneCapabilityReconcileInputV1a,
        >,
        run_generation: i64,
        result: &mut crate::messages::MessageSyncOutcome,
    ) -> crate::ImResult<crate::internal::local_state::sync_v2::MessageSyncState> {
        let now = unix_time_i64();
        db.upsert_sync_recovery_state(crate::internal::local_state::sync_v2::RecoveryState {
            owner_identity_id: binding.owner_identity_id.clone(),
            mode: "compact_recovery".to_owned(),
            requested_from_epoch: previous.stream_epoch.clone(),
            requested_from_seq: previous.scan_seq.clone(),
            recovery_id_hash: Some(hex_sha256(&recovery.recovery_id)),
            snapshot_scan_seq: Some(recovery.snapshot_scan_seq.clone()),
            status: "downloading".to_owned(),
            retry_count: 0,
            last_error_code: None,
            started_at: now,
            updated_at: now,
        })
        .await?;
        let params = crate::internal::wire::sync_v2::build_snapshot_params(
            &wire_identity(self.client),
            recovery,
        )?;
        let raw = self
            .transport
            .authenticated_rpc(MESSAGE_RPC_ENDPOINT, "sync.snapshot", params)
            .await?;
        let snapshot = crate::internal::wire::sync_v2::parse_snapshot(&raw)?;
        if snapshot.snapshot_schema != recovery.snapshot_schema {
            return Err(sync_error(
                "SYNC_INVALID_SNAPSHOT",
                "sync.snapshot schema does not match the authorized recovery schema",
            ));
        }
        if snapshot.account_id != binding.account_id
            || snapshot.device_id != binding.protocol_device_id
        {
            return Err(sync_error(
                "SYNC_ACCOUNT_BINDING_MISMATCH",
                "sync.snapshot response does not match the active account device",
            ));
        }
        if snapshot.snapshot_cursor.stream_epoch != recovery.stream_epoch
            || snapshot.snapshot_cursor.scan_seq != recovery.snapshot_scan_seq
        {
            return Err(sync_error(
                "SYNC_INVALID_SNAPSHOT",
                "sync.snapshot cursor does not match the authorized recovery anchor",
            ));
        }
        db.upsert_sync_recovery_state(crate::internal::local_state::sync_v2::RecoveryState {
            owner_identity_id: binding.owner_identity_id.clone(),
            mode: "compact_recovery".to_owned(),
            requested_from_epoch: previous.stream_epoch.clone(),
            requested_from_seq: previous.scan_seq.clone(),
            recovery_id_hash: Some(hex_sha256(&recovery.recovery_id)),
            snapshot_scan_seq: Some(recovery.snapshot_scan_seq.clone()),
            status: "applying".to_owned(),
            retry_count: 0,
            last_error_code: None,
            started_at: now,
            updated_at: unix_time_i64(),
        })
        .await?;

        let mut public_messages = BTreeMap::new();
        let mut events = snapshot
            .recent_plain_messages
            .iter()
            .map(|item| {
                reduce_event(
                    self.client,
                    &item.event,
                    Some(&item.message),
                    None,
                    &mut public_messages,
                )
            })
            .collect::<crate::ImResult<Vec<_>>>()?;
        for item in &snapshot.unexpired_system_notifications {
            validate_system_notification_event_contract(self.client, binding, &item.event)?;
            let verified = prepare_system_notification(
                self.client,
                binding,
                &item.event,
                &item.message,
                &mut self.directory_transport,
            )
            .await?;
            events.push(reduce_event(
                self.client,
                &item.event,
                None,
                Some(verified),
                &mut public_messages,
            )?);
        }
        let direct_peer_dids = direct_peer_dids_from_events(&events);
        let resolved_conversations = self
            .resolve_unresolved_peer_dids(db, binding, direct_peer_dids, &mut result.warnings)
            .await?;
        result
            .changed_conversation_ids
            .extend(resolved_conversations);
        let groups = snapshot
            .groups
            .iter()
            .enumerate()
            .map(|(index, group)| {
                baseline_group_record(self.client, group, &snapshot.server_time, index)
            })
            .collect::<crate::ImResult<Vec<_>>>()?;
        let read_states = snapshot
            .read_states
            .iter()
            .map(read_state_from_snapshot)
            .collect::<crate::ImResult<Vec<_>>>()?;
        let outcome = db
            .apply_sync_snapshot_v2(
                crate::internal::local_state::sync_v2::SnapshotApplyInputV2 {
                    owner_identity_id: binding.owner_identity_id.clone(),
                    expected_run_generation: Some(run_generation),
                    owner_did: binding.current_did.clone(),
                    account_id: binding.account_id.clone(),
                    protocol_device_id: binding.protocol_device_id.clone(),
                    device_auth_generation: binding.device_auth_generation.clone(),
                    expected_stream_epoch: previous.stream_epoch.clone(),
                    expected_scan_seq: previous.scan_seq.clone(),
                    allow_missing_previous: previous.bootstrap_state == "uninitialized",
                    recovery_id_hash: hex_sha256(&recovery.recovery_id),
                    stream_epoch: snapshot.snapshot_cursor.stream_epoch.clone(),
                    snapshot_scan_seq: snapshot.snapshot_cursor.scan_seq.clone(),
                    server_time: snapshot.server_time.clone(),
                    server_cutoff: snapshot.server_cutoff.clone(),
                    lane_capability_reconcile,
                    events,
                    groups,
                    read_states,
                },
            )
            .await?;
        result.pages_fetched = result.pages_fetched.saturating_add(1);
        result.messages_hydrated = result.messages_hydrated.saturating_add(
            u32::try_from(snapshot.recent_plain_messages.len()).unwrap_or(u32::MAX),
        );
        result.events_applied = result
            .events_applied
            .saturating_add(u32::try_from(outcome.applied_event_ids.len()).unwrap_or(u32::MAX));
        result.duplicates_skipped = result
            .duplicates_skipped
            .saturating_add(u32::try_from(outcome.duplicate_events).unwrap_or(u32::MAX));
        result
            .changed_conversation_ids
            .extend(outcome.invalidation.conversation_ids.clone());
        super::sync::emit_committed_sync_invalidation(self.client, &outcome.invalidation);
        for snapshot in outcome.committed_system_notifications {
            self.client.emit_committed_system_notification(snapshot);
        }
        let next = crate::internal::local_state::sync_v2::MessageSyncState {
            owner_identity_id: binding.owner_identity_id.clone(),
            account_id: binding.account_id.clone(),
            protocol_device_id: binding.protocol_device_id.clone(),
            device_auth_generation: binding.device_auth_generation.clone(),
            stream_epoch: snapshot.snapshot_cursor.stream_epoch,
            scan_seq: snapshot.snapshot_cursor.scan_seq,
            bootstrap_state: "active".to_owned(),
            last_server_time: Some(snapshot.server_time),
            last_success_at: Some(unix_time_i64()),
            last_error_code: None,
            metadata_json: Some("{\"mode\":\"compact_recovery\"}".to_owned()),
            updated_at: unix_time_i64(),
        };
        let next = best_effort_cleanup(db, &next, next.clone()).await;
        Ok(next)
    }

    async fn persist_recovery_failure(
        &self,
        db: &crate::internal::local_state::actor::LocalStateDb,
        owner_identity_id: &str,
        previous: &crate::internal::local_state::sync_v2::MessageSyncState,
        code: &str,
    ) -> crate::ImResult<()> {
        let now = unix_time_i64();
        let current = db.load_sync_recovery_state(owner_identity_id).await?;
        db.upsert_sync_recovery_state(crate::internal::local_state::sync_v2::RecoveryState {
            owner_identity_id: owner_identity_id.to_owned(),
            mode: "compact_recovery".to_owned(),
            requested_from_epoch: previous.stream_epoch.clone(),
            requested_from_seq: previous.scan_seq.clone(),
            recovery_id_hash: current
                .as_ref()
                .and_then(|state| state.recovery_id_hash.clone()),
            snapshot_scan_seq: current
                .as_ref()
                .and_then(|state| state.snapshot_scan_seq.clone()),
            status: "retryable".to_owned(),
            retry_count: current.map_or(1, |state| state.retry_count.saturating_add(1)),
            last_error_code: Some(code.to_owned()),
            started_at: now,
            updated_at: now,
        })
        .await
    }

    async fn bootstrap(
        &mut self,
        db: &crate::internal::local_state::actor::LocalStateDb,
        binding: &crate::identity::ActiveSyncAccountBinding,
        run_generation: i64,
        result: &mut crate::messages::MessageSyncOutcome,
    ) -> crate::ImResult<crate::internal::local_state::sync_v2::MessageSyncState> {
        require_explicit_sync_negotiation(&mut self.transport, self.client).await?;
        let client_instance_id = db
            .load_or_create_sync_client_instance_id(&binding.owner_identity_id)
            .await?;
        let requested_lanes = desired_v1b_lanes(db, &binding.owner_identity_id).await?;
        let params = crate::internal::wire::sync_v2::build_bootstrap_params_with_lanes(
            &wire_identity(self.client),
            &client_instance_id,
            &requested_lanes,
        )?;
        let raw = self
            .transport
            .authenticated_rpc(MESSAGE_RPC_ENDPOINT, "sync.bootstrap", params)
            .await?;
        let response = crate::internal::wire::sync_v2::parse_bootstrap_response(&raw)?;
        if let crate::internal::wire::sync_v2::SyncBootstrapResponseV2::RecoveryRequired {
            account_id,
            device_id,
            recovery,
            lane_bootstrap,
            p6_delivery_client_instance_id,
        } = &response
        {
            if account_id != &binding.account_id
                || device_id != &binding.protocol_device_id
                || lane_bootstrap.capabilities != requested_lanes
                || !bootstrap_p6_activation_matches(
                    lane_bootstrap,
                    p6_delivery_client_instance_id.as_deref(),
                    &client_instance_id,
                )
            {
                return Err(sync_error(
                    "SYNC_ACCOUNT_BINDING_MISMATCH",
                    "sync.bootstrap recovery does not match the active account device",
                ));
            }
            let now = unix_time_i64();
            db.upsert_identity_account_binding(
                crate::internal::local_state::sync_v2::IdentityAccountBinding {
                    owner_identity_id: binding.owner_identity_id.clone(),
                    account_id: binding.account_id.clone(),
                    handle_scope: self
                        .client
                        .handle()
                        .map(|handle| handle.as_str().to_owned()),
                    current_did: binding.current_did.clone(),
                    protocol_device_id: binding.protocol_device_id.clone(),
                    identity_generation: binding.identity_generation.clone(),
                    device_auth_generation: binding.device_auth_generation.clone(),
                    created_at: now,
                    updated_at: now,
                },
            )
            .await?;
            let previous = crate::internal::local_state::sync_v2::MessageSyncState {
                owner_identity_id: binding.owner_identity_id.clone(),
                account_id: binding.account_id.clone(),
                protocol_device_id: binding.protocol_device_id.clone(),
                device_auth_generation: binding.device_auth_generation.clone(),
                stream_epoch: recovery.stream_epoch.clone(),
                scan_seq: "0".to_owned(),
                bootstrap_state: "uninitialized".to_owned(),
                last_server_time: None,
                last_success_at: None,
                last_error_code: None,
                metadata_json: None,
                updated_at: now,
            };
            let lane_capability_reconcile = Some(
                crate::internal::local_state::sync_v2::LaneCapabilityReconcileInputV1a {
                    client_instance_id: client_instance_id.clone(),
                    negotiated_capabilities_json: negotiated_lane_capabilities_json(
                        lane_bootstrap,
                    )?,
                    lane_states: lane_states_from_bootstrap(
                        &binding.owner_identity_id,
                        lane_bootstrap,
                    ),
                },
            );
            let state = self
                .recover_snapshot(
                    db,
                    binding,
                    &previous,
                    recovery,
                    lane_capability_reconcile,
                    run_generation,
                    result,
                )
                .await?;
            return Ok(state);
        }
        let crate::internal::wire::sync_v2::SyncBootstrapResponseV2::TailOnly(bootstrap) = response
        else {
            unreachable!("bootstrap response variants were handled")
        };
        if bootstrap.account_id != binding.account_id
            || bootstrap.device_id != binding.protocol_device_id
            || bootstrap.lane_bootstrap.capabilities != requested_lanes
            || !bootstrap_p6_activation_matches(
                &bootstrap.lane_bootstrap,
                bootstrap.p6_delivery_client_instance_id.as_deref(),
                &client_instance_id,
            )
        {
            return Err(sync_error(
                "SYNC_ACCOUNT_BINDING_MISMATCH",
                "sync.bootstrap response does not match the active account device",
            ));
        }
        let groups = bootstrap
            .group_state_baseline
            .iter()
            .enumerate()
            .map(|(index, group)| {
                baseline_group_record(self.client, group, &bootstrap.server_time, index)
            })
            .collect::<crate::ImResult<Vec<_>>>()?;
        let read_states = bootstrap
            .read_state_baseline
            .iter()
            .map(read_state_from_snapshot)
            .collect::<crate::ImResult<Vec<_>>>()?;
        let now = unix_time_i64();
        let lane_states =
            lane_states_from_bootstrap(&binding.owner_identity_id, &bootstrap.lane_bootstrap);
        let state = crate::internal::local_state::sync_v2::MessageSyncState {
            owner_identity_id: binding.owner_identity_id.clone(),
            account_id: binding.account_id.clone(),
            protocol_device_id: binding.protocol_device_id.clone(),
            device_auth_generation: binding.device_auth_generation.clone(),
            stream_epoch: bootstrap.cursor.stream_epoch,
            scan_seq: bootstrap.cursor.scan_seq,
            bootstrap_state: "tail_bootstrapped".to_owned(),
            last_server_time: Some(bootstrap.server_time),
            last_success_at: Some(now),
            last_error_code: None,
            metadata_json: Some(json!({"mode": "tail_only"}).to_string()),
            updated_at: now,
        };
        db.apply_bootstrap_v2(
            crate::internal::local_state::sync_v2::BootstrapApplyInputV2 {
                binding: crate::internal::local_state::sync_v2::IdentityAccountBinding {
                    owner_identity_id: binding.owner_identity_id.clone(),
                    account_id: binding.account_id.clone(),
                    handle_scope: self
                        .client
                        .handle()
                        .map(|handle| handle.as_str().to_owned()),
                    current_did: binding.current_did.clone(),
                    protocol_device_id: binding.protocol_device_id.clone(),
                    identity_generation: binding.identity_generation.clone(),
                    device_auth_generation: binding.device_auth_generation.clone(),
                    created_at: now,
                    updated_at: now,
                },
                state: state.clone(),
                client_instance_id: client_instance_id.clone(),
                negotiated_capabilities_json: negotiated_lane_capabilities_json(
                    &bootstrap.lane_bootstrap,
                )?,
                groups,
                read_states,
                lane_states,
            },
        )
        .await?;
        Ok(state)
    }
}

#[cfg(feature = "group-e2ee")]
fn validate_strict_p6_lane_wire(
    event: &crate::internal::wire::sync_v2::SyncLaneEventV3,
) -> crate::ImResult<()> {
    match event {
        crate::internal::wire::sync_v2::SyncLaneEventV3::P6Delivery { envelope, .. } => {
            let wire = json!({
                "method": anp::group_e2ee::METHOD_GROUP_INCOMING_V2,
                "params": {
                    "meta": envelope.get("meta").cloned().ok_or_else(|| {
                        sync_error("SYNC_P6_NONCONFORMANT", "P6 delivery meta is missing")
                    })?,
                    "auth": envelope.get("auth").cloned().ok_or_else(|| {
                        sync_error("SYNC_P6_NONCONFORMANT", "P6 delivery auth is missing")
                    })?,
                    "body": envelope.get("body").cloned().ok_or_else(|| {
                        sync_error("SYNC_P6_NONCONFORMANT", "P6 delivery body is missing")
                    })?,
                }
            });
            anp::group_e2ee::parse_group_incoming_notification_v2(&wire)
                .map(|_| ())
                .map_err(|_| {
                    sync_error(
                        "SYNC_P6_NONCONFORMANT",
                        "P6 delivery does not satisfy the strict live-wire contract",
                    )
                })
        }
        crate::internal::wire::sync_v2::SyncLaneEventV3::P6ControlNotice { notice, .. } => {
            crate::internal::group_e2ee::v2_notice::parse_notice(notice)
                .map(|_| ())
                .map_err(|_| {
                    sync_error(
                        "SYNC_P6_NONCONFORMANT",
                        "P6 notice does not satisfy the strict live-wire contract",
                    )
                })
        }
        crate::internal::wire::sync_v2::SyncLaneEventV3::P5Delivery { .. } => Err(sync_error(
            "SYNC_P6_NONCONFORMANT",
            "P6 lane contains a P5 delivery",
        )),
    }
}

#[cfg(not(feature = "group-e2ee"))]
fn validate_strict_p6_lane_wire(
    _event: &crate::internal::wire::sync_v2::SyncLaneEventV3,
) -> crate::ImResult<()> {
    Err(sync_error(
        "SYNC_P6_NONCONFORMANT",
        "group-e2ee support is not compiled",
    ))
}

pub(crate) async fn refresh_lane_bootstrap_with_transport_async<T>(
    client: &crate::core::ImClient,
    transport: &mut T,
    db: &crate::internal::local_state::actor::LocalStateDb,
    binding: &crate::identity::ActiveSyncAccountBinding,
) -> crate::ImResult<
    BTreeMap<
        crate::internal::wire::sync_v2::SyncLaneV3,
        crate::internal::local_state::sync_v2::LaneSyncState,
    >,
>
where
    T: AsyncAuthenticatedRpcTransport,
{
    require_explicit_sync_negotiation(transport, client).await?;
    let client_instance_id = db
        .load_or_create_sync_client_instance_id(&binding.owner_identity_id)
        .await?;
    let requested_lanes = desired_v1b_lanes(db, &binding.owner_identity_id).await?;
    let params = crate::internal::wire::sync_v2::build_bootstrap_params_with_lanes(
        &wire_identity(client),
        &client_instance_id,
        &requested_lanes,
    )?;
    let raw = transport
        .authenticated_rpc(MESSAGE_RPC_ENDPOINT, "sync.bootstrap", params)
        .await?;
    let response = crate::internal::wire::sync_v2::parse_bootstrap_response(&raw)?;
    let (account_id, device_id, lane_bootstrap, activated_client_instance_id) = match &response {
        crate::internal::wire::sync_v2::SyncBootstrapResponseV2::TailOnly(bootstrap) => (
            bootstrap.account_id.as_str(),
            bootstrap.device_id.as_str(),
            &bootstrap.lane_bootstrap,
            bootstrap.p6_delivery_client_instance_id.as_deref(),
        ),
        crate::internal::wire::sync_v2::SyncBootstrapResponseV2::RecoveryRequired {
            account_id,
            device_id,
            lane_bootstrap,
            p6_delivery_client_instance_id,
            ..
        } => (
            account_id.as_str(),
            device_id.as_str(),
            lane_bootstrap,
            p6_delivery_client_instance_id.as_deref(),
        ),
    };
    if account_id != binding.account_id
        || device_id != binding.protocol_device_id
        || lane_bootstrap.capabilities != requested_lanes
        || !bootstrap_p6_activation_matches(
            lane_bootstrap,
            activated_client_instance_id,
            &client_instance_id,
        )
    {
        return Err(sync_error(
            "SYNC_ACCOUNT_BINDING_MISMATCH",
            "lane bootstrap does not match the active account device",
        ));
    }
    let states = lane_states_from_bootstrap(&binding.owner_identity_id, lane_bootstrap);
    reconcile_explicit_lane_negotiation(
        db,
        binding,
        &client_instance_id,
        lane_bootstrap,
        states.clone(),
    )
    .await?;
    Ok(lane_state_map(states))
}

async fn reconcile_explicit_lane_negotiation(
    db: &crate::internal::local_state::actor::LocalStateDb,
    binding: &crate::identity::ActiveSyncAccountBinding,
    client_instance_id: &str,
    lanes: &crate::internal::wire::sync_v2::SyncLaneBootstrapV3,
    states: Vec<crate::internal::local_state::sync_v2::LaneSyncState>,
) -> crate::ImResult<()> {
    let capabilities = negotiated_lane_capabilities_json(lanes)?;
    db.reconcile_sync_lane_capability_v1a(
        &binding.owner_identity_id,
        states,
        &binding.device_auth_generation,
        client_instance_id,
        capabilities,
    )
    .await
}

async fn desired_v1b_lanes(
    db: &crate::internal::local_state::actor::LocalStateDb,
    owner_identity_id: &str,
) -> crate::ImResult<BTreeSet<crate::internal::wire::sync_v2::SyncLaneV3>> {
    use crate::internal::wire::sync_v2::SyncLaneV3;
    // Compile-time features declare what this Core implementation can consume;
    // they do not activate a lane or bypass explicit Service negotiation. The
    // resulting desired set is sent in extended bootstrap and becomes active
    // only if the Service returns the same authoritative negotiated set.
    // Local readiness/migration failures remove a supported lane before that
    // request. Requiring a lane to be "already negotiated" here would be
    // circular and would make first bootstrap impossible.
    let mut lanes = BTreeSet::new();
    #[cfg(feature = "secure-direct")]
    lanes.insert(SyncLaneV3::P5Device);
    #[cfg(feature = "group-e2ee")]
    lanes.insert(SyncLaneV3::P6Group);
    for state in db
        .load_lane_transport_states(owner_identity_id.to_owned())
        .await?
    {
        let not_ready = state.last_transport_error.as_deref() == Some("lane_consumer_not_ready")
            || (state.lane == SyncLaneV3::P6Group
                && state.last_transport_error.as_deref() == Some("lane_migration_repair_required"));
        if not_ready {
            lanes.remove(&state.lane);
        }
    }
    Ok(lanes)
}

fn negotiated_lane_capabilities_json(
    lanes: &crate::internal::wire::sync_v2::SyncLaneBootstrapV3,
) -> crate::ImResult<String> {
    let capabilities = [
        crate::internal::wire::sync_v2::SyncLaneV3::P5Device,
        crate::internal::wire::sync_v2::SyncLaneV3::P6Group,
    ]
    .into_iter()
    .filter(|lane| lanes.capabilities.contains(lane))
    .map(|lane| lane.capability())
    .collect::<Vec<_>>();
    let capabilities = serde_json::to_string(&capabilities).map_err(|error| {
        sync_error(
            "SYNC_NEGOTIATION_STATE_INVALID",
            format!("failed to encode negotiated lane state: {error}"),
        )
    })?;
    Ok(capabilities)
}

async fn require_current_sync_run_generation(
    db: &crate::internal::local_state::actor::LocalStateDb,
    owner_identity_id: &str,
    expected_generation: i64,
) -> crate::ImResult<()> {
    let current = db
        .load_message_sync_run_state(owner_identity_id)
        .await?
        .ok_or_else(|| sync_error("SYNC_RUN_SUPERSEDED", "sync run state is missing"))?;
    if current.run_generation != expected_generation {
        return Err(sync_error(
            "SYNC_RUN_SUPERSEDED",
            "a newer sync run superseded the local apply",
        ));
    }
    Ok(())
}

async fn require_explicit_sync_negotiation<T>(
    transport: &mut T,
    client: &crate::core::ImClient,
) -> crate::ImResult<()>
where
    T: AsyncAuthenticatedRpcTransport,
{
    let params =
        crate::internal::wire::sync_v2::build_capability_discovery_params(&wire_identity(client))?;
    let raw = transport
        .authenticated_rpc(MESSAGE_RPC_ENDPOINT, "anp.get_capabilities", params)
        .await?;
    crate::internal::wire::sync_v2::require_explicit_sync_negotiation_capability(&raw)
}

fn bootstrap_p6_activation_matches(
    lanes: &crate::internal::wire::sync_v2::SyncLaneBootstrapV3,
    activated_client_instance_id: Option<&str>,
    expected_client_instance_id: &str,
) -> bool {
    if lanes
        .capabilities
        .contains(&crate::internal::wire::sync_v2::SyncLaneV3::P6Group)
    {
        activated_client_instance_id == Some(expected_client_instance_id)
    } else {
        activated_client_instance_id.is_none()
    }
}

fn lane_states_from_bootstrap(
    owner_identity_id: &str,
    bootstrap: &crate::internal::wire::sync_v2::SyncLaneBootstrapV3,
) -> Vec<crate::internal::local_state::sync_v2::LaneSyncState> {
    bootstrap
        .lanes
        .iter()
        .filter(|(lane, _)| bootstrap.capabilities.contains(lane))
        .map(
            |(lane, state)| crate::internal::local_state::sync_v2::LaneSyncState {
                owner_identity_id: owner_identity_id.to_owned(),
                lane: *lane,
                stream_epoch: state.cursor.stream_epoch.clone(),
                scan_seq: state.cursor.scan_seq.clone(),
                committed_seq: state.committed_seq.clone(),
            },
        )
        .collect()
}

fn lane_state_map(
    states: Vec<crate::internal::local_state::sync_v2::LaneSyncState>,
) -> BTreeMap<
    crate::internal::wire::sync_v2::SyncLaneV3,
    crate::internal::local_state::sync_v2::LaneSyncState,
> {
    states
        .into_iter()
        .map(|state| (state.lane, state))
        .collect()
}

fn lane_handoff_input_from_wire(
    binding: &crate::identity::ActiveSyncAccountBinding,
    client_instance_id: &str,
    lane_epoch: &str,
    event: &crate::internal::wire::sync_v2::SyncLaneEventV3,
    received_at: &str,
) -> crate::ImResult<crate::internal::local_state::sync_v2::SyncLaneHandoffInput> {
    use crate::internal::wire::sync_v2::{SyncLaneEventV3, SyncLaneV3};
    let (lane, position, event_id, event_type, raw_payload, group_did) = match event {
        SyncLaneEventV3::P5Delivery {
            delivery_id,
            seq,
            envelope,
        } => (
            SyncLaneV3::P5Device,
            seq,
            delivery_id,
            "p5.delivery.created",
            envelope,
            None,
        ),
        SyncLaneEventV3::P6Delivery {
            delivery_id,
            seq,
            group_did,
            envelope,
            ..
        } => (
            SyncLaneV3::P6Group,
            seq,
            delivery_id,
            "p6.delivery.created",
            envelope,
            Some(group_did.clone()),
        ),
        SyncLaneEventV3::P6ControlNotice {
            notice_id,
            seq,
            group_did,
            notice,
        } => (
            SyncLaneV3::P6Group,
            seq,
            notice_id,
            "p6.control.notice",
            notice,
            Some(group_did.clone()),
        ),
    };
    let optional_wire_time = |pointers: &[&str]| {
        pointers.iter().find_map(|pointer| {
            raw_payload
                .pointer(pointer)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
        })
    };
    Ok(
        crate::internal::local_state::sync_v2::SyncLaneHandoffInput {
            owner_identity_id: binding.owner_identity_id.clone(),
            account_id_snapshot: binding.account_id.clone(),
            device_id_snapshot: binding.protocol_device_id.clone(),
            auth_generation_snapshot: binding.device_auth_generation.clone(),
            client_instance_id_snapshot: client_instance_id.to_owned(),
            lane,
            lane_epoch: lane_epoch.to_owned(),
            position: position.clone(),
            event_id: event_id.clone(),
            event_type: event_type.to_owned(),
            raw_payload: raw_payload.clone(),
            group_did,
            received_at: received_at.to_owned(),
            source_created_at: optional_wire_time(&["/meta/created_at"]),
            source_expires_at: optional_wire_time(&["/meta/expires_at", "/body/expires_at"]),
        },
    )
}

fn lane_state_at_event(
    current: &crate::internal::local_state::sync_v2::LaneSyncState,
    event_seq: &str,
) -> crate::internal::local_state::sync_v2::LaneSyncState {
    crate::internal::local_state::sync_v2::LaneSyncState {
        owner_identity_id: current.owner_identity_id.clone(),
        lane: current.lane,
        stream_epoch: current.stream_epoch.clone(),
        scan_seq: event_seq.to_owned(),
        committed_seq: event_seq.to_owned(),
    }
}

fn validate_lane_page_progress(
    current: &crate::internal::local_state::sync_v2::LaneSyncState,
    events: &[crate::internal::wire::sync_v2::SyncLaneEventV3],
    next_cursor: &crate::internal::wire::sync_v2::SyncCursorV2,
) -> crate::ImResult<()> {
    let expected_next = events
        .last()
        .map(crate::internal::wire::sync_v2::SyncLaneEventV3::seq)
        .unwrap_or(current.scan_seq.as_str());
    if expected_next != next_cursor.scan_seq {
        return Err(sync_error(
            "SYNC_INVALID_PAGE",
            "lane next cursor must equal the last returned event sequence",
        ));
    }
    if events.first().is_some_and(|event| {
        crate::internal::local_state::sync_v2::compare_decimal(event.seq(), &current.scan_seq)
            .map(|order| order != std::cmp::Ordering::Greater)
            .unwrap_or(true)
    }) {
        return Err(sync_error(
            "SYNC_INVALID_PAGE",
            "lane events must be strictly ahead of the requested cursor",
        ));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn lane_event_receipt(
    binding: &crate::identity::ActiveSyncAccountBinding,
    lane: crate::internal::wire::sync_v2::SyncLaneV3,
    event_id: &str,
    stream_epoch: &str,
    event_seq: &str,
    group_did: Option<String>,
    group_event_seq: Option<String>,
) -> crate::internal::local_state::sync_v2::SyncLaneEventReceipt {
    crate::internal::local_state::sync_v2::SyncLaneEventReceipt {
        owner_identity_id: binding.owner_identity_id.clone(),
        lane,
        event_id: event_id.to_owned(),
        stream_epoch: stream_epoch.to_owned(),
        event_seq: event_seq.to_owned(),
        group_did,
        group_event_seq,
        applied_at: unix_time_i64(),
    }
}

fn validate_p6_delta_binding(
    group_did: &str,
    group_event_seq: &str,
    envelope: &Value,
) -> crate::ImResult<()> {
    let body_group_did = envelope.pointer("/body/group_did").and_then(Value::as_str);
    let body_group_event_seq = envelope.pointer("/body/group_event_seq").and_then(|value| {
        value
            .as_str()
            .map(ToOwned::to_owned)
            .or_else(|| value.as_u64().map(|value| value.to_string()))
    });
    if body_group_did != Some(group_did) || body_group_event_seq.as_deref() != Some(group_event_seq)
    {
        return Err(sync_error(
            "SYNC_INVALID_PAGE",
            "P6 delivery position conflicts with its envelope",
        ));
    }
    Ok(())
}

fn message_conversation_id(
    message: &crate::messages::Message,
    binding: &crate::identity::ActiveSyncAccountBinding,
) -> String {
    crate::messages::ConversationIdentity::from_thread_ref_for_owner(
        &message.thread,
        &binding.current_did,
    )
    .conversation_id
}

fn direct_peer_dids_from_events(
    events: &[crate::internal::local_state::sync_v2::DeltaApplyEventV2],
) -> Vec<String> {
    events
        .iter()
        .flat_map(|event| event.messages.iter())
        .filter_map(crate::internal::local_state::inbound_resolution_backlog::direct_peer_did)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn append_backlog_warning(warnings: &mut Vec<String>, backlogged_messages: usize) {
    if backlogged_messages > 0 {
        warnings.push(format!("identity_unresolved_backlog:{backlogged_messages}"));
    }
}

fn push_identity_resolution_deferred(warnings: &mut Vec<String>) {
    if !warnings
        .iter()
        .any(|warning| warning == "identity_resolution_deferred")
    {
        warnings.push("identity_resolution_deferred".to_owned());
    }
}

fn canonical_read_remote_thread_key(thread_kind: &str, remote_thread_key: &str) -> String {
    if thread_kind == "group" {
        remote_thread_key
            .strip_prefix("group:")
            .unwrap_or(remote_thread_key)
            .to_owned()
    } else {
        remote_thread_key.to_owned()
    }
}

fn validate_system_notification_event_contract(
    client: &crate::core::ImClient,
    binding: &crate::identity::ActiveSyncAccountBinding,
    event: &crate::internal::wire::sync_v2::SyncEventV2,
) -> crate::ImResult<()> {
    if event.schema_version != 1 {
        return Err(sync_error(
            "SYNC_SCHEMA_UNSUPPORTED",
            format!(
                "required system.notification uses unsupported schema version {}",
                event.schema_version
            ),
        ));
    }
    if event.ignore_safe {
        return Err(sync_error(
            "SYNC_INVALID_PAGE",
            "required system.notification must not be marked ignore_safe",
        ));
    }
    if event.aggregate_kind != "system_notification"
        || event.state_version.is_some()
        || event.thread_key.is_some()
        || event.origin_device_id.is_some()
    {
        return Err(sync_error(
            "SYNC_INVALID_PAGE",
            "system.notification envelope fields violate the frozen schema",
        ));
    }
    if event.recipient_device_id.as_deref() != Some(binding.protocol_device_id.as_str()) {
        return Err(sync_error(
            "SYNC_DEVICE_BINDING_MISMATCH",
            "system.notification must target the active exact device",
        ));
    }
    let origin_did = event.origin_did.as_deref().ok_or_else(|| {
        sync_error(
            "SYNC_INVALID_PAGE",
            "system.notification is missing its service origin DID",
        )
    })?;
    let service_did = client
        .core_inner()
        .sdk_config()
        .anp_service_did
        .as_ref()
        .map(crate::ids::Did::as_str)
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("did:wba:{}", client.did_domain()));
    let expected_origin_prefix = format!("{service_did}:agents:system-notification:e1_");
    let origin_fingerprint = origin_did
        .strip_prefix(&expected_origin_prefix)
        .ok_or_else(|| {
            sync_error(
                "SYNC_INVALID_PAGE",
                "system.notification origin is not the local service notification Agent",
            )
        })?;
    if origin_fingerprint.len() != 43
        || !origin_fingerprint
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    {
        return Err(sync_error(
            "SYNC_INVALID_PAGE",
            "system.notification origin has an invalid e1 fingerprint",
        ));
    }

    let payload = event.payload.as_object().ok_or_else(|| {
        sync_error(
            "SYNC_INVALID_PAGE",
            "system.notification payload must be an object",
        )
    })?;
    let expected_payload_fields = ["projection_kind", "event_id", "message_id"];
    if payload.len() != expected_payload_fields.len()
        || expected_payload_fields
            .iter()
            .any(|field| !payload.contains_key(*field))
    {
        return Err(sync_error(
            "SYNC_INVALID_PAGE",
            "system.notification payload must contain exactly the frozen fields",
        ));
    }
    let payload_string = |field: &str| {
        payload
            .get(field)
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty() && value.trim() == *value)
            .ok_or_else(|| {
                sync_error(
                    "SYNC_INVALID_PAGE",
                    format!("system.notification payload {field} must be canonical"),
                )
            })
    };
    if payload_string("projection_kind")? != "system_notification" {
        return Err(sync_error(
            "SYNC_INVALID_PAGE",
            "system.notification payload projection_kind is invalid",
        ));
    }
    let notification_event_id = payload_string("event_id")?;
    let message_id = payload_string("message_id")?;
    if notification_event_id != message_id || event.aggregate_id != notification_event_id {
        return Err(sync_error(
            "SYNC_INVALID_PAGE",
            "system.notification business identifiers do not match",
        ));
    }
    if event.event_id
        != format!(
            "system.notification:{notification_event_id}:{}",
            binding.protocol_device_id
        )
    {
        return Err(sync_error(
            "SYNC_INVALID_PAGE",
            "system.notification sync event_id is not exact-device qualified",
        ));
    }

    let source = event
        .source
        .as_ref()
        .and_then(Value::as_object)
        .ok_or_else(|| {
            sync_error(
                "SYNC_INVALID_PAGE",
                "system.notification source must be an object",
            )
        })?;
    let expected_source_fields = ["method", "operation_id", "client_message_id"];
    if source.len() != expected_source_fields.len()
        || expected_source_fields
            .iter()
            .any(|field| !source.contains_key(*field))
        || source.get("method").and_then(Value::as_str) != Some("direct.send")
        || source.get("operation_id").and_then(Value::as_str) != Some(message_id)
        || source.get("client_message_id").and_then(Value::as_str) != Some(message_id)
    {
        return Err(sync_error(
            "SYNC_INVALID_PAGE",
            "system.notification source violates the frozen service binding",
        ));
    }
    Ok(())
}

async fn prepare_system_notification<R>(
    client: &crate::core::ImClient,
    binding: &crate::identity::ActiveSyncAccountBinding,
    event: &crate::internal::wire::sync_v2::SyncEventV2,
    hydrated_projection: &Value,
    directory_transport: &mut R,
) -> crate::ImResult<crate::internal::system_notification::store::SystemNotificationApplyInput>
where
    R: AsyncRpcTransport,
{
    prepare_system_notification_at(
        client,
        binding,
        event,
        hydrated_projection,
        directory_transport,
        chrono::Utc::now(),
    )
    .await
}

async fn prepare_system_notification_at<R>(
    client: &crate::core::ImClient,
    binding: &crate::identity::ActiveSyncAccountBinding,
    event: &crate::internal::wire::sync_v2::SyncEventV2,
    hydrated_projection: &Value,
    directory_transport: &mut R,
    received_at: chrono::DateTime<chrono::Utc>,
) -> crate::ImResult<crate::internal::system_notification::store::SystemNotificationApplyInput>
where
    R: AsyncRpcTransport,
{
    let wrapper = hydrated_projection.as_object().ok_or_else(|| {
        sync_error(
            "SYNC_HYDRATION_INCOMPLETE",
            "system.notification hydration must be an object",
        )
    })?;
    let expected_wrapper_fields = ["projection_kind", "meta", "auth", "body"];
    if wrapper.len() != expected_wrapper_fields.len()
        || expected_wrapper_fields
            .iter()
            .any(|field| !wrapper.contains_key(*field))
        || !crate::internal::system_notification::wire::is_trusted_delivery_marker(
            hydrated_projection,
        )
    {
        return Err(sync_error(
            "SYNC_HYDRATION_INCOMPLETE",
            "system.notification hydration must be the exact trusted projection wrapper",
        ));
    }
    let notification_event_id = event.payload["event_id"]
        .as_str()
        .expect("validated system notification event_id");
    let message_id = event.payload["message_id"]
        .as_str()
        .expect("validated system notification message_id");
    let meta = wrapper
        .get("meta")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            sync_error(
                "SYNC_HYDRATION_INCOMPLETE",
                "system.notification trusted projection meta is missing",
            )
        })?;
    if meta.get("message_id").and_then(Value::as_str) != Some(message_id)
        || meta.get("operation_id").and_then(Value::as_str) != Some(message_id)
        || meta.get("sender_did").and_then(Value::as_str) != event.origin_did.as_deref()
        || meta
            .get("target")
            .and_then(Value::as_object)
            .and_then(|target| target.get("did"))
            .and_then(Value::as_str)
            != Some(binding.current_did.as_str())
    {
        return Err(sync_error(
            "SYNC_INVALID_PAGE",
            "system.notification hydrated projection conflicts with its sync envelope",
        ));
    }
    let normalized =
        crate::internal::system_notification::dispatch::normalize_delivery(hydrated_projection);
    let verified = crate::internal::system_notification::verify::verify_with_transport_async(
        directory_transport,
        client.did().as_str(),
        &normalized,
        received_at,
    )
    .await?;
    if verified.envelope.notification.event_id != notification_event_id
        || verified.envelope.meta.message_id != message_id
        || verified.envelope.meta.operation_id != message_id
        || verified.envelope.meta.sender_did != event.origin_did.as_deref().unwrap_or_default()
    {
        return Err(sync_error(
            "SYNC_INVALID_PAGE",
            "system.notification verified envelope conflicts with its sync event",
        ));
    }
    Ok(
        crate::internal::system_notification::store::SystemNotificationApplyInput {
            owner_identity_id: binding.owner_identity_id.clone(),
            owner_did: binding.current_did.clone(),
            protocol_device_id: binding.protocol_device_id.clone(),
            verified,
            received_at,
        },
    )
}

fn reduce_event(
    client: &crate::core::ImClient,
    event: &crate::internal::wire::sync_v2::SyncEventV2,
    hydrated_message: Option<&Value>,
    verified_system_notification: Option<
        crate::internal::system_notification::store::SystemNotificationApplyInput,
    >,
    public_messages: &mut BTreeMap<String, crate::messages::Message>,
) -> crate::ImResult<crate::internal::local_state::sync_v2::DeltaApplyEventV2> {
    if matches!(
        event.event_type.as_str(),
        "message.created"
            | "message.read_state_updated"
            | "group.member_changed"
            | "group.profile_updated"
            | "system.notification"
    ) && event.ignore_safe
    {
        return Err(sync_error(
            "SYNC_INVALID_PAGE",
            "registered message and Group state events must not be marked ignore_safe",
        ));
    }
    if event.schema_version != 1 {
        if event.ignore_safe {
            return Ok(receipt_only(event));
        }
        return Err(sync_error(
            "SYNC_SCHEMA_UNSUPPORTED",
            format!(
                "required event {:?} uses unsupported schema version {}",
                event.event_type, event.schema_version
            ),
        ));
    }
    let mut apply = receipt_only(event);
    match event.event_type.as_str() {
        "message.created" => {
            let message_kind = event
                .payload
                .get("message_kind")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if !matches!(message_kind, "direct_plain" | "group_plain") {
                return Err(sync_error(
                    "SYNC_UNKNOWN_REQUIRED_EVENT",
                    "message.created is not an ordinary Direct/Group message",
                ));
            }
            let hydrated_message = hydrated_message.ok_or_else(|| {
                sync_error(
                    "SYNC_HYDRATION_INCOMPLETE",
                    "message.created has no exact hydrated ordinary message",
                )
            })?;
            let expected_thread_kind = if message_kind == "group_plain" {
                "group"
            } else {
                "direct"
            };
            if hydrated_message.get("thread_kind").and_then(Value::as_str)
                != Some(expected_thread_kind)
            {
                return Err(sync_error(
                    "SYNC_INVALID_PAGE",
                    "hydrated message thread_kind conflicts with event message_kind",
                ));
            }
            let synthetic = hydrated_v1_event(event, hydrated_message)?;
            let projection =
                super::sync::sync_delta_message_from_payload(client, &synthetic, true)?;
            let mut message = projection.message;
            message.direction = event_direction(event)?;
            add_v2_metadata(&mut message, event);
            let mut record =
                super::local_projection::message_record_from_message(client, &message)?;
            record.hydration_state = projection.hydration_state;
            let remote_thread_key = event.thread_key.as_deref().ok_or_else(|| {
                sync_error(
                    "SYNC_INVALID_PAGE",
                    "message.created is missing its remote thread key",
                )
            })?;
            apply
                .thread_bindings
                .push(crate::internal::local_state::sync_v2::SyncThreadBinding {
                    owner_identity_id: client.current_identity().id.as_str().to_owned(),
                    remote_thread_key: remote_thread_key.to_owned(),
                    thread_kind: expected_thread_kind.to_owned(),
                    conversation_id: record.conversation_id.clone(),
                    updated_at: unix_time_i64(),
                });
            apply.messages.push(record);
            public_messages.insert(event.event_id.clone(), message);
        }
        "message.read_state_updated" => {
            apply.read_states.push(read_state_from_event(event)?);
        }
        "group.member_changed" | "group.profile_updated" => {
            validate_group_state_version(event)?;
            let synthetic = v1_event(event, event.payload.clone());
            let projection = super::sync::sync_delta_group_projection(client, &synthetic)?;
            apply.groups.push(projection.group);
            if let Some(record) = projection.system_message {
                apply.messages.push(record);
            }
        }
        "system.notification" => {
            apply.system_notification = Some(verified_system_notification.ok_or_else(|| {
                sync_error(
                    "SYNC_HYDRATION_INCOMPLETE",
                    "system.notification has no verified hydrated projection",
                )
            })?);
        }
        _ if event.ignore_safe => {}
        _ => {
            return Err(sync_error(
                "SYNC_UNKNOWN_REQUIRED_EVENT",
                format!("unsupported required event type {:?}", event.event_type),
            ));
        }
    }
    Ok(apply)
}

fn receipt_only(
    event: &crate::internal::wire::sync_v2::SyncEventV2,
) -> crate::internal::local_state::sync_v2::DeltaApplyEventV2 {
    crate::internal::local_state::sync_v2::DeltaApplyEventV2 {
        event_id: event.event_id.clone(),
        event_seq: event.event_seq.clone(),
        event_type: event.event_type.clone(),
        ..Default::default()
    }
}

fn hydrated_v1_event(
    event: &crate::internal::wire::sync_v2::SyncEventV2,
    hydrated_message: &Value,
) -> crate::ImResult<crate::internal::wire::sync::SyncDeltaEvent> {
    let mut payload = event
        .payload
        .as_object()
        .cloned()
        .ok_or_else(|| sync_error("SYNC_INVALID_PAGE", "event payload must be an object"))?;
    let mut message = hydrated_message
        .as_object()
        .cloned()
        .ok_or_else(|| sync_error("SYNC_INVALID_PAGE", "hydrated message must be an object"))?;
    normalize_hydrated_message(&mut message, &payload);
    payload.insert("message".to_owned(), Value::Object(message.clone()));
    if !payload.contains_key("thread") {
        payload.insert(
            "thread".to_owned(),
            Value::Object(thread_for_event(event, &message)?),
        );
    }
    Ok(v1_event(event, Value::Object(payload)))
}

fn normalize_hydrated_message(
    message: &mut Map<String, Value>,
    event_payload: &Map<String, Value>,
) {
    if !message.contains_key("accepted_at") {
        if let Some(accepted_at) = event_payload
            .get("accepted_at")
            .cloned()
            .or_else(|| message.get("created_at").cloned())
        {
            message.insert("accepted_at".to_owned(), accepted_at);
        }
    }
    if !message.contains_key("operation_id") {
        if let Some(client_msg_id) = message.get("client_msg_id").cloned() {
            message.insert("operation_id".to_owned(), client_msg_id);
        }
    }
}

fn hydration_event_id_batches(event_ids: &[String]) -> std::slice::Chunks<'_, String> {
    event_ids.chunks(crate::internal::wire::sync_v2::MESSAGE_GET_BATCH_CLIENT_CHUNK_EVENT_IDS)
}

async fn hydrate_required_messages<T: AsyncAuthenticatedRpcTransport>(
    transport: &mut T,
    identity: &crate::internal::wire::common::WireIdentity,
    message_event_ids: &[String],
) -> crate::ImResult<(BTreeMap<String, Value>, u32)> {
    hydrate_required_messages_with_budget(
        transport,
        identity,
        message_event_ids,
        &mut BTreeMap::new(),
    )
    .await
}

async fn hydrate_required_messages_with_budget<T: AsyncAuthenticatedRpcTransport>(
    transport: &mut T,
    identity: &crate::internal::wire::common::WireIdentity,
    message_event_ids: &[String],
    failure_fingerprints: &mut BTreeMap<String, u8>,
) -> crate::ImResult<(BTreeMap<String, Value>, u32)> {
    let mut hydrated = BTreeMap::new();
    let mut count = 0_u32;
    for event_ids in hydration_event_id_batches(message_event_ids) {
        let params =
            crate::internal::wire::sync_v2::build_message_get_batch_params(identity, event_ids)?;
        let raw = loop {
            match transport
                .authenticated_rpc(MESSAGE_RPC_ENDPOINT, "message.get_batch", params.clone())
                .await
            {
                Ok(raw) => break raw,
                Err(error) if is_transient_sync_io_error(&error) => {
                    let fingerprint = sync_failure_fingerprint("message.get_batch", &error);
                    let failures = failure_fingerprints.entry(fingerprint).or_default();
                    *failures = failures.saturating_add(1);
                    if *failures >= 3 {
                        return Err(error);
                    }
                    tokio::time::sleep(sync_retry_delay(&error, *failures)).await;
                }
                Err(error) => return Err(error),
            }
        };
        let batch = crate::internal::wire::sync_v2::parse_message_batch(&raw, event_ids)?;
        if !batch.unavailable.is_empty() {
            return Err(sync_error(
                "SYNC_HYDRATION_INCOMPLETE",
                "one or more required ordinary messages are unavailable",
            ));
        }
        count = count.saturating_add(u32::try_from(batch.items.len()).unwrap_or(u32::MAX));
        for item in batch.items {
            if hydrated.insert(item.event_id, item.message).is_some() {
                return Err(sync_error(
                    "SYNC_INVALID_PAGE",
                    "message.get_batch returned a duplicate event across chunks",
                ));
            }
        }
    }
    Ok((hydrated, count))
}

fn is_transient_sync_io_error(error: &crate::ImError) -> bool {
    match error {
        crate::ImError::TransportUnavailable { .. } => true,
        crate::ImError::Service {
            status_code, code, ..
        } => {
            matches!(status_code, Some(408 | 425 | 429 | 500..=599))
                || code.as_deref() == Some("anp.temporarily_unavailable")
        }
        _ => false,
    }
}

fn sync_failure_fingerprint(operation: &str, error: &crate::ImError) -> String {
    match error {
        crate::ImError::Service {
            status_code, code, ..
        } => format!(
            "{operation}:service:{}:{}",
            status_code.map_or_else(|| "none".to_owned(), |value| value.to_string()),
            code.as_deref().unwrap_or("none")
        ),
        crate::ImError::TransportUnavailable { .. } => format!("{operation}:transport"),
        _ => format!("{operation}:non_transient"),
    }
}

fn sync_retry_delay(error: &crate::ImError, failure_count: u8) -> StdDuration {
    let retry_after_seconds = match error {
        crate::ImError::Service {
            data: Some(data), ..
        } => data
            .get("retry_after_seconds")
            .or_else(|| data.get("retry_after"))
            .and_then(Value::as_u64),
        _ => None,
    };
    if let Some(seconds) = retry_after_seconds {
        return StdDuration::from_secs(seconds.min(30));
    }
    let exponent = u32::from(failure_count.saturating_sub(1)).min(8);
    StdDuration::from_millis(100_u64.saturating_mul(1_u64 << exponent))
}

fn thread_for_event(
    event: &crate::internal::wire::sync_v2::SyncEventV2,
    message: &Map<String, Value>,
) -> crate::ImResult<Map<String, Value>> {
    let message_kind = event
        .payload
        .get("message_kind")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let mut thread = Map::new();
    if message_kind == "group_plain" {
        let group_did = message
            .get("group_did")
            .or_else(|| event.payload.get("group_did"))
            .and_then(Value::as_str)
            .ok_or_else(|| sync_error("SYNC_INVALID_PAGE", "group message missing group_did"))?;
        thread.insert("kind".to_owned(), Value::String("group".to_owned()));
        thread.insert("group_did".to_owned(), Value::String(group_did.to_owned()));
    } else {
        let direction = event
            .payload
            .get("direction")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let key = if direction == "incoming" {
            "sender_did_snapshot"
        } else {
            "recipient_did_snapshot"
        };
        let peer_did = event
            .payload
            .get(key)
            .and_then(Value::as_str)
            .or_else(|| {
                if direction == "incoming" {
                    message.get("sender_did").and_then(Value::as_str)
                } else {
                    message.get("receiver_did").and_then(Value::as_str)
                }
            })
            .ok_or_else(|| sync_error("SYNC_INVALID_PAGE", "Direct event missing peer DID"))?;
        thread.insert("kind".to_owned(), Value::String("direct".to_owned()));
        thread.insert("peer_did".to_owned(), Value::String(peer_did.to_owned()));
    }
    Ok(thread)
}

fn event_direction(
    event: &crate::internal::wire::sync_v2::SyncEventV2,
) -> crate::ImResult<crate::messages::MessageDirection> {
    match event
        .payload
        .get("direction")
        .and_then(Value::as_str)
        .unwrap_or_default()
    {
        "incoming" => Ok(crate::messages::MessageDirection::Incoming),
        "outgoing" | "outgoing/self" => Ok(crate::messages::MessageDirection::Outgoing),
        _ => Err(sync_error(
            "SYNC_INVALID_PAGE",
            "message.created direction must be incoming, outgoing, or outgoing/self",
        )),
    }
}

fn add_v2_metadata(
    message: &mut crate::messages::Message,
    event: &crate::internal::wire::sync_v2::SyncEventV2,
) {
    for (key, value) in [
        ("stream_epoch", Some(event.stream_epoch.as_str())),
        ("account_id", Some(event.account_id.as_str())),
        ("origin_device_id", event.origin_device_id.as_deref()),
        (
            "client_message_id",
            event
                .payload
                .get("client_message_id")
                .and_then(Value::as_str),
        ),
        ("remote_thread_key", event.thread_key.as_deref()),
    ] {
        if let Some(value) = value {
            message
                .metadata
                .attributes
                .push(crate::messages::MessageMetadataAttribute {
                    key: key.to_owned(),
                    value: value.to_owned(),
                });
        }
    }
}

fn read_state_from_event(
    event: &crate::internal::wire::sync_v2::SyncEventV2,
) -> crate::ImResult<crate::internal::local_state::sync_v2::ReadStateApplyV2> {
    let remote_thread_key = event.thread_key.as_deref().ok_or_else(|| {
        sync_error(
            "SYNC_INVALID_PAGE",
            "read state event is missing envelope thread_key",
        )
    })?;
    let payload_thread_key = event
        .payload
        .get("thread_key")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            sync_error(
                "SYNC_INVALID_PAGE",
                "read state event is missing payload thread_key",
            )
        })?;
    if payload_thread_key != remote_thread_key {
        return Err(sync_error(
            "SYNC_INVALID_PAGE",
            "read state payload thread key conflicts with its envelope",
        ));
    }
    let thread_kind = event
        .payload
        .get("thread_kind")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            sync_error(
                "SYNC_INVALID_PAGE",
                "read state event is missing payload thread_kind",
            )
        })?;
    if !matches!(thread_kind, "direct" | "group") {
        return Err(sync_error(
            "SYNC_INVALID_PAGE",
            "read state thread_kind must be direct or group",
        ));
    }
    let state_version = event.state_version.as_deref().ok_or_else(|| {
        sync_error(
            "SYNC_INVALID_PAGE",
            "read state event is missing envelope state_version",
        )
    })?;
    let payload_version = event
        .payload
        .get("state_version")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            sync_error(
                "SYNC_INVALID_PAGE",
                "read state event is missing payload state_version",
            )
        })?;
    if payload_version != state_version {
        return Err(sync_error(
            "SYNC_INVALID_PAGE",
            "read state version conflicts with its event envelope",
        ));
    }
    let read_watermark_seq = event
        .payload
        .get("read_up_to_thread_seq")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            sync_error(
                "SYNC_INVALID_PAGE",
                "read state event is missing its ordinary thread watermark",
            )
        })?
        .to_owned();
    crate::internal::local_state::sync_v2::validate_decimal(
        "read_up_to_thread_seq",
        &read_watermark_seq,
    )?;
    Ok(crate::internal::local_state::sync_v2::ReadStateApplyV2 {
        remote_thread_key: remote_thread_key.to_owned(),
        thread_kind: thread_kind.to_owned(),
        read_watermark_seq,
        read_watermark_message_id: event
            .payload
            .get("read_up_to_message_id")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        state_version: state_version.to_owned(),
        occurred_at: event.occurred_at.clone(),
    })
}

fn read_state_from_snapshot(
    value: &Value,
) -> crate::ImResult<crate::internal::local_state::sync_v2::ReadStateApplyV2> {
    let object = value.as_object().ok_or_else(|| {
        sync_error(
            "SYNC_INVALID_SNAPSHOT",
            "snapshot read state must be an object",
        )
    })?;
    let required = |field: &str| {
        object
            .get(field)
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty() && value.trim() == *value)
            .map(ToOwned::to_owned)
            .ok_or_else(|| {
                sync_error(
                    "SYNC_INVALID_SNAPSHOT",
                    format!("snapshot read state is missing {field}"),
                )
            })
    };
    let nullable_string = |field: &str| match object.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) if !value.trim().is_empty() && value.trim() == value => {
            Ok(Some(value.clone()))
        }
        Some(_) => Err(sync_error(
            "SYNC_INVALID_SNAPSHOT",
            format!("snapshot read state {field} must be a canonical string or null"),
        )),
    };
    let thread_kind = required("thread_kind")?;
    if !matches!(thread_kind.as_str(), "direct" | "group") {
        return Err(sync_error(
            "SYNC_INVALID_SNAPSHOT",
            "snapshot read state thread_kind must be direct or group",
        ));
    }
    let read_watermark_seq = required("read_up_to_thread_seq")?;
    let state_version = required("state_version")?;
    crate::internal::local_state::sync_v2::validate_decimal(
        "read_up_to_thread_seq",
        &read_watermark_seq,
    )?;
    crate::internal::local_state::sync_v2::validate_positive_decimal(
        "state_version",
        &state_version,
    )?;
    let read_watermark_message_id = nullable_string("read_up_to_message_id")?;
    nullable_string("updated_by_device_id")?;
    Ok(crate::internal::local_state::sync_v2::ReadStateApplyV2 {
        remote_thread_key: required("thread_key")?,
        thread_kind,
        read_watermark_seq,
        read_watermark_message_id,
        state_version,
        occurred_at: object
            .get("updated_at")
            .and_then(Value::as_str)
            .filter(|value| chrono::DateTime::parse_from_rfc3339(value).is_ok())
            .ok_or_else(|| {
                sync_error(
                    "SYNC_INVALID_SNAPSHOT",
                    "snapshot read state updated_at must be RFC3339",
                )
            })?
            .to_owned(),
    })
}

fn validate_group_state_version(
    event: &crate::internal::wire::sync_v2::SyncEventV2,
) -> crate::ImResult<()> {
    let envelope = event.state_version.as_deref().ok_or_else(|| {
        sync_error(
            "SYNC_INVALID_PAGE",
            "Group state event is missing envelope state_version",
        )
    })?;
    let payload_version = event
        .payload
        .pointer("/group/group_state_version")
        .or_else(|| event.payload.get("group_state_version"))
        .and_then(Value::as_str)
        .ok_or_else(|| {
            sync_error(
                "SYNC_INVALID_PAGE",
                "Group state event is missing payload group_state_version",
            )
        })?;
    if envelope != payload_version {
        return Err(sync_error(
            "SYNC_INVALID_PAGE",
            "group state version conflicts with its event envelope",
        ));
    }
    Ok(())
}

fn v1_event(
    event: &crate::internal::wire::sync_v2::SyncEventV2,
    payload: Value,
) -> crate::internal::wire::sync::SyncDeltaEvent {
    crate::internal::wire::sync::SyncDeltaEvent {
        event_id: event.event_id.clone(),
        event_seq: event.event_seq.clone(),
        event_type: event.event_type.clone(),
        aggregate_kind: Some(event.aggregate_kind.clone()),
        aggregate_id: Some(event.aggregate_id.clone()),
        owner_subject_id: Some(event.account_id.clone()),
        created_at: Some(event.occurred_at.clone()),
        payload,
    }
}

fn baseline_group_record(
    client: &crate::core::ImClient,
    value: &Value,
    server_time: &str,
    index: usize,
) -> crate::ImResult<crate::internal::local_state::groups::GroupRecord> {
    let object = value
        .as_object()
        .ok_or_else(|| sync_error("SYNC_INVALID_PAGE", "group baseline must be an object"))?;
    let group = object.get("group").unwrap_or(value);
    let group_object = group.as_object().ok_or_else(|| {
        sync_error(
            "SYNC_INVALID_PAGE",
            "group baseline group must be an object",
        )
    })?;
    let group_did = group_object
        .get("group_did")
        .or_else(|| object.get("group_did"))
        .and_then(Value::as_str)
        .ok_or_else(|| sync_error("SYNC_INVALID_PAGE", "group baseline missing group_did"))?;
    let payload = normalize_baseline_group(value, client.did().as_str());
    let synthetic = crate::internal::wire::sync::SyncDeltaEvent {
        event_id: format!("bootstrap-group-{index}"),
        event_seq: "1".to_owned(),
        event_type: "group.profile_updated".to_owned(),
        aggregate_kind: Some("group".to_owned()),
        aggregate_id: Some(group_did.to_owned()),
        owner_subject_id: None,
        created_at: Some(server_time.to_owned()),
        payload,
    };
    super::sync::sync_delta_group_record(client, &synthetic)
}

fn normalize_baseline_group(value: &Value, owner_did: &str) -> Value {
    let Some(object) = value.as_object() else {
        return value.clone();
    };
    if object.contains_key("group") {
        return value.clone();
    }
    let mut group = Map::new();
    for field in [
        "group_did",
        "group_state_version",
        "group_event_seq",
        "required_security_profile",
    ] {
        if let Some(value) = object.get(field) {
            group.insert(field.to_owned(), value.clone());
        }
    }
    if let Some(profile) = object.get("group_profile") {
        group.insert("profile".to_owned(), profile.clone());
    }
    let mut membership = Map::new();
    membership.insert(
        "subject_did".to_owned(),
        Value::String(owner_did.to_owned()),
    );
    if let Some(status) = object.get("membership_status") {
        membership.insert("status".to_owned(), status.clone());
    }
    if let Some(role) = object
        .get("member_role")
        .or_else(|| object.get("role"))
        .or_else(|| object.get("membership_role"))
    {
        membership.insert("role".to_owned(), role.clone());
    }
    json!({
        "group": group,
        "membership": membership
    })
}

fn validate_page_binding(
    page: &crate::internal::wire::sync_v2::SyncDeltaPageV2,
    binding: &crate::identity::ActiveSyncAccountBinding,
    state: &crate::internal::local_state::sync_v2::MessageSyncState,
) -> crate::ImResult<()> {
    if page.next_cursor.stream_epoch != state.stream_epoch {
        return Err(sync_error(
            "SYNC_CURSOR_EPOCH_MISMATCH",
            "sync.delta response changed stream epoch without controlled recovery",
        ));
    }
    if crate::internal::local_state::sync_v2::compare_decimal(
        &page.next_cursor.scan_seq,
        &state.scan_seq,
    )? == std::cmp::Ordering::Less
    {
        return Err(sync_error(
            "SYNC_CURSOR_REGRESSION",
            "sync.delta response cursor moved backwards",
        ));
    }
    for event in &page.events {
        if event.account_id != binding.account_id || event.stream_epoch != state.stream_epoch {
            return Err(sync_error(
                "SYNC_ACCOUNT_BINDING_MISMATCH",
                "sync event does not match the active account stream",
            ));
        }
        if event
            .recipient_device_id
            .as_deref()
            .is_some_and(|device_id| device_id != binding.protocol_device_id)
        {
            return Err(sync_error(
                "SYNC_DEVICE_BINDING_MISMATCH",
                "sync event targets another device",
            ));
        }
        if crate::internal::local_state::sync_v2::compare_decimal(
            &event.event_seq,
            &page.next_cursor.scan_seq,
        )? == std::cmp::Ordering::Greater
        {
            return Err(sync_error(
                "SYNC_INVALID_PAGE",
                "visible event is ahead of next cursor",
            ));
        }
    }
    Ok(())
}

#[cfg(feature = "group-e2ee")]
fn apply_p4_terminal_events(
    client: &crate::core::ImClient,
    events: &[crate::internal::wire::sync_v2::SyncEventV2],
) -> crate::ImResult<()> {
    for event in events {
        if event.event_type != "group.member_changed" {
            continue;
        }
        let membership = event.payload.get("membership").and_then(Value::as_object);
        if membership
            .and_then(|value| value.get("subject_did"))
            .and_then(Value::as_str)
            != Some(client.did().as_str())
        {
            continue;
        }
        let signal = match membership
            .and_then(|value| value.get("status"))
            .and_then(Value::as_str)
        {
            Some("removed") => anp::group_e2ee::operations::v2::V2TerminalSignal::MemberRemoved,
            Some("left") => anp::group_e2ee::operations::v2::V2TerminalSignal::MemberLeft,
            _ => continue,
        };
        let group_did = event
            .payload
            .pointer("/group/group_did")
            .and_then(Value::as_str)
            .or(event.thread_key.as_deref())
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                sync_error(
                    "SYNC_INVALID_PAGE",
                    "terminal Group event is missing group_did",
                )
            })?;
        crate::internal::group_e2ee::v2_runtime::mark_terminal_intent_for_client(
            client, group_did, signal,
        )?;
    }
    Ok(())
}

#[cfg(not(feature = "group-e2ee"))]
fn apply_p4_terminal_events(
    _client: &crate::core::ImClient,
    _events: &[crate::internal::wire::sync_v2::SyncEventV2],
) -> crate::ImResult<()> {
    Ok(())
}

fn wire_identity(client: &crate::core::ImClient) -> crate::internal::wire::common::WireIdentity {
    crate::internal::wire::common::WireIdentity {
        did: client.did().as_str().to_owned(),
    }
}

fn empty_outcome() -> crate::messages::MessageSyncOutcome {
    crate::messages::MessageSyncOutcome {
        status: crate::messages::MessageSyncStatus::Idle,
        events_applied: 0,
        pages_fetched: 0,
        messages_hydrated: 0,
        duplicates_skipped: 0,
        changed_conversation_ids: Vec::new(),
        committed_incoming_messages: Vec::new(),
        error_code: None,
        warnings: Vec::new(),
    }
}

fn is_device_epoch_rejection(error: &crate::ImError) -> bool {
    matches!(
        error_code(error),
        Some("anp.device_not_eligible" | "anp.device_state_changed")
    )
}

pub(crate) fn failure_outcome(
    error: &crate::ImError,
) -> Option<crate::messages::MessageSyncOutcome> {
    let (status, code) = match error {
        crate::ImError::AuthRequired
        | crate::ImError::SessionExpired
        | crate::ImError::PermissionDenied
        | crate::ImError::IdentityBindingConflict { .. } => (
            crate::messages::MessageSyncStatus::AuthRevoked,
            "AUTH_REVOKED".to_owned(),
        ),
        crate::ImError::Service {
            status_code: Some(401 | 403),
            ..
        } => (
            crate::messages::MessageSyncStatus::AuthRevoked,
            "AUTH_REVOKED".to_owned(),
        ),
        crate::ImError::Service { code, .. }
            if code.as_deref() == Some("SYNC_RECOVERY_REQUIRED") =>
        {
            (
                crate::messages::MessageSyncStatus::RecoveryRequired,
                "SYNC_RECOVERY_REQUIRED".to_owned(),
            )
        }
        crate::ImError::Service { data, .. }
            if data
                .as_ref()
                .and_then(|value| value.get("local_action"))
                .and_then(Value::as_str)
                == Some("device_reprovision_required") =>
        {
            (
                crate::messages::MessageSyncStatus::Blocked,
                "device_reprovision_required".to_owned(),
            )
        }
        crate::ImError::Service { data, .. }
            if data
                .as_ref()
                .and_then(|value| value.get("local_action"))
                .and_then(Value::as_str)
                == Some("server_repair_required") =>
        {
            (
                crate::messages::MessageSyncStatus::Blocked,
                "server_repair_required".to_owned(),
            )
        }
        crate::ImError::Service { code, .. }
            if matches!(
                code.as_deref(),
                Some(
                    "sync.client_upgrade_required" | "sync.invalid_request" | "sync.invalid_cursor"
                )
            ) =>
        {
            (
                crate::messages::MessageSyncStatus::Blocked,
                code.clone().unwrap_or_else(|| "SYNC_BLOCKED".to_owned()),
            )
        }
        crate::ImError::Service { code, .. }
            if matches!(
                code.as_deref(),
                Some(
                    "1401"
                        | "anp.device_not_eligible"
                        | "anp.device_state_changed"
                        | "SYNC_ACCOUNT_BINDING_MISMATCH"
                        | "sync.device_binding_mismatch"
                        | "SYNC_AUTH_GENERATION_MISMATCH"
                )
            ) =>
        {
            (
                crate::messages::MessageSyncStatus::AuthRevoked,
                code.clone().unwrap_or_else(|| "AUTH_REVOKED".to_owned()),
            )
        }
        crate::ImError::TransportUnavailable { .. }
        | crate::ImError::LocalStateUnavailable { .. }
        | crate::ImError::Service { .. } => (
            crate::messages::MessageSyncStatus::RetryableFailure,
            error_code(error)
                .unwrap_or("SYNC_RETRYABLE_FAILURE")
                .to_owned(),
        ),
        _ => return None,
    };
    let warnings = match error {
        crate::ImError::TransportUnavailable { .. } => {
            vec!["sync.retry.transport_unavailable".to_owned()]
        }
        crate::ImError::LocalStateUnavailable { detail } => {
            vec![
                "sync.retry.local_state_unavailable".to_owned(),
                local_state_retry_warning(detail).to_owned(),
            ]
        }
        crate::ImError::Service { .. }
            if status == crate::messages::MessageSyncStatus::RetryableFailure =>
        {
            vec!["sync.retry.service_unavailable".to_owned()]
        }
        _ if status == crate::messages::MessageSyncStatus::Blocked => {
            vec!["sync.blocked.action_required".to_owned()]
        }
        _ => Vec::new(),
    };
    Some(crate::messages::MessageSyncOutcome {
        status,
        error_code: Some(code),
        warnings,
        ..empty_outcome()
    })
}

fn local_state_retry_warning(detail: &str) -> &'static str {
    let detail = detail.to_ascii_lowercase();
    if detail.contains("actor is closed") {
        "sync.retry.local_state.actor_closed"
    } else if detail.contains("database is locked") || detail.contains("database is busy") {
        "sync.retry.local_state.database_busy"
    } else if detail.contains("constraint failed") {
        "sync.retry.local_state.constraint_failed"
    } else if detail.contains("no such table") || detail.contains("no such column") {
        "sync.retry.local_state.schema_unavailable"
    } else if detail.contains("unable to open database")
        || detail.contains("disk i/o error")
        || detail.contains("readonly database")
    {
        "sync.retry.local_state.storage_unavailable"
    } else if detail.contains("failed to encode") || detail.contains("failed to decode") {
        "sync.retry.local_state.codec_unavailable"
    } else {
        "sync.retry.local_state.other"
    }
}

fn error_code(error: &crate::ImError) -> Option<&str> {
    match error {
        crate::ImError::Service {
            code: Some(code), ..
        } => Some(code.as_str()),
        _ => None,
    }
}

fn is_read_target_not_found(
    record: &crate::internal::local_state::sync_v2::LocalMutationRecord,
    error: &crate::ImError,
) -> bool {
    error_code(error) == Some("anp.target_not_found")
        && matches!(
            read_mutation_thread_kind(record),
            Some("direct") | Some("group")
        )
}

fn is_sequence_only_group_read(
    record: &crate::internal::local_state::sync_v2::LocalMutationRecord,
) -> bool {
    if read_mutation_thread_kind(record) != Some("group") {
        return false;
    }
    serde_json::from_str::<Value>(&record.payload_json)
        .ok()
        .and_then(|payload| {
            payload
                .get("read_watermark_message_id")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|message_id| !message_id.is_empty())
                .map(|_| false)
        })
        .unwrap_or(true)
}

fn sequence_only_group_read_retry_delay(attempt_count: i64) -> i64 {
    if attempt_count <= 1 {
        5
    } else {
        30
    }
}

fn read_mutation_thread_kind(
    record: &crate::internal::local_state::sync_v2::LocalMutationRecord,
) -> Option<&str> {
    let payload = serde_json::from_str::<Value>(&record.payload_json).ok()?;
    match payload.get("thread_kind").and_then(Value::as_str) {
        Some("direct") => Some("direct"),
        Some("group") => Some("group"),
        _ => None,
    }
}

fn unix_time_i64() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .try_into()
        .unwrap_or(i64::MAX)
}

async fn best_effort_cleanup<T>(
    db: &crate::internal::local_state::actor::LocalStateDb,
    state: &crate::internal::local_state::sync_v2::MessageSyncState,
    successful_sync: T,
) -> T {
    let cleanup_result = db
        .cleanup_terminal_sync_state(
            state.owner_identity_id.clone(),
            state.stream_epoch.clone(),
            state.scan_seq.clone(),
            crate::internal::local_state::sync_v2::SYNC_CLEANUP_BATCH_SIZE,
            unix_time_i64(),
        )
        .await;
    preserve_success_after_cleanup(successful_sync, cleanup_result)
}

fn preserve_success_after_cleanup<T>(
    successful_sync: T,
    _cleanup_result: crate::ImResult<crate::internal::local_state::sync_v2::SyncCleanupOutcome>,
) -> T {
    successful_sync
}

fn lane_consumer_scope_lock(
    owner_identity_id: &str,
    lane: crate::internal::wire::sync_v2::SyncLaneV3,
    scope: &str,
) -> Arc<tokio::sync::Mutex<()>> {
    static LOCKS: OnceLock<StdMutex<BTreeMap<String, Arc<tokio::sync::Mutex<()>>>>> =
        OnceLock::new();
    let lock_key = serde_json::json!([owner_identity_id, lane.as_str(), scope]).to_string();
    let mut locks = LOCKS
        .get_or_init(|| StdMutex::new(BTreeMap::new()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    locks
        .entry(lock_key)
        .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
        .clone()
}

fn hex_sha256(value: &str) -> String {
    let digest = Sha256::digest(value.as_bytes());
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn sync_error(code: &str, message: impl Into<String>) -> crate::ImError {
    crate::ImError::Service {
        status_code: None,
        code: Some(code.to_owned()),
        message: message.into(),
        data: None,
    }
}

fn incomplete_read_ack(message: impl Into<String>) -> crate::ImError {
    sync_error("READ_STATE_INCOMPLETE_ACK", message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failure_outcome_treats_http_auth_rejection_as_terminal() {
        for status_code in [401, 403] {
            let outcome = failure_outcome(&crate::ImError::Service {
                status_code: Some(status_code),
                code: None,
                message: "authorization rejected".to_owned(),
                data: None,
            })
            .expect("HTTP auth rejection must produce a typed sync outcome");

            assert_eq!(
                outcome.status,
                crate::messages::MessageSyncStatus::AuthRevoked
            );
            assert_eq!(outcome.error_code.as_deref(), Some("AUTH_REVOKED"));
        }
    }

    #[test]
    fn failure_outcome_treats_exhausted_rpc_auth_as_terminal() {
        for code in [
            "1401",
            "anp.device_not_eligible",
            "anp.device_state_changed",
        ] {
            let outcome = failure_outcome(&crate::ImError::Service {
                status_code: Some(200),
                code: Some(code.to_owned()),
                message: "authentication rejected".to_owned(),
                data: None,
            })
            .expect("device auth fence must produce a typed sync outcome");

            assert_eq!(
                outcome.status,
                crate::messages::MessageSyncStatus::AuthRevoked
            );
            assert_eq!(outcome.error_code.as_deref(), Some(code));
        }
    }

    #[test]
    fn failure_outcome_keeps_server_failures_retryable() {
        for status_code in [500, 502, 503] {
            let outcome = failure_outcome(&crate::ImError::Service {
                status_code: Some(status_code),
                code: Some("INTERNAL_ERROR".to_owned()),
                message: "server failure".to_owned(),
                data: None,
            })
            .expect("server failure must produce a typed sync outcome");

            assert_eq!(
                outcome.status,
                crate::messages::MessageSyncStatus::RetryableFailure
            );
            assert_eq!(outcome.error_code.as_deref(), Some("INTERNAL_ERROR"));
            assert_eq!(outcome.warnings, ["sync.retry.service_unavailable"]);
        }
    }

    #[test]
    fn v1a_failure_outcome_blocks_client_upgrade_and_reprovision_actions() {
        let upgrade = failure_outcome(&crate::ImError::Service {
            status_code: Some(409),
            code: Some("sync.client_upgrade_required".to_owned()),
            message: "upgrade required".to_owned(),
            data: None,
        })
        .expect("client upgrade must produce a typed outcome");
        assert_eq!(upgrade.status, crate::messages::MessageSyncStatus::Blocked);
        assert_eq!(
            upgrade.error_code.as_deref(),
            Some("sync.client_upgrade_required")
        );
        assert_eq!(upgrade.warnings, ["sync.blocked.action_required"]);

        let reprovision = failure_outcome(&crate::ImError::Service {
            status_code: Some(409),
            code: Some("sync.device_binding_mismatch".to_owned()),
            message: "new installation requires a new device".to_owned(),
            data: Some(json!({"local_action": "device_reprovision_required"})),
        })
        .expect("device reprovision must produce a typed outcome");
        assert_eq!(
            reprovision.status,
            crate::messages::MessageSyncStatus::Blocked
        );
        assert_eq!(
            reprovision.error_code.as_deref(),
            Some("device_reprovision_required")
        );

        let repair = failure_outcome(&crate::ImError::Service {
            status_code: Some(409),
            code: Some("sync.invalid_cursor".to_owned()),
            message: "required control event blocks recovery".to_owned(),
            data: Some(json!({"local_action": "server_repair_required"})),
        })
        .expect("server repair must produce a typed outcome");
        assert_eq!(repair.status, crate::messages::MessageSyncStatus::Blocked);
        assert_eq!(repair.error_code.as_deref(), Some("server_repair_required"));
    }

    #[test]
    fn failure_outcome_classifies_redacted_retryable_sources() {
        let cases = [crate::ImError::TransportUnavailable {
            detail: "sensitive transport detail".to_owned(),
        }];
        for error in cases {
            let outcome = failure_outcome(&error).expect("retryable error must be classified");
            assert_eq!(
                outcome.status,
                crate::messages::MessageSyncStatus::RetryableFailure
            );
            assert_eq!(
                outcome.error_code.as_deref(),
                Some("SYNC_RETRYABLE_FAILURE")
            );
            assert_eq!(outcome.warnings, ["sync.retry.transport_unavailable"]);
            assert!(!outcome.warnings[0].contains("sensitive"));
        }

        for (detail, expected_warning) in [
            (
                "UNIQUE constraint failed: messages.owner_identity_id",
                "sync.retry.local_state.constraint_failed",
            ),
            ("sensitive local detail", "sync.retry.local_state.other"),
        ] {
            let outcome = failure_outcome(&crate::ImError::LocalStateUnavailable {
                detail: detail.to_owned(),
            })
            .expect("retryable local-state error must be classified");
            assert_eq!(
                outcome.warnings,
                ["sync.retry.local_state_unavailable", expected_warning]
            );
            assert!(outcome
                .warnings
                .iter()
                .all(|warning| !warning.contains(detail)));
        }
    }
    use crate::internal::auth::session::SessionProvider;
    use crate::vault::{DeviceVaultRootKey, FileSecretVault, FileSecretVaultStore};
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::rc::Rc;
    use std::sync::Arc;

    #[test]
    fn group_read_outbox_legacy_local_key_is_normalized_for_the_wire() {
        let group_did = "did:wba:awiki.info:groups:legacy-outbox";

        assert_eq!(
            canonical_read_remote_thread_key("group", &format!("group:{group_did}")),
            group_did
        );
        assert_eq!(
            canonical_read_remote_thread_key("group", group_did),
            group_did
        );
        assert_eq!(
            canonical_read_remote_thread_key("direct", "dconv-alice-bob"),
            "dconv-alice-bob"
        );
    }

    #[test]
    fn sequence_only_group_read_retries_use_bounded_backoff() {
        assert_eq!(sequence_only_group_read_retry_delay(1), 5);
        assert_eq!(sequence_only_group_read_retry_delay(2), 30);
    }

    struct Fixture {
        root: std::path::PathBuf,
        did_domain: String,
        anp_service_did: Option<crate::ids::Did>,
    }

    impl Fixture {
        fn new(prefix: &str) -> Self {
            Self::new_with_identity(prefix, "did:example:alice", "awiki.test")
        }

        fn new_with_identity(prefix: &str, did: &str, did_domain: &str) -> Self {
            Self::new_with_identity_and_service(prefix, did, did_domain, None)
        }

        fn new_with_identity_and_service(
            prefix: &str,
            did: &str,
            did_domain: &str,
            anp_service_did: Option<&str>,
        ) -> Self {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let root = std::env::temp_dir().join(format!(
                "im-core-sync-v2-{prefix}-{}-{nanos}",
                std::process::id()
            ));
            let identity_root = root.join("identities");
            let identity_dir = identity_root.join("alice");
            std::fs::create_dir_all(&identity_dir).unwrap();
            std::fs::create_dir_all(root.join("local")).unwrap();
            std::fs::write(identity_root.join("default"), "alice\n").unwrap();
            std::fs::write(
                identity_root.join("registry.json"),
                json!({
                    "default_identity": "alice",
                    "identities": [{
                        "id": "alice-id",
                        "did": did,
                        "local_alias": "alice",
                        "ready_for_auth": true,
                        "ready_for_messaging": true,
                        "missing": []
                    }]
                })
                .to_string(),
            )
            .unwrap();
            std::fs::write(identity_dir.join("did.json"), "{}").unwrap();
            Self {
                root,
                did_domain: did_domain.to_owned(),
                anp_service_did: anp_service_did
                    .map(crate::ids::Did::parse)
                    .transpose()
                    .unwrap(),
            }
        }

        fn client(&self) -> crate::core::ImClient {
            crate::core::ImCore::new(
                crate::ImCoreConfig {
                    service_base_url: crate::ServiceEndpoint::parse("https://example.test")
                        .unwrap(),
                    did_domain: self.did_domain.clone(),
                    client_version_info: None,
                    user_service_endpoint: None,
                    message_service_endpoint: None,
                    mail_service_endpoint: None,
                    anp_service_endpoint: None,
                    anp_service_did: self.anp_service_did.clone(),
                    ca_bundle: None,
                    transport_policy: crate::MessageTransportPolicy::HttpOnly,
                },
                crate::ImCorePaths {
                    identities: crate::IdentityRegistryPaths {
                        identity_root_dir: self.root.join("identities"),
                        registry_path: self.root.join("identities").join("registry.json"),
                        default_identity_path: Some(self.root.join("identities").join("default")),
                    },
                    local_state: crate::LocalStatePaths {
                        sqlite_path: self.root.join("local").join("im.sqlite"),
                    },
                    runtime: crate::RuntimePaths {
                        cache_dir: self.root.join("cache"),
                        temp_dir: self.root.join("tmp"),
                    },
                },
            )
            .unwrap()
            .client(crate::identity::IdentitySelector::LocalAlias(
                "alice".to_owned(),
            ))
            .unwrap()
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn cleanup_failure_does_not_replace_a_successful_sync_outcome() {
        let successful_sync = crate::messages::MessageSyncOutcome {
            status: crate::messages::MessageSyncStatus::Changed,
            events_applied: 3,
            ..empty_outcome()
        };
        let cleanup_failure = Err(crate::ImError::LocalStateUnavailable {
            detail: "forced cleanup failure".to_owned(),
        });

        let preserved = preserve_success_after_cleanup(successful_sync.clone(), cleanup_failure);

        assert_eq!(preserved, successful_sync);
    }

    struct SyncSnapshotFixture {
        root: std::path::PathBuf,
    }

    impl SyncSnapshotFixture {
        const VAULT_SEED: [u8; 32] = [73_u8; 32];
        const WORKSPACE_ID: &'static str = "sync-snapshot-workspace";
        const VAULT_DEVICE_ID: &'static str = "sync-snapshot-vault-device";

        fn new(prefix: &str) -> Self {
            use crate::internal::identity_device_state::{
                DeviceAuthorizationProjection, DeviceAuthorizationRole, DeviceAuthorizationStatus,
                IdentityDeviceMode, IdentityDeviceState, IdentityInternalCheckpoint,
                IDENTITY_DEVICE_STATE_SCHEMA_VERSION,
            };
            use crate::internal::identity_store::{
                IdentityStore, SaveIdentityInput, SaveIdentityKeyMode, SaveIdentitySecretStorage,
            };

            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let root = std::env::temp_dir().join(format!(
                "im-core-sync-snapshot-{prefix}-{}-{nanos}",
                std::process::id()
            ));
            let paths = Self::paths(&root);
            std::fs::create_dir_all(&paths.identities.identity_root_dir).unwrap();
            let did = crate::ids::Did::parse("did:wba:awiki.test:alice:e1_root").unwrap();
            let signing_key_id = format!("{}#dev-a-sign", did.as_str());
            let e2ee_key_id = format!("{}#dev-a-e2ee", did.as_str());
            let vault = Arc::new(FileSecretVault::new(
                DeviceVaultRootKey::from_bytes(Self::VAULT_SEED),
                FileSecretVaultStore::new(root.join("vault")),
            ));
            IdentityStore::new(&paths.identities)
                .save_identity_with_secret_storage(
                    SaveIdentityInput {
                        local_alias: "alice".to_owned(),
                        did: did.clone(),
                        unique_id: "alice-id".to_owned(),
                        user_id: "account-alice".to_owned(),
                        display_name: "Alice".to_owned(),
                        handle: "alice".to_owned(),
                        full_handle: "alice.awiki.test".to_owned(),
                        binding_generation: Some("1".to_owned()),
                        jwt_token: "test-device-token".to_owned(),
                        did_document: Some(json!({"id": did.as_str()})),
                        key_mode: SaveIdentityKeyMode::VNext {
                            root_key_id: format!("{}#key-1", did.as_str()),
                            device_signing_key_id: signing_key_id.clone(),
                            device_e2ee_key_id: e2ee_key_id.clone(),
                        },
                        device_state: Some(IdentityDeviceState {
                            schema_version: IDENTITY_DEVICE_STATE_SCHEMA_VERSION,
                            mode: IdentityDeviceMode::VNext,
                            authorization: Some(DeviceAuthorizationProjection {
                                protocol_device_id: crate::ids::ProtocolDeviceId::parse("dev-a")
                                    .unwrap(),
                                signing_key_id,
                                e2ee_key_id,
                                status: DeviceAuthorizationStatus::Active,
                                role: DeviceAuthorizationRole::Member,
                                management_ready: false,
                                auth_generation: 1,
                            }),
                            checkpoint: Some(IdentityInternalCheckpoint {
                                document_version: 1,
                                document_hash: "sha256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
                                    .to_owned(),
                                registry_version: 1,
                            }),
                        }),
                        key1_private_pem: "root-private".to_owned(),
                        key1_public_pem: "root-public".to_owned(),
                        e2ee_signing_private_pem: "device-signing-private".to_owned(),
                        e2ee_agreement_private_pem: "device-e2ee-private".to_owned(),
                        daemon_subkey_package: None,
                        make_default: true,
                    },
                    SaveIdentitySecretStorage::Vault {
                        workspace_id: Self::WORKSPACE_ID.to_owned(),
                        device_id: Self::VAULT_DEVICE_ID.to_owned(),
                        vault,
                    },
                )
                .unwrap();
            Self { root }
        }

        fn paths(root: &std::path::Path) -> crate::ImCorePaths {
            crate::ImCorePaths {
                identities: crate::paths::IdentityRegistryPaths {
                    identity_root_dir: root.join("identities"),
                    registry_path: root.join("identities").join("registry.json"),
                    default_identity_path: Some(root.join("identities").join("default")),
                },
                local_state: crate::paths::LocalStatePaths {
                    sqlite_path: root.join("local").join("im.sqlite"),
                },
                runtime: crate::paths::RuntimePaths {
                    cache_dir: root.join("cache"),
                    temp_dir: root.join("tmp"),
                },
            }
        }

        fn client(&self) -> crate::core::ImClient {
            crate::core::ImCore::new_with_options(
                crate::ImCoreConfig {
                    service_base_url: crate::ServiceEndpoint::parse("https://example.test")
                        .unwrap(),
                    did_domain: "awiki.test".to_owned(),
                    client_version_info: None,
                    user_service_endpoint: None,
                    message_service_endpoint: None,
                    mail_service_endpoint: None,
                    anp_service_endpoint: None,
                    anp_service_did: None,
                    ca_bundle: None,
                    transport_policy: crate::MessageTransportPolicy::HttpOnly,
                },
                Self::paths(&self.root),
                crate::ImCoreOpenOptions::default().with_identity_secret_vault(
                    crate::IdentitySecretStoragePolicy::VaultRequired,
                    crate::ImCoreSecretVaultOptions::new(
                        DeviceVaultRootKey::from_bytes(Self::VAULT_SEED),
                        self.root.join("vault"),
                        Self::WORKSPACE_ID,
                        Self::VAULT_DEVICE_ID,
                    ),
                ),
            )
            .unwrap()
            .client(crate::identity::IdentitySelector::LocalAlias(
                "alice".to_owned(),
            ))
            .unwrap()
        }

        fn sqlite_path(&self) -> std::path::PathBuf {
            self.root.join("local").join("im.sqlite")
        }

        fn has_message_content(&self, content: &str) -> bool {
            rusqlite::Connection::open(self.sqlite_path())
                .unwrap()
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM messages WHERE content = ?1)",
                    [content],
                    |row| row.get::<_, bool>(0),
                )
                .unwrap()
        }
    }

    impl Drop for SyncSnapshotFixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    #[derive(Clone)]
    struct ReadySyncSnapshotSessionProvider;

    impl SessionProvider for ReadySyncSnapshotSessionProvider {
        fn ensure_session(
            &self,
            scope: crate::auth::AuthScope,
        ) -> crate::ImResult<crate::auth::SessionBundle> {
            assert_eq!(scope, crate::auth::AuthScope::Messaging);
            Ok(crate::auth::SessionBundle {
                subject: crate::ids::Did::parse("did:wba:awiki.test:alice:e1_root")?,
                scope,
                expires_at: None,
                refreshed: false,
                bearer_token: None,
            })
        }

        fn refresh_session(&self) -> crate::ImResult<crate::auth::SessionUpdate> {
            unreachable!("sync snapshot tests never refresh the session")
        }

        fn status(&self) -> crate::ImResult<crate::auth::AuthStatus> {
            unreachable!("sync snapshot tests never inspect session status")
        }
    }

    impl AsyncSessionProvider for ReadySyncSnapshotSessionProvider {
        async fn ensure_session(
            &self,
            scope: crate::auth::AuthScope,
        ) -> crate::ImResult<crate::auth::SessionBundle> {
            SessionProvider::ensure_session(self, scope)
        }

        async fn refresh_session(&self) -> crate::ImResult<crate::auth::SessionUpdate> {
            SessionProvider::refresh_session(self)
        }

        async fn status(&self) -> crate::ImResult<crate::auth::AuthStatus> {
            SessionProvider::status(self)
        }
    }

    #[derive(Debug, Clone)]
    struct SyncSnapshotCall {
        method: String,
        params: Value,
    }

    struct SyncSnapshotTransport {
        calls: Rc<RefCell<Vec<SyncSnapshotCall>>>,
        responses: VecDeque<crate::ImResult<Value>>,
    }

    impl SyncSnapshotTransport {
        fn queued(
            calls: Rc<RefCell<Vec<SyncSnapshotCall>>>,
            responses: Vec<crate::ImResult<Value>>,
        ) -> Self {
            Self {
                calls,
                responses: responses.into(),
            }
        }
    }

    impl AsyncAuthenticatedRpcTransport for SyncSnapshotTransport {
        async fn authenticated_rpc(
            &mut self,
            endpoint: &str,
            method: &str,
            params: Value,
        ) -> crate::ImResult<Value> {
            assert_eq!(endpoint, MESSAGE_RPC_ENDPOINT);
            self.calls.borrow_mut().push(SyncSnapshotCall {
                method: method.to_owned(),
                params,
            });
            self.responses
                .pop_front()
                .expect("queued sync snapshot response")
        }
    }

    struct HangingSyncSnapshotTransport {
        calls: Rc<RefCell<Vec<SyncSnapshotCall>>>,
    }

    impl AsyncAuthenticatedRpcTransport for HangingSyncSnapshotTransport {
        async fn authenticated_rpc(
            &mut self,
            endpoint: &str,
            method: &str,
            params: Value,
        ) -> crate::ImResult<Value> {
            assert_eq!(endpoint, MESSAGE_RPC_ENDPOINT);
            self.calls.borrow_mut().push(SyncSnapshotCall {
                method: method.to_owned(),
                params,
            });
            std::future::pending().await
        }
    }

    #[derive(Clone)]
    struct RefreshingSyncSnapshotSessionProvider {
        refresh_calls: Rc<RefCell<u32>>,
        fail_refresh: bool,
    }

    impl AsyncSessionProvider for RefreshingSyncSnapshotSessionProvider {
        async fn ensure_session(
            &self,
            scope: crate::auth::AuthScope,
        ) -> crate::ImResult<crate::auth::SessionBundle> {
            assert_eq!(scope, crate::auth::AuthScope::Messaging);
            Ok(crate::auth::SessionBundle {
                subject: crate::ids::Did::parse("did:wba:awiki.test:alice:e1_root")?,
                scope,
                expires_at: None,
                refreshed: false,
                bearer_token: None,
            })
        }

        async fn refresh_session(&self) -> crate::ImResult<crate::auth::SessionUpdate> {
            *self.refresh_calls.borrow_mut() += 1;
            if self.fail_refresh {
                return Err(crate::ImError::AuthRequired);
            }
            Ok(crate::auth::SessionUpdate {
                subject: crate::ids::Did::parse("did:wba:awiki.test:alice:e1_root")?,
                previous_expires_at: None,
                new_expires_at: None,
                refreshed: true,
                bearer_token: None,
            })
        }

        async fn status(&self) -> crate::ImResult<crate::auth::AuthStatus> {
            unreachable!("sync snapshot tests never inspect session status")
        }
    }

    struct ReloadingSyncSnapshotTransport {
        inner: SyncSnapshotTransport,
        authentication_reloads: Rc<RefCell<u32>>,
    }

    impl AsyncAuthenticatedRpcTransport for ReloadingSyncSnapshotTransport {
        async fn authenticated_rpc(
            &mut self,
            endpoint: &str,
            method: &str,
            params: Value,
        ) -> crate::ImResult<Value> {
            self.inner.authenticated_rpc(endpoint, method, params).await
        }

        fn reload_authentication_state(&mut self) -> crate::ImResult<()> {
            *self.authentication_reloads.borrow_mut() += 1;
            Ok(())
        }
    }

    struct NoopAsyncDirectoryTransport;

    impl AsyncRpcTransport for NoopAsyncDirectoryTransport {
        async fn rpc(
            &mut self,
            _endpoint: &str,
            _method: &str,
            _params: Value,
        ) -> crate::ImResult<Value> {
            unreachable!("group-only sync tests do not resolve Direct peers")
        }
    }

    fn sync_snapshot_request() -> crate::messages::MessageSyncRequest {
        crate::messages::MessageSyncRequest {
            reason: "app_resume".to_owned(),
            limit: Some(100),
        }
    }

    fn sync_snapshot_recovery(
        recovery_id: &str,
        token: &str,
        stream_epoch: &str,
        snapshot_scan_seq: &str,
    ) -> Value {
        json!({
            "mode": "compact_recovery_required",
            "server_time": "2026-07-28T12:00:03Z",
            "events": [],
            "next_cursor": null,
            "has_more": false,
            "recovery": {
                "recovery_id": recovery_id,
                "token": token,
                "stream_epoch": stream_epoch,
                "snapshot_scan_seq": snapshot_scan_seq,
                "message_cutoff": "2026-07-26T12:00:03Z",
                "message_limit": 500,
                "expires_at": "2026-07-28T12:10:03Z"
            },
            "warnings": []
        })
    }

    fn sync_snapshot_bootstrap_recovery(
        binding: &crate::identity::ActiveSyncAccountBinding,
        client_instance_id: &str,
        recovery_id: &str,
        token: &str,
        stream_epoch: &str,
        snapshot_scan_seq: &str,
    ) -> Value {
        let mut response = json!({
            "mode": "compact_recovery_required",
            "account_id": binding.account_id,
            "device_id": binding.protocol_device_id,
            "recovery": {
                "recovery_id": recovery_id,
                "token": token,
                "stream_epoch": stream_epoch,
                "snapshot_scan_seq": snapshot_scan_seq,
                "message_cutoff": "2026-07-26T12:00:03Z",
                "message_limit": 500,
                "expires_at": "2026-07-28T12:10:03Z"
            }
        });
        add_enabled_lane_bootstrap(&mut response, client_instance_id, "41", "42");
        response
    }

    async fn sync_snapshot_tail_bootstrap_for_current_features(
        client: &crate::core::ImClient,
        binding: &crate::identity::ActiveSyncAccountBinding,
        stream_epoch: &str,
        scan_seq: &str,
    ) -> Value {
        use crate::internal::wire::sync_v2::SyncLaneV3;

        let db = client.core_inner().local_state_db().await.unwrap();
        let client_instance_id = db
            .load_or_create_sync_client_instance_id(&binding.owner_identity_id)
            .await
            .unwrap();
        let desired = desired_v1b_lanes(&db, &binding.owner_identity_id)
            .await
            .unwrap();
        let mut sync_capabilities = Vec::new();
        let mut lanes = serde_json::Map::new();
        if desired.contains(&SyncLaneV3::P5Device) {
            sync_capabilities.push(crate::internal::wire::sync_v2::SYNC_CAPABILITY_P5_DEVICE_V1);
            lanes.insert(
                "p5_device".to_owned(),
                json!({
                    "cursor": {"stream_epoch": "41", "scan_seq": "0"},
                    "committed_seq": "0"
                }),
            );
        }
        if desired.contains(&SyncLaneV3::P6Group) {
            sync_capabilities.push(crate::internal::wire::sync_v2::SYNC_CAPABILITY_P6_GROUP_V1);
            sync_capabilities
                .push(crate::internal::wire::sync_v2::P6_DELIVERY_CONTEXT_CAPABILITY_V1);
            lanes.insert(
                "p6_group".to_owned(),
                json!({
                    "cursor": {"stream_epoch": "42", "scan_seq": "0"},
                    "committed_seq": "0"
                }),
            );
        }
        let mut response = json!({
            "mode": "tail_only",
            "account_id": binding.account_id,
            "device_id": binding.protocol_device_id,
            "server_time": "2026-08-28T00:00:00Z",
            "cursor": {"stream_epoch": stream_epoch, "scan_seq": scan_seq},
            "read_state_baseline": [],
            "group_state_baseline": [],
            "warnings": [],
            "sync_capabilities": sync_capabilities,
            "lanes": lanes
        });
        if desired.contains(&SyncLaneV3::P6Group) {
            response["p6_delivery"] = json!({
                "profile": crate::internal::wire::sync_v2::P6_DELIVERY_CONTEXT_CAPABILITY_V1,
                "client_instance_id": client_instance_id,
                "activated": true
            });
        }
        response
    }

    fn sync_snapshot_delta(stream_epoch: &str, next_scan_seq: &str, events: Vec<Value>) -> Value {
        json!({
            "mode": "delta",
            "server_time": "2026-07-28T12:00:05Z",
            "events": events,
            "next_cursor": {
                "stream_epoch": stream_epoch,
                "scan_seq": next_scan_seq
            },
            "has_more": false,
            "recovery": null,
            "warnings": []
        })
    }

    fn explicit_sync_negotiation_response() -> Value {
        json!({
            "supported_profiles": [
                crate::internal::wire::sync_v2::MESSAGE_SYNC_EXPLICIT_NEGOTIATION_V1
            ]
        })
    }

    fn add_enabled_lane_bootstrap(
        response: &mut Value,
        client_instance_id: &str,
        p5_stream_epoch: &str,
        p6_stream_epoch: &str,
    ) {
        #[allow(unused_mut)]
        let mut capabilities = Vec::new();
        #[allow(unused_mut)]
        let mut lanes = serde_json::Map::new();
        #[cfg(feature = "secure-direct")]
        {
            capabilities.push(json!(
                crate::internal::wire::sync_v2::SYNC_CAPABILITY_P5_DEVICE_V1
            ));
            lanes.insert(
                "p5_device".to_owned(),
                json!({
                    "cursor": {"stream_epoch": p5_stream_epoch, "scan_seq": "0"},
                    "committed_seq": "0"
                }),
            );
        }
        #[cfg(feature = "group-e2ee")]
        {
            capabilities.push(json!(
                crate::internal::wire::sync_v2::SYNC_CAPABILITY_P6_GROUP_V1
            ));
            lanes.insert(
                "p6_group".to_owned(),
                json!({
                    "cursor": {"stream_epoch": p6_stream_epoch, "scan_seq": "0"},
                    "committed_seq": "0"
                }),
            );
            response["p6_delivery"] = json!({
                "profile": crate::internal::wire::sync_v2::P6_DELIVERY_CONTEXT_CAPABILITY_V1,
                "client_instance_id": client_instance_id,
                "activated": true
            });
        }
        if !capabilities.is_empty() {
            response["sync_capabilities"] = Value::Array(capabilities);
            response["lanes"] = Value::Object(lanes);
        }
        let _ = (client_instance_id, p5_stream_epoch, p6_stream_epoch);
    }

    fn sync_snapshot_delta_with_lanes(
        stream_epoch: &str,
        next_scan_seq: &str,
        lanes: Value,
    ) -> Value {
        let mut response = sync_snapshot_delta(stream_epoch, next_scan_seq, Vec::new());
        response["lanes"] = lanes;
        response
    }

    fn poison_p5_lane_event(event_id: &str, seq: &str) -> Value {
        json!({
            "event_type": "p5.delivery.created",
            "delivery_id": event_id,
            "seq": seq,
            "envelope": {
                "meta": {
                    "profile": "anp.direct.e2ee.v2",
                    "security_profile": "direct-e2ee",
                    "message_id": event_id
                },
                "body": {},
                "server_seq": 1
            }
        })
    }

    fn p5_lane_event(delivery_id: &str, seq: &str, envelope: &Value) -> Value {
        json!({
            "event_type": "p5.delivery.created",
            "delivery_id": delivery_id,
            "seq": seq,
            "envelope": envelope
        })
    }

    fn realtime_inline_p5_notification(
        delivery_id: &str,
        stream_epoch: &str,
        event_seq: &str,
        envelope: &Value,
    ) -> Value {
        json!({
            "jsonrpc": "2.0",
            "method": "sync.changed",
            "params": {
                "domains": ["message"],
                "reason": "direct_message_available"
            },
            "sync": {
                "schema_version": 3,
                "domain_versions": {},
                "event": {
                    "lane": "p5_device",
                    "event_id": delivery_id,
                    "stream_epoch": stream_epoch,
                    "event_seq": event_seq,
                    "event_type": "p5.delivery.created"
                },
                "projection": envelope
            }
        })
    }

    #[cfg(feature = "group-e2ee")]
    fn p6_lane_envelope(
        binding: &crate::identity::ActiveSyncAccountBinding,
        message_id: &str,
        group_did: &str,
        group_event_seq: &str,
    ) -> Value {
        json!({
            "meta": {
                "anp_version": "2.0",
                "profile": "anp.group.e2ee.v2",
                "security_profile": "group-e2ee",
                "sender_did": binding.current_did.clone(),
                "sender_device_id": binding.protocol_device_id.clone(),
                "target": {"kind": "agent", "did": binding.current_did.clone()},
                "recipient_device_id": binding.protocol_device_id.clone(),
                "operation_id": message_id,
                "message_id": message_id,
                "content_type": anp::group_e2ee::GROUP_CIPHER_CONTENT_TYPE_V2
            },
            "auth": {
                "scheme": anp::group_e2ee::RFC9421_ORIGIN_PROOF_SCHEME_V2,
                "origin_proof": {
                    "contentDigest": "digest",
                    "signatureInput": "signature-input",
                    "signature": "signature"
                },
                "origin_context": {
                    "extra_meta": {"anp_version": "2.0"}
                }
            },
            "body": {
                "group_did": group_did,
                "group_event_seq": group_event_seq,
                "group_state_version": "1",
                "accepted_at": "2026-08-15T00:00:00Z",
                "group_receipt": {},
                "group_cipher_object": {
                    "crypto_group_id_b64u": "AA",
                    "epoch": "1",
                    "private_message_b64u": "AA",
                    "group_state_ref": {
                        "group_did": group_did,
                        "group_state_version": "1"
                    }
                }
            }
        })
    }

    #[cfg(feature = "group-e2ee")]
    fn realtime_inline_p6_notification(
        delivery_id: &str,
        stream_epoch: &str,
        event_seq: &str,
        group_did: &str,
        group_event_seq: &str,
        envelope: &Value,
    ) -> Value {
        json!({
            "jsonrpc": "2.0",
            "method": "sync.changed",
            "params": {
                "domains": ["message"],
                "reason": "group_message_available"
            },
            "sync": {
                "schema_version": 3,
                "domain_versions": {},
                "event": {
                    "lane": "p6_group",
                    "event_id": delivery_id,
                    "stream_epoch": stream_epoch,
                    "event_seq": event_seq,
                    "event_type": "p6.delivery.created",
                    "group_did": group_did,
                    "group_event_seq": group_event_seq
                },
                "projection": envelope
            }
        })
    }

    #[cfg(feature = "group-e2ee")]
    fn p6_lane_event(
        delivery_id: &str,
        seq: &str,
        group_did: &str,
        group_event_seq: &str,
        envelope: &Value,
    ) -> Value {
        json!({
            "event_type": "p6.delivery.created",
            "delivery_id": delivery_id,
            "seq": seq,
            "group_did": group_did,
            "group_event_seq": group_event_seq,
            "envelope": envelope
        })
    }

    fn sqlite_count(path: &std::path::Path, sql: &str) -> i64 {
        rusqlite::Connection::open(path)
            .unwrap()
            .query_row(sql, [], |row| row.get(0))
            .unwrap()
    }

    fn sync_group_member_changed_event(
        binding: &crate::identity::ActiveSyncAccountBinding,
        event_id: &str,
        event_seq: &str,
        group_did: &str,
        group_state_version: &str,
        group_event_seq: &str,
        actor_did: &str,
        subject_did: &str,
        membership_status: &str,
    ) -> Value {
        json!({
            "event_id": event_id,
            "stream_epoch": "1",
            "event_seq": event_seq,
            "event_type": "group.member_changed",
            "schema_version": 1,
            "ignore_safe": false,
            "account_id": binding.account_id,
            "recipient_device_id": null,
            "origin_did": actor_did,
            "origin_device_id": "device-actor",
            "aggregate_kind": "group",
            "aggregate_id": group_did,
            "state_version": group_state_version,
            "thread_key": group_did,
            "occurred_at": "2026-07-28T12:00:01Z",
            "payload": {
                "thread_kind": "group",
                "thread": {
                    "kind": "group",
                    "group_did": group_did
                },
                "group": {
                    "group_did": group_did,
                    "group_state_version": group_state_version,
                    "group_event_seq": group_event_seq
                },
                "membership": {
                    "actor_did": actor_did,
                    "subject_did": subject_did,
                    "status": membership_status
                }
            },
            "source": {
                "method": "group.add",
                "operation_id": event_id
            }
        })
    }

    #[cfg(feature = "group-e2ee")]
    #[test]
    fn strict_p6_lane_wire_rejects_legacy_delivery_before_checkpoint_advance() {
        let group_did = "did:wba:example.com:groups:strict:e1";
        let envelope = json!({
            "meta": {
                "profile": anp::group_e2ee::GROUP_E2EE_PROFILE_V2,
                "security_profile": anp::group_e2ee::GROUP_E2EE_SECURITY_PROFILE_V2,
                "sender_did": "did:wba:example.com:users:alice:e1",
                "sender_device_id": "dev-a",
                "target": {"kind": "agent", "did": "did:wba:example.com:users:bob:e1"},
                "recipient_device_id": "dev-b",
                "operation_id": "op-1",
                "message_id": "msg-1",
                "content_type": anp::group_e2ee::GROUP_CIPHER_CONTENT_TYPE_V2
            },
            "auth": {
                "scheme": anp::group_e2ee::RFC9421_ORIGIN_PROOF_SCHEME_V2,
                "origin_proof": {
                    "contentDigest": "digest",
                    "signatureInput": "signature-input",
                    "signature": "signature"
                }
            },
            "body": {
                "group_did": group_did,
                "group_state_version": "1",
                "group_event_seq": "1",
                "accepted_at": "2026-08-18T00:00:00Z",
                "group_receipt": {},
                "group_cipher_object": {
                    "group_state_ref": {
                        "group_did": group_did,
                        "group_state_version": "1"
                    },
                    "crypto_group_id_b64u": "AQ",
                    "epoch": "1",
                    "private_message_b64u": "AQ"
                }
            }
        });
        let legacy = crate::internal::wire::sync_v2::SyncLaneEventV3::P6Delivery {
            delivery_id: "delivery-1".to_owned(),
            seq: "1".to_owned(),
            group_did: group_did.to_owned(),
            group_event_seq: "1".to_owned(),
            envelope: envelope.clone(),
        };
        assert!(validate_strict_p6_lane_wire(&legacy).is_err());

        let mut strict_envelope = envelope;
        strict_envelope["auth"]["origin_context"] = json!({});
        let strict = crate::internal::wire::sync_v2::SyncLaneEventV3::P6Delivery {
            delivery_id: "delivery-1".to_owned(),
            seq: "1".to_owned(),
            group_did: group_did.to_owned(),
            group_event_seq: "1".to_owned(),
            envelope: strict_envelope,
        };
        validate_strict_p6_lane_wire(&strict).expect("strict P6 wire shape");
    }

    fn sync_group_profile_updated_event(
        binding: &crate::identity::ActiveSyncAccountBinding,
        event_id: &str,
        event_seq: &str,
        group_did: &str,
        group_state_version: &str,
        group_event_seq: &str,
        actor_did: &str,
    ) -> Value {
        json!({
            "event_id": event_id,
            "stream_epoch": "1",
            "event_seq": event_seq,
            "event_type": "group.profile_updated",
            "schema_version": 1,
            "ignore_safe": false,
            "account_id": binding.account_id,
            "recipient_device_id": null,
            "origin_did": actor_did,
            "origin_device_id": "device-actor",
            "aggregate_kind": "group",
            "aggregate_id": group_did,
            "state_version": group_state_version,
            "thread_key": group_did,
            "occurred_at": "2026-07-28T12:00:02Z",
            "payload": {
                "thread_kind": "group",
                "thread": {
                    "kind": "group",
                    "group_did": group_did
                },
                "group": {
                    "group_did": group_did,
                    "group_state_version": group_state_version,
                    "group_event_seq": group_event_seq,
                    "profile": {
                        "display_name": "Renamed Group"
                    }
                },
                "actor_did": actor_did
            },
            "source": {
                "method": "group.update_profile",
                "operation_id": event_id
            }
        })
    }

    fn sync_read_ack(
        binding: &crate::identity::ActiveSyncAccountBinding,
        thread_key: &str,
        seq: &str,
        message_id: &str,
        read_at: &str,
    ) -> Value {
        json!({
            "user_did": binding.current_did,
            "thread": {"kind": "direct", "thread_key": thread_key},
            "updated_count": 0,
            "remote_acknowledged": true,
            "partial": false,
            "fallback_used": false,
            "pending_remote_ack": false,
            "read_watermark_server_seq": seq,
            "previous_read_watermark_server_seq": Value::Null,
            "read_watermark_message_id": message_id,
            "advanced": true,
            "read_at": read_at,
            "unread_count": Value::Null,
            "warnings": []
        })
    }

    fn sync_group_read_ack(
        binding: &crate::identity::ActiveSyncAccountBinding,
        group_did: &str,
        seq: &str,
        message_id: &str,
        read_at: &str,
    ) -> Value {
        json!({
            "user_did": binding.current_did,
            "thread": {"kind": "group", "thread_key": group_did},
            "updated_count": 0,
            "remote_acknowledged": true,
            "partial": false,
            "fallback_used": false,
            "pending_remote_ack": false,
            "read_watermark_server_seq": seq,
            "previous_read_watermark_server_seq": Value::Null,
            "read_watermark_message_id": message_id,
            "advanced": true,
            "read_at": read_at,
            "unread_count": Value::Null,
            "warnings": []
        })
    }

    async fn sync_group_read_target_not_found(
        client: &crate::core::ImClient,
        calls: Rc<RefCell<Vec<SyncSnapshotCall>>>,
        next_scan_seq: &str,
    ) -> crate::messages::MessageSyncOutcome {
        MessageSyncRuntimeV2::new(
            client,
            ReadySyncSnapshotSessionProvider,
            SyncSnapshotTransport::queued(
                calls,
                vec![
                    Ok(sync_snapshot_delta("1", next_scan_seq, vec![])),
                    Err(crate::ImError::Service {
                        status_code: Some(404),
                        code: Some("anp.target_not_found".to_owned()),
                        message: "the Group target is not visible".to_owned(),
                        data: None,
                    }),
                ],
            ),
            NoopAsyncDirectoryTransport,
        )
        .sync_now(sync_snapshot_request())
        .await
        .unwrap()
    }

    fn sync_snapshot_message_event(
        binding: &crate::identity::ActiveSyncAccountBinding,
        event_id: &str,
        stream_epoch: &str,
        event_seq: &str,
        message_id: &str,
        group_did: &str,
    ) -> Value {
        json!({
            "event_id": event_id,
            "stream_epoch": stream_epoch,
            "event_seq": event_seq,
            "event_type": "message.created",
            "schema_version": 1,
            "ignore_safe": false,
            "account_id": binding.account_id,
            "recipient_device_id": null,
            "origin_did": "did:example:bob",
            "origin_device_id": "device-bob",
            "aggregate_kind": "group_message",
            "aggregate_id": message_id,
            "state_version": null,
            "thread_key": group_did,
            "occurred_at": "2026-07-28T12:00:04Z",
            "payload": {
                "message_kind": "group_plain",
                "direction": "incoming",
                "group_did": group_did,
                "sender_did_snapshot": "did:example:bob",
                "recipient_did_snapshot": binding.current_did,
                "client_message_id": message_id
            },
            "source": {}
        })
    }

    fn sync_snapshot_message(
        binding: &crate::identity::ActiveSyncAccountBinding,
        message_id: &str,
        group_did: &str,
        server_seq: &str,
        content: &str,
    ) -> Value {
        json!({
            "id": message_id,
            "thread_kind": "group",
            "group_did": group_did,
            "sender_did": "did:example:bob",
            "receiver_did": binding.current_did,
            "content_type": "text/plain",
            "content": content,
            "server_seq": server_seq,
            "created_at": "2026-07-28T12:00:04Z",
            "client_msg_id": message_id
        })
    }

    fn realtime_inline_group_notification(
        binding: &crate::identity::ActiveSyncAccountBinding,
        event_id: &str,
        event_seq: &str,
        message_id: &str,
        group_did: &str,
        content: &str,
    ) -> Value {
        let mut event =
            sync_snapshot_message_event(binding, event_id, "1", event_seq, message_id, group_did);
        event["payload"]["message_id"] = Value::String(message_id.to_owned());
        json!({
            "jsonrpc": "2.0",
            "method": "sync.changed",
            "params": {
                "domains": ["message"],
                "reason": "group_message_available"
            },
            "sync": {
                "schema_version": 3,
                "account_scan_seq_hint": event_seq,
                "domain_versions": {},
                "event": event,
                "projection": sync_snapshot_message(
                    binding,
                    message_id,
                    group_did,
                    "1",
                    content
                )
            }
        })
    }

    fn sync_direct_message_event(
        binding: &crate::identity::ActiveSyncAccountBinding,
        event_id: &str,
        event_seq: &str,
        message_id: &str,
        peer_did: &str,
        remote_thread_key: &str,
    ) -> Value {
        json!({
            "event_id": event_id,
            "stream_epoch": "1",
            "event_seq": event_seq,
            "event_type": "message.created",
            "schema_version": 1,
            "ignore_safe": false,
            "account_id": binding.account_id,
            "recipient_device_id": null,
            "origin_did": peer_did,
            "origin_device_id": "device-peer",
            "aggregate_kind": "direct_message",
            "aggregate_id": message_id,
            "state_version": null,
            "thread_key": remote_thread_key,
            "occurred_at": "2026-07-31T00:00:01Z",
            "payload": {
                "message_kind": "direct_plain",
                "direction": "incoming",
                "sender_did_snapshot": peer_did,
                "recipient_did_snapshot": binding.current_did,
                "client_message_id": message_id
            },
            "source": {}
        })
    }

    fn sync_direct_message(
        binding: &crate::identity::ActiveSyncAccountBinding,
        message_id: &str,
        peer_did: &str,
        content: &str,
    ) -> Value {
        json!({
            "id": message_id,
            "thread_kind": "direct",
            "sender_did": peer_did,
            "receiver_did": binding.current_did,
            "content_type": "text/plain",
            "content": content,
            "server_seq": "1",
            "created_at": "2026-07-31T00:00:01Z",
            "client_msg_id": message_id
        })
    }

    struct DirectLookupTransport {
        expected_did: String,
        calls: Rc<RefCell<u32>>,
    }

    struct FailingDirectoryTransport;

    impl AsyncRpcTransport for FailingDirectoryTransport {
        async fn rpc(
            &mut self,
            _endpoint: &str,
            _method: &str,
            _params: Value,
        ) -> crate::ImResult<Value> {
            Err(crate::ImError::TransportUnavailable {
                detail: "directory unavailable in test".to_owned(),
            })
        }
    }

    impl AsyncRpcTransport for DirectLookupTransport {
        async fn rpc(
            &mut self,
            _endpoint: &str,
            method: &str,
            params: Value,
        ) -> crate::ImResult<Value> {
            assert_eq!(method, "lookup");
            assert_eq!(params["did"], self.expected_did);
            *self.calls.borrow_mut() += 1;
            Ok(json!({
                "did": self.expected_did,
                "full_handle": "peer.awiki.info",
                "user_id": "user-peer",
                "domain": "awiki.info",
                "status": "active",
                "binding_generation": "1"
            }))
        }
    }

    fn sync_snapshot_response(
        binding: &crate::identity::ActiveSyncAccountBinding,
        stream_epoch: &str,
        snapshot_scan_seq: &str,
        recent_plain_messages: Vec<Value>,
    ) -> Value {
        json!({
            "mode": "compact_recovery",
            "account_id": binding.account_id,
            "device_id": binding.protocol_device_id,
            "server_time": "2026-07-28T12:00:04Z",
            "snapshot_cursor": {
                "stream_epoch": stream_epoch,
                "scan_seq": snapshot_scan_seq
            },
            "read_states": [],
            "groups": [],
            "message_policy": {
                "server_cutoff": "2026-07-26T12:00:03Z",
                "max_logical_messages": 500,
                "returned_logical_messages": recent_plain_messages.len()
            },
            "excluded": {
                "e2ee_messages": true,
                "plain_messages_before_cutoff": true
            },
            "recent_plain_messages": recent_plain_messages
        })
    }

    async fn seed_sync_snapshot_ready_state(
        client: &crate::core::ImClient,
        binding: &crate::identity::ActiveSyncAccountBinding,
        stream_epoch: &str,
        scan_seq: &str,
    ) {
        seed_legacy_sync_snapshot_ready_state(client, binding, stream_epoch, scan_seq).await;
        let db = client.core_inner().local_state_db().await.unwrap();
        let lanes = desired_v1b_lanes(&db, &binding.owner_identity_id)
            .await
            .unwrap();
        let states = lanes
            .iter()
            .map(
                |lane| crate::internal::local_state::sync_v2::LaneSyncState {
                    owner_identity_id: binding.owner_identity_id.clone(),
                    lane: *lane,
                    stream_epoch: stream_epoch.to_owned(),
                    scan_seq: "0".to_owned(),
                    committed_seq: "0".to_owned(),
                },
            )
            .collect::<Vec<_>>();
        let client_instance_id = db
            .load_or_create_sync_client_instance_id(&binding.owner_identity_id)
            .await
            .unwrap();
        let negotiated_capabilities_json = serde_json::to_string(
            &lanes
                .iter()
                .map(|lane| lane.capability())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        db.reconcile_sync_lane_capability_v1a(
            &binding.owner_identity_id,
            states,
            &binding.device_auth_generation,
            client_instance_id,
            negotiated_capabilities_json,
        )
        .await
        .unwrap();
    }

    async fn seed_legacy_sync_snapshot_ready_state(
        client: &crate::core::ImClient,
        binding: &crate::identity::ActiveSyncAccountBinding,
        stream_epoch: &str,
        scan_seq: &str,
    ) {
        client
            .core_inner()
            .local_state_db()
            .await
            .unwrap()
            .bootstrap_message_sync_state(crate::internal::local_state::sync_v2::MessageSyncState {
                owner_identity_id: binding.owner_identity_id.clone(),
                account_id: binding.account_id.clone(),
                protocol_device_id: binding.protocol_device_id.clone(),
                device_auth_generation: binding.device_auth_generation.clone(),
                stream_epoch: stream_epoch.to_owned(),
                scan_seq: scan_seq.to_owned(),
                bootstrap_state: "active".to_owned(),
                last_server_time: None,
                last_success_at: Some(1),
                last_error_code: None,
                metadata_json: None,
                updated_at: 1,
            })
            .await
            .unwrap();
    }

    async fn seed_lane_states(
        client: &crate::core::ImClient,
        binding: &crate::identity::ActiveSyncAccountBinding,
        lanes: &[(crate::internal::wire::sync_v2::SyncLaneV3, &str)],
    ) {
        let db = client.core_inner().local_state_db().await.unwrap();
        db.replace_lane_sync_states(
            &binding.owner_identity_id,
            lanes
                .iter()
                .map(
                    |(lane, epoch)| crate::internal::local_state::sync_v2::LaneSyncState {
                        owner_identity_id: binding.owner_identity_id.clone(),
                        lane: *lane,
                        stream_epoch: (*epoch).to_owned(),
                        scan_seq: "0".to_owned(),
                        committed_seq: "0".to_owned(),
                    },
                )
                .collect(),
        )
        .await
        .unwrap();
        let client_instance_id = db
            .load_or_create_sync_client_instance_id(&binding.owner_identity_id)
            .await
            .unwrap();
        let capabilities = lanes
            .iter()
            .map(|(lane, _)| lane.capability())
            .collect::<Vec<_>>();
        db.record_sync_lane_capability_negotiation_v1a(
            &binding.owner_identity_id,
            &binding.device_auth_generation,
            client_instance_id,
            serde_json::to_string(&capabilities).unwrap(),
        )
        .await
        .unwrap();
    }

    #[cfg(feature = "group-e2ee")]
    async fn seed_cached_p6_plaintext(
        client: &crate::core::ImClient,
        binding: &crate::identity::ActiveSyncAccountBinding,
        group_did: &str,
        group_event_seq: &str,
        content: &str,
    ) {
        client
            .core_inner()
            .local_state_db()
            .await
            .unwrap()
            .store_messages(vec![
                crate::internal::local_state::messages::MessageRecord {
                    msg_id: format!("{group_did}:{group_event_seq}"),
                    owner_identity_id: binding.owner_identity_id.clone(),
                    owner_did: binding.current_did.clone(),
                    conversation_id: format!("group:{group_did}"),
                    thread_id: format!("group:{group_did}"),
                    direction: -1,
                    sender_did: binding.current_did.clone(),
                    group_id: group_did.to_owned(),
                    group_did: group_did.to_owned(),
                    content_type: "text/plain".to_owned(),
                    content: content.to_owned(),
                    server_seq: group_event_seq.parse().ok(),
                    is_e2ee: true,
                    metadata: json!({
                        "decryption_state": "decrypted",
                        "security": "group-e2ee"
                    })
                    .to_string(),
                    ..crate::internal::local_state::messages::MessageRecord::default()
                },
            ])
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn v1a_serialized_empty_renegotiation_preserves_history_and_polls_no_secure_lanes() {
        use crate::internal::wire::sync_v2::SyncLaneV3;

        let fixture = SyncSnapshotFixture::new("v1a-explicit-zero");
        let client = fixture.client();
        let binding = client.active_sync_account_binding().await.unwrap();
        seed_legacy_sync_snapshot_ready_state(&client, &binding, "1", "0").await;
        client
            .core_inner()
            .local_state_db()
            .await
            .unwrap()
            .replace_lane_sync_states(
                &binding.owner_identity_id,
                vec![
                    crate::internal::local_state::sync_v2::LaneSyncState {
                        owner_identity_id: binding.owner_identity_id.clone(),
                        lane: SyncLaneV3::P5Device,
                        stream_epoch: "41".to_owned(),
                        scan_seq: "7".to_owned(),
                        committed_seq: "7".to_owned(),
                    },
                    crate::internal::local_state::sync_v2::LaneSyncState {
                        owner_identity_id: binding.owner_identity_id.clone(),
                        lane: SyncLaneV3::P6Group,
                        stream_epoch: "42".to_owned(),
                        scan_seq: "9".to_owned(),
                        committed_seq: "9".to_owned(),
                    },
                ],
            )
            .await
            .unwrap();
        let connection = rusqlite::Connection::open(fixture.sqlite_path()).unwrap();
        for lane in ["p5_device", "p6_group"] {
            connection
                .execute(
                    "INSERT INTO sync_lane_transport_state(
                         owner_identity_id,lane,last_transport_error,updated_at
                     ) VALUES (?1,?2,'lane_consumer_not_ready',1)",
                    rusqlite::params![binding.owner_identity_id, lane],
                )
                .unwrap();
        }
        let calls = Rc::new(RefCell::new(Vec::new()));
        let transport = SyncSnapshotTransport::queued(
            Rc::clone(&calls),
            vec![
                Ok(explicit_sync_negotiation_response()),
                Ok(json!({
                    "mode": "tail_only",
                    "account_id": binding.account_id,
                    "device_id": binding.protocol_device_id,
                    "server_time": "2026-08-27T00:00:00Z",
                    "cursor": {"stream_epoch": "1", "scan_seq": "0"},
                    "read_state_baseline": [],
                    "group_state_baseline": [],
                    "warnings": [],
                    "sync_capabilities": []
                })),
                Ok(sync_snapshot_delta("1", "0", Vec::new())),
            ],
        );

        let outcome = MessageSyncRuntimeV2::new(
            &client,
            ReadySyncSnapshotSessionProvider,
            transport,
            NoopAsyncDirectoryTransport,
        )
        .sync_now(sync_snapshot_request())
        .await
        .unwrap();
        assert_eq!(outcome.status, crate::messages::MessageSyncStatus::Idle);

        let calls = calls.borrow();
        assert_eq!(
            calls
                .iter()
                .map(|call| call.method.as_str())
                .collect::<Vec<_>>(),
            ["anp.get_capabilities", "sync.bootstrap", "sync.delta"]
        );
        assert_eq!(
            calls[1].params["body"]["capabilities"]["requested_sync_capabilities"],
            json!([])
        );
        assert!(calls[2].params["body"].get("lanes").is_none());
        assert!(calls[2].params["body"].get("p6_delivery").is_none());
        let db = client.core_inner().local_state_db().await.unwrap();
        assert!(db
            .load_lane_sync_states(binding.owner_identity_id.clone())
            .await
            .unwrap()
            .is_empty());
        let connection = rusqlite::Connection::open(fixture.sqlite_path()).unwrap();
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM lane_sync_state WHERE owner_identity_id = ?1",
                    [&binding.owner_identity_id],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            2
        );
    }

    #[tokio::test]
    async fn v1a_v1b_cross_stream_malformed_lane_does_not_rollback_ordinary_delta() {
        use crate::internal::wire::sync_v2::SyncLaneV3;

        let fixture = SyncSnapshotFixture::new("v1a-malformed-lane-isolation");
        let client = fixture.client();
        let binding = client.active_sync_account_binding().await.unwrap();
        seed_sync_snapshot_ready_state(&client, &binding, "1", "0").await;
        seed_lane_states(&client, &binding, &[(SyncLaneV3::P5Device, "41")]).await;
        let group_did = "did:wba:awiki.test:groups:v1a-lane-isolation";
        let mut response = sync_snapshot_delta(
            "1",
            "1",
            vec![sync_group_member_changed_event(
                &binding,
                "v1a-ordinary-event-1",
                "1",
                group_did,
                "1",
                "1",
                "did:wba:awiki.test:user:bob:e1_actor",
                "did:wba:awiki.test:user:carol:e1_subject",
                "active",
            )],
        );
        response["lanes"] = json!({
            "p5_device": {
                "events": "malformed",
                "next_cursor": {"stream_epoch": "41", "scan_seq": "1"},
                "has_more": false
            }
        });

        let outcome = MessageSyncRuntimeV2::new(
            &client,
            ReadySyncSnapshotSessionProvider,
            SyncSnapshotTransport::queued(Rc::new(RefCell::new(Vec::new())), vec![Ok(response)]),
            NoopAsyncDirectoryTransport,
        )
        .sync_now(sync_snapshot_request())
        .await
        .unwrap();

        assert_eq!(outcome.events_applied, 1);
        assert!(outcome
            .warnings
            .contains(&"sync.lane.p5_device.transport_invalid".to_owned()));
        let state = load_sync_snapshot_state(&client, &binding.owner_identity_id).await;
        assert_eq!(state.scan_seq, "1");
        let lane_states = client
            .core_inner()
            .local_state_db()
            .await
            .unwrap()
            .load_lane_sync_states(binding.owner_identity_id)
            .await
            .unwrap();
        assert_eq!(lane_states.len(), 1);
        assert_eq!(lane_states[0].lane, SyncLaneV3::P5Device);
        assert_eq!(lane_states[0].scan_seq, "0");
    }

    #[cfg(feature = "group-e2ee")]
    #[tokio::test]
    async fn v1b_cross_stream_ordinary_failure_keeps_p5_and_p6_handoffs_committed() {
        use crate::internal::wire::sync_v2::SyncLaneV3;

        let fixture = SyncSnapshotFixture::new("v1b-ordinary-failure-secure-handoffs");
        let client = fixture.client();
        let binding = client.active_sync_account_binding().await.unwrap();
        seed_sync_snapshot_ready_state(&client, &binding, "1", "0").await;
        seed_lane_states(
            &client,
            &binding,
            &[(SyncLaneV3::P5Device, "41"), (SyncLaneV3::P6Group, "42")],
        )
        .await;
        let group_did = "did:wba:awiki.test:groups:v1b-ordinary-failure";
        let p6_envelope = p6_lane_envelope(&binding, "v1b-p6-handoff-message-1", group_did, "7");
        let mut response = sync_snapshot_delta(
            "1",
            "1",
            vec![sync_snapshot_message_event(
                &binding,
                "v1b-ordinary-event-1",
                "1",
                "1",
                "v1b-ordinary-message-1",
                group_did,
            )],
        );
        response["lanes"] = json!({
            "p5_device": {
                "events": [poison_p5_lane_event("v1b-p5-handoff-1", "1")],
                "next_cursor": {"stream_epoch": "41", "scan_seq": "1"},
                "has_more": false
            },
            "p6_group": {
                "events": [p6_lane_event(
                    "v1b-p6-handoff-1",
                    "1",
                    group_did,
                    "7",
                    &p6_envelope
                )],
                "next_cursor": {"stream_epoch": "42", "scan_seq": "1"},
                "has_more": false
            }
        });
        let error = MessageSyncRuntimeV2::new(
            &client,
            ReadySyncSnapshotSessionProvider,
            SyncSnapshotTransport::queued(
                Rc::new(RefCell::new(Vec::new())),
                vec![
                    Ok(response),
                    Err(crate::ImError::Serialization {
                        detail: "forced ordinary hydration failure".to_owned(),
                    }),
                ],
            ),
            NoopAsyncDirectoryTransport,
        )
        .sync_now(sync_snapshot_request())
        .await
        .expect_err("ordinary hydration must fail independently");
        assert!(matches!(error, crate::ImError::Serialization { .. }));

        let ordinary = load_sync_snapshot_state(&client, &binding.owner_identity_id).await;
        assert_eq!(ordinary.scan_seq, "0");
        let lane = client
            .core_inner()
            .local_state_db()
            .await
            .unwrap()
            .load_lane_sync_states(binding.owner_identity_id.clone())
            .await
            .unwrap();
        assert_eq!(lane.len(), 2);
        assert!(lane.iter().all(|state| state.scan_seq == "1"));
        let connection = rusqlite::Connection::open(fixture.sqlite_path()).unwrap();
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM sync_lane_inbox
                     WHERE owner_identity_id=?1",
                    [&binding.owner_identity_id],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            2
        );
    }

    #[tokio::test]
    async fn v1b_cross_stream_shared_sqlite_failure_is_a_global_fence() {
        use crate::internal::wire::sync_v2::SyncLaneV3;
        let fixture = SyncSnapshotFixture::new("v1b-global-sqlite-fence");
        let client = fixture.client();
        let binding = client.active_sync_account_binding().await.unwrap();
        seed_sync_snapshot_ready_state(&client, &binding, "1", "0").await;
        seed_lane_states(&client, &binding, &[(SyncLaneV3::P5Device, "41")]).await;
        let connection = rusqlite::Connection::open(fixture.sqlite_path()).unwrap();
        connection
            .execute_batch(
                r#"
CREATE TRIGGER fail_v1b_shared_lane_inbox
BEFORE INSERT ON sync_lane_inbox
BEGIN
    SELECT RAISE(ABORT, 'forced shared SQLite failure');
END;
"#,
            )
            .unwrap();
        let mut response = sync_snapshot_delta(
            "1",
            "1",
            vec![sync_group_profile_updated_event(
                &binding,
                "v1b-global-ordinary-1",
                "1",
                "did:wba:awiki.test:groups:v1b-global-fence",
                "1",
                "1",
                "did:wba:awiki.test:user:owner:e1_actor",
            )],
        );
        response["lanes"] = json!({
            "p5_device": {
                "events": [poison_p5_lane_event("v1b-global-p5-1", "1")],
                "next_cursor": {"stream_epoch": "41", "scan_seq": "1"},
                "has_more": false
            }
        });
        let error = MessageSyncRuntimeV2::new(
            &client,
            ReadySyncSnapshotSessionProvider,
            SyncSnapshotTransport::queued(Rc::new(RefCell::new(Vec::new())), vec![Ok(response)]),
            NoopAsyncDirectoryTransport,
        )
        .sync_now(sync_snapshot_request())
        .await
        .expect_err("shared SQLite failure must stop every stream commit");
        assert!(matches!(
            error,
            crate::ImError::LocalStateUnavailable { .. }
        ));
        assert_eq!(
            load_sync_snapshot_state(&client, &binding.owner_identity_id)
                .await
                .scan_seq,
            "0"
        );
        assert_eq!(
            client
                .core_inner()
                .local_state_db()
                .await
                .unwrap()
                .load_lane_sync_states(binding.owner_identity_id)
                .await
                .unwrap()[0]
                .scan_seq,
            "0"
        );
    }

    #[tokio::test]
    async fn v1a_run_deadline_cancels_hung_rpc_and_allows_the_next_run() {
        let fixture = SyncSnapshotFixture::new("v1a-run-deadline");
        let client = fixture.client();
        let binding = client.active_sync_account_binding().await.unwrap();
        seed_sync_snapshot_ready_state(&client, &binding, "1", "0").await;
        seed_lane_states(&client, &binding, &[]).await;

        let hanging_calls = Rc::new(RefCell::new(Vec::new()));
        let timed_out = MessageSyncRuntimeV2::new(
            &client,
            ReadySyncSnapshotSessionProvider,
            HangingSyncSnapshotTransport {
                calls: Rc::clone(&hanging_calls),
            },
            NoopAsyncDirectoryTransport,
        )
        .with_run_deadline_for_test(StdDuration::from_millis(500))
        .sync_now(sync_snapshot_request())
        .await
        .unwrap();
        assert!(timed_out
            .warnings
            .contains(&"sync.budget_exhausted".to_owned()));
        assert_eq!(hanging_calls.borrow()[0].method, "sync.delta");

        let resumed_calls = Rc::new(RefCell::new(Vec::new()));
        let resumed = MessageSyncRuntimeV2::new(
            &client,
            ReadySyncSnapshotSessionProvider,
            SyncSnapshotTransport::queued(
                Rc::clone(&resumed_calls),
                vec![Ok(sync_snapshot_delta("1", "0", Vec::new()))],
            ),
            NoopAsyncDirectoryTransport,
        )
        .with_run_deadline_for_test(StdDuration::from_secs(2))
        .sync_now(sync_snapshot_request())
        .await
        .unwrap();
        assert_eq!(resumed.status, crate::messages::MessageSyncStatus::Idle);
        assert_eq!(resumed_calls.borrow()[0].method, "sync.delta");
    }

    #[tokio::test]
    async fn v1a_blocked_wire_error_clears_durable_retry_schedule() {
        let fixture = SyncSnapshotFixture::new("v1a-blocked-error");
        let client = fixture.client();
        let binding = client.active_sync_account_binding().await.unwrap();
        seed_sync_snapshot_ready_state(&client, &binding, "1", "0").await;
        seed_lane_states(&client, &binding, &[]).await;
        let error = MessageSyncRuntimeV2::new(
            &client,
            ReadySyncSnapshotSessionProvider,
            SyncSnapshotTransport::queued(
                Rc::new(RefCell::new(Vec::new())),
                vec![Err(crate::ImError::Service {
                    status_code: Some(409),
                    code: Some("sync.client_upgrade_required".to_owned()),
                    message: "upgrade required".to_owned(),
                    data: None,
                })],
            ),
            NoopAsyncDirectoryTransport,
        )
        .sync_now(sync_snapshot_request())
        .await
        .expect_err("client upgrade must stop the run");
        assert_eq!(
            failure_outcome(&error).unwrap().status,
            crate::messages::MessageSyncStatus::Blocked
        );
        let run = client
            .core_inner()
            .local_state_db()
            .await
            .unwrap()
            .load_message_sync_run_state(binding.owner_identity_id)
            .await
            .unwrap()
            .unwrap();
        assert!(!run.sync_pending);
        assert_eq!(run.next_retry_at, None);
    }

    #[test]
    fn v1b_handoff_adapter_consumes_the_portable_production_delta_fixture() {
        use crate::internal::wire::sync_v2::{parse_delta, SyncLaneDeltaSectionV3, SyncLaneV3};
        let fixture_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("testdata/message-sync/lane-handoff-fixtures.json");
        let fixture: Value = serde_json::from_slice(&std::fs::read(fixture_path).unwrap()).unwrap();
        let cases = fixture["cases"].as_array().unwrap();
        let mut adapted_event_types = BTreeSet::new();
        for case in cases
            .iter()
            .filter(|case| case.get("serialized_delta").is_some())
        {
            let expected = &case["envelope"];
            let lane = match expected["lane_kind"].as_str().unwrap() {
                "p5_device" => SyncLaneV3::P5Device,
                "p6_group" => SyncLaneV3::P6Group,
                other => panic!("unexpected fixture lane {other}"),
            };
            let page = parse_delta(&case["serialized_delta"]).unwrap();
            let SyncLaneDeltaSectionV3::Page { events, .. } = &page.lanes[&lane] else {
                panic!("fixture lane must decode as a page")
            };
            let [event] = events.as_slice() else {
                panic!("fixture must contain one production lane event")
            };
            let replica = &expected["replica_binding"];
            let binding = crate::identity::ActiveSyncAccountBinding {
                owner_identity_id: replica["owner_identity_id"].as_str().unwrap().to_owned(),
                account_id: replica["account_id"].as_str().unwrap().to_owned(),
                current_did: "did:wba:example.test:users:alice:e1_owner".to_owned(),
                protocol_device_id: replica["device_id"].as_str().unwrap().to_owned(),
                identity_generation: "1".to_owned(),
                device_auth_generation: replica["auth_generation"].as_str().unwrap().to_owned(),
            };
            let adapted = lane_handoff_input_from_wire(
                &binding,
                replica["client_instance_id"].as_str().unwrap(),
                expected["lane_epoch"].as_str().unwrap(),
                event,
                expected["received_at"].as_str().unwrap(),
            )
            .unwrap();
            assert_eq!(adapted.lane.as_str(), expected["lane_kind"]);
            assert_eq!(adapted.lane_epoch, expected["lane_epoch"]);
            assert_eq!(adapted.position, expected["position"]);
            assert_eq!(adapted.event_id, expected["event_id"]);
            assert_eq!(adapted.event_type, expected["event_type"]);
            assert_eq!(adapted.raw_payload, expected["raw_payload"]);
            assert_eq!(adapted.received_at, expected["received_at"]);
            assert_eq!(adapted.source_created_at, None);
            assert_eq!(adapted.source_expires_at, None);
            adapted_event_types.insert(adapted.event_type);
        }
        assert_eq!(
            adapted_event_types,
            BTreeSet::from([
                "p5.delivery.created".to_owned(),
                "p6.delivery.created".to_owned(),
                "p6.control.notice".to_owned(),
            ])
        );
    }

    #[test]
    fn v1b_p5_consumer_lock_is_scoped_by_peer() {
        use crate::internal::wire::sync_v2::SyncLaneV3;
        let first = lane_consumer_scope_lock("owner-a", SyncLaneV3::P5Device, "did:peer:a");
        let same_peer = lane_consumer_scope_lock("owner-a", SyncLaneV3::P5Device, "did:peer:a");
        let another_peer = lane_consumer_scope_lock("owner-a", SyncLaneV3::P5Device, "did:peer:b");
        assert!(Arc::ptr_eq(&first, &same_peer));
        assert!(!Arc::ptr_eq(&first, &another_peer));
    }

    #[test]
    fn v1b_p5_own_sync_defers_peer_validation_and_commits_the_inner_target_scope() {
        assert_eq!(
            p5_expected_decryption_peer("did:owner", "did:peer"),
            Some("did:owner")
        );
        assert_eq!(
            p5_expected_decryption_peer("did:owner", "did:owner"),
            None,
            "an own-sync envelope carries its business peer only inside authenticated plaintext"
        );
        assert_eq!(
            p5_committed_domain_scope("did:owner", Some("did:peer")),
            "did:peer"
        );
        assert_eq!(p5_committed_domain_scope("did:peer", None), "did:peer");
    }

    #[test]
    fn v1b_p5_lane_builds_a_trusted_reliable_delivery_context() {
        let metadata = anp::direct_e2ee::V2DirectMetadata {
            anp_version: None,
            profile: anp::direct_e2ee::DIRECT_E2EE_PROFILE_V2.to_owned(),
            security_profile: anp::direct_e2ee::DIRECT_E2EE_SECURITY_PROFILE.to_owned(),
            sender_did: "did:wba:example.test:users:alice:e1_test".to_owned(),
            sender_device_id: "device-a".to_owned(),
            target: anp::direct_e2ee::V2Target {
                kind: "agent".to_owned(),
                did: "did:wba:example.test:users:bob:e1_test".to_owned(),
            },
            recipient_device_id: "device-b".to_owned(),
            operation_id: "root-message-1".to_owned(),
            message_id: "root-message-1".to_owned(),
            content_type: anp::direct_e2ee::CONTENT_TYPE_DIRECT_CIPHER_V2.to_owned(),
            created_at: None,
        };

        let context = p5_reliable_delivery_context(
            &metadata,
            &json!({"accepted_at": "2026-08-28T00:00:00Z"}),
        )
        .expect("a committed lane envelope is an authenticated reliable delivery");
        assert_eq!(
            context.source,
            crate::internal::identity_root_import_completion::TrustedDirectDeliverySource::ReliableSync
        );
        assert_eq!(context.accepted_at.as_deref(), Some("2026-08-28T00:00:00Z"));
        assert_eq!(context.message_id, "root-message-1");
        assert_eq!(context.sender_device_id, "device-a");
        assert_eq!(context.recipient_device_id, "device-b");
        let ordinary_compatible = p5_reliable_delivery_context(&metadata, &json!({})).unwrap();
        assert_eq!(ordinary_compatible.accepted_at, None);
    }

    #[test]
    fn v1b_p6_consumer_lock_is_scoped_by_group() {
        use crate::internal::wire::sync_v2::SyncLaneV3;
        let first = lane_consumer_scope_lock("owner-a", SyncLaneV3::P6Group, "did:group:a");
        let same_group = lane_consumer_scope_lock("owner-a", SyncLaneV3::P6Group, "did:group:a");
        let another_group = lane_consumer_scope_lock("owner-a", SyncLaneV3::P6Group, "did:group:b");
        assert!(Arc::ptr_eq(&first, &same_group));
        assert!(!Arc::ptr_eq(&first, &another_group));
    }

    #[tokio::test]
    async fn v1b_p5_one_bad_peer_does_not_block_another_peer() {
        use crate::internal::local_state::sync_v2::{SyncLaneDomainStatus, SyncLaneHandoffInput};
        use crate::internal::wire::sync_v2::SyncLaneV3;
        let fixture = SyncSnapshotFixture::new("v1b-p5-peer-isolation");
        let client = fixture.client();
        let binding = client.active_sync_account_binding().await.unwrap();
        seed_sync_snapshot_ready_state(&client, &binding, "1", "0").await;
        seed_lane_states(&client, &binding, &[(SyncLaneV3::P5Device, "41")]).await;
        let db = client.core_inner().local_state_db().await.unwrap();
        let client_instance_id = db
            .load_or_create_sync_client_instance_id(&binding.owner_identity_id)
            .await
            .unwrap();
        for (position, peer) in [("1", "did:example:peer-a"), ("2", "did:example:peer-b")] {
            db.commit_sync_lane_handoff(SyncLaneHandoffInput {
                owner_identity_id: binding.owner_identity_id.clone(),
                account_id_snapshot: binding.account_id.clone(),
                device_id_snapshot: binding.protocol_device_id.clone(),
                auth_generation_snapshot: binding.device_auth_generation.clone(),
                client_instance_id_snapshot: client_instance_id.clone(),
                lane: SyncLaneV3::P5Device,
                lane_epoch: "41".to_owned(),
                position: position.to_owned(),
                event_id: format!("v1b-p5-bad-{position}"),
                event_type: "p5.delivery.created".to_owned(),
                raw_payload: json!({
                    "meta": {"sender_did": peer, "message_id": format!("v1b-p5-bad-{position}")},
                    "body": {}
                }),
                group_did: None,
                received_at: "2026-08-28T00:00:00Z".to_owned(),
                source_created_at: None,
                source_expires_at: None,
            })
            .await
            .unwrap();
        }
        let summary = drain_p5_lane_inputs(&client, 16).await.unwrap();
        assert_eq!(summary.closed, 2);
        let states = db
            .load_sync_lane_domain_states(binding.owner_identity_id.clone())
            .await
            .unwrap();
        assert_eq!(states.len(), 2);
        assert!(states
            .iter()
            .all(|state| state.status == SyncLaneDomainStatus::Terminal));
        assert_eq!(
            states
                .iter()
                .map(|state| state.scope.as_str())
                .collect::<BTreeSet<_>>(),
            BTreeSet::from(["did:example:peer-a", "did:example:peer-b"])
        );
        assert_eq!(
            db.load_lane_sync_states(binding.owner_identity_id.clone())
                .await
                .unwrap()[0]
                .scan_seq,
            "2"
        );
    }

    #[tokio::test]
    async fn v1b_p6_one_bad_group_does_not_block_another_group() {
        use crate::internal::local_state::sync_v2::{
            stable_sync_lane_input_id, SyncLaneHandoffInput, SyncLaneInboxRecord,
        };
        use crate::internal::wire::sync_v2::SyncLaneV3;
        let fixture = SyncSnapshotFixture::new("v1b-p6-group-isolation");
        let client = fixture.client();
        let binding = client.active_sync_account_binding().await.unwrap();
        seed_sync_snapshot_ready_state(&client, &binding, "1", "0").await;
        seed_lane_states(&client, &binding, &[(SyncLaneV3::P6Group, "42")]).await;
        let db = client.core_inner().local_state_db().await.unwrap();
        let client_instance_id = db
            .load_or_create_sync_client_instance_id(&binding.owner_identity_id)
            .await
            .unwrap();
        for (position, group_did, event_type, raw_payload) in [
            (
                "1",
                "did:example:group-a",
                "p6.delivery.created",
                json!({"meta": {}, "body": {"group_did": "did:example:group-a"}}),
            ),
            (
                "2",
                "did:example:group-b",
                "p6.control.notice",
                json!({
                    "meta": {},
                    "body": {"group_did": "did:example:group-b", "notice_type": "commit"}
                }),
            ),
        ] {
            db.commit_sync_lane_handoff(SyncLaneHandoffInput {
                owner_identity_id: binding.owner_identity_id.clone(),
                account_id_snapshot: binding.account_id.clone(),
                device_id_snapshot: binding.protocol_device_id.clone(),
                auth_generation_snapshot: binding.device_auth_generation.clone(),
                client_instance_id_snapshot: client_instance_id.clone(),
                lane: SyncLaneV3::P6Group,
                lane_epoch: "42".to_owned(),
                position: position.to_owned(),
                event_id: format!("v1b-p6-bad-{position}"),
                event_type: event_type.to_owned(),
                raw_payload,
                group_did: Some(group_did.to_owned()),
                received_at: "2026-08-28T00:00:00Z".to_owned(),
                source_created_at: None,
                source_expires_at: None,
            })
            .await
            .unwrap();
        }
        let summary = drain_p6_lane_inputs(&client, 16).await.unwrap();
        assert_eq!(summary.closed, 2);
        let states = db
            .load_sync_lane_domain_states(binding.owner_identity_id.clone())
            .await
            .unwrap();
        assert_eq!(states.len(), 2);
        assert!(states.iter().all(|state| state.status.is_closed()));
        assert_eq!(
            states
                .iter()
                .map(|state| state.scope.as_str())
                .collect::<BTreeSet<_>>(),
            BTreeSet::from(["did:example:group-a", "did:example:group-b"])
        );
        assert_eq!(
            db.load_lane_sync_states(binding.owner_identity_id.clone())
                .await
                .unwrap()[0]
                .scan_seq,
            "2"
        );
        let already_closed = SyncLaneInboxRecord {
            input_id: stable_sync_lane_input_id(
                &binding.owner_identity_id,
                SyncLaneV3::P6Group,
                "42",
                "v1b-p6-bad-1",
            )
            .unwrap(),
            owner_identity_id: binding.owner_identity_id,
            lane: SyncLaneV3::P6Group,
            lane_epoch: "42".to_owned(),
            position: "1".to_owned(),
            event_id: "v1b-p6-bad-1".to_owned(),
            event_type: "p6.delivery.created".to_owned(),
            raw_payload: json!({
                "meta": {},
                "body": {"group_did": "did:example:group-a"}
            }),
            group_did: Some("did:example:group-a".to_owned()),
            received_at: "2026-08-28T00:00:00Z".to_owned(),
            source_created_at: None,
            source_expires_at: None,
        };
        assert!(consume_p6_lane_input(&client, &already_closed, 2)
            .await
            .unwrap()
            .is_closed());
    }

    #[cfg(feature = "group-e2ee")]
    #[tokio::test]
    async fn v1b_p6_application_success_and_replay_leave_cursor_domain_independent() {
        use crate::internal::local_state::sync_v2::{
            stable_sync_lane_input_id, SyncLaneDomainStatus, SyncLaneHandoffInput,
            SyncLaneInboxRecord,
        };
        use crate::internal::wire::sync_v2::SyncLaneV3;

        let fixture = SyncSnapshotFixture::new("v1b-p6-application-replay");
        let client = fixture.client();
        let binding = client.active_sync_account_binding().await.unwrap();
        seed_sync_snapshot_ready_state(&client, &binding, "1", "0").await;
        seed_lane_states(&client, &binding, &[(SyncLaneV3::P6Group, "42")]).await;
        let db = client.core_inner().local_state_db().await.unwrap();
        let client_instance_id = db
            .load_or_create_sync_client_instance_id(&binding.owner_identity_id)
            .await
            .unwrap();
        let group_did = "did:wba:awiki.test:groups:v1b-p6-application";
        let event_id = "v1b-p6-application-1";
        let envelope = p6_lane_envelope(&binding, "v1b-p6-wire-message-1", group_did, "7");
        seed_cached_p6_plaintext(
            &client,
            &binding,
            group_did,
            "7",
            "cached V1-B P6 plaintext",
        )
        .await;
        db.commit_sync_lane_handoff(SyncLaneHandoffInput {
            owner_identity_id: binding.owner_identity_id.clone(),
            account_id_snapshot: binding.account_id.clone(),
            device_id_snapshot: binding.protocol_device_id.clone(),
            auth_generation_snapshot: binding.device_auth_generation.clone(),
            client_instance_id_snapshot: client_instance_id,
            lane: SyncLaneV3::P6Group,
            lane_epoch: "42".to_owned(),
            position: "1".to_owned(),
            event_id: event_id.to_owned(),
            event_type: "p6.delivery.created".to_owned(),
            raw_payload: envelope.clone(),
            group_did: Some(group_did.to_owned()),
            received_at: "2026-08-28T00:00:00Z".to_owned(),
            source_created_at: None,
            source_expires_at: None,
        })
        .await
        .unwrap();
        let input = SyncLaneInboxRecord {
            input_id: stable_sync_lane_input_id(
                &binding.owner_identity_id,
                SyncLaneV3::P6Group,
                "42",
                event_id,
            )
            .unwrap(),
            owner_identity_id: binding.owner_identity_id.clone(),
            lane: SyncLaneV3::P6Group,
            lane_epoch: "42".to_owned(),
            position: "1".to_owned(),
            event_id: event_id.to_owned(),
            event_type: "p6.delivery.created".to_owned(),
            raw_payload: envelope,
            group_did: Some(group_did.to_owned()),
            received_at: "2026-08-28T00:00:00Z".to_owned(),
            source_created_at: None,
            source_expires_at: None,
        };

        assert_eq!(
            consume_p6_lane_input(&client, &input, 1).await.unwrap(),
            SyncLaneDomainStatus::Applied
        );
        assert_eq!(
            consume_p6_lane_input(&client, &input, 2).await.unwrap(),
            SyncLaneDomainStatus::Applied,
            "a completed replay must return its closed outcome without reapplying the domain effect"
        );
        assert_eq!(
            db.load_lane_sync_states(binding.owner_identity_id.clone())
                .await
                .unwrap()[0]
                .scan_seq,
            "1",
            "domain processing must not rewrite the committed lane cursor"
        );
        let states = db
            .load_sync_lane_domain_states(binding.owner_identity_id)
            .await
            .unwrap();
        assert_eq!(states.len(), 1);
        assert_eq!(states[0].status, SyncLaneDomainStatus::Applied);
    }

    #[test]
    fn v1b_p6_closed_classification_is_bounded_and_actionable() {
        use crate::internal::local_state::sync_v2::{SyncLaneDomainStatus, SyncLaneInboxRecord};
        use crate::internal::wire::sync_v2::SyncLaneV3;
        let input = SyncLaneInboxRecord {
            input_id: "v1b-p6-classification".to_owned(),
            owner_identity_id: "owner".to_owned(),
            lane: SyncLaneV3::P6Group,
            lane_epoch: "1".to_owned(),
            position: "1".to_owned(),
            event_id: "event".to_owned(),
            event_type: "p6.control.notice".to_owned(),
            raw_payload: json!({"body": {"notice_type": "commit"}}),
            group_did: Some("did:example:group".to_owned()),
            received_at: "2026-08-28T00:00:00Z".to_owned(),
            source_created_at: None,
            source_expires_at: None,
        };
        let action = p6_failure_domain_state(
            &input,
            "did:example:group",
            "operation",
            1,
            &crate::ImError::Service {
                status_code: None,
                code: Some("group.e2ee.owner_unavailable".to_owned()),
                message: "owner unavailable".to_owned(),
                data: None,
            },
            1,
        );
        assert_eq!(action.status, SyncLaneDomainStatus::ActionRequired);
        assert!(!action.retryable);
        assert_eq!(action.next_retry_at, None);
        let transient_exhausted = p6_failure_domain_state(
            &input,
            "did:example:group",
            "operation",
            3,
            &crate::ImError::TransportUnavailable {
                detail: "offline".to_owned(),
            },
            1,
        );
        assert_eq!(
            transient_exhausted.status,
            SyncLaneDomainStatus::RepairRequired
        );
        assert!(!transient_exhausted.retryable);
    }

    #[cfg(feature = "secure-direct")]
    #[tokio::test]
    async fn upgraded_client_negotiates_lane_capabilities_before_first_delta() {
        use crate::internal::wire::sync_v2::SyncLaneV3;

        let fixture = SyncSnapshotFixture::new("lane-capability-upgrade");
        let client = fixture.client();
        let binding = client.active_sync_account_binding().await.unwrap();
        let client_instance_id = client
            .core_inner()
            .local_state_db()
            .await
            .unwrap()
            .load_or_create_sync_client_instance_id(&binding.owner_identity_id)
            .await
            .unwrap();
        seed_legacy_sync_snapshot_ready_state(&client, &binding, "1", "10").await;
        let mut bootstrap = json!({
            "mode": "tail_only",
            "account_id": binding.account_id,
            "device_id": binding.protocol_device_id,
            "server_time": "2026-08-15T00:00:00Z",
            "cursor": {"stream_epoch": "1", "scan_seq": "10"},
            "read_state_baseline": [],
            "group_state_baseline": [],
            "warnings": [],
        });
        add_enabled_lane_bootstrap(&mut bootstrap, &client_instance_id, "41", "42");
        let delta = sync_snapshot_delta_with_lanes(
            "1",
            "10",
            json!({
                "p5_device": {
                    "events": [],
                    "next_cursor": {"stream_epoch": "41", "scan_seq": "0"},
                    "has_more": false
                },
                "p6_group": {
                    "events": [],
                    "next_cursor": {"stream_epoch": "42", "scan_seq": "0"},
                    "has_more": false
                }
            }),
        );
        let calls = Rc::new(RefCell::new(Vec::new()));

        let outcome = MessageSyncRuntimeV2::new(
            &client,
            ReadySyncSnapshotSessionProvider,
            SyncSnapshotTransport::queued(
                Rc::clone(&calls),
                vec![
                    Ok(explicit_sync_negotiation_response()),
                    Ok(bootstrap),
                    Ok(delta),
                ],
            ),
            NoopAsyncDirectoryTransport,
        )
        .sync_now(sync_snapshot_request())
        .await
        .unwrap();

        assert_eq!(outcome.status, crate::messages::MessageSyncStatus::Idle);
        {
            let calls = calls.borrow();
            assert_eq!(
                calls
                    .iter()
                    .map(|call| call.method.as_str())
                    .collect::<Vec<_>>(),
                ["anp.get_capabilities", "sync.bootstrap", "sync.delta"]
            );
            assert_eq!(
                calls[2]
                    .params
                    .pointer("/body/lanes/p5_device/cursor/stream_epoch"),
                Some(&json!("41"))
            );
        }
        let db = client.core_inner().local_state_db().await.unwrap();
        assert!(!db
            .lane_capability_negotiation_required(
                binding.owner_identity_id.clone(),
                binding.device_auth_generation,
            )
            .await
            .unwrap());
        assert_eq!(
            db.load_lane_sync_states(binding.owner_identity_id)
                .await
                .unwrap()
                .into_iter()
                .map(|state| state.lane)
                .collect::<Vec<_>>(),
            [SyncLaneV3::P5Device, SyncLaneV3::P6Group]
        );

        let second_calls = Rc::new(RefCell::new(Vec::new()));
        MessageSyncRuntimeV2::new(
            &client,
            ReadySyncSnapshotSessionProvider,
            SyncSnapshotTransport::queued(
                Rc::clone(&second_calls),
                vec![Ok(sync_snapshot_delta_with_lanes(
                    "1",
                    "10",
                    json!({
                        "p5_device": {
                            "events": [],
                            "next_cursor": {"stream_epoch": "41", "scan_seq": "0"},
                            "has_more": false
                        }
                    }),
                ))],
            ),
            NoopAsyncDirectoryTransport,
        )
        .sync_now(sync_snapshot_request())
        .await
        .unwrap();
        assert_eq!(
            second_calls
                .borrow()
                .iter()
                .map(|call| call.method.as_str())
                .collect::<Vec<_>>(),
            ["sync.delta"]
        );
    }

    #[cfg(all(feature = "secure-direct", feature = "group-e2ee"))]
    #[tokio::test]
    async fn domain_poison_p5_commits_handoff_without_blocking_ordinary_or_p6() {
        use crate::internal::wire::sync_v2::SyncLaneV3;

        let fixture = SyncSnapshotFixture::new("p5-poison-isolated");
        let client = fixture.client();
        let binding = client.active_sync_account_binding().await.unwrap();
        seed_sync_snapshot_ready_state(&client, &binding, "1", "10").await;
        seed_lane_states(
            &client,
            &binding,
            &[(SyncLaneV3::P5Device, "41"), (SyncLaneV3::P6Group, "42")],
        )
        .await;
        let p6_group_did = "did:wba:awiki.test:groups:p6-independent";
        let p6_envelope = p6_lane_envelope(&binding, "p6-independent-message-1", p6_group_did, "1");
        let calls = Rc::new(RefCell::new(Vec::new()));
        let response = sync_snapshot_delta_with_lanes(
            "1",
            "11",
            json!({
                "p5_device": {
                    "events": [poison_p5_lane_event("p5-poison-1", "1")],
                    "next_cursor": {"stream_epoch": "41", "scan_seq": "1"},
                    "has_more": false
                },
                "p6_group": {
                    "events": [p6_lane_event(
                        "p6-independent-1",
                        "1",
                        p6_group_did,
                        "1",
                        &p6_envelope
                    )],
                    "next_cursor": {"stream_epoch": "42", "scan_seq": "1"},
                    "has_more": false
                }
            }),
        );
        let outcome = MessageSyncRuntimeV2::new(
            &client,
            ReadySyncSnapshotSessionProvider,
            SyncSnapshotTransport::queued(Rc::clone(&calls), vec![Ok(response.clone())]),
            NoopAsyncDirectoryTransport,
        )
        .sync_now(sync_snapshot_request())
        .await
        .unwrap();

        assert_ne!(
            outcome.status,
            crate::messages::MessageSyncStatus::AuthRevoked
        );
        assert_eq!(
            load_sync_snapshot_state(&client, &binding.owner_identity_id)
                .await
                .scan_seq,
            "11"
        );
        let lanes = client
            .core_inner()
            .local_state_db()
            .await
            .unwrap()
            .load_lane_sync_states(binding.owner_identity_id.clone())
            .await
            .unwrap()
            .into_iter()
            .map(|state| (state.lane, state.scan_seq))
            .collect::<BTreeMap<_, _>>();
        assert_eq!(lanes[&SyncLaneV3::P5Device], "1");
        assert_eq!(lanes[&SyncLaneV3::P6Group], "1");
        drain_pending_secure_lane_consumers(&client, 64)
            .await
            .unwrap();
        let p5_domain = client
            .core_inner()
            .local_state_db()
            .await
            .unwrap()
            .load_sync_lane_domain_states(binding.owner_identity_id.clone())
            .await
            .unwrap()
            .into_iter()
            .find(|state| state.operation_ref.as_deref() == Some("p5-poison-1"))
            .unwrap();
        assert_eq!(
            p5_domain.status,
            crate::internal::local_state::sync_v2::SyncLaneDomainStatus::Terminal
        );
        assert!(!p5_domain.retryable);
        assert_eq!(
            p5_domain.last_error_code.as_deref(),
            Some("p5.malformed_input")
        );
        {
            let calls = calls.borrow();
            let request = &calls[0].params;
            assert!(request.pointer("/body/lanes/p5_device").is_some());
            assert!(request.pointer("/body/lanes/p6_group").is_some());
        }

        let mut retry_response = response;
        retry_response["lanes"]["p5_device"]["events"] = json!([]);
        retry_response["lanes"]["p6_group"]["events"] = json!([]);
        let retry = MessageSyncRuntimeV2::new(
            &client,
            ReadySyncSnapshotSessionProvider,
            SyncSnapshotTransport::queued(Rc::clone(&calls), vec![Ok(retry_response)]),
            NoopAsyncDirectoryTransport,
        )
        .sync_now(sync_snapshot_request())
        .await
        .unwrap();
        assert_ne!(
            retry.status,
            crate::messages::MessageSyncStatus::AuthRevoked
        );
        let calls = calls.borrow();
        assert_eq!(
            calls[1].params["body"]["lanes"]["p5_device"]["cursor"]["scan_seq"],
            "1"
        );
        assert_eq!(
            calls[1].params["body"]["lanes"]["p6_group"]["cursor"]["scan_seq"],
            "1"
        );
    }

    #[tokio::test]
    async fn v1b_cross_stream_inline_e2ee_is_only_a_pull_hint() {
        use crate::internal::wire::sync_v2::SyncLaneV3;
        let fixture = SyncSnapshotFixture::new("v1b-inline-pull-hint");
        let client = fixture.client();
        let binding = client.active_sync_account_binding().await.unwrap();
        seed_sync_snapshot_ready_state(&client, &binding, "1", "0").await;
        seed_lane_states(&client, &binding, &[(SyncLaneV3::P5Device, "41")]).await;
        let envelope = json!({
            "meta": {
                "profile": "anp.direct.e2ee.v2",
                "security_profile": "direct-e2ee",
                "message_id": "v1b-inline-hint"
            },
            "body": {"ciphertext_b64u": "AQ"}
        });
        let notification = realtime_inline_p5_notification("v1b-inline-hint", "41", "1", &envelope);
        let inline =
            crate::internal::realtime::notification::parse_inline_sync_event_v3(&notification)
                .unwrap()
                .unwrap();
        assert_eq!(
            apply_realtime_e2ee_lane_v3_with_directory_async(
                &client,
                inline,
                &mut NoopAsyncDirectoryTransport,
            )
            .await
            .unwrap(),
            RealtimeInlineMessageApplyOutcome::Deferred
        );
        let connection = rusqlite::Connection::open(fixture.sqlite_path()).unwrap();
        assert_eq!(
            connection
                .query_row("SELECT COUNT(*) FROM sync_lane_inbox", [], |row| {
                    row.get::<_, i64>(0)
                })
                .unwrap(),
            0
        );
    }

    #[tokio::test]
    async fn realtime_inline_v3_reuses_sync_reducer_and_commits_without_reliable_receipt() {
        let fixture = SyncSnapshotFixture::new("realtime-inline-v3");
        let client = fixture.client();
        let binding = client.active_sync_account_binding().await.unwrap();
        seed_sync_snapshot_ready_state(&client, &binding, "1", "10").await;
        let group_did = "did:wba:awiki.test:group:e1_realtime";
        client
            .core_inner()
            .local_state_db()
            .await
            .unwrap()
            .upsert_group(crate::internal::local_state::groups::GroupRecord {
                owner_identity_id: binding.owner_identity_id.clone(),
                owner_did: binding.current_did.clone(),
                group_id: group_did.to_owned(),
                group_did: group_did.to_owned(),
                membership_status: "active".to_owned(),
                stored_at: "2026-08-15T00:00:00Z".to_owned(),
                credential_name: binding.owner_identity_id.clone(),
                ..Default::default()
            })
            .await
            .unwrap();
        let notification = realtime_inline_group_notification(
            &binding,
            "sev2g_realtime_11",
            "11",
            "msg_realtime_11",
            group_did,
            "zero RTT",
        );

        let outcome = apply_realtime_inline_message_v3_async(&client, &notification)
            .await
            .unwrap();
        assert!(matches!(
            outcome,
            RealtimeInlineMessageApplyOutcome::Applied {
                message,
                local_scan_seq
            } if message.id.as_str() == "msg_realtime_11"
                && matches!(
                    message.body,
                    crate::messages::MessageBodyView::Text {
                        ref text,
                        kind: crate::messages::MessageKind::Text,
                    } if text == "zero RTT"
                )
                && local_scan_seq.as_deref() == Some("10")
        ));
        let db = client.core_inner().local_state_db().await.unwrap();
        let state = db
            .load_message_sync_state(binding.owner_identity_id.clone())
            .await
            .unwrap();
        assert!(matches!(
            state,
            crate::internal::local_state::sync_v2::MessageSyncStateAccess::Ready(state)
                if state.scan_seq == "10"
        ));
        db.shutdown().await.unwrap();
        let connection = rusqlite::Connection::open(fixture.sqlite_path()).unwrap();
        assert_eq!(
            connection
                .query_row("SELECT COUNT(*) FROM sync_applied_events", [], |row| row
                    .get::<_, i64>(0),)
                .unwrap(),
            0
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT content FROM messages
                     WHERE json_extract(metadata, '$.sync_event_id') = 'sev2g_realtime_11'",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .unwrap(),
            "zero RTT"
        );
    }

    async fn seed_sync_snapshot_message(
        client: &crate::core::ImClient,
        binding: &crate::identity::ActiveSyncAccountBinding,
        message_id: &str,
        group_did: &str,
        content: &str,
    ) {
        let conversation_id =
            crate::internal::local_state::owner_scope::group_conversation_id(group_did);
        client
            .core_inner()
            .local_state_db()
            .await
            .unwrap()
            .store_messages(vec![
                crate::internal::local_state::messages::MessageRecord {
                    msg_id: message_id.to_owned(),
                    owner_identity_id: binding.owner_identity_id.clone(),
                    owner_did: binding.current_did.clone(),
                    conversation_id: conversation_id.clone(),
                    wire_thread_kind: "group".to_owned(),
                    wire_thread_ref: group_did.to_owned(),
                    wire_identity_resolution_state: "resolved".to_owned(),
                    thread_id: conversation_id,
                    direction: 0,
                    sender_did: "did:example:bob".to_owned(),
                    receiver_did: binding.current_did.clone(),
                    group_id: group_did.to_owned(),
                    group_did: group_did.to_owned(),
                    content_type: "text/plain".to_owned(),
                    content: content.to_owned(),
                    server_seq: Some(1),
                    sent_at: "2026-07-20T00:00:00Z".to_owned(),
                    stored_at: "2026-07-20T00:00:00Z".to_owned(),
                    ..Default::default()
                },
            ])
            .await
            .unwrap();
    }

    async fn seed_sync_read_direct_message(
        client: &crate::core::ImClient,
        binding: &crate::identity::ActiveSyncAccountBinding,
        message_id: &str,
        conversation_id: &str,
        server_seq: i64,
    ) {
        client
            .core_inner()
            .local_state_db()
            .await
            .unwrap()
            .store_messages(vec![
                crate::internal::local_state::messages::MessageRecord {
                    msg_id: message_id.to_owned(),
                    owner_identity_id: binding.owner_identity_id.clone(),
                    owner_did: binding.current_did.clone(),
                    conversation_id: conversation_id.to_owned(),
                    wire_thread_kind: "direct".to_owned(),
                    wire_thread_ref: "did:example:bob".to_owned(),
                    wire_identity_resolution_state: "resolved".to_owned(),
                    thread_id: conversation_id.to_owned(),
                    direction: 0,
                    sender_did: "did:example:bob".to_owned(),
                    receiver_did: binding.current_did.clone(),
                    content_type: "text/plain".to_owned(),
                    content: format!("direct message {server_seq}"),
                    server_seq: Some(server_seq),
                    sent_at: "2026-07-28T12:00:00Z".to_owned(),
                    stored_at: "2026-07-28T12:00:00Z".to_owned(),
                    ..Default::default()
                },
            ])
            .await
            .unwrap();
    }

    async fn seed_sync_read_group_message(
        client: &crate::core::ImClient,
        binding: &crate::identity::ActiveSyncAccountBinding,
        local_message_id: &str,
        raw_message_id: Option<&str>,
        group_did: &str,
        server_seq: i64,
    ) {
        let conversation_id =
            crate::internal::local_state::owner_scope::group_conversation_id(group_did);
        let metadata = raw_message_id
            .map(|message_id| json!({"raw_message_id": message_id}))
            .unwrap_or_else(|| json!({}));
        client
            .core_inner()
            .local_state_db()
            .await
            .unwrap()
            .store_messages(vec![
                crate::internal::local_state::messages::MessageRecord {
                    msg_id: local_message_id.to_owned(),
                    owner_identity_id: binding.owner_identity_id.clone(),
                    owner_did: binding.current_did.clone(),
                    conversation_id: conversation_id.clone(),
                    wire_thread_kind: "group".to_owned(),
                    wire_thread_ref: group_did.to_owned(),
                    wire_identity_resolution_state: "resolved".to_owned(),
                    thread_id: conversation_id,
                    direction: 0,
                    sender_did: "did:example:bob".to_owned(),
                    receiver_did: binding.current_did.clone(),
                    group_id: group_did.to_owned(),
                    group_did: group_did.to_owned(),
                    content_type: "text/plain".to_owned(),
                    content: format!("group message {server_seq}"),
                    server_seq: Some(server_seq),
                    sent_at: "2026-07-28T12:00:00Z".to_owned(),
                    stored_at: "2026-07-28T12:00:00Z".to_owned(),
                    metadata: metadata.to_string(),
                    ..Default::default()
                },
            ])
            .await
            .unwrap();
    }

    async fn apply_sync_read_thread_binding(
        client: &crate::core::ImClient,
        binding: &crate::identity::ActiveSyncAccountBinding,
        event_id: &str,
        event_seq: &str,
        remote_thread_key: &str,
        conversation_id: &str,
    ) {
        client
            .core_inner()
            .local_state_db()
            .await
            .unwrap()
            .apply_sync_delta_v2(crate::internal::local_state::sync_v2::DeltaApplyInputV2 {
                owner_identity_id: binding.owner_identity_id.clone(),
                expected_run_generation: None,
                owner_did: binding.current_did.clone(),
                account_id: binding.account_id.clone(),
                protocol_device_id: binding.protocol_device_id.clone(),
                device_auth_generation: binding.device_auth_generation.clone(),
                stream_epoch: "1".to_owned(),
                next_scan_seq: event_seq.to_owned(),
                server_time: "2026-07-28T12:00:01Z".to_owned(),
                events: vec![crate::internal::local_state::sync_v2::DeltaApplyEventV2 {
                    event_id: event_id.to_owned(),
                    event_seq: event_seq.to_owned(),
                    event_type: "message.created".to_owned(),
                    thread_bindings: vec![
                        crate::internal::local_state::sync_v2::SyncThreadBinding {
                            owner_identity_id: binding.owner_identity_id.clone(),
                            remote_thread_key: remote_thread_key.to_owned(),
                            thread_kind: "direct".to_owned(),
                            conversation_id: conversation_id.to_owned(),
                            updated_at: 1,
                        },
                    ],
                    ..Default::default()
                }],
            })
            .await
            .unwrap();
    }

    async fn load_sync_snapshot_state(
        client: &crate::core::ImClient,
        owner_identity_id: &str,
    ) -> crate::internal::local_state::sync_v2::MessageSyncState {
        let crate::internal::local_state::sync_v2::MessageSyncStateAccess::Ready(state) = client
            .core_inner()
            .local_state_db()
            .await
            .unwrap()
            .load_message_sync_state(owner_identity_id.to_owned())
            .await
            .unwrap()
        else {
            panic!("expected ready message sync state");
        };
        state
    }

    #[tokio::test]
    async fn sync_v2_group_state_events_commit_system_timeline_messages() {
        let fixture = SyncSnapshotFixture::new("group-system-events");
        let client = fixture.client();
        let binding = client.active_sync_account_binding().await.unwrap();
        seed_sync_snapshot_ready_state(&client, &binding, "1", "0").await;
        let group_did = "did:wba:awiki.test:groups:sync-system-events";
        let actor_did = "did:wba:awiki.test:user:bob:e1_actor";
        let subject_did = "did:wba:awiki.test:user:carol:e1_subject";
        let member_event_id = "event-group-member-added";
        let profile_event_id = "event-group-profile-updated";
        let transport = SyncSnapshotTransport::queued(
            Rc::new(RefCell::new(Vec::new())),
            vec![Ok(sync_snapshot_delta(
                "1",
                "2",
                vec![
                    sync_group_member_changed_event(
                        &binding,
                        member_event_id,
                        "1",
                        group_did,
                        "7",
                        "11",
                        actor_did,
                        subject_did,
                        "active",
                    ),
                    sync_group_profile_updated_event(
                        &binding,
                        profile_event_id,
                        "2",
                        group_did,
                        "8",
                        "12",
                        actor_did,
                    ),
                ],
            ))],
        );

        let outcome = MessageSyncRuntimeV2::new(
            &client,
            ReadySyncSnapshotSessionProvider,
            transport,
            NoopAsyncDirectoryTransport,
        )
        .sync_now(sync_snapshot_request())
        .await
        .unwrap();

        assert_eq!(outcome.events_applied, 2);
        assert!(outcome.committed_incoming_messages.is_empty());
        assert_eq!(
            outcome.changed_conversation_ids,
            vec![crate::internal::local_state::owner_scope::group_conversation_id(group_did)]
        );
        let connection = rusqlite::Connection::open(fixture.sqlite_path()).unwrap();
        let mut statement = connection
            .prepare(
                "SELECT msg_id, content_type, content, server_seq, is_read, metadata
                 FROM messages
                 WHERE owner_identity_id = ?1 AND group_did = ?2
                 ORDER BY server_seq ASC",
            )
            .unwrap();
        let records = statement
            .query_map([binding.owner_identity_id.as_str(), group_did], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, bool>(4)?,
                    row.get::<_, String>(5)?,
                ))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(records.len(), 2);

        let member_payload = serde_json::from_str::<Value>(&records[0].2).unwrap();
        assert_eq!(records[0].0, format!("{group_did}:11"));
        assert_eq!(records[0].1, "application/json");
        assert_eq!(records[0].3, 11);
        assert!(records[0].4);
        assert_eq!(member_payload["schema"], "awiki.group.system_event.v1");
        assert_eq!(member_payload["type"], "member_added");
        assert_eq!(member_payload["actor_did"], actor_did);
        assert_eq!(member_payload["subject_did"], subject_did);
        assert_eq!(member_payload["sync_event_id"], member_event_id);
        assert_eq!(member_payload["sync_event_type"], "group.member_changed");
        assert_eq!(
            serde_json::from_str::<Value>(&records[0].5).unwrap()["message_role"],
            "group_system_event"
        );

        let profile_payload = serde_json::from_str::<Value>(&records[1].2).unwrap();
        assert_eq!(records[1].0, format!("{group_did}:12"));
        assert_eq!(records[1].3, 12);
        assert!(records[1].4);
        assert_eq!(profile_payload["schema"], "awiki.group.system_event.v1");
        assert_eq!(profile_payload["type"], "group_profile_updated");
        assert_eq!(profile_payload["actor_did"], actor_did);
        assert_eq!(profile_payload["sync_event_id"], profile_event_id);
        assert_eq!(profile_payload["sync_event_type"], "group.profile_updated");
    }

    #[tokio::test]
    async fn sync_v2_group_state_event_converges_with_realtime_projection_exactly_once() {
        let fixture = SyncSnapshotFixture::new("group-system-event-realtime-race");
        let client = fixture.client();
        let binding = client.active_sync_account_binding().await.unwrap();
        seed_sync_snapshot_ready_state(&client, &binding, "1", "0").await;
        let group_did = "did:wba:awiki.test:groups:realtime-race";
        let actor_did = "did:wba:awiki.test:user:bob:e1_actor";
        let subject_did = "did:wba:awiki.test:user:carol:e1_subject";
        let event_id = "event-group-member-realtime-race";
        let realtime_record = crate::internal::group_system_events::record_from_input(
            &client,
            crate::internal::group_system_events::GroupSystemEventInput {
                event_type: "member_added".to_owned(),
                group_did: group_did.to_owned(),
                group_event_seq: 21,
                group_state_version: Some("9".to_owned()),
                actor_did: Some(actor_did.to_owned()),
                subject_did: Some(subject_did.to_owned()),
                subject_handle: None,
                previous_subject_did: None,
                handle_binding_generation: None,
                membership_status: Some("active".to_owned()),
                changed_at: Some("2026-07-28T12:00:01Z".to_owned()),
                sync_event_id: Some(event_id.to_owned()),
                sync_event_seq: Some("1".to_owned()),
                sync_event_type: Some("group.member_changed".to_owned()),
                source: "im-core.realtime".to_owned(),
            },
        )
        .unwrap();
        client
            .core_inner()
            .local_state_db()
            .await
            .unwrap()
            .store_messages(vec![realtime_record])
            .await
            .unwrap();
        let transport = SyncSnapshotTransport::queued(
            Rc::new(RefCell::new(Vec::new())),
            vec![Ok(sync_snapshot_delta(
                "1",
                "1",
                vec![sync_group_member_changed_event(
                    &binding,
                    event_id,
                    "1",
                    group_did,
                    "9",
                    "21",
                    actor_did,
                    subject_did,
                    "active",
                )],
            ))],
        );

        let outcome = MessageSyncRuntimeV2::new(
            &client,
            ReadySyncSnapshotSessionProvider,
            transport,
            NoopAsyncDirectoryTransport,
        )
        .sync_now(sync_snapshot_request())
        .await
        .unwrap();

        assert_eq!(outcome.events_applied, 1);
        assert!(outcome.committed_incoming_messages.is_empty());
        let connection = rusqlite::Connection::open(fixture.sqlite_path()).unwrap();
        let (count, message_id, server_seq, is_read) = connection
            .query_row(
                "SELECT COUNT(*), MIN(msg_id), MIN(server_seq), MIN(is_read)
                 FROM messages
                 WHERE owner_identity_id = ?1 AND group_did = ?2",
                [binding.owner_identity_id.as_str(), group_did],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, bool>(3)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(count, 1);
        assert_eq!(message_id, format!("{group_did}:21"));
        assert_eq!(server_seq, 21);
        assert!(is_read);
    }

    #[tokio::test]
    async fn first_direct_message_resolves_authoritative_persona_before_v2_commit() {
        let fixture = SyncSnapshotFixture::new("first-direct-persona");
        let client = fixture.client();
        let binding = client.active_sync_account_binding().await.unwrap();
        seed_sync_snapshot_ready_state(&client, &binding, "1", "0").await;
        let peer_did = "did:wba:awiki.info:user:peer:e1_peer";
        let event_id = "event-first-direct";
        let message_id = "message-first-direct";
        let remote_thread_key = "remote-thread-first-direct";
        let message_calls = Rc::new(RefCell::new(Vec::new()));
        let message_transport = SyncSnapshotTransport::queued(
            Rc::clone(&message_calls),
            vec![
                Ok(sync_snapshot_delta(
                    "1",
                    "1",
                    vec![sync_direct_message_event(
                        &binding,
                        event_id,
                        "1",
                        message_id,
                        peer_did,
                        remote_thread_key,
                    )],
                )),
                Ok(json!({
                    "items": [{
                        "event_id": event_id,
                        "message": sync_direct_message(
                            &binding,
                            message_id,
                            peer_did,
                            "first direct body",
                        )
                    }],
                    "unavailable": []
                })),
            ],
        );
        let directory_calls = Rc::new(RefCell::new(0_u32));

        let outcome = MessageSyncRuntimeV2::new(
            &client,
            ReadySyncSnapshotSessionProvider,
            message_transport,
            DirectLookupTransport {
                expected_did: peer_did.to_owned(),
                calls: Rc::clone(&directory_calls),
            },
        )
        .sync_now(sync_snapshot_request())
        .await
        .unwrap();

        assert_eq!(*directory_calls.borrow(), 1);
        assert_eq!(outcome.committed_incoming_messages.len(), 1);
        assert_eq!(outcome.committed_incoming_messages[0].event_id, event_id);
        assert!(!outcome
            .warnings
            .iter()
            .any(|warning| warning.starts_with("identity_unresolved_backlog:")));
        let db = rusqlite::Connection::open(fixture.sqlite_path()).unwrap();
        let projection: (String, String) = db
            .query_row(
                "SELECT m.conversation_id, b.conversation_id
                 FROM messages AS m
                 JOIN sync_thread_bindings AS b
                   ON b.owner_identity_id = m.owner_identity_id
                  AND b.remote_thread_key = ?1
                 WHERE m.owner_identity_id = ?2 AND m.msg_id = ?3",
                (
                    remote_thread_key,
                    binding.owner_identity_id.as_str(),
                    message_id,
                ),
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        let expected_conversation_id =
            crate::internal::canonical_identity::PeerPersona::from_verified_handle(
                "awiki.info",
                "user-peer",
                "peer.awiki.info",
                Some("active"),
            )
            .unwrap()
            .direct_conversation_id();
        assert_eq!(
            projection,
            (
                expected_conversation_id.clone(),
                expected_conversation_id.clone(),
            )
        );
        assert!(outcome
            .changed_conversation_ids
            .contains(&expected_conversation_id));
        assert_eq!(
            crate::internal::local_state::inbound_resolution_backlog::pending_count(
                &db,
                &binding.owner_identity_id,
            )
            .unwrap(),
            0
        );
    }

    #[tokio::test]
    async fn recovery_snapshot_resolves_authoritative_persona_before_v2_commit() {
        let fixture = SyncSnapshotFixture::new("snapshot-direct-persona");
        let client = fixture.client();
        let binding = client.active_sync_account_binding().await.unwrap();
        seed_sync_snapshot_ready_state(&client, &binding, "1", "10").await;
        let peer_did = "did:wba:awiki.info:user:snapshot-peer:e1_peer";
        let event_id = "event-snapshot-direct";
        let message_id = "message-snapshot-direct";
        let remote_thread_key = "remote-thread-snapshot-direct";
        let directory_calls = Rc::new(RefCell::new(0_u32));
        let mut snapshot_event = sync_direct_message_event(
            &binding,
            event_id,
            "19",
            message_id,
            peer_did,
            remote_thread_key,
        );
        snapshot_event["stream_epoch"] = json!("2");
        let transport = SyncSnapshotTransport::queued(
            Rc::new(RefCell::new(Vec::new())),
            vec![
                Ok(sync_snapshot_recovery(
                    "recovery-direct",
                    "snapshot-token-direct",
                    "2",
                    "20",
                )),
                Ok(sync_snapshot_response(
                    &binding,
                    "2",
                    "20",
                    vec![json!({
                        "event": snapshot_event,
                        "message": sync_direct_message(
                            &binding,
                            message_id,
                            peer_did,
                            "snapshot direct body",
                        )
                    })],
                )),
                Ok(sync_snapshot_delta("2", "20", vec![])),
            ],
        );

        let outcome = MessageSyncRuntimeV2::new(
            &client,
            ReadySyncSnapshotSessionProvider,
            transport,
            DirectLookupTransport {
                expected_did: peer_did.to_owned(),
                calls: Rc::clone(&directory_calls),
            },
        )
        .sync_now(sync_snapshot_request())
        .await
        .unwrap();

        assert_eq!(*directory_calls.borrow(), 1);
        assert!(fixture.has_message_content("snapshot direct body"));
        assert!(!outcome
            .warnings
            .iter()
            .any(|warning| warning.starts_with("identity_unresolved_backlog:")));
        let expected_conversation_id =
            crate::internal::canonical_identity::PeerPersona::from_verified_handle(
                "awiki.info",
                "user-peer",
                "peer.awiki.info",
                Some("active"),
            )
            .unwrap()
            .direct_conversation_id();
        let db = rusqlite::Connection::open(fixture.sqlite_path()).unwrap();
        assert_eq!(
            db.query_row(
                "SELECT m.conversation_id
                 FROM messages AS m
                 WHERE m.owner_identity_id = ?1 AND m.msg_id = ?2",
                (binding.owner_identity_id.as_str(), message_id),
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
            expected_conversation_id
        );
        assert_eq!(
            db.query_row(
                "SELECT conversation_id FROM sync_thread_bindings
                 WHERE owner_identity_id = ?1 AND remote_thread_key = ?2",
                (binding.owner_identity_id.as_str(), remote_thread_key),
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
            expected_conversation_id
        );
    }

    #[tokio::test]
    async fn deferred_direct_persona_resolution_replays_on_next_v2_sync() {
        let fixture = SyncSnapshotFixture::new("deferred-direct-persona");
        let client = fixture.client();
        let binding = client.active_sync_account_binding().await.unwrap();
        seed_sync_snapshot_ready_state(&client, &binding, "1", "0").await;
        let peer_did = "did:wba:awiki.info:user:deferred:e1_peer";
        let event_id = "event-deferred-direct";
        let message_id = "message-deferred-direct";
        let remote_thread_key = "remote-thread-deferred-direct";
        let first = MessageSyncRuntimeV2::new(
            &client,
            ReadySyncSnapshotSessionProvider,
            SyncSnapshotTransport::queued(
                Rc::new(RefCell::new(Vec::new())),
                vec![
                    Ok(sync_snapshot_delta(
                        "1",
                        "1",
                        vec![sync_direct_message_event(
                            &binding,
                            event_id,
                            "1",
                            message_id,
                            peer_did,
                            remote_thread_key,
                        )],
                    )),
                    Ok(json!({
                        "items": [{
                            "event_id": event_id,
                            "message": sync_direct_message(
                                &binding,
                                message_id,
                                peer_did,
                                "deferred direct body",
                            )
                        }],
                        "unavailable": []
                    })),
                ],
            ),
            FailingDirectoryTransport,
        )
        .sync_now(sync_snapshot_request())
        .await
        .unwrap();
        assert!(first
            .warnings
            .iter()
            .any(|warning| warning == "identity_unresolved_backlog:1"));
        assert!(!fixture.has_message_content("deferred direct body"));

        let directory_calls = Rc::new(RefCell::new(0_u32));
        let second = MessageSyncRuntimeV2::new(
            &client,
            ReadySyncSnapshotSessionProvider,
            SyncSnapshotTransport::queued(
                Rc::new(RefCell::new(Vec::new())),
                vec![Ok(sync_snapshot_delta("1", "1", vec![]))],
            ),
            DirectLookupTransport {
                expected_did: peer_did.to_owned(),
                calls: Rc::clone(&directory_calls),
            },
        )
        .sync_now(sync_snapshot_request())
        .await
        .unwrap();

        assert_eq!(*directory_calls.borrow(), 1);
        assert!(fixture.has_message_content("deferred direct body"));
        let expected_conversation_id =
            crate::internal::canonical_identity::PeerPersona::from_verified_handle(
                "awiki.info",
                "user-peer",
                "peer.awiki.info",
                Some("active"),
            )
            .unwrap()
            .direct_conversation_id();
        assert!(second
            .changed_conversation_ids
            .contains(&expected_conversation_id));
        let db = rusqlite::Connection::open(fixture.sqlite_path()).unwrap();
        assert_eq!(
            db.query_row(
                "SELECT conversation_id FROM sync_thread_bindings
                 WHERE owner_identity_id = ?1 AND remote_thread_key = ?2",
                (binding.owner_identity_id.as_str(), remote_thread_key),
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
            expected_conversation_id
        );
        assert_eq!(
            crate::internal::local_state::inbound_resolution_backlog::pending_count(
                &db,
                &binding.owner_identity_id,
            )
            .unwrap(),
            0
        );
    }

    #[tokio::test]
    async fn sync_snapshot_recovery_preserves_old_messages_and_only_commits_post_anchor_delta() {
        let fixture = SyncSnapshotFixture::new("preserve-and-post-anchor");
        let client = fixture.client();
        let binding = client.active_sync_account_binding().await.unwrap();
        seed_sync_snapshot_ready_state(&client, &binding, "1", "10").await;
        seed_sync_snapshot_message(
            &client,
            &binding,
            "message-before-recovery",
            "did:example:sync-snapshot-old",
            "must survive compact recovery",
        )
        .await;

        let snapshot_event = sync_snapshot_message_event(
            &binding,
            "event-snapshot-19",
            "2",
            "19",
            "message-from-snapshot",
            "did:example:sync-snapshot-recovered",
        );
        let snapshot_message = sync_snapshot_message(
            &binding,
            "message-from-snapshot",
            "did:example:sync-snapshot-recovered",
            "19",
            "snapshot ordinary message",
        );
        let live_event = sync_snapshot_message_event(
            &binding,
            "event-live-21",
            "2",
            "21",
            "message-after-anchor",
            "did:example:sync-snapshot-live",
        );
        let live_message = sync_snapshot_message(
            &binding,
            "message-after-anchor",
            "did:example:sync-snapshot-live",
            "21",
            "post-anchor ordinary message",
        );
        let calls = Rc::new(RefCell::new(Vec::new()));
        let transport = SyncSnapshotTransport::queued(
            Rc::clone(&calls),
            vec![
                Ok(sync_snapshot_recovery(
                    "recovery-preserve",
                    "snapshot-token-preserve",
                    "2",
                    "20",
                )),
                Ok(sync_snapshot_response(
                    &binding,
                    "2",
                    "20",
                    vec![json!({
                        "event": snapshot_event,
                        "message": snapshot_message
                    })],
                )),
                Ok(sync_snapshot_delta("2", "22", vec![live_event])),
                Ok(json!({
                    "items": [{
                        "event_id": "event-live-21",
                        "message": live_message
                    }],
                    "unavailable": []
                })),
            ],
        );

        let outcome = MessageSyncRuntimeV2::new(
            &client,
            ReadySyncSnapshotSessionProvider,
            transport,
            NoopAsyncDirectoryTransport,
        )
        .sync_now(sync_snapshot_request())
        .await
        .unwrap();

        assert!(fixture.has_message_content("must survive compact recovery"));
        assert!(fixture.has_message_content("snapshot ordinary message"));
        assert!(fixture.has_message_content("post-anchor ordinary message"));
        let state = load_sync_snapshot_state(&client, &binding.owner_identity_id).await;
        assert_eq!(
            (state.stream_epoch.as_str(), state.scan_seq.as_str()),
            ("2", "22")
        );
        assert_eq!(outcome.committed_incoming_messages.len(), 1);
        assert_eq!(
            outcome.committed_incoming_messages[0].event_id,
            "event-live-21"
        );
        assert_eq!(
            outcome.committed_incoming_messages[0].logical_message_id,
            "message-after-anchor"
        );
        assert_eq!(outcome.committed_incoming_messages[0].source, "live_delta");
        let calls = calls.borrow();
        assert_eq!(
            calls
                .iter()
                .map(|call| call.method.as_str())
                .collect::<Vec<_>>(),
            [
                "sync.delta",
                "sync.snapshot",
                "sync.delta",
                "message.get_batch"
            ]
        );
        assert_eq!(
            calls[0].params.pointer("/body/cursor/scan_seq"),
            Some(&json!("10"))
        );
        assert_eq!(
            calls[2].params.pointer("/body/cursor/scan_seq"),
            Some(&json!("20"))
        );
    }

    #[tokio::test]
    async fn sync_snapshot_parse_failure_preserves_cursor_projection_and_emits_no_patch() {
        let fixture = SyncSnapshotFixture::new("parse-failure");
        let client = fixture.client();
        let binding = client.active_sync_account_binding().await.unwrap();
        seed_sync_snapshot_ready_state(&client, &binding, "1", "10").await;
        seed_sync_snapshot_message(
            &client,
            &binding,
            "message-before-invalid-snapshot",
            "did:example:sync-snapshot-failed",
            "original projection",
        )
        .await;
        let snapshot_event = sync_snapshot_message_event(
            &binding,
            "event-invalid-snapshot",
            "2",
            "19",
            "message-invalid-snapshot",
            "did:example:sync-snapshot-failed",
        );
        let snapshot_message = sync_snapshot_message(
            &binding,
            "message-invalid-snapshot",
            "did:example:sync-snapshot-failed",
            "19",
            "must not commit",
        );
        let mut invalid_snapshot = sync_snapshot_response(
            &binding,
            "2",
            "20",
            vec![json!({
                "event": snapshot_event,
                "message": snapshot_message
            })],
        );
        invalid_snapshot["message_policy"]["max_logical_messages"] = json!(499);
        let calls = Rc::new(RefCell::new(Vec::new()));
        let error = MessageSyncRuntimeV2::new(
            &client,
            ReadySyncSnapshotSessionProvider,
            SyncSnapshotTransport::queued(
                Rc::clone(&calls),
                vec![
                    Ok(sync_snapshot_recovery(
                        "recovery-invalid",
                        "snapshot-token-invalid",
                        "2",
                        "20",
                    )),
                    Ok(invalid_snapshot),
                ],
            ),
            NoopAsyncDirectoryTransport,
        )
        .sync_now(sync_snapshot_request())
        .await
        .unwrap_err();

        assert!(matches!(
            error,
            crate::ImError::Service {
                code: Some(code),
                ..
            } if code == "SYNC_INVALID_PAGE"
        ));
        let state = load_sync_snapshot_state(&client, &binding.owner_identity_id).await;
        assert_eq!(
            (state.stream_epoch.as_str(), state.scan_seq.as_str()),
            ("1", "10")
        );
        assert!(fixture.has_message_content("original projection"));
        assert!(!fixture.has_message_content("must not commit"));
        let failed_conversation = crate::internal::local_state::owner_scope::group_conversation_id(
            "did:example:sync-snapshot-failed",
        );
        assert!(
            !super::super::sync::committed_sync_invalidations_for_test()
                .iter()
                .any(|invalidation| invalidation.conversation_ids.contains(&failed_conversation)),
            "failed snapshot must not emit a committed projection patch"
        );
        assert_eq!(
            calls
                .borrow()
                .iter()
                .map(|call| call.method.as_str())
                .collect::<Vec<_>>(),
            ["sync.delta", "sync.snapshot"]
        );
    }

    #[tokio::test]
    async fn schema_two_recovery_rejects_schema_one_snapshot_without_advancing_cursor() {
        let fixture = SyncSnapshotFixture::new("schema-two-missing-snapshot-fields");
        let client = fixture.client();
        let binding = client.active_sync_account_binding().await.unwrap();
        seed_sync_snapshot_ready_state(&client, &binding, "1", "10").await;
        let mut recovery = sync_snapshot_recovery(
            "recovery-schema-two",
            "snapshot-token-schema-two",
            "2",
            "20",
        );
        recovery["recovery"]["snapshot_schema"] = json!(2);
        let calls = Rc::new(RefCell::new(Vec::new()));
        let error = MessageSyncRuntimeV2::new(
            &client,
            ReadySyncSnapshotSessionProvider,
            SyncSnapshotTransport::queued(
                Rc::clone(&calls),
                vec![
                    Ok(recovery),
                    Ok(sync_snapshot_response(&binding, "2", "20", Vec::new())),
                ],
            ),
            NoopAsyncDirectoryTransport,
        )
        .sync_now(sync_snapshot_request())
        .await
        .unwrap_err();
        assert!(matches!(
            error,
            crate::ImError::Service { code: Some(code), .. }
                if code == "SYNC_INVALID_SNAPSHOT"
        ));
        let state = load_sync_snapshot_state(&client, &binding.owner_identity_id).await;
        assert_eq!(
            (state.stream_epoch.as_str(), state.scan_seq.as_str()),
            ("1", "10")
        );
        assert_eq!(
            calls
                .borrow()
                .iter()
                .map(|call| call.method.as_str())
                .collect::<Vec<_>>(),
            ["sync.delta", "sync.snapshot"]
        );
    }

    #[tokio::test]
    async fn schema_two_empty_notification_snapshot_commits_and_resumes_delta() {
        let fixture = SyncSnapshotFixture::new("schema-two-empty-notifications");
        let client = fixture.client();
        let binding = client.active_sync_account_binding().await.unwrap();
        seed_sync_snapshot_ready_state(&client, &binding, "1", "10").await;
        let mut recovery = sync_snapshot_recovery(
            "recovery-schema-two-empty",
            "snapshot-token-schema-two-empty",
            "2",
            "20",
        );
        recovery["recovery"]["snapshot_schema"] = json!(2);
        let mut snapshot = sync_snapshot_response(&binding, "2", "20", Vec::new());
        snapshot["snapshot_schema"] = json!(2);
        snapshot["unexpired_system_notifications"] = json!([]);
        snapshot["system_notification_policy"] = json!({
            "scope": "exact_device_unexpired",
            "complete_through_scan_seq": "20",
            "returned_events": 0,
            "complete": true
        });
        let calls = Rc::new(RefCell::new(Vec::new()));
        MessageSyncRuntimeV2::new(
            &client,
            ReadySyncSnapshotSessionProvider,
            SyncSnapshotTransport::queued(
                Rc::clone(&calls),
                vec![
                    Ok(recovery),
                    Ok(snapshot),
                    Ok(sync_snapshot_delta("2", "21", Vec::new())),
                ],
            ),
            NoopAsyncDirectoryTransport,
        )
        .sync_now(sync_snapshot_request())
        .await
        .unwrap();
        let state = load_sync_snapshot_state(&client, &binding.owner_identity_id).await;
        assert_eq!(
            (state.stream_epoch.as_str(), state.scan_seq.as_str()),
            ("2", "21")
        );
        assert_eq!(
            calls
                .borrow()
                .iter()
                .map(|call| call.method.as_str())
                .collect::<Vec<_>>(),
            ["sync.delta", "sync.snapshot", "sync.delta"]
        );
    }

    #[tokio::test]
    async fn filtered_exact_device_empty_delta_persists_cursor_across_runs() {
        let fixture = SyncSnapshotFixture::new("filtered-exact-device-empty-delta");
        let client = fixture.client();
        let binding = client.active_sync_account_binding().await.unwrap();
        seed_sync_snapshot_ready_state(&client, &binding, "1", "10").await;
        let calls = Rc::new(RefCell::new(Vec::new()));

        let first = MessageSyncRuntimeV2::new(
            &client,
            ReadySyncSnapshotSessionProvider,
            SyncSnapshotTransport::queued(
                Rc::clone(&calls),
                vec![Ok(sync_snapshot_delta("1", "11", Vec::new()))],
            ),
            NoopAsyncDirectoryTransport,
        )
        .sync_now(sync_snapshot_request())
        .await
        .unwrap();
        assert_eq!(first.status, crate::messages::MessageSyncStatus::Idle);
        assert_eq!(first.events_applied, 0);
        let advanced = load_sync_snapshot_state(&client, &binding.owner_identity_id).await;
        assert_eq!(advanced.scan_seq, "11");

        MessageSyncRuntimeV2::new(
            &client,
            ReadySyncSnapshotSessionProvider,
            SyncSnapshotTransport::queued(
                Rc::clone(&calls),
                vec![Ok(sync_snapshot_delta("1", "11", Vec::new()))],
            ),
            NoopAsyncDirectoryTransport,
        )
        .sync_now(sync_snapshot_request())
        .await
        .unwrap();

        let calls = calls.borrow();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].method, "sync.delta");
        assert_eq!(calls[1].method, "sync.delta");
        assert_eq!(
            calls[0].params.pointer("/body/cursor/scan_seq"),
            Some(&json!("10"))
        );
        assert_eq!(
            calls[1].params.pointer("/body/cursor/scan_seq"),
            Some(&json!("11"))
        );
    }

    #[tokio::test]
    async fn sync_snapshot_missing_state_bootstrap_recovery_closes_with_delta_ack() {
        let fixture = SyncSnapshotFixture::new("bootstrap-recovery");
        let client = fixture.client();
        let binding = client.active_sync_account_binding().await.unwrap();
        let client_instance_id = client
            .core_inner()
            .local_state_db()
            .await
            .unwrap()
            .load_or_create_sync_client_instance_id(&binding.owner_identity_id)
            .await
            .unwrap();
        let calls = Rc::new(RefCell::new(Vec::new()));
        let outcome = MessageSyncRuntimeV2::new(
            &client,
            ReadySyncSnapshotSessionProvider,
            SyncSnapshotTransport::queued(
                Rc::clone(&calls),
                vec![
                    Ok(explicit_sync_negotiation_response()),
                    Ok(sync_snapshot_bootstrap_recovery(
                        &binding,
                        &client_instance_id,
                        "recovery-bootstrap",
                        "snapshot-token-bootstrap",
                        "3",
                        "40",
                    )),
                    Ok(sync_snapshot_response(&binding, "3", "40", vec![])),
                    Ok(sync_snapshot_delta("3", "41", vec![])),
                ],
            ),
            NoopAsyncDirectoryTransport,
        )
        .sync_now(sync_snapshot_request())
        .await
        .unwrap();

        assert_eq!(outcome.status, crate::messages::MessageSyncStatus::Idle);
        let state = load_sync_snapshot_state(&client, &binding.owner_identity_id).await;
        assert_eq!(
            (state.stream_epoch.as_str(), state.scan_seq.as_str()),
            ("3", "41")
        );
        assert_eq!(state.bootstrap_state, "active");
        let calls = calls.borrow();
        assert_eq!(
            calls
                .iter()
                .map(|call| call.method.as_str())
                .collect::<Vec<_>>(),
            [
                "anp.get_capabilities",
                "sync.bootstrap",
                "sync.snapshot",
                "sync.delta"
            ]
        );
        assert!(calls[1]
            .params
            .pointer("/body/client_instance_id")
            .and_then(Value::as_str)
            .is_some());
        assert_eq!(
            calls[3].params.pointer("/body/cursor"),
            Some(&json!({"stream_epoch": "3", "scan_seq": "40"}))
        );
        assert!(calls[3]
            .params
            .pointer("/body/client_instance_id")
            .is_none());
    }

    #[tokio::test]
    async fn sync_snapshot_invalid_and_expired_tokens_redelta_once_and_restart_from_original_cursor(
    ) {
        let fixture = SyncSnapshotFixture::new("token-retry");
        let client = fixture.client();
        let binding = client.active_sync_account_binding().await.unwrap();
        seed_sync_snapshot_ready_state(&client, &binding, "1", "10").await;
        let first_calls = Rc::new(RefCell::new(Vec::new()));
        let error = MessageSyncRuntimeV2::new(
            &client,
            ReadySyncSnapshotSessionProvider,
            SyncSnapshotTransport::queued(
                Rc::clone(&first_calls),
                vec![
                    Ok(sync_snapshot_recovery(
                        "recovery-token-invalid",
                        "RAW_SYNC_TOKEN_INVALID",
                        "2",
                        "20",
                    )),
                    Err(sync_error(
                        "sync.recovery_token_invalid",
                        "recovery token was invalid",
                    )),
                    Ok(sync_snapshot_recovery(
                        "recovery-token-expired",
                        "RAW_SYNC_TOKEN_EXPIRED",
                        "2",
                        "20",
                    )),
                    Err(sync_error(
                        "sync.recovery_token_expired",
                        "replacement recovery token expired",
                    )),
                ],
            ),
            NoopAsyncDirectoryTransport,
        )
        .sync_now(sync_snapshot_request())
        .await
        .unwrap_err();

        assert!(matches!(
            error,
            crate::ImError::Service {
                code: Some(code),
                ..
            } if code == "sync.recovery_token_expired"
        ));
        let failed_state = load_sync_snapshot_state(&client, &binding.owner_identity_id).await;
        assert_eq!(
            (
                failed_state.stream_epoch.as_str(),
                failed_state.scan_seq.as_str()
            ),
            ("1", "10")
        );
        {
            let calls = first_calls.borrow();
            assert_eq!(
                calls
                    .iter()
                    .map(|call| call.method.as_str())
                    .collect::<Vec<_>>(),
                ["sync.delta", "sync.snapshot", "sync.delta", "sync.snapshot"]
            );
            assert_eq!(
                calls[0].params.pointer("/body/cursor/scan_seq"),
                Some(&json!("10"))
            );
            assert_eq!(
                calls[2].params.pointer("/body/cursor/scan_seq"),
                Some(&json!("10"))
            );
        }

        let restart_calls = Rc::new(RefCell::new(Vec::new()));
        let restart_outcome = MessageSyncRuntimeV2::new(
            &client,
            ReadySyncSnapshotSessionProvider,
            SyncSnapshotTransport::queued(
                Rc::clone(&restart_calls),
                vec![
                    Ok(sync_snapshot_recovery(
                        "recovery-token-restart",
                        "RAW_SYNC_TOKEN_RESTART",
                        "2",
                        "20",
                    )),
                    Ok(sync_snapshot_response(&binding, "2", "20", vec![])),
                    Ok(sync_snapshot_delta("2", "21", vec![])),
                ],
            ),
            NoopAsyncDirectoryTransport,
        )
        .sync_now(sync_snapshot_request())
        .await
        .unwrap();

        assert_eq!(
            restart_outcome.status,
            crate::messages::MessageSyncStatus::Idle
        );
        let restart_cursor = restart_calls.borrow()[0]
            .params
            .pointer("/body/cursor/scan_seq")
            .cloned();
        assert_eq!(
            restart_cursor,
            Some(json!("10")),
            "restart must request a fresh recovery authorization from the durable cursor"
        );
        let state = load_sync_snapshot_state(&client, &binding.owner_identity_id).await;
        assert_eq!(
            (state.stream_epoch.as_str(), state.scan_seq.as_str()),
            ("2", "21")
        );

        let connection = rusqlite::Connection::open(fixture.sqlite_path()).unwrap();
        let persisted_recovery: String = connection
            .query_row(
                "SELECT mode || '|' || requested_from_epoch || '|' ||
                        requested_from_seq || '|' || COALESCE(recovery_id_hash, '') || '|' ||
                        COALESCE(snapshot_scan_seq, '') || '|' || status || '|' ||
                        COALESCE(last_error_code, '')
                 FROM sync_recovery_state
                 WHERE owner_identity_id = ?1",
                [binding.owner_identity_id.as_str()],
                |row| row.get(0),
            )
            .unwrap();
        for raw_token in [
            "RAW_SYNC_TOKEN_INVALID",
            "RAW_SYNC_TOKEN_EXPIRED",
            "RAW_SYNC_TOKEN_RESTART",
        ] {
            assert!(
                !persisted_recovery.contains(raw_token),
                "SQLite recovery state must not contain raw recovery tokens"
            );
        }
        assert!(persisted_recovery.contains(&hex_sha256("recovery-token-restart")));
    }

    #[tokio::test]
    async fn sync_read_outbox_unbound_local_mark_waits_for_exact_binding_then_drains() {
        let fixture = SyncSnapshotFixture::new("read-outbox-late-binding");
        let client = fixture.client();
        let binding = client.active_sync_account_binding().await.unwrap();
        seed_sync_snapshot_ready_state(&client, &binding, "1", "10").await;
        let conversation_id = "dm:peer-scope:v1:alice:bob";
        seed_sync_read_direct_message(&client, &binding, "message-read-30", conversation_id, 30)
            .await;
        let local = client
            .core_inner()
            .local_state_db()
            .await
            .unwrap()
            .mark_thread_read_watermark(
                binding.owner_identity_id.clone(),
                binding.current_did.clone(),
                crate::internal::local_state::messages::MarkThreadReadWatermarkInput {
                    thread: crate::messages::ThreadRef::Thread(
                        crate::ids::ThreadId::parse(conversation_id).unwrap(),
                    ),
                    read_watermark_message_id: Some("message-read-30".to_owned()),
                    read_watermark_seq: Some("30".to_owned()),
                    read_watermark_at: Some("2026-07-28T12:00:02Z".to_owned()),
                    pending_remote_ack: true,
                },
            )
            .await
            .unwrap();
        assert_eq!(local.remote_thread_key, None);
        assert_eq!(local.outbox_operation_id, None);
        {
            let connection = rusqlite::Connection::open(fixture.sqlite_path()).unwrap();
            assert_eq!(
                connection
                    .query_row(
                        "SELECT pending_remote_ack FROM thread_read_state
                         WHERE owner_identity_id = ?1 AND conversation_id = ?2",
                        [binding.owner_identity_id.as_str(), conversation_id],
                        |row| row.get::<_, i64>(0),
                    )
                    .unwrap(),
                1
            );
            assert_eq!(
                connection
                    .query_row(
                        "SELECT COUNT(*) FROM local_mutation_outbox
                         WHERE owner_identity_id = ?1",
                        [binding.owner_identity_id.as_str()],
                        |row| row.get::<_, i64>(0),
                    )
                    .unwrap(),
                0,
                "an unresolved Direct conversation must not guess a remote DID/thread key"
            );
        }

        apply_sync_read_thread_binding(
            &client,
            &binding,
            "event-read-binding-11",
            "11",
            "remote-thread-key-exact-bob",
            conversation_id,
        )
        .await;
        let operation_id = {
            let connection = rusqlite::Connection::open(fixture.sqlite_path()).unwrap();
            let rows = connection
                .prepare(
                    "SELECT operation_id, aggregate_id,
                            json_extract(payload_json, '$.read_watermark_seq')
                     FROM local_mutation_outbox
                     WHERE owner_identity_id = ?1
                       AND status NOT IN ('committed', 'permanent_failure')",
                )
                .unwrap()
                .query_map([binding.owner_identity_id.as_str()], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                })
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].1, "remote-thread-key-exact-bob");
            assert_eq!(rows[0].2, "30");
            rows[0].0.clone()
        };

        let calls = Rc::new(RefCell::new(Vec::new()));
        MessageSyncRuntimeV2::new(
            &client,
            ReadySyncSnapshotSessionProvider,
            SyncSnapshotTransport::queued(
                Rc::clone(&calls),
                vec![
                    Ok(sync_snapshot_delta("1", "12", vec![])),
                    Ok(sync_read_ack(
                        &binding,
                        "remote-thread-key-exact-bob",
                        "30",
                        "message-read-30",
                        "2026-07-28T12:00:03Z",
                    )),
                ],
            ),
            NoopAsyncDirectoryTransport,
        )
        .sync_now(sync_snapshot_request())
        .await
        .unwrap();

        let calls = calls.borrow();
        assert_eq!(
            calls
                .iter()
                .map(|call| call.method.as_str())
                .collect::<Vec<_>>(),
            ["sync.delta", "read_state.mark_read"]
        );
        assert_eq!(
            calls[1].params.pointer("/body/thread"),
            Some(&json!({
                "kind": "direct",
                "thread_key": "remote-thread-key-exact-bob"
            }))
        );
        assert_eq!(
            calls[1].params.pointer("/meta/operation_id"),
            Some(&json!(operation_id))
        );
        let connection = rusqlite::Connection::open(fixture.sqlite_path()).unwrap();
        assert_eq!(
            connection
                .query_row(
                    "SELECT status FROM local_mutation_outbox
                     WHERE owner_identity_id = ?1",
                    [binding.owner_identity_id.as_str()],
                    |row| row.get::<_, String>(0),
                )
                .unwrap(),
            "committed"
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT pending_remote_ack FROM thread_read_state
                     WHERE owner_identity_id = ?1 AND conversation_id = ?2",
                    [binding.owner_identity_id.as_str(), conversation_id],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            0
        );
    }

    #[tokio::test]
    async fn sync_read_outbox_drain_merges_higher_service_watermark_without_regression() {
        let fixture = SyncSnapshotFixture::new("read-outbox-higher-service-watermark");
        let client = fixture.client();
        let binding = client.active_sync_account_binding().await.unwrap();
        seed_sync_snapshot_ready_state(&client, &binding, "1", "10").await;
        let conversation_id = "dm:peer-scope:v1:alice:bob-higher";
        seed_sync_read_direct_message(&client, &binding, "message-read-low", conversation_id, 30)
            .await;
        seed_sync_read_direct_message(&client, &binding, "message-read-high", conversation_id, 50)
            .await;
        apply_sync_read_thread_binding(
            &client,
            &binding,
            "event-read-binding-higher-11",
            "11",
            "remote-thread-key-higher-bob",
            conversation_id,
        )
        .await;
        client
            .core_inner()
            .local_state_db()
            .await
            .unwrap()
            .mark_thread_read_watermark(
                binding.owner_identity_id.clone(),
                binding.current_did.clone(),
                crate::internal::local_state::messages::MarkThreadReadWatermarkInput {
                    thread: crate::messages::ThreadRef::Thread(
                        crate::ids::ThreadId::parse(conversation_id).unwrap(),
                    ),
                    read_watermark_message_id: Some("message-read-low".to_owned()),
                    read_watermark_seq: Some("30".to_owned()),
                    read_watermark_at: Some("2026-07-28T12:00:02Z".to_owned()),
                    pending_remote_ack: true,
                },
            )
            .await
            .unwrap();

        MessageSyncRuntimeV2::new(
            &client,
            ReadySyncSnapshotSessionProvider,
            SyncSnapshotTransport::queued(
                Rc::new(RefCell::new(Vec::new())),
                vec![
                    Ok(sync_snapshot_delta("1", "12", vec![])),
                    Ok(sync_read_ack(
                        &binding,
                        "remote-thread-key-higher-bob",
                        "50",
                        "message-read-high",
                        "2026-07-28T12:00:09Z",
                    )),
                ],
            ),
            NoopAsyncDirectoryTransport,
        )
        .sync_now(sync_snapshot_request())
        .await
        .unwrap();

        let connection = rusqlite::Connection::open(fixture.sqlite_path()).unwrap();
        let projected = connection
            .query_row(
                "SELECT read_watermark_seq, read_watermark_message_id,
                        read_watermark_at, pending_remote_ack
                 FROM thread_read_state
                 WHERE owner_identity_id = ?1 AND conversation_id = ?2",
                [binding.owner_identity_id.as_str(), conversation_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, i64>(3)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(
            projected,
            (
                "50".to_owned(),
                "message-read-high".to_owned(),
                "2026-07-28T12:00:09Z".to_owned(),
                0
            )
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT status FROM local_mutation_outbox
                     WHERE owner_identity_id = ?1",
                    [binding.owner_identity_id.as_str()],
                    |row| row.get::<_, String>(0),
                )
                .unwrap(),
            "committed"
        );
    }

    #[tokio::test]
    async fn sync_read_outbox_pseudo_ack_does_not_change_delta_result() {
        let fixture = SyncSnapshotFixture::new("read-outbox-pseudo-ack");
        let client = fixture.client();
        let binding = client.active_sync_account_binding().await.unwrap();
        seed_sync_snapshot_ready_state(&client, &binding, "1", "10").await;
        let conversation_id = "dm:peer-scope:v1:alice:pseudo-ack";
        seed_sync_read_direct_message(
            &client,
            &binding,
            "message-read-pseudo",
            conversation_id,
            30,
        )
        .await;
        apply_sync_read_thread_binding(
            &client,
            &binding,
            "event-read-binding-pseudo",
            "11",
            "remote-thread-key-pseudo",
            conversation_id,
        )
        .await;
        client
            .core_inner()
            .local_state_db()
            .await
            .unwrap()
            .mark_thread_read_watermark(
                binding.owner_identity_id.clone(),
                binding.current_did.clone(),
                crate::internal::local_state::messages::MarkThreadReadWatermarkInput {
                    thread: crate::messages::ThreadRef::Thread(
                        crate::ids::ThreadId::parse(conversation_id).unwrap(),
                    ),
                    read_watermark_message_id: Some("message-read-pseudo".to_owned()),
                    read_watermark_seq: Some("30".to_owned()),
                    read_watermark_at: Some("2026-07-28T12:00:02Z".to_owned()),
                    pending_remote_ack: true,
                },
            )
            .await
            .unwrap();
        let mut pseudo_ack = sync_read_ack(
            &binding,
            "remote-thread-key-pseudo",
            "30",
            "message-read-pseudo",
            "2026-07-28T12:00:03Z",
        );
        pseudo_ack["remote_acknowledged"] = json!(false);
        pseudo_ack["pending_remote_ack"] = json!(true);
        let calls = Rc::new(RefCell::new(Vec::new()));
        let outcome = MessageSyncRuntimeV2::new(
            &client,
            ReadySyncSnapshotSessionProvider,
            SyncSnapshotTransport::queued(
                Rc::clone(&calls),
                vec![Ok(sync_snapshot_delta("1", "12", vec![])), Ok(pseudo_ack)],
            ),
            NoopAsyncDirectoryTransport,
        )
        .sync_now(sync_snapshot_request())
        .await
        .unwrap();
        assert_eq!(outcome.status, crate::messages::MessageSyncStatus::Idle);
        assert_eq!(
            calls
                .borrow()
                .iter()
                .map(|call| call.method.as_str())
                .collect::<Vec<_>>(),
            ["sync.delta", "read_state.mark_read"]
        );
        assert_eq!(
            load_sync_snapshot_state(&client, &binding.owner_identity_id)
                .await
                .scan_seq,
            "12"
        );
        let connection = rusqlite::Connection::open(fixture.sqlite_path()).unwrap();
        let state = connection
            .query_row(
                "SELECT status, in_flight_since, last_error_code
                 FROM local_mutation_outbox
                 WHERE owner_identity_id = ?1",
                [binding.owner_identity_id.as_str()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<i64>>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(
            state,
            (
                "retryable".to_owned(),
                None,
                "READ_STATE_INCOMPLETE_ACK".to_owned(),
            )
        );
    }

    #[tokio::test]
    async fn retryable_read_outbox_transport_failure_does_not_change_delta_result() {
        let fixture = SyncSnapshotFixture::new("read-outbox-transport-failure");
        let client = fixture.client();
        let binding = client.active_sync_account_binding().await.unwrap();
        seed_sync_snapshot_ready_state(&client, &binding, "1", "10").await;
        let conversation_id = "dm:peer-scope:v1:alice:transport-failure";
        seed_sync_read_direct_message(
            &client,
            &binding,
            "message-read-transport-failure",
            conversation_id,
            30,
        )
        .await;
        apply_sync_read_thread_binding(
            &client,
            &binding,
            "event-read-binding-transport-failure",
            "11",
            "remote-thread-key-transport-failure",
            conversation_id,
        )
        .await;
        client
            .core_inner()
            .local_state_db()
            .await
            .unwrap()
            .mark_thread_read_watermark(
                binding.owner_identity_id.clone(),
                binding.current_did.clone(),
                crate::internal::local_state::messages::MarkThreadReadWatermarkInput {
                    thread: crate::messages::ThreadRef::Thread(
                        crate::ids::ThreadId::parse(conversation_id).unwrap(),
                    ),
                    read_watermark_message_id: Some("message-read-transport-failure".to_owned()),
                    read_watermark_seq: Some("30".to_owned()),
                    read_watermark_at: Some("2026-07-28T12:00:02Z".to_owned()),
                    pending_remote_ack: true,
                },
            )
            .await
            .unwrap();

        let calls = Rc::new(RefCell::new(Vec::new()));
        let outcome = MessageSyncRuntimeV2::new(
            &client,
            ReadySyncSnapshotSessionProvider,
            SyncSnapshotTransport::queued(
                Rc::clone(&calls),
                vec![
                    Ok(sync_snapshot_delta("1", "12", vec![])),
                    Err(crate::ImError::TransportUnavailable {
                        detail: "read writeback transport unavailable".to_owned(),
                    }),
                ],
            ),
            NoopAsyncDirectoryTransport,
        )
        .sync_now(sync_snapshot_request())
        .await
        .unwrap();

        assert_eq!(outcome.status, crate::messages::MessageSyncStatus::Idle);
        assert_eq!(
            load_sync_snapshot_state(&client, &binding.owner_identity_id)
                .await
                .scan_seq,
            "12"
        );
        assert_eq!(
            calls
                .borrow()
                .iter()
                .map(|call| call.method.as_str())
                .collect::<Vec<_>>(),
            ["sync.delta", "read_state.mark_read"]
        );
        let connection = rusqlite::Connection::open(fixture.sqlite_path()).unwrap();
        assert_eq!(
            connection
                .query_row(
                    "SELECT status || '|' || last_error_code
                     FROM local_mutation_outbox
                     WHERE owner_identity_id = ?1",
                    [binding.owner_identity_id.as_str()],
                    |row| row.get::<_, String>(0),
                )
                .unwrap(),
            "retryable|READ_STATE_RETRY"
        );
    }

    #[tokio::test]
    async fn corrupt_read_outbox_is_permanently_failed_after_delta_commits() {
        let fixture = SyncSnapshotFixture::new("read-outbox-corrupt");
        let client = fixture.client();
        let binding = client.active_sync_account_binding().await.unwrap();
        seed_sync_snapshot_ready_state(&client, &binding, "1", "10").await;
        let conversation_id = "dm:peer-scope:v1:alice:corrupt-outbox";
        seed_sync_read_direct_message(
            &client,
            &binding,
            "message-read-corrupt-outbox",
            conversation_id,
            30,
        )
        .await;
        apply_sync_read_thread_binding(
            &client,
            &binding,
            "event-read-binding-corrupt-outbox",
            "11",
            "remote-thread-key-corrupt-outbox",
            conversation_id,
        )
        .await;
        client
            .core_inner()
            .local_state_db()
            .await
            .unwrap()
            .mark_thread_read_watermark(
                binding.owner_identity_id.clone(),
                binding.current_did.clone(),
                crate::internal::local_state::messages::MarkThreadReadWatermarkInput {
                    thread: crate::messages::ThreadRef::Thread(
                        crate::ids::ThreadId::parse(conversation_id).unwrap(),
                    ),
                    read_watermark_message_id: Some("message-read-corrupt-outbox".to_owned()),
                    read_watermark_seq: Some("30".to_owned()),
                    read_watermark_at: Some("2026-07-28T12:00:02Z".to_owned()),
                    pending_remote_ack: true,
                },
            )
            .await
            .unwrap();
        rusqlite::Connection::open(fixture.sqlite_path())
            .unwrap()
            .execute(
                "UPDATE local_mutation_outbox SET payload_json = 'not-json'
                 WHERE owner_identity_id = ?1",
                [binding.owner_identity_id.as_str()],
            )
            .unwrap();

        let calls = Rc::new(RefCell::new(Vec::new()));
        let outcome = MessageSyncRuntimeV2::new(
            &client,
            ReadySyncSnapshotSessionProvider,
            SyncSnapshotTransport::queued(
                Rc::clone(&calls),
                vec![Ok(sync_snapshot_delta("1", "12", vec![]))],
            ),
            NoopAsyncDirectoryTransport,
        )
        .sync_now(sync_snapshot_request())
        .await
        .unwrap();

        assert_eq!(outcome.status, crate::messages::MessageSyncStatus::Idle);
        assert_eq!(
            calls
                .borrow()
                .iter()
                .map(|call| call.method.as_str())
                .collect::<Vec<_>>(),
            ["sync.delta"]
        );
        let connection = rusqlite::Connection::open(fixture.sqlite_path()).unwrap();
        assert_eq!(
            connection
                .query_row(
                    "SELECT status || '|' || last_error_code
                     FROM local_mutation_outbox
                     WHERE owner_identity_id = ?1",
                    [binding.owner_identity_id.as_str()],
                    |row| row.get::<_, String>(0),
                )
                .unwrap(),
            "permanent_failure|SYNC_LOCAL_OUTBOX_CORRUPT"
        );
    }

    #[tokio::test]
    async fn device_epoch_refresh_retries_read_outbox_after_delta_commits() {
        let fixture = SyncSnapshotFixture::new("device-epoch-refresh");
        let client = fixture.client();
        let binding = client.active_sync_account_binding().await.unwrap();
        seed_sync_snapshot_ready_state(&client, &binding, "1", "10").await;
        let group_did = "did:wba:awiki.test:groups:device-epoch-refresh";
        seed_sync_read_group_message(
            &client,
            &binding,
            &format!("{group_did}:30"),
            None,
            group_did,
            30,
        )
        .await;
        client
            .core_inner()
            .local_state_db()
            .await
            .unwrap()
            .mark_thread_read_watermark(
                binding.owner_identity_id.clone(),
                binding.current_did.clone(),
                crate::internal::local_state::messages::MarkThreadReadWatermarkInput {
                    thread: crate::messages::ThreadRef::Group(
                        crate::ids::GroupRef::parse(group_did).unwrap(),
                    ),
                    read_watermark_message_id: Some(format!("{group_did}:30")),
                    read_watermark_seq: Some("30".to_owned()),
                    read_watermark_at: Some("2026-07-28T12:00:02Z".to_owned()),
                    pending_remote_ack: true,
                },
            )
            .await
            .unwrap();

        let event_id = "event-after-device-epoch-refresh";
        let message_id = "message-after-device-epoch-refresh";
        let calls = Rc::new(RefCell::new(Vec::new()));
        let refresh_calls = Rc::new(RefCell::new(0));
        let authentication_reloads = Rc::new(RefCell::new(0));
        let lane_bootstrap =
            sync_snapshot_tail_bootstrap_for_current_features(&client, &binding, "1", "12").await;
        let outcome = MessageSyncRuntimeV2::new(
            &client,
            RefreshingSyncSnapshotSessionProvider {
                refresh_calls: Rc::clone(&refresh_calls),
                fail_refresh: false,
            },
            ReloadingSyncSnapshotTransport {
                inner: SyncSnapshotTransport::queued(
                    Rc::clone(&calls),
                    vec![
                        Ok(sync_snapshot_delta(
                            "1",
                            "12",
                            vec![sync_snapshot_message_event(
                                &binding, event_id, "1", "12", message_id, group_did,
                            )],
                        )),
                        Ok(json!({
                            "items": [{
                                "event_id": event_id,
                                "message": sync_snapshot_message(
                                    &binding,
                                    message_id,
                                    group_did,
                                    "31",
                                    "message delivered after device epoch refresh",
                                )
                            }],
                            "unavailable": []
                        })),
                        Err(crate::ImError::Service {
                            status_code: Some(409),
                            code: Some("anp.device_state_changed".to_owned()),
                            message: "device authorization epoch is stale".to_owned(),
                            data: None,
                        }),
                        Ok(json!({
                            "supported_profiles": [
                                crate::internal::wire::sync_v2::MESSAGE_SYNC_EXPLICIT_NEGOTIATION_V1
                            ]
                        })),
                        Ok(lane_bootstrap),
                        Ok(sync_group_read_ack(
                            &binding,
                            group_did,
                            "30",
                            &format!("{group_did}:30"),
                            "2026-07-28T12:00:03Z",
                        )),
                    ],
                ),
                authentication_reloads: Rc::clone(&authentication_reloads),
            },
            NoopAsyncDirectoryTransport,
        )
        .sync_now(sync_snapshot_request())
        .await
        .unwrap();

        assert_eq!(outcome.status, crate::messages::MessageSyncStatus::Changed);
        assert_eq!(outcome.events_applied, 1);
        assert!(fixture.has_message_content("message delivered after device epoch refresh"));
        assert_eq!(*refresh_calls.borrow(), 1);
        assert_eq!(*authentication_reloads.borrow(), 1);
        let calls = calls.borrow();
        assert_eq!(
            calls
                .iter()
                .map(|call| call.method.as_str())
                .collect::<Vec<_>>(),
            [
                "sync.delta",
                "message.get_batch",
                "read_state.mark_read",
                "anp.get_capabilities",
                "sync.bootstrap",
                "read_state.mark_read"
            ]
        );
    }

    #[cfg(feature = "secure-direct")]
    #[tokio::test]
    async fn device_epoch_refresh_revalidates_p5_lane_epoch_before_retry() {
        use crate::internal::wire::sync_v2::SyncLaneV3;

        let fixture = SyncSnapshotFixture::new("device-epoch-p5-lane-refresh");
        let client = fixture.client();
        let binding = client.active_sync_account_binding().await.unwrap();
        let client_instance_id = client
            .core_inner()
            .local_state_db()
            .await
            .unwrap()
            .load_or_create_sync_client_instance_id(&binding.owner_identity_id)
            .await
            .unwrap();
        seed_sync_snapshot_ready_state(&client, &binding, "1", "10").await;
        seed_lane_states(&client, &binding, &[(SyncLaneV3::P5Device, "41")]).await;
        let calls = Rc::new(RefCell::new(Vec::new()));
        let refresh_calls = Rc::new(RefCell::new(0));
        let authentication_reloads = Rc::new(RefCell::new(0));
        let rejected = crate::ImError::Service {
            status_code: Some(403),
            code: Some("anp.device_not_eligible".to_owned()),
            message: "device authorization epoch is stale".to_owned(),
            data: None,
        };
        let mut lane_bootstrap = json!({
            "mode": "tail_only",
            "account_id": binding.account_id,
            "device_id": binding.protocol_device_id,
            "server_time": "2026-08-15T00:00:00Z",
            "cursor": {"stream_epoch": "1", "scan_seq": "10"},
            "read_state_baseline": [],
            "group_state_baseline": [],
            "warnings": [],
        });
        add_enabled_lane_bootstrap(&mut lane_bootstrap, &client_instance_id, "51", "52");
        let retry_delta = sync_snapshot_delta_with_lanes(
            "1",
            "10",
            json!({
                "p5_device": {
                    "events": [],
                    "next_cursor": {"stream_epoch": "51", "scan_seq": "0"},
                    "has_more": false
                },
                "p6_group": {
                    "events": [],
                    "next_cursor": {"stream_epoch": "52", "scan_seq": "0"},
                    "has_more": false
                }
            }),
        );

        let outcome = MessageSyncRuntimeV2::new(
            &client,
            RefreshingSyncSnapshotSessionProvider {
                refresh_calls: Rc::clone(&refresh_calls),
                fail_refresh: false,
            },
            ReloadingSyncSnapshotTransport {
                inner: SyncSnapshotTransport::queued(
                    Rc::clone(&calls),
                    vec![
                        Err(rejected),
                        Ok(explicit_sync_negotiation_response()),
                        Ok(lane_bootstrap),
                        Ok(retry_delta),
                    ],
                ),
                authentication_reloads: Rc::clone(&authentication_reloads),
            },
            NoopAsyncDirectoryTransport,
        )
        .sync_now(sync_snapshot_request())
        .await
        .unwrap();

        assert_eq!(outcome.status, crate::messages::MessageSyncStatus::Idle);
        assert_eq!(*refresh_calls.borrow(), 1);
        assert_eq!(*authentication_reloads.borrow(), 1);
        {
            let calls = calls.borrow();
            assert_eq!(
                calls
                    .iter()
                    .map(|call| call.method.as_str())
                    .collect::<Vec<_>>(),
                [
                    "sync.delta",
                    "anp.get_capabilities",
                    "sync.bootstrap",
                    "sync.delta"
                ]
            );
            assert_eq!(
                calls[0]
                    .params
                    .pointer("/body/lanes/p5_device/cursor/stream_epoch"),
                Some(&json!("41"))
            );
            assert_eq!(
                calls[3]
                    .params
                    .pointer("/body/lanes/p5_device/cursor/stream_epoch"),
                Some(&json!("51"))
            );
        }
        let lane = client
            .core_inner()
            .local_state_db()
            .await
            .unwrap()
            .load_lane_sync_states(binding.owner_identity_id)
            .await
            .unwrap()
            .into_iter()
            .find(|state| state.lane == SyncLaneV3::P5Device)
            .unwrap();
        assert_eq!(
            (lane.stream_epoch.as_str(), lane.scan_seq.as_str()),
            ("51", "0")
        );
    }

    #[tokio::test]
    async fn repeated_device_epoch_rejection_is_terminal_after_one_refresh() {
        let fixture = SyncSnapshotFixture::new("device-epoch-refresh-exhausted");
        let client = fixture.client();
        let binding = client.active_sync_account_binding().await.unwrap();
        seed_sync_snapshot_ready_state(&client, &binding, "1", "10").await;
        let calls = Rc::new(RefCell::new(Vec::new()));
        let refresh_calls = Rc::new(RefCell::new(0));
        let authentication_reloads = Rc::new(RefCell::new(0));
        let rejected = || crate::ImError::Service {
            status_code: Some(403),
            code: Some("anp.device_not_eligible".to_owned()),
            message: "device remains ineligible".to_owned(),
            data: None,
        };
        let lane_bootstrap =
            sync_snapshot_tail_bootstrap_for_current_features(&client, &binding, "1", "10").await;
        let error = MessageSyncRuntimeV2::new(
            &client,
            RefreshingSyncSnapshotSessionProvider {
                refresh_calls: Rc::clone(&refresh_calls),
                fail_refresh: false,
            },
            ReloadingSyncSnapshotTransport {
                inner: SyncSnapshotTransport::queued(
                    Rc::clone(&calls),
                    vec![
                        Err(rejected()),
                        Ok(json!({
                            "supported_profiles": [
                                crate::internal::wire::sync_v2::MESSAGE_SYNC_EXPLICIT_NEGOTIATION_V1
                            ]
                        })),
                        Ok(lane_bootstrap),
                        Err(rejected()),
                    ],
                ),
                authentication_reloads: Rc::clone(&authentication_reloads),
            },
            NoopAsyncDirectoryTransport,
        )
        .sync_now(sync_snapshot_request())
        .await
        .unwrap_err();

        let outcome = failure_outcome(&error).expect("device rejection must be classified");
        assert_eq!(
            outcome.status,
            crate::messages::MessageSyncStatus::AuthRevoked
        );
        assert_eq!(*refresh_calls.borrow(), 1);
        assert_eq!(*authentication_reloads.borrow(), 1);
        assert_eq!(
            calls
                .borrow()
                .iter()
                .map(|call| call.method.as_str())
                .collect::<Vec<_>>(),
            [
                "sync.delta",
                "anp.get_capabilities",
                "sync.bootstrap",
                "sync.delta"
            ]
        );
    }

    #[tokio::test]
    async fn failed_device_epoch_refresh_is_auth_revoked() {
        let fixture = SyncSnapshotFixture::new("device-epoch-refresh-failed");
        let client = fixture.client();
        let binding = client.active_sync_account_binding().await.unwrap();
        seed_sync_snapshot_ready_state(&client, &binding, "1", "10").await;
        let calls = Rc::new(RefCell::new(Vec::new()));
        let refresh_calls = Rc::new(RefCell::new(0));
        let error = MessageSyncRuntimeV2::new(
            &client,
            RefreshingSyncSnapshotSessionProvider {
                refresh_calls: Rc::clone(&refresh_calls),
                fail_refresh: true,
            },
            SyncSnapshotTransport::queued(
                Rc::clone(&calls),
                vec![Err(crate::ImError::Service {
                    status_code: Some(409),
                    code: Some("anp.device_state_changed".to_owned()),
                    message: "device authorization epoch is stale".to_owned(),
                    data: None,
                })],
            ),
            NoopAsyncDirectoryTransport,
        )
        .sync_now(sync_snapshot_request())
        .await
        .unwrap_err();

        let outcome = failure_outcome(&error).expect("refresh failure must be classified");
        assert_eq!(
            outcome.status,
            crate::messages::MessageSyncStatus::AuthRevoked
        );
        assert_eq!(*refresh_calls.borrow(), 1);
        assert_eq!(
            calls
                .borrow()
                .iter()
                .map(|call| call.method.as_str())
                .collect::<Vec<_>>(),
            ["sync.delta"]
        );
    }

    #[tokio::test]
    async fn stale_direct_read_target_does_not_block_delta_sync() {
        let fixture = SyncSnapshotFixture::new("read-outbox-stale-direct-target");
        let client = fixture.client();
        let binding = client.active_sync_account_binding().await.unwrap();
        seed_sync_snapshot_ready_state(&client, &binding, "1", "10").await;
        let conversation_id = "dm:peer-scope:v1:alice:stale-target";
        seed_sync_read_direct_message(&client, &binding, "message-read-stale", conversation_id, 30)
            .await;
        apply_sync_read_thread_binding(
            &client,
            &binding,
            "event-read-binding-stale",
            "11",
            "remote-thread-key-stale",
            conversation_id,
        )
        .await;
        client
            .core_inner()
            .local_state_db()
            .await
            .unwrap()
            .mark_thread_read_watermark(
                binding.owner_identity_id.clone(),
                binding.current_did.clone(),
                crate::internal::local_state::messages::MarkThreadReadWatermarkInput {
                    thread: crate::messages::ThreadRef::Thread(
                        crate::ids::ThreadId::parse(conversation_id).unwrap(),
                    ),
                    read_watermark_message_id: Some("message-read-stale".to_owned()),
                    read_watermark_seq: Some("30".to_owned()),
                    read_watermark_at: Some("2026-07-28T12:00:02Z".to_owned()),
                    pending_remote_ack: true,
                },
            )
            .await
            .unwrap();

        let calls = Rc::new(RefCell::new(Vec::new()));
        let outcome = MessageSyncRuntimeV2::new(
            &client,
            ReadySyncSnapshotSessionProvider,
            SyncSnapshotTransport::queued(
                Rc::clone(&calls),
                vec![
                    Ok(sync_snapshot_delta("1", "12", vec![])),
                    Err(crate::ImError::Service {
                        status_code: Some(404),
                        code: Some("anp.target_not_found".to_owned()),
                        message: "the old Direct target no longer exists".to_owned(),
                        data: None,
                    }),
                ],
            ),
            NoopAsyncDirectoryTransport,
        )
        .sync_now(sync_snapshot_request())
        .await
        .unwrap();

        assert_eq!(outcome.status, crate::messages::MessageSyncStatus::Idle);
        assert_eq!(
            calls
                .borrow()
                .iter()
                .map(|call| call.method.as_str())
                .collect::<Vec<_>>(),
            ["sync.delta", "read_state.mark_read"]
        );
        let connection = rusqlite::Connection::open(fixture.sqlite_path()).unwrap();
        assert_eq!(
            connection
                .query_row(
                    "SELECT status || '|' || last_error_code
                     FROM local_mutation_outbox
                     WHERE owner_identity_id = ?1",
                    [binding.owner_identity_id.as_str()],
                    |row| row.get::<_, String>(0),
                )
                .unwrap(),
            "retryable|anp.target_not_found"
        );
    }

    #[tokio::test]
    async fn failed_legacy_group_read_task_is_repaired_to_sequence_only() {
        let fixture = SyncSnapshotFixture::new("read-outbox-legacy-group-target");
        let client = fixture.client();
        let binding = client.active_sync_account_binding().await.unwrap();
        seed_sync_snapshot_ready_state(&client, &binding, "1", "10").await;
        let group_did = "did:wba:awiki.info:groups:legacy-read-target";
        let local_message_id = format!("{group_did}:30");
        seed_sync_read_group_message(
            &client,
            &binding,
            &local_message_id,
            Some("business-group-30"),
            group_did,
            30,
        )
        .await;
        client
            .core_inner()
            .local_state_db()
            .await
            .unwrap()
            .mark_thread_read_watermark(
                binding.owner_identity_id.clone(),
                binding.current_did.clone(),
                crate::internal::local_state::messages::MarkThreadReadWatermarkInput {
                    thread: crate::messages::ThreadRef::Group(
                        crate::ids::GroupRef::parse(group_did).unwrap(),
                    ),
                    read_watermark_message_id: Some(local_message_id.clone()),
                    read_watermark_seq: Some("30".to_owned()),
                    read_watermark_at: Some("2026-07-28T12:00:02Z".to_owned()),
                    pending_remote_ack: true,
                },
            )
            .await
            .unwrap();
        rusqlite::Connection::open(fixture.sqlite_path())
            .unwrap()
            .execute(
                "UPDATE local_mutation_outbox
                 SET payload_json = json_set(
                         payload_json,
                         '$.read_watermark_message_id',
                         ?1
                     ),
                     status = 'retryable', retry_at = 0,
                     last_error_code = 'anp.target_not_found'",
                [&local_message_id],
            )
            .unwrap();

        let calls = Rc::new(RefCell::new(Vec::new()));
        let outcome = MessageSyncRuntimeV2::new(
            &client,
            ReadySyncSnapshotSessionProvider,
            SyncSnapshotTransport::queued(
                Rc::clone(&calls),
                vec![
                    Ok(sync_snapshot_delta("1", "12", vec![])),
                    Ok(sync_group_read_ack(
                        &binding,
                        group_did,
                        "30",
                        "business-group-30",
                        "2026-07-28T12:00:03Z",
                    )),
                ],
            ),
            NoopAsyncDirectoryTransport,
        )
        .sync_now(sync_snapshot_request())
        .await
        .unwrap();

        assert_eq!(outcome.status, crate::messages::MessageSyncStatus::Idle);
        let calls = calls.borrow();
        assert_eq!(
            calls
                .iter()
                .map(|call| call.method.as_str())
                .collect::<Vec<_>>(),
            ["sync.delta", "read_state.mark_read"]
        );
        assert_eq!(
            calls[1].params.pointer("/body/read_up_to_server_seq"),
            Some(&json!("30"))
        );
        assert_eq!(
            calls[1].params.pointer("/body/read_up_to_message_id"),
            None,
            "a failed legacy Group task must drop its untrusted local message id"
        );
        drop(calls);
        let connection = rusqlite::Connection::open(fixture.sqlite_path()).unwrap();
        assert_eq!(
            connection
                .query_row(
                    "SELECT status || '|' || COALESCE(
                            json_type(payload_json, '$.read_watermark_message_id'),
                            'missing'
                        )
                     FROM local_mutation_outbox
                     WHERE owner_identity_id = ?1",
                    [binding.owner_identity_id.as_str()],
                    |row| row.get::<_, String>(0),
                )
                .unwrap(),
            "committed|missing"
        );
    }

    #[tokio::test]
    async fn sequence_only_group_read_target_retries_before_becoming_permanent() {
        let fixture = SyncSnapshotFixture::new("read-outbox-missing-group-target");
        let client = fixture.client();
        let binding = client.active_sync_account_binding().await.unwrap();
        seed_sync_snapshot_ready_state(&client, &binding, "1", "10").await;
        let group_did = "did:wba:awiki.info:groups:missing-read-target";
        let local_message_id = format!("{group_did}:30");
        seed_sync_read_group_message(&client, &binding, &local_message_id, None, group_did, 30)
            .await;
        client
            .core_inner()
            .local_state_db()
            .await
            .unwrap()
            .mark_thread_read_watermark(
                binding.owner_identity_id.clone(),
                binding.current_did.clone(),
                crate::internal::local_state::messages::MarkThreadReadWatermarkInput {
                    thread: crate::messages::ThreadRef::Group(
                        crate::ids::GroupRef::parse(group_did).unwrap(),
                    ),
                    read_watermark_message_id: Some(local_message_id),
                    read_watermark_seq: Some("30".to_owned()),
                    read_watermark_at: Some("2026-07-28T12:00:02Z".to_owned()),
                    pending_remote_ack: true,
                },
            )
            .await
            .unwrap();

        let calls = Rc::new(RefCell::new(Vec::new()));
        let outcome = sync_group_read_target_not_found(&client, Rc::clone(&calls), "12").await;
        assert_eq!(outcome.status, crate::messages::MessageSyncStatus::Idle);
        {
            let connection = rusqlite::Connection::open(fixture.sqlite_path()).unwrap();
            assert_eq!(
                connection
                    .query_row(
                        "SELECT outbox.status || '|' || outbox.attempt_count || '|' ||
                                read_state.pending_remote_ack
                         FROM local_mutation_outbox outbox
                         JOIN thread_read_state read_state
                           ON read_state.owner_identity_id = outbox.owner_identity_id
                          AND read_state.thread_id = json_extract(
                              outbox.payload_json,
                              '$.thread_id'
                          )
                         WHERE outbox.owner_identity_id = ?1",
                        [binding.owner_identity_id.as_str()],
                        |row| row.get::<_, String>(0),
                    )
                    .unwrap(),
                "retryable|1|1"
            );
            connection
                .execute(
                    "UPDATE local_mutation_outbox SET retry_at = 0
                     WHERE owner_identity_id = ?1",
                    [binding.owner_identity_id.as_str()],
                )
                .unwrap();
        }

        let outcome = sync_group_read_target_not_found(&client, Rc::clone(&calls), "13").await;
        assert_eq!(outcome.status, crate::messages::MessageSyncStatus::Idle);
        {
            let connection = rusqlite::Connection::open(fixture.sqlite_path()).unwrap();
            assert_eq!(
                connection
                    .query_row(
                        "SELECT status || '|' || attempt_count
                         FROM local_mutation_outbox
                         WHERE owner_identity_id = ?1",
                        [binding.owner_identity_id.as_str()],
                        |row| row.get::<_, String>(0),
                    )
                    .unwrap(),
                "retryable|2"
            );
            connection
                .execute(
                    "UPDATE local_mutation_outbox SET retry_at = 0
                     WHERE owner_identity_id = ?1",
                    [binding.owner_identity_id.as_str()],
                )
                .unwrap();
        }

        let outcome = sync_group_read_target_not_found(&client, Rc::clone(&calls), "14").await;
        assert_eq!(outcome.status, crate::messages::MessageSyncStatus::Idle);
        let calls = calls.borrow();
        assert_eq!(
            calls
                .iter()
                .map(|call| call.method.as_str())
                .collect::<Vec<_>>(),
            [
                "sync.delta",
                "read_state.mark_read",
                "sync.delta",
                "read_state.mark_read",
                "sync.delta",
                "read_state.mark_read",
            ]
        );
        for call in calls
            .iter()
            .filter(|call| call.method == "read_state.mark_read")
        {
            assert_eq!(call.params.pointer("/body/read_up_to_message_id"), None);
        }
        drop(calls);
        let connection = rusqlite::Connection::open(fixture.sqlite_path()).unwrap();
        assert_eq!(
            connection
                .query_row(
                    "SELECT outbox.status || '|' || outbox.attempt_count || '|' ||
                            outbox.last_error_code || '|' || read_state.pending_remote_ack
                     FROM local_mutation_outbox outbox
                     JOIN thread_read_state read_state
                       ON read_state.owner_identity_id = outbox.owner_identity_id
                      AND read_state.thread_id = json_extract(
                          outbox.payload_json,
                          '$.thread_id'
                      )
                     WHERE outbox.owner_identity_id = ?1",
                    [binding.owner_identity_id.as_str()],
                    |row| row.get::<_, String>(0),
                )
                .unwrap(),
            "permanent_failure|3|anp.target_not_found|0"
        );
    }

    #[test]
    fn read_state_event_requires_explicit_thread_kind() {
        let event = crate::internal::wire::sync_v2::SyncEventV2 {
            event_id: "event-read-kind-required".to_owned(),
            stream_epoch: "1".to_owned(),
            event_seq: "1".to_owned(),
            event_type: "message.read_state_updated".to_owned(),
            schema_version: 1,
            ignore_safe: false,
            account_id: "account-1".to_owned(),
            recipient_device_id: None,
            origin_did: Some("did:example:alice".to_owned()),
            origin_device_id: Some("device-1".to_owned()),
            aggregate_kind: "conversation_read_state".to_owned(),
            aggregate_id: "dconv-kind-required".to_owned(),
            state_version: Some("1".to_owned()),
            thread_key: Some("dconv-kind-required".to_owned()),
            occurred_at: "2026-07-28T12:00:00Z".to_owned(),
            payload: json!({
                "thread_key": "dconv-kind-required",
                "state_version": "1",
                "read_up_to_thread_seq": "10"
            }),
            source: None,
        };
        assert!(matches!(
            read_state_from_event(&event),
            Err(crate::ImError::Service {
                code: Some(code),
                ..
            }) if code == "SYNC_INVALID_PAGE"
        ));
    }

    struct BatchTransport {
        responses: VecDeque<Value>,
        calls: Vec<Vec<String>>,
    }

    impl AsyncAuthenticatedRpcTransport for BatchTransport {
        async fn authenticated_rpc(
            &mut self,
            endpoint: &str,
            method: &str,
            params: Value,
        ) -> crate::ImResult<Value> {
            assert_eq!(endpoint, MESSAGE_RPC_ENDPOINT);
            assert_eq!(method, "message.get_batch");
            self.calls.push(
                params
                    .pointer("/body/event_ids")
                    .and_then(Value::as_array)
                    .unwrap()
                    .iter()
                    .map(|value| value.as_str().unwrap().to_owned())
                    .collect(),
            );
            Ok(self.responses.pop_front().expect("queued batch response"))
        }
    }

    #[test]
    fn flat_group_bootstrap_preserves_profile_membership_and_versions() {
        let normalized = normalize_baseline_group(
            &json!({
                "group_did": "did:example:group",
                "host_service_did": "did:example:message-service",
                "creator_did": "did:example:alice",
                "group_state_version": "17",
                "group_event_seq": "23",
                "required_security_profile": "transport-protected",
                "group_profile": {
                    "display_name": "Stage Two",
                    "description": "ordinary group"
                },
                "member_role": "admin",
                "membership_status": "active",
                "member_count": 3,
                "updated_at": "2026-07-28T10:00:00Z"
            }),
            "did:example:alice",
        );
        assert_eq!(
            normalized.pointer("/group/profile/display_name"),
            Some(&json!("Stage Two"))
        );
        assert_eq!(
            normalized.pointer("/group/group_state_version"),
            Some(&json!("17"))
        );
        assert_eq!(
            normalized.pointer("/group/group_event_seq"),
            Some(&json!("23"))
        );
        assert_eq!(
            normalized.pointer("/group/required_security_profile"),
            Some(&json!("transport-protected"))
        );
        assert_eq!(
            normalized.pointer("/membership/subject_did"),
            Some(&json!("did:example:alice"))
        );
        assert_eq!(
            normalized.pointer("/membership/status"),
            Some(&json!("active"))
        );
        assert_eq!(
            normalized.pointer("/membership/role"),
            Some(&json!("admin"))
        );
    }

    #[test]
    fn flat_group_bootstrap_profile_is_persisted_in_group_projection() {
        let fixture = Fixture::new("group-baseline");
        let client = fixture.client();
        let snapshot = crate::internal::wire::sync_v2::parse_snapshot(&json!({
            "mode": "compact_recovery",
            "account_id": "alice-account",
            "device_id": "alice-device",
            "server_time": "2026-07-28T10:00:00Z",
            "snapshot_cursor": {
                "stream_epoch": "2",
                "scan_seq": "23"
            },
            "read_states": [],
            "groups": [{
                "group_did": "did:example:group",
                "host_service_did": "did:example:message-service",
                "creator_did": "did:example:alice",
                "group_state_version": "17",
                "group_event_seq": "23",
                "required_security_profile": "transport-protected",
                "group_profile": {
                    "display_name": "Stage Two",
                    "description": "ordinary group"
                },
                "member_role": "admin",
                "membership_status": "active",
                "member_count": 3,
                "updated_at": "2026-07-28T10:00:00Z"
            }],
            "recent_plain_messages": [],
            "message_policy": {
                "server_cutoff": "2026-07-26T10:00:00Z",
                "max_logical_messages": 500,
                "returned_logical_messages": 0
            },
            "excluded": {
                "e2ee_messages": true,
                "plain_messages_before_cutoff": true
            }
        }))
        .unwrap();
        let group =
            baseline_group_record(&client, &snapshot.groups[0], &snapshot.server_time, 0).unwrap();
        let db = rusqlite::Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        crate::internal::local_state::groups::upsert_group(&db, group).unwrap();
        let persisted = db
            .query_row(
                "SELECT name, description, my_role, membership_status, last_synced_seq
                 FROM groups
                 WHERE owner_identity_id = 'alice-id'
                   AND group_id = 'did:example:group'",
                [],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, i64>(4)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(
            persisted,
            (
                "Stage Two".to_owned(),
                "ordinary group".to_owned(),
                "admin".to_owned(),
                "active".to_owned(),
                23
            )
        );

        let sparse = crate::internal::wire::sync::SyncDeltaEvent {
            event_id: "group-profile-sparse".to_owned(),
            event_seq: "24".to_owned(),
            event_type: "group.profile_updated".to_owned(),
            aggregate_kind: Some("group".to_owned()),
            aggregate_id: Some("did:example:group".to_owned()),
            owner_subject_id: None,
            created_at: Some("2026-07-28T10:01:00Z".to_owned()),
            payload: json!({
                "group": {
                    "group_did": "did:example:group",
                    "group_state_version": "18",
                    "group_event_seq": "24",
                    "profile": {"display_name": "Stage Two Updated"}
                }
            }),
        };
        let sparse_group =
            crate::internal::message_runtime::sync::sync_delta_group_record(&client, &sparse)
                .unwrap();
        crate::internal::local_state::groups::upsert_group(&db, sparse_group).unwrap();
        let metadata: String = db
            .query_row(
                "SELECT metadata FROM groups
                 WHERE owner_identity_id = 'alice-id'
                   AND group_id = 'did:example:group'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let metadata: Value = serde_json::from_str(&metadata).unwrap();
        assert_eq!(metadata["required_security_profile"], "transport-protected");
    }

    #[test]
    fn unrelated_group_events_do_not_reactivate_or_remove_owner_membership() {
        let db = rusqlite::Connection::open_in_memory().unwrap();
        crate::internal::local_state::schema::ensure_schema(&db).unwrap();
        let event = |event_id: &str, group_did: &str, payload: Value| {
            crate::internal::wire::sync::SyncDeltaEvent {
                event_id: event_id.to_owned(),
                event_seq: "1".to_owned(),
                event_type: "group.member_changed".to_owned(),
                aggregate_kind: Some("group".to_owned()),
                aggregate_id: Some(group_did.to_owned()),
                owner_subject_id: None,
                created_at: Some("2026-07-28T10:00:00Z".to_owned()),
                payload,
            }
        };
        let persist = |db: &rusqlite::Connection,
                       event: crate::internal::wire::sync::SyncDeltaEvent| {
            let record = super::super::sync::sync_delta_group_record_for_owner(
                "alice-id",
                "did:example:alice",
                &event,
            )
            .unwrap();
            crate::internal::local_state::groups::upsert_group(db, record).unwrap();
        };

        persist(
            &db,
            event(
                "owner-removed",
                "did:example:removed-group",
                json!({
                    "group": {
                        "group_did": "did:example:removed-group",
                        "group_state_version": "1",
                        "profile": {"display_name": "Removed group"}
                    },
                    "membership": {
                        "subject_did": "did:example:alice",
                        "status": "removed"
                    }
                }),
            ),
        );
        persist(
            &db,
            event(
                "other-member-active",
                "did:example:removed-group",
                json!({
                    "group": {
                        "group_did": "did:example:removed-group",
                        "group_state_version": "2"
                    },
                    "membership": {
                        "subject_did": "did:example:bob",
                        "status": "active"
                    }
                }),
            ),
        );
        persist(
            &db,
            event(
                "profile-update",
                "did:example:removed-group",
                json!({
                    "group": {
                        "group_did": "did:example:removed-group",
                        "group_state_version": "3",
                        "profile": {"display_name": "Renamed"}
                    }
                }),
            ),
        );
        assert_eq!(
            db.query_row(
                "SELECT membership_status FROM groups
                 WHERE owner_identity_id = 'alice-id'
                   AND group_id = 'did:example:removed-group'",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
            "removed"
        );

        persist(
            &db,
            event(
                "initial-profile",
                "did:example:active-group",
                json!({
                    "group": {
                        "group_did": "did:example:active-group",
                        "group_state_version": "1",
                        "profile": {"display_name": "Active group"}
                    }
                }),
            ),
        );
        persist(
            &db,
            event(
                "other-member-removed",
                "did:example:active-group",
                json!({
                    "group": {
                        "group_did": "did:example:active-group",
                        "group_state_version": "2"
                    },
                    "membership": {
                        "subject_did": "did:example:bob",
                        "status": "removed"
                    }
                }),
            ),
        );
        assert_eq!(
            db.query_row(
                "SELECT membership_status FROM groups
                 WHERE owner_identity_id = 'alice-id'
                   AND group_id = 'did:example:active-group'",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
            "active"
        );
    }

    #[test]
    fn unknown_required_v2_event_fails_closed() {
        let fixture = Fixture::new("unknown-required");
        let client = fixture.client();
        let event = crate::internal::wire::sync_v2::SyncEventV2 {
            event_id: "event-1".to_owned(),
            stream_epoch: "1".to_owned(),
            event_seq: "1".to_owned(),
            event_type: "future.required".to_owned(),
            schema_version: 1,
            ignore_safe: false,
            account_id: "account-1".to_owned(),
            recipient_device_id: None,
            origin_did: None,
            origin_device_id: None,
            aggregate_kind: "future".to_owned(),
            aggregate_id: "future-1".to_owned(),
            state_version: None,
            thread_key: None,
            occurred_at: "2026-07-28T10:00:00Z".to_owned(),
            payload: json!({}),
            source: None,
        };
        let error = reduce_event(&client, &event, None, None, &mut BTreeMap::new()).unwrap_err();
        assert!(matches!(
            error,
            crate::ImError::Service {
                code: Some(code),
                ..
            } if code == "SYNC_UNKNOWN_REQUIRED_EVENT"
        ));
    }

    fn system_notification_contract_fixture(
        binding: &crate::identity::ActiveSyncAccountBinding,
    ) -> crate::internal::wire::sync_v2::SyncEventV2 {
        let business_event_id = "evt-system-notification";
        crate::internal::wire::sync_v2::SyncEventV2 {
            event_id: format!(
                "system.notification:{business_event_id}:{}",
                binding.protocol_device_id
            ),
            stream_epoch: "1".to_owned(),
            event_seq: "1".to_owned(),
            event_type: "system.notification".to_owned(),
            schema_version: 1,
            ignore_safe: false,
            account_id: binding.account_id.clone(),
            recipient_device_id: Some(binding.protocol_device_id.clone()),
            origin_did: Some(format!(
                "did:wba:awiki.test:agents:system-notification:e1_{}",
                "A".repeat(43)
            )),
            origin_device_id: None,
            aggregate_kind: "system_notification".to_owned(),
            aggregate_id: business_event_id.to_owned(),
            state_version: None,
            thread_key: None,
            occurred_at: "2026-07-23T02:00:01Z".to_owned(),
            payload: json!({
                "projection_kind": "system_notification",
                "event_id": business_event_id,
                "message_id": business_event_id
            }),
            source: Some(json!({
                "method": "direct.send",
                "operation_id": business_event_id,
                "client_message_id": business_event_id
            })),
        }
    }

    #[tokio::test]
    async fn system_notification_event_contract_is_closed_and_exact_device_scoped() {
        let fixture = SyncSnapshotFixture::new("system-notification-contract");
        let client = fixture.client();
        let binding = client.active_sync_account_binding().await.unwrap();
        let canonical = system_notification_contract_fixture(&binding);
        validate_system_notification_event_contract(&client, &binding, &canonical).unwrap();

        let mut invalid = Vec::new();
        let mut event = canonical.clone();
        event.payload["extra"] = json!(true);
        invalid.push(event);
        let mut event = canonical.clone();
        event.payload["message_id"] = json!("evt-other");
        invalid.push(event);
        let mut event = canonical.clone();
        event.recipient_device_id = None;
        invalid.push(event);
        let mut event = canonical.clone();
        event.aggregate_id = "evt-other".to_owned();
        invalid.push(event);
        let mut event = canonical.clone();
        event.origin_device_id = Some("service-device".to_owned());
        invalid.push(event);
        let mut event = canonical.clone();
        event.source.as_mut().unwrap()["extra"] = json!(true);
        invalid.push(event);
        let mut event = canonical.clone();
        event.source.as_mut().unwrap()["method"] = json!("direct.incoming");
        invalid.push(event);
        let mut event = canonical.clone();
        event.schema_version = 2;
        invalid.push(event);
        let mut event = canonical;
        event.ignore_safe = true;
        invalid.push(event);

        for event in invalid {
            assert!(
                validate_system_notification_event_contract(&client, &binding, &event).is_err()
            );
        }
    }

    #[tokio::test]
    async fn system_notification_origin_uses_message_service_did_for_split_tenant() {
        let fixture = Fixture::new_with_identity_and_service(
            "system-notification-split-tenant",
            "did:wba:tenant.test:user:alice:e1_example",
            "tenant.test",
            Some("did:wba:awiki.test"),
        );
        let client = fixture.client();
        let binding = crate::identity::ActiveSyncAccountBinding {
            owner_identity_id: client.current_identity().id.as_str().to_owned(),
            account_id: "account-1".to_owned(),
            current_did: client.did().as_str().to_owned(),
            protocol_device_id: "device-1".to_owned(),
            identity_generation: "1".to_owned(),
            device_auth_generation: "1".to_owned(),
        };
        let canonical = system_notification_contract_fixture(&binding);
        validate_system_notification_event_contract(&client, &binding, &canonical).unwrap();

        let mut tenant_origin = canonical;
        tenant_origin.origin_did = Some(format!(
            "did:wba:tenant.test:agents:system-notification:e1_{}",
            "A".repeat(43)
        ));
        assert!(
            validate_system_notification_event_contract(&client, &binding, &tenant_origin).is_err()
        );
    }

    #[tokio::test]
    async fn system_notification_reducer_carries_verified_input_without_chat_projection() {
        let fixture = SyncSnapshotFixture::new("system-notification-reducer");
        let client = fixture.client();
        let binding = client.active_sync_account_binding().await.unwrap();
        let event = system_notification_contract_fixture(&binding);
        let fixture_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/multi_device_v1/system-notification-v1.json");
        let source: Value = serde_json::from_slice(&std::fs::read(fixture_path).unwrap()).unwrap();
        let mut request = source["p3_vector"]["request"].clone();
        request["method"] = json!("direct.incoming");
        let verified = crate::internal::system_notification::verify::VerifiedSystemNotification {
            envelope: crate::internal::system_notification::wire::parse_envelope(&request).unwrap(),
            payload_hash: "sha256:payload".to_owned(),
            proof_hash: "sha256:proof".to_owned(),
        };
        let input = crate::internal::system_notification::store::SystemNotificationApplyInput {
            owner_identity_id: binding.owner_identity_id.clone(),
            owner_did: binding.current_did.clone(),
            protocol_device_id: binding.protocol_device_id.clone(),
            verified,
            received_at: chrono::DateTime::parse_from_rfc3339("2026-07-23T02:00:01Z")
                .unwrap()
                .with_timezone(&chrono::Utc),
        };
        let mut public_messages = BTreeMap::new();
        let apply = reduce_event(&client, &event, None, Some(input), &mut public_messages).unwrap();
        assert!(apply.system_notification.is_some());
        assert!(apply.messages.is_empty());
        assert!(apply.groups.is_empty());
        assert!(public_messages.is_empty());
    }

    #[tokio::test]
    async fn malformed_system_notification_wrapper_fails_before_directory_or_apply() {
        let fixture = SyncSnapshotFixture::new("system-notification-wrapper");
        let client = fixture.client();
        let binding = client.active_sync_account_binding().await.unwrap();
        let event = system_notification_contract_fixture(&binding);
        let exact_shape = json!({
            "projection_kind": "system_notification",
            "meta": {},
            "auth": {},
            "body": {}
        });
        let mut with_extra = exact_shape.clone();
        with_extra["id"] = json!("forbidden");
        let mut missing = exact_shape;
        missing.as_object_mut().unwrap().remove("auth");
        for wrapper in [with_extra, missing] {
            let error = prepare_system_notification_at(
                &client,
                &binding,
                &event,
                &wrapper,
                &mut NoopAsyncDirectoryTransport,
                chrono::Utc::now(),
            )
            .await
            .unwrap_err();
            assert!(matches!(
                error,
                crate::ImError::Service { code: Some(code), .. }
                    if code == "SYNC_HYDRATION_INCOMPLETE"
            ));
        }
    }

    struct FixtureDirectoryTransport {
        documents: VecDeque<Value>,
    }

    impl AsyncRpcTransport for FixtureDirectoryTransport {
        async fn rpc(
            &mut self,
            _endpoint: &str,
            _method: &str,
            _params: Value,
        ) -> crate::ImResult<Value> {
            unreachable!("fixture verification only resolves DID documents")
        }

        async fn directory_get_json_url(
            &mut self,
            _url: &str,
            _headers: std::collections::BTreeMap<String, String>,
        ) -> crate::ImResult<Value> {
            self.documents
                .pop_front()
                .ok_or_else(|| crate::ImError::TransportUnavailable {
                    detail: "fixture DID document unavailable".to_owned(),
                })
        }
    }

    #[tokio::test]
    async fn exact_system_notification_wrapper_reaches_full_verification_before_apply() {
        let fixture_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/multi_device_v1/system-notification-v1.json");
        let source: Value = serde_json::from_slice(&std::fs::read(fixture_path).unwrap()).unwrap();
        let target_document_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/multi_device_v1/device-join-rpc-v1.json");
        let target_source: Value =
            serde_json::from_slice(&std::fs::read(target_document_path).unwrap()).unwrap();
        let target_did = source["p3_vector"]["request"]["params"]["meta"]["target"]["did"]
            .as_str()
            .unwrap();
        let origin_did = source["p3_vector"]["request"]["params"]["meta"]["sender_did"]
            .as_str()
            .unwrap();
        let business_event_id = source["p3_vector"]["request"]["params"]["meta"]["message_id"]
            .as_str()
            .unwrap();
        let fixture = Fixture::new_with_identity(
            "system-notification-verified-wrapper",
            target_did,
            "example.com",
        );
        let client = fixture.client();
        let binding = crate::identity::ActiveSyncAccountBinding {
            owner_identity_id: client.current_identity().id.as_str().to_owned(),
            account_id: "account-1".to_owned(),
            current_did: target_did.to_owned(),
            protocol_device_id: "dev-exact".to_owned(),
            identity_generation: "1".to_owned(),
            device_auth_generation: "1".to_owned(),
        };
        let mut event = system_notification_contract_fixture(&binding);
        event.event_id = format!(
            "system.notification:{business_event_id}:{}",
            binding.protocol_device_id
        );
        event.aggregate_id = business_event_id.to_owned();
        event.origin_did = Some(origin_did.to_owned());
        event.payload = json!({
            "projection_kind": "system_notification",
            "event_id": business_event_id,
            "message_id": business_event_id
        });
        event.source = Some(json!({
            "method": "direct.send",
            "operation_id": business_event_id,
            "client_message_id": business_event_id
        }));
        validate_system_notification_event_contract(&client, &binding, &event).unwrap();

        let params = source["p3_vector"]["request"]["params"]
            .as_object()
            .unwrap();
        let wrapper = json!({
            "projection_kind": "system_notification",
            "meta": params["meta"],
            "auth": params["auth"],
            "body": params["body"]
        });
        let documents = || {
            VecDeque::from([
                target_source["approve_vector"]["new_document"].clone(),
                source["p3_vector"]["origin_did_document"].clone(),
            ])
        };
        let received_at = chrono::DateTime::parse_from_rfc3339("2026-07-23T02:00:01Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let error = prepare_system_notification_at(
            &client,
            &binding,
            &event,
            &wrapper,
            &mut FixtureDirectoryTransport {
                documents: documents(),
            },
            received_at,
        )
        .await
        .unwrap_err();
        assert!(matches!(error, crate::ImError::InvalidInput { .. }));
    }

    #[test]
    fn reduce_event_preserves_hydration_state_and_v2_projection_metadata() {
        let fixture = Fixture::new("hydration-state");
        let client = fixture.client();
        let event = crate::internal::wire::sync_v2::SyncEventV2 {
            event_id: "event-discovered".to_owned(),
            stream_epoch: "3".to_owned(),
            event_seq: "17".to_owned(),
            event_type: "message.created".to_owned(),
            schema_version: 1,
            ignore_safe: false,
            account_id: "account-1".to_owned(),
            recipient_device_id: None,
            origin_did: Some("did:example:bob".to_owned()),
            origin_device_id: Some("device-bob".to_owned()),
            aggregate_kind: "direct_message".to_owned(),
            aggregate_id: "message-discovered".to_owned(),
            state_version: None,
            thread_key: Some("remote-thread-bob".to_owned()),
            occurred_at: "2026-07-28T10:00:00Z".to_owned(),
            payload: json!({
                "message_kind": "direct_plain",
                "direction": "incoming",
                "sender_did_snapshot": "did:example:bob",
                "recipient_did_snapshot": "did:example:alice",
                "client_message_id": "client-message-discovered",
                "accepted_at": "2026-07-28T10:00:00.123456Z"
            }),
            source: None,
        };
        let hydrated_message = json!({
            "id": "message-discovered",
            "thread_kind": "direct",
            "sender_did": "did:example:bob",
            "receiver_did": "did:example:alice",
            "content_type": "text/plain",
            "server_seq": "17",
            "created_at": "2026-07-28T09:59:59Z"
        });
        let mut public_messages = BTreeMap::new();

        let apply = reduce_event(
            &client,
            &event,
            Some(&hydrated_message),
            None,
            &mut public_messages,
        )
        .unwrap();

        assert_eq!(apply.messages.len(), 1);
        assert_eq!(
            apply.messages[0].hydration_state,
            crate::internal::local_state::messages::MessageHydrationState::Discovered
        );
        assert_eq!(apply.messages[0].sent_at, "2026-07-28T10:00:00.123456Z");
        assert_eq!(apply.thread_bindings.len(), 1);
        assert_eq!(
            apply.thread_bindings[0].remote_thread_key,
            "remote-thread-bob"
        );
        assert_eq!(apply.thread_bindings[0].thread_kind, "direct");
        assert_eq!(
            serde_json::from_str::<Value>(&apply.messages[0].metadata).unwrap()
                ["remote_thread_key"],
            "remote-thread-bob"
        );
        let message = public_messages.get("event-discovered").unwrap();
        assert_eq!(
            message.sent_at.as_deref(),
            Some("2026-07-28T10:00:00.123456Z")
        );
        assert_eq!(
            message.direction,
            crate::messages::MessageDirection::Incoming
        );
        for (key, value) in [
            ("stream_epoch", "3"),
            ("account_id", "account-1"),
            ("origin_device_id", "device-bob"),
            ("client_message_id", "client-message-discovered"),
            ("remote_thread_key", "remote-thread-bob"),
        ] {
            assert!(message
                .metadata
                .attributes
                .iter()
                .any(|attribute| attribute.key == key && attribute.value == value));
        }
    }

    #[test]
    fn reduce_event_preserves_group_business_message_id_for_read_ack() {
        let fixture = Fixture::new("group-business-message-id");
        let client = fixture.client();
        let group_did = "did:wba:awiki.info:groups:group-read-sync";
        let event = crate::internal::wire::sync_v2::SyncEventV2 {
            event_id: "event-group-22".to_owned(),
            stream_epoch: "3".to_owned(),
            event_seq: "22".to_owned(),
            event_type: "message.created".to_owned(),
            schema_version: 1,
            ignore_safe: false,
            account_id: "account-1".to_owned(),
            recipient_device_id: None,
            origin_did: Some("did:example:bob".to_owned()),
            origin_device_id: Some("device-bob".to_owned()),
            aggregate_kind: "group_message".to_owned(),
            aggregate_id: "business-group-22".to_owned(),
            state_version: None,
            thread_key: Some(group_did.to_owned()),
            occurred_at: "2026-08-12T09:00:00Z".to_owned(),
            payload: json!({
                "message_kind": "group_plain",
                "direction": "incoming",
                "group_did": group_did,
                "sender_did_snapshot": "did:example:bob",
                "recipient_did_snapshot": client.did().as_str(),
                "client_message_id": "business-group-22"
            }),
            source: None,
        };
        let hydrated_message = json!({
            "id": format!("{group_did}:22"),
            "message_id": "business-group-22",
            "thread_kind": "group",
            "group_did": group_did,
            "sender_did": "did:example:bob",
            "content_type": "text/plain",
            "content": "hello",
            "server_seq": "22",
            "created_at": "2026-08-12T09:00:00Z"
        });
        let mut public_messages = BTreeMap::new();

        let apply = reduce_event(
            &client,
            &event,
            Some(&hydrated_message),
            None,
            &mut public_messages,
        )
        .unwrap();

        assert_eq!(apply.messages.len(), 1);
        assert_eq!(apply.messages[0].msg_id, format!("{group_did}:22"));
        assert_eq!(
            serde_json::from_str::<Value>(&apply.messages[0].metadata).unwrap()["raw_message_id"],
            "business-group-22"
        );
        assert!(public_messages["event-group-22"]
            .metadata
            .attributes
            .iter()
            .any(|attribute| {
                attribute.key == "raw_message_id" && attribute.value == "business-group-22"
            }));
    }

    #[test]
    fn exact_hydration_splits_large_delta_pages_for_response_budget() {
        let event_ids = (0..17)
            .map(|index| format!("event-{index}"))
            .collect::<Vec<_>>();
        assert_eq!(
            hydration_event_id_batches(&event_ids)
                .map(<[String]>::len)
                .collect::<Vec<_>>(),
            vec![8, 8, 1]
        );
    }

    #[tokio::test]
    async fn middle_hydration_chunk_unavailable_does_not_advance_cursor() {
        let event_ids = (0..17)
            .map(|index| format!("event-{index}"))
            .collect::<Vec<_>>();
        let complete = |ids: &[String]| {
            json!({
                "items": ids
                    .iter()
                    .map(|event_id| json!({"event_id": event_id, "message": {}}))
                    .collect::<Vec<_>>(),
                "unavailable": []
            })
        };
        let mut transport = BatchTransport {
            responses: VecDeque::from([
                complete(&event_ids[..8]),
                json!({
                    "items": [{
                        "event_id": event_ids[8],
                        "message": {}
                    }],
                    "unavailable": event_ids[9..16]
                }),
            ]),
            calls: Vec::new(),
        };

        let db = rusqlite::Connection::open_in_memory().unwrap();
        crate::internal::local_state::sync_v2::create_schema(&db).unwrap();
        let binding = crate::internal::local_state::sync_v2::IdentityAccountBinding {
            owner_identity_id: "alice-id".to_owned(),
            account_id: "account-1".to_owned(),
            handle_scope: None,
            current_did: "did:example:alice".to_owned(),
            protocol_device_id: "device-1".to_owned(),
            identity_generation: "1".to_owned(),
            device_auth_generation: "1".to_owned(),
            created_at: 1,
            updated_at: 1,
        };
        crate::internal::local_state::sync_v2::upsert_identity_account_binding(&db, &binding)
            .unwrap();
        let state = crate::internal::local_state::sync_v2::MessageSyncState {
            owner_identity_id: binding.owner_identity_id.clone(),
            account_id: binding.account_id.clone(),
            protocol_device_id: binding.protocol_device_id.clone(),
            device_auth_generation: binding.device_auth_generation.clone(),
            stream_epoch: "1".to_owned(),
            scan_seq: "40".to_owned(),
            bootstrap_state: "active".to_owned(),
            last_server_time: None,
            last_success_at: Some(1),
            last_error_code: None,
            metadata_json: None,
            updated_at: 1,
        };
        crate::internal::local_state::sync_v2::bootstrap_message_sync_state(&db, &state).unwrap();

        let error = hydrate_required_messages(
            &mut transport,
            &crate::internal::wire::common::WireIdentity {
                did: binding.current_did.clone(),
            },
            &event_ids,
        )
        .await
        .unwrap_err();
        assert!(matches!(
            error,
            crate::ImError::Service {
                code: Some(code),
                ..
            } if code == "SYNC_HYDRATION_INCOMPLETE"
        ));
        assert_eq!(
            transport.calls,
            vec![event_ids[..8].to_vec(), event_ids[8..16].to_vec()]
        );
        let crate::internal::local_state::sync_v2::MessageSyncStateAccess::Ready(current) =
            crate::internal::local_state::sync_v2::load_message_sync_state(
                &db,
                &binding.owner_identity_id,
            )
            .unwrap()
        else {
            panic!("failed hydration must preserve the ready cursor");
        };
        assert_eq!(current.scan_seq, "40");
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM sync_applied_events", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
            0
        );
    }

    #[tokio::test]
    async fn malformed_system_notification_batch_never_calls_legacy_or_advances_cursor() {
        let fixture = SyncSnapshotFixture::new("system-notification-fail-closed");
        let client = fixture.client();
        let binding = client.active_sync_account_binding().await.unwrap();
        seed_sync_snapshot_ready_state(&client, &binding, "1", "10").await;
        let business_event_id = "evt-system-notification";
        let sync_event_id = format!(
            "system.notification:{business_event_id}:{}",
            binding.protocol_device_id
        );
        let event = json!({
            "event_id": sync_event_id.clone(),
            "stream_epoch": "1",
            "event_seq": "11",
            "event_type": "system.notification",
            "schema_version": 1,
            "ignore_safe": false,
            "account_id": binding.account_id.clone(),
            "recipient_device_id": binding.protocol_device_id.clone(),
            "origin_did": format!(
                "did:wba:awiki.test:agents:system-notification:e1_{}",
                "A".repeat(43)
            ),
            "origin_device_id": null,
            "aggregate_kind": "system_notification",
            "aggregate_id": business_event_id,
            "state_version": null,
            "thread_key": null,
            "occurred_at": "2026-07-23T02:00:01Z",
            "payload": {
                "projection_kind": "system_notification",
                "event_id": business_event_id,
                "message_id": business_event_id
            },
            "source": {
                "method": "direct.send",
                "operation_id": business_event_id,
                "client_message_id": business_event_id
            }
        });
        let calls = Rc::new(RefCell::new(Vec::new()));
        let error = MessageSyncRuntimeV2::new(
            &client,
            ReadySyncSnapshotSessionProvider,
            SyncSnapshotTransport::queued(
                Rc::clone(&calls),
                vec![
                    Ok(sync_snapshot_delta("1", "11", vec![event])),
                    Ok(json!({
                        "items": [{
                            "event_id": sync_event_id,
                            "message": {
                                "projection_kind": "system_notification",
                                "meta": {},
                                "auth": {},
                                "body": {},
                                "id": "forbidden-top-level-field"
                            }
                        }],
                        "unavailable": []
                    })),
                ],
            ),
            NoopAsyncDirectoryTransport,
        )
        .sync_now(sync_snapshot_request())
        .await
        .unwrap_err();
        assert!(matches!(
            error,
            crate::ImError::Service { code: Some(code), .. }
                if code == "SYNC_HYDRATION_INCOMPLETE"
        ));
        let state = load_sync_snapshot_state(&client, &binding.owner_identity_id).await;
        assert_eq!(state.scan_seq, "10");
        let db = rusqlite::Connection::open(fixture.sqlite_path()).unwrap();
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM sync_applied_events", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
            0
        );
        assert_eq!(
            db.query_row(
                "SELECT COUNT(*) FROM system_notification_receipts",
                [],
                |row| { row.get::<_, i64>(0) }
            )
            .unwrap(),
            0
        );
        assert_eq!(
            calls
                .borrow()
                .iter()
                .map(|call| call.method.as_str())
                .collect::<Vec<_>>(),
            ["sync.delta", "message.get_batch"]
        );
    }
}
