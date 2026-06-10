//! Migration-only message runtime bridge for `awiki-cli`.

use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub struct DirectTextSendBridgeRequest {
    pub request: crate::messages::SendMessageRequest,
    pub resolved_target_did: String,
    pub credentials: DirectTextCredentials,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DirectTextSendBridgeResult {
    pub sdk_result: crate::messages::SendMessageResult,
    pub target_did: String,
    pub message_type: String,
    pub text: String,
    pub raw: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DirectTextCredentials {
    pub identity_name: String,
    pub did_document: Option<Value>,
    pub key1_private_pem: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GroupTextSendBridgeRequest {
    pub request: crate::messages::SendMessageRequest,
    pub credentials: GroupTextCredentials,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GroupTextSendBridgeResult {
    pub sdk_result: crate::messages::SendMessageResult,
    pub group_did: String,
    pub message_type: String,
    pub text: String,
    pub raw: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GroupTextCredentials {
    pub identity_name: String,
    pub did_document: Option<Value>,
    pub key1_private_pem: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct InboxReadBridgeRequest {
    pub query: crate::messages::InboxQuery,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HistoryReadBridgeRequest {
    pub thread: crate::messages::ThreadRef,
    pub query: crate::messages::HistoryQuery,
    pub resolved_peer_did: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ReadBridgeResult {
    pub page: crate::ids::Page<crate::messages::Message>,
    pub raw: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MarkReadBridgeResult {
    pub sdk_result: crate::messages::MarkReadResult,
    pub raw: Option<Value>,
    pub direct_ids: Vec<String>,
    pub group_ids: Vec<String>,
    pub local_only_ids: Vec<String>,
}

pub trait BridgeSessionProvider {
    fn ensure_messaging_session(&self) -> crate::ImResult<crate::auth::SessionBundle>;
}

pub trait BridgeGroupSessionProvider {
    fn ensure_group_messaging_session(&self) -> crate::ImResult<crate::auth::SessionBundle>;
}

pub trait BridgeAuthenticatedRpcTransport {
    fn authenticated_rpc(
        &mut self,
        endpoint: &str,
        method: &str,
        params: Value,
    ) -> crate::ImResult<Value>;
}

pub trait BridgeRpcTransport {
    fn rpc(&mut self, endpoint: &str, method: &str, params: Value) -> crate::ImResult<Value>;
}

#[doc(hidden)]
pub fn send_direct_text_with_bridge<P, T>(
    client: &crate::core::ImClient,
    session_provider: P,
    transport: T,
    request: DirectTextSendBridgeRequest,
) -> crate::ImResult<DirectTextSendBridgeResult>
where
    P: BridgeSessionProvider,
    T: BridgeAuthenticatedRpcTransport,
{
    let result = crate::internal::message_runtime::direct::DirectTextSender::new(
        client,
        CompatSessionProvider(session_provider),
        CompatTransport(transport),
    )
    .send(crate::internal::message_runtime::direct::DirectTextSend {
        request: request.request,
        resolved_target_did: Some(request.resolved_target_did),
        credentials: Some(
            crate::internal::message_runtime::direct::DirectTextCredentials {
                identity_name: request.credentials.identity_name,
                did_document: request.credentials.did_document,
                key1_private_pem: request.credentials.key1_private_pem,
            },
        ),
    })?;
    Ok(DirectTextSendBridgeResult {
        sdk_result: result.sdk_result,
        target_did: result.target_did,
        message_type: result.message_type.to_string(),
        text: result.text,
        raw: result.raw,
    })
}

#[doc(hidden)]
pub fn read_inbox_with_bridge<P, T>(
    client: &crate::core::ImClient,
    session_provider: P,
    transport: T,
    request: InboxReadBridgeRequest,
) -> crate::ImResult<ReadBridgeResult>
where
    P: BridgeSessionProvider,
    T: BridgeAuthenticatedRpcTransport + BridgeRpcTransport + Clone,
{
    let result = crate::internal::message_runtime::read::MessageReadRuntime::new(
        client,
        CompatSessionProvider(session_provider),
        CompatTransport(transport.clone()),
        CompatTransport(transport),
    )
    .inbox(crate::internal::message_runtime::read::InboxRead {
        query: request.query,
    })?;
    Ok(ReadBridgeResult {
        page: result.page,
        raw: result.raw,
    })
}

#[doc(hidden)]
pub fn read_history_with_bridge<P, T>(
    client: &crate::core::ImClient,
    session_provider: P,
    transport: T,
    request: HistoryReadBridgeRequest,
) -> crate::ImResult<ReadBridgeResult>
where
    P: BridgeSessionProvider,
    T: BridgeAuthenticatedRpcTransport + BridgeRpcTransport + Clone,
{
    let result = crate::internal::message_runtime::read::MessageReadRuntime::new(
        client,
        CompatSessionProvider(session_provider),
        CompatTransport(transport.clone()),
        CompatTransport(transport),
    )
    .history(crate::internal::message_runtime::read::HistoryRead {
        thread: request.thread,
        query: request.query,
        resolved_peer_did: request.resolved_peer_did,
        peer_scope: None,
    })?;
    Ok(ReadBridgeResult {
        page: result.page,
        raw: result.raw,
    })
}

#[doc(hidden)]
pub fn send_group_text_with_bridge<P, T>(
    client: &crate::core::ImClient,
    session_provider: P,
    transport: T,
    request: GroupTextSendBridgeRequest,
) -> crate::ImResult<GroupTextSendBridgeResult>
where
    P: BridgeGroupSessionProvider,
    T: BridgeAuthenticatedRpcTransport,
{
    let result = crate::internal::message_runtime::group::GroupTextSender::new(
        client,
        CompatGroupSessionProvider(session_provider),
        CompatTransport(transport),
    )
    .send(crate::internal::message_runtime::group::GroupTextSend {
        request: request.request,
        credentials: Some(
            crate::internal::message_runtime::group::GroupTextCredentials {
                identity_name: request.credentials.identity_name,
                did_document: request.credentials.did_document,
                key1_private_pem: request.credentials.key1_private_pem,
            },
        ),
    })?;
    Ok(GroupTextSendBridgeResult {
        sdk_result: result.sdk_result,
        group_did: result.group_did,
        message_type: result.message_type.to_string(),
        text: result.text,
        raw: result.raw,
    })
}

#[doc(hidden)]
pub fn mark_read_with_bridge<P, T>(
    client: &crate::core::ImClient,
    message_ids: Vec<crate::ids::MessageId>,
    session_provider: P,
    transport: T,
) -> crate::ImResult<MarkReadBridgeResult>
where
    P: BridgeSessionProvider,
    T: BridgeAuthenticatedRpcTransport,
{
    let result = crate::internal::message_runtime::mark_read::MessageMarkReadRuntime::new(
        client,
        CompatSessionProvider(session_provider),
        CompatTransport(transport),
    )
    .mark_read(crate::internal::message_runtime::mark_read::MarkReadInput { message_ids })?;
    Ok(MarkReadBridgeResult {
        sdk_result: result.sdk_result,
        raw: result.raw,
        direct_ids: result.direct_ids,
        group_ids: result.group_ids,
        local_only_ids: result.local_only_ids,
    })
}

struct CompatSessionProvider<P>(P);

impl<P> crate::internal::auth::session::SessionProvider for CompatSessionProvider<P>
where
    P: BridgeSessionProvider,
{
    fn ensure_session(
        &self,
        scope: crate::auth::AuthScope,
    ) -> crate::ImResult<crate::auth::SessionBundle> {
        if scope != crate::auth::AuthScope::Messaging {
            return Err(crate::ImError::unsupported("auth-scope"));
        }
        self.0.ensure_messaging_session()
    }

    fn refresh_session(&self) -> crate::ImResult<crate::auth::SessionUpdate> {
        Err(crate::ImError::TransportUnavailable {
            detail: "refresh is owned by the bridge transport in Phase 1-beta".to_string(),
        })
    }

    fn status(&self) -> crate::ImResult<crate::auth::AuthStatus> {
        Err(crate::ImError::TransportUnavailable {
            detail: "status is owned by the bridge transport in Phase 1-beta".to_string(),
        })
    }
}

struct CompatGroupSessionProvider<P>(P);

impl<P> crate::internal::auth::session::SessionProvider for CompatGroupSessionProvider<P>
where
    P: BridgeGroupSessionProvider,
{
    fn ensure_session(
        &self,
        scope: crate::auth::AuthScope,
    ) -> crate::ImResult<crate::auth::SessionBundle> {
        if scope != crate::auth::AuthScope::GroupMessaging {
            return Err(crate::ImError::unsupported("auth-scope"));
        }
        self.0.ensure_group_messaging_session()
    }

    fn refresh_session(&self) -> crate::ImResult<crate::auth::SessionUpdate> {
        Err(crate::ImError::TransportUnavailable {
            detail: "refresh is owned by the bridge transport in Phase 1-beta".to_string(),
        })
    }

    fn status(&self) -> crate::ImResult<crate::auth::AuthStatus> {
        Err(crate::ImError::TransportUnavailable {
            detail: "status is owned by the bridge transport in Phase 1-beta".to_string(),
        })
    }
}

struct CompatTransport<T>(T);

impl<T> crate::internal::transport::AuthenticatedRpcTransport for CompatTransport<T>
where
    T: BridgeAuthenticatedRpcTransport,
{
    fn authenticated_rpc(
        &mut self,
        endpoint: &str,
        method: &str,
        params: Value,
    ) -> crate::ImResult<Value> {
        self.0.authenticated_rpc(endpoint, method, params)
    }
}

impl<T> crate::internal::transport::RpcTransport for CompatTransport<T>
where
    T: BridgeRpcTransport,
{
    fn rpc(&mut self, endpoint: &str, method: &str, params: Value) -> crate::ImResult<Value> {
        self.0.rpc(endpoint, method, params)
    }
}
