use super::service::{
    auth_session, int_value, refresh_jwt_fallback, require_active_identity, runtime_mode,
    string_value,
};
use super::{build_mark_read_rpc_params, websocket_http_fallback_warning, Client, CommandResult};
use super::{MarkReadRequest, MessageError, WSProxyTransport, MESSAGE_RPC_ENDPOINT};
use crate::authsdk::Session;
use crate::config::Resolved;
use crate::identity::types::StoredIdentity;
use crate::identity::Manager;
use crate::runtime;
use crate::store;
use crate::transportcfg::Profile;
use serde_json::{json, Map, Value};
use std::collections::HashMap;

pub fn mark_read(
    resolved: &Resolved,
    manager: &Manager,
    request: MarkReadRequest,
) -> Result<CommandResult, MessageError> {
    if request.message_ids.is_empty() {
        return Err(MessageError::MessageNotFound);
    }
    let record = require_active_identity(resolved, manager, &request.identity_name)?;
    let mut direct_ids = Vec::new();
    let mut group_ids = Vec::new();
    let mut local_only_ids = Vec::new();
    let connection = store::open(&resolved.paths).ok();
    if let Some(connection) = connection.as_ref() {
        let _ = store::ensure_schema(connection);
        match store::list_messages_by_ids(connection, &record.did, &request.message_ids) {
            Ok(rows) => {
                let known = rows
                    .iter()
                    .map(|row| (string_value(row.get("msg_id")), row))
                    .filter(|(id, _)| !id.is_empty())
                    .collect::<HashMap<_, _>>();
                for id in &request.message_ids {
                    if let Some(row) = known.get(id) {
                        if is_local_mail_notification_message(row) {
                            local_only_ids.push(id.clone());
                            continue;
                        }
                        if !string_value(row.get("group_did")).is_empty()
                            || !string_value(row.get("group_id")).is_empty()
                        {
                            group_ids.push(id.clone());
                            continue;
                        }
                    }
                    direct_ids.push(id.clone());
                }
            }
            Err(_) => direct_ids.extend(request.message_ids.iter().cloned()),
        }
    } else {
        direct_ids.extend(request.message_ids.iter().cloned());
    }

    let mut warnings = Vec::new();
    let mut updated_count = 0_i64;
    if !direct_ids.is_empty() {
        let remote_request = MarkReadRequest {
            identity_name: request.identity_name.clone(),
            message_ids: direct_ids.clone(),
        };
        let (raw, mut transport_warnings) =
            mark_direct_ids(resolved, manager, &record, remote_request)?;
        warnings.append(&mut transport_warnings);
        updated_count += int_value(raw.get("updated_count"), direct_ids.len() as i64);
    }

    if let Some(connection) = connection.as_ref() {
        let mut local_ids = direct_ids;
        local_ids.extend(group_ids.iter().cloned());
        local_ids.extend(local_only_ids.iter().cloned());
        if !local_ids.is_empty() {
            match store::mark_messages_read(connection, &record.did, &local_ids) {
                Ok(count) if updated_count == 0 => updated_count = count,
                Ok(_) => updated_count += (group_ids.len() + local_only_ids.len()) as i64,
                Err(err) => warnings.push(format!("Failed to mark local messages read: {err}")),
            }
        }
    }

    Ok(CommandResult {
        data: json!({
            "action": "mark_read",
            "updated_count": updated_count,
            "message_ids": request.message_ids,
        }),
        summary: format!("Marked {updated_count} messages as read"),
        warnings: super::compact_warnings(warnings),
    })
}

fn mark_direct_ids(
    resolved: &Resolved,
    manager: &Manager,
    record: &StoredIdentity,
    request: MarkReadRequest,
) -> Result<(Value, Vec<String>), MessageError> {
    if runtime_mode(resolved) != runtime::bridge::MODE_WEBSOCKET {
        return mark_read_http_with_fallback_refresh(resolved, manager, record, request)
            .map(|raw| (raw, Vec::new()));
    }
    let bridge = WSProxyTransport::new(resolved, &record.identity_name);
    match bridge.mark_read(request.clone()) {
        Ok(result) => Ok((Value::Object(result), Vec::new())),
        Err(bridge_err) => {
            let refreshed;
            let fallback_record = if super::service::is_session_unauthorized(&bridge_err) {
                refreshed = match refresh_jwt_fallback(resolved, manager, record) {
                    Ok(refreshed) => refreshed,
                    Err(_) => return Err(bridge_err),
                };
                &refreshed
            } else {
                record
            };
            let mut http = match prepare_mark_read_http_context(resolved, manager, fallback_record)
            {
                Ok(http) => http,
                Err(_) => return Err(bridge_err),
            };
            match mark_read_http_with_context(&mut http, fallback_record, request) {
                Ok(raw) => {
                    crate::traceutil::mark_fallback(
                        "websocket_to_http",
                        Some(&bridge_err.to_string()),
                    );
                    Ok((
                        raw,
                        vec![websocket_http_fallback_warning(Some(&bridge_err))],
                    ))
                }
                Err(err) => Err(err),
            }
        }
    }
}

struct MarkReadHttpContext {
    auth: Session,
    client: Client,
}

fn prepare_mark_read_http_context(
    resolved: &Resolved,
    manager: &Manager,
    record: &StoredIdentity,
) -> Result<MarkReadHttpContext, MessageError> {
    Ok(MarkReadHttpContext {
        auth: auth_session(resolved, manager, record)?,
        client: Client::new(resolved)?,
    })
}

fn mark_read_http(
    resolved: &Resolved,
    manager: &Manager,
    record: &StoredIdentity,
    request: MarkReadRequest,
) -> Result<Value, MessageError> {
    let mut http = prepare_mark_read_http_context(resolved, manager, record)?;
    mark_read_http_with_context(&mut http, record, request)
}

fn mark_read_http_with_context(
    http: &mut MarkReadHttpContext,
    record: &StoredIdentity,
    request: MarkReadRequest,
) -> Result<Value, MessageError> {
    let params = build_mark_read_rpc_params(record, request)?;
    http.client.authenticated_rpc_call_profile(
        Profile::RpcDefault,
        MESSAGE_RPC_ENDPOINT,
        "inbox.mark_read",
        params,
        &mut http.auth,
    )
}

fn mark_read_http_with_fallback_refresh(
    resolved: &Resolved,
    manager: &Manager,
    record: &StoredIdentity,
    request: MarkReadRequest,
) -> Result<Value, MessageError> {
    match mark_read_http(resolved, manager, record, request.clone()) {
        Ok(raw) => Ok(raw),
        Err(err) if super::service::is_session_unauthorized(&err) => {
            match refresh_jwt_fallback(resolved, manager, record) {
                Ok(refreshed) => mark_read_http(resolved, manager, &refreshed, request),
                Err(_) => Err(err),
            }
        }
        Err(err) => Err(err),
    }
}

fn is_local_mail_notification_message(message: &Value) -> bool {
    if string_value(message.get("content_type")).trim() == "mail.notification" {
        return true;
    }
    parse_message_metadata(message.get("metadata"))
        .get("source_kind")
        .map(|value| string_value(Some(value)).trim() == "mail")
        .unwrap_or(false)
}

fn parse_message_metadata(value: Option<&Value>) -> Map<String, Value> {
    match value {
        Some(Value::Object(metadata)) => metadata.clone(),
        Some(Value::String(metadata)) if !metadata.trim().is_empty() => {
            serde_json::from_str::<Map<String, Value>>(metadata).unwrap_or_default()
        }
        _ => Map::new(),
    }
}
