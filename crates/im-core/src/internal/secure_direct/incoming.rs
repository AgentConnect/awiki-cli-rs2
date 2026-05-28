use serde_json::{Map, Value};
use std::future::Future;

use crate::internal::transport::{
    AsyncAuthenticatedRpcTransport, AsyncRpcTransport, AuthenticatedRpcTransport, RpcTransport,
};

use super::client::{
    prepare_direct_secure_client, DirectSecureClientInput, MessageServiceDirectSecureClient,
};

#[cfg(test)]
const DIRECT_E2EE_PROFILE: &str = "anp.direct.e2ee.v1";
#[cfg(test)]
const DIRECT_E2EE_SECURITY_PROFILE: &str = "direct-e2ee";
const DIRECT_INIT_CONTENT_TYPE: &str = "application/anp-direct-init+json";
#[cfg(test)]
const DIRECT_CIPHER_CONTENT_TYPE: &str = "application/anp-direct-cipher+json";
const ATTACHMENT_MANIFEST_CONTENT_TYPE: &str = "application/anp-attachment-manifest+json";
const REDACTED_SECURE_DIRECT_CONTENT_TYPE: &str = "application/x-awiki-secure-direct-redacted";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DirectDecryptMode {
    ReadOnly,
    #[allow(dead_code)]
    WithSideEffects,
}

