use super::group_e2ee_decrypt::maybe_decrypt_group_messages;
use super::group_e2ee_send::maybe_send_group_e2ee;
use super::group_service::{
    cached_group_messages, compact_warnings, group_send_message_id, persist_group_messages,
    persist_group_send_result, values_from_array, GroupSendResult,
};
use super::service::{
    auth_session, bool_value, default_message_type, int_value, require_active_identity,
    runtime_mode, CommandResult,
};
use super::{
    build_group_messages_rpc_params, build_group_send_rpc_params, websocket_cache_fallback_warning,
    websocket_http_fallback_warning, Client, GroupMessagesRequest, MessageError, SendRequest,
    WSProxyTransport, MESSAGE_RPC_ENDPOINT,
};
use crate::authsdk::Session;
use crate::config::Resolved;
use crate::identity::types::StoredIdentity;
use crate::identity::Manager;
use crate::runtime;
use crate::transportcfg::Profile;
use serde_json::{json, Value};

pub(crate) fn group_messages(
    resolved: &Resolved,
    manager: &Manager,
    request: GroupMessagesRequest,
) -> Result<CommandResult, MessageError> {
    if request.group.trim().is_empty() {
        return Err(MessageError::GroupRequired);
    }
    let record = require_active_identity(resolved, manager, &request.identity_name)?;
    let source_mode = runtime_mode(resolved);
    let (mut raw, mut warnings, result_source_mode) =
        if source_mode == runtime::bridge::MODE_WEBSOCKET {
            match group_messages_websocket(resolved, manager, &record, &request)? {
                GroupMessagesTransportOutcome::Remote {
                    raw,
                    warnings,
                    source_mode,
                } => (raw, warnings, source_mode),
                GroupMessagesTransportOutcome::LocalCache(result) => return Ok(result),
            }
        } else {
            (
                group_messages_http(resolved, manager, &record, request.clone())?,
                Vec::new(),
                source_mode,
            )
        };

    warnings.extend(maybe_decrypt_group_messages(
        resolved,
        &record,
        &request.group,
        &mut raw,
    ));
    warnings.extend(persist_group_messages(
        resolved,
        &record,
        &request.group,
        &raw,
    ));
    let messages = cached_group_messages(
        resolved,
        &record,
        &request.group,
        request.limit,
        &request.cursor,
    )
    .filter(|items| !items.is_empty())
    .unwrap_or_else(|| values_from_array(raw.get("messages")));
    let total = int_value(raw.get("total"), messages.len() as i64);
    Ok(CommandResult {
        data: json!({
            "group": request.group,
            "messages": messages,
            "total": total,
            "has_more": bool_value(raw.get("has_more")),
            "next_since_seq": raw.get("next_since_seq").cloned().unwrap_or(Value::Null),
            "source": source_with_default_for_mode(&raw, result_source_mode),
        }),
        summary: format!("Loaded {total} group messages"),
        warnings: compact_warnings(&mut warnings),
    })
}

pub(crate) fn send_group(
    resolved: &Resolved,
    manager: &Manager,
    request: SendRequest,
) -> Result<CommandResult, MessageError> {
    if request.has_attachment() {
        return Err(MessageError::AttachmentNotSupported);
    }
    if request.group.trim().is_empty() {
        return Err(MessageError::GroupRequired);
    }
    if request.text.trim().is_empty() {
        return Err(MessageError::TextRequired);
    }
    let record = require_active_identity(resolved, manager, &request.identity_name)?;
    if let Some(result) = maybe_send_group_e2ee(resolved, manager, &record, &request)? {
        return Ok(result);
    }

    let message_type = default_message_type(&request.message_type).to_string();
    let source_mode = runtime_mode(resolved);
    let (mut result, mut warnings, result_source_mode) =
        if source_mode == runtime::bridge::MODE_WEBSOCKET {
            send_group_websocket(resolved, manager, &record, &request)?
        } else {
            (
                send_group_http(resolved, manager, &record, &request, &message_type)?,
                Vec::new(),
                source_mode,
            )
        };
    if result.group_did.trim().is_empty() {
        result.group_did = request.group.clone();
    }
    warnings.extend(persist_group_send_result(
        resolved,
        &record,
        &request,
        &message_type,
        &result,
    ));
    let message_id = group_send_message_id(&request.group, &result);
    Ok(CommandResult {
        data: json!({
            "action": "send_message",
            "target": {
                "kind": "group",
                "did": request.group,
            },
            "message": {
                "id": message_id,
                "type": message_type,
                "secure": false,
                "sent_at": result.accepted_at,
            },
            "delivery": result,
            "source": transport_source(result_source_mode),
        }),
        summary: format!("Sent a group {message_type} message"),
        warnings: compact_warnings(&mut warnings),
    })
}

enum GroupMessagesTransportOutcome {
    Remote {
        raw: Value,
        warnings: Vec<String>,
        source_mode: &'static str,
    },
    LocalCache(CommandResult),
}

