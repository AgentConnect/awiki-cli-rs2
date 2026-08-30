use super::bridge;
use super::host_notify_sink::{new_host_notify_sink, HostNotifySink, HostNotifySinkImpl};
use super::listener::{self, SessionStatus, Status};
use super::listener_bridge_connection::{
    execute_listener_bridge_request, ListenerBridgeRuntime, ListenerBridgeSession,
};
use super::listener_known_sessions::{
    known_session_startup_decision, record_known_session_startup_error,
    KnownSessionStartupDecision, KnownSessionStartupError,
};
use super::listener_session_methods::SessionDisconnectReason;
use super::listener_shutdown_signal::{
    install_foreground_shutdown_handler, wait_for_foreground_shutdown,
    wait_for_foreground_shutdown_async,
};
use crate::host_runtime;
use crate::host_runtime::listener_im_event_adapter::{CliImEventRoute, CliRealtimeEventSink};
use crate::m_core_cli_adapter::realtime::{
    self as im_core_realtime_adapter, ListenerRunHostKind, ListenerRunnerMode,
};
use crate::workspace_config::Resolved;
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::fs;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::thread;
use std::time::{Duration, Instant};

const RELIABLE_SYNC_LIMIT: u32 = 100;
const RELIABLE_SYNC_RECONCILE_INTERVAL: Duration = Duration::from_secs(5 * 60);
const RELIABLE_SYNC_INITIAL_RETRY_DELAY: Duration = Duration::from_secs(1);
const RELIABLE_SYNC_MAX_RETRY_DELAY: Duration = Duration::from_secs(60);

pub fn run_foreground(resolved: Resolved) -> anyhow::Result<()> {
    run_listener(resolved, ListenerRunHostKind::Foreground)
}

pub async fn run_foreground_async(resolved: Resolved) -> anyhow::Result<()> {
    run_listener_async(resolved, ListenerRunHostKind::Foreground).await
}

pub fn run_service(resolved: Resolved) -> anyhow::Result<()> {
    run_listener(resolved, ListenerRunHostKind::Service)
}

pub async fn run_service_async(resolved: Resolved) -> anyhow::Result<()> {
    run_listener_async(resolved, ListenerRunHostKind::Service).await
}

fn run_listener(resolved: Resolved, host: ListenerRunHostKind) -> anyhow::Result<()> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|err| anyhow::anyhow!("build listener runtime: {err}"))?
        .block_on(run_listener_async(resolved, host))
}

async fn run_listener_async(resolved: Resolved, host: ListenerRunHostKind) -> anyhow::Result<()> {
    let runner = im_core_realtime_adapter::listener_runner_selection(host);
    let core = crate::m_core_cli_adapter::build_im_core_async(&resolved).await?;
    let mut supervisor = ListenerSupervisor::new(resolved, core, runner.mode, host)?;
    let result = supervisor.run_async().await;
    supervisor.cleanup_runtime_artifacts();
    result
}

struct ListenerSupervisor {
    resolved: Arc<Resolved>,
    core: im_core::ImCore,
    status: Arc<Mutex<Status>>,
    shutdown: Arc<AtomicBool>,
    listener: Option<bridge::BridgeListener>,
    host_notify: Arc<HostNotifySinkImpl>,
    clients: Arc<Mutex<HashMap<String, im_core::ImClient>>>,
    runner_mode: ListenerRunnerMode,
    host: ListenerRunHostKind,
}

impl ListenerSupervisor {
    fn new(
        resolved: Resolved,
        core: im_core::ImCore,
        runner_mode: ListenerRunnerMode,
        host: ListenerRunHostKind,
    ) -> anyhow::Result<Self> {
        let runtime_paths = listener::paths(&resolved)?;
        let boot_id = super::listener_service::resolve_runtime_boot_id(&resolved)?;
        let (host_notify, host_notify_status) = new_host_notify_sink(&resolved)?;
        let status = Status {
            mode: host_runtime::resolve(&resolved).mode,
            installed: running_in_listener_service_mode(),
            running: true,
            boot_id,
            pid: i64::from(std::process::id()),
            pid_file: runtime_paths.pid_file,
            log_file: runtime_paths.log_file,
            status_file: runtime_paths.status_file,
            socket_path: runtime_paths.socket_path,
            service_name: super::listener_service::service_name_for(&resolved),
            service_platform: if running_in_listener_service_mode() {
                super::listener_service_manager::service_platform().to_string()
            } else {
                "rust-local".to_string()
            },
            host_notify: host_notify_status,
            ..Status::default()
        };
        Ok(Self {
            resolved: Arc::new(resolved),
            core,
            status: Arc::new(Mutex::new(status)),
            shutdown: Arc::new(AtomicBool::new(false)),
            listener: None,
            host_notify: Arc::new(host_notify),
            clients: Arc::new(Mutex::new(HashMap::new())),
            runner_mode,
            host,
        })
    }

    async fn run_async(&mut self) -> anyhow::Result<()> {
        install_foreground_shutdown_handler(self.shutdown.clone())?;
        if host_runtime::resolve(&self.resolved).mode != bridge::MODE_WEBSOCKET {
            anyhow::bail!("runtime mode must be websocket before starting the listener");
        }
        if self.runner_mode == ListenerRunnerMode::ImCore
            && !im_core_realtime_adapter::runtime_mode_supports_sdk_runner(&self.resolved)
        {
            anyhow::bail!(
                "{} requires websocket runtime mode for the SDK runner",
                im_core_realtime_adapter::runner_host_label(self.host)
            );
        }
        {
            let status = self.lock_status();
            listener::write_pid(&status.pid_file, status.pid)?;
            listener::write_status(&status.status_file, &status)?;
        }
        self.start_socket()?;
        self.start_known_sessions_async().await?;
        wait_for_shutdown_signal_async(self.shutdown.clone()).await;
        self.shutdown.store(true, Ordering::SeqCst);
        if let Some(listener) = self.listener.take() {
            drop(listener);
        }
        let _ = self.host_notify.close();
        Ok(())
    }

