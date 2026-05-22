// Temporary migration-only legacy bridge exception.
// Delete in PR C7 when identity/group/profile default handlers no longer keep
// message rendering adapters coupled to legacy cache/render helpers.

use std::fs;

use im_core::prelude::{
    Cursor, DeliveryState, GroupRef, Handle, HistoryQuery, InboxQuery, InboxScope, MessageBody,
    MessageBodyView, MessageDeliveryOptions, MessageDirection, MessageId, MessageKind,
    MessageMetadataAttribute, MessageSecurityMode, MessageTarget, Page, PageLimit, PeerRef,
    SendMessageRequest, SendMessageResult, ThreadRef,
};
use serde_json::{json, Value};

use crate::cli::ParsedCommand;
use crate::config::Resolved;
use crate::im_core_adapter::message_result::{CommandResult, MessageAdapterError, ServiceError};
use crate::message;
use crate::output::ExitError;
use crate::store::{self, MessageRecord};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct TargetResolution {
    did: String,
    handle: String,
}

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
) -> Result<CommandResult, MessageAdapterError> {
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
        ThreadRef::Thread(_) => Err(MessageAdapterError::Internal(
            "thread send results are not supported by the CLI renderer".to_string(),
        )),
    }
}

pub fn read_inbox_via_im_core(
    resolved: &Resolved,
    manager: &crate::identity::Manager,
    client: &im_core::ImClient,
    identity_name: &str,
    query: InboxQuery,
) -> Result<CommandResult, MessageAdapterError> {
    let record = message::require_active_identity(resolved, manager, identity_name)
        .map_err(MessageAdapterError::from)?;
    let mut warnings = message::maybe_publish_secure_prekeys(resolved, manager, &record);
    let page = client
        .messages()
        .inbox(query.clone())
        .map_err(im_error_to_message_error)?;
    let raw = read_page_to_cli_raw(&page, source_default());
    let mut messages =
        message::persist_inbox_messages(resolved, manager, &record, &raw, "", &mut warnings);
    let source = source_with_default(&raw);
    messages = apply_inbox_filters(messages, "", query.unread_only, i64::from(query.limit.0));
    let total = messages.len();
    let data = match query.scope {
        InboxScope::DirectOnly => json!({
            "messages": messages,
            "total": total,
            "source": source,
            "with": "",
        }),
        InboxScope::All | InboxScope::GroupOnly => json!({
            "messages": messages,
            "total": total,
            "source": source,
        }),
    };
    Ok(CommandResult {
        data,
        summary: format!("Loaded {total} inbox messages"),
        warnings: compact_warnings(warnings),
    })
}

pub fn read_history_via_im_core(
    resolved: &Resolved,
    manager: &crate::identity::Manager,
    client: &im_core::ImClient,
    identity_name: &str,
    thread: ThreadRef,
    query: HistoryQuery,
) -> Result<CommandResult, MessageAdapterError> {
    match thread {
        ThreadRef::Direct(peer) => {
            read_direct_history_via_im_core(resolved, manager, client, identity_name, peer, query)
        }
        ThreadRef::Group(group) => {
            read_group_history_via_im_core(resolved, manager, client, identity_name, group, query)
        }
        ThreadRef::Thread(_) => Err(MessageAdapterError::Internal(
            "thread history is not supported by the CLI renderer".to_string(),
        )),
    }
}

fn read_direct_history_via_im_core(
    resolved: &Resolved,
    manager: &crate::identity::Manager,
    client: &im_core::ImClient,
    identity_name: &str,
    peer: PeerRef,
    query: HistoryQuery,
) -> Result<CommandResult, MessageAdapterError> {
    let record = message::require_active_identity(resolved, manager, identity_name)
        .map_err(MessageAdapterError::from)?;
    let mut warnings = message::maybe_publish_secure_prekeys(resolved, manager, &record);
    let (thread, target, target_is_handle) = resolve_history_thread(client, peer)?;
    let page = client
        .messages()
        .history(thread, query.clone())
        .map_err(im_error_to_message_error)?;
    let mut raw = read_page_to_cli_raw(&page, source_default());
    if target_is_handle {
        let dids =
            peer_dids_for_handle_from_store(resolved, &record.did, &target.handle, &target.did)
                .unwrap_or_else(|_| vec![target.did.clone()]);
        raw["resolved_dids"] = json!(dids);
    }
    let mut messages = message::persist_history_messages(
        resolved,
        manager,
        &record,
        &target.did,
        &target.handle,
        &raw,
        &mut warnings,
    );
    let mut source = source_with_default(&raw);
    let mut resolved_dids = resolved_dids_value(&raw);
    if target_is_handle {
        let dids = merge_handle_history_messages(
            resolved,
            &record.did,
            &target,
            i64::from(query.limit.0),
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
        warnings: compact_warnings(warnings),
    })
}

