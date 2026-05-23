pub use crate::internal::identity_wire::{RpcCall, TransportProfile};
use serde_json::Value;

pub const HANDLE_RPC_ENDPOINT: &str = crate::internal::identity_wire::HANDLE_RPC_ENDPOINT;
pub const DID_PROFILE_RPC_ENDPOINT: &str = crate::internal::identity_wire::DID_PROFILE_RPC_ENDPOINT;
pub const DID_RELATIONSHIPS_RPC_ENDPOINT: &str =
    crate::internal::identity_wire::DID_RELATIONSHIPS_RPC_ENDPOINT;

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

pub fn build_follow_rpc_call(target_did: &str) -> crate::ImResult<RpcCall> {
    crate::internal::identity_wire::relationships::build_follow_rpc_call(target_did)
}

pub fn build_unfollow_rpc_call(target_did: &str) -> crate::ImResult<RpcCall> {
    crate::internal::identity_wire::relationships::build_unfollow_rpc_call(target_did)
}

pub fn build_relationship_status_rpc_call(target_did: &str) -> crate::ImResult<RpcCall> {
    crate::internal::identity_wire::relationships::build_relationship_status_rpc_call(target_did)
}

pub fn build_followers_rpc_call(limit: u32, offset: u32) -> crate::ImResult<RpcCall> {
    crate::internal::identity_wire::relationships::build_followers_rpc_call(limit, offset)
}

pub fn build_following_rpc_call(limit: u32, offset: u32) -> crate::ImResult<RpcCall> {
    crate::internal::identity_wire::relationships::build_following_rpc_call(limit, offset)
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

#[cfg(feature = "sqlite")]
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ContactRecord {
    pub owner_identity_id: String,
    pub owner_did: String,
    pub did: String,
    pub name: String,
    pub handle: String,
    pub nick_name: String,
    pub bio: String,
    pub profile_md: String,
    pub tags: String,
    pub relationship: String,
    pub source_type: String,
    pub source_name: String,
    pub source_group_id: String,
    pub connected_at: String,
    pub recommended_reason: String,
    pub followed: Option<bool>,
    pub messaged: Option<bool>,
    pub note: String,
    pub first_seen_at: String,
    pub last_seen_at: String,
    pub metadata: String,
    pub credential_name: String,
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

#[cfg(feature = "sqlite")]
#[doc(hidden)]
pub fn get_contact_by_did(
    connection: &rusqlite::Connection,
    owner_did: &str,
    did: &str,
) -> crate::ImResult<Value> {
    crate::internal::contact_store::records::get_contact_by_did_json(connection, owner_did, did)
}

#[cfg(feature = "sqlite")]
#[doc(hidden)]
pub fn get_contact_by_did_for_owner_identity(
    connection: &rusqlite::Connection,
    owner_identity_id: &str,
    owner_did: &str,
    did: &str,
) -> crate::ImResult<Value> {
    crate::internal::contact_store::records::get_contact_by_did_json_for_owner_identity(
        connection,
        owner_identity_id,
        owner_did,
        did,
    )
}

#[cfg(feature = "sqlite")]
#[doc(hidden)]
pub fn get_current_contact_by_handle(
    connection: &rusqlite::Connection,
    owner_did: &str,
    handle: &str,
) -> crate::ImResult<Value> {
    crate::internal::contact_store::records::get_current_contact_by_handle_json(
        connection, owner_did, handle,
    )
}

#[cfg(feature = "sqlite")]
#[doc(hidden)]
pub fn get_current_contact_by_handle_for_owner_identity(
    connection: &rusqlite::Connection,
    owner_identity_id: &str,
    owner_did: &str,
    handle: &str,
) -> crate::ImResult<Value> {
    crate::internal::contact_store::records::get_current_contact_by_handle_json_for_owner_identity(
        connection,
        owner_identity_id,
        owner_did,
        handle,
    )
}

#[cfg(feature = "sqlite")]
#[doc(hidden)]
pub fn resolve_contact_handle_by_did(
    connection: &rusqlite::Connection,
    owner_did: &str,
    did: &str,
) -> crate::ImResult<String> {
    crate::internal::contact_store::records::resolve_contact_handle_by_did(
        connection, owner_did, did,
    )
}

#[cfg(feature = "sqlite")]
#[doc(hidden)]
pub fn resolve_contact_handle_by_did_for_owner_identity(
    connection: &rusqlite::Connection,
    owner_identity_id: &str,
    owner_did: &str,
    did: &str,
) -> crate::ImResult<String> {
    crate::internal::contact_store::records::resolve_contact_handle_by_did_for_owner_identity(
        connection,
        owner_identity_id,
        owner_did,
        did,
    )
}

#[cfg(feature = "sqlite")]
#[doc(hidden)]
pub fn list_dids_by_handle(
    connection: &rusqlite::Connection,
    owner_did: &str,
    handle: &str,
) -> crate::ImResult<Vec<String>> {
    crate::internal::contact_store::records::list_dids_by_handle(connection, owner_did, handle)
}

#[cfg(feature = "sqlite")]
#[doc(hidden)]
pub fn list_dids_by_handle_for_owner_identity(
    connection: &rusqlite::Connection,
    owner_identity_id: &str,
    owner_did: &str,
    handle: &str,
) -> crate::ImResult<Vec<String>> {
    crate::internal::contact_store::records::list_dids_by_handle_for_owner_identity(
        connection,
        owner_identity_id,
        owner_did,
        handle,
    )
}

#[cfg(feature = "sqlite")]
#[doc(hidden)]
pub fn list_contact_handle_history(
    connection: &rusqlite::Connection,
    handle: &str,
) -> crate::ImResult<Vec<Value>> {
    crate::internal::contact_store::records::list_contact_handle_history(connection, handle)
}

#[cfg(feature = "sqlite")]
#[doc(hidden)]
pub fn upsert_contact(
    connection: &mut rusqlite::Connection,
    record: ContactRecord,
) -> crate::ImResult<()> {
    crate::internal::contact_store::records::upsert_contact(connection, record.into())
}

#[cfg(feature = "sqlite")]
impl From<ContactRecord> for crate::internal::contact_store::records::ContactRecord {
    fn from(record: ContactRecord) -> Self {
        Self {
            owner_identity_id: record.owner_identity_id,
            owner_did: record.owner_did,
            did: record.did,
            name: record.name,
            handle: record.handle,
            nick_name: record.nick_name,
            bio: record.bio,
            profile_md: record.profile_md,
            tags: record.tags,
            relationship: record.relationship,
            source_type: record.source_type,
            source_name: record.source_name,
            source_group_id: record.source_group_id,
            connected_at: record.connected_at,
            recommended_reason: record.recommended_reason,
            followed: record.followed,
            messaged: record.messaged,
            note: record.note,
            first_seen_at: record.first_seen_at,
            last_seen_at: record.last_seen_at,
            metadata: record.metadata,
            credential_name: record.credential_name,
        }
    }
}
