use std::collections::HashSet;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use im_core::attachments::{
    AttachmentDestination, DownloadAttachmentRequest, DownloadedAttachmentDestination,
};
use im_core::ids::{MessageId, ThreadId};
use im_core::messages::{
    InboxQuery, InboxScope, Message, MessageBodyView, MessageDeliveryOptions, MessageDirection,
    ThreadRef,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::agent_status::HeartbeatScheduler;
use crate::app_bridge::message_control::{
    handle_app_control_payload, is_app_control_payload, IncomingAppControlPayload,
};
use crate::cli_wrapper::CliWrapperRequest;
use crate::commands::{
    handle_agent_payload_message, AgentCommandOutcome, IncomingAgentPayloadMessage,
};
use crate::controller_scope::{verify_daemon_controller_sender, VerifiedControllerSender};
use crate::inbox::user_delegated::{flush_message_sync_outbox, process_user_delegated_inbox_once};
use crate::inbox::ControllerTextMessage;
use crate::local_rpc::call_uds_once;
#[cfg(unix)]
use crate::local_rpc::{
    bind_uds_listener, handle_uds_stream_with_outbox, verify_socket_permissions,
};
use crate::outbox::{
    AgentManagementOutbox, AgentStatusResponse, ImCoreAgentOutbox, RuntimeAttachmentSend,
    RuntimeAttachmentSendResult, RuntimeMessageSecurity, RuntimeMessageSend,
    RuntimeMessageSendResult, RuntimeOutbox,
};
use crate::plugins::generic_cli::{GenericCliDriverRegistry, GENERIC_CLI_RUNTIME_PLUGIN_ID};
use crate::plugins::hermes::{
    HermesGateway, HermesRuntimePlugin, StdioHermesGateway, HERMES_RUNTIME_PLUGIN_ID,
};
use crate::registration::UserServiceAgentRegistrationClient;
use crate::runtime::host::{
    flush_runtime_final_outbox, run_controller_text_task_with_config,
    run_existing_runtime_task_with_config,
};
use crate::runtime::{
    RuntimeInstallStatus, RuntimeLaunchContext, RuntimeLaunchOutcome, RuntimePlugin,
    RuntimeRunStatus, RuntimeTask,
};
use crate::runtime_inbox::repair_runtime_controller_inbox_projection;
use crate::{DaemonConfig, DaemonState, ImCoreAdapter};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForegroundOptions {
    pub state_root: PathBuf,
    pub poll_interval_ms: u64,
    pub max_runtime_ms: Option<u64>,
    pub max_processed_messages: Option<usize>,
    pub ready_file: Option<PathBuf>,
    pub agent_jwt_token: Option<String>,
    pub mock_status_outbox: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForegroundRunSummary {
    pub status: crate::DaemonStatus,
    pub processed_messages: usize,
    pub sent_status_messages: usize,
    pub status_message_ids: Vec<String>,
    pub runtime_ms: u128,
    pub exit_reason: String,
}

impl ForegroundOptions {
    pub fn new(state_root: PathBuf) -> Self {
        Self {
            state_root,
            poll_interval_ms: 250,
            max_runtime_ms: None,
            max_processed_messages: None,
            ready_file: None,
            agent_jwt_token: None,
            mock_status_outbox: false,
        }
    }
}

pub async fn run_foreground(options: ForegroundOptions) -> Result<ForegroundRunSummary> {
    let started_at = Instant::now();
    let config = DaemonConfig::for_state_root(options.state_root.clone())?;
    config.validate()?;
    config.ensure_state_layout()?;
    let state = DaemonState::open(&config)?;
    let state_summary = state.initialize()?;
    let im_core = ImCoreAdapter::open(&config)?;
    let im_core_status = im_core
        .initialize_local_state()
        .await
        .context("initialize im-core local state")?;
    let status = crate::DaemonStatus {
        state_root: config.state_root.clone(),
        database_path: state_summary.database_path,
        local_socket_path: config.local_socket_path.clone(),
        im_core_sqlite_path: config.im_core_sqlite_path.clone(),
        daemon_schema_version: state_summary.schema_version,
        im_core_schema_version: im_core_status.schema_version,
    };

    if let Some(token) = options.agent_jwt_token.as_deref() {
        store_agent_token_for_configured_agents(&state, token)?;
    }
    sync_configured_agent_identities(&config, &state, &im_core)?;

    let rpc_outbox =
        runtime_callback_outbox(&config, &state, &im_core, options.mock_status_outbox)?;
    let rpc_worker = start_runtime_rpc_worker(
        config.local_socket_path.clone(),
        state.clone(),
        rpc_outbox.clone(),
    )?;
    {
        let outbox = rpc_outbox
            .lock()
            .map_err(|_| anyhow::anyhow!("runtime callback outbox lock poisoned"))?;
        if let Err(error) = flush_runtime_final_outbox(&state, &*outbox, 20) {
            state.insert_audit_event_json(
                "runtime.final_outbox.flush.failed",
                None,
                None,
                None,
                None,
                json!({
                    "error": sanitize_error_message(&error.to_string()),
                }),
            )?;
        }
    }
    if let Err(error) = flush_message_sync_outbox(&config, &state, &im_core, 20) {
        state.insert_audit_event_json(
            "message_sync_outbox.flush.failed",
            None,
            None,
            None,
            None,
            json!({
                "error": sanitize_error_message(&error.to_string()),
            }),
        )?;
    }
    if let Some(path) = options.ready_file.as_ref() {
        write_ready_file(path, &status)?;
    }
    println!(
        "awiki-deamon foreground ready state_root={} socket={}",
        status.state_root.display(),
        status.local_socket_path.display()
    );

    let mut processed = HashSet::new();
    let mut processed_messages = 0usize;
    let mut heartbeat = HeartbeatScheduler::new();
    let exit_reason = loop {
        let newly_processed = process_inbox_once(&config, &state, &im_core, &mut processed).await?;
        let delegated_processed = process_user_delegated_inbox_once(&config, &state, &im_core)?;
        if let Err(error) = flush_message_sync_outbox(&config, &state, &im_core, 20) {
            state.insert_audit_event_json(
                "message_sync_outbox.flush.failed",
                None,
                None,
                None,
                None,
                json!({
                    "error": sanitize_error_message(&error.to_string()),
                }),
            )?;
        }
        let retry_processed = {
            let outbox = rpc_outbox
                .lock()
                .map_err(|_| anyhow::anyhow!("runtime callback outbox lock poisoned"))?;
            drain_runtime_retry_queue_once(&config, &state, &*outbox)?
        };
        {
            let outbox = rpc_outbox
                .lock()
                .map_err(|_| anyhow::anyhow!("runtime callback outbox lock poisoned"))?;
            if let Err(error) = flush_runtime_final_outbox(&state, &*outbox, 20) {
                state.insert_audit_event_json(
                    "runtime.final_outbox.flush.failed",
                    None,
                    None,
                    None,
                    None,
                    json!({
                        "error": sanitize_error_message(&error.to_string()),
                    }),
                )?;
            }
        }
        if let Err(error) = flush_message_sync_outbox(&config, &state, &im_core, 20) {
            state.insert_audit_event_json(
                "message_sync_outbox.flush.failed",
                None,
                None,
                None,
                None,
                json!({
                    "error": sanitize_error_message(&error.to_string()),
                }),
            )?;
        }
        {
            let outbox = rpc_outbox
                .lock()
                .map_err(|_| anyhow::anyhow!("runtime callback outbox lock poisoned"))?;
            match heartbeat.tick(&config, &state, &im_core, &*outbox) {
                Err(error) => {
                    let _ = record_foreground_status_error(&state, &error.to_string());
                }
                Ok(_) => {}
            }
        };
        let newly_processed = newly_processed + delegated_processed + retry_processed;
        processed_messages += newly_processed;
        if let Some(limit) = options.max_processed_messages {
            if processed_messages >= limit {
                break "max_processed_messages".to_string();
            }
        }
        if let Some(limit_ms) = options.max_runtime_ms {
            if started_at.elapsed() >= Duration::from_millis(limit_ms) {
                break "max_runtime_ms".to_string();
            }
        }
        tokio::time::sleep(Duration::from_millis(options.poll_interval_ms)).await;
    };
    rpc_worker.stop();
    let _ = std::fs::remove_file(&config.local_socket_path);
    Ok(ForegroundRunSummary {
        status,
        processed_messages,
        sent_status_messages: rpc_outbox
            .lock()
            .map(|outbox| outbox.sent_messages())
            .unwrap_or_default(),
        status_message_ids: rpc_outbox
            .lock()
            .map(|outbox| outbox.status_message_ids())
            .unwrap_or_default(),
        runtime_ms: started_at.elapsed().as_millis(),
        exit_reason,
    })
}

async fn process_inbox_once(
    config: &DaemonConfig,
    state: &DaemonState,
    im_core: &ImCoreAdapter,
    processed: &mut HashSet<String>,
) -> Result<usize> {
    let agents = state.list_agent_definitions()?;
    let registration = UserServiceAgentRegistrationClient::new(&config.user_service_base_url)?;
    let mut processed_count = 0usize;
    for agent in agents {
        let identity = match state.load_agent_identity(&agent.agent_did) {
            Ok(identity) => identity,
            Err(_) => continue,
        };
        let jwt_token = state.load_agent_auth_token(&agent.agent_did)?;
        let client = im_core.client_for_agent_identity(config, &identity, jwt_token.as_deref())?;
        ensure_agent_messaging_session(&client, &agent.agent_did).await?;
        let inbox = client
            .messages()
            .inbox_with_metadata_async(InboxQuery {
                scope: InboxScope::DirectOnly,
                limit: im_core::ids::PageLimit::new(20)?,
                cursor: None,
                unread_only: false,
                inbox_history_options: None,
            })
            .await
            .with_context(|| format!("poll inbox for agent {}", agent.agent_did))?;
        for message in inbox.items.into_iter().rev() {
            if message.direction == MessageDirection::Outgoing {
                continue;
            }
            let message_key = format!("{}:{}", agent.agent_did, message.id.as_str());
            if !processed.insert(message_key) {
                continue;
            }
            match route_message(
                config,
                state,
                im_core,
                &registration,
                &client,
                &agent.agent_did,
                &message,
            )
            .await
            {
                Ok(true) => {
                    processed_count += 1;
                }
                Ok(false) => {}
                Err(error) => {
                    let sanitized = sanitize_error_message(&error.to_string());
                    eprintln!("warning: daemon inbox message route failed: {sanitized}");
                    if let Err(audit_error) =
                        record_inbox_route_error(state, &agent.agent_did, &message, &error)
                    {
                        eprintln!(
                            "warning: daemon inbox route error audit failed: {}",
                            sanitize_error_message(&audit_error.to_string())
                        );
                    }
                }
            }
        }
    }
    Ok(processed_count)
}

fn drain_runtime_retry_queue_once(
    config: &DaemonConfig,
    state: &DaemonState,
    outbox: &impl RuntimeOutbox,
) -> Result<usize> {
    let retries = state.list_queued_runtime_retries(10)?;
    let mut processed = 0usize;
    for retry in retries {
        state.mark_runtime_retry_status(&retry.retry_id, "running")?;
        let result = run_runtime_retry(config, state, outbox, &retry);
        match result {
            Ok(run_id) => {
                state.mark_runtime_retry_status(&retry.retry_id, "succeeded")?;
                state.insert_audit_event_json(
                    "runtime.run.retry.succeeded",
                    Some(&retry.agent_did),
                    Some(&retry.runtime_profile_id),
                    Some(&run_id),
                    None,
                    json!({
                        "retry_id": retry.retry_id,
                        "original_run_id": retry.original_run_id,
                        "task_id": retry.task_id,
                    }),
                )?;
            }
            Err(error) => {
                state.mark_runtime_retry_status(&retry.retry_id, "failed")?;
                state.insert_audit_event_json(
                    "runtime.run.retry.failed",
                    Some(&retry.agent_did),
                    Some(&retry.runtime_profile_id),
                    Some(&retry.original_run_id),
                    None,
                    json!({
                        "retry_id": retry.retry_id,
                        "original_run_id": retry.original_run_id,
                        "task_id": retry.task_id,
                        "error": sanitize_error_message(&error.to_string()),
                    }),
                )?;
            }
        }
        processed += 1;
    }
    Ok(processed)
}

fn record_foreground_status_error(state: &DaemonState, message: &str) -> Result<()> {
    for daemon in state
        .list_agent_definitions()?
        .into_iter()
        .filter(|agent| agent.agent_kind == crate::agent::AgentKind::Daemon)
    {
        state.insert_audit_event_json(
            "daemon.status.heartbeat.failed",
            Some(&daemon.agent_did),
            None,
            None,
            None,
            json!({
                "error": sanitize_error_message(message),
            }),
        )?;
    }
    Ok(())
}

fn run_runtime_retry(
    config: &DaemonConfig,
    state: &DaemonState,
    outbox: &impl RuntimeOutbox,
    retry: &crate::state::RuntimeRetryQueueRecord,
) -> Result<String> {
    let original_run = state.load_runtime_run(&retry.original_run_id)?;
    if original_run.status != RuntimeRunStatus::Failed {
        bail!("runtime retry original run is no longer failed");
    }
    if original_run.agent_did != retry.agent_did
        || original_run.task_id != retry.task_id
        || original_run.runtime_profile_id != retry.runtime_profile_id
        || original_run.runtime_plugin_id != retry.runtime_plugin_id
    {
        bail!("runtime retry queue record does not match original run");
    }
    let task = state.load_runtime_task(&retry.task_id)?;
    let profile = state.load_runtime_agent_profile(&retry.agent_did)?;
    validate_retry_task_binding(&task, &profile)?;
    let run_id = format!("run_{}", retry.retry_id);
    match profile.runtime_plugin_id.as_str() {
        HERMES_RUNTIME_PLUGIN_ID => {
            let hermes_profile = state.load_hermes_profile(&profile.agent_did)?;
            let plugin = HermesRuntimePlugin::with_state(
                StdioHermesGateway::from_config(config),
                hermes_profile,
                state.clone(),
            );
            run_existing_runtime_task_with_config(
                config,
                state,
                &profile,
                &plugin,
                outbox,
                task,
                run_id.clone(),
            )?;
        }
        GENERIC_CLI_RUNTIME_PLUGIN_ID => {
            let cli_profile = state.load_cli_runtime_profile(&profile.runtime_profile_id)?;
            let plugin = GenericCliDriverRegistry::new(cli_profile);
            run_existing_runtime_task_with_config(
                config,
                state,
                &profile,
                &plugin,
                outbox,
                task,
                run_id.clone(),
            )?;
        }
        _ => {
            let plugin = UdsTestRuntimePlugin::new(config.local_socket_path.clone());
            run_existing_runtime_task_with_config(
                config,
                state,
                &profile,
                &plugin,
                outbox,
                task,
                run_id.clone(),
            )?;
        }
    }
    Ok(run_id)
}

fn validate_retry_task_binding(
    task: &RuntimeTask,
    profile: &crate::runtime::RuntimeAgentProfile,
) -> Result<()> {
    if task.agent_did != profile.agent_did
        || task.controller_user_id != profile.controller_user_id
        || task.controller_full_handle != profile.controller_full_handle
        || task.controller_scope_key != profile.controller_scope_key
        || task.sender_did != task.controller_did
    {
        bail!("runtime retry task does not match profile controller scope");
    }
    Ok(())
}

fn sanitize_error_message(message: &str) -> String {
    let mut sanitized = message
        .split_whitespace()
        .map(|part| {
            let lower = part.to_ascii_lowercase();
            if lower.contains("token")
                || lower.contains("secret")
                || lower.contains("jwt")
                || lower.contains("key")
            {
                "<redacted>"
            } else if part.starts_with('/') || part.starts_with("file://") {
                "<path>"
            } else {
                part
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    if sanitized.chars().count() > 240 {
        sanitized = sanitized.chars().take(240).collect::<String>();
    }
    sanitized
}

async fn route_message(
    config: &DaemonConfig,
    state: &DaemonState,
    im_core: &ImCoreAdapter,
    registration: &UserServiceAgentRegistrationClient,
    target_client: &im_core::ImClient,
    target_agent_did: &str,
    message: &Message,
) -> Result<bool> {
    let sender_did = message.sender.as_str().to_string();
    let conversation_id = conversation_id(message);
    match &message.body {
        MessageBodyView::Payload { payload } => {
            let content_type = message
                .metadata
                .content_type
                .clone()
                .unwrap_or_else(|| "application/json".to_string());
            let payload_message = IncomingAgentPayloadMessage {
                message_id: message.id.as_str().to_string(),
                conversation_id: conversation_id.clone(),
                sender_did: sender_did.clone(),
                target_agent_did: target_agent_did.to_string(),
                content_type: content_type.clone(),
                payload: payload.clone(),
            };
            if is_app_control_payload(payload) {
                handle_app_control_payload(
                    config,
                    state,
                    registration,
                    IncomingAppControlPayload {
                        message_id: payload_message.message_id.clone(),
                        conversation_id: payload_message.conversation_id.clone(),
                        sender_did: payload_message.sender_did.clone(),
                        target_agent_did: payload_message.target_agent_did.clone(),
                        content_type: payload_message.content_type.clone(),
                        payload: payload_message.payload.clone(),
                    },
                )?;
                return Ok(true);
            }
            if is_attachment_manifest_message(message, &content_type, payload) {
                let verified_sender = verify_runtime_controller_sender(
                    config,
                    state,
                    registration,
                    target_agent_did,
                    &sender_did,
                )?;
                let task_text = attachment_runtime_prompt_text(
                    config,
                    target_client,
                    target_agent_did,
                    message,
                    &sender_did,
                    payload,
                )
                .await?;
                route_runtime_controller_text(
                    config,
                    state,
                    im_core,
                    target_client,
                    target_agent_did,
                    message.id.as_str(),
                    conversation_id.clone(),
                    sender_did.clone(),
                    Some(verified_sender),
                    task_text,
                )?;
                return Ok(true);
            }
            if !is_awiki_agent_command_payload(payload) {
                record_ignored_non_command_payload(
                    state,
                    target_agent_did,
                    message,
                    &content_type,
                    payload,
                )?;
                return Ok(false);
            }
            if payload.get("command").and_then(Value::as_str) == Some("runtime.task.submit") {
                run_runtime_task_command(config, state, im_core, registration, payload_message)?;
            } else {
                let outbox = ImCoreAgentOutbox::new(target_client.clone());
                let outcome = handle_agent_payload_message(
                    config,
                    state,
                    registration,
                    &outbox,
                    payload_message,
                )?;
                sync_configured_agent_identities(config, state, im_core)?;
                if let AgentCommandOutcome::RuntimeAgentCreated(created) = outcome {
                    send_runtime_agent_welcome_message(config, state, im_core, &created)?;
                }
            }
            Ok(true)
        }
        MessageBodyView::Text { text, .. } => {
            route_runtime_controller_text(
                config,
                state,
                im_core,
                target_client,
                target_agent_did,
                message.id.as_str(),
                conversation_id,
                sender_did,
                None,
                text.clone(),
            )?;
            Ok(true)
        }
        MessageBodyView::Unsupported { .. } => Ok(false),
    }
}

fn is_awiki_agent_command_payload(payload: &Value) -> bool {
    payload.get("schema").and_then(Value::as_str) == Some("awiki.agent.command.v1")
}

fn is_attachment_manifest_message(message: &Message, content_type: &str, payload: &Value) -> bool {
    let manifest_content_type = im_core::attachments::attachment_manifest_content_type();
    let content_type_matches = content_type.trim() == manifest_content_type
        || message.metadata.content_type.as_deref().map(str::trim) == Some(manifest_content_type)
        || message.metadata.attributes.iter().any(|attribute| {
            attribute.key.trim() == "content_type"
                && attribute.value.trim() == manifest_content_type
        });
    content_type_matches
        && payload
            .get("attachments")
            .and_then(Value::as_array)
            .is_some()
}

fn route_runtime_controller_text(
    config: &DaemonConfig,
    state: &DaemonState,
    im_core: &ImCoreAdapter,
    target_client: &im_core::ImClient,
    target_agent_did: &str,
    message_id: &str,
    conversation_id: Option<String>,
    sender_did: String,
    verified_sender: Option<VerifiedControllerSender>,
    text: String,
) -> Result<()> {
    let _verified_sender = match verified_sender {
        Some(verified_sender) => verified_sender,
        None => {
            let registration =
                UserServiceAgentRegistrationClient::new(&config.user_service_base_url)?;
            verify_runtime_controller_sender(
                config,
                state,
                &registration,
                target_agent_did,
                &sender_did,
            )?
        }
    };
    repair_runtime_controller_inbox_projection_best_effort(config, state, target_agent_did);
    let outbox = ImCoreAgentOutbox::new(target_client.clone());
    let status_sender = runtime_status_sender_for_agent(config, state, im_core, target_agent_did)?;
    let runtime_outbox = ControllerRuntimeOutbox::new(
        ControllerOutboxSender::ImCore(outbox.clone()),
        status_sender.sender,
        Some(status_sender.daemon_agent_did),
        sender_did.clone(),
        format!("task_{message_id}"),
        conversation_id.clone(),
        Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        Arc::new(Mutex::new(Vec::new())),
    );
    run_runtime_text_message_with_gateway(
        config,
        state,
        &runtime_outbox,
        ControllerTextMessage {
            message_id: message_id.to_string(),
            conversation_id,
            sender_did,
            target_agent_did: target_agent_did.to_string(),
            text,
        },
        || StdioHermesGateway::from_config(config),
    )?;
    Ok(())
}

fn verify_runtime_controller_sender<C>(
    config: &DaemonConfig,
    state: &DaemonState,
    registration: &C,
    target_agent_did: &str,
    sender_did: &str,
) -> Result<VerifiedControllerSender>
where
    C: crate::registration::AgentInventoryClient,
{
    let binding = state
        .load_runtime_daemon_binding(target_agent_did)?
        .with_context(|| format!("runtime daemon binding missing for {target_agent_did}"))?;
    let daemon_agent = state.load_agent_definition(&binding.daemon_agent_did)?;
    let verified =
        verify_daemon_controller_sender(config, state, registration, &daemon_agent, sender_did)?;
    if binding.controller_scope_key != verified.controller_scope_key {
        bail!("runtime controller scope does not match daemon controller scope");
    }
    Ok(verified)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RuntimeInboundAttachment {
    attachment_id: String,
    filename: String,
    mime_type: String,
    size: String,
    size_bytes: Option<u64>,
    local_path: Option<PathBuf>,
    download_status: String,
    error: Option<String>,
}

async fn attachment_runtime_prompt_text(
    config: &DaemonConfig,
    target_client: &im_core::ImClient,
    target_agent_did: &str,
    message: &Message,
    sender_did: &str,
    payload: &Value,
) -> Result<String> {
    let caption = attachment_caption(payload).unwrap_or_default();
    let attachments = attachment_items_from_payload(payload)?;
    let mut resolved = Vec::new();
    for attachment in attachments {
        resolved.push(
            resolve_inbound_attachment(
                config,
                target_client,
                target_agent_did,
                message,
                sender_did,
                attachment,
            )
            .await,
        );
    }
    Ok(render_attachment_runtime_prompt(&caption, &resolved))
}

fn attachment_caption(payload: &Value) -> Option<String> {
    payload
        .get("caption")
        .and_then(Value::as_str)
        .or_else(|| payload.get("text").and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn attachment_items_from_payload(payload: &Value) -> Result<Vec<RuntimeInboundAttachment>> {
    let attachments = payload
        .get("attachments")
        .and_then(Value::as_array)
        .context("attachment manifest attachments must be an array")?;
    let mut items = Vec::new();
    for item in attachments {
        let attachment_id = string_field(item.get("attachment_id"));
        if attachment_id.is_empty() {
            bail!("attachment manifest item is missing attachment_id");
        }
        let filename = first_non_empty_string([
            string_field(item.get("filename")),
            "attachment.bin".to_string(),
        ]);
        items.push(RuntimeInboundAttachment {
            attachment_id,
            filename,
            mime_type: string_field(item.get("mime_type")),
            size: string_field(item.get("size")),
            size_bytes: attachment_size_bytes(item),
            local_path: None,
            download_status: "pending".to_string(),
            error: None,
        });
    }
    if items.is_empty() {
        bail!("attachment manifest must contain at least one attachment");
    }
    Ok(items)
}

async fn resolve_inbound_attachment(
    config: &DaemonConfig,
    target_client: &im_core::ImClient,
    target_agent_did: &str,
    message: &Message,
    sender_did: &str,
    mut attachment: RuntimeInboundAttachment,
) -> RuntimeInboundAttachment {
    match download_inbound_attachment(
        config,
        target_client,
        target_agent_did,
        message,
        sender_did,
        &attachment,
    )
    .await
    {
        Ok(path) => {
            attachment.local_path = Some(path);
            attachment.download_status = "downloaded".to_string();
        }
        Err(error) => {
            attachment.download_status = "failed".to_string();
            attachment.error = Some(sanitize_error_message(&error.to_string()));
        }
    }
    attachment
}

async fn download_inbound_attachment(
    config: &DaemonConfig,
    target_client: &im_core::ImClient,
    target_agent_did: &str,
    message: &Message,
    sender_did: &str,
    attachment: &RuntimeInboundAttachment,
) -> Result<PathBuf> {
    let destination = inbound_attachment_path(
        config,
        target_agent_did,
        message,
        &attachment.attachment_id,
        &attachment.filename,
    )?;
    if destination.exists() {
        set_private_file_permissions(&destination)?;
        return Ok(destination);
    }
    let message_id = MessageId::parse(message.id.as_str())?;
    let download_thread = attachment_download_thread(message, sender_did)?;
    let downloaded = target_client
        .attachments()
        .download_async(DownloadAttachmentRequest {
            thread: download_thread,
            message_id,
            attachment_id: Some(attachment.attachment_id.clone()),
            destination: AttachmentDestination::LocalFile(destination.clone()),
            overwrite: false,
        })
        .await?;
    match downloaded.destination {
        DownloadedAttachmentDestination::LocalFile(path) => {
            set_private_file_permissions(&path)?;
            Ok(path)
        }
        DownloadedAttachmentDestination::Memory(_) => bail!(
            "attachment {} downloaded to memory instead of local file for sender {}",
            attachment.attachment_id,
            sender_did
        ),
    }
}

fn attachment_download_thread(message: &Message, sender_did: &str) -> Result<ThreadRef> {
    match &message.thread {
        ThreadRef::Direct(_) | ThreadRef::Group(_) => Ok(message.thread.clone()),
        ThreadRef::Thread(thread) => {
            let raw = thread.as_str();
            if let Some(group) = raw.strip_prefix("group:") {
                return Ok(ThreadRef::Group(im_core::ids::GroupRef::parse(group)?));
            }
            let peer = if message.sender.as_str().trim().is_empty() {
                sender_did.trim()
            } else {
                message.sender.as_str().trim()
            };
            if peer.starts_with("did:") {
                return Ok(ThreadRef::Direct(im_core::ids::PeerRef::parse(peer, "")?));
            }
            Ok(ThreadRef::Thread(ThreadId::parse(raw)?))
        }
    }
}

fn inbound_attachment_path(
    config: &DaemonConfig,
    target_agent_did: &str,
    message: &Message,
    attachment_id: &str,
    filename: &str,
) -> Result<PathBuf> {
    let file_name = safe_file_name(filename, "attachment.bin");
    let path = config
        .state_root
        .join("runtime-attachments")
        .join(safe_path_segment(target_agent_did, "agent"))
        .join(safe_path_segment(
            thread_ref_segment(&message.thread).as_str(),
            "conversation",
        ))
        .join(safe_path_segment(message.id.as_str(), "message"))
        .join(safe_path_segment(attachment_id, "attachment"))
        .join(file_name);
    ensure_path_under_root(&path, &config.state_root)?;
    if let Some(parent) = path.parent() {
        create_private_dir_all(&config.state_root.join("runtime-attachments"), parent)?;
    }
    Ok(path)
}

fn thread_ref_segment(thread: &ThreadRef) -> String {
    match thread {
        ThreadRef::Direct(peer) => peer.as_str().to_string(),
        ThreadRef::Group(group) => group.as_str().to_string(),
        ThreadRef::Thread(thread) => thread.as_str().to_string(),
    }
}

fn render_attachment_runtime_prompt(
    caption: &str,
    attachments: &[RuntimeInboundAttachment],
) -> String {
    let mut text = String::new();
    text.push_str("控制者消息:\n");
    if caption.trim().is_empty() {
        text.push_str("（控制者只发送了附件，没有输入文本消息。）\n");
    } else {
        text.push_str(caption.trim());
        text.push('\n');
    }
    text.push('\n');
    text.push_str("附件资源:\n");
    for (index, attachment) in attachments.iter().enumerate() {
        text.push_str(&format!(
            "{}. attachment_id: {}\n",
            index + 1,
            attachment.attachment_id
        ));
        text.push_str(&format!("   filename: {}\n", attachment.filename));
        text.push_str(&format!("   mime_type: {}\n", attachment.mime_type));
        if let Some(size_bytes) = attachment.size_bytes {
            text.push_str(&format!("   size_bytes: {size_bytes}\n"));
        } else if !attachment.size.trim().is_empty() {
            text.push_str(&format!("   size: {}\n", attachment.size));
        }
        text.push_str(&format!(
            "   download_status: {}\n",
            attachment.download_status
        ));
        if let Some(path) = attachment.local_path.as_ref() {
            text.push_str(&format!("   local_path: {}\n", path.display()));
        }
        if let Some(error) = attachment.error.as_ref() {
            text.push_str(&format!("   error: {error}\n"));
        }
    }
    text.push('\n');
    text.push_str(
        "附件处理规则：这些文件是控制者提供的资源。只有当控制者消息或会话上下文表明需要使用文件时，才读取或检查这些文件。\n",
    );
    text
}

fn string_field(value: Option<&Value>) -> String {
    value
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn first_non_empty_string(values: impl IntoIterator<Item = String>) -> String {
    values
        .into_iter()
        .map(|value| value.trim().to_string())
        .find(|value| !value.is_empty())
        .unwrap_or_default()
}

fn attachment_size_bytes(value: &Value) -> Option<u64> {
    value
        .get("size_bytes")
        .and_then(Value::as_u64)
        .or_else(|| value.get("size").and_then(Value::as_str)?.parse().ok())
}

fn create_private_dir_all(root: &Path, target: &Path) -> Result<()> {
    if !target.starts_with(root) {
        bail!("private directory target must stay under root");
    }
    std::fs::create_dir_all(target)
        .with_context(|| format!("create inbound attachment directory {}", target.display()))?;

    let mut current = root.to_path_buf();
    set_private_dir_permissions(&current)?;
    let relative = target
        .strip_prefix(root)
        .with_context(|| format!("strip private directory root {}", root.display()))?;
    for component in relative.components() {
        if let std::path::Component::Normal(segment) = component {
            current.push(segment);
            set_private_dir_permissions(&current)?;
        }
    }
    Ok(())
}

fn safe_path_segment(value: &str, fallback: &str) -> String {
    let segment = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches(['-', '.'])
        .to_string();
    if segment.is_empty() {
        fallback.to_string()
    } else {
        segment
    }
}

fn safe_file_name(value: &str, fallback: &str) -> String {
    let name = Path::new(value)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(value);
    let segment = safe_path_segment(name, fallback);
    if segment == "." || segment == ".." {
        fallback.to_string()
    } else {
        segment
    }
}

fn ensure_path_under_root(path: &Path, root: &Path) -> Result<()> {
    if path
        .components()
        .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        bail!("attachment path must not contain parent/root components");
    }
    if !path.starts_with(root) {
        bail!("attachment path must stay under daemon state root");
    }
    Ok(())
}

#[cfg(unix)]
fn set_private_dir_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).with_context(|| {
        format!(
            "set private attachment directory permissions {}",
            path.display()
        )
    })
}

#[cfg(not(unix))]
fn set_private_dir_permissions(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
fn set_private_file_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .with_context(|| format!("set private attachment file permissions {}", path.display()))
}

#[cfg(not(unix))]
fn set_private_file_permissions(_path: &Path) -> Result<()> {
    Ok(())
}

fn record_ignored_non_command_payload(
    state: &DaemonState,
    target_agent_did: &str,
    message: &Message,
    content_type: &str,
    payload: &Value,
) -> Result<()> {
    state.insert_audit_event_json(
        "daemon.inbox.payload.ignored",
        Some(target_agent_did),
        None,
        None,
        None,
        json!({
            "reason": "not_awiki_agent_command",
            "message_id": message.id.as_str(),
            "sender_did": message.sender.as_str(),
            "target_agent_did": target_agent_did,
            "content_type": content_type,
            "schema": payload.get("schema").and_then(Value::as_str),
        }),
    )
}

fn record_inbox_route_error(
    state: &DaemonState,
    target_agent_did: &str,
    message: &Message,
    error: &anyhow::Error,
) -> Result<()> {
    state.insert_audit_event_json(
        "daemon.inbox.message.route.failed",
        Some(target_agent_did),
        None,
        None,
        None,
        json!({
            "message_id": message.id.as_str(),
            "sender_did": message.sender.as_str(),
            "target_agent_did": target_agent_did,
            "content_type": message.metadata.content_type.as_deref().unwrap_or(""),
            "error": sanitize_error_message(&error.to_string()),
        }),
    )
}

fn send_runtime_agent_welcome_message(
    config: &DaemonConfig,
    state: &DaemonState,
    im_core: &ImCoreAdapter,
    created: &crate::commands::RuntimeAgentCreateOutcome,
) -> Result<()> {
    send_runtime_agent_welcome_message_with_sender(
        config,
        state,
        &ImCoreWelcomeSender { im_core },
        created,
    )
}

fn send_runtime_agent_welcome_message_with_sender(
    config: &DaemonConfig,
    state: &DaemonState,
    sender: &impl RuntimeWelcomeSender,
    created: &crate::commands::RuntimeAgentCreateOutcome,
) -> Result<()> {
    if created.runtime_plugin_id != HERMES_RUNTIME_PLUGIN_ID {
        return Ok(());
    }
    let controller_did = controller_did_for_runtime(state, &created.agent_did)?;
    let idempotency_key = welcome_idempotency_key(&created.agent_did, &controller_did);
    if state.audit_event_exists(
        "runtime.welcome.sent",
        Some(&created.agent_did),
        Some(&idempotency_key),
    )? {
        return Ok(());
    }
    if let Err(error) = try_send_runtime_agent_welcome_message(
        config,
        state,
        sender,
        created,
        &controller_did,
        &idempotency_key,
    ) {
        state.insert_audit_event_json(
            "runtime.welcome.failed",
            Some(&created.agent_did),
            Some(&created.runtime_profile_id),
            None,
            None,
            json!({
                "idempotency_key": idempotency_key,
                "error_code": "welcome_send_failed",
                "reason": sanitize_error_message(&error.to_string()),
            }),
        )?;
    }
    Ok(())
}

fn try_send_runtime_agent_welcome_message(
    config: &DaemonConfig,
    state: &DaemonState,
    sender: &impl RuntimeWelcomeSender,
    created: &crate::commands::RuntimeAgentCreateOutcome,
    controller_did: &str,
    idempotency_key: &str,
) -> Result<()> {
    let identity = state.load_agent_identity(&created.agent_did)?;
    let jwt_token = state.load_agent_auth_token(&created.agent_did)?;
    let result = sender.send_welcome(
        config,
        &identity,
        jwt_token.as_deref(),
        &controller_did,
        "Hermes 已准备好。",
        RuntimeMessageSecurity::DefaultPlain,
        MessageDeliveryOptions {
            idempotency_key: Some(idempotency_key.to_string()),
            wait_for_final_acceptance: false,
        },
    )?;
    state.insert_audit_event_json(
        "runtime.welcome.sent",
        Some(&created.agent_did),
        Some(&created.runtime_profile_id),
        None,
        None,
        json!({
            "idempotency_key": idempotency_key,
            "message_id": result.message.id.as_str(),
        }),
    )?;
    Ok(())
}

fn welcome_idempotency_key(runtime_agent_did: &str, controller_did: &str) -> String {
    format!("welcome:{runtime_agent_did}:{controller_did}")
}

fn controller_did_for_runtime(state: &DaemonState, runtime_agent_did: &str) -> Result<String> {
    state
        .load_runtime_daemon_binding(runtime_agent_did)?
        .map(|binding| binding.controller_did)
        .with_context(|| format!("runtime daemon binding missing for {runtime_agent_did}"))
}

trait RuntimeWelcomeSender {
    fn send_welcome(
        &self,
        config: &DaemonConfig,
        identity: &crate::agent::AgentIdentityRecord,
        jwt_token: Option<&str>,
        controller_did: &str,
        text: &str,
        security: RuntimeMessageSecurity,
        delivery: MessageDeliveryOptions,
    ) -> Result<im_core::messages::SendMessageResult>;
}

struct ImCoreWelcomeSender<'a> {
    im_core: &'a ImCoreAdapter,
}

impl RuntimeWelcomeSender for ImCoreWelcomeSender<'_> {
    fn send_welcome(
        &self,
        config: &DaemonConfig,
        identity: &crate::agent::AgentIdentityRecord,
        jwt_token: Option<&str>,
        controller_did: &str,
        text: &str,
        security: RuntimeMessageSecurity,
        delivery: MessageDeliveryOptions,
    ) -> Result<im_core::messages::SendMessageResult> {
        let client = self
            .im_core
            .client_for_agent_identity(config, identity, jwt_token)?;
        ImCoreAgentOutbox::new(client).send_text_with_delivery(
            controller_did,
            text,
            security,
            delivery,
        )
    }
}

fn run_runtime_text_message_with_gateway<G, F>(
    config: &DaemonConfig,
    state: &DaemonState,
    outbox: &impl RuntimeOutbox,
    message: ControllerTextMessage,
    hermes_gateway_factory: F,
) -> Result<crate::runtime::host::RuntimeTaskRunResult>
where
    G: HermesGateway + Clone,
    F: Fn() -> G,
{
    let profile = state.load_runtime_agent_profile(&message.target_agent_did)?;
    match profile.runtime_plugin_id.as_str() {
        HERMES_RUNTIME_PLUGIN_ID => {
            let hermes_profile = state.load_hermes_profile(&profile.agent_did)?;
            let plugin = HermesRuntimePlugin::with_state(
                hermes_gateway_factory(),
                hermes_profile,
                state.clone(),
            );
            run_controller_text_task_with_config(config, state, &profile, &plugin, outbox, message)
        }
        GENERIC_CLI_RUNTIME_PLUGIN_ID => {
            let cli_profile = state.load_cli_runtime_profile(&profile.runtime_profile_id)?;
            let plugin = GenericCliDriverRegistry::new(cli_profile);
            run_controller_text_task_with_config(config, state, &profile, &plugin, outbox, message)
        }
        _ => {
            let plugin = UdsTestRuntimePlugin::new(config.local_socket_path.clone());
            run_controller_text_task_with_config(config, state, &profile, &plugin, outbox, message)
        }
    }
}

fn run_runtime_task_command(
    config: &DaemonConfig,
    state: &DaemonState,
    im_core: &ImCoreAdapter,
    registration: &UserServiceAgentRegistrationClient,
    message: IncomingAgentPayloadMessage,
) -> Result<()> {
    let payload = RuntimeTaskSubmitPayload::parse(&message.payload)?;
    let target_agent_did = payload
        .target_agent_did
        .as_deref()
        .unwrap_or(&message.target_agent_did)
        .to_string();
    let verified_sender = verify_runtime_controller_sender(
        config,
        state,
        registration,
        &target_agent_did,
        &message.sender_did,
    )?;
    repair_runtime_controller_inbox_projection_best_effort(config, state, &target_agent_did);
    let profile = state.load_runtime_agent_profile(&target_agent_did)?;
    let message_id = payload.message_id(&message.message_id);
    let status_sender = runtime_status_sender_for_agent(config, state, im_core, &target_agent_did)?;
    let runtime_outbox = ControllerRuntimeOutbox::new(
        runtime_message_sender_for_agent(config, state, im_core, &target_agent_did)?,
        status_sender.sender,
        Some(status_sender.daemon_agent_did),
        message.sender_did.clone(),
        format!("task_{message_id}"),
        message.conversation_id.clone(),
        Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        Arc::new(Mutex::new(Vec::new())),
    );
    let task_message = ControllerTextMessage {
        message_id,
        conversation_id: message.conversation_id,
        sender_did: verified_sender.sender_did,
        target_agent_did,
        text: payload.text,
    };
    match profile.runtime_plugin_id.as_str() {
        GENERIC_CLI_RUNTIME_PLUGIN_ID => {
            let cli_profile = state.load_cli_runtime_profile(&profile.runtime_profile_id)?;
            let plugin = GenericCliDriverRegistry::new(cli_profile);
            run_controller_text_task_with_config(
                config,
                state,
                &profile,
                &plugin,
                &runtime_outbox,
                task_message,
            )?;
        }
        _ => {
            let plugin = UdsTestRuntimePlugin::new(config.local_socket_path.clone());
            run_controller_text_task_with_config(
                config,
                state,
                &profile,
                &plugin,
                &runtime_outbox,
                task_message,
            )?;
        }
    }
    Ok(())
}

fn repair_runtime_controller_inbox_projection_best_effort(
    config: &DaemonConfig,
    state: &DaemonState,
    target_agent_did: &str,
) {
    if let Err(error) = repair_runtime_controller_inbox_projection(config, state, target_agent_did)
    {
        eprintln!(
            "warning: runtime inbox projection repair failed: {}",
            sanitize_error_message(&error.to_string())
        );
    }
}

#[derive(Debug, Deserialize)]
struct RuntimeTaskSubmitPayload {
    schema: String,
    command: String,
    #[serde(default)]
    command_id: Option<String>,
    #[serde(default)]
    task_id: Option<String>,
    #[serde(default)]
    target_agent_did: Option<String>,
    args: RuntimeTaskSubmitArgs,
}

#[derive(Debug, Deserialize)]
struct RuntimeTaskSubmitArgs {
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    prompt: Option<String>,
}

#[derive(Debug)]
struct ParsedRuntimeTaskSubmit {
    command_id: Option<String>,
    task_id: Option<String>,
    target_agent_did: Option<String>,
    text: String,
}

impl RuntimeTaskSubmitPayload {
    fn parse(value: &Value) -> Result<ParsedRuntimeTaskSubmit> {
        let payload: Self =
            serde_json::from_value(value.clone()).context("parse runtime task submit payload")?;
        if payload.schema != "awiki.agent.command.v1" {
            bail!("unsupported agent command schema: {}", payload.schema);
        }
        if payload.command != "runtime.task.submit" {
            bail!("unsupported runtime task command: {}", payload.command);
        }
        let text = payload
            .args
            .text
            .or(payload.args.prompt)
            .filter(|value| !value.trim().is_empty())
            .context("runtime.task.submit args.text is required")?;
        Ok(ParsedRuntimeTaskSubmit {
            command_id: payload.command_id,
            task_id: payload.task_id,
            target_agent_did: payload.target_agent_did,
            text,
        })
    }
}

impl ParsedRuntimeTaskSubmit {
    fn message_id(&self, fallback: &str) -> String {
        self.task_id
            .as_deref()
            .or(self.command_id.as_deref())
            .unwrap_or(fallback)
            .trim_start_matches("task_")
            .to_string()
    }
}

#[derive(Clone)]
struct ControllerRuntimeOutbox {
    message_sender: ControllerOutboxSender,
    status_sender: ControllerOutboxSender,
    daemon_agent_did: Option<String>,
    recipient_did: String,
    task_id: String,
    conversation_id: Option<String>,
    sent_counter: Arc<std::sync::atomic::AtomicUsize>,
    sent_message_ids: Arc<Mutex<Vec<String>>>,
}

impl ControllerRuntimeOutbox {
    fn new(
        message_sender: ControllerOutboxSender,
        status_sender: ControllerOutboxSender,
        daemon_agent_did: Option<String>,
        recipient_did: impl Into<String>,
        task_id: impl Into<String>,
        conversation_id: Option<String>,
        sent_counter: Arc<std::sync::atomic::AtomicUsize>,
        sent_message_ids: Arc<Mutex<Vec<String>>>,
    ) -> Self {
        Self {
            message_sender,
            status_sender,
            daemon_agent_did,
            recipient_did: recipient_did.into(),
            task_id: task_id.into(),
            conversation_id,
            sent_counter,
            sent_message_ids,
        }
    }

    fn send_status_payload(&self, recipient_did: &str, payload: Value) -> Result<()> {
        let message_id = self.status_sender.send_payload(recipient_did, payload)?;
        self.sent_counter
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if let Ok(mut ids) = self.sent_message_ids.lock() {
            ids.push(message_id);
        }
        Ok(())
    }
}

#[derive(Clone)]
enum ControllerOutboxSender {
    ImCore(ImCoreAgentOutbox),
    Mock,
    #[cfg(test)]
    Recording(ControllerOutboxRecorder),
}

impl ControllerOutboxSender {
    fn send_payload(&self, recipient_did: &str, payload: Value) -> Result<String> {
        match self {
            Self::ImCore(outbox) => Ok(outbox
                .send_payload(recipient_did, payload)?
                .message
                .id
                .as_str()
                .to_string()),
            Self::Mock => Ok(format!(
                "mock-status-{}",
                payload
                    .get("state")
                    .and_then(Value::as_str)
                    .unwrap_or("message")
            )),
            #[cfg(test)]
            Self::Recording(recorder) => recorder.record_payload(recipient_did, payload),
        }
    }

    fn send_runtime_message(&self, message: &RuntimeMessageSend) -> Result<String> {
        match self {
            Self::ImCore(outbox) => Ok(outbox
                .send_runtime_message(message.clone())?
                .message
                .id
                .as_str()
                .to_string()),
            Self::Mock => Ok(format!("mock-message-{}", message.security.as_str())),
            #[cfg(test)]
            Self::Recording(recorder) => recorder.record_message(message),
        }
    }

    fn send_runtime_attachment(
        &self,
        recipient_did: &str,
        attachment: RuntimeAttachmentSend,
    ) -> Result<RuntimeAttachmentSendResult> {
        match self {
            Self::ImCore(outbox) => {
                let result = outbox.send_attachment(recipient_did, attachment.clone())?;
                Ok(RuntimeAttachmentSendResult {
                    message_id: Some(result.message.message.id.as_str().to_string()),
                    target: attachment.target,
                    display_filename: attachment.display_filename,
                    size_bytes: Some(result.attachment.size_bytes),
                    agent_did: String::new(),
                })
            }
            Self::Mock => Ok(RuntimeAttachmentSendResult {
                message_id: Some("mock-attachment".to_string()),
                target: attachment.target,
                display_filename: attachment.display_filename,
                size_bytes: std::fs::metadata(&attachment.file_path)
                    .ok()
                    .map(|metadata| metadata.len()),
                agent_did: String::new(),
            }),
            #[cfg(test)]
            Self::Recording(recorder) => recorder.record_attachment(recipient_did, attachment),
        }
    }

    fn resolve_recipient_did(&self, recipient: &str) -> Result<Option<String>> {
        match self {
            Self::ImCore(outbox) => outbox.resolve_handle(recipient),
            Self::Mock => {
                let recipient = recipient.trim();
                if recipient.starts_with("did:") {
                    Ok(Some(recipient.to_string()))
                } else {
                    Ok(None)
                }
            }
            #[cfg(test)]
            Self::Recording(recorder) => recorder.resolve_recipient_did(recipient),
        }
    }
}

#[cfg(test)]
#[derive(Debug, Clone)]
struct ControllerOutboxRecorder {
    sender_id: String,
    calls: Arc<Mutex<Vec<ControllerOutboxCall>>>,
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
struct ControllerOutboxCall {
    sender_id: String,
    kind: &'static str,
    recipient_did: String,
    state: Option<String>,
    text: Option<String>,
    security: Option<RuntimeMessageSecurity>,
    payload: Option<Value>,
}

#[cfg(test)]
impl ControllerOutboxRecorder {
    fn new(sender_id: impl Into<String>, calls: Arc<Mutex<Vec<ControllerOutboxCall>>>) -> Self {
        Self {
            sender_id: sender_id.into(),
            calls,
        }
    }

    fn record_payload(&self, recipient_did: &str, payload: Value) -> Result<String> {
        self.push(
            "payload",
            recipient_did,
            payload
                .get("state")
                .and_then(Value::as_str)
                .map(str::to_string),
            payload
                .get("message")
                .and_then(Value::as_str)
                .map(str::to_string),
            None,
            Some(payload),
        )
    }

    fn record_message(&self, message: &RuntimeMessageSend) -> Result<String> {
        self.push(
            "message",
            message.resolved_recipient(),
            None,
            Some(message.text.clone()),
            Some(message.security),
            None,
        )
    }

    fn record_attachment(
        &self,
        recipient_did: &str,
        attachment: RuntimeAttachmentSend,
    ) -> Result<RuntimeAttachmentSendResult> {
        let message_id = self.push(
            "attachment",
            recipient_did,
            None,
            attachment.caption.clone(),
            Some(RuntimeMessageSecurity::DefaultPlain),
            None,
        )?;
        Ok(RuntimeAttachmentSendResult {
            message_id: Some(message_id),
            target: attachment.target,
            display_filename: attachment.display_filename,
            size_bytes: std::fs::metadata(&attachment.file_path)
                .ok()
                .map(|metadata| metadata.len()),
            agent_did: String::new(),
        })
    }

    fn resolve_recipient_did(&self, recipient: &str) -> Result<Option<String>> {
        let recipient = recipient.trim();
        if recipient.starts_with("did:") {
            Ok(Some(recipient.to_string()))
        } else {
            Ok(None)
        }
    }

    fn push(
        &self,
        kind: &'static str,
        recipient_did: &str,
        state: Option<String>,
        text: Option<String>,
        security: Option<RuntimeMessageSecurity>,
        payload: Option<Value>,
    ) -> Result<String> {
        let mut calls = self
            .calls
            .lock()
            .map_err(|_| anyhow::anyhow!("controller outbox recorder lock poisoned"))?;
        let next = calls.len() + 1;
        calls.push(ControllerOutboxCall {
            sender_id: self.sender_id.clone(),
            kind,
            recipient_did: recipient_did.to_string(),
            state,
            text,
            security,
            payload,
        });
        Ok(format!("recording-{}-{kind}-{next}", self.sender_id))
    }
}

fn outbox_sender_for_agent(
    config: &DaemonConfig,
    state: &DaemonState,
    im_core: &ImCoreAdapter,
    agent_did: &str,
) -> Result<ControllerOutboxSender> {
    let identity = state.load_agent_identity(agent_did)?;
    let jwt_token = state.load_agent_auth_token(agent_did)?;
    let client = im_core.client_for_agent_identity(config, &identity, jwt_token.as_deref())?;
    Ok(ControllerOutboxSender::ImCore(ImCoreAgentOutbox::new(
        client,
    )))
}

fn runtime_message_sender_for_agent(
    config: &DaemonConfig,
    state: &DaemonState,
    im_core: &ImCoreAdapter,
    runtime_agent_did: &str,
) -> Result<ControllerOutboxSender> {
    outbox_sender_for_agent(config, state, im_core, runtime_agent_did)
        .with_context(|| format!("load runtime message sender for {runtime_agent_did}"))
}

struct RuntimeStatusSender {
    daemon_agent_did: String,
    sender: ControllerOutboxSender,
}

fn runtime_status_sender_for_agent(
    config: &DaemonConfig,
    state: &DaemonState,
    im_core: &ImCoreAdapter,
    runtime_agent_did: &str,
) -> Result<RuntimeStatusSender> {
    let binding = state
        .load_runtime_daemon_binding(runtime_agent_did)?
        .with_context(|| format!("runtime daemon binding missing for {runtime_agent_did}"))?;
    let sender = outbox_sender_for_agent(config, state, im_core, &binding.daemon_agent_did)
        .with_context(|| {
            format!(
                "load daemon status sender {} for runtime {runtime_agent_did}",
                binding.daemon_agent_did
            )
        })?;
    Ok(RuntimeStatusSender {
        daemon_agent_did: binding.daemon_agent_did,
        sender,
    })
}

impl RuntimeOutbox for ControllerRuntimeOutbox {
    fn resolve_recipient_did(
        &self,
        _context: &crate::state::AuthorizedRuntimeContext,
        recipient: &str,
    ) -> Result<Option<String>> {
        self.message_sender.resolve_recipient_did(recipient)
    }

    fn send_status(
        &self,
        context: &crate::state::AuthorizedRuntimeContext,
        state: &str,
        text: Option<&str>,
    ) -> Result<()> {
        self.send_status_with_detail(context, state, text, None, None)
    }

    fn send_status_with_detail(
        &self,
        context: &crate::state::AuthorizedRuntimeContext,
        state: &str,
        text: Option<&str>,
        last_error_code: Option<&str>,
        last_error_summary: Option<&str>,
    ) -> Result<()> {
        let sent_at = time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string());
        self.send_status_payload(
            &self.recipient_did,
            json!({
                "schema": "awiki.agent.status.v1",
                "event_id": format!("evt_{}", crate::security::runtime_token::current_time_millis().unwrap_or(0)),
                "sent_at": sent_at,
                "daemon_agent_did": self.daemon_agent_did.clone(),
                "status_scope": "run",
                "task_id": self.task_id.clone(),
                "run_id": context.run_id.clone(),
                "conversation_id": self.conversation_id.clone(),
                "state": state,
                "message": text,
                "daemon": null,
                "runtimes": [],
                "runs": [{
                    "run_id": context.run_id.clone(),
                    "message_id": self.task_id.clone(),
                    "runtime_agent_did": context.agent_did.clone(),
                    "conversation_id": self.conversation_id.clone(),
                    "status": state,
                    "started_at": sent_at,
                    "updated_at": sent_at,
                    "last_error_code": last_error_code,
                    "last_error_summary": last_error_summary,
                }],
            }),
        )
    }

    fn send_final(
        &self,
        context: &crate::state::AuthorizedRuntimeContext,
        text: Option<&str>,
    ) -> Result<()> {
        let sent_at = time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string());
        self.send_status_payload(
            &self.recipient_did,
            json!({
                "schema": "awiki.agent.status.v1",
                "event_id": format!("evt_{}", crate::security::runtime_token::current_time_millis().unwrap_or(0)),
                "sent_at": sent_at,
                "daemon_agent_did": self.daemon_agent_did.clone(),
                "status_scope": "run",
                "task_id": self.task_id.clone(),
                "run_id": context.run_id.clone(),
                "conversation_id": self.conversation_id.clone(),
                "state": "finished",
                "message": text,
                "daemon": null,
                "runtimes": [],
                "runs": [{
                    "run_id": context.run_id.clone(),
                    "message_id": self.task_id.clone(),
                    "runtime_agent_did": context.agent_did.clone(),
                    "conversation_id": self.conversation_id.clone(),
                    "status": "finished",
                    "started_at": sent_at,
                    "updated_at": sent_at,
                    "last_error_code": null,
                    "last_error_summary": null,
                }],
                "result": {
                    "type": "text",
                    "content": text.unwrap_or_default(),
                },
            }),
        )
    }

    fn send_message(
        &self,
        _context: &crate::state::AuthorizedRuntimeContext,
        message: &RuntimeMessageSend,
    ) -> Result<RuntimeMessageSendResult> {
        let message_id = self.message_sender.send_runtime_message(message)?;
        Ok(RuntimeMessageSendResult {
            message_id: Some(message_id),
            raw_recipient: message.raw_recipient().to_string(),
            resolved_did: message.resolved_recipient().to_string(),
            target_kind: message.target_kind().to_string(),
            security: message.security,
        })
    }

    fn send_attachment(
        &self,
        context: &crate::state::AuthorizedRuntimeContext,
        attachment: &RuntimeAttachmentSend,
    ) -> Result<RuntimeAttachmentSendResult> {
        let recipient_did = attachment
            .target_did
            .as_deref()
            .unwrap_or(self.recipient_did.as_str());
        let mut result = self
            .message_sender
            .send_runtime_attachment(recipient_did, attachment.clone())?;
        result.agent_did = context.agent_did.clone();
        self.sent_counter
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if let Some(message_id) = result.message_id.as_ref() {
            if let Ok(mut ids) = self.sent_message_ids.lock() {
                ids.push(message_id.clone());
            }
        }
        Ok(result)
    }
}

#[derive(Clone)]
struct RuntimeCallbackOutbox {
    config: DaemonConfig,
    state: DaemonState,
    im_core: ImCoreAdapter,
    sent_counter: Arc<std::sync::atomic::AtomicUsize>,
    sent_message_ids: Arc<Mutex<Vec<String>>>,
    mock_status_outbox: bool,
}

impl RuntimeCallbackOutbox {
    fn new(
        config: DaemonConfig,
        state: DaemonState,
        im_core: ImCoreAdapter,
        mock_status_outbox: bool,
    ) -> Self {
        Self {
            config,
            state,
            im_core,
            sent_counter: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            sent_message_ids: Arc::new(Mutex::new(Vec::new())),
            mock_status_outbox,
        }
    }

    fn sent_messages(&self) -> usize {
        self.sent_counter.load(std::sync::atomic::Ordering::Relaxed)
    }

    fn status_message_ids(&self) -> Vec<String> {
        self.sent_message_ids
            .lock()
            .map(|ids| ids.clone())
            .unwrap_or_default()
    }

    fn controller_outbox(
        &self,
        context: &crate::state::AuthorizedRuntimeContext,
    ) -> Result<ControllerRuntimeOutbox> {
        let task = self.state.load_runtime_task_for_run(&context.run_id)?;
        let (message_sender, daemon_agent_did, status_sender) = if self.mock_status_outbox {
            (
                ControllerOutboxSender::Mock,
                None,
                ControllerOutboxSender::Mock,
            )
        } else {
            let status_sender = runtime_status_sender_for_agent(
                &self.config,
                &self.state,
                &self.im_core,
                &context.agent_did,
            )?;
            (
                runtime_message_sender_for_agent(
                    &self.config,
                    &self.state,
                    &self.im_core,
                    &context.agent_did,
                )?,
                Some(status_sender.daemon_agent_did),
                status_sender.sender,
            )
        };
        Ok(ControllerRuntimeOutbox::new(
            message_sender,
            status_sender,
            daemon_agent_did,
            task.sender_did,
            task.task_id,
            task.conversation_id,
            Arc::clone(&self.sent_counter),
            Arc::clone(&self.sent_message_ids),
        ))
    }
}

impl RuntimeOutbox for RuntimeCallbackOutbox {
    fn resolve_recipient_did(
        &self,
        context: &crate::state::AuthorizedRuntimeContext,
        recipient: &str,
    ) -> Result<Option<String>> {
        let recipient = recipient.trim();
        if recipient.starts_with("did:") {
            return Ok(Some(recipient.to_string()));
        }
        if self.mock_status_outbox {
            return Ok(None);
        }
        let message_sender = runtime_message_sender_for_agent(
            &self.config,
            &self.state,
            &self.im_core,
            &context.agent_did,
        )?;
        message_sender.resolve_recipient_did(recipient)
    }

    fn send_status(
        &self,
        context: &crate::state::AuthorizedRuntimeContext,
        state: &str,
        text: Option<&str>,
    ) -> Result<()> {
        self.controller_outbox(context)?
            .send_status(context, state, text)
    }

    fn send_final(
        &self,
        context: &crate::state::AuthorizedRuntimeContext,
        text: Option<&str>,
    ) -> Result<()> {
        self.controller_outbox(context)?.send_final(context, text)
    }

    fn send_message(
        &self,
        context: &crate::state::AuthorizedRuntimeContext,
        message: &RuntimeMessageSend,
    ) -> Result<RuntimeMessageSendResult> {
        self.controller_outbox(context)?
            .send_message(context, message)
    }

    fn send_attachment(
        &self,
        context: &crate::state::AuthorizedRuntimeContext,
        attachment: &RuntimeAttachmentSend,
    ) -> Result<RuntimeAttachmentSendResult> {
        self.controller_outbox(context)?
            .send_attachment(context, attachment)
    }
}

impl AgentManagementOutbox for RuntimeCallbackOutbox {
    fn send_agent_status(&self, response: &AgentStatusResponse) -> Result<()> {
        let inner = if self.mock_status_outbox {
            ControllerOutboxSender::Mock
        } else {
            let identity = self.state.load_agent_identity(&response.agent_did)?;
            let jwt_token = self.state.load_agent_auth_token(&response.agent_did)?;
            let client = self.im_core.client_for_agent_identity(
                &self.config,
                &identity,
                jwt_token.as_deref(),
            )?;
            ControllerOutboxSender::ImCore(ImCoreAgentOutbox::new(client))
        };
        let message_id = inner.send_payload(&response.recipient_did, response.payload.clone())?;
        self.sent_counter
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if let Ok(mut ids) = self.sent_message_ids.lock() {
            ids.push(message_id);
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
struct UdsTestRuntimePlugin {
    socket_path: PathBuf,
}

impl UdsTestRuntimePlugin {
    fn new(socket_path: PathBuf) -> Self {
        Self { socket_path }
    }
}

impl RuntimePlugin for UdsTestRuntimePlugin {
    fn plugin_id(&self) -> &str {
        "test-runtime-uds"
    }

    fn check_install_status(&self) -> Result<RuntimeInstallStatus> {
        Ok(RuntimeInstallStatus {
            installed: true,
            detail: Some("test runtime uses daemon UDS local RPC".to_string()),
        })
    }

    fn launch_run(&self, context: RuntimeLaunchContext) -> Result<RuntimeLaunchOutcome> {
        let token = context.runtime_rpc_token.as_str().to_string();
        ensure_runtime_rpc_success(call_uds_once(
            &self.socket_path,
            &CliWrapperRequest::task_status(
                token.clone(),
                context.task.task_id.clone(),
                "running",
                "runtime started",
            )
            .into_rpc_request(),
        )?)?;
        ensure_runtime_rpc_success(call_uds_once(
            &self.socket_path,
            &CliWrapperRequest::task_finish(token, context.task.task_id, "runtime finished")
                .into_rpc_request(),
        )?)?;
        Ok(RuntimeLaunchOutcome {
            run_id: context.run.run_id,
            status: RuntimeRunStatus::Finished,
            exit_code: Some(0),
            callbacks: Vec::new(),
            metadata: serde_json::json!({}),
        })
    }
}

fn ensure_runtime_rpc_success(response: crate::local_rpc::RuntimeRpcResponse) -> Result<()> {
    if response.ok {
        return Ok(());
    }
    let message = response
        .error
        .map(|error| format!("{}: {}", error.code, error.message))
        .unwrap_or_else(|| "runtime RPC returned ok=false".to_string());
    bail!(message)
}

fn conversation_id(message: &Message) -> Option<String> {
    match &message.thread {
        ThreadRef::Direct(peer) => Some(format!("direct:{}", peer.as_str())),
        ThreadRef::Group(group) => Some(format!("group:{}", group.as_str())),
        ThreadRef::Thread(thread) => Some(thread.as_str().to_string()),
    }
}

fn runtime_callback_outbox(
    config: &DaemonConfig,
    state: &DaemonState,
    im_core: &ImCoreAdapter,
    mock_status_outbox: bool,
) -> Result<Arc<Mutex<RuntimeCallbackOutbox>>> {
    if state.list_agent_definitions()?.is_empty() {
        bail!("foreground requires at least one configured agent identity");
    }
    Ok(Arc::new(Mutex::new(RuntimeCallbackOutbox::new(
        config.clone(),
        state.clone(),
        im_core.clone(),
        mock_status_outbox,
    ))))
}

fn store_agent_token_for_configured_agents(state: &DaemonState, token: &str) -> Result<()> {
    for agent in state.list_agent_definitions()? {
        state.store_agent_auth_token(&agent.agent_did, token)?;
    }
    Ok(())
}

fn sync_configured_agent_identities(
    config: &DaemonConfig,
    state: &DaemonState,
    im_core: &ImCoreAdapter,
) -> Result<()> {
    for agent in state.list_agent_definitions()? {
        let identity = match state.load_agent_identity(&agent.agent_did) {
            Ok(identity) => identity,
            Err(_) => continue,
        };
        let jwt_token = state.load_agent_auth_token(&agent.agent_did)?;
        let _client = im_core.client_for_agent_identity(config, &identity, jwt_token.as_deref())?;
    }
    Ok(())
}

async fn ensure_agent_messaging_session(client: &im_core::ImClient, agent_did: &str) -> Result<()> {
    match client
        .auth()
        .ensure_session_async(im_core::auth::AuthScope::Messaging)
        .await
    {
        Ok(_) => Ok(()),
        Err(_) => {
            client
                .auth()
                .refresh_session_async()
                .await
                .with_context(|| format!("refresh DID WBA session for agent {agent_did}"))?;
            client
                .auth()
                .ensure_session_async(im_core::auth::AuthScope::Messaging)
                .await
                .with_context(|| format!("ensure messaging session for agent {agent_did}"))?;
            Ok(())
        }
    }
}

#[cfg(unix)]
fn start_runtime_rpc_worker(
    socket_path: PathBuf,
    state: DaemonState,
    outbox: Arc<Mutex<RuntimeCallbackOutbox>>,
) -> Result<RuntimeRpcWorker> {
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let listener = bind_uds_listener(&socket_path)?;
    verify_socket_permissions(&socket_path)?;
    let worker_stop = Arc::clone(&stop);
    let handle = thread::Builder::new()
        .name("awiki-daemon-local-rpc".to_string())
        .spawn(move || {
            while !worker_stop.load(std::sync::atomic::Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        if let Ok(outbox) = outbox.lock() {
                            let _ = handle_uds_stream_with_outbox(&state, &*outbox, stream);
                        }
                    }
                    Err(error) if error.kind() == ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(_) => break,
                }
            }
        })
        .context("spawn daemon local RPC worker")?;
    Ok(RuntimeRpcWorker {
        stop,
        handle: Some(handle),
    })
}

#[cfg(not(unix))]
fn start_runtime_rpc_worker(
    _socket_path: PathBuf,
    _state: DaemonState,
    _outbox: Arc<Mutex<RuntimeCallbackOutbox>>,
) -> Result<RuntimeRpcWorker> {
    bail!("daemon long-running local RPC requires Unix domain sockets")
}

struct RuntimeRpcWorker {
    stop: Arc<std::sync::atomic::AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
}

impl RuntimeRpcWorker {
    fn stop(mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn write_ready_file(path: &Path, status: &crate::DaemonStatus) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(
        path,
        serde_json::to_vec_pretty(&json!({
            "ready": true,
            "state_root": status.state_root,
            "local_socket_path": status.local_socket_path,
            "daemon_schema_version": status.daemon_schema_version,
            "im_core_schema_version": status.im_core_schema_version,
        }))?,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    use anyhow::{bail, Context};
    use im_core::ids::PeerRef;
    use im_core::messages::{DeliveryState, SendMessageResult};

    use super::*;
    use crate::app_bridge::bootstrap::BootstrapProcessOutcome;
    use crate::app_bridge::message_agent::EnsureAppMessageAgentOutcome;
    use crate::app_bridge::message_control::{
        handle_app_control_payload, is_app_control_payload, AppControlOutcome,
        IncomingAppControlPayload,
    };
    use crate::commands::{
        handle_agent_payload_message, setup_daemon_agent, AgentCommandOutcome,
        IncomingAgentPayloadMessage, RuntimeAgentCreateOutcome,
    };
    use crate::outbox::{MemoryRuntimeOutbox, OutboxRecordKind};
    use crate::plugins::hermes::{FakeHermesGateway, AWIKI_SKILLS_VERSION};
    use crate::registration::{
        AgentInventoryClient, AgentLatestStatusUpdateItem, AgentRegistrationClient,
        AgentRegistrationExchangeRequest, AgentRegistrationExchangeResult, ControllerSenderScope,
        DidAuthMaterial, RegistrationToken,
    };
    use crate::runtime::RuntimeAgentProfile;
    use crate::state::HermesProfileRecord;
    use crate::workspace::WorkspaceMode;

    #[derive(Debug, Clone, Default)]
    struct MockRegistrationClient;

    impl AgentRegistrationClient for MockRegistrationClient {
        fn exchange_token(
            &self,
            request: AgentRegistrationExchangeRequest,
        ) -> Result<AgentRegistrationExchangeResult> {
            let did = request
                .did_document
                .get("id")
                .and_then(Value::as_str)
                .context("mock registration did document missing id")?
                .to_string();
            Ok(AgentRegistrationExchangeResult {
                token_id: format!("agtok_{}_{}", request.agent_kind.as_str(), request.handle),
                did,
                user_id: Some(format!("user_{}", request.handle)),
                agent_kind: request.agent_kind,
                controller_user_id: "user-alice".to_string(),
                controller_full_handle: "alice.anpclaw.com".to_string(),
                controller_did: request.controller_did,
                handle: request.handle,
                status: "registered".to_string(),
            })
        }
    }

    impl AgentInventoryClient for MockRegistrationClient {
        fn verify_token(
            &self,
            _token: &RegistrationToken,
        ) -> Result<crate::registration::RegistrationTokenMetadata> {
            anyhow::bail!("verify_token is not used in foreground command tests")
        }

        fn sync_controller_scope(
            &self,
            daemon_agent_did: &str,
            _auth: &DidAuthMaterial,
        ) -> Result<Value> {
            Ok(json!({
                "agent_did": daemon_agent_did,
                "controller_user_id": "user-alice",
                "controller_full_handle": "alice.anpclaw.com",
                "controller_did": "did:human:alice",
                "updated_count": 1,
            }))
        }

        fn verify_controller_sender(
            &self,
            _daemon_agent_did: &str,
            sender_did: &str,
            _auth: &DidAuthMaterial,
        ) -> Result<ControllerSenderScope> {
            if sender_did == "did:human:alice" || sender_did == "did:human:alice-new" {
                Ok(ControllerSenderScope {
                    controller_user_id: "user-alice".to_string(),
                    controller_full_handle: "alice.anpclaw.com".to_string(),
                    controller_did: sender_did.to_string(),
                    sender_did: sender_did.to_string(),
                })
            } else {
                anyhow::bail!("controller_scope_mismatch")
            }
        }

        fn update_latest_status(
            &self,
            _daemon_agent_did: &str,
            _statuses: Vec<AgentLatestStatusUpdateItem>,
            _auth: &DidAuthMaterial,
        ) -> Result<Value> {
            anyhow::bail!("update_latest_status is not used in foreground command tests")
        }

        fn archive_agent(
            &self,
            _daemon_agent_did: &str,
            _agent_did: &str,
            _auth: &DidAuthMaterial,
        ) -> Result<Value> {
            Ok(json!({ "archived": [] }))
        }
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct WelcomeSendCall {
        agent_did: String,
        jwt_token: Option<String>,
        controller_did: String,
        text: String,
        security: RuntimeMessageSecurity,
        delivery: MessageDeliveryOptions,
    }

    #[derive(Debug, Default)]
    struct MockWelcomeSender {
        calls: Mutex<Vec<WelcomeSendCall>>,
        fail_message: Option<String>,
        counter: AtomicUsize,
    }

    impl MockWelcomeSender {
        fn calls(&self) -> Vec<WelcomeSendCall> {
            self.calls
                .lock()
                .expect("welcome sender lock poisoned")
                .clone()
        }
    }

    impl RuntimeWelcomeSender for MockWelcomeSender {
        fn send_welcome(
            &self,
            _config: &DaemonConfig,
            identity: &crate::agent::AgentIdentityRecord,
            jwt_token: Option<&str>,
            controller_did: &str,
            text: &str,
            security: RuntimeMessageSecurity,
            delivery: MessageDeliveryOptions,
        ) -> Result<SendMessageResult> {
            self.calls
                .lock()
                .expect("welcome sender lock poisoned")
                .push(WelcomeSendCall {
                    agent_did: identity.agent_did.clone(),
                    jwt_token: jwt_token.map(str::to_string),
                    controller_did: controller_did.to_string(),
                    text: text.to_string(),
                    security,
                    delivery: delivery.clone(),
                });
            if let Some(message) = self.fail_message.as_deref() {
                bail!("{message}");
            }
            let index = self.counter.fetch_add(1, Ordering::Relaxed) + 1;
            let message_id = delivery
                .idempotency_key
                .clone()
                .unwrap_or_else(|| format!("mock-welcome-{index}"));
            Ok(SendMessageResult {
                message: Message {
                    id: im_core::ids::MessageId::parse(&message_id)?,
                    thread: ThreadRef::Direct(PeerRef::parse(controller_did, "")?),
                    direction: MessageDirection::Outgoing,
                    sender: PeerRef::parse(&identity.agent_did, "")?,
                    receiver: Some(PeerRef::parse(controller_did, "")?),
                    group: None,
                    body: MessageBodyView::Text {
                        text: text.to_string(),
                        kind: im_core::messages::MessageKind::Text,
                    },
                    sent_at: None,
                    received_at: None,
                    metadata: im_core::messages::MessageMetadata::default(),
                },
                delivery: DeliveryState::Accepted,
                warnings: Vec::new(),
            })
        }
    }

    fn fixture() -> (tempfile::TempDir, DaemonConfig, DaemonState) {
        let root = tempfile::tempdir().unwrap();
        let config = DaemonConfig::for_state_root(root.path()).unwrap();
        config.ensure_state_layout().unwrap();
        let state = DaemonState::open(&config).unwrap();
        state.initialize().unwrap();
        (root, config, state)
    }

    fn expect_bootstrap_received(
        outcome: AppControlOutcome,
    ) -> (BootstrapProcessOutcome, EnsureAppMessageAgentOutcome) {
        match outcome {
            AppControlOutcome::BootstrapReceived {
                bootstrap,
                message_agent,
            } => (bootstrap, message_agent),
            other => panic!("expected bootstrap outcome, got {other:?}"),
        }
    }

    fn create_hermes_runtime(
        root: &Path,
        config: &DaemonConfig,
        state: &DaemonState,
    ) -> RuntimeAgentCreateOutcome {
        let registration = MockRegistrationClient;
        let daemon = setup_daemon_agent(
            config,
            state,
            &registration,
            "alice-mac-daemon",
            "did:human:alice",
            RegistrationToken::new("tok_daemon_secret_value").unwrap(),
        )
        .unwrap();
        let outbox = MemoryRuntimeOutbox::default();
        match handle_agent_payload_message(
            config,
            state,
            &registration,
            &outbox,
            IncomingAgentPayloadMessage {
                message_id: "msg_create_hermes_runtime".to_string(),
                conversation_id: Some("conv_create_hermes_runtime".to_string()),
                sender_did: "did:human:alice".to_string(),
                target_agent_did: daemon.agent_did,
                content_type: "application/json".to_string(),
                payload: json!({
                    "schema": "awiki.agent.command.v1",
                    "command_id": "cmd_create_hermes_runtime",
                    "command": "runtime.agent.create",
                    "target_agent_kind": "runtime",
                    "args": {
                        "handle": "@alice-hermes-runtime",
                        "runtime": "hermes",
                        "workspace": root.join("workspace").display().to_string(),
                        "controller_did": "did:human:alice",
                        "registration_token": "tok_runtime_secret_value",
                        "display_name": "Alice Hermes"
                    }
                }),
            },
        )
        .unwrap()
        {
            AgentCommandOutcome::RuntimeAgentCreated(created) => created,
            other => panic!("expected runtime agent create outcome, got {other:?}"),
        }
    }

    fn bootstrap_key_material() -> (String, String) {
        let mut key_bytes = [0_u8; 32];
        key_bytes[0] = 17;
        let private_key =
            crate::app_bridge::secret_store::ed25519_private_key_pem_for_test(&key_bytes);
        let public_key =
            crate::app_bridge::secret_store::public_key_multibase_from_private_material(
                &private_key,
            )
            .unwrap();
        (public_key, private_key)
    }

    fn bootstrap_payload_fixture() -> Value {
        let (public_key, private_key) = bootstrap_key_material();
        json!({
            "schema": "awiki.daemon.bootstrap.v1",
            "bootstrap_id": "boot_1",
            "idempotency_key": "message-agent-bootstrap:did:human:alice:app_1",
            "app_instance_id": "app_1",
            "controller_did": "did:human:alice",
            "user_subkey_package": {
                "schema": "awiki.daemon.user_subkey_package.v2",
                "user_did": "did:human:alice",
                "verification_method": "did:human:alice#daemon-key-1",
                "key_type": "Multikey/Ed25519",
                "key_algorithm": "Ed25519",
                "public_key_multibase": public_key,
                "private_key_encoding": "pem",
                "private_key_pem": private_key,
                "allowed_scopes": [
                    "message.inbox.read.plain",
                    "message.history.read.plain",
                    "message.send.plain"
                ]
            },
            "desired_message_agent": {
                "role": "app_message_handler",
                "runtime": "hermes",
                "display_name": "Hermes Message Agent",
                "ensure_once_key": "app-message-agent:did:human:alice:app_1",
                "runtime_registration_token": "tok_runtime_secret_value"
            },
            "capability_policy": {
                "schema": "awiki.app.capabilities.v1",
                "capabilities": [
                    "message.summarize_plain",
                    "message.create_draft",
                    "contact.read",
                    "contact.update_display_name",
                    "contact.update_note"
                ],
                "require_confirmation_for_write_actions": true
            }
        })
    }

    fn write_bootstrap_did_document_cache(config: &DaemonConfig, payload: &Value) {
        let package = &payload["user_subkey_package"];
        let user_did = package["user_did"].as_str().unwrap();
        let method = package["verification_method"].as_str().unwrap();
        let public_key = package["public_key_multibase"].as_str().unwrap();
        let identity_dir = config.identity_root_dir.join("alice");
        std::fs::create_dir_all(&identity_dir).unwrap();
        std::fs::write(
            identity_dir.join("did.json"),
            serde_json::to_vec_pretty(&json!({
                "id": user_did,
                "verificationMethod": [{
                    "id": method,
                    "type": "Multikey",
                    "controller": user_did,
                    "publicKeyMultibase": public_key
                }],
                "authentication": [method]
            }))
            .unwrap(),
        )
        .unwrap();
        if let Some(parent) = config.identity_registry_path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(
            &config.identity_registry_path,
            serde_json::to_vec_pretty(&json!({
                "default_identity": "alice",
                "identities": [{
                    "id": "alice",
                    "did": user_did,
                    "dir_name": "alice",
                    "local_alias": "alice"
                }]
            }))
            .unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn hermes_runtime_welcome_send_uses_runtime_identity_text_and_idempotency() {
        let (root, config, state) = fixture();
        let created = create_hermes_runtime(root.path(), &config, &state);
        state
            .store_agent_auth_token(&created.agent_did, "jwt-runtime-secret")
            .unwrap();
        let controller_did = controller_did_for_runtime(&state, &created.agent_did).unwrap();
        let idempotency_key = welcome_idempotency_key(&created.agent_did, &controller_did);
        let sender = MockWelcomeSender::default();

        try_send_runtime_agent_welcome_message(
            &config,
            &state,
            &sender,
            &created,
            &controller_did,
            &idempotency_key,
        )
        .unwrap();

        let calls = sender.calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].agent_did, created.agent_did);
        assert_eq!(calls[0].jwt_token.as_deref(), Some("jwt-runtime-secret"));
        assert_eq!(calls[0].controller_did, "did:human:alice");
        assert_eq!(calls[0].text, "Hermes 已准备好。");
        assert_eq!(calls[0].security, RuntimeMessageSecurity::DefaultPlain);
        assert_eq!(
            calls[0].delivery.idempotency_key.as_deref(),
            Some(idempotency_key.as_str())
        );
        assert!(!calls[0].delivery.wait_for_final_acceptance);
        assert!(state
            .audit_event_exists(
                "runtime.welcome.sent",
                Some(&created.agent_did),
                Some(&idempotency_key),
            )
            .unwrap());

        send_runtime_agent_welcome_message_with_sender(&config, &state, &sender, &created).unwrap();
        assert_eq!(
            sender.calls().len(),
            1,
            "existing sent audit should make welcome delivery idempotent"
        );
    }

    #[test]
    fn hermes_runtime_welcome_failure_is_sanitized_and_non_fatal() {
        let (root, config, state) = fixture();
        let created = create_hermes_runtime(root.path(), &config, &state);
        let controller_did = controller_did_for_runtime(&state, &created.agent_did).unwrap();
        let idempotency_key = welcome_idempotency_key(&created.agent_did, &controller_did);
        let sender = MockWelcomeSender {
            fail_message: Some(
                "failed with jwt secret at /Users/alice/.awiki/private.key".to_string(),
            ),
            ..MockWelcomeSender::default()
        };

        send_runtime_agent_welcome_message_with_sender(&config, &state, &sender, &created).unwrap();

        let connection = rusqlite::Connection::open(&config.daemon_db_path).unwrap();
        let detail_json: String = connection
            .query_row(
                "SELECT COALESCE(detail_json, '') FROM audit_log WHERE event_type = 'runtime.welcome.failed' ORDER BY created_at_ms DESC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(detail_json.contains("welcome_send_failed"));
        assert!(detail_json.contains(&idempotency_key));
        assert!(!detail_json.contains("jwt"));
        assert!(!detail_json.contains("secret"));
        assert!(!detail_json.contains("/Users/alice"));
        assert!(!state
            .audit_event_exists(
                "runtime.welcome.sent",
                Some(&created.agent_did),
                Some(&idempotency_key),
            )
            .unwrap());
    }

    #[test]
    fn controller_runtime_outbox_splits_runtime_messages_from_daemon_status() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let runtime_sender = ControllerOutboxSender::Recording(ControllerOutboxRecorder::new(
            "runtime",
            Arc::clone(&calls),
        ));
        let daemon_sender = ControllerOutboxSender::Recording(ControllerOutboxRecorder::new(
            "daemon",
            Arc::clone(&calls),
        ));
        let sent_counter = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let sent_message_ids = Arc::new(Mutex::new(Vec::new()));
        let outbox = ControllerRuntimeOutbox::new(
            runtime_sender,
            daemon_sender,
            Some("did:agent:daemon-control".to_string()),
            "did:human:alice",
            "task_identity_split",
            Some("direct:did:human:alice".to_string()),
            Arc::clone(&sent_counter),
            Arc::clone(&sent_message_ids),
        );
        let context = crate::state::AuthorizedRuntimeContext {
            token_id: "rtok_identity_split".to_string(),
            agent_did: "did:agent:runtime-hermes".to_string(),
            runtime_profile_id: "profile_identity_split".to_string(),
            run_id: "run_identity_split".to_string(),
            method: crate::security::runtime_token::RpcMethod::TaskStatus,
        };
        let attachment_path = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(attachment_path.path(), b"attachment").unwrap();

        outbox
            .send_status(&context, "succeeded", Some("Hermes response sent"))
            .unwrap();
        outbox.send_final(&context, Some("legacy final")).unwrap();
        outbox
            .send_message(
                &context,
                &RuntimeMessageSend {
                    target: crate::outbox::RuntimeMessageTarget::Direct {
                        recipient: "did:human:alice".to_string(),
                        raw_recipient: "did:human:alice".to_string(),
                        resolved_did: Some("did:human:alice".to_string()),
                    },
                    text: "Hermes 已准备好。".to_string(),
                    file_path: None,
                    display_filename: None,
                    mime_type: None,
                    idempotency_key: None,
                    security: RuntimeMessageSecurity::DefaultPlain,
                },
            )
            .unwrap();
        outbox
            .send_attachment(
                &context,
                &RuntimeAttachmentSend {
                    target: "current_conversation".to_string(),
                    target_did: Some("did:human:alice".to_string()),
                    file_path: attachment_path.path().to_path_buf(),
                    display_filename: Some("report.txt".to_string()),
                    caption: Some("report".to_string()),
                },
            )
            .unwrap();
        let resolved = outbox
            .resolve_recipient_did(&context, "did:human:alice")
            .unwrap();

        assert_eq!(resolved.as_deref(), Some("did:human:alice"));
        assert_eq!(sent_counter.load(Ordering::Relaxed), 3);
        let calls = calls.lock().expect("recorded calls lock poisoned").clone();
        assert_eq!(calls.len(), 4);
        assert_eq!(calls[0].sender_id, "daemon");
        assert_eq!(calls[0].kind, "payload");
        assert_eq!(calls[0].state.as_deref(), Some("succeeded"));
        assert_eq!(calls[0].recipient_did, "did:human:alice");
        let run_status_payload = calls[0].payload.as_ref().expect("status payload recorded");
        assert_eq!(run_status_payload["schema"], "awiki.agent.status.v1");
        assert_eq!(run_status_payload["status_scope"], "run");
        assert_eq!(
            run_status_payload["daemon_agent_did"],
            "did:agent:daemon-control"
        );
        assert_eq!(
            run_status_payload["conversation_id"],
            "direct:did:human:alice"
        );
        assert_eq!(run_status_payload["daemon"], Value::Null);
        assert_eq!(run_status_payload["runtimes"], json!([]));
        assert_eq!(
            run_status_payload["runs"][0]["runtime_agent_did"],
            "did:agent:runtime-hermes"
        );
        assert_eq!(
            run_status_payload["runs"][0]["conversation_id"],
            "direct:did:human:alice"
        );
        assert_eq!(run_status_payload["runs"][0]["status"], "succeeded");
        assert_eq!(
            run_status_payload["runs"][0]["last_error_code"],
            Value::Null
        );
        assert_eq!(calls[1].sender_id, "daemon");
        assert_eq!(calls[1].kind, "payload");
        assert_eq!(calls[1].state.as_deref(), Some("finished"));
        let final_payload = calls[1].payload.as_ref().expect("final payload recorded");
        assert_eq!(final_payload["schema"], "awiki.agent.status.v1");
        assert_eq!(final_payload["status_scope"], "run");
        assert_eq!(
            final_payload["daemon_agent_did"],
            "did:agent:daemon-control"
        );
        assert_eq!(final_payload["runs"][0]["status"], "finished");
        assert_eq!(calls[2].sender_id, "runtime");
        assert_eq!(calls[2].kind, "message");
        assert_eq!(calls[2].text.as_deref(), Some("Hermes 已准备好。"));
        assert_eq!(
            calls[2].security,
            Some(RuntimeMessageSecurity::DefaultPlain)
        );
        assert_eq!(calls[3].sender_id, "runtime");
        assert_eq!(calls[3].kind, "attachment");
        assert_eq!(calls[3].recipient_did, "did:human:alice");
        assert_eq!(calls[3].text.as_deref(), Some("report"));
        let message_ids = sent_message_ids
            .lock()
            .expect("sent ids lock poisoned")
            .clone();
        assert_eq!(message_ids.len(), 3);
        assert!(message_ids[0].contains("daemon-payload"));
        assert!(message_ids[1].contains("daemon-payload"));
        assert!(message_ids[2].contains("runtime-attachment"));
    }

    fn profile(root: &Path) -> RuntimeAgentProfile {
        RuntimeAgentProfile {
            agent_did: "did:agent:hermes".to_string(),
            controller_user_id: "user-alice".to_string(),
            controller_full_handle: "alice.anpclaw.com".to_string(),
            controller_scope_key: "controller-scope:v1:test-alice-anpclaw-com".to_string(),
            controller_did: "did:human:alice".to_string(),
            runtime_profile_id: "profile_hermes_alice".to_string(),
            runtime_plugin_id: HERMES_RUNTIME_PLUGIN_ID.to_string(),
            display_name: Some("Alice Hermes".to_string()),
            workspace_id: Some("workspace_hermes".to_string()),
            workspace_root: Some(root.join("workspace")),
            workspace_mode: Some(WorkspaceMode::SharedRoot),
        }
    }

    fn hermes_record(root: &Path) -> HermesProfileRecord {
        HermesProfileRecord {
            agent_did: "did:agent:hermes".to_string(),
            runtime_profile_id: "profile_hermes_alice".to_string(),
            hermes_profile: "awiki_alice_hermes".to_string(),
            hermes_home: root.join("runtime/hermes/profile"),
            hermes_version: None,
            awiki_skills_version: AWIKI_SKILLS_VERSION.to_string(),
            status: "ready".to_string(),
        }
    }

    #[test]
    fn hermes_foreground_runtime_route_uses_hermes_plugin_and_persists_session() {
        let (root, config, state) = fixture();
        let profile = profile(root.path());
        state.upsert_runtime_agent_profile(&profile).unwrap();
        state
            .upsert_hermes_profile(&hermes_record(root.path()))
            .unwrap();
        let outbox = MemoryRuntimeOutbox::default();
        let gateway = FakeHermesGateway::default();

        let result = run_runtime_text_message_with_gateway(
            &config,
            &state,
            &outbox,
            ControllerTextMessage {
                message_id: "msg_foreground_hermes".to_string(),
                conversation_id: Some("direct:did:human:alice".to_string()),
                sender_did: "did:human:alice".to_string(),
                target_agent_did: "did:agent:hermes".to_string(),
                text: "foreground route to Hermes".to_string(),
            },
            || gateway.clone(),
        )
        .unwrap();

        assert_eq!(result.launch_outcome.status, RuntimeRunStatus::Running);
        assert_eq!(gateway.created_sessions().len(), 1);
        assert_eq!(gateway.submitted_prompts().len(), 1);
        assert!(
            state
                .count_active_hermes_sessions_for_agent("did:agent:hermes")
                .unwrap()
                >= 1
        );
        let records = outbox.records();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].kind, OutboxRecordKind::Message);
        assert_eq!(records[0].text.as_deref(), Some("fake complete"));
        assert_eq!(records[1].kind, OutboxRecordKind::Status);
        assert_eq!(records[1].state.as_deref(), Some("succeeded"));
    }

    #[test]
    fn hermes_foreground_runtime_route_accepts_verified_rotated_controller_did() {
        let (root, config, state) = fixture();
        let profile = profile(root.path());
        state.upsert_runtime_agent_profile(&profile).unwrap();
        state
            .upsert_hermes_profile(&hermes_record(root.path()))
            .unwrap();
        let outbox = MemoryRuntimeOutbox::default();
        let gateway = FakeHermesGateway::default();

        let result = run_runtime_text_message_with_gateway(
            &config,
            &state,
            &outbox,
            ControllerTextMessage {
                message_id: "msg_foreground_rotated_controller".to_string(),
                conversation_id: Some("direct:did:human:alice-new".to_string()),
                sender_did: "did:human:alice-new".to_string(),
                target_agent_did: "did:agent:hermes".to_string(),
                text: "rotated controller foreground route".to_string(),
            },
            || gateway.clone(),
        )
        .unwrap();

        assert_eq!(result.launch_outcome.status, RuntimeRunStatus::Running);
        assert_eq!(gateway.submitted_prompts().len(), 1);
        assert_eq!(
            outbox.records()[0].recipient.as_deref(),
            Some("did:human:alice-new")
        );
    }

    #[test]
    fn foreground_controller_scope_verification_rejects_unowned_sender_before_gateway() {
        let (root, config, state) = fixture();
        let created = create_hermes_runtime(root.path(), &config, &state);
        let registration = MockRegistrationClient;

        let verified = verify_runtime_controller_sender(
            &config,
            &state,
            &registration,
            &created.agent_did,
            "did:human:alice-new",
        )
        .unwrap();
        assert_eq!(verified.controller_did, "did:human:alice-new");

        let error = verify_runtime_controller_sender(
            &config,
            &state,
            &registration,
            &created.agent_did,
            "did:human:bob",
        )
        .unwrap_err();
        assert!(error.to_string().contains("controller_scope_mismatch"));
    }

    #[test]
    fn generic_cli_foreground_route_uses_cli_profile_registry_not_test_fallback() {
        let (root, config, state) = fixture();
        let mut profile = profile(root.path());
        profile.agent_did = "did:agent:generic-cli".to_string();
        profile.runtime_profile_id = "profile_generic_cli_foreground".to_string();
        profile.runtime_plugin_id = GENERIC_CLI_RUNTIME_PLUGIN_ID.to_string();
        profile.display_name = Some("Alice Generic CLI".to_string());
        state.upsert_runtime_agent_profile(&profile).unwrap();
        let mut cli_profile =
            crate::state::CliRuntimeProfileRecord::for_driver(&profile.runtime_profile_id, "codex")
                .unwrap();
        cli_profile.binary_path = Some(root.path().join("missing-codex"));
        state.upsert_cli_runtime_profile(&cli_profile).unwrap();
        let outbox = MemoryRuntimeOutbox::default();
        let gateway = FakeHermesGateway::default();

        let error = run_runtime_text_message_with_gateway(
            &config,
            &state,
            &outbox,
            ControllerTextMessage {
                message_id: "msg_foreground_generic_cli".to_string(),
                conversation_id: Some("direct:did:human:alice".to_string()),
                sender_did: "did:human:alice".to_string(),
                target_agent_did: "did:agent:generic-cli".to_string(),
                text: "foreground route to generic cli".to_string(),
            },
            || gateway.clone(),
        )
        .unwrap_err();

        assert!(error.to_string().contains("generic-cli"));
        assert!(error.to_string().contains("not installed"));
        assert!(gateway.created_sessions().is_empty());
        assert!(outbox.records().is_empty());
    }

    #[test]
    fn conversation_id_projects_direct_peer_without_message_content() {
        let message = Message {
            id: im_core::ids::MessageId::parse("msg_foreground").unwrap(),
            thread: ThreadRef::Direct(PeerRef::parse("did:human:alice", "").unwrap()),
            direction: MessageDirection::Incoming,
            sender: PeerRef::parse("did:human:alice", "").unwrap(),
            receiver: Some(PeerRef::parse("did:agent:hermes", "").unwrap()),
            group: None,
            body: MessageBodyView::Text {
                text: "secret prompt text".to_string(),
                kind: im_core::messages::MessageKind::Text,
            },
            sent_at: None,
            received_at: None,
            metadata: im_core::messages::MessageMetadata::default(),
        };

        assert_eq!(
            conversation_id(&message).as_deref(),
            Some("direct:did:human:alice")
        );
    }

    #[test]
    fn daemon_bootstrap_payload_is_system_control_and_persists_state() {
        let (_root, config, state) = fixture();
        let registration = MockRegistrationClient;
        let daemon = setup_daemon_agent(
            &config,
            &state,
            &registration,
            "alice-mac-daemon",
            "did:human:alice",
            RegistrationToken::new("tok_daemon_secret_value").unwrap(),
        )
        .unwrap();
        let payload = bootstrap_payload_fixture();
        write_bootstrap_did_document_cache(&config, &payload);

        assert!(is_app_control_payload(&payload));
        assert!(!is_awiki_agent_command_payload(&payload));
        let outcome = handle_app_control_payload(
            &config,
            &state,
            &registration,
            IncomingAppControlPayload {
                message_id: "msg_bootstrap".to_string(),
                conversation_id: Some("direct:did:agent:daemon".to_string()),
                sender_did: "did:human:alice".to_string(),
                target_agent_did: daemon.agent_did.clone(),
                content_type: "application/json".to_string(),
                payload,
            },
        )
        .unwrap();
        let (bootstrap, message_agent) = expect_bootstrap_received(outcome);
        assert_eq!(bootstrap.status, "paired_key_received");
        assert!(!bootstrap.replayed);
        assert!(message_agent.created_runtime_agent);
        assert_eq!(
            message_agent.binding.binding_id,
            "app-message-agent:did:human:alice:app_1"
        );
        assert_eq!(message_agent.binding.role, "app_message_handler");
        assert_eq!(
            message_agent.binding.inbox_auth_verification_method,
            "did:human:alice#daemon-key-1"
        );
        assert!(!message_agent
            .binding
            .desired_agent_json
            .to_string()
            .contains("tok_runtime_secret_value"));

        let loaded = state
            .load_user_delegated_identity("did:human:alice#daemon-key-1")
            .unwrap()
            .unwrap();
        assert_eq!(loaded.user_did, "did:human:alice");
        assert_eq!(loaded.daemon_agent_did, daemon.agent_did);
        assert_eq!(
            loaded.private_key_material,
            bootstrap_key_material().1.trim()
        );
        assert!(!format!("{loaded:?}").contains("BEGIN PRIVATE KEY"));
        let binding = state
            .load_active_app_message_agent_binding(
                "did:human:alice",
                "app_1",
                "app_message_handler",
            )
            .unwrap()
            .unwrap();
        assert_eq!(
            binding.runtime_agent_did,
            message_agent.binding.runtime_agent_did
        );
        assert_eq!(
            binding.runtime_profile_id,
            message_agent.binding.runtime_profile_id
        );
        assert!(!state
            .audit_event_exists(
                "daemon.inbox.payload.ignored",
                Some(&daemon.agent_did),
                Some("msg_bootstrap"),
            )
            .unwrap());
    }

    #[test]
    fn app_capabilities_and_action_result_are_system_control_payloads() {
        let (_root, config, state) = fixture();
        let registration = MockRegistrationClient;
        let daemon = setup_daemon_agent(
            &config,
            &state,
            &registration,
            "alice-mac-daemon",
            "did:human:alice",
            RegistrationToken::new("tok_daemon_secret_value").unwrap(),
        )
        .unwrap();
        let capabilities_payload = json!({
            "schema": "awiki.app.capabilities.v1",
            "capabilities": ["message.summarize_plain", "contact.update_note"],
            "require_confirmation_for_write_actions": true
        });
        assert!(is_app_control_payload(&capabilities_payload));
        let capabilities = handle_app_control_payload(
            &config,
            &state,
            &registration,
            IncomingAppControlPayload {
                message_id: "msg_app_capabilities".to_string(),
                conversation_id: Some("direct:did:agent:daemon".to_string()),
                sender_did: "did:human:alice".to_string(),
                target_agent_did: daemon.agent_did.clone(),
                content_type: "application/json".to_string(),
                payload: capabilities_payload,
            },
        )
        .unwrap();
        match capabilities {
            AppControlOutcome::CapabilitiesReceived { capabilities } => {
                assert_eq!(
                    capabilities,
                    vec![
                        "message.summarize_plain".to_string(),
                        "contact.update_note".to_string()
                    ]
                );
            }
            other => panic!("expected app capabilities outcome, got {other:?}"),
        }

        let result_payload = json!({
            "schema": "awiki.app.action.result.v1",
            "action_id": "act_draft_1",
            "action": "message.create_draft",
            "state": "succeeded",
            "result": {"draft_text": "Looks good"}
        });
        assert!(is_app_control_payload(&result_payload));
        let result = handle_app_control_payload(
            &config,
            &state,
            &registration,
            IncomingAppControlPayload {
                message_id: "msg_app_action_result".to_string(),
                conversation_id: Some("direct:did:agent:daemon".to_string()),
                sender_did: "did:human:alice".to_string(),
                target_agent_did: daemon.agent_did.clone(),
                content_type: "application/json".to_string(),
                payload: result_payload,
            },
        )
        .unwrap();
        match result {
            AppControlOutcome::ActionResultReceived {
                action_id,
                action,
                state: action_state,
            } => {
                assert_eq!(action_id, "act_draft_1");
                assert_eq!(action, "message.create_draft");
                assert_eq!(action_state, "succeeded");
            }
            other => panic!("expected app action result outcome, got {other:?}"),
        }
        assert!(state
            .audit_event_exists("app.capabilities.received", Some(&daemon.agent_did), None,)
            .unwrap());
        assert!(state
            .audit_event_exists("app.action.result.received", Some(&daemon.agent_did), None,)
            .unwrap());
    }

    #[test]
    fn daemon_bootstrap_replay_reuses_message_agent_without_runtime_token() {
        let (_root, config, state) = fixture();
        let registration = MockRegistrationClient;
        let daemon = setup_daemon_agent(
            &config,
            &state,
            &registration,
            "alice-mac-daemon",
            "did:human:alice",
            RegistrationToken::new("tok_daemon_secret_value").unwrap(),
        )
        .unwrap();
        let first_payload = bootstrap_payload_fixture();
        write_bootstrap_did_document_cache(&config, &first_payload);

        let first = handle_app_control_payload(
            &config,
            &state,
            &registration,
            IncomingAppControlPayload {
                message_id: "msg_bootstrap_first".to_string(),
                conversation_id: Some("direct:did:agent:daemon".to_string()),
                sender_did: "did:human:alice".to_string(),
                target_agent_did: daemon.agent_did.clone(),
                content_type: "application/json".to_string(),
                payload: first_payload,
            },
        )
        .unwrap();
        let (first_bootstrap, first_agent) = expect_bootstrap_received(first);
        assert!(!first_bootstrap.replayed);
        assert!(first_agent.created_runtime_agent);

        let reopened_state = DaemonState::open(&config).unwrap();
        let mut replay_payload = bootstrap_payload_fixture();
        write_bootstrap_did_document_cache(&config, &replay_payload);
        replay_payload["desired_message_agent"]
            .as_object_mut()
            .unwrap()
            .remove("runtime_registration_token");
        let replay = handle_app_control_payload(
            &config,
            &reopened_state,
            &registration,
            IncomingAppControlPayload {
                message_id: "msg_bootstrap_replay".to_string(),
                conversation_id: Some("direct:did:agent:daemon".to_string()),
                sender_did: "did:human:alice".to_string(),
                target_agent_did: daemon.agent_did.clone(),
                content_type: "application/json".to_string(),
                payload: replay_payload,
            },
        )
        .unwrap();
        let (replay_bootstrap, replay_agent) = expect_bootstrap_received(replay);

        assert!(replay_bootstrap.replayed);
        assert!(!replay_agent.created_runtime_agent);
        assert_eq!(
            replay_agent.binding.runtime_agent_did,
            first_agent.binding.runtime_agent_did
        );
        assert_eq!(
            replay_agent.binding.runtime_profile_id,
            first_agent.binding.runtime_profile_id
        );

        let connection = rusqlite::Connection::open(&config.daemon_db_path).unwrap();
        let runtime_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM agent_definition WHERE agent_kind = 'runtime'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(runtime_count, 1);
        let binding_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM app_message_agent_binding",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(binding_count, 1);
        let stored_non_secret_text: String = connection
            .query_row(
                r#"
SELECT
    COALESCE((SELECT GROUP_CONCAT(desired_agent_json || ' ' || capability_policy_json, char(10)) FROM app_message_agent_binding), '')
    || ' ' ||
    COALESCE((SELECT GROUP_CONCAT(outcome_json, char(10)) FROM runtime_agent_create_request), '')
    || ' ' ||
    COALESCE((SELECT GROUP_CONCAT(hermes_profile || ' ' || hermes_home || ' ' || awiki_skills_version, char(10)) FROM hermes_profiles), '')
"#,
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(!stored_non_secret_text.contains("tok_runtime_secret_value"));
        let audit_dump: String = connection
            .query_row(
                "SELECT GROUP_CONCAT(COALESCE(detail_json, ''), '\n') FROM audit_log",
                [],
                |row| row.get::<_, Option<String>>(0),
            )
            .unwrap()
            .unwrap_or_default();
        assert!(!audit_dump.contains("tok_runtime_secret_value"));
    }

    #[test]
    fn app_message_agent_runtime_token_scope_is_limited_to_bound_user() {
        let (_root, config, state) = fixture();
        let registration = MockRegistrationClient;
        let daemon = setup_daemon_agent(
            &config,
            &state,
            &registration,
            "alice-mac-daemon",
            "did:human:alice",
            RegistrationToken::new("tok_daemon_secret_value").unwrap(),
        )
        .unwrap();
        let payload = bootstrap_payload_fixture();
        write_bootstrap_did_document_cache(&config, &payload);
        let outcome = handle_app_control_payload(
            &config,
            &state,
            &registration,
            IncomingAppControlPayload {
                message_id: "msg_bootstrap_scope".to_string(),
                conversation_id: Some("direct:did:agent:daemon".to_string()),
                sender_did: "did:human:alice".to_string(),
                target_agent_did: daemon.agent_did,
                content_type: "application/json".to_string(),
                payload,
            },
        )
        .unwrap();
        let (_bootstrap, message_agent) = expect_bootstrap_received(outcome);

        let outbox = MemoryRuntimeOutbox::default();
        let gateway = FakeHermesGateway::default();
        let result = run_runtime_text_message_with_gateway(
            &config,
            &state,
            &outbox,
            ControllerTextMessage {
                message_id: "msg_scope".to_string(),
                conversation_id: Some("direct:did:human:alice".to_string()),
                sender_did: "did:human:alice".to_string(),
                target_agent_did: message_agent.binding.runtime_agent_did.clone(),
                text: "message handler task".to_string(),
            },
            || gateway.clone(),
        )
        .unwrap();

        let connection = rusqlite::Connection::open(&config.daemon_db_path).unwrap();
        let (allowed_recipients_json, allowed_security_json): (String, String) = connection
            .query_row(
                r#"
SELECT COALESCE(allowed_recipients_json, ''), COALESCE(allowed_message_security_json, '')
FROM runtime_rpc_tokens
WHERE token_id = ?1
"#,
                [&result.token_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        let allowed_recipients: Vec<String> =
            serde_json::from_str(&allowed_recipients_json).unwrap();
        let allowed_security: Vec<String> = serde_json::from_str(&allowed_security_json).unwrap();
        assert_eq!(allowed_recipients, vec!["did:human:alice".to_string()]);
        assert_eq!(allowed_security, vec!["default_plain".to_string()]);
        assert!(!allowed_recipients_json.contains("@active_handle_lookup"));
        assert!(!allowed_recipients_json.contains("@any_group"));
    }

    #[test]
    fn attachment_manifest_payload_is_ignored_without_auditing_content() {
        let (_root, config, state) = fixture();
        let payload = json!({
            "schema": "anp.attachment.manifest.v1",
            "caption": "secret caption from controller",
            "attachments": [{
                "attachment_id": "att_secret_manifest",
                "filename": "secret-plan.md",
                "mime_type": "text/markdown",
                "size_bytes": 42
            }]
        });
        let message = Message {
            id: im_core::ids::MessageId::parse("msg_attachment_manifest").unwrap(),
            thread: ThreadRef::Direct(PeerRef::parse("did:human:alice", "").unwrap()),
            direction: MessageDirection::Incoming,
            sender: PeerRef::parse("did:human:alice", "").unwrap(),
            receiver: Some(PeerRef::parse("did:agent:hermes", "").unwrap()),
            group: None,
            body: MessageBodyView::Payload {
                payload: payload.clone(),
            },
            sent_at: None,
            received_at: None,
            metadata: im_core::messages::MessageMetadata {
                content_type: Some(
                    im_core::attachments::attachment_manifest_content_type().to_string(),
                ),
                ..im_core::messages::MessageMetadata::default()
            },
        };
        let content_type = message
            .metadata
            .content_type
            .as_deref()
            .expect("fixture content type");

        assert!(!is_awiki_agent_command_payload(&payload));
        record_ignored_non_command_payload(
            &state,
            "did:agent:hermes",
            &message,
            content_type,
            &payload,
        )
        .unwrap();

        let connection = rusqlite::Connection::open(&config.daemon_db_path).unwrap();
        let detail_json: String = connection
            .query_row(
                "SELECT COALESCE(detail_json, '') FROM audit_log WHERE event_type = 'daemon.inbox.payload.ignored' ORDER BY created_at_ms DESC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(detail_json.contains("not_awiki_agent_command"));
        assert!(detail_json.contains("msg_attachment_manifest"));
        assert!(detail_json.contains("did:human:alice"));
        assert!(detail_json.contains("did:agent:hermes"));
        assert!(detail_json.contains(im_core::attachments::attachment_manifest_content_type()));
        assert!(detail_json.contains("anp.attachment.manifest.v1"));
        assert!(!detail_json.contains("secret caption"));
        assert!(!detail_json.contains("secret-plan.md"));
        assert!(!detail_json.contains("att_secret_manifest"));
    }

    #[test]
    fn attachment_runtime_prompt_lists_paths_without_requesting_auto_read() {
        let prompt = render_attachment_runtime_prompt(
            "读取我发给你的文件，看看说的什么内容。",
            &[RuntimeInboundAttachment {
                attachment_id: "att_1".to_string(),
                filename: "notes.md".to_string(),
                mime_type: "text/markdown".to_string(),
                size: "42".to_string(),
                size_bytes: Some(42),
                local_path: Some(PathBuf::from(
                    "/tmp/awiki-state/runtime-attachments/agent/msg/att/notes.md",
                )),
                download_status: "downloaded".to_string(),
                error: None,
            }],
        );

        assert!(prompt.contains("控制者消息:"));
        assert!(prompt.contains("读取我发给你的文件"));
        assert!(prompt.contains("attachment_id: att_1"));
        assert!(prompt.contains("filename: notes.md"));
        assert!(prompt.contains("mime_type: text/markdown"));
        assert!(prompt
            .contains("local_path: /tmp/awiki-state/runtime-attachments/agent/msg/att/notes.md"));
        assert!(prompt.contains("只有当控制者消息或会话上下文表明需要使用文件时"));
        assert!(!prompt.contains("content:"));
        assert!(!prompt.contains("```"));
    }

    #[test]
    fn pure_attachment_runtime_prompt_has_empty_controller_message() {
        let prompt = render_attachment_runtime_prompt(
            "",
            &[RuntimeInboundAttachment {
                attachment_id: "att_only".to_string(),
                filename: "image.png".to_string(),
                mime_type: "image/png".to_string(),
                size: "1024".to_string(),
                size_bytes: Some(1024),
                local_path: Some(PathBuf::from("/tmp/awiki-state/image.png")),
                download_status: "downloaded".to_string(),
                error: None,
            }],
        );

        assert!(prompt.contains("控制者消息:\n（控制者只发送了附件，没有输入文本消息。）"));
        assert!(prompt.contains("filename: image.png"));
        assert!(prompt.contains("mime_type: image/png"));
        assert!(prompt.contains("附件处理规则："));
        assert!(!prompt.contains("Controller message:"));
        assert!(!prompt.contains("<empty>"));
    }

    #[test]
    fn scoped_thread_attachment_download_uses_sender_direct_thread() {
        let message = Message {
            id: im_core::ids::MessageId::parse("msg_attachment_manifest").unwrap(),
            thread: ThreadRef::Thread(
                im_core::ids::ThreadId::parse("dm:peer-scope:v1:user-alice:alice.anpclaw.com")
                    .unwrap(),
            ),
            direction: MessageDirection::Incoming,
            sender: PeerRef::parse("did:human:alice", "").unwrap(),
            receiver: Some(PeerRef::parse("did:agent:hermes", "").unwrap()),
            group: None,
            body: MessageBodyView::Payload {
                payload: serde_json::json!({}),
            },
            sent_at: None,
            received_at: None,
            metadata: im_core::messages::MessageMetadata::default(),
        };

        let thread = attachment_download_thread(&message, "did:human:alice").unwrap();

        assert_eq!(
            thread,
            ThreadRef::Direct(PeerRef::parse("did:human:alice", "").unwrap())
        );
    }

    #[test]
    fn group_thread_attachment_download_uses_group_thread() {
        let message = Message {
            id: im_core::ids::MessageId::parse("msg_group_attachment").unwrap(),
            thread: ThreadRef::Thread(
                im_core::ids::ThreadId::parse("group:did:example:group").unwrap(),
            ),
            direction: MessageDirection::Incoming,
            sender: PeerRef::parse("did:human:alice", "").unwrap(),
            receiver: None,
            group: Some(im_core::ids::GroupRef::parse("did:example:group").unwrap()),
            body: MessageBodyView::Payload {
                payload: serde_json::json!({}),
            },
            sent_at: None,
            received_at: None,
            metadata: im_core::messages::MessageMetadata::default(),
        };

        let thread = attachment_download_thread(&message, "did:human:alice").unwrap();

        assert_eq!(
            thread,
            ThreadRef::Group(im_core::ids::GroupRef::parse("did:example:group").unwrap())
        );
    }

    #[test]
    fn metadata_attribute_content_type_marks_attachment_manifest() {
        let payload = json!({
            "attachments": [{
                "attachment_id": "att_1",
                "filename": "notes.md"
            }]
        });
        let message = Message {
            id: im_core::ids::MessageId::parse("msg_attachment_manifest").unwrap(),
            thread: ThreadRef::Direct(PeerRef::parse("did:human:alice", "").unwrap()),
            direction: MessageDirection::Incoming,
            sender: PeerRef::parse("did:human:alice", "").unwrap(),
            receiver: Some(PeerRef::parse("did:agent:hermes", "").unwrap()),
            group: None,
            body: MessageBodyView::Payload {
                payload: payload.clone(),
            },
            sent_at: None,
            received_at: None,
            metadata: im_core::messages::MessageMetadata {
                content_type: Some("application/json".to_string()),
                attributes: vec![im_core::messages::MessageMetadataAttribute {
                    key: "content_type".to_string(),
                    value: im_core::attachments::attachment_manifest_content_type().to_string(),
                }],
                ..im_core::messages::MessageMetadata::default()
            },
        };

        assert!(is_attachment_manifest_message(
            &message,
            "application/json",
            &payload
        ));
    }

    #[test]
    fn inbound_attachment_path_sanitizes_segments_under_state_root() {
        let root = tempfile::tempdir().unwrap();
        let config = DaemonConfig::for_state_root(root.path()).unwrap();
        let message = Message {
            id: im_core::ids::MessageId::parse("msg/unsafe").unwrap(),
            thread: ThreadRef::Direct(PeerRef::parse("did:human:alice", "").unwrap()),
            direction: MessageDirection::Incoming,
            sender: PeerRef::parse("did:human:alice", "").unwrap(),
            receiver: Some(PeerRef::parse("did:agent:hermes", "").unwrap()),
            group: None,
            body: MessageBodyView::Payload {
                payload: serde_json::json!({}),
            },
            sent_at: None,
            received_at: None,
            metadata: im_core::messages::MessageMetadata::default(),
        };

        let path = inbound_attachment_path(
            &config,
            "did:agent:hermes",
            &message,
            "../att-secret",
            "../../secret.md",
        )
        .unwrap();

        assert!(path.starts_with(root.path()));
        assert_eq!(
            path.file_name().and_then(|name| name.to_str()),
            Some("secret.md")
        );
        assert!(!path.to_string_lossy().contains(".."));
        assert!(path.parent().unwrap().is_dir());
    }
}
