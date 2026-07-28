use std::collections::BTreeMap;

use serde_json::{json, Map, Value};

use crate::internal::auth::session::AsyncSessionProvider;
use crate::internal::message_runtime::read::MESSAGE_RPC_ENDPOINT;
use crate::internal::transport::AsyncAuthenticatedRpcTransport;

pub(crate) struct MessageSyncRuntimeV2<'a, P, T> {
    client: &'a crate::core::ImClient,
    session_provider: P,
    transport: T,
}

impl<'a, P, T> MessageSyncRuntimeV2<'a, P, T> {
    pub(crate) fn new(
        client: &'a crate::core::ImClient,
        session_provider: P,
        transport: T,
    ) -> Self {
        Self {
            client,
            session_provider,
            transport,
        }
    }
}

impl<P, T> MessageSyncRuntimeV2<'_, P, T>
where
    P: AsyncSessionProvider,
    T: AsyncAuthenticatedRpcTransport,
{
    pub(crate) async fn sync_now(
        mut self,
        request: crate::messages::MessageSyncRequest,
    ) -> crate::ImResult<crate::messages::MessageSyncOutcome> {
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
        let mut state = match db
            .load_message_sync_state(owner_identity_id.clone())
            .await?
        {
            crate::internal::local_state::sync_v2::MessageSyncStateAccess::Ready(state) => state,
            crate::internal::local_state::sync_v2::MessageSyncStateAccess::BootstrapRequired(_) => {
                self.bootstrap(&db, &binding).await?
            }
        };

        let mut result = empty_outcome();
        loop {
            let cursor = crate::internal::wire::sync_v2::SyncCursorV2 {
                stream_epoch: state.stream_epoch.clone(),
                scan_seq: state.scan_seq.clone(),
            };
            let params = crate::internal::wire::sync_v2::build_delta_params(
                &wire_identity(self.client),
                &cursor,
                limit,
                reason,
            )?;
            let raw = match self
                .transport
                .authenticated_rpc(MESSAGE_RPC_ENDPOINT, "sync.delta", params)
                .await
            {
                Err(error) if error_code(&error) == Some("SYNC_RECOVERY_REQUIRED") => {
                    result.status = crate::messages::MessageSyncStatus::RecoveryRequired;
                    result.error_code = Some("SYNC_RECOVERY_REQUIRED".to_owned());
                    return Ok(result);
                }
                other => other?,
            };
            let page = crate::internal::wire::sync_v2::parse_delta(&raw)?;
            validate_page_binding(&page, &binding, &state)?;
            result.pages_fetched = result.pages_fetched.saturating_add(1);
            result.warnings.extend(page.warnings.clone());

            let message_event_ids = page
                .events
                .iter()
                .filter(|event| event.event_type == "message.created")
                .map(|event| event.event_id.clone())
                .collect::<Vec<_>>();
            let hydrated = if message_event_ids.is_empty() {
                BTreeMap::new()
            } else {
                let (hydrated, count) = hydrate_required_messages(
                    &mut self.transport,
                    &wire_identity(self.client),
                    &message_event_ids,
                )
                .await?;
                result.messages_hydrated = result.messages_hydrated.saturating_add(count);
                hydrated
            };

            let mut public_messages = BTreeMap::new();
            let apply_events = page
                .events
                .iter()
                .map(|event| {
                    reduce_event(
                        self.client,
                        event,
                        hydrated.get(&event.event_id),
                        &mut public_messages,
                    )
                })
                .collect::<crate::ImResult<Vec<_>>>()?;
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
            result.events_applied = result
                .events_applied
                .saturating_add(u32::try_from(outcome.applied_event_ids.len()).unwrap_or(u32::MAX));
            result.duplicates_skipped = result
                .duplicates_skipped
                .saturating_add(u32::try_from(outcome.duplicate_events).unwrap_or(u32::MAX));
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
            super::sync::emit_committed_sync_invalidation(self.client, &outcome.invalidation);
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
            if !page.has_more {
                result.changed_conversation_ids.sort();
                result.changed_conversation_ids.dedup();
                result.status =
                    if result.events_applied == 0 && result.changed_conversation_ids.is_empty() {
                        crate::messages::MessageSyncStatus::Idle
                    } else {
                        crate::messages::MessageSyncStatus::Changed
                    };
                return Ok(result);
            }
        }
    }

    async fn bootstrap(
        &mut self,
        db: &crate::internal::local_state::actor::LocalStateDb,
        binding: &crate::identity::ActiveSyncAccountBinding,
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
        let bootstrap = crate::internal::wire::sync_v2::parse_bootstrap(&raw)?;
        if bootstrap.account_id != binding.account_id
            || bootstrap.device_id != binding.protocol_device_id
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
        let now = unix_time_i64();
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
            },
        )
        .await?;
        Ok(state)
    }
}