fn read_group_history_via_im_core(
    resolved: &Resolved,
    manager: &crate::identity::Manager,
    client: &im_core::ImClient,
    identity_name: &str,
    group: GroupRef,
    query: HistoryQuery,
) -> Result<CommandResult, MessageAdapterError> {
    let record = message::require_active_identity(resolved, manager, identity_name)
        .map_err(MessageAdapterError::from)?;
    let mut warnings = message::maybe_publish_secure_prekeys(resolved, manager, &record);
    let page = client
        .messages()
        .history(ThreadRef::Group(group.clone()), query.clone())
        .map_err(im_error_to_message_error)?;
    let mut raw = read_page_to_cli_raw(&page, source_default());
    warnings.extend(message::maybe_decrypt_group_messages(
        resolved,
        &record,
        group.as_str(),
        &mut raw,
    ));
    warnings.extend(message::persist_group_messages(
        resolved,
        &record,
        group.as_str(),
        &raw,
    ));
    let messages = raw
        .get("messages")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let source = source_with_default(&raw);
    let total = messages.len();
    Ok(CommandResult {
        data: json!({
            "messages": messages,
            "total": total,
            "source": source,
            "group": group.as_str(),
        }),
        summary: format!("Loaded {total} group history messages"),
        warnings: compact_warnings(warnings),
    })
}

