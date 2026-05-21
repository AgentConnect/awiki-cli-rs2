pub use crate::internal::identity_wire::{
    ProfileUpdateCall, RecoverHandleRpcParams, RegisterRpcParams, ReplaceDidRpcParams, RestCall,
    RpcCall, TransportProfile, UpdateProfileParams,
};

pub const DID_AUTH_RPC_ENDPOINT: &str = crate::internal::identity_wire::DID_AUTH_RPC_ENDPOINT;
pub const HANDLE_RPC_ENDPOINT: &str = crate::internal::identity_wire::HANDLE_RPC_ENDPOINT;
pub const DID_PROFILE_RPC_ENDPOINT: &str = crate::internal::identity_wire::DID_PROFILE_RPC_ENDPOINT;
pub const EMAIL_SEND_ENDPOINT: &str = crate::internal::identity_wire::EMAIL_SEND_ENDPOINT;
pub const EMAIL_STATUS_ENDPOINT: &str = crate::internal::identity_wire::EMAIL_STATUS_ENDPOINT;
pub const PHONE_BIND_SEND_ENDPOINT: &str = crate::internal::identity_wire::PHONE_BIND_SEND_ENDPOINT;
pub const PHONE_BIND_VERIFY_ENDPOINT: &str =
    crate::internal::identity_wire::PHONE_BIND_VERIFY_ENDPOINT;

pub fn build_get_me_profile_rpc_call() -> RpcCall {
    crate::internal::identity_wire::profile::build_get_me_profile_rpc_call()
}

pub fn build_refresh_token_rpc_call() -> RpcCall {
    crate::internal::identity_wire::profile::build_refresh_token_rpc_call()
}

pub fn build_update_me_profile_rpc_call(
    params: UpdateProfileParams,
) -> crate::ImResult<ProfileUpdateCall> {
    crate::internal::identity_wire::profile::build_update_me_profile_rpc_call(params)
}

pub fn build_update_profile_payload(
    params: UpdateProfileParams,
) -> crate::ImResult<(serde_json::Value, Vec<String>)> {
    crate::internal::identity_wire::profile::build_update_profile_payload(params)
}

pub fn build_register_rpc_call(params: RegisterRpcParams) -> crate::ImResult<RpcCall> {
    crate::internal::identity_wire::recovery::build_register_rpc_call(params)
}

pub fn build_recover_handle_rpc_call(params: RecoverHandleRpcParams) -> crate::ImResult<RpcCall> {
    crate::internal::identity_wire::recovery::build_recover_handle_rpc_call(params)
}

pub fn build_replace_did_rpc_call(params: ReplaceDidRpcParams) -> RpcCall {
    crate::internal::identity_wire::replace_did::build_replace_did_rpc_call(params)
}

pub fn build_email_send_rest_call(
    email: &str,
    handle: Option<&str>,
    authenticated: bool,
) -> crate::ImResult<RestCall> {
    crate::internal::identity_wire::bind::build_email_send_rest_call(email, handle, authenticated)
}

pub fn build_email_status_rest_call(
    email: &str,
    handle: Option<&str>,
    authenticated: bool,
) -> crate::ImResult<RestCall> {
    crate::internal::identity_wire::bind::build_email_status_rest_call(email, handle, authenticated)
}

pub fn build_phone_bind_send_rest_call(phone: &str) -> crate::ImResult<RestCall> {
    crate::internal::identity_wire::bind::build_phone_bind_send_rest_call(phone)
}

pub fn build_phone_bind_verify_rest_call(phone: &str, code: &str) -> crate::ImResult<RestCall> {
    crate::internal::identity_wire::bind::build_phone_bind_verify_rest_call(phone, code)
}

pub fn normalize_phone(phone: &str) -> crate::ImResult<String> {
    crate::internal::identity_wire::normalize_phone(phone)
}

pub fn sanitize_otp(code: &str) -> String {
    crate::internal::identity_wire::sanitize_otp(code)
}

pub fn split_csv(raw: &str) -> Vec<String> {
    crate::internal::identity_wire::split_csv(raw)
}

pub fn normalize_email(email: &str) -> String {
    crate::internal::identity_wire::normalize_email(email)
}

#[derive(Debug, Clone, PartialEq)]
pub struct ContactBindingBridgeResult {
    pub result: crate::identity::ContactBindingResult,
    pub raw_status: Option<serde_json::Value>,
    pub raw_send: Option<serde_json::Value>,
}

pub trait BridgeIdentitySessionProvider {
    fn ensure_identity_session(&self) -> crate::ImResult<crate::auth::SessionBundle>;
}

pub trait BridgeIdentityAuthenticatedRestTransport {
    fn authenticated_rest_post(
        &mut self,
        endpoint: &str,
        method: &str,
        body: serde_json::Value,
    ) -> crate::ImResult<serde_json::Value>;

    fn authenticated_rest_get(
        &mut self,
        endpoint: &str,
        method: &str,
        query: &std::collections::BTreeMap<String, String>,
    ) -> crate::ImResult<serde_json::Value>;
}

