use crate::identity::types::StoredIdentity;
use crate::message::{
    build_direct_send_rpc_params, build_group_add_rpc_params, build_group_create_rpc_params,
    build_group_get_info_rpc_params, build_group_get_rpc_params, build_group_join_rpc_params,
    build_group_leave_rpc_params, build_group_list_rpc_params, build_group_members_rpc_params,
    build_group_messages_rpc_params, build_group_remove_rpc_params, build_group_send_rpc_params,
    build_group_update_policy_rpc_params, build_group_update_profile_rpc_params,
    build_history_rpc_params, build_inbox_rpc_params, build_mark_read_rpc_params,
    GroupCreateRequest, GroupGetRequest, GroupInfoRequest, GroupJoinRequest, GroupLeaveRequest,
    GroupListRequest, GroupMemberRequest, GroupMembersRequest, GroupMessagesRequest,
    HistoryRequest, InboxRequest, MarkReadRequest,
};
use crate::runtime::bridge::BridgeRequest;
use serde_json::{Map, Value};

#[derive(Debug, Clone, PartialEq)]
pub struct BridgeRpcCall {
    pub method: String,
    pub params: Value,
    pub mark_read_message_ids: Vec<String>,
}

pub fn build_bridge_rpc_call(
    record: &StoredIdentity,
    service_did: &str,
    request: &BridgeRequest,
) -> anyhow::Result<BridgeRpcCall> {
    let params = &request.params;
    let (method, rpc_params, mark_read_message_ids) = match request.method.as_str() {
        "direct.send" => (
            "direct.send",
            build_direct_send_rpc_params(
                record,
                &string_value(params.get("target")),
                &string_value(params.get("text")),
                &string_value(params.get("type")),
            )?,
            Vec::new(),
        ),
        "inbox.get" => (
            "inbox.get",
            build_inbox_rpc_params(
                record,
                InboxRequest {
                    limit: int_value(params.get("limit")),
                    with: string_value(params.get("with")),
                    unread_only: bool_value(params.get("unread")),
                    mark_read: bool_value(params.get("mark_read")),
                    ..InboxRequest::default()
                },
            ),
            Vec::new(),
        ),
        "direct.get_history" => (
            "direct.get_history",
            build_history_rpc_params(
                record,
                HistoryRequest {
                    with: string_value(params.get("with")),
                    limit: int_value(params.get("limit")),
                    cursor: string_value(params.get("cursor")),
                    skip: int_value(params.get("skip")),
                    ..HistoryRequest::default()
                },
            )?,
            Vec::new(),
        ),
        "inbox.mark_read" => {
            let message_ids = string_array_value(params.get("message_ids"));
            (
                "inbox.mark_read",
                build_mark_read_rpc_params(
                    record,
                    MarkReadRequest {
                        message_ids: message_ids.clone(),
                        ..MarkReadRequest::default()
                    },
                )?,
                message_ids,
            )
        }
        "group.create" => (
            "group.create",
            build_group_create_rpc_params(
                record,
                service_did,
                GroupCreateRequest {
                    name: string_value(params.get("name")),
                    description: string_value(params.get("description")),
                    discoverability: string_value(params.get("discoverability")),
                    admission_mode: string_value(params.get("admission_mode")),
                    slug: string_value(params.get("slug")),
                    goal: string_value(params.get("goal")),
                    rules: string_value(params.get("rules")),
                    message_prompt: string_value(params.get("message_prompt")),
                    doc_url: string_value(params.get("doc_url")),
                    attachments_allowed: bool_ptr_value(params.get("attachments_allowed")),
                    max_members: string_value(params.get("max_members")),
                    member_max_messages: int64_ptr_value(params.get("member_max_messages")),
                    member_max_total_chars: int64_ptr_value(params.get("member_max_total_chars")),
                    ..GroupCreateRequest::default()
                },
            )?,
            Vec::new(),
        ),
        "group.get_info" => (
            "group.get_info",
            build_group_get_info_rpc_params(
                record,
                GroupInfoRequest {
                    group: string_value(params.get("group")),
                    include_policy: bool_value(params.get("include_policy")),
                    include_member_list: bool_value(params.get("include_member_list")),
                    ..GroupInfoRequest::default()
                },
            )?,
            Vec::new(),
        ),
        "group.join" => (
            "group.join",
            build_group_join_rpc_params(
                record,
                GroupJoinRequest {
                    group: string_value(params.get("group")),
                    reason_text: string_value(params.get("reason_text")),
                    ..GroupJoinRequest::default()
                },
            )?,
            Vec::new(),
        ),
        "group.add" => (
            "group.add",
            build_group_add_rpc_params(
                record,
                GroupMemberRequest {
                    group: string_value(params.get("group")),
                    member: string_value(params.get("member")),
                    role: string_value(params.get("role")),
                    reason_text: string_value(params.get("reason_text")),
                    ..GroupMemberRequest::default()
                },
            )?,
            Vec::new(),
        ),
        "group.remove" => (
            "group.remove",
            build_group_remove_rpc_params(
                record,
                GroupMemberRequest {
                    group: string_value(params.get("group")),
                    member: string_value(params.get("member")),
                    reason_text: string_value(params.get("reason_text")),
                    ..GroupMemberRequest::default()
                },
            )?,
            Vec::new(),
        ),
        "group.leave" => (
            "group.leave",
            build_group_leave_rpc_params(
                record,
                GroupLeaveRequest {
                    group: string_value(params.get("group")),
                    ..GroupLeaveRequest::default()
                },
            )?,
            Vec::new(),
        ),
        "group.update_profile" => (
            "group.update_profile",
            build_group_update_profile_rpc_params(
                record,
                &string_value(params.get("group")),
                map_value(params.get("patch")),
            )?,
            Vec::new(),
        ),
        "group.update_policy" => (
            "group.update_policy",
            build_group_update_policy_rpc_params(
                record,
                &string_value(params.get("group")),
                map_value(params.get("patch")),
            )?,
            Vec::new(),
        ),
        "group.send" => (
            "group.send",
            build_group_send_rpc_params(
                record,
                &string_value(params.get("group")),
                &string_value(params.get("text")),
                &string_value(params.get("type")),
            )?,
            Vec::new(),
        ),
        "group.get" => (
            "group.get",
            build_group_get_rpc_params(
                record,
                GroupGetRequest {
                    group: string_value(params.get("group")),
                    ..GroupGetRequest::default()
                },
            )?,
            Vec::new(),
        ),
        "group.list" => (
            "group.list",
            build_group_list_rpc_params(
                record,
                GroupListRequest {
                    limit: int_value(params.get("limit")),
                    ..GroupListRequest::default()
                },
            ),
            Vec::new(),
        ),
        "group.list_members" => (
            "group.list_members",
            build_group_members_rpc_params(
                record,
                GroupMembersRequest {
                    group: string_value(params.get("group")),
                    limit: int_value(params.get("limit")),
                    ..GroupMembersRequest::default()
                },
            )?,
            Vec::new(),
        ),
        "group.list_messages" => (
            "group.list_messages",
            build_group_messages_rpc_params(
                record,
                GroupMessagesRequest {
                    group: string_value(params.get("group")),
                    limit: int_value(params.get("limit")),
                    cursor: string_value(params.get("cursor")),
                    skip: int_value(params.get("skip")),
                    ..GroupMessagesRequest::default()
                },
            )?,
            Vec::new(),
        ),
        _ => {
            anyhow::bail!("unsupported websocket bridge method: {}", request.method);
        }
    };

    Ok(BridgeRpcCall {
        method: method.to_string(),
        params: rpc_params,
        mark_read_message_ids,
    })
}

