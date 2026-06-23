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
    parse_message_mention_payload, InboxQuery, InboxScope, Message, MessageBodyView,
    MessageDeliveryOptions, MessageDirection, MessageMention, MessageMentionPayload,
    MessageMentionRole, MessageMentionTarget, ThreadRef,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::Digest;

use crate::agent_status::HeartbeatScheduler;
use crate::app_bridge::message_control::{
    handle_app_control_payload, is_app_control_payload, IncomingAppControlPayload,
};
use crate::cli_wrapper::CliWrapperRequest;
use crate::commands::{
    handle_agent_payload_message, AgentCommandOutcome, IncomingAgentPayloadMessage,
};
use crate::controller_scope::{
    daemon_auth_material, verify_daemon_controller_sender, VerifiedControllerSender,
};
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
    repair_hermes_profile_if_needed, HermesGateway, HermesRuntimePlugin, StdioHermesGateway,
    HERMES_RUNTIME_PLUGIN_ID,
};
use crate::registration::{AgentInventoryClient, UserServiceAgentRegistrationClient};
use crate::runtime::host::{
    flush_runtime_final_outbox, run_controller_text_task_with_config,
    run_controller_text_task_with_verified_sender_config, run_existing_runtime_task_with_config,
};
use crate::runtime::{
    is_group_conversation_id, runtime_task_matches_profile_controller_scope, RuntimeInstallStatus,
    RuntimeInvocationAuthority, RuntimeLaunchContext, RuntimeLaunchOutcome, RuntimePlugin,
    RuntimeRunStatus, RuntimeTask,
};
use crate::runtime_inbox::repair_runtime_controller_inbox_projection;
use crate::{DaemonConfig, DaemonState, ImCoreAdapter};

mod attachments;
mod group_context;
mod lifecycle_support;
mod outbox;
mod runtime_support;

use attachments::attachment_runtime_prompt_text;
#[cfg(test)]
use attachments::{
    attachment_download_thread, inbound_attachment_path, render_attachment_runtime_prompt,
    RuntimeInboundAttachment,
};
use group_context::{build_recent_group_context, GROUP_CONTEXT_FETCH_LIMIT};
use lifecycle_support::{
    ensure_agent_messaging_session, runtime_callback_outbox, start_runtime_rpc_worker,
    store_agent_token_for_configured_agents, sync_configured_agent_identities, write_ready_file,
};
use outbox::{
    runtime_message_sender_for_agent, runtime_status_sender_for_agent, ControllerOutboxSender,
    ControllerRuntimeOutbox, RuntimeCallbackOutbox, RuntimeStatusSender,
};
#[cfg(test)]
use outbox::{ControllerOutboxCall, ControllerOutboxRecorder};
use runtime_support::UdsTestRuntimePlugin;

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
    let hermes_gateway = StdioHermesGateway::from_config(&config);
    println!(
        "awiki-deamon foreground ready state_root={} socket={}",
        status.state_root.display(),
        status.local_socket_path.display()
    );

    let mut processed = HashSet::new();
    let mut processed_messages = 0usize;
    let mut heartbeat = HeartbeatScheduler::new();
    let exit_reason = loop {
        let newly_processed =
            process_inbox_once(&config, &state, &im_core, &hermes_gateway, &mut processed).await?;
        let delegated_processed =
            process_user_delegated_inbox_once(&config, &state, &im_core, hermes_gateway.clone())?;
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
            drain_runtime_retry_queue_once(&config, &state, &*outbox, &hermes_gateway)?
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
    hermes_gateway: &StdioHermesGateway,
    processed: &mut HashSet<String>,
) -> Result<usize> {
    let agents = state.list_agent_definitions()?;
    let registration = UserServiceAgentRegistrationClient::new(&config.user_service_base_url)?;
    let mut processed_count = 0usize;
    for agent in agents {
        let identity = match state.load_agent_identity(&agent.agent_did) {
            Ok(identity) => identity,
            Err(error) => {
                record_runtime_inbox_poll_error(
                    state,
                    "daemon.runtime_inbox.agent_identity.failed",
                    &agent.agent_did,
                    error,
                );
                continue;
            }
        };
        let jwt_token = match state.load_agent_auth_token(&agent.agent_did) {
            Ok(token) => token,
            Err(error) => {
                record_runtime_inbox_poll_error(
                    state,
                    "daemon.runtime_inbox.agent_token.failed",
                    &agent.agent_did,
                    error,
                );
                continue;
            }
        };
        let client =
            match im_core.client_for_agent_identity(config, &identity, jwt_token.as_deref()) {
                Ok(client) => client,
                Err(error) => {
                    record_runtime_inbox_poll_error(
                        state,
                        "daemon.runtime_inbox.client.failed",
                        &agent.agent_did,
                        error,
                    );
                    continue;
                }
            };
        if let Err(error) = ensure_agent_messaging_session(&client, &agent.agent_did).await {
            record_runtime_inbox_poll_error(
                state,
                "daemon.runtime_inbox.session.failed",
                &agent.agent_did,
                error,
            );
            continue;
        }
        for poll_scope in runtime_agent_inbox_poll_scopes() {
            match poll_scope {
                RuntimeInboxPollScope::Direct => {
                    match process_agent_direct_inbox_once(
                        config,
                        state,
                        im_core,
                        hermes_gateway,
                        &registration,
                        &client,
                        &agent.agent_did,
                        processed,
                    )
                    .await
                    {
                        Ok(count) => processed_count += count,
                        Err(error) => record_runtime_inbox_poll_error(
                            state,
                            "daemon.direct_inbox.poll.failed",
                            &agent.agent_did,
                            error,
                        ),
                    }
                }
                RuntimeInboxPollScope::Group => {
                    match process_agent_group_inbox_once(
                        config,
                        state,
                        im_core,
                        hermes_gateway,
                        &registration,
                        &client,
                        &agent.agent_did,
                        processed,
                    )
                    .await
                    {
                        Ok(count) => processed_count += count,
                        Err(error) => record_runtime_inbox_poll_error(
                            state,
                            "daemon.group_inbox.poll.failed",
                            &agent.agent_did,
                            error,
                        ),
                    }
                }
            }
        }
    }
    Ok(processed_count)
}