    fn start_socket(&mut self) -> anyhow::Result<()> {
        let socket_path = self.lock_status().socket_path.clone();
        let listener = bridge::listen_bridge(&socket_path)?;
        bridge::set_bridge_listener_nonblocking(&listener, true)?;
        self.listener = Some(bridge::clone_bridge_listener(&listener)?);
        self.set_bridge_available(true);
        let status = self.status.clone();
        let resolved = self.resolved.clone();
        let host_notify = self.host_notify.clone();
        let clients = self.clients.clone();
        let runtime = tokio::runtime::Handle::current();
        let shutdown = self.shutdown.clone();
        thread::spawn(move || {
            while !shutdown.load(Ordering::SeqCst) {
                match bridge::accept_bridge(&listener) {
                    Ok(stream) => {
                        let runtime = BridgeRuntime {
                            status: status.clone(),
                            host_notify: host_notify.clone(),
                            resolved: resolved.clone(),
                            clients: clients.clone(),
                            runtime: runtime.clone(),
                        };
                        thread::spawn(move || handle_bridge_stream(stream, runtime));
                    }
                    Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(50));
                    }
                    Err(_) => break,
                }
            }
        });
        Ok(())
    }

    async fn start_known_sessions_async(&self) -> anyhow::Result<()> {
        let identities = self.core.identities().list_async().await?;
        let sessions: Vec<(String, String)> = identities
            .iter()
            .map(|summary| {
                (
                    summary
                        .local_alias
                        .clone()
                        .unwrap_or_else(|| summary.id.as_str().to_owned()),
                    summary.did.as_str().to_owned(),
                )
            })
            .collect();
        {
            let mut status = self.lock_status();
            for (identity_name, did) in &sessions {
                upsert_session(
                    &mut status,
                    SessionStatus {
                        identity_name: identity_name.clone(),
                        did: did.clone(),
                        connected: false,
                        last_error: String::new(),
                    },
                );
            }
            status.reliable_sync.v2_subprotocol_negotiated = false;
            let _ = listener::write_status(&status.status_file, &status);
        }
        for (identity_name, did) in sessions {
            spawn_im_core_runner_session_async(
                self.core.clone(),
                self.status.clone(),
                self.host_notify.clone(),
                self.clients.clone(),
                self.shutdown.clone(),
                identity_name,
                did,
            )
            .await;
        }
        self.refresh_status();
        Ok(())
    }

    async fn ensure_known_session_async(
        &self,
        identity_name: &str,
        did: &str,
    ) -> anyhow::Result<()> {
        let already_known = self
            .lock_status()
            .sessions
            .iter()
            .any(|session| session.identity_name == identity_name);
        let KnownSessionStartupDecision::StartAndWait { .. } =
            known_session_startup_decision(identity_name, already_known)
        else {
            return Ok(());
        };
        spawn_im_core_runner_session_async(
            self.core.clone(),
            self.status.clone(),
            self.host_notify.clone(),
            self.clients.clone(),
            self.shutdown.clone(),
            identity_name.to_string(),
            did.to_string(),
        )
        .await;
        Ok(())
    }

    fn record_session_error(&self, identity_name: &str, did: &str, error: &str) {
        let mut status = self.lock_status();
        record_known_session_startup_error(
            &mut status,
            KnownSessionStartupError {
                identity_name: identity_name.to_string(),
                did: did.to_string(),
                error: error.to_string(),
            },
        );
        let _ = listener::write_status(&status.status_file, &status);
    }

    fn set_bridge_available(&self, available: bool) {
        let mut status = self.lock_status();
        if status.bridge_available == available {
            return;
        }
        status.bridge_available = available;
        let _ = listener::write_status(&status.status_file, &status);
    }

    fn refresh_status(&self) {
        let status = self.lock_status();
        let _ = listener::write_status(&status.status_file, &status);
    }

    fn cleanup_runtime_artifacts(&self) {
        let status = self.lock_status();
        cleanup_runtime_artifacts(&self.resolved, &status.boot_id);
    }

    fn lock_status(&self) -> std::sync::MutexGuard<'_, Status> {
        self.status.lock().expect("listener status mutex poisoned")
    }
}

struct BridgeRuntime {
    status: Arc<Mutex<Status>>,
    host_notify: Arc<HostNotifySinkImpl>,
    resolved: Arc<Resolved>,
    clients: Arc<Mutex<HashMap<String, im_core::ImClient>>>,
    runtime: tokio::runtime::Handle,
}

impl ListenerBridgeRuntime for BridgeRuntime {
    fn ensure_session(&mut self, identity_name: &str) -> anyhow::Result<ListenerBridgeSession> {
        Ok(ListenerBridgeSession::disconnected(
            identity_name.trim().to_string(),
            None,
        ))
    }

    fn fetch_message_service_did(
        &mut self,
        session: &ListenerBridgeSession,
    ) -> anyhow::Result<String> {
        anyhow::bail!(
            "{}",
            super::listener_service_did::disconnected_websocket_session_error(
                &session.identity_name
            )
        )
    }

    fn send_rpc(
        &mut self,
        session: &ListenerBridgeSession,
        method: &str,
        params: Value,
    ) -> anyhow::Result<Map<String, Value>> {
        let _ = (method, params);
        anyhow::bail!(
            "{}",
            super::listener_service_did::disconnected_websocket_session_error(
                &session.identity_name
            )
        )
    }

    fn mark_messages_read(
        &mut self,
        owner_did: &str,
        message_ids: &[String],
    ) -> anyhow::Result<()> {
        let _ = (owner_did, message_ids);
        Ok(())
    }
}

impl Drop for BridgeRuntime {
    fn drop(&mut self) {
        let _ = self.host_notify.close();
        let _ = self
            .status
            .lock()
            .map(|status| listener::write_status(&status.status_file, &status));
    }
}

