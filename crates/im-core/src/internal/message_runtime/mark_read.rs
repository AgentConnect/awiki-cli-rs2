use serde_json::Value;

use crate::internal::auth::session::{AsyncSessionProvider, SessionProvider};
use crate::internal::transport::{AsyncAuthenticatedRpcTransport, AuthenticatedRpcTransport};

pub(crate) struct MessageMarkReadRuntime<'a, P, T> {
    client: &'a crate::core::ImClient,
    session_provider: P,
    transport: T,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MarkReadInput {
    pub message_ids: Vec<crate::ids::MessageId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MarkThreadReadInput {
    pub request: crate::messages::MarkThreadReadRequest,
    pub remote_thread: Option<crate::messages::ThreadRef>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct MarkReadRuntimeResult {
    pub sdk_result: crate::messages::MarkReadResult,
    pub raw: Option<Value>,
    pub direct_ids: Vec<String>,
    pub group_ids: Vec<String>,
    pub local_only_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct MarkThreadReadRuntimeResult {
    pub sdk_result: crate::messages::MarkThreadReadResult,
    pub raw: Option<Value>,
    pub direct_ids: Vec<String>,
    pub group_ids: Vec<String>,
    pub local_only_ids: Vec<String>,
}

#[derive(Debug, Clone)]
struct ClaimedReadSend {
    mutation_id: String,
    operation_id: String,
    thread: crate::messages::ThreadRef,
    watermark: crate::messages::ReadWatermark,
    remote_thread_key: String,
}

impl<'a, P, T> MessageMarkReadRuntime<'a, P, T>
where
    P: SessionProvider,
    T: AuthenticatedRpcTransport,
{
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

    pub(crate) fn mark_read(
        mut self,
        input: MarkReadInput,
    ) -> crate::ImResult<MarkReadRuntimeResult> {
        if input.message_ids.is_empty() {
            return Err(crate::ImError::MessageNotFound {
                message_id: "message_ids".to_string(),
            });
        }
        let ids = input
            .message_ids
            .iter()
            .map(|id| id.as_str().to_string())
            .collect::<Vec<_>>();
        let classification = classify_mark_read_ids(self.client, &ids);
        let direct_ids = classification
            .as_ref()
            .map(|value| value.direct_ids.clone())
            .unwrap_or_else(|_| ids.clone());
        let remote_direct_ids = classification
            .as_ref()
            .map(|value| value.remote_direct_ids.clone())
            .unwrap_or_else(|_| ids.clone());
        let group_ids = classification
            .as_ref()
            .map(|value| value.group_ids.clone())
            .unwrap_or_default();
        let local_only_ids = classification
            .as_ref()
            .map(|value| value.local_only_ids.clone())
            .unwrap_or_default();

        let mut warnings = Vec::new();
        let mut updated_count = 0_i64;
        let mut raw = None;
        if !remote_direct_ids.is_empty() {
            let (updated, response) = mark_direct_ids_remote_sync(
                self.client,
                &mut self.session_provider,
                &mut self.transport,
                &remote_direct_ids,
            )?;
            updated_count += updated;
            warnings.extend(warnings_from_raw(&response));
            raw = Some(response);
        }

        if let Ok(local_updated) = mark_local_messages_read(self.client, classification.as_ref()) {
            if updated_count == 0 {
                updated_count = local_updated;
            } else {
                updated_count += (group_ids.len() + local_only_ids.len()) as i64;
            }
            if local_updated > 0 {
                self.client
                    .emit_committed_conversation_projection("local_mark_read");
            }
        } else if classification.is_ok() {
            warnings.push("Failed to mark local messages read".to_string());
        }

        Ok(MarkReadRuntimeResult {
            sdk_result: crate::messages::MarkReadResult {
                updated_count: u32::try_from(updated_count).unwrap_or(u32::MAX),
                message_ids: input.message_ids,
                warnings,
            },
            raw,
            direct_ids,
            group_ids,
            local_only_ids,
        })
    }

    pub(crate) fn mark_thread_read(
        mut self,
        input: MarkThreadReadInput,
    ) -> crate::ImResult<MarkThreadReadRuntimeResult> {
        let effective_watermark = effective_watermark_sync(
            self.client,
            &input.request.thread,
            input.request.watermark.as_ref(),
        )?;
        let mut warnings = Vec::new();
        let mut partial = false;
        let local_result = mark_thread_read_watermark_local(
            self.client,
            &input.request.thread,
            effective_watermark.as_ref(),
            true,
        )?;
        if local_result.updated_count > 0 {
            self.client
                .emit_committed_conversation_projection("local_mark_thread_read_watermark");
        }
        if !local_result.remote_ack_applicable {
            return Ok(MarkThreadReadRuntimeResult {
                sdk_result: crate::messages::MarkThreadReadResult {
                    updated_count: u32_count_i64(local_result.updated_count),
                    remote_acknowledged: false,
                    partial: false,
                    fallback_used: false,
                    pending_remote_ack: false,
                    effective_watermark,
                    legacy_message_ids: Vec::new(),
                    warnings,
                },
                raw: None,
                direct_ids: Vec::new(),
                group_ids: Vec::new(),
                local_only_ids: Vec::new(),
            });
        }
        let claimed =
            claim_read_send_sync(self.client, local_result.outbox_operation_id.as_deref())?;
        if local_result.outbox_operation_id.is_some() && claimed.is_none() {
            return Ok(waiting_successor_result(
                local_result.updated_count,
                effective_watermark,
            ));
        }
        let send_thread = claimed
            .as_ref()
            .map(|value| &value.thread)
            .unwrap_or_else(|| {
                input
                    .remote_thread
                    .as_ref()
                    .unwrap_or(&input.request.thread)
            });
        let send_watermark = claimed
            .as_ref()
            .map(|value| &value.watermark)
            .or(effective_watermark.as_ref());
        let operation_id = claimed
            .as_ref()
            .map(|value| value.operation_id.as_str())
            .or(local_result.outbox_operation_id.as_deref());
        let remote_thread_key = claimed
            .as_ref()
            .map(|value| value.remote_thread_key.as_str())
            .or(local_result.remote_thread_key.as_deref());

        let (remote_acknowledged, fallback_used, pending_remote_ack, raw, legacy_message_ids) =
            match mark_read_state_remote_sync(
                self.client,
                &mut self.session_provider,
                &mut self.transport,
                send_thread,
                send_watermark,
                input.request.fallback_max_message_ids,
                operation_id,
                remote_thread_key,
            ) {
                Ok(response) => {
                    let validated = send_watermark
                        .ok_or_else(|| {
                            read_ack_error("READ_STATE_INVALID_ACK", "sent watermark is missing")
                        })
                        .and_then(|requested| {
                            validate_final_read_response(
                                self.client,
                                &response,
                                send_thread,
                                remote_thread_key,
                                requested,
                            )
                        });
                    match validated {
                        Ok(ack) => {
                            warnings.extend(ack.warnings.clone());
                            let fallback_used = ack.fallback_used;
                            let acknowledged = Some(crate::messages::ReadWatermark {
                                last_read_message_id: ack
                                    .read_watermark_message_id
                                    .as_deref()
                                    .map(crate::ids::MessageId::parse)
                                    .transpose()?,
                                last_read_thread_seq: ack.read_watermark_server_seq,
                                read_at: Some(
                                    chrono::DateTime::parse_from_rfc3339(&ack.read_at)
                                        .expect("strict parser validated read_at")
                                        .with_timezone(&chrono::Utc),
                                ),
                            });
                            if let Err(error) = mark_thread_read_watermark_local(
                                self.client,
                                &input.request.thread,
                                acknowledged.as_ref(),
                                false,
                            ) {
                                retry_read_send_sync(
                                    self.client,
                                    claimed.as_ref(),
                                    "READ_STATE_LOCAL_ACK_FAILED",
                                )?;
                                partial = true;
                                warnings.push(format!("Failed to commit read-state ACK: {error}"));
                                (false, false, true, Some(response), Vec::new())
                            } else {
                                (true, fallback_used, false, Some(response), Vec::new())
                            }
                        }
                        Err(error) => {
                            retry_read_send_sync(
                                self.client,
                                claimed.as_ref(),
                                error_service_code(&error).unwrap_or("READ_STATE_INVALID_ACK"),
                            )?;
                            partial = true;
                            warnings.push(format!("Invalid read-state ACK: {error}"));
                            (false, false, true, Some(response), Vec::new())
                        }
                    }
                }
                Err(error) if is_read_state_unsupported_error(&error) => {
                    if !legacy_read_state_fallback_allowed(self.client) {
                        retry_read_send_sync(
                            self.client,
                            claimed.as_ref(),
                            error_service_code(&error).unwrap_or("READ_STATE_UNSUPPORTED"),
                        )?;
                        return Err(error);
                    }
                    let fallback = legacy_fallback_mark_thread_read_sync(
                        self.client,
                        &mut self.session_provider,
                        &mut self.transport,
                        &input.request.thread,
                        effective_watermark.as_ref(),
                        input.request.fallback_max_message_ids,
                    );
                    let fallback = match fallback {
                        Ok(fallback) => fallback,
                        Err(error) => {
                            retry_read_send_sync(
                                self.client,
                                claimed.as_ref(),
                                error_service_code(&error).unwrap_or("READ_STATE_FALLBACK_RETRY"),
                            )?;
                            return Err(error);
                        }
                    };
                    warnings.extend(fallback.warnings);
                    partial |= fallback.partial;
                    let mut fallback_acknowledged = fallback.remote_acknowledged;
                    if fallback.remote_acknowledged {
                        if let Err(error) = mark_thread_read_watermark_local(
                            self.client,
                            &input.request.thread,
                            send_watermark,
                            false,
                        ) {
                            retry_read_send_sync(
                                self.client,
                                claimed.as_ref(),
                                "READ_STATE_LOCAL_ACK_FAILED",
                            )?;
                            fallback_acknowledged = false;
                            partial = true;
                            warnings.push(format!("Failed to commit fallback ACK: {error}"));
                        }
                    } else {
                        retry_read_send_sync(
                            self.client,
                            claimed.as_ref(),
                            "READ_STATE_FALLBACK_PENDING",
                        )?;
                    }
                    (
                        fallback_acknowledged,
                        true,
                        !fallback_acknowledged,
                        fallback.raw,
                        fallback.message_ids,
                    )
                }
                Err(error) => {
                    retry_read_send_sync(
                        self.client,
                        claimed.as_ref(),
                        error_service_code(&error).unwrap_or("READ_STATE_RETRY"),
                    )?;
                    partial = true;
                    warnings.push(format!("Remote read-state mark-read failed: {error}"));
                    (false, false, true, None, Vec::new())
                }
            };

        Ok(MarkThreadReadRuntimeResult {
            sdk_result: crate::messages::MarkThreadReadResult {
                updated_count: u32_count_i64(local_result.updated_count),
                remote_acknowledged,
                partial,
                fallback_used,
                pending_remote_ack,
                effective_watermark,
                legacy_message_ids,
                warnings,
            },
            raw,
            direct_ids: Vec::new(),
            group_ids: Vec::new(),
            local_only_ids: Vec::new(),
        })
    }
}

impl<'a, P, T> MessageMarkReadRuntime<'a, P, T>
where
    P: AsyncSessionProvider,
    T: AsyncAuthenticatedRpcTransport,
{
    pub(crate) async fn mark_read_async(
        mut self,
        input: MarkReadInput,
    ) -> crate::ImResult<MarkReadRuntimeResult> {
        if input.message_ids.is_empty() {
            return Err(crate::ImError::MessageNotFound {
                message_id: "message_ids".to_string(),
            });
        }
        let ids = input
            .message_ids
            .iter()
            .map(|id| id.as_str().to_string())
            .collect::<Vec<_>>();
        let classification = classify_mark_read_ids_async(self.client, &ids).await;
        let direct_ids = classification
            .as_ref()
            .map(|value| value.direct_ids.clone())
            .unwrap_or_else(|_| ids.clone());
        let remote_direct_ids = classification
            .as_ref()
            .map(|value| value.remote_direct_ids.clone())
            .unwrap_or_else(|_| ids.clone());
        let group_ids = classification
            .as_ref()
            .map(|value| value.group_ids.clone())
            .unwrap_or_default();
        let local_only_ids = classification
            .as_ref()
            .map(|value| value.local_only_ids.clone())
            .unwrap_or_default();

        let mut warnings = Vec::new();
        let mut updated_count = 0_i64;
        let mut raw = None;
        if !remote_direct_ids.is_empty() {
            let (updated, response) = mark_direct_ids_remote_async(
                self.client,
                &mut self.session_provider,
                &mut self.transport,
                &remote_direct_ids,
            )
            .await?;
            updated_count += updated;
            warnings.extend(warnings_from_raw(&response));
            raw = Some(response);
        }

        if let Ok(local_updated) =
            mark_local_messages_read_async(self.client, classification.as_ref()).await
        {
            if updated_count == 0 {
                updated_count = local_updated;
            } else {
                updated_count += (group_ids.len() + local_only_ids.len()) as i64;
            }
            if local_updated > 0 {
                self.client
                    .emit_committed_conversation_projection("local_mark_read");
            }
        } else if classification.is_ok() {
            warnings.push("Failed to mark local messages read".to_string());
        }

        Ok(MarkReadRuntimeResult {
            sdk_result: crate::messages::MarkReadResult {
                updated_count: u32::try_from(updated_count).unwrap_or(u32::MAX),
                message_ids: input.message_ids,
                warnings,
            },
            raw,
            direct_ids,
            group_ids,
            local_only_ids,
        })
    }

    pub(crate) async fn mark_thread_read_async(
        mut self,
        input: MarkThreadReadInput,
    ) -> crate::ImResult<MarkThreadReadRuntimeResult> {
        let effective_watermark = effective_watermark_async(
            self.client,
            &input.request.thread,
            input.request.watermark.as_ref(),
        )
        .await?;
        let mut warnings = Vec::new();
        let mut partial = false;
        let local_result = mark_thread_read_watermark_local_async(
            self.client,
            &input.request.thread,
            effective_watermark.as_ref(),
            true,
        )
        .await?;
        if local_result.updated_count > 0 {
            self.client
                .emit_committed_conversation_projection("local_mark_thread_read_watermark");
        }
        if !local_result.remote_ack_applicable {
            return Ok(MarkThreadReadRuntimeResult {
                sdk_result: crate::messages::MarkThreadReadResult {
                    updated_count: u32_count_i64(local_result.updated_count),
                    remote_acknowledged: false,
                    partial: false,
                    fallback_used: false,
                    pending_remote_ack: false,
                    effective_watermark,
                    legacy_message_ids: Vec::new(),
                    warnings,
                },
                raw: None,
                direct_ids: Vec::new(),
                group_ids: Vec::new(),
                local_only_ids: Vec::new(),
            });
        }
        let claimed =
            claim_read_send_async(self.client, local_result.outbox_operation_id.as_deref()).await?;
        if local_result.outbox_operation_id.is_some() && claimed.is_none() {
            return Ok(waiting_successor_result(
                local_result.updated_count,
                effective_watermark,
            ));
        }
        let send_thread = claimed
            .as_ref()
            .map(|value| &value.thread)
            .unwrap_or_else(|| {
                input
                    .remote_thread
                    .as_ref()
                    .unwrap_or(&input.request.thread)
            });
        let send_watermark = claimed
            .as_ref()
            .map(|value| &value.watermark)
            .or(effective_watermark.as_ref());
        let operation_id = claimed
            .as_ref()
            .map(|value| value.operation_id.as_str())
            .or(local_result.outbox_operation_id.as_deref());
        let remote_thread_key = claimed
            .as_ref()
            .map(|value| value.remote_thread_key.as_str())
            .or(local_result.remote_thread_key.as_deref());

        let (remote_acknowledged, fallback_used, pending_remote_ack, raw, legacy_message_ids) =
            match mark_read_state_remote_async(
                self.client,
                &mut self.session_provider,
                &mut self.transport,
                send_thread,
                send_watermark,
                input.request.fallback_max_message_ids,
                operation_id,
                remote_thread_key,
            )
            .await
            {
                Ok(response) => {
                    let validated = send_watermark
                        .ok_or_else(|| {
                            read_ack_error("READ_STATE_INVALID_ACK", "sent watermark is missing")
                        })
                        .and_then(|requested| {
                            validate_final_read_response(
                                self.client,
                                &response,
                                send_thread,
                                remote_thread_key,
                                requested,
                            )
                        });
                    match validated {
                        Ok(ack) => {
                            warnings.extend(ack.warnings.clone());
                            let fallback_used = ack.fallback_used;
                            let acknowledged = Some(crate::messages::ReadWatermark {
                                last_read_message_id: ack
                                    .read_watermark_message_id
                                    .as_deref()
                                    .map(crate::ids::MessageId::parse)
                                    .transpose()?,
                                last_read_thread_seq: ack.read_watermark_server_seq,
                                read_at: Some(
                                    chrono::DateTime::parse_from_rfc3339(&ack.read_at)
                                        .expect("strict parser validated read_at")
                                        .with_timezone(&chrono::Utc),
                                ),
                            });
                            if let Err(error) = mark_thread_read_watermark_local_async(
                                self.client,
                                &input.request.thread,
                                acknowledged.as_ref(),
                                false,
                            )
                            .await
                            {
                                retry_read_send_async(
                                    self.client,
                                    claimed.as_ref(),
                                    "READ_STATE_LOCAL_ACK_FAILED",
                                )
                                .await?;
                                partial = true;
                                warnings.push(format!("Failed to commit read-state ACK: {error}"));
                                (false, false, true, Some(response), Vec::new())
                            } else {
                                (true, fallback_used, false, Some(response), Vec::new())
                            }
                        }
                        Err(error) => {
                            retry_read_send_async(
                                self.client,
                                claimed.as_ref(),
                                error_service_code(&error).unwrap_or("READ_STATE_INVALID_ACK"),
                            )
                            .await?;
                            partial = true;
                            warnings.push(format!("Invalid read-state ACK: {error}"));
                            (false, false, true, Some(response), Vec::new())
                        }
                    }
                }
                Err(error) if is_read_state_unsupported_error(&error) => {
                    if !legacy_read_state_fallback_allowed(self.client) {
                        retry_read_send_async(
                            self.client,
                            claimed.as_ref(),
                            error_service_code(&error).unwrap_or("READ_STATE_UNSUPPORTED"),
                        )
                        .await?;
                        return Err(error);
                    }
                    let fallback = legacy_fallback_mark_thread_read_async(
                        self.client,
                        &mut self.session_provider,
                        &mut self.transport,
                        &input.request.thread,
                        effective_watermark.as_ref(),
                        input.request.fallback_max_message_ids,
                    )
                    .await;
                    let fallback = match fallback {
                        Ok(fallback) => fallback,
                        Err(error) => {
                            retry_read_send_async(
                                self.client,
                                claimed.as_ref(),
                                error_service_code(&error).unwrap_or("READ_STATE_FALLBACK_RETRY"),
                            )
                            .await?;
                            return Err(error);
                        }
                    };
                    warnings.extend(fallback.warnings);
                    partial |= fallback.partial;
                    let mut fallback_acknowledged = fallback.remote_acknowledged;
                    if fallback.remote_acknowledged {
                        if let Err(error) = mark_thread_read_watermark_local_async(
                            self.client,
                            &input.request.thread,
                            send_watermark,
                            false,
                        )
                        .await
                        {
                            retry_read_send_async(
                                self.client,
                                claimed.as_ref(),
                                "READ_STATE_LOCAL_ACK_FAILED",
                            )
                            .await?;
                            fallback_acknowledged = false;
                            partial = true;
                            warnings.push(format!("Failed to commit fallback ACK: {error}"));
                        }
                    } else {
                        retry_read_send_async(
                            self.client,
                            claimed.as_ref(),
                            "READ_STATE_FALLBACK_PENDING",
                        )
                        .await?;
                    }
                    (
                        fallback_acknowledged,
                        true,
                        !fallback_acknowledged,
                        fallback.raw,
                        fallback.message_ids,
                    )
                }
                Err(error) => {
                    retry_read_send_async(
                        self.client,
                        claimed.as_ref(),
                        error_service_code(&error).unwrap_or("READ_STATE_RETRY"),
                    )
                    .await?;
                    partial = true;
                    warnings.push(format!("Remote read-state mark-read failed: {error}"));
                    (false, false, true, None, Vec::new())
                }
            };

        Ok(MarkThreadReadRuntimeResult {
            sdk_result: crate::messages::MarkThreadReadResult {
                updated_count: u32_count_i64(local_result.updated_count),
                remote_acknowledged,
                partial,
                fallback_used,
                pending_remote_ack,
                effective_watermark,
                legacy_message_ids,
                warnings,
            },
            raw,
            direct_ids: Vec::new(),
            group_ids: Vec::new(),
            local_only_ids: Vec::new(),
        })
    }
}

fn legacy_read_state_fallback_allowed(client: &crate::core::ImClient) -> bool {
    matches!(client.realtime_requires_sync_changed_v2(), Ok(false))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ThreadUnreadLookup {
    message_ids: Vec<String>,
    truncated: bool,
}

#[cfg(all(feature = "sqlite", any(feature = "blocking", test)))]
fn list_incoming_message_ids_for_legacy_fallback(
    client: &crate::core::ImClient,
    thread: &crate::messages::ThreadRef,
    watermark: Option<&crate::messages::ReadWatermark>,
    fallback_max_message_ids: Option<u32>,
) -> crate::ImResult<ThreadUnreadLookup> {
    let connection = crate::internal::local_state::open_writable(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    )?;
    let result = crate::internal::local_state::messages::list_incoming_message_ids_up_to_watermark_for_owner_identity(
        &connection,
        client.current_identity().id.as_str(),
        client.did().as_str(),
        thread,
        watermark.and_then(|watermark| {
            watermark
                .last_read_message_id
                .as_ref()
                .map(|id| id.as_str())
        }),
        watermark.and_then(|watermark| watermark.last_read_thread_seq.as_deref()),
        thread_mark_read_limit(fallback_max_message_ids),
    )?;
    Ok(ThreadUnreadLookup {
        message_ids: result.message_ids,
        truncated: result.truncated,
    })
}

#[cfg(all(feature = "sqlite", not(any(feature = "blocking", test))))]
fn list_incoming_message_ids_for_legacy_fallback(
    _client: &crate::core::ImClient,
    _thread: &crate::messages::ThreadRef,
    _watermark: Option<&crate::messages::ReadWatermark>,
    _fallback_max_message_ids: Option<u32>,
) -> crate::ImResult<ThreadUnreadLookup> {
    Err(crate::ImError::unsupported("sync-message-mark-thread-read"))
}

#[cfg(not(feature = "sqlite"))]
fn list_incoming_message_ids_for_legacy_fallback(
    _client: &crate::core::ImClient,
    _thread: &crate::messages::ThreadRef,
    _watermark: Option<&crate::messages::ReadWatermark>,
    _fallback_max_message_ids: Option<u32>,
) -> crate::ImResult<ThreadUnreadLookup> {
    Err(crate::ImError::unsupported(
        "message-mark-thread-read-local-state",
    ))
}

#[cfg(feature = "sqlite")]
async fn list_incoming_message_ids_for_legacy_fallback_async(
    client: &crate::core::ImClient,
    thread: &crate::messages::ThreadRef,
    watermark: Option<&crate::messages::ReadWatermark>,
    fallback_max_message_ids: Option<u32>,
) -> crate::ImResult<ThreadUnreadLookup> {
    let result = client
        .core_inner()
        .local_state_db()
        .await?
        .list_incoming_message_ids_up_to_watermark(
            client.current_identity().id.as_str(),
            client.did().as_str(),
            thread.clone(),
            watermark.and_then(|watermark| {
                watermark
                    .last_read_message_id
                    .as_ref()
                    .map(|id| id.as_str().to_owned())
            }),
            watermark.and_then(|watermark| watermark.last_read_thread_seq.clone()),
            thread_mark_read_limit(fallback_max_message_ids),
        )
        .await?;
    Ok(ThreadUnreadLookup {
        message_ids: result.message_ids,
        truncated: result.truncated,
    })
}

#[cfg(not(feature = "sqlite"))]
async fn list_incoming_message_ids_for_legacy_fallback_async(
    _client: &crate::core::ImClient,
    _thread: &crate::messages::ThreadRef,
    _watermark: Option<&crate::messages::ReadWatermark>,
    _fallback_max_message_ids: Option<u32>,
) -> crate::ImResult<ThreadUnreadLookup> {
    Err(crate::ImError::unsupported(
        "message-mark-thread-read-local-state",
    ))
}

#[cfg(all(feature = "sqlite", any(feature = "blocking", test)))]
fn classify_mark_read_ids(
    client: &crate::core::ImClient,
    ids: &[String],
) -> crate::ImResult<crate::internal::local_state::messages::MarkReadClassification> {
    let connection = crate::internal::local_state::open_writable(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    )?;
    crate::internal::local_state::messages::classify_mark_read_ids_for_owner_identity(
        &connection,
        client.current_identity().id.as_str(),
        client.did().as_str(),
        ids,
    )
}

#[cfg(all(feature = "sqlite", not(any(feature = "blocking", test))))]
fn classify_mark_read_ids(
    _client: &crate::core::ImClient,
    _ids: &[String],
) -> crate::ImResult<crate::internal::local_state::messages::MarkReadClassification> {
    Err(crate::ImError::unsupported("sync-message-mark-read"))
}

#[cfg(not(feature = "sqlite"))]
fn classify_mark_read_ids(
    _client: &crate::core::ImClient,
    ids: &[String],
) -> crate::ImResult<NoSqliteMarkReadClassification> {
    Ok(NoSqliteMarkReadClassification {
        direct_ids: ids.to_vec(),
        remote_direct_ids: ids.to_vec(),
        group_ids: Vec::new(),
        local_only_ids: Vec::new(),
    })
}

#[cfg(feature = "sqlite")]
async fn classify_mark_read_ids_async(
    client: &crate::core::ImClient,
    ids: &[String],
) -> crate::ImResult<crate::internal::local_state::messages::MarkReadClassification> {
    client
        .core_inner()
        .local_state_db()
        .await?
        .classify_mark_read_ids(
            client.current_identity().id.as_str(),
            client.did().as_str(),
            ids.to_vec(),
        )
        .await
}

#[cfg(not(feature = "sqlite"))]
async fn classify_mark_read_ids_async(
    _client: &crate::core::ImClient,
    ids: &[String],
) -> crate::ImResult<NoSqliteMarkReadClassification> {
    Ok(NoSqliteMarkReadClassification {
        direct_ids: ids.to_vec(),
        remote_direct_ids: ids.to_vec(),
        group_ids: Vec::new(),
        local_only_ids: Vec::new(),
    })
}

#[cfg(all(feature = "sqlite", any(feature = "blocking", test)))]
fn mark_local_messages_read(
    client: &crate::core::ImClient,
    classification: Result<
        &crate::internal::local_state::messages::MarkReadClassification,
        &crate::ImError,
    >,
) -> crate::ImResult<i64> {
    let classification = classification.map_err(Clone::clone)?;
    let local_ids = classification.local_ids();
    if local_ids.is_empty() {
        return Ok(0);
    }
    let connection = crate::internal::local_state::open_writable(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    )?;
    crate::internal::local_state::messages::mark_messages_read_for_owner_identity(
        &connection,
        client.current_identity().id.as_str(),
        client.did().as_str(),
        &local_ids,
    )
}

#[cfg(all(feature = "sqlite", not(any(feature = "blocking", test))))]
fn mark_local_messages_read(
    _client: &crate::core::ImClient,
    _classification: Result<
        &crate::internal::local_state::messages::MarkReadClassification,
        &crate::ImError,
    >,
) -> crate::ImResult<i64> {
    Err(crate::ImError::unsupported("sync-message-mark-read"))
}

#[cfg(feature = "sqlite")]
async fn mark_local_messages_read_async(
    client: &crate::core::ImClient,
    classification: Result<
        &crate::internal::local_state::messages::MarkReadClassification,
        &crate::ImError,
    >,
) -> crate::ImResult<i64> {
    let classification = classification.map_err(Clone::clone)?;
    let local_ids = classification.local_ids();
    if local_ids.is_empty() {
        return Ok(0);
    }
    client
        .core_inner()
        .local_state_db()
        .await?
        .mark_messages_read(
            client.current_identity().id.as_str(),
            client.did().as_str(),
            local_ids,
        )
        .await
}

#[cfg(not(feature = "sqlite"))]
async fn mark_local_messages_read_async(
    _client: &crate::core::ImClient,
    _classification: Result<&NoSqliteMarkReadClassification, &crate::ImError>,
) -> crate::ImResult<i64> {
    Ok(0)
}

#[cfg(not(feature = "sqlite"))]
fn mark_local_messages_read(
    _client: &crate::core::ImClient,
    _classification: Result<&NoSqliteMarkReadClassification, &crate::ImError>,
) -> crate::ImResult<i64> {
    Ok(0)
}

fn int_value(value: Option<&Value>, fallback: i64) -> i64 {
    match value {
        Some(Value::Number(number)) => number
            .as_i64()
            .or_else(|| number.as_u64().and_then(|value| i64::try_from(value).ok()))
            .or_else(|| number.as_f64().map(|value| value as i64))
            .unwrap_or(fallback),
        Some(Value::String(value)) => value.trim().parse().unwrap_or(fallback),
        _ => fallback,
    }
}

#[cfg(all(feature = "sqlite", any(feature = "blocking", test)))]
fn claim_read_send_sync(
    client: &crate::core::ImClient,
    operation_id: Option<&str>,
) -> crate::ImResult<Option<ClaimedReadSend>> {
    let Some(operation_id) = operation_id else {
        return Ok(None);
    };
    let connection = crate::internal::local_state::open_writable(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    )?;
    let record = crate::internal::local_state::sync_v2::claim_read_mutation_by_operation_id(
        &connection,
        client.current_identity().id.as_str(),
        operation_id,
        unix_time_i64(),
    )?;
    let Some(record) = record else {
        return Ok(None);
    };
    match claimed_read_send(record.clone()) {
        Ok(claimed) => Ok(Some(claimed)),
        Err(error) => {
            crate::internal::local_state::sync_v2::retry_local_mutation(
                &connection,
                client.current_identity().id.as_str(),
                &record.mutation_id,
                error_service_code(&error).unwrap_or("SYNC_LOCAL_OUTBOX_CORRUPT"),
                unix_time_i64().saturating_add(5),
            )?;
            Err(error)
        }
    }
}

#[cfg(all(feature = "sqlite", any(feature = "blocking", test)))]
fn retry_read_send_sync(
    client: &crate::core::ImClient,
    claimed: Option<&ClaimedReadSend>,
    code: &str,
) -> crate::ImResult<()> {
    let Some(claimed) = claimed else {
        return Ok(());
    };
    let connection = crate::internal::local_state::open_writable(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    )?;
    crate::internal::local_state::sync_v2::retry_local_mutation(
        &connection,
        client.current_identity().id.as_str(),
        &claimed.mutation_id,
        code,
        unix_time_i64().saturating_add(5),
    )
}

#[cfg(not(all(feature = "sqlite", any(feature = "blocking", test))))]
fn retry_read_send_sync(
    _client: &crate::core::ImClient,
    _claimed: Option<&ClaimedReadSend>,
    _code: &str,
) -> crate::ImResult<()> {
    Ok(())
}

#[cfg(not(all(feature = "sqlite", any(feature = "blocking", test))))]
fn claim_read_send_sync(
    _client: &crate::core::ImClient,
    _operation_id: Option<&str>,
) -> crate::ImResult<Option<ClaimedReadSend>> {
    Ok(None)
}

#[cfg(feature = "sqlite")]
async fn claim_read_send_async(
    client: &crate::core::ImClient,
    operation_id: Option<&str>,
) -> crate::ImResult<Option<ClaimedReadSend>> {
    let Some(operation_id) = operation_id else {
        return Ok(None);
    };
    let db = client.core_inner().local_state_db().await?;
    let record = db
        .claim_read_mutation_by_operation_id(
            client.current_identity().id.as_str(),
            operation_id,
            unix_time_i64(),
        )
        .await?;
    let Some(record) = record else {
        return Ok(None);
    };
    match claimed_read_send(record.clone()) {
        Ok(claimed) => Ok(Some(claimed)),
        Err(error) => {
            db.retry_local_mutation(
                client.current_identity().id.as_str(),
                &record.mutation_id,
                error_service_code(&error).unwrap_or("SYNC_LOCAL_OUTBOX_CORRUPT"),
                unix_time_i64().saturating_add(5),
            )
            .await?;
            Err(error)
        }
    }
}

#[cfg(feature = "sqlite")]
async fn retry_read_send_async(
    client: &crate::core::ImClient,
    claimed: Option<&ClaimedReadSend>,
    code: &str,
) -> crate::ImResult<()> {
    let Some(claimed) = claimed else {
        return Ok(());
    };
    client
        .core_inner()
        .local_state_db()
        .await?
        .retry_local_mutation(
            client.current_identity().id.as_str(),
            &claimed.mutation_id,
            code,
            unix_time_i64().saturating_add(5),
        )
        .await
}

#[cfg(not(feature = "sqlite"))]
async fn retry_read_send_async(
    _client: &crate::core::ImClient,
    _claimed: Option<&ClaimedReadSend>,
    _code: &str,
) -> crate::ImResult<()> {
    Ok(())
}

#[cfg(not(feature = "sqlite"))]
async fn claim_read_send_async(
    _client: &crate::core::ImClient,
    _operation_id: Option<&str>,
) -> crate::ImResult<Option<ClaimedReadSend>> {
    Ok(None)
}

fn claimed_read_send(
    record: crate::internal::local_state::sync_v2::LocalMutationRecord,
) -> crate::ImResult<ClaimedReadSend> {
    let payload: Value = serde_json::from_str(&record.payload_json).map_err(|error| {
        read_ack_error(
            "SYNC_LOCAL_OUTBOX_CORRUPT",
            format!("read outbox payload is invalid: {error}"),
        )
    })?;
    let field = |name: &str| {
        payload
            .get(name)
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty() && value.trim() == *value)
            .ok_or_else(|| {
                read_ack_error(
                    "SYNC_LOCAL_OUTBOX_CORRUPT",
                    format!("read outbox payload is missing {name}"),
                )
            })
    };
    let thread = match field("thread_kind")? {
        "direct" => {
            crate::messages::ThreadRef::Thread(crate::ids::ThreadId::parse(field("thread_id")?)?)
        }
        "group" => {
            crate::messages::ThreadRef::Group(crate::ids::GroupRef::parse(field("thread_id")?)?)
        }
        _ => {
            return Err(read_ack_error(
                "SYNC_LOCAL_OUTBOX_CORRUPT",
                "read outbox thread_kind must be direct or group",
            ))
        }
    };
    let seq = crate::internal::local_state::sync_state::normalize_decimal_seq(field(
        "read_watermark_seq",
    )?)
    .map_err(|_| {
        read_ack_error(
            "SYNC_LOCAL_OUTBOX_CORRUPT",
            "read outbox watermark must be a canonical decimal",
        )
    })?;
    let message_id = payload
        .get("read_watermark_message_id")
        .and_then(Value::as_str)
        .map(crate::ids::MessageId::parse)
        .transpose()?;
    let read_at = payload
        .get("read_watermark_at")
        .and_then(Value::as_str)
        .map(chrono::DateTime::parse_from_rfc3339)
        .transpose()
        .map_err(|_| {
            read_ack_error(
                "SYNC_LOCAL_OUTBOX_CORRUPT",
                "read outbox read_at must be RFC3339",
            )
        })?
        .map(|value| value.with_timezone(&chrono::Utc));
    Ok(ClaimedReadSend {
        mutation_id: record.mutation_id,
        operation_id: record.operation_id,
        thread,
        watermark: crate::messages::ReadWatermark {
            last_read_message_id: message_id,
            last_read_thread_seq: Some(seq),
            read_at,
        },
        remote_thread_key: field("remote_thread_key")?.to_owned(),
    })
}

fn validate_final_read_response(
    client: &crate::core::ImClient,
    raw: &Value,
    thread: &crate::messages::ThreadRef,
    remote_thread_key: Option<&str>,
    requested: &crate::messages::ReadWatermark,
) -> crate::ImResult<crate::internal::wire::read_state::MarkReadStateWireResponse> {
    let expected_thread =
        crate::internal::wire::read_state::read_state_thread_to_wire(thread, remote_thread_key)?;
    let response = crate::internal::wire::read_state::parse_mark_read_state_response(
        raw,
        client.did().as_str(),
        &expected_thread,
    )?;
    let requested_seq = requested
        .last_read_thread_seq
        .as_deref()
        .ok_or_else(|| read_ack_error("READ_STATE_INVALID_ACK", "request has no watermark"))?;
    let acknowledged_seq = response
        .read_watermark_server_seq
        .as_deref()
        .ok_or_else(|| read_ack_error("READ_STATE_INCOMPLETE_ACK", "response has no watermark"))?;
    if !response.remote_acknowledged
        || response.pending_remote_ack
        || response.partial
        || crate::internal::local_state::sync_v2::compare_decimal(acknowledged_seq, requested_seq)?
            == std::cmp::Ordering::Less
    {
        return Err(read_ack_error(
            "READ_STATE_INCOMPLETE_ACK",
            "response is not a final ACK for the sent watermark",
        ));
    }
    Ok(response)
}

fn read_ack_error(code: &str, message: impl Into<String>) -> crate::ImError {
    crate::ImError::Service {
        status_code: None,
        code: Some(code.to_owned()),
        message: message.into(),
        data: None,
    }
}

fn error_service_code(error: &crate::ImError) -> Option<&str> {
    match error {
        crate::ImError::Service {
            code: Some(code), ..
        } => Some(code),
        _ => None,
    }
}

fn waiting_successor_result(
    updated_count: i64,
    effective_watermark: Option<crate::messages::ReadWatermark>,
) -> MarkThreadReadRuntimeResult {
    MarkThreadReadRuntimeResult {
        sdk_result: crate::messages::MarkThreadReadResult {
            updated_count: u32_count_i64(updated_count),
            remote_acknowledged: false,
            partial: true,
            fallback_used: false,
            pending_remote_ack: true,
            effective_watermark,
            legacy_message_ids: Vec::new(),
            warnings: vec!["Read watermark is queued behind an in-flight predecessor".to_owned()],
        },
        raw: None,
        direct_ids: Vec::new(),
        group_ids: Vec::new(),
        local_only_ids: Vec::new(),
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

#[derive(Debug, Clone, PartialEq)]
struct LegacyThreadFallbackResult {
    message_ids: Vec<crate::ids::MessageId>,
    remote_acknowledged: bool,
    partial: bool,
    warnings: Vec<String>,
    raw: Option<Value>,
}

#[cfg(all(feature = "sqlite", any(feature = "blocking", test)))]
fn effective_watermark_sync(
    client: &crate::core::ImClient,
    thread: &crate::messages::ThreadRef,
    requested: Option<&crate::messages::ReadWatermark>,
) -> crate::ImResult<Option<crate::messages::ReadWatermark>> {
    if let Some(watermark) = normalize_requested_watermark(requested)? {
        return Ok(Some(watermark));
    }
    let connection = crate::internal::local_state::open_writable(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    )?;
    let seq =
        crate::internal::local_state::messages::max_server_seq_for_thread_ref_for_owner_identity(
            &connection,
            client.current_identity().id.as_str(),
            client.did().as_str(),
            thread,
        )?;
    Ok(seq.map(|seq| crate::messages::ReadWatermark {
        last_read_message_id: None,
        last_read_thread_seq: Some(seq.to_string()),
        read_at: Some(chrono::Utc::now()),
    }))
}

#[cfg(all(feature = "sqlite", not(any(feature = "blocking", test))))]
fn effective_watermark_sync(
    _client: &crate::core::ImClient,
    _thread: &crate::messages::ThreadRef,
    requested: Option<&crate::messages::ReadWatermark>,
) -> crate::ImResult<Option<crate::messages::ReadWatermark>> {
    normalize_requested_watermark(requested)
}

#[cfg(not(feature = "sqlite"))]
fn effective_watermark_sync(
    _client: &crate::core::ImClient,
    _thread: &crate::messages::ThreadRef,
    requested: Option<&crate::messages::ReadWatermark>,
) -> crate::ImResult<Option<crate::messages::ReadWatermark>> {
    normalize_requested_watermark(requested)
}

#[cfg(feature = "sqlite")]
async fn effective_watermark_async(
    client: &crate::core::ImClient,
    thread: &crate::messages::ThreadRef,
    requested: Option<&crate::messages::ReadWatermark>,
) -> crate::ImResult<Option<crate::messages::ReadWatermark>> {
    if let Some(watermark) = normalize_requested_watermark(requested)? {
        return Ok(Some(watermark));
    }
    let seq = client
        .core_inner()
        .local_state_db()
        .await?
        .max_server_seq_for_thread_ref(
            client.current_identity().id.as_str(),
            client.did().as_str(),
            thread.clone(),
        )
        .await?;
    Ok(seq.map(|seq| crate::messages::ReadWatermark {
        last_read_message_id: None,
        last_read_thread_seq: Some(seq.to_string()),
        read_at: Some(chrono::Utc::now()),
    }))
}

#[cfg(not(feature = "sqlite"))]
async fn effective_watermark_async(
    _client: &crate::core::ImClient,
    _thread: &crate::messages::ThreadRef,
    requested: Option<&crate::messages::ReadWatermark>,
) -> crate::ImResult<Option<crate::messages::ReadWatermark>> {
    normalize_requested_watermark(requested)
}

fn normalize_requested_watermark(
    requested: Option<&crate::messages::ReadWatermark>,
) -> crate::ImResult<Option<crate::messages::ReadWatermark>> {
    let Some(watermark) = requested else {
        return Ok(None);
    };
    let seq = watermark
        .last_read_thread_seq
        .as_deref()
        .map(normalize_decimal_seq)
        .transpose()?;
    let message_id = watermark.last_read_message_id.clone();
    if seq.is_none() && message_id.is_none() {
        return Ok(None);
    }
    Ok(Some(crate::messages::ReadWatermark {
        last_read_message_id: message_id,
        last_read_thread_seq: seq,
        read_at: watermark.read_at.or_else(|| Some(chrono::Utc::now())),
    }))
}

#[cfg(all(feature = "sqlite", any(feature = "blocking", test)))]
fn mark_thread_read_watermark_local(
    client: &crate::core::ImClient,
    thread: &crate::messages::ThreadRef,
    watermark: Option<&crate::messages::ReadWatermark>,
    pending_remote_ack: bool,
) -> crate::ImResult<crate::internal::local_state::messages::MarkThreadReadWatermarkResult> {
    let connection = crate::internal::local_state::open_writable(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    )?;
    crate::internal::local_state::sync_v2::mark_thread_read_and_update_outbox(
        &connection,
        client.current_identity().id.as_str(),
        client.did().as_str(),
        local_watermark_input(thread, watermark, pending_remote_ack),
    )
}

#[cfg(all(feature = "sqlite", not(any(feature = "blocking", test))))]
fn mark_thread_read_watermark_local(
    _client: &crate::core::ImClient,
    thread: &crate::messages::ThreadRef,
    watermark: Option<&crate::messages::ReadWatermark>,
    pending_remote_ack: bool,
) -> crate::ImResult<crate::internal::local_state::messages::MarkThreadReadWatermarkResult> {
    let _ = (thread, watermark, pending_remote_ack);
    Err(crate::ImError::unsupported("sync-message-mark-thread-read"))
}

#[cfg(not(feature = "sqlite"))]
fn mark_thread_read_watermark_local(
    _client: &crate::core::ImClient,
    thread: &crate::messages::ThreadRef,
    watermark: Option<&crate::messages::ReadWatermark>,
    pending_remote_ack: bool,
) -> crate::ImResult<NoSqliteThreadReadWatermarkResult> {
    let _ = (thread, watermark, pending_remote_ack);
    Ok(NoSqliteThreadReadWatermarkResult { updated_count: 0 })
}

#[cfg(feature = "sqlite")]
async fn mark_thread_read_watermark_local_async(
    client: &crate::core::ImClient,
    thread: &crate::messages::ThreadRef,
    watermark: Option<&crate::messages::ReadWatermark>,
    pending_remote_ack: bool,
) -> crate::ImResult<crate::internal::local_state::messages::MarkThreadReadWatermarkResult> {
    client
        .core_inner()
        .local_state_db()
        .await?
        .mark_thread_read_watermark(
            client.current_identity().id.as_str(),
            client.did().as_str(),
            local_watermark_input(thread, watermark, pending_remote_ack),
        )
        .await
}

#[cfg(not(feature = "sqlite"))]
async fn mark_thread_read_watermark_local_async(
    _client: &crate::core::ImClient,
    thread: &crate::messages::ThreadRef,
    watermark: Option<&crate::messages::ReadWatermark>,
    pending_remote_ack: bool,
) -> crate::ImResult<NoSqliteThreadReadWatermarkResult> {
    let _ = (thread, watermark, pending_remote_ack);
    Ok(NoSqliteThreadReadWatermarkResult { updated_count: 0 })
}

#[cfg(feature = "sqlite")]
fn local_watermark_input(
    thread: &crate::messages::ThreadRef,
    watermark: Option<&crate::messages::ReadWatermark>,
    pending_remote_ack: bool,
) -> crate::internal::local_state::messages::MarkThreadReadWatermarkInput {
    crate::internal::local_state::messages::MarkThreadReadWatermarkInput {
        thread: thread.clone(),
        read_watermark_message_id: watermark
            .and_then(|watermark| watermark.last_read_message_id.as_ref())
            .map(|id| id.as_str().to_owned()),
        read_watermark_seq: watermark.and_then(|watermark| watermark.last_read_thread_seq.clone()),
        read_watermark_at: watermark
            .and_then(|watermark| watermark.read_at)
            .map(|value| value.to_rfc3339()),
        pending_remote_ack,
    }
}

fn mark_read_state_remote_sync<P, T>(
    client: &crate::core::ImClient,
    session_provider: &mut P,
    transport: &mut T,
    thread: &crate::messages::ThreadRef,
    watermark: Option<&crate::messages::ReadWatermark>,
    fallback_max_message_ids: Option<u32>,
    operation_id: Option<&str>,
    remote_thread_key: Option<&str>,
) -> crate::ImResult<Value>
where
    P: SessionProvider,
    T: AuthenticatedRpcTransport,
{
    let Some(watermark) = watermark else {
        return Err(crate::ImError::invalid_input(
            Some("watermark".to_owned()),
            "read watermark is required",
        ));
    };
    session_provider.ensure_session(crate::auth::AuthScope::Messaging)?;
    let params = crate::internal::wire::read_state::build_mark_read_state_rpc_params(
        &crate::internal::wire::common::WireIdentity {
            did: client.did().as_str().to_string(),
        },
        crate::internal::wire::read_state::MarkReadStateWireRequest {
            thread: thread.clone(),
            read_up_to_server_seq: watermark.last_read_thread_seq.clone(),
            read_up_to_message_id: watermark
                .last_read_message_id
                .as_ref()
                .map(|id| id.as_str().to_owned()),
            client_observed_at: watermark.read_at.map(|value| value.to_rfc3339()),
            fallback_max_message_ids,
            device_id: client.current_identity().device_id.clone(),
            operation_id: operation_id.map(ToOwned::to_owned),
            remote_thread_key: remote_thread_key.map(ToOwned::to_owned),
        },
    )?;
    transport.authenticated_rpc(
        super::read::MESSAGE_RPC_ENDPOINT,
        "read_state.mark_read",
        params,
    )
}

async fn mark_read_state_remote_async<P, T>(
    client: &crate::core::ImClient,
    session_provider: &mut P,
    transport: &mut T,
    thread: &crate::messages::ThreadRef,
    watermark: Option<&crate::messages::ReadWatermark>,
    fallback_max_message_ids: Option<u32>,
    operation_id: Option<&str>,
    remote_thread_key: Option<&str>,
) -> crate::ImResult<Value>
where
    P: AsyncSessionProvider,
    T: AsyncAuthenticatedRpcTransport,
{
    let Some(watermark) = watermark else {
        return Err(crate::ImError::invalid_input(
            Some("watermark".to_owned()),
            "read watermark is required",
        ));
    };
    session_provider
        .ensure_session(crate::auth::AuthScope::Messaging)
        .await?;
    let params = crate::internal::wire::read_state::build_mark_read_state_rpc_params(
        &crate::internal::wire::common::WireIdentity {
            did: client.did().as_str().to_string(),
        },
        crate::internal::wire::read_state::MarkReadStateWireRequest {
            thread: thread.clone(),
            read_up_to_server_seq: watermark.last_read_thread_seq.clone(),
            read_up_to_message_id: watermark
                .last_read_message_id
                .as_ref()
                .map(|id| id.as_str().to_owned()),
            client_observed_at: watermark.read_at.map(|value| value.to_rfc3339()),
            fallback_max_message_ids,
            device_id: client.current_identity().device_id.clone(),
            operation_id: operation_id.map(ToOwned::to_owned),
            remote_thread_key: remote_thread_key.map(ToOwned::to_owned),
        },
    )?;
    transport
        .authenticated_rpc(
            super::read::MESSAGE_RPC_ENDPOINT,
            "read_state.mark_read",
            params,
        )
        .await
}

fn legacy_fallback_mark_thread_read_sync<P, T>(
    client: &crate::core::ImClient,
    session_provider: &mut P,
    transport: &mut T,
    thread: &crate::messages::ThreadRef,
    watermark: Option<&crate::messages::ReadWatermark>,
    fallback_max_message_ids: Option<u32>,
) -> crate::ImResult<LegacyThreadFallbackResult>
where
    P: SessionProvider,
    T: AuthenticatedRpcTransport,
{
    let lookup = list_incoming_message_ids_for_legacy_fallback(
        client,
        thread,
        watermark,
        fallback_max_message_ids,
    )?;
    let mut warnings = truncated_warnings(lookup.truncated);
    if lookup.message_ids.is_empty() {
        return Ok(LegacyThreadFallbackResult {
            message_ids: Vec::new(),
            remote_acknowledged: false,
            partial: lookup.truncated,
            warnings,
            raw: None,
        });
    }
    let classification = classify_mark_read_ids(client, &lookup.message_ids);
    let remote_direct_ids = classification
        .as_ref()
        .map(|value| value.remote_direct_ids.clone())
        .unwrap_or_else(|_| lookup.message_ids.clone());
    let has_group_or_local = classification
        .as_ref()
        .map(|value| !value.group_ids.is_empty() || !value.local_only_ids.is_empty())
        .unwrap_or(false);
    let mut remote_acknowledged = false;
    let mut raw = None;
    if !remote_direct_ids.is_empty() {
        match mark_direct_ids_remote_sync(client, session_provider, transport, &remote_direct_ids) {
            Ok((_updated, response)) => {
                warnings.extend(warnings_from_raw(&response));
                raw = Some(response);
                remote_acknowledged = true;
            }
            Err(error) => {
                warnings.push(format!("Legacy remote mark-read failed: {error}"));
            }
        }
    }
    Ok(LegacyThreadFallbackResult {
        message_ids: parse_message_ids(&lookup.message_ids)?,
        remote_acknowledged,
        partial: lookup.truncated || has_group_or_local || !remote_acknowledged,
        warnings,
        raw,
    })
}

async fn legacy_fallback_mark_thread_read_async<P, T>(
    client: &crate::core::ImClient,
    session_provider: &mut P,
    transport: &mut T,
    thread: &crate::messages::ThreadRef,
    watermark: Option<&crate::messages::ReadWatermark>,
    fallback_max_message_ids: Option<u32>,
) -> crate::ImResult<LegacyThreadFallbackResult>
where
    P: AsyncSessionProvider,
    T: AsyncAuthenticatedRpcTransport,
{
    let lookup = list_incoming_message_ids_for_legacy_fallback_async(
        client,
        thread,
        watermark,
        fallback_max_message_ids,
    )
    .await?;
    let mut warnings = truncated_warnings(lookup.truncated);
    if lookup.message_ids.is_empty() {
        return Ok(LegacyThreadFallbackResult {
            message_ids: Vec::new(),
            remote_acknowledged: false,
            partial: lookup.truncated,
            warnings,
            raw: None,
        });
    }
    let classification = classify_mark_read_ids_async(client, &lookup.message_ids).await;
    let remote_direct_ids = classification
        .as_ref()
        .map(|value| value.remote_direct_ids.clone())
        .unwrap_or_else(|_| lookup.message_ids.clone());
    let has_group_or_local = classification
        .as_ref()
        .map(|value| !value.group_ids.is_empty() || !value.local_only_ids.is_empty())
        .unwrap_or(false);
    let mut remote_acknowledged = false;
    let mut raw = None;
    if !remote_direct_ids.is_empty() {
        match mark_direct_ids_remote_async(client, session_provider, transport, &remote_direct_ids)
            .await
        {
            Ok((_updated, response)) => {
                warnings.extend(warnings_from_raw(&response));
                raw = Some(response);
                remote_acknowledged = true;
            }
            Err(error) => {
                warnings.push(format!("Legacy remote mark-read failed: {error}"));
            }
        }
    }
    Ok(LegacyThreadFallbackResult {
        message_ids: parse_message_ids(&lookup.message_ids)?,
        remote_acknowledged,
        partial: lookup.truncated || has_group_or_local || !remote_acknowledged,
        warnings,
        raw,
    })
}

fn mark_direct_ids_remote_sync<P, T>(
    client: &crate::core::ImClient,
    session_provider: &mut P,
    transport: &mut T,
    direct_ids: &[String],
) -> crate::ImResult<(i64, Value)>
where
    P: SessionProvider,
    T: AuthenticatedRpcTransport,
{
    session_provider.ensure_session(crate::auth::AuthScope::Messaging)?;
    let params = crate::internal::wire::inbox::build_mark_read_rpc_params(
        &crate::internal::wire::common::WireIdentity {
            did: client.did().as_str().to_string(),
        },
        crate::internal::wire::inbox::MarkReadWireRequest {
            message_ids: direct_ids.to_vec(),
        },
    )?;
    let response = transport.authenticated_rpc(
        super::read::MESSAGE_RPC_ENDPOINT,
        "inbox.mark_read",
        params,
    )?;
    Ok((
        int_value(response.get("updated_count"), direct_ids.len() as i64),
        response,
    ))
}

async fn mark_direct_ids_remote_async<P, T>(
    client: &crate::core::ImClient,
    session_provider: &mut P,
    transport: &mut T,
    direct_ids: &[String],
) -> crate::ImResult<(i64, Value)>
where
    P: AsyncSessionProvider,
    T: AsyncAuthenticatedRpcTransport,
{
    session_provider
        .ensure_session(crate::auth::AuthScope::Messaging)
        .await?;
    let params = crate::internal::wire::inbox::build_mark_read_rpc_params(
        &crate::internal::wire::common::WireIdentity {
            did: client.did().as_str().to_string(),
        },
        crate::internal::wire::inbox::MarkReadWireRequest {
            message_ids: direct_ids.to_vec(),
        },
    )?;
    let response = transport
        .authenticated_rpc(super::read::MESSAGE_RPC_ENDPOINT, "inbox.mark_read", params)
        .await?;
    Ok((
        int_value(response.get("updated_count"), direct_ids.len() as i64),
        response,
    ))
}

fn parse_message_ids(ids: &[String]) -> crate::ImResult<Vec<crate::ids::MessageId>> {
    ids.iter()
        .map(crate::ids::MessageId::parse)
        .collect::<crate::ImResult<Vec<_>>>()
}

fn thread_mark_read_limit(max_message_ids: Option<u32>) -> i64 {
    i64::from(max_message_ids.unwrap_or(500).clamp(1, 500))
}

fn truncated_warnings(truncated: bool) -> Vec<String> {
    if truncated {
        vec!["Local unread ids were truncated by fallback_max_message_ids".to_string()]
    } else {
        Vec::new()
    }
}

fn u32_count_i64(value: i64) -> u32 {
    u32::try_from(value.max(0)).unwrap_or(u32::MAX)
}

fn normalize_decimal_seq(value: &str) -> crate::ImResult<String> {
    crate::internal::local_state::sync_state::normalize_decimal_seq(value).map_err(|_| {
        crate::ImError::invalid_input(
            Some("read_watermark_seq".to_owned()),
            format!("read_watermark_seq must be a non-negative decimal string, got {value:?}"),
        )
    })
}

fn is_read_state_unsupported_error(error: &crate::ImError) -> bool {
    match error {
        crate::ImError::UnsupportedCapability { capability } => {
            capability.contains("read-state") || capability.contains("read_state")
        }
        crate::ImError::Service {
            status_code,
            code,
            message,
            ..
        } => {
            let code = code.as_deref().unwrap_or_default();
            let message = message.to_ascii_lowercase();
            matches!(status_code, Some(404))
                || code == "-32601"
                || code == "4210"
                || code == "read_state.unsupported"
                || message.contains("method not found")
                || message.contains("read_state.unsupported")
        }
        _ => false,
    }
}

fn warnings_from_raw(value: &Value) -> Vec<String> {
    value
        .get("warnings")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(not(feature = "sqlite"))]
struct NoSqliteMarkReadClassification {
    direct_ids: Vec<String>,
    remote_direct_ids: Vec<String>,
    group_ids: Vec<String>,
    local_only_ids: Vec<String>,
}

#[cfg(not(feature = "sqlite"))]
struct NoSqliteThreadReadWatermarkResult {
    updated_count: i64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::internal::auth::session::SessionProvider;
    use crate::internal::transport::AuthenticatedRpcTransport;
    use serde_json::{json, Value};
    use std::cell::RefCell;
    use std::fs;
    use std::path::PathBuf;
    use std::rc::Rc;

    #[test]
    fn mark_read_runtime_marks_direct_remote_and_local_rows() {
        let fixture = Fixture::new();
        let client = fixture.client();
        fixture.seed_message(&client, "direct-1", "", "text/plain", "", 0);
        let calls = Rc::new(RefCell::new(Vec::new()));

        let result = MessageMarkReadRuntime::new(
            &client,
            ReadySessionProvider,
            RecordingTransport {
                calls: Rc::clone(&calls),
                response: json!({"updated_count": 3}),
            },
        )
        .mark_read(MarkReadInput {
            message_ids: vec![crate::ids::MessageId::parse("direct-1").unwrap()],
        })
        .unwrap();

        assert_eq!(result.sdk_result.updated_count, 3);
        assert_eq!(result.direct_ids, vec!["direct-1"]);
        assert_eq!(fixture.is_read(&client, "direct-1"), 1);
        let calls = calls.borrow();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].endpoint, super::super::read::MESSAGE_RPC_ENDPOINT);
        assert_eq!(calls[0].method, "inbox.mark_read");
        assert_eq!(calls[0].params["body"]["message_ids"], json!(["direct-1"]));
    }

    #[test]
    fn mark_read_runtime_keeps_group_and_mail_local_only() {
        let fixture = Fixture::new();
        let client = fixture.client();
        fixture.seed_message(&client, "group-1", "did:example:group", "text/plain", "", 0);
        fixture.seed_message(
            &client,
            "mail-1",
            "",
            "mail.notification",
            r#"{"source_kind":"mail"}"#,
            0,
        );
        let calls = Rc::new(RefCell::new(Vec::new()));

        let result = MessageMarkReadRuntime::new(
            &client,
            ReadySessionProvider,
            RecordingTransport {
                calls: Rc::clone(&calls),
                response: json!({}),
            },
        )
        .mark_read(MarkReadInput {
            message_ids: vec![
                crate::ids::MessageId::parse("group-1").unwrap(),
                crate::ids::MessageId::parse("mail-1").unwrap(),
            ],
        })
        .unwrap();

        assert_eq!(result.sdk_result.updated_count, 2);
        assert!(result.direct_ids.is_empty());
        assert_eq!(result.group_ids, vec!["group-1"]);
        assert_eq!(result.local_only_ids, vec!["mail-1"]);
        assert!(calls.borrow().is_empty());
        assert_eq!(fixture.is_read(&client, "did:example:group:1"), 1);
        assert_eq!(fixture.is_read(&client, "mail-1"), 1);
    }

    #[test]
    fn mark_thread_read_runtime_marks_local_unread_ids_without_history_rpc() {
        let fixture = Fixture::new();
        let client = fixture.client();
        fixture.seed_message(&client, "direct-thread-1", "", "text/plain", "", 0);
        let calls = Rc::new(RefCell::new(Vec::new()));

        let result = MessageMarkReadRuntime::new(
            &client,
            ReadySessionProvider,
            RecordingTransport {
                calls: Rc::clone(&calls),
                response: json!({"updated_count": 1}),
            },
        )
        .mark_thread_read(MarkThreadReadInput {
            request: crate::messages::MarkThreadReadRequest {
                thread: crate::messages::ThreadRef::Direct(
                    crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
                ),
                watermark: None,
                fallback_max_message_ids: None,
            },
            remote_thread: None,
        })
        .unwrap();

        assert_eq!(result.sdk_result.updated_count, 1);
        assert!(result.sdk_result.remote_acknowledged);
        assert!(!result.sdk_result.partial);
        assert!(!result.sdk_result.fallback_used);
        assert!(!result.sdk_result.pending_remote_ack);
        assert_eq!(
            result
                .sdk_result
                .effective_watermark
                .as_ref()
                .and_then(|watermark| watermark.last_read_thread_seq.as_deref()),
            Some("1")
        );
        assert!(result.sdk_result.legacy_message_ids.is_empty());
        assert_eq!(fixture.is_read(&client, "direct-thread-1"), 1);
        let calls = calls.borrow();
        assert_eq!(
            calls
                .iter()
                .map(|call| call.method.as_str())
                .collect::<Vec<_>>(),
            vec!["read_state.mark_read"]
        );
        assert_eq!(
            calls[0].params["meta"]["profile"],
            "anp.read_state.local.v1"
        );
        assert_eq!(calls[0].params["body"]["read_up_to_server_seq"], "1");
        assert_eq!(calls[0].params["body"]["thread"]["kind"], "direct");
        assert_eq!(
            calls[0].params["body"]["thread"]["peer_did"],
            "did:example:bob"
        );
        assert!(calls[0].params["body"].get("event_seq").is_none());
        assert!(calls[0].params["body"]
            .get("read_up_to_group_event_seq")
            .is_none());
    }

    #[test]
    fn mark_thread_read_without_explicit_watermark_stops_at_latest_hydrated_message() {
        let fixture = Fixture::new();
        let client = fixture.client();
        fixture.seed_message_with_seq(&client, "hydrated-5", "", "text/plain", "", 0, 5);
        fixture.seed_message_with_seq(&client, "discovered-10", "", "text/plain", "", 0, 10);
        let connection = crate::internal::local_state::open_writable(
            &client.core_inner().sdk_paths().local_state.sqlite_path,
        )
        .unwrap();
        connection
            .execute(
                "UPDATE messages SET content = '', hydration_state = 'discovered' WHERE owner_identity_id = 'alice-id' AND msg_id = 'discovered-10'",
                [],
            )
            .unwrap();
        drop(connection);
        let calls = Rc::new(RefCell::new(Vec::new()));

        let result = MessageMarkReadRuntime::new(
            &client,
            ReadySessionProvider,
            RecordingTransport {
                calls: Rc::clone(&calls),
                response: json!({"updated_count": 1}),
            },
        )
        .mark_thread_read(MarkThreadReadInput {
            request: crate::messages::MarkThreadReadRequest {
                thread: crate::messages::ThreadRef::Direct(
                    crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
                ),
                watermark: None,
                fallback_max_message_ids: None,
            },
            remote_thread: None,
        })
        .unwrap();

        assert_eq!(result.sdk_result.updated_count, 1);
        assert_eq!(
            result
                .sdk_result
                .effective_watermark
                .as_ref()
                .and_then(|watermark| watermark.last_read_thread_seq.as_deref()),
            Some("5")
        );
        assert_eq!(fixture.is_read(&client, "hydrated-5"), 1);
        assert_eq!(fixture.is_read(&client, "discovered-10"), 0);
        assert_eq!(
            calls.borrow()[0].params["body"]["read_up_to_server_seq"],
            "5"
        );

        let connection = crate::internal::local_state::open_writable(
            &client.core_inner().sdk_paths().local_state.sqlite_path,
        )
        .unwrap();
        crate::internal::local_state::messages::upsert_message(
            &connection,
            &crate::internal::local_state::messages::MessageRecord {
                msg_id: "discovered-10".to_owned(),
                owner_identity_id: "alice-id".to_owned(),
                owner_did: "did:example:alice".to_owned(),
                conversation_id: "dm:did:example:bob".to_owned(),
                thread_id: "dm:did:example:bob".to_owned(),
                direction: 0,
                sender_did: "did:example:bob".to_owned(),
                receiver_did: "did:example:alice".to_owned(),
                content_type: "text/plain".to_owned(),
                content: "now visible".to_owned(),
                server_seq: Some(10),
                hydration_state:
                    crate::internal::local_state::messages::MessageHydrationState::Hydrated,
                sent_at: "2026-05-21T00:00:10Z".to_owned(),
                stored_at: "2026-05-21T00:00:10Z".to_owned(),
                is_read: false,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(fixture.is_read(&client, "discovered-10"), 0);
    }

    #[test]
    fn mark_thread_read_runtime_uses_remote_thread_override_for_read_state_wire() {
        let fixture = Fixture::new();
        let client = fixture.client();
        fixture.seed_message(&client, "direct-thread-storage-1", "", "text/plain", "", 0);
        let calls = Rc::new(RefCell::new(Vec::new()));

        let result = MessageMarkReadRuntime::new(
            &client,
            ReadySessionProvider,
            RecordingTransport {
                calls: Rc::clone(&calls),
                response: json!({"updated_count": 1}),
            },
        )
        .mark_thread_read(MarkThreadReadInput {
            request: crate::messages::MarkThreadReadRequest {
                thread: crate::messages::ThreadRef::Thread(
                    crate::ids::ThreadId::parse("dm:did:example:bob").unwrap(),
                ),
                watermark: None,
                fallback_max_message_ids: None,
            },
            remote_thread: Some(crate::messages::ThreadRef::Direct(
                crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
            )),
        })
        .unwrap();

        assert_eq!(result.sdk_result.updated_count, 1);
        assert!(result.sdk_result.remote_acknowledged);
        assert!(!result.sdk_result.pending_remote_ack);
        assert_eq!(fixture.is_read(&client, "direct-thread-storage-1"), 1);
        let calls = calls.borrow();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].method, "read_state.mark_read");
        assert_eq!(calls[0].params["body"]["thread"]["kind"], "direct");
        assert_eq!(
            calls[0].params["body"]["thread"]["peer_did"],
            "did:example:bob"
        );
        assert!(calls[0].params["body"]["thread"].get("thread_id").is_none());
    }

    #[tokio::test]
    async fn mark_thread_read_emits_conversation_store_patch_after_local_commit() {
        let fixture = Fixture::new();
        let client = fixture.client();
        fixture.seed_projected_message(&client, "direct-thread-patch", "", 0);
        let mut patches = client.messages().watch_conversation_patches().unwrap();
        let _initial_hydrate = patches.next_patch().await.unwrap();
        let calls = Rc::new(RefCell::new(Vec::new()));

        MessageMarkReadRuntime::new(
            &client,
            ReadySessionProvider,
            RecordingTransport {
                calls,
                response: json!({"updated_count": 1}),
            },
        )
        .mark_thread_read(MarkThreadReadInput {
            request: crate::messages::MarkThreadReadRequest {
                thread: crate::messages::ThreadRef::Direct(
                    crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
                ),
                watermark: None,
                fallback_max_message_ids: None,
            },
            remote_thread: None,
        })
        .unwrap();
        let patch = patches.next_patch().await.unwrap();

        match patch {
            crate::messages::ConversationStorePatch::Upsert {
                owner_identity_id,
                owner_did,
                unread_total,
                item,
                ..
            } => {
                assert_eq!(owner_identity_id, "alice-id");
                assert_eq!(owner_did, "did:example:alice");
                assert_eq!(unread_total, 0);
                assert_eq!(item.thread_id, "did:example:bob");
                assert_eq!(item.unread_count, 0);
            }
            other => panic!("expected unread upsert patch, got {other:?}"),
        }
    }

    #[test]
    fn mark_thread_read_runtime_empty_unread_does_not_call_remote() {
        let fixture = Fixture::new();
        let client = fixture.client();
        fixture.seed_message(&client, "direct-thread-read", "", "text/plain", "", 1);
        let calls = Rc::new(RefCell::new(Vec::new()));

        let result = MessageMarkReadRuntime::new(
            &client,
            ReadySessionProvider,
            RecordingTransport {
                calls: Rc::clone(&calls),
                response: json!({"updated_count": 1}),
            },
        )
        .mark_thread_read(MarkThreadReadInput {
            request: crate::messages::MarkThreadReadRequest {
                thread: crate::messages::ThreadRef::Direct(
                    crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
                ),
                watermark: None,
                fallback_max_message_ids: None,
            },
            remote_thread: None,
        })
        .unwrap();

        assert_eq!(result.sdk_result.updated_count, 0);
        assert!(result.sdk_result.legacy_message_ids.is_empty());
        assert!(result.sdk_result.remote_acknowledged);
        assert_eq!(calls.borrow().len(), 1);
        assert_eq!(calls.borrow()[0].method, "read_state.mark_read");
    }

    #[test]
    fn mark_thread_read_runtime_remote_failure_is_partial_after_local_update() {
        let fixture = Fixture::new();
        let client = fixture.client();
        fixture.seed_message(&client, "direct-thread-fail", "", "text/plain", "", 0);
        let calls = Rc::new(RefCell::new(Vec::new()));

        let result = MessageMarkReadRuntime::new(
            &client,
            ReadySessionProvider,
            FailingTransport {
                calls: Rc::clone(&calls),
            },
        )
        .mark_thread_read(MarkThreadReadInput {
            request: crate::messages::MarkThreadReadRequest {
                thread: crate::messages::ThreadRef::Direct(
                    crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
                ),
                watermark: None,
                fallback_max_message_ids: None,
            },
            remote_thread: None,
        })
        .unwrap();

        assert_eq!(result.sdk_result.updated_count, 1);
        assert!(!result.sdk_result.remote_acknowledged);
        assert!(result.sdk_result.partial);
        assert!(result.sdk_result.pending_remote_ack);
        assert!(result
            .sdk_result
            .warnings
            .iter()
            .any(|warning| warning.contains("Remote read-state mark-read failed")));
        assert_eq!(fixture.is_read(&client, "direct-thread-fail"), 1);
        assert_eq!(calls.borrow().len(), 1);
        assert_eq!(calls.borrow()[0].method, "read_state.mark_read");
    }

    #[test]
    fn mark_thread_read_runtime_rejects_pseudo_ack() {
        let fixture = Fixture::new();
        let client = fixture.client();
        fixture.seed_message(&client, "direct-thread-pseudo-ack", "", "text/plain", "", 0);
        let response = json!({
            "user_did": client.did().as_str(),
            "thread": {"kind": "direct", "peer_did": "did:example:bob"},
            "updated_count": 0,
            "remote_acknowledged": false,
            "partial": false,
            "fallback_used": false,
            "pending_remote_ack": true,
            "read_watermark_server_seq": "1",
            "previous_read_watermark_server_seq": null,
            "read_watermark_message_id": "direct-thread-pseudo-ack",
            "advanced": false,
            "read_at": "2026-07-28T12:00:00Z",
            "unread_count": null,
            "warnings": []
        });
        let result = MessageMarkReadRuntime::new(
            &client,
            ReadySessionProvider,
            RecordingTransport {
                calls: Rc::new(RefCell::new(Vec::new())),
                response,
            },
        )
        .mark_thread_read(MarkThreadReadInput {
            request: crate::messages::MarkThreadReadRequest {
                thread: crate::messages::ThreadRef::Direct(
                    crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
                ),
                watermark: None,
                fallback_max_message_ids: None,
            },
            remote_thread: None,
        })
        .unwrap();
        assert!(!result.sdk_result.remote_acknowledged);
        assert!(result.sdk_result.partial);
        assert!(result.sdk_result.pending_remote_ack);
        assert_eq!(fixture.is_read(&client, "direct-thread-pseudo-ack"), 1);
    }

    #[test]
    fn mark_thread_read_runtime_unsupported_read_state_falls_back_to_legacy_direct() {
        let fixture = Fixture::new();
        let client = fixture.client();
        fixture.seed_message_with_seq(
            &client,
            "direct-thread-fallback-1",
            "",
            "text/plain",
            "",
            0,
            1,
        );
        fixture.seed_message_with_seq(
            &client,
            "direct-thread-fallback-2",
            "",
            "text/plain",
            "",
            0,
            2,
        );
        let calls = Rc::new(RefCell::new(Vec::new()));

        let result = MessageMarkReadRuntime::new(
            &client,
            ReadySessionProvider,
            UnsupportedReadStateThenLegacyTransport {
                calls: Rc::clone(&calls),
            },
        )
        .mark_thread_read(MarkThreadReadInput {
            request: crate::messages::MarkThreadReadRequest {
                thread: crate::messages::ThreadRef::Direct(
                    crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
                ),
                watermark: None,
                fallback_max_message_ids: Some(1),
            },
            remote_thread: None,
        })
        .unwrap();

        assert_eq!(result.sdk_result.updated_count, 2);
        assert!(result.sdk_result.remote_acknowledged);
        assert!(result.sdk_result.fallback_used);
        assert!(!result.sdk_result.pending_remote_ack);
        assert!(result.sdk_result.partial);
        assert_eq!(
            result
                .sdk_result
                .legacy_message_ids
                .iter()
                .map(|id| id.as_str())
                .collect::<Vec<_>>(),
            vec!["direct-thread-fallback-2"]
        );
        assert_eq!(fixture.is_read(&client, "direct-thread-fallback-1"), 1);
        assert_eq!(fixture.is_read(&client, "direct-thread-fallback-2"), 1);
        let calls = calls.borrow();
        assert_eq!(
            calls
                .iter()
                .map(|call| call.method.as_str())
                .collect::<Vec<_>>(),
            vec!["read_state.mark_read", "inbox.mark_read"]
        );
        assert_eq!(
            calls[1].params["body"]["message_ids"],
            json!(["direct-thread-fallback-2"])
        );
    }

    #[tokio::test]
    async fn mark_read_runtime_async_marks_direct_remote_and_actor_local_rows() {
        let fixture = Fixture::new();
        let client = fixture.client();
        fixture.seed_message(
            &client,
            "direct-async-1",
            "",
            "text/plain",
            &json!({
                "raw_message_id": "p5-v2-delivery:direct-async-1",
                "p5_cache_profile": anp::direct_e2ee::DIRECT_E2EE_PROFILE_V2,
                "p5_cache_sender_did": "did:example:bob",
                "p5_cache_sender_device_id": "device-bob",
                "p5_cache_recipient_did": "did:example:alice",
                "p5_cache_recipient_device_id": "device-alice",
                "p5_cache_binding_digest": "sha256:test-binding",
            })
            .to_string(),
            0,
        );
        let calls = Rc::new(RefCell::new(Vec::new()));

        let result = MessageMarkReadRuntime::new(
            &client,
            ReadySessionProvider,
            RecordingTransport {
                calls: Rc::clone(&calls),
                response: json!({"updated_count": 1}),
            },
        )
        .mark_read_async(MarkReadInput {
            message_ids: vec![crate::ids::MessageId::parse("direct-async-1").unwrap()],
        })
        .await
        .unwrap();

        assert_eq!(result.sdk_result.updated_count, 1);
        assert_eq!(result.direct_ids, vec!["direct-async-1"]);
        assert_eq!(fixture.is_read(&client, "direct-async-1"), 1);
        let calls = calls.borrow();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].endpoint, super::super::read::MESSAGE_RPC_ENDPOINT);
        assert_eq!(calls[0].method, "inbox.mark_read");
        assert_eq!(
            calls[0].params["body"]["message_ids"],
            json!(["p5-v2-delivery:direct-async-1"])
        );
    }

    #[tokio::test]
    async fn mark_thread_read_runtime_async_uses_local_unread_ids_without_history_rpc() {
        let fixture = Fixture::new();
        let client = fixture.client();
        fixture.seed_message(&client, "direct-thread-async-1", "", "text/plain", "", 0);
        let calls = Rc::new(RefCell::new(Vec::new()));

        let result = MessageMarkReadRuntime::new(
            &client,
            ReadySessionProvider,
            RecordingTransport {
                calls: Rc::clone(&calls),
                response: json!({"updated_count": 1}),
            },
        )
        .mark_thread_read_async(MarkThreadReadInput {
            request: crate::messages::MarkThreadReadRequest {
                thread: crate::messages::ThreadRef::Direct(
                    crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
                ),
                watermark: None,
                fallback_max_message_ids: None,
            },
            remote_thread: None,
        })
        .await
        .unwrap();

        assert_eq!(result.sdk_result.updated_count, 1);
        assert!(result.sdk_result.remote_acknowledged);
        assert!(!result.sdk_result.fallback_used);
        assert_eq!(fixture.is_read(&client, "direct-thread-async-1"), 1);
        let calls = calls.borrow();
        assert_eq!(
            calls
                .iter()
                .map(|call| call.method.as_str())
                .collect::<Vec<_>>(),
            vec!["read_state.mark_read"]
        );
    }

    #[derive(Clone)]
    struct ReadySessionProvider;

    impl SessionProvider for ReadySessionProvider {
        fn ensure_session(
            &self,
            scope: crate::auth::AuthScope,
        ) -> crate::ImResult<crate::auth::SessionBundle> {
            assert_eq!(scope, crate::auth::AuthScope::Messaging);
            Ok(crate::auth::SessionBundle {
                subject: crate::ids::Did::parse("did:example:alice")?,
                scope,
                expires_at: None,
                refreshed: false,
                bearer_token: None,
            })
        }

        fn refresh_session(&self) -> crate::ImResult<crate::auth::SessionUpdate> {
            unreachable!("mark_read runtime refresh is transport-owned in migration")
        }

        fn status(&self) -> crate::ImResult<crate::auth::AuthStatus> {
            unreachable!("mark_read runtime should not read status")
        }
    }

    impl crate::internal::auth::session::AsyncSessionProvider for ReadySessionProvider {
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

    struct RecordingTransport {
        calls: Rc<RefCell<Vec<RecordedCall>>>,
        response: Value,
    }

    impl AuthenticatedRpcTransport for RecordingTransport {
        fn authenticated_rpc(
            &mut self,
            endpoint: &str,
            method: &str,
            params: Value,
        ) -> crate::ImResult<Value> {
            let response = if method == "read_state.mark_read"
                && self
                    .response
                    .as_object()
                    .is_some_and(|value| value.len() == 1 && value.contains_key("updated_count"))
            {
                json!({
                    "user_did": params.pointer("/body/user_did").cloned().unwrap_or(Value::Null),
                    "thread": params.pointer("/body/thread").cloned().unwrap_or(Value::Null),
                    "updated_count": self.response["updated_count"],
                    "remote_acknowledged": true,
                    "partial": false,
                    "fallback_used": false,
                    "pending_remote_ack": false,
                    "read_watermark_server_seq": params
                        .pointer("/body/read_up_to_server_seq")
                        .cloned()
                        .unwrap_or(Value::Null),
                    "previous_read_watermark_server_seq": Value::Null,
                    "read_watermark_message_id": params
                        .pointer("/body/read_up_to_message_id")
                        .cloned()
                        .unwrap_or(Value::Null),
                    "advanced": true,
                    "read_at": "2026-07-28T12:00:00Z",
                    "unread_count": Value::Null,
                    "warnings": []
                })
            } else {
                self.response.clone()
            };
            self.calls.borrow_mut().push(RecordedCall {
                endpoint: endpoint.to_string(),
                method: method.to_string(),
                params,
            });
            Ok(response)
        }
    }

    impl crate::internal::transport::AsyncAuthenticatedRpcTransport for RecordingTransport {
        async fn authenticated_rpc(
            &mut self,
            endpoint: &str,
            method: &str,
            params: Value,
        ) -> crate::ImResult<Value> {
            AuthenticatedRpcTransport::authenticated_rpc(self, endpoint, method, params)
        }
    }

    struct UnsupportedReadStateThenLegacyTransport {
        calls: Rc<RefCell<Vec<RecordedCall>>>,
    }

    impl AuthenticatedRpcTransport for UnsupportedReadStateThenLegacyTransport {
        fn authenticated_rpc(
            &mut self,
            endpoint: &str,
            method: &str,
            params: Value,
        ) -> crate::ImResult<Value> {
            self.calls.borrow_mut().push(RecordedCall {
                endpoint: endpoint.to_string(),
                method: method.to_string(),
                params,
            });
            if method == "read_state.mark_read" {
                return Err(crate::ImError::Service {
                    status_code: None,
                    code: Some("-32601".to_owned()),
                    message: "method not found".to_owned(),
                    data: None,
                });
            }
            Ok(json!({"updated_count": 1}))
        }
    }

    impl crate::internal::transport::AsyncAuthenticatedRpcTransport
        for UnsupportedReadStateThenLegacyTransport
    {
        async fn authenticated_rpc(
            &mut self,
            endpoint: &str,
            method: &str,
            params: Value,
        ) -> crate::ImResult<Value> {
            AuthenticatedRpcTransport::authenticated_rpc(self, endpoint, method, params)
        }
    }

    struct FailingTransport {
        calls: Rc<RefCell<Vec<RecordedCall>>>,
    }

    impl AuthenticatedRpcTransport for FailingTransport {
        fn authenticated_rpc(
            &mut self,
            endpoint: &str,
            method: &str,
            params: Value,
        ) -> crate::ImResult<Value> {
            self.calls.borrow_mut().push(RecordedCall {
                endpoint: endpoint.to_string(),
                method: method.to_string(),
                params,
            });
            Err(crate::ImError::TransportUnavailable {
                detail: "offline".to_string(),
            })
        }
    }

    impl crate::internal::transport::AsyncAuthenticatedRpcTransport for FailingTransport {
        async fn authenticated_rpc(
            &mut self,
            endpoint: &str,
            method: &str,
            params: Value,
        ) -> crate::ImResult<Value> {
            AuthenticatedRpcTransport::authenticated_rpc(self, endpoint, method, params)
        }
    }

    struct RecordedCall {
        endpoint: String,
        method: String,
        params: Value,
    }

    struct Fixture {
        root: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let root = unique_temp_root();
            let identities = root.join("identities");
            fs::create_dir_all(&identities).unwrap();
            fs::write(identities.join("default"), "alice\n").unwrap();
            fs::write(
                identities.join("registry.json"),
                r#"{
                  "default_identity": "alice",
                  "identities": [{
                    "id": "alice-id",
                    "did": "did:example:alice",
                    "local_alias": "alice",
                    "ready_for_auth": true,
                    "ready_for_messaging": true,
                    "missing": []
                  }]
                }"#,
            )
            .unwrap();
            fs::create_dir_all(identities.join("alice")).unwrap();
            Self { root }
        }

        fn client(&self) -> crate::core::ImClient {
            crate::core::ImCore::new(
                crate::ImCoreConfig {
                    service_base_url: crate::ServiceEndpoint::parse("https://example.test")
                        .unwrap(),
                    did_domain: "awiki.test".to_string(),
                    user_service_endpoint: None,
                    message_service_endpoint: None,
                    mail_service_endpoint: None,
                    anp_service_endpoint: None,
                    anp_service_did: None,
                    ca_bundle: None,
                    transport_policy: crate::MessageTransportPolicy::HttpOnly,
                },
                crate::ImCorePaths {
                    identities: crate::paths::IdentityRegistryPaths {
                        identity_root_dir: self.root.join("identities"),
                        registry_path: self.root.join("identities").join("registry.json"),
                        default_identity_path: Some(self.root.join("identities").join("default")),
                    },
                    local_state: crate::paths::LocalStatePaths {
                        sqlite_path: self.root.join("local").join("im.sqlite"),
                    },
                    runtime: crate::paths::RuntimePaths {
                        cache_dir: self.root.join("cache"),
                        temp_dir: self.root.join("tmp"),
                    },
                },
            )
            .unwrap()
            .client(crate::identity::IdentitySelector::LocalAlias(
                "alice".to_string(),
            ))
            .unwrap()
        }

        fn seed_message(
            &self,
            client: &crate::core::ImClient,
            message_id: &str,
            group_did: &str,
            content_type: &str,
            metadata: &str,
            is_read: i64,
        ) {
            self.seed_message_with_seq(
                client,
                message_id,
                group_did,
                content_type,
                metadata,
                is_read,
                1,
            );
        }

        fn seed_message_with_seq(
            &self,
            client: &crate::core::ImClient,
            message_id: &str,
            group_did: &str,
            content_type: &str,
            metadata: &str,
            is_read: i64,
            server_seq: i64,
        ) {
            let connection = crate::internal::local_state::open_writable(
                &client.core_inner().sdk_paths().local_state.sqlite_path,
            )
            .unwrap();
            let conversation_id = if group_did.trim().is_empty() {
                crate::internal::local_state::owner_scope::direct_conversation_id("did:example:bob")
            } else {
                crate::internal::local_state::owner_scope::group_conversation_id(group_did)
            };
            connection
                .execute(
                    r#"
INSERT INTO messages
    (msg_id, owner_identity_id, owner_did, conversation_id, thread_id, direction, sender_did, receiver_did, group_id, group_did,
     content_type, content, server_seq, stored_at, metadata, is_read)
VALUES (?1, ?2, ?3, ?4, ?4, 0, 'did:example:bob', ?3, ?5, ?5, ?6, 'hello', ?7, ?8, ?9, ?10)"#,
                    (
                        message_id,
                        client.current_identity().id.as_str(),
                        client.did().as_str(),
                        conversation_id,
                        group_did,
                        content_type,
                        server_seq,
                        format!("2026-05-21T00:00:{server_seq:02}Z"),
                        metadata,
                        is_read,
                    ),
                )
                .unwrap();
        }

        fn seed_projected_message(
            &self,
            client: &crate::core::ImClient,
            message_id: &str,
            group_did: &str,
            direction: i64,
        ) {
            let connection = crate::internal::local_state::open_writable(
                &client.core_inner().sdk_paths().local_state.sqlite_path,
            )
            .unwrap();
            let conversation_id = if group_did.trim().is_empty() {
                crate::internal::local_state::owner_scope::direct_conversation_id("did:example:bob")
            } else {
                crate::internal::local_state::owner_scope::group_conversation_id(group_did)
            };
            crate::internal::local_state::messages::upsert_message(
                &connection,
                &crate::internal::local_state::messages::MessageRecord {
                    msg_id: message_id.to_owned(),
                    owner_identity_id: client.current_identity().id.as_str().to_owned(),
                    owner_did: client.did().as_str().to_owned(),
                    conversation_id: conversation_id.clone(),
                    thread_id: conversation_id,
                    direction,
                    sender_did: "did:example:bob".to_owned(),
                    receiver_did: client.did().as_str().to_owned(),
                    group_id: String::new(),
                    group_did: group_did.to_owned(),
                    content_type: "text/plain".to_owned(),
                    content: "hello".to_owned(),
                    server_seq: Some(1),
                    sent_at: "2026-05-21T00:00:00Z".to_owned(),
                    stored_at: "2026-05-21T00:00:00Z".to_owned(),
                    is_read: false,
                    ..Default::default()
                },
            )
            .unwrap();
        }

        fn is_read(&self, client: &crate::core::ImClient, message_id: &str) -> i64 {
            let connection = rusqlite::Connection::open(
                &client.core_inner().sdk_paths().local_state.sqlite_path,
            )
            .unwrap();
            connection
                .query_row(
                    "SELECT is_read FROM messages WHERE owner_identity_id = ?1 AND msg_id = ?2",
                    (client.current_identity().id.as_str(), message_id),
                    |row| row.get(0),
                )
                .unwrap()
        }
    }

    fn unique_temp_root() -> PathBuf {
        static TEMP_ROOT_COUNTER: std::sync::atomic::AtomicU64 =
            std::sync::atomic::AtomicU64::new(0);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let sequence = TEMP_ROOT_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "im-core-mark-read-runtime-{}-{nanos}-{sequence}",
            std::process::id()
        ))
    }
}
