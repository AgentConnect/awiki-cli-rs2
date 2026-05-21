use im_core::prelude::{
    AuthScope, Cursor, Did, GroupCreateRequest, GroupJoinRequest, GroupLeaveRequest,
    GroupListRequest, GroupMemberMutationRequest, GroupMembersRequest, GroupMessagesRequest,
    GroupPolicyPatch, GroupProfilePatch, GroupRef, GroupUpdatePolicyRequest,
    GroupUpdateProfileRequest, Handle, PageLimit, SessionBundle,
};
use serde_json::{json, Value};

use crate::authsdk::Session;
use crate::config::Resolved;
use crate::identity::{types::StoredIdentity, Manager};
use crate::message::{self, MessageError, WSProxyTransport};
use crate::runtime;
use crate::transportcfg::Profile;

pub fn create_group_via_im_core(
    resolved: &Resolved,
    manager: &Manager,
    client: &im_core::ImClient,
    request: message::GroupCreateRequest,
) -> Result<message::CommandResult, MessageError> {
    if request.name.trim().is_empty() {
        return Err(MessageError::GroupRequired);
    }
    let record = message::require_active_identity(resolved, manager, &request.identity_name)?;
    let bridge_result = im_core::compat::groups::create_group_with_bridge(
        client,
        group_session_provider(resolved, manager, client, &record),
        group_transport(resolved, manager, &record, Profile::RpcDefault),
        im_core::compat::groups::GroupCreateBridgeRequest {
            request: group_create_request(request, &resolved.anp_service_did)?,
            credentials: group_lifecycle_credentials(&record),
        },
    )
    .map_err(im_error_to_message_error)?;
    let raw = bridge_result.raw;
    let group_did = message::group_did_from_result(&raw);
    let mut warnings = group_control_warnings(resolved, bridge_result.warnings);
    warnings.extend(message::sync_group_state(
        resolved, manager, &record, &group_did, true,
    ));
    let snapshot = message::cached_group_snapshot(resolved, &record, &group_did)
        .or_else(|| message::normalize_group_snapshot(&raw))
        .unwrap_or(Value::Null);
    let members =
        message::cached_group_members(resolved, &record, &group_did, 100).unwrap_or_default();
    Ok(message::CommandResult {
        data: json!({
            "group": snapshot,
            "members": members,
            "delivery": raw,
            "source": message::group_control_source(&raw),
        }),
        summary: format!("Created group {group_did}"),
        warnings: message::compact_warnings(warnings),
    })
}

pub fn join_group_via_im_core(
    resolved: &Resolved,
    manager: &Manager,
    client: &im_core::ImClient,
    request: message::GroupJoinRequest,
) -> Result<message::CommandResult, MessageError> {
    if request.group.trim().is_empty() {
        return Err(MessageError::GroupRequired);
    }
    let record = message::require_active_identity(resolved, manager, &request.identity_name)?;
    let requested_group = request.group.clone();
    let bridge_result = im_core::compat::groups::join_group_with_bridge(
        client,
        group_session_provider(resolved, manager, client, &record),
        group_transport(resolved, manager, &record, Profile::RpcDefault),
        im_core::compat::groups::GroupJoinBridgeRequest {
            request: GroupJoinRequest {
                group: GroupRef::parse(&request.group).map_err(im_error_to_message_error)?,
                reason_text: optional_string(&request.reason_text),
            },
            credentials: group_lifecycle_credentials(&record),
        },
    )
    .map_err(im_error_to_message_error)?;
    let raw = bridge_result.raw;
    let group_did = default_string(&message::group_did_from_result(&raw), &requested_group);
    let mut warnings = group_control_warnings(resolved, bridge_result.warnings);
    warnings.extend(message::sync_group_state(
        resolved, manager, &record, &group_did, true,
    ));
    let snapshot = message::cached_group_snapshot(resolved, &record, &group_did)
        .or_else(|| message::normalize_group_snapshot(&raw))
        .unwrap_or_else(|| json!({ "group_did": group_did }));
    Ok(message::CommandResult {
        data: json!({
            "group": snapshot,
            "delivery": raw,
            "source": message::group_control_source(&raw),
        }),
        summary: format!("Joined group {group_did}"),
        warnings: message::compact_warnings(warnings),
    })
}

