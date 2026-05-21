use std::fs;

use im_core::prelude::{
    AuthScope, Cursor, GroupRef, HistoryQuery, InboxQuery, InboxScope, MessageBody,
    MessageDeliveryOptions, MessageKind, MessageSecurityMode, MessageTarget, PageLimit, PeerRef,
    SendMessageRequest, SessionBundle, ThreadRef,
};
use serde_json::{json, Value};

use crate::authsdk::Session;
use crate::cli::ParsedCommand;
use crate::config::Resolved;
use crate::identity::Manager;
use crate::message;
use crate::message::MessageError;
use crate::output::ExitError;
use crate::store::{self, MessageRecord};
use crate::transportcfg::Profile;

pub fn send_message_request(
    command: &ParsedCommand,
    default_domain: &str,
) -> Result<SendMessageRequest, ExitError> {
    let target = message_target(command, default_domain)?;
    let body = message_body(command)?;
    let security = message_security(command, &target)?;
    Ok(SendMessageRequest {
        target,
        body,
        security,
        client_message_id: None,
        delivery: MessageDeliveryOptions::default(),
    })
}

pub fn inbox_query(command: &ParsedCommand) -> Result<InboxQuery, ExitError> {
    Ok(InboxQuery {
        scope: inbox_scope(&string_flag(command, "scope"))?,
        limit: page_limit(command, "limit", 20)?,
        cursor: optional_cursor(command)?,
        unread_only: bool_flag(command, "unread"),
    })
}

pub fn history_request(
    command: &ParsedCommand,
    default_domain: &str,
) -> Result<(ThreadRef, HistoryQuery), ExitError> {
    let with = string_flag(command, "with");
    let group = string_flag(command, "group");
    let thread = match (with.trim().is_empty(), group.trim().is_empty()) {
        (false, true) => ThreadRef::Direct(parse_peer(&with, default_domain)?),
        (true, false) => ThreadRef::Group(parse_group(&group)?),
        (true, true) => {
            return Err(ExitError::new(
                "invalid_argument",
                2,
                "history requires either --with or --group.",
                "Use --with <handle|did> for direct history or --group <group_did>.",
            ));
        }
        (false, false) => {
            return Err(ExitError::new(
                "invalid_argument",
                2,
                "history accepts either --with or --group, but not both.",
                "Choose direct history with --with or group history with --group.",
            ));
        }
    };
    Ok((
        thread,
        HistoryQuery {
            limit: page_limit(command, "limit", 50)?,
            cursor: optional_cursor(command)?,
        },
    ))
}

pub fn legacy_text_send_request(
    identity_name: &str,
    request: SendMessageRequest,
) -> Result<message::SendRequest, ExitError> {
    let (target, group) = match request.target {
        MessageTarget::Direct(peer) => (peer.as_str().to_string(), String::new()),
        MessageTarget::Group(group) => (String::new(), group.as_str().to_string()),
    };
    let (text, message_type) = match request.body {
        MessageBody::Text { text, kind } => (text, legacy_message_type(kind)),
        MessageBody::Attachment { .. } => {
            return Err(ExitError::new(
                "unsupported_capability",
                2,
                "attachments are not supported by the Phase 1 IM Core adapter.",
                "Use the existing legacy attachment command path until attachment migration starts.",
            ));
        }
    };
    let secure_mode = match request.security {
        MessageSecurityMode::DefaultPlain => String::new(),
        MessageSecurityMode::Plain => "off".to_string(),
        MessageSecurityMode::SecureDirect => {
            return Err(ExitError::new(
                "unsupported_capability",
                2,
                "secure direct messages are not supported by the Phase 1 IM Core adapter.",
                "Use the existing legacy secure command path until secure migration starts.",
            ));
        }
        MessageSecurityMode::GroupE2ee => {
            return Err(ExitError::new(
                "unsupported_capability",
                2,
                "group E2EE is not supported by the Phase 1 IM Core adapter.",
                "Use the existing legacy group E2EE command path until secure migration starts.",
            ));
        }
    };
    Ok(message::SendRequest {
        identity_name: identity_name.to_string(),
        target,
        group,
        text,
        message_type,
        secure_mode,
        ..message::SendRequest::default()
    })
}

