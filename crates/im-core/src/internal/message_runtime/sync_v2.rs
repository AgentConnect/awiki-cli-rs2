use std::collections::BTreeMap;
use std::sync::{Arc, Mutex as StdMutex, OnceLock};

use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

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
        let owner_lock = owner_sync_lock(&binding.owner_identity_id);
        let _owner_guard = owner_lock.lock().await;
        let db = self.client.core_inner().local_state_db().await?;
        let owner_identity_id = binding.owner_identity_id.clone();
        let mut result = empty_outcome();
        let mut state = match db
            .load_message_sync_state(owner_identity_id.clone())
            .await?
        {
            crate::internal::local_state::sync_v2::MessageSyncStateAccess::Ready(state) => state,
            crate::internal::local_state::sync_v2::MessageSyncStateAccess::BootstrapRequired(_) => {
                self.bootstrap(&db, &binding, &mut result).await?
            }
        };

        self.drain_read_outbox(&db, &binding).await?;
        let mut recovery_token_retries = 0_u8;
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
                return Ok(best_effort_cleanup(&db, &state, result).await);
            }
        }
    }

    async fn drain_read_outbox(
        &mut self,
        db: &crate::internal::local_state::actor::LocalStateDb,
        binding: &crate::identity::ActiveSyncAccountBinding,
    ) -> crate::ImResult<()> {
        for _ in 0..16 {
            let now = unix_time_i64();
            let Some(record) = db
                .claim_next_read_mutation(&binding.owner_identity_id, now)
                .await?
            else {
                break;
            };
            if let Err(error) = self.send_claimed_read_mutation(db, binding, &record).await {
                db.retry_local_mutation(
                    &binding.owner_identity_id,
                    &record.mutation_id,
                    error_code(&error).unwrap_or("READ_STATE_RETRY"),
                    now.saturating_add(5),
                )
                .await?;
                return Err(error);
            }
        }
        Ok(())
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
        let events = snapshot
            .recent_plain_messages
            .iter()
            .map(|item| {
                reduce_event(
                    self.client,
                    &item.event,
                    Some(&item.message),
                    &mut public_messages,
                )
            })
            .collect::<crate::ImResult<Vec<_>>>()?;
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
        } = &response
        {
            if account_id != &binding.account_id || device_id != &binding.protocol_device_id {
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
            return Ok(state);
        }
        let crate::internal::wire::sync_v2::SyncBootstrapResponseV2::TailOnly(bootstrap) = response
        else {
            unreachable!("bootstrap response variants were handled")
        };
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
        let read_states = bootstrap
            .read_state_baseline
            .iter()
            .map(read_state_from_snapshot)
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
                read_states,
            },
        )
        .await?;
        Ok(state)
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

fn reduce_event(
    client: &crate::core::ImClient,
    event: &crate::internal::wire::sync_v2::SyncEventV2,
    hydrated_message: Option<&Value>,
    public_messages: &mut BTreeMap<String, crate::messages::Message>,
) -> crate::ImResult<crate::internal::local_state::sync_v2::DeltaApplyEventV2> {
    if matches!(
        event.event_type.as_str(),
        "message.created"
            | "message.read_state_updated"
            | "group.member_changed"
            | "group.profile_updated"
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
        | crate::ImError::PermissionDenied
        | crate::ImError::IdentityBindingConflict { .. } => (
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
                    "SYNC_ACCOUNT_BINDING_MISMATCH"
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
        recovery_id: &str,
        token: &str,
        stream_epoch: &str,
        snapshot_scan_seq: &str,
    ) -> Value {
        json!({
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

        let outcome =
            MessageSyncRuntimeV2::new(&client, ReadySyncSnapshotSessionProvider, transport)
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
    async fn sync_snapshot_missing_state_bootstrap_recovery_closes_with_delta_ack() {
        let fixture = SyncSnapshotFixture::new("bootstrap-recovery");
        let client = fixture.client();
        let binding = client.active_sync_account_binding().await.unwrap();
        let calls = Rc::new(RefCell::new(Vec::new()));
        let outcome = MessageSyncRuntimeV2::new(
            &client,
            ReadySyncSnapshotSessionProvider,
            SyncSnapshotTransport::queued(
                Rc::clone(&calls),
                vec![
                    Ok(sync_snapshot_bootstrap_recovery(
                        &binding,
                        "recovery-bootstrap",
                        "snapshot-token-bootstrap",
                        "3",
                        "40",
                    )),
                    Ok(sync_snapshot_response(&binding, "3", "40", vec![])),
                    Ok(sync_snapshot_delta("3", "41", vec![])),
                ],
            ),
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
                    Ok(sync_read_ack(
                        &binding,
                        "remote-thread-key-exact-bob",
                        "30",
                        "message-read-30",
                        "2026-07-28T12:00:03Z",
                    )),
                    Ok(sync_snapshot_delta("1", "12", vec![])),
                ],
            ),
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
            ["read_state.mark_read", "sync.delta"]
        );
        assert_eq!(
            calls[0].params.pointer("/body/thread"),
            Some(&json!({
                "kind": "direct",
                "thread_key": "remote-thread-key-exact-bob"
            }))
        );
        assert_eq!(
            calls[0].params.pointer("/meta/operation_id"),
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
                    Ok(sync_read_ack(
                        &binding,
                        "remote-thread-key-higher-bob",
                        "50",
                        "message-read-high",
                        "2026-07-28T12:00:09Z",
                    )),
                    Ok(sync_snapshot_delta("1", "12", vec![])),
                ],
            ),
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
    async fn sync_read_outbox_pseudo_ack_returns_claim_to_retryable() {
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
        let error = MessageSyncRuntimeV2::new(
            &client,
            ReadySyncSnapshotSessionProvider,
            SyncSnapshotTransport::queued(Rc::new(RefCell::new(Vec::new())), vec![Ok(pseudo_ack)]),
        )
        .sync_now(sync_snapshot_request())
        .await
        .unwrap_err();
        assert!(matches!(
            error,
            crate::ImError::Service {
                code: Some(code),
                ..
            } if code == "READ_STATE_INCOMPLETE_ACK"
        ));
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
                "required_security_profile": "plaintext",
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
                "required_security_profile": "plaintext",
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
                "client_message_id": "client-message-discovered"
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
            "created_at": "2026-07-28T10:00:00Z"
        });
        let mut public_messages = BTreeMap::new();

        let apply = reduce_event(
            &client,
            &event,
            Some(&hydrated_message),
            &mut public_messages,
        )
        .unwrap();

        assert_eq!(apply.messages.len(), 1);
        assert_eq!(
            apply.messages[0].hydration_state,
            crate::internal::local_state::messages::MessageHydrationState::Discovered
        );
        assert_eq!(apply.thread_bindings.len(), 1);
        assert_eq!(
            apply.thread_bindings[0].remote_thread_key,
            "remote-thread-bob"
        );
        assert_eq!(apply.thread_bindings[0].thread_kind, "direct");
        let message = public_messages.get("event-discovered").unwrap();
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