impl BridgeRuntime {
    fn execute_local_request(
        &self,
        request: &bridge::BridgeRequest,
    ) -> anyhow::Result<Map<String, Value>> {
        let client = self
            .clients
            .lock()
            .map_err(|_| anyhow::anyhow!("listener client registry is unavailable"))?
            .get(request.identity_name.trim())
            .cloned()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "{}",
                    super::listener_service_did::disconnected_websocket_session_error(
                        request.identity_name.trim()
                    )
                )
            })?;
        let result = match request.method.as_str() {
            "local.inbox" => {
                let query = serde_json::from_value::<im_core::messages::InboxQuery>(
                    request
                        .params
                        .get("query")
                        .cloned()
                        .ok_or_else(|| anyhow::anyhow!("local inbox query is missing"))?,
                )?;
                self.runtime.block_on(async {
                    let mut warnings = listener_local_inbox_transport_or_warning(
                        crate::m_core_cli_adapter::messages::reconcile_foreground_message_sync_async(
                            &client,
                        )
                        .await,
                        "sync.foreground_reconcile_deferred",
                    )
                    .map_err(|error| {
                        anyhow::anyhow!(
                            "local inbox reconciliation failed: {}",
                            local_inbox_reconciliation_error_category(&error)
                        )
                    })?;
                    warnings.extend(
                        listener_local_inbox_secure_hydration_or_warning(
                            crate::m_core_cli_adapter::messages::hydrate_secure_inbox_via_im_core_async(
                                &client, &query,
                            )
                            .await,
                        )
                        .map_err(|error| {
                            anyhow::anyhow!(
                                "local inbox secure hydration failed: {}",
                                local_inbox_reconciliation_error_category(&error)
                            )
                        })?,
                    );
                    crate::m_core_cli_adapter::messages::read_local_inbox_projection_via_im_core_async(
                        &client,
                        query,
                        warnings,
                    )
                    .await
                    .map_err(|error| {
                        anyhow::anyhow!(
                            "local inbox reconciliation/read failed: {}",
                            local_inbox_reconciliation_error_category(&error)
                        )
                    })
                })?
            }
            "local.history" => {
                let thread = serde_json::from_value::<im_core::messages::ThreadRef>(
                    request
                        .params
                        .get("thread")
                        .cloned()
                        .ok_or_else(|| anyhow::anyhow!("local history thread is missing"))?,
                )?;
                let query = serde_json::from_value::<im_core::messages::HistoryQuery>(
                    request
                        .params
                        .get("query")
                        .cloned()
                        .ok_or_else(|| anyhow::anyhow!("local history query is missing"))?,
                )?;
                self.runtime.block_on(
                    crate::m_core_cli_adapter::messages::read_history_via_im_core_async(
                        &self.resolved,
                        &client,
                        thread,
                        query,
                    ),
                )?
            }
            "local.mark_read" => {
                let message_ids = request
                    .params
                    .get("message_ids")
                    .and_then(Value::as_array)
                    .ok_or_else(|| anyhow::anyhow!("local mark-read message_ids are missing"))?
                    .iter()
                    .map(|value| {
                        value
                            .as_str()
                            .filter(|value| !value.trim().is_empty())
                            .map(str::to_owned)
                            .ok_or_else(|| anyhow::anyhow!("local mark-read message id is invalid"))
                    })
                    .collect::<anyhow::Result<Vec<_>>>()?;
                self.runtime.block_on(
                    crate::m_core_cli_adapter::messages::mark_read_via_im_core_async(
                        &self.resolved,
                        &client,
                        message_ids,
                    ),
                )?
            }
            _ => anyhow::bail!("unsupported local listener bridge method"),
        };
        Ok(Map::from_iter([
            ("data".to_owned(), result.data),
            ("summary".to_owned(), Value::String(result.summary)),
            (
                "warnings".to_owned(),
                Value::Array(result.warnings.into_iter().map(Value::String).collect()),
            ),
        ]))
    }
}

fn listener_local_inbox_secure_hydration_or_warning(
    result: Result<Vec<String>, crate::m_core_cli_adapter::message_result::MessageAdapterError>,
) -> Result<Vec<String>, crate::m_core_cli_adapter::message_result::MessageAdapterError> {
    use crate::m_core_cli_adapter::message_result::MessageAdapterError;

    match result {
        Ok(warnings) => Ok(warnings),
        Err(MessageAdapterError::TransportUnavailable(_)) => {
            Ok(vec!["sync.secure_inbox_hydration_deferred".to_owned()])
        }
        Err(MessageAdapterError::LocalStateUnavailable(_)) => Ok(vec![
            "sync.secure_inbox_hydration_local_state_deferred".to_owned(),
        ]),
        Err(error) => Err(error),
    }
}

fn listener_local_inbox_transport_or_warning(
    result: Result<Vec<String>, crate::m_core_cli_adapter::message_result::MessageAdapterError>,
    warning: &'static str,
) -> Result<Vec<String>, crate::m_core_cli_adapter::message_result::MessageAdapterError> {
    match result {
        Ok(warnings) => Ok(warnings),
        Err(
            crate::m_core_cli_adapter::message_result::MessageAdapterError::TransportUnavailable(_),
        ) => Ok(vec![warning.to_owned()]),
        Err(error) => Err(error),
    }
}

fn local_inbox_reconciliation_error_category(
    error: &crate::m_core_cli_adapter::message_result::MessageAdapterError,
) -> &'static str {
    use crate::m_core_cli_adapter::message_result::MessageAdapterError;

    match error {
        MessageAdapterError::TransportUnavailable(_) => "transport_unavailable",
        MessageAdapterError::LocalStateUnavailable(_) => "local_state_unavailable",
        MessageAdapterError::IdentityRequired(_) => "identity_required",
        MessageAdapterError::PermissionDenied => "permission_denied",
        MessageAdapterError::PublicServiceCode(_) | MessageAdapterError::Service(_) => {
            "service_error"
        }
        MessageAdapterError::Identity(_) => "identity_error",
        _ => "other",
    }
}

fn handle_bridge_stream(stream: bridge::BridgeStream, mut runtime: BridgeRuntime) {
    let _ = bridge::handle_bridge_connection_once(stream, |request| {
        let method = request.method.clone();
        let result = if method.starts_with("local.") {
            runtime.execute_local_request(&request)
        } else {
            execute_listener_bridge_request(&mut runtime, request)
        };
        if let Err(error) = &result {
            eprintln!("listener bridge request {method} failed: {error:#}");
        }
        result
    });
}