pub fn send_auth_scope(request: &SendMessageRequest) -> AuthScope {
    match request.target {
        MessageTarget::Direct(_) => AuthScope::Messaging,
        MessageTarget::Group(_) => AuthScope::GroupMessaging,
    }
}

pub fn send_direct_text_via_im_core(
    resolved: &Resolved,
    manager: &Manager,
    client: &im_core::ImClient,
    identity_name: &str,
    request: SendMessageRequest,
) -> Result<message::CommandResult, MessageError> {
    let record = message::require_active_identity(resolved, manager, identity_name)?;
    let target = match &request.target {
        MessageTarget::Direct(peer) => message::resolve_target(resolved, peer.as_str())?,
        MessageTarget::Group(_) => return Err(MessageError::GroupNotSupported),
    };
    let bridge_result = im_core::compat::messages::send_direct_text_with_bridge(
        client,
        DirectTextSessionProvider {
            subject: client.did().clone(),
            resolved,
            manager,
            record: record.clone(),
        },
        DirectTextLegacyTransport {
            resolved,
            manager,
            record: record.clone(),
        },
        im_core::compat::messages::DirectTextSendBridgeRequest {
            request,
            resolved_target_did: target.did.clone(),
            credentials: im_core::compat::messages::DirectTextCredentials {
                identity_name: record.identity_name.clone(),
                did_document: record.did_document.clone(),
                key1_private_pem: record.key1_private_pem.clone(),
            },
        },
    )
    .map_err(im_error_to_message_error)?;
    let mut result = DirectSendResult::from_sdk_bridge(&bridge_result);
    fill_direct_send_result(&mut result, &Value::Null, &target.did);
    persist_send_result(
        resolved,
        &record,
        &target,
        &bridge_result.text,
        &bridge_result.message_type,
        false,
        &result,
        bridge_result.sdk_result.warnings,
    )
}

pub fn send_group_text_via_im_core(
    resolved: &Resolved,
    manager: &Manager,
    client: &im_core::ImClient,
    identity_name: &str,
    request: SendMessageRequest,
) -> Result<message::CommandResult, MessageError> {
    let record = message::require_active_identity(resolved, manager, identity_name)?;
    let group_did = match &request.target {
        MessageTarget::Group(group) => group.as_str().to_string(),
        MessageTarget::Direct(_) => return Err(MessageError::TargetRequired),
    };
    let bridge_result = im_core::compat::messages::send_group_text_with_bridge(
        client,
        GroupTextSessionProvider {
            subject: client.did().clone(),
            resolved,
            manager,
            record: record.clone(),
        },
        GroupTextLegacyTransport {
            resolved,
            manager,
            record: record.clone(),
        },
        im_core::compat::messages::GroupTextSendBridgeRequest {
            request,
            credentials: im_core::compat::messages::GroupTextCredentials {
                identity_name: record.identity_name.clone(),
                did_document: record.did_document.clone(),
                key1_private_pem: record.key1_private_pem.clone(),
            },
        },
    )
    .map_err(im_error_to_message_error)?;
    let mut result = GroupSendResult::from_sdk_bridge(&bridge_result);
    fill_group_send_result(&mut result, &bridge_result.raw, &group_did);
    persist_group_send_result(
        resolved,
        &record,
        &bridge_result.group_did,
        &bridge_result.text,
        &bridge_result.message_type,
        &result,
        bridge_result.sdk_result.warnings,
    )
}