pub trait BridgeIdentityRpcTransport {
    fn rpc(
        &mut self,
        endpoint: &str,
        method: &str,
        params: serde_json::Value,
    ) -> crate::ImResult<serde_json::Value>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoverLocalStateMergeRequest {
    pub old_owner_dids: Vec<String>,
    pub new_owner_did: String,
    pub final_identity_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoverLocalStateMergeResult {
    pub store_merge_counts: std::collections::BTreeMap<String, i64>,
    pub e2ee_cleanup_counts: std::collections::BTreeMap<String, i64>,
}

pub trait BridgeRecoverLocalStateStore {
    fn merge_recovered_handle_local_state(
        &mut self,
        request: RecoverLocalStateMergeRequest,
    ) -> crate::ImResult<RecoverLocalStateMergeResult>;
}

#[doc(hidden)]
pub fn bind_contact_with_bridge<P, T>(
    client: &crate::core::ImClient,
    request: crate::identity::ContactBindingRequest,
    session_provider: P,
    transport: T,
) -> crate::ImResult<ContactBindingBridgeResult>
where
    P: BridgeIdentitySessionProvider,
    T: BridgeIdentityAuthenticatedRestTransport,
{
    let result = client.identity().bind_contact_with_runtime(
        request,
        CompatIdentitySessionProvider(session_provider),
        CompatIdentityRestTransport(transport),
    )?;
    Ok(ContactBindingBridgeResult {
        result: result.sdk_result,
        raw_status: result.raw_status,
        raw_send: result.raw_send,
    })
}

#[doc(hidden)]
pub fn bind_email_status_with_bridge<P, T>(
    client: &crate::core::ImClient,
    email: String,
    session_provider: P,
    transport: T,
) -> crate::ImResult<ContactBindingBridgeResult>
where
    P: BridgeIdentitySessionProvider,
    T: BridgeIdentityAuthenticatedRestTransport,
{
    let result = client.identity().bind_email_status_with_runtime(
        email,
        CompatIdentitySessionProvider(session_provider),
        CompatIdentityRestTransport(transport),
    )?;
    Ok(ContactBindingBridgeResult {
        result: result.sdk_result,
        raw_status: result.raw_status,
        raw_send: result.raw_send,
    })
}

#[derive(Debug, Clone, PartialEq)]
pub struct RecoverHandleBridgeResult {
    pub result: crate::identity::RecoverHandleResult,
    pub raw: serde_json::Value,
}

#[doc(hidden)]
pub fn recover_handle_with_bridge<T>(
    request: crate::identity::RecoverHandleRequest,
    transport: T,
) -> crate::ImResult<RecoverHandleBridgeResult>
where
    T: BridgeIdentityRpcTransport,
{
    let result = crate::internal::identity_recovery_runtime::IdentityRecoveryRuntime::new(
        CompatIdentityRpcTransport(transport),
    )
    .recover_handle(request)?;
    Ok(RecoverHandleBridgeResult {
        result: result.sdk_result,
        raw: result.raw,
    })
}

#[doc(hidden)]
pub fn merge_recovered_handle_local_state_with_bridge<S>(
    request: RecoverLocalStateMergeRequest,
    mut store: S,
) -> crate::ImResult<RecoverLocalStateMergeResult>
where
    S: BridgeRecoverLocalStateStore,
{
    validate_recover_local_state_merge_request(&request)?;
    store.merge_recovered_handle_local_state(request)
}

fn validate_recover_local_state_merge_request(
    request: &RecoverLocalStateMergeRequest,
) -> crate::ImResult<()> {
    if request.new_owner_did.trim().is_empty() {
        return Err(crate::ImError::invalid_input(
            Some("new_owner_did".to_string()),
            "new owner DID is required",
        ));
    }
    if request.final_identity_name.trim().is_empty() {
        return Err(crate::ImError::invalid_input(
            Some("final_identity_name".to_string()),
            "final identity name is required",
        ));
    }
    Ok(())
}

struct CompatIdentitySessionProvider<P>(P);

impl<P> crate::internal::auth::session::SessionProvider for CompatIdentitySessionProvider<P>
where
    P: BridgeIdentitySessionProvider,
{
    fn ensure_session(
        &self,
        scope: crate::auth::AuthScope,
    ) -> crate::ImResult<crate::auth::SessionBundle> {
        if scope != crate::auth::AuthScope::UserProfile {
            return Err(crate::ImError::unsupported("auth-scope"));
        }
        self.0.ensure_identity_session()
    }

    fn refresh_session(&self) -> crate::ImResult<crate::auth::SessionUpdate> {
        Err(crate::ImError::TransportUnavailable {
            detail: "refresh is owned by the bridge transport in Phase 2".to_string(),
        })
    }

    fn status(&self) -> crate::ImResult<crate::auth::AuthStatus> {
        Err(crate::ImError::TransportUnavailable {
            detail: "status is owned by the bridge transport in Phase 2".to_string(),
        })
    }
}

struct CompatIdentityRestTransport<T>(T);

impl<T> crate::internal::transport::AuthenticatedRestTransport for CompatIdentityRestTransport<T>
where
    T: BridgeIdentityAuthenticatedRestTransport,
{
    fn authenticated_rest_post(
        &mut self,
        endpoint: &str,
        method: &str,
        body: serde_json::Value,
    ) -> crate::ImResult<serde_json::Value> {
        self.0.authenticated_rest_post(endpoint, method, body)
    }

    fn authenticated_rest_get(
        &mut self,
        endpoint: &str,
        method: &str,
        query: &std::collections::BTreeMap<String, String>,
    ) -> crate::ImResult<serde_json::Value> {
        self.0.authenticated_rest_get(endpoint, method, query)
    }
}

struct CompatIdentityRpcTransport<T>(T);

impl<T> crate::internal::transport::RpcTransport for CompatIdentityRpcTransport<T>
where
    T: BridgeIdentityRpcTransport,
{
    fn rpc(
        &mut self,
        endpoint: &str,
        method: &str,
        params: serde_json::Value,
    ) -> crate::ImResult<serde_json::Value> {
        self.0.rpc(endpoint, method, params)
    }
}
