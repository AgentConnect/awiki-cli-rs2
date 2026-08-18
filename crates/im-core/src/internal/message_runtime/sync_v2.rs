use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex as StdMutex, OnceLock};

use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

use crate::internal::auth::session::AsyncSessionProvider;
use crate::internal::message_runtime::read::MESSAGE_RPC_ENDPOINT;
use crate::internal::transport::{AsyncAuthenticatedRpcTransport, AsyncRpcTransport};

const PENDING_PERSONA_RESOLUTION_LIMIT: u32 = 32;
const GROUP_SEQUENCE_ONLY_TARGET_NOT_FOUND_MAX_ATTEMPTS: i64 = 3;

pub(crate) struct MessageSyncRuntimeV2<'a, P, T, R> {
    client: &'a crate::core::ImClient,
    session_provider: P,
    transport: T,
    directory_transport: R,
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
    use crate::internal::local_state::sync_v2::{SyncLaneEventReceipt, SyncLaneEventReceiptMatch};
    use crate::internal::realtime::notification::InlineSyncLaneV3;
    use crate::internal::wire::sync_v2::SyncLaneV3;

    let context = client.sync_account_context()?;
    let owner_identity_id = client.current_identity().id.as_str().to_owned();
    let db = client.core_inner().local_state_db().await?;
    let lane = match inline.lane {
        InlineSyncLaneV3::Ordinary => unreachable!("ordinary inline events use the v2 reducer"),
        InlineSyncLaneV3::P5Device => SyncLaneV3::P5Device,
        InlineSyncLaneV3::P6Group => SyncLaneV3::P6Group,
    };
    let Some(lane_state) = db
        .load_lane_sync_states(owner_identity_id.clone())
        .await?
        .into_iter()
        .find(|state| state.lane == lane)
    else {
        return Ok(RealtimeInlineMessageApplyOutcome::Deferred);
    };
    if lane_state.stream_epoch != inline.stream_epoch {
        return Ok(RealtimeInlineMessageApplyOutcome::Deferred);
    }
    let receipt = SyncLaneEventReceipt {
        owner_identity_id,
        lane,
        event_id: inline.event_id.clone(),
        stream_epoch: inline.stream_epoch.clone(),
        event_seq: inline.event_seq.clone(),
        group_did: inline.group_did.clone(),
        group_event_seq: inline.group_event_seq.clone(),
        applied_at: unix_time_i64(),
    };
    match db.match_sync_lane_event_receipt(receipt.clone()).await? {
        SyncLaneEventReceiptMatch::Exact | SyncLaneEventReceiptMatch::Conflict => {
            return Ok(RealtimeInlineMessageApplyOutcome::Deferred);
        }
        SyncLaneEventReceiptMatch::Missing => {}
    }

    let message = match inline.lane {
        InlineSyncLaneV3::P5Device => {
            match apply_p5_lane_projection_async(
                client,
                &inline.event_id,
                &inline.projection,
                directory_transport,
            )
            .await?
            {
                P5LaneProjectionOutcome::Projected(message) => Some(message),
                P5LaneProjectionOutcome::TerminalControl => None,
                P5LaneProjectionOutcome::ReplayWithoutReceipt
                | P5LaneProjectionOutcome::Deferred => {
                    return Ok(RealtimeInlineMessageApplyOutcome::Deferred);
                }
            }
        }
        InlineSyncLaneV3::P6Group => {
            validate_p6_inline_binding(&inline)?;
            match apply_p6_lane_delivery_projection_async(client, &inline.projection).await {
                Ok(message) => Some(message),
                Err(error) => {
                    eprintln!("P6 realtime fast-path deferred after projection: {error}");
                    return Ok(RealtimeInlineMessageApplyOutcome::Deferred);
                }
            }
        }
        InlineSyncLaneV3::Ordinary => unreachable!("ordinary lane was handled above"),
    };
    // The receipt proves idempotent local application, but deliberately does
    // not carry a checkpoint. sync.delta remains the sole authoritative ACK.
    db.commit_sync_lane_event(receipt, None, false).await?;
    if let Some(message) = message {
        client.emit_committed_local_message_projection("sync_v3_e2ee_realtime_fast_path");
        Ok(RealtimeInlineMessageApplyOutcome::Applied {
            message,
            local_scan_seq: None,
        })
    } else {
        let _ = context;
        Ok(RealtimeInlineMessageApplyOutcome::Deferred)
    }
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

#[cfg(feature = "sqlite")]
enum P5LaneProjectionOutcome {
    Projected(crate::messages::Message),
    TerminalControl,
    ReplayWithoutReceipt,
    Deferred,
}

#[cfg(feature = "sqlite")]
async fn apply_p5_lane_projection_async<R>(
    client: &crate::core::ImClient,
    expected_delivery_id: &str,
    envelope: &Value,
    directory_transport: &mut R,
) -> crate::ImResult<P5LaneProjectionOutcome>
where
    R: AsyncRpcTransport,
{
    let raw_message_id = envelope
        .pointer("/meta/message_id")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty() && value.trim() == *value)
        .ok_or_else(|| {
            sync_error(
                "SYNC_INVALID_PAGE",
                "P5 lane envelope requires canonical meta.message_id",
            )
        })?
        .to_owned();
    if raw_message_id != expected_delivery_id {
        return Err(sync_error(
            "SYNC_INVALID_PAGE",
            "P5 delivery id conflicts with envelope meta.message_id",
        ));
    }
    let mut raw = json!({
        "messages": [envelope.clone()],
        "has_more": false,
        "warnings": []
    });
    let mut provenance = super::read::project_secure_direct_messages_from_reliable_sync_async(
        client,
        &mut raw,
        directory_transport,
    )
    .await;
    match provenance.disposition_for_raw_message_id(&raw_message_id) {
        super::read::DirectP5ProjectionDisposition::Projected => {
            super::read::annotate_direct_peer_scopes_async(
                client,
                &mut raw,
                directory_transport,
                None,
                None,
                Some(&mut provenance),
            )
            .await;
            let page = super::read::page_from_raw(client, &raw, crate::ids::PageLimit::new(1)?)?;
            let [message] = page.items.as_slice() else {
                return Err(sync_error(
                    "SYNC_P5_APPLICATION_INCOMPLETE",
                    "P5 delivery did not produce one durable message projection",
                ));
            };
            let outcome = super::read::persist_projection_async(
                client,
                std::slice::from_ref(message),
                &provenance,
            )
            .await?;
            if outcome
                .stored_messages
                .saturating_add(outcome.backlogged_messages)
                != 1
            {
                return Err(sync_error(
                    "SYNC_P5_APPLICATION_INCOMPLETE",
                    "P5 delivery was not durably stored or backlogged",
                ));
            }
            Ok(P5LaneProjectionOutcome::Projected(message.clone()))
        }
        super::read::DirectP5ProjectionDisposition::TerminalControl => {
            Ok(P5LaneProjectionOutcome::TerminalControl)
        }
        super::read::DirectP5ProjectionDisposition::Replay => {
            Ok(P5LaneProjectionOutcome::ReplayWithoutReceipt)
        }
        super::read::DirectP5ProjectionDisposition::NotConsumed => {
            Ok(P5LaneProjectionOutcome::Deferred)
        }
    }
}