pub fn legacy_inbox_request(
    identity_name: &str,
    query: InboxQuery,
) -> Result<message::InboxRequest, ExitError> {
    if query.cursor.is_some() {
        return Err(ExitError::new(
            "unsupported_capability",
            2,
            "inbox cursor is not supported by the Phase 1G IM Core adapter bridge.",
            "Use the existing legacy inbox path until cursor pagination is migrated.",
        ));
    }
    Ok(message::InboxRequest {
        identity_name: identity_name.to_string(),
        scope: legacy_inbox_scope(query.scope),
        limit: query.limit.0 as i64,
        unread_only: query.unread_only,
        mark_read: false,
        ..message::InboxRequest::default()
    })
}

pub fn legacy_history_request(
    identity_name: &str,
    thread: ThreadRef,
    query: HistoryQuery,
) -> Result<message::HistoryRequest, ExitError> {
    let with = match thread {
        ThreadRef::Direct(peer) => peer.as_str().to_string(),
        ThreadRef::Group(_) => {
            return Err(ExitError::new(
                "unsupported_capability",
                2,
                "group history is not routed through the Phase 1G IM Core adapter.",
                "Use the existing group messages command until group history is migrated.",
            ));
        }
        ThreadRef::Thread(_) => {
            return Err(ExitError::new(
                "unsupported_capability",
                2,
                "thread history is not supported by the Phase 1G IM Core adapter.",
                "Use direct history with --with in this phase.",
            ));
        }
    };
    Ok(message::HistoryRequest {
        identity_name: identity_name.to_string(),
        with,
        limit: query.limit.0 as i64,
        cursor: query
            .cursor
            .map(|cursor| cursor.as_str().to_string())
            .unwrap_or_default(),
        ..message::HistoryRequest::default()
    })
}

fn message_target(
    command: &ParsedCommand,
    default_domain: &str,
) -> Result<MessageTarget, ExitError> {
    let to = string_flag(command, "to");
    let group = string_flag(command, "group");
    match (to.trim().is_empty(), group.trim().is_empty()) {
        (false, true) => Ok(MessageTarget::Direct(parse_peer(&to, default_domain)?)),
        (true, false) => Ok(MessageTarget::Group(parse_group(&group)?)),
        (true, true) => Err(ExitError::new(
            "invalid_argument",
            2,
            "msg send requires either --to or --group.",
            "Use --to <handle|did> or --group <group_did>.",
        )),
        (false, false) => Err(ExitError::new(
            "invalid_argument",
            2,
            "msg send accepts either --to or --group, but not both.",
            "Choose direct messaging with --to or group messaging with --group.",
        )),
    }
}

fn message_body(command: &ParsedCommand) -> Result<MessageBody, ExitError> {
    let file_path = string_flag(command, "file");
    if !file_path.trim().is_empty() {
        return Err(ExitError::new(
            "unsupported_capability",
            2,
            "attachments are not supported by the Phase 1 IM Core adapter.",
            "Use the existing legacy attachment command path until attachment migration starts.",
        ));
    }
    let mut text = string_flag(command, "text");
    let text_file = string_flag(command, "text-file");
    if !text.trim().is_empty() && !text_file.trim().is_empty() {
        return Err(ExitError::new(
            "invalid_argument",
            2,
            "Use either --text or --text-file, not both.",
            "Choose one message body source.",
        ));
    }
    if text.trim().is_empty() && !text_file.trim().is_empty() {
        text = fs::read_to_string(&text_file).map_err(|err| {
            ExitError::new(
                "invalid_argument",
                2,
                format!("read text file {text_file:?}: {err}"),
                "Check the --text-file path and permissions.",
            )
        })?;
    }
    if text.trim().is_empty() {
        return Err(ExitError::new(
            "invalid_argument",
            2,
            "msg send requires --text or --text-file.",
            "Provide a text body for Phase 1 IM Core messages.",
        ));
    }
    Ok(MessageBody::Text {
        text,
        kind: message_kind(&string_flag(command, "type"))?,
    })
}

