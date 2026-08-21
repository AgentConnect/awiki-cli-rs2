//! Migration-only group read bridge for `awiki-cli`.

use serde_json::Value;

#[doc(hidden)]
pub fn raw_response(result: &crate::groups::GroupReadResult) -> Option<&Value> {
    result.raw_response()
}

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

#[derive(Debug, Clone, PartialEq)]
pub struct GroupCreateBridgeRequest {
    pub request: crate::groups::GroupCreateRequest,
    pub credentials: GroupLifecycleCredentials,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GroupJoinBridgeRequest {
    pub request: crate::groups::GroupJoinRequest,
    pub credentials: GroupLifecycleCredentials,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GroupLeaveBridgeRequest {
    pub request: crate::groups::GroupLeaveRequest,
    pub credentials: GroupLifecycleCredentials,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GroupMemberMutationBridgeRequest {
    pub request: crate::groups::GroupMemberMutationRequest,
    pub credentials: GroupLifecycleCredentials,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GroupUpdateProfileBridgeRequest {
    pub request: crate::groups::GroupUpdateProfileRequest,
    pub credentials: GroupLifecycleCredentials,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GroupUpdatePolicyBridgeRequest {
    pub request: crate::groups::GroupUpdatePolicyRequest,
    pub credentials: GroupLifecycleCredentials,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GroupLifecycleCredentials {
    pub identity_name: String,
    pub did_document: Option<Value>,
    pub key1_private_pem: String,
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
pub fn create_group_with_bridge<P, T>(
    client: &crate::core::ImClient,
    session_provider: P,
    transport: T,
    request: GroupCreateBridgeRequest,
) -> crate::ImResult<crate::groups::GroupReadResult>
where
    P: BridgeGroupSessionProvider,
    T: BridgeAuthenticatedRpcTransport,
{
    crate::internal::group_runtime::lifecycle::GroupLifecycleRuntime::new(
        client,
        CompatGroupSessionProvider(session_provider),
        CompatTransport(transport),
    )
    .create(request.request, Some(request.credentials.into_internal()))
}

#[doc(hidden)]
pub fn join_group_with_bridge<P, T>(
    client: &crate::core::ImClient,
    session_provider: P,
    transport: T,
    request: GroupJoinBridgeRequest,
) -> crate::ImResult<crate::groups::GroupReadResult>
where
    P: BridgeGroupSessionProvider,
    T: BridgeAuthenticatedRpcTransport,
{
    crate::internal::group_runtime::lifecycle::GroupLifecycleRuntime::new(
        client,
        CompatGroupSessionProvider(session_provider),
        CompatTransport(transport),
    )
    .join(request.request, Some(request.credentials.into_internal()))
}

#[doc(hidden)]
pub fn leave_group_with_bridge<P, T>(
    client: &crate::core::ImClient,
    session_provider: P,
    transport: T,
    request: GroupLeaveBridgeRequest,
) -> crate::ImResult<crate::groups::GroupReadResult>
where
    P: BridgeGroupSessionProvider,
    T: BridgeAuthenticatedRpcTransport,
{
    crate::internal::group_runtime::lifecycle::GroupLifecycleRuntime::new(
        client,
        CompatGroupSessionProvider(session_provider),
        CompatTransport(transport),
    )
    .leave(request.request, Some(request.credentials.into_internal()))
}

#[doc(hidden)]
pub fn add_group_member_with_bridge<P, T>(
    client: &crate::core::ImClient,
    session_provider: P,
    transport: T,
    request: GroupMemberMutationBridgeRequest,
) -> crate::ImResult<crate::groups::GroupReadResult>
where
    P: BridgeGroupSessionProvider,
    T: BridgeAuthenticatedRpcTransport,
{
    crate::internal::group_runtime::lifecycle::GroupLifecycleRuntime::new(
        client,
        CompatGroupSessionProvider(session_provider),
        CompatTransport(transport),
    )
    .add_member(request.request, Some(request.credentials.into_internal()))
}

#[doc(hidden)]
pub fn remove_group_member_with_bridge<P, T>(
    client: &crate::core::ImClient,
    session_provider: P,
    transport: T,
    request: GroupMemberMutationBridgeRequest,
) -> crate::ImResult<crate::groups::GroupReadResult>
where
    P: BridgeGroupSessionProvider,
    T: BridgeAuthenticatedRpcTransport,
{
    crate::internal::group_runtime::lifecycle::GroupLifecycleRuntime::new(
        client,
        CompatGroupSessionProvider(session_provider),
        CompatTransport(transport),
    )
    .remove_member(request.request, Some(request.credentials.into_internal()))
}

#[doc(hidden)]
pub fn update_group_profile_with_bridge<P, T>(
    client: &crate::core::ImClient,
    session_provider: P,
    transport: T,
    request: GroupUpdateProfileBridgeRequest,
) -> crate::ImResult<crate::groups::GroupReadResult>
where
    P: BridgeGroupSessionProvider,
    T: BridgeAuthenticatedRpcTransport,
{
    crate::internal::group_runtime::lifecycle::GroupLifecycleRuntime::new(
        client,
        CompatGroupSessionProvider(session_provider),
        CompatTransport(transport),
    )
    .update_profile(request.request, Some(request.credentials.into_internal()))
}

#[doc(hidden)]
pub fn update_group_policy_with_bridge<P, T>(
    client: &crate::core::ImClient,
    session_provider: P,
    transport: T,
    request: GroupUpdatePolicyBridgeRequest,
) -> crate::ImResult<crate::groups::GroupReadResult>
where
    P: BridgeGroupSessionProvider,
    T: BridgeAuthenticatedRpcTransport,
{
    crate::internal::group_runtime::lifecycle::GroupLifecycleRuntime::new(
        client,
        CompatGroupSessionProvider(session_provider),
        CompatTransport(transport),
    )
    .update_policy(request.request, Some(request.credentials.into_internal()))
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

impl GroupLifecycleCredentials {
    fn into_internal(self) -> crate::internal::group_runtime::lifecycle::GroupLifecycleCredentials {
        crate::internal::group_runtime::lifecycle::GroupLifecycleCredentials {
            identity_name: self.identity_name,
            did_document: self.did_document,
            signer: crate::internal::proof::origin::OriginProofSigner::PrivateKeyPem(
                self.key1_private_pem,
            ),
            verification_method: None,
        }
    }
}
