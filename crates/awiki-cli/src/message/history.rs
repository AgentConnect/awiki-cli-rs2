use super::service::{
    auth_session, maybe_publish_secure_prekeys, merge_handle_history_messages, peer_handle_or_did,
    persist_history_messages, require_active_identity, resolve_target, resolved_dids_value,
    runtime_mode, CommandResult, TargetResolution,
};
use super::{
    build_history_rpc_params, websocket_cache_fallback_warning, websocket_http_fallback_warning,
    Client, HistoryRequest, MessageError, WSProxyTransport, MESSAGE_RPC_ENDPOINT,
};
use crate::config::Resolved;
use crate::identity::types::StoredIdentity;
use crate::identity::Manager;
use crate::runtime;
use crate::store;
use crate::transportcfg::Profile;
use serde_json::{json, Value};

pub fn history(
    resolved: &Resolved,
    manager: &Manager,
    mut request: HistoryRequest,
) -> Result<CommandResult, MessageError> {
    if request.with.trim().is_empty() {
        return Err(MessageError::TargetRequired);
    }
    if request.limit <= 0 {
        request.limit = 50;
    }
    let record = require_active_identity(resolved, manager, &request.identity_name)?;
    let publish_warnings = maybe_publish_secure_prekeys(resolved, manager, &record);
    let original_with = request.with.trim().to_string();
    let target_is_handle = !original_with.is_empty() && !original_with.starts_with("did:");
    let target = match resolve_target(resolved, &original_with) {
        Ok(target) => target,
        Err(err) => {
            if target_is_handle {
                if let Ok(Some(result)) = unresolved_handle_cache_fallback(
                    resolved,
                    &record,
                    &original_with,
                    request.limit,
                ) {
                    return Ok(result);
                }
            }
            return Err(err);
        }
    };
    request.with = target.did.clone();
    let source_mode = runtime_mode(resolved);
    let (raw, transport_warnings) = if source_mode == runtime::bridge::MODE_WEBSOCKET {
        match history_websocket(
            resolved,
            manager,
            &record,
            &target,
            &request,
            target_is_handle,
        )? {
            HistoryTransportOutcome::Remote { raw, warnings } => (raw, warnings),
            HistoryTransportOutcome::LocalCache(result) => return Ok(result),
        }
    } else {
        (
            history_http(resolved, manager, &record, request.clone())?,
            Vec::new(),
        )
    };
    let mut warnings = publish_warnings;
    warnings.extend(transport_warnings);
    let mut messages = persist_history_messages(
        resolved,
        manager,
        &record,
        &target.did,
        &target.handle,
        &raw,
        &mut warnings,
    );
    let mut source = source_with_default_for_mode(&raw, source_mode);
    let mut resolved_dids = resolved_dids_value(&raw);
    if target_is_handle {
        let dids = merge_handle_history_messages(
            resolved,
            &record.did,
            &target,
            request.limit,
            false,
            false,
            &mut messages,
            &mut source,
            &mut warnings,
        );
        if let Some(dids) = dids {
            resolved_dids = json!(dids);
        }
    }
    let total = messages.len();
    Ok(CommandResult {
        data: json!({
            "messages": messages,
            "total": total,
            "source": source,
            "with": peer_handle_or_did(&target),
            "resolved_dids": resolved_dids,
        }),
        summary: format!("Loaded {total} direct history messages"),
        warnings,
    })
}

enum HistoryTransportOutcome {
    Remote { raw: Value, warnings: Vec<String> },
    LocalCache(CommandResult),
}

fn history_websocket(
    resolved: &Resolved,
    manager: &Manager,
    record: &StoredIdentity,
    target: &TargetResolution,
    request: &HistoryRequest,
    target_is_handle: bool,
) -> Result<HistoryTransportOutcome, MessageError> {
    let bridge = WSProxyTransport::new(resolved, &record.identity_name);
    match bridge.get_history(request.clone()) {
        Ok(result) => Ok(HistoryTransportOutcome::Remote {
            raw: Value::Object(result),
            warnings: Vec::new(),
        }),
        Err(bridge_err) => {
            if let Some(cached) =
                history_cache_fallback(resolved, record, target, request.limit, target_is_handle)?
            {
                return Ok(HistoryTransportOutcome::LocalCache(
                    local_cache_command_result(cached, target, &bridge_err),
                ));
            }
            match history_http(resolved, manager, record, request.clone()) {
                Ok(raw) => {
                    crate::traceutil::mark_fallback(
                        "websocket_to_http",
                        Some(&bridge_err.to_string()),
                    );
                    Ok(HistoryTransportOutcome::Remote {
                        raw,
                        warnings: vec![websocket_http_fallback_warning(Some(&bridge_err))],
                    })
                }
                Err(err) => Err(err),
            }
        }
    }
}