pub fn leave_group_via_im_core(
    resolved: &Resolved,
    manager: &Manager,
    client: &im_core::ImClient,
    request: message::GroupLeaveRequest,
) -> Result<message::CommandResult, MessageError> {
    if request.group.trim().is_empty() {
        return Err(MessageError::GroupRequired);
    }
    if request.e2ee {
        return Err(MessageError::GroupNotSupported);
    }
    let record = message::require_active_identity(resolved, manager, &request.identity_name)?;
    let cached_snapshot = message::cached_group_snapshot(resolved, &record, &request.group);
    if cached_snapshot
        .as_ref()
        .is_some_and(message::is_active_group_owner)
    {
        return Err(MessageError::GroupOwnerCannotLeave);
    }
    if cached_snapshot
        .as_ref()
        .is_some_and(group_snapshot_uses_e2ee)
    {
        return Err(MessageError::GroupNotSupported);
    }
    let bridge_result = im_core::compat::groups::leave_group_with_bridge(
        client,
        group_session_provider(resolved, manager, client, &record),
        group_transport(resolved, manager, &record, Profile::RpcDefault),
        im_core::compat::groups::GroupLeaveBridgeRequest {
            request: GroupLeaveRequest {
                group: GroupRef::parse(&request.group).map_err(im_error_to_message_error)?,
            },
            credentials: group_lifecycle_credentials(&record),
        },
    )
    .map_err(im_error_to_message_error)?;
    let raw = bridge_result.raw;
    let mut warnings = group_control_warnings(resolved, bridge_result.warnings);
    warnings.extend(message::mark_cached_group_left(
        resolved,
        &record,
        &request.group,
    ));
    Ok(message::CommandResult {
        data: json!({
            "delivery": raw,
            "group": request.group,
        }),
        summary: format!("Left group {}", request.group),
        warnings: message::compact_warnings(warnings),
    })
}

pub fn add_group_member_via_im_core(
    resolved: &Resolved,
    manager: &Manager,
    client: &im_core::ImClient,
    request: message::GroupMemberRequest,
) -> Result<message::CommandResult, MessageError> {
    mutate_group_member_via_im_core(resolved, manager, client, request, "add")
}

pub fn remove_group_member_via_im_core(
    resolved: &Resolved,
    manager: &Manager,
    client: &im_core::ImClient,
    request: message::GroupMemberRequest,
) -> Result<message::CommandResult, MessageError> {
    mutate_group_member_via_im_core(resolved, manager, client, request, "remove")
}

fn mutate_group_member_via_im_core(
    resolved: &Resolved,
    manager: &Manager,
    client: &im_core::ImClient,
    request: message::GroupMemberRequest,
    action: &str,
) -> Result<message::CommandResult, MessageError> {
    if request.group.trim().is_empty() {
        return Err(MessageError::GroupRequired);
    }
    if request.member.trim().is_empty() {
        return Err(MessageError::MemberRequired);
    }
    if request.e2ee {
        return Err(MessageError::GroupNotSupported);
    }
    let record = message::require_active_identity(resolved, manager, &request.identity_name)?;
    let pre_mutation_snapshot = message::cached_group_snapshot(resolved, &record, &request.group);
    if pre_mutation_snapshot
        .as_ref()
        .is_some_and(group_snapshot_uses_e2ee)
    {
        return Err(MessageError::GroupNotSupported);
    }
    let member = resolve_group_member_via_directory(resolved, client, &request.member)?;
    let sdk_request = GroupMemberMutationRequest {
        group: GroupRef::parse(&request.group).map_err(im_error_to_message_error)?,
        member: Did::parse(&member.did).map_err(im_error_to_message_error)?,
        role: optional_string(&request.role),
        reason_text: optional_string(&request.reason_text),
    };
    let bridge_request = im_core::compat::groups::GroupMemberMutationBridgeRequest {
        request: sdk_request,
        credentials: group_lifecycle_credentials(&record),
    };
    let bridge_result = if action == "add" {
        im_core::compat::groups::add_group_member_with_bridge(
            client,
            group_session_provider(resolved, manager, client, &record),
            group_transport(resolved, manager, &record, Profile::RpcDefault),
            bridge_request,
        )
    } else {
        im_core::compat::groups::remove_group_member_with_bridge(
            client,
            group_session_provider(resolved, manager, client, &record),
            group_transport(resolved, manager, &record, Profile::RpcDefault),
            bridge_request,
        )
    }
    .map_err(im_error_to_message_error)?;
    let raw = bridge_result.raw;
    let mut warnings = group_control_warnings(resolved, bridge_result.warnings);
    warnings.extend(message::sync_group_state(
        resolved,
        manager,
        &record,
        &request.group,
        true,
    ));
    let snapshot = message::cached_group_snapshot(resolved, &record, &request.group)
        .or_else(|| message::normalize_group_snapshot(&raw))
        .unwrap_or_else(|| json!({ "group_did": request.group }));
    let members =
        message::cached_group_members(resolved, &record, &request.group, 100).unwrap_or_default();
    Ok(message::CommandResult {
        data: json!({
            "group": snapshot,
            "members": members,
            "delivery": raw,
            "member": {
                "did": member.did,
                "handle": member.handle,
            },
        }),
        summary: format!("Updated group membership via {action}"),
        warnings: message::compact_warnings(warnings),
    })
}

