use std::fs;
use std::path::{Path, PathBuf};

use im_core::messages::{
    LocalHistoryQuery, MessageSyncOutcome, MessageSyncRequest, MessageSyncStatus,
};
use im_core::prelude::{
    AttachmentDestination, AttachmentInput, AttachmentSelection, AttachmentSendRequest,
    AttachmentSendResult, Cursor, DeliveryState, DownloadAttachmentRequest,
    DownloadedAttachmentDestination, GroupRef, HistoryQuery, InboxQuery, InboxScope, MessageBody,
    MessageBodyView, MessageDeliveryOptions, MessageDirection, MessageId, MessageKind,
    MessageMetadataAttribute, MessagePage, MessageSecurityMode, MessageTarget, PageLimit, PeerRef,
    SendMessageRequest, SendMessageResult, ThreadRef, UploadedAttachment,
};
use serde_json::{json, Value};

use crate::cli_output::ExitError;
use crate::cli_parser::ParsedCommand;
use crate::m_core_cli_adapter::message_result::{CommandResult, MessageAdapterError, ServiceError};
use crate::workspace_config::Resolved;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct TargetResolution {
    did: String,
    handle: String,
}

pub fn send_message_request(
    command: &ParsedCommand,
    default_domain: &str,
) -> Result<(SendMessageRequest, Vec<String>), ExitError> {
    let target = message_target(command, default_domain)?;
    let body = message_body(command)?;
    let (security, warnings) = message_security(command, &target)?;
    let client_message_id = optional_message_id_flag(command, "client-message-id")?;
    let idempotency_key = optional_string_flag(command, "idempotency-key");
    Ok((
        SendMessageRequest {
            target,
            body,
            security,
            client_message_id,
            delivery: MessageDeliveryOptions {
                idempotency_key,
                wait_for_final_acceptance: false,
            },
            delegated_signing: None,
        },
        warnings,
    ))
}

pub fn send_attachment_request(
    command: &ParsedCommand,
    default_domain: &str,
) -> Result<
    (
        MessageTarget,
        AttachmentSendRequest,
        Option<MessageId>,
        Vec<String>,
    ),
    ExitError,
> {
    let target = message_target(command, default_domain)?;
    let (security, warnings) = message_security(command, &target)?;
    let client_message_id = optional_message_id_flag(command, "client-message-id")?;
    let idempotency_key = optional_string_flag(command, "idempotency-key");
    let file_path = string_flag(command, "file");
    if file_path.trim().is_empty() {
        return Err(ExitError::new(
            "invalid_argument",
            2,
            "attachment file path is required",
            "Use --file <path> for attachment messages.",
        ));
    }
    let file_path = clean_input_file_path(&file_path);
    if !command.globals.dry_run {
        validate_attachment_input_path(&file_path)?;
    }
    let text = message_text(command, true)?;
    Ok((
        target,
        AttachmentSendRequest {
            input: AttachmentInput::LocalFile(file_path),
            caption: Some(text).filter(|value| !value.trim().is_empty()),
            mention_payload: None,
            mime_type: Some(string_flag(command, "mime-type"))
                .filter(|value| !value.trim().is_empty()),
            filename: None,
            delivery: MessageDeliveryOptions {
                idempotency_key,
                wait_for_final_acceptance: false,
            },
            security,
        },
        client_message_id,
        warnings,
    ))
}

pub fn download_attachment_request(
    command: &ParsedCommand,
    default_domain: &str,
) -> Result<DownloadAttachmentRequest, ExitError> {
    let with = string_flag(command, "with");
    let group = string_flag(command, "group");
    let thread = match (with.trim().is_empty(), group.trim().is_empty()) {
        (false, true) => ThreadRef::Direct(parse_peer(&with, default_domain)?),
        (true, false) => ThreadRef::Group(parse_group(&group)?),
        (true, true) => {
            return Err(ExitError::new(
                "invalid_argument",
                2,
                "attachment download requires either --with or --group",
                "Use --with <handle|did> for direct messages or --group <group_did>.",
            ));
        }
        (false, false) => {
            return Err(ExitError::new(
                "invalid_argument",
                2,
                "attachment download accepts either --with or --group, but not both",
                "Choose direct attachment download with --with or group download with --group.",
            ));
        }
    };
    let message_id = string_flag(command, "message-id");
    if message_id.trim().is_empty() {
        return Err(ExitError::new(
            "invalid_argument",
            2,
            "attachment message id is required",
            "Pass --message-id <id>.",
        ));
    }
    let output = string_flag(command, "output");
    if output.trim().is_empty() {
        return Err(ExitError::new(
            "invalid_argument",
            2,
            "attachment output path is required",
            "Pass --output <path>.",
        ));
    }
    Ok(DownloadAttachmentRequest {
        thread,
        message_id: MessageId::parse(message_id.trim()).map_err(im_error_to_exit_error)?,
        attachment_id: Some(string_flag(command, "attachment-id"))
            .filter(|value| !value.trim().is_empty()),
        destination: AttachmentDestination::LocalFile(clean_output_path(&output)),
        overwrite: true,
    })
}

pub fn inbox_query(command: &ParsedCommand) -> Result<InboxQuery, ExitError> {
    Ok(InboxQuery {
        scope: inbox_scope(&string_flag(command, "scope"))?,
        limit: page_limit(command, "limit", 20)?,
        cursor: optional_cursor(command)?,
        unread_only: bool_flag(command, "unread"),
        inbox_history_options: None,
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
                "internal_error",
                1,
                "required flag(s) \"with\" not set",
                "",
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
            inbox_history_options: None,
        },
    ))
}

pub fn send_message_via_im_core(
    _resolved: &Resolved,
    client: &im_core::ImClient,
    request: SendMessageRequest,
) -> Result<CommandResult, MessageAdapterError> {
    require_messaging_ready(client)?;
    let mut rpc_phase = crate::cli_trace::rpc_phase(sdk_send_trace_operation(&request));
    let secure = matches!(
        request.security,
        MessageSecurityMode::E2eeRequired
            | MessageSecurityMode::SecureDirect
            | MessageSecurityMode::GroupE2ee
    );
    let result = client.messages().send(request).map_err(|err| {
        rpc_phase.finish();
        im_error_to_message_error(err)
    })?;
    rpc_phase.finish();
    match &result.message.thread {
        ThreadRef::Direct(_) | ThreadRef::Thread(_) => {
            let target = direct_target_from_result(&result);
            render_send_result(&target, &result, secure)
        }
        ThreadRef::Group(group) => render_group_send_result(group.as_str(), &result, secure),
    }
}