fn group_messages_websocket(
    resolved: &Resolved,
    manager: &Manager,
    record: &StoredIdentity,
    request: &GroupMessagesRequest,
) -> Result<GroupMessagesTransportOutcome, MessageError> {
    let bridge = WSProxyTransport::new(resolved, &record.identity_name);
    match bridge.list_group_messages(request.clone()) {
        Ok(result) => Ok(GroupMessagesTransportOutcome::Remote {
            raw: Value::Object(result),
            warnings: Vec::new(),
            source_mode: runtime::bridge::MODE_WEBSOCKET,
        }),
        Err(bridge_err) => {
            if let Some(cached) = cached_group_messages(
                resolved,
                record,
                &request.group,
                request.limit,
                &request.cursor,
            )
            .filter(|items| !items.is_empty())
            {
                return Ok(GroupMessagesTransportOutcome::LocalCache(
                    group_messages_local_cache_result(request, cached, &bridge_err),
                ));
            }
            let mut http = match prepare_group_http_context(resolved, manager, record) {
                Ok(http) => http,
                Err(_) => return Err(bridge_err),
            };
            match group_messages_http_with_context(&mut http, record, request.clone()) {
                Ok(raw) => {
                    crate::traceutil::mark_fallback(
                        "websocket_to_http",
                        Some(&bridge_err.to_string()),
                    );
                    Ok(GroupMessagesTransportOutcome::Remote {
                        raw,
                        warnings: vec![websocket_http_fallback_warning(Some(&bridge_err))],
                        source_mode: runtime::bridge::MODE_HTTP,
                    })
                }
                Err(err) => Err(err),
            }
        }
    }
}

fn send_group_websocket(
    resolved: &Resolved,
    manager: &Manager,
    record: &StoredIdentity,
    request: &SendRequest,
) -> Result<(GroupSendResult, Vec<String>, &'static str), MessageError> {
    let bridge = WSProxyTransport::new(resolved, &record.identity_name);
    match bridge.send_group(request.clone()) {
        Ok(result) => Ok((
            group_send_result_from_bridge(result),
            Vec::new(),
            runtime::bridge::MODE_WEBSOCKET,
        )),
        Err(bridge_err) => {
            let mut http = match prepare_group_http_context(resolved, manager, record) {
                Ok(http) => http,
                Err(_) => return Err(bridge_err),
            };
            match send_group_http_with_context(
                &mut http,
                record,
                request,
                default_message_type(&request.message_type),
            ) {
                Ok(result) => {
                    crate::traceutil::mark_fallback(
                        "websocket_to_http",
                        Some(&bridge_err.to_string()),
                    );
                    Ok((
                        result,
                        vec![websocket_http_fallback_warning(Some(&bridge_err))],
                        runtime::bridge::MODE_HTTP,
                    ))
                }
                Err(err) => Err(err),
            }
        }
    }
}

struct GroupHttpContext {
    auth: Session,
    client: Client,
}

fn prepare_group_http_context(
    resolved: &Resolved,
    manager: &Manager,
    record: &StoredIdentity,
) -> Result<GroupHttpContext, MessageError> {
    Ok(GroupHttpContext {
        auth: auth_session(resolved, manager, record)?,
        client: Client::new(resolved)?,
    })
}

fn group_messages_http(
    resolved: &Resolved,
    manager: &Manager,
    record: &StoredIdentity,
    request: GroupMessagesRequest,
) -> Result<Value, MessageError> {
    let mut http = prepare_group_http_context(resolved, manager, record)?;
    group_messages_http_with_context(&mut http, record, request)
}

fn group_messages_http_with_context(
    http: &mut GroupHttpContext,
    record: &StoredIdentity,
    request: GroupMessagesRequest,
) -> Result<Value, MessageError> {
    let params = build_group_messages_rpc_params(record, request)?;
    http.client.authenticated_rpc_call_profile(
        Profile::RpcReadHeavy,
        MESSAGE_RPC_ENDPOINT,
        "group.list_messages",
        params,
        &mut http.auth,
    )
}

fn send_group_http(
    resolved: &Resolved,
    manager: &Manager,
    record: &StoredIdentity,
    request: &SendRequest,
    message_type: &str,
) -> Result<GroupSendResult, MessageError> {
    let mut http = prepare_group_http_context(resolved, manager, record)?;
    send_group_http_with_context(&mut http, record, request, message_type)
}

fn send_group_http_with_context(
    http: &mut GroupHttpContext,
    record: &StoredIdentity,
    request: &SendRequest,
    message_type: &str,
) -> Result<GroupSendResult, MessageError> {
    let params = build_group_send_rpc_params(record, &request.group, &request.text, message_type)?;
    http.client.authenticated_rpc_call_profile(
        Profile::RpcDefault,
        MESSAGE_RPC_ENDPOINT,
        "group.send",
        params,
        &mut http.auth,
    )
}

fn group_messages_local_cache_result(
    request: &GroupMessagesRequest,
    messages: Vec<Value>,
    bridge_err: &MessageError,
) -> CommandResult {
    let total = messages.len();
    CommandResult {
        data: json!({
            "group": request.group,
            "messages": messages,
            "total": total,
            "source": "local_ws_cache_fallback",
        }),
        summary: "Loaded group messages from local cache".to_string(),
        warnings: vec![websocket_cache_fallback_warning(Some(bridge_err))],
    }
}

fn group_send_result_from_bridge(result: super::ws_proxy::GroupSendResult) -> GroupSendResult {
    GroupSendResult {
        accepted: result.accepted,
        final_acceptance: result.final_acceptance,
        group_did: result.group_did,
        message_id: result.message_id,
        operation_id: result.operation_id,
        group_event_seq: result.group_event_seq,
        group_state_version: result.group_state_version,
        accepted_at: result.accepted_at,
    }
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

fn transport_source(mode: &str) -> &'static str {
    if mode == runtime::bridge::MODE_WEBSOCKET {
        "local_ws_cache"
    } else {
        "remote_http"
    }
}