pub fn update_group_via_im_core(
    resolved: &Resolved,
    manager: &Manager,
    client: &im_core::ImClient,
    request: message::GroupUpdateRequest,
) -> Result<message::CommandResult, MessageError> {
    if request.group.trim().is_empty() {
        return Err(MessageError::GroupRequired);
    }
    let profile_patch = group_profile_patch(&request);
    let policy_patch = group_policy_patch(&request);
    if profile_patch == GroupProfilePatch::default() && policy_patch == GroupPolicyPatch::default()
    {
        return Err(MessageError::Internal(
            "group update requires at least one mutable field".to_string(),
        ));
    }
    let record = message::require_active_identity(resolved, manager, &request.identity_name)?;
    let cached_snapshot = message::cached_group_snapshot(resolved, &record, &request.group);
    if cached_snapshot
        .as_ref()
        .is_some_and(group_snapshot_uses_e2ee)
    {
        return Err(MessageError::GroupNotSupported);
    }
    let group = GroupRef::parse(&request.group).map_err(im_error_to_message_error)?;
    let mut responses = Vec::new();
    let mut warnings = Vec::new();
    if profile_patch != GroupProfilePatch::default() {
        let bridge_result = im_core::compat::groups::update_group_profile_with_bridge(
            client,
            group_session_provider(resolved, manager, client, &record),
            group_transport(resolved, manager, &record, Profile::RpcDefault),
            im_core::compat::groups::GroupUpdateProfileBridgeRequest {
                request: GroupUpdateProfileRequest {
                    group: group.clone(),
                    patch: profile_patch,
                },
                credentials: group_lifecycle_credentials(&record),
            },
        )
        .map_err(im_error_to_message_error)?;
        warnings.extend(bridge_result.warnings);
        responses.push(bridge_result.raw);
    }
    if policy_patch != GroupPolicyPatch::default() {
        let bridge_result = im_core::compat::groups::update_group_policy_with_bridge(
            client,
            group_session_provider(resolved, manager, client, &record),
            group_transport(resolved, manager, &record, Profile::RpcDefault),
            im_core::compat::groups::GroupUpdatePolicyBridgeRequest {
                request: GroupUpdatePolicyRequest {
                    group,
                    patch: policy_patch,
                },
                credentials: group_lifecycle_credentials(&record),
            },
        )
        .map_err(im_error_to_message_error)?;
        warnings.extend(bridge_result.warnings);
        responses.push(bridge_result.raw);
    }
    let mut warnings = group_control_warnings(resolved, warnings);
    warnings.extend(message::sync_group_state(
        resolved,
        manager,
        &record,
        &request.group,
        false,
    ));
    let snapshot = message::cached_group_snapshot(resolved, &record, &request.group)
        .unwrap_or_else(|| json!({ "group_did": request.group }));
    Ok(message::CommandResult {
        data: json!({
            "group": snapshot,
            "delivery": responses,
        }),
        summary: format!("Updated group {}", request.group),
        warnings: message::compact_warnings(warnings),
    })
}