pub(crate) type DirectIncomingProcessorResult = crate::ImResult<Map<String, Value>>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DirectRealtimeNotificationDecision {
    KeepOriginal,
    Normalized,
    DroppedControl,
    Redacted,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DirectRealtimeNotificationProjection {
    pub(crate) notification: Option<Value>,
    pub(crate) additional_notifications: Vec<Value>,
    pub(crate) warnings: Vec<String>,
    pub(crate) decision: DirectRealtimeNotificationDecision,
}

impl DirectRealtimeNotificationProjection {
    fn with_additional_notifications(mut self, additional_notifications: Vec<Value>) -> Self {
        self.additional_notifications = additional_notifications;
        self
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum DirectRealtimeAsyncProjectionOutcome {
    Projected(DirectRealtimeNotificationProjection),
    Fallback(Value),
}

pub(crate) fn maybe_decrypt_direct_e2ee_messages_for_client<R>(
    client: &crate::core::ImClient,
    messages: &mut [Value],
    directory_transport: &mut R,
    mode: DirectDecryptMode,
) -> Vec<String>
where
    R: RpcTransport,
{
    if messages.is_empty() || !contains_direct_e2ee_messages(messages) {
        return Vec::new();
    }
    let runtime = client.runtime();
    let local_did_document = match read_json_file(&runtime.did_document_path, "did_document") {
        Ok(value) => value,
        Err(err) => return decryptor_init_error(messages, err),
    };
    let signing_private_pem = match read_text_file(&runtime.private_key_path, "private_key") {
        Ok(value) => value,
        Err(err) => return decryptor_init_error(messages, err),
    };
    let agreement_private_pem = match read_text_file(
        &runtime.e2ee_agreement_private_key_path,
        "e2ee_agreement_private_key",
    ) {
        Ok(value) => value,
        Err(err) => return decryptor_init_error(messages, err),
    };
    let connection = match crate::internal::local_state::open_writable(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    ) {
        Ok(connection) => connection,
        Err(err) => return decryptor_init_error(messages, err),
    };
    let owner_did = client.did().as_str().to_owned();
    let local_document_for_resolver = local_did_document.clone();
    let identity_paths = client.core_inner().sdk_paths().identities.clone();
    let resolver = Box::new(move |did: &str| {
        if did == owner_did {
            return Ok(local_document_for_resolver.clone());
        }
        match resolve_did_document_with_transport(directory_transport, did) {
            Ok(document) => Ok(document),
            Err(err) => match crate::internal::identity_document_cache::load_local_did_document(
                &identity_paths,
                did,
            ) {
                Ok(Some(document)) => Ok(document),
                Ok(None) | Err(_) => Err(err),
            },
        }
    });
    let rpc = Box::new(|method: &str, _params: Map<String, Value>| {
        Err(crate::ImError::TransportUnavailable {
            detail: format!("direct incoming read-only decryptor does not call {method}"),
        })
    });
    let prepared = match prepare_direct_secure_client(DirectSecureClientInput {
        owner_identity_id: client.current_identity().id.as_str().to_owned(),
        owner_did: client.did().as_str().to_owned(),
        identity_name: client
            .current_identity()
            .local_alias
            .clone()
            .unwrap_or_else(|| client.current_identity().id.as_str().to_owned()),
        signing_key_id: format!("{}#key-1", client.did().as_str()),
        agreement_key_id: format!("{}#key-3", client.did().as_str()),
        signing_private_pem,
        agreement_private_pem,
        local_did_document,
        local_state: &connection,
    }) {
        Ok(value) => value,
        Err(err) => return decryptor_init_error(messages, err),
    };
    let mut direct_client = MessageServiceDirectSecureClient::new(prepared, rpc, resolver);
    maybe_decrypt_direct_e2ee_messages_with_processor_and_side_effects(
        messages,
        |notification| direct_client.process_incoming(notification),
        |_message, _result| match mode {
            DirectDecryptMode::ReadOnly => Vec::new(),
            // The mode boundary is here so realtime can later opt into ACK/outbox
            // side effects without changing inbox/history read semantics.
            DirectDecryptMode::WithSideEffects => Vec::new(),
        },
    )
}

pub(crate) fn maybe_normalize_direct_e2ee_notification_for_client<R>(
    client: &crate::core::ImClient,
    notification: Value,
    directory_transport: &mut R,
    mode: DirectDecryptMode,
) -> DirectRealtimeNotificationProjection
where
    R: RpcTransport,
{
    if !is_direct_e2ee_incoming_notification(&notification) {
        return keep_realtime_notification(notification);
    }
    let runtime = client.runtime();
    let local_did_document = match read_json_file(&runtime.did_document_path, "did_document") {
        Ok(value) => value,
        Err(err) => return realtime_decryptor_init_error(notification, err),
    };
    let signing_private_pem = match read_text_file(&runtime.private_key_path, "private_key") {
        Ok(value) => value,
        Err(err) => return realtime_decryptor_init_error(notification, err),
    };
    let agreement_private_pem = match read_text_file(
        &runtime.e2ee_agreement_private_key_path,
        "e2ee_agreement_private_key",
    ) {
        Ok(value) => value,
        Err(err) => return realtime_decryptor_init_error(notification, err),
    };
    let connection = match crate::internal::local_state::open_writable(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    ) {
        Ok(connection) => connection,
        Err(err) => return realtime_decryptor_init_error(notification, err),
    };
    let owner_did = client.did().as_str().to_owned();
    let local_document_for_resolver = local_did_document.clone();
    let identity_paths = client.core_inner().sdk_paths().identities.clone();
    let resolver = Box::new(move |did: &str| {
        if did == owner_did {
            return Ok(local_document_for_resolver.clone());
        }
        match resolve_did_document_with_transport(directory_transport, did) {
            Ok(document) => Ok(document),
            Err(err) => match crate::internal::identity_document_cache::load_local_did_document(
                &identity_paths,
                did,
            ) {
                Ok(Some(document)) => Ok(document),
                Ok(None) | Err(_) => Err(err),
            },
        }
    });
    let mut message_transport = crate::internal::transport::CoreHttpTransport::new(client);
    let rpc = Box::new(move |method: &str, params: Map<String, Value>| {
        let value = AuthenticatedRpcTransport::authenticated_rpc(
            &mut message_transport,
            super::send::MESSAGE_RPC_ENDPOINT,
            method,
            Value::Object(params),
        )?;
        object_result(value)
    });
    let prepared = match prepare_direct_secure_client(DirectSecureClientInput {
        owner_identity_id: client.current_identity().id.as_str().to_owned(),
        owner_did: client.did().as_str().to_owned(),
        identity_name: client
            .current_identity()
            .local_alias
            .clone()
            .unwrap_or_else(|| client.current_identity().id.as_str().to_owned()),
        signing_key_id: format!("{}#key-1", client.did().as_str()),
        agreement_key_id: format!("{}#key-3", client.did().as_str()),
        signing_private_pem,
        agreement_private_pem,
        local_did_document,
        local_state: &connection,
    }) {
        Ok(value) => value,
        Err(err) => return realtime_decryptor_init_error(notification, err),
    };
    let mut direct_client = MessageServiceDirectSecureClient::new(prepared, rpc, resolver);
    let (params, sender_did) = match direct_realtime_params_and_sender(&notification) {
        Ok(value) => value,
        Err(warning) => {
            return redacted_realtime_notification(notification, "failed", vec![warning])
        }
    };
    let result = match direct_client.process_incoming(params) {
        Ok(result) => result,
        Err(err) => {
            return redacted_realtime_notification(
                notification,
                "failed",
                vec![format!(
                    "Failed to decrypt secure direct realtime notification: {err}"
                )],
            );
        }
    };
    let mut plaintext =
        match plaintext_from_realtime_result(notification.clone(), result, &sender_did) {
            Ok(plaintext) => plaintext,
            Err(projection) => return projection,
        };
    let mut control_warnings = Vec::new();
    if mode == DirectDecryptMode::WithSideEffects {
        if super::control::is_secure_ack_plaintext(&plaintext) {
            control_warnings =
                flush_secure_outbox_after_ack(client, &connection, &mut direct_client, &sender_did);
        } else if super::control::is_secure_init_plaintext(&plaintext) {
            control_warnings = send_ack_after_secure_init(
                &mut direct_client,
                &notification,
                &sender_did,
                &plaintext,
            );
            if control_warnings.is_empty() {
                control_warnings = flush_secure_outbox_after_ack(
                    client,
                    &connection,
                    &mut direct_client,
                    &sender_did,
                );
            }
        }
    }
    normalize_direct_realtime_plaintext_notification(
        notification,
        mode,
        &mut plaintext,
        control_warnings,
    )
}

pub(crate) fn maybe_normalize_direct_e2ee_notification_with_processor(
    notification: Value,
    mode: DirectDecryptMode,
    mut process_incoming: impl FnMut(Map<String, Value>) -> DirectIncomingProcessorResult,
) -> DirectRealtimeNotificationProjection {
    if !is_direct_e2ee_incoming_notification(&notification) {
        return keep_realtime_notification(notification);
    }
    let (params, sender_did) = match direct_realtime_params_and_sender(&notification) {
        Ok(value) => value,
        Err(warning) => {
            return redacted_realtime_notification(notification, "failed", vec![warning])
        }
    };
    let result = match process_incoming(params) {
        Ok(result) => result,
        Err(err) => {
            return redacted_realtime_notification(
                notification,
                "failed",
                vec![format!(
                    "Failed to decrypt secure direct realtime notification: {err}"
                )],
            );
        }
    };
    let mut plaintext =
        match plaintext_from_realtime_result(notification.clone(), result, &sender_did) {
            Ok(plaintext) => plaintext,
            Err(projection) => return projection,
        };
    normalize_direct_realtime_plaintext_notification(notification, mode, &mut plaintext, Vec::new())
}

pub(crate) async fn try_normalize_direct_e2ee_notification_for_client_async<S, Fut>(
    client: &crate::core::ImClient,
    notification: Value,
    mode: DirectDecryptMode,
    mut side_effects: S,
) -> DirectRealtimeAsyncProjectionOutcome
where
    S: FnMut(DirectRealtimeControlSideEffect) -> Fut,
    Fut: Future<Output = Vec<String>>,
{
    let processor = super::async_receive::AsyncDirectSecureIncomingProcessor::new(client);
    normalize_direct_e2ee_notification_with_async_processor(
        client,
        &processor,
        notification,
        mode,
        |client, processor, params| async move {
            process_direct_realtime_incoming_async(client, processor, params).await
        },
        &mut side_effects,
    )
    .await
}

pub(crate) async fn normalize_direct_e2ee_notification_with_async_processor<'a, S, Fut, F, PFut>(
    client: &'a crate::core::ImClient,
    processor: &'a super::async_receive::AsyncDirectSecureIncomingProcessor<'a>,
    notification: Value,
    mode: DirectDecryptMode,
    mut process_incoming: F,
    mut side_effects: S,
) -> DirectRealtimeAsyncProjectionOutcome
where
    S: FnMut(DirectRealtimeControlSideEffect) -> Fut,
    Fut: Future<Output = Vec<String>>,
    F: FnMut(
        &'a crate::core::ImClient,
        &'a super::async_receive::AsyncDirectSecureIncomingProcessor<'a>,
        Map<String, Value>,
    ) -> PFut,
    PFut: Future<Output = crate::ImResult<super::async_receive::AsyncDirectSecureReceiveOutcome>>,
{
    if !is_direct_e2ee_incoming_notification(&notification) {
        return DirectRealtimeAsyncProjectionOutcome::Projected(keep_realtime_notification(
            notification,
        ));
    }
    let fallback_notification = notification.clone();
    let (params, sender_did) =
        match direct_realtime_params_and_sender(&notification) {
            Ok(value) => value,
            Err(warning) => {
                return DirectRealtimeAsyncProjectionOutcome::Projected(
                    redacted_realtime_notification(notification, "failed", vec![warning]),
                )
            }
        };
    let result = match process_incoming(client, processor, params).await {
        Ok(super::async_receive::AsyncDirectSecureReceiveOutcome::Processed(result)) => {
            AsyncDirectRealtimeProcessResult {
                result,
                replayed: Vec::new(),
            }
        }
        Ok(super::async_receive::AsyncDirectSecureReceiveOutcome::ProcessedWithReplay {
            result,
            replayed,
        }) => AsyncDirectRealtimeProcessResult { result, replayed },
        Ok(super::async_receive::AsyncDirectSecureReceiveOutcome::Fallback(_)) => {
            return DirectRealtimeAsyncProjectionOutcome::Fallback(fallback_notification)
        }
        Err(err) => {
            return DirectRealtimeAsyncProjectionOutcome::Projected(redacted_realtime_notification(
                notification,
                "failed",
                vec![format!(
                    "Failed to decrypt secure direct realtime notification: {err}"
                )],
            ))
        }
    };
    let mut plaintext =
        match plaintext_from_realtime_result(notification.clone(), result.result, &sender_did) {
            Ok(plaintext) => plaintext,
            Err(projection) => return DirectRealtimeAsyncProjectionOutcome::Projected(projection),
        };
    let control_warnings =
        match realtime_control_side_effect(&notification, &sender_did, &plaintext) {
            Some(side_effect) if mode == DirectDecryptMode::WithSideEffects => {
                side_effects(side_effect).await
            }
            _ => Vec::new(),
        };
    let mut projection = normalize_direct_realtime_plaintext_notification(
        notification,
        mode,
        &mut plaintext,
        control_warnings,
    );
    let replayed = result
        .replayed
        .into_iter()
        .filter_map(|replay| {
            normalize_replayed_direct_realtime_pending(replay.notification, replay.result, mode)
        })
        .collect::<Vec<_>>();
    if !replayed.is_empty() {
        projection = projection.with_additional_notifications(replayed);
    }
    DirectRealtimeAsyncProjectionOutcome::Projected(projection)
}

pub(crate) async fn try_normalize_direct_e2ee_notification_for_client_with_transports_async<M, D>(
    client: &crate::core::ImClient,
    notification: Value,
    mode: DirectDecryptMode,
    message_transport: &mut M,
    directory_transport: &mut D,
) -> DirectRealtimeAsyncProjectionOutcome
where
    M: AsyncAuthenticatedRpcTransport,
    D: AsyncRpcTransport,
{
    let processor = super::async_receive::AsyncDirectSecureIncomingProcessor::new(client);
    normalize_direct_e2ee_notification_with_async_processor_and_directory(
        client,
        &processor,
        notification,
        mode,
        message_transport,
        directory_transport,
    )
    .await
}

pub(crate) async fn normalize_direct_e2ee_notification_with_async_processor_and_directory<
    'a,
    M,
    D,
>(
    client: &'a crate::core::ImClient,
    processor: &'a super::async_receive::AsyncDirectSecureIncomingProcessor<'a>,
    notification: Value,
    mode: DirectDecryptMode,
    message_transport: &mut M,
    directory_transport: &mut D,
) -> DirectRealtimeAsyncProjectionOutcome
where
    M: AsyncAuthenticatedRpcTransport,
    D: AsyncRpcTransport,
{
    if !is_direct_e2ee_incoming_notification(&notification) {
        return DirectRealtimeAsyncProjectionOutcome::Projected(keep_realtime_notification(
            notification,
        ));
    }
    let fallback_notification = notification.clone();
    let (params, sender_did) =
        match direct_realtime_params_and_sender(&notification) {
            Ok(value) => value,
            Err(warning) => {
                return DirectRealtimeAsyncProjectionOutcome::Projected(
                    redacted_realtime_notification(notification, "failed", vec![warning]),
                )
            }
        };
    let result = match process_direct_realtime_incoming_with_directory_async(
        client,
        processor,
        params,
        directory_transport,
    )
    .await
    {
        Ok(super::async_receive::AsyncDirectSecureReceiveOutcome::Processed(result)) => {
            AsyncDirectRealtimeProcessResult {
                result,
                replayed: Vec::new(),
            }
        }
        Ok(super::async_receive::AsyncDirectSecureReceiveOutcome::ProcessedWithReplay {
            result,
            replayed,
        }) => AsyncDirectRealtimeProcessResult { result, replayed },
        Ok(super::async_receive::AsyncDirectSecureReceiveOutcome::Fallback(_)) => {
            return DirectRealtimeAsyncProjectionOutcome::Fallback(fallback_notification)
        }
        Err(err) => {
            return DirectRealtimeAsyncProjectionOutcome::Projected(redacted_realtime_notification(
                notification,
                "failed",
                vec![format!(
                    "Failed to decrypt secure direct realtime notification: {err}"
                )],
            ))
        }
    };
    let mut plaintext =
        match plaintext_from_realtime_result(notification.clone(), result.result, &sender_did) {
            Ok(plaintext) => plaintext,
            Err(projection) => return DirectRealtimeAsyncProjectionOutcome::Projected(projection),
        };
    let control_warnings =
        match realtime_control_side_effect(&notification, &sender_did, &plaintext) {
            Some(side_effect) if mode == DirectDecryptMode::WithSideEffects => {
                async_direct_realtime_side_effect(
                    client,
                    message_transport,
                    directory_transport,
                    side_effect,
                )
                .await
            }
            _ => Vec::new(),
        };
    let mut projection = normalize_direct_realtime_plaintext_notification(
        notification,
        mode,
        &mut plaintext,
        control_warnings,
    );
    let replayed = result
        .replayed
        .into_iter()
        .filter_map(|replay| {
            normalize_replayed_direct_realtime_pending(replay.notification, replay.result, mode)
        })
        .collect::<Vec<_>>();
    if !replayed.is_empty() {
        projection = projection.with_additional_notifications(replayed);
    }
    DirectRealtimeAsyncProjectionOutcome::Projected(projection)
}

struct AsyncDirectRealtimeProcessResult {
    result: Map<String, Value>,
    replayed: Vec<super::async_receive::AsyncDirectSecurePendingReplay>,
}

async fn process_direct_realtime_incoming_async(
    client: &crate::core::ImClient,
    processor: &super::async_receive::AsyncDirectSecureIncomingProcessor<'_>,
    params: Map<String, Value>,
) -> crate::ImResult<super::async_receive::AsyncDirectSecureReceiveOutcome> {
    let content_type = params
        .get("meta")
        .and_then(Value::as_object)
        .and_then(|meta| meta.get("content_type"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    if content_type == DIRECT_INIT_CONTENT_TYPE {
        let sender_did = params
            .get("meta")
            .and_then(Value::as_object)
            .and_then(|meta| meta.get("sender_did"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        let sender_document = match crate::internal::identity_document_cache::load_local_did_document_async(
            &client.core_inner().sdk_paths().identities,
            sender_did,
        )
        .await?
        {
            Some(document) => document,
            None => {
                return Ok(super::async_receive::AsyncDirectSecureReceiveOutcome::Fallback(
                    super::async_receive::AsyncDirectSecureReceiveFallback::NoEstablishedSession,
                ))
            }
        };
        return processor
            .process_init_if_ready(params, sender_document)
            .await;
    }
    processor.process_cipher_if_ready(params).await
}

async fn process_direct_realtime_incoming_with_directory_async<D>(
    client: &crate::core::ImClient,
    processor: &super::async_receive::AsyncDirectSecureIncomingProcessor<'_>,
    params: Map<String, Value>,
    directory_transport: &mut D,
) -> crate::ImResult<super::async_receive::AsyncDirectSecureReceiveOutcome>
where
    D: AsyncRpcTransport,
{
    let content_type = params
        .get("meta")
        .and_then(Value::as_object)
        .and_then(|meta| meta.get("content_type"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    if content_type == DIRECT_INIT_CONTENT_TYPE {
        let sender_did = params
            .get("meta")
            .and_then(Value::as_object)
            .and_then(|meta| meta.get("sender_did"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        let sender_document =
            resolve_direct_sender_document_async(client, directory_transport, sender_did).await?;
        return processor
            .process_init_if_ready(params, sender_document)
            .await;
    }
    processor.process_cipher_if_ready(params).await
}

async fn async_direct_realtime_side_effect<M, D>(
    client: &crate::core::ImClient,
    message_transport: &mut M,
    _directory_transport: &mut D,
    side_effect: DirectRealtimeControlSideEffect,
) -> Vec<String>
where
    M: AsyncAuthenticatedRpcTransport,
    D: AsyncRpcTransport,
{
    match side_effect {
        DirectRealtimeControlSideEffect::FlushOutboxAfterAck { peer_did } => {
            async_flush_secure_outbox_after_ack(client, message_transport, &peer_did).await
        }
        DirectRealtimeControlSideEffect::SendAckAfterInit {
            peer_did,
            message_id,
            ack_id,
            payload,
        } => {
            let mut warnings = async_send_ack_after_secure_init(
                client,
                message_transport,
                &peer_did,
                &message_id,
                &ack_id,
                payload,
            )
            .await;
            if warnings.is_empty() {
                warnings.extend(
                    async_flush_secure_outbox_after_ack(client, message_transport, &peer_did).await,
                );
            }
            warnings
        }
    }
}

#[allow(dead_code)]
pub(crate) async fn maybe_normalize_direct_e2ee_notification_with_processor_async<S, Fut>(
    notification: Value,
    mode: DirectDecryptMode,
    mut process_incoming: impl FnMut(Map<String, Value>) -> DirectIncomingProcessorResult,
    mut side_effects: S,
) -> DirectRealtimeNotificationProjection
where
    S: FnMut(DirectRealtimeControlSideEffect) -> Fut,
    Fut: Future<Output = Vec<String>>,
{
    if !is_direct_e2ee_incoming_notification(&notification) {
        return keep_realtime_notification(notification);
    }
    let (params, sender_did) = match direct_realtime_params_and_sender(&notification) {
        Ok(value) => value,
        Err(warning) => {
            return redacted_realtime_notification(notification, "failed", vec![warning])
        }
    };
    let result = match process_incoming(params) {
        Ok(result) => result,
        Err(err) => {
            return redacted_realtime_notification(
                notification,
                "failed",
                vec![format!(
                    "Failed to decrypt secure direct realtime notification: {err}"
                )],
            );
        }
    };
    let mut plaintext =
        match plaintext_from_realtime_result(notification.clone(), result, &sender_did) {
            Ok(plaintext) => plaintext,
            Err(projection) => return projection,
        };
    let control_warnings =
        match realtime_control_side_effect(&notification, &sender_did, &plaintext) {
            Some(side_effect) if mode == DirectDecryptMode::WithSideEffects => {
                side_effects(side_effect).await
            }
            _ => Vec::new(),
        };
    normalize_direct_realtime_plaintext_notification(
        notification,
        mode,
        &mut plaintext,
        control_warnings,
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DirectRealtimeControlSideEffect {
    FlushOutboxAfterAck {
        peer_did: String,
    },
    SendAckAfterInit {
        peer_did: String,
        message_id: String,
        ack_id: String,
        payload: Map<String, Value>,
    },
}

fn realtime_control_side_effect(
    notification: &Value,
    sender_did: &str,
    plaintext: &Map<String, Value>,
) -> Option<DirectRealtimeControlSideEffect> {
    if super::control::is_secure_ack_plaintext(plaintext) {
        return Some(DirectRealtimeControlSideEffect::FlushOutboxAfterAck {
            peer_did: sender_did.trim().to_owned(),
        });
    }
    if super::control::is_secure_init_plaintext(plaintext) {
        let request = ack_request_from_secure_init(notification, sender_did, plaintext).ok()?;
        return Some(DirectRealtimeControlSideEffect::SendAckAfterInit {
            peer_did: request.peer_did,
            message_id: request.message_id,
            ack_id: request.ack_id,
            payload: request.payload,
        });
    }
    None
}

fn plaintext_from_realtime_result(
    notification: Value,
    result: Map<String, Value>,
    sender_did: &str,
) -> Result<Map<String, Value>, DirectRealtimeNotificationProjection> {
    let state = string_value(result.get("state"));
    if state != "decrypted" {
        return Err(redacted_realtime_notification(
            notification,
            &state,
            Vec::new(),
        ));
    }
    let Some(plaintext) = result.get("plaintext").and_then(Value::as_object) else {
        return Err(redacted_realtime_notification(
            notification,
            "failed",
            vec!["Secure direct realtime decrypt result did not contain plaintext".to_owned()],
        ));
    };
    let mut plaintext = plaintext.clone();
    if !sender_did.is_empty() {
        plaintext.insert(
            "_secure_sender_did".to_owned(),
            Value::String(sender_did.to_owned()),
        );
    }
    Ok(plaintext)
}

fn normalize_direct_realtime_plaintext_notification(
    notification: Value,
    mode: DirectDecryptMode,
    plaintext: &mut Map<String, Value>,
    control_warnings: Vec<String>,
) -> DirectRealtimeNotificationProjection {
    if super::control::is_secure_ack_plaintext(plaintext) {
        return DirectRealtimeNotificationProjection {
            notification: None,
            additional_notifications: Vec::new(),
            warnings: compact_warnings(control_warnings),
            decision: DirectRealtimeNotificationDecision::DroppedControl,
        };
    }
    if super::control::is_secure_init_plaintext(plaintext) {
        return DirectRealtimeNotificationProjection {
            notification: None,
            additional_notifications: Vec::new(),
            warnings: match mode {
                DirectDecryptMode::ReadOnly => Vec::new(),
                DirectDecryptMode::WithSideEffects => compact_warnings(control_warnings),
            },
            decision: DirectRealtimeNotificationDecision::DroppedControl,
        };
    }
    let mut normalized = notification;
    apply_plaintext_to_realtime_notification(&mut normalized, plaintext);
    DirectRealtimeNotificationProjection {
        notification: Some(normalized),
        additional_notifications: Vec::new(),
        warnings: Vec::new(),
        decision: DirectRealtimeNotificationDecision::Normalized,
    }
}

fn normalize_replayed_direct_realtime_pending(
    params: Map<String, Value>,
    result: Map<String, Value>,
    mode: DirectDecryptMode,
) -> Option<Value> {
    let notification = Value::Object(Map::from_iter([
        (
            "method".to_owned(),
            Value::String("direct.incoming".to_owned()),
        ),
        ("params".to_owned(), Value::Object(params)),
    ]));
    let sender_did = notification
        .pointer("/params/meta/sender_did")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let mut plaintext =
        plaintext_from_realtime_result(notification.clone(), result, &sender_did).ok()?;
    normalize_direct_realtime_plaintext_notification(notification, mode, &mut plaintext, Vec::new())
        .notification
}

fn direct_realtime_params_and_sender(
    notification: &Value,
) -> Result<(Map<String, Value>, String), String> {
    let params = notification
        .get("params")
        .and_then(Value::as_object)
        .cloned()
        .ok_or_else(|| "Skipped secure direct realtime notification: missing params".to_owned())?;
    let sender_did = notification
        .pointer("/params/meta/sender_did")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    Ok((params, sender_did))
}

fn flush_secure_outbox_after_ack(
    client: &crate::core::ImClient,
    connection: &rusqlite::Connection,
    direct_client: &mut MessageServiceDirectSecureClient<'_>,
    peer_did: &str,
) -> Vec<String> {
    if peer_did.trim().is_empty() {
        return vec![
            "Secure direct ACK did not include sender; queued outbox was not flushed".to_owned(),
        ];
    }
    super::outbox::flush_queued_secure_outbox_with_sender(
        connection,
        &crate::internal::store::e2ee_outbox::E2eeOutboxOwnerScope::for_client(client),
        peer_did,
        |request| {
            let send = match send_secure_outbox_request(direct_client, &request) {
                Ok(outcome) => outcome,
                Err(err) => super::outbox::SecureOutboxSendOutcome::Error(err.to_string()),
            };
            let session_id = match &send {
                super::outbox::SecureOutboxSendOutcome::Success { .. } => {
                    direct_client.current_session_id(&request.target_did)
                }
                super::outbox::SecureOutboxSendOutcome::Error(_) => String::new(),
            };
            super::outbox::SecureOutboxSendResult { send, session_id }
        },
    )
}

fn send_ack_after_secure_init(
    direct_client: &mut MessageServiceDirectSecureClient<'_>,
    notification: &Value,
    sender_did: &str,
    plaintext: &Map<String, Value>,
) -> Vec<String> {
    let request = match ack_request_from_secure_init(notification, sender_did, plaintext) {
        Ok(request) => request,
        Err(warning) => return vec![warning],
    };
    match direct_client.send_json(
        &request.peer_did,
        request.payload,
        &request.ack_id,
        &request.ack_id,
    ) {
        Ok(_) => Vec::new(),
        Err(err) => vec![format!(
            "Failed to send secure direct ACK for {}: {err}",
            request.message_id
        )],
    }
}

#[derive(Debug)]
struct SecureInitAckRequest {
    peer_did: String,
    message_id: String,
    ack_id: String,
    payload: Map<String, Value>,
}

fn ack_request_from_secure_init(
    notification: &Value,
    sender_did: &str,
    plaintext: &Map<String, Value>,
) -> Result<SecureInitAckRequest, String> {
    let session_id = default_string(
        &string_value(
            plaintext
                .get("payload")
                .and_then(Value::as_object)
                .and_then(|payload| payload.get("session_id")),
        ),
        &direct_init_session_id_from_realtime_notification(notification),
    );
    let message_id = message_id_from_realtime_notification(notification);
    let sender_did = sender_did.trim();
    if sender_did.is_empty() {
        return Err("Secure direct init did not include sender; ACK was not sent".to_owned());
    }
    if session_id.trim().is_empty() {
        return Err(
            "Secure direct init did not include session state reference; ACK was not sent"
                .to_owned(),
        );
    }
    if message_id.trim().is_empty() {
        return Err("Secure direct init did not include message id; ACK was not sent".to_owned());
    }
    let ack_id = format!("ack-{session_id}");
    Ok(SecureInitAckRequest {
        peer_did: sender_did.to_owned(),
        message_id: message_id.clone(),
        ack_id,
        payload: super::control::build_secure_ack_payload(&session_id, &message_id),
    })
}

fn message_id_from_realtime_notification(notification: &Value) -> String {
    first_non_empty_string(&[
        notification.pointer("/params/meta/message_id"),
        notification.pointer("/params/body/message_id"),
        notification.get("id"),
    ])
}

fn direct_init_session_id_from_realtime_notification(notification: &Value) -> String {
    first_non_empty_string(&[
        notification.pointer("/params/body/session_id"),
        notification.pointer("/params/content/session_id"),
        notification.pointer("/params/meta/session_id"),
    ])
}

fn decryptor_init_error(messages: &mut [Value], err: crate::ImError) -> Vec<String> {
    redact_all_direct_e2ee_messages(messages, "failed");
    compact_warnings(vec![format!(
        "Failed to initialize secure direct decryptor: {err}"
    )])
}

fn realtime_decryptor_init_error(
    notification: Value,
    err: crate::ImError,
) -> DirectRealtimeNotificationProjection {
    redacted_realtime_notification(
        notification,
        "failed",
        vec![format!(
            "Failed to initialize secure direct realtime decryptor: {err}"
        )],
    )
}

fn send_secure_outbox_request(
    direct_client: &mut MessageServiceDirectSecureClient<'_>,
    request: &super::outbox::SecureOutboxSendRequest,
) -> crate::ImResult<super::outbox::SecureOutboxSendOutcome> {
    let operation_id = request.outbox_id.trim();
    let raw = if request.original_type.trim() == "json" {
        let payload =
            request
                .json_payload
                .clone()
                .ok_or_else(|| crate::ImError::Serialization {
                    detail: "queued secure JSON payload was not parsed".to_owned(),
                })?;
        direct_client.send_json(&request.target_did, payload, operation_id, operation_id)?
    } else {
        direct_client.send_text(
            &request.target_did,
            &request.plaintext,
            operation_id,
            operation_id,
        )?
    };
    Ok(super::outbox::SecureOutboxSendOutcome::Success {
        message_id: default_string(
            &string_value(raw.get("message_id")),
            &default_string(operation_id, "secure-outbox-message"),
        ),
        operation_id: default_string(&string_value(raw.get("operation_id")), operation_id),
        delivery_state: default_string(&string_value(raw.get("delivery_state")), "accepted"),
        accepted_at: string_value(raw.get("accepted_at").or_else(|| raw.get("finalized_at"))),
    })
}

async fn async_send_ack_after_secure_init<M>(
    client: &crate::core::ImClient,
    message_transport: &mut M,
    peer_did: &str,
    message_id: &str,
    ack_id: &str,
    payload: Map<String, Value>,
) -> Vec<String>
where
    M: AsyncAuthenticatedRpcTransport,
{
    let peer_did = peer_did.trim();
    if peer_did.is_empty() {
        return vec!["Secure direct init did not include sender; ACK was not sent".to_owned()];
    }
    let db = match client.core_inner().local_state_db().await {
        Ok(db) => db,
        Err(err) => {
            return compact_warnings(vec![format!(
                "Failed to open local state for secure direct ACK: {err}"
            )])
        }
    };
    match super::async_send::send_established_follow_up_payload_async(
        client,
        &db,
        message_transport,
        super::async_send::AsyncDirectSecureFollowUpSend {
            target_did: peer_did.to_owned(),
            operation_id: ack_id.trim().to_owned(),
            message_id: ack_id.trim().to_owned(),
            plaintext: anp::direct_e2ee::ApplicationPlaintext::new_json(
                "application/json",
                Value::Object(payload),
            ),
        },
    )
    .await
    {
        Ok(_) => Vec::new(),
        Err(err) => compact_warnings(vec![format!(
            "Failed to send secure direct ACK for {}: {err}",
            message_id.trim()
        )]),
    }
}

async fn async_flush_secure_outbox_after_ack<M>(
    client: &crate::core::ImClient,
    message_transport: &mut M,
    peer_did: &str,
) -> Vec<String>
where
    M: AsyncAuthenticatedRpcTransport,
{
    if peer_did.trim().is_empty() {
        return vec![
            "Secure direct ACK did not include sender; queued outbox was not flushed".to_owned(),
        ];
    }
    let db = match client.core_inner().local_state_db().await {
        Ok(db) => db,
        Err(err) => {
            return compact_warnings(vec![format!(
                "Failed to open local state for secure outbox flush: {err}"
            )])
        }
    };
    let scope = crate::internal::store::e2ee_outbox::E2eeOutboxOwnerScope::for_client(client);
    let rows = match db
        .list_e2ee_outbox(scope.clone(), Some("queued".to_owned()))
        .await
    {
        Ok(rows) => rows,
        Err(err) => {
            return compact_warnings(vec![format!("Failed to list secure outbox: {err}")]);
        }
    };
    let mut rows = rows
        .iter()
        .filter_map(super::outbox::queued_secure_outbox_row_from_record)
        .collect::<Vec<_>>();
    if rows.is_empty() {
        return Vec::new();
    }

    let mut warnings = Vec::new();
    rows.sort_by(|left, right| left.created_at.cmp(&right.created_at));
    let peer_filter = peer_did.trim().to_owned();
    for row in rows {
        if !peer_filter.is_empty() && row.peer_did != peer_filter {
            continue;
        }
        let preflight = super::outbox::flush_queued_secure_outbox_rows_plan(
            &scope.owner_identity_id,
            &scope.owner_did,
            &scope.credential_name,
            "",
            std::slice::from_ref(&row),
            |_| super::outbox::SecureOutboxFlushRowOutcome {
                send: super::outbox::SecureOutboxSendOutcome::Error("preflight".to_owned()),
                ..super::outbox::SecureOutboxFlushRowOutcome::default()
            },
        );
        let Some(request) = super::outbox::send_request_from_plan(&preflight) else {
            warnings.extend(
                super::outbox::execute_secure_outbox_flush_plan_async(&db, &scope, preflight).await,
            );
            continue;
        };
        let result =
            async_send_secure_outbox_request(client, &db, message_transport, request).await;
        let plan = super::outbox::flush_queued_secure_outbox_rows_plan(
            &scope.owner_identity_id,
            &scope.owner_did,
            &scope.credential_name,
            "",
            std::slice::from_ref(&row),
            |_| super::outbox::SecureOutboxFlushRowOutcome {
                send: result.send.clone(),
                session_id: result.session_id.clone(),
                mark_sent: super::outbox::MarkSentOutcome::Success,
                store_message: super::outbox::StoreMessageOutcome::Success,
            },
        );
        warnings
            .extend(super::outbox::execute_secure_outbox_flush_plan_async(&db, &scope, plan).await);
    }
    compact_warnings(warnings)
}

async fn async_send_secure_outbox_request<M>(
    client: &crate::core::ImClient,
    db: &crate::internal::local_state::actor::LocalStateDb,
    message_transport: &mut M,
    request: super::outbox::SecureOutboxSendRequest,
) -> super::outbox::SecureOutboxSendResult
where
    M: AsyncAuthenticatedRpcTransport,
{
    let operation_id = request.outbox_id.trim().to_owned();
    let plaintext = if request.original_type.trim() == "json" {
        match request.json_payload.clone() {
            Some(payload) => anp::direct_e2ee::ApplicationPlaintext::new_json(
                "application/json",
                Value::Object(payload),
            ),
            None => {
                return super::outbox::SecureOutboxSendResult {
                    send: super::outbox::SecureOutboxSendOutcome::Error(
                        "queued secure JSON payload was not parsed".to_owned(),
                    ),
                    session_id: String::new(),
                }
            }
        }
    } else {
        anp::direct_e2ee::ApplicationPlaintext::new_text("text/plain", &request.plaintext)
    };
    match super::async_send::send_established_follow_up_payload_async(
        client,
        db,
        message_transport,
        super::async_send::AsyncDirectSecureFollowUpSend {
            target_did: request.target_did,
            operation_id: operation_id.clone(),
            message_id: operation_id,
            plaintext,
        },
    )
    .await
    {
        Ok(result) => super::outbox::SecureOutboxSendResult {
            session_id: result.session_id,
            send: super::outbox::SecureOutboxSendOutcome::Success {
                message_id: result.message_id,
                operation_id: result.operation_id,
                delivery_state: result.delivery_state,
                accepted_at: result.accepted_at,
            },
        },
        Err(err) => super::outbox::SecureOutboxSendResult {
            send: super::outbox::SecureOutboxSendOutcome::Error(err.to_string()),
            session_id: String::new(),
        },
    }
}

#[allow(dead_code)]
pub(crate) fn maybe_decrypt_direct_e2ee_messages_with_processor(
    messages: &mut [Value],
    mut process_incoming: impl FnMut(Map<String, Value>) -> DirectIncomingProcessorResult,
) -> Vec<String> {
    maybe_decrypt_direct_e2ee_messages_with_processor_and_side_effects(
        messages,
        &mut process_incoming,
        |_message, _result| Vec::new(),
    )
}

#[cfg(test)]
pub(crate) fn project_direct_e2ee_message_values_with_processor(
    messages: Vec<Value>,
    process_incoming: impl FnMut(Map<String, Value>) -> DirectIncomingProcessorResult,
) -> (Vec<Value>, Vec<String>) {
    let mut messages = messages;
    let warnings =
        maybe_decrypt_direct_e2ee_messages_with_processor(&mut messages, process_incoming);
    (filter_displayable_direct_e2ee_messages(messages), warnings)
}

#[allow(dead_code)]
pub(crate) fn maybe_decrypt_direct_e2ee_messages_with_processor_and_side_effects(
    messages: &mut [Value],
    mut process_incoming: impl FnMut(Map<String, Value>) -> DirectIncomingProcessorResult,
    mut side_effects: impl FnMut(&Value, &Map<String, Value>) -> Vec<String>,
) -> Vec<String> {
    if messages.is_empty() || !contains_direct_e2ee_messages(messages) {
        return Vec::new();
    }
    let mut order = (0..messages.len()).collect::<Vec<_>>();
    order.sort_by(|left, right| compare_message_order(&messages[*left], &messages[*right]));
    let mut warnings = Vec::new();
    for index in order {
        let content_type = string_from_message(&messages[index], "content_type");
        if !anp::direct_e2ee::is_direct_e2ee_wire_content_type(&content_type) {
            continue;
        }
        let notification = match direct_e2ee_notification_from_message_view(&messages[index]) {
            Ok(notification) => notification,
            Err(err) => {
                mark_direct_e2ee_processing_state(&mut messages[index], "failed");
                if !is_direct_e2ee_wire_control_message(&messages[index]) {
                    warnings.push(format!(
                        "Skipped secure direct message {}: {err}",
                        string_from_message(&messages[index], "id")
                    ));
                }
                continue;
            }
        };
        let result = match process_incoming(notification) {
            Ok(result) => result,
            Err(err) => {
                mark_direct_e2ee_processing_state(&mut messages[index], "failed");
                if !is_direct_e2ee_wire_control_message(&messages[index]) {
                    warnings.push(format!(
                        "Failed to decrypt secure direct message {}: {err}",
                        string_from_message(&messages[index], "id")
                    ));
                }
                continue;
            }
        };
        warnings.extend(side_effects(&messages[index], &result));
        apply_direct_e2ee_processing_result(&mut messages[index], &Value::Object(result));
    }
    compact_warnings(warnings)
}

pub(crate) fn filter_displayable_direct_e2ee_messages(messages: Vec<Value>) -> Vec<Value> {
    messages
        .into_iter()
        .filter(|message| !is_direct_e2ee_control_message_projection(message))
        .collect()
}

pub(crate) fn direct_e2ee_notification_from_message_view(
    message: &Value,
) -> Result<Map<String, Value>, String> {
    let notification = anp::direct_e2ee::direct_notification_from_message_view(message)
        .map_err(|err| err.to_string())?;
    notification
        .as_object()
        .cloned()
        .ok_or_else(|| "direct-e2ee notification is not an object".to_owned())
}

pub(crate) fn apply_direct_e2ee_processing_result(message: &mut Value, result: &Value) {
    let Some(message) = message.as_object_mut() else {
        return;
    };
    message.insert("secure".to_owned(), Value::Bool(true));
    let state = string_value(result.get("state"));
    if state.is_empty() {
        return;
    }
    message.insert("decryption_state".to_owned(), Value::String(state.clone()));
    if state != "decrypted" {
        redact_direct_e2ee_content(message);
        return;
    }
    let Some(plaintext) = result.get("plaintext").and_then(Value::as_object) else {
        return;
    };
    if super::control::is_secure_ack_plaintext(plaintext)
        || super::control::is_secure_init_plaintext(plaintext)
    {
        message.insert("secure_control".to_owned(), Value::Bool(true));
        message.insert(
            "type".to_owned(),
            Value::String("secure_control".to_owned()),
        );
        message.insert("content".to_owned(), Value::String(String::new()));
        return;
    }
    let content_type = string_value(plaintext.get("application_content_type"));
    if !content_type.is_empty() {
        message.insert(
            "content_type".to_owned(),
            Value::String(content_type.clone()),
        );
    }
    if let Some(text) = plaintext
        .get("text")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
    {
        message.insert("content".to_owned(), Value::String(text.to_owned()));
        message.insert("type".to_owned(), Value::String("text".to_owned()));
    } else if let Some(payload) = plaintext.get("payload") {
        message.insert("content".to_owned(), payload.clone());
        let message_type = if content_type == ATTACHMENT_MANIFEST_CONTENT_TYPE {
            "attachment_manifest"
        } else {
            "json"
        };
        message.insert("type".to_owned(), Value::String(message_type.to_owned()));
    } else if let Some(payload_b64u) = plaintext
        .get("payload_b64u")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
    {
        message.insert("content".to_owned(), Value::String(payload_b64u.to_owned()));
        message.insert("type".to_owned(), Value::String("binary".to_owned()));
    }
}

#[allow(dead_code)]
pub(crate) fn direct_init_session_id_from_message(message: &Value) -> String {
    message
        .as_object()
        .and_then(|object| object_body_value(object.get("content")))
        .and_then(|value| {
            value
                .as_object()
                .map(|object| string_value(object.get("session_id")))
        })
        .unwrap_or_default()
}

fn resolve_did_document_with_transport(
    transport: &mut impl RpcTransport,
    did: &str,
) -> crate::ImResult<Value> {
    let call = crate::internal::identity_wire::profile::build_profile_resolve_rpc_call(did)?;
    let raw = transport.rpc(call.endpoint, call.method, call.params)?;
    did_document_from_resolve(raw).ok_or_else(|| crate::ImError::PeerNotFound {
        peer: did.to_owned(),
    })
}

async fn resolve_direct_sender_document_async<D>(
    client: &crate::core::ImClient,
    directory_transport: &mut D,
    did: &str,
) -> crate::ImResult<Value>
where
    D: AsyncRpcTransport,
{
    if did == client.did().as_str() {
        return read_json_file_async(client.runtime().did_document_path.clone(), "did_document")
            .await;
    }
    match resolve_did_document_with_transport_async(directory_transport, did).await {
        Ok(document) => Ok(document),
        Err(err) => match crate::internal::identity_document_cache::load_local_did_document_async(
            &client.core_inner().sdk_paths().identities,
            did,
        )
        .await
        {
            Ok(Some(document)) => Ok(document),
            Ok(None) | Err(_) => Err(err),
        },
    }
}

async fn resolve_did_document_with_transport_async<D>(
    transport: &mut D,
    did: &str,
) -> crate::ImResult<Value>
where
    D: AsyncRpcTransport,
{
    let call = crate::internal::identity_wire::profile::build_profile_resolve_rpc_call(did)?;
    let raw = transport
        .rpc(call.endpoint, call.method, call.params)
        .await?;
    did_document_from_resolve(raw).ok_or_else(|| crate::ImError::PeerNotFound {
        peer: did.to_owned(),
    })
}

fn did_document_from_resolve(value: Value) -> Option<Value> {
    if looks_like_did_document(&value) {
        return Some(value);
    }
    for pointer in [
        "/did_document",
        "/didDocument",
        "/document",
        "/profile/did_document",
        "/profile/didDocument",
        "/result/did_document",
        "/result/didDocument",
    ] {
        if let Some(candidate) = value.pointer(pointer) {
            if looks_like_did_document(candidate) {
                return Some(candidate.clone());
            }
        }
    }
    None
}

fn looks_like_did_document(value: &Value) -> bool {
    value
        .get("id")
        .and_then(Value::as_str)
        .is_some_and(|value| value.starts_with("did:"))
        && value.get("verificationMethod").is_some()
}

fn read_json_file(path: &std::path::Path, path_kind: &str) -> crate::ImResult<Value> {
    let raw = std::fs::read(path).map_err(|err| crate::ImError::CredentialFileUnreadable {
        path_kind: path_kind.to_owned(),
        detail: err.to_string(),
    })?;
    serde_json::from_slice(&raw).map_err(|err| crate::ImError::Serialization {
        detail: err.to_string(),
    })
}

async fn read_json_file_async(path: std::path::PathBuf, path_kind: &str) -> crate::ImResult<Value> {
    let raw =
        tokio::fs::read(&path)
            .await
            .map_err(|err| crate::ImError::CredentialFileUnreadable {
                path_kind: path_kind.to_owned(),
                detail: err.to_string(),
            })?;
    serde_json::from_slice(&raw).map_err(|err| crate::ImError::Serialization {
        detail: err.to_string(),
    })
}

fn read_text_file(path: &std::path::Path, path_kind: &str) -> crate::ImResult<String> {
    std::fs::read_to_string(path).map_err(|err| crate::ImError::CredentialFileUnreadable {
        path_kind: path_kind.to_owned(),
        detail: err.to_string(),
    })
}

fn object_result(value: Value) -> crate::ImResult<Map<String, Value>> {
    match value {
        Value::Object(object) => Ok(object),
        Value::Null => Ok(Map::new()),
        other => Err(crate::ImError::Serialization {
            detail: format!("expected object RPC result, got {other}"),
        }),
    }
}

fn contains_direct_e2ee_messages(messages: &[Value]) -> bool {
    messages.iter().any(|message| {
        anp::direct_e2ee::is_direct_e2ee_wire_content_type(&string_from_message(
            message,
            "content_type",
        ))
    })
}

fn redact_all_direct_e2ee_messages(messages: &mut [Value], state: &str) {
    for message in messages {
        if anp::direct_e2ee::is_direct_e2ee_wire_content_type(&string_from_message(
            message,
            "content_type",
        )) {
            mark_direct_e2ee_processing_state(message, state);
        }
    }
}

fn mark_direct_e2ee_processing_state(message: &mut Value, state: &str) {
    let Some(object) = message.as_object_mut() else {
        return;
    };
    object.insert("secure".to_owned(), Value::Bool(true));
    object.insert(
        "decryption_state".to_owned(),
        Value::String(state.to_owned()),
    );
    if is_direct_e2ee_wire_control_message(&Value::Object(object.clone())) {
        object.insert("secure_control".to_owned(), Value::Bool(true));
        object.insert(
            "type".to_owned(),
            Value::String("secure_control".to_owned()),
        );
    }
    redact_direct_e2ee_content(object);
}

fn redact_direct_e2ee_content(message: &mut Map<String, Value>) {
    message.insert("content".to_owned(), Value::Null);
    message.insert(
        "type".to_owned(),
        Value::String("secure_ciphertext".to_owned()),
    );
}

fn keep_realtime_notification(notification: Value) -> DirectRealtimeNotificationProjection {
    DirectRealtimeNotificationProjection {
        notification: Some(notification),
        additional_notifications: Vec::new(),
        warnings: Vec::new(),
        decision: DirectRealtimeNotificationDecision::KeepOriginal,
    }
}

fn redacted_realtime_notification(
    mut notification: Value,
    state: &str,
    warnings: Vec<String>,
) -> DirectRealtimeNotificationProjection {
    redact_realtime_notification(&mut notification, state);
    DirectRealtimeNotificationProjection {
        notification: Some(notification),
        additional_notifications: Vec::new(),
        warnings: compact_warnings(warnings),
        decision: DirectRealtimeNotificationDecision::Redacted,
    }
}

fn redact_realtime_notification(notification: &mut Value, state: &str) {
    let Some(params) = notification
        .get_mut("params")
        .and_then(Value::as_object_mut)
    else {
        return;
    };
    params.insert("body".to_owned(), Value::Object(Map::new()));
    params.insert("secure".to_owned(), Value::Bool(true));
    params.insert(
        "secure_state".to_owned(),
        Value::String(default_string(state, "failed")),
    );
    let mut secure_wire_content_type = None;
    if let Some(meta) = params.get_mut("meta").and_then(Value::as_object_mut) {
        let original_content_type = string_value(meta.get("content_type"));
        if !original_content_type.is_empty() {
            secure_wire_content_type = Some(original_content_type);
        }
        meta.insert(
            "content_type".to_owned(),
            Value::String(REDACTED_SECURE_DIRECT_CONTENT_TYPE.to_owned()),
        );
    }
    if let Some(content_type) = secure_wire_content_type {
        params.insert(
            "secure_wire_content_type".to_owned(),
            Value::String(content_type),
        );
    }
}

fn apply_plaintext_to_realtime_notification(
    notification: &mut Value,
    plaintext: &Map<String, Value>,
) {
    let Some(params) = notification
        .get_mut("params")
        .and_then(Value::as_object_mut)
    else {
        return;
    };
    let original_body = params.remove("body").unwrap_or(Value::Null);
    let mut secure_wire_content_type = None;
    if let Some(meta) = params.get_mut("meta").and_then(Value::as_object_mut) {
        let original_content_type = string_value(meta.get("content_type"));
        if !original_content_type.is_empty() {
            secure_wire_content_type = Some(original_content_type);
        }
        let content_type = string_value(plaintext.get("application_content_type"));
        if !content_type.is_empty() {
            meta.insert("content_type".to_owned(), Value::String(content_type));
        }
    }
    if let Some(content_type) = secure_wire_content_type {
        params.insert(
            "secure_wire_content_type".to_owned(),
            Value::String(content_type),
        );
    }
    params.insert(
        "body".to_owned(),
        Value::Object(plaintext_body_to_notification_body(plaintext)),
    );
    params.insert("secure".to_owned(), Value::Bool(true));
    params.insert(
        "secure_state".to_owned(),
        Value::String("decrypted".to_owned()),
    );
    // Keep only a marker that secure normalization happened. The original
    // encrypted wire body is intentionally not retained in SDK-visible events.
    if !original_body.is_null() {
        params.insert("secure_wire_body_redacted".to_owned(), Value::Bool(true));
    }
}

fn plaintext_body_to_notification_body(plaintext: &Map<String, Value>) -> Map<String, Value> {
    let mut body = Map::new();
    for key in ["conversation_id", "reply_to_message_id", "annotations"] {
        if let Some(value) = plaintext.get(key).filter(|value| !value.is_null()) {
            body.insert(key.to_owned(), value.clone());
        }
    }
    if let Some(text) = plaintext
        .get("text")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
    {
        body.insert("text".to_owned(), Value::String(text.to_owned()));
    }
    if let Some(payload) = plaintext.get("payload").filter(|value| !value.is_null()) {
        body.insert("payload".to_owned(), payload.clone());
    }
    if let Some(payload_b64u) = plaintext
        .get("payload_b64u")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
    {
        body.insert(
            "payload_b64u".to_owned(),
            Value::String(payload_b64u.to_owned()),
        );
    }
    body
}

fn is_direct_e2ee_incoming_notification(notification: &Value) -> bool {
    if notification.get("method").and_then(Value::as_str) != Some("direct.incoming") {
        return false;
    }
    let content_type = notification
        .pointer("/params/meta/content_type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    anp::direct_e2ee::is_direct_e2ee_wire_content_type(content_type)
}

fn is_direct_e2ee_control_message_projection(message: &Value) -> bool {
    message
        .as_object()
        .and_then(|object| object.get("secure_control"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn is_direct_e2ee_wire_control_message(message: &Value) -> bool {
    let content_type = string_from_message(message, "content_type");
    if content_type == DIRECT_INIT_CONTENT_TYPE {
        return true;
    }
    let mut id = string_from_message(message, "id");
    if id.is_empty() {
        id = string_from_message(message, "msg_id");
    }
    id.starts_with("secure-init-") || id.starts_with("ack-")
}

fn compare_message_order(left: &Value, right: &Value) -> std::cmp::Ordering {
    let left_seq =
        int64_value(left.as_object().and_then(|value| value.get("server_seq"))).unwrap_or_default();
    let right_seq = int64_value(right.as_object().and_then(|value| value.get("server_seq")))
        .unwrap_or_default();
    if left_seq == right_seq {
        return string_from_message(left, "id").cmp(&string_from_message(right, "id"));
    }
    if left_seq == 0 {
        return std::cmp::Ordering::Greater;
    }
    if right_seq == 0 {
        return std::cmp::Ordering::Less;
    }
    left_seq.cmp(&right_seq)
}

fn object_body_value(value: Option<&Value>) -> Option<Value> {
    match value? {
        Value::Object(_) => value.cloned(),
        Value::String(text) if !text.trim().is_empty() => {
            let decoded: Value = serde_json::from_str(text).ok()?;
            decoded.is_object().then_some(decoded)
        }
        _ => None,
    }
}

fn string_from_message(message: &Value, key: &str) -> String {
    message
        .as_object()
        .map(|object| string_value(object.get(key)))
        .unwrap_or_default()
}

fn string_value(value: Option<&Value>) -> String {
    value.and_then(Value::as_str).unwrap_or_default().to_owned()
}

fn first_non_empty_string(values: &[Option<&Value>]) -> String {
    values
        .iter()
        .flatten()
        .filter_map(|value| value.as_str())
        .map(str::trim)
        .find(|value| !value.is_empty())
        .unwrap_or_default()
        .to_owned()
}

fn default_string(value: &str, fallback: &str) -> String {
    if value.trim().is_empty() {
        fallback.to_owned()
    } else {
        value.to_owned()
    }
}

fn int64_value(value: Option<&Value>) -> Option<i64> {
    match value {
        Some(Value::Number(number)) => number
            .as_i64()
            .or_else(|| number.as_u64().and_then(|value| i64::try_from(value).ok()))
            .or_else(|| number.as_f64().map(|value| value as i64)),
        Some(Value::String(value)) => value.trim().parse::<i64>().ok(),
        _ => None,
    }
}

fn compact_warnings(warnings: Vec<String>) -> Vec<String> {
    let mut compact = Vec::new();
    for warning in warnings {
        let warning = warning.trim();
        if warning.is_empty() || compact.iter().any(|known: &String| known == warning) {
            continue;
        }
        compact.push(warning.to_owned());
    }
    compact
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn incoming_processor_decrypts_in_server_order_and_filters_secure_controls() {
        let mut messages = vec![
            json!({
                "id": "msg-2",
                "sender_did": "did:example:bob",
                "receiver_did": "did:example:alice",
                "content_type": DIRECT_CIPHER_CONTENT_TYPE,
                "server_seq": 2,
                "content": {
                    "session_id": "session-1",
                    "ratchet_header": {"dh_pub_b64u": "dh", "pn": "0", "n": "1"},
                    "ciphertext_b64u": "cipher-2"
                }
            }),
            json!({
                "id": "msg-1",
                "sender_did": "did:example:bob",
                "receiver_did": "did:example:alice",
                "content_type": DIRECT_CIPHER_CONTENT_TYPE,
                "server_seq": 1,
                "content": {
                    "session_id": "session-1",
                    "ratchet_header": {"dh_pub_b64u": "dh", "pn": "0", "n": "0"},
                    "ciphertext_b64u": "cipher-1"
                }
            }),
            json!({
                "id": "ack-session-1",
                "sender_did": "did:example:bob",
                "receiver_did": "did:example:alice",
                "content_type": DIRECT_CIPHER_CONTENT_TYPE,
                "server_seq": 3,
                "content": {
                    "session_id": "session-1",
                    "ratchet_header": {"dh_pub_b64u": "dh", "pn": "0", "n": "2"},
                    "ciphertext_b64u": "cipher-ack"
                }
            }),
        ];
        let mut seen = Vec::new();

        let warnings =
            maybe_decrypt_direct_e2ee_messages_with_processor(&mut messages, |notification| {
                let notification = Value::Object(notification);
                let message_id = notification
                    .pointer("/meta/message_id")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned();
                seen.push(message_id.clone());
                let plaintext = if message_id.starts_with("ack-") {
                    json!({
                        "application_content_type": "application/json",
                        "payload": {
                            "system_type": super::super::control::SECURE_ACK_SYSTEM_TYPE,
                            "session_id": "session-1",
                            "acked_message_id": "msg-1"
                        }
                    })
                } else {
                    json!({
                        "application_content_type": "text/plain",
                        "text": format!("plain-{message_id}")
                    })
                };
                Ok(Map::from_iter([
                    ("state".to_owned(), json!("decrypted")),
                    ("plaintext".to_owned(), plaintext),
                ]))
            });

        assert!(warnings.is_empty());
        assert_eq!(seen, vec!["msg-1", "msg-2", "ack-session-1"]);
        let displayable = filter_displayable_direct_e2ee_messages(messages);
        assert_eq!(displayable.len(), 2);
        assert_eq!(displayable[0]["content"], json!("plain-msg-2"));
        assert_eq!(displayable[1]["content"], json!("plain-msg-1"));
        assert_eq!(displayable[0]["content_type"], json!("text/plain"));
    }

    #[test]
    fn incoming_processor_redacts_failed_ciphertext_without_leaking_body() {
        let mut messages = vec![json!({
            "id": "msg-failed",
            "sender_did": "did:example:bob",
            "receiver_did": "did:example:alice",
            "content_type": DIRECT_CIPHER_CONTENT_TYPE,
            "server_seq": 1,
            "content": {
                "session_id": "session-1",
                "ratchet_header": {"dh_pub_b64u": "dh", "pn": "0", "n": "1"},
                "ciphertext_b64u": "SECRET-CIPHERTEXT"
            }
        })];

        let warnings =
            maybe_decrypt_direct_e2ee_messages_with_processor(&mut messages, |_notification| {
                Err(crate::ImError::Serialization {
                    detail: "decrypt failed".to_owned(),
                })
            });

        assert_eq!(warnings.len(), 1);
        assert_eq!(messages[0]["secure"], json!(true));
        assert_eq!(messages[0]["decryption_state"], json!("failed"));
        assert_eq!(messages[0]["content"], Value::Null);
        assert!(!messages[0].to_string().contains("SECRET-CIPHERTEXT"));
    }

    #[test]
    fn direct_notification_from_message_view_accepts_string_content() {
        let notification = direct_e2ee_notification_from_message_view(&json!({
            "id": "msg-1",
            "sender_did": "did:example:bob",
            "receiver_did": "did:example:alice",
            "content_type": DIRECT_CIPHER_CONTENT_TYPE,
            "content": r#"{"session_id":"session-1","ratchet_header":{"dh_pub_b64u":"dh","pn":"0","n":"1"},"ciphertext_b64u":"cipher"}"#,
        }))
        .unwrap();
        let notification = Value::Object(notification);

        assert_eq!(
            notification.pointer("/meta/profile"),
            Some(&json!(DIRECT_E2EE_PROFILE))
        );
        assert_eq!(
            notification.pointer("/meta/security_profile"),
            Some(&json!(DIRECT_E2EE_SECURITY_PROFILE))
        );
        assert_eq!(
            notification.pointer("/body/ciphertext_b64u"),
            Some(&json!("cipher"))
        );
    }

    #[test]
    fn realtime_notification_normalizer_returns_plaintext_without_wire_body() {
        let projection = maybe_normalize_direct_e2ee_notification_with_processor(
            secure_direct_notification("msg-secure", "SECRET-CIPHER"),
            DirectDecryptMode::ReadOnly,
            |_params| {
                Ok(Map::from_iter([
                    ("state".to_owned(), json!("decrypted")),
                    (
                        "plaintext".to_owned(),
                        json!({
                            "application_content_type": "text/plain",
                            "text": "decrypted realtime text"
                        }),
                    ),
                ]))
            },
        );

        assert_eq!(
            projection.decision,
            DirectRealtimeNotificationDecision::Normalized
        );
        assert!(projection.warnings.is_empty());
        let notification = projection.notification.unwrap();
        assert_eq!(
            notification.pointer("/params/meta/content_type"),
            Some(&json!("text/plain"))
        );
        assert_eq!(
            notification.pointer("/params/body/text"),
            Some(&json!("decrypted realtime text"))
        );
        assert_eq!(
            notification.pointer("/params/secure_state"),
            Some(&json!("decrypted"))
        );
        assert_eq!(
            notification.pointer("/params/secure_wire_content_type"),
            Some(&json!(DIRECT_CIPHER_CONTENT_TYPE))
        );
        let encoded = serde_json::to_string(&notification).unwrap();
        assert!(!encoded.contains("SECRET-CIPHER"));
        assert!(!encoded.contains("ratchet_header"));
        assert!(!encoded.contains("session_id"));
    }

    #[test]
    fn realtime_notification_normalizer_drops_secure_control_plaintexts() {
        let projection = maybe_normalize_direct_e2ee_notification_with_processor(
            secure_direct_notification("ack-session-1", "ACK-CIPHER"),
            DirectDecryptMode::ReadOnly,
            |_params| {
                Ok(Map::from_iter([
                    ("state".to_owned(), json!("decrypted")),
                    (
                        "plaintext".to_owned(),
                        json!({
                            "application_content_type": "application/json",
                            "payload": {
                                "system_type": super::super::control::SECURE_ACK_SYSTEM_TYPE,
                                "session_id": "session-1",
                                "acked_message_id": "msg-secure"
                            }
                        }),
                    ),
                ]))
            },
        );

        assert_eq!(
            projection.decision,
            DirectRealtimeNotificationDecision::DroppedControl
        );
        assert!(projection.notification.is_none());
        assert!(projection.warnings.is_empty());
    }

    #[tokio::test]
    async fn realtime_notification_async_side_effect_flushes_outbox_after_ack_via_actor() {
        let temp = tempfile::tempdir().unwrap();
        let sqlite_path = temp.path().join("local").join("im.sqlite");
        let scope = crate::internal::store::e2ee_outbox::E2eeOutboxOwnerScope {
            owner_identity_id: "alice-id".to_owned(),
            owner_did: "did:example:alice".to_owned(),
            credential_name: "alice".to_owned(),
        };
        {
            let connection = crate::internal::local_state::open_writable(&sqlite_path).unwrap();
            crate::internal::store::e2ee_outbox::queue_e2ee_outbox(
                &connection,
                crate::internal::store::e2ee_outbox::E2eeOutboxRecord {
                    outbox_id: "outbox-ack-flush".to_owned(),
                    owner_identity_id: scope.owner_identity_id.clone(),
                    owner_did: scope.owner_did.clone(),
                    credential_name: scope.credential_name.clone(),
                    peer_did: "did:example:bob".to_owned(),
                    original_type: "text".to_owned(),
                    plaintext: "queued after ack".to_owned(),
                    local_status: "queued".to_owned(),
                    created_at: "2026-05-24T00:00:00Z".to_owned(),
                    updated_at: "2026-05-24T00:00:00Z".to_owned(),
                    ..Default::default()
                },
            )
            .unwrap();
        }
        let db = crate::internal::local_state::actor::LocalStateDb::open(sqlite_path.clone())
            .await
            .unwrap();

        let projection = maybe_normalize_direct_e2ee_notification_with_processor_async(
            secure_direct_notification("ack-session-1", "ACK-CIPHER"),
            DirectDecryptMode::WithSideEffects,
            |_params| {
                Ok(Map::from_iter([
                    ("state".to_owned(), json!("decrypted")),
                    (
                        "plaintext".to_owned(),
                        json!({
                            "application_content_type": "application/json",
                            "payload": {
                                "system_type": super::super::control::SECURE_ACK_SYSTEM_TYPE,
                                "session_id": "session-1",
                                "acked_message_id": "msg-secure"
                            }
                        }),
                    ),
                ]))
            },
            |side_effect| {
                let db = db.clone();
                let scope = scope.clone();
                async move {
                    let DirectRealtimeControlSideEffect::FlushOutboxAfterAck { peer_did } =
                        side_effect
                    else {
                        return vec!["unexpected side effect".to_owned()];
                    };
                    super::super::outbox::flush_queued_secure_outbox_with_sender_async(
                        &db,
                        &scope,
                        &peer_did,
                        |request| async move {
                            assert_eq!(request.outbox_id, "outbox-ack-flush");
                            assert_eq!(request.target_did, "did:example:bob");
                            assert_eq!(request.plaintext, "queued after ack");
                            super::super::outbox::SecureOutboxSendResult {
                                session_id: "session-after-ack".to_owned(),
                                send: super::super::outbox::SecureOutboxSendOutcome::Success {
                                    message_id: "msg-after-ack".to_owned(),
                                    operation_id: "outbox-ack-flush".to_owned(),
                                    delivery_state: "accepted".to_owned(),
                                    accepted_at: "2026-05-24T00:00:01Z".to_owned(),
                                },
                            }
                        },
                    )
                    .await
                }
            },
        )
        .await;

        assert_eq!(
            projection.decision,
            DirectRealtimeNotificationDecision::DroppedControl
        );
        assert!(projection.notification.is_none());
        assert!(projection.warnings.is_empty());

        let sent = db
            .list_e2ee_outbox(scope.clone(), Some("sent".to_owned()))
            .await
            .unwrap();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].session_id, "session-after-ack");
        assert_eq!(sent[0].sent_msg_id, "msg-after-ack");
        db.shutdown().await.unwrap();

        let connection = rusqlite::Connection::open(sqlite_path).unwrap();
        let (content, is_e2ee): (String, i64) = connection
            .query_row(
                "SELECT content, is_e2ee FROM messages WHERE msg_id = 'msg-after-ack'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(content, "queued after ack");
        assert_eq!(is_e2ee, 1);
    }

    #[test]
    fn secure_init_ack_request_uses_realtime_wire_session_without_public_projection() {
        let plaintext = Map::from_iter([
            (
                "application_content_type".to_owned(),
                json!("application/json"),
            ),
            (
                "payload".to_owned(),
                json!({
                    "system_type": super::super::control::SECURE_INIT_SYSTEM_TYPE,
                    "reason": "manual_init"
                }),
            ),
        ]);
        let request = ack_request_from_secure_init(
            &secure_direct_init_notification("secure-init-message", "session-wire-secret"),
            " did:example:bob ",
            &plaintext,
        )
        .unwrap();

        assert_eq!(request.peer_did, "did:example:bob");
        assert_eq!(request.message_id, "secure-init-message");
        assert_eq!(request.ack_id, "ack-session-wire-secret");
        assert_eq!(
            Value::Object(request.payload),
            json!({
                "system_type": super::super::control::SECURE_ACK_SYSTEM_TYPE,
                "session_id": "session-wire-secret",
                "acked_message_id": "secure-init-message"
            })
        );

        let projection = normalize_direct_realtime_plaintext_notification(
            secure_direct_init_notification("secure-init-message", "session-wire-secret"),
            DirectDecryptMode::WithSideEffects,
            &mut plaintext.clone(),
            Vec::new(),
        );
        assert_eq!(
            projection.decision,
            DirectRealtimeNotificationDecision::DroppedControl
        );
        assert!(projection.notification.is_none());
        assert!(projection.warnings.is_empty());
    }

    #[test]
    fn secure_init_ack_request_fails_closed_when_session_is_missing() {
        let plaintext = Map::from_iter([
            (
                "application_content_type".to_owned(),
                json!("application/json"),
            ),
            (
                "payload".to_owned(),
                json!({
                    "system_type": super::super::control::SECURE_INIT_SYSTEM_TYPE,
                    "reason": "manual_init"
                }),
            ),
        ]);
        let warning = ack_request_from_secure_init(
            &secure_direct_init_notification("secure-init-message", ""),
            "did:example:bob",
            &plaintext,
        )
        .unwrap_err();

        assert!(warning.contains("session state reference"));
    }

    #[tokio::test]
    async fn realtime_notification_client_async_normalizer_uses_actor_cas_for_established_cipher() {
        let fixture = super::super::async_receive::test_support::Fixture::new();
        let client = fixture.client();
        let exchange = super::super::async_receive::test_support::established_exchange();
        let db = client.core_inner().local_state_db().await.unwrap();
        db.save_direct_secure_session_if_revision(
            super::super::sqlite_store::DirectSessionRecord {
                owner_identity_id: "alice-id".to_owned(),
                owner_did: "did:example:alice".to_owned(),
                peer_did: "did:example:bob".to_owned(),
                session_id: exchange.alice_session.session_id.clone(),
                state_blob: super::super::sqlite_store::direct_session_to_blob(
                    &exchange.alice_session,
                )
                .unwrap(),
                metadata_json: super::super::sqlite_store::direct_session_metadata_json(
                    &exchange.alice_session,
                )
                .unwrap(),
                revision: 0,
                created_at: "2026-05-24T00:00:00Z".to_owned(),
                updated_at: "2026-05-24T00:00:00Z".to_owned(),
            },
            0,
        )
        .await
        .unwrap();
        let notification =
            super::super::async_receive::test_support::direct_cipher_realtime_notification(
                &exchange.message_metadata,
                &exchange.cipher_body,
            );

        let outcome = try_normalize_direct_e2ee_notification_for_client_async(
            &client,
            notification,
            DirectDecryptMode::WithSideEffects,
            |_side_effect| async { vec!["unexpected side effect".to_owned()] },
        )
        .await;

        let DirectRealtimeAsyncProjectionOutcome::Projected(projection) = outcome else {
            panic!("established cipher should be normalized by async realtime path");
        };
        assert_eq!(
            projection.decision,
            DirectRealtimeNotificationDecision::Normalized
        );
        assert!(projection.warnings.is_empty());
        let notification = projection.notification.unwrap();
        assert_eq!(
            notification.pointer("/params/body/text"),
            Some(&json!("async receive secret"))
        );
        assert_eq!(
            notification.pointer("/params/secure_state"),
            Some(&json!("decrypted"))
        );
        let encoded = serde_json::to_string(&notification).unwrap();
        assert!(!encoded.contains("ciphertext_b64u"));
        assert!(!encoded.contains("ratchet_header"));

        let saved = db
            .get_direct_secure_session("alice-id", "did:example:bob")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(saved.revision, 1);
        let saved_session =
            super::super::sqlite_store::direct_session_from_blob(&saved.state_blob).unwrap();
        assert_eq!(saved_session.recv_n, exchange.alice_session.recv_n + 1);
    }

    #[tokio::test]
    async fn realtime_notification_client_async_normalizer_falls_back_without_established_session()
    {
        let fixture = super::super::async_receive::test_support::Fixture::new();
        let client = fixture.client();
        let exchange = super::super::async_receive::test_support::established_exchange();
        let notification =
            super::super::async_receive::test_support::direct_cipher_realtime_notification(
                &exchange.message_metadata,
                &exchange.cipher_body,
            );

        let outcome = try_normalize_direct_e2ee_notification_for_client_async(
            &client,
            notification.clone(),
            DirectDecryptMode::ReadOnly,
            |_side_effect| async { Vec::new() },
        )
        .await;

        assert_eq!(
            outcome,
            DirectRealtimeAsyncProjectionOutcome::Fallback(notification)
        );
    }

    #[tokio::test]
    async fn realtime_notification_client_async_normalizer_sends_ack_after_secure_init_control() {
        let fixture = super::super::async_receive::test_support::Fixture::new();
        let client = fixture.client();
        let exchange = super::super::async_receive::test_support::established_exchange();
        let db = client.core_inner().local_state_db().await.unwrap();
        db.save_direct_secure_session_if_revision(
            super::super::sqlite_store::DirectSessionRecord {
                owner_identity_id: "alice-id".to_owned(),
                owner_did: "did:example:alice".to_owned(),
                peer_did: "did:example:bob".to_owned(),
                session_id: exchange.alice_session.session_id.clone(),
                state_blob: super::super::sqlite_store::direct_session_to_blob(
                    &exchange.alice_session,
                )
                .unwrap(),
                metadata_json: super::super::sqlite_store::direct_session_metadata_json(
                    &exchange.alice_session,
                )
                .unwrap(),
                revision: 0,
                created_at: "2026-05-24T00:00:00Z".to_owned(),
                updated_at: "2026-05-24T00:00:00Z".to_owned(),
            },
            0,
        )
        .await
        .unwrap();

        let mut bob_session = exchange.bob_session.clone();
        let metadata = anp::direct_e2ee::DirectEnvelopeMetadata {
            sender_did: "did:example:bob".to_owned(),
            recipient_did: "did:example:alice".to_owned(),
            message_id: "secure-init-control-msg".to_owned(),
            profile: DIRECT_E2EE_PROFILE.to_owned(),
            security_profile: DIRECT_E2EE_SECURITY_PROFILE.to_owned(),
        };
        let mut init_payload = super::super::control::build_secure_init_payload();
        init_payload.insert(
            "session_id".to_owned(),
            Value::String(exchange.alice_session.session_id.clone()),
        );
        let (_pending, init_control_body) = anp::direct_e2ee::DirectE2eeSession::encrypt_follow_up(
            &mut bob_session,
            &metadata,
            "secure-init-control-msg",
            &anp::direct_e2ee::ApplicationPlaintext::new_json(
                "application/json",
                Value::Object(init_payload),
            ),
        )
        .unwrap();
        let notification =
            super::super::async_receive::test_support::direct_cipher_realtime_notification(
                &metadata,
                &init_control_body,
            );
        let calls = std::sync::Arc::new(std::sync::Mutex::new(Vec::<Value>::new()));
        let mut message_transport = RecordingAsyncTransport {
            calls: std::sync::Arc::clone(&calls),
        };
        let mut directory_transport = NoopAsyncTransport;

        let outcome = try_normalize_direct_e2ee_notification_for_client_with_transports_async(
            &client,
            notification,
            DirectDecryptMode::WithSideEffects,
            &mut message_transport,
            &mut directory_transport,
        )
        .await;

        let DirectRealtimeAsyncProjectionOutcome::Projected(projection) = outcome else {
            panic!("secure init control should be projected by async realtime path");
        };
        assert_eq!(
            projection.decision,
            DirectRealtimeNotificationDecision::DroppedControl
        );
        assert!(projection.notification.is_none());
        assert!(projection.warnings.is_empty());
        let calls = calls.lock().unwrap().clone();
        assert_eq!(calls.len(), 1);
        assert_eq!(
            calls[0].pointer("/meta/content_type"),
            Some(&json!("application/anp-direct-cipher+json"))
        );
        assert!(!calls[0].to_string().contains("awiki.direct.secure_ack.v1"));
        let saved = db
            .get_direct_secure_session("alice-id", "did:example:bob")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(saved.revision, 2);
    }

    #[tokio::test]
    async fn realtime_notification_client_async_normalizer_resolves_init_sender_document_over_async_directory(
    ) {
        let exchange = super::super::async_receive::test_support::incoming_init_exchange();
        let fixture = super::super::async_receive::test_support::Fixture::with_identity(
            &exchange.recipient_did,
            exchange.recipient_document.clone(),
        );
        let client = fixture.client();
        std::fs::write(
            fixture.identity_dir().join("e2ee-agreement-private.pem"),
            exchange.recipient_agreement_private.to_pem(),
        )
        .unwrap();
        seed_direct_init_prekeys(&fixture, &exchange);
        let notification = Value::Object(Map::from_iter([
            (
                "method".to_owned(),
                Value::String("direct.incoming".to_owned()),
            ),
            (
                "params".to_owned(),
                Value::Object(
                    super::super::async_receive::test_support::direct_init_notification(
                        &exchange.metadata,
                        &exchange.init_body,
                    ),
                ),
            ),
        ]));
        let mut message_transport = RecordingAsyncTransport {
            calls: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        };
        let directory_calls = std::sync::Arc::new(std::sync::Mutex::new(Vec::<Value>::new()));
        let mut directory_transport = ResolvingAsyncDirectoryTransport {
            did: exchange.sender_did.clone(),
            document: exchange.sender_document.clone(),
            calls: std::sync::Arc::clone(&directory_calls),
        };

        let outcome = try_normalize_direct_e2ee_notification_for_client_with_transports_async(
            &client,
            notification,
            DirectDecryptMode::ReadOnly,
            &mut message_transport,
            &mut directory_transport,
        )
        .await;

        let DirectRealtimeAsyncProjectionOutcome::Projected(projection) = outcome else {
            panic!("direct init should be normalized by async realtime path after DID resolve");
        };
        assert_eq!(
            projection.decision,
            DirectRealtimeNotificationDecision::Normalized
        );
        assert!(projection.warnings.is_empty());
        let notification = projection.notification.unwrap();
        assert_eq!(
            notification.pointer("/params/body/text"),
            Some(&json!("hello from init"))
        );
        assert_eq!(
            notification.pointer("/params/secure_state"),
            Some(&json!("decrypted"))
        );
        assert_eq!(directory_calls.lock().unwrap().len(), 1);
        let saved = client
            .core_inner()
            .local_state_db()
            .await
            .unwrap()
            .get_direct_secure_session("alice-id", exchange.sender_did)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(saved.revision, 0);
    }

    #[tokio::test]
    async fn realtime_notification_client_async_normalizer_redacts_bad_cipher_without_saving() {
        let fixture = super::super::async_receive::test_support::Fixture::new();
        let client = fixture.client();
        let mut exchange = super::super::async_receive::test_support::established_exchange();
        let db = client.core_inner().local_state_db().await.unwrap();
        db.save_direct_secure_session_if_revision(
            super::super::sqlite_store::DirectSessionRecord {
                owner_identity_id: "alice-id".to_owned(),
                owner_did: "did:example:alice".to_owned(),
                peer_did: "did:example:bob".to_owned(),
                session_id: exchange.alice_session.session_id.clone(),
                state_blob: super::super::sqlite_store::direct_session_to_blob(
                    &exchange.alice_session,
                )
                .unwrap(),
                metadata_json: super::super::sqlite_store::direct_session_metadata_json(
                    &exchange.alice_session,
                )
                .unwrap(),
                revision: 0,
                created_at: "2026-05-24T00:00:00Z".to_owned(),
                updated_at: "2026-05-24T00:00:00Z".to_owned(),
            },
            0,
        )
        .await
        .unwrap();
        exchange.cipher_body.ciphertext_b64u =
            super::super::async_receive::test_support::corrupt_b64u_tail(
                &exchange.cipher_body.ciphertext_b64u,
            );
        let notification =
            super::super::async_receive::test_support::direct_cipher_realtime_notification(
                &exchange.message_metadata,
                &exchange.cipher_body,
            );

        let outcome = try_normalize_direct_e2ee_notification_for_client_async(
            &client,
            notification,
            DirectDecryptMode::ReadOnly,
            |_side_effect| async { Vec::new() },
        )
        .await;

        let DirectRealtimeAsyncProjectionOutcome::Projected(projection) = outcome else {
            panic!("bad established cipher should produce an async projection");
        };
        assert_eq!(
            projection.decision,
            DirectRealtimeNotificationDecision::Redacted
        );
        assert!(projection.warnings.is_empty());
        let notification = projection.notification.unwrap();
        assert_eq!(
            notification.pointer("/params/secure_state"),
            Some(&json!("undecryptable"))
        );
        let encoded = serde_json::to_string(&notification).unwrap();
        assert!(!encoded.contains("ciphertext_b64u"));
        assert!(!encoded.contains("ratchet_header"));
        let saved = db
            .get_direct_secure_session("alice-id", "did:example:bob")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(saved.revision, 0);
    }

    #[test]
    fn realtime_notification_normalizer_redacts_failed_ciphertext() {
        let projection = maybe_normalize_direct_e2ee_notification_with_processor(
            secure_direct_notification("msg-failed", "FAILED-CIPHER"),
            DirectDecryptMode::ReadOnly,
            |_params| {
                Err(crate::ImError::Serialization {
                    detail: "decrypt failed".to_owned(),
                })
            },
        );

        assert_eq!(
            projection.decision,
            DirectRealtimeNotificationDecision::Redacted
        );
        assert_eq!(projection.warnings.len(), 1);
        let notification = projection.notification.unwrap();
        assert_eq!(
            notification.pointer("/params/meta/content_type"),
            Some(&json!(REDACTED_SECURE_DIRECT_CONTENT_TYPE))
        );
        assert_eq!(notification.pointer("/params/body"), Some(&json!({})));
        assert_eq!(
            notification.pointer("/params/secure_state"),
            Some(&json!("failed"))
        );
        let encoded = serde_json::to_string(&notification).unwrap();
        assert!(!encoded.contains("FAILED-CIPHER"));
        assert!(!encoded.contains("ratchet_header"));
    }

    struct RecordingAsyncTransport {
        calls: std::sync::Arc<std::sync::Mutex<Vec<Value>>>,
    }

    impl crate::internal::transport::AsyncAuthenticatedRpcTransport for RecordingAsyncTransport {
        async fn authenticated_rpc(
            &mut self,
            endpoint: &str,
            method: &str,
            params: Value,
        ) -> crate::ImResult<Value> {
            assert_eq!(endpoint, super::super::send::MESSAGE_RPC_ENDPOINT);
            assert_eq!(method, "direct.send");
            self.calls.lock().unwrap().push(params);
            Ok(json!({
                "accepted": true,
                "message_id": "ack-secure-init",
                "operation_id": "ack-secure-init",
                "delivery_state": "accepted",
                "accepted_at": "2026-05-24T00:00:01Z"
            }))
        }
    }

    struct NoopAsyncTransport;

    impl crate::internal::transport::AsyncRpcTransport for NoopAsyncTransport {
        async fn rpc(
            &mut self,
            _endpoint: &str,
            _method: &str,
            _params: Value,
        ) -> crate::ImResult<Value> {
            Err(crate::ImError::PeerNotFound {
                peer: "noop".to_owned(),
            })
        }
    }

    struct ResolvingAsyncDirectoryTransport {
        did: String,
        document: Value,
        calls: std::sync::Arc<std::sync::Mutex<Vec<Value>>>,
    }

    impl crate::internal::transport::AsyncRpcTransport for ResolvingAsyncDirectoryTransport {
        async fn rpc(
            &mut self,
            endpoint: &str,
            method: &str,
            params: Value,
        ) -> crate::ImResult<Value> {
            assert_eq!(
                endpoint,
                crate::internal::identity_wire::DID_PROFILE_RPC_ENDPOINT
            );
            assert_eq!(method, "resolve");
            assert_eq!(params.get("did"), Some(&json!(self.did)));
            self.calls.lock().unwrap().push(params);
            Ok(json!({
                "profile": {
                    "did_document": self.document.clone()
                }
            }))
        }
    }

    fn seed_direct_init_prekeys(
        fixture: &super::super::async_receive::test_support::Fixture,
        exchange: &super::super::async_receive::test_support::IncomingInitExchange,
    ) {
        let connection =
            crate::internal::local_state::open_writable(&fixture.sqlite_path()).unwrap();
        let store =
            super::super::sqlite_store::SqliteDirectSecureStateStore::new(&connection).unwrap();
        store
            .upsert_signed_prekey(&super::super::sqlite_store::DirectSignedPrekeyRecord {
                owner_identity_id: "alice-id".to_owned(),
                owner_did: exchange.recipient_did.clone(),
                key_id: exchange.recipient_signed_prekey.key_id.clone(),
                private_key_blob: exchange
                    .recipient_signed_prekey_private
                    .to_pem()
                    .into_bytes(),
                public_key_blob: exchange
                    .recipient_signed_prekey
                    .public_key_b64u
                    .as_bytes()
                    .to_vec(),
                status: super::super::sqlite_store::DirectPrekeyStatus::Active,
                metadata_json: serde_json::to_string(&json!({
                    "metadata": exchange.recipient_signed_prekey,
                }))
                .unwrap(),
                created_at: "2026-05-24T00:00:00Z".to_owned(),
                updated_at: "2026-05-24T00:00:00Z".to_owned(),
            })
            .unwrap();
        store
            .upsert_one_time_prekey(&super::super::sqlite_store::DirectOneTimePrekeyRecord {
                owner_identity_id: "alice-id".to_owned(),
                owner_did: exchange.recipient_did.clone(),
                key_id: exchange.recipient_one_time_prekey.key_id.clone(),
                private_key_blob: exchange
                    .recipient_one_time_prekey_private
                    .to_pem()
                    .into_bytes(),
                public_key_blob: exchange
                    .recipient_one_time_prekey
                    .public_key_b64u
                    .as_bytes()
                    .to_vec(),
                status: super::super::sqlite_store::DirectPrekeyStatus::Available,
                metadata_json: serde_json::to_string(&json!({
                    "metadata": exchange.recipient_one_time_prekey,
                }))
                .unwrap(),
                created_at: "2026-05-24T00:00:00Z".to_owned(),
                consumed_at: String::new(),
            })
            .unwrap();
    }

    fn secure_direct_notification(message_id: &str, ciphertext: &str) -> Value {
        json!({
            "method": "direct.incoming",
            "params": {
                "meta": {
                    "message_id": message_id,
                    "sender_did": "did:example:bob",
                    "target": {"kind": "agent", "did": "did:example:alice"},
                    "content_type": DIRECT_CIPHER_CONTENT_TYPE,
                    "profile": DIRECT_E2EE_PROFILE,
                    "security_profile": DIRECT_E2EE_SECURITY_PROFILE,
                },
                "body": {
                    "session_id": "session-1",
                    "ratchet_header": {"dh_pub_b64u": "dh", "pn": "0", "n": "1"},
                    "ciphertext_b64u": ciphertext
                }
            }
        })
    }

    fn secure_direct_init_notification(message_id: &str, session_id: &str) -> Value {
        json!({
            "method": "direct.incoming",
            "params": {
                "meta": {
                    "message_id": message_id,
                    "sender_did": "did:example:bob",
                    "target": {"kind": "agent", "did": "did:example:alice"},
                    "content_type": DIRECT_INIT_CONTENT_TYPE,
                    "profile": DIRECT_E2EE_PROFILE,
                    "security_profile": DIRECT_E2EE_SECURITY_PROFILE,
                },
                "body": {
                    "session_id": session_id,
                    "sender_ephemeral_key_b64u": "ephemeral",
                    "sender_static_key_agreement_id": "did:example:bob#key-3",
                    "recipient_signed_prekey_id": "spk-initial",
                    "ciphertext_b64u": "SECRET-INIT-CIPHER"
                }
            }
        })
    }
}
