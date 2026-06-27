use serde_json::{json, Map, Value};

use crate::internal::auth::session::{AsyncSessionProvider, SessionProvider};
use crate::internal::message_runtime::read::MESSAGE_RPC_ENDPOINT;
use crate::internal::transport::{
    AsyncAuthenticatedRpcTransport, AsyncRpcTransport, AuthenticatedRpcTransport, RpcTransport,
};

pub(crate) struct MessageSyncRuntime<'a, P, T, R> {
    client: &'a crate::core::ImClient,
    session_provider: P,
    transport: T,
    directory_transport: R,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SyncThreadAfterInput {
    pub(crate) request: crate::messages::SyncThreadAfterRequest,
    pub(crate) resolved_peer_did: Option<String>,
    pub(crate) peer_scope: Option<crate::internal::local_state::owner_scope::DirectPeerScope>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SyncDeltaInput {
    pub(crate) request: crate::messages::SyncDeltaRequest,
}

impl<'a, P, T, R> MessageSyncRuntime<'a, P, T, R> {
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

impl<'a, P, T, R> MessageSyncRuntime<'a, P, T, R>
where
    P: SessionProvider,
    T: AuthenticatedRpcTransport,
    R: RpcTransport,
{
    pub(crate) fn sync_delta(
        mut self,
        input: SyncDeltaInput,
    ) -> crate::ImResult<crate::messages::SyncDeltaResult> {
        self.session_provider
            .ensure_session(crate::auth::AuthScope::Messaging)?;
        let limit = sync_delta_limit(input.request.limit)?;
        let mut result = empty_sync_delta_result();

        loop {
            let since_event_seq = load_global_checkpoint_blocking(self.client)?;
            let params = crate::internal::wire::sync::build_sync_delta_rpc_params(
                &crate::internal::wire::common::WireIdentity {
                    did: self.client.did().as_str().to_owned(),
                },
                crate::internal::wire::sync::SyncDeltaWireRequest {
                    since_event_seq: since_event_seq.clone(),
                    limit,
                    device_id: input.request.device_id.clone(),
                    reason: input.request.reason.clone(),
                },
            )?;
            let raw =
                self.transport
                    .authenticated_rpc(MESSAGE_RPC_ENDPOINT, "sync.delta", params)?;
            let page = crate::internal::wire::sync::parse_sync_delta_page(&raw)?;
            result.pages_fetched = result.pages_fetched.saturating_add(1);
            result.warnings.extend(page.warnings.clone());
            if page.snapshot_required {
                result.snapshot_required = true;
                result.has_more = page.has_more;
                result.retention_floor_event_seq = page.retention_floor_event_seq;
                return Ok(result);
            }
            reject_invalid_delta_page_shape(&page)?;
            let apply_input = sync_delta_apply_input(self.client, &page)?;
            let outcome = apply_sync_delta_blocking(self.client, apply_input)?;
            result.events_applied = result
                .events_applied
                .saturating_add(u32::try_from(outcome.applied_events).unwrap_or(u32::MAX));
            result.last_applied_event_seq = Some(outcome.last_applied_event_seq);
            emit_committed_sync_invalidation(self.client, &outcome.invalidation);
            result.retention_floor_event_seq = page.retention_floor_event_seq;
            result.has_more = page.has_more;
            reject_has_more_without_checkpoint_progress(
                page.has_more,
                since_event_seq.as_str(),
                result.last_applied_event_seq.as_deref(),
            )?;
            if !page.has_more {
                return Ok(result);
            }
        }
    }

    pub(crate) fn sync_thread_after(
        mut self,
        input: SyncThreadAfterInput,
    ) -> crate::ImResult<crate::messages::SyncThreadAfterResult> {
        let limit = sync_thread_after_limit(input.request.limit)?;
        let after_server_seq = explicit_after_server_seq(
            input.request.after_server_seq.as_deref(),
        )?
        .unwrap_or_else(|| local_max_server_seq_blocking(self.client, &input.request.thread));
        match input.request.thread {
            crate::messages::ThreadRef::Direct(peer) => {
                self.session_provider
                    .ensure_session(crate::auth::AuthScope::Messaging)?;
                let peer = crate::internal::message_runtime::read::direct_thread(
                    peer,
                    input.resolved_peer_did,
                )?;
                let params = crate::internal::wire::history::build_history_rpc_params(
                    &crate::internal::wire::common::WireIdentity {
                        did: self.client.did().as_str().to_owned(),
                    },
                    crate::internal::wire::history::HistoryWireRequest {
                        peer_did: peer.resolved_did.clone(),
                        limit: i64::from(limit),
                        cursor: Some(after_server_seq.to_string()),
                        skip: 0,
                        auth: None,
                    },
                )?;
                let mut raw = self.transport.authenticated_rpc(
                    MESSAGE_RPC_ENDPOINT,
                    "direct.get_history",
                    params,
                )?;
                crate::internal::message_runtime::read::project_secure_direct_messages(
                    self.client,
                    &mut raw,
                    &mut self.directory_transport,
                );
                crate::internal::message_runtime::read::annotate_direct_peer_scopes(
                    self.client,
                    &mut raw,
                    &mut self.directory_transport,
                    input.peer_scope.as_ref(),
                );
                let page = crate::internal::message_runtime::read::page_from_raw(
                    self.client,
                    &raw,
                    crate::ids::PageLimit(limit),
                )?;
                let result = thread_after_result(page.items, after_server_seq, raw, limit)?;
                crate::internal::message_runtime::read::persist_projection_best_effort(
                    self.client,
                    &result.messages,
                );
                Ok(result)
            }
            crate::messages::ThreadRef::Group(group) => {
                self.session_provider
                    .ensure_session(crate::auth::AuthScope::GroupMessaging)?;
                let params = crate::internal::wire::group::build_group_messages_rpc_params(
                    self.client.did().as_str(),
                    group.as_str(),
                    i64::from(limit),
                    Some(&after_server_seq.to_string()),
                    0,
                )?;
                let mut raw = self.transport.authenticated_rpc(
                    MESSAGE_RPC_ENDPOINT,
                    "group.list_messages",
                    params,
                )?;
                crate::internal::message_runtime::read::project_group_e2ee_messages(
                    self.client,
                    &mut raw,
                );
                let page = crate::internal::message_runtime::read::page_from_raw_with_group(
                    self.client,
                    &raw,
                    crate::ids::PageLimit(limit),
                    Some(&group),
                )?;
                let result = thread_after_result(page.items, after_server_seq, raw, limit)?;
                crate::internal::message_runtime::read::persist_projection_best_effort(
                    self.client,
                    &result.messages,
                );
                Ok(result)
            }
            crate::messages::ThreadRef::Thread(_) => {
                Err(crate::ImError::unsupported("sync-thread-after-raw-thread"))
            }
        }
    }
}

impl<'a, P, T, R> MessageSyncRuntime<'a, P, T, R>
where
    P: AsyncSessionProvider,
    T: AsyncAuthenticatedRpcTransport,
    R: AsyncRpcTransport,
{
    pub(crate) async fn sync_delta_async(
        mut self,
        input: SyncDeltaInput,
    ) -> crate::ImResult<crate::messages::SyncDeltaResult> {
        self.session_provider
            .ensure_session(crate::auth::AuthScope::Messaging)
            .await?;
        let limit = sync_delta_limit(input.request.limit)?;
        let mut result = empty_sync_delta_result();

        loop {
            let since_event_seq = load_global_checkpoint_async(self.client).await?;
            let params = crate::internal::wire::sync::build_sync_delta_rpc_params(
                &crate::internal::wire::common::WireIdentity {
                    did: self.client.did().as_str().to_owned(),
                },
                crate::internal::wire::sync::SyncDeltaWireRequest {
                    since_event_seq: since_event_seq.clone(),
                    limit,
                    device_id: input.request.device_id.clone(),
                    reason: input.request.reason.clone(),
                },
            )?;
            let raw = self
                .transport
                .authenticated_rpc(MESSAGE_RPC_ENDPOINT, "sync.delta", params)
                .await?;
            let page = crate::internal::wire::sync::parse_sync_delta_page(&raw)?;
            result.pages_fetched = result.pages_fetched.saturating_add(1);
            result.warnings.extend(page.warnings.clone());
            if page.snapshot_required {
                result.snapshot_required = true;
                result.has_more = page.has_more;
                result.retention_floor_event_seq = page.retention_floor_event_seq;
                return Ok(result);
            }
            reject_invalid_delta_page_shape(&page)?;
            let apply_input = sync_delta_apply_input(self.client, &page)?;
            let db = self.client.core_inner().local_state_db().await?;
            let outcome = db.apply_sync_delta(apply_input).await?;
            result.events_applied = result
                .events_applied
                .saturating_add(u32::try_from(outcome.applied_events).unwrap_or(u32::MAX));
            result.last_applied_event_seq = Some(outcome.last_applied_event_seq);
            emit_committed_sync_invalidation(self.client, &outcome.invalidation);
            result.retention_floor_event_seq = page.retention_floor_event_seq;
            result.has_more = page.has_more;
            reject_has_more_without_checkpoint_progress(
                page.has_more,
                since_event_seq.as_str(),
                result.last_applied_event_seq.as_deref(),
            )?;
            if !page.has_more {
                return Ok(result);
            }
        }
    }

    pub(crate) async fn sync_thread_after_async(
        mut self,
        input: SyncThreadAfterInput,
    ) -> crate::ImResult<crate::messages::SyncThreadAfterResult> {
        let limit = sync_thread_after_limit(input.request.limit)?;
        let after_server_seq =
            match explicit_after_server_seq(input.request.after_server_seq.as_deref())? {
                Some(value) => value,
                None => local_max_server_seq_async(self.client, &input.request.thread).await,
            };
        match input.request.thread {
            crate::messages::ThreadRef::Direct(peer) => {
                self.session_provider
                    .ensure_session(crate::auth::AuthScope::Messaging)
                    .await?;
                let peer = crate::internal::message_runtime::read::direct_thread(
                    peer,
                    input.resolved_peer_did,
                )?;
                let params = crate::internal::wire::history::build_history_rpc_params(
                    &crate::internal::wire::common::WireIdentity {
                        did: self.client.did().as_str().to_owned(),
                    },
                    crate::internal::wire::history::HistoryWireRequest {
                        peer_did: peer.resolved_did.clone(),
                        limit: i64::from(limit),
                        cursor: Some(after_server_seq.to_string()),
                        skip: 0,
                        auth: None,
                    },
                )?;
                let mut raw = self
                    .transport
                    .authenticated_rpc(MESSAGE_RPC_ENDPOINT, "direct.get_history", params)
                    .await?;
                crate::internal::message_runtime::read::project_secure_direct_messages_async(
                    self.client,
                    &mut raw,
                    &mut self.directory_transport,
                )
                .await;
                crate::internal::message_runtime::read::annotate_direct_peer_scopes_async(
                    self.client,
                    &mut raw,
                    &mut self.directory_transport,
                    input.peer_scope.as_ref(),
                )
                .await;
                let page = crate::internal::message_runtime::read::page_from_raw(
                    self.client,
                    &raw,
                    crate::ids::PageLimit(limit),
                )?;
                let result = thread_after_result(page.items, after_server_seq, raw, limit)?;
                crate::internal::message_runtime::read::persist_projection_best_effort_async(
                    self.client,
                    &result.messages,
                )
                .await;
                Ok(result)
            }
            crate::messages::ThreadRef::Group(group) => {
                self.session_provider
                    .ensure_session(crate::auth::AuthScope::GroupMessaging)
                    .await?;
                let params = crate::internal::wire::group::build_group_messages_rpc_params(
                    self.client.did().as_str(),
                    group.as_str(),
                    i64::from(limit),
                    Some(&after_server_seq.to_string()),
                    0,
                )?;
                let mut raw = self
                    .transport
                    .authenticated_rpc(MESSAGE_RPC_ENDPOINT, "group.list_messages", params)
                    .await?;
                crate::internal::message_runtime::read::project_group_e2ee_messages_async(
                    self.client,
                    &mut raw,
                )
                .await;
                let page = crate::internal::message_runtime::read::page_from_raw_with_group(
                    self.client,
                    &raw,
                    crate::ids::PageLimit(limit),
                    Some(&group),
                )?;
                let result = thread_after_result(page.items, after_server_seq, raw, limit)?;
                crate::internal::message_runtime::read::persist_projection_best_effort_async(
                    self.client,
                    &result.messages,
                )
                .await;
                Ok(result)
            }
            crate::messages::ThreadRef::Thread(_) => {
                Err(crate::ImError::unsupported("sync-thread-after-raw-thread"))
            }
        }
    }
}

fn sync_thread_after_limit(limit: Option<u32>) -> crate::ImResult<u32> {
    let limit = limit.unwrap_or(100);
    if limit == 0 {
        return Err(crate::ImError::invalid_input(
            Some("limit".to_owned()),
            "limit must be greater than zero",
        ));
    }
    if limit > 500 {
        return Err(crate::ImError::invalid_input(
            Some("limit".to_owned()),
            "sync.thread_after limit must be less than or equal to 500",
        ));
    }
    Ok(limit)
}

fn sync_delta_limit(limit: Option<u32>) -> crate::ImResult<u32> {
    crate::internal::wire::sync::validate_limit(limit.unwrap_or(100))
}

fn empty_sync_delta_result() -> crate::messages::SyncDeltaResult {
    crate::messages::SyncDeltaResult {
        events_applied: 0,
        pages_fetched: 0,
        last_applied_event_seq: None,
        has_more: false,
        snapshot_required: false,
        retention_floor_event_seq: None,
        warnings: Vec::new(),
    }
}

fn reject_invalid_delta_page_shape(
    page: &crate::internal::wire::sync::SyncDeltaPage,
) -> crate::ImResult<()> {
    if page.has_more && page.events.is_empty() {
        return Err(sync_invalid_page(
            "sync.delta returned has_more=true with no events",
        ));
    }
    Ok(())
}

fn reject_has_more_without_checkpoint_progress(
    has_more: bool,
    since_event_seq: &str,
    last_applied_event_seq: Option<&str>,
) -> crate::ImResult<()> {
    if has_more && last_applied_event_seq == Some(since_event_seq) {
        return Err(sync_invalid_page(
            "sync.delta returned has_more=true without checkpoint progress",
        ));
    }
    Ok(())
}

fn emit_committed_sync_invalidation(
    client: &crate::core::ImClient,
    invalidation: &crate::internal::local_state::sync_state::SyncDeltaInvalidation,
) {
    if !invalidation.has_changes() {
        return;
    }
    #[cfg(test)]
    record_committed_sync_invalidation_for_test(invalidation.clone());
    client
        .conversation_store()
        .on_committed_sync_invalidation(client, invalidation);
}

#[cfg(test)]
fn record_committed_sync_invalidation_for_test(
    invalidation: crate::internal::local_state::sync_state::SyncDeltaInvalidation,
) {
    committed_sync_invalidation_log_for_test()
        .lock()
        .expect("committed sync invalidation test log poisoned")
        .push(invalidation);
}

#[cfg(test)]
fn committed_sync_invalidations_for_test(
) -> Vec<crate::internal::local_state::sync_state::SyncDeltaInvalidation> {
    committed_sync_invalidation_log_for_test()
        .lock()
        .expect("committed sync invalidation test log poisoned")
        .clone()
}

#[cfg(test)]
fn committed_sync_invalidation_log_for_test(
) -> &'static std::sync::Mutex<Vec<crate::internal::local_state::sync_state::SyncDeltaInvalidation>>
{
    static LOG: std::sync::OnceLock<
        std::sync::Mutex<Vec<crate::internal::local_state::sync_state::SyncDeltaInvalidation>>,
    > = std::sync::OnceLock::new();
    LOG.get_or_init(|| std::sync::Mutex::new(Vec::new()))
}

#[cfg(all(feature = "sqlite", any(feature = "blocking", test)))]
fn load_global_checkpoint_blocking(client: &crate::core::ImClient) -> crate::ImResult<String> {
    let connection = crate::internal::local_state::open_writable(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    )?;
    Ok(
        crate::internal::local_state::sync_state::load_global_checkpoint(
            &connection,
            client.current_identity().id.as_str(),
        )?
        .map(|checkpoint| checkpoint.event_seq)
        .unwrap_or_else(|| "0".to_owned()),
    )
}

#[cfg(not(all(feature = "sqlite", any(feature = "blocking", test))))]
fn load_global_checkpoint_blocking(_client: &crate::core::ImClient) -> crate::ImResult<String> {
    Err(crate::ImError::unsupported("sync-delta-local-state"))
}

#[cfg(all(feature = "sqlite", any(feature = "blocking", test)))]
fn apply_sync_delta_blocking(
    client: &crate::core::ImClient,
    input: crate::internal::local_state::sync_state::SyncDeltaApplyInput,
) -> crate::ImResult<crate::internal::local_state::sync_state::SyncDeltaApplyOutcome> {
    let mut connection = crate::internal::local_state::open_writable(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    )?;
    let transaction = connection
        .transaction()
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    let outcome =
        crate::internal::local_state::sync_state::apply_sync_delta_tx(&transaction, input)?;
    transaction
        .commit()
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    Ok(outcome)
}

#[cfg(not(all(feature = "sqlite", any(feature = "blocking", test))))]
fn apply_sync_delta_blocking(
    _client: &crate::core::ImClient,
    _input: crate::internal::local_state::sync_state::SyncDeltaApplyInput,
) -> crate::ImResult<crate::internal::local_state::sync_state::SyncDeltaApplyOutcome> {
    Err(crate::ImError::unsupported("sync-delta-local-state"))
}

#[cfg(feature = "sqlite")]
async fn load_global_checkpoint_async(client: &crate::core::ImClient) -> crate::ImResult<String> {
    let db = client.core_inner().local_state_db().await?;
    Ok(db
        .load_global_checkpoint(client.current_identity().id.as_str())
        .await?
        .map(|checkpoint| checkpoint.event_seq)
        .unwrap_or_else(|| "0".to_owned()))
}

#[cfg(not(feature = "sqlite"))]
async fn load_global_checkpoint_async(_client: &crate::core::ImClient) -> crate::ImResult<String> {
    Err(crate::ImError::unsupported("sync-delta-local-state"))
}

#[cfg(feature = "sqlite")]
fn sync_delta_apply_input(
    client: &crate::core::ImClient,
    page: &crate::internal::wire::sync::SyncDeltaPage,
) -> crate::ImResult<crate::internal::local_state::sync_state::SyncDeltaApplyInput> {
    let mut events = Vec::with_capacity(page.events.len());
    for event in &page.events {
        events.push(sync_delta_apply_event(client, event)?);
    }
    Ok(
        crate::internal::local_state::sync_state::SyncDeltaApplyInput {
            owner_identity_id: client.current_identity().id.as_str().to_owned(),
            owner_did: client.did().as_str().to_owned(),
            events,
            next_event_seq: page.next_event_seq.clone(),
            metadata_json: Some(sync_delta_checkpoint_metadata(page)),
        },
    )
}

#[cfg(not(feature = "sqlite"))]
fn sync_delta_apply_input(
    _client: &crate::core::ImClient,
    _page: &crate::internal::wire::sync::SyncDeltaPage,
) -> crate::ImResult<crate::internal::local_state::sync_state::SyncDeltaApplyInput> {
    Err(crate::ImError::unsupported("sync-delta-local-state"))
}

#[cfg(feature = "sqlite")]
fn sync_delta_apply_event(
    client: &crate::core::ImClient,
    event: &crate::internal::wire::sync::SyncDeltaEvent,
) -> crate::ImResult<crate::internal::local_state::sync_state::SyncDeltaApplyEvent> {
    let mut apply = crate::internal::local_state::sync_state::SyncDeltaApplyEvent {
        event_id: event.event_id.clone(),
        event_seq: event.event_seq.clone(),
        ..crate::internal::local_state::sync_state::SyncDeltaApplyEvent::default()
    };

    match event.event_type.as_str() {
        "message.created" => {
            let message = sync_delta_message_from_payload(client, event, true)?;
            apply.messages.push(
                crate::internal::message_runtime::local_projection::message_record_from_message(
                    client, &message,
                )?,
            );
        }
        "conversation.updated" => {
            let message = sync_delta_message_from_payload(client, event, false)?;
            apply.messages.push(
                crate::internal::message_runtime::local_projection::message_record_from_message(
                    client, &message,
                )?,
            );
        }
        "group.member_changed" | "group.profile_updated" => {
            apply.groups.push(sync_delta_group_record(client, event)?);
        }
        unsupported => {
            return Err(sync_invalid_page(format!(
                "unsupported sync.delta event_type {unsupported:?}"
            )));
        }
    }

    Ok(apply)
}

#[cfg(feature = "sqlite")]
fn sync_delta_message_from_payload(
    client: &crate::core::ImClient,
    event: &crate::internal::wire::sync::SyncDeltaEvent,
    require_message: bool,
) -> crate::ImResult<crate::messages::Message> {
    let payload = event
        .payload
        .as_object()
        .ok_or_else(|| sync_invalid_page("event payload must be an object"))?;
    let message = map_value(payload.get("message"))
        .or_else(|| map_value(payload.get("latest_message")))
        .or_else(|| map_value(payload.get("last_message")))
        .or_else(|| map_value(payload.get("body")));
    let Some(message) = message else {
        if require_message {
            return Err(sync_invalid_page("message.created payload missing message"));
        }
        return Err(sync_invalid_page(
            "conversation.updated payload missing latest_message",
        ));
    };
    let thread = map_value(payload.get("thread"));
    let thread_kind = string_from_object(thread, "kind")
        .or_else(|| string_from_object(Some(payload), "thread_kind"))
        .unwrap_or_default();
    let group_did = string_from_object(Some(message), "group_did")
        .or_else(|| string_from_object(thread, "group_did"))
        .or_else(|| string_from_object(thread, "group"))
        .unwrap_or_default();
    let peer_did = string_from_object(thread, "peer_did")
        .or_else(|| string_from_object(thread, "peer"))
        .unwrap_or_default();
    let sender_did = required_payload_string(message, "sender_did")?;
    let receiver_did = string_from_object(Some(message), "receiver_did")
        .unwrap_or_else(|| inferred_receiver_did(client.did().as_str(), &sender_did, &peer_did));
    let message_id = string_from_object(Some(message), "id")
        .or_else(|| string_from_object(Some(message), "message_id"))
        .or_else(|| event.aggregate_id.clone())
        .ok_or_else(|| sync_invalid_page("sync event message id is required"))?;
    let server_seq = decimal_i64_from_object(message, "server_seq")
        .or_else(|| decimal_i64_from_object(message, "group_event_seq"));
    let content_type = string_from_object(Some(message), "content_type")
        .unwrap_or_else(|| "text/plain".to_owned());
    let body = sync_delta_message_body(message.get("content"), &content_type);
    let is_group = !group_did.trim().is_empty() || thread_kind == "group";
    let direction = if sender_did.trim() == client.did().as_str() {
        crate::messages::MessageDirection::Outgoing
    } else {
        crate::messages::MessageDirection::Incoming
    };
    let mut attributes = Vec::new();
    attributes.push(crate::messages::MessageMetadataAttribute {
        key: "sync_event_id".to_owned(),
        value: event.event_id.clone(),
    });
    attributes.push(crate::messages::MessageMetadataAttribute {
        key: "sync_event_seq".to_owned(),
        value: event.event_seq.clone(),
    });
    if let Some(event_type) = Some(event.event_type.trim()).filter(|value| !value.is_empty()) {
        attributes.push(crate::messages::MessageMetadataAttribute {
            key: "sync_event_type".to_owned(),
            value: event_type.to_owned(),
        });
    }
    if let Some(operation_id) = string_from_object(Some(message), "operation_id") {
        attributes.push(crate::messages::MessageMetadataAttribute {
            key: "operation_id".to_owned(),
            value: operation_id,
        });
    }
    let thread = if is_group {
        crate::messages::ThreadRef::Group(crate::ids::GroupRef::parse(&group_did)?)
    } else {
        let peer = if !peer_did.trim().is_empty() {
            peer_did
        } else if sender_did.trim() == client.did().as_str() {
            receiver_did.clone()
        } else {
            sender_did.clone()
        };
        crate::messages::ThreadRef::Direct(crate::ids::PeerRef::parse(peer, "")?)
    };

    Ok(crate::messages::Message {
        id: crate::ids::MessageId::parse(message_id)?,
        thread,
        direction,
        sender: crate::ids::PeerRef::parse(sender_did, "")?,
        receiver: (!receiver_did.trim().is_empty())
            .then(|| crate::ids::PeerRef::parse(&receiver_did, ""))
            .transpose()?,
        group: is_group
            .then(|| crate::ids::GroupRef::parse(&group_did))
            .transpose()?,
        body,
        sent_at: string_from_object(Some(message), "sent_at")
            .or_else(|| string_from_object(Some(message), "accepted_at"))
            .or_else(|| event.created_at.clone()),
        received_at: string_from_object(Some(message), "received_at"),
        metadata: crate::messages::MessageMetadata {
            operation_id: string_from_object(Some(message), "operation_id"),
            server_sequence: server_seq,
            content_type: Some(content_type),
            attributes,
            ..crate::messages::MessageMetadata::default()
        },
    })
}

#[cfg(feature = "sqlite")]
fn sync_delta_message_body(
    content: Option<&Value>,
    content_type: &str,
) -> crate::messages::MessageBodyView {
    match content {
        Some(Value::String(text)) if content_type == "text/markdown" => {
            crate::messages::MessageBodyView::Text {
                text: text.clone(),
                kind: crate::messages::MessageKind::Markdown,
            }
        }
        Some(Value::String(text))
            if content_type == "text/plain" || content_type.trim().is_empty() =>
        {
            crate::messages::MessageBodyView::Text {
                text: text.clone(),
                kind: crate::messages::MessageKind::Text,
            }
        }
        Some(value) if content_type == "application/json" && value.is_object() => {
            crate::messages::MessageBodyView::Payload {
                payload: value.clone(),
            }
        }
        Some(Value::String(text)) if content_type == "application/json" => {
            serde_json::from_str::<Value>(text)
                .ok()
                .filter(Value::is_object)
                .map(|payload| crate::messages::MessageBodyView::Payload { payload })
                .unwrap_or_else(|| crate::messages::MessageBodyView::Unsupported {
                    content_type: Some(content_type.to_owned()),
                })
        }
        _ => crate::messages::MessageBodyView::Unsupported {
            content_type: Some(content_type.to_owned()),
        },
    }
}

#[cfg(feature = "sqlite")]
fn sync_delta_group_record(
    client: &crate::core::ImClient,
    event: &crate::internal::wire::sync::SyncDeltaEvent,
) -> crate::ImResult<crate::internal::local_state::groups::GroupRecord> {
    let payload = event
        .payload
        .as_object()
        .ok_or_else(|| sync_invalid_page("event payload must be an object"))?;
    let thread = map_value(payload.get("thread"));
    let group = map_value(payload.get("group"));
    let membership = map_value(payload.get("membership"));
    let group_did = string_from_object(group, "group_did")
        .or_else(|| string_from_object(thread, "group_did"))
        .or_else(|| event.aggregate_id.clone())
        .ok_or_else(|| sync_invalid_page("group event missing group_did"))?;
    let group_state_version = string_from_object(group, "group_state_version");
    let group_event_seq = decimal_i64_from_object_opt(group, "group_event_seq");
    let profile = group.and_then(|group| map_value(group.get("profile")));
    let subject_did = string_from_object(membership, "subject_did").unwrap_or_default();
    let membership_status = string_from_object(membership, "status")
        .filter(|_| subject_did.trim() == client.did().as_str())
        .unwrap_or_else(|| "active".to_owned());
    let mut metadata = Map::new();
    metadata.insert(
        "source".to_owned(),
        Value::String("im-core.sync_delta".to_owned()),
    );
    metadata.insert("event_id".to_owned(), Value::String(event.event_id.clone()));
    metadata.insert(
        "event_seq".to_owned(),
        Value::String(event.event_seq.clone()),
    );
    metadata.insert(
        "event_type".to_owned(),
        Value::String(event.event_type.clone()),
    );
    insert_json_string(
        &mut metadata,
        "group_state_version",
        group_state_version.as_deref(),
    );
    if let Some(group_event_seq) = group_event_seq {
        metadata.insert(
            "group_event_seq".to_owned(),
            Value::String(group_event_seq.to_string()),
        );
    }
    insert_json_string(
        &mut metadata,
        "actor_did",
        string_from_object(membership, "actor_did")
            .or_else(|| string_from_object(Some(payload), "actor_did"))
            .as_deref(),
    );
    insert_json_string(&mut metadata, "subject_did", Some(subject_did.as_str()));
    insert_json_string(
        &mut metadata,
        "membership_status",
        Some(membership_status.as_str()),
    );

    Ok(crate::internal::local_state::groups::GroupRecord {
        owner_identity_id: client.current_identity().id.as_str().to_owned(),
        owner_did: client.did().as_str().to_owned(),
        group_id: group_did.trim().to_owned(),
        group_did: group_did.trim().to_owned(),
        name: string_from_object(profile, "display_name")
            .or_else(|| string_from_object(profile, "name"))
            .unwrap_or_default(),
        slug: string_from_object(profile, "slug").unwrap_or_default(),
        description: string_from_object(profile, "description").unwrap_or_default(),
        goal: string_from_object(profile, "goal").unwrap_or_default(),
        rules: string_from_object(profile, "rules").unwrap_or_default(),
        message_prompt: string_from_object(profile, "message_prompt").unwrap_or_default(),
        doc_url: string_from_object(profile, "doc_url").unwrap_or_default(),
        membership_status,
        last_synced_seq: group_event_seq,
        remote_updated_at: event.created_at.clone().unwrap_or_default(),
        stored_at: event.created_at.clone().unwrap_or_default(),
        metadata: Value::Object(metadata).to_string(),
        credential_name: client.current_identity().id.as_str().to_owned(),
        ..crate::internal::local_state::groups::GroupRecord::default()
    })
}

#[cfg(feature = "sqlite")]
fn sync_delta_checkpoint_metadata(page: &crate::internal::wire::sync::SyncDeltaPage) -> String {
    json!({
        "source": "im-core.sync_delta",
        "events": page.events.len(),
        "has_more": page.has_more,
        "snapshot_required": page.snapshot_required,
        "retention_floor_event_seq": page.retention_floor_event_seq,
    })
    .to_string()
}

#[cfg(feature = "sqlite")]
fn map_value(value: Option<&Value>) -> Option<&Map<String, Value>> {
    value.and_then(Value::as_object)
}

#[cfg(feature = "sqlite")]
fn string_from_object(object: Option<&Map<String, Value>>, key: &str) -> Option<String> {
    object
        .and_then(|object| object.get(key))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

#[cfg(feature = "sqlite")]
fn required_payload_string(
    object: &Map<String, Value>,
    key: &'static str,
) -> crate::ImResult<String> {
    string_from_object(Some(object), key)
        .ok_or_else(|| sync_invalid_page(format!("sync event message field {key:?} is required")))
}

#[cfg(feature = "sqlite")]
fn decimal_i64_from_object(object: &Map<String, Value>, key: &str) -> Option<i64> {
    decimal_i64_from_object_opt(Some(object), key)
}

#[cfg(feature = "sqlite")]
fn decimal_i64_from_object_opt(object: Option<&Map<String, Value>>, key: &str) -> Option<i64> {
    object
        .and_then(|object| object.get(key))
        .and_then(|value| match value {
            Value::Number(number) => number
                .as_i64()
                .or_else(|| number.as_u64().and_then(|value| i64::try_from(value).ok())),
            Value::String(value) => {
                crate::internal::local_state::sync_state::parse_decimal_seq(value)
                    .ok()
                    .and_then(|value| i64::try_from(value).ok())
            }
            _ => None,
        })
}

#[cfg(feature = "sqlite")]
fn inferred_receiver_did(owner_did: &str, sender_did: &str, peer_did: &str) -> String {
    if sender_did.trim() == owner_did.trim() {
        peer_did.trim().to_owned()
    } else {
        owner_did.trim().to_owned()
    }
}

#[cfg(feature = "sqlite")]
fn insert_json_string(object: &mut Map<String, Value>, key: &str, value: Option<&str>) {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return;
    };
    object.insert(key.to_owned(), Value::String(value.to_owned()));
}

fn sync_invalid_page(message: impl Into<String>) -> crate::ImError {
    crate::ImError::Service {
        status_code: None,
        code: Some("sync.invalid_page".to_owned()),
        message: message.into(),
    }
}

fn explicit_after_server_seq(value: Option<&str>) -> crate::ImResult<Option<i64>> {
    value.map(parse_after_server_seq).transpose()
}

fn parse_after_server_seq(value: &str) -> crate::ImResult<i64> {
    let parsed = crate::internal::local_state::sync_state::parse_decimal_seq(value)
        .map_err(|_| invalid_after_server_seq(value))?;
    i64::try_from(parsed).map_err(|_| invalid_after_server_seq(value))
}

fn invalid_after_server_seq(value: &str) -> crate::ImError {
    crate::ImError::invalid_input(
        Some("after_server_seq".to_owned()),
        format!("after_server_seq must be a non-negative decimal string: {value:?}"),
    )
}

fn thread_after_result(
    mut messages: Vec<crate::messages::Message>,
    after_server_seq: i64,
    raw: Value,
    limit: u32,
) -> crate::ImResult<crate::messages::SyncThreadAfterResult> {
    messages.retain(|message| {
        message
            .metadata
            .server_sequence
            .is_some_and(|server_sequence| server_sequence > after_server_seq)
    });
    messages.sort_by(|left, right| {
        left.metadata
            .server_sequence
            .unwrap_or_default()
            .cmp(&right.metadata.server_sequence.unwrap_or_default())
            .then_with(|| left.id.as_str().cmp(right.id.as_str()))
    });
    let truncated = truncate_messages(&mut messages, limit);
    let next_after_server_seq = messages
        .last()
        .and_then(|message| message.metadata.server_sequence)
        .unwrap_or(after_server_seq)
        .to_string();
    let has_more = raw
        .get("has_more")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        || truncated;
    let warnings = warnings_from_raw(&raw);
    Ok(crate::messages::SyncThreadAfterResult {
        messages,
        next_after_server_seq: Some(next_after_server_seq),
        has_more,
        warnings,
    })
}

fn truncate_messages(messages: &mut Vec<crate::messages::Message>, limit: u32) -> bool {
    let limit = usize::try_from(limit).unwrap_or(usize::MAX);
    if messages.len() <= limit {
        return false;
    }
    messages.truncate(limit);
    true
}

fn warnings_from_raw(raw: &Value) -> Vec<String> {
    raw.get("warnings")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

#[cfg(all(feature = "sqlite", any(feature = "blocking", test)))]
fn local_max_server_seq_blocking(
    client: &crate::core::ImClient,
    thread: &crate::messages::ThreadRef,
) -> i64 {
    let Ok(connection) = crate::internal::local_state::open_writable(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    ) else {
        return 0;
    };
    crate::internal::local_state::messages::max_server_seq_for_thread_ref_for_owner_identity(
        &connection,
        client.current_identity().id.as_str(),
        client.did().as_str(),
        thread,
    )
    .ok()
    .flatten()
    .unwrap_or_default()
}

#[cfg(not(all(feature = "sqlite", any(feature = "blocking", test))))]
fn local_max_server_seq_blocking(
    _client: &crate::core::ImClient,
    _thread: &crate::messages::ThreadRef,
) -> i64 {
    0
}

#[cfg(feature = "sqlite")]
async fn local_max_server_seq_async(
    client: &crate::core::ImClient,
    thread: &crate::messages::ThreadRef,
) -> i64 {
    let db = match client.core_inner().local_state_db().await {
        Ok(db) => db,
        Err(_) => return 0,
    };
    db.max_server_seq_for_thread_ref(
        client.current_identity().id.as_str(),
        client.did().as_str(),
        thread.clone(),
    )
    .await
    .ok()
    .flatten()
    .unwrap_or_default()
}

#[cfg(not(feature = "sqlite"))]
async fn local_max_server_seq_async(
    _client: &crate::core::ImClient,
    _thread: &crate::messages::ThreadRef,
) -> i64 {
    0
}

#[cfg(test)]
mod tests;