async fn process_agent_direct_inbox_once(
    config: &DaemonConfig,
    state: &DaemonState,
    im_core: &ImCoreAdapter,
    hermes_gateway: &StdioHermesGateway,
    registration: &UserServiceAgentRegistrationClient,
    client: &im_core::ImClient,
    agent_did: &str,
    processed: &mut HashSet<String>,
) -> Result<usize> {
    let inbox = client
        .messages()
        .inbox_with_metadata_async(InboxQuery {
            scope: RuntimeInboxPollScope::Direct.inbox_scope(),
            limit: im_core::ids::PageLimit::new(20)?,
            cursor: None,
            unread_only: false,
            inbox_history_options: None,
        })
        .await
        .with_context(|| {
            format!(
                "poll {} inbox for agent {agent_did}",
                RuntimeInboxPollScope::Direct.as_str()
            )
        })?;
    let mut processed_count = 0usize;
    for message in inbox.items.into_iter().rev() {
        if process_runtime_inbox_message(
            config,
            state,
            im_core,
            hermes_gateway,
            registration,
            client,
            agent_did,
            &message,
            processed,
            None,
        )
        .await?
        .unwrap_or(false)
        {
            processed_count += 1;
        }
    }
    Ok(processed_count)
}

async fn process_agent_group_inbox_once(
    config: &DaemonConfig,
    state: &DaemonState,
    im_core: &ImCoreAdapter,
    hermes_gateway: &StdioHermesGateway,
    registration: &UserServiceAgentRegistrationClient,
    client: &im_core::ImClient,
    agent_did: &str,
    processed: &mut HashSet<String>,
) -> Result<usize> {
    let groups = client
        .groups()
        .list_async(im_core::groups::GroupListRequest {
            limit: im_core::ids::PageLimit::new(50)?,
        })
        .await
        .with_context(|| format!("list groups for agent {agent_did}"))?;
    let mut processed_count = 0usize;
    for group in groups.groups {
        if group
            .membership_status
            .as_deref()
            .is_some_and(|status| status != "active")
        {
            continue;
        }
        let messages = client
            .groups()
            .messages_async(im_core::groups::GroupMessagesRequest {
                group: group.did.clone(),
                limit: im_core::ids::PageLimit::new(GROUP_CONTEXT_FETCH_LIMIT)?,
                cursor: None,
            })
            .await
            .with_context(|| {
                format!(
                    "poll group inbox for agent {agent_did} group {}",
                    group.did.as_str()
                )
            })?;
        let group_history = messages.messages.items;
        for message in group_history.iter().rev() {
            if process_runtime_inbox_message(
                config,
                state,
                im_core,
                hermes_gateway,
                registration,
                client,
                agent_did,
                message,
                processed,
                Some(group_history.as_slice()),
            )
            .await?
            .unwrap_or(false)
            {
                processed_count += 1;
            }
        }
    }
    Ok(processed_count)
}

async fn process_runtime_inbox_message(
    config: &DaemonConfig,
    state: &DaemonState,
    im_core: &ImCoreAdapter,
    hermes_gateway: &StdioHermesGateway,
    registration: &UserServiceAgentRegistrationClient,
    client: &im_core::ImClient,
    agent_did: &str,
    message: &Message,
    processed: &mut HashSet<String>,
    group_history: Option<&[Message]>,
) -> Result<Option<bool>> {
    if should_skip_runtime_inbox_message(agent_did, message) {
        return Ok(None);
    }
    let processed_message_id = runtime_processed_message_id(message);
    if runtime_processed_message_blocks_route(state, agent_did, &processed_message_id, message)? {
        return Ok(None);
    }
    let message_key = format!("{agent_did}:{processed_message_id}");
    if !processed.insert(message_key) {
        return Ok(None);
    }
    match route_message(
        config,
        state,
        im_core,
        hermes_gateway,
        registration,
        client,
        agent_did,
        message,
        group_history,
    )
    .await
    {
        Ok(routed) => {
            record_runtime_processed_message(state, agent_did, &processed_message_id, routed)?;
            Ok(Some(routed))
        }
        Err(error) => {
            let sanitized = sanitize_error_message(&error.to_string());
            eprintln!("warning: daemon inbox message route failed: {sanitized}");
            if let Err(audit_error) = record_inbox_route_error(state, agent_did, message, &error) {
                eprintln!(
                    "warning: daemon inbox route error audit failed: {}",
                    sanitize_error_message(&audit_error.to_string())
                );
            }
            Ok(None)
        }
    }
}

fn should_skip_runtime_inbox_message(agent_did: &str, message: &Message) -> bool {
    message.direction == MessageDirection::Outgoing
        || message.sender.as_str() == agent_did
        || (is_group_message(message) && is_agent_did_like(message.sender.as_str()))
}

fn is_group_message(message: &Message) -> bool {
    matches!(message.thread, ThreadRef::Group(_)) || message.group.is_some()
}

fn is_agent_did_like(did: &str) -> bool {
    let did = did.trim();
    did.starts_with("did:agent:") || did.contains(":agent:")
}

fn record_runtime_processed_message(
    state: &DaemonState,
    agent_did: &str,
    processed_message_id: &str,
    routed: bool,
) -> Result<()> {
    let status = if routed { "done" } else { "ignored" };
    let inserted = state.try_insert_processed_message(&crate::state::ProcessedMessageRecord {
        owner_did: agent_did.to_string(),
        message_id: processed_message_id.to_string(),
        schema: "awiki.daemon.runtime_inbox.v1".to_string(),
        processed_at_ms: 0,
        status: status.to_string(),
    })?;
    if !inserted {
        state.mark_processed_message_status(agent_did, processed_message_id, status)?;
    }
    Ok(())
}

