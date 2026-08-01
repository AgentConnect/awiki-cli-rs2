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
    let mut supervisor = ListenerSupervisor::new(resolved, runner.mode, host)?;
    let result = supervisor.run_async().await;
    supervisor.cleanup_runtime_artifacts();
    result
}

struct ListenerSupervisor {
    resolved: Arc<Resolved>,
    status: Arc<Mutex<Status>>,
    shutdown: Arc<AtomicBool>,
    listener: Option<bridge::BridgeListener>,
    host_notify: Arc<HostNotifySinkImpl>,
    runner_mode: ListenerRunnerMode,
    host: ListenerRunHostKind,
}

impl ListenerSupervisor {
    fn new(
        resolved: Resolved,
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
            status: Arc::new(Mutex::new(status)),
            shutdown: Arc::new(AtomicBool::new(false)),
            listener: None,
            host_notify: Arc::new(host_notify),
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
        let shutdown = self.shutdown.clone();
        thread::spawn(move || {
            while !shutdown.load(Ordering::SeqCst) {
                match bridge::accept_bridge(&listener) {
                    Ok(stream) => {
                        let runtime = BridgeRuntime {
                            status: status.clone(),
                            host_notify: host_notify.clone(),
                            _resolved: resolved.clone(),
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
        let core = crate::m_core_cli_adapter::build_im_core_async(&self.resolved).await?;
        let identities = core.identities().list_async().await?;
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
                self.resolved.clone(),
                self.status.clone(),
                self.host_notify.clone(),
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
            self.resolved.clone(),
            self.status.clone(),
            self.host_notify.clone(),
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
    _resolved: Arc<Resolved>,
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

fn handle_bridge_stream(stream: bridge::BridgeStream, mut runtime: BridgeRuntime) {
    let _ = bridge::handle_bridge_connection_once(stream, |request| {
        execute_listener_bridge_request(&mut runtime, request)
    });
}

async fn spawn_im_core_runner_session_async(
    resolved: Arc<Resolved>,
    status: Arc<Mutex<Status>>,
    host_notify: Arc<HostNotifySinkImpl>,
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
    let client = match crate::m_core_cli_adapter::build_im_client_async(&resolved, selector).await {
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
    tokio::spawn(async move {
        let mut event_error = None;
        let mut scheduler = ListenerSyncScheduler::new();
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
                            .sync_now_async(im_core::messages::MessageSyncRequest {
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
            mark_session_disconnected(
                &status,
                &identity_name,
                did,
                Some(SessionDisconnectReason::ContextCanceled),
            );
            return;
        }
        if let Some(error) = event_error {
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
        mark_session_disconnected(
            &status,
            &identity_name,
            did,
            Some(SessionDisconnectReason::Other(error)),
        );
    });
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
            Self::Startup => "cli_listener_startup",
            Self::Reconnect => "cli_listener_reconnect",
            Self::RealtimeHint => "cli_listener_realtime_hint",
            Self::Timer => "cli_listener_timer",
            Self::Retry => "cli_listener_retry",
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
}
