use super::service::{
    apply_inbox_filters, auth_session, collect_message_ids, mark_messages_read_in_result,
    merge_handle_history_messages, peer_handle_or_did, persist_inbox_messages,
    require_active_identity, resolve_target, runtime_mode, CommandResult, TargetResolution,
};
use super::{
    build_inbox_rpc_params, websocket_cache_fallback_warning, websocket_http_fallback_warning,
    Client, InboxRequest, MarkReadRequest, MessageError, WSProxyTransport, MESSAGE_RPC_ENDPOINT,
};
use crate::config::Resolved;
use crate::identity::types::StoredIdentity;
use crate::identity::Manager;
use crate::runtime;
use crate::store;
use crate::transportcfg::Profile;
use serde_json::{json, Value};

pub fn inbox(
    resolved: &Resolved,
    manager: &Manager,
    mut request: InboxRequest,
) -> Result<CommandResult, MessageError> {
    if request.limit <= 0 {
        request.limit = 20;
    }
    if request.scope.trim().is_empty() {
        request.scope = "all".to_string();
    }
    if request.scope.trim() == "group" || !request.group.trim().is_empty() {
        return Err(MessageError::GroupNotSupported);
    }
    let record = require_active_identity(resolved, manager, &request.identity_name)?;

    if request.scope.trim() == "all" {
        return inbox_http(resolved, manager, &record, request.clone()).map(|raw| {
            inbox_result_from_remote(
                resolved,
                manager,
                &record,
                request,
                &TargetResolution::default(),
                false,
                raw,
                Vec::new(),
            )
        });
    }

    let original_with = request.with.trim().to_string();
    let target_is_handle = !original_with.is_empty() && !original_with.starts_with("did:");
    let target = if original_with.is_empty() {
        TargetResolution::default()
    } else {
        match resolve_target(resolved, &original_with) {
            Ok(target) => target,
            Err(err) => {
                if target_is_handle {
                    if let Ok(Some(result)) = unresolved_handle_cache_fallback(
                        resolved,
                        &record,
                        &original_with,
                        request.limit,
                        request.unread_only,
                    ) {
                        return Ok(result);
                    }
                }
                return Err(err);
            }
        }
    };
    request.with = target.did.clone();
    let source_mode = runtime_mode(resolved);
    if source_mode == runtime::bridge::MODE_WEBSOCKET {
        match inbox_websocket(
            resolved,
            manager,
            &record,
            &target,
            &request,
            target_is_handle,
        )? {
            InboxTransportOutcome::Remote { raw, warnings } => {
                return Ok(inbox_result_from_remote(
                    resolved,
                    manager,
                    &record,
                    request,
                    &target,
                    target_is_handle,
                    raw,
                    warnings,
                ));
            }
            InboxTransportOutcome::LocalCache(result) => return Ok(result),
        }
    }
    let raw = inbox_http(resolved, manager, &record, request.clone())?;
    Ok(inbox_result_from_remote(
        resolved,
        manager,
        &record,
        request,
        &target,
        target_is_handle,
        raw,
        Vec::new(),
    ))
}

enum InboxTransportOutcome {
    Remote { raw: Value, warnings: Vec<String> },
    LocalCache(CommandResult),
}

fn inbox_websocket(
    resolved: &Resolved,
    manager: &Manager,
    record: &StoredIdentity,
    target: &TargetResolution,
    request: &InboxRequest,
    target_is_handle: bool,
) -> Result<InboxTransportOutcome, MessageError> {
    let bridge = WSProxyTransport::new(resolved, &record.identity_name);
    match bridge.get_inbox(request.clone()) {
        Ok(result) => Ok(InboxTransportOutcome::Remote {
            raw: Value::Object(result),
            warnings: Vec::new(),
        }),
        Err(bridge_err) => {
            if let Some(cached) =
                inbox_cache_fallback(resolved, record, target, request, target_is_handle)?
            {
                return Ok(InboxTransportOutcome::LocalCache(
                    local_cache_command_result(cached, target, &bridge_err),
                ));
            }
            match inbox_http(resolved, manager, record, request.clone()) {
                Ok(raw) => {
                    crate::traceutil::mark_fallback(
                        "websocket_to_http",
                        Some(&bridge_err.to_string()),
                    );
                    Ok(InboxTransportOutcome::Remote {
                        raw,
                        warnings: vec![websocket_http_fallback_warning(Some(&bridge_err))],
                    })
                }
                Err(err) => Err(err),
            }
        }
    }
}