async fn spawn_im_core_runner_session_async(
    core: im_core::ImCore,
    status: Arc<Mutex<Status>>,
    host_notify: Arc<HostNotifySinkImpl>,
    clients: Arc<Mutex<HashMap<String, im_core::ImClient>>>,
    shutdown: Arc<AtomicBool>,
    identity_name: String,
    did: String,
) {
    {
        let mut guard = status.lock().expect("listener status mutex poisoned");
        upsert_session(
            &mut guard,
            SessionStatus {
                identity_name: identity_name.clone(),
                did: did.clone(),
                connected: false,
                last_error: String::new(),
            },
        );
        guard.reliable_sync.v2_subprotocol_negotiated = false;
        let _ = listener::write_status(&guard.status_file, &guard);
    }
    let selector = im_core::IdentitySelector::LocalAlias(identity_name.clone());
    let client = match core.client_async(selector).await {
        Ok(client) => client,
        Err(err) => {
            mark_session_disconnected(
                &status,
                &identity_name,
                did,
                Some(SessionDisconnectReason::Other(err.to_string())),
            );
            return;
        }
    };
    let did = client.did().as_str().to_string();
    if let Err(error) = client.active_sync_account_binding().await {
        mark_session_disconnected(
            &status,
            &identity_name,
            did,
            Some(SessionDisconnectReason::Other(v2_binding_error_text(
                &error,
            ))),
        );
        return;
    }
    // Seed the exact-device notification projection before the realtime
    // session can report readiness or the first account Sync v2 run can
    // commit. Sync v2 publishes committed notification changes only to an
    // initialized Core watch store.
    let mut system_notifications = match watch_system_notifications(&client).await {
        Ok(session) => session,
        Err(error) => {
            mark_session_disconnected(
                &status,
                &identity_name,
                did,
                Some(SessionDisconnectReason::Other(error.to_string())),
            );
            return;
        }
    };
    let mut session = match client
        .realtime()
        .start_async(im_core_realtime_adapter::listener_realtime_options())
        .await
        .map_err(im_core_realtime_adapter::map_sdk_runner_error)
    {
        Ok(session) => session,
        Err(err) => {
            mark_session_disconnected(
                &status,
                &identity_name,
                did,
                Some(SessionDisconnectReason::Other(err.to_string())),
            );
            return;
        }
    };
    let mut events = match session.subscribe() {
        Ok(events) => events,
        Err(err) => {
            mark_session_disconnected(
                &status,
                &identity_name,
                did,
                Some(SessionDisconnectReason::Other(err.to_string())),
            );
            return;
        }
    };
    {
        let mut registry = clients
            .lock()
            .expect("listener client registry mutex poisoned");
        registry.insert(identity_name.clone(), client.clone());
        registry.insert(did.clone(), client.clone());
    }
    tokio::spawn(async move {
        let mut event_error = None;
        let mut scheduler = ListenerSyncScheduler::new();
        let mut notification_state = ListenerSystemNotificationState::default();
        let mut sync_task = None;
        let mut reconcile_timer = tokio::time::interval_at(
            tokio::time::Instant::now() + RELIABLE_SYNC_RECONCILE_INTERVAL,
            RELIABLE_SYNC_RECONCILE_INTERVAL,
        );
        reconcile_timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            if shutdown.load(Ordering::SeqCst) {
                let _ = session.stop().await;
                break;
            }
            if sync_task.is_none() {
                if let Some(reason) = scheduler.take_ready(Instant::now()) {
                    let sync_client = client.clone();
                    sync_task = Some(tokio::spawn(async move {
                        sync_client
                            .messages()
                            .request_sync_async(im_core::messages::MessageSyncRequest {
                                reason: reason.as_str().to_owned(),
                                limit: Some(RELIABLE_SYNC_LIMIT),
                            })
                            .await
                    }));
                }
            }
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_millis(100)) => {}
                _ = reconcile_timer.tick() => {
                    scheduler.request(ListenerSyncReason::Timer);
                }
                event = events.recv() => {
                    let Some(event) = event else {
                        break;
                    };
                    let mut event_sink = CliRealtimeEventSink {
                        status: &status,
                        host_notify: &host_notify,
                        identity_name: &identity_name,
                        did: &did,
                    };
                    match event_sink.emit_remote(event) {
                        Ok(result) => {
                            if result.route == CliImEventRoute::ConnectionStateChanged {
                                record_v2_subprotocol_negotiated(&status);
                            }
                            if result.connection_became_connected {
                                scheduler.observe_connected_transition();
                            }
                            if result.reliable_sync_requested {
                                scheduler.request(ListenerSyncReason::RealtimeHint);
                            }
                        }
                        Err(err) => {
                            event_error = Some(err.to_string());
                            let _ = session.stop().await;
                            break;
                        }
                    }
                }
                change = system_notifications.next_change() => {
                    let Some(change) = change else {
                        match watch_system_notifications(&client).await {
                            Ok(rebuilt) => {
                                system_notifications = rebuilt;
                                continue;
                            }
                            Err(error) => {
                                event_error = Some(error.to_string());
                                let _ = session.stop().await;
                                break;
                            }
                        }
                    };
                    match notification_state.plan(change) {
                        ListenerSystemNotificationPlan::Emit(items) => {
                            for item in items {
                                if let Err(error) = emit_listener_system_notification(
                                    &status,
                                    &host_notify,
                                    &identity_name,
                                    &did,
                                    item,
                                ) {
                                    event_error = Some(error.to_string());
                                    let _ = session.stop().await;
                                    break;
                                }
                            }
                            if event_error.is_some() {
                                break;
                            }
                        }
                        ListenerSystemNotificationPlan::Rebuild => {
                            match watch_system_notifications(&client).await {
                                Ok(rebuilt) => system_notifications = rebuilt,
                                Err(error) => {
                                    event_error = Some(error.to_string());
                                    let _ = session.stop().await;
                                    break;
                                }
                            }
                        }
                    }
                }
                sync_result = async {
                    sync_task
                        .as_mut()
                        .expect("guarded reliable sync task")
                        .await
                }, if sync_task.is_some() => {
                    sync_task = None;
                    match sync_result {
                        Ok(Ok(outcome)) => {
                            if let Err(error) = emit_committed_sync_messages(
                                &status,
                                &host_notify,
                                &identity_name,
                                &did,
                                &outcome,
                            ) {
                                event_error = Some(error.to_string());
                                let _ = session.stop().await;
                                break;
                            }
                            match record_reliable_sync_outcome(&status, outcome.status) {
                                ListenerSyncDisposition::Success => {
                                    scheduler.complete_success();
                                }
                                ListenerSyncDisposition::Retryable => {
                                    scheduler.complete_retryable(Instant::now());
                                }
                                ListenerSyncDisposition::Blocked => {
                                    event_error = Some("reliable v2 sync is blocked and requires intervention".to_owned());
                                    let _ = session.stop().await;
                                    break;
                                }
                                ListenerSyncDisposition::AuthRevoked => {
                                    event_error = Some("reliable v2 sync authorization was revoked".to_owned());
                                    let _ = session.stop().await;
                                    break;
                                }
                            }
                        }
                        Ok(Err(error)) => {
                            if listener_sync_error_is_terminal(&error) {
                                event_error = Some("reliable v2 sync authorization is unavailable".to_owned());
                                let _ = session.stop().await;
                                break;
                            }
                            scheduler.complete_retryable(Instant::now());
                        }
                        Err(_) => {
                            scheduler.complete_retryable(Instant::now());
                        }
                    }
                }
            }
        }
        if let Some(task) = sync_task.take() {
            task.abort();
            let _ = task.await;
        }
        let exit = session
            .join()
            .await
            .map_err(im_core_realtime_adapter::map_sdk_runner_error);
        if shutdown.load(Ordering::SeqCst) {
            remove_listener_client(&clients, &identity_name, &did);
            mark_session_disconnected(
                &status,
                &identity_name,
                did,
                Some(SessionDisconnectReason::ContextCanceled),
            );
            return;
        }
        if let Some(error) = event_error {
            remove_listener_client(&clients, &identity_name, &did);
            mark_session_disconnected(
                &status,
                &identity_name,
                did,
                Some(SessionDisconnectReason::Other(error)),
            );
            return;
        }
        let error = match exit {
            Ok(exit) => realtime_exit_error_text(&exit),
            Err(err) => err.to_string(),
        };
        remove_listener_client(&clients, &identity_name, &did);
        mark_session_disconnected(
            &status,
            &identity_name,
            did,
            Some(SessionDisconnectReason::Other(error)),
        );
    });
}

fn remove_listener_client(
    clients: &Arc<Mutex<HashMap<String, im_core::ImClient>>>,
    identity_name: &str,
    did: &str,
) {
    if let Ok(mut registry) = clients.lock() {
        registry.remove(identity_name);
        registry.remove(did);
    }
}

