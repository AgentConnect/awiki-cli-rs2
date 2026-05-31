use std::collections::HashSet;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use im_core::messages::{
    InboxQuery, InboxScope, Message, MessageBodyView, MessageDirection, ThreadRef,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::cli_wrapper::CliWrapperRequest;
use crate::commands::{handle_agent_payload_message, IncomingAgentPayloadMessage};
use crate::inbox::ControllerTextMessage;
use crate::local_rpc::call_uds_once;
#[cfg(unix)]
use crate::local_rpc::{
    bind_uds_listener, handle_uds_stream_with_outbox, verify_socket_permissions,
};
use crate::outbox::{ImCoreAgentOutbox, RuntimeMessageSend, RuntimeOutbox};
use crate::registration::UserServiceAgentRegistrationClient;
use crate::runtime::host::run_controller_text_task;
use crate::runtime::{
    RuntimeInstallStatus, RuntimeLaunchContext, RuntimeLaunchOutcome, RuntimePlugin,
    RuntimeRunStatus,
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
    let exit_reason = loop {
        let newly_processed = process_inbox_once(&config, &state, &im_core, &mut processed).await?;
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
                run_runtime_task_command(config, state, outbox, payload_message)?;
            } else {
                handle_agent_payload_message(
                    config,
                    state,
                    registration,
                    &outbox,
                    payload_message,
                )?;
                sync_configured_agent_identities(config, state, im_core)?;
            }
            Ok(true)
        }
        MessageBodyView::Text { text, .. } => {
            let profile = state.load_runtime_agent_profile(target_agent_did)?;
            let runtime_outbox = ControllerRuntimeOutbox::new(
                ControllerOutboxSender::ImCore(outbox.clone()),
                sender_did.clone(),
                format!("task_{}", message.id.as_str()),
                conversation_id.clone(),
                Arc::new(std::sync::atomic::AtomicUsize::new(0)),
                Arc::new(Mutex::new(Vec::new())),
            );
            let plugin = UdsTestRuntimePlugin::new(config.local_socket_path.clone());
            run_controller_text_task(
                state,
                &profile,
                &plugin,
                &runtime_outbox,
                ControllerTextMessage {
                    message_id: message.id.as_str().to_string(),
                    conversation_id,
                    sender_did,
                    target_agent_did: target_agent_did.to_string(),
                    text: text.clone(),
                },
            )?;
            Ok(true)
        }
        MessageBodyView::Unsupported { .. } => Ok(false),
    }
}

fn run_runtime_task_command(
    config: &DaemonConfig,
    state: &DaemonState,
    outbox: ImCoreAgentOutbox,
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
    let runtime_outbox = ControllerRuntimeOutbox::new(
        ControllerOutboxSender::ImCore(outbox),
        message.sender_did.clone(),
        format!("task_{message_id}"),
        message.conversation_id.clone(),
        Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        Arc::new(Mutex::new(Vec::new())),
    );
    let plugin = UdsTestRuntimePlugin::new(config.local_socket_path.clone());
    run_controller_text_task(
        state,
        &profile,
        &plugin,
        &runtime_outbox,
        ControllerTextMessage {
            message_id,
            conversation_id: message.conversation_id,
            sender_did: message.sender_did,
            target_agent_did,
            text: payload.text,
        },
    )?;
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
    inner: ControllerOutboxSender,
    recipient_did: String,
    task_id: String,
    sent_counter: Arc<std::sync::atomic::AtomicUsize>,
    sent_message_ids: Arc<Mutex<Vec<String>>>,
}

impl ControllerRuntimeOutbox {
    fn new(
        inner: ControllerOutboxSender,
        recipient_did: impl Into<String>,
        task_id: impl Into<String>,
        _conversation_id: Option<String>,
        sent_counter: Arc<std::sync::atomic::AtomicUsize>,
        sent_message_ids: Arc<Mutex<Vec<String>>>,
    ) -> Self {
        Self {
            inner,
            recipient_did: recipient_did.into(),
            task_id: task_id.into(),
            sent_counter,
            sent_message_ids,
        }
    }

    fn send_status_payload(&self, recipient_did: &str, payload: Value) -> Result<()> {
        let message_id = self.inner.send_payload(recipient_did, payload)?;
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
        }
    }

    fn send_runtime_message(&self, message: &RuntimeMessageSend) -> Result<String> {
        match self {
            Self::ImCore(outbox) => Ok(outbox
                .send_text(&message.recipient, &message.text, message.security)?
                .message
                .id
                .as_str()
                .to_string()),
            Self::Mock => Ok(format!("mock-message-{}", message.security.as_str())),
        }
    }
}

impl RuntimeOutbox for ControllerRuntimeOutbox {
    fn send_status(
        &self,
        context: &crate::state::AuthorizedRuntimeContext,
        state: &str,
        text: Option<&str>,
    ) -> Result<()> {
        self.send_status_payload(
            &self.recipient_did,
            json!({
                "schema": "awiki.agent.status.v1",
                "task_id": self.task_id.clone(),
                "run_id": context.run_id.clone(),
                "state": state,
                "message": text,
            }),
        )
    }

    fn send_final(
        &self,
        context: &crate::state::AuthorizedRuntimeContext,
        text: Option<&str>,
    ) -> Result<()> {
        self.send_status_payload(
            &self.recipient_did,
            json!({
                "schema": "awiki.agent.status.v1",
                "task_id": self.task_id.clone(),
                "run_id": context.run_id.clone(),
                "state": "finished",
                "message": text,
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
    ) -> Result<()> {
        let _message_id = self.inner.send_runtime_message(message)?;
        Ok(())
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
        let inner = if self.mock_status_outbox {
            ControllerOutboxSender::Mock
        } else {
            let identity = self.state.load_agent_identity(&context.agent_did)?;
            let jwt_token = self.state.load_agent_auth_token(&context.agent_did)?;
            let client = self.im_core.client_for_agent_identity(
                &self.config,
                &identity,
                jwt_token.as_deref(),
            )?;
            ControllerOutboxSender::ImCore(ImCoreAgentOutbox::new(client))
        };
        Ok(ControllerRuntimeOutbox::new(
            inner,
            task.sender_did,
            task.task_id,
            task.conversation_id,
            Arc::clone(&self.sent_counter),
            Arc::clone(&self.sent_message_ids),
        ))
    }
}

impl RuntimeOutbox for RuntimeCallbackOutbox {
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
    ) -> Result<()> {
        self.controller_outbox(context)?
            .send_message(context, message)
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