#[cfg(all(feature = "sqlite", feature = "group-e2ee"))]
async fn apply_p6_lane_delivery_projection_async(
    client: &crate::core::ImClient,
    envelope: &Value,
) -> crate::ImResult<crate::messages::Message> {
    let mut projected = p6_projection_for_application(envelope)?;
    crate::internal::message_runtime::read::apply_cached_group_e2ee_messages_async(
        client,
        std::slice::from_mut(&mut projected),
    )
    .await;
    if projected.get("decryption_state").and_then(Value::as_str) != Some("decrypted") {
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

#[cfg(feature = "sqlite")]
fn validate_p6_inline_binding(
    inline: &crate::internal::realtime::notification::InlineSyncEventV3,
) -> crate::ImResult<()> {
    let body_group_did = inline
        .projection
        .pointer("/body/group_did")
        .and_then(Value::as_str);
    let body_group_event_seq =
        inline
            .projection
            .pointer("/body/group_event_seq")
            .and_then(|value| {
                value
                    .as_str()
                    .map(ToOwned::to_owned)
                    .or_else(|| value.as_u64().map(|value| value.to_string()))
            });
    if body_group_did != inline.group_did.as_deref()
        || body_group_event_seq.as_deref() != inline.group_event_seq.as_deref()
    {
        return Err(sync_error(
            "SYNC_INVALID_PAGE",
            "P6 inline event position conflicts with its envelope",
        ));
    }
    Ok(())
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
        }
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
        let mut device_epoch_refresh_attempted = false;
        loop {
            match self.sync_now_once(&request).await {
                Err(error)
                    if !device_epoch_refresh_attempted && is_device_epoch_rejection(&error) =>
                {
                    device_epoch_refresh_attempted = true;
                    self.refresh_session_and_lane_epoch().await?;
                }
                Ok((mut outcome, Some(_error))) if !device_epoch_refresh_attempted => {
                    self.refresh_session_and_lane_epoch().await?;
                    let binding = self.client.active_sync_account_binding().await?;
                    let owner_lock = owner_sync_lock(&binding.owner_identity_id);
                    let _owner_guard = owner_lock.lock().await;
                    let db = self.client.core_inner().local_state_db().await?;
                    match self.drain_read_outbox(&db, &binding).await {
                        Ok(None) => return Ok(outcome),
                        Ok(Some(error)) => return Err(error),
                        Err(_) => {
                            outcome
                                .warnings
                                .push("sync.read_state_writeback_deferred".to_owned());
                            return Ok(outcome);
                        }
                    }
                }
                Ok((_outcome, Some(error))) => return Err(error),
                Ok((outcome, None)) => return Ok(outcome),
                Err(error) => return Err(error),
            }
        }
    }

    async fn refresh_session_and_lane_epoch(&mut self) -> crate::ImResult<()> {
        self.session_provider.refresh_session().await?;
        self.transport.reload_authentication_state()?;
        let binding = self.client.active_sync_account_binding().await?;
        let owner_lock = owner_sync_lock(&binding.owner_identity_id);
        let _owner_guard = owner_lock.lock().await;
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
        let owner_lock = owner_sync_lock(&binding.owner_identity_id);
        let _owner_guard = owner_lock.lock().await;
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
                self.bootstrap(&db, &binding, &mut result).await?
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
        self.retry_p6_lane_blockers(&db, &binding, &lane_states, &mut result)
            .await;

        let mut recovery_token_retries = 0_u8;
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
                        .recover_snapshot(&db, &binding, &state, &recovery, &mut result)
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
                                Some("SYNC_RECOVERY_TOKEN_INVALID" | "SYNC_RECOVERY_TOKEN_EXPIRED")
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
                let (hydrated, _) = hydrate_required_messages(
                    &mut self.transport,
                    &wire_identity(self.client),
                    &hydration_event_ids,
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
            let outcome = db
                .apply_sync_delta_v2(crate::internal::local_state::sync_v2::DeltaApplyInputV2 {
                    owner_identity_id: binding.owner_identity_id.clone(),
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
            let lane_has_more = self
                .apply_lane_delta_sections(
                    &db,
                    &binding,
                    &page.lanes,
                    &requested_lanes,
                    &mut lane_states,
                    &mut blocked_lanes,
                    &mut p5_lane_recovery_attempted,
                    &mut result,
                )
                .await?;
            state.scan_seq = page.next_cursor.scan_seq;
            state.stream_epoch = page.next_cursor.stream_epoch;
            state.bootstrap_state = "active".to_owned();
            state.last_server_time = Some(page.server_time);
            state.last_success_at = Some(unix_time_i64());
            if page.has_more && state.scan_seq == cursor.scan_seq {
                return Err(sync_error(
                    "SYNC_INVALID_PAGE",
                    "sync.delta returned has_more without cursor progress",
                ));
            }
            if !page.has_more && !lane_has_more {
                self.retry_p6_lane_blockers(&db, &binding, &lane_states, &mut result)
                    .await;
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
        sections: &BTreeMap<
            crate::internal::wire::sync_v2::SyncLaneV3,
            crate::internal::wire::sync_v2::SyncLaneDeltaSectionV3,
        >,
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
        use crate::internal::wire::sync_v2::{SyncLaneDeltaSectionV3, SyncLaneV3};

        if requested.is_empty() {
            if !sections.is_empty() {
                return Err(sync_error(
                    "SYNC_INVALID_PAGE",
                    "sync.delta returned unrequested lane sections",
                ));
            }
            return Ok(false);
        }
        if requested.keys().any(|lane| !sections.contains_key(lane))
            || sections.keys().any(|lane| !requested.contains_key(lane))
        {
            return Err(sync_error(
                "SYNC_INVALID_PAGE",
                "sync.delta lane sections do not match the requested lanes",
            ));
        }
        let mut any_has_more = false;
        for lane in [SyncLaneV3::P5Device, SyncLaneV3::P6Group] {
            let Some(section) = sections.get(&lane) else {
                continue;
            };
            match section {
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
                    let completed = match lane {
                        SyncLaneV3::P5Device => {
                            self.apply_p5_lane_events(
                                db,
                                binding,
                                events,
                                lane_states,
                                blocked_lanes,
                                result,
                            )
                            .await?
                        }
                        SyncLaneV3::P6Group => {
                            self.apply_p6_lane_events(
                                db,
                                binding,
                                events,
                                lane_states,
                                blocked_lanes,
                                result,
                            )
                            .await?
                        }
                    };
                    if completed {
                        let previous_scan_seq = lane_states
                            .get(&lane)
                            .map(|state| state.scan_seq.clone())
                            .ok_or_else(|| {
                                sync_error(
                                    "SYNC_LANE_BOOTSTRAP_REQUIRED",
                                    "lane state disappeared during application",
                                )
                            })?;
                        let next = crate::internal::local_state::sync_v2::LaneSyncState {
                            owner_identity_id: binding.owner_identity_id.clone(),
                            lane,
                            stream_epoch: next_cursor.stream_epoch.clone(),
                            scan_seq: next_cursor.scan_seq.clone(),
                            committed_seq: next_cursor.scan_seq.clone(),
                        };
                        db.advance_lane_sync_state(next.clone()).await?;
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
        }
        Ok(any_has_more)
    }

    async fn apply_p5_lane_events(
        &mut self,
        db: &crate::internal::local_state::actor::LocalStateDb,
        binding: &crate::identity::ActiveSyncAccountBinding,
        events: &[crate::internal::wire::sync_v2::SyncLaneEventV3],
        lane_states: &mut BTreeMap<
            crate::internal::wire::sync_v2::SyncLaneV3,
            crate::internal::local_state::sync_v2::LaneSyncState,
        >,
        blocked_lanes: &mut BTreeSet<crate::internal::wire::sync_v2::SyncLaneV3>,
        result: &mut crate::messages::MessageSyncOutcome,
    ) -> crate::ImResult<bool> {
        use crate::internal::local_state::sync_v2::SyncLaneEventReceiptMatch;
        use crate::internal::wire::sync_v2::{SyncLaneEventV3, SyncLaneV3};

        for event in events {
            let SyncLaneEventV3::P5Delivery {
                delivery_id,
                seq,
                envelope,
            } = event
            else {
                return Err(sync_error(
                    "SYNC_INVALID_PAGE",
                    "P5 lane contains a non-P5 event",
                ));
            };
            let state = lane_states
                .get(&SyncLaneV3::P5Device)
                .cloned()
                .ok_or_else(|| {
                    sync_error("SYNC_LANE_BOOTSTRAP_REQUIRED", "P5 lane state is missing")
                })?;
            let receipt = lane_event_receipt(
                binding,
                SyncLaneV3::P5Device,
                delivery_id,
                &state.stream_epoch,
                seq,
                None,
                None,
            );
            let receipt_match = db.match_sync_lane_event_receipt(receipt.clone()).await?;
            if receipt_match == SyncLaneEventReceiptMatch::Conflict {
                blocked_lanes.insert(SyncLaneV3::P5Device);
                result
                    .warnings
                    .push("sync.lane.p5_device.receipt_conflict".to_owned());
                return Ok(false);
            }
            let next = lane_state_at_event(&state, seq);
            if receipt_match == SyncLaneEventReceiptMatch::Exact {
                db.commit_sync_lane_event(receipt, Some(next.clone()), false)
                    .await?;
                lane_states.insert(SyncLaneV3::P5Device, next);
                result.duplicates_skipped = result.duplicates_skipped.saturating_add(1);
                continue;
            }
            let projection = apply_p5_lane_projection_async(
                self.client,
                delivery_id,
                envelope,
                &mut self.directory_transport,
            )
            .await;
            let projected_message = match projection {
                Ok(P5LaneProjectionOutcome::Projected(message)) => Some(message),
                Ok(P5LaneProjectionOutcome::TerminalControl) => None,
                Ok(
                    P5LaneProjectionOutcome::ReplayWithoutReceipt
                    | P5LaneProjectionOutcome::Deferred,
                )
                | Err(_) => {
                    blocked_lanes.insert(SyncLaneV3::P5Device);
                    result
                        .warnings
                        .push("sync.lane.p5_device.deferred".to_owned());
                    return Ok(false);
                }
            };
            db.commit_sync_lane_event(receipt, Some(next.clone()), false)
                .await?;
            lane_states.insert(SyncLaneV3::P5Device, next);
            result.events_applied = result.events_applied.saturating_add(1);
            if let Some(message) = projected_message {
                result.messages_hydrated = result.messages_hydrated.saturating_add(1);
                result
                    .changed_conversation_ids
                    .push(message_conversation_id(&message, binding));
                if message.direction == crate::messages::MessageDirection::Incoming {
                    result.committed_incoming_messages.push(
                        crate::messages::CommittedIncomingMessage {
                            event_id: delivery_id.clone(),
                            logical_message_id: message.id.as_str().to_owned(),
                            source: "live_delta_p5_device".to_owned(),
                            direction: crate::messages::MessageDirection::Incoming,
                            message,
                        },
                    );
                }
                self.client
                    .emit_committed_local_message_projection("sync_v3_p5_device_delta");
            }
        }
        Ok(true)
    }

    async fn apply_p6_lane_events(
        &mut self,
        db: &crate::internal::local_state::actor::LocalStateDb,
        binding: &crate::identity::ActiveSyncAccountBinding,
        events: &[crate::internal::wire::sync_v2::SyncLaneEventV3],
        lane_states: &mut BTreeMap<
            crate::internal::wire::sync_v2::SyncLaneV3,
            crate::internal::local_state::sync_v2::LaneSyncState,
        >,
        blocked_lanes: &mut BTreeSet<crate::internal::wire::sync_v2::SyncLaneV3>,
        result: &mut crate::messages::MessageSyncOutcome,
    ) -> crate::ImResult<bool> {
        use crate::internal::local_state::sync_v2::SyncLaneEventReceiptMatch;
        use crate::internal::wire::sync_v2::{SyncLaneEventV3, SyncLaneV3};

        for event in events {
            let state = lane_states
                .get(&SyncLaneV3::P6Group)
                .cloned()
                .ok_or_else(|| {
                    sync_error("SYNC_LANE_BOOTSTRAP_REQUIRED", "P6 lane state is missing")
                })?;
            let (event_id, seq, group_did, group_event_seq, event_type, payload) = match event {
                SyncLaneEventV3::P6Delivery {
                    delivery_id,
                    seq,
                    group_did,
                    group_event_seq,
                    envelope,
                } => (
                    delivery_id,
                    seq,
                    group_did,
                    Some(group_event_seq.as_str()),
                    "p6.delivery.created",
                    envelope,
                ),
                SyncLaneEventV3::P6ControlNotice {
                    notice_id,
                    seq,
                    group_did,
                    notice,
                } => (notice_id, seq, group_did, None, "p6.control.notice", notice),
                SyncLaneEventV3::P5Delivery { .. } => {
                    return Err(sync_error(
                        "SYNC_INVALID_PAGE",
                        "P6 lane contains a P5 event",
                    ));
                }
            };
            let receipt = lane_event_receipt(
                binding,
                SyncLaneV3::P6Group,
                event_id,
                &state.stream_epoch,
                seq,
                Some(group_did.clone()),
                group_event_seq.map(ToOwned::to_owned),
            );
            let receipt_match = db.match_sync_lane_event_receipt(receipt.clone()).await?;
            if receipt_match == SyncLaneEventReceiptMatch::Conflict {
                blocked_lanes.insert(SyncLaneV3::P6Group);
                result
                    .warnings
                    .push("sync.lane.p6_group.receipt_conflict".to_owned());
                return Ok(false);
            }
            let next = lane_state_at_event(&state, seq);
            if receipt_match == SyncLaneEventReceiptMatch::Exact {
                db.commit_sync_lane_event(receipt, Some(next.clone()), true)
                    .await?;
                lane_states.insert(SyncLaneV3::P6Group, next);
                result.duplicates_skipped = result.duplicates_skipped.saturating_add(1);
                continue;
            }
            if validate_strict_p6_lane_wire(event).is_err() {
                blocked_lanes.insert(SyncLaneV3::P6Group);
                result
                    .warnings
                    .push(format!("sync.lane.p6_group.nonconformant:{event_id}"));
                return Ok(false);
            }
            let projected = match event {
                SyncLaneEventV3::P6Delivery {
                    group_did,
                    group_event_seq,
                    envelope,
                    ..
                } => {
                    validate_p6_delta_binding(group_did, group_event_seq, envelope)
                        .and_then(|_| Ok(()))?;
                    apply_p6_lane_delivery_projection_async(self.client, envelope)
                        .await
                        .map(Some)
                }
                SyncLaneEventV3::P6ControlNotice { notice, .. } => {
                    super::read::consume_group_e2ee_control_notice_from_reliable_sync_async(
                        self.client,
                        notice,
                    )
                    .await
                    .map(|_| None)
                }
                SyncLaneEventV3::P5Delivery { .. } => unreachable!(),
            };
            match projected {
                Ok(message) => {
                    db.commit_sync_lane_event(receipt, Some(next.clone()), true)
                        .await?;
                    lane_states.insert(SyncLaneV3::P6Group, next);
                    result.events_applied = result.events_applied.saturating_add(1);
                    if let Some(message) = message {
                        result.messages_hydrated = result.messages_hydrated.saturating_add(1);
                        result
                            .changed_conversation_ids
                            .push(message_conversation_id(&message, binding));
                        if message.direction == crate::messages::MessageDirection::Incoming {
                            result.committed_incoming_messages.push(
                                crate::messages::CommittedIncomingMessage {
                                    event_id: event_id.clone(),
                                    logical_message_id: message.id.as_str().to_owned(),
                                    source: "live_delta_p6_group".to_owned(),
                                    direction: crate::messages::MessageDirection::Incoming,
                                    message,
                                },
                            );
                        }
                        self.client
                            .emit_committed_local_message_projection("sync_v3_p6_group_delta");
                    }
                }
                Err(error) => {
                    let now = unix_time_i64();
                    db.record_p6_lane_blocker_and_advance(
                        crate::internal::local_state::sync_v2::P6LaneBlocker {
                            owner_identity_id: binding.owner_identity_id.clone(),
                            event_id: event_id.clone(),
                            stream_epoch: state.stream_epoch.clone(),
                            event_seq: seq.clone(),
                            event_type: event_type.to_owned(),
                            group_did: group_did.clone(),
                            group_event_seq: group_event_seq.map(ToOwned::to_owned),
                            payload_json: serde_json::to_string(payload).map_err(|error| {
                                sync_error(
                                    "SYNC_INVALID_PAGE",
                                    format!("serialize P6 blocker payload: {error}"),
                                )
                            })?,
                            attempt_count: 1,
                            last_error_code: error_code(&error)
                                .unwrap_or("SYNC_P6_DEFERRED")
                                .to_owned(),
                            created_at: now,
                            updated_at: now,
                        },
                        next.clone(),
                    )
                    .await?;
                    lane_states.insert(SyncLaneV3::P6Group, next);
                    result
                        .warnings
                        .push("sync.lane.p6_group.deferred".to_owned());
                }
            }
        }
        Ok(true)
    }

    async fn retry_p6_lane_blockers(
        &mut self,
        db: &crate::internal::local_state::actor::LocalStateDb,
        binding: &crate::identity::ActiveSyncAccountBinding,
        lane_states: &BTreeMap<
            crate::internal::wire::sync_v2::SyncLaneV3,
            crate::internal::local_state::sync_v2::LaneSyncState,
        >,
        result: &mut crate::messages::MessageSyncOutcome,
    ) {
        use crate::internal::wire::sync_v2::SyncLaneV3;

        let Some(current) = lane_states.get(&SyncLaneV3::P6Group) else {
            return;
        };
        let Ok(blockers) = db
            .list_p6_lane_blockers(binding.owner_identity_id.clone(), 100)
            .await
        else {
            result
                .warnings
                .push("sync.lane.p6_group.backlog_unavailable".to_owned());
            return;
        };
        for blocker in blockers {
            if blocker.stream_epoch != current.stream_epoch {
                result
                    .warnings
                    .push("sync.lane.p6_group.backlog_epoch_mismatch".to_owned());
                continue;
            }
            let payload = match serde_json::from_str::<Value>(&blocker.payload_json) {
                Ok(payload) => payload,
                Err(_) => continue,
            };
            let projected = match blocker.event_type.as_str() {
                "p6.delivery.created" => {
                    if let Some(group_event_seq) = blocker.group_event_seq.as_deref() {
                        if validate_p6_delta_binding(&blocker.group_did, group_event_seq, &payload)
                            .is_err()
                        {
                            continue;
                        }
                    }
                    apply_p6_lane_delivery_projection_async(self.client, &payload)
                        .await
                        .map(Some)
                }
                "p6.control.notice" => {
                    let notice = legacy_p6_notice_storage_adapter(&payload, &blocker.event_id);
                    super::read::consume_group_e2ee_control_notice_from_reliable_sync_async(
                        self.client,
                        &notice,
                    )
                    .await
                    .map(|_| None)
                }
                _ => continue,
            };
            let receipt = lane_event_receipt(
                binding,
                SyncLaneV3::P6Group,
                &blocker.event_id,
                &blocker.stream_epoch,
                &blocker.event_seq,
                Some(blocker.group_did.clone()),
                blocker.group_event_seq.clone(),
            );
            match projected {
                Ok(message) => {
                    if db
                        .commit_sync_lane_event(receipt, None, true)
                        .await
                        .is_err()
                    {
                        continue;
                    }
                    result.events_applied = result.events_applied.saturating_add(1);
                    if let Some(message) = message {
                        result.messages_hydrated = result.messages_hydrated.saturating_add(1);
                        result
                            .changed_conversation_ids
                            .push(message_conversation_id(&message, binding));
                        if message.direction == crate::messages::MessageDirection::Incoming {
                            result.committed_incoming_messages.push(
                                crate::messages::CommittedIncomingMessage {
                                    event_id: blocker.event_id.clone(),
                                    logical_message_id: message.id.as_str().to_owned(),
                                    source: "p6_group_backlog".to_owned(),
                                    direction: crate::messages::MessageDirection::Incoming,
                                    message,
                                },
                            );
                        }
                        self.client
                            .emit_committed_local_message_projection("sync_v3_p6_group_backlog");
                    }
                }
                Err(error) => {
                    let now = unix_time_i64();
                    let mut retry = blocker;
                    retry.last_error_code =
                        error_code(&error).unwrap_or("SYNC_P6_DEFERRED").to_owned();
                    retry.updated_at = now;
                    let _ = db
                        .record_p6_lane_blocker_and_advance(retry, current.clone())
                        .await;
                }
            }
        }
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
        result: &mut crate::messages::MessageSyncOutcome,
    ) -> crate::ImResult<crate::internal::local_state::sync_v2::MessageSyncState> {
        let client_instance_id = db
            .load_or_create_sync_client_instance_id(&binding.owner_identity_id)
            .await?;
        let params = crate::internal::wire::sync_v2::build_bootstrap_params(
            &wire_identity(self.client),
            &client_instance_id,
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
                || p6_delivery_client_instance_id != &client_instance_id
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
            let state = self
                .recover_snapshot(db, binding, &previous, recovery, result)
                .await?;
            db.replace_lane_sync_states(
                &binding.owner_identity_id,
                lane_states_from_bootstrap(&binding.owner_identity_id, lane_bootstrap),
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
            || bootstrap.p6_delivery_client_instance_id != client_instance_id
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
                groups,
                read_states,
                lane_states,
            },
        )
        .await?;
        Ok(state)
    }
}

fn legacy_p6_notice_storage_adapter(payload: &Value, stable_event_id: &str) -> Value {
    if payload
        .pointer("/body/notice_id")
        .and_then(Value::as_str)
        .is_some_and(|value| !value.is_empty())
    {
        return payload.clone();
    }
    let mut adapted = payload.clone();
    if let Some(body) = adapted.get_mut("body").and_then(Value::as_object_mut) {
        body.insert(
            "notice_id".to_owned(),
            Value::String(stable_event_id.to_owned()),
        );
    }
    if let Some(meta) = adapted.get_mut("meta").and_then(Value::as_object_mut) {
        meta.entry("operation_id".to_owned())
            .or_insert_with(|| Value::String(stable_event_id.to_owned()));
    }
    adapted
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
    let client_instance_id = db
        .load_or_create_sync_client_instance_id(&binding.owner_identity_id)
        .await?;
    let params = crate::internal::wire::sync_v2::build_bootstrap_params(
        &wire_identity(client),
        &client_instance_id,
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
            bootstrap.p6_delivery_client_instance_id.as_str(),
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
            p6_delivery_client_instance_id.as_str(),
        ),
    };
    if account_id != binding.account_id
        || device_id != binding.protocol_device_id
        || activated_client_instance_id != client_instance_id
    {
        return Err(sync_error(
            "SYNC_ACCOUNT_BINDING_MISMATCH",
            "lane bootstrap does not match the active account device",
        ));
    }
    let states = lane_states_from_bootstrap(&binding.owner_identity_id, lane_bootstrap);
    db.replace_lane_sync_states(&binding.owner_identity_id, states.clone())
        .await?;
    Ok(lane_state_map(states))
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
    let expected_origin_prefix = format!(
        "did:wba:{}:agents:system-notification:e1_",
        client.did_domain()
    );
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
    let mut hydrated = BTreeMap::new();
    let mut count = 0_u32;
    for event_ids in hydration_event_id_batches(message_event_ids) {
        let params =
            crate::internal::wire::sync_v2::build_message_get_batch_params(identity, event_ids)?;
        let raw = transport
            .authenticated_rpc(MESSAGE_RPC_ENDPOINT, "message.get_batch", params)
            .await?;
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
        crate::ImError::Service { code, .. }
            if matches!(
                code.as_deref(),
                Some(
                    "1401"
                        | "anp.device_not_eligible"
                        | "anp.device_state_changed"
                        | "SYNC_ACCOUNT_BINDING_MISMATCH"
                        | "SYNC_DEVICE_BINDING_MISMATCH"
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

fn owner_sync_lock(owner_identity_id: &str) -> Arc<tokio::sync::Mutex<()>> {
    static LOCKS: OnceLock<StdMutex<BTreeMap<String, Arc<tokio::sync::Mutex<()>>>>> =
        OnceLock::new();
    let mut locks = LOCKS
        .get_or_init(|| StdMutex::new(BTreeMap::new()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    locks
        .entry(owner_identity_id.to_owned())
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
    }

    impl Fixture {
        fn new(prefix: &str) -> Self {
            Self::new_with_identity(prefix, "did:example:alice", "awiki.test")
        }

        fn new_with_identity(prefix: &str, did: &str, did_domain: &str) -> Self {
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
                    anp_service_did: None,
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
        json!({
            "mode": "compact_recovery_required",
            "account_id": binding.account_id,
            "device_id": binding.protocol_device_id,
            "p6_delivery": {
                "profile": crate::internal::wire::sync_v2::P6_DELIVERY_CONTEXT_CAPABILITY_V1,
                "client_instance_id": client_instance_id,
                "activated": true
            },
            "recovery": {
                "recovery_id": recovery_id,
                "token": token,
                "stream_epoch": stream_epoch,
                "snapshot_scan_seq": snapshot_scan_seq,
                "message_cutoff": "2026-07-26T12:00:03Z",
                "message_limit": 500,
                "expires_at": "2026-07-28T12:10:03Z"
            }
        })
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

    fn accounted_p6_lane_event(
        delivery_id: &str,
        seq: &str,
        group_did: &str,
        group_event_seq: &str,
    ) -> Value {
        json!({
            "event_type": "p6.delivery.created",
            "delivery_id": delivery_id,
            "seq": seq,
            "group_did": group_did,
            "group_event_seq": group_event_seq,
            "envelope": {
                "meta": {
                    "profile": "anp.group.e2ee.v2",
                    "security_profile": "group-e2ee"
                },
                "auth": {},
                "body": {
                    "group_did": group_did,
                    "group_event_seq": group_event_seq
                }
            }
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

    #[test]
    fn legacy_notice_storage_adapter_uses_the_immutable_event_id() {
        let legacy = json!({
            "meta": {"profile": "anp.group.e2ee.v2"},
            "body": {"notice_type": "commit-delivery"}
        });
        let first = legacy_p6_notice_storage_adapter(&legacy, "notice-row-1");
        let replay = legacy_p6_notice_storage_adapter(&legacy, "notice-row-1");
        assert_eq!(first, replay);
        assert_eq!(first["body"]["notice_id"], "notice-row-1");
        assert_eq!(first["meta"]["operation_id"], "notice-row-1");
        assert!(legacy.pointer("/body/notice_id").is_none());
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
        client
            .core_inner()
            .local_state_db()
            .await
            .unwrap()
            .replace_lane_sync_states(&binding.owner_identity_id, Vec::new())
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
        client
            .core_inner()
            .local_state_db()
            .await
            .unwrap()
            .replace_lane_sync_states(
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
    async fn upgraded_client_negotiates_lane_capabilities_before_first_delta() {
        use crate::internal::wire::sync_v2::{SyncLaneV3, SYNC_CAPABILITY_P5_DEVICE_V1};

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
        let bootstrap = json!({
            "mode": "tail_only",
            "account_id": binding.account_id,
            "device_id": binding.protocol_device_id,
            "server_time": "2026-08-15T00:00:00Z",
            "cursor": {"stream_epoch": "1", "scan_seq": "10"},
            "read_state_baseline": [],
            "group_state_baseline": [],
            "warnings": [],
            "p6_delivery": {
                "profile": crate::internal::wire::sync_v2::P6_DELIVERY_CONTEXT_CAPABILITY_V1,
                "client_instance_id": client_instance_id,
                "activated": true
            },
            "sync_capabilities": [SYNC_CAPABILITY_P5_DEVICE_V1],
            "lanes": {
                "p5_device": {
                    "cursor": {"stream_epoch": "41", "scan_seq": "0"},
                    "committed_seq": "0"
                }
            }
        });
        let delta = sync_snapshot_delta_with_lanes(
            "1",
            "10",
            json!({
                "p5_device": {
                    "events": [],
                    "next_cursor": {"stream_epoch": "41", "scan_seq": "0"},
                    "has_more": false
                }
            }),
        );
        let calls = Rc::new(RefCell::new(Vec::new()));

        let outcome = MessageSyncRuntimeV2::new(
            &client,
            ReadySyncSnapshotSessionProvider,
            SyncSnapshotTransport::queued(Rc::clone(&calls), vec![Ok(bootstrap), Ok(delta)]),
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
            ["sync.bootstrap", "sync.delta"]
        );
        assert_eq!(
            calls[1]
                .params
                .pointer("/body/lanes/p5_device/cursor/stream_epoch"),
            Some(&json!("41"))
        );
        drop(calls);
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
                .unwrap()[0]
                .lane,
            SyncLaneV3::P5Device
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

    #[tokio::test]
    async fn poison_p5_lane_stops_only_p5_while_ordinary_and_p6_advance() {
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
        let p6_group_did = "did:wba:awiki.test:groups:p6-accounted";
        client
            .core_inner()
            .local_state_db()
            .await
            .unwrap()
            .commit_sync_lane_event(
                lane_event_receipt(
                    &binding,
                    SyncLaneV3::P6Group,
                    "p6-accounted-1",
                    "42",
                    "1",
                    Some(p6_group_did.to_owned()),
                    Some("1".to_owned()),
                ),
                None,
                false,
            )
            .await
            .unwrap();
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
                    "events": [accounted_p6_lane_event(
                        "p6-accounted-1",
                        "1",
                        p6_group_did,
                        "1"
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
        assert!(outcome
            .warnings
            .contains(&"sync.lane.p5_device.deferred".to_owned()));
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
        assert_eq!(lanes[&SyncLaneV3::P5Device], "0");
        assert_eq!(lanes[&SyncLaneV3::P6Group], "1");
        {
            let calls = calls.borrow();
            let request = &calls[0].params;
            assert!(request.pointer("/body/lanes/p5_device").is_some());
            assert!(request.pointer("/body/lanes/p6_group").is_some());
        }

        let mut retry_response = response;
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
        assert!(retry
            .warnings
            .contains(&"sync.lane.p5_device.deferred".to_owned()));
        let calls = calls.borrow();
        assert_eq!(
            calls[1].params["body"]["lanes"]["p5_device"]["cursor"]["scan_seq"],
            "0"
        );
        assert_eq!(
            calls[1].params["body"]["lanes"]["p6_group"]["cursor"]["scan_seq"],
            "1"
        );
    }

    #[tokio::test]
    async fn inline_p5_receipt_is_accounted_by_delta_without_second_crypto_application() {
        use crate::internal::wire::sync_v2::SyncLaneV3;

        let fixture = SyncSnapshotFixture::new("p5-inline-then-delta");
        let client = fixture.client();
        let binding = client.active_sync_account_binding().await.unwrap();
        seed_sync_snapshot_ready_state(&client, &binding, "1", "10").await;
        seed_lane_states(&client, &binding, &[(SyncLaneV3::P5Device, "41")]).await;
        let receipt = lane_event_receipt(
            &binding,
            SyncLaneV3::P5Device,
            "p5-inline-1",
            "41",
            "1",
            None,
            None,
        );
        client
            .core_inner()
            .local_state_db()
            .await
            .unwrap()
            .commit_sync_lane_event(receipt, None, false)
            .await
            .unwrap();
        let response = sync_snapshot_delta_with_lanes(
            "1",
            "10",
            json!({
                "p5_device": {
                    "events": [poison_p5_lane_event("p5-inline-1", "1")],
                    "next_cursor": {"stream_epoch": "41", "scan_seq": "1"},
                    "has_more": false
                }
            }),
        );
        let outcome = MessageSyncRuntimeV2::new(
            &client,
            ReadySyncSnapshotSessionProvider,
            SyncSnapshotTransport::queued(Rc::new(RefCell::new(Vec::new())), vec![Ok(response)]),
            NoopAsyncDirectoryTransport,
        )
        .sync_now(sync_snapshot_request())
        .await
        .unwrap();
        assert_eq!(outcome.duplicates_skipped, 1);
        let db = client.core_inner().local_state_db().await.unwrap();
        assert_eq!(
            db.load_lane_sync_states(binding.owner_identity_id.clone())
                .await
                .unwrap()[0]
                .scan_seq,
            "1"
        );
        db.shutdown().await.unwrap();
        let connection = rusqlite::Connection::open(fixture.sqlite_path()).unwrap();
        assert_eq!(
            connection
                .query_row("SELECT COUNT(*) FROM sync_lane_applied_events", [], |row| {
                    row.get::<_, i64>(0)
                },)
                .unwrap(),
            1
        );
        assert_eq!(
            connection
                .query_row("SELECT COUNT(*) FROM sync_applied_events", [], |row| {
                    row.get::<_, i64>(0)
                })
                .unwrap(),
            0
        );
    }

    #[tokio::test]
    async fn real_p5_delivery_is_exactly_once_for_inline_then_delta_and_reverse_order() {
        use crate::internal::secure_direct::v2_product::v2_product_tests::{
            prepare_runtime_p5_test_wires, RuntimeP5TestBody, RuntimeP5TestClientFixture,
            RuntimeP5TestWire,
        };
        use crate::internal::wire::sync_v2::SyncLaneV3;

        const PEER_DID: &str = "did:example:sync-lane-bob";
        for inline_first in [true, false] {
            let fixture = RuntimeP5TestClientFixture::new(if inline_first {
                "lane-inline-first"
            } else {
                "lane-delta-first"
            });
            let client = fixture.client();
            let binding = client.active_sync_account_binding().await.unwrap();
            seed_sync_snapshot_ready_state(&client, &binding, "1", "10").await;
            seed_lane_states(&client, &binding, &[(SyncLaneV3::P5Device, "41")]).await;
            let envelope = prepare_runtime_p5_test_wires(
                &client,
                vec![RuntimeP5TestWire {
                    peer_did: PEER_DID,
                    peer_device_id: if inline_first {
                        "lane-bob-inline"
                    } else {
                        "lane-bob-delta"
                    },
                    seed: if inline_first { 201 } else { 211 },
                    logical_message_id: if inline_first {
                        "logical-lane-inline-first"
                    } else {
                        "logical-lane-delta-first"
                    },
                    server_seq: 1,
                    body: RuntimeP5TestBody::Text(if inline_first {
                        "lane inline first plaintext"
                    } else {
                        "lane delta first plaintext"
                    }),
                }],
            )
            .await
            .pop()
            .unwrap();
            let delivery_id = envelope["meta"]["message_id"].as_str().unwrap().to_owned();
            let notification = realtime_inline_p5_notification(&delivery_id, "41", "1", &envelope);
            let delta = sync_snapshot_delta_with_lanes(
                "1",
                "10",
                json!({
                    "p5_device": {
                        "events": [p5_lane_event(&delivery_id, "1", &envelope)],
                        "next_cursor": {"stream_epoch": "41", "scan_seq": "1"},
                        "has_more": false
                    }
                }),
            );

            if inline_first {
                let inline = crate::internal::realtime::notification::parse_inline_sync_event_v3(
                    &notification,
                )
                .unwrap()
                .unwrap();
                let applied = apply_realtime_e2ee_lane_v3_with_directory_async(
                    &client,
                    inline,
                    &mut DirectLookupTransport {
                        expected_did: PEER_DID.to_owned(),
                        calls: Rc::new(RefCell::new(0)),
                    },
                )
                .await
                .unwrap();
                assert!(matches!(
                    applied,
                    RealtimeInlineMessageApplyOutcome::Applied {
                        local_scan_seq: None,
                        ..
                    }
                ));
                assert_eq!(
                    client
                        .core_inner()
                        .local_state_db()
                        .await
                        .unwrap()
                        .load_lane_sync_states(binding.owner_identity_id.clone())
                        .await
                        .unwrap()[0]
                        .scan_seq,
                    "0",
                    "inline must not advance the P5 checkpoint"
                );
            }

            let outcome = MessageSyncRuntimeV2::new(
                &client,
                ReadySyncSnapshotSessionProvider,
                SyncSnapshotTransport::queued(Rc::new(RefCell::new(Vec::new())), vec![Ok(delta)]),
                DirectLookupTransport {
                    expected_did: PEER_DID.to_owned(),
                    calls: Rc::new(RefCell::new(0)),
                },
            )
            .sync_now(sync_snapshot_request())
            .await
            .unwrap();
            assert_eq!(
                client
                    .core_inner()
                    .local_state_db()
                    .await
                    .unwrap()
                    .load_lane_sync_states(binding.owner_identity_id.clone())
                    .await
                    .unwrap()[0]
                    .scan_seq,
                "1"
            );
            if inline_first {
                assert_eq!(outcome.duplicates_skipped, 1);
            } else {
                let before_inline = client
                    .core_inner()
                    .local_state_db()
                    .await
                    .unwrap()
                    .load_lane_sync_states(binding.owner_identity_id.clone())
                    .await
                    .unwrap()[0]
                    .clone();
                let inline = crate::internal::realtime::notification::parse_inline_sync_event_v3(
                    &notification,
                )
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
                assert_eq!(
                    client
                        .core_inner()
                        .local_state_db()
                        .await
                        .unwrap()
                        .load_lane_sync_states(binding.owner_identity_id.clone())
                        .await
                        .unwrap()[0],
                    before_inline,
                    "delta-first inline replay must not move the P5 checkpoint"
                );
            }

            let path = fixture.sqlite_path();
            assert_eq!(
                sqlite_count(&path, "SELECT COUNT(*) FROM direct_e2ee_v2_replay"),
                1,
                "the inbound ratchet/replay transaction commits exactly once"
            );
            assert_eq!(
                sqlite_count(
                    &path,
                    "SELECT COUNT(*) FROM sync_lane_applied_events WHERE lane = 'p5_device'"
                ),
                1
            );
            assert_eq!(
                sqlite_count(&path, "SELECT COUNT(*) FROM messages WHERE is_e2ee = 1"),
                1
            );
        }
    }

    #[tokio::test]
    async fn failed_p5_inline_does_not_pollute_ratchet_and_delta_retry_succeeds() {
        use crate::internal::secure_direct::v2_product::v2_product_tests::{
            prepare_runtime_p5_test_wires, RuntimeP5TestBody, RuntimeP5TestClientFixture,
            RuntimeP5TestWire,
        };
        use crate::internal::wire::sync_v2::SyncLaneV3;

        const PEER_DID: &str = "did:example:sync-lane-retry-bob";
        let fixture = RuntimeP5TestClientFixture::new("lane-inline-retry");
        let client = fixture.client();
        let binding = client.active_sync_account_binding().await.unwrap();
        seed_sync_snapshot_ready_state(&client, &binding, "1", "10").await;
        seed_lane_states(&client, &binding, &[(SyncLaneV3::P5Device, "41")]).await;
        let envelope = prepare_runtime_p5_test_wires(
            &client,
            vec![RuntimeP5TestWire {
                peer_did: PEER_DID,
                peer_device_id: "lane-retry-bob-device",
                seed: 221,
                logical_message_id: "logical-lane-inline-retry",
                server_seq: 1,
                body: RuntimeP5TestBody::Text("delta recovers rejected inline ciphertext"),
            }],
        )
        .await
        .pop()
        .unwrap();
        let delivery_id = envelope["meta"]["message_id"].as_str().unwrap().to_owned();
        let mut tampered = envelope.clone();
        tampered["body"]["ciphertext_b64u"] = json!("QU5PVEhFUi1WQUxJRC1DSVBIRVJURVhU");
        let notification = realtime_inline_p5_notification(&delivery_id, "41", "1", &tampered);
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
        assert_eq!(
            client
                .core_inner()
                .local_state_db()
                .await
                .unwrap()
                .load_lane_sync_states(binding.owner_identity_id.clone())
                .await
                .unwrap()[0]
                .scan_seq,
            "0"
        );
        assert_eq!(
            sqlite_count(
                &fixture.sqlite_path(),
                "SELECT COUNT(*) FROM direct_e2ee_v2_replay"
            ),
            0,
            "rejected inline ciphertext must not commit ratchet/replay state"
        );

        let outcome = MessageSyncRuntimeV2::new(
            &client,
            ReadySyncSnapshotSessionProvider,
            SyncSnapshotTransport::queued(
                Rc::new(RefCell::new(Vec::new())),
                vec![Ok(sync_snapshot_delta_with_lanes(
                    "1",
                    "10",
                    json!({
                        "p5_device": {
                            "events": [p5_lane_event(&delivery_id, "1", &envelope)],
                            "next_cursor": {"stream_epoch": "41", "scan_seq": "1"},
                            "has_more": false
                        }
                    }),
                ))],
            ),
            DirectLookupTransport {
                expected_did: PEER_DID.to_owned(),
                calls: Rc::new(RefCell::new(0)),
            },
        )
        .sync_now(sync_snapshot_request())
        .await
        .unwrap();
        assert_ne!(
            outcome.status,
            crate::messages::MessageSyncStatus::AuthRevoked
        );
        assert_eq!(
            client
                .core_inner()
                .local_state_db()
                .await
                .unwrap()
                .load_lane_sync_states(binding.owner_identity_id.clone())
                .await
                .unwrap()[0]
                .scan_seq,
            "1"
        );
        assert_eq!(
            sqlite_count(
                &fixture.sqlite_path(),
                "SELECT COUNT(*) FROM direct_e2ee_v2_replay"
            ),
            1
        );
        assert_eq!(
            sqlite_count(
                &fixture.sqlite_path(),
                "SELECT COUNT(*) FROM messages WHERE content = 'delta recovers rejected inline ciphertext'"
            ),
            1
        );
    }

    #[cfg(feature = "group-e2ee")]
    #[tokio::test]
    async fn p6_out_of_order_inline_defers_then_delta_uses_durable_plaintext_cache() {
        use crate::internal::wire::sync_v2::SyncLaneV3;

        let fixture = SyncSnapshotFixture::new("p6-inline-deferred-delta");
        let client = fixture.client();
        let binding = client.active_sync_account_binding().await.unwrap();
        seed_sync_snapshot_ready_state(&client, &binding, "1", "10").await;
        seed_lane_states(&client, &binding, &[(SyncLaneV3::P6Group, "42")]).await;
        let group_did = "did:wba:awiki.test:groups:p6-inline-delta";
        let delivery_id = "p6-lane-delivery-1";
        let envelope = p6_lane_envelope(&binding, "p6-wire-message-1", group_did, "7");
        let notification =
            realtime_inline_p6_notification(delivery_id, "42", "1", group_did, "7", &envelope);
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
        assert_eq!(
            client
                .core_inner()
                .local_state_db()
                .await
                .unwrap()
                .load_lane_sync_states(binding.owner_identity_id.clone())
                .await
                .unwrap()[0]
                .scan_seq,
            "0",
            "a deferred P6 inline event must not advance its checkpoint"
        );
        assert_eq!(
            sqlite_count(
                &fixture.sqlite_path(),
                "SELECT COUNT(*) FROM sync_lane_applied_events WHERE lane = 'p6_group'"
            ),
            0
        );

        seed_cached_p6_plaintext(
            &client,
            &binding,
            group_did,
            "7",
            "cached P6 plaintext after prerequisite repair",
        )
        .await;
        let outcome = MessageSyncRuntimeV2::new(
            &client,
            ReadySyncSnapshotSessionProvider,
            SyncSnapshotTransport::queued(
                Rc::new(RefCell::new(Vec::new())),
                vec![Ok(sync_snapshot_delta_with_lanes(
                    "1",
                    "10",
                    json!({
                        "p6_group": {
                            "events": [p6_lane_event(
                                delivery_id,
                                "1",
                                group_did,
                                "7",
                                &envelope
                            )],
                            "next_cursor": {"stream_epoch": "42", "scan_seq": "1"},
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
        assert_ne!(
            outcome.status,
            crate::messages::MessageSyncStatus::AuthRevoked
        );
        assert_eq!(
            client
                .core_inner()
                .local_state_db()
                .await
                .unwrap()
                .load_lane_sync_states(binding.owner_identity_id.clone())
                .await
                .unwrap()[0]
                .scan_seq,
            "1"
        );
        assert_eq!(
            sqlite_count(
                &fixture.sqlite_path(),
                "SELECT COUNT(*) FROM sync_lane_applied_events WHERE lane = 'p6_group'"
            ),
            1
        );
        assert_eq!(
            sqlite_count(
                &fixture.sqlite_path(),
                "SELECT COUNT(*) FROM p6_lane_blockers"
            ),
            0
        );

        let second_delivery_id = "p6-lane-delivery-2";
        let second_envelope = p6_lane_envelope(&binding, "p6-wire-message-2", group_did, "8");
        seed_cached_p6_plaintext(
            &client,
            &binding,
            group_did,
            "8",
            "cached P6 inline plaintext",
        )
        .await;
        let second_notification = realtime_inline_p6_notification(
            second_delivery_id,
            "42",
            "2",
            group_did,
            "8",
            &second_envelope,
        );
        let inline = crate::internal::realtime::notification::parse_inline_sync_event_v3(
            &second_notification,
        )
        .unwrap()
        .unwrap();
        assert!(matches!(
            apply_realtime_e2ee_lane_v3_with_directory_async(
                &client,
                inline,
                &mut NoopAsyncDirectoryTransport,
            )
            .await
            .unwrap(),
            RealtimeInlineMessageApplyOutcome::Applied {
                local_scan_seq: None,
                ..
            }
        ));
        assert_eq!(
            client
                .core_inner()
                .local_state_db()
                .await
                .unwrap()
                .load_lane_sync_states(binding.owner_identity_id.clone())
                .await
                .unwrap()[0]
                .scan_seq,
            "1",
            "a successfully applied P6 inline event still must not advance its checkpoint"
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
            ["sync.bootstrap", "sync.snapshot", "sync.delta"]
        );
        assert!(calls[0]
            .params
            .pointer("/body/client_instance_id")
            .and_then(Value::as_str)
            .is_some());
        assert_eq!(
            calls[2].params.pointer("/body/cursor"),
            Some(&json!({"stream_epoch": "3", "scan_seq": "40"}))
        );
        assert!(calls[2]
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
                        "SYNC_RECOVERY_TOKEN_INVALID",
                        "recovery token was invalid",
                    )),
                    Ok(sync_snapshot_recovery(
                        "recovery-token-expired",
                        "RAW_SYNC_TOKEN_EXPIRED",
                        "2",
                        "20",
                    )),
                    Err(sync_error(
                        "SYNC_RECOVERY_TOKEN_EXPIRED",
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
            } if code == "SYNC_RECOVERY_TOKEN_EXPIRED"
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
                "read_state.mark_read"
            ]
        );
    }

    #[tokio::test]
    async fn device_epoch_refresh_revalidates_p5_lane_epoch_before_retry() {
        use crate::internal::wire::sync_v2::{SyncLaneV3, SYNC_CAPABILITY_P5_DEVICE_V1};

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
        let lane_bootstrap = json!({
            "mode": "tail_only",
            "account_id": binding.account_id,
            "device_id": binding.protocol_device_id,
            "server_time": "2026-08-15T00:00:00Z",
            "cursor": {"stream_epoch": "1", "scan_seq": "10"},
            "read_state_baseline": [],
            "group_state_baseline": [],
            "warnings": [],
            "p6_delivery": {
                "profile": crate::internal::wire::sync_v2::P6_DELIVERY_CONTEXT_CAPABILITY_V1,
                "client_instance_id": client_instance_id,
                "activated": true
            },
            "sync_capabilities": [SYNC_CAPABILITY_P5_DEVICE_V1],
            "lanes": {
                "p5_device": {
                    "cursor": {"stream_epoch": "51", "scan_seq": "0"},
                    "committed_seq": "0"
                }
            }
        });
        let retry_delta = sync_snapshot_delta_with_lanes(
            "1",
            "10",
            json!({
                "p5_device": {
                    "events": [],
                    "next_cursor": {"stream_epoch": "51", "scan_seq": "0"},
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
                    vec![Err(rejected), Ok(lane_bootstrap), Ok(retry_delta)],
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
        let calls = calls.borrow();
        assert_eq!(
            calls
                .iter()
                .map(|call| call.method.as_str())
                .collect::<Vec<_>>(),
            ["sync.delta", "sync.bootstrap", "sync.delta"]
        );
        assert_eq!(
            calls[0]
                .params
                .pointer("/body/lanes/p5_device/cursor/stream_epoch"),
            Some(&json!("41"))
        );
        assert_eq!(
            calls[2]
                .params
                .pointer("/body/lanes/p5_device/cursor/stream_epoch"),
            Some(&json!("51"))
        );
        drop(calls);
        let lane = client
            .core_inner()
            .local_state_db()
            .await
            .unwrap()
            .load_lane_sync_states(binding.owner_identity_id)
            .await
            .unwrap()
            .pop()
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
        let error = MessageSyncRuntimeV2::new(
            &client,
            RefreshingSyncSnapshotSessionProvider {
                refresh_calls: Rc::clone(&refresh_calls),
                fail_refresh: false,
            },
            ReloadingSyncSnapshotTransport {
                inner: SyncSnapshotTransport::queued(
                    Rc::clone(&calls),
                    vec![Err(rejected()), Err(rejected())],
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
            ["sync.delta", "sync.delta"]
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