fn message_kind(raw: &str) -> Result<MessageKind, ExitError> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "" | "text" => Ok(MessageKind::Text),
        "markdown" => Ok(MessageKind::Markdown),
        value => Err(ExitError::new(
            "unsupported_capability",
            2,
            format!("message type {value:?} is not supported by the Phase 1 IM Core adapter."),
            "Use --type text or --type markdown.",
        )),
    }
}

fn legacy_message_type(kind: MessageKind) -> String {
    match kind {
        MessageKind::Text => "text".to_string(),
        MessageKind::Markdown => "markdown".to_string(),
    }
}

fn message_security(
    command: &ParsedCommand,
    target: &MessageTarget,
) -> Result<MessageSecurityMode, ExitError> {
    match string_flag(command, "secure")
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "" | "default" => Ok(MessageSecurityMode::DefaultPlain),
        "plain" | "off" | "false" => Ok(MessageSecurityMode::Plain),
        "direct" | "secure-direct" | "on" | "true" => match target {
            MessageTarget::Direct(_) => Err(ExitError::new(
                "unsupported_capability",
                2,
                "secure direct messages are not supported by the Phase 1 IM Core adapter.",
                "Use the existing legacy secure command path until secure migration starts.",
            )),
            MessageTarget::Group(_) => Err(ExitError::new(
                "unsupported_capability",
                2,
                "group E2EE is not supported by the Phase 1 IM Core adapter.",
                "Use the existing legacy group E2EE command path until secure migration starts.",
            )),
        },
        "group-e2ee" | "e2ee" => Err(ExitError::new(
            "unsupported_capability",
            2,
            "group E2EE is not supported by the Phase 1 IM Core adapter.",
            "Use the existing legacy group E2EE command path until secure migration starts.",
        )),
        value => Err(ExitError::new(
            "invalid_argument",
            2,
            format!("unsupported --secure value {value:?}."),
            "Use --secure plain, --secure off, or leave it unset for Phase 1.",
        )),
    }
}

fn inbox_scope(raw: &str) -> Result<InboxScope, ExitError> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "" | "all" => Ok(InboxScope::All),
        "direct" | "direct-only" => Ok(InboxScope::DirectOnly),
        "group" | "group-only" => Ok(InboxScope::GroupOnly),
        value => Err(ExitError::new(
            "invalid_argument",
            2,
            format!("unsupported inbox scope {value:?}."),
            "Use --scope all, --scope direct, or --scope group.",
        )),
    }
}

fn legacy_inbox_scope(scope: InboxScope) -> String {
    match scope {
        InboxScope::All => "all",
        InboxScope::DirectOnly => "direct",
        InboxScope::GroupOnly => "group",
    }
    .to_string()
}

struct DirectTextSessionProvider<'a> {
    subject: im_core::prelude::Did,
    resolved: &'a Resolved,
    manager: &'a Manager,
    record: crate::identity::types::StoredIdentity,
}

impl im_core::compat::messages::BridgeSessionProvider for DirectTextSessionProvider<'_> {
    fn ensure_messaging_session(&self) -> im_core::ImResult<SessionBundle> {
        let session = auth_session(&self.resolved, &self.manager, &self.record)
            .map_err(message_error_to_im_error)?;
        Ok(SessionBundle {
            subject: self.subject.clone(),
            scope: AuthScope::Messaging,
            expires_at: None,
            refreshed: session.current_jwt().trim() != self.record.jwt_token.trim(),
        })
    }
}

struct GroupTextSessionProvider<'a> {
    subject: im_core::prelude::Did,
    resolved: &'a Resolved,
    manager: &'a Manager,
    record: crate::identity::types::StoredIdentity,
}

impl im_core::compat::messages::BridgeGroupSessionProvider for GroupTextSessionProvider<'_> {
    fn ensure_group_messaging_session(&self) -> im_core::ImResult<SessionBundle> {
        let session = auth_session(&self.resolved, &self.manager, &self.record)
            .map_err(message_error_to_im_error)?;
        Ok(SessionBundle {
            subject: self.subject.clone(),
            scope: AuthScope::GroupMessaging,
            expires_at: None,
            refreshed: session.current_jwt().trim() != self.record.jwt_token.trim(),
        })
    }
}