pub async fn send_message_via_im_core_async(
    _resolved: &Resolved,
    client: &im_core::ImClient,
    request: SendMessageRequest,
) -> Result<CommandResult, MessageAdapterError> {
    require_messaging_ready(client)?;
    let mut rpc_phase = crate::cli_trace::rpc_phase(sdk_send_trace_operation(&request));
    let secure = matches!(
        request.security,
        MessageSecurityMode::E2eeRequired
            | MessageSecurityMode::SecureDirect
            | MessageSecurityMode::GroupE2ee
    );
    let result = client.messages().send_async(request).await.map_err(|err| {
        rpc_phase.finish();
        im_error_to_message_error(err)
    })?;
    rpc_phase.finish();
    match &result.message.thread {
        ThreadRef::Direct(_) | ThreadRef::Thread(_) => {
            let target = direct_target_from_result(&result);
            render_send_result(&target, &result, secure)
        }
        ThreadRef::Group(group) => render_group_send_result(group.as_str(), &result, secure),
    }
}

pub fn send_attachment_via_im_core(
    resolved: &Resolved,
    client: &im_core::ImClient,
    target: MessageTarget,
    request: AttachmentSendRequest,
    client_message_id: Option<MessageId>,
) -> Result<CommandResult, MessageAdapterError> {
    require_messaging_ready(client)?;
    let result = match client_message_id {
        Some(client_message_id) => {
            client
                .attachments()
                .send_with_client_message_id(target, request, client_message_id)
        }
        None => client.attachments().send(target, request),
    }
    .map_err(im_error_to_message_error)?;
    match &result.message.message.thread {
        ThreadRef::Direct(_) | ThreadRef::Thread(_) => {
            let target = direct_target_from_attachment_result(&result);
            render_direct_attachment_result(resolved, &target, &result)
        }
        ThreadRef::Group(group) => {
            render_group_attachment_result(resolved, group.as_str(), &result)
        }
    }
}

pub async fn send_attachment_via_im_core_async(
    resolved: &Resolved,
    client: &im_core::ImClient,
    target: MessageTarget,
    request: AttachmentSendRequest,
    client_message_id: Option<MessageId>,
) -> Result<CommandResult, MessageAdapterError> {
    require_messaging_ready(client)?;
    let result = match client_message_id {
        Some(client_message_id) => {
            client
                .attachments()
                .send_with_client_message_id_async(target, request, client_message_id)
                .await
        }
        None => client.attachments().send_async(target, request).await,
    }
    .map_err(im_error_to_message_error)?;
    match &result.message.message.thread {
        ThreadRef::Direct(_) | ThreadRef::Thread(_) => {
            let target = direct_target_from_attachment_result(&result);
            render_direct_attachment_result(resolved, &target, &result)
        }
        ThreadRef::Group(group) => {
            render_group_attachment_result(resolved, group.as_str(), &result)
        }
    }
}

pub fn download_attachment_via_im_core(
    resolved: &Resolved,
    client: &im_core::ImClient,
    request: DownloadAttachmentRequest,
) -> Result<CommandResult, MessageAdapterError> {
    require_messaging_ready(client)?;
    prepare_download_destination(&request)?;
    let thread = request.thread.clone();
    let downloaded = client
        .attachments()
        .download(request)
        .map_err(im_error_to_message_error)?;
    let output_path = match &downloaded.destination {
        DownloadedAttachmentDestination::LocalFile(path) => path.clone(),
        DownloadedAttachmentDestination::Memory(_) => {
            return Err(MessageAdapterError::Internal(
                "attachment download expected local-file destination".to_string(),
            ));
        }
    };
    apply_private_download_permissions(&output_path)?;
    let output_path_string = output_path.to_string_lossy().into_owned();
    let mut warnings = attachment_transport_warnings(resolved, true);
    warnings.extend(downloaded.warnings);
    let selection = downloaded
        .selection
        .ok_or(MessageAdapterError::AttachmentMessageInvalid)?;
    let target = download_target_value(&thread, Some(&selection));
    Ok(CommandResult {
        data: json!({
            "action": "download_attachment",
            "message_id": selection.message_id,
            "target": target,
            "attachment": attachment_selection_value(&selection),
            "output": {
                "path": output_path_string,
                "size_bytes": downloaded.size_bytes.unwrap_or_default(),
                "content_type": downloaded.mime_type.unwrap_or_default(),
            },
        }),
        summary: format!("Downloaded attachment to {output_path_string}"),
        warnings: compact_warnings(warnings),
    })
}

pub async fn download_attachment_via_im_core_async(
    resolved: &Resolved,
    client: &im_core::ImClient,
    request: DownloadAttachmentRequest,
) -> Result<CommandResult, MessageAdapterError> {
    require_messaging_ready(client)?;
    prepare_download_destination(&request)?;
    let thread = request.thread.clone();
    let downloaded = client
        .attachments()
        .download_async(request)
        .await
        .map_err(im_error_to_message_error)?;
    let output_path = match &downloaded.destination {
        DownloadedAttachmentDestination::LocalFile(path) => path.clone(),
        DownloadedAttachmentDestination::Memory(_) => {
            return Err(MessageAdapterError::Internal(
                "attachment download expected local-file destination".to_string(),
            ));
        }
    };
    apply_private_download_permissions(&output_path)?;
    let output_path_string = output_path.to_string_lossy().into_owned();
    let mut warnings = attachment_transport_warnings(resolved, true);
    warnings.extend(downloaded.warnings);
    let selection = downloaded
        .selection
        .ok_or(MessageAdapterError::AttachmentMessageInvalid)?;
    let target = download_target_value(&thread, Some(&selection));
    Ok(CommandResult {
        data: json!({
            "action": "download_attachment",
            "message_id": selection.message_id,
            "target": target,
            "attachment": attachment_selection_value(&selection),
            "output": {
                "path": output_path_string,
                "size_bytes": downloaded.size_bytes.unwrap_or_default(),
                "content_type": downloaded.mime_type.unwrap_or_default(),
            },
        }),
        summary: format!("Downloaded attachment to {output_path_string}"),
        warnings: compact_warnings(warnings),
    })
}

fn require_messaging_ready(client: &im_core::ImClient) -> Result<(), MessageAdapterError> {
    let identity = client.current_identity();
    if identity.readiness.ready_for_messaging {
        return Ok(());
    }
    let identity_name = identity
        .local_alias
        .as_deref()
        .unwrap_or_else(|| identity.id.as_str());
    Err(MessageAdapterError::IdentityRequired(format!(
        "identity {identity_name} requires user registration before messaging"
    )))
}