fn runtime_processed_message_blocks_route(
    state: &DaemonState,
    agent_did: &str,
    processed_message_id: &str,
    _message: &Message,
) -> Result<bool> {
    let Some(record) = state.load_processed_message(agent_did, processed_message_id)? else {
        return Ok(false);
    };
    Ok(matches!(record.status.as_str(), "done" | "ignored"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RuntimeInboxPollScope {
    Direct,
    Group,
}

impl RuntimeInboxPollScope {
    fn as_str(self) -> &'static str {
        match self {
            Self::Direct => "direct",
            Self::Group => "group",
        }
    }

    fn inbox_scope(self) -> InboxScope {
        match self {
            Self::Direct => InboxScope::DirectOnly,
            Self::Group => InboxScope::GroupOnly,
        }
    }
}

fn runtime_agent_inbox_poll_scopes() -> [RuntimeInboxPollScope; 2] {
    [RuntimeInboxPollScope::Direct, RuntimeInboxPollScope::Group]
}

fn runtime_task_status_correlation(task: &RuntimeTask) -> (Option<String>, Option<String>) {
    let fallback_source_message_id = task
        .task_id
        .strip_prefix("task_")
        .map(str::to_string)
        .filter(|value| !value.trim().is_empty());
    let Ok(payload) = serde_json::from_str::<Value>(&task.text) else {
        return (fallback_source_message_id, None);
    };
    let source_message_id = payload
        .get("source_message_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or(fallback_source_message_id);
    let mention_id = payload
        .get("mention_context")
        .and_then(|context| context.get("mention_id"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    (source_message_id, mention_id)
}

fn record_runtime_inbox_poll_error(
    state: &DaemonState,
    event_type: &str,
    agent_did: &str,
    error: impl std::fmt::Display,
) {
    let sanitized = sanitize_error_message(&error.to_string());
    eprintln!("warning: {event_type} for agent {agent_did}: {sanitized}");
    if let Err(audit_error) = state.insert_audit_event_json(
        event_type,
        Some(agent_did),
        None,
        None,
        None,
        json!({
            "error": sanitized,
        }),
    ) {
        eprintln!(
            "warning: daemon inbox poll error audit failed: {}",
            sanitize_error_message(&audit_error.to_string())
        );
    }
}

fn runtime_processed_message_id(message: &Message) -> String {
    match &message.thread {
        ThreadRef::Group(group) => message
            .metadata
            .attributes
            .iter()
            .find(|attribute| attribute.key == "group_event_seq")
            .map(|attribute| attribute.value.trim())
            .filter(|value| !value.is_empty())
            .map(|seq| format!("group:{}:{seq}", group.as_str()))
            .unwrap_or_else(|| format!("group:{}:{}", group.as_str(), message.id.as_str())),
        _ => message.id.as_str().to_string(),
    }
}

fn drain_runtime_retry_queue_once(
    config: &DaemonConfig,
    state: &DaemonState,
    outbox: &impl RuntimeOutbox,
    hermes_gateway: &StdioHermesGateway,
) -> Result<usize> {
    let retries = state.list_queued_runtime_retries(10)?;
    let mut processed = 0usize;
    for retry in retries {
        state.mark_runtime_retry_status(&retry.retry_id, "running")?;
        let result = run_runtime_retry(config, state, outbox, hermes_gateway, &retry);
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
    hermes_gateway: &StdioHermesGateway,
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
            let hermes_profile = current_hermes_profile_for_runtime(config, state, &profile)?;
            let plugin = HermesRuntimePlugin::with_state(
                hermes_gateway.clone(),
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
    if !runtime_task_matches_profile_controller_scope(task, profile) {
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
    hermes_gateway: &StdioHermesGateway,
    registration: &UserServiceAgentRegistrationClient,
    target_client: &im_core::ImClient,
    target_agent_did: &str,
    message: &Message,
    group_history: Option<&[Message]>,
) -> Result<bool> {
    let sender_did = message.sender.as_str().to_string();
    let conversation_id = conversation_id(message);
    if is_opaque_group_e2ee_message(message) {
        record_ignored_opaque_group_e2ee_message(state, target_agent_did, message)?;
        return Ok(false);
    }
    if is_group_message(message) {
        if let Some(routed) = try_route_group_agent_mention(
            config,
            state,
            im_core,
            hermes_gateway,
            registration,
            target_client,
            target_agent_did,
            message,
            group_history.unwrap_or(&[]),
        )? {
            return Ok(routed);
        }
        return Ok(false);
    }
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
                    hermes_gateway,
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
                run_runtime_task_command(
                    config,
                    state,
                    im_core,
                    hermes_gateway,
                    registration,
                    payload_message,
                )?;
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
            route_runtime_direct_text(
                config,
                state,
                im_core,
                hermes_gateway,
                registration,
                target_client,
                target_agent_did,
                message.id.as_str(),
                conversation_id,
                sender_did,
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

fn is_opaque_group_e2ee_message(message: &Message) -> bool {
    matches!(message.thread, ThreadRef::Group(_))
        && !matches!(message.body, MessageBodyView::Text { .. })
        && (message.metadata.attributes.iter().any(|attribute| {
            matches!(
                attribute.key.as_str(),
                "security" | "message_security_profile" | "security_profile"
            ) && attribute.value == "group-e2ee"
        }) || matches!(
            &message.body,
            MessageBodyView::Payload { payload }
                if payload.get("group_cipher_object").is_some()
                    || payload.get("ciphertext_b64u").is_some()
        ))
}

fn try_route_group_agent_mention<C>(
    config: &DaemonConfig,
    state: &DaemonState,
    im_core: &ImCoreAdapter,
    hermes_gateway: &StdioHermesGateway,
    registration: &C,
    target_client: &im_core::ImClient,
    target_agent_did: &str,
    message: &Message,
    group_history: &[Message],
) -> Result<Option<bool>>
where
    C: AgentInventoryClient,
{
    if should_skip_runtime_inbox_message(target_agent_did, message) {
        return Ok(Some(false));
    }
    let MessageBodyView::Payload { payload } = &message.body else {
        return Ok(None);
    };
    let mention_payload = match parse_message_mention_payload(payload) {
        Ok(payload) => payload,
        Err(_) => return Ok(None),
    };
    let Some(mention_context) = group_agent_mention_context(target_agent_did, &mention_payload)
    else {
        return Ok(Some(false));
    };
    let binding = state
        .load_runtime_daemon_binding(target_agent_did)?
        .with_context(|| format!("runtime daemon binding missing for {target_agent_did}"))?;
    state.load_agent_definition(&binding.daemon_agent_did)?;
    state.load_agent_definition(target_agent_did)?;
    let conversation_id = conversation_id(message);
    let status_sender = runtime_status_sender_for_agent(config, state, im_core, target_agent_did)?;
    let auth = daemon_auth_material(
        config,
        state,
        &state.load_agent_definition(&binding.daemon_agent_did)?,
    )?;
    let authorization = registration.authorize_agent_invocation(
        &binding.daemon_agent_did,
        target_agent_did,
        message.sender.as_str(),
        conversation_id.as_deref(),
        Some(message.id.as_str()),
        &auth,
    )?;
    if !authorization.allowed {
        emit_group_agent_mention_rejection(
            state,
            &status_sender,
            target_agent_did,
            message,
            &binding.daemon_agent_did,
            &binding.controller_scope_key,
            &mention_context,
            authorization.reason.as_str(),
            authorization.active_mode.as_str(),
        )?;
        return Ok(Some(true));
    }

    let task_payload = group_agent_mention_task_payload(
        message,
        &mention_payload,
        &mention_context,
        authorization.sender_full_handle.as_deref(),
        Some(build_recent_group_context(message, group_history)),
    );
    let task_message_id =
        group_agent_mention_task_message_id(message, &mention_context.mention_id, target_agent_did);
    let runtime_outbox = ControllerRuntimeOutbox::with_status_correlation(
        ControllerOutboxSender::ImCore(ImCoreAgentOutbox::new(target_client.clone())),
        status_sender.sender,
        Some(status_sender.daemon_agent_did),
        message.sender.as_str().to_string(),
        Some(binding.controller_did.clone()),
        format!("task_{task_message_id}"),
        conversation_id.clone(),
        Some(message.id.as_str().to_string()),
        Some(mention_context.mention_id.clone()),
        Some(message.sender.as_str().to_string()),
        authorization.sender_full_handle.clone(),
        Some(
            crate::runtime::RuntimeTaskTriggerKind::GroupMention
                .as_str()
                .to_string(),
        ),
        Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        Arc::new(Mutex::new(Vec::new())),
    );
    run_runtime_text_message_with_gateway(
        config,
        state,
        &runtime_outbox,
        ControllerTextMessage {
            message_id: task_message_id,
            conversation_id: conversation_id.clone(),
            sender_did: message.sender.as_str().to_string(),
            requester_user_id: authorization.sender_user_id.clone(),
            requester_full_handle: authorization.sender_full_handle.clone(),
            trigger_kind: crate::runtime::RuntimeTaskTriggerKind::GroupMention,
            invocation_authority: group_mention_invocation_authority(
                &binding,
                authorization.sender_user_id.as_deref(),
                authorization.sender_full_handle.as_deref(),
            ),
            target_agent_did: target_agent_did.to_string(),
            text: task_payload.to_string(),
        },
        None,
        hermes_gateway.clone(),
    )?;
    state.insert_audit_event_json(
        "daemon.group_mention.dispatched",
        Some(target_agent_did),
        Some(&binding.controller_scope_key),
        None,
        None,
        json!({
            "runtime_agent_did": target_agent_did,
            "daemon_agent_did": binding.daemon_agent_did,
            "source_message_id": message.id.as_str(),
            "source_conversation_id": conversation_id,
            "source_sender_did": message.sender.as_str(),
            "mention_id": mention_context.mention_id,
            "mention_role": mention_context.mention_role,
            "target_kind": mention_context.target_kind,
            "selector": mention_context.selector,
            "match_kind": mention_context.match_kind,
        }),
    )?;
    Ok(Some(true))
}

fn emit_group_agent_mention_rejection(
    state: &DaemonState,
    status_sender: &RuntimeStatusSender,
    target_agent_did: &str,
    message: &Message,
    daemon_agent_did: &str,
    controller_scope_key: &str,
    mention_context: &GroupAgentMentionContext,
    reason: &str,
    active_mode: &str,
) -> Result<()> {
    let task_id = format!(
        "task_{}",
        group_agent_mention_task_message_id(message, &mention_context.mention_id, target_agent_did)
    );
    let conversation_id = conversation_id(message);
    let sent_at = time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string());
    let run_id = format!("run_{task_id}");
    status_sender.sender.send_payload(
        message.sender.as_str(),
        json!({
            "schema": "awiki.agent.status.v1",
            "event_id": format!("evt_{}", crate::security::runtime_token::current_time_millis().unwrap_or(0)),
            "sent_at": sent_at,
            "daemon_agent_did": daemon_agent_did,
            "status_scope": "run",
            "task_id": task_id,
            "run_id": run_id,
            "conversation_id": conversation_id,
            "state": "failed",
            "message": "访问权限不允许该智能体响应这条群聊 @",
            "daemon": null,
            "runtimes": [],
            "runs": [{
                "run_id": run_id,
                "message_id": task_id,
                "source_message_id": message.id.as_str(),
                "mention_id": mention_context.mention_id,
                "runtime_agent_did": target_agent_did,
                "conversation_id": conversation_id,
                "status": "failed",
                "started_at": sent_at,
                "updated_at": sent_at,
                "last_error_code": "agent_invocation_denied",
                "last_error_summary": reason,
            }],
        }),
    )?;
    state.insert_audit_event_json(
        "daemon.group_mention.authorization_denied",
        Some(target_agent_did),
        Some(controller_scope_key),
        None,
        None,
        json!({
            "runtime_agent_did": target_agent_did,
            "daemon_agent_did": daemon_agent_did,
            "source_message_id": message.id.as_str(),
            "source_conversation_id": conversation_id,
            "source_sender_did": message.sender.as_str(),
            "reason": reason,
            "active_mode": active_mode,
            "mention_id": mention_context.mention_id,
            "match_kind": mention_context.match_kind,
        }),
    )?;
    Ok(())
}

#[derive(Debug, Clone)]
struct GroupAgentMentionContext {
    mention_id: String,
    mention_role: &'static str,
    target_kind: &'static str,
    selector: Option<&'static str>,
    surface: String,
    range_start: usize,
    range_end: usize,
    match_kind: &'static str,
    prompt_hint: &'static str,
}

fn group_agent_mention_context(
    target_agent_did: &str,
    payload: &MessageMentionPayload,
) -> Option<GroupAgentMentionContext> {
    payload
        .mentions
        .iter()
        .filter_map(|mention| {
            group_agent_mention_context_for_mention(target_agent_did, payload, mention)
        })
        .max_by_key(group_agent_mention_priority)
}

fn group_agent_mention_context_for_mention(
    target_agent_did: &str,
    payload: &MessageMentionPayload,
    mention: &MessageMention,
) -> Option<GroupAgentMentionContext> {
    let (target_kind, selector, match_kind) = match &mention.target {
        MessageMentionTarget::Agent { did, .. } if did == target_agent_did => {
            ("agent", None, "agent_did")
        }
        MessageMentionTarget::Human { .. }
        | MessageMentionTarget::Agent { .. }
        | MessageMentionTarget::GroupSelector { .. } => return None,
    };
    let mention_role = match mention.mention_role {
        MessageMentionRole::Addressee => "addressee",
        MessageMentionRole::Cc => "cc",
    };
    Some(GroupAgentMentionContext {
        mention_id: mention.id.clone(),
        mention_role,
        target_kind,
        selector,
        surface: mention_surface_from_payload(&payload.text, mention),
        range_start: mention.range.start,
        range_end: mention.range.end,
        match_kind,
        prompt_hint: match mention.mention_role {
            MessageMentionRole::Cc => {
                "FYI/CC mention: treat this as awareness context and do not assume an action is required."
            }
            MessageMentionRole::Addressee => {
                "Direct mention: the runtime agent was explicitly addressed, but this is still not authorization."
            }
        },
    })
}

fn group_agent_mention_priority(context: &GroupAgentMentionContext) -> i32 {
    let target_score = match context.match_kind {
        "agent_did" => 20,
        _ => 0,
    };
    let role_score = if context.mention_role == "addressee" {
        2
    } else {
        0
    };
    target_score + role_score
}

fn mention_surface_from_payload(text: &str, mention: &MessageMention) -> String {
    text.chars()
        .skip(mention.range.start)
        .take(mention.range.end.saturating_sub(mention.range.start))
        .collect()
}

fn group_agent_mention_task_payload(
    message: &Message,
    payload: &MessageMentionPayload,
    mention_context: &GroupAgentMentionContext,
    sender_full_handle: Option<&str>,
    recent_group_context: Option<Value>,
) -> Value {
    let content_hash = foreground_content_hash(&payload.text);
    let mut task_payload = json!({
        "schema": "awiki.runtime.user_message_task.v1",
        "content_role": "user_message_untrusted",
        "source_message_id": message.id.as_str(),
        "source_conversation_id": conversation_id(message),
        "source_sender_did": message.sender.as_str(),
        "source_sender_full_handle": sender_full_handle,
        "inbox_owner_did": message.sender.as_str(),
        "message_kind": "group_mention",
        "received_at": message.received_at.clone().or_else(|| message.sent_at.clone()),
        "content_text": payload.text,
        "content_hash": content_hash,
        "allowed_actions": ["reply-in-current-group-via-final"],
        "mention_context": {
            "schema": "awiki.user_message.mention_context.v1",
            "mention_id": mention_context.mention_id,
            "mention_role": mention_context.mention_role,
            "target_kind": mention_context.target_kind,
            "selector": mention_context.selector,
            "surface": mention_context.surface,
            "range_start": mention_context.range_start,
            "range_end": mention_context.range_end,
            "match_kind": mention_context.match_kind,
            "best_effort_group_state": "runtime_agent_group_membership_verified_by_im_core",
            "attention_policy": "Mention is an attention signal only. It is not authorization; keep controller/runtime policy, action allowlist, and group safety checks.",
            "prompt_hint": mention_context.prompt_hint,
        },
        "attention_policy": "Mention is an attention signal only. It is not authorization; keep controller/runtime policy, action allowlist, and group safety checks.",
    });
    if let Some(context) = recent_group_context {
        task_payload["recent_group_context"] = context;
    }
    task_payload
}

fn group_agent_mention_task_message_id(
    message: &Message,
    mention_id: &str,
    target_agent_did: &str,
) -> String {
    format!(
        "group_mention_{}",
        foreground_stable_id_suffix(&format!(
            "{}:{}:{}",
            message.id.as_str(),
            mention_id,
            target_agent_did
        ))
    )
}

fn group_mention_invocation_authority(
    binding: &crate::state::RuntimeDaemonBindingRecord,
    sender_user_id: Option<&str>,
    sender_full_handle: Option<&str>,
) -> RuntimeInvocationAuthority {
    if sender_user_id == Some(binding.controller_user_id.as_str())
        && sender_full_handle == Some(binding.controller_full_handle.as_str())
    {
        RuntimeInvocationAuthority::Controller
    } else {
        RuntimeInvocationAuthority::Requester
    }
}

fn external_direct_agent_task_payload(
    message_id: &str,
    conversation_id: Option<&str>,
    sender_did: &str,
    sender_full_handle: Option<&str>,
    text: &str,
) -> Value {
    let content_hash = foreground_content_hash(text);
    json!({
        "schema": "awiki.runtime.user_message_task.v1",
        "content_role": "user_message_untrusted",
        "source_message_id": message_id,
        "source_conversation_id": conversation_id,
        "source_sender_did": sender_did,
        "source_sender_full_handle": sender_full_handle,
        "inbox_owner_did": sender_did,
        "message_kind": "external_direct",
        "content_text": text,
        "content_hash": content_hash,
        "allowed_actions": ["reply-in-current-direct-via-final"],
        "direct_request_context": {
            "schema": "awiki.user_message.direct_request_context.v1",
            "requester_did": sender_did,
            "requester_full_handle": sender_full_handle,
            "prompt_hint": "This is a direct private request from a non-controller user to this agent."
        },
        "attention_policy": "Direct access is allowed only because user-service invocation policy authorized this requester for this agent. The requester is not the controller.",
    })
}

fn external_direct_task_message_id(message_id: &str, target_agent_did: &str) -> String {
    format!(
        "external_direct_{}",
        foreground_stable_id_suffix(&format!("{message_id}:{target_agent_did}"))
    )
}

fn foreground_content_hash(text: &str) -> String {
    let digest = sha2::Sha256::digest(text.as_bytes());
    format!("{digest:x}")
}

fn foreground_stable_id_suffix(input: &str) -> String {
    let digest = sha2::Sha256::digest(input.as_bytes());
    digest
        .iter()
        .take(16)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn route_runtime_controller_text(
    config: &DaemonConfig,
    state: &DaemonState,
    im_core: &ImCoreAdapter,
    hermes_gateway: &StdioHermesGateway,
    target_client: &im_core::ImClient,
    target_agent_did: &str,
    message_id: &str,
    conversation_id: Option<String>,
    sender_did: String,
    verified_sender: Option<VerifiedControllerSender>,
    text: String,
) -> Result<()> {
    if is_group_conversation_id(conversation_id.as_deref()) {
        verify_runtime_group_sender(state, target_agent_did, &sender_did)?;
    } else {
        match verified_sender.as_ref() {
            Some(_verified_sender) => {}
            None => {
                let registration =
                    UserServiceAgentRegistrationClient::new(&config.user_service_base_url)?;
                verify_runtime_controller_sender(
                    config,
                    state,
                    &registration,
                    target_agent_did,
                    &sender_did,
                )?;
            }
        }
    }
    repair_runtime_controller_inbox_projection_best_effort(config, state, target_agent_did);
    let outbox = ImCoreAgentOutbox::new(target_client.clone());
    let status_sender = runtime_status_sender_for_agent(config, state, im_core, target_agent_did)?;
    let runtime_outbox = ControllerRuntimeOutbox::with_status_correlation(
        ControllerOutboxSender::ImCore(outbox.clone()),
        status_sender.sender,
        Some(status_sender.daemon_agent_did),
        sender_did.clone(),
        Some(
            state
                .load_runtime_agent_profile(target_agent_did)?
                .controller_did,
        ),
        format!("task_{message_id}"),
        conversation_id.clone(),
        Some(message_id.to_string()),
        None,
        Some(sender_did.clone()),
        None,
        Some(
            crate::runtime::RuntimeTaskTriggerKind::ControllerDirect
                .as_str()
                .to_string(),
        ),
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
            sender_did: sender_did.clone(),
            requester_user_id: verified_sender
                .as_ref()
                .map(|sender| sender.controller_user_id.clone()),
            requester_full_handle: None,
            trigger_kind: crate::runtime::RuntimeTaskTriggerKind::ControllerDirect,
            invocation_authority: RuntimeInvocationAuthority::Controller,
            target_agent_did: target_agent_did.to_string(),
            text,
        },
        verified_sender,
        hermes_gateway.clone(),
    )?;
    Ok(())
}

fn route_runtime_direct_text<C>(
    config: &DaemonConfig,
    state: &DaemonState,
    im_core: &ImCoreAdapter,
    hermes_gateway: &StdioHermesGateway,
    registration: &C,
    target_client: &im_core::ImClient,
    target_agent_did: &str,
    message_id: &str,
    conversation_id: Option<String>,
    sender_did: String,
    text: String,
) -> Result<()>
where
    C: AgentInventoryClient,
{
    match verify_runtime_controller_sender(
        config,
        state,
        registration,
        target_agent_did,
        &sender_did,
    ) {
        Ok(verified_sender) => route_runtime_controller_text(
            config,
            state,
            im_core,
            hermes_gateway,
            target_client,
            target_agent_did,
            message_id,
            conversation_id,
            sender_did,
            Some(verified_sender),
            text,
        ),
        Err(controller_error) => route_runtime_external_direct_text(
            config,
            state,
            im_core,
            hermes_gateway,
            registration,
            target_client,
            target_agent_did,
            message_id,
            conversation_id,
            sender_did,
            text,
            &controller_error,
        ),
    }
}

fn route_runtime_external_direct_text<C>(
    config: &DaemonConfig,
    state: &DaemonState,
    im_core: &ImCoreAdapter,
    hermes_gateway: &StdioHermesGateway,
    registration: &C,
    target_client: &im_core::ImClient,
    target_agent_did: &str,
    message_id: &str,
    conversation_id: Option<String>,
    sender_did: String,
    text: String,
    controller_error: &anyhow::Error,
) -> Result<()>
where
    C: AgentInventoryClient,
{
    if is_group_conversation_id(conversation_id.as_deref()) {
        return Err(anyhow::anyhow!(
            "group runtime text requires structured mention"
        ));
    }
    let binding = state
        .load_runtime_daemon_binding(target_agent_did)?
        .with_context(|| format!("runtime daemon binding missing for {target_agent_did}"))?;
    let daemon_agent = state.load_agent_definition(&binding.daemon_agent_did)?;
    let auth = daemon_auth_material(config, state, &daemon_agent)?;
    let authorization = registration.authorize_agent_invocation(
        &binding.daemon_agent_did,
        target_agent_did,
        &sender_did,
        conversation_id.as_deref(),
        Some(message_id),
        &auth,
    )?;
    if !authorization.allowed {
        let status_sender =
            runtime_status_sender_for_agent(config, state, im_core, target_agent_did)?;
        emit_external_direct_invocation_rejection(
            state,
            &status_sender,
            target_agent_did,
            &binding.daemon_agent_did,
            &binding.controller_scope_key,
            message_id,
            conversation_id.as_deref(),
            &sender_did,
            authorization.sender_full_handle.as_deref(),
            authorization.reason.as_str(),
            authorization.active_mode.as_str(),
        )?;
        return Ok(());
    }
    repair_runtime_controller_inbox_projection_best_effort(config, state, target_agent_did);
    let task_payload = external_direct_agent_task_payload(
        message_id,
        conversation_id.as_deref(),
        &sender_did,
        authorization.sender_full_handle.as_deref(),
        &text,
    );
    let outbox = ImCoreAgentOutbox::new(target_client.clone());
    let status_sender = runtime_status_sender_for_agent(config, state, im_core, target_agent_did)?;
    let task_message_id = external_direct_task_message_id(message_id, target_agent_did);
    let runtime_outbox = ControllerRuntimeOutbox::with_status_correlation(
        ControllerOutboxSender::ImCore(outbox.clone()),
        status_sender.sender,
        Some(status_sender.daemon_agent_did),
        sender_did.clone(),
        Some(binding.controller_did.clone()),
        format!("task_{task_message_id}"),
        conversation_id.clone(),
        Some(message_id.to_string()),
        None,
        Some(sender_did.clone()),
        authorization.sender_full_handle.clone(),
        Some(
            crate::runtime::RuntimeTaskTriggerKind::ExternalDirect
                .as_str()
                .to_string(),
        ),
        Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        Arc::new(Mutex::new(Vec::new())),
    );
    run_runtime_text_message_with_gateway(
        config,
        state,
        &runtime_outbox,
        ControllerTextMessage {
            message_id: task_message_id,
            conversation_id,
            sender_did,
            requester_user_id: authorization.sender_user_id,
            requester_full_handle: authorization.sender_full_handle,
            trigger_kind: crate::runtime::RuntimeTaskTriggerKind::ExternalDirect,
            invocation_authority: RuntimeInvocationAuthority::Requester,
            target_agent_did: target_agent_did.to_string(),
            text: task_payload.to_string(),
        },
        None,
        hermes_gateway.clone(),
    )
    .with_context(|| {
        format!(
            "route external direct runtime text after controller verification failed: {}",
            sanitize_error_message(&controller_error.to_string())
        )
    })?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn emit_external_direct_invocation_rejection(
    state: &DaemonState,
    status_sender: &RuntimeStatusSender,
    target_agent_did: &str,
    daemon_agent_did: &str,
    controller_scope_key: &str,
    source_message_id: &str,
    conversation_id: Option<&str>,
    sender_did: &str,
    sender_full_handle: Option<&str>,
    reason: &str,
    active_mode: &str,
) -> Result<()> {
    state.insert_audit_event_json(
        "daemon.direct_invocation.rejected",
        Some(target_agent_did),
        Some(controller_scope_key),
        None,
        None,
        json!({
            "runtime_agent_did": target_agent_did,
            "daemon_agent_did": daemon_agent_did,
            "source_message_id": source_message_id,
            "source_conversation_id": conversation_id,
            "source_sender_did": sender_did,
            "reason": reason,
            "active_mode": active_mode,
        }),
    )?;
    let task_id = format!(
        "task_{}",
        external_direct_task_message_id(source_message_id, target_agent_did)
    );
    let runtime_outbox = ControllerRuntimeOutbox::with_status_correlation(
        ControllerOutboxSender::Mock,
        status_sender.sender.clone(),
        Some(status_sender.daemon_agent_did.clone()),
        sender_did.to_string(),
        None,
        task_id.clone(),
        conversation_id.map(str::to_string),
        Some(source_message_id.to_string()),
        None,
        Some(sender_did.to_string()),
        sender_full_handle.map(str::to_string),
        Some(
            crate::runtime::RuntimeTaskTriggerKind::ExternalDirect
                .as_str()
                .to_string(),
        ),
        Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        Arc::new(Mutex::new(Vec::new())),
    );
    let context = crate::state::AuthorizedRuntimeContext {
        token_id: "host-direct-invocation-rejected".to_string(),
        agent_did: target_agent_did.to_string(),
        runtime_profile_id: state
            .load_runtime_agent_profile(target_agent_did)?
            .runtime_profile_id,
        run_id: format!(
            "run_task_{}",
            external_direct_task_message_id(source_message_id, target_agent_did)
        ),
        method: crate::security::runtime_token::RpcMethod::TaskStatus,
    };
    runtime_outbox.send_status_with_detail(
        &context,
        "failed",
        Some("Agent invocation is not allowed"),
        Some("agent_invocation_denied"),
        Some(reason),
    )?;
    Ok(())
}

fn verify_runtime_group_sender(
    state: &DaemonState,
    target_agent_did: &str,
    sender_did: &str,
) -> Result<()> {
    if sender_did.trim().is_empty() {
        bail!("runtime group sender_did must not be empty");
    }
    let binding = state
        .load_runtime_daemon_binding(target_agent_did)?
        .with_context(|| format!("runtime daemon binding missing for {target_agent_did}"))?;
    state.load_agent_definition(&binding.daemon_agent_did)?;
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

fn record_ignored_opaque_group_e2ee_message(
    state: &DaemonState,
    target_agent_did: &str,
    message: &Message,
) -> Result<()> {
    state.insert_audit_event_json(
        "daemon.group_e2ee.opaque.ignored",
        Some(target_agent_did),
        None,
        None,
        None,
        json!({
            "reason": "opaque_group_e2ee_not_promptable",
            "message_id": message.id.as_str(),
            "sender_did": message.sender.as_str(),
            "target_agent_did": target_agent_did,
            "conversation_id": conversation_id(message),
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

fn run_runtime_text_message_with_gateway<G>(
    config: &DaemonConfig,
    state: &DaemonState,
    outbox: &impl RuntimeOutbox,
    message: ControllerTextMessage,
    verified_sender: Option<VerifiedControllerSender>,
    hermes_gateway: G,
) -> Result<crate::runtime::host::RuntimeTaskRunResult>
where
    G: HermesGateway + Clone,
{
    let current_profile = state.load_runtime_agent_profile(&message.target_agent_did)?;
    match current_profile.runtime_plugin_id.as_str() {
        HERMES_RUNTIME_PLUGIN_ID => {
            let hermes_profile =
                current_hermes_profile_for_runtime(config, state, &current_profile)?;
            let plugin =
                HermesRuntimePlugin::with_state(hermes_gateway, hermes_profile, state.clone());
            if let Some(verified) = verified_sender.as_ref() {
                run_controller_text_task_with_verified_sender_config(
                    config,
                    state,
                    &current_profile,
                    verified,
                    &plugin,
                    outbox,
                    message,
                )
            } else {
                run_controller_text_task_with_config(
                    config,
                    state,
                    &current_profile,
                    &plugin,
                    outbox,
                    message,
                )
            }
        }
        GENERIC_CLI_RUNTIME_PLUGIN_ID => {
            let cli_profile =
                state.load_cli_runtime_profile(&current_profile.runtime_profile_id)?;
            let plugin = GenericCliDriverRegistry::new(cli_profile);
            if let Some(verified) = verified_sender.as_ref() {
                run_controller_text_task_with_verified_sender_config(
                    config,
                    state,
                    &current_profile,
                    verified,
                    &plugin,
                    outbox,
                    message,
                )
            } else {
                run_controller_text_task_with_config(
                    config,
                    state,
                    &current_profile,
                    &plugin,
                    outbox,
                    message,
                )
            }
        }
        _ => {
            let plugin = UdsTestRuntimePlugin::new(config.local_socket_path.clone());
            if let Some(verified) = verified_sender.as_ref() {
                run_controller_text_task_with_verified_sender_config(
                    config,
                    state,
                    &current_profile,
                    verified,
                    &plugin,
                    outbox,
                    message,
                )
            } else {
                run_controller_text_task_with_config(
                    config,
                    state,
                    &current_profile,
                    &plugin,
                    outbox,
                    message,
                )
            }
        }
    }
}

fn current_hermes_profile_for_runtime(
    config: &DaemonConfig,
    state: &DaemonState,
    profile: &crate::runtime::RuntimeAgentProfile,
) -> Result<crate::state::HermesProfileRecord> {
    let definition = state.load_agent_definition(&profile.agent_did)?;
    if let Some(repaired) =
        repair_hermes_profile_if_needed(config, state, profile, &definition.handle)?
    {
        return Ok(repaired.record);
    }
    state.load_hermes_profile(&profile.agent_did)
}

fn run_runtime_task_command(
    config: &DaemonConfig,
    state: &DaemonState,
    im_core: &ImCoreAdapter,
    hermes_gateway: &StdioHermesGateway,
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
        Some(profile.controller_did.clone()),
        format!("task_{message_id}"),
        message.conversation_id.clone(),
        Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        Arc::new(Mutex::new(Vec::new())),
    );
    let task_message = ControllerTextMessage {
        message_id,
        conversation_id: message.conversation_id,
        sender_did: verified_sender.sender_did,
        requester_user_id: Some(verified_sender.controller_user_id),
        requester_full_handle: Some(verified_sender.controller_full_handle),
        trigger_kind: crate::runtime::RuntimeTaskTriggerKind::ControllerDirect,
        invocation_authority: RuntimeInvocationAuthority::Controller,
        target_agent_did,
        text: payload.text,
    };
    match profile.runtime_plugin_id.as_str() {
        HERMES_RUNTIME_PLUGIN_ID => {
            let hermes_profile = current_hermes_profile_for_runtime(config, state, &profile)?;
            let plugin = HermesRuntimePlugin::with_state(
                hermes_gateway.clone(),
                hermes_profile,
                state.clone(),
            );
            run_controller_text_task_with_config(
                config,
                state,
                &profile,
                &plugin,
                &runtime_outbox,
                task_message,
            )?;
        }
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

fn conversation_id(message: &Message) -> Option<String> {
    match &message.thread {
        ThreadRef::Direct(peer) => Some(format!("direct:{}", peer.as_str())),
        ThreadRef::Group(group) => Some(format!("group:{}", group.as_str())),
        ThreadRef::Thread(thread) => Some(thread.as_str().to_string()),
    }
}

#[cfg(test)]
mod tests;