fn emit_listener_system_notification(
    status: &Arc<Mutex<Status>>,
    host_notify: &Arc<HostNotifySinkImpl>,
    identity_name: &str,
    did: &str,
    notification: im_core::system_notifications::SystemNotificationSnapshot,
) -> im_core::ImResult<()> {
    let mut event_sink = CliRealtimeEventSink {
        status,
        host_notify,
        identity_name,
        did,
    };
    event_sink.emit_remote(im_core::prelude::ImEvent::SystemNotificationChanged(
        im_core::prelude::SystemNotificationChangedEvent {
            notification,
            sync: None,
        },
    ))?;
    Ok(())
}

async fn watch_system_notifications(
    client: &im_core::ImClient,
) -> im_core::ImResult<im_core::system_notifications::SystemNotificationChangeSession> {
    client
        .system_notifications()
        .watch(im_core::system_notifications::SystemNotificationListQuery {
            limit: Some(500),
            include_terminal: false,
        })
        .await
}

#[derive(Debug, Default)]
struct ListenerSystemNotificationState {
    latest_session_revision: HashMap<String, u64>,
}

#[derive(Debug, PartialEq, Eq)]
enum ListenerSystemNotificationPlan {
    Emit(Vec<im_core::system_notifications::SystemNotificationSnapshot>),
    Rebuild,
}

impl ListenerSystemNotificationState {
    fn plan(
        &mut self,
        change: im_core::system_notifications::SystemNotificationChange,
    ) -> ListenerSystemNotificationPlan {
        use im_core::system_notifications::SystemNotificationChange;

        match change {
            SystemNotificationChange::Reset { items } => {
                ListenerSystemNotificationPlan::Emit(self.admit(items))
            }
            SystemNotificationChange::Changed { item } => {
                ListenerSystemNotificationPlan::Emit(self.admit([item]))
            }
            SystemNotificationChange::RepairRequired { .. } => {
                ListenerSystemNotificationPlan::Rebuild
            }
        }
    }

    fn admit(
        &mut self,
        items: impl IntoIterator<Item = im_core::system_notifications::SystemNotificationSnapshot>,
    ) -> Vec<im_core::system_notifications::SystemNotificationSnapshot> {
        let mut admitted = Vec::new();
        for item in items {
            let session_id = item.join_session_id.trim();
            if session_id.is_empty()
                || self
                    .latest_session_revision
                    .get(session_id)
                    .is_some_and(|revision| *revision >= item.session_revision)
            {
                continue;
            }
            self.latest_session_revision
                .insert(session_id.to_owned(), item.session_revision);
            admitted.push(item);
        }
        admitted
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ListenerSyncReason {
    Startup,
    Reconnect,
    RealtimeHint,
    Timer,
    Retry,
}

impl ListenerSyncReason {
    fn as_str(self) -> &'static str {
        match self {
            Self::Startup => "session_start",
            Self::Reconnect => "websocket_reconnect",
            Self::RealtimeHint => "websocket_hint",
            Self::Timer | Self::Retry => "foreground_reconcile",
        }
    }
}

#[derive(Debug)]
struct ListenerSyncScheduler {
    pending: Option<ListenerSyncReason>,
    retry_not_before: Option<Instant>,
    retry_delay: Duration,
    observed_connected_transition: bool,
}

impl ListenerSyncScheduler {
    fn new() -> Self {
        Self {
            pending: Some(ListenerSyncReason::Startup),
            retry_not_before: None,
            retry_delay: RELIABLE_SYNC_INITIAL_RETRY_DELAY,
            observed_connected_transition: false,
        }
    }

    fn request(&mut self, reason: ListenerSyncReason) {
        if self.pending.is_none() {
            self.pending = Some(reason);
        }
    }

    fn observe_connected_transition(&mut self) {
        if self.observed_connected_transition {
            self.request(ListenerSyncReason::Reconnect);
        } else {
            self.observed_connected_transition = true;
        }
    }

    fn take_ready(&mut self, now: Instant) -> Option<ListenerSyncReason> {
        if self
            .retry_not_before
            .is_some_and(|not_before| now < not_before)
        {
            return None;
        }
        let reason = self.pending.take()?;
        self.retry_not_before = None;
        Some(reason)
    }

    fn complete_success(&mut self) {
        self.retry_not_before = None;
        self.retry_delay = RELIABLE_SYNC_INITIAL_RETRY_DELAY;
    }