fn prepare_download_destination(
    request: &DownloadAttachmentRequest,
) -> Result<(), MessageAdapterError> {
    let AttachmentDestination::LocalFile(path) = &request.destination else {
        return Ok(());
    };
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent).map_err(|err| {
            MessageAdapterError::PathUnavailable(format!(
                "create attachment output directory {}: {err}",
                parent.display()
            ))
        })?;
        set_private_dir_mode(parent)?;
    }
    Ok(())
}

fn download_target_value(thread: &ThreadRef, selection: Option<&AttachmentSelection>) -> Value {
    match thread {
        ThreadRef::Direct(peer) => json!({
            "kind": "direct",
            "did": selection
                .map(|selection| selection.sender_did.as_str())
                .unwrap_or_else(|| peer.as_str()),
        }),
        ThreadRef::Group(group) => json!({
            "kind": "group",
            "did": group.as_str(),
        }),
        ThreadRef::Thread(thread) => json!({
            "kind": "thread",
            "did": thread.as_str(),
        }),
    }
}

pub fn read_inbox_via_im_core(
    _resolved: &Resolved,
    client: &im_core::ImClient,
    query: InboxQuery,
) -> Result<CommandResult, MessageAdapterError> {
    require_messaging_ready(client)?;
    let mut rpc_phase = crate::cli_trace::rpc_phase("inbox.get");
    let page = client
        .messages()
        .inbox_with_metadata(query.clone())
        .map_err(|err| {
            rpc_phase.finish();
            im_error_to_message_error(err)
        })?;
    rpc_phase.finish();
    let raw = message_page_to_cli_raw(&page);
    let mut messages = messages_from_raw(&raw);
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
        warnings: Vec::new(),
    })
}

pub async fn read_inbox_via_im_core_async(
    _resolved: &Resolved,
    client: &im_core::ImClient,
    query: InboxQuery,
) -> Result<CommandResult, MessageAdapterError> {
    require_messaging_ready(client)?;
    let mut rpc_phase = crate::cli_trace::rpc_phase("sync.v2.foreground_reconcile");
    let reconciled = async {
        let secure_warnings = if matches!(&query.scope, InboxScope::DirectOnly | InboxScope::All) {
            client
                .messages()
                .hydrate_exact_device_secure_inbox_async(query.limit)
                .await
                .map_err(im_error_to_message_error)?
        } else {
            Vec::new()
        };
        reconcile_foreground_message_sync_async(client).await?;
        let page = client
            .messages()
            .local_inbox_projection_with_metadata_async(query.clone())
            .await
            .map_err(im_error_to_message_error)?;
        Ok::<_, MessageAdapterError>((page, secure_warnings))
    }
    .await;
    rpc_phase.finish();
    let (page, secure_warnings) = reconciled?;
    let raw = message_page_to_cli_raw(&page);
    let mut messages = messages_from_raw(&raw);
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
        warnings: compact_warnings(secure_warnings),
    })
}

fn foreground_message_sync_reason() -> &'static str {
    "foreground_reconcile"
}

fn require_foreground_message_sync(
    outcome: &MessageSyncOutcome,
) -> Result<(), MessageAdapterError> {
    match outcome.status {
        MessageSyncStatus::Idle | MessageSyncStatus::Changed => Ok(()),
        MessageSyncStatus::RecoveryRequired => Err(MessageAdapterError::LocalStateUnavailable(
            "foreground message recovery did not complete".to_owned(),
        )),
        MessageSyncStatus::RetryableFailure => Err(MessageAdapterError::TransportUnavailable(
            "foreground message reconciliation did not complete".to_owned(),
        )),
        MessageSyncStatus::AuthRevoked => Err(MessageAdapterError::IdentityRequired(
            "foreground message synchronization authorization is unavailable".to_owned(),
        )),
    }
}

fn reconcile_foreground_message_sync(
    client: &im_core::ImClient,
) -> Result<(), MessageAdapterError> {
    let outcome = client
        .messages()
        .sync_now(MessageSyncRequest {
            reason: foreground_message_sync_reason().to_owned(),
            limit: Some(100),
        })
        .map_err(im_error_to_message_error)?;
    require_foreground_message_sync(&outcome)
}

async fn reconcile_foreground_message_sync_async(
    client: &im_core::ImClient,
) -> Result<(), MessageAdapterError> {
    let outcome = client
        .messages()
        .sync_now_async(MessageSyncRequest {
            reason: foreground_message_sync_reason().to_owned(),
            limit: Some(100),
        })
        .await
        .map_err(im_error_to_message_error)?;
    require_foreground_message_sync(&outcome)
}

fn local_history_query(query: HistoryQuery) -> LocalHistoryQuery {
    LocalHistoryQuery {
        limit: query.limit,
        cursor: query.cursor,
    }
}

pub fn read_history_via_im_core(
    resolved: &Resolved,
    client: &im_core::ImClient,
    thread: ThreadRef,
    query: HistoryQuery,
) -> Result<CommandResult, MessageAdapterError> {
    match thread {
        ThreadRef::Direct(peer) => read_direct_history_via_im_core(resolved, client, peer, query),
        ThreadRef::Group(group) => read_group_history_via_im_core(resolved, client, group, query),
        ThreadRef::Thread(_) => Err(MessageAdapterError::Internal(
            "thread history is not supported by the CLI renderer".to_string(),
        )),
    }
}

pub async fn read_history_via_im_core_async(
    resolved: &Resolved,
    client: &im_core::ImClient,
    thread: ThreadRef,
    query: HistoryQuery,
) -> Result<CommandResult, MessageAdapterError> {
    match thread {
        ThreadRef::Direct(peer) => {
            read_direct_history_via_im_core_async(resolved, client, peer, query).await
        }
        ThreadRef::Group(group) => {
            read_group_history_via_im_core_async(resolved, client, group, query).await
        }
        ThreadRef::Thread(_) => Err(MessageAdapterError::Internal(
            "thread history is not supported by the CLI renderer".to_string(),
        )),
    }
}

fn read_direct_history_via_im_core(
    _resolved: &Resolved,
    client: &im_core::ImClient,
    peer: PeerRef,
    query: HistoryQuery,
) -> Result<CommandResult, MessageAdapterError> {
    require_messaging_ready(client)?;
    reconcile_foreground_message_sync(client)?;
    let target_is_handle = !peer.as_str().trim().starts_with("did:");
    let page = client
        .messages()
        .local_history_with_metadata(ThreadRef::Direct(peer.clone()), local_history_query(query))
        .map_err(im_error_to_message_error)?;
    let raw = message_page_to_cli_raw(&page);
    let messages = messages_from_raw(&raw);
    let source = source_with_default(&raw);
    let resolved_dids = resolved_dids_value(&raw);
    let target = history_target_from_page(&peer, &resolved_dids, target_is_handle);
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
        warnings: Vec::new(),
    })
}

