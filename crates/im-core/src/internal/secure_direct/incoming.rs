use serde_json::{Map, Value};
use std::future::Future;

use crate::internal::transport::{AsyncAuthenticatedRpcTransport, AsyncRpcTransport, RpcTransport};

#[cfg(any(feature = "blocking", test))]
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

#[cfg(any(feature = "blocking", test))]
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
    let identity_material = match super::identity_material::local_identity_material(client) {
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
    let local_document_for_resolver = identity_material.local_did_document.clone();
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
        owner_identity_id: identity_material.owner_identity_id,
        owner_did: identity_material.owner_did,
        identity_name: identity_material.identity_name,
        signing_key_id: identity_material.signing_key_id,
        agreement_key_id: identity_material.agreement_key_id,
        identity_signer: std::sync::Arc::clone(&identity_material.identity_signer),
        local_did_document: identity_material.local_did_document,
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

#[cfg(not(any(feature = "blocking", test)))]
pub(crate) fn maybe_decrypt_direct_e2ee_messages_for_client<R>(
    _client: &crate::core::ImClient,
    messages: &mut [Value],
    _directory_transport: &mut R,
    _mode: DirectDecryptMode,
) -> Vec<String>
where
    R: RpcTransport,
{
    if !messages.is_empty() && contains_direct_e2ee_messages(messages) {
        vec!["direct E2EE sync read fallback is disabled".to_owned()]
    } else {
        Vec::new()
    }
}

#[cfg(any(feature = "blocking", test))]
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
    let identity_material = match super::identity_material::local_identity_material(client) {
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
    let local_document_for_resolver = identity_material.local_did_document.clone();
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
            detail: format!("historical direct E2EE reader does not call {method}"),
        })
    });
    let prepared = match prepare_direct_secure_client(DirectSecureClientInput {
        owner_identity_id: identity_material.owner_identity_id,
        owner_did: identity_material.owner_did,
        identity_name: identity_material.identity_name,
        signing_key_id: identity_material.signing_key_id,
        agreement_key_id: identity_material.agreement_key_id,
        identity_signer: std::sync::Arc::clone(&identity_material.identity_signer),
        local_did_document: identity_material.local_did_document,
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
    normalize_direct_realtime_plaintext_notification(notification, mode, &mut plaintext, Vec::new())
}

#[cfg(not(any(feature = "blocking", test)))]
pub(crate) fn maybe_normalize_direct_e2ee_notification_for_client<R>(
    _client: &crate::core::ImClient,
    notification: Value,
    _directory_transport: &mut R,
    _mode: DirectDecryptMode,
) -> DirectRealtimeNotificationProjection
where
    R: RpcTransport,
{
    DirectRealtimeNotificationProjection {
        notification: Some(notification),
        additional_notifications: Vec::new(),
        warnings: vec!["direct E2EE sync realtime fallback is disabled".to_owned()],
        decision: DirectRealtimeNotificationDecision::KeepOriginal,
    }
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
    _message_transport: &mut M,
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
    let mut projection = normalize_direct_realtime_plaintext_notification(
        notification,
        mode,
        &mut plaintext,
        Vec::new(),
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
        return super::identity_material::local_did_document(client);
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
    let application_content_type = plaintext
        .get("application_content_type")
        .and_then(Value::as_str)
        .unwrap_or_default();
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
        let payload = if application_content_type == ATTACHMENT_MANIFEST_CONTENT_TYPE {
            crate::attachments::manifest::redact_attachment_manifest(payload)
        } else {
            payload.clone()
        };
        body.insert("payload".to_owned(), payload);
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
mod tests;
