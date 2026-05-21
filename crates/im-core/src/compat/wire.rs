//! Migration-only wire helpers for `awiki-cli` wrappers.

pub use crate::internal::wire::direct::DirectPayload;

use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WireIdentity {
    pub did: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InboxWireRequest {
    pub limit: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryWireRequest {
    pub peer_did: String,
    pub limit: i64,
    pub cursor: Option<String>,
    pub skip: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkReadWireRequest {
    pub message_ids: Vec<String>,
}

#[doc(hidden)]
pub fn now_rfc3339() -> String {
    crate::internal::wire::common::now_rfc3339()
}

#[doc(hidden)]
pub fn generate_operation_id() -> String {
    crate::internal::wire::common::generate_operation_id()
}

#[doc(hidden)]
pub fn content_type_for_message_kind(
    kind: crate::messages::MessageKind,
    message_type: Option<&str>,
) -> &'static str {
    crate::internal::wire::common::content_type_for_message_kind(kind, message_type)
}

#[doc(hidden)]
pub fn build_direct_text_payload(
    sender_did: &str,
    target_did: &str,
    text: &str,
    content_type: &str,
) -> crate::ImResult<DirectPayload> {
    crate::internal::wire::direct::build_direct_text_payload(
        sender_did,
        target_did,
        text,
        content_type,
    )
}

#[doc(hidden)]
pub fn build_group_send_payload(
    sender_did: &str,
    group_did: &str,
    text: &str,
    content_type: &str,
) -> crate::ImResult<DirectPayload> {
    crate::internal::wire::group::build_group_send_payload(
        sender_did,
        group_did,
        text,
        content_type,
    )
}

#[doc(hidden)]
pub fn build_group_create_payload(
    sender_did: &str,
    request: &crate::groups::GroupCreateRequest,
) -> crate::ImResult<DirectPayload> {
    crate::internal::wire::group::build_group_create_payload(sender_did, request)
}

#[doc(hidden)]
pub fn build_group_join_payload(
    sender_did: &str,
    request: &crate::groups::GroupJoinRequest,
) -> crate::ImResult<DirectPayload> {
    crate::internal::wire::group::build_group_join_payload(sender_did, request)
}

#[doc(hidden)]
pub fn build_group_leave_payload(
    sender_did: &str,
    request: &crate::groups::GroupLeaveRequest,
) -> crate::ImResult<DirectPayload> {
    crate::internal::wire::group::build_group_leave_payload(sender_did, request)
}

#[doc(hidden)]
pub fn build_group_add_member_payload(
    sender_did: &str,
    request: &crate::groups::GroupMemberMutationRequest,
) -> crate::ImResult<DirectPayload> {
    crate::internal::wire::group::build_group_add_member_payload(sender_did, request)
}

#[doc(hidden)]
pub fn build_group_remove_member_payload(
    sender_did: &str,
    request: &crate::groups::GroupMemberMutationRequest,
) -> crate::ImResult<DirectPayload> {
    crate::internal::wire::group::build_group_remove_member_payload(sender_did, request)
}

#[doc(hidden)]
pub fn build_group_update_profile_payload(
    sender_did: &str,
    request: &crate::groups::GroupUpdateProfileRequest,
) -> crate::ImResult<DirectPayload> {
    crate::internal::wire::group::build_group_update_profile_payload(sender_did, request)
}

#[doc(hidden)]
pub fn build_group_update_profile_patch_payload(
    sender_did: &str,
    group_did: &str,
    patch: serde_json::Map<String, Value>,
) -> crate::ImResult<DirectPayload> {
    crate::internal::wire::group::build_group_update_profile_patch_payload(
        sender_did, group_did, patch,
    )
}

#[doc(hidden)]
pub fn build_group_update_policy_payload(
    sender_did: &str,
    request: &crate::groups::GroupUpdatePolicyRequest,
) -> crate::ImResult<DirectPayload> {
    crate::internal::wire::group::build_group_update_policy_payload(sender_did, request)
}

#[doc(hidden)]
pub fn build_group_update_policy_patch_payload(
    sender_did: &str,
    group_did: &str,
    patch: serde_json::Map<String, Value>,
) -> crate::ImResult<DirectPayload> {
    crate::internal::wire::group::build_group_update_policy_patch_payload(
        sender_did, group_did, patch,
    )
}

#[doc(hidden)]
pub fn build_group_get_rpc_params(sender_did: &str, group_did: &str) -> crate::ImResult<Value> {
    crate::internal::wire::group::build_group_get_rpc_params(sender_did, group_did)
}

#[doc(hidden)]
pub fn build_group_list_rpc_params(sender_did: &str, limit: i64) -> Value {
    crate::internal::wire::group::build_group_list_rpc_params(sender_did, limit)
}

#[doc(hidden)]
pub fn build_group_members_rpc_params(
    sender_did: &str,
    group_did: &str,
    limit: i64,
) -> crate::ImResult<Value> {
    crate::internal::wire::group::build_group_members_rpc_params(sender_did, group_did, limit)
}

#[doc(hidden)]
pub fn build_group_messages_rpc_params(
    sender_did: &str,
    group_did: &str,
    limit: i64,
    cursor: Option<&str>,
    skip: i64,
) -> crate::ImResult<Value> {
    crate::internal::wire::group::build_group_messages_rpc_params(
        sender_did, group_did, limit, cursor, skip,
    )
}

#[doc(hidden)]
pub fn build_inbox_rpc_params(identity: &WireIdentity, request: InboxWireRequest) -> Value {
    crate::internal::wire::inbox::build_inbox_rpc_params(
        &to_internal_identity(identity),
        crate::internal::wire::inbox::InboxWireRequest {
            limit: request.limit,
        },
    )
}

#[doc(hidden)]
pub fn build_history_rpc_params(
    identity: &WireIdentity,
    request: HistoryWireRequest,
) -> crate::ImResult<Value> {
    crate::internal::wire::history::build_history_rpc_params(
        &to_internal_identity(identity),
        crate::internal::wire::history::HistoryWireRequest {
            peer_did: request.peer_did,
            limit: request.limit,
            cursor: request.cursor,
            skip: request.skip,
        },
    )
}

#[doc(hidden)]
pub fn build_mark_read_rpc_params(
    identity: &WireIdentity,
    request: MarkReadWireRequest,
) -> crate::ImResult<Value> {
    crate::internal::wire::inbox::build_mark_read_rpc_params(
        &to_internal_identity(identity),
        crate::internal::wire::inbox::MarkReadWireRequest {
            message_ids: request.message_ids,
        },
    )
}

#[doc(hidden)]
pub fn message_meta(sender_did: &str, service_did: &str, profile: &str) -> Value {
    crate::internal::wire::common::message_meta(sender_did, service_did, profile)
}

#[doc(hidden)]
pub fn signed_message_meta(
    sender_did: &str,
    target_kind: &str,
    target_did: &str,
    profile: &str,
    content_type: &str,
) -> Value {
    crate::internal::wire::common::signed_message_meta(
        sender_did,
        target_kind,
        target_did,
        profile,
        content_type,
    )
}

fn to_internal_identity(identity: &WireIdentity) -> crate::internal::wire::common::WireIdentity {
    crate::internal::wire::common::WireIdentity {
        did: identity.did.clone(),
    }
}
