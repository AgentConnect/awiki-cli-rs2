use serde_json::Value;

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
