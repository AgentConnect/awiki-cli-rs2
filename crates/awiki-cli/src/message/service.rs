use super::types::{MessageError, SendRequest, MESSAGE_RPC_ENDPOINT};
use super::{
    build_direct_send_rpc_params, content_type_for_message_type, new_secure_e2ee_client_for_record,
    Client, MessageServiceE2EEClient, SecureE2EERpc,
};
use crate::authsdk::Session;
use crate::config::{join_base_url, Resolved};
use crate::identity::types::StoredIdentity;
use crate::identity::wire::{build_handle_lookup_by_handle_rpc_call, DID_AUTH_RPC_ENDPOINT};
use crate::identity::Manager;
use crate::runtime;
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

impl From<super::ws_proxy::DirectSendResult> for DirectSendResult {
    fn from(value: super::ws_proxy::DirectSendResult) -> Self {
        Self {
            accepted: value.accepted,
            message_id: value.message_id,
            operation_id: value.operation_id,
            target_did: value.target_did,
            accepted_at: value.accepted_at,
            final_acceptance: value.final_acceptance,
            delivery_state: value.delivery_state,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecureDirectSendRequest {
    pub target_did: String,
    pub target_handle: String,
    pub plaintext: String,
    pub message_type: String,
    pub message_id: String,
    pub operation_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SecureDirectSendOutcome {
    Success {
        accepted: bool,
        message_id: String,
        operation_id: String,
        target_did: String,
        accepted_at: String,
        final_acceptance: bool,
        delivery_state: String,
    },
    Error(String),
}

pub fn send(
    resolved: &Resolved,
    manager: &Manager,
    request: SendRequest,
) -> Result<CommandResult, MessageError> {
    if request.has_attachment() {
        if !request.group.trim().is_empty() {
            return super::attachment_service::send_group_attachment(resolved, manager, request);
        }
        return super::attachment_service::send_direct_attachment(resolved, manager, request);
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
    if request.secure_mode == "on" {
        return send_secure_direct(resolved, manager, request);
    }

    let record = require_active_identity(resolved, manager, &request.identity_name)?;
    let target = resolve_target(resolved, &request.target)?;
    let source_mode = runtime_mode(resolved);
    if source_mode == runtime::bridge::MODE_WEBSOCKET {
        let bridge = super::WSProxyTransport::new(resolved, &record.identity_name);
        let mut bridge_request = request.clone();
        bridge_request.target = target.did.clone();
        match bridge.send_direct(bridge_request) {
            Ok(result) => {
                let mut result = DirectSendResult::from(result);
                fill_direct_send_result(&mut result, &Value::Null, &target.did);
                return persist_send_result(
                    resolved,
                    &record,
                    &target,
                    &request,
                    &result,
                    Vec::new(),
                );
            }
            Err(bridge_err) => {
                match send_direct_http(resolved, manager, &record, &target, &request) {
                    Ok(mut result) => {
                        crate::traceutil::mark_fallback(
                            "websocket_to_http",
                            Some(&bridge_err.to_string()),
                        );
                        fill_direct_send_result(&mut result, &Value::Null, &target.did);
                        let warnings =
                            vec![super::websocket_http_fallback_warning(Some(&bridge_err))];
                        return persist_send_result(
                            resolved, &record, &target, &request, &result, warnings,
                        );
                    }
                    Err(_) => return Err(bridge_err),
                }
            }
        }
    }
    let result = send_direct_http(resolved, manager, &record, &target, &request)?;
    persist_send_result(resolved, &record, &target, &request, &result, Vec::new())
}

fn send_direct_http(
    resolved: &Resolved,
    manager: &Manager,
    record: &StoredIdentity,
    target: &TargetResolution,
    request: &SendRequest,
) -> Result<DirectSendResult, MessageError> {
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
    Ok(result)
}

pub fn send_secure_direct_with_sender(
    resolved: &Resolved,
    manager: &Manager,
    request: SendRequest,
    sender: impl FnMut(SecureDirectSendRequest) -> SecureDirectSendOutcome,
) -> Result<CommandResult, MessageError> {
    send_secure_direct_with_sender_and_warnings(resolved, manager, request, Vec::new(), sender)
}

pub(crate) fn send_secure_direct_with_sender_and_warnings(
    resolved: &Resolved,
    manager: &Manager,
    request: SendRequest,
    initial_warnings: Vec<String>,
    mut sender: impl FnMut(SecureDirectSendRequest) -> SecureDirectSendOutcome,
) -> Result<CommandResult, MessageError> {
    let (record, target) = prepare_secure_direct_send(resolved, manager, &request)?;
    send_secure_direct_resolved(
        resolved,
        manager,
        request,
        &record,
        target,
        initial_warnings,
        &mut sender,
    )
}

fn prepare_secure_direct_send(
    resolved: &Resolved,
    manager: &Manager,
    request: &SendRequest,
) -> Result<(StoredIdentity, TargetResolution), MessageError> {
    let record = require_active_identity(resolved, manager, &request.identity_name)?;
    if record.e2ee_agreement_private_pem.is_empty() || record.key1_private_pem.is_empty() {
        return Err(MessageError::Internal(
            "secure direct messaging requires DID signing and X25519 E2EE private keys".to_string(),
        ));
    }
    if request.target.trim().is_empty() {
        return Err(MessageError::TargetRequired);
    }
    if request.text.trim().is_empty() {
        return Err(MessageError::TextRequired);
    }

    let target = resolve_target(resolved, &request.target)?;
    Ok((record, target))
}

fn send_secure_direct_resolved(
    resolved: &Resolved,
    manager: &Manager,
    mut request: SendRequest,
    record: &StoredIdentity,
    target: TargetResolution,
    initial_warnings: Vec<String>,
    sender: &mut impl FnMut(SecureDirectSendRequest) -> SecureDirectSendOutcome,
) -> Result<CommandResult, MessageError> {
    let message_type = default_message_type(&request.message_type).to_string();
    let generated_message_id = format!("msg-{}", super::wire::generate_operation_id());
    let warnings = super::compact_warnings(initial_warnings);
    let outcome = sender(SecureDirectSendRequest {
        target_did: target.did.clone(),
        target_handle: target.handle.clone(),
        plaintext: request.text.clone(),
        message_type: message_type.clone(),
        message_id: generated_message_id.clone(),
        operation_id: generated_message_id.clone(),
    });

    let result = match outcome {
        SecureDirectSendOutcome::Success {
            accepted,
            mut message_id,
            mut operation_id,
            mut target_did,
            accepted_at,
            final_acceptance,
            delivery_state,
        } => {
            if message_id.is_empty() {
                message_id = generated_message_id.clone();
            }
            if operation_id.is_empty() {
                operation_id = generated_message_id.clone();
            }
            if target_did.is_empty() {
                target_did = target.did.clone();
            }
            DirectSendResult {
                accepted,
                message_id,
                operation_id,
                target_did,
                accepted_at,
                final_acceptance,
                delivery_state,
            }
        }
        SecureDirectSendOutcome::Error(err) if super::is_pending_confirmation_error(Some(&err)) => {
            let outbox_id = super::queue_secure_outbox_record(
                resolved,
                manager,
                Some(&record),
                &target.did,
                &message_type,
                &request.text,
            )?;
            return Ok(CommandResult {
                data: json!({
                    "action": "queue_secure_message",
                    "target": {
                        "did": target.did,
                        "handle": target.handle,
                        "kind": "direct",
                    },
                    "message": {
                        "type": message_type,
                        "secure": true,
                        "queued": true,
                    },
                    "delivery": {
                        "delivery_state": "queued",
                        "outbox_id": outbox_id,
                        "target_did": target.did,
                    },
                }),
                summary: "Queued secure direct message pending peer confirmation".to_string(),
                warnings,
            });
        }
        SecureDirectSendOutcome::Error(err) => return Err(MessageError::Internal(err)),
    };

    request.target = target.did.clone();
    request.secure_mode = "on".to_string();
    persist_send_result(resolved, &record, &target, &request, &result, warnings)
}

fn send_secure_direct(
    resolved: &Resolved,
    manager: &Manager,
    request: SendRequest,
) -> Result<CommandResult, MessageError> {
    let (record, target) = prepare_secure_direct_send(resolved, manager, &request)?;
    let auth = auth_session(resolved, manager, &record)?;
    let client = Client::new(resolved)?;
    let rpc = secure_rpc(client, auth);
    let mut e2ee_client = new_secure_e2ee_client_for_record(Some(manager), Some(&record), rpc)
        .map_err(MessageError::Internal)?;
    let warnings = publish_secure_prekeys_with_client(&mut e2ee_client);
    send_secure_direct_resolved(
        resolved,
        manager,
        request,
        &record,
        target,
        warnings,
        &mut |request| match e2ee_client.send_text(
            &request.target_did,
            &request.plaintext,
            &request.operation_id,
            &request.message_id,
        ) {
            Ok(result) => SecureDirectSendOutcome::Success {
                accepted: bool_value(result.get("accepted")),
                message_id: string_value(result.get("message_id")),
                operation_id: string_value(result.get("operation_id")),
                target_did: string_value(result.get("target_did")),
                accepted_at: string_value(result.get("accepted_at")),
                final_acceptance: bool_value(result.get("final_acceptance")),
                delivery_state: string_value(result.get("delivery_state")),
            },
            Err(err) => SecureDirectSendOutcome::Error(err),
        },
    )
}

pub(crate) fn publish_secure_prekeys_with_client(
    client: &mut MessageServiceE2EEClient,
) -> Vec<String> {
    match client.publish_prekey_bundle() {
        Ok(_) => Vec::new(),
        Err(err) => {
            super::compact_warnings(vec![format!("Failed to publish secure prekeys: {err}")])
        }
    }
}

pub(crate) fn maybe_publish_secure_prekeys(
    resolved: &Resolved,
    manager: &Manager,
    record: &StoredIdentity,
) -> Vec<String> {
    if record.e2ee_agreement_private_pem.is_empty() || record.key1_private_pem.is_empty() {
        return Vec::new();
    }
    let auth = match auth_session(resolved, manager, record) {
        Ok(auth) => auth,
        Err(err) => {
            return super::compact_warnings(vec![format!(
                "Failed to initialize secure prekey auth: {err}"
            )])
        }
    };
    let rpc: Box<SecureE2EERpc> = if resolved.service_base_url.trim().is_empty() {
        Box::new(|_, _| Err("message service url is required".to_string()))
    } else {
        let rpc_client = match Client::new(resolved) {
            Ok(client) => client,
            Err(err) => {
                return super::compact_warnings(vec![format!(
                    "Failed to initialize secure prekey publisher: {err}"
                )])
            }
        };
        secure_rpc(rpc_client, auth)
    };
    let mut client = match new_secure_e2ee_client_for_record(Some(manager), Some(record), rpc) {
        Ok(client) => client,
        Err(err) => {
            return super::compact_warnings(vec![format!(
                "Failed to initialize secure prekey publisher: {err}"
            )])
        }
    };
    publish_secure_prekeys_with_client(&mut client)
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
    let anp_service_endpoint = resolved.anp_service_endpoint.trim();
    if !base_url.is_empty() {
        session.remember_scope(base_url);
        session.remember_scope(&did_auth_url);
        session.remember_scope(&message_rpc_url);
    }
    if !anp_service_endpoint.is_empty() {
        session.remember_scope(anp_service_endpoint);
    }
    let token = record.jwt_token.trim();
    if !token.is_empty() && !base_url.is_empty() {
        session.set_bearer(base_url, token);
        session.set_bearer(&did_auth_url, token);
        session.set_bearer(&message_rpc_url, token);
    }
    if !token.is_empty() && !anp_service_endpoint.is_empty() {
        session.set_bearer(anp_service_endpoint, token);
    }
    if token.is_empty() {
        let client = Client::new(resolved)?;
        client
            .ensure_jwt(&mut session, &did_auth_url, "message_bootstrap")
            .map(|_| ())?;
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
    initial_warnings: Vec<String>,
) -> Result<CommandResult, MessageError> {
    let message_type = default_message_type(&request.message_type).to_string();
    let secure = request.secure_mode.trim() == "on";
    let mut warnings = initial_warnings;
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
                    is_e2ee: secure,
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
                "secure": secure,
                "sent_at": result.accepted_at,
            },
            "delivery": result,
        }),
        summary: format!("Sent a direct {message_type} message"),
        warnings: super::compact_warnings(warnings),
    })
}

fn secure_rpc(client: Client, mut auth: Session) -> Box<super::SecureE2EERpc> {
    Box::new(move |method, params| {
        client
            .authenticated_rpc_call_profile::<serde_json::Map<String, Value>, _>(
                Profile::RpcDefault,
                MESSAGE_RPC_ENDPOINT,
                method,
                params,
                &mut auth,
            )
            .map_err(|err| err.to_string())
    })
}

pub(crate) fn persist_inbox_messages(
    resolved: &Resolved,
    manager: &Manager,
    record: &StoredIdentity,
    raw: &Value,
    known_handle: &str,
    warnings: &mut Vec<String>,
) -> Vec<Value> {
    let mut messages = messages_from_result(raw.get("messages"));
    if messages.is_empty() {
        return messages;
    }
    let Ok(mut connection) = store::open(&resolved.paths) else {
        return messages;
    };
    if store::ensure_schema(&connection).is_err() {
        return messages;
    }
    warnings.extend(super::secure_incoming::maybe_decrypt_direct_e2ee_messages(
        resolved,
        manager,
        record,
        &mut messages,
    ));
    let records = messages
        .iter()
        .filter_map(|message| inbound_record(record, message))
        .collect::<Vec<_>>();
    if let Err(err) = store::store_messages_batch(&mut connection, &records) {
        warnings.push(format!("Failed to persist inbox messages: {err}"));
    }
    warnings.extend(super::contact_sync::sync_direct_peer_handles(
        resolved,
        &mut connection,
        &record.did,
        &messages,
        known_handle,
        "msg.inbox",
    ));
    super::secure_incoming::filter_displayable_direct_e2ee_messages(messages)
}

pub(crate) fn persist_history_messages(
    resolved: &Resolved,
    manager: &Manager,
    record: &StoredIdentity,
    peer_did: &str,
    known_handle: &str,
    raw: &Value,
    warnings: &mut Vec<String>,
) -> Vec<Value> {
    let mut messages = messages_from_result(raw.get("messages"));
    if messages.is_empty() {
        return messages;
    }
    let Ok(mut connection) = store::open(&resolved.paths) else {
        return messages;
    };
    if store::ensure_schema(&connection).is_err() {
        return messages;
    }
    warnings.extend(super::secure_incoming::maybe_decrypt_direct_e2ee_messages(
        resolved,
        manager,
        record,
        &mut messages,
    ));
    let records = messages
        .iter()
        .filter_map(|message| history_record(record, peer_did, message))
        .collect::<Vec<_>>();
    if let Err(err) = store::store_messages_batch(&mut connection, &records) {
        warnings.push(format!("Failed to persist history messages: {err}"));
    }
    warnings.extend(super::contact_sync::sync_direct_peer_handles(
        resolved,
        &mut connection,
        &record.did,
        &messages,
        known_handle,
        "msg.history",
    ));
    super::secure_incoming::filter_displayable_direct_e2ee_messages(messages)
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

pub(crate) fn apply_inbox_filters(
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

pub(crate) fn merge_handle_history_messages(
    resolved: &Resolved,
    owner_did: &str,
    target: &TargetResolution,
    limit: i64,
    unread_only: bool,
    inbox_only: bool,
    messages: &mut Vec<Value>,
    source: &mut String,
    warnings: &mut Vec<String>,
) -> Option<Vec<String>> {
    let dids = match super::contact_sync::peer_dids_for_handle_from_store(
        resolved,
        owner_did,
        &target.handle,
        &target.did,
    ) {
        Ok(dids) => dids,
        Err(err) => {
            warnings.push(format!("Failed to expand handle history: {err}"));
            return None;
        }
    };
    if dids.is_empty() {
        return Some(dids);
    }
    let Ok(connection) = store::open(&resolved.paths) else {
        return Some(dids);
    };
    if store::ensure_schema(&connection).is_err() {
        return Some(dids);
    }
    match store::list_direct_messages_by_peer_dids(
        &connection,
        owner_did,
        &dids,
        limit,
        unread_only,
        inbox_only,
    ) {
        Ok(cached) if !cached.is_empty() => {
            let cached = super::secure_incoming::filter_displayable_direct_e2ee_messages(cached);
            if cached.is_empty() {
                return Some(dids);
            }
            let merged = merge_direct_history_messages(messages, cached, limit);
            if merged.len() > messages.len() || !merged.is_empty() {
                *messages = merged;
                if !source.ends_with("+handle_history") {
                    source.push_str("+handle_history");
                }
            }
        }
        Ok(_) => {}
        Err(err) => warnings.push(format!("Failed to load handle history from cache: {err}")),
    }
    Some(dids)
}

fn merge_direct_history_messages(remote: &[Value], cached: Vec<Value>, limit: i64) -> Vec<Value> {
    let mut merged = Vec::with_capacity(remote.len() + cached.len());
    let mut seen = Vec::new();
    for message in remote.iter().cloned().chain(cached) {
        let id = message_identity(&message);
        if id.is_empty() || seen.iter().any(|known| known == &id) {
            continue;
        }
        seen.push(id);
        merged.push(message);
    }
    merged.sort_by(|left, right| {
        comparable_message_time(right)
            .cmp(&comparable_message_time(left))
            .then_with(|| message_identity(right).cmp(&message_identity(left)))
    });
    let limit = if limit <= 0 { 50 } else { limit as usize };
    merged.truncate(limit);
    merged
}

fn comparable_message_time(message: &Value) -> String {
    message
        .get("sent_at")
        .or_else(|| message.get("stored_at"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
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

pub(crate) fn runtime_mode(resolved: &Resolved) -> &'static str {
    if resolved.runtime_mode.trim().eq_ignore_ascii_case("http") {
        runtime::bridge::MODE_HTTP
    } else {
        runtime::bridge::MODE_WEBSOCKET
    }
}

fn messages_from_result(value: Option<&Value>) -> Vec<Value> {
    value.and_then(Value::as_array).cloned().unwrap_or_default()
}

pub(crate) fn collect_message_ids(messages: &[Value]) -> Vec<String> {
    let mut ids = Vec::new();
    for message in messages {
        let id = message_identity(message);
        if !id.is_empty() && !ids.iter().any(|known| known == &id) {
            ids.push(id);
        }
    }
    ids
}

pub(crate) fn mark_messages_read_in_result(messages: &mut [Value], ids: &[String]) {
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

pub(crate) fn resolved_dids_value(raw: &Value) -> Value {
    raw.get("resolved_dids").cloned().unwrap_or(Value::Null)
}

pub(crate) fn peer_handle_or_did(target: &TargetResolution) -> String {
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
    super::contact_sync::normalize_handle_value(value)
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
