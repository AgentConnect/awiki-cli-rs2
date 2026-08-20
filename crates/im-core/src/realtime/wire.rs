//! Migration-only wire helpers for `awiki-cli` wrappers.

pub use crate::internal::wire::direct::DirectPayload;

use serde_json::{json, Map, Value};

const ATTACHMENT_MANIFEST_CONTENT_TYPE: &str = "application/anp-attachment-manifest+json";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WireIdentity {
    pub did: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BridgeWireIdentity {
    pub identity_name: String,
    pub did: String,
    pub did_document: Option<Value>,
    pub key1_private_pem: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InboxWireRequest {
    pub limit: i64,
    pub auth: Option<InboxWireAuth>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InboxWireAuth {
    pub inbox_owner_did: String,
    pub inbox_auth_verification_method: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryWireRequest {
    pub peer_did: String,
    pub limit: i64,
    pub cursor: Option<String>,
    pub skip: i64,
    pub auth: Option<HistoryWireAuth>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryWireAuth {
    pub inbox_owner_did: String,
    pub inbox_auth_verification_method: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkReadWireRequest {
    pub message_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupCreateWireRequest {
    pub name: String,
    pub description: String,
    pub avatar_uri: String,
    pub discoverability: String,
    pub admission_mode: String,
    pub message_security_profile: String,
    pub e2ee: bool,
    pub slug: String,
    pub goal: String,
    pub rules: String,
    pub message_prompt: String,
    pub doc_url: String,
    pub attachments_allowed: Option<bool>,
    pub max_members: String,
    pub member_max_messages: Option<i64>,
    pub member_max_total_chars: Option<i64>,
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
pub fn build_direct_json_payload(
    sender_did: &str,
    target_did: &str,
    payload: Value,
) -> crate::ImResult<DirectPayload> {
    crate::internal::wire::direct::build_direct_json_payload(sender_did, target_did, payload)
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
pub fn build_group_json_send_payload(
    sender_did: &str,
    group_did: &str,
    payload: Value,
) -> crate::ImResult<DirectPayload> {
    crate::internal::wire::group::build_group_json_send_payload(sender_did, group_did, payload)
}

#[doc(hidden)]
pub fn build_group_create_payload(
    sender_did: &str,
    request: &crate::groups::GroupCreateRequest,
    service_did: &crate::ids::Did,
) -> crate::ImResult<DirectPayload> {
    crate::internal::wire::group::build_group_create_payload(sender_did, request, service_did)
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
    crate::internal::wire::group::build_group_list_rpc_params(sender_did, limit, None)
}

#[doc(hidden)]
pub fn build_group_list_rpc_params_with_cursor(
    sender_did: &str,
    limit: i64,
    cursor: Option<&str>,
) -> Value {
    crate::internal::wire::group::build_group_list_rpc_params(sender_did, limit, cursor)
}

#[doc(hidden)]
pub fn build_group_members_rpc_params(
    sender_did: &str,
    group_did: &str,
    limit: i64,
) -> crate::ImResult<Value> {
    crate::internal::wire::group::build_group_members_rpc_params(sender_did, group_did, limit, None)
}

#[doc(hidden)]
pub fn build_group_members_rpc_params_with_cursor(
    sender_did: &str,
    group_did: &str,
    limit: i64,
    cursor: Option<&str>,
) -> crate::ImResult<Value> {
    crate::internal::wire::group::build_group_members_rpc_params(
        sender_did, group_did, limit, cursor,
    )
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
pub fn build_bridge_direct_send_rpc_params(
    identity: &BridgeWireIdentity,
    target_did: &str,
    text: &str,
    message_type: &str,
) -> crate::ImResult<Value> {
    let payload = crate::internal::wire::direct::build_direct_text_payload(
        &identity.did,
        target_did,
        text,
        content_type_for_bridge_message_type(message_type),
    )?;
    signed_bridge_params(identity, payload)
}

#[doc(hidden)]
pub fn build_bridge_group_create_rpc_params(
    identity: &BridgeWireIdentity,
    service_did: &str,
    request: GroupCreateWireRequest,
) -> crate::ImResult<Value> {
    if service_did.trim().is_empty() {
        return Err(crate::ImError::invalid_input(
            Some("service_did".to_string()),
            "message service did is required",
        ));
    }
    if request.name.trim().is_empty() {
        return Err(crate::ImError::invalid_input(
            Some("name".to_string()),
            "group display name is required",
        ));
    }
    let service_did = crate::ids::Did::parse(service_did)?;
    let payload = crate::internal::wire::group::build_group_create_payload(
        &identity.did,
        &crate::groups::GroupCreateRequest {
            creator_handle: None,
            name: request.name,
            description: optional_trimmed(request.description),
            avatar_uri: optional_trimmed(request.avatar_uri),
            discoverability: crate::groups::GroupDiscoverability::parse_optional(
                request.discoverability,
            )?,
            admission_mode: crate::groups::GroupAdmissionMode::parse_optional(
                request.admission_mode,
            )?,
            message_security_profile: crate::groups::GroupMessageSecurityProfile::parse_optional(
                request.message_security_profile,
            )?,
            security: if request.e2ee {
                crate::groups::GroupSecurityRequirement::Required
            } else {
                crate::groups::GroupSecurityRequirement::Default
            },
            e2ee: request.e2ee,
            slug: optional_trimmed(request.slug),
            goal: optional_trimmed(request.goal),
            rules: optional_trimmed(request.rules),
            message_prompt: optional_trimmed(request.message_prompt),
            doc_url: optional_trimmed(request.doc_url),
            attachments_allowed: request.attachments_allowed,
            max_members: crate::groups::GroupMemberLimit::parse_optional(request.max_members)?,
            member_max_messages: request.member_max_messages,
            member_max_total_chars: request.member_max_total_chars,
        },
        &service_did,
    )?;
    signed_bridge_params(identity, payload)
}

#[doc(hidden)]
pub fn build_bridge_group_get_info_rpc_params(
    identity: &BridgeWireIdentity,
    group_did: &str,
    include_policy: bool,
    include_member_list: bool,
) -> crate::ImResult<Value> {
    let group_did = require_group_did(group_did)?;
    let mut body = Map::new();
    if include_policy {
        body.insert("include_policy".to_string(), Value::Bool(true));
    }
    if include_member_list {
        body.insert("include_member_list".to_string(), Value::Bool(true));
    }
    Ok(json!({
        "meta": group_base_meta(&identity.did, Some(("group", group_did))),
        "body": body,
    }))
}

#[doc(hidden)]
pub fn build_bridge_group_join_rpc_params(
    identity: &BridgeWireIdentity,
    group_did: &str,
    reason_text: &str,
) -> crate::ImResult<Value> {
    let payload = crate::internal::wire::group::build_group_join_payload(
        &identity.did,
        &crate::groups::GroupJoinRequest {
            member_handle: None,
            group: crate::ids::GroupRef::parse(group_did)?,
            reason_text: optional_trimmed(reason_text),
        },
    )?;
    signed_bridge_params(identity, payload)
}

#[doc(hidden)]
pub fn build_bridge_group_add_rpc_params(
    identity: &BridgeWireIdentity,
    group_did: &str,
    member_did: &str,
    role: &str,
    reason_text: &str,
) -> crate::ImResult<Value> {
    let request = group_member_mutation_request(group_did, member_did, Some(role), reason_text)?;
    let payload =
        crate::internal::wire::group::build_group_add_member_payload(&identity.did, &request)?;
    signed_bridge_params(identity, payload)
}

#[doc(hidden)]
pub fn build_bridge_group_remove_rpc_params(
    identity: &BridgeWireIdentity,
    group_did: &str,
    member_did: &str,
    reason_text: &str,
) -> crate::ImResult<Value> {
    let request = group_member_mutation_request(group_did, member_did, None, reason_text)?;
    let payload =
        crate::internal::wire::group::build_group_remove_member_payload(&identity.did, &request)?;
    signed_bridge_params(identity, payload)
}

#[doc(hidden)]
pub fn build_bridge_group_leave_rpc_params(
    identity: &BridgeWireIdentity,
    group_did: &str,
) -> crate::ImResult<Value> {
    let payload = crate::internal::wire::group::build_group_leave_payload(
        &identity.did,
        &crate::groups::GroupLeaveRequest {
            group: crate::ids::GroupRef::parse(group_did)?,
            reason_text: None,
            security: crate::groups::GroupSecurityRequirement::Default,
        },
    )?;
    signed_bridge_params(identity, payload)
}

#[doc(hidden)]
pub fn build_bridge_group_update_profile_rpc_params(
    identity: &BridgeWireIdentity,
    group_did: &str,
    patch: Map<String, Value>,
) -> crate::ImResult<Value> {
    let payload = crate::internal::wire::group::build_group_update_profile_patch_payload(
        &identity.did,
        group_did,
        patch,
    )?;
    signed_bridge_params(identity, payload)
}

#[doc(hidden)]
pub fn build_bridge_group_update_policy_rpc_params(
    identity: &BridgeWireIdentity,
    group_did: &str,
    patch: Map<String, Value>,
) -> crate::ImResult<Value> {
    let payload = crate::internal::wire::group::build_group_update_policy_patch_payload(
        &identity.did,
        group_did,
        patch,
    )?;
    signed_bridge_params(identity, payload)
}

#[doc(hidden)]
pub fn build_bridge_group_send_rpc_params(
    identity: &BridgeWireIdentity,
    group_did: &str,
    text: &str,
    message_type: &str,
) -> crate::ImResult<Value> {
    let payload = crate::internal::wire::group::build_group_send_payload(
        &identity.did,
        group_did,
        text,
        content_type_for_bridge_message_type(message_type),
    )?;
    signed_bridge_params(identity, payload)
}

#[doc(hidden)]
pub fn build_inbox_rpc_params(identity: &WireIdentity, request: InboxWireRequest) -> Value {
    crate::internal::wire::inbox::build_inbox_rpc_params(
        &to_internal_identity(identity),
        crate::internal::wire::inbox::InboxWireRequest {
            limit: request.limit,
            auth: request
                .auth
                .map(|auth| crate::internal::wire::inbox::InboxWireAuth {
                    inbox_owner_did: auth.inbox_owner_did,
                    inbox_auth_verification_method: auth.inbox_auth_verification_method,
                    service_did:
                        crate::internal::wire::common::DEFAULT_DELEGATED_MESSAGE_SERVICE_DID
                            .to_owned(),
                }),
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
            auth: request
                .auth
                .map(|auth| crate::internal::wire::history::HistoryWireAuth {
                    inbox_owner_did: auth.inbox_owner_did,
                    inbox_auth_verification_method: auth.inbox_auth_verification_method,
                    service_did:
                        crate::internal::wire::common::DEFAULT_DELEGATED_MESSAGE_SERVICE_DID
                            .to_owned(),
                }),
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

fn signed_bridge_params(
    identity: &BridgeWireIdentity,
    payload: DirectPayload,
) -> crate::ImResult<Value> {
    let origin_proof = crate::internal::proof::origin::build_origin_proof(
        &crate::internal::proof::origin::OriginProofIdentity {
            identity_name: identity.identity_name.clone(),
            did_document: identity.did_document.clone(),
            signer: crate::internal::proof::origin::OriginProofSigner::PrivateKeyPem(
                identity.key1_private_pem.clone(),
            ),
            verification_method: None,
        },
        &payload,
    )?;
    Ok(json!({
        "meta": payload.meta,
        "auth": crate::internal::proof::origin::origin_auth_value(&origin_proof),
        "body": payload.body,
    }))
}

fn group_member_mutation_request(
    group_did: &str,
    member_did: &str,
    role: Option<&str>,
    reason_text: &str,
) -> crate::ImResult<crate::groups::GroupMemberMutationRequest> {
    Ok(crate::groups::GroupMemberMutationRequest {
        group: crate::ids::GroupRef::parse(group_did)?,
        member: crate::groups::GroupMemberRef::parse(member_did, "")?,
        role: match role {
            Some(role) => crate::groups::GroupMemberRole::parse_optional(role)?,
            None => None,
        },
        reason_text: optional_trimmed(reason_text),
        leave_request_id: None,
        security: crate::groups::GroupSecurityRequirement::Default,
    })
}

fn content_type_for_bridge_message_type(message_type: &str) -> &'static str {
    match message_type.trim().to_ascii_lowercase().as_str() {
        "attachment_manifest" => ATTACHMENT_MANIFEST_CONTENT_TYPE,
        "event" => "application/json",
        _ => "text/plain",
    }
}

fn optional_trimmed(value: impl AsRef<str>) -> Option<String> {
    let value = value.as_ref().trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn require_group_did(group_did: &str) -> crate::ImResult<&str> {
    let group_did = group_did.trim();
    if group_did.is_empty() {
        return Err(crate::ImError::invalid_input(
            Some("group".to_string()),
            "group is required",
        ));
    }
    Ok(group_did)
}

fn group_base_meta(sender_did: &str, target: Option<(&str, &str)>) -> Value {
    let mut meta = Map::new();
    meta.insert(
        "profile".to_string(),
        Value::String("anp.group.base.v1".to_string()),
    );
    meta.insert(
        "security_profile".to_string(),
        Value::String("transport-protected".to_string()),
    );
    meta.insert(
        "sender_did".to_string(),
        Value::String(sender_did.to_string()),
    );
    if let Some((kind, did)) = target {
        meta.insert(
            "target".to_string(),
            json!({
                "kind": kind,
                "did": did,
            }),
        );
    }
    Value::Object(meta)
}
