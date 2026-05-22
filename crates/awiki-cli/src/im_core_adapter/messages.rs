mod read_bridge;

// Temporary migration-only legacy bridge exception.
// Delete in PR C4/C7 when msg send/inbox/history/mark-read default handlers call
// im-core public message APIs directly instead of translating SDK DTOs back to
// legacy message requests, compat bridges, and legacy render records.

use std::fs;

use im_core::prelude::{
    Cursor, DeliveryState, GroupRef, Handle, HistoryQuery, InboxQuery, InboxScope, MessageBody,
    MessageBodyView, MessageDeliveryOptions, MessageKind, MessageMetadataAttribute,
    MessageSecurityMode, MessageTarget, PageLimit, PeerRef, SendMessageRequest, SendMessageResult,
    ThreadRef,
};
use serde_json::{json, Value};

use crate::cli::ParsedCommand;
use crate::config::Resolved;
use crate::message;
use crate::message::MessageError;
use crate::output::ExitError;
use crate::store::{self, MessageRecord};

pub use read_bridge::{mark_read_via_im_core, read_history_via_im_core, read_inbox_via_im_core};

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

pub fn send_text_via_im_core(
    resolved: &Resolved,
    client: &im_core::ImClient,
    mut request: SendMessageRequest,
) -> Result<message::CommandResult, MessageError> {
    let direct_target = resolve_direct_target_for_sdk(client, &request)?;
    if let Some(target) = &direct_target {
        request.target = MessageTarget::Direct(
            PeerRef::parse(&target.did, "").map_err(im_error_to_message_error)?,
        );
    }
    let result = client
        .messages()
        .send(request)
        .map_err(im_error_to_message_error)?;
    match &result.message.thread {
        ThreadRef::Direct(_) => {
            let target = direct_target.unwrap_or_else(|| direct_target_from_result(&result));
            persist_send_result(resolved, client, &target, &result)
        }
        ThreadRef::Group(group) => {
            persist_group_send_result(resolved, client, group.as_str(), &result)
        }
        ThreadRef::Thread(_) => Err(MessageError::Internal(
            "thread send results are not supported by the CLI renderer".to_string(),
        )),
    }
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

fn resolve_direct_target_for_sdk(
    client: &im_core::ImClient,
    request: &SendMessageRequest,
) -> Result<Option<message::TargetResolution>, MessageError> {
    let MessageTarget::Direct(peer) = &request.target else {
        return Ok(None);
    };
    if peer.as_str().starts_with("did:") {
        return Ok(Some(message::TargetResolution {
            did: peer.as_str().to_string(),
            handle: String::new(),
        }));
    }
    let handle = Handle::parse(peer.as_str(), "").map_err(im_error_to_message_error)?;
    let lookup = client
        .directory()
        .lookup_handle(handle)
        .map_err(im_error_to_message_error)?;
    Ok(Some(message::TargetResolution {
        did: lookup.did.as_str().to_string(),
        handle: lookup.handle.as_str().to_string(),
    }))
}

fn direct_target_from_result(result: &SendMessageResult) -> message::TargetResolution {
    let did = result
        .message
        .receiver
        .as_ref()
        .or_else(|| match &result.message.thread {
            ThreadRef::Direct(peer) => Some(peer),
            _ => None,
        })
        .map(|peer| peer.as_str().to_string())
        .unwrap_or_default();
    message::TargetResolution {
        did,
        handle: String::new(),
    }
}

fn message_text_and_type(body: &MessageBodyView) -> Result<(&str, &'static str), MessageError> {
    match body {
        MessageBodyView::Text { text, kind } => Ok((text.as_str(), message_type_for_kind(kind))),
        MessageBodyView::Unsupported { content_type } => Err(MessageError::Internal(format!(
            "unsupported message body returned by im-core: {}",
            content_type.as_deref().unwrap_or("unknown")
        ))),
    }
}

fn message_type_for_kind(kind: &MessageKind) -> &'static str {
    match kind {
        MessageKind::Text => "text",
        MessageKind::Markdown => "markdown",
    }
}

fn delivery_was_accepted(delivery: &DeliveryState) -> bool {
    matches!(delivery, DeliveryState::Accepted | DeliveryState::Sent)
}

fn delivery_state_label(result: &SendMessageResult) -> String {
    result
        .message
        .metadata
        .delivery_state
        .clone()
        .unwrap_or_else(|| match &result.delivery {
            DeliveryState::Accepted => "accepted".to_string(),
            DeliveryState::Sent => "sent".to_string(),
            DeliveryState::StoredLocally => "stored_locally".to_string(),
            DeliveryState::Failed { reason } if reason.trim().is_empty() => "failed".to_string(),
            DeliveryState::Failed { reason } => reason.clone(),
        })
}

fn message_attribute(attributes: &[MessageMetadataAttribute], key: &str) -> Option<String> {
    attributes
        .iter()
        .find(|attribute| attribute.key == key)
        .map(|attribute| attribute.value.clone())
        .filter(|value| !value.trim().is_empty())
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
    #[serde(default)]
    delivery_state: String,
}