async fn read_direct_history_via_im_core_async(
    _resolved: &Resolved,
    client: &im_core::ImClient,
    peer: PeerRef,
    query: HistoryQuery,
) -> Result<CommandResult, MessageAdapterError> {
    require_messaging_ready(client)?;
    reconcile_foreground_message_sync_async(client).await?;
    let target_is_handle = !peer.as_str().trim().starts_with("did:");
    let page = client
        .messages()
        .local_history_with_metadata_async(
            ThreadRef::Direct(peer.clone()),
            local_history_query(query),
        )
        .await
        .map_err(im_error_to_message_error)?;
    let raw = message_page_to_cli_raw(&page);
    let messages = messages_from_raw(&raw);
    let source = source_with_default(&raw);
    let resolved_dids = resolved_dids_value(&raw);
    let target = history_target_from_page(&peer, &resolved_dids, target_is_handle);
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
        warnings: Vec::new(),
    })
}

fn read_group_history_via_im_core(
    _resolved: &Resolved,
    client: &im_core::ImClient,
    group: GroupRef,
    query: HistoryQuery,
) -> Result<CommandResult, MessageAdapterError> {
    require_messaging_ready(client)?;
    reconcile_foreground_message_sync(client)?;
    let page = client
        .messages()
        .local_history_with_metadata(ThreadRef::Group(group.clone()), local_history_query(query))
        .map_err(im_error_to_message_error)?;
    let raw = message_page_to_cli_raw(&page);
    let messages = messages_from_raw(&raw);
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
        warnings: Vec::new(),
    })
}

async fn read_group_history_via_im_core_async(
    _resolved: &Resolved,
    client: &im_core::ImClient,
    group: GroupRef,
    query: HistoryQuery,
) -> Result<CommandResult, MessageAdapterError> {
    require_messaging_ready(client)?;
    reconcile_foreground_message_sync_async(client).await?;
    let page = client
        .messages()
        .local_history_with_metadata_async(
            ThreadRef::Group(group.clone()),
            local_history_query(query),
        )
        .await
        .map_err(im_error_to_message_error)?;
    let raw = message_page_to_cli_raw(&page);
    let messages = messages_from_raw(&raw);
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
        warnings: Vec::new(),
    })
}