struct DirectTextLegacyTransport<'a> {
    resolved: &'a Resolved,
    manager: &'a Manager,
    record: crate::identity::types::StoredIdentity,
}

impl im_core::compat::messages::BridgeAuthenticatedRpcTransport for DirectTextLegacyTransport<'_> {
    fn authenticated_rpc(
        &mut self,
        endpoint: &str,
        method: &str,
        params: Value,
    ) -> im_core::ImResult<Value> {
        send_authenticated_direct_rpc_with_fallback(
            self.resolved,
            self.manager,
            &self.record,
            endpoint,
            method,
            params,
        )
        .map_err(message_error_to_im_error)
    }
}

struct GroupTextLegacyTransport<'a> {
    resolved: &'a Resolved,
    manager: &'a Manager,
    record: crate::identity::types::StoredIdentity,
}

impl im_core::compat::messages::BridgeAuthenticatedRpcTransport for GroupTextLegacyTransport<'_> {
    fn authenticated_rpc(
        &mut self,
        endpoint: &str,
        method: &str,
        params: Value,
    ) -> im_core::ImResult<Value> {
        send_authenticated_direct_rpc_with_fallback(
            self.resolved,
            self.manager,
            &self.record,
            endpoint,
            method,
            params,
        )
        .map_err(message_error_to_im_error)
    }
}

fn send_authenticated_direct_rpc_with_fallback(
    resolved: &Resolved,
    manager: &Manager,
    record: &crate::identity::types::StoredIdentity,
    endpoint: &str,
    method: &str,
    params: Value,
) -> Result<Value, MessageError> {
    match send_authenticated_direct_rpc(resolved, manager, record, endpoint, method, params.clone())
    {
        Ok(result) => Ok(result),
        Err(err) if message::is_session_unauthorized(&err) => {
            let refreshed = message::refresh_jwt_fallback(resolved, manager, record).ok();
            match send_authenticated_direct_rpc(
                resolved,
                manager,
                refreshed.as_ref().unwrap_or(record),
                endpoint,
                method,
                params,
            ) {
                Ok(result) => Ok(result),
                Err(_) => Err(err),
            }
        }
        Err(err) => Err(err),
    }
}

fn send_authenticated_direct_rpc(
    resolved: &Resolved,
    manager: &Manager,
    record: &crate::identity::types::StoredIdentity,
    endpoint: &str,
    method: &str,
    params: Value,
) -> Result<Value, MessageError> {
    let mut auth = auth_session(resolved, manager, record)?;
    let client = message::Client::new(resolved)?;
    client.authenticated_rpc_call_profile(Profile::RpcDefault, endpoint, method, params, &mut auth)
}

