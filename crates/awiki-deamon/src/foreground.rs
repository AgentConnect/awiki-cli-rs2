use std::collections::HashSet;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use im_core::messages::{
    InboxQuery, InboxScope, Message, MessageBodyView, MessageDeliveryOptions, MessageDirection,
    ThreadRef,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::agent_status::HeartbeatScheduler;
use crate::cli_wrapper::CliWrapperRequest;
use crate::commands::{
    handle_agent_payload_message, AgentCommandOutcome, IncomingAgentPayloadMessage,
};
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
        let newly_processed = newly_processed + retry_processed;
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
            if route_message(
                config,
                state,
                im_core,
                &registration,
                &client,
                &agent.agent_did,
                &message,
            )? {
                processed_count += 1;
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
                StdioHermesGateway::from_env(),
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
        || task.controller_did != profile.controller_did
        || task.sender_did != profile.controller_did
    {
        bail!("runtime retry task does not match profile binding");
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

fn route_message(
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
    let outbox = ImCoreAgentOutbox::new(target_client.clone());
    match &message.body {
        MessageBodyView::Payload { payload } => {
            let payload_message = IncomingAgentPayloadMessage {
                message_id: message.id.as_str().to_string(),
                conversation_id,
                sender_did,
                target_agent_did: target_agent_did.to_string(),
                content_type: message
                    .metadata
                    .content_type
                    .clone()
                    .unwrap_or_else(|| "application/json".to_string()),
                payload: payload.clone(),
            };
            if payload.get("schema").and_then(Value::as_str) == Some("awiki.agent.command.v1")
                && payload.get("command").and_then(Value::as_str) == Some("runtime.task.submit")
            {
                run_runtime_task_command(config, state, im_core, payload_message)?;
            } else {
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
            let status_sender =
                runtime_status_sender_for_agent(config, state, im_core, target_agent_did)?;
            let runtime_outbox = ControllerRuntimeOutbox::new(
                ControllerOutboxSender::ImCore(outbox.clone()),
                status_sender.sender,
                Some(status_sender.daemon_agent_did),
                sender_did.clone(),
                format!("task_{}", message.id.as_str()),
                conversation_id.clone(),
                Arc::new(std::sync::atomic::AtomicUsize::new(0)),
                Arc::new(Mutex::new(Vec::new())),
            );
            run_runtime_text_message_with_gateway(
                config,
                state,
                &runtime_outbox,
                ControllerTextMessage {
                    message_id: message.id.as_str().to_string(),
                    conversation_id,
                    sender_did,
                    target_agent_did: target_agent_did.to_string(),
                    text: text.clone(),
                },
                StdioHermesGateway::from_env,
            )?;
            Ok(true)
        }
        MessageBodyView::Unsupported { .. } => Ok(false),
    }
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
        RuntimeMessageSecurity::DirectE2ee,
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
    message: IncomingAgentPayloadMessage,
) -> Result<()> {
    let payload = RuntimeTaskSubmitPayload::parse(&message.payload)?;
    let target_agent_did = payload
        .target_agent_did
        .as_deref()
        .unwrap_or(&message.target_agent_did)
        .to_string();
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
        sender_did: message.sender_did,
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
            Some(RuntimeMessageSecurity::DirectE2ee),
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
    use crate::commands::{
        handle_agent_payload_message, setup_daemon_agent, AgentCommandOutcome,
        IncomingAgentPayloadMessage, RuntimeAgentCreateOutcome,
    };
    use crate::outbox::{MemoryRuntimeOutbox, OutboxRecordKind};
    use crate::plugins::hermes::{FakeHermesGateway, AWIKI_SKILLS_VERSION};
    use crate::registration::{
        AgentRegistrationClient, AgentRegistrationExchangeRequest, AgentRegistrationExchangeResult,
        RegistrationToken,
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
                controller_did: request.controller_did,
                handle: request.handle,
                status: "registered".to_string(),
            })
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
                        "registration_token": "tok_runtime_secret_value"
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
        assert_eq!(calls[0].security, RuntimeMessageSecurity::DirectE2ee);
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
                    security: RuntimeMessageSecurity::DirectE2ee,
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
        assert_eq!(calls[2].security, Some(RuntimeMessageSecurity::DirectE2ee));
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
    fn hermes_foreground_non_controller_text_is_rejected_before_gateway() {
        let (root, config, state) = fixture();
        let profile = profile(root.path());
        state.upsert_runtime_agent_profile(&profile).unwrap();
        state
            .upsert_hermes_profile(&hermes_record(root.path()))
            .unwrap();
        let outbox = MemoryRuntimeOutbox::default();
        let gateway = FakeHermesGateway::default();

        let error = run_runtime_text_message_with_gateway(
            &config,
            &state,
            &outbox,
            ControllerTextMessage {
                message_id: "msg_foreground_unauthorized".to_string(),
                conversation_id: Some("direct:did:human:bob".to_string()),
                sender_did: "did:human:bob".to_string(),
                target_agent_did: "did:agent:hermes".to_string(),
                text: "unauthorized foreground route".to_string(),
            },
            || gateway.clone(),
        )
        .unwrap_err();

        assert!(error.to_string().contains("controller_did"));
        assert!(gateway.created_sessions().is_empty());
        assert!(gateway.submitted_prompts().is_empty());
        assert!(outbox.records().is_empty());
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
}