pub fn mark_read_via_im_core(
    _resolved: &Resolved,
    client: &im_core::ImClient,
    message_ids: Vec<String>,
) -> Result<CommandResult, MessageAdapterError> {
    require_messaging_ready(client)?;
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

pub async fn mark_read_via_im_core_async(
    _resolved: &Resolved,
    client: &im_core::ImClient,
    message_ids: Vec<String>,
) -> Result<CommandResult, MessageAdapterError> {
    require_messaging_ready(client)?;
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
        .mark_read_async(ids)
        .await
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

pub fn direct_secure_status_via_im_core(
    client: &im_core::ImClient,
    peer: String,
    default_domain: &str,
) -> Result<CommandResult, MessageAdapterError> {
    require_messaging_ready(client)?;
    let peer_ref = PeerRef::parse(&peer, default_domain).map_err(im_error_to_message_error)?;
    let status = client
        .secure()
        .direct(peer_ref)
        .status()
        .map_err(im_error_to_message_error)?;
    let warnings = status.warnings.clone();
    Ok(CommandResult {
        data: json!({
            "status": serde_json::to_value(&status).unwrap_or(Value::Null),
        }),
        summary: "Loaded direct secure status".to_string(),
        warnings: compact_warnings(warnings),
    })
}

pub async fn direct_secure_status_via_im_core_async(
    client: &im_core::ImClient,
    peer: String,
    default_domain: &str,
) -> Result<CommandResult, MessageAdapterError> {
    require_messaging_ready(client)?;
    let peer_ref = PeerRef::parse(&peer, default_domain).map_err(im_error_to_message_error)?;
    let status = client
        .secure()
        .direct(peer_ref)
        .status_async()
        .await
        .map_err(im_error_to_message_error)?;
    let warnings = status.warnings.clone();
    Ok(CommandResult {
        data: json!({
            "status": serde_json::to_value(&status).unwrap_or(Value::Null),
        }),
        summary: "Loaded direct secure status".to_string(),
        warnings: compact_warnings(warnings),
    })
}

pub fn direct_secure_repair_via_im_core(
    client: &im_core::ImClient,
    peer: String,
    default_domain: &str,
) -> Result<CommandResult, MessageAdapterError> {
    require_messaging_ready(client)?;
    let peer_ref = PeerRef::parse(&peer, default_domain).map_err(im_error_to_message_error)?;
    let repair = client
        .secure()
        .direct(peer_ref)
        .repair()
        .map_err(im_error_to_message_error)?;
    let warnings = repair.warnings.clone();
    Ok(CommandResult {
        data: json!({
            "repair": serde_json::to_value(&repair).unwrap_or(Value::Null),
        }),
        summary: "Repaired direct secure state".to_string(),
        warnings: compact_warnings(warnings),
    })
}

pub async fn direct_secure_repair_via_im_core_async(
    client: &im_core::ImClient,
    peer: String,
    default_domain: &str,
) -> Result<CommandResult, MessageAdapterError> {
    require_messaging_ready(client)?;
    let peer_ref = PeerRef::parse(&peer, default_domain).map_err(im_error_to_message_error)?;
    let repair = client
        .secure()
        .direct(peer_ref)
        .repair_async()
        .await
        .map_err(im_error_to_message_error)?;
    let warnings = repair.warnings.clone();
    Ok(CommandResult {
        data: json!({
            "repair": serde_json::to_value(&repair).unwrap_or(Value::Null),
        }),
        summary: "Repaired direct secure state".to_string(),
        warnings: compact_warnings(warnings),
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
    if has_payload_input(command) {
        reject_payload_conflicts(command)?;
        return Ok(MessageBody::Payload {
            payload: message_payload(command)?,
        });
    }
    let file_path = string_flag(command, "file");
    if !file_path.trim().is_empty() {
        let text = message_text(command, true)?;
        return Ok(MessageBody::Attachment {
            input: AttachmentInput::LocalFile(PathBuf::from(file_path.trim())),
            caption: Some(text).filter(|value| !value.trim().is_empty()),
            mention_payload: None,
            mime_type: Some(string_flag(command, "mime-type"))
                .filter(|value| !value.trim().is_empty()),
            filename: None,
        });
    }
    let text = message_text(command, false)?;
    Ok(MessageBody::Text {
        text,
        kind: message_kind(&string_flag(command, "type"))?,
    })
}

fn has_payload_input(command: &ParsedCommand) -> bool {
    !string_flag(command, "payload").trim().is_empty()
        || !string_flag(command, "payload-file").trim().is_empty()
}

fn reject_payload_conflicts(command: &ParsedCommand) -> Result<(), ExitError> {
    for flag in ["text", "text-file", "file"] {
        if !string_flag(command, flag).trim().is_empty() {
            return Err(ExitError::new(
                "invalid_argument",
                2,
                "--payload/--payload-file cannot be combined with text or attachment inputs.",
                "Use payload input by itself, or use --text/--text-file/--file for non-payload messages.",
            ));
        }
    }
    if !string_flag(command, "mime-type").trim().is_empty() {
        return Err(ExitError::new(
            "invalid_argument",
            2,
            "--mime-type requires an attachment file",
            "Use --mime-type only together with --file.",
        ));
    }
    let message_type = string_flag(command, "type");
    let message_type = message_type.trim();
    if !message_type.is_empty() && !message_type.eq_ignore_ascii_case("text") {
        return Err(ExitError::new(
            "invalid_argument",
            2,
            "--type cannot be used with payload messages.",
            "Payload messages always use application/json.",
        ));
    }
    Ok(())
}

fn message_payload(command: &ParsedCommand) -> Result<Value, ExitError> {
    let inline = string_flag(command, "payload");
    let payload_file = string_flag(command, "payload-file");
    if !inline.trim().is_empty() && !payload_file.trim().is_empty() {
        return Err(ExitError::new(
            "invalid_argument",
            2,
            "Use either --payload or --payload-file, not both.",
            "Choose one JSON payload source.",
        ));
    }
    let raw = if !payload_file.trim().is_empty() {
        fs::read_to_string(payload_file.trim()).map_err(|err| {
            ExitError::new(
                "invalid_argument",
                2,
                format!("read payload file {:?}: {err}", payload_file.trim()),
                "Check the --payload-file path and permissions.",
            )
        })?
    } else {
        inline
    };
    let payload = serde_json::from_str::<Value>(&raw).map_err(|err| {
        ExitError::new(
            "invalid_argument",
            2,
            format!("payload must be valid JSON: {err}"),
            "Pass a JSON object with --payload or --payload-file.",
        )
    })?;
    if !payload.is_object() {
        return Err(ExitError::new(
            "invalid_argument",
            2,
            "payload must be a JSON object.",
            "Pass an object such as '{\"text\":\"hello\"}'.",
        ));
    }
    Ok(payload)
}

fn message_text(command: &ParsedCommand, allow_empty: bool) -> Result<String, ExitError> {
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
    if !allow_empty && text.trim().is_empty() {
        return Err(ExitError::new(
            "invalid_argument",
            2,
            "msg send requires --text or --text-file.",
            "Provide a text body for Phase 1 IM Core messages.",
        ));
    }
    Ok(text)
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
) -> Result<(MessageSecurityMode, Vec<String>), ExitError> {
    let raw = string_flag(command, "secure");
    let normalized = raw.trim().to_ascii_lowercase();
    let warnings = secure_alias_warnings(normalized.as_str());
    match normalized.as_str() {
        "" | "default" => Ok((MessageSecurityMode::DefaultPlain, Vec::new())),
        "plain" | "off" | "false" => Ok((MessageSecurityMode::Plain, warnings)),
        "required" | "on" | "true" | "e2ee" => Ok((MessageSecurityMode::E2eeRequired, warnings)),
        "direct" | "secure-direct" => match target {
            MessageTarget::Direct(_) => Ok((MessageSecurityMode::E2eeRequired, warnings)),
            MessageTarget::Group(_) => Err(ExitError::new(
                "invalid_argument",
                2,
                "--secure secure-direct can only be used with --to.",
                "Use --secure required for group E2EE text messages.",
            )),
        },
        "group-e2ee" => match target {
            MessageTarget::Group(_) => Ok((MessageSecurityMode::E2eeRequired, warnings)),
            MessageTarget::Direct(_) => Err(ExitError::new(
                "invalid_argument",
                2,
                "--secure group-e2ee can only be used with --group.",
                "Use --secure required for direct E2EE text messages.",
            )),
        },
        value => Err(ExitError::new(
            "invalid_argument",
            2,
            format!("unsupported --secure value {value:?}."),
            "Use --secure required, --secure off, or leave it unset.",
        )),
    }
}

fn secure_alias_warnings(value: &str) -> Vec<String> {
    match value {
        "on" | "true" | "e2ee" | "direct" | "secure-direct" | "group-e2ee" => vec![format!(
            "--secure {value} is deprecated; use --secure required."
        )],
        "plain" | "false" => vec![format!("--secure {value} is deprecated; use --secure off.")],
        _ => Vec::new(),
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

fn message_page_to_cli_raw(page: &MessagePage) -> Value {
    json!({
        "messages": page.items.iter().map(message_to_cli_json).collect::<Vec<_>>(),
        "total": page.items.len(),
        "source": page.source.as_deref().unwrap_or(source_default()),
        "next_cursor": page.next_cursor.as_ref().map(|cursor| cursor.as_str().to_string()),
        "has_more": page.has_more,
        "resolved_dids": page.resolved_dids.iter().map(|did| did.as_str()).collect::<Vec<_>>(),
        "warnings": page.warnings,
    })
}

fn messages_from_raw(raw: &Value) -> Vec<Value> {
    raw.get("messages")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

fn message_to_cli_json(message: &im_core::prelude::Message) -> Value {
    let mut value = json!({
        "id": message.id.as_str(),
        "msg_id": message.id.as_str(),
        "message_id": message.id.as_str(),
        "sender_did": message.sender.as_str(),
        "receiver_did": message.receiver.as_ref().map(|peer| peer.as_str()).unwrap_or_default(),
        "group_did": message.group.as_ref().map(|group| group.as_str()).unwrap_or_default(),
        "content": message_body_content(message),
        "content_type": message_content_type(message),
        "sent_at": message.sent_at.clone().unwrap_or_default(),
        "received_at": message.received_at.clone().unwrap_or_default(),
        "is_read": message_is_read(message),
        "secure": message_is_secure(message),
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
    if message_content_type(message) == im_core::attachments::attachment_manifest_content_type() {
        value["type"] = json!("attachment_manifest");
    }
    for attribute in &message.metadata.attributes {
        if matches!(attribute.key.as_str(), "raw_content" | "is_read") {
            continue;
        }
        if !attribute.key.trim().is_empty() {
            value[attribute.key.as_str()] = json!(attribute.value);
        }
    }
    value
}

fn message_body_content(message: &im_core::prelude::Message) -> Value {
    match &message.body {
        MessageBodyView::Text { text, .. } => Value::String(text.clone()),
        MessageBodyView::Payload { payload } => payload.clone(),
        MessageBodyView::Unsupported { .. } => {
            raw_content_value(&message.metadata.attributes).unwrap_or(Value::Null)
        }
    }
}

fn message_content_type(message: &im_core::prelude::Message) -> String {
    if let Some(content_type) = message
        .metadata
        .content_type
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
    {
        return content_type.to_string();
    }
    match &message.body {
        MessageBodyView::Text {
            kind: MessageKind::Markdown,
            ..
        } => "text/markdown".to_string(),
        MessageBodyView::Text { .. } => "text/plain".to_string(),
        MessageBodyView::Payload { .. } => "application/json".to_string(),
        MessageBodyView::Unsupported { content_type } => content_type
            .as_ref()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
            .unwrap_or("application/octet-stream")
            .to_string(),
    }
}

fn message_is_secure(message: &im_core::prelude::Message) -> bool {
    message_attribute(&message.metadata.attributes, "security")
        .is_some_and(|value| matches!(value.as_str(), "direct-e2ee" | "group-e2ee"))
}

fn message_is_read(message: &im_core::prelude::Message) -> bool {
    message_attribute(&message.metadata.attributes, "is_read").is_some_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "y" | "on"
        )
    })
}

fn sdk_send_trace_operation(request: &SendMessageRequest) -> &'static str {
    match &request.target {
        MessageTarget::Direct(_) => "direct send",
        MessageTarget::Group(_) => "group send",
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
    let raw_peer = result
        .message
        .receiver
        .as_ref()
        .or_else(|| direct_thread_peer(result))
        .map(|peer| peer.as_str())
        .unwrap_or_default();
    let target_handle = message_attribute(&result.message.metadata.attributes, "target_handle")
        .or_else(|| message_attribute(&result.message.metadata.attributes, "peer_full_handle"));
    let did = message_attribute(&result.message.metadata.attributes, "resolved_target_did")
        .unwrap_or_else(|| {
            if raw_peer.starts_with("did:") {
                raw_peer.to_string()
            } else {
                String::new()
            }
        });
    TargetResolution {
        did,
        handle: target_handle.unwrap_or_else(|| {
            if raw_peer.starts_with("did:") {
                String::new()
            } else {
                raw_peer.to_string()
            }
        }),
    }
}

fn direct_target_from_attachment_result(result: &AttachmentSendResult) -> TargetResolution {
    let mut target = direct_target_from_result(&result.message);
    if target.did.trim().is_empty() {
        target.did = result.target_did.clone();
    }
    target
}

fn direct_thread_peer(result: &SendMessageResult) -> Option<&PeerRef> {
    match &result.message.thread {
        ThreadRef::Direct(peer) => Some(peer),
        _ => None,
    }
}

fn peer_handle_or_did(target: &TargetResolution) -> String {
    if target.handle.trim().is_empty() {
        target.did.clone()
    } else {
        target.handle.clone()
    }
}

fn history_target_from_page(
    peer: &PeerRef,
    resolved_dids: &Value,
    target_is_handle: bool,
) -> TargetResolution {
    let did = resolved_dids
        .as_array()
        .and_then(|items| items.first())
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| peer.as_str())
        .to_string();
    TargetResolution {
        did,
        handle: if target_is_handle {
            peer.as_str().to_string()
        } else {
            String::new()
        },
    }
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

fn string_value(value: Option<&Value>) -> String {
    value
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
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

fn raw_content_value(attributes: &[MessageMetadataAttribute]) -> Option<Value> {
    let raw = message_attribute(attributes, "raw_content")?;
    serde_json::from_str::<Value>(&raw)
        .ok()
        .or(Some(Value::String(raw)))
}

pub(super) fn send_result_message_type(
    body: &MessageBodyView,
) -> Result<&'static str, MessageAdapterError> {
    match body {
        MessageBodyView::Text { kind, .. } => Ok(message_type_for_kind(kind)),
        MessageBodyView::Payload { .. } => Ok("application/json"),
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

fn render_direct_attachment_result(
    resolved: &Resolved,
    target: &TargetResolution,
    attachment_result: &AttachmentSendResult,
) -> Result<CommandResult, MessageAdapterError> {
    let result = DirectSendResult::from_sdk_result(&attachment_result.message, target);
    let manifest = &attachment_result.manifest;
    let caption = manifest
        .get("caption")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let mut warnings = attachment_transport_warnings(resolved, false);
    warnings.extend(attachment_result.message.warnings.clone());
    Ok(CommandResult {
        data: json!({
            "action": "send_attachment",
            "target": {
                "did": target.did,
                "handle": target.handle,
                "kind": "direct",
            },
            "message": attachment_message_value(
                &result.message_id,
                &result.accepted_at,
                &caption,
                attachment_result_is_secure(attachment_result),
            ),
            "attachment": uploaded_attachment_value(&attachment_result.attachment),
            "delivery": result,
        }),
        summary: "Sent a direct attachment message".to_string(),
        warnings: compact_warnings(warnings),
    })
}

fn render_group_attachment_result(
    resolved: &Resolved,
    group_did: &str,
    attachment_result: &AttachmentSendResult,
) -> Result<CommandResult, MessageAdapterError> {
    let result = GroupSendResult::from_sdk_result(&attachment_result.message, group_did);
    let manifest = &attachment_result.manifest;
    let caption = manifest
        .get("caption")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let mut warnings = attachment_transport_warnings(resolved, false);
    warnings.extend(attachment_result.message.warnings.clone());
    let message_id = group_send_message_id(group_did, &result);
    Ok(CommandResult {
        data: json!({
            "action": "send_attachment",
            "target": {
                "kind": "group",
                "did": group_did,
            },
            "message": attachment_message_value(
                &message_id,
                &result.accepted_at,
                &caption,
                attachment_result_is_secure(attachment_result),
            ),
            "attachment": uploaded_attachment_value(&attachment_result.attachment),
            "delivery": result,
        }),
        summary: "Sent a group attachment message".to_string(),
        warnings: compact_warnings(warnings),
    })
}

fn attachment_message_value(message_id: &str, sent_at: &str, caption: &str, secure: bool) -> Value {
    json!({
        "id": message_id,
        "type": "attachment_manifest",
        "content_type": im_core::attachments::attachment_manifest_content_type(),
        "caption": caption,
        "secure": secure,
        "sent_at": sent_at,
    })
}

fn uploaded_attachment_value(attachment: &UploadedAttachment) -> Value {
    json!({
        "attachment_id": attachment.attachment_id,
        "filename": attachment.filename,
        "mime_type": attachment.mime_type,
        "size": attachment.size,
        "digest": {
            "alg": "sha-256",
            "value_b64u": attachment.digest_b64u,
        },
        "object_uri": attachment.object_uri,
        "object_encryption_mode": attachment.object_encryption_mode,
        "plaintext_size_bytes": attachment.plaintext_size_bytes,
    })
}

fn attachment_selection_value(selection: &AttachmentSelection) -> Value {
    json!({
        "attachment_id": selection.attachment_id,
        "filename": selection.filename,
        "mime_type": selection.mime_type,
        "size": selection.size,
        "digest": {
            "alg": "sha-256",
            "value_b64u": selection.digest_b64u,
        },
        "object_uri": selection.object_uri,
        "sender_did": selection.sender_did,
        "caption": selection.caption,
        "message_security_profile": selection.message_security_profile,
        "object_encryption_mode": selection.object_encryption_mode,
        "object_cipher": selection.object_cipher,
        "plaintext_size": selection.plaintext_size,
    })
}

fn attachment_result_is_secure(attachment_result: &AttachmentSendResult) -> bool {
    attachment_result
        .message
        .message
        .metadata
        .attributes
        .iter()
        .any(|attribute| {
            attribute.key == "security" && attribute.value.to_ascii_lowercase().contains("e2ee")
        })
}

fn clean_output_path(output_path: &str) -> PathBuf {
    PathBuf::from(output_path.trim())
}

fn clean_input_file_path(file_path: &str) -> PathBuf {
    PathBuf::from(file_path.trim())
}

fn validate_attachment_input_path(path: &Path) -> Result<(), ExitError> {
    if path.as_os_str().is_empty() {
        return Err(ExitError::new(
            "invalid_argument",
            2,
            "attachment file path is required",
            "Use --file <path> for attachment messages.",
        ));
    }
    let metadata = std::fs::metadata(path).map_err(|err| {
        ExitError::new(
            "invalid_argument",
            2,
            format!(
                "attachment file path is unavailable: {}: {err}",
                path.display()
            ),
            "Check the attachment file path and permissions.",
        )
    })?;
    if !metadata.is_file() {
        return Err(ExitError::new(
            "invalid_argument",
            2,
            format!(
                "attachment file path must be a regular file: {}",
                path.display()
            ),
            "Pass a readable file path with --file.",
        ));
    }
    std::fs::File::open(path).map_err(|err| {
        ExitError::new(
            "invalid_argument",
            2,
            format!("attachment file is not readable: {}: {err}", path.display()),
            "Check the attachment file path and permissions.",
        )
    })?;
    Ok(())
}

fn attachment_transport_warnings(resolved: &Resolved, download: bool) -> Vec<String> {
    attachment_transport_warnings_for_mode(&resolved.runtime_mode, download)
}

fn attachment_transport_warnings_for_mode(runtime_mode: &str, download: bool) -> Vec<String> {
    if runtime_mode.trim() != "websocket" {
        return Vec::new();
    }
    if download {
        vec![
            "Attachment downloads use HTTP transport even when runtime.mode is websocket."
                .to_string(),
        ]
    } else {
        vec![
            "Attachment messages use HTTP transport even when runtime.mode is websocket."
                .to_string(),
        ]
    }
}

fn apply_private_download_permissions(path: &Path) -> Result<(), MessageAdapterError> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        set_private_dir_mode(parent)?;
    }
    set_private_file_mode(path)
}

#[cfg(unix)]
fn set_private_dir_mode(path: &Path) -> Result<(), MessageAdapterError> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).map_err(|err| {
        MessageAdapterError::PathUnavailable(format!(
            "set attachment output directory permissions {}: {err}",
            path.display()
        ))
    })
}

#[cfg(not(unix))]
fn set_private_dir_mode(_path: &Path) -> Result<(), MessageAdapterError> {
    Ok(())
}

#[cfg(unix)]
fn set_private_file_mode(path: &Path) -> Result<(), MessageAdapterError> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).map_err(|err| {
        MessageAdapterError::PathUnavailable(format!(
            "set attachment output file permissions {}: {err}",
            path.display()
        ))
    })
}

#[cfg(not(unix))]
fn set_private_file_mode(_path: &Path) -> Result<(), MessageAdapterError> {
    Ok(())
}

fn message_attribute(attributes: &[MessageMetadataAttribute], key: &str) -> Option<String> {
    attributes
        .iter()
        .find(|attribute| attribute.key == key)
        .map(|attribute| attribute.value.clone())
        .filter(|value| !value.trim().is_empty())
}

fn message_bool_attribute(attributes: &[MessageMetadataAttribute], key: &str) -> Option<bool> {
    message_attribute(attributes, key).and_then(|value| match value.trim() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    })
}

fn message_u64_attribute(attributes: &[MessageMetadataAttribute], key: &str) -> Option<u64> {
    message_attribute(attributes, key).and_then(|value| value.trim().parse().ok())
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    attempted_device_count: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    previously_accepted_device_count: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    newly_accepted_device_count: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    accepted_device_count: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    failed_device_count: Option<u64>,
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
            final_acceptance: message_bool_attribute(
                &result.message.metadata.attributes,
                "final_acceptance",
            )
            .unwrap_or(matches!(result.delivery, DeliveryState::Sent)),
            delivery_state: delivery_state_label(result),
            attempted_device_count: message_u64_attribute(
                &result.message.metadata.attributes,
                "attempted_device_count",
            ),
            previously_accepted_device_count: message_u64_attribute(
                &result.message.metadata.attributes,
                "previously_accepted_device_count",
            ),
            newly_accepted_device_count: message_u64_attribute(
                &result.message.metadata.attributes,
                "newly_accepted_device_count",
            ),
            accepted_device_count: message_u64_attribute(
                &result.message.metadata.attributes,
                "accepted_device_count",
            ),
            failed_device_count: message_u64_attribute(
                &result.message.metadata.attributes,
                "failed_device_count",
            ),
        }
    }
}