fn string_value(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(value)) => value.clone(),
        _ => String::new(),
    }
}

fn int_value(value: Option<&Value>) -> i64 {
    match value {
        Some(Value::Number(number)) => number
            .as_i64()
            .or_else(|| number.as_u64().and_then(|value| i64::try_from(value).ok()))
            .or_else(|| number.as_f64().map(|value| value as i64))
            .unwrap_or_default(),
        _ => 0,
    }
}

fn bool_value(value: Option<&Value>) -> bool {
    match value {
        Some(Value::Bool(value)) => *value,
        Some(Value::Number(number)) => number
            .as_i64()
            .map(|value| value != 0)
            .or_else(|| number.as_u64().map(|value| value != 0))
            .or_else(|| number.as_f64().map(|value| value != 0.0))
            .unwrap_or(false),
        Some(Value::String(value)) => value == "1" || value.eq_ignore_ascii_case("true"),
        _ => false,
    }
}

fn bool_ptr_value(value: Option<&Value>) -> Option<bool> {
    match value {
        Some(Value::Bool(value)) => Some(*value),
        _ => None,
    }
}

fn int64_ptr_value(value: Option<&Value>) -> Option<i64> {
    match value {
        Some(Value::Number(number)) => number
            .as_i64()
            .or_else(|| number.as_u64().and_then(|value| i64::try_from(value).ok()))
            .or_else(|| number.as_f64().map(|value| value as i64)),
        Some(Value::String(value)) if !value.is_empty() => value.parse::<i64>().ok(),
        _ => None,
    }
}

fn string_array_value(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::Array(values)) => values
            .iter()
            .filter_map(|value| match value {
                Value::String(value) if !value.is_empty() => Some(value.clone()),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn map_value(value: Option<&Value>) -> Map<String, Value> {
    match value {
        Some(Value::Object(map)) => map.clone(),
        _ => Map::new(),
    }
}
