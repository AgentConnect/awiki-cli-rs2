pub use crate::internal::identity_wire::{RpcCall, TransportProfile};
use serde_json::Value;

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

#[derive(Debug, Clone, PartialEq)]
pub struct DirectoryResolveBridgeResult {
    pub resolution: crate::directory::DirectoryResolution,
    pub resolve: Option<Value>,
    pub lookup: Option<Value>,
    pub public_profile: Option<Value>,
}

pub trait BridgeDirectoryRpcTransport {
    fn rpc(&mut self, endpoint: &str, method: &str, params: Value) -> crate::ImResult<Value>;
}

#[doc(hidden)]
pub fn resolve_peer_with_bridge<T>(
    client: &crate::core::ImClient,
    peer: crate::ids::PeerRef,
    transport: T,
) -> crate::ImResult<DirectoryResolveBridgeResult>
where
    T: BridgeDirectoryRpcTransport,
{
    let result = client
        .directory()
        .resolve_peer_with_runtime(peer, CompatDirectoryTransport(transport))?;
    Ok(DirectoryResolveBridgeResult {
        resolution: result.resolution,
        resolve: result.resolve,
        lookup: result.lookup,
        public_profile: result.public_profile,
    })
}

#[doc(hidden)]
pub fn lookup_handle_with_bridge<T>(
    client: &crate::core::ImClient,
    handle: crate::ids::Handle,
    transport: T,
) -> crate::ImResult<crate::directory::HandleLookupResult>
where
    T: BridgeDirectoryRpcTransport,
{
    client
        .directory()
        .lookup_handle_with_runtime(handle, CompatDirectoryTransport(transport))
}

pub(crate) struct CompatDirectoryTransport<T>(T);

impl<T> crate::internal::transport::RpcTransport for CompatDirectoryTransport<T>
where
    T: BridgeDirectoryRpcTransport,
{
    fn rpc(&mut self, endpoint: &str, method: &str, params: Value) -> crate::ImResult<Value> {
        self.0.rpc(endpoint, method, params)
    }
}