fn auth_session(
    resolved: &Resolved,
    manager: &Manager,
    record: &crate::identity::types::StoredIdentity,
) -> Result<Session, MessageError> {
    message::auth_session(resolved, manager, record)
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

#[derive(Debug, Clone, Default, serde::Deserialize, serde::Serialize)]
struct GroupSendResult {
    #[serde(default)]
    accepted: bool,
    #[serde(default)]
    final_acceptance: bool,
    #[serde(default)]
    group_did: String,
    #[serde(default)]
    message_id: String,
    #[serde(default)]
    operation_id: String,
    #[serde(default)]
    group_event_seq: String,
    #[serde(default)]
    group_state_version: String,
    #[serde(default)]
    accepted_at: String,
}

impl DirectSendResult {
    fn from_sdk_bridge(result: &im_core::compat::messages::DirectTextSendBridgeResult) -> Self {
        let mut value: Self = serde_json::from_value(result.raw.clone()).unwrap_or_default();
        value.message_id =
            default_string_value(&value.message_id, result.sdk_result.message.id.as_str());
        value.operation_id = default_string_value(
            &value.operation_id,
            result
                .sdk_result
                .message
                .metadata
                .operation_id
                .as_deref()
                .unwrap_or_default(),
        );
        value.target_did = default_string_value(&value.target_did, &result.target_did);
        value.accepted_at = default_string_value(
            &value.accepted_at,
            result
                .sdk_result
                .message
                .sent_at
                .as_deref()
                .unwrap_or_default(),
        );
        value.delivery_state = default_string_value(
            &value.delivery_state,
            result
                .sdk_result
                .message
                .metadata
                .delivery_state
                .as_deref()
                .unwrap_or_default(),
        );
        value
    }
}

impl GroupSendResult {
    fn from_sdk_bridge(result: &im_core::compat::messages::GroupTextSendBridgeResult) -> Self {
        let mut value: Self = serde_json::from_value(result.raw.clone()).unwrap_or_default();
        value.message_id =
            default_string_value(&value.message_id, result.sdk_result.message.id.as_str());
        value.operation_id = default_string_value(
            &value.operation_id,
            result
                .sdk_result
                .message
                .metadata
                .operation_id
                .as_deref()
                .unwrap_or_default(),
        );
        value.group_did = default_string_value(&value.group_did, &result.group_did);
        value.accepted_at = default_string_value(
            &value.accepted_at,
            result
                .sdk_result
                .message
                .sent_at
                .as_deref()
                .unwrap_or_default(),
        );
        if value.group_event_seq.trim().is_empty() {
            value.group_event_seq = result
                .sdk_result
                .message
                .metadata
                .server_sequence
                .map(|value| value.to_string())
                .unwrap_or_default();
        }
        if value.group_state_version.trim().is_empty() {
            value.group_state_version = result
                .sdk_result
                .message
                .metadata
                .attributes
                .iter()
                .find(|attribute| attribute.key == "group_state_version")
                .map(|attribute| attribute.value.clone())
                .unwrap_or_default();
        }
        value
    }
}

fn fill_direct_send_result(result: &mut DirectSendResult, meta: &Value, target_did: &str) {
    if result.message_id.is_empty() {
        result.message_id = value_string(meta.get("message_id"));
    }
    if result.operation_id.is_empty() {
        result.operation_id = value_string(meta.get("operation_id"));
    }
    if result.target_did.is_empty() {
        result.target_did = target_did.to_string();
    }
}

fn fill_group_send_result(result: &mut GroupSendResult, raw: &Value, group_did: &str) {
    if result.message_id.is_empty() {
        result.message_id = value_string(raw.get("message_id"));
    }
    if result.operation_id.is_empty() {
        result.operation_id = value_string(raw.get("operation_id"));
    }
    if result.group_did.is_empty() {
        result.group_did = group_did.to_string();
    }
    if result.group_event_seq.is_empty() {
        result.group_event_seq = value_string(raw.get("group_event_seq"));
    }
    if result.group_state_version.is_empty() {
        result.group_state_version = value_string(raw.get("group_state_version"));
    }
    if result.accepted_at.is_empty() {
        result.accepted_at = value_string(raw.get("accepted_at"));
    }
}

fn persist_send_result(
    resolved: &Resolved,
    record: &crate::identity::types::StoredIdentity,
    target: &message::TargetResolution,
    text: &str,
    message_type: &str,
    secure: bool,
    result: &DirectSendResult,
    initial_warnings: Vec<String>,
) -> Result<message::CommandResult, MessageError> {
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
                    content_type: content_type_for_message_type(message_type).to_string(),
                    content: text.to_string(),
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
    Ok(message::CommandResult {
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
        warnings: message::compact_warnings(warnings),
    })
}

fn persist_group_send_result(
    resolved: &Resolved,
    record: &crate::identity::types::StoredIdentity,
    group_did: &str,
    text: &str,
    message_type: &str,
    result: &GroupSendResult,
    initial_warnings: Vec<String>,
) -> Result<message::CommandResult, MessageError> {
    let mut warnings = initial_warnings;
    let group_key = group_storage_key(group_did);
    if let Ok(connection) = store::open(&resolved.paths) {
        if store::ensure_schema(&connection).is_ok() {
            let message_id = group_send_message_id(group_did, result);
            let stored = store::store_message(
                &connection,
                MessageRecord {
                    msg_id: message_id,
                    owner_did: record.did.clone(),
                    thread_id: store::make_thread_id(&record.did, "", &group_key),
                    direction: 1,
                    sender_did: record.did.clone(),
                    group_id: group_key.clone(),
                    group_did: group_did.to_string(),
                    content_type: content_type_for_message_type(message_type).to_string(),
                    content: text.to_string(),
                    sent_at: result.accepted_at.clone(),
                    is_read: true,
                    metadata: metadata_string(json!({
                        "group_event_seq": result.group_event_seq,
                        "group_state_version": result.group_state_version,
                        "operation_id": result.operation_id,
                    })),
                    credential_name: record.identity_name.clone(),
                    ..MessageRecord::default()
                },
            );
            if let Err(err) = stored {
                warnings.push(format!("Failed to persist local group message: {err}"));
            }
            let touched = store::touch_group_after_message(
                &connection,
                &record.did,
                &group_key,
                group_did,
                &result.accepted_at,
                i64_option_from_string(&result.group_event_seq),
                &record.identity_name,
                &metadata_string(json!({ "group_state_version": result.group_state_version })),
            );
            if let Err(err) = touched {
                warnings.push(format!("Failed to update group cache: {err}"));
            }
        }
    }
    Ok(message::CommandResult {
        data: json!({
            "action": "send_message",
            "target": {
                "kind": "group",
                "did": group_did,
            },
            "message": {
                "id": group_send_message_id(group_did, result),
                "type": message_type,
                "secure": false,
                "sent_at": result.accepted_at,
            },
            "delivery": result,
            "source": "remote_http",
        }),
        summary: format!("Sent a group {message_type} message"),
        warnings: message::compact_warnings(warnings),
    })
}

fn group_send_message_id(group_did: &str, result: &GroupSendResult) -> String {
    if !result.group_did.trim().is_empty() && !result.group_event_seq.trim().is_empty() {
        return format!(
            "{}:{}",
            result.group_did.trim(),
            result.group_event_seq.trim()
        );
    }
    if !result.group_event_seq.trim().is_empty() {
        return format!("{}:{}", group_did.trim(), result.group_event_seq.trim());
    }
    if !result.message_id.trim().is_empty() {
        return result.message_id.clone();
    }
    format!("msg-{}", im_core::compat::wire::generate_operation_id())
}

fn group_storage_key(group_did: &str) -> String {
    group_did.trim().to_string()
}

fn i64_option_from_string(value: &str) -> Option<i64> {
    value.trim().parse().ok()
}

fn content_type_for_message_type(message_type: &str) -> &'static str {
    match message_type.trim().to_ascii_lowercase().as_str() {
        "markdown" => "text/markdown",
        _ => "text/plain",
    }
}