pub fn get_group_via_im_core(
    resolved: &Resolved,
    manager: &Manager,
    client: &im_core::ImClient,
    identity_name: &str,
    group: String,
) -> Result<message::CommandResult, MessageError> {
    let record = message::require_active_identity(resolved, manager, identity_name)?;
    let group_ref = GroupRef::parse(&group).map_err(im_error_to_message_error)?;
    let bridge_result = im_core::compat::groups::get_group_with_bridge(
        client,
        group_session_provider(resolved, manager, client, &record),
        group_transport(resolved, manager, &record, Profile::RpcReadHeavy),
        im_core::compat::groups::GroupGetBridgeRequest { group: group_ref },
    )
    .map_err(im_error_to_message_error)?;
    let raw = bridge_result.raw;
    let mut warnings = group_control_warnings(resolved, bridge_result.warnings);
    warnings.extend(message::persist_group_snapshot(resolved, &record, &raw));
    let snapshot = message::cached_group_snapshot(resolved, &record, &group)
        .or_else(|| message::normalize_group_snapshot(&raw))
        .unwrap_or(Value::Null);
    Ok(message::CommandResult {
        data: json!({
            "group": snapshot,
            "source": message::group_control_source(&raw),
        }),
        summary: "Loaded group snapshot".to_string(),
        warnings: message::compact_warnings(warnings),
    })
}

pub fn list_groups_via_im_core(
    resolved: &Resolved,
    manager: &Manager,
    client: &im_core::ImClient,
    identity_name: &str,
    limit: i64,
) -> Result<message::CommandResult, MessageError> {
    let record = message::require_active_identity(resolved, manager, identity_name)?;
    let request = GroupListRequest {
        limit: page_limit(limit, 50)?,
    };
    let bridge_result = im_core::compat::groups::list_groups_with_bridge(
        client,
        group_session_provider(resolved, manager, client, &record),
        group_transport(resolved, manager, &record, Profile::RpcReadHeavy),
        im_core::compat::groups::GroupListBridgeRequest { request },
    )
    .map_err(im_error_to_message_error)?;
    let raw = bridge_result.raw;
    let groups = message::values_from_array(raw.get("groups"));
    let total = message::int_value(raw.get("total"), groups.len() as i64);
    Ok(message::CommandResult {
        data: json!({
            "groups": groups,
            "total": total,
            "source": message::group_control_source(&raw),
        }),
        summary: format!("Loaded {total} groups"),
        warnings: group_control_warnings(resolved, bridge_result.warnings),
    })
}

pub fn group_members_via_im_core(
    resolved: &Resolved,
    manager: &Manager,
    client: &im_core::ImClient,
    identity_name: &str,
    group: String,
    limit: i64,
) -> Result<message::CommandResult, MessageError> {
    let record = message::require_active_identity(resolved, manager, identity_name)?;
    let request = GroupMembersRequest {
        group: GroupRef::parse(&group).map_err(im_error_to_message_error)?,
        limit: page_limit(limit, 100)?,
    };
    let bridge_result = im_core::compat::groups::list_group_members_with_bridge(
        client,
        group_session_provider(resolved, manager, client, &record),
        group_transport(resolved, manager, &record, Profile::RpcReadHeavy),
        im_core::compat::groups::GroupMembersBridgeRequest { request },
    )
    .map_err(im_error_to_message_error)?;
    let raw = bridge_result.raw;
    let mut warnings = group_control_warnings(resolved, bridge_result.warnings);
    warnings.extend(message::persist_group_members(
        resolved, &record, &group, &raw,
    ));
    let members = message::cached_group_members(resolved, &record, &group, limit)
        .filter(|items| !items.is_empty())
        .unwrap_or_else(|| message::values_from_array(raw.get("members")));
    let total = message::int_value(raw.get("total"), members.len() as i64);
    Ok(message::CommandResult {
        data: json!({
            "group": group,
            "members": members,
            "total": total,
            "source": message::group_control_source(&raw),
        }),
        summary: format!("Loaded {total} group members"),
        warnings: message::compact_warnings(warnings),
    })
}

