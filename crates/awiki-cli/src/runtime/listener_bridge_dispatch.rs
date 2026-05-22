use crate::identity::types::StoredIdentity;
use crate::runtime::bridge::BridgeRequest;
// Migration-only websocket bridge wire shape; remove when listener bridge
// dispatch no longer builds raw message RPC calls.
use im_core::compat::wire::{
    self, BridgeWireIdentity, GroupCreateWireRequest, HistoryWireRequest, InboxWireRequest,
    MarkReadWireRequest, WireIdentity,
};
use serde_json::{Map, Value};

#[derive(Debug, Clone, PartialEq)]
pub struct BridgeRpcCall {
    pub method: String,
    pub params: Value,
    pub mark_read_message_ids: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BridgeSessionSnapshot {
    pub identity_name: String,
    pub record_did: Option<String>,
    pub has_client: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BridgeEnsureSessionOutcome {
    Ok(BridgeSessionSnapshot),
    Error(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BridgeServiceDidOutcome {
    Ok { service_did: String },
    Error(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BridgeRpcBuildOutcome {
    Ok {
        method: String,
        mark_read_message_ids: Vec<String>,
    },
    Error(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum BridgeSendRpcOutcome {
    Ok { result: Map<String, Value> },
    Error(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BridgeRequestFlowAction {
    EnsureSession {
        identity_name: String,
    },
    ReadCurrentRecord {
        identity_name: String,
    },
    ReadCurrentClient {
        identity_name: String,
    },
    FetchMessageServiceDID {
        identity_name: String,
    },
    BuildRpcCall {
        method: String,
        service_did: Option<String>,
    },
    SendRpc {
        method: String,
    },
    MarkMessagesRead {
        owner_did: String,
        message_ids: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum BridgeRequestFlowDecision {
    ReturnOk { result: Map<String, Value> },
    ReturnError(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct BridgeRequestFlowPlan {
    pub actions: Vec<BridgeRequestFlowAction>,
    pub decision: BridgeRequestFlowDecision,
}

pub fn bridge_request_flow_plan(
    request: &BridgeRequest,
    ensure_session: BridgeEnsureSessionOutcome,
    service_did: BridgeServiceDidOutcome,
    build_rpc: BridgeRpcBuildOutcome,
    send_rpc: BridgeSendRpcOutcome,
) -> BridgeRequestFlowPlan {
    let mut actions = vec![BridgeRequestFlowAction::EnsureSession {
        identity_name: request.identity_name.clone(),
    }];
    let session = match ensure_session {
        BridgeEnsureSessionOutcome::Ok(session) => session,
        BridgeEnsureSessionOutcome::Error(error) => {
            return BridgeRequestFlowPlan {
                actions,
                decision: BridgeRequestFlowDecision::ReturnError(error),
            };
        }
    };

    actions.push(BridgeRequestFlowAction::ReadCurrentRecord {
        identity_name: session.identity_name.clone(),
    });
    actions.push(BridgeRequestFlowAction::ReadCurrentClient {
        identity_name: session.identity_name.clone(),
    });
    let Some(record_did) = session.record_did else {
        return disconnected_bridge_session_plan(actions, &session.identity_name);
    };
    if !session.has_client {
        return disconnected_bridge_session_plan(actions, &session.identity_name);
    }

    let service_did = if request.method == "group.create" {
        actions.push(BridgeRequestFlowAction::FetchMessageServiceDID {
            identity_name: session.identity_name.clone(),
        });
        match service_did {
            BridgeServiceDidOutcome::Ok { service_did } => Some(service_did),
            BridgeServiceDidOutcome::Error(error) => {
                return BridgeRequestFlowPlan {
                    actions,
                    decision: BridgeRequestFlowDecision::ReturnError(error),
                };
            }
        }
    } else {
        None
    };

    actions.push(BridgeRequestFlowAction::BuildRpcCall {
        method: request.method.clone(),
        service_did,
    });
    let (method, mark_read_message_ids) = match build_rpc {
        BridgeRpcBuildOutcome::Ok {
            method,
            mark_read_message_ids,
        } => (method, mark_read_message_ids),
        BridgeRpcBuildOutcome::Error(error) => {
            return BridgeRequestFlowPlan {
                actions,
                decision: BridgeRequestFlowDecision::ReturnError(error),
            };
        }
    };

    actions.push(BridgeRequestFlowAction::SendRpc {
        method: method.clone(),
    });
    let result = match send_rpc {
        BridgeSendRpcOutcome::Ok { result } => result,
        BridgeSendRpcOutcome::Error(error) => {
            return BridgeRequestFlowPlan {
                actions,
                decision: BridgeRequestFlowDecision::ReturnError(error),
            };
        }
    };

    if request.method == "inbox.mark_read" {
        actions.push(BridgeRequestFlowAction::MarkMessagesRead {
            owner_did: record_did,
            message_ids: mark_read_message_ids,
        });
    }

    BridgeRequestFlowPlan {
        actions,
        decision: BridgeRequestFlowDecision::ReturnOk { result },
    }
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
            wire::build_bridge_direct_send_rpc_params(
                &bridge_wire_identity(record),
                &string_value(params.get("target")),
                &string_value(params.get("text")),
                &string_value(params.get("type")),
            )?,
            Vec::new(),
        ),
        "inbox.get" => (
            "inbox.get",
            wire::build_inbox_rpc_params(
                &wire_identity(record),
                InboxWireRequest {
                    limit: int_value(params.get("limit")),
                },
            ),
            Vec::new(),
        ),
        "direct.get_history" => (
            "direct.get_history",
            wire::build_history_rpc_params(
                &wire_identity(record),
                HistoryWireRequest {
                    peer_did: string_value(params.get("with")),
                    limit: int_value(params.get("limit")),
                    cursor: optional_string_value(params.get("cursor")),
                    skip: int_value(params.get("skip")),
                },
            )?,
            Vec::new(),
        ),
        "inbox.mark_read" => {
            let message_ids = string_array_value(params.get("message_ids"));
            (
                "inbox.mark_read",
                wire::build_mark_read_rpc_params(
                    &wire_identity(record),
                    MarkReadWireRequest {
                        message_ids: message_ids.clone(),
                    },
                )?,
                message_ids,
            )
        }
        "group.create" => (
            "group.create",
            wire::build_bridge_group_create_rpc_params(
                &bridge_wire_identity(record),
                service_did,
                GroupCreateWireRequest {
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
                    message_security_profile: String::new(),
                    e2ee: false,
                },
            )?,
            Vec::new(),
        ),
        "group.get_info" => (
            "group.get_info",
            wire::build_bridge_group_get_info_rpc_params(
                &bridge_wire_identity(record),
                &string_value(params.get("group")),
                bool_value(params.get("include_policy")),
                bool_value(params.get("include_member_list")),
            )?,
            Vec::new(),
        ),
        "group.join" => (
            "group.join",
            wire::build_bridge_group_join_rpc_params(
                &bridge_wire_identity(record),
                &string_value(params.get("group")),
                &string_value(params.get("reason_text")),
            )?,
            Vec::new(),
        ),
        "group.add" => (
            "group.add",
            wire::build_bridge_group_add_rpc_params(
                &bridge_wire_identity(record),
                &string_value(params.get("group")),
                &string_value(params.get("member")),
                &string_value(params.get("role")),
                &string_value(params.get("reason_text")),
            )?,
            Vec::new(),
        ),
        "group.remove" => (
            "group.remove",
            wire::build_bridge_group_remove_rpc_params(
                &bridge_wire_identity(record),
                &string_value(params.get("group")),
                &string_value(params.get("member")),
                &string_value(params.get("reason_text")),
            )?,
            Vec::new(),
        ),
        "group.leave" => (
            "group.leave",
            wire::build_bridge_group_leave_rpc_params(
                &bridge_wire_identity(record),
                &string_value(params.get("group")),
            )?,
            Vec::new(),
        ),
        "group.update_profile" => (
            "group.update_profile",
            wire::build_bridge_group_update_profile_rpc_params(
                &bridge_wire_identity(record),
                &string_value(params.get("group")),
                map_value(params.get("patch")),
            )?,
            Vec::new(),
        ),
        "group.update_policy" => (
            "group.update_policy",
            wire::build_bridge_group_update_policy_rpc_params(
                &bridge_wire_identity(record),
                &string_value(params.get("group")),
                map_value(params.get("patch")),
            )?,
            Vec::new(),
        ),
        "group.send" => (
            "group.send",
            wire::build_bridge_group_send_rpc_params(
                &bridge_wire_identity(record),
                &string_value(params.get("group")),
                &string_value(params.get("text")),
                &string_value(params.get("type")),
            )?,
            Vec::new(),
        ),
        "group.get" => (
            "group.get",
            wire::build_group_get_rpc_params(&record.did, &string_value(params.get("group")))?,
            Vec::new(),
        ),
        "group.list" => (
            "group.list",
            wire::build_group_list_rpc_params(&record.did, int_value(params.get("limit"))),
            Vec::new(),
        ),
        "group.list_members" => (
            "group.list_members",
            wire::build_group_members_rpc_params(
                &record.did,
                &string_value(params.get("group")),
                int_value(params.get("limit")),
            )?,
            Vec::new(),
        ),
        "group.list_messages" => (
            "group.list_messages",
            wire::build_group_messages_rpc_params(
                &record.did,
                &string_value(params.get("group")),
                int_value(params.get("limit")),
                optional_string_value(params.get("cursor")).as_deref(),
                int_value(params.get("skip")),
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

fn disconnected_bridge_session_plan(
    actions: Vec<BridgeRequestFlowAction>,
    identity_name: &str,
) -> BridgeRequestFlowPlan {
    BridgeRequestFlowPlan {
        actions,
        decision: BridgeRequestFlowDecision::ReturnError(format!(
            "websocket session is not connected for identity {identity_name}"
        )),
    }
}

fn string_value(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(value)) => value.clone(),
        _ => String::new(),
    }
}

fn optional_string_value(value: Option<&Value>) -> Option<String> {
    match value {
        Some(Value::String(value)) if !value.is_empty() => Some(value.clone()),
        _ => None,
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

fn wire_identity(record: &StoredIdentity) -> WireIdentity {
    WireIdentity {
        did: record.did.clone(),
    }
}

fn bridge_wire_identity(record: &StoredIdentity) -> BridgeWireIdentity {
    BridgeWireIdentity {
        identity_name: record.identity_name.clone(),
        did: record.did.clone(),
        did_document: record.did_document.clone(),
        key1_private_pem: record.key1_private_pem.clone(),
    }
}