pub fn mark_read_via_im_core(
    _resolved: &Resolved,
    _manager: &crate::identity::Manager,
    client: &im_core::ImClient,
    _identity_name: &str,
    message_ids: Vec<String>,
) -> Result<CommandResult, MessageAdapterError> {
    if message_ids.is_empty() {
        return Err(MessageAdapterError::MessageNotFound);
    }
    let ids = message_ids
        .iter()
        .map(MessageId::parse)
        .collect::<im_core::ImResult<Vec<_>>>()
        .map_err(im_error_to_message_error)?;
    let result = client
        .messages()
        .mark_read(ids)
        .map_err(im_error_to_message_error)?;
    let updated_count = result.updated_count;
    Ok(CommandResult {
        data: json!({
            "action": "mark_read",
            "updated_count": updated_count,
            "message_ids": message_ids,
        }),
        summary: format!("Marked {updated_count} messages as read"),
        warnings: compact_warnings(result.warnings),
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

fn resolve_direct_target_for_sdk(
    client: &im_core::ImClient,
    request: &SendMessageRequest,
) -> Result<Option<TargetResolution>, MessageAdapterError> {
    let MessageTarget::Direct(peer) = &request.target else {
        return Ok(None);
    };
    if peer.as_str().starts_with("did:") {
        return Ok(Some(TargetResolution {
            did: peer.as_str().to_string(),
            handle: String::new(),
        }));
    }
    let handle = Handle::parse(peer.as_str(), "").map_err(im_error_to_message_error)?;
    let lookup = client
        .directory()
        .lookup_handle(handle)
        .map_err(im_error_to_message_error)?;
    Ok(Some(TargetResolution {
        did: lookup.did.as_str().to_string(),
        handle: lookup.handle.as_str().to_string(),
    }))
}

fn resolve_history_thread(
    client: &im_core::ImClient,
    peer: PeerRef,
) -> Result<(ThreadRef, TargetResolution, bool), MessageAdapterError> {
    let original = peer.as_str().trim().to_string();
    let target_is_handle = !original.is_empty() && !original.starts_with("did:");
    let target = if target_is_handle {
        let handle = Handle::parse(&original, "").map_err(im_error_to_message_error)?;
        let lookup = client
            .directory()
            .lookup_handle(handle)
            .map_err(im_error_to_message_error)?;
        TargetResolution {
            did: lookup.did.as_str().to_string(),
            handle: lookup.handle.as_str().to_string(),
        }
    } else {
        TargetResolution {
            did: original,
            handle: String::new(),
        }
    };
    let thread =
        ThreadRef::Direct(PeerRef::parse(&target.did, "").map_err(im_error_to_message_error)?);
    Ok((thread, target, target_is_handle))
}

fn read_page_to_cli_raw(page: &Page<im_core::prelude::Message>, source: &str) -> Value {
    json!({
        "messages": page.items.iter().map(message_to_cli_json).collect::<Vec<_>>(),
        "total": page.items.len(),
        "source": source,
        "next_cursor": page.next_cursor.as_ref().map(|cursor| cursor.as_str().to_string()),
        "has_more": page.has_more,
    })
}

fn message_to_cli_json(message: &im_core::prelude::Message) -> Value {
    let mut value = json!({
        "id": message.id.as_str(),
        "msg_id": message.id.as_str(),
        "message_id": message.id.as_str(),
        "sender_did": message.sender.as_str(),
        "receiver_did": message.receiver.as_ref().map(|peer| peer.as_str()).unwrap_or_default(),
        "group_did": message.group.as_ref().map(|group| group.as_str()).unwrap_or_default(),
        "content": message_body_content(&message.body),
        "content_type": message_content_type(&message.body),
        "sent_at": message.sent_at.clone().unwrap_or_default(),
        "received_at": message.received_at.clone().unwrap_or_default(),
        "is_read": false,
        "secure": false,
        "direction": match message.direction {
            MessageDirection::Outgoing => 1,
            MessageDirection::Incoming => 0,
            MessageDirection::Unknown => -1,
        },
    });
    if let Some(sequence) = message.metadata.server_sequence {
        value["server_seq"] = json!(sequence);
    }
    if let Some(operation_id) = &message.metadata.operation_id {
        value["operation_id"] = json!(operation_id);
    }
    if let Some(delivery_state) = &message.metadata.delivery_state {
        value["delivery_state"] = json!(delivery_state);
    }
    for attribute in &message.metadata.attributes {
        if !attribute.key.trim().is_empty() {
            value[attribute.key.as_str()] = json!(attribute.value);
        }
    }
    value
}

fn message_body_content(body: &MessageBodyView) -> String {
    match body {
        MessageBodyView::Text { text, .. } => text.clone(),
        MessageBodyView::Unsupported { .. } => String::new(),
    }
}

fn message_content_type(body: &MessageBodyView) -> &'static str {
    match body {
        MessageBodyView::Text {
            kind: MessageKind::Markdown,
            ..
        } => "text/markdown",
        MessageBodyView::Text { .. } => "text/plain",
        MessageBodyView::Unsupported { .. } => "application/octet-stream",
    }
}

fn source_default() -> &'static str {
    "remote_http"
}

fn source_with_default(raw: &Value) -> String {
    raw.get("source")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(source_default())
        .to_string()
}

fn direct_target_from_result(result: &SendMessageResult) -> TargetResolution {
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
    TargetResolution {
        did,
        handle: String::new(),
    }
}

fn peer_handle_or_did(target: &TargetResolution) -> String {
    if target.handle.trim().is_empty() {
        target.did.clone()
    } else {
        target.handle.clone()
    }
}

fn merge_handle_history_messages(
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
    let dids =
        match peer_dids_for_handle_from_store(resolved, owner_did, &target.handle, &target.did) {
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
            let cache_has_pending_direct_e2ee = contains_pending_direct_e2ee_wire_messages(&cached);
            let cached = filter_displayable_direct_e2ee_messages(cached);
            if cached.is_empty() {
                return Some(dids);
            }
            let should_prefer_cache = !cache_has_pending_direct_e2ee
                && should_prefer_direct_cache_messages(messages, &cached);
            let merged = merge_direct_history_messages(messages, cached, limit);
            if merged.len() > messages.len() || should_prefer_cache {
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

fn peer_dids_for_handle_from_store(
    resolved: &Resolved,
    owner_did: &str,
    handle: &str,
    current_did: &str,
) -> Result<Vec<String>, MessageAdapterError> {
    let handle = normalize_handle_value(handle);
    if handle.is_empty() {
        return Ok(merge_peer_dids(current_did, &[]));
    }
    let connection = store::open(&resolved.paths)
        .map_err(|err| MessageAdapterError::Internal(format!("open local message store: {err}")))?;
    store::ensure_schema(&connection).map_err(|err| {
        MessageAdapterError::Internal(format!("ensure local message store schema: {err}"))
    })?;
    let dids = store::list_dids_by_handle(&connection, owner_did, &handle).map_err(|err| {
        MessageAdapterError::Internal(format!("list contact DIDs by handle: {err}"))
    })?;
    Ok(merge_peer_dids(current_did, &dids))
}

fn normalize_handle_value(value: &str) -> String {
    let value = value.trim().to_ascii_lowercase();
    if value.is_empty() {
        return String::new();
    }
    let value = value.trim_start_matches("wba://");
    match value.find('.') {
        Some(index) if index > 0 => value[..index].to_string(),
        _ => value.to_string(),
    }
}

fn merge_peer_dids(current: &str, historical: &[String]) -> Vec<String> {
    let mut seen = Vec::with_capacity(historical.len() + 1);
    let mut result = Vec::with_capacity(historical.len() + 1);
    let current = current.trim();
    if !current.is_empty() {
        seen.push(current.to_string());
        result.push(current.to_string());
    }
    for did in historical {
        let did = did.trim();
        if did.is_empty() || seen.iter().any(|known| known == did) {
            continue;
        }
        seen.push(did.to_string());
        result.push(did.to_string());
    }
    result
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

fn resolved_dids_value(raw: &Value) -> Value {
    raw.get("resolved_dids").cloned().unwrap_or(Value::Null)
}

fn should_prefer_direct_cache_messages(remote: &[Value], cached: &[Value]) -> bool {
    !cached.is_empty() && !contains_processed_direct_e2ee_messages(remote)
}

fn contains_processed_direct_e2ee_messages(messages: &[Value]) -> bool {
    messages.iter().any(|message| {
        let Some(object) = message.as_object() else {
            return false;
        };
        if !bool_value(object.get("secure")) {
            return false;
        }
        let state = string_value(object.get("decryption_state"));
        state == "decrypted" || state == "undecryptable" || bool_value(object.get("secure_control"))
    })
}

fn contains_pending_direct_e2ee_wire_messages(messages: &[Value]) -> bool {
    messages.iter().any(|message| {
        message
            .get("content_type")
            .and_then(Value::as_str)
            .map(is_direct_e2ee_wire_content_type)
            .unwrap_or(false)
    })
}

fn filter_displayable_direct_e2ee_messages(messages: Vec<Value>) -> Vec<Value> {
    messages
        .into_iter()
        .filter(|message| !is_direct_e2ee_control_or_undisplayable(message))
        .collect()
}

fn is_direct_e2ee_control_or_undisplayable(message: &Value) -> bool {
    let Some(object) = message.as_object() else {
        return false;
    };
    if bool_value(object.get("secure_control")) {
        return true;
    }
    if !is_direct_e2ee_wire_content_type(&string_value(object.get("content_type"))) {
        return false;
    }
    matches!(
        string_value(object.get("decryption_state")).as_str(),
        "" | "undecryptable" | "failed"
    )
}

fn is_direct_e2ee_wire_content_type(content_type: &str) -> bool {
    matches!(
        content_type,
        "application/anp-direct-init+json" | "application/anp-direct-cipher+json"
    )
}

fn merge_direct_history_messages(remote: &[Value], cached: Vec<Value>, limit: i64) -> Vec<Value> {
    if remote.is_empty() {
        return limit_messages(cached, limit);
    }
    if cached.is_empty() {
        return remote.to_vec();
    }
    let mut merged = Vec::with_capacity(remote.len() + cached.len());
    let mut seen = Vec::new();
    for message in cached.into_iter().chain(remote.iter().cloned()) {
        let mut id = message_identity(&message);
        if id.is_empty() {
            id = format!("idx:{}", merged.len());
        }
        if seen.iter().any(|known| known == &id) {
            continue;
        }
        seen.push(id);
        merged.push(message);
    }
    merged.sort_by(|left, right| {
        comparable_server_seq(right)
            .cmp(&comparable_server_seq(left))
            .then_with(|| comparable_message_time(right).cmp(&comparable_message_time(left)))
            .then_with(|| message_identity(right).cmp(&message_identity(left)))
    });
    limit_messages(merged, limit)
}

fn limit_messages(mut messages: Vec<Value>, limit: i64) -> Vec<Value> {
    if limit > 0 {
        messages.truncate(limit as usize);
    }
    messages
}

fn message_identity(message: &Value) -> String {
    message
        .as_object()
        .and_then(|object| {
            object
                .get("id")
                .or_else(|| object.get("message_id"))
                .or_else(|| object.get("msg_id"))
                .or_else(|| object.get("client_msg_id"))
        })
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn comparable_server_seq(message: &Value) -> i64 {
    i64_value(message.get("server_seq")).unwrap_or_default()
}

fn comparable_message_time(message: &Value) -> String {
    message
        .get("sent_at")
        .or_else(|| message.get("created_at"))
        .or_else(|| message.get("stored_at"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn string_value(value: Option<&Value>) -> String {
    value
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn i64_value(value: Option<&Value>) -> Option<i64> {
    match value {
        Some(Value::Number(number)) => number
            .as_i64()
            .or_else(|| number.as_u64().and_then(|value| i64::try_from(value).ok()))
            .or_else(|| number.as_f64().map(|value| value as i64)),
        Some(Value::String(value)) => value.trim().parse::<i64>().ok(),
        _ => None,
    }
}

fn bool_value(value: Option<&Value>) -> bool {
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

fn message_text_and_type(
    body: &MessageBodyView,
) -> Result<(&str, &'static str), MessageAdapterError> {
    match body {
        MessageBodyView::Text { text, kind } => Ok((text.as_str(), message_type_for_kind(kind))),
        MessageBodyView::Unsupported { content_type } => {
            Err(MessageAdapterError::Internal(format!(
                "unsupported message body returned by im-core: {}",
                content_type.as_deref().unwrap_or("unknown")
            )))
        }
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
    fn from_sdk_result(result: &SendMessageResult, target: &TargetResolution) -> Self {
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
            message_id: message_attribute(&result.message.metadata.attributes, "raw_message_id")
                .unwrap_or_else(|| result.message.id.as_str().to_string()),
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
    target: &TargetResolution,
    sdk_result: &SendMessageResult,
) -> Result<CommandResult, MessageAdapterError> {
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
        warnings: compact_warnings(warnings),
    })
}

fn persist_group_send_result(
    resolved: &Resolved,
    client: &im_core::ImClient,
    group_did: &str,
    sdk_result: &SendMessageResult,
) -> Result<CommandResult, MessageAdapterError> {
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
    Ok(CommandResult {
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
        warnings: compact_warnings(warnings),
    })
}

fn compact_warnings(warnings: Vec<String>) -> Vec<String> {
    let mut compact = Vec::new();
    for warning in warnings {
        let warning = warning.trim().to_string();
        if warning.is_empty() || compact.iter().any(|known| known == &warning) {
            continue;
        }
        compact.push(warning);
    }
    compact
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

fn im_error_to_message_error(err: im_core::ImError) -> MessageAdapterError {
    match err {
        im_core::ImError::InvalidInput { field, .. } if field.as_deref() == Some("text") => {
            MessageAdapterError::TextRequired
        }
        im_core::ImError::PeerNotFound { peer } => {
            MessageAdapterError::IdentityRequired(format!("peer not found: {peer}"))
        }
        im_core::ImError::UnsupportedCapability { capability } if capability == "group-send" => {
            MessageAdapterError::GroupNotSupported
        }
        im_core::ImError::UnsupportedCapability { capability } if capability == "attachments" => {
            MessageAdapterError::AttachmentNotSupported
        }
        im_core::ImError::UnsupportedCapability { capability } if capability == "secure-direct" => {
            MessageAdapterError::SecureNotSupported
        }
        im_core::ImError::AuthRequired | im_core::ImError::SessionExpired => {
            MessageAdapterError::IdentityRequired("authentication is required".to_string())
        }
        im_core::ImError::IdentityNotReady { identity, missing } => {
            MessageAdapterError::IdentityRequired(format!(
                "identity {identity} is not ready: {}",
                missing.join(", ")
            ))
        }
        im_core::ImError::Service {
            status_code,
            code,
            message,
        } => MessageAdapterError::Service(ServiceError {
            status_code: status_code.unwrap_or_default(),
            rpc_code: code
                .and_then(|value| value.parse().ok())
                .unwrap_or_default(),
            message,
            data: None,
        }),
        im_core::ImError::TransportUnavailable { detail } => {
            MessageAdapterError::TransportUnavailable(detail)
        }
        err => MessageAdapterError::Internal(err.to_string()),
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