pub fn group_messages_via_im_core(
    resolved: &Resolved,
    manager: &Manager,
    client: &im_core::ImClient,
    identity_name: &str,
    group: String,
    limit: i64,
    cursor: String,
) -> Result<message::CommandResult, MessageError> {
    let record = message::require_active_identity(resolved, manager, identity_name)?;
    let source_mode = message::runtime_mode(resolved);
    let request = GroupMessagesRequest {
        group: GroupRef::parse(&group).map_err(im_error_to_message_error)?,
        limit: page_limit(limit, 50)?,
        cursor: optional_cursor(&cursor)?,
    };
    let (mut raw, mut warnings, result_source_mode) = if source_mode
        == runtime::bridge::MODE_WEBSOCKET
    {
        match group_messages_websocket(resolved, manager, client, &record, request.clone())? {
            GroupMessagesOutcome::Remote {
                raw,
                warnings,
                source_mode,
            } => (raw, warnings, source_mode),
            GroupMessagesOutcome::LocalCache(result) => return Ok(result),
        }
    } else {
        let bridge_result = list_group_messages_http(resolved, manager, client, &record, request)?;
        (bridge_result.raw, bridge_result.warnings, source_mode)
    };

    warnings.extend(message::maybe_decrypt_group_messages(
        resolved, &record, &group, &mut raw,
    ));
    warnings.extend(message::persist_group_messages(
        resolved, &record, &group, &raw,
    ));
    let messages = message::cached_group_messages(resolved, &record, &group, limit, &cursor)
        .filter(|items| !items.is_empty())
        .unwrap_or_else(|| message::values_from_array(raw.get("messages")));
    let total = message::int_value(raw.get("total"), messages.len() as i64);
    Ok(message::CommandResult {
        data: json!({
            "group": group,
            "messages": messages,
            "total": total,
            "has_more": message::bool_value(raw.get("has_more")),
            "next_since_seq": raw.get("next_since_seq").cloned().unwrap_or(Value::Null),
            "source": source_with_default_for_mode(&raw, result_source_mode),
        }),
        summary: format!("Loaded {total} group messages"),
        warnings: message::compact_warnings(warnings),
    })
}

enum GroupMessagesOutcome {
    Remote {
        raw: Value,
        warnings: Vec<String>,
        source_mode: &'static str,
    },
    LocalCache(message::CommandResult),
}

fn group_messages_websocket(
    resolved: &Resolved,
    manager: &Manager,
    client: &im_core::ImClient,
    record: &StoredIdentity,
    request: GroupMessagesRequest,
) -> Result<GroupMessagesOutcome, MessageError> {
    let legacy_request = message::GroupMessagesRequest {
        identity_name: record.identity_name.clone(),
        group: request.group.as_str().to_string(),
        limit: i64::from(request.limit.0),
        cursor: request
            .cursor
            .as_ref()
            .map(|cursor| cursor.as_str().to_string())
            .unwrap_or_default(),
        skip: 0,
    };
    let bridge = WSProxyTransport::new(resolved, &record.identity_name);
    match bridge.list_group_messages(legacy_request.clone()) {
        Ok(result) => Ok(GroupMessagesOutcome::Remote {
            raw: Value::Object(result),
            warnings: Vec::new(),
            source_mode: runtime::bridge::MODE_WEBSOCKET,
        }),
        Err(bridge_err) => {
            if let Some(cached) = message::cached_group_messages(
                resolved,
                record,
                &legacy_request.group,
                legacy_request.limit,
                &legacy_request.cursor,
            )
            .filter(|items| !items.is_empty())
            {
                return Ok(GroupMessagesOutcome::LocalCache(
                    group_messages_local_cache_result(&legacy_request, cached, &bridge_err),
                ));
            }
            match list_group_messages_http(resolved, manager, client, record, request) {
                Ok(result) => {
                    crate::traceutil::mark_fallback(
                        "websocket_to_http",
                        Some(&bridge_err.to_string()),
                    );
                    Ok(GroupMessagesOutcome::Remote {
                        raw: result.raw,
                        warnings: vec![message::websocket_http_fallback_warning(Some(&bridge_err))],
                        source_mode: runtime::bridge::MODE_HTTP,
                    })
                }
                Err(_) => Err(bridge_err),
            }
        }
    }
}

fn list_group_messages_http(
    resolved: &Resolved,
    manager: &Manager,
    client: &im_core::ImClient,
    record: &StoredIdentity,
    request: GroupMessagesRequest,
) -> Result<im_core::groups::GroupReadResult, MessageError> {
    im_core::compat::groups::list_group_messages_with_bridge(
        client,
        group_session_provider(resolved, manager, client, record),
        group_transport(resolved, manager, record, Profile::RpcReadHeavy),
        im_core::compat::groups::GroupMessagesBridgeRequest { request },
    )
    .map_err(im_error_to_message_error)
}

fn group_session_provider<'a>(
    resolved: &'a Resolved,
    manager: &'a Manager,
    client: &im_core::ImClient,
    record: &StoredIdentity,
) -> GroupReadSessionProvider<'a> {
    GroupReadSessionProvider {
        subject: client.did().clone(),
        resolved,
        manager,
        record: record.clone(),
    }
}

fn group_transport<'a>(
    resolved: &'a Resolved,
    manager: &'a Manager,
    record: &StoredIdentity,
    profile: Profile,
) -> GroupReadLegacyTransport<'a> {
    GroupReadLegacyTransport {
        resolved,
        manager,
        record: record.clone(),
        profile,
    }
}

