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
