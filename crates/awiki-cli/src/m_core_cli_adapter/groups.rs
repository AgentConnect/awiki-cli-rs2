use im_core::groups::{
    GroupAdmissionMode, GroupDiscoverability, GroupKeyPackagePurpose, GroupMemberLimit,
    GroupMemberRole, GroupMessageSecurityProfile, GroupReadResult, GroupSecurityRequirement,
};
use im_core::prelude::{
    Cursor, Did, GroupCreateRequest as SdkGroupCreateRequest,
    GroupJoinRequest as SdkGroupJoinRequest, GroupLeaveRequest as SdkGroupLeaveRequest,
    GroupListRequest, GroupMember, GroupMemberMutationRequest, GroupMemberRef,
    GroupMemberResolution, GroupMembersRequest, GroupMessagesRequest, GroupPolicyPatch,
    GroupProfilePatch, GroupRef, GroupSnapshot, GroupSummary,
    GroupUpdateRequest as SdkGroupUpdateRequest, Handle, Message, PageLimit,
};
use serde_json::{json, Value};

use crate::host_runtime;
use crate::m_core_cli_adapter::message_result::{CommandResult, MessageAdapterError, ServiceError};
use crate::workspace_config::Resolved;

pub const GROUP_E2EE_SECURITY_PROFILE: &str = "group-e2ee";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GroupCreateRequest {
    pub identity_name: String,
    pub name: String,
    pub description: String,
    pub avatar_uri: String,
    pub discoverability: String,
    pub admission_mode: String,
    pub message_security_profile: String,
    pub secure_required: bool,
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

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GroupJoinRequest {
    pub identity_name: String,
    pub group: String,
    pub reason_text: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GroupMemberRequest {
    pub identity_name: String,
    pub group: String,
    pub member: String,
    pub role: String,
    pub reason_text: String,
    pub secure_required: bool,
    pub e2ee: bool,
    pub leave_request_id: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GroupLeaveRequest {
    pub identity_name: String,
    pub group: String,
    pub reason_text: String,
    pub secure_required: bool,
    pub e2ee: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GroupUpdateRequest {
    pub identity_name: String,
    pub group: String,
    pub name: String,
    pub description: String,
    pub avatar_uri: String,
    pub discoverability: String,
    pub admission_mode: String,
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

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GroupKeyPackagePublishRequest {
    pub identity_name: String,
    pub group: String,
    pub purpose: String,
    pub device: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GroupE2eeProcessLeaveRequest {
    pub identity_name: String,
    pub group: String,
    pub member: String,
    pub leave_request_id: String,
    pub reason_text: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GroupE2eeMemberKeyRequest {
    pub identity_name: String,
    pub group: String,
    pub member: String,
    pub device: String,
}

fn group_raw_response(result: &im_core::groups::GroupReadResult) -> Value {
    result.response_json().cloned().unwrap_or(Value::Null)
}

pub fn create_group_via_im_core(
    resolved: &Resolved,
    client: &im_core::ImClient,
    request: GroupCreateRequest,
) -> Result<CommandResult, MessageAdapterError> {
    if request.name.trim().is_empty() {
        return Err(MessageAdapterError::GroupRequired);
    }
    let result = client
        .groups()
        .create(group_create_request(request)?)
        .map_err(im_error_to_message_error)?;
    let raw = group_raw_response(&result);
    let group_did = group_did_from_result(&result, &raw).unwrap_or_default();
    let warnings = group_control_warnings(resolved, result.warnings.clone());
    let snapshot = group_snapshot_result_json(&result, &raw).unwrap_or(Value::Null);
    let members = group_members_to_cli_json(&result, &raw);
    Ok(CommandResult {
        data: json!({
            "group": snapshot,
            "members": members,
            "delivery": raw,
            "source": group_control_source(&raw),
        }),
        summary: format!("Created group {group_did}"),
        warnings: compact_warnings(warnings),
    })
}

pub async fn create_group_via_im_core_async(
    resolved: &Resolved,
    client: &im_core::ImClient,
    request: GroupCreateRequest,
) -> Result<CommandResult, MessageAdapterError> {
    if request.name.trim().is_empty() {
        return Err(MessageAdapterError::GroupRequired);
    }
    let result = client
        .groups()
        .create_async(group_create_request(request)?)
        .await
        .map_err(im_error_to_message_error)?;
    let raw = group_raw_response(&result);
    let group_did = group_did_from_result(&result, &raw).unwrap_or_default();
    let warnings = group_control_warnings(resolved, result.warnings.clone());
    let snapshot = group_snapshot_result_json(&result, &raw).unwrap_or(Value::Null);
    let members = group_members_to_cli_json(&result, &raw);
    Ok(CommandResult {
        data: json!({
            "group": snapshot,
            "members": members,
            "delivery": raw,
            "source": group_control_source(&raw),
        }),
        summary: format!("Created group {group_did}"),
        warnings: compact_warnings(warnings),
    })
}

pub fn join_group_via_im_core(
    resolved: &Resolved,
    client: &im_core::ImClient,
    request: GroupJoinRequest,
) -> Result<CommandResult, MessageAdapterError> {
    if request.group.trim().is_empty() {
        return Err(MessageAdapterError::GroupRequired);
    }
    let requested_group = request.group.clone();
    let result = client
        .groups()
        .join(SdkGroupJoinRequest {
            group: GroupRef::parse(&request.group).map_err(im_error_to_message_error)?,
            member_handle: None,
            reason_text: optional_string(&request.reason_text),
        })
        .map_err(im_error_to_message_error)?;
    let raw = group_raw_response(&result);
    let group_did = group_did_from_result(&result, &raw).unwrap_or(requested_group);
    let warnings = group_control_warnings(resolved, result.warnings.clone());
    let snapshot = group_snapshot_result_json(&result, &raw)
        .unwrap_or_else(|| json!({ "group_did": group_did }));
    Ok(CommandResult {
        data: json!({
            "group": snapshot,
            "delivery": raw,
            "source": group_control_source(&raw),
        }),
        summary: format!("Joined group {group_did}"),
        warnings: compact_warnings(warnings),
    })
}

pub async fn join_group_via_im_core_async(
    resolved: &Resolved,
    client: &im_core::ImClient,
    request: GroupJoinRequest,
) -> Result<CommandResult, MessageAdapterError> {
    if request.group.trim().is_empty() {
        return Err(MessageAdapterError::GroupRequired);
    }
    let requested_group = request.group.clone();
    let result = client
        .groups()
        .join_async(SdkGroupJoinRequest {
            group: GroupRef::parse(&request.group).map_err(im_error_to_message_error)?,
            member_handle: None,
            reason_text: optional_string(&request.reason_text),
        })
        .await
        .map_err(im_error_to_message_error)?;
    let raw = group_raw_response(&result);
    let group_did = group_did_from_result(&result, &raw).unwrap_or(requested_group);
    let warnings = group_control_warnings(resolved, result.warnings.clone());
    let snapshot = group_snapshot_result_json(&result, &raw)
        .unwrap_or_else(|| json!({ "group_did": group_did }));
    Ok(CommandResult {
        data: json!({
            "group": snapshot,
            "delivery": raw,
            "source": group_control_source(&raw),
        }),
        summary: format!("Joined group {group_did}"),
        warnings: compact_warnings(warnings),
    })
}

pub fn leave_group_via_im_core(
    resolved: &Resolved,
    client: &im_core::ImClient,
    request: GroupLeaveRequest,
) -> Result<CommandResult, MessageAdapterError> {
    if request.group.trim().is_empty() {
        return Err(MessageAdapterError::GroupRequired);
    }
    let result = client
        .groups()
        .leave(SdkGroupLeaveRequest {
            group: GroupRef::parse(&request.group).map_err(im_error_to_message_error)?,
            reason_text: optional_string(&request.reason_text),
            security: group_security_requirement(request.secure_required || request.e2ee),
        })
        .map_err(im_error_to_message_error)?;
    let raw = group_raw_response(&result);
    let warnings = group_control_warnings(resolved, result.warnings);
    Ok(CommandResult {
        data: json!({
            "delivery": raw,
            "group": request.group,
        }),
        summary: format!("Left group {}", request.group),
        warnings: compact_warnings(warnings),
    })
}

pub async fn leave_group_via_im_core_async(
    resolved: &Resolved,
    client: &im_core::ImClient,
    request: GroupLeaveRequest,
) -> Result<CommandResult, MessageAdapterError> {
    if request.group.trim().is_empty() {
        return Err(MessageAdapterError::GroupRequired);
    }
    let result = client
        .groups()
        .leave_async(SdkGroupLeaveRequest {
            group: GroupRef::parse(&request.group).map_err(im_error_to_message_error)?,
            reason_text: optional_string(&request.reason_text),
            security: group_security_requirement(request.secure_required || request.e2ee),
        })
        .await
        .map_err(im_error_to_message_error)?;
    let raw = group_raw_response(&result);
    let warnings = group_control_warnings(resolved, result.warnings);
    Ok(CommandResult {
        data: json!({
            "delivery": raw,
            "group": request.group,
        }),
        summary: format!("Left group {}", request.group),
        warnings: compact_warnings(warnings),
    })
}

pub fn add_group_member_via_im_core(
    resolved: &Resolved,
    client: &im_core::ImClient,
    request: GroupMemberRequest,
) -> Result<CommandResult, MessageAdapterError> {
    mutate_group_member_via_im_core(resolved, client, request, "add")
}

pub async fn add_group_member_via_im_core_async(
    resolved: &Resolved,
    client: &im_core::ImClient,
    request: GroupMemberRequest,
) -> Result<CommandResult, MessageAdapterError> {
    mutate_group_member_via_im_core_async(resolved, client, request, "add").await
}

pub fn remove_group_member_via_im_core(
    resolved: &Resolved,
    client: &im_core::ImClient,
    request: GroupMemberRequest,
) -> Result<CommandResult, MessageAdapterError> {
    mutate_group_member_via_im_core(resolved, client, request, "remove")
}

pub async fn remove_group_member_via_im_core_async(
    resolved: &Resolved,
    client: &im_core::ImClient,
    request: GroupMemberRequest,
) -> Result<CommandResult, MessageAdapterError> {
    mutate_group_member_via_im_core_async(resolved, client, request, "remove").await
}

fn mutate_group_member_via_im_core(
    resolved: &Resolved,
    client: &im_core::ImClient,
    request: GroupMemberRequest,
    action: &str,
) -> Result<CommandResult, MessageAdapterError> {
    if request.group.trim().is_empty() {
        return Err(MessageAdapterError::GroupRequired);
    }
    if request.member.trim().is_empty() {
        return Err(MessageAdapterError::MemberRequired);
    }
    let member = GroupMemberRef::parse(&request.member, &resolved.did_domain)
        .map_err(im_error_to_message_error)?;
    let sdk_request = GroupMemberMutationRequest {
        group: GroupRef::parse(&request.group).map_err(im_error_to_message_error)?,
        member,
        role: GroupMemberRole::parse_optional(&request.role).map_err(im_error_to_message_error)?,
        reason_text: optional_string(&request.reason_text),
        leave_request_id: optional_string(&request.leave_request_id),
        security: group_security_requirement(request.secure_required || request.e2ee),
    };
    let result = if action == "add" {
        client.groups().add_member(sdk_request)
    } else {
        client.groups().remove_member(sdk_request)
    }
    .map_err(im_error_to_message_error)?;
    let raw = group_raw_response(&result);
    let warnings = group_control_warnings(resolved, result.warnings.clone());
    let snapshot = group_snapshot_result_json(&result, &raw)
        .unwrap_or_else(|| json!({ "group_did": request.group }));
    let members = group_members_to_cli_json(&result, &raw);
    let resolved_member = group_member_resolution_json(result.resolved_member.as_ref());
    Ok(CommandResult {
        data: json!({
            "group": snapshot,
            "members": members,
            "delivery": raw,
            "member": resolved_member,
        }),
        summary: format!("Updated group membership via {action}"),
        warnings: compact_warnings(warnings),
    })
}

async fn mutate_group_member_via_im_core_async(
    resolved: &Resolved,
    client: &im_core::ImClient,
    request: GroupMemberRequest,
    action: &str,
) -> Result<CommandResult, MessageAdapterError> {
    if request.group.trim().is_empty() {
        return Err(MessageAdapterError::GroupRequired);
    }
    if request.member.trim().is_empty() {
        return Err(MessageAdapterError::MemberRequired);
    }
    let member = GroupMemberRef::parse(&request.member, &resolved.did_domain)
        .map_err(im_error_to_message_error)?;
    let sdk_request = GroupMemberMutationRequest {
        group: GroupRef::parse(&request.group).map_err(im_error_to_message_error)?,
        member,
        role: GroupMemberRole::parse_optional(&request.role).map_err(im_error_to_message_error)?,
        reason_text: optional_string(&request.reason_text),
        leave_request_id: optional_string(&request.leave_request_id),
        security: group_security_requirement(request.secure_required || request.e2ee),
    };
    let result = if action == "add" {
        client.groups().add_member_async(sdk_request).await
    } else {
        client.groups().remove_member_async(sdk_request).await
    }
    .map_err(im_error_to_message_error)?;
    let raw = group_raw_response(&result);
    let warnings = group_control_warnings(resolved, result.warnings.clone());
    let snapshot = group_snapshot_result_json(&result, &raw)
        .unwrap_or_else(|| json!({ "group_did": request.group }));
    let members = group_members_to_cli_json(&result, &raw);
    let resolved_member = group_member_resolution_json(result.resolved_member.as_ref());
    Ok(CommandResult {
        data: json!({
            "group": snapshot,
            "members": members,
            "delivery": raw,
            "member": resolved_member,
        }),
        summary: format!("Updated group membership via {action}"),
        warnings: compact_warnings(warnings),
    })
}

pub fn update_group_via_im_core(
    resolved: &Resolved,
    client: &im_core::ImClient,
    request: GroupUpdateRequest,
) -> Result<CommandResult, MessageAdapterError> {
    if request.group.trim().is_empty() {
        return Err(MessageAdapterError::GroupRequired);
    }
    let profile_patch = group_profile_patch(&request)?;
    let policy_patch = group_policy_patch(&request)?;
    if profile_patch == GroupProfilePatch::default() && policy_patch == GroupPolicyPatch::default()
    {
        return Err(MessageAdapterError::Internal(
            "group update requires at least one mutable field".to_string(),
        ));
    }
    let group = GroupRef::parse(&request.group).map_err(im_error_to_message_error)?;
    let result = client
        .groups()
        .update(SdkGroupUpdateRequest {
            group,
            profile_patch,
            policy_patch,
        })
        .map_err(im_error_to_message_error)?;
    let responses = result
        .deliveries
        .iter()
        .map(group_raw_response)
        .collect::<Vec<_>>();
    let warnings = group_control_warnings(resolved, result.warnings.clone());
    let refreshed_raw = result
        .refreshed
        .as_ref()
        .map(group_raw_response)
        .unwrap_or(Value::Null);
    let snapshot = result
        .refreshed
        .as_ref()
        .and_then(|result| group_snapshot_result_json(result, &refreshed_raw))
        .unwrap_or_else(|| json!({ "group_did": request.group }));
    Ok(CommandResult {
        data: json!({
            "group": snapshot,
            "delivery": responses,
        }),
        summary: format!("Updated group {}", request.group),
        warnings: compact_warnings(warnings),
    })
}

pub async fn update_group_via_im_core_async(
    resolved: &Resolved,
    client: &im_core::ImClient,
    request: GroupUpdateRequest,
) -> Result<CommandResult, MessageAdapterError> {
    if request.group.trim().is_empty() {
        return Err(MessageAdapterError::GroupRequired);
    }
    let profile_patch = group_profile_patch(&request)?;
    let policy_patch = group_policy_patch(&request)?;
    if profile_patch == GroupProfilePatch::default() && policy_patch == GroupPolicyPatch::default()
    {
        return Err(MessageAdapterError::Internal(
            "group update requires at least one mutable field".to_string(),
        ));
    }
    let group = GroupRef::parse(&request.group).map_err(im_error_to_message_error)?;
    let result = client
        .groups()
        .update_async(SdkGroupUpdateRequest {
            group,
            profile_patch,
            policy_patch,
        })
        .await
        .map_err(im_error_to_message_error)?;
    let responses = result
        .deliveries
        .iter()
        .map(group_raw_response)
        .collect::<Vec<_>>();
    let warnings = group_control_warnings(resolved, result.warnings.clone());
    let refreshed_raw = result
        .refreshed
        .as_ref()
        .map(group_raw_response)
        .unwrap_or(Value::Null);
    let snapshot = result
        .refreshed
        .as_ref()
        .and_then(|result| group_snapshot_result_json(result, &refreshed_raw))
        .unwrap_or_else(|| json!({ "group_did": request.group }));
    Ok(CommandResult {
        data: json!({
            "group": snapshot,
            "delivery": responses,
        }),
        summary: format!("Updated group {}", request.group),
        warnings: compact_warnings(warnings),
    })
}

pub fn get_group_via_im_core(
    resolved: &Resolved,
    client: &im_core::ImClient,
    group: String,
) -> Result<CommandResult, MessageAdapterError> {
    let group_ref = GroupRef::parse(&group).map_err(im_error_to_message_error)?;
    let result = client
        .groups()
        .get(group_ref)
        .map_err(im_error_to_message_error)?;
    let raw = group_raw_response(&result);
    let snapshot = group_snapshot_result_json(&result, &raw).unwrap_or(Value::Null);
    Ok(CommandResult {
        data: json!({
            "group": snapshot,
            "source": group_read_source(&result, &raw),
        }),
        summary: "Loaded group snapshot".to_string(),
        warnings: group_control_warnings(resolved, result.warnings),
    })
}

pub async fn get_group_via_im_core_async(
    resolved: &Resolved,
    client: &im_core::ImClient,
    group: String,
) -> Result<CommandResult, MessageAdapterError> {
    let group_ref = GroupRef::parse(&group).map_err(im_error_to_message_error)?;
    let result = client
        .groups()
        .get_async(group_ref)
        .await
        .map_err(im_error_to_message_error)?;
    let raw = group_raw_response(&result);
    let snapshot = group_snapshot_result_json(&result, &raw).unwrap_or(Value::Null);
    Ok(CommandResult {
        data: json!({
            "group": snapshot,
            "source": group_read_source(&result, &raw),
        }),
        summary: "Loaded group snapshot".to_string(),
        warnings: group_control_warnings(resolved, result.warnings),
    })
}

pub fn list_groups_via_im_core(
    resolved: &Resolved,
    client: &im_core::ImClient,
    limit: i64,
    cursor: String,
) -> Result<CommandResult, MessageAdapterError> {
    let request = GroupListRequest {
        limit: page_limit(limit, 50)?,
        cursor: optional_cursor(&cursor)?,
    };
    let result = client
        .groups()
        .list(request)
        .map_err(im_error_to_message_error)?;
    let raw = group_raw_response(&result);
    let groups = groups_to_cli_json(&result);
    let total = group_read_total(&result, groups.len());
    Ok(CommandResult {
        data: json!({
            "groups": groups,
            "total": total,
            "has_more": result.has_more,
            "next_cursor": result.next_cursor.as_ref().map(im_core::ids::Cursor::as_str),
            "source": group_read_source(&result, &raw),
        }),
        summary: format!("Loaded {total} groups"),
        warnings: group_control_warnings(resolved, result.warnings),
    })
}

pub async fn list_groups_via_im_core_async(
    resolved: &Resolved,
    client: &im_core::ImClient,
    limit: i64,
    cursor: String,
) -> Result<CommandResult, MessageAdapterError> {
    let request = GroupListRequest {
        limit: page_limit(limit, 50)?,
        cursor: optional_cursor(&cursor)?,
    };
    let result = client
        .groups()
        .list_async(request)
        .await
        .map_err(im_error_to_message_error)?;
    let raw = group_raw_response(&result);
    let groups = groups_to_cli_json(&result);
    let total = group_read_total(&result, groups.len());
    Ok(CommandResult {
        data: json!({
            "groups": groups,
            "total": total,
            "has_more": result.has_more,
            "next_cursor": result.next_cursor.as_ref().map(im_core::ids::Cursor::as_str),
            "source": group_read_source(&result, &raw),
        }),
        summary: format!("Loaded {total} groups"),
        warnings: group_control_warnings(resolved, result.warnings),
    })
}

pub fn group_members_via_im_core(
    resolved: &Resolved,
    client: &im_core::ImClient,
    group: String,
    limit: i64,
    cursor: String,
) -> Result<CommandResult, MessageAdapterError> {
    let requested_group = GroupRef::parse(&group).map_err(im_error_to_message_error)?;
    let request = GroupMembersRequest {
        group: requested_group.clone(),
        limit: page_limit(limit, 100)?,
        cursor: optional_cursor(&cursor)?,
    };
    let result = client
        .groups()
        .members(request)
        .map_err(im_error_to_message_error)?;
    let page_group =
        required_group_member_page_binding(result.page_group.as_ref(), &requested_group)?;
    let raw = group_raw_response(&result);
    let members = group_members_to_cli_json(&result, &raw);
    let total = group_read_total(&result, members.len());
    Ok(CommandResult {
        data: json!({
            "group": page_group,
            "page_group": page_group,
            "members": members,
            "total": total,
            "has_more": result.has_more,
            "next_cursor": result.next_cursor.as_ref().map(im_core::ids::Cursor::as_str),
            "group_state_version": result.group_state_version.as_deref(),
            "source": group_read_source(&result, &raw),
        }),
        summary: format!("Loaded {total} group members"),
        warnings: group_control_warnings(resolved, result.warnings),
    })
}

pub async fn group_members_via_im_core_async(
    resolved: &Resolved,
    client: &im_core::ImClient,
    group: String,
    limit: i64,
    cursor: String,
) -> Result<CommandResult, MessageAdapterError> {
    let requested_group = GroupRef::parse(&group).map_err(im_error_to_message_error)?;
    let request = GroupMembersRequest {
        group: requested_group.clone(),
        limit: page_limit(limit, 100)?,
        cursor: optional_cursor(&cursor)?,
    };
    let result = client
        .groups()
        .members_async(request)
        .await
        .map_err(im_error_to_message_error)?;
    let page_group =
        required_group_member_page_binding(result.page_group.as_ref(), &requested_group)?;
    let raw = group_raw_response(&result);
    let members = group_members_to_cli_json(&result, &raw);
    let total = group_read_total(&result, members.len());
    Ok(CommandResult {
        data: json!({
            "group": page_group,
            "page_group": page_group,
            "members": members,
            "total": total,
            "has_more": result.has_more,
            "next_cursor": result.next_cursor.as_ref().map(im_core::ids::Cursor::as_str),
            "group_state_version": result.group_state_version.as_deref(),
            "source": group_read_source(&result, &raw),
        }),
        summary: format!("Loaded {total} group members"),
        warnings: group_control_warnings(resolved, result.warnings),
    })
}

pub fn group_messages_via_im_core(
    resolved: &Resolved,
    client: &im_core::ImClient,
    group: String,
    limit: i64,
    cursor: String,
) -> Result<CommandResult, MessageAdapterError> {
    let request = GroupMessagesRequest {
        group: GroupRef::parse(&group).map_err(im_error_to_message_error)?,
        limit: page_limit(limit, 50)?,
        cursor: optional_cursor(&cursor)?,
    };
    let result = client
        .groups()
        .messages(request)
        .map_err(im_error_to_message_error)?;
    let raw = group_raw_response(&result);
    let result_source_mode = host_runtime::bridge::MODE_HTTP;

    let messages = group_messages_to_cli_json(&result, &raw);
    let total = group_read_total(&result, messages.len());
    Ok(CommandResult {
        data: json!({
            "group": group,
            "messages": messages,
            "total": total,
            "has_more": bool_value(raw.get("has_more")),
            "next_since_seq": raw.get("next_since_seq").cloned().unwrap_or(Value::Null),
            "source": source_with_default_for_mode(&raw, result_source_mode),
        }),
        summary: format!("Loaded {total} group messages"),
        warnings: group_control_warnings(resolved, result.warnings),
    })
}

pub async fn group_messages_via_im_core_async(
    resolved: &Resolved,
    client: &im_core::ImClient,
    group: String,
    limit: i64,
    cursor: String,
) -> Result<CommandResult, MessageAdapterError> {
    let request = GroupMessagesRequest {
        group: GroupRef::parse(&group).map_err(im_error_to_message_error)?,
        limit: page_limit(limit, 50)?,
        cursor: optional_cursor(&cursor)?,
    };
    let result = client
        .groups()
        .messages_async(request)
        .await
        .map_err(im_error_to_message_error)?;
    let raw = group_raw_response(&result);
    let result_source_mode = host_runtime::bridge::MODE_HTTP;

    let messages = group_messages_to_cli_json(&result, &raw);
    let total = group_read_total(&result, messages.len());
    Ok(CommandResult {
        data: json!({
            "group": group,
            "messages": messages,
            "total": total,
            "has_more": bool_value(raw.get("has_more")),
            "next_since_seq": raw.get("next_since_seq").cloned().unwrap_or(Value::Null),
            "source": source_with_default_for_mode(&raw, result_source_mode),
        }),
        summary: format!("Loaded {total} group messages"),
        warnings: group_control_warnings(resolved, result.warnings),
    })
}

pub fn group_secure_status_via_im_core(
    client: &im_core::ImClient,
    group: String,
) -> Result<CommandResult, MessageAdapterError> {
    let group_ref = GroupRef::parse(&group).map_err(im_error_to_message_error)?;
    let status = client
        .secure()
        .group(group_ref)
        .status()
        .map_err(im_error_to_message_error)?;
    let warnings = status.warnings.clone();
    Ok(CommandResult {
        data: json!({
            "status": serde_json::to_value(&status).unwrap_or(Value::Null),
        }),
        summary: "Loaded group secure status".to_string(),
        warnings: compact_warnings(warnings),
    })
}

pub async fn group_secure_status_via_im_core_async(
    client: &im_core::ImClient,
    group: String,
) -> Result<CommandResult, MessageAdapterError> {
    let group_ref = GroupRef::parse(&group).map_err(im_error_to_message_error)?;
    let status = client
        .secure()
        .group(group_ref)
        .status_async()
        .await
        .map_err(im_error_to_message_error)?;
    let warnings = status.warnings.clone();
    Ok(CommandResult {
        data: json!({
            "status": serde_json::to_value(&status).unwrap_or(Value::Null),
        }),
        summary: "Loaded group secure status".to_string(),
        warnings: compact_warnings(warnings),
    })
}

pub fn group_secure_repair_via_im_core(
    client: &im_core::ImClient,
    group: String,
) -> Result<CommandResult, MessageAdapterError> {
    let group_ref = GroupRef::parse(&group).map_err(im_error_to_message_error)?;
    let repair = client
        .secure()
        .group(group_ref)
        .repair()
        .map_err(im_error_to_message_error)?;
    let warnings = repair.warnings.clone();
    Ok(CommandResult {
        data: json!({
            "repair": serde_json::to_value(&repair).unwrap_or(Value::Null),
        }),
        summary: "Repaired group secure state".to_string(),
        warnings: compact_warnings(warnings),
    })
}

pub async fn group_secure_repair_via_im_core_async(
    client: &im_core::ImClient,
    group: String,
) -> Result<CommandResult, MessageAdapterError> {
    let group_ref = GroupRef::parse(&group).map_err(im_error_to_message_error)?;
    let repair = client
        .secure()
        .group(group_ref)
        .repair_async()
        .await
        .map_err(im_error_to_message_error)?;
    let warnings = repair.warnings.clone();
    Ok(CommandResult {
        data: json!({
            "repair": serde_json::to_value(&repair).unwrap_or(Value::Null),
        }),
        summary: "Repaired group secure state".to_string(),
        warnings: compact_warnings(warnings),
    })
}

pub fn publish_group_key_package_via_im_core(
    client: &im_core::ImClient,
    request: GroupKeyPackagePublishRequest,
) -> Result<CommandResult, MessageAdapterError> {
    let purpose = GroupKeyPackagePurpose::parse(if request.purpose.trim().is_empty() {
        "normal"
    } else {
        &request.purpose
    })
    .map_err(im_error_to_message_error)?;
    let group = if request.group.trim().is_empty() {
        None
    } else {
        Some(GroupRef::parse(&request.group).map_err(im_error_to_message_error)?)
    };
    let result = client
        .groups()
        .publish_key_package(im_core::groups::GroupKeyPackagePublishRequest {
            purpose,
            group,
            device_id: optional_string(&request.device),
            key_package_id: None,
        })
        .map_err(im_error_to_message_error)?;
    let group = result
        .group
        .as_ref()
        .map(|group| Value::String(group.as_str().to_owned()))
        .unwrap_or(Value::Null);
    Ok(CommandResult {
        data: json!({
            "published": true,
            "owner_did": result.owner_did.as_str(),
            "device_id": result.device_id,
            "key_package_id": result.key_package_id,
            "purpose": result.purpose.as_str(),
            "group": group,
            "delivery": result.raw_response,
        }),
        summary: "Published group E2EE KeyPackage".to_owned(),
        warnings: compact_warnings(result.warnings),
    })
}

pub async fn publish_group_key_package_via_im_core_async(
    client: &im_core::ImClient,
    request: GroupKeyPackagePublishRequest,
) -> Result<CommandResult, MessageAdapterError> {
    let purpose = GroupKeyPackagePurpose::parse(if request.purpose.trim().is_empty() {
        "normal"
    } else {
        &request.purpose
    })
    .map_err(im_error_to_message_error)?;
    let group = if request.group.trim().is_empty() {
        None
    } else {
        Some(GroupRef::parse(&request.group).map_err(im_error_to_message_error)?)
    };
    let result = client
        .groups()
        .publish_key_package_async(im_core::groups::GroupKeyPackagePublishRequest {
            purpose,
            group,
            device_id: optional_string(&request.device),
            key_package_id: None,
        })
        .await
        .map_err(im_error_to_message_error)?;
    let group = result
        .group
        .as_ref()
        .map(|group| Value::String(group.as_str().to_owned()))
        .unwrap_or(Value::Null);
    Ok(CommandResult {
        data: json!({
            "published": true,
            "owner_did": result.owner_did.as_str(),
            "device_id": result.device_id,
            "key_package_id": result.key_package_id,
            "purpose": result.purpose.as_str(),
            "group": group,
            "delivery": result.raw_response,
        }),
        summary: "Published group E2EE KeyPackage".to_owned(),
        warnings: compact_warnings(result.warnings),
    })
}

pub fn process_group_e2ee_leave_request_via_im_core(
    resolved: &Resolved,
    client: &im_core::ImClient,
    request: GroupE2eeProcessLeaveRequest,
) -> Result<CommandResult, MessageAdapterError> {
    if request.group.trim().is_empty() {
        return Err(MessageAdapterError::GroupRequired);
    }
    if request.member.trim().is_empty() {
        return Err(MessageAdapterError::MemberRequired);
    }
    if request.leave_request_id.trim().is_empty() {
        return Err(MessageAdapterError::Internal(
            "group e2ee process-leave-request requires leave_request_id".to_string(),
        ));
    }
    let result = client
        .groups()
        .process_e2ee_leave_request(im_core::groups::GroupE2eeProcessLeaveRequest {
            group: GroupRef::parse(&request.group).map_err(im_error_to_message_error)?,
            member: GroupMemberRef::parse(&request.member, &resolved.did_domain)
                .map_err(im_error_to_message_error)?,
            leave_request_id: request.leave_request_id,
            reason_text: optional_string(&request.reason_text),
        })
        .map_err(im_error_to_message_error)?;
    let raw = group_raw_response(&result);
    Ok(CommandResult {
        data: json!({
            "group": group_snapshot_result_json(&result, &raw).unwrap_or_else(|| json!({ "group_did": request.group })),
            "members": group_members_to_cli_json(&result, &raw),
            "delivery": raw,
            "member": group_member_resolution_json(result.resolved_member.as_ref()),
        }),
        summary: "Processed group E2EE leave request".to_string(),
        warnings: compact_warnings(result.warnings),
    })
}

pub async fn process_group_e2ee_leave_request_via_im_core_async(
    resolved: &Resolved,
    client: &im_core::ImClient,
    request: GroupE2eeProcessLeaveRequest,
) -> Result<CommandResult, MessageAdapterError> {
    if request.group.trim().is_empty() {
        return Err(MessageAdapterError::GroupRequired);
    }
    if request.member.trim().is_empty() {
        return Err(MessageAdapterError::MemberRequired);
    }
    if request.leave_request_id.trim().is_empty() {
        return Err(MessageAdapterError::Internal(
            "group e2ee process-leave-request requires leave_request_id".to_string(),
        ));
    }
    let result = client
        .groups()
        .process_e2ee_leave_request_async(im_core::groups::GroupE2eeProcessLeaveRequest {
            group: GroupRef::parse(&request.group).map_err(im_error_to_message_error)?,
            member: GroupMemberRef::parse(&request.member, &resolved.did_domain)
                .map_err(im_error_to_message_error)?,
            leave_request_id: request.leave_request_id,
            reason_text: optional_string(&request.reason_text),
        })
        .await
        .map_err(im_error_to_message_error)?;
    let raw = group_raw_response(&result);
    Ok(CommandResult {
        data: json!({
            "group": group_snapshot_result_json(&result, &raw).unwrap_or_else(|| json!({ "group_did": request.group })),
            "members": group_members_to_cli_json(&result, &raw),
            "delivery": raw,
            "member": group_member_resolution_json(result.resolved_member.as_ref()),
        }),
        summary: "Processed group E2EE leave request".to_string(),
        warnings: compact_warnings(result.warnings),
    })
}

pub fn update_group_e2ee_key_via_im_core(
    resolved: &Resolved,
    client: &im_core::ImClient,
    request: GroupE2eeMemberKeyRequest,
) -> Result<CommandResult, MessageAdapterError> {
    if request.group.trim().is_empty() {
        return Err(MessageAdapterError::GroupRequired);
    }
    if request.member.trim().is_empty() {
        return Err(MessageAdapterError::MemberRequired);
    }
    let result = client
        .groups()
        .update_member_key(im_core::groups::GroupE2eeUpdateKeyRequest {
            group: GroupRef::parse(&request.group).map_err(im_error_to_message_error)?,
            member: GroupMemberRef::parse(&request.member, &resolved.did_domain)
                .map_err(im_error_to_message_error)?,
            device_id: optional_string(&request.device),
        })
        .map_err(im_error_to_message_error)?;
    group_e2ee_key_command_result("Updated group E2EE member key", request.group, result)
}

pub async fn update_group_e2ee_key_via_im_core_async(
    resolved: &Resolved,
    client: &im_core::ImClient,
    request: GroupE2eeMemberKeyRequest,
) -> Result<CommandResult, MessageAdapterError> {
    if request.group.trim().is_empty() {
        return Err(MessageAdapterError::GroupRequired);
    }
    if request.member.trim().is_empty() {
        return Err(MessageAdapterError::MemberRequired);
    }
    let result = client
        .groups()
        .update_member_key_async(im_core::groups::GroupE2eeUpdateKeyRequest {
            group: GroupRef::parse(&request.group).map_err(im_error_to_message_error)?,
            member: GroupMemberRef::parse(&request.member, &resolved.did_domain)
                .map_err(im_error_to_message_error)?,
            device_id: optional_string(&request.device),
        })
        .await
        .map_err(im_error_to_message_error)?;
    group_e2ee_key_command_result("Updated group E2EE member key", request.group, result)
}

pub fn recover_group_e2ee_member_via_im_core(
    resolved: &Resolved,
    client: &im_core::ImClient,
    request: GroupE2eeMemberKeyRequest,
) -> Result<CommandResult, MessageAdapterError> {
    if request.group.trim().is_empty() {
        return Err(MessageAdapterError::GroupRequired);
    }
    if request.member.trim().is_empty() {
        return Err(MessageAdapterError::MemberRequired);
    }
    let result = client
        .groups()
        .recover_member(im_core::groups::GroupE2eeRecoverMemberRequest {
            group: GroupRef::parse(&request.group).map_err(im_error_to_message_error)?,
            member: GroupMemberRef::parse(&request.member, &resolved.did_domain)
                .map_err(im_error_to_message_error)?,
            device_id: optional_string(&request.device),
        })
        .map_err(im_error_to_message_error)?;
    group_e2ee_key_command_result("Recovered group E2EE member", request.group, result)
}

pub async fn recover_group_e2ee_member_via_im_core_async(
    resolved: &Resolved,
    client: &im_core::ImClient,
    request: GroupE2eeMemberKeyRequest,
) -> Result<CommandResult, MessageAdapterError> {
    if request.group.trim().is_empty() {
        return Err(MessageAdapterError::GroupRequired);
    }
    if request.member.trim().is_empty() {
        return Err(MessageAdapterError::MemberRequired);
    }
    let result = client
        .groups()
        .recover_member_async(im_core::groups::GroupE2eeRecoverMemberRequest {
            group: GroupRef::parse(&request.group).map_err(im_error_to_message_error)?,
            member: GroupMemberRef::parse(&request.member, &resolved.did_domain)
                .map_err(im_error_to_message_error)?,
            device_id: optional_string(&request.device),
        })
        .await
        .map_err(im_error_to_message_error)?;
    group_e2ee_key_command_result("Recovered group E2EE member", request.group, result)
}

fn group_e2ee_key_command_result(
    summary: &str,
    group: String,
    result: im_core::groups::GroupReadResult,
) -> Result<CommandResult, MessageAdapterError> {
    let raw = group_raw_response(&result);
    Ok(CommandResult {
        data: json!({
            "group": group_snapshot_result_json(&result, &raw).unwrap_or_else(|| json!({ "group_did": group })),
            "members": group_members_to_cli_json(&result, &raw),
            "delivery": raw,
            "member": group_member_resolution_json(result.resolved_member.as_ref()),
        }),
        summary: summary.to_string(),
        warnings: compact_warnings(result.warnings),
    })
}

fn group_create_request(
    request: GroupCreateRequest,
) -> Result<SdkGroupCreateRequest, MessageAdapterError> {
    let secure_required = request.secure_required
        || request.e2ee
        || request.message_security_profile.trim() == GROUP_E2EE_SECURITY_PROFILE;
    Ok(SdkGroupCreateRequest {
        name: request.name,
        creator_handle: None,
        description: optional_string(&request.description),
        avatar_uri: optional_string(&request.avatar_uri),
        discoverability: GroupDiscoverability::parse_optional(&request.discoverability)
            .map_err(im_error_to_message_error)?,
        admission_mode: GroupAdmissionMode::parse_optional(&request.admission_mode)
            .map_err(im_error_to_message_error)?,
        message_security_profile: GroupMessageSecurityProfile::parse_optional(if secure_required {
            GROUP_E2EE_SECURITY_PROFILE
        } else {
            &request.message_security_profile
        })
        .map_err(im_error_to_message_error)?,
        security: group_security_requirement(secure_required),
        e2ee: secure_required,
        slug: optional_string(&request.slug),
        goal: optional_string(&request.goal),
        rules: optional_string(&request.rules),
        message_prompt: optional_string(&request.message_prompt),
        doc_url: optional_string(&request.doc_url),
        attachments_allowed: request.attachments_allowed,
        max_members: GroupMemberLimit::parse_optional(&request.max_members)
            .map_err(im_error_to_message_error)?,
        member_max_messages: request.member_max_messages,
        member_max_total_chars: request.member_max_total_chars,
    })
}

fn group_security_requirement(required: bool) -> GroupSecurityRequirement {
    if required {
        GroupSecurityRequirement::Required
    } else {
        GroupSecurityRequirement::Default
    }
}

fn group_profile_patch(
    request: &GroupUpdateRequest,
) -> Result<GroupProfilePatch, MessageAdapterError> {
    Ok(GroupProfilePatch {
        name: optional_string(&request.name),
        description: optional_string(&request.description),
        avatar_uri: optional_string(&request.avatar_uri),
        discoverability: GroupDiscoverability::parse_optional(&request.discoverability)
            .map_err(im_error_to_message_error)?,
        slug: optional_string(&request.slug),
        goal: optional_string(&request.goal),
        rules: optional_string(&request.rules),
        message_prompt: optional_string(&request.message_prompt),
        doc_url: optional_string(&request.doc_url),
    })
}

fn group_policy_patch(
    request: &GroupUpdateRequest,
) -> Result<GroupPolicyPatch, MessageAdapterError> {
    Ok(GroupPolicyPatch {
        admission_mode: GroupAdmissionMode::parse_optional(&request.admission_mode)
            .map_err(im_error_to_message_error)?,
        attachments_allowed: request.attachments_allowed,
        max_members: GroupMemberLimit::parse_optional(&request.max_members)
            .map_err(im_error_to_message_error)?,
        member_max_messages: request.member_max_messages,
        member_max_total_chars: request.member_max_total_chars,
    })
}

fn group_control_warnings(resolved: &Resolved, mut warnings: Vec<String>) -> Vec<String> {
    if runtime_mode(resolved) == host_runtime::bridge::MODE_WEBSOCKET {
        warnings.push(
            "Group lifecycle commands use HTTP transport even when runtime.mode is websocket."
                .to_string(),
        );
    }
    compact_warnings(warnings)
}

fn group_read_source(result: &GroupReadResult, raw: &Value) -> String {
    result
        .source
        .as_ref()
        .filter(|value| !value.trim().is_empty())
        .cloned()
        .unwrap_or_else(|| group_control_source(raw))
}

fn group_read_total(result: &GroupReadResult, fallback_len: usize) -> i64 {
    result
        .total
        .map(i64::from)
        .unwrap_or_else(|| fallback_len as i64)
}

fn groups_to_cli_json(result: &GroupReadResult) -> Vec<Value> {
    if !result.groups.is_empty() {
        return result.groups.iter().map(group_summary_to_json).collect();
    }
    result
        .response_json()
        .map(|raw| values_from_array(raw.get("groups")))
        .unwrap_or_default()
}

fn group_members_to_cli_json(result: &GroupReadResult, raw: &Value) -> Vec<Value> {
    if !result.members.is_empty() {
        return result.members.iter().map(group_member_to_json).collect();
    }
    values_from_array(raw.get("members"))
        .into_iter()
        .map(normalize_group_member_json)
        .collect()
}

fn group_messages_to_cli_json(result: &GroupReadResult, raw: &Value) -> Vec<Value> {
    if !result.messages.items.is_empty() {
        return result
            .messages
            .items
            .iter()
            .map(group_message_to_json)
            .collect();
    }
    values_from_array(raw.get("messages"))
}

fn group_snapshot_to_cli_json(snapshot: Option<&GroupSnapshot>) -> Option<Value> {
    let snapshot = snapshot?;
    let display_name = snapshot
        .display_name
        .clone()
        .or_else(|| snapshot.name.clone());
    let group_profile = group_profile_json(
        display_name.clone(),
        snapshot.description.clone(),
        snapshot.avatar_uri.clone(),
    );
    Some(json!({
        "id": snapshot.id,
        "group_did": snapshot.did.as_str(),
        "did": snapshot.did.as_str(),
        "name": display_name,
        "display_name": display_name,
        "description": snapshot.description,
        "avatar_uri": snapshot.avatar_uri,
        "avatar": snapshot.avatar_uri,
        "group_profile": group_profile,
        "member_role": snapshot.my_role,
        "my_role": snapshot.my_role,
        "member_status": snapshot.membership_status,
        "membership_status": snapshot.membership_status,
        "member_count": snapshot.member_count,
        "last_message_at": snapshot.last_message_at,
    }))
}

fn group_snapshot_result_json(result: &GroupReadResult, raw: &Value) -> Option<Value> {
    group_snapshot_to_cli_json(result.group.as_ref())
        .or_else(|| normalize_group_snapshot(raw))
        .map(|snapshot| merge_group_snapshot_raw(snapshot, raw))
}

fn merge_group_snapshot_raw(mut snapshot: Value, raw: &Value) -> Value {
    let Some(Value::Object(raw_object)) = normalize_group_snapshot(raw) else {
        return snapshot;
    };
    let Some(object) = snapshot.as_object_mut() else {
        return snapshot;
    };
    for (key, value) in raw_object {
        object.entry(key).or_insert(value);
    }
    snapshot
}

fn group_summary_to_json(group: &GroupSummary) -> Value {
    let display_name = group.display_name.clone().or_else(|| group.name.clone());
    let group_profile = group_profile_json(display_name.clone(), None, group.avatar_uri.clone());
    json!({
        "id": group.id,
        "group_did": group.did.as_str(),
        "did": group.did.as_str(),
        "name": display_name,
        "display_name": display_name,
        "avatar_uri": group.avatar_uri,
        "avatar": group.avatar_uri,
        "group_profile": group_profile,
        "member_role": group.my_role,
        "my_role": group.my_role,
        "member_status": group.membership_status,
        "membership_status": group.membership_status,
        "member_count": group.member_count,
        "last_message_at": group.last_message_at,
    })
}

fn group_member_to_json(member: &GroupMember) -> Value {
    let did = member.did.as_ref().map(Did::as_str).unwrap_or_default();
    let handle = member
        .handle
        .as_ref()
        .map(Handle::as_str)
        .map(normalize_handle_value)
        .unwrap_or_default();
    json!({
        "member_did": did,
        "did": did,
        "member_handle": handle,
        "handle": handle,
        "role": member.role,
        "status": member.status,
        "joined_at": member.joined_at,
    })
}

fn group_member_resolution_json(member: Option<&GroupMemberResolution>) -> Value {
    match member {
        Some(member) => json!({
            "did": member.did.as_str(),
            "handle": member
                .handle
                .as_ref()
                .map(Handle::as_str)
                .map(normalize_handle_value)
                .unwrap_or_default(),
        }),
        None => json!({
            "did": "",
            "handle": "",
        }),
    }
}

fn normalize_group_member_json(mut member: Value) -> Value {
    let Some(object) = member.as_object_mut() else {
        return member;
    };
    let did = default_string(
        &string_value(object.get("agent_did")),
        &default_string(
            &string_value(object.get("member_did")),
            &string_value(object.get("did")),
        ),
    );
    if !did.trim().is_empty() {
        object
            .entry("member_did".to_string())
            .or_insert_with(|| Value::String(did.clone()));
        object
            .entry("did".to_string())
            .or_insert_with(|| Value::String(did));
    }
    let handle = normalize_handle_value(&default_string(
        &string_value(object.get("handle")),
        &default_string(
            &string_value(object.get("member_handle")),
            &string_value(object.get("agent_handle")),
        ),
    ));
    if !handle.is_empty() {
        object.insert("member_handle".to_string(), Value::String(handle.clone()));
        object
            .entry("handle".to_string())
            .or_insert_with(|| Value::String(handle));
    }
    member
}

fn group_message_to_json(message: &Message) -> Value {
    let content = message_body_content(&message.body);
    let content_type = message_content_type(message);
    let mut value = json!({
        "id": message.id.as_str(),
        "msg_id": message.id.as_str(),
        "message_id": message.id.as_str(),
        "sender_did": message.sender.as_str(),
        "group_did": message.group.as_ref().map(GroupRef::as_str).unwrap_or_default(),
        "content": content,
        "content_type": content_type,
        "sent_at": message.sent_at.clone().unwrap_or_default(),
        "received_at": message.received_at.clone().unwrap_or_default(),
        "is_read": false,
        "secure": group_message_is_secure(message),
        "direction": match message.direction {
            im_core::prelude::MessageDirection::Outgoing => 1,
            im_core::prelude::MessageDirection::Incoming => 0,
            im_core::prelude::MessageDirection::Unknown => -1,
        },
    });
    if content_type == im_core::attachments::attachment_manifest_content_type() {
        value["type"] = json!("attachment_manifest");
    }
    if let Some(sequence) = message.metadata.server_sequence {
        value["server_seq"] = json!(sequence);
    }
    if let Some(operation_id) = &message.metadata.operation_id {
        value["operation_id"] = json!(operation_id);
    }
    if let Some(delivery_state) = &message.metadata.delivery_state {
        value["delivery_state"] = json!(delivery_state);
    }
    for attribute in &message.metadata.attributes {
        if !attribute.key.trim().is_empty() {
            value[attribute.key.as_str()] = json!(attribute.value);
        }
    }
    value
}

fn message_body_content(body: &im_core::prelude::MessageBodyView) -> Value {
    match body {
        im_core::prelude::MessageBodyView::Text { text, .. } => json!(text),
        im_core::prelude::MessageBodyView::Payload { payload } => payload.clone(),
        im_core::prelude::MessageBodyView::Unsupported { .. } => json!(""),
    }
}

fn message_content_type(message: &Message) -> String {
    if let Some(content_type) = message
        .metadata
        .content_type
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
    {
        return content_type.to_string();
    }
    if matches!(
        &message.body,
        im_core::prelude::MessageBodyView::Payload { payload }
            if is_attachment_manifest_payload(payload)
    ) {
        return im_core::attachments::attachment_manifest_content_type().to_string();
    }
    match &message.body {
        im_core::prelude::MessageBodyView::Text {
            kind: im_core::prelude::MessageKind::Markdown,
            ..
        } => "text/markdown".to_string(),
        im_core::prelude::MessageBodyView::Text { .. } => "text/plain".to_string(),
        im_core::prelude::MessageBodyView::Payload { .. } => "application/json".to_string(),
        im_core::prelude::MessageBodyView::Unsupported { content_type } => content_type
            .as_ref()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
            .unwrap_or("application/octet-stream")
            .to_string(),
    }
}

fn is_attachment_manifest_payload(payload: &Value) -> bool {
    let Some(primary_attachment_id) = payload
        .get("primary_attachment_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return false;
    };
    payload
        .get("attachments")
        .and_then(Value::as_array)
        .is_some_and(|attachments| {
            attachments.iter().any(|attachment| {
                attachment
                    .get("attachment_id")
                    .and_then(Value::as_str)
                    .is_some_and(|value| value == primary_attachment_id)
            })
        })
}

fn group_message_is_secure(message: &Message) -> bool {
    message
        .metadata
        .attributes
        .iter()
        .any(|attribute| attribute.key == "security" && attribute.value == "group-e2ee")
}

fn compact_warnings(warnings: Vec<String>) -> Vec<String> {
    let mut compact = Vec::new();
    for warning in warnings {
        let warning = warning.trim().to_string();
        if warning.is_empty() || compact.iter().any(|known| known == &warning) {
            continue;
        }
        compact.push(warning);
    }
    compact
}

fn source_with_default_for_mode(raw: &Value, mode: &str) -> String {
    raw.get("source")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(if mode == host_runtime::bridge::MODE_WEBSOCKET {
            "local_ws_cache"
        } else {
            "remote_http"
        })
        .to_string()
}

fn runtime_mode(resolved: &Resolved) -> &'static str {
    if resolved.runtime_mode.trim().eq_ignore_ascii_case("http") {
        host_runtime::bridge::MODE_HTTP
    } else {
        host_runtime::bridge::MODE_WEBSOCKET
    }
}

fn page_limit(value: i64, fallback: u32) -> Result<PageLimit, MessageAdapterError> {
    let value = if value <= 0 {
        fallback
    } else {
        u32::try_from(value)
            .map_err(|_| MessageAdapterError::Json("limit is too large".to_string()))?
    };
    PageLimit::new(value).map_err(im_error_to_message_error)
}

fn optional_cursor(value: &str) -> Result<Option<Cursor>, MessageAdapterError> {
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

fn required_group_member_page_binding<'a>(
    page_group: Option<&'a GroupRef>,
    requested_group: &GroupRef,
) -> Result<&'a str, MessageAdapterError> {
    match page_group {
        Some(page_group) if page_group == requested_group => Ok(page_group.as_str()),
        Some(_) | None => Err(MessageAdapterError::PublicServiceCode(
            "group.local_inventory_incomplete".to_owned(),
        )),
    }
}

fn default_string(value: &str, fallback: &str) -> String {
    if value.trim().is_empty() {
        fallback.to_string()
    } else {
        value.to_string()
    }
}

fn group_profile_json(
    display_name: Option<String>,
    description: Option<String>,
    avatar_uri: Option<String>,
) -> Value {
    let mut profile = serde_json::Map::new();
    if let Some(display_name) = display_name.filter(|value| !value.trim().is_empty()) {
        profile.insert("display_name".to_string(), Value::String(display_name));
    }
    if let Some(description) = description.filter(|value| !value.trim().is_empty()) {
        profile.insert("description".to_string(), Value::String(description));
    }
    if let Some(avatar_uri) = avatar_uri.filter(|value| !value.trim().is_empty()) {
        profile.insert("avatar_uri".to_string(), Value::String(avatar_uri));
    }
    Value::Object(profile)
}

fn normalize_group_snapshot(raw: &Value) -> Option<Value> {
    if raw.is_null() {
        return None;
    }
    if let Some(snapshot) = raw.get("group_snapshot").filter(|value| value.is_object()) {
        return Some(snapshot.clone());
    }
    let group_did = group_did_from_raw(raw);
    if group_did.trim().is_empty() {
        return None;
    }
    if let Some(profile) = raw.get("group_profile").filter(|value| value.is_object()) {
        let display_name = string_value(profile.get("display_name"));
        let avatar_uri = string_value(profile.get("avatar_uri"))
            .or_else_nonempty(|| string_value(profile.get("avatar_url")))
            .or_else_nonempty(|| string_value(profile.get("avatar")));
        return Some(json!({
            "group_did": group_did,
            "did": group_did,
            "group_state_version": raw.get("group_state_version").cloned().unwrap_or(Value::Null),
            "name": display_name,
            "display_name": display_name,
            "description": profile.get("description").cloned().unwrap_or(Value::Null),
            "avatar_uri": avatar_uri,
            "avatar": avatar_uri,
            "discoverability": profile.get("discoverability").cloned().unwrap_or(Value::Null),
            "slug": profile.get("slug").cloned().unwrap_or(Value::Null),
            "goal": profile.get("goal").cloned().unwrap_or(Value::Null),
            "rules": profile.get("rules").cloned().unwrap_or(Value::Null),
            "message_prompt": profile.get("message_prompt").cloned().unwrap_or(Value::Null),
            "doc_url": profile.get("doc_url").cloned().unwrap_or(Value::Null),
            "owner_did": raw.get("owner_did").cloned().unwrap_or(Value::Null),
            "member_role": raw.get("member_role").or_else(|| raw.get("my_role")).cloned().unwrap_or(Value::Null),
            "my_role": raw.get("my_role").or_else(|| raw.get("member_role")).cloned().unwrap_or(Value::Null),
            "member_status": raw.get("member_status").or_else(|| raw.get("membership_status")).cloned().unwrap_or(Value::Null),
            "membership_status": raw.get("membership_status").or_else(|| raw.get("member_status")).cloned().unwrap_or(Value::Null),
            "join_enabled": raw.get("join_enabled").cloned().unwrap_or(Value::Null),
            "member_count": raw.get("member_count").cloned().unwrap_or(Value::Null),
            "group_profile": profile,
            "group_policy": raw.get("group_policy").cloned().unwrap_or(Value::Null),
            "created_at": raw.get("created_at").cloned().unwrap_or(Value::Null),
            "updated_at": raw.get("updated_at").cloned().unwrap_or(Value::Null),
        }));
    }
    Some(raw.clone())
}

fn values_from_array(value: Option<&Value>) -> Vec<Value> {
    value.and_then(Value::as_array).cloned().unwrap_or_default()
}

fn group_did_from_result(result: &GroupReadResult, raw: &Value) -> Option<String> {
    result
        .group
        .as_ref()
        .map(|group| group.did.as_str().to_string())
        .or_else(|| {
            let value = group_did_from_raw(raw);
            (!value.trim().is_empty()).then_some(value)
        })
}

fn group_did_from_raw(raw: &Value) -> String {
    string_value(raw.get("group_did"))
        .trim()
        .to_string()
        .or_else_nonempty(|| string_value(raw.get("did")))
}

trait NonEmptyString {
    fn or_else_nonempty(self, fallback: impl FnOnce() -> String) -> String;
}

impl NonEmptyString for String {
    fn or_else_nonempty(self, fallback: impl FnOnce() -> String) -> String {
        if self.trim().is_empty() {
            fallback()
        } else {
            self
        }
    }
}

fn group_control_source(raw: &Value) -> String {
    string_value(raw.get("source")).or_else_nonempty(|| "remote_http".to_string())
}

fn string_value(value: Option<&Value>) -> String {
    value
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn bool_value(value: Option<&Value>) -> bool {
    match value {
        Some(Value::Bool(value)) => *value,
        Some(Value::Number(number)) => number.as_i64().unwrap_or_default() != 0,
        Some(Value::String(value)) => matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "y" | "on"
        ),
        _ => false,
    }
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

fn im_error_to_message_error(err: im_core::ImError) -> MessageAdapterError {
    match err {
        im_core::ImError::InvalidInput { field, message }
            if field.as_deref() == Some("group")
                && message == "group owner cannot leave the group" =>
        {
            MessageAdapterError::GroupOwnerCannotLeave
        }
        im_core::ImError::InvalidInput { field, .. } if field.as_deref() == Some("group") => {
            MessageAdapterError::GroupRequired
        }
        im_core::ImError::GroupNotFound { .. } => MessageAdapterError::GroupRequired,
        im_core::ImError::CursorInvalid => {
            MessageAdapterError::PublicServiceCode("group.local_cursor_invalid".to_owned())
        }
        im_core::ImError::CursorStale => {
            MessageAdapterError::PublicServiceCode("group.local_cursor_stale".to_owned())
        }
        im_core::ImError::InventoryIncomplete => {
            MessageAdapterError::PublicServiceCode("group.local_inventory_incomplete".to_owned())
        }
        im_core::ImError::InventoryTooLarge => {
            MessageAdapterError::PublicServiceCode("group.local_inventory_too_large".to_owned())
        }
        im_core::ImError::UnsupportedCapability { capability } if capability == "group-e2ee" => {
            MessageAdapterError::GroupNotSupported
        }
        im_core::ImError::AuthRequired | im_core::ImError::SessionExpired => {
            MessageAdapterError::IdentityRequired("authentication is required".to_string())
        }
        im_core::ImError::IdentityNotReady { identity, missing } => {
            MessageAdapterError::IdentityRequired(format!(
                "identity {identity} is not ready: {}",
                missing.join(", ")
            ))
        }
        im_core::ImError::PermissionDenied => MessageAdapterError::PermissionDenied,
        im_core::ImError::LocalStateUnavailable { detail } => {
            MessageAdapterError::LocalStateUnavailable(detail)
        }
        im_core::ImError::Service {
            status_code,
            code,
            message,
            data,
        } => {
            let status_code = status_code.unwrap_or_default();
            let public_code = code
                .as_deref()
                .filter(|value| super::error::is_public_service_code(value))
                .map(str::to_owned);
            let rpc_code = code
                .as_deref()
                .and_then(|value| value.parse().ok())
                .unwrap_or_default();
            if matches!(status_code, 401 | 403) {
                return MessageAdapterError::Service(ServiceError {
                    status_code,
                    rpc_code,
                    message: "remote service request failed".to_owned(),
                    data: None,
                });
            }
            let classification_rpc_code = match public_code.as_deref() {
                Some("anp.not_supported") => {
                    service_data_json_rpc_code(data.as_ref()).unwrap_or_default()
                }
                Some(_) => 0,
                None => rpc_code,
            };
            if group_e2ee_service_unsupported(classification_rpc_code, &message) {
                return MessageAdapterError::GroupNotSupported;
            }
            if let Some(public_code) = public_code {
                return MessageAdapterError::PublicServiceCode(public_code);
            }
            MessageAdapterError::Service(ServiceError {
                status_code,
                rpc_code,
                message: "remote service request failed".to_owned(),
                data: None,
            })
        }
        im_core::ImError::TransportUnavailable { detail } => {
            MessageAdapterError::TransportUnavailable(detail)
        }
        err => MessageAdapterError::Internal(err.to_string()),
    }
}

fn service_data_json_rpc_code(data: Option<&Value>) -> Option<i64> {
    data?.as_object()?.get("json_rpc_code")?.as_i64()
}

fn group_e2ee_service_unsupported(rpc_code: i64, message: &str) -> bool {
    if rpc_code != 1405 {
        return false;
    }
    let message = message.to_ascii_lowercase();
    message.contains("group e2ee contract-test apis are disabled")
        || message.contains("group e2ee p6 apis are disabled")
}

#[cfg(test)]
mod group_message_projection_tests {
    use serde_json::json;

    use super::{
        im_error_to_message_error, is_attachment_manifest_payload,
        required_group_member_page_binding,
    };
    use crate::m_core_cli_adapter::message_result::{MessageAdapterError, ServiceError};

    #[test]
    fn group_adapter_preserves_fail_closed_error_categories() {
        assert_eq!(
            im_error_to_message_error(im_core::ImError::PermissionDenied),
            MessageAdapterError::PermissionDenied
        );
        assert_eq!(
            im_error_to_message_error(im_core::ImError::LocalStateUnavailable {
                detail: "authoritative P4 member roster is incomplete".to_owned(),
            }),
            MessageAdapterError::LocalStateUnavailable(
                "authoritative P4 member roster is incomplete".to_owned()
            )
        );
    }

    #[test]
    fn group_member_page_binding_is_host_response_owned_and_fail_closed() {
        let requested = im_core::ids::GroupRef::parse("did:example:requested").unwrap();
        let returned = im_core::ids::GroupRef::parse("did:example:requested").unwrap();
        assert_eq!(
            required_group_member_page_binding(Some(&returned), &requested).unwrap(),
            returned.as_str()
        );

        for page_group in [
            None,
            Some(im_core::ids::GroupRef::parse("did:example:conflict").unwrap()),
        ] {
            assert_eq!(
                required_group_member_page_binding(page_group.as_ref(), &requested),
                Err(MessageAdapterError::PublicServiceCode(
                    "group.local_inventory_incomplete".to_owned()
                ))
            );
        }
    }

    #[test]
    fn group_service_public_code_does_not_preserve_private_payload() {
        let private_marker = "remote-private-group-error";
        for service_code in ["anp.group_state_changed", "group.device_not_eligible"] {
            let mapped = im_error_to_message_error(im_core::ImError::Service {
                status_code: None,
                code: Some(service_code.to_owned()),
                message: private_marker.to_owned(),
                data: Some(json!({"private": private_marker})),
            });

            assert_eq!(
                mapped,
                MessageAdapterError::PublicServiceCode(service_code.to_owned())
            );
            assert!(!format!("{mapped:?}").contains(private_marker));
            assert!(!mapped.to_string().contains(private_marker));
        }
    }

    #[test]
    fn group_service_private_code_and_payload_are_not_preserved() {
        let private_marker = "remote-private-group-payload";
        for private_code in [
            "secret.token",
            "anp.token=private",
            "ANP.group_state_changed",
            "anp..group_state_changed",
        ] {
            let mapped = im_error_to_message_error(im_core::ImError::Service {
                status_code: None,
                code: Some(private_code.to_owned()),
                message: private_marker.to_owned(),
                data: Some(json!({"private": private_marker})),
            });

            assert_eq!(
                mapped,
                MessageAdapterError::Service(ServiceError {
                    status_code: 0,
                    rpc_code: 0,
                    message: "remote service request failed".to_owned(),
                    data: None,
                })
            );
            assert!(!format!("{mapped:?}").contains(private_marker));
            assert!(!mapped.to_string().contains(private_marker));
        }
    }

    #[test]
    fn group_service_auth_status_precedes_public_and_unsupported_codes() {
        let private_marker = "remote-private-group-auth-error";
        for status_code in [401, 403] {
            let mapped = im_error_to_message_error(im_core::ImError::Service {
                status_code: Some(status_code),
                code: Some("anp.not_supported".to_owned()),
                message: "group E2EE P6 APIs are disabled; remote-private-group-auth-error"
                    .to_owned(),
                data: Some(json!({
                    "json_rpc_code": 1405,
                    "private": private_marker,
                })),
            });

            assert_eq!(
                mapped,
                MessageAdapterError::Service(ServiceError {
                    status_code,
                    rpc_code: 0,
                    message: "remote service request failed".to_owned(),
                    data: None,
                })
            );
            assert!(!format!("{mapped:?}").contains(private_marker));
            assert!(!mapped.to_string().contains(private_marker));
        }
    }

    #[test]
    fn group_service_1405_keeps_e2ee_unsupported_compatibility() {
        for (code, data) in [
            ("1405", None),
            (
                "anp.not_supported",
                Some(json!({
                    "json_rpc_code": 1405,
                    "private": "must-not-be-preserved",
                })),
            ),
        ] {
            assert_eq!(
                im_error_to_message_error(im_core::ImError::Service {
                    status_code: None,
                    code: Some(code.to_owned()),
                    message: "group E2EE P6 APIs are disabled".to_owned(),
                    data,
                }),
                MessageAdapterError::GroupNotSupported
            );
        }
    }

    #[test]
    fn group_service_invalid_data_rpc_code_does_not_trigger_unsupported() {
        for json_rpc_code in [
            json!("1405"),
            json!(1405.0),
            json!(u64::MAX),
            json!(null),
            json!([1405]),
        ] {
            let mapped = im_error_to_message_error(im_core::ImError::Service {
                status_code: None,
                code: Some("anp.not_supported".to_owned()),
                message: "group E2EE P6 APIs are disabled".to_owned(),
                data: Some(json!({"json_rpc_code": json_rpc_code})),
            });

            assert_eq!(
                mapped,
                MessageAdapterError::PublicServiceCode("anp.not_supported".to_owned())
            );
        }

        assert_eq!(
            im_error_to_message_error(im_core::ImError::Service {
                status_code: None,
                code: Some("group.device_not_eligible".to_owned()),
                message: "group E2EE P6 APIs are disabled".to_owned(),
                data: Some(json!({"json_rpc_code": 1405})),
            }),
            MessageAdapterError::PublicServiceCode("group.device_not_eligible".to_owned())
        );

        assert_eq!(
            im_error_to_message_error(im_core::ImError::Service {
                status_code: None,
                code: Some("secret.not_supported".to_owned()),
                message: "group E2EE P6 APIs are disabled".to_owned(),
                data: Some(json!({"json_rpc_code": 1405})),
            }),
            MessageAdapterError::Service(ServiceError {
                status_code: 0,
                rpc_code: 0,
                message: "remote service request failed".to_owned(),
                data: None,
            })
        );
    }

    #[test]
    fn attachment_manifest_payload_requires_matching_primary_attachment() {
        assert!(is_attachment_manifest_payload(&json!({
            "attachments": [{"attachment_id": "att-1"}],
            "primary_attachment_id": "att-1"
        })));
        assert!(!is_attachment_manifest_payload(&json!({
            "attachments": [{"attachment_id": "att-1"}],
            "primary_attachment_id": "att-2"
        })));
        assert!(!is_attachment_manifest_payload(&json!({
            "attachments": [],
            "text": "ordinary payload"
        })));
    }
}