impl GroupSendResult {
    fn from_sdk_result(result: &SendMessageResult, group_did: &str) -> Self {
        Self {
            accepted: delivery_was_accepted(&result.delivery),
            final_acceptance: message_bool_attribute(
                &result.message.metadata.attributes,
                "final_acceptance",
            )
            .unwrap_or(matches!(result.delivery, DeliveryState::Sent)),
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

fn render_send_result(
    target: &TargetResolution,
    sdk_result: &SendMessageResult,
    secure: bool,
) -> Result<CommandResult, MessageAdapterError> {
    let result = DirectSendResult::from_sdk_result(sdk_result, target);
    let message_type = send_result_message_type(&sdk_result.message.body)?;
    let warnings = sdk_result.warnings.clone();
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
        warnings: compact_warnings(warnings),
    })
}

fn render_group_send_result(
    group_did: &str,
    sdk_result: &SendMessageResult,
    secure: bool,
) -> Result<CommandResult, MessageAdapterError> {
    let result = GroupSendResult::from_sdk_result(sdk_result, group_did);
    let message_type = send_result_message_type(&sdk_result.message.body)?;
    let warnings = sdk_result.warnings.clone();
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
                "secure": secure,
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

fn im_error_to_message_error(err: im_core::ImError) -> MessageAdapterError {
    match err {
        im_core::ImError::InvalidInput { field, .. } if field.as_deref() == Some("text") => {
            MessageAdapterError::TextRequired
        }
        im_core::ImError::InvalidInput { field, .. } if field.as_deref() == Some("filename") => {
            MessageAdapterError::FilePathRequired
        }
        im_core::ImError::InvalidInput { field, message }
            if field.as_deref() == Some("service_did")
                && message == "message service did is required" =>
        {
            MessageAdapterError::MissingMessageServiceDid
        }
        im_core::ImError::InvalidInput { field, message }
            if field.as_deref() == Some("service_did")
                && message == "attachment service did is required" =>
        {
            MessageAdapterError::MissingAttachmentServiceDid
        }
        im_core::ImError::InvalidInput { field, .. } if field.as_deref() == Some("sender_did") => {
            MessageAdapterError::AttachmentSenderRequired
        }
        im_core::ImError::InvalidInput { field, message }
            if field.as_deref() == Some("destination")
                && message.contains("overwrite is false") =>
        {
            MessageAdapterError::PathUnavailable(message)
        }
        im_core::ImError::InvalidInput { field, message }
            if field.as_deref() == Some("destination") =>
        {
            MessageAdapterError::PathUnavailable(message)
        }
        im_core::ImError::InvalidInput { message, .. }
            if message == im_core::attachments::ERR_ATTACHMENT_NOT_FOUND =>
        {
            MessageAdapterError::AttachmentNotFound
        }
        im_core::ImError::InvalidInput { message, .. }
            if message == im_core::attachments::ERR_ATTACHMENT_ID_REQUIRED =>
        {
            MessageAdapterError::AttachmentIdRequired
        }
        im_core::ImError::InvalidInput { message, .. }
            if message == im_core::attachments::ERR_ATTACHMENT_MESSAGE_INVALID =>
        {
            MessageAdapterError::AttachmentMessageInvalid
        }
        im_core::ImError::PeerNotFound { peer } => {
            MessageAdapterError::IdentityRequired(format!("peer not found: {peer}"))
        }
        im_core::ImError::MessageNotFound { .. } => MessageAdapterError::MessageNotFound,
        im_core::ImError::UnsupportedCapability { capability } if capability == "group-send" => {
            MessageAdapterError::GroupNotSupported
        }
        im_core::ImError::UnsupportedCapability { capability } if capability == "group-e2ee" => {
            MessageAdapterError::GroupNotSupported
        }
        im_core::ImError::UnsupportedCapability { capability } if capability == "attachments" => {
            MessageAdapterError::AttachmentNotSupported
        }
        im_core::ImError::UnsupportedCapability { capability }
            if capability == "secure-attachments" =>
        {
            MessageAdapterError::SecureAttachmentNotSupported
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
            ..
        } => {
            let status_code = status_code.unwrap_or_default();
            let public_code = code
                .as_deref()
                .filter(|value| super::error::is_public_service_code(value))
                .map(str::to_owned);
            if !matches!(status_code, 401 | 403) {
                if let Some(public_code) = public_code {
                    return MessageAdapterError::PublicServiceCode(public_code);
                }
            }
            let rpc_code = code
                .as_deref()
                .and_then(|value| value.parse().ok())
                .unwrap_or_default();
            if group_e2ee_service_unsupported(rpc_code, &message) {
                return MessageAdapterError::GroupNotSupported;
            }
            MessageAdapterError::Service(ServiceError {
                status_code,
                rpc_code,
                message: "remote service request failed".to_owned(),
                data: None,
            })
        }
        im_core::ImError::TransportUnavailable { detail } => {
            MessageAdapterError::TransportUnavailable(detail)
        }
        im_core::ImError::PathUnavailable { path_kind, detail } => {
            MessageAdapterError::PathUnavailable(format!("{path_kind} path unavailable: {detail}"))
        }
        im_core::ImError::Io { detail } => MessageAdapterError::PathUnavailable(detail),
        err => MessageAdapterError::Internal(err.to_string()),
    }
}

fn group_e2ee_service_unsupported(rpc_code: i64, message: &str) -> bool {
    if rpc_code != 1405 {
        return false;
    }
    let message = message.to_ascii_lowercase();
    message.contains("group e2ee contract-test apis are disabled")
        || message.contains("group e2ee p6 apis are disabled")
}

fn im_error_to_exit_error(err: im_core::ImError) -> ExitError {
    match im_error_to_message_error(err) {
        MessageAdapterError::MessageIdRequired => ExitError::new(
            "invalid_argument",
            2,
            "attachment message id is required",
            "Pass --message-id <id>.",
        ),
        other => ExitError::new(
            "invalid_argument",
            2,
            other.to_string(),
            "Check the message command arguments and try again.",
        ),
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

fn optional_string_flag(command: &ParsedCommand, name: &str) -> Option<String> {
    let value = string_flag(command, name);
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn optional_message_id_flag(
    command: &ParsedCommand,
    name: &str,
) -> Result<Option<MessageId>, ExitError> {
    optional_string_flag(command, name)
        .map(MessageId::parse)
        .transpose()
        .map_err(|err| {
            ExitError::new(
                "invalid_argument",
                2,
                format!("invalid --{name}: {err}"),
                "Use a non-empty message id value.",
            )
        })
}

#[cfg(test)]
#[path = "messages_tests.rs"]
mod tests;
