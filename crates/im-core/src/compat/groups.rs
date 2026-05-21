//! Migration-only group read bridge for `awiki-cli`.

use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub struct GroupGetBridgeRequest {
    pub group: crate::ids::GroupRef,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GroupListBridgeRequest {
    pub request: crate::groups::GroupListRequest,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GroupMembersBridgeRequest {
    pub request: crate::groups::GroupMembersRequest,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GroupMessagesBridgeRequest {
    pub request: crate::groups::GroupMessagesRequest,
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

#[doc(hidden)]
pub fn get_group_with_bridge<P, T>(
    client: &crate::core::ImClient,
    session_provider: P,
    transport: T,
    request: GroupGetBridgeRequest,
) -> crate::ImResult<crate::groups::GroupReadResult>
where
    P: BridgeGroupSessionProvider,
    T: BridgeAuthenticatedRpcTransport,
{
    crate::internal::group_runtime::read::GroupReadRuntime::new(
        client,
        CompatGroupSessionProvider(session_provider),
        CompatTransport(transport),
    )
    .get(request.group)
}

#[doc(hidden)]
pub fn list_groups_with_bridge<P, T>(
    client: &crate::core::ImClient,
    session_provider: P,
    transport: T,
    request: GroupListBridgeRequest,
) -> crate::ImResult<crate::groups::GroupReadResult>
where
    P: BridgeGroupSessionProvider,
    T: BridgeAuthenticatedRpcTransport,
{
    crate::internal::group_runtime::read::GroupReadRuntime::new(
        client,
        CompatGroupSessionProvider(session_provider),
        CompatTransport(transport),
    )
    .list(request.request)
}

#[doc(hidden)]
pub fn list_group_members_with_bridge<P, T>(
    client: &crate::core::ImClient,
    session_provider: P,
    transport: T,
    request: GroupMembersBridgeRequest,
) -> crate::ImResult<crate::groups::GroupReadResult>
where
    P: BridgeGroupSessionProvider,
    T: BridgeAuthenticatedRpcTransport,
{
    crate::internal::group_runtime::read::GroupReadRuntime::new(
        client,
        CompatGroupSessionProvider(session_provider),
        CompatTransport(transport),
    )
    .members(request.request)
}

#[doc(hidden)]
pub fn list_group_messages_with_bridge<P, T>(
    client: &crate::core::ImClient,
    session_provider: P,
    transport: T,
    request: GroupMessagesBridgeRequest,
) -> crate::ImResult<crate::groups::GroupReadResult>
where
    P: BridgeGroupSessionProvider,
    T: BridgeAuthenticatedRpcTransport,
{
    crate::internal::group_runtime::read::GroupReadRuntime::new(
        client,
        CompatGroupSessionProvider(session_provider),
        CompatTransport(transport),
    )
    .messages(request.request)
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
            detail: "refresh is owned by the bridge transport in Phase 3".to_string(),
        })
    }

    fn status(&self) -> crate::ImResult<crate::auth::AuthStatus> {
        Err(crate::ImError::TransportUnavailable {
            detail: "status is owned by the bridge transport in Phase 3".to_string(),
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
