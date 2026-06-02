use std::collections::HashMap;

use serde_json::{Map, Value};

use crate::internal::auth::session::{AsyncSessionProvider, SessionProvider};
use crate::internal::transport::{
    AsyncAuthenticatedRpcTransport, AsyncRpcTransport, AuthenticatedRpcTransport, RpcTransport,
};

pub(crate) const MESSAGE_RPC_ENDPOINT: &str = "/im/rpc";

pub(crate) struct MessageReadRuntime<'a, P, T, R> {
    client: &'a crate::core::ImClient,
    session_provider: P,
    transport: T,
    directory_transport: R,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct InboxRead {
    pub query: crate::messages::InboxQuery,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct HistoryRead {
    pub thread: crate::messages::ThreadRef,
    pub query: crate::messages::HistoryQuery,
    pub resolved_peer_did: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ReadPageResult {
    pub page: crate::ids::Page<crate::messages::Message>,
    pub raw: Value,
}

impl<'a, P, T, R> MessageReadRuntime<'a, P, T, R>
where
    P: SessionProvider,
    T: AuthenticatedRpcTransport,
    R: RpcTransport,
{
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

    pub(crate) fn inbox(mut self, input: InboxRead) -> crate::ImResult<ReadPageResult> {
        self.session_provider
            .ensure_session(crate::auth::AuthScope::Messaging)?;
        let limit = page_limit(input.query.limit, 20);
        let params = crate::internal::wire::inbox::build_inbox_rpc_params(
            &crate::internal::wire::common::WireIdentity {
                did: self.client.did().as_str().to_string(),
            },
            crate::internal::wire::inbox::InboxWireRequest { limit },
        );
        let mut raw =
            self.transport
                .authenticated_rpc(MESSAGE_RPC_ENDPOINT, "inbox.get", params)?;
        project_secure_direct_messages(self.client, &mut raw, &mut self.directory_transport);
        let page = page_from_raw(&raw, input.query.limit)?;
        persist_projection_best_effort(self.client, &page.items);
        Ok(ReadPageResult { page, raw })
    }

    pub(crate) fn history(mut self, input: HistoryRead) -> crate::ImResult<ReadPageResult> {
        match input.thread {
            crate::messages::ThreadRef::Direct(peer) => {
                self.session_provider
                    .ensure_session(crate::auth::AuthScope::Messaging)?;
                let peer = direct_thread(peer, input.resolved_peer_did)?;
                let params = crate::internal::wire::history::build_history_rpc_params(
                    &crate::internal::wire::common::WireIdentity {
                        did: self.client.did().as_str().to_string(),
                    },
                    crate::internal::wire::history::HistoryWireRequest {
                        peer_did: peer.resolved_did.clone(),
                        limit: page_limit(input.query.limit, 50),
                        cursor: input.query.cursor.map(|cursor| cursor.as_str().to_string()),
                        skip: 0,
                    },
                )?;
                let mut raw = self.transport.authenticated_rpc(
                    MESSAGE_RPC_ENDPOINT,
                    "direct.get_history",
                    params,
                )?;
                project_secure_direct_messages(
                    self.client,
                    &mut raw,
                    &mut self.directory_transport,
                );
                let page = page_from_raw(&raw, input.query.limit)?;
                persist_projection_best_effort(self.client, &page.items);
                Ok(ReadPageResult { page, raw })
            }
            crate::messages::ThreadRef::Group(group) => {
                self.session_provider
                    .ensure_session(crate::auth::AuthScope::GroupMessaging)?;
                let params = crate::internal::wire::group::build_group_messages_rpc_params(
                    self.client.did().as_str(),
                    group.as_str(),
                    page_limit(input.query.limit, 50),
                    input.query.cursor.as_ref().map(crate::ids::Cursor::as_str),
                    0,
                )?;
                let mut raw = self.transport.authenticated_rpc(
                    MESSAGE_RPC_ENDPOINT,
                    "group.list_messages",
                    params,
                )?;
                project_group_e2ee_messages(self.client, &mut raw);
                let page = page_from_raw_with_group(&raw, input.query.limit, Some(&group))?;
                persist_projection_best_effort(self.client, &page.items);
                Ok(ReadPageResult { page, raw })
            }
            crate::messages::ThreadRef::Thread(_) => {
                Err(crate::ImError::unsupported("thread-history"))
            }
        }
    }
}

impl<'a, P, T, R> MessageReadRuntime<'a, P, T, R>
where
    P: AsyncSessionProvider,
    T: AsyncAuthenticatedRpcTransport,
    R: AsyncRpcTransport,
{
    pub(crate) async fn inbox_async(mut self, input: InboxRead) -> crate::ImResult<ReadPageResult> {
        self.session_provider
            .ensure_session(crate::auth::AuthScope::Messaging)
            .await?;
        let limit = page_limit(input.query.limit, 20);
        let params = crate::internal::wire::inbox::build_inbox_rpc_params(
            &crate::internal::wire::common::WireIdentity {
                did: self.client.did().as_str().to_string(),
            },
            crate::internal::wire::inbox::InboxWireRequest { limit },
        );
        let mut raw = self
            .transport
            .authenticated_rpc(MESSAGE_RPC_ENDPOINT, "inbox.get", params)
            .await?;
        project_secure_direct_messages_async(self.client, &mut raw, &mut self.directory_transport)
            .await;
        let page = page_from_raw(&raw, input.query.limit)?;
        persist_projection_best_effort_async(self.client, &page.items).await;
        Ok(ReadPageResult { page, raw })
    }

    pub(crate) async fn history_async(
        mut self,
        input: HistoryRead,
    ) -> crate::ImResult<ReadPageResult> {
        match input.thread {
            crate::messages::ThreadRef::Direct(peer) => {
                self.session_provider
                    .ensure_session(crate::auth::AuthScope::Messaging)
                    .await?;
                let peer = direct_thread(peer, input.resolved_peer_did)?;
                let params = crate::internal::wire::history::build_history_rpc_params(
                    &crate::internal::wire::common::WireIdentity {
                        did: self.client.did().as_str().to_string(),
                    },
                    crate::internal::wire::history::HistoryWireRequest {
                        peer_did: peer.resolved_did.clone(),
                        limit: page_limit(input.query.limit, 50),
                        cursor: input.query.cursor.map(|cursor| cursor.as_str().to_string()),
                        skip: 0,
                    },
                )?;
                let mut raw = self
                    .transport
                    .authenticated_rpc(MESSAGE_RPC_ENDPOINT, "direct.get_history", params)
                    .await?;
                project_secure_direct_messages_async(
                    self.client,
                    &mut raw,
                    &mut self.directory_transport,
                )
                .await;
                let page = page_from_raw(&raw, input.query.limit)?;
                persist_projection_best_effort_async(self.client, &page.items).await;
                Ok(ReadPageResult { page, raw })
            }
            crate::messages::ThreadRef::Group(group) => {
                self.session_provider
                    .ensure_session(crate::auth::AuthScope::GroupMessaging)
                    .await?;
                let params = crate::internal::wire::group::build_group_messages_rpc_params(
                    self.client.did().as_str(),
                    group.as_str(),
                    page_limit(input.query.limit, 50),
                    input.query.cursor.as_ref().map(crate::ids::Cursor::as_str),
                    0,
                )?;
                let mut raw = self
                    .transport
                    .authenticated_rpc(MESSAGE_RPC_ENDPOINT, "group.list_messages", params)
                    .await?;
                project_group_e2ee_messages_async(self.client, &mut raw).await;
                let page = page_from_raw_with_group(&raw, input.query.limit, Some(&group))?;
                persist_projection_best_effort_async(self.client, &page.items).await;
                Ok(ReadPageResult { page, raw })
            }
            crate::messages::ThreadRef::Thread(_) => {
                Err(crate::ImError::unsupported("thread-history"))
            }
        }
    }
}

fn persist_projection_best_effort(
    client: &crate::core::ImClient,
    messages: &[crate::messages::Message],
) {
    let _ = crate::internal::message_runtime::local_projection::persist_messages(client, messages);
}

async fn persist_projection_best_effort_async(
    client: &crate::core::ImClient,
    messages: &[crate::messages::Message],
) {
    let _ = crate::internal::message_runtime::local_projection::persist_messages_async(
        client, messages,
    )
    .await;
}

struct DirectThread {
    resolved_did: String,
}

fn direct_thread(
    peer: crate::ids::PeerRef,
    resolved_peer_did: Option<String>,
) -> crate::ImResult<DirectThread> {
    let resolved = resolved_peer_did
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| peer.as_str().trim());
    if !resolved.starts_with("did:") {
        return Err(crate::ImError::PeerNotFound {
            peer: peer.as_str().to_string(),
        });
    }
    Ok(DirectThread {
        resolved_did: resolved.to_string(),
    })
}

fn page_limit(limit: crate::ids::PageLimit, fallback: i64) -> i64 {
    if limit.0 == 0 {
        fallback
    } else {
        i64::from(limit.0)
    }
}

fn page_from_raw(
    raw: &Value,
    requested_limit: crate::ids::PageLimit,
) -> crate::ImResult<crate::ids::Page<crate::messages::Message>> {
    page_from_raw_with_group(raw, requested_limit, None)
}

fn page_from_raw_with_group(
    raw: &Value,
    requested_limit: crate::ids::PageLimit,
    group: Option<&crate::ids::GroupRef>,
) -> crate::ImResult<crate::ids::Page<crate::messages::Message>> {
    let messages = raw
        .get("messages")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| message_from_value(item, group).transpose())
                .collect::<crate::ImResult<Vec<_>>>()
        })
        .transpose()?
        .unwrap_or_default();
    let limit = usize::try_from(requested_limit.0).unwrap_or_default();
    let has_more = raw
        .get("has_more")
        .and_then(Value::as_bool)
        .unwrap_or(limit > 0 && messages.len() >= limit);
    let next_cursor = raw
        .get("next_cursor")
        .or_else(|| raw.get("next_since_seq"))
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(crate::ids::Cursor::parse)
        .transpose()?;
    Ok(crate::ids::Page {
        items: messages,
        next_cursor,
        has_more,
    })
}