fn inbox_result_from_remote(
    resolved: &Resolved,
    manager: &Manager,
    record: &StoredIdentity,
    request: InboxRequest,
    target: &TargetResolution,
    target_is_handle: bool,
    raw: Value,
    mut warnings: Vec<String>,
) -> CommandResult {
    let mut messages = persist_inbox_messages(
        resolved,
        manager,
        record,
        &raw,
        &target.handle,
        &mut warnings,
    );
    let mut source = source_with_default_for_mode(&raw, runtime_mode(resolved));
    if target_is_handle {
        merge_handle_history_messages(
            resolved,
            &record.did,
            target,
            request.limit,
            request.unread_only,
            true,
            &mut messages,
            &mut source,
            &mut warnings,
        );
    }
    let filter_peer_did = if target_is_handle { "" } else { &target.did };
    messages = apply_inbox_filters(
        messages,
        filter_peer_did,
        request.unread_only,
        request.limit,
    );
    let total = messages.len();
    if request.mark_read && !messages.is_empty() {
        let ids = collect_message_ids(&messages);
        if !ids.is_empty()
            && super::mark_read(
                resolved,
                manager,
                MarkReadRequest {
                    identity_name: record.identity_name.clone(),
                    message_ids: ids.clone(),
                },
            )
            .is_ok()
        {
            mark_messages_read_in_result(&mut messages, &ids);
        }
    }
    CommandResult {
        data: json!({
            "messages": messages,
            "total": total,
            "source": source,
            "with": peer_handle_or_did(target),
        }),
        summary: format!("Loaded {total} direct inbox messages"),
        warnings,
    }
}

fn inbox_cache_fallback(
    resolved: &Resolved,
    record: &StoredIdentity,
    target: &TargetResolution,
    request: &InboxRequest,
    target_is_handle: bool,
) -> Result<Option<Value>, MessageError> {
    let mut cached = read_inbox_from_cache(
        resolved,
        record,
        &target.did,
        request.limit,
        request.unread_only,
    );
    if target_is_handle {
        if let Ok(dids) = super::contact_sync::peer_dids_for_handle_from_store(
            resolved,
            &record.did,
            &target.handle,
            &target.did,
        ) {
            if !dids.is_empty() {
                cached = read_inbox_from_cache_by_peer_dids(
                    resolved,
                    record,
                    &dids,
                    request.limit,
                    request.unread_only,
                );
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
        summary: "Loaded inbox from local websocket cache".to_string(),
        warnings: vec![websocket_cache_fallback_warning(Some(bridge_err))],
    }
}

fn unresolved_handle_cache_fallback(
    resolved: &Resolved,
    record: &StoredIdentity,
    original_with: &str,
    limit: i64,
    unread_only: bool,
) -> Result<Option<CommandResult>, MessageError> {
    let handle = super::contact_sync::normalize_handle_value(original_with);
    let dids =
        super::contact_sync::peer_dids_for_handle_from_store(resolved, &record.did, &handle, "")?;
    if dids.is_empty() {
        return Ok(None);
    }
    let cached = read_inbox_from_cache_by_peer_dids(resolved, record, &dids, limit, unread_only)?;
    let cached = super::secure_incoming::filter_displayable_direct_e2ee_messages(cached);
    let total = cached.len();
    Ok(Some(CommandResult {
        data: json!({
            "messages": cached,
            "total": total,
            "source": "local_handle_history_cache",
            "with": peer_handle_or_did(&TargetResolution {
                did: String::new(),
                handle,
            }),
        }),
        summary: "Loaded inbox from local handle history cache".to_string(),
        warnings: Vec::new(),
    }))
}

fn inbox_http(
    resolved: &Resolved,
    manager: &Manager,
    record: &StoredIdentity,
    request: InboxRequest,
) -> Result<Value, MessageError> {
    let mut auth = auth_session(resolved, manager, record)?;
    let client = Client::new(resolved)?;
    let params = build_inbox_rpc_params(record, request);
    client.authenticated_rpc_call_profile(
        Profile::RpcReadHeavy,
        MESSAGE_RPC_ENDPOINT,
        "inbox.get",
        params,
        &mut auth,
    )
}

fn read_inbox_from_cache(
    resolved: &Resolved,
    record: &StoredIdentity,
    peer_did: &str,
    limit: i64,
    unread_only: bool,
) -> Result<Vec<Value>, MessageError> {
    let connection =
        store::open(&resolved.paths).map_err(|err| MessageError::Internal(err.to_string()))?;
    store::ensure_schema(&connection).map_err(|err| MessageError::Internal(err.to_string()))?;
    store::list_inbox_messages(
        &connection,
        &record.did,
        limit,
        peer_did,
        unread_only,
        false,
    )
    .map_err(|err| MessageError::Internal(err.to_string()))
}

fn read_inbox_from_cache_by_peer_dids(
    resolved: &Resolved,
    record: &StoredIdentity,
    peer_dids: &[String],
    limit: i64,
    unread_only: bool,
) -> Result<Vec<Value>, MessageError> {
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
        unread_only,
        true,
    )
    .map_err(|err| MessageError::Internal(err.to_string()))
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
