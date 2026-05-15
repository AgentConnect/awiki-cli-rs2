use super::types::{
    HistoryRequest, InboxRequest, MarkReadRequest, MessageError, SendRequest, MESSAGE_RPC_ENDPOINT,
};
use super::{
    build_direct_send_rpc_params, build_history_rpc_params, build_inbox_rpc_params,
    build_mark_read_rpc_params, content_type_for_message_type, Client,
};
use crate::authsdk::Session;
use crate::config::{join_base_url, Resolved};
use crate::identity::types::StoredIdentity;
use crate::identity::wire::{build_handle_lookup_by_handle_rpc_call, DID_AUTH_RPC_ENDPOINT};
use crate::identity::Manager;
use crate::store::{self, MessageRecord};
use crate::transportcfg::Profile;
use serde_json::{json, Value};

#[derive(Debug, Clone)]
pub struct CommandResult {
    pub data: Value,
    pub summary: String,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct TargetResolution {
    pub(crate) did: String,
    pub(crate) handle: String,
}

#[derive(Debug, Clone, Default, serde::Deserialize, serde::Serialize)]
struct DirectSendResult {
    #[serde(default)]
    accepted: bool,
    #[serde(default)]
    message_id: String,
    #[serde(default)]
    operation_id: String,
    #[serde(default)]
    target_did: String,
    #[serde(default)]
    accepted_at: String,
    #[serde(default)]
    final_acceptance: bool,
    #[serde(default)]
    delivery_state: String,
}

pub fn send(
    resolved: &Resolved,
    manager: &Manager,
    request: SendRequest,
) -> Result<CommandResult, MessageError> {
    if request.has_attachment() {
        return Err(MessageError::AttachmentNotSupported);
    }
    if !request.group.trim().is_empty() {
        return super::group_service::send_group(resolved, manager, request);
    }
    if request.target.trim().is_empty() {
        return Err(MessageError::TargetRequired);
    }
    if request.text.trim().is_empty() {
        return Err(MessageError::TextRequired);
    }
    if request.secure_mode.trim().eq_ignore_ascii_case("on") {
        return Err(MessageError::SecureNotSupported);
    }

    let record = require_active_identity(resolved, manager, &request.identity_name)?;
    let target = resolve_target(resolved, &request.target)?;
    let mut auth = auth_session(resolved, manager, &record)?;
    let client = Client::new(resolved)?;
    let params = build_direct_send_rpc_params(
        &record,
        &target.did,
        &request.text,
        default_message_type(&request.message_type),
    )?;
    let meta = params.get("meta").cloned().unwrap_or(Value::Null);
    let mut result: DirectSendResult = client.authenticated_rpc_call_profile(
        Profile::RpcDefault,
        MESSAGE_RPC_ENDPOINT,
        "direct.send",
        params,
        &mut auth,
    )?;
    fill_direct_send_result(&mut result, &meta, &target.did);
    persist_send_result(resolved, &record, &target, &request, &result)
}

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
    let target = if request.with.trim().is_empty() {
        TargetResolution::default()
    } else {
        resolve_target(resolved, &request.with)?
    };
    request.with = target.did.clone();
    let mut auth = auth_session(resolved, manager, &record)?;
    let client = Client::new(resolved)?;
    let params = build_inbox_rpc_params(&record, request.clone());
    let raw: Value = client.authenticated_rpc_call_profile(
        Profile::RpcReadHeavy,
        MESSAGE_RPC_ENDPOINT,
        "inbox.get",
        params,
        &mut auth,
    )?;
    let mut warnings = Vec::new();
    let mut messages = persist_inbox_messages(resolved, &record, &raw, &mut warnings);
    messages = apply_inbox_filters(messages, &target.did, request.unread_only, request.limit);
    let total = messages.len();
    if request.mark_read && !messages.is_empty() {
        let ids = collect_message_ids(&messages);
        if !ids.is_empty() {
            if mark_read(
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
    }
    Ok(CommandResult {
        data: json!({
            "messages": messages,
            "total": total,
            "source": source_with_default(&raw),
            "with": peer_handle_or_did(&target),
        }),
        summary: format!("Loaded {total} direct inbox messages"),
        warnings,
    })
}

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
    let target = resolve_target(resolved, &request.with)?;
    request.with = target.did.clone();
    let mut auth = auth_session(resolved, manager, &record)?;
    let client = Client::new(resolved)?;
    let params = build_history_rpc_params(&record, request.clone())?;
    let raw: Value = client.authenticated_rpc_call_profile(
        Profile::RpcReadHeavy,
        MESSAGE_RPC_ENDPOINT,
        "direct.get_history",
        params,
        &mut auth,
    )?;
    let mut warnings = Vec::new();
    let messages = persist_history_messages(resolved, &record, &target.did, &raw, &mut warnings);
    let total = messages.len();
    Ok(CommandResult {
        data: json!({
            "messages": messages,
            "total": total,
            "source": source_with_default(&raw),
            "with": peer_handle_or_did(&target),
            "resolved_dids": resolved_dids_value(&raw),
        }),
        summary: format!("Loaded {total} direct history messages"),
        warnings,
    })
}

pub fn mark_read(
    resolved: &Resolved,
    manager: &Manager,
    request: MarkReadRequest,
) -> Result<CommandResult, MessageError> {
    if request.message_ids.is_empty() {
        return Err(MessageError::MessageNotFound);
    }
    let record = require_active_identity(resolved, manager, &request.identity_name)?;
    let mut auth = auth_session(resolved, manager, &record)?;
    let client = Client::new(resolved)?;
    let params = build_mark_read_rpc_params(&record, request.clone())?;
    let raw: Value = client.authenticated_rpc_call_profile(
        Profile::RpcDefault,
        MESSAGE_RPC_ENDPOINT,
        "inbox.mark_read",
        params,
        &mut auth,
    )?;
    let mut warnings = Vec::new();
    let mut updated_count = int_value(raw.get("updated_count"), request.message_ids.len() as i64);
    if let Ok(connection) = store::open(&resolved.paths) {
        if store::ensure_schema(&connection).is_ok() {
            match store::mark_messages_read(&connection, &record.did, &request.message_ids) {
                Ok(count) if updated_count == 0 => updated_count = count,
                Ok(_) => {}
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
        warnings,
    })
}

pub(crate) fn require_active_identity(
    resolved: &Resolved,
    manager: &Manager,
    requested: &str,
) -> Result<StoredIdentity, MessageError> {
    let identity_name = if requested.trim().is_empty() {
        if resolved.active_identity.trim().is_empty() {
            manager.current()?.identity_name
        } else {
            resolved.active_identity.clone()
        }
    } else {
        requested.trim().to_string()
    };
    let record = manager.load(&identity_name)?;
    let user_state = crate::identity::store::evaluate_user_state(&record.user_id, &record.handle);
    if !user_state.ready_for_messaging {
        return Err(MessageError::IdentityRequired(format!(
            "identity {} requires user registration before messaging",
            record.identity_name
        )));
    }
    Ok(record)
}

pub(crate) fn auth_session(
    resolved: &Resolved,
    manager: &Manager,
    record: &StoredIdentity,
) -> Result<Session, MessageError> {
    if record.identity_name.trim().is_empty() {
        return Err(MessageError::Internal(
            "active identity is required".to_string(),
        ));
    }
    let paths = manager.paths_for_identity(&record.identity_name)?;
    let identity_name = record.identity_name.clone();
    let persist_manager = manager.clone();
    let persist_identity_name = identity_name.clone();
    let persist_token: crate::authsdk::PersistToken = Box::new(move |token| {
        persist_manager.update_jwt(&persist_identity_name, token)?;
        Ok(())
    });
    let mut session = Session::new(
        &paths.did_document_path,
        &paths.key1_private_path,
        identity_name,
        record.did.as_str(),
        record.jwt_token.as_str(),
        Some(persist_token),
    );
    let base_url = resolved.service_base_url.trim();
    let did_auth_url = join_base_url(base_url, DID_AUTH_RPC_ENDPOINT);
    let message_rpc_url = join_base_url(base_url, MESSAGE_RPC_ENDPOINT);
    if !base_url.is_empty() {
        session.remember_scope(base_url);
        session.remember_scope(&did_auth_url);
        session.remember_scope(&message_rpc_url);
    }
    let token = record.jwt_token.trim();
    if !token.is_empty() && !base_url.is_empty() {
        session.set_bearer(base_url, token);
        session.set_bearer(&did_auth_url, token);
        session.set_bearer(&message_rpc_url, token);
    }
    if token.is_empty() {
        let client = Client::new(resolved)?;
        client.ensure_jwt(&mut session, &did_auth_url).map(|_| ())?;
    }
    Ok(session)
}

pub(crate) fn resolve_target(
    resolved: &Resolved,
    target: &str,
) -> Result<TargetResolution, MessageError> {
    let target = target.trim();
    if target.is_empty() {
        return Err(MessageError::TargetRequired);
    }
    if target.starts_with("did:") {
        return Ok(TargetResolution {
            did: target.to_string(),
            handle: String::new(),
        });
    }
    let normalized = crate::identity::normalize_handle_input(target, &resolved.did_domain)?;
    let call = build_handle_lookup_by_handle_rpc_call(&normalized.full_handle)?;
    let lookup: Value = crate::identity::client::Client::new(resolved)?.rpc_call_profile(
        call.profile,
        call.endpoint,
        call.method,
        call.params,
    )?;
    let did = lookup
        .get("did")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    if did.is_empty() {
        return Err(MessageError::TargetRequired);
    }
    let handle = lookup
        .get("full_handle")
        .or_else(|| lookup.get("handle"))
        .and_then(Value::as_str)
        .map(normalize_handle_value)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| normalized.full_handle.to_ascii_lowercase());
    Ok(TargetResolution { did, handle })
}

fn persist_send_result(
    resolved: &Resolved,
    record: &StoredIdentity,
    target: &TargetResolution,
    request: &SendRequest,
    result: &DirectSendResult,
) -> Result<CommandResult, MessageError> {
    let message_type = default_message_type(&request.message_type).to_string();
    let mut warnings = Vec::new();
    if let Ok(connection) = store::open(&resolved.paths) {
        if store::ensure_schema(&connection).is_ok() {
            let stored = store::store_message(
                &connection,
                MessageRecord {
                    msg_id: result.message_id.clone(),
                    owner_did: record.did.clone(),
                    thread_id: store::make_thread_id(&record.did, &target.did, ""),
                    direction: 1,
                    sender_did: record.did.clone(),
                    receiver_did: target.did.clone(),
                    content_type: content_type_for_message_type(&message_type).to_string(),
                    content: request.text.clone(),
                    sent_at: result.accepted_at.clone(),
                    is_read: true,
                    is_e2ee: false,
                    metadata: metadata_string(json!({
                        "delivery_state": result.delivery_state,
                        "operation_id": result.operation_id,
                        "target_handle": target.handle,
                    })),
                    credential_name: record.identity_name.clone(),
                    ..MessageRecord::default()
                },
            );
            if let Err(err) = stored {
                warnings.push(format!("Failed to persist local message: {err}"));
            }
        }
    }
    Ok(CommandResult {
        data: json!({
            "action": "send_message",
            "target": {
                "did": target.did,
                "handle": target.handle,
                "kind": "direct",
            },
            "message": {
                "id": result.message_id,
                "type": message_type,
                "secure": false,
                "sent_at": result.accepted_at,
            },
            "delivery": result,
        }),
        summary: format!("Sent a direct {message_type} message"),
        warnings,
    })
}

fn persist_inbox_messages(
    resolved: &Resolved,
    record: &StoredIdentity,
    raw: &Value,
    warnings: &mut Vec<String>,
) -> Vec<Value> {
    let messages = messages_from_result(raw.get("messages"));
    if messages.is_empty() {
        return messages;
    }
    let Ok(mut connection) = store::open(&resolved.paths) else {
        return messages;
    };
    if store::ensure_schema(&connection).is_err() {
        return messages;
    }
    let records = messages
        .iter()
        .filter_map(|message| inbound_record(record, message))
        .collect::<Vec<_>>();
    if let Err(err) = store::store_messages_batch(&mut connection, &records) {
        warnings.push(format!("Failed to persist inbox messages: {err}"));
    }
    messages
}

fn persist_history_messages(
    resolved: &Resolved,
    record: &StoredIdentity,
    peer_did: &str,
    raw: &Value,
    warnings: &mut Vec<String>,
) -> Vec<Value> {
    let messages = messages_from_result(raw.get("messages"));
    if messages.is_empty() {
        return messages;
    }
    let Ok(mut connection) = store::open(&resolved.paths) else {
        return messages;
    };
    if store::ensure_schema(&connection).is_err() {
        return messages;
    }
    let records = messages
        .iter()
        .filter_map(|message| history_record(record, peer_did, message))
        .collect::<Vec<_>>();
    if let Err(err) = store::store_messages_batch(&mut connection, &records) {
        warnings.push(format!("Failed to persist history messages: {err}"));
    }
    messages
}

fn inbound_record(record: &StoredIdentity, message: &Value) -> Option<MessageRecord> {
    let object = message.as_object()?;
    let msg_id = message_identity(message);
    if msg_id.is_empty() || bool_value(object.get("secure_control")) {
        return None;
    }
    let sender_did = string_value(object.get("sender_did"));
    let receiver_did = string_value(object.get("receiver_did"));
    let peer_did = if sender_did == record.did {
        receiver_did.clone()
    } else {
        sender_did.clone()
    };
    Some(MessageRecord {
        msg_id,
        owner_did: record.did.clone(),
        thread_id: store::make_thread_id(&record.did, &peer_did, ""),
        direction: 0,
        sender_did,
        receiver_did,
        content_type: string_value(object.get("content_type")),
        content: content_string(object.get("content")),
        server_seq: i64_value(object.get("server_seq")),
        sent_at: string_value(object.get("sent_at")),
        is_e2ee: bool_value(object.get("secure")),
        is_read: bool_value(object.get("is_read")),
        sender_name: string_value(object.get("sender_name")),
        metadata: metadata_string(message.clone()),
        credential_name: record.identity_name.clone(),
        ..MessageRecord::default()
    })
}

fn history_record(
    record: &StoredIdentity,
    peer_did: &str,
    message: &Value,
) -> Option<MessageRecord> {
    let object = message.as_object()?;
    let msg_id = message_identity(message);
    if msg_id.is_empty() || bool_value(object.get("secure_control")) {
        return None;
    }
    let sender_did = string_value(object.get("sender_did"));
    let receiver_did = string_value(object.get("receiver_did"));
    Some(MessageRecord {
        msg_id,
        owner_did: record.did.clone(),
        thread_id: store::make_thread_id(&record.did, peer_did, ""),
        direction: if sender_did == record.did { 1 } else { 0 },
        sender_did,
        receiver_did,
        content_type: string_value(object.get("content_type")),
        content: content_string(object.get("content")),
        server_seq: i64_value(object.get("server_seq")),
        sent_at: string_value(object.get("sent_at")),
        is_e2ee: bool_value(object.get("secure")),
        is_read: bool_value(object.get("is_read")),
        sender_name: string_value(object.get("sender_name")),
        metadata: metadata_string(message.clone()),
        credential_name: record.identity_name.clone(),
        ..MessageRecord::default()
    })
}

fn apply_inbox_filters(
    messages: Vec<Value>,
    peer_did: &str,
    unread_only: bool,
    limit: i64,
) -> Vec<Value> {
    let limit = if limit <= 0 { 20 } else { limit as usize };
    messages
        .into_iter()
        .filter(|message| {
            if peer_did.trim().is_empty() {
                true
            } else {
                let Some(object) = message.as_object() else {
                    return false;
                };
                string_value(object.get("sender_did")) == peer_did
                    || string_value(object.get("receiver_did")) == peer_did
            }
        })
        .filter(|message| {
            if !unread_only {
                return true;
            }
            message
                .as_object()
                .map(|object| !bool_value(object.get("is_read")))
                .unwrap_or(false)
        })
        .take(limit)
        .collect()
}

fn fill_direct_send_result(result: &mut DirectSendResult, meta: &Value, target_did: &str) {
    if result.message_id.is_empty() {
        result.message_id = string_value(meta.get("message_id"));
    }
    if result.operation_id.is_empty() {
        result.operation_id = string_value(meta.get("operation_id"));
    }
    if result.target_did.is_empty() {
        result.target_did = target_did.to_string();
    }
}

fn messages_from_result(value: Option<&Value>) -> Vec<Value> {
    value.and_then(Value::as_array).cloned().unwrap_or_default()
}

fn collect_message_ids(messages: &[Value]) -> Vec<String> {
    let mut ids = Vec::new();
    for message in messages {
        let id = message_identity(message);
        if !id.is_empty() && !ids.iter().any(|known| known == &id) {
            ids.push(id);
        }
    }
    ids
}

fn mark_messages_read_in_result(messages: &mut [Value], ids: &[String]) {
    for message in messages {
        let id = message_identity(message);
        if id.is_empty() || !ids.iter().any(|known| known == &id) {
            continue;
        }
        if let Some(object) = message.as_object_mut() {
            object.insert("is_read".to_string(), Value::Bool(true));
        }
    }
}

fn message_identity(message: &Value) -> String {
    message
        .as_object()
        .and_then(|object| {
            object
                .get("id")
                .or_else(|| object.get("message_id"))
                .or_else(|| object.get("msg_id"))
        })
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn source_with_default(raw: &Value) -> String {
    raw.get("source")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("remote_http")
        .to_string()
}

fn resolved_dids_value(raw: &Value) -> Value {
    raw.get("resolved_dids").cloned().unwrap_or(Value::Null)
}

fn peer_handle_or_did(target: &TargetResolution) -> String {
    if target.handle.trim().is_empty() {
        target.did.clone()
    } else {
        target.handle.clone()
    }
}

pub(crate) fn default_message_type(message_type: &str) -> &str {
    if message_type.trim().is_empty() {
        "text"
    } else {
        message_type.trim()
    }
}

pub(crate) fn normalize_handle_value(value: &str) -> String {
    value
        .trim()
        .trim_start_matches("wba://")
        .to_ascii_lowercase()
}

pub(crate) fn metadata_string(value: Value) -> String {
    serde_json::to_string(&value).unwrap_or_default()
}

pub(crate) fn content_string(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(value)) => value.clone(),
        Some(value) => serde_json::to_string(value).unwrap_or_default(),
        None => String::new(),
    }
}

pub(crate) fn string_value(value: Option<&Value>) -> String {
    value
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

pub(crate) fn int_value(value: Option<&Value>, fallback: i64) -> i64 {
    match value {
        Some(Value::Number(number)) => number
            .as_i64()
            .or_else(|| number.as_u64().and_then(|value| i64::try_from(value).ok()))
            .or_else(|| number.as_f64().map(|value| value as i64))
            .unwrap_or(fallback),
        Some(Value::String(value)) => value.trim().parse::<i64>().unwrap_or(fallback),
        _ => fallback,
    }
}

pub(crate) fn i64_value(value: Option<&Value>) -> Option<i64> {
    match value {
        Some(Value::Number(number)) => number
            .as_i64()
            .or_else(|| number.as_u64().and_then(|value| i64::try_from(value).ok()))
            .or_else(|| number.as_f64().map(|value| value as i64)),
        Some(Value::String(value)) => value.trim().parse::<i64>().ok(),
        _ => None,
    }
}

pub(crate) fn bool_value(value: Option<&Value>) -> bool {
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