fn group_lifecycle_credentials(
    record: &StoredIdentity,
) -> im_core::compat::groups::GroupLifecycleCredentials {
    im_core::compat::groups::GroupLifecycleCredentials {
        identity_name: record.identity_name.clone(),
        did_document: record.did_document.clone(),
        key1_private_pem: record.key1_private_pem.clone(),
    }
}

fn group_create_request(
    request: message::GroupCreateRequest,
    service_did: &str,
) -> Result<GroupCreateRequest, MessageError> {
    let service_did = service_did.trim();
    if service_did.is_empty() {
        return Err(MessageError::MissingMessageServiceDid);
    }
    Ok(GroupCreateRequest {
        name: request.name,
        description: optional_string(&request.description),
        discoverability: optional_string(&request.discoverability),
        admission_mode: optional_string(&request.admission_mode),
        message_security_profile: optional_string(&request.message_security_profile),
        e2ee: request.e2ee,
        slug: optional_string(&request.slug),
        goal: optional_string(&request.goal),
        rules: optional_string(&request.rules),
        message_prompt: optional_string(&request.message_prompt),
        doc_url: optional_string(&request.doc_url),
        attachments_allowed: request.attachments_allowed,
        max_members: optional_string(&request.max_members),
        member_max_messages: request.member_max_messages,
        member_max_total_chars: request.member_max_total_chars,
        service_did: Did::parse(service_did).map_err(im_error_to_message_error)?,
    })
}

fn resolve_group_member_via_directory(
    resolved: &Resolved,
    client: &im_core::ImClient,
    member: &str,
) -> Result<message::TargetResolution, MessageError> {
    let member = member.trim();
    if member.is_empty() {
        return Err(MessageError::MemberRequired);
    }
    if member.starts_with("did:") {
        return Ok(message::TargetResolution {
            did: member.to_string(),
            handle: String::new(),
        });
    }
    let handle = Handle::parse(member, &resolved.did_domain).map_err(im_error_to_message_error)?;
    let lookup = im_core::compat::directory::lookup_handle_with_bridge(
        client,
        handle.clone(),
        DirectoryLegacyTransport { resolved },
    )
    .map_err(im_error_to_message_error)?;
    Ok(message::TargetResolution {
        did: lookup.did.as_str().to_string(),
        handle: normalize_handle_value(lookup.handle.as_str()),
    })
}

fn group_profile_patch(request: &message::GroupUpdateRequest) -> GroupProfilePatch {
    GroupProfilePatch {
        name: optional_string(&request.name),
        description: optional_string(&request.description),
        discoverability: optional_string(&request.discoverability),
        slug: optional_string(&request.slug),
        goal: optional_string(&request.goal),
        rules: optional_string(&request.rules),
        message_prompt: optional_string(&request.message_prompt),
        doc_url: optional_string(&request.doc_url),
    }
}

fn group_policy_patch(request: &message::GroupUpdateRequest) -> GroupPolicyPatch {
    GroupPolicyPatch {
        admission_mode: optional_string(&request.admission_mode),
        attachments_allowed: request.attachments_allowed,
        max_members: optional_string(&request.max_members),
        member_max_messages: request.member_max_messages,
        member_max_total_chars: request.member_max_total_chars,
    }
}

fn group_messages_local_cache_result(
    request: &message::GroupMessagesRequest,
    messages: Vec<Value>,
    bridge_err: &MessageError,
) -> message::CommandResult {
    let total = messages.len();
    message::CommandResult {
        data: json!({
            "group": request.group,
            "messages": messages,
            "total": total,
            "source": "local_ws_cache_fallback",
        }),
        summary: "Loaded group messages from local cache".to_string(),
        warnings: vec![message::websocket_cache_fallback_warning(Some(bridge_err))],
    }
}

struct GroupReadSessionProvider<'a> {
    subject: im_core::prelude::Did,
    resolved: &'a Resolved,
    manager: &'a Manager,
    record: StoredIdentity,
}

impl im_core::compat::groups::BridgeGroupSessionProvider for GroupReadSessionProvider<'_> {
    fn ensure_group_messaging_session(&self) -> im_core::ImResult<SessionBundle> {
        let session = message::auth_session(self.resolved, self.manager, &self.record)
            .map_err(message_error_to_im_error)?;
        Ok(SessionBundle {
            subject: self.subject.clone(),
            scope: AuthScope::GroupMessaging,
            expires_at: None,
            refreshed: session.current_jwt().trim() != self.record.jwt_token.trim(),
        })
    }
}