fn reduce_event(
    client: &crate::core::ImClient,
    event: &crate::internal::wire::sync_v2::SyncEventV2,
    hydrated_message: Option<&Value>,
    public_messages: &mut BTreeMap<String, crate::messages::Message>,
) -> crate::ImResult<crate::internal::local_state::sync_v2::DeltaApplyEventV2> {
    if matches!(
        event.event_type.as_str(),
        "message.created" | "group.member_changed" | "group.profile_updated"
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
            let synthetic = hydrated_v1_event(event, hydrated_message)?;
            let mut message =
                super::sync::sync_delta_message_from_payload(client, &synthetic, true)?;
            message.direction = event_direction(event)?;
            add_v2_metadata(&mut message, event);
            let record = super::local_projection::message_record_from_message(client, &message)?;
            apply.messages.push(record);
            public_messages.insert(event.event_id.clone(), message);
        }
        "group.member_changed" | "group.profile_updated" => {
            validate_group_state_version(event)?;
            let synthetic = v1_event(event, event.payload.clone());
            apply
                .groups
                .push(super::sync::sync_delta_group_record(client, &synthetic)?);
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
    normalize_hydrated_message(&mut message);
    payload.insert("message".to_owned(), Value::Object(message.clone()));
    if !payload.contains_key("thread") {
        payload.insert(
            "thread".to_owned(),
            Value::Object(thread_for_event(event, &message)?),
        );
    }
    Ok(v1_event(event, Value::Object(payload)))
}

fn normalize_hydrated_message(message: &mut Map<String, Value>) {
    if !message.contains_key("accepted_at") {
        if let Some(created_at) = message.get("created_at").cloned() {
            message.insert("accepted_at".to_owned(), created_at);
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
    for field in ["group_did", "group_state_version", "group_event_seq"] {
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
    if let Some(role) = object.get("role").or_else(|| object.get("membership_role")) {
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

pub(crate) fn failure_outcome(
    error: &crate::ImError,
) -> Option<crate::messages::MessageSyncOutcome> {
    let (status, code) = match error {
        crate::ImError::AuthRequired
        | crate::ImError::SessionExpired
        | crate::ImError::PermissionDenied => (
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
    Some(crate::messages::MessageSyncOutcome {
        status,
        error_code: Some(code),
        ..empty_outcome()
    })
}

fn error_code(error: &crate::ImError) -> Option<&str> {
    match error {
        crate::ImError::Service {
            code: Some(code), ..
        } => Some(code.as_str()),
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

fn sync_error(code: &str, message: impl Into<String>) -> crate::ImError {
    crate::ImError::Service {
        status_code: None,
        code: Some(code.to_owned()),
        message: message.into(),
        data: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    struct Fixture {
        root: std::path::PathBuf,
    }

    impl Fixture {
        fn new(prefix: &str) -> Self {
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
                        "did": "did:example:alice",
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
            Self { root }
        }

        fn client(&self) -> crate::core::ImClient {
            crate::core::ImCore::new(
                crate::ImCoreConfig {
                    service_base_url: crate::ServiceEndpoint::parse("https://example.test")
                        .unwrap(),
                    did_domain: "awiki.test".to_owned(),
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
                "group_state_version": "17",
                "group_event_seq": "23",
                "group_profile": {
                    "display_name": "Stage Two",
                    "description": "ordinary group"
                },
                "membership_status": "active",
                "role": "admin"
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
        let group = baseline_group_record(
            &client,
            &json!({
                "group_did": "did:example:group",
                "group_state_version": "17",
                "group_event_seq": "23",
                "group_profile": {
                    "display_name": "Stage Two",
                    "description": "ordinary group"
                },
                "membership_status": "active",
                "role": "admin"
            }),
            "2026-07-28T10:00:00Z",
            0,
        )
        .unwrap();
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
        let error = reduce_event(&client, &event, None, &mut BTreeMap::new()).unwrap_err();
        assert!(matches!(
            error,
            crate::ImError::Service {
                code: Some(code),
                ..
            } if code == "SYNC_UNKNOWN_REQUIRED_EVENT"
        ));
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
}