fn project_secure_direct_messages(
    client: &crate::core::ImClient,
    raw: &mut Value,
    directory_transport: &mut impl RpcTransport,
) {
    #[cfg(not(feature = "sqlite"))]
    {
        let _ = (client, raw, directory_transport);
    }
    #[cfg(feature = "sqlite")]
    {
        let Some(messages) = raw.get_mut("messages").and_then(Value::as_array_mut) else {
            return;
        };
        let mut message_values = std::mem::take(messages);
        let warnings =
            crate::internal::secure_direct::incoming::maybe_decrypt_direct_e2ee_messages_for_client(
                client,
                &mut message_values,
                directory_transport,
                crate::internal::secure_direct::incoming::DirectDecryptMode::ReadOnly,
            );
        let filtered =
            crate::internal::secure_direct::incoming::filter_displayable_direct_e2ee_messages(
                message_values,
            );
        *messages = filtered;
        append_secure_direct_warnings(raw, warnings);
    }
}

#[cfg(feature = "sqlite")]
pub(crate) fn project_secure_direct_messages_for_attachment_download(
    client: &crate::core::ImClient,
    raw: &mut Value,
    directory_transport: &mut impl RpcTransport,
) {
    project_secure_direct_messages(client, raw, directory_transport);
}

async fn project_secure_direct_messages_async(
    client: &crate::core::ImClient,
    raw: &mut Value,
    directory_transport: &mut impl AsyncRpcTransport,
) {
    #[cfg(not(feature = "sqlite"))]
    {
        let _ = (client, raw, directory_transport);
    }
    #[cfg(feature = "sqlite")]
    {
        let Some(messages) = raw.get_mut("messages").and_then(Value::as_array_mut) else {
            return;
        };
        let mut message_values = std::mem::take(messages);
        let mut processed_async = vec![false; message_values.len()];
        let mut async_warnings = Vec::new();
        let mut pending_cipher_indices: HashMap<String, Vec<usize>> = HashMap::new();
        let async_receive =
            crate::internal::secure_direct::async_receive::AsyncDirectSecureIncomingProcessor::new(
                client,
            );
        let mut order = (0..message_values.len()).collect::<Vec<_>>();
        order.sort_by(|left, right| {
            compare_secure_direct_message_order(&message_values[*left], &message_values[*right])
        });
        for index in order {
            let content_type = direct_message_content_type(&message_values[index]);
            let notification = match crate::internal::secure_direct::incoming::direct_e2ee_notification_from_message_view(&message_values[index]) {
                Ok(notification) => notification,
                Err(_) => continue,
            };
            let pending_cipher_message_id = if content_type == "application/anp-direct-cipher+json"
            {
                direct_notification_message_id(&notification)
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or_else(|| secure_direct_message_id(&message_values[index]))
            } else {
                String::new()
            };
            let result = if content_type == "application/anp-direct-init+json" {
                let sender_did = notification
                    .get("meta")
                    .and_then(Value::as_object)
                    .and_then(|meta| meta.get("sender_did"))
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned();
                let sender_document = match resolve_direct_sender_document_async(
                    client,
                    directory_transport,
                    &sender_did,
                )
                .await
                {
                    Ok(document) => document,
                    Err(err) => {
                        mark_async_direct_failure(
                            &mut message_values[index],
                            &mut async_warnings,
                            err,
                        );
                        continue;
                    }
                };
                async_receive
                    .process_init_if_ready(notification, sender_document)
                    .await
            } else if content_type == "application/anp-direct-cipher+json" {
                async_receive.process_cipher_if_ready(notification).await
            } else {
                continue;
            };
            match result {
                Ok(crate::internal::secure_direct::async_receive::AsyncDirectSecureReceiveOutcome::Processed(result)) => {
                    crate::internal::secure_direct::incoming::apply_direct_e2ee_processing_result(
                        &mut message_values[index],
                        &Value::Object(result),
                    );
                    processed_async[index] = true;
                }
                Ok(crate::internal::secure_direct::async_receive::AsyncDirectSecureReceiveOutcome::ProcessedWithReplay {
                    result,
                    replayed,
                }) => {
                    crate::internal::secure_direct::incoming::apply_direct_e2ee_processing_result(
                        &mut message_values[index],
                        &Value::Object(result),
                    );
                    processed_async[index] = true;
                    for replay in replayed {
                        let Some(indices) = pending_cipher_indices.get_mut(&replay.message_id) else {
                            continue;
                        };
                        let Some(pending_index) = indices.pop() else {
                            continue;
                        };
                        crate::internal::secure_direct::incoming::apply_direct_e2ee_processing_result(
                            &mut message_values[pending_index],
                            &Value::Object(replay.result),
                        );
                        processed_async[pending_index] = true;
                    }
                }
                Ok(crate::internal::secure_direct::async_receive::AsyncDirectSecureReceiveOutcome::Fallback(
                    crate::internal::secure_direct::async_receive::AsyncDirectSecureReceiveFallback::NoEstablishedSession,
                )) if content_type == "application/anp-direct-cipher+json" => {
                    if !pending_cipher_message_id.trim().is_empty() {
                        pending_cipher_indices
                            .entry(pending_cipher_message_id)
                            .or_default()
                            .push(index);
                    }
                    continue;
                }
                Ok(crate::internal::secure_direct::async_receive::AsyncDirectSecureReceiveOutcome::Fallback(_)) => {
                    continue;
                }
                Err(err) => {
                    mark_async_direct_failure(&mut message_values[index], &mut async_warnings, err);
                    continue;
                }
            };
        }
        #[cfg(feature = "blocking")]
        {
            let mut fallback_entries = message_values
                .iter()
                .cloned()
                .enumerate()
                .filter(|(index, message)| {
                    !processed_async[*index] && is_direct_e2ee_wire_message(message)
                })
                .collect::<Vec<_>>();
            let mut fallback_messages = fallback_entries
                .iter()
                .map(|(_, message)| message.clone())
                .collect::<Vec<_>>();
            let warnings = if fallback_messages.is_empty() {
                Vec::new()
            } else {
                crate::internal::secure_direct::incoming::maybe_decrypt_direct_e2ee_messages_for_client(
                    client,
                    &mut fallback_messages,
                    &mut crate::internal::transport::CoreHttpTransport::new(client),
                    crate::internal::secure_direct::incoming::DirectDecryptMode::ReadOnly,
                )
            };
            async_warnings.extend(warnings);
            for ((index, _), message) in fallback_entries.drain(..).zip(fallback_messages) {
                message_values[index] = message;
            }
        }
        #[cfg(not(feature = "blocking"))]
        {
            let _ = client;
            for (index, message) in message_values.iter_mut().enumerate() {
                if !processed_async[index] && is_direct_e2ee_wire_message(message) {
                    mark_async_direct_failure(
                        message,
                        &mut async_warnings,
                        crate::ImError::unsupported("sync-direct-e2ee-read-fallback"),
                    );
                }
            }
        }
        let filtered =
            crate::internal::secure_direct::incoming::filter_displayable_direct_e2ee_messages(
                message_values,
            );
        *messages = filtered;
        append_secure_direct_warnings(raw, compact_secure_direct_warnings(async_warnings));
    }
}