struct GroupReadLegacyTransport<'a> {
    resolved: &'a Resolved,
    manager: &'a Manager,
    record: StoredIdentity,
    profile: Profile,
}

struct DirectoryLegacyTransport<'a> {
    resolved: &'a Resolved,
}

impl im_core::compat::groups::BridgeAuthenticatedRpcTransport for GroupReadLegacyTransport<'_> {
    fn authenticated_rpc(
        &mut self,
        endpoint: &str,
        method: &str,
        params: Value,
    ) -> im_core::ImResult<Value> {
        send_authenticated_group_read_rpc_with_fallback(
            self.resolved,
            self.manager,
            &self.record,
            self.profile,
            endpoint,
            method,
            params,
        )
        .map_err(message_error_to_im_error)
    }
}

impl im_core::compat::directory::BridgeDirectoryRpcTransport for DirectoryLegacyTransport<'_> {
    fn rpc(&mut self, endpoint: &str, method: &str, params: Value) -> im_core::ImResult<Value> {
        let client = crate::identity::client::Client::new(self.resolved)
            .map_err(identity_error_to_im_error)?;
        let profile = match method {
            "get_public_profile" => Profile::RpcReadHeavy,
            _ => Profile::RpcDefault,
        };
        client
            .rpc_call_profile(profile, endpoint, method, params)
            .map_err(identity_error_to_im_error)
    }
}

fn send_authenticated_group_read_rpc_with_fallback(
    resolved: &Resolved,
    manager: &Manager,
    record: &StoredIdentity,
    profile: Profile,
    endpoint: &str,
    method: &str,
    params: Value,
) -> Result<Value, MessageError> {
    match send_authenticated_group_read_rpc(
        resolved,
        manager,
        record,
        profile,
        endpoint,
        method,
        params.clone(),
    ) {
        Ok(result) => Ok(result),
        Err(err) if message::is_session_unauthorized(&err) => {
            let refreshed = message::refresh_jwt_fallback(resolved, manager, record).ok();
            match send_authenticated_group_read_rpc(
                resolved,
                manager,
                refreshed.as_ref().unwrap_or(record),
                profile,
                endpoint,
                method,
                params,
            ) {
                Ok(result) => Ok(result),
                Err(_) => Err(err),
            }
        }
        Err(err) => Err(err),
    }
}

fn send_authenticated_group_read_rpc(
    resolved: &Resolved,
    manager: &Manager,
    record: &StoredIdentity,
    profile: Profile,
    endpoint: &str,
    method: &str,
    params: Value,
) -> Result<Value, MessageError> {
    let mut auth = auth_session(resolved, manager, record)?;
    let client = message::Client::new(resolved)?;
    client.authenticated_rpc_call_profile(profile, endpoint, method, params, &mut auth)
}

fn auth_session(
    resolved: &Resolved,
    manager: &Manager,
    record: &StoredIdentity,
) -> Result<Session, MessageError> {
    message::auth_session(resolved, manager, record)
}

fn group_control_warnings(resolved: &Resolved, mut warnings: Vec<String>) -> Vec<String> {
    warnings.extend(message::group_control_warnings(resolved));
    message::compact_warnings(warnings)
}

fn source_with_default_for_mode(raw: &Value, mode: &str) -> String {
    raw.get("source")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(if mode == runtime::bridge::MODE_WEBSOCKET {
            "local_ws_cache"
        } else {
            "remote_http"
        })
        .to_string()
}

fn page_limit(value: i64, fallback: u32) -> Result<PageLimit, MessageError> {
    let value = if value <= 0 {
        fallback
    } else {
        u32::try_from(value).map_err(|_| MessageError::Json("limit is too large".to_string()))?
    };
    PageLimit::new(value).map_err(im_error_to_message_error)
}

fn optional_cursor(value: &str) -> Result<Option<Cursor>, MessageError> {
    if value.trim().is_empty() {
        return Ok(None);
    }
    Cursor::parse(value)
        .map(Some)
        .map_err(im_error_to_message_error)
}

