pub use crate::internal::identity_wire::{RpcCall, TransportProfile};

pub const HANDLE_RPC_ENDPOINT: &str = crate::internal::identity_wire::HANDLE_RPC_ENDPOINT;
pub const DID_PROFILE_RPC_ENDPOINT: &str = crate::internal::identity_wire::DID_PROFILE_RPC_ENDPOINT;

pub fn build_handle_lookup_by_did_rpc_call(did: &str) -> crate::ImResult<RpcCall> {
    crate::internal::identity_wire::directory::build_handle_lookup_by_did_rpc_call(did)
}

pub fn build_handle_lookup_by_handle_rpc_call(handle: &str) -> crate::ImResult<RpcCall> {
    crate::internal::identity_wire::directory::build_handle_lookup_by_handle_rpc_call(handle)
}

pub fn build_profile_resolve_rpc_call(did: &str) -> crate::ImResult<RpcCall> {
    crate::internal::identity_wire::profile::build_profile_resolve_rpc_call(did)
}

pub fn build_public_profile_rpc_call(did: &str) -> crate::ImResult<RpcCall> {
    crate::internal::identity_wire::profile::build_public_profile_rpc_call(did)
}

pub fn build_send_otp_rpc_call(phone: &str) -> crate::ImResult<RpcCall> {
    crate::internal::identity_wire::directory::build_send_otp_rpc_call(phone)
}