    fn complete_retryable(&mut self, now: Instant) {
        self.pending = Some(ListenerSyncReason::Retry);
        self.retry_not_before = Some(now + self.retry_delay);
        self.retry_delay = self
            .retry_delay
            .saturating_mul(2)
            .min(RELIABLE_SYNC_MAX_RETRY_DELAY);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ListenerSyncDisposition {
    Success,
    Retryable,
    Blocked,
    AuthRevoked,
}

fn listener_sync_disposition(
    status: im_core::messages::MessageSyncStatus,
) -> ListenerSyncDisposition {
    match status {
        im_core::messages::MessageSyncStatus::Idle
        | im_core::messages::MessageSyncStatus::Changed => ListenerSyncDisposition::Success,
        im_core::messages::MessageSyncStatus::RecoveryRequired
        | im_core::messages::MessageSyncStatus::RetryableFailure => {
            ListenerSyncDisposition::Retryable
        }
        im_core::messages::MessageSyncStatus::Blocked => ListenerSyncDisposition::Blocked,
        im_core::messages::MessageSyncStatus::AuthRevoked => ListenerSyncDisposition::AuthRevoked,
    }
}

fn record_reliable_sync_outcome(
    listener_status: &Arc<Mutex<Status>>,
    sync_status: im_core::messages::MessageSyncStatus,
) -> ListenerSyncDisposition {
    let disposition = listener_sync_disposition(sync_status);
    if disposition == ListenerSyncDisposition::Success {
        record_v2_bootstrap_completed(listener_status);
    }
    disposition
}

fn listener_sync_error_is_terminal(error: &im_core::ImError) -> bool {
    matches!(
        error,
        im_core::ImError::AuthRequired
            | im_core::ImError::SessionExpired
            | im_core::ImError::PermissionDenied
            | im_core::ImError::IdentityBindingConflict { .. }
            | im_core::ImError::UnsupportedCapability { .. }
    )
}

fn emit_committed_sync_messages(
    status: &Arc<Mutex<Status>>,
    host_notify: &Arc<HostNotifySinkImpl>,
    identity_name: &str,
    did: &str,
    outcome: &im_core::messages::MessageSyncOutcome,
) -> im_core::ImResult<()> {
    for committed in &outcome.committed_incoming_messages {
        let mut event_sink = CliRealtimeEventSink {
            status,
            host_notify,
            identity_name,
            did,
        };
        event_sink.emit_committed_message(committed.message.clone())?;
    }
    Ok(())
}

fn v2_binding_error_text(error: &im_core::ImError) -> String {
    if matches!(error, im_core::ImError::UnsupportedCapability { .. }) {
        return "reliable v2 sync requires a VNext device identity; run `awiki-cli onboarding migrate-legacy` for an existing Skill identity".to_owned();
    }
    format!("reliable v2 sync binding is unavailable: {error}")
}

fn record_v2_subprotocol_negotiated(status: &Arc<Mutex<Status>>) {
    let Ok(mut guard) = status.lock() else {
        return;
    };
    let all_sessions_connected =
        !guard.sessions.is_empty() && guard.sessions.iter().all(|session| session.connected);
    guard.reliable_sync.v2_subprotocol_negotiated = all_sessions_connected;
    let _ = listener::write_status(&guard.status_file, &guard);
}

fn record_v2_bootstrap_completed(status: &Arc<Mutex<Status>>) {
    update_reliable_sync_status(status, |reliable_sync| {
        reliable_sync.v2_bootstrap_completed = true;
        reliable_sync.last_reconcile_protocol = "sync_v2".to_owned();
        reliable_sync.legacy_sync_used = false;
    });
}

fn update_reliable_sync_status(
    status: &Arc<Mutex<Status>>,
    update: impl FnOnce(&mut crate::host_runtime::listener::ReliableSyncStatus),
) {
    let Ok(mut guard) = status.lock() else {
        return;
    };
    update(&mut guard.reliable_sync);
    let _ = listener::write_status(&guard.status_file, &guard);
}

fn realtime_exit_error_text(exit: &im_core::prelude::RealtimeExit) -> String {
    if let Some(warning) = exit.warnings.last() {
        return warning.clone();
    }
    match exit.reason {
        im_core::prelude::RealtimeExitReason::ShutdownRequested => "context canceled".to_string(),
        im_core::prelude::RealtimeExitReason::ConnectionClosed => {
            "websocket notification loop closed".to_string()
        }
        im_core::prelude::RealtimeExitReason::AuthFailed => {
            "listener authentication is required".to_string()
        }
        im_core::prelude::RealtimeExitReason::TransportUnavailable => {
            "runtime listener websocket transport unavailable".to_string()
        }
        im_core::prelude::RealtimeExitReason::FatalError => {
            "runtime listener SDK runner failed".to_string()
        }
    }
}

fn mark_session_disconnected(
    status: &Arc<Mutex<Status>>,
    identity_name: &str,
    did: String,
    error: Option<SessionDisconnectReason>,
) {
    let mut guard = status.lock().expect("listener status mutex poisoned");
    let last_error = disconnected_last_error(&guard, identity_name, error);
    let did = if did.is_empty() {
        guard
            .sessions
            .iter()
            .find(|session| session.identity_name == identity_name)
            .map(|session| session.did.clone())
            .unwrap_or_default()
    } else {
        did
    };
    upsert_session(
        &mut guard,
        SessionStatus {
            identity_name: identity_name.to_string(),
            did,
            connected: false,
            last_error,
        },
    );
    guard.reliable_sync.v2_subprotocol_negotiated = false;
    let _ = listener::write_status(&guard.status_file, &guard);
}

fn disconnected_last_error(
    status: &Status,
    identity_name: &str,
    error: Option<SessionDisconnectReason>,
) -> String {
    match error {
        Some(SessionDisconnectReason::Other(error)) if !error.is_empty() => error,
        _ => status
            .sessions
            .iter()
            .find(|session| session.identity_name == identity_name)
            .map(|session| session.last_error.clone())
            .unwrap_or_default(),
    }
}

fn upsert_session(status: &mut Status, session: SessionStatus) {
    if let Some(existing) = status
        .sessions
        .iter_mut()
        .find(|existing| existing.identity_name == session.identity_name)
    {
        *existing = session;
        return;
    }
    status.sessions.push(session);
    status
        .sessions
        .sort_by(|left, right| left.identity_name.cmp(&right.identity_name));
}

fn cleanup_runtime_artifacts(resolved: &Resolved, boot_id: &str) {
    if let Ok(path) = listener::boot_id_path(resolved) {
        let expected_boot_id = listener::read_expected_boot_id(&path).ok();
        if !super::listener_service::runtime_artifacts_belong_to_boot_id(
            expected_boot_id.as_deref(),
            boot_id,
        ) {
            return;
        }
    }
    if let Ok(paths) = listener::paths(resolved) {
        let _ = fs::remove_file(paths.pid_file);
        let _ = fs::remove_file(paths.status_file);
        let _ = fs::remove_file(paths.socket_path);
    }
}

fn running_in_listener_service_mode() -> bool {
    let env_value = std::env::var("AWIKI_LISTENER_SERVICE_MODE").unwrap_or_default();
    if env_value.trim() == "1" || env_value.trim().eq_ignore_ascii_case("true") {
        return true;
    }
    let args = std::env::args().collect::<Vec<_>>();
    args.windows(3).any(|words| {
        words[0].eq_ignore_ascii_case("runtime")
            && words[1].eq_ignore_ascii_case("listener")
            && words[2].eq_ignore_ascii_case("service-run")
    })
}

fn wait_for_shutdown_signal(shutdown: Arc<AtomicBool>) {
    wait_for_foreground_shutdown(&shutdown);
}

async fn wait_for_shutdown_signal_async(shutdown: Arc<AtomicBool>) {
    wait_for_foreground_shutdown_async(&shutdown).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use im_core::system_notifications::{
        SystemNotificationChange, SystemNotificationKind, SystemNotificationSnapshot,
        SystemNotificationState,
    };
    use serde_json::Value;

    #[test]
    fn local_inbox_reconciliation_errors_are_secret_free_categories() {
        use crate::m_core_cli_adapter::message_result::MessageAdapterError;

        assert_eq!(
            local_inbox_reconciliation_error_category(&MessageAdapterError::TransportUnavailable(
                "secret-bearing-detail".to_owned()
            ),),
            "transport_unavailable"
        );
        assert_eq!(
            local_inbox_reconciliation_error_category(&MessageAdapterError::LocalStateUnavailable(
                "secret-bearing-detail".to_owned()
            ),),
            "local_state_unavailable"
        );
    }

    #[test]
    fn local_inbox_transport_failure_becomes_warning_but_auth_stays_closed() {
        use crate::m_core_cli_adapter::message_result::MessageAdapterError;

        assert_eq!(
            listener_local_inbox_transport_or_warning(
                Err(MessageAdapterError::TransportUnavailable(
                    "secret-bearing-detail".to_owned(),
                )),
                "sync.foreground_reconcile_deferred",
            )
            .unwrap(),
            vec!["sync.foreground_reconcile_deferred"]
        );
        assert!(matches!(
            listener_local_inbox_transport_or_warning(
                Err(MessageAdapterError::IdentityRequired(
                    "secret-bearing-detail".to_owned(),
                )),
                "sync.foreground_reconcile_deferred",
            ),
            Err(MessageAdapterError::IdentityRequired(_))
        ));
    }

    #[test]
    fn secure_hydration_local_state_failure_defers_but_service_stays_closed() {
        use crate::m_core_cli_adapter::message_result::{MessageAdapterError, ServiceError};

        assert_eq!(
            listener_local_inbox_secure_hydration_or_warning(Err(
                MessageAdapterError::LocalStateUnavailable("secret-bearing-detail".to_owned()),
            ))
            .unwrap(),
            vec!["sync.secure_inbox_hydration_local_state_deferred"]
        );
        assert!(matches!(
            listener_local_inbox_secure_hydration_or_warning(Err(MessageAdapterError::Service(
                ServiceError {
                    status_code: 409,
                    rpc_code: 1409,
                    message: "secret-bearing-detail".to_owned(),
                    data: None,
                }
            ),)),
            Err(MessageAdapterError::Service(_))
        ));
    }

    #[test]
    fn reliable_sync_scheduler_starts_once_and_coalesces_hints_during_sync() {
        let now = Instant::now();
        let mut scheduler = ListenerSyncScheduler::new();

        assert_eq!(scheduler.take_ready(now), Some(ListenerSyncReason::Startup));
        assert_eq!(scheduler.take_ready(now), None);

        scheduler.request(ListenerSyncReason::RealtimeHint);
        scheduler.request(ListenerSyncReason::Timer);
        scheduler.complete_success();
        assert_eq!(
            scheduler.take_ready(now),
            Some(ListenerSyncReason::RealtimeHint),
            "multiple hints received while a sync is active must coalesce"
        );
        assert_eq!(scheduler.take_ready(now), None);
    }

    #[test]
    fn sync_v2_committed_notification_emits_one_exact_redacted_join_wake() {
        let temp = TempDir::new("system-notification-wake");
        let events_path = temp.path().join("host-events.jsonl");
        let host_notify = Arc::new(HostNotifySinkImpl::File(
            super::super::host_notify_sink::new_file_host_notify_sink(
                events_path.to_str().unwrap(),
            )
            .unwrap(),
        ));
        let status = Arc::new(Mutex::new(Status {
            status_file: temp
                .path()
                .join("listener-status.json")
                .display()
                .to_string(),
            ..Status::default()
        }));
        let mut state = ListenerSystemNotificationState::default();
        let pending = system_notification("evt-join-1", "join-1", 1, false);

        let ListenerSystemNotificationPlan::Emit(items) =
            state.plan(SystemNotificationChange::Changed {
                item: pending.clone(),
            })
        else {
            panic!("committed notification must emit");
        };
        assert_eq!(items, vec![pending.clone()]);
        for item in items {
            emit_listener_system_notification(&status, &host_notify, "admin", &pending.did, item)
                .unwrap();
        }
        assert_eq!(
            state.plan(SystemNotificationChange::Changed { item: pending }),
            ListenerSystemNotificationPlan::Emit(Vec::new()),
            "a replay of the same committed revision must not wake twice"
        );
        let terminal = system_notification("evt-join-terminal", "join-1", 2, true);
        let ListenerSystemNotificationPlan::Emit(items) =
            state.plan(SystemNotificationChange::Changed { item: terminal })
        else {
            panic!("terminal revision must still advance local dedupe state");
        };
        for item in items {
            emit_listener_system_notification(
                &status,
                &host_notify,
                "admin",
                "did:wba:example:agents:admin:e1_admin",
                item,
            )
            .unwrap();
        }

        let lines = fs::read_to_string(&events_path).unwrap();
        let events = lines
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["topic"], "im.device.join.requested");
        assert_eq!(events[0]["id"], "evt-join-1");
        assert_eq!(
            events[0]["data"]
                .as_object()
                .unwrap()
                .keys()
                .cloned()
                .collect::<std::collections::BTreeSet<_>>(),
            std::collections::BTreeSet::from([
                "channel".to_owned(),
                "event_id".to_owned(),
                "expires_at".to_owned(),
                "issued_at".to_owned(),
                "join_session_id".to_owned(),
                "recipient_did".to_owned(),
            ])
        );
    }

    #[test]
    fn startup_seed_and_repair_reseed_are_monotonic() {
        let mut state = ListenerSystemNotificationState::default();
        let first = system_notification("evt-join-1", "join-1", 1, false);
        let second = system_notification("evt-join-2", "join-2", 1, false);

        assert_eq!(
            state.plan(SystemNotificationChange::Reset {
                items: vec![first.clone()],
            }),
            ListenerSystemNotificationPlan::Emit(vec![first.clone()]),
            "listener startup must recover an already committed pending Join"
        );
        assert_eq!(
            state.plan(SystemNotificationChange::RepairRequired {
                reason: "subscriber_lag".to_owned(),
            }),
            ListenerSystemNotificationPlan::Rebuild,
            "lag must force an authoritative watch rebuild"
        );
        assert_eq!(
            state.plan(SystemNotificationChange::Reset {
                items: vec![first, second.clone()],
            }),
            ListenerSystemNotificationPlan::Emit(vec![second]),
            "repair seed must retain unseen work without replaying an old wake"
        );
    }

    #[test]
    fn reliable_sync_scheduler_reconnects_after_first_connected_transition_only() {
        let now = Instant::now();
        let mut scheduler = ListenerSyncScheduler::new();
        assert_eq!(scheduler.take_ready(now), Some(ListenerSyncReason::Startup));

        scheduler.observe_connected_transition();
        assert_eq!(scheduler.take_ready(now), None);
        scheduler.observe_connected_transition();
        assert_eq!(
            scheduler.take_ready(now),
            Some(ListenerSyncReason::Reconnect)
        );
    }

    #[test]
    fn reliable_sync_scheduler_applies_bounded_retry_backoff() {
        let now = Instant::now();
        let mut scheduler = ListenerSyncScheduler::new();
        assert_eq!(scheduler.take_ready(now), Some(ListenerSyncReason::Startup));

        scheduler.complete_retryable(now);
        assert_eq!(scheduler.take_ready(now), None);
        assert_eq!(
            scheduler.take_ready(now + RELIABLE_SYNC_INITIAL_RETRY_DELAY),
            Some(ListenerSyncReason::Retry)
        );
        assert_eq!(scheduler.retry_delay, Duration::from_secs(2));

        let mut retry_at = now + RELIABLE_SYNC_INITIAL_RETRY_DELAY;
        for _ in 0..10 {
            scheduler.complete_retryable(retry_at);
            retry_at += scheduler.retry_delay;
            assert_eq!(
                scheduler.take_ready(retry_at),
                Some(ListenerSyncReason::Retry)
            );
        }
        assert_eq!(scheduler.retry_delay, RELIABLE_SYNC_MAX_RETRY_DELAY);
    }

    #[test]
    fn reliable_sync_reasons_use_the_sync_v2_wire_vocabulary() {
        assert_eq!(ListenerSyncReason::Startup.as_str(), "session_start");
        assert_eq!(
            ListenerSyncReason::Reconnect.as_str(),
            "websocket_reconnect"
        );
        assert_eq!(ListenerSyncReason::RealtimeHint.as_str(), "websocket_hint");
        assert_eq!(ListenerSyncReason::Timer.as_str(), "foreground_reconcile");
        assert_eq!(ListenerSyncReason::Retry.as_str(), "foreground_reconcile");
    }

    #[test]
    fn reliable_sync_status_classification_stops_auth_revocation() {
        assert_eq!(
            listener_sync_disposition(im_core::messages::MessageSyncStatus::Idle),
            ListenerSyncDisposition::Success
        );
        assert_eq!(
            listener_sync_disposition(im_core::messages::MessageSyncStatus::RetryableFailure),
            ListenerSyncDisposition::Retryable
        );
        assert_eq!(
            listener_sync_disposition(im_core::messages::MessageSyncStatus::Blocked),
            ListenerSyncDisposition::Blocked
        );
        assert_eq!(
            listener_sync_disposition(im_core::messages::MessageSyncStatus::AuthRevoked),
            ListenerSyncDisposition::AuthRevoked
        );
        assert!(listener_sync_error_is_terminal(
            &im_core::ImError::PermissionDenied
        ));
    }

    #[test]
    fn failed_first_sync_does_not_claim_a_successful_v2_reconcile() {
        let status = Arc::new(Mutex::new(single_connected_session_status()));
        record_v2_subprotocol_negotiated(&status);

        assert_eq!(
            record_reliable_sync_outcome(
                &status,
                im_core::messages::MessageSyncStatus::RetryableFailure,
            ),
            ListenerSyncDisposition::Retryable
        );

        let guard = status.lock().unwrap();
        assert!(guard.reliable_sync.v2_subprotocol_negotiated);
        assert!(!guard.reliable_sync.v2_bootstrap_completed);
        assert!(guard.reliable_sync.last_reconcile_protocol.is_empty());
        assert!(!guard.reliable_sync.legacy_sync_used);
    }

    #[test]
    fn successful_sync_records_only_v2_runtime_facts() {
        let status = Arc::new(Mutex::new(single_connected_session_status()));
        record_v2_subprotocol_negotiated(&status);
        assert_eq!(
            record_reliable_sync_outcome(&status, im_core::messages::MessageSyncStatus::Idle),
            ListenerSyncDisposition::Success
        );

        let guard = status.lock().unwrap();
        assert!(guard.reliable_sync.v2_subprotocol_negotiated);
        assert!(guard.reliable_sync.v2_bootstrap_completed);
        assert_eq!(guard.reliable_sync.last_reconcile_protocol, "sync_v2");
        assert!(!guard.reliable_sync.legacy_sync_used);
    }

    #[test]
    fn v2_negotiation_is_current_boot_all_session_aggregate() {
        let status = Arc::new(Mutex::new(Status {
            sessions: vec![
                SessionStatus {
                    identity_name: "skill-a".to_owned(),
                    connected: true,
                    ..SessionStatus::default()
                },
                SessionStatus {
                    identity_name: "skill-b".to_owned(),
                    connected: false,
                    ..SessionStatus::default()
                },
            ],
            ..Status::default()
        }));

        record_v2_subprotocol_negotiated(&status);
        assert!(
            !status
                .lock()
                .unwrap()
                .reliable_sync
                .v2_subprotocol_negotiated
        );
        status.lock().unwrap().sessions[1].connected = true;
        record_v2_subprotocol_negotiated(&status);
        assert!(
            status
                .lock()
                .unwrap()
                .reliable_sync
                .v2_subprotocol_negotiated
        );
        status.lock().unwrap().sessions[0].connected = false;
        record_v2_subprotocol_negotiated(&status);
        assert!(
            !status
                .lock()
                .unwrap()
                .reliable_sync
                .v2_subprotocol_negotiated
        );
    }

    fn single_connected_session_status() -> Status {
        Status {
            sessions: vec![SessionStatus {
                identity_name: "skill".to_owned(),
                connected: true,
                ..SessionStatus::default()
            }],
            ..Status::default()
        }
    }

    #[test]
    fn disconnected_last_error_preserves_shutdown_and_records_reader_errors_like_go() {
        let status = Status {
            sessions: vec![SessionStatus {
                identity_name: "alice".to_string(),
                did: "did:alice".to_string(),
                connected: true,
                last_error: "previous reader error".to_string(),
            }],
            ..Status::default()
        };

        assert_eq!(
            disconnected_last_error(
                &status,
                "alice",
                Some(SessionDisconnectReason::Other("reader stopped".to_string())),
            ),
            "reader stopped"
        );
        assert_eq!(
            disconnected_last_error(
                &status,
                "alice",
                Some(SessionDisconnectReason::ContextCanceled),
            ),
            "previous reader error",
            "Go markDisconnected ignores context.Canceled rather than overwriting lastError"
        );
        assert_eq!(
            disconnected_last_error(&status, "alice", None),
            "previous reader error",
            "closeCurrentClient-style shutdown does not mutate lastError"
        );
    }

    fn system_notification(
        event_id: &str,
        join_session_id: &str,
        session_revision: u64,
        terminal: bool,
    ) -> SystemNotificationSnapshot {
        SystemNotificationSnapshot {
            event_id: event_id.to_owned(),
            did: "did:wba:example:agents:admin:e1_admin".to_owned(),
            join_session_id: join_session_id.to_owned(),
            kind: if terminal {
                SystemNotificationKind::JoinCompleted
            } else {
                SystemNotificationKind::JoinRequested
            },
            state: if terminal {
                SystemNotificationState::Consumed
            } else {
                SystemNotificationState::Pending
            },
            session_revision,
            issued_at: "2026-08-02T13:14:00Z".to_owned(),
            expires_at: "2026-08-02T13:24:00Z".to_owned(),
            first_seen_at: "2026-08-02T13:14:01Z".to_owned(),
            terminal,
        }
    }

    struct TempDir {
        path: std::path::PathBuf,
    }

    impl TempDir {
        fn new(label: &str) -> Self {
            let nonce = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "awiki-listener-{label}-{}-{nonce}",
                std::process::id()
            ));
            fs::create_dir_all(&path).unwrap();
            Self { path }
        }

        fn path(&self) -> &std::path::Path {
            &self.path
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}