impl DirectSendResult {
    fn from_sdk_result(result: &SendMessageResult, target: &message::TargetResolution) -> Self {
        Self {
            accepted: delivery_was_accepted(&result.delivery),
            message_id: result.message.id.as_str().to_string(),
            operation_id: result
                .message
                .metadata
                .operation_id
                .clone()
                .unwrap_or_default(),
            target_did: target.did.clone(),
            accepted_at: result.message.sent_at.clone().unwrap_or_default(),
            final_acceptance: matches!(result.delivery, DeliveryState::Sent),
            delivery_state: delivery_state_label(result),
        }
    }
}

impl GroupSendResult {
    fn from_sdk_result(result: &SendMessageResult, group_did: &str) -> Self {
        Self {
            accepted: delivery_was_accepted(&result.delivery),
            final_acceptance: matches!(result.delivery, DeliveryState::Sent),
            group_did: group_did.to_string(),
            message_id: result.message.id.as_str().to_string(),
            operation_id: result
                .message
                .metadata
                .operation_id
                .clone()
                .unwrap_or_default(),
            group_event_seq: result
                .message
                .metadata
                .server_sequence
                .map(|value| value.to_string())
                .or_else(|| {
                    message_attribute(&result.message.metadata.attributes, "group_event_seq")
                })
                .unwrap_or_default(),
            group_state_version: message_attribute(
                &result.message.metadata.attributes,
                "group_state_version",
            )
            .unwrap_or_default(),
            accepted_at: result.message.sent_at.clone().unwrap_or_default(),
            delivery_state: delivery_state_label(result),
        }
    }
}

fn persist_send_result(
    resolved: &Resolved,
    client: &im_core::ImClient,
    target: &message::TargetResolution,
    sdk_result: &SendMessageResult,
) -> Result<message::CommandResult, MessageError> {
    let result = DirectSendResult::from_sdk_result(sdk_result, target);
    let (text, message_type) = message_text_and_type(&sdk_result.message.body)?;
    let owner_did = client.did().as_str();
    let credential_name = client.current_identity().id.as_str();
    let mut warnings = sdk_result.warnings.clone();
    if let Ok(connection) = store::open(&resolved.paths) {
        if store::ensure_schema(&connection).is_ok() {
            let stored = store::store_message(
                &connection,
                MessageRecord {
                    msg_id: result.message_id.clone(),
                    owner_identity_id: credential_name.to_string(),
                    owner_did: owner_did.to_string(),
                    thread_id: store::make_thread_id(owner_did, &target.did, ""),
                    direction: 1,
                    sender_did: owner_did.to_string(),
                    receiver_did: target.did.clone(),
                    content_type: content_type_for_message_type(message_type).to_string(),
                    content: text.to_string(),
                    sent_at: result.accepted_at.clone(),
                    is_read: true,
                    is_e2ee: false,
                    metadata: metadata_string(json!({
                        "delivery_state": result.delivery_state,
                        "operation_id": result.operation_id,
                        "target_handle": target.handle,
                    })),
                    credential_name: credential_name.to_string(),
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
                "secure": false,
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
    client: &im_core::ImClient,
    group_did: &str,
    sdk_result: &SendMessageResult,
) -> Result<message::CommandResult, MessageError> {
    let result = GroupSendResult::from_sdk_result(sdk_result, group_did);
    let (text, message_type) = message_text_and_type(&sdk_result.message.body)?;
    let owner_did = client.did().as_str();
    let credential_name = client.current_identity().id.as_str();
    let mut warnings = sdk_result.warnings.clone();
    let group_key = group_storage_key(group_did);
    if let Ok(connection) = store::open(&resolved.paths) {
        if store::ensure_schema(&connection).is_ok() {
            let message_id = group_send_message_id(group_did, &result);
            let stored = store::store_message(
                &connection,
                MessageRecord {
                    msg_id: message_id,
                    owner_identity_id: credential_name.to_string(),
                    owner_did: owner_did.to_string(),
                    thread_id: store::make_thread_id(owner_did, "", &group_key),
                    direction: 1,
                    sender_did: owner_did.to_string(),
                    group_id: group_key.clone(),
                    group_did: group_did.to_string(),
                    content_type: content_type_for_message_type(message_type).to_string(),
                    content: text.to_string(),
                    sent_at: result.accepted_at.clone(),
                    is_read: true,
                    metadata: metadata_string(json!({
                        "delivery_state": result.delivery_state,
                        "group_event_seq": result.group_event_seq,
                        "group_state_version": result.group_state_version,
                        "operation_id": result.operation_id,
                    })),
                    credential_name: credential_name.to_string(),
                    ..MessageRecord::default()
                },
            );
            if let Err(err) = stored {
                warnings.push(format!("Failed to persist local group message: {err}"));
            }
            let touched = store::touch_group_after_message(
                &connection,
                owner_did,
                &group_key,
                group_did,
                &result.accepted_at,
                i64_option_from_string(&result.group_event_seq),
                credential_name,
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
                "id": group_send_message_id(group_did, &result),
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
    format!("{}:local", group_did.trim())
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

fn im_error_to_message_error(err: im_core::ImError) -> MessageError {
    match err {
        im_core::ImError::InvalidInput { field, .. } if field.as_deref() == Some("text") => {
            MessageError::TextRequired
        }
        im_core::ImError::PeerNotFound { peer } => {
            MessageError::IdentityRequired(format!("peer not found: {peer}"))
        }
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