#[cfg(feature = "sqlite")]
pub(crate) async fn project_secure_direct_messages_for_attachment_download_async(
    client: &crate::core::ImClient,
    raw: &mut Value,
    directory_transport: &mut impl AsyncRpcTransport,
) {
    project_secure_direct_messages_async(client, raw, directory_transport).await;
}

async fn resolve_direct_sender_document_async(
    client: &crate::core::ImClient,
    directory_transport: &mut impl AsyncRpcTransport,
    did: &str,
) -> crate::ImResult<Value> {
    if did == client.did().as_str() {
        return read_json_file_async(client.runtime().did_document_path.clone(), "did_document")
            .await;
    }
    let call = crate::internal::identity_wire::profile::build_profile_resolve_rpc_call(did)?;
    match directory_transport
        .rpc(call.endpoint, call.method, call.params)
        .await
        .and_then(|raw| {
            did_document_from_resolve(raw).ok_or_else(|| crate::ImError::PeerNotFound {
                peer: did.to_owned(),
            })
        }) {
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
        let candidate = value.pointer(pointer)?;
        if looks_like_did_document(candidate) {
            return Some(candidate.clone());
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

fn mark_async_direct_failure(message: &mut Value, warnings: &mut Vec<String>, err: crate::ImError) {
    crate::internal::secure_direct::incoming::apply_direct_e2ee_processing_result(
        message,
        &json_object([("state", Value::String("failed".to_owned()))]),
    );
    if !is_secure_direct_control_like_message(message) {
        warnings.push(format!(
            "Failed to decrypt secure direct message {}: {err}",
            secure_direct_message_id(message)
        ));
    }
}

fn json_object<const N: usize>(entries: [(&str, Value); N]) -> Value {
    Value::Object(
        entries
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value))
            .collect(),
    )
}

fn is_direct_e2ee_wire_message(message: &Value) -> bool {
    anp::direct_e2ee::is_direct_e2ee_wire_content_type(&direct_message_content_type(message))
}

fn direct_message_content_type(message: &Value) -> String {
    message
        .as_object()
        .and_then(|object| object.get("content_type"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

fn compare_secure_direct_message_order(left: &Value, right: &Value) -> std::cmp::Ordering {
    let left_seq = secure_direct_message_server_seq(left).unwrap_or_default();
    let right_seq = secure_direct_message_server_seq(right).unwrap_or_default();
    if left_seq == right_seq {
        return secure_direct_message_id(left).cmp(&secure_direct_message_id(right));
    }
    if left_seq == 0 {
        return std::cmp::Ordering::Greater;
    }
    if right_seq == 0 {
        return std::cmp::Ordering::Less;
    }
    left_seq.cmp(&right_seq)
}

fn secure_direct_message_server_seq(message: &Value) -> Option<i64> {
    match message.as_object()?.get("server_seq")? {
        Value::Number(number) => number
            .as_i64()
            .or_else(|| number.as_u64().and_then(|value| i64::try_from(value).ok()))
            .or_else(|| number.as_f64().map(|value| value as i64)),
        Value::String(value) => value.trim().parse::<i64>().ok(),
        _ => None,
    }
}

fn secure_direct_message_id(message: &Value) -> String {
    message
        .as_object()
        .and_then(|object| object.get("id").or_else(|| object.get("msg_id")))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

fn direct_notification_message_id(notification: &Map<String, Value>) -> Option<String> {
    notification
        .get("meta")
        .and_then(Value::as_object)
        .and_then(|meta| meta.get("message_id"))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn is_secure_direct_control_like_message(message: &Value) -> bool {
    let content_type = direct_message_content_type(message);
    if content_type == "application/anp-direct-init+json" {
        return true;
    }
    let id = secure_direct_message_id(message);
    id.starts_with("secure-init-") || id.starts_with("ack-")
}

fn compact_secure_direct_warnings(warnings: Vec<String>) -> Vec<String> {
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

fn append_secure_direct_warnings(raw: &mut Value, warnings: Vec<String>) {
    if warnings.is_empty() {
        return;
    }
    let Some(object) = raw.as_object_mut() else {
        return;
    };
    let entry = object
        .entry("warnings")
        .or_insert_with(|| Value::Array(Vec::new()));
    if let Value::Array(items) = entry {
        items.extend(warnings.into_iter().map(Value::String));
    }
}

#[cfg(feature = "group-e2ee")]
fn project_group_e2ee_messages(client: &crate::core::ImClient, raw: &mut Value) {
    let Some(messages) = raw.get_mut("messages").and_then(Value::as_array_mut) else {
        return;
    };
    let mut message_values = std::mem::take(messages);
    let warnings =
        crate::internal::group_e2ee::incoming::maybe_decrypt_group_e2ee_messages_for_client(
            client,
            &mut message_values,
        );
    *messages = message_values;
    append_secure_direct_warnings(raw, warnings);
}

#[cfg(not(feature = "group-e2ee"))]
fn project_group_e2ee_messages(_client: &crate::core::ImClient, _raw: &mut Value) {}

pub(crate) fn project_group_e2ee_messages_for_attachment_download(
    client: &crate::core::ImClient,
    raw: &mut Value,
) {
    project_group_e2ee_messages(client, raw);
}

#[cfg(feature = "group-e2ee")]
async fn project_group_e2ee_messages_async(client: &crate::core::ImClient, raw: &mut Value) {
    let Some(messages) = raw.get_mut("messages").and_then(Value::as_array_mut) else {
        return;
    };
    let mut message_values = std::mem::take(messages);
    let warnings =
        crate::internal::group_e2ee::incoming::maybe_decrypt_group_e2ee_messages_for_client_async(
            client,
            &mut message_values,
        )
        .await;
    *messages = message_values;
    append_secure_direct_warnings(raw, warnings);
}

#[cfg(not(feature = "group-e2ee"))]
async fn project_group_e2ee_messages_async(_client: &crate::core::ImClient, _raw: &mut Value) {}

pub(crate) async fn project_group_e2ee_messages_for_attachment_download_async(
    client: &crate::core::ImClient,
    raw: &mut Value,
) {
    project_group_e2ee_messages_async(client, raw).await;
}

fn message_from_value(
    value: &Value,
    fallback_group: Option<&crate::ids::GroupRef>,
) -> crate::ImResult<Option<crate::messages::Message>> {
    let Some(object) = value.as_object() else {
        return Ok(None);
    };
    let sender_did = string_value(object.get("sender_did"));
    let receiver_did = string_value(object.get("receiver_did"));
    let mut group_did = string_value(object.get("group_did"));
    if group_did.trim().is_empty() {
        if let Some(group) = fallback_group {
            group_did = group.as_str().to_string();
        }
    }
    let id = message_identity(
        value,
        (!group_did.trim().is_empty()).then_some(group_did.as_str()),
    );
    if id.trim().is_empty() {
        return Ok(None);
    }
    let retry_target = if group_did.trim().is_empty() {
        Some(crate::internal::message_runtime::state::MessageRetryTarget::DirectText)
    } else {
        Some(crate::internal::message_runtime::state::MessageRetryTarget::GroupText)
    };
    let metadata = message_metadata_from_object(object, &id, retry_target);
    let thread = if !group_did.trim().is_empty() {
        crate::messages::ThreadRef::Group(crate::ids::GroupRef::parse(&group_did)?)
    } else {
        let peer = if !receiver_did.trim().is_empty() {
            receiver_did.as_str()
        } else {
            sender_did.as_str()
        };
        crate::messages::ThreadRef::Direct(crate::ids::PeerRef::parse(peer, "")?)
    };
    Ok(Some(crate::messages::Message {
        id: crate::ids::MessageId::parse(id)?,
        thread,
        direction: message_direction(value),
        sender: crate::ids::PeerRef::parse(non_empty_or(&sender_did, "did:unknown:sender"), "")?,
        receiver: (!receiver_did.trim().is_empty())
            .then(|| crate::ids::PeerRef::parse(&receiver_did, ""))
            .transpose()?,
        group: (!group_did.trim().is_empty())
            .then(|| crate::ids::GroupRef::parse(&group_did))
            .transpose()?,
        body: message_body(value),
        sent_at: message_sent_at(object),
        received_at: Some(string_value(object.get("received_at")))
            .filter(|value| !value.trim().is_empty()),
        metadata,
    }))
}

fn message_metadata_from_object(
    object: &serde_json::Map<String, Value>,
    message_id: &str,
    retry_target: Option<crate::internal::message_runtime::state::MessageRetryTarget>,
) -> crate::messages::MessageMetadata {
    let metadata_json = metadata_projection_json(object, message_id);
    let send_state = crate::internal::message_runtime::state::send_state_from_metadata(
        &metadata_json,
        message_id,
    );
    let retry_plan = crate::internal::message_runtime::state::retry_plan_from_metadata(
        &metadata_json,
        send_state.as_ref(),
        retry_target,
    );
    let content_type =
        Some(string_value(object.get("content_type"))).filter(|value| !value.trim().is_empty());
    let mut attributes =
        metadata_attributes_from_object(object, message_id, content_type.as_deref());
    attributes.extend(secure_message_attributes(object));
    crate::messages::MessageMetadata {
        operation_id: Some(string_value(object.get("operation_id")))
            .filter(|value| !value.trim().is_empty()),
        delivery_state: Some(string_value(object.get("delivery_state")))
            .filter(|value| !value.trim().is_empty()),
        send_state,
        retry_plan,
        server_sequence: i64_value(object.get("server_seq"))
            .or_else(|| i64_value(object.get("sequence")))
            .or_else(|| i64_value(object.get("group_event_seq"))),
        content_type: content_type.clone(),
        attributes,
    }
}

fn metadata_projection_json(object: &serde_json::Map<String, Value>, message_id: &str) -> String {
    let mut metadata = serde_json::Map::new();
    metadata.insert(
        "message_id".to_string(),
        Value::String(message_id.to_string()),
    );
    for key in [
        "operation_id",
        "delivery_state",
        "failure_reason",
        "send_state_updated_at",
        "accepted_at",
        "send_state",
        "retry_plan",
    ] {
        if let Some(value) = object.get(key) {
            metadata.insert(key.to_string(), value.clone());
        }
    }
    Value::Object(metadata).to_string()
}

fn message_sent_at(object: &serde_json::Map<String, Value>) -> Option<String> {
    [
        object.get("sent_at"),
        object.get("accepted_at"),
        object.get("created_at"),
    ]
    .into_iter()
    .map(string_or_number_value)
    .find(|value| !value.trim().is_empty())
}

fn metadata_attributes_from_object(
    object: &serde_json::Map<String, Value>,
    message_id: &str,
    content_type: Option<&str>,
) -> Vec<crate::messages::MessageMetadataAttribute> {
    let mut attributes = raw_content_attributes(object.get("content"), content_type);
    if let Some(is_read) = bool_value(object.get("is_read")) {
        attributes.push(crate::messages::MessageMetadataAttribute {
            key: "is_read".to_string(),
            value: is_read.to_string(),
        });
    }
    let raw_message_id = raw_message_identity(object);
    if !raw_message_id.trim().is_empty() && raw_message_id != message_id {
        attributes.push(crate::messages::MessageMetadataAttribute {
            key: "raw_message_id".to_string(),
            value: raw_message_id,
        });
    }
    let group_event_seq = string_or_number_value(object.get("group_event_seq"));
    if !group_event_seq.trim().is_empty() {
        attributes.push(crate::messages::MessageMetadataAttribute {
            key: "group_event_seq".to_string(),
            value: group_event_seq,
        });
    }
    attributes
}

fn message_identity(message: &Value, group_did: Option<&str>) -> String {
    let Some(object) = message.as_object() else {
        return String::new();
    };
    let group_event_seq = string_or_number_value(object.get("group_event_seq"));
    if let Some(group_did) = group_did.filter(|_| !group_event_seq.trim().is_empty()) {
        if !group_did.trim().is_empty() {
            return format!("{}:{}", group_did.trim(), group_event_seq.trim());
        }
    }
    raw_message_identity(object)
}

fn raw_message_identity(object: &serde_json::Map<String, Value>) -> String {
    string_or_number_value(
        object
            .get("id")
            .or_else(|| object.get("message_id"))
            .or_else(|| object.get("msg_id"))
            .or_else(|| object.get("client_msg_id")),
    )
}

fn message_direction(value: &Value) -> crate::messages::MessageDirection {
    let direction = value.get("direction").and_then(Value::as_i64).or_else(|| {
        value
            .get("direction")
            .and_then(Value::as_str)
            .and_then(|value| value.parse().ok())
    });
    match direction {
        Some(1) => crate::messages::MessageDirection::Outgoing,
        Some(0) => crate::messages::MessageDirection::Incoming,
        _ => crate::messages::MessageDirection::Unknown,
    }
}

fn message_body(value: &Value) -> crate::messages::MessageBodyView {
    let content_type = value
        .get("content_type")
        .and_then(Value::as_str)
        .map(str::to_string);
    let Some(content) = value.get("content") else {
        return crate::messages::MessageBodyView::Unsupported { content_type };
    };
    if content_type.as_deref() == Some("application/json") {
        let payload = match content {
            Value::String(value) => match serde_json::from_str::<Value>(value) {
                Ok(value) => value,
                Err(_) => {
                    return crate::messages::MessageBodyView::Unsupported { content_type };
                }
            },
            value => value.clone(),
        };
        if payload.is_object() {
            return crate::messages::MessageBodyView::Payload { payload };
        }
        return crate::messages::MessageBodyView::Unsupported { content_type };
    }
    let text = match content {
        Value::String(value) => value.clone(),
        value => serde_json::to_string(value).unwrap_or_default(),
    };
    let kind = match content_type.as_deref() {
        Some("text/markdown") => crate::messages::MessageKind::Markdown,
        Some("text/plain") | None | Some("") => crate::messages::MessageKind::Text,
        _ => return crate::messages::MessageBodyView::Unsupported { content_type },
    };
    crate::messages::MessageBodyView::Text { text, kind }
}

fn raw_content_attributes(
    content: Option<&Value>,
    content_type: Option<&str>,
) -> Vec<crate::messages::MessageMetadataAttribute> {
    let Some(content) = content else {
        return Vec::new();
    };
    let Some(content_type) = content_type
        .map(str::trim)
        .filter(|content_type| !content_type.is_empty())
    else {
        return Vec::new();
    };
    if content_type != crate::attachments::manifest::attachment_manifest_content_type() {
        return Vec::new();
    }
    if content.is_null() {
        return Vec::new();
    }
    let value = match content {
        Value::String(text) => text.clone(),
        value => serde_json::to_string(value).unwrap_or_default(),
    };
    if value.trim().is_empty() {
        return Vec::new();
    }
    vec![crate::messages::MessageMetadataAttribute {
        key: "raw_content".to_string(),
        value,
    }]
}

fn secure_message_attributes(
    object: &serde_json::Map<String, Value>,
) -> Vec<crate::messages::MessageMetadataAttribute> {
    if !object
        .get("secure")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return Vec::new();
    }
    let mut attributes = vec![crate::messages::MessageMetadataAttribute {
        key: "security".to_owned(),
        value: "direct-e2ee".to_owned(),
    }];
    for key in ["decryption_state", "secure_wire_content_type"] {
        let value = string_value(object.get(key));
        if !value.trim().is_empty() {
            attributes.push(crate::messages::MessageMetadataAttribute {
                key: key.to_owned(),
                value,
            });
        }
    }
    attributes
}

fn string_value(value: Option<&Value>) -> String {
    value
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn bool_value(value: Option<&Value>) -> Option<bool> {
    match value {
        Some(Value::Bool(value)) => Some(*value),
        Some(Value::Number(value)) => value.as_i64().map(|value| value != 0),
        Some(Value::String(value)) => match value.trim() {
            "true" | "1" => Some(true),
            "false" | "0" => Some(false),
            _ => None,
        },
        _ => None,
    }
}

fn string_or_number_value(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(value)) => value.clone(),
        Some(Value::Number(value)) => value.to_string(),
        _ => String::new(),
    }
}

fn i64_value(value: Option<&Value>) -> Option<i64> {
    match value {
        Some(Value::Number(number)) => number
            .as_i64()
            .or_else(|| number.as_u64().and_then(|value| i64::try_from(value).ok()))
            .or_else(|| number.as_f64().map(|value| value as i64)),
        Some(Value::String(value)) => value.trim().parse::<i64>().ok(),
        _ => None,
    }
}

fn non_empty_or<'a>(value: &'a str, fallback: &'a str) -> &'a str {
    if value.trim().is_empty() {
        fallback
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::internal::auth::session::SessionProvider;
    use crate::internal::transport::{
        AsyncAuthenticatedRpcTransport, AsyncRpcTransport, AuthenticatedRpcTransport, RpcTransport,
    };
    use serde_json::{json, Value};
    use std::cell::RefCell;
    use std::fs;
    use std::path::PathBuf;
    use std::rc::Rc;

    #[test]
    fn messages_read_runtime_builds_inbox_rpc_and_maps_page() {
        let fixture = Fixture::new();
        let client = fixture.client();
        let calls = Rc::new(RefCell::new(Vec::new()));
        let runtime = MessageReadRuntime::new(
            &client,
            ReadySessionProvider,
            RecordingTransport {
                calls: Rc::clone(&calls),
                response: json!({
                    "messages": [{
                        "id": "msg-inbox-1",
                        "sender_did": "did:example:bob",
                        "receiver_did": "did:example:alice",
                        "content": "hello alice",
                        "content_type": "text/plain",
                        "sent_at": "2026-05-21T00:00:00Z",
                        "server_seq": 7
                    }],
                    "has_more": false
                }),
            },
            NoopDirectoryTransport,
        );

        let result = runtime
            .inbox(InboxRead {
                query: crate::messages::InboxQuery {
                    scope: crate::messages::InboxScope::DirectOnly,
                    limit: crate::ids::PageLimit(20),
                    cursor: None,
                    unread_only: false,
                },
            })
            .unwrap();

        assert_eq!(result.page.items.len(), 1);
        assert_eq!(result.page.items[0].id.as_str(), "msg-inbox-1");
        assert_eq!(result.page.items[0].metadata.server_sequence, Some(7));
        let calls = calls.borrow();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].endpoint, MESSAGE_RPC_ENDPOINT);
        assert_eq!(calls[0].method, "inbox.get");
        assert_eq!(calls[0].params["meta"]["sender_did"], "did:example:alice");
        assert_eq!(calls[0].params["body"]["user_did"], "did:example:alice");
        assert_eq!(calls[0].params["body"]["limit"], 20);
    }

    #[test]
    fn messages_read_runtime_persists_inbox_projection_for_conversations() {
        let fixture = Fixture::new();
        let client = fixture.client();
        let runtime = MessageReadRuntime::new(
            &client,
            ReadySessionProvider,
            RecordingTransport {
                calls: Rc::new(RefCell::new(Vec::new())),
                response: json!({
                    "messages": [{
                        "id": "msg-inbox-projected",
                        "sender_did": "did:example:bob",
                        "receiver_did": "did:example:alice",
                        "content": "restored from inbox",
                        "content_type": "text/plain",
                        "sent_at": "2026-05-21T00:00:00Z"
                    }]
                }),
            },
            NoopDirectoryTransport,
        );

        runtime
            .inbox(InboxRead {
                query: crate::messages::InboxQuery {
                    scope: crate::messages::InboxScope::All,
                    limit: crate::ids::PageLimit(20),
                    cursor: None,
                    unread_only: false,
                },
            })
            .unwrap();

        let conversations =
            crate::internal::message_runtime::conversations::MessageConversationRuntime::new(
                &client,
            )
            .conversations(crate::messages::ConversationQuery {
                limit: crate::ids::PageLimit(10),
                include_groups: true,
                include_direct: true,
                unread_only: false,
            })
            .unwrap();

        assert_eq!(conversations.items.len(), 1);
        let conversation = &conversations.items[0];
        assert_eq!(
            conversation.last_message.as_ref().unwrap().id.as_str(),
            "msg-inbox-projected"
        );
        assert_eq!(
            conversation.last_message_at.as_deref(),
            Some("2026-05-21T00:00:00Z")
        );
        assert_eq!(conversation.unread_count, 1);
        assert!(matches!(
            conversation.thread,
            crate::messages::ThreadRef::Direct(_)
        ));
    }

    #[test]
    fn messages_read_runtime_preserves_remote_read_state_in_projection() {
        let fixture = Fixture::new();
        let client = fixture.client();
        let runtime = MessageReadRuntime::new(
            &client,
            ReadySessionProvider,
            RecordingTransport {
                calls: Rc::new(RefCell::new(Vec::new())),
                response: json!({
                    "messages": [{
                        "id": "msg-inbox-read",
                        "sender_did": "did:example:bob",
                        "receiver_did": "did:example:alice",
                        "content": "already read",
                        "content_type": "text/plain",
                        "sent_at": "2026-05-21T00:00:01Z",
                        "is_read": true
                    }]
                }),
            },
            NoopDirectoryTransport,
        );

        runtime
            .inbox(InboxRead {
                query: crate::messages::InboxQuery {
                    scope: crate::messages::InboxScope::All,
                    limit: crate::ids::PageLimit(20),
                    cursor: None,
                    unread_only: false,
                },
            })
            .unwrap();

        let conversations =
            crate::internal::message_runtime::conversations::MessageConversationRuntime::new(
                &client,
            )
            .conversations(crate::messages::ConversationQuery {
                limit: crate::ids::PageLimit(10),
                include_groups: true,
                include_direct: true,
                unread_only: false,
            })
            .unwrap();

        assert_eq!(conversations.items.len(), 1);
        assert_eq!(conversations.items[0].unread_count, 0);
    }

    #[test]
    fn message_state_read_projection_maps_failed_retry_plan() {
        let fixture = Fixture::new();
        let client = fixture.client();
        let runtime = MessageReadRuntime::new(
            &client,
            ReadySessionProvider,
            RecordingTransport {
                calls: Rc::new(RefCell::new(Vec::new())),
                response: json!({
                    "messages": [{
                        "id": "msg-read-failed",
                        "sender_did": "did:example:alice",
                        "receiver_did": "did:example:bob",
                        "content": "hello bob",
                        "content_type": "text/plain",
                        "operation_id": "op-read-failed",
                        "delivery_state": "failed",
                        "failure_reason": "timeout"
                    }]
                }),
            },
            NoopDirectoryTransport,
        );

        let result = runtime
            .history(HistoryRead {
                thread: crate::messages::ThreadRef::Direct(
                    crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
                ),
                query: crate::messages::HistoryQuery {
                    limit: crate::ids::PageLimit(5),
                    cursor: None,
                },
                resolved_peer_did: None,
            })
            .unwrap();

        let metadata = &result.page.items[0].metadata;
        let send_state = metadata.send_state.as_ref().unwrap();
        assert_eq!(
            send_state.state,
            crate::messages::MessageSendStateKind::Failed
        );
        assert_eq!(send_state.reason.as_deref(), Some("timeout"));
        let retry_plan = metadata.retry_plan.as_ref().unwrap();
        assert!(retry_plan.retryable);
        assert_eq!(
            retry_plan.action,
            crate::messages::MessageRetryAction::RetryDirectText
        );
        assert_eq!(retry_plan.operation_id.as_deref(), Some("op-read-failed"));
    }

    #[test]
    fn messages_read_runtime_builds_direct_history_rpc() {
        let fixture = Fixture::new();
        let client = fixture.client();
        let calls = Rc::new(RefCell::new(Vec::new()));
        let runtime = MessageReadRuntime::new(
            &client,
            ReadySessionProvider,
            RecordingTransport {
                calls: Rc::clone(&calls),
                response: json!({
                    "messages": [{
                        "id": "msg-history-1",
                        "sender_did": "did:example:alice",
                        "receiver_did": "did:example:bob",
                        "content": "hello bob",
                        "content_type": "text/plain"
                    }]
                }),
            },
            NoopDirectoryTransport,
        );

        let result = runtime
            .history(HistoryRead {
                thread: crate::messages::ThreadRef::Direct(
                    crate::ids::PeerRef::parse("bob.awiki.test", "").unwrap(),
                ),
                query: crate::messages::HistoryQuery {
                    limit: crate::ids::PageLimit(5),
                    cursor: Some(crate::ids::Cursor::parse("42").unwrap()),
                },
                resolved_peer_did: Some("did:example:bob".to_string()),
            })
            .unwrap();

        assert_eq!(result.page.items.len(), 1);
        let calls = calls.borrow();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].method, "direct.get_history");
        assert_eq!(calls[0].params["body"]["peer_did"], "did:example:bob");
        assert_eq!(calls[0].params["body"]["limit"], 5);
        assert_eq!(calls[0].params["body"]["since_seq"], "42");
    }

    #[test]
    fn messages_read_runtime_uses_remote_created_at_as_sent_at() {
        let fixture = Fixture::new();
        let client = fixture.client();
        let runtime = MessageReadRuntime::new(
            &client,
            ReadySessionProvider,
            RecordingTransport {
                calls: Rc::new(RefCell::new(Vec::new())),
                response: json!({
                    "messages": [{
                        "id": "msg-history-created-at",
                        "sender_did": "did:example:bob",
                        "receiver_did": "did:example:alice",
                        "content": "created timestamp",
                        "content_type": "text/plain",
                        "created_at": "2026-05-21T03:04:05Z"
                    }]
                }),
            },
            NoopDirectoryTransport,
        );

        let result = runtime
            .history(HistoryRead {
                thread: crate::messages::ThreadRef::Direct(
                    crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
                ),
                query: crate::messages::HistoryQuery {
                    limit: crate::ids::PageLimit(5),
                    cursor: None,
                },
                resolved_peer_did: None,
            })
            .unwrap();

        assert_eq!(
            result.page.items[0].sent_at.as_deref(),
            Some("2026-05-21T03:04:05Z")
        );
    }

    #[test]
    fn messages_read_runtime_maps_application_json_content_to_payload_body() {
        let fixture = Fixture::new();
        let client = fixture.client();
        let runtime = MessageReadRuntime::new(
            &client,
            ReadySessionProvider,
            RecordingTransport {
                calls: Rc::new(RefCell::new(Vec::new())),
                response: json!({
                    "messages": [{
                        "id": "msg-history-payload",
                        "sender_did": "did:example:bob",
                        "receiver_did": "did:example:alice",
                        "content": {
                            "schema": "awiki.agent.command.v1",
                            "command": "runtime.agent.create"
                        },
                        "content_type": "application/json",
                        "created_at": "2026-05-21T03:04:05Z"
                    }]
                }),
            },
            NoopDirectoryTransport,
        );

        let result = runtime
            .history(HistoryRead {
                thread: crate::messages::ThreadRef::Direct(
                    crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
                ),
                query: crate::messages::HistoryQuery {
                    limit: crate::ids::PageLimit(5),
                    cursor: None,
                },
                resolved_peer_did: None,
            })
            .unwrap();

        assert_eq!(
            result.page.items[0].body,
            crate::messages::MessageBodyView::Payload {
                payload: json!({
                    "schema": "awiki.agent.command.v1",
                    "command": "runtime.agent.create"
                })
            }
        );
        assert_eq!(
            result.page.items[0].metadata.content_type.as_deref(),
            Some("application/json")
        );
    }

    #[test]
    fn messages_read_runtime_builds_group_history_rpc() {
        let fixture = Fixture::new();
        let client = fixture.client();
        let calls = Rc::new(RefCell::new(Vec::new()));
        let group = crate::ids::GroupRef::parse("did:example:group").unwrap();
        let runtime = MessageReadRuntime::new(
            &client,
            ReadyGroupSessionProvider,
            RecordingTransport {
                calls: Rc::clone(&calls),
                response: json!({
                    "messages": [{
                        "id": "msg-group-history-1",
                        "sender_did": "did:example:bob",
                        "content": "hello group",
                        "content_type": "text/plain",
                        "group_event_seq": 9
                    }],
                    "has_more": false
                }),
            },
            NoopDirectoryTransport,
        );

        let result = runtime
            .history(HistoryRead {
                thread: crate::messages::ThreadRef::Group(group.clone()),
                query: crate::messages::HistoryQuery {
                    limit: crate::ids::PageLimit(5),
                    cursor: Some(crate::ids::Cursor::parse("42").unwrap()),
                },
                resolved_peer_did: None,
            })
            .unwrap();

        assert_eq!(result.page.items.len(), 1);
        let message = &result.page.items[0];
        assert_eq!(message.id.as_str(), "did:example:group:9");
        assert_eq!(message.group.as_ref(), Some(&group));
        assert_eq!(
            message.thread,
            crate::messages::ThreadRef::Group(group.clone())
        );
        assert_eq!(message.metadata.server_sequence, Some(9));
        assert!(message.metadata.attributes.iter().any(|attribute| {
            attribute.key == "raw_message_id" && attribute.value == "msg-group-history-1"
        }));
        assert!(message
            .metadata
            .attributes
            .iter()
            .any(|attribute| { attribute.key == "group_event_seq" && attribute.value == "9" }));
        let calls = calls.borrow();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].endpoint, MESSAGE_RPC_ENDPOINT);
        assert_eq!(calls[0].method, "group.list_messages");
        assert_eq!(calls[0].params["meta"]["sender_did"], "did:example:alice");
        assert_eq!(
            calls[0].params["meta"]["target"],
            json!({"kind": "group", "did": "did:example:group"})
        );
        assert_eq!(calls[0].params["body"]["group_did"], "did:example:group");
        assert_eq!(calls[0].params["body"]["limit"], 5);
        assert_eq!(calls[0].params["body"]["since_seq"], "42");
    }

    #[tokio::test]
    async fn messages_read_runtime_builds_group_history_rpc_async() {
        let fixture = Fixture::new();
        let client = fixture.client();
        let calls = Rc::new(RefCell::new(Vec::new()));
        let group = crate::ids::GroupRef::parse("did:example:group").unwrap();
        let runtime = MessageReadRuntime::new(
            &client,
            ReadyGroupSessionProvider,
            RecordingTransport {
                calls: Rc::clone(&calls),
                response: json!({
                    "messages": [{
                        "id": "msg-group-history-async-1",
                        "sender_did": "did:example:bob",
                        "content": "hello group async",
                        "content_type": "text/plain",
                        "group_event_seq": 10
                    }],
                    "has_more": false
                }),
            },
            NoopDirectoryTransport,
        );

        let result = runtime
            .history_async(HistoryRead {
                thread: crate::messages::ThreadRef::Group(group.clone()),
                query: crate::messages::HistoryQuery {
                    limit: crate::ids::PageLimit(6),
                    cursor: Some(crate::ids::Cursor::parse("43").unwrap()),
                },
                resolved_peer_did: None,
            })
            .await
            .unwrap();

        assert_eq!(result.page.items.len(), 1);
        let message = &result.page.items[0];
        assert_eq!(message.id.as_str(), "did:example:group:10");
        assert_eq!(message.group.as_ref(), Some(&group));
        assert_eq!(message.metadata.server_sequence, Some(10));
        let calls = calls.borrow();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].endpoint, MESSAGE_RPC_ENDPOINT);
        assert_eq!(calls[0].method, "group.list_messages");
        assert_eq!(calls[0].params["body"]["group_did"], "did:example:group");
        assert_eq!(calls[0].params["body"]["limit"], 6);
        assert_eq!(calls[0].params["body"]["since_seq"], "43");
    }

    #[test]
    fn direct_e2ee_projection_helper_returns_plaintext_and_filters_controls() {
        let messages = vec![
            json!({
                "id": "msg-secure",
                "sender_did": "did:example:bob",
                "receiver_did": "did:example:alice",
                "content_type": "application/anp-direct-cipher+json",
                "server_seq": 2,
                "content": {
                    "session_id": "session-1",
                    "ratchet_header": {"dh_pub_b64u": "dh", "pn": "0", "n": "1"},
                    "ciphertext_b64u": "CIPHER"
                }
            }),
            json!({
                "id": "ack-session-1",
                "sender_did": "did:example:bob",
                "receiver_did": "did:example:alice",
                "content_type": "application/anp-direct-cipher+json",
                "server_seq": 3,
                "content": {
                    "session_id": "session-1",
                    "ratchet_header": {"dh_pub_b64u": "dh", "pn": "0", "n": "2"},
                    "ciphertext_b64u": "ACK-CIPHER"
                }
            }),
        ];

        let (projected, warnings) =
            crate::internal::secure_direct::incoming::project_direct_e2ee_message_values_with_processor(
                messages,
                |notification| {
                    let message_id = notification
                        .get("meta")
                        .and_then(Value::as_object)
                        .and_then(|meta| meta.get("message_id"))
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    let plaintext = if message_id.starts_with("ack-") {
                        json!({
                            "application_content_type": "application/json",
                            "payload": {
                                "system_type": crate::internal::secure_direct::control::SECURE_ACK_SYSTEM_TYPE,
                                "session_id": "session-1",
                                "acked_message_id": "msg-secure"
                            }
                        })
                    } else {
                        json!({
                            "application_content_type": "text/plain",
                            "text": "decrypted direct text"
                        })
                    };
                    Ok(serde_json::Map::from_iter([
                        ("state".to_owned(), json!("decrypted")),
                        ("plaintext".to_owned(), plaintext),
                    ]))
                },
            );

        assert!(warnings.is_empty());
        assert_eq!(projected.len(), 1);
        let page = page_from_raw(
            &json!({
                "messages": projected,
                "has_more": false
            }),
            crate::ids::PageLimit(20),
        )
        .unwrap();
        assert_eq!(page.items.len(), 1);
        assert_eq!(
            page.items[0].body,
            crate::messages::MessageBodyView::Text {
                text: "decrypted direct text".to_owned(),
                kind: crate::messages::MessageKind::Text,
            }
        );
        assert!(page.items[0]
            .metadata
            .attributes
            .iter()
            .any(|attribute| attribute.key == "security" && attribute.value == "direct-e2ee"));
        assert!(!serde_json::to_string(&page).unwrap().contains("CIPHER"));
    }

    #[test]
    fn direct_e2ee_projection_helper_redacts_failed_ciphertext() {
        let messages = vec![json!({
            "id": "msg-secure-failed",
            "sender_did": "did:example:bob",
            "receiver_did": "did:example:alice",
            "content_type": "application/anp-direct-cipher+json",
            "server_seq": 1,
            "content": {
                "session_id": "session-1",
                "ratchet_header": {"dh_pub_b64u": "dh", "pn": "0", "n": "1"},
                "ciphertext_b64u": "FAILED-CIPHER"
            }
        })];

        let (projected, warnings) =
            crate::internal::secure_direct::incoming::project_direct_e2ee_message_values_with_processor(
                messages,
                |_notification| {
                    Err(crate::ImError::Serialization {
                        detail: "decrypt failed".to_owned(),
                    })
                },
            );

        assert_eq!(warnings.len(), 1);
        assert_eq!(projected.len(), 1);
        assert_eq!(projected[0]["content"], Value::Null);
        let page = page_from_raw(
            &json!({
                "messages": projected,
                "has_more": false
            }),
            crate::ids::PageLimit(20),
        )
        .unwrap();
        assert_eq!(page.items.len(), 1);
        assert!(matches!(
            page.items[0].body,
            crate::messages::MessageBodyView::Unsupported { .. }
        ));
        assert!(!serde_json::to_string(&page)
            .unwrap()
            .contains("FAILED-CIPHER"));
    }

    #[test]
    fn inbox_projection_preserves_attachment_manifest_content() {
        let fixture = Fixture::new();
        let client = fixture.client();
        let runtime = MessageReadRuntime::new(
            &client,
            ReadySessionProvider,
            RecordingTransport {
                calls: Rc::new(RefCell::new(Vec::new())),
                response: json!({
                    "messages": [{
                        "id": "msg-attachment-1",
                        "sender_did": "did:example:bob",
                        "receiver_did": "did:example:alice",
                        "content_type": "application/anp-attachment-manifest+json",
                        "content": {
                            "attachments": [{
                                "attachment_id": "att-1",
                                "filename": "report.txt",
                                "mime_type": "text/plain",
                                "size": "12",
                                "digest": {
                                    "alg": "sha-256",
                                    "value_b64u": "digest"
                                },
                                "access_info": {
                                    "object_uri": "https://objects.example/att-1"
                                },
                                "encryption_info": {
                                    "mode": "none"
                                }
                            }],
                            "caption": "direct attachment",
                            "primary_attachment_id": "att-1"
                        },
                        "server_seq": 42
                    }],
                    "has_more": false
                }),
            },
            NoopDirectoryTransport,
        );

        let result = runtime
            .inbox(InboxRead {
                query: crate::messages::InboxQuery {
                    scope: crate::messages::InboxScope::DirectOnly,
                    limit: crate::ids::PageLimit(20),
                    cursor: None,
                    unread_only: true,
                },
            })
            .unwrap();

        let message = &result.page.items[0];
        assert_eq!(
            message.metadata.content_type.as_deref(),
            Some("application/anp-attachment-manifest+json")
        );
        assert!(matches!(
            message.body,
            crate::messages::MessageBodyView::Unsupported { .. }
        ));
        let raw_content = message
            .metadata
            .attributes
            .iter()
            .find(|attribute| attribute.key == "raw_content")
            .expect("raw content attribute");
        let content: Value = serde_json::from_str(&raw_content.value).unwrap();
        assert_eq!(content["attachments"][0]["attachment_id"], "att-1");
        assert_eq!(content["caption"], "direct attachment");
    }

    #[tokio::test]
    async fn messages_read_async_projects_direct_init_without_legacy_fallback() {
        let fixture = Fixture::new();
        let exchange =
            crate::internal::secure_direct::async_receive::test_support::incoming_init_exchange();
        fixture.write_direct_credentials(&exchange);
        fixture.write_peer_document("bob", &exchange.sender_did, &exchange.sender_document);
        fixture.seed_direct_prekeys(&exchange);
        let client = fixture.client();
        let response = json!({
            "messages": [
                {
                    "id": "msg-init-async",
                    "sender_did": exchange.sender_did.clone(),
                    "receiver_did": exchange.recipient_did.clone(),
                    "content_type": "application/anp-direct-init+json",
                    "server_seq": 1,
                    "content": anp::direct_e2ee::direct_init_body_to_value(&exchange.init_body),
                }
            ],
            "has_more": false
        });
        let runtime = MessageReadRuntime::new(
            &client,
            ReadySessionProvider,
            RecordingTransport {
                calls: Rc::new(RefCell::new(Vec::new())),
                response,
            },
            NoopDirectoryTransport,
        );

        let result = runtime
            .inbox_async(InboxRead {
                query: crate::messages::InboxQuery {
                    scope: crate::messages::InboxScope::All,
                    limit: crate::ids::PageLimit(20),
                    cursor: None,
                    unread_only: false,
                },
            })
            .await
            .unwrap();

        assert_eq!(result.page.items.len(), 1);
        assert_eq!(
            result.page.items[0].body,
            crate::messages::MessageBodyView::Text {
                text: "hello from init".to_owned(),
                kind: crate::messages::MessageKind::Text,
            }
        );
        assert!(result
            .raw
            .get("warnings")
            .and_then(Value::as_array)
            .is_none_or(Vec::is_empty));
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
        let saved_session = crate::internal::secure_direct::sqlite_store::direct_session_from_blob(
            &saved.state_blob,
        )
        .unwrap();
        assert_eq!(saved_session.recv_n, 1);
    }

    #[tokio::test]
    async fn messages_read_async_replays_pending_direct_cipher_after_init() {
        use anp::direct_e2ee::{ApplicationPlaintext, DirectE2eeSession, DirectEnvelopeMetadata};

        let fixture = Fixture::new();
        let exchange =
            crate::internal::secure_direct::async_receive::test_support::incoming_init_exchange();
        fixture.write_direct_credentials(&exchange);
        fixture.write_peer_document("bob", &exchange.sender_did, &exchange.sender_document);
        fixture.seed_direct_prekeys(&exchange);
        let client = fixture.client();
        let mut sender_session = exchange.sender_session.clone();
        sender_session.status = anp::direct_e2ee::models::SESSION_STATUS_ESTABLISHED.to_owned();
        sender_session.recv_chain_key_b64u = sender_session.send_chain_key_b64u.clone();
        sender_session.peer_ratchet_public_key_b64u =
            Some(sender_session.ratchet_public_key_b64u.clone());
        sender_session.send_n = 1;
        let follow_up_metadata = DirectEnvelopeMetadata {
            sender_did: exchange.sender_did.clone(),
            recipient_did: exchange.recipient_did.clone(),
            message_id: "msg-pending-follow-up".to_owned(),
            profile: "anp.direct.e2ee.v1".to_owned(),
            security_profile: "direct-e2ee".to_owned(),
        };
        let (_, follow_up_body) = DirectE2eeSession::encrypt_follow_up(
            &mut sender_session,
            &follow_up_metadata,
            "msg-pending-follow-up",
            &ApplicationPlaintext::new_text("text/plain", "follow-up after init"),
        )
        .unwrap();
        let response = json!({
            "messages": [
                {
                    "id": "msg-pending-follow-up",
                    "sender_did": exchange.sender_did.clone(),
                    "receiver_did": exchange.recipient_did.clone(),
                    "content_type": "application/anp-direct-cipher+json",
                    "server_seq": 1,
                    "content": anp::direct_e2ee::direct_cipher_body_to_value(&follow_up_body),
                },
                {
                    "id": "msg-init-async",
                    "sender_did": exchange.sender_did.clone(),
                    "receiver_did": exchange.recipient_did.clone(),
                    "content_type": "application/anp-direct-init+json",
                    "server_seq": 2,
                    "content": anp::direct_e2ee::direct_init_body_to_value(&exchange.init_body),
                }
            ],
            "has_more": false
        });
        let runtime = MessageReadRuntime::new(
            &client,
            ReadySessionProvider,
            RecordingTransport {
                calls: Rc::new(RefCell::new(Vec::new())),
                response,
            },
            NoopDirectoryTransport,
        );

        let result = runtime
            .inbox_async(InboxRead {
                query: crate::messages::InboxQuery {
                    scope: crate::messages::InboxScope::All,
                    limit: crate::ids::PageLimit(20),
                    cursor: None,
                    unread_only: false,
                },
            })
            .await
            .unwrap();

        assert_eq!(result.page.items.len(), 2);
        assert_eq!(
            result.page.items[0].body,
            crate::messages::MessageBodyView::Text {
                text: "follow-up after init".to_owned(),
                kind: crate::messages::MessageKind::Text,
            }
        );
        assert_eq!(
            result.page.items[1].body,
            crate::messages::MessageBodyView::Text {
                text: "hello from init".to_owned(),
                kind: crate::messages::MessageKind::Text,
            }
        );
        assert!(result
            .raw
            .get("warnings")
            .and_then(Value::as_array)
            .is_none_or(Vec::is_empty));
        let raw_messages = result
            .raw
            .get("messages")
            .and_then(Value::as_array)
            .expect("raw messages");
        assert_eq!(raw_messages[0]["decryption_state"], json!("decrypted"));
        assert_eq!(raw_messages[0]["content"], json!("follow-up after init"));
        let saved = client
            .core_inner()
            .local_state_db()
            .await
            .unwrap()
            .get_direct_secure_session("alice-id", exchange.sender_did)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(saved.revision, 1);
        let saved_session = crate::internal::secure_direct::sqlite_store::direct_session_from_blob(
            &saved.state_blob,
        )
        .unwrap();
        assert_eq!(saved_session.recv_n, 2);
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
            })
        }

        fn refresh_session(&self) -> crate::ImResult<crate::auth::SessionUpdate> {
            unreachable!("read runtime should not refresh through the session provider")
        }

        fn status(&self) -> crate::ImResult<crate::auth::AuthStatus> {
            unreachable!("read runtime should not read status")
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

    #[derive(Clone)]
    struct ReadyGroupSessionProvider;

    impl SessionProvider for ReadyGroupSessionProvider {
        fn ensure_session(
            &self,
            scope: crate::auth::AuthScope,
        ) -> crate::ImResult<crate::auth::SessionBundle> {
            assert_eq!(scope, crate::auth::AuthScope::GroupMessaging);
            Ok(crate::auth::SessionBundle {
                subject: crate::ids::Did::parse("did:example:alice")?,
                scope,
                expires_at: None,
                refreshed: false,
            })
        }

        fn refresh_session(&self) -> crate::ImResult<crate::auth::SessionUpdate> {
            unreachable!("read runtime should not refresh through the session provider")
        }

        fn status(&self) -> crate::ImResult<crate::auth::AuthStatus> {
            unreachable!("read runtime should not read status")
        }
    }

    impl crate::internal::auth::session::AsyncSessionProvider for ReadyGroupSessionProvider {
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
            self.calls.borrow_mut().push(RecordedCall {
                endpoint: endpoint.to_string(),
                method: method.to_string(),
                params,
            });
            Ok(self.response.clone())
        }
    }

    impl AsyncAuthenticatedRpcTransport for RecordingTransport {
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

    struct NoopDirectoryTransport;

    impl RpcTransport for NoopDirectoryTransport {
        fn rpc(
            &mut self,
            _endpoint: &str,
            _method: &str,
            _params: Value,
        ) -> crate::ImResult<Value> {
            Err(crate::ImError::PeerNotFound {
                peer: "noop-directory".to_owned(),
            })
        }
    }

    impl AsyncRpcTransport for NoopDirectoryTransport {
        async fn rpc(
            &mut self,
            endpoint: &str,
            method: &str,
            params: Value,
        ) -> crate::ImResult<Value> {
            RpcTransport::rpc(self, endpoint, method, params)
        }
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

        fn identity_dir(&self) -> PathBuf {
            self.root.join("identities").join("alice")
        }

        fn sqlite_path(&self) -> PathBuf {
            self.root.join("local").join("im.sqlite")
        }

        fn write_direct_credentials(
            &self,
            exchange: &crate::internal::secure_direct::async_receive::test_support::IncomingInitExchange,
        ) {
            let identity_dir = self.identity_dir();
            fs::write(
                identity_dir.join("did.json"),
                exchange.recipient_document.to_string(),
            )
            .unwrap();
            fs::write(identity_dir.join("private.key"), "test-key").unwrap();
            fs::write(
                identity_dir.join("e2ee-agreement-private.pem"),
                exchange.recipient_agreement_private.to_pem(),
            )
            .unwrap();
            fs::write(
                identity_dir.join("auth.json"),
                r#"{"jwt_token":"test-token"}"#,
            )
            .unwrap();
        }

        fn write_peer_document(&self, alias: &str, did: &str, document: &Value) {
            let identities = self.root.join("identities");
            let identity_dir = identities.join(alias);
            fs::create_dir_all(&identity_dir).unwrap();
            fs::write(
                identities.join("registry.json"),
                format!(
                    r#"{{
                      "default_identity": "alice",
                      "identities": [
                        {{
                          "id": "alice-id",
                          "did": "did:example:alice",
                          "local_alias": "alice",
                          "ready_for_auth": true,
                          "ready_for_messaging": true,
                          "missing": []
                        }},
                        {{
                          "id": "{alias}-id",
                          "did": "{did}",
                          "local_alias": "{alias}",
                          "ready_for_auth": true,
                          "ready_for_messaging": true,
                          "missing": []
                        }}
                      ]
                    }}"#
                ),
            )
            .unwrap();
            fs::write(identity_dir.join("did.json"), document.to_string()).unwrap();
        }

        fn seed_direct_prekeys(
            &self,
            exchange: &crate::internal::secure_direct::async_receive::test_support::IncomingInitExchange,
        ) {
            let connection =
                crate::internal::local_state::open_writable(&self.sqlite_path()).unwrap();
            let store =
                crate::internal::secure_direct::sqlite_store::SqliteDirectSecureStateStore::new(
                    &connection,
                )
                .unwrap();
            store
                .upsert_signed_prekey(
                    &crate::internal::secure_direct::sqlite_store::DirectSignedPrekeyRecord {
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
                        status:
                            crate::internal::secure_direct::sqlite_store::DirectPrekeyStatus::Active,
                        metadata_json: serde_json::to_string(&json!({
                            "metadata": exchange.recipient_signed_prekey,
                        }))
                        .unwrap(),
                        created_at: "2026-05-24T00:00:00Z".to_owned(),
                        updated_at: "2026-05-24T00:00:00Z".to_owned(),
                    },
                )
                .unwrap();
            store
                .upsert_one_time_prekey(
                    &crate::internal::secure_direct::sqlite_store::DirectOneTimePrekeyRecord {
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
                        status:
                            crate::internal::secure_direct::sqlite_store::DirectPrekeyStatus::Available,
                        metadata_json: serde_json::to_string(&json!({
                            "metadata": exchange.recipient_one_time_prekey,
                        }))
                        .unwrap(),
                        created_at: "2026-05-24T00:00:00Z".to_owned(),
                        consumed_at: String::new(),
                    },
                )
                .unwrap();
        }
    }

    fn unique_temp_root() -> PathBuf {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let counter = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "im-core-read-runtime-{}-{nanos}-{counter}",
            std::process::id()
        ))
    }
}
