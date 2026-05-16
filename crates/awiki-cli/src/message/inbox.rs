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
        return all_inbox(resolved, manager, &record, request);
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

fn all_inbox(
    resolved: &Resolved,
    manager: &Manager,
    record: &StoredIdentity,
    request: InboxRequest,
) -> Result<CommandResult, MessageError> {
    let mut warnings = Vec::new();
    let group_messages =
        match read_all_group_inbox_from_cache(resolved, record, request.limit, request.unread_only)
        {
            Ok(messages) => messages,
            Err(err) => {
                warnings.push(format!("Failed to read local group inbox cache: {err}"));
                Vec::new()
            }
        };

    let mut direct_messages = None;
    let mut source = "local_direct_cache+local_group_cache".to_string();
    if runtime_mode(resolved) == runtime::bridge::MODE_WEBSOCKET {
        match read_unified_direct_inbox_from_cache(
            resolved,
            record,
            request.limit,
            request.unread_only,
        ) {
            Ok(messages) => direct_messages = Some(normalize_mail_notification_messages(messages)),
            Err(err) => warnings.push(format!("Failed to read local direct inbox cache: {err}")),
        }
    }

    let direct_messages = match direct_messages {
        Some(messages) => messages,
        None => {
            let mut direct_request = request.clone();
            direct_request.scope = "direct".to_string();
            direct_request.group.clear();
            let direct_result = inbox(resolved, manager, direct_request)?;
            let mail_notifications = match read_all_mail_notifications_from_cache(
                resolved,
                record,
                request.limit,
                request.unread_only,
            ) {
                Ok(messages) => messages,
                Err(err) => {
                    warnings.push(format!(
                        "Failed to read local mail notification cache: {err}"
                    ));
                    Vec::new()
                }
            };
            warnings.extend(direct_result.warnings);
            source = "remote_http+local_group_cache+local_mail_cache".to_string();
            merge_inbox_messages(
                request.limit,
                normalize_mail_notification_messages(messages_from_data(&direct_result.data)),
                normalize_mail_notification_messages(mail_notifications),
            )
        }
    };

    let mut merged = merge_inbox_messages(request.limit, direct_messages, group_messages);
    if request.mark_read && !merged.is_empty() {
        let ids = collect_message_ids(&merged);
        if !ids.is_empty() {
            let _ = super::mark_read(
                resolved,
                manager,
                MarkReadRequest {
                    identity_name: record.identity_name.clone(),
                    message_ids: ids.clone(),
                },
            );
            for message in &mut merged {
                if let Some(object) = message.as_object_mut() {
                    object.insert("is_read".to_string(), Value::Bool(true));
                }
            }
        }
    }
    let total = merged.len();
    Ok(CommandResult {
        data: json!({
            "messages": merged,
            "total": total,
            "source": source,
        }),
        summary: format!("Loaded {total} inbox messages"),
        warnings: super::compact_warnings(warnings),
    })
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

fn read_unified_direct_inbox_from_cache(
    resolved: &Resolved,
    record: &StoredIdentity,
    limit: i64,
    unread_only: bool,
) -> Result<Vec<Value>, MessageError> {
    let connection =
        store::open(&resolved.paths).map_err(|err| MessageError::Internal(err.to_string()))?;
    store::ensure_schema(&connection).map_err(|err| MessageError::Internal(err.to_string()))?;
    store::list_inbox_messages(&connection, &record.did, limit, "", unread_only, true)
        .map_err(|err| MessageError::Internal(err.to_string()))
}

fn read_all_group_inbox_from_cache(
    resolved: &Resolved,
    record: &StoredIdentity,
    limit: i64,
    unread_only: bool,
) -> Result<Vec<Value>, MessageError> {
    let connection =
        store::open(&resolved.paths).map_err(|err| MessageError::Internal(err.to_string()))?;
    store::ensure_schema(&connection).map_err(|err| MessageError::Internal(err.to_string()))?;
    store::list_group_inbox_messages(&connection, &record.did, limit, "", unread_only)
        .map_err(|err| MessageError::Internal(err.to_string()))
}

fn read_all_mail_notifications_from_cache(
    resolved: &Resolved,
    record: &StoredIdentity,
    limit: i64,
    unread_only: bool,
) -> Result<Vec<Value>, MessageError> {
    let connection =
        store::open(&resolved.paths).map_err(|err| MessageError::Internal(err.to_string()))?;
    store::ensure_schema(&connection).map_err(|err| MessageError::Internal(err.to_string()))?;
    store::list_notification_inbox_messages(&connection, &record.did, limit, unread_only)
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

fn messages_from_data(data: &Value) -> Vec<Value> {
    data.get("messages")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

fn merge_inbox_messages(limit: i64, left: Vec<Value>, right: Vec<Value>) -> Vec<Value> {
    let mut all = Vec::with_capacity(left.len() + right.len());
    all.extend(left);
    all.extend(right);
    if all.len() > 1 {
        all.sort_by(|left, right| message_sort_time(right).cmp(&message_sort_time(left)));
    }
    if limit > 0 && all.len() > limit as usize {
        all.truncate(limit as usize);
    }
    all
}

fn message_sort_time(message: &Value) -> String {
    message
        .get("sent_at")
        .or_else(|| message.get("stored_at"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn normalize_mail_notification_messages(messages: Vec<Value>) -> Vec<Value> {
    messages
        .into_iter()
        .map(normalize_mail_notification_message)
        .collect()
}

fn normalize_mail_notification_message(message: Value) -> Value {
    let Some(object) = message.as_object() else {
        return message;
    };
    if !is_local_mail_notification_message(object) {
        return Value::Object(object.clone());
    }
    let metadata = parse_message_metadata(object.get("metadata"));
    let mut mailbox_address = default_string(
        metadata.get("mailbox_address").and_then(Value::as_str),
        object
            .get("thread_id")
            .and_then(Value::as_str)
            .unwrap_or_default(),
    );
    if let Some(stripped) = mailbox_address.strip_prefix("mail:") {
        mailbox_address = stripped.to_string();
    }
    let mut subject = default_string(
        metadata.get("subject").and_then(Value::as_str),
        object
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or_default(),
    );
    if let Some(stripped) = subject.strip_prefix("[邮件] ") {
        subject = stripped.to_string();
    }
    if subject.trim().is_empty() {
        subject = "(no subject)".to_string();
    }
    let from_addr = metadata
        .get("from_addr")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let preview = metadata
        .get("preview")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let has_attachments = metadata.get("has_attachments").is_some_and(bool_value_ref);

    let mut normalized = object.clone();
    normalized.insert("source_kind".to_string(), json!("mail"));
    normalized.insert("title".to_string(), json!(format!("[邮件] {subject}")));
    normalized.insert(
        "content".to_string(),
        json!(build_normalized_mail_notification_content(
            &mailbox_address,
            from_addr,
            &subject,
            preview,
            has_attachments,
        )),
    );
    Value::Object(normalized)
}

fn is_local_mail_notification_message(object: &serde_json::Map<String, Value>) -> bool {
    if object
        .get("content_type")
        .and_then(Value::as_str)
        .is_some_and(|value| value.trim() == "mail.notification")
    {
        return true;
    }
    let metadata = parse_message_metadata(object.get("metadata"));
    metadata
        .get("source_kind")
        .and_then(Value::as_str)
        .is_some_and(|value| value.trim() == "mail")
}

fn parse_message_metadata(value: Option<&Value>) -> serde_json::Map<String, Value> {
    match value {
        Some(Value::Object(object)) => object.clone(),
        Some(Value::String(text)) if !text.trim().is_empty() => serde_json::from_str::<Value>(text)
            .ok()
            .and_then(|value| value.as_object().cloned())
            .unwrap_or_default(),
        _ => serde_json::Map::new(),
    }
}

fn build_normalized_mail_notification_content(
    mailbox_address: &str,
    from_addr: &str,
    subject: &str,
    preview: &str,
    has_attachments: bool,
) -> String {
    let mut lines = vec![format!("[邮件] 收件邮箱: {mailbox_address}")];
    if !from_addr.is_empty() {
        lines.push(format!("发件人: {from_addr}"));
    }
    if !subject.is_empty() {
        lines.push(format!("主题: {subject}"));
    }
    if !preview.is_empty() {
        lines.push(String::new());
        lines.push(preview.to_string());
    }
    if has_attachments {
        lines.push(String::new());
        lines.push("(这封邮件包含附件)".to_string());
    }
    lines.join("\n")
}

fn default_string(value: Option<&str>, fallback: &str) -> String {
    value
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(fallback)
        .to_string()
}

fn bool_value_ref(value: &Value) -> bool {
    match value {
        Value::Bool(value) => *value,
        Value::Number(number) => number.as_i64().unwrap_or_default() != 0,
        Value::String(value) => matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "y" | "on"
        ),
        _ => false,
    }
}