fn history_cache_fallback(
    resolved: &Resolved,
    record: &StoredIdentity,
    target: &TargetResolution,
    limit: i64,
    target_is_handle: bool,
) -> Result<Option<Value>, MessageError> {
    let mut cached = read_history_from_cache(resolved, record, &target.did, limit);
    if target_is_handle {
        if let Ok(dids) = super::contact_sync::peer_dids_for_handle_from_store(
            resolved,
            &record.did,
            &target.handle,
            &target.did,
        ) {
            if !dids.is_empty() {
                cached = read_history_from_cache_by_peer_dids(resolved, record, &dids, limit);
            }
        }
    }
    let Ok(cached) = cached else {
        return Ok(None);
    };
    if cached.is_empty() || contains_direct_e2ee_wire_messages(&cached) {
        return Ok(None);
    }
    let cached = super::secure_incoming::filter_displayable_direct_e2ee_messages(cached);
    if cached.is_empty() {
        return Ok(None);
    }
    let total = cached.len();
    Ok(Some(json!({
        "messages": cached,
        "total": total,
        "source": "local_ws_cache_fallback",
        "with": peer_handle_or_did(target),
    })))
}

fn local_cache_command_result(
    data: Value,
    target: &TargetResolution,
    bridge_err: &MessageError,
) -> CommandResult {
    let total = data
        .get("messages")
        .and_then(Value::as_array)
        .map(|messages| messages.len())
        .unwrap_or(0);
    let mut data = data;
    if let Some(object) = data.as_object_mut() {
        object.insert("total".to_string(), json!(total));
        object.insert("source".to_string(), json!("local_ws_cache_fallback"));
        object.insert("with".to_string(), json!(peer_handle_or_did(target)));
    }
    CommandResult {
        data,
        summary: "Loaded history from local websocket cache".to_string(),
        warnings: vec![websocket_cache_fallback_warning(Some(bridge_err))],
    }
}

fn unresolved_handle_cache_fallback(
    resolved: &Resolved,
    record: &StoredIdentity,
    original_with: &str,
    limit: i64,
) -> Result<Option<CommandResult>, MessageError> {
    let handle = super::contact_sync::normalize_handle_value(original_with);
    let dids =
        super::contact_sync::peer_dids_for_handle_from_store(resolved, &record.did, &handle, "")?;
    if dids.is_empty() {
        return Ok(None);
    }
    let cached = read_history_from_cache_by_peer_dids(resolved, record, &dids, limit)?;
    let cached = super::secure_incoming::filter_displayable_direct_e2ee_messages(cached);
    let total = cached.len();
    Ok(Some(CommandResult {
        data: json!({
            "messages": cached,
            "total": total,
            "source": "local_handle_history_cache",
            "with": handle,
            "resolved_dids": dids,
        }),
        summary: "Loaded history from local handle history cache".to_string(),
        warnings: Vec::new(),
    }))
}

fn history_http(
    resolved: &Resolved,
    manager: &Manager,
    record: &StoredIdentity,
    request: HistoryRequest,
) -> Result<Value, MessageError> {
    let mut auth = auth_session(resolved, manager, record)?;
    let client = Client::new(resolved)?;
    let params = build_history_rpc_params(record, request)?;
    client.authenticated_rpc_call_profile(
        Profile::RpcReadHeavy,
        MESSAGE_RPC_ENDPOINT,
        "direct.get_history",
        params,
        &mut auth,
    )
}

fn read_history_from_cache(
    resolved: &Resolved,
    record: &StoredIdentity,
    peer_did: &str,
    limit: i64,
) -> Result<Vec<Value>, MessageError> {
    let mut phase = crate::traceutil::local_db_phase("read_history_cache");
    let result = (|| {
        let connection =
            store::open(&resolved.paths).map_err(|err| MessageError::Internal(err.to_string()))?;
        store::ensure_schema(&connection).map_err(|err| MessageError::Internal(err.to_string()))?;
        let thread_id = store::make_thread_id(&record.did, peer_did, "");
        store::list_thread_messages(&connection, &record.did, &thread_id, limit)
            .map_err(|err| MessageError::Internal(err.to_string()))
    })();
    phase.finish();
    result
}

fn read_history_from_cache_by_peer_dids(
    resolved: &Resolved,
    record: &StoredIdentity,
    peer_dids: &[String],
    limit: i64,
) -> Result<Vec<Value>, MessageError> {
    let mut phase = crate::traceutil::local_db_phase("read_history_cache_by_peer_dids");
    let result = (|| {
        if peer_dids.is_empty() {
            return Ok(Vec::new());
        }
        let connection =
            store::open(&resolved.paths).map_err(|err| MessageError::Internal(err.to_string()))?;
        store::ensure_schema(&connection).map_err(|err| MessageError::Internal(err.to_string()))?;
        store::list_direct_messages_by_peer_dids(
            &connection,
            &record.did,
            peer_dids,
            limit,
            false,
            false,
        )
        .map_err(|err| MessageError::Internal(err.to_string()))
    })();
    phase.finish();
    result
}

fn contains_direct_e2ee_wire_messages(messages: &[Value]) -> bool {
    messages.iter().any(|message| {
        message
            .get("content_type")
            .and_then(Value::as_str)
            .map(super::is_direct_e2ee_wire_content_type)
            .unwrap_or(false)
    })
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