fn optional_string(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn default_string(value: &str, fallback: &str) -> String {
    if value.trim().is_empty() {
        fallback.to_string()
    } else {
        value.to_string()
    }
}

fn group_snapshot_uses_e2ee(snapshot: &Value) -> bool {
    if snapshot.is_null() {
        return false;
    }
    if value_string(snapshot.get("message_security_profile"))
        == message::GROUP_E2EE_SECURITY_PROFILE
    {
        return true;
    }
    if snapshot
        .get("group_policy")
        .and_then(Value::as_object)
        .map(|policy| value_string(policy.get("message_security_profile")))
        .is_some_and(|profile| profile == message::GROUP_E2EE_SECURITY_PROFILE)
    {
        return true;
    }
    decoded_metadata(snapshot)
        .as_ref()
        .map(|metadata| value_string(metadata.get("message_security_profile")))
        .is_some_and(|profile| profile == message::GROUP_E2EE_SECURITY_PROFILE)
}

fn decoded_metadata(snapshot: &Value) -> Option<Value> {
    snapshot
        .get("metadata")
        .and_then(Value::as_str)
        .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
}

fn value_string(value: Option<&Value>) -> String {
    value
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn normalize_handle_value(value: &str) -> String {
    let value = value.trim().to_ascii_lowercase();
    if value.is_empty() {
        return String::new();
    }
    let value = value.trim_start_matches("wba://");
    match value.find('.') {
        Some(index) if index > 0 => value[..index].to_string(),
        _ => value.to_string(),
    }
}

fn im_error_to_message_error(err: im_core::ImError) -> MessageError {
    match err {
        im_core::ImError::InvalidInput { field, .. } if field.as_deref() == Some("group") => {
            MessageError::GroupRequired
        }
        im_core::ImError::GroupNotFound { .. } => MessageError::GroupRequired,
        im_core::ImError::AuthRequired | im_core::ImError::SessionExpired => {
            MessageError::IdentityRequired("authentication is required".to_string())
        }
        im_core::ImError::IdentityNotReady { identity, missing } => MessageError::IdentityRequired(
            format!("identity {identity} is not ready: {}", missing.join(", ")),
        ),
        im_core::ImError::Service {
            status_code,
            code,
            message,
        } => MessageError::Service(crate::identity::wire::ServiceError {
            status_code: status_code.unwrap_or_default(),
            rpc_code: code
                .and_then(|value| value.parse().ok())
                .unwrap_or_default(),
            message,
            data: None,
        }),
        im_core::ImError::TransportUnavailable { detail } => {
            MessageError::TransportUnavailable(detail)
        }
        err => MessageError::Internal(err.to_string()),
    }
}

fn message_error_to_im_error(err: MessageError) -> im_core::ImError {
    match err {
        MessageError::Service(service_err) => im_core::ImError::Service {
            status_code: (service_err.status_code != 0).then_some(service_err.status_code),
            code: (service_err.rpc_code != 0).then(|| service_err.rpc_code.to_string()),
            message: service_err.message,
        },
        MessageError::TransportUnavailable(detail) => {
            im_core::ImError::TransportUnavailable { detail }
        }
        MessageError::GroupRequired => {
            im_core::ImError::invalid_input(Some("group".to_string()), "group target is required")
        }
        MessageError::IdentityRequired(message) => im_core::ImError::IdentityNotReady {
            identity: message,
            missing: Vec::new(),
        },
        err => im_core::ImError::Internal {
            message: err.to_string(),
        },
    }
}

fn identity_error_to_im_error(err: crate::identity::IdentityError) -> im_core::ImError {
    match err {
        crate::identity::IdentityError::InvalidInput(message) => {
            im_core::ImError::invalid_input(None, message)
        }
        crate::identity::IdentityError::NotFound(message)
        | crate::identity::IdentityError::NoDefaultIdentity(message) => {
            im_core::ImError::IdentityNotFound { selector: message }
        }
        crate::identity::IdentityError::AuthRequired(_) => im_core::ImError::AuthRequired,
        crate::identity::IdentityError::Service(service) => im_core::ImError::Service {
            status_code: (service.status_code != 0).then_some(service.status_code),
            code: (service.rpc_code != 0).then(|| service.rpc_code.to_string()),
            message: service.message,
        },
        crate::identity::IdentityError::Io(err) => im_core::ImError::Io {
            detail: err.to_string(),
        },
        crate::identity::IdentityError::Json(err) => im_core::ImError::Serialization {
            detail: err.to_string(),
        },
        err => im_core::ImError::Internal {
            message: err.to_string(),
        },
    }
}