fn metadata_string(value: Value) -> String {
    serde_json::to_string(&value).unwrap_or_default()
}

fn default_string_value(value: &str, fallback: &str) -> String {
    if value.trim().is_empty() {
        fallback.to_string()
    } else {
        value.to_string()
    }
}

fn value_string(value: Option<&Value>) -> String {
    value
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn im_error_to_message_error(err: im_core::ImError) -> MessageError {
    match err {
        im_core::ImError::InvalidInput { field, .. } if field.as_deref() == Some("text") => {
            MessageError::TextRequired
        }
        im_core::ImError::PeerNotFound { .. } => MessageError::TargetRequired,
        im_core::ImError::UnsupportedCapability { capability } if capability == "group-send" => {
            MessageError::GroupNotSupported
        }
        im_core::ImError::UnsupportedCapability { capability } if capability == "attachments" => {
            MessageError::AttachmentNotSupported
        }
        im_core::ImError::UnsupportedCapability { capability } if capability == "secure-direct" => {
            MessageError::SecureNotSupported
        }
        im_core::ImError::AuthRequired | im_core::ImError::SessionExpired => {
            MessageError::IdentityRequired("authentication is required".to_string())
        }
        im_core::ImError::IdentityNotReady { identity, missing } => MessageError::IdentityRequired(
            format!("identity {identity} is not ready: {}", missing.join(", ")),
        ),
        im_core::ImError::Service {
            status_code,
            code,
            message,
        } => MessageError::Service(crate::identity::wire::ServiceError {
            status_code: status_code.unwrap_or_default(),
            rpc_code: code
                .and_then(|value| value.parse().ok())
                .unwrap_or_default(),
            message,
            data: None,
        }),
        im_core::ImError::TransportUnavailable { detail } => {
            MessageError::TransportUnavailable(detail)
        }
        err => MessageError::Internal(err.to_string()),
    }
}

fn message_error_to_im_error(err: MessageError) -> im_core::ImError {
    match err {
        MessageError::Service(service_err) => im_core::ImError::Service {
            status_code: (service_err.status_code != 0).then_some(service_err.status_code),
            code: (service_err.rpc_code != 0).then(|| service_err.rpc_code.to_string()),
            message: service_err.message,
        },
        MessageError::TransportUnavailable(detail) => {
            im_core::ImError::TransportUnavailable { detail }
        }
        MessageError::TargetRequired => im_core::ImError::PeerNotFound {
            peer: "direct target".to_string(),
        },
        MessageError::TextRequired => im_core::ImError::invalid_input(
            Some("text".to_string()),
            "text message must not be empty",
        ),
        MessageError::IdentityRequired(message) => im_core::ImError::IdentityNotReady {
            identity: message,
            missing: Vec::new(),
        },
        err => im_core::ImError::Internal {
            message: err.to_string(),
        },
    }
}

fn page_limit(command: &ParsedCommand, flag: &str, default: u32) -> Result<PageLimit, ExitError> {
    let raw = string_flag(command, flag);
    let value = if raw.trim().is_empty() {
        default
    } else {
        raw.trim().parse::<u32>().map_err(|err| {
            ExitError::new(
                "invalid_argument",
                2,
                format!("invalid --{flag}: {err}"),
                "Use a positive integer limit.",
            )
        })?
    };
    PageLimit::new(value).map_err(|err| {
        ExitError::new(
            "invalid_argument",
            2,
            format!("invalid --{flag}: {err}"),
            "Use a positive integer limit.",
        )
    })
}

fn optional_cursor(command: &ParsedCommand) -> Result<Option<Cursor>, ExitError> {
    let raw = string_flag(command, "cursor");
    if raw.trim().is_empty() {
        return Ok(None);
    }
    Cursor::parse(raw).map(Some).map_err(|err| {
        ExitError::new(
            "invalid_argument",
            2,
            format!("invalid --cursor: {err}"),
            "Use a non-empty cursor returned by the service.",
        )
    })
}

fn parse_peer(raw: &str, default_domain: &str) -> Result<PeerRef, ExitError> {
    PeerRef::parse(raw, default_domain).map_err(|err| {
        ExitError::new(
            "invalid_argument",
            2,
            format!("invalid peer target: {err}"),
            "Use a peer DID or handle.",
        )
    })
}

fn parse_group(raw: &str) -> Result<GroupRef, ExitError> {
    GroupRef::parse(raw).map_err(|err| {
        ExitError::new(
            "invalid_argument",
            2,
            format!("invalid group target: {err}"),
            "Use an existing group DID or id.",
        )
    })
}

fn bool_flag(command: &ParsedCommand, name: &str) -> bool {
    string_flag(command, name).trim() == "true"
}

fn string_flag(command: &ParsedCommand, name: &str) -> String {
    command.flags.get(name).cloned().unwrap_or_default()
}
