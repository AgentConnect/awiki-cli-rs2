use super::bridge;
use super::host_notify_sink::{new_host_notify_sink, HostNotifySink, HostNotifySinkImpl};
use super::listener::{self, SessionStatus, Status};
use super::listener_bridge_connection::{
    execute_listener_bridge_request, ListenerBridgeRuntime, ListenerBridgeSession,
};
use super::listener_bridge_runtime::{
    bridge_session_bootstrap_signal_pair, host_notify_for_bridge_created_session,
    local_notifications_for_bridge_created_session, wait_for_bridge_session_bootstrap,
    BridgeSessionBootstrapResult, BridgeSessionBootstrapSignal,
};
use super::listener_handle_lookup::lookup_listener_handle_by_did;
use super::listener_known_sessions::{
    known_session_startup_decision, known_session_startup_error,
    record_known_session_startup_error, KnownSessionStartupDecision, KnownSessionStartupError,
};
use super::listener_local_notification_flush::{
    flush_queued_local_notifications, LocalNotificationFlushTargetSession,
};
use super::listener_local_notifications::{LocalNotification, LocalNotificationQueue};
use super::listener_notification_handler::handle_listener_notification;
use super::listener_notification_plan::{
    NotificationSessionContext, SecureNotificationNormalization,
};
use super::listener_secure_ack_delivery::{
    deliver_local_secure_ack_plan, LocalSecureAckDeliveryAction,
};
use super::listener_secure_notifications::{
    is_direct_secure_incoming_notification, plaintext_body_to_notification_body,
};
use super::listener_secure_replay::ReplayStoreLookup;
use super::listener_secure_sessions;
use super::listener_secure_sync::{
    secure_pending_confirmation_history_replay_actions,
    secure_pending_confirmation_history_rpc_call, secure_unread_direct_inbox_replay_actions,
    secure_unread_direct_inbox_rpc_call,
};
use super::listener_service_did::{
    fetch_message_service_did, ListenerServiceDidRpc, ListenerServiceDidSession,
};
use super::listener_session_methods::SessionDisconnectReason;
use super::listener_session_rpc::{
    close_session_rpc_active, drain_session_rpc_requests, expire_session_rpc_pending,
    fail_session_rpc_pending, fail_session_rpc_queued, remove_session_rpc_sender,
    route_session_rpc_response, set_session_rpc_sender, PendingSessionRpc, SessionRpcRegistry,
    SessionRpcRequest, SessionRpcSender, SessionSharedRpc,
};
use super::listener_shutdown_signal::{
    install_foreground_shutdown_handler, wait_for_foreground_shutdown,
};
use super::listener_ws_transport::WsTransport;
use crate::config::Resolved;
use crate::identity::{self, types::StoredIdentity, Manager};
use crate::im_core_adapter::realtime::{
    self as im_core_realtime_adapter, ListenerRunHostKind, ListenerRunnerMode,
};
use crate::runtime;
use crate::runtime::listener_im_event_adapter::CliRealtimeEventSink;
use crate::store;
use im_core::realtime::{
    connect_realtime_with_transport, realtime_client_endpoints, secure_prekey_retry_decision,
    RealtimeAuthProvider, RealtimeDialOutcome, RealtimeRefreshOutcome, RealtimeTransport,
    SecurePrekeyRetryDecision, SESSION_PING_INTERVAL, SESSION_PING_TIMEOUT,
};
use rand::RngCore;
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::thread;
use std::time::{Duration, Instant};
use time::OffsetDateTime;

const SESSION_RECONNECT_BASE_DELAY: Duration = Duration::from_secs(1);
const SESSION_RECONNECT_MAX_DELAY: Duration = Duration::from_secs(30);
const WATCH_IDENTITIES_INTERVAL: Duration = Duration::from_secs(3);
const SESSION_NOTIFICATION_READ_POLL: Duration = Duration::from_millis(500);

pub fn run_foreground(resolved: Resolved) -> anyhow::Result<()> {
    run_listener(resolved, ListenerRunHostKind::Foreground)
}

pub fn run_service(resolved: Resolved) -> anyhow::Result<()> {
    run_listener(resolved, ListenerRunHostKind::Service)
}

fn run_listener(resolved: Resolved, host: ListenerRunHostKind) -> anyhow::Result<()> {
    let runner = im_core_realtime_adapter::listener_runner_selection(host);
    let mut supervisor = ListenerSupervisor::new(resolved, runner.mode, host)?;
    let result = supervisor.run();
    supervisor.cleanup_runtime_artifacts();
    result
}

struct ListenerSupervisor {
    resolved: Arc<Resolved>,
    manager: Manager,
    status: Arc<Mutex<Status>>,
    shutdown: Arc<AtomicBool>,
    listener: Option<bridge::BridgeListener>,
    host_notify: Arc<HostNotifySinkImpl>,
    local_notifications: Arc<Mutex<LocalNotificationQueue>>,
    session_rpcs: Arc<Mutex<SessionRpcRegistry>>,
    runner_mode: ListenerRunnerMode,
    host: ListenerRunHostKind,
}

impl ListenerSupervisor {
    fn new(
        resolved: Resolved,
        runner_mode: ListenerRunnerMode,
        host: ListenerRunHostKind,
    ) -> anyhow::Result<Self> {
        let db = store::open(&resolved.paths)?;
        store::ensure_schema(&db)?;
        drop(db);
        let runtime_paths = listener::paths(&resolved)?;
        let boot_id = resolve_runtime_boot_id(&resolved)?;
        let (host_notify, host_notify_status) = new_host_notify_sink(&resolved)?;
        let status = Status {
            mode: runtime::resolve(&resolved).mode,
            installed: running_in_listener_service_mode(),
            running: true,
            boot_id,
            pid: i64::from(std::process::id()),
            pid_file: runtime_paths.pid_file,
            log_file: runtime_paths.log_file,
            status_file: runtime_paths.status_file,
            socket_path: runtime_paths.socket_path,
            service_name: super::listener_service::service_name_for(&resolved),
            service_platform: if running_in_listener_service_mode() && cfg!(target_os = "linux") {
                "linux-systemd".to_string()
            } else {
                "rust-local".to_string()
            },
            host_notify: host_notify_status,
            ..Status::default()
        };
        Ok(Self {
            manager: Manager::new(resolved.paths.clone()),
            resolved: Arc::new(resolved),
            status: Arc::new(Mutex::new(status)),
            shutdown: Arc::new(AtomicBool::new(false)),
            listener: None,
            host_notify: Arc::new(host_notify),
            local_notifications: Arc::new(Mutex::new(LocalNotificationQueue::default())),
            session_rpcs: Arc::new(Mutex::new(SessionRpcRegistry::default())),
            runner_mode,
            host,
        })
    }

    fn run(&mut self) -> anyhow::Result<()> {
        install_foreground_shutdown_handler(self.shutdown.clone())?;
        if runtime::resolve(&self.resolved).mode != bridge::MODE_WEBSOCKET {
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
        self.start_known_sessions()?;
        self.spawn_identity_watcher();
        wait_for_shutdown_signal(self.shutdown.clone());
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
        let manager = self.manager.clone();
        let host_notify = self.host_notify.clone();
        let local_notifications = self.local_notifications.clone();
        let session_rpcs = self.session_rpcs.clone();
        let shutdown = self.shutdown.clone();
        let runner_mode = self.runner_mode;
        let listener_for_loop = listener;
        thread::spawn(move || {
            while !shutdown.load(Ordering::SeqCst) {
                match bridge::accept_bridge(&listener_for_loop) {
                    Ok(stream) => {
                        let runtime = BridgeRuntime {
                            resolved: resolved.clone(),
                            manager: manager.clone(),
                            status: status.clone(),
                            host_notify: host_notify.clone(),
                            local_notifications: local_notifications.clone(),
                            session_rpcs: session_rpcs.clone(),
                            shutdown: shutdown.clone(),
                            runner_mode,
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

    fn start_known_sessions(&self) -> anyhow::Result<()> {
        let identities = self.manager.list()?;
        for summary in identities {
            if let Err(err) = self.ensure_known_session(&summary.identity_name, &summary.did) {
                self.record_session_error(&summary.identity_name, &summary.did, &err.to_string());
            }
        }
        self.refresh_status();
        Ok(())
    }

    fn spawn_identity_watcher(&self) {
        let manager = self.manager.clone();
        let resolved = self.resolved.clone();
        let status = self.status.clone();
        let shutdown = self.shutdown.clone();
        let host_notify = self.host_notify.clone();
        let local_notifications = self.local_notifications.clone();
        let session_rpcs = self.session_rpcs.clone();
        let runner_mode = self.runner_mode;
        thread::spawn(move || {
            while !shutdown.load(Ordering::SeqCst) {
                if let Ok(identities) = manager.list() {
                    for summary in identities {
                        let present = status
                            .lock()
                            .map(|status| {
                                status
                                    .sessions
                                    .iter()
                                    .any(|session| session.identity_name == summary.identity_name)
                            })
                            .unwrap_or(false);
                        if !present {
                            spawn_session_loop(
                                resolved.clone(),
                                manager.clone(),
                                status.clone(),
                                host_notify.clone(),
                                local_notifications.clone(),
                                session_rpcs.clone(),
                                shutdown.clone(),
                                summary.identity_name,
                                summary.did,
                                None,
                                runner_mode,
                            );
                        }
                    }
                }
                sleep_or_shutdown(&shutdown, WATCH_IDENTITIES_INTERVAL);
            }
        });
    }

    fn ensure_known_session(&self, identity_name: &str, did: &str) -> anyhow::Result<()> {
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
        let (initial_signal, initial_waiter) = bridge_session_bootstrap_signal_pair(identity_name);
        spawn_session_loop(
            self.resolved.clone(),
            self.manager.clone(),
            self.status.clone(),
            self.host_notify.clone(),
            self.local_notifications.clone(),
            self.session_rpcs.clone(),
            self.shutdown.clone(),
            identity_name.to_string(),
            did.to_string(),
            Some(initial_signal),
            self.runner_mode,
        );
        match known_session_startup_error(
            identity_name,
            did,
            wait_for_bridge_session_bootstrap(initial_waiter),
        ) {
            Some(error) => Err(anyhow::anyhow!(error.error)),
            None => Ok(()),
        }
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
        cleanup_runtime_artifacts(&self.resolved);
    }

    fn lock_status(&self) -> std::sync::MutexGuard<'_, Status> {
        self.status.lock().expect("listener status mutex poisoned")
    }
}

struct BridgeRuntime {
    resolved: Arc<Resolved>,
    manager: Manager,
    status: Arc<Mutex<Status>>,
    host_notify: Arc<HostNotifySinkImpl>,
    local_notifications: Arc<Mutex<LocalNotificationQueue>>,
    session_rpcs: Arc<Mutex<SessionRpcRegistry>>,
    shutdown: Arc<AtomicBool>,
    runner_mode: ListenerRunnerMode,
}

impl ListenerBridgeRuntime for BridgeRuntime {
    fn ensure_session(&mut self, identity_name: &str) -> anyhow::Result<ListenerBridgeSession> {
        let identity_name = resolve_identity_name(&self.manager, identity_name)?;
        let record = self.manager.load(&identity_name).ok();
        let did = record
            .as_ref()
            .map(|record| record.did.clone())
            .unwrap_or_default();
        let known_session = self
            .status
            .lock()
            .map(|status| {
                status
                    .sessions
                    .iter()
                    .any(|session| session.identity_name == identity_name)
            })
            .unwrap_or(false);
        if !known_session {
            let (initial_signal, initial_waiter) =
                bridge_session_bootstrap_signal_pair(&identity_name);
            spawn_session_loop(
                self.resolved.clone(),
                self.manager.clone(),
                self.status.clone(),
                host_notify_for_bridge_created_session(&self.host_notify),
                local_notifications_for_bridge_created_session(&self.local_notifications),
                self.session_rpcs.clone(),
                self.shutdown.clone(),
                identity_name.clone(),
                did,
                Some(initial_signal),
                self.runner_mode,
            );
            match wait_for_bridge_session_bootstrap(initial_waiter) {
                BridgeSessionBootstrapResult::Connected => {}
                BridgeSessionBootstrapResult::InitialError(error)
                | BridgeSessionBootstrapResult::Timeout(error) => {
                    return Err(anyhow::anyhow!(error))
                }
            }
        }
        let record = self.manager.load(&identity_name).ok();
        let connected = session_is_connected(&self.status, &identity_name);
        Ok(if connected {
            ListenerBridgeSession::connected(identity_name, record.unwrap_or_default())
        } else {
            ListenerBridgeSession::disconnected(identity_name, record)
        })
    }

    fn fetch_message_service_did(
        &mut self,
        session: &ListenerBridgeSession,
    ) -> anyhow::Result<String> {
        let mut rpc = SessionSharedRpc::new(&self.session_rpcs, &session.identity_name)?;
        fetch_message_service_did(
            &ListenerServiceDidSession {
                identity_name: session.identity_name.clone(),
                has_current_client: true,
            },
            &mut rpc,
        )
    }

    fn send_rpc(
        &mut self,
        session: &ListenerBridgeSession,
        method: &str,
        params: Value,
    ) -> anyhow::Result<Map<String, Value>> {
        let params = match params {
            Value::Object(map) => map,
            _ => Map::new(),
        };
        let mut rpc = SessionSharedRpc::new(&self.session_rpcs, &session.identity_name)?;
        rpc.send_rpc(method, params)
    }

    fn mark_messages_read(
        &mut self,
        owner_did: &str,
        message_ids: &[String],
    ) -> anyhow::Result<()> {
        let connection = store::open(&self.resolved.paths)?;
        store::mark_messages_read(&connection, owner_did, message_ids)?;
        Ok(())
    }
}

impl Drop for BridgeRuntime {
    fn drop(&mut self) {}
}

struct SessionNotificationTask {
    notification: Map<String, Value>,
}

fn handle_bridge_stream(stream: bridge::BridgeStream, mut runtime: BridgeRuntime) {
    let _ = bridge::handle_bridge_connection_once(stream, |request| {
        execute_listener_bridge_request(&mut runtime, request)
    });
}

fn spawn_session_loop(
    resolved: Arc<Resolved>,
    manager: Manager,
    status: Arc<Mutex<Status>>,
    host_notify: Arc<HostNotifySinkImpl>,
    local_notifications: Arc<Mutex<LocalNotificationQueue>>,
    session_rpcs: Arc<Mutex<SessionRpcRegistry>>,
    shutdown: Arc<AtomicBool>,
    identity_name: String,
    _did: String,
    initial_signal: Option<BridgeSessionBootstrapSignal>,
    runner_mode: ListenerRunnerMode,
) {
    {
        let mut guard = status.lock().expect("listener status mutex poisoned");
        upsert_session(
            &mut guard,
            SessionStatus {
                identity_name: identity_name.clone(),
                did: String::new(),
                connected: false,
                last_error: String::new(),
            },
        );
        let _ = listener::write_status(&guard.status_file, &guard);
    }
    thread::spawn(move || {
        let mut delay = SESSION_RECONNECT_BASE_DELAY;
        let mut initial_signal = initial_signal;
        while !shutdown.load(Ordering::SeqCst) {
            match connect_session(&resolved, &manager, &identity_name) {
                Ok((record, mut transport)) => {
                    let (rpc_tx, rpc_rx) = mpsc::channel();
                    let rpc_active = Arc::new(Mutex::new(true));
                    set_session_rpc_sender(
                        &session_rpcs,
                        &identity_name,
                        SessionRpcSender {
                            tx: rpc_tx,
                            active: rpc_active.clone(),
                        },
                    );
                    mark_session_connected(&status, &identity_name, &record.did);
                    if let Some(signal) = initial_signal.take() {
                        signal.signal_success();
                    }
                    delay = SESSION_RECONNECT_BASE_DELAY;
                    start_connected_session_tasks(
                        &resolved,
                        &manager,
                        &status,
                        &host_notify,
                        &local_notifications,
                        &session_rpcs,
                        &shutdown,
                        &record,
                    );
                    let error = match runner_mode {
                        ListenerRunnerMode::ImCore => consume_notifications_with_im_core_runner(
                            &resolved,
                            &manager,
                            &status,
                            &host_notify,
                            &local_notifications,
                            &record,
                            &mut transport,
                            &rpc_rx,
                            &session_rpcs,
                            &rpc_active,
                            &shutdown,
                        )
                        .err()
                        .map(|err| err.to_string()),
                    };
                    close_session_rpc_active(&rpc_active);
                    remove_session_rpc_sender(&session_rpcs, &identity_name);
                    let _ = transport.close();
                    if shutdown.load(Ordering::SeqCst) {
                        mark_session_disconnected(
                            &status,
                            &identity_name,
                            record.did,
                            Some(SessionDisconnectReason::ContextCanceled),
                        );
                        break;
                    }
                    mark_session_disconnected(
                        &status,
                        &identity_name,
                        record.did,
                        error.map(SessionDisconnectReason::Other),
                    );
                }
                Err(err) => {
                    mark_session_disconnected(
                        &status,
                        &identity_name,
                        String::new(),
                        Some(SessionDisconnectReason::Other(err.to_string())),
                    );
                    if let Some(signal) = initial_signal.take() {
                        signal.signal_error(err.to_string());
                    }
                }
            }
            sleep_or_shutdown(&shutdown, delay);
            delay = std::cmp::min(delay.saturating_mul(2), SESSION_RECONNECT_MAX_DELAY);
        }
    });
}

fn connect_session(
    resolved: &Resolved,
    manager: &Manager,
    identity_name: &str,
) -> anyhow::Result<(StoredIdentity, WsTransport)> {
    let record = manager.load(identity_name)?;
    let user_state = identity::store::evaluate_user_state(&record.user_id, &record.handle);
    if !user_state.ready_for_messaging {
        anyhow::bail!("{}", user_registration_error(&record, &user_state));
    }
    let selector = im_core::IdentitySelector::LocalAlias(record.identity_name.clone());
    let client =
        crate::im_core_adapter::build_im_client(resolved, selector).map_err(anyhow::Error::new)?;
    let endpoints =
        realtime_client_endpoints(&resolved.service_base_url).map_err(anyhow::Error::msg)?;
    let mut dialer = ListenerRealtimeDialer::new(&resolved.ca_bundle);
    let mut auth =
        ListenerRealtimeAuth::new(&client, manager.clone(), record.identity_name.clone());
    connect_realtime_with_transport(&endpoints, record.jwt_token.trim(), &mut dialer, &mut auth)
        .map_err(im_core_realtime_adapter::map_sdk_runner_error)?;
    let token = auth
        .refreshed_token()
        .unwrap_or(record.jwt_token.trim())
        .trim()
        .to_string();
    if token.trim() != record.jwt_token.trim() {
        let _ = manager.update_jwt(&record.identity_name, token.trim());
    }
    let transport = dialer.take_connected_transport()?;
    let mut updated = record;
    updated.jwt_token = token;
    Ok((updated, transport))
}

struct ListenerRealtimeDialer<'a> {
    ca_bundle: &'a str,
    connected: Option<WsTransport>,
}

impl<'a> ListenerRealtimeDialer<'a> {
    fn new(ca_bundle: &'a str) -> Self {
        Self {
            ca_bundle,
            connected: None,
        }
    }

    fn take_connected_transport(&mut self) -> anyhow::Result<WsTransport> {
        self.connected
            .take()
            .ok_or_else(|| anyhow::anyhow!("runtime listener websocket transport was not attached"))
    }
}

impl RealtimeTransport for ListenerRealtimeDialer<'_> {
    fn dial_bearer(&mut self, websocket_url: &str, bearer_token: &str) -> RealtimeDialOutcome {
        match WsTransport::connect(websocket_url, bearer_token, self.ca_bundle) {
            Ok(transport) => {
                self.connected = Some(transport);
                RealtimeDialOutcome::Connected
            }
            Err(err) => RealtimeDialOutcome::Failed {
                status_code: err.status_code,
                error: err.message,
                response_body: None,
            },
        }
    }
}

struct ListenerRealtimeAuth<'a> {
    client: &'a im_core::ImClient,
    manager: Manager,
    identity_name: String,
    refreshed_token: Option<String>,
}

impl<'a> ListenerRealtimeAuth<'a> {
    fn new(client: &'a im_core::ImClient, manager: Manager, identity_name: String) -> Self {
        Self {
            client,
            manager,
            identity_name,
            refreshed_token: None,
        }
    }

    fn refreshed_token(&self) -> Option<&str> {
        self.refreshed_token.as_deref()
    }
}

impl RealtimeAuthProvider for ListenerRealtimeAuth<'_> {
    fn refresh_realtime_bearer(
        &mut self,
        did_auth_url: &str,
    ) -> im_core::ImResult<RealtimeRefreshOutcome> {
        let _ = did_auth_url;
        self.client.auth().refresh_session()?;
        let token = self
            .manager
            .load(&self.identity_name)
            .map(|record| record.jwt_token)
            .map_err(|err| im_core::ImError::CredentialFileUnreadable {
                path_kind: "auth_state".to_string(),
                detail: err.to_string(),
            })?;
        let token = token.trim().to_string();
        if !token.is_empty() {
            self.refreshed_token = Some(token.clone());
            return Ok(RealtimeRefreshOutcome::Refreshed { current_jwt: token });
        }
        Ok(RealtimeRefreshOutcome::Failed {
            error: "did-auth did not return a websocket bearer token".to_string(),
        })
    }
}

fn start_connected_session_tasks(
    resolved: &Arc<Resolved>,
    manager: &Manager,
    status: &Arc<Mutex<Status>>,
    host_notify: &Arc<HostNotifySinkImpl>,
    local_notifications: &Arc<Mutex<LocalNotificationQueue>>,
    session_rpcs: &Arc<Mutex<SessionRpcRegistry>>,
    shutdown: &Arc<AtomicBool>,
    record: &StoredIdentity,
) {
    flush_local_notifications_for_session(
        resolved,
        manager,
        status,
        host_notify,
        local_notifications,
        session_rpcs,
        record,
    );
    spawn_secure_prekey_retry(
        resolved.clone(),
        manager.clone(),
        status.clone(),
        session_rpcs.clone(),
        shutdown.clone(),
        record.clone(),
    );
    spawn_secure_backlog_poller(
        resolved.clone(),
        manager.clone(),
        status.clone(),
        host_notify.clone(),
        local_notifications.clone(),
        session_rpcs.clone(),
        shutdown.clone(),
        record.identity_name.clone(),
    );
}

fn spawn_secure_prekey_retry(
    resolved: Arc<Resolved>,
    manager: Manager,
    status: Arc<Mutex<Status>>,
    session_rpcs: Arc<Mutex<SessionRpcRegistry>>,
    shutdown: Arc<AtomicBool>,
    record: StoredIdentity,
) {
    let identity_name = record.identity_name.clone();
    thread::spawn(move || {
        while !shutdown.load(Ordering::SeqCst) && session_is_connected(&status, &identity_name) {
            let warnings =
                publish_runtime_secure_prekeys(&resolved, &manager, &session_rpcs, &record);
            match secure_prekey_retry_decision(&identity_name, &warnings) {
                SecurePrekeyRetryDecision::Finished => return,
                SecurePrekeyRetryDecision::Retry {
                    log_line,
                    sleep_delay,
                } => {
                    eprintln!("{log_line}");
                    if !sleep_or_shutdown_or_disconnected(
                        &shutdown,
                        &status,
                        &identity_name,
                        sleep_delay,
                    ) {
                        return;
                    }
                }
            }
        }
    });
}

fn publish_runtime_secure_prekeys(
    resolved: &Resolved,
    manager: &Manager,
    session_rpcs: &Arc<Mutex<SessionRpcRegistry>>,
    record: &StoredIdentity,
) -> Vec<String> {
    if record.e2ee_agreement_private_pem.is_empty() || record.key1_private_pem.is_empty() {
        return Vec::new();
    }
    let mut client = match secure_client_for_record(resolved, manager, session_rpcs, record) {
        Ok(client) => client,
        Err(err) => {
            return compact_warnings(vec![format!(
                "Failed to initialize secure prekey publisher: {err}"
            )])
        }
    };
    match client.prepare_prekey_bundle() {
        Ok(_) => Vec::new(),
        Err(err) => compact_warnings(vec![format!("Failed to publish secure prekeys: {err}")]),
    }
}

fn sleep_or_shutdown_or_disconnected(
    shutdown: &AtomicBool,
    status: &Arc<Mutex<Status>>,
    identity_name: &str,
    duration: Duration,
) -> bool {
    let deadline = Instant::now() + duration;
    while !shutdown.load(Ordering::SeqCst)
        && session_is_connected(status, identity_name)
        && Instant::now() < deadline
    {
        thread::sleep(Duration::from_millis(100));
    }
    !shutdown.load(Ordering::SeqCst) && session_is_connected(status, identity_name)
}

fn spawn_secure_backlog_poller(
    resolved: Arc<Resolved>,
    manager: Manager,
    status: Arc<Mutex<Status>>,
    host_notify: Arc<HostNotifySinkImpl>,
    local_notifications: Arc<Mutex<LocalNotificationQueue>>,
    session_rpcs: Arc<Mutex<SessionRpcRegistry>>,
    shutdown: Arc<AtomicBool>,
    identity_name: String,
) {
    thread::spawn(move || {
        let mut run_immediately = true;
        while !shutdown.load(Ordering::SeqCst) && session_is_connected(&status, &identity_name) {
            if run_immediately {
                run_immediately = false;
            } else {
                sleep_or_shutdown(
                    &shutdown,
                    super::listener_secure_inbox_poll::SECURE_DIRECT_INBOX_POLL_INTERVAL,
                );
                if shutdown.load(Ordering::SeqCst) || !session_is_connected(&status, &identity_name)
                {
                    break;
                }
            }
            let Ok(record) = manager.load(&identity_name) else {
                continue;
            };
            sync_unread_secure_direct_inbox(
                &resolved,
                &manager,
                &status,
                &host_notify,
                &local_notifications,
                &session_rpcs,
                &record,
            );
            sync_pending_confirmation_secure_history(
                &resolved,
                &manager,
                &status,
                &host_notify,
                &local_notifications,
                &session_rpcs,
                &record,
            );
        }
    });
}

fn sync_unread_secure_direct_inbox(
    resolved: &Resolved,
    manager: &Manager,
    status: &Arc<Mutex<Status>>,
    host_notify: &Arc<HostNotifySinkImpl>,
    local_notifications: &Arc<Mutex<LocalNotificationQueue>>,
    session_rpcs: &Arc<Mutex<SessionRpcRegistry>>,
    record: &StoredIdentity,
) {
    let Ok(mut rpc) = SessionSharedRpc::new(session_rpcs, &record.identity_name) else {
        return;
    };
    let call = secure_unread_direct_inbox_rpc_call(record);
    let Value::Object(params) = call.params else {
        return;
    };
    let Ok(result) = rpc.send_rpc_with_timeout(&call.method, params, Some(call.timeout)) else {
        return;
    };
    replay_secure_messages_from_rpc_result(
        resolved,
        manager,
        status,
        host_notify,
        local_notifications,
        session_rpcs,
        record,
        &result,
        false,
    );
}

fn sync_pending_confirmation_secure_history(
    resolved: &Resolved,
    manager: &Manager,
    status: &Arc<Mutex<Status>>,
    host_notify: &Arc<HostNotifySinkImpl>,
    local_notifications: &Arc<Mutex<LocalNotificationQueue>>,
    session_rpcs: &Arc<Mutex<SessionRpcRegistry>>,
    record: &StoredIdentity,
) {
    let peer_dids = listener_secure_sessions::pending_confirmation_peer_dids(
        Some(manager),
        &record.identity_name,
    );
    if peer_dids.is_empty() {
        return;
    }
    let Ok(mut rpc) = SessionSharedRpc::new(session_rpcs, &record.identity_name) else {
        return;
    };
    for peer_did in peer_dids {
        let Ok(call) = secure_pending_confirmation_history_rpc_call(record, &peer_did) else {
            continue;
        };
        let Value::Object(params) = call.params else {
            continue;
        };
        let Ok(result) = rpc.send_rpc_with_timeout(&call.method, params, Some(call.timeout)) else {
            continue;
        };
        replay_secure_messages_from_rpc_result(
            resolved,
            manager,
            status,
            host_notify,
            local_notifications,
            session_rpcs,
            record,
            &result,
            true,
        );
    }
}

fn replay_secure_messages_from_rpc_result(
    resolved: &Resolved,
    manager: &Manager,
    status: &Arc<Mutex<Status>>,
    host_notify: &Arc<HostNotifySinkImpl>,
    local_notifications: &Arc<Mutex<LocalNotificationQueue>>,
    session_rpcs: &Arc<Mutex<SessionRpcRegistry>>,
    record: &StoredIdentity,
    result: &Map<String, Value>,
    skip_self_sent: bool,
) {
    let mut lookup = |message_id: &str, owner_did: &str, _credential_name: &str| {
        if cached_message_exists(resolved, owner_did, message_id) {
            ReplayStoreLookup::Exists
        } else {
            ReplayStoreLookup::Missing
        }
    };
    let actions = if skip_self_sent {
        secure_pending_confirmation_history_replay_actions(
            record,
            &Value::Object(result.clone()),
            &mut lookup,
        )
    } else {
        secure_unread_direct_inbox_replay_actions(
            record,
            &Value::Object(result.clone()),
            &mut lookup,
        )
    };
    for action in actions {
        if let super::listener_secure_sync::SecureSyncAction::HandleNotification { notification } =
            action
        {
            handle_replayed_secure_notification(
                resolved,
                manager,
                status,
                host_notify,
                local_notifications,
                session_rpcs,
                record,
                &notification,
            );
        }
    }
}

fn handle_replayed_secure_notification(
    resolved: &Resolved,
    manager: &Manager,
    status: &Arc<Mutex<Status>>,
    host_notify: &Arc<HostNotifySinkImpl>,
    local_notifications: &Arc<Mutex<LocalNotificationQueue>>,
    session_rpcs: &Arc<Mutex<SessionRpcRegistry>>,
    record: &StoredIdentity,
    notification: &Value,
) {
    let secure_normalization = normalize_secure_notification(
        resolved,
        manager,
        status,
        host_notify,
        local_notifications,
        session_rpcs,
        record,
        notification,
    );
    let mut connection = match store::open(&resolved.paths) {
        Ok(connection) => connection,
        Err(_) => return,
    };
    if store::ensure_schema(&connection).is_err() {
        return;
    }
    let mut guard = status.lock().expect("listener status mutex poisoned");
    let session = NotificationSessionContext {
        identity_name: record.identity_name.clone(),
        did: record.did.clone(),
        handle: record.handle.clone(),
    };
    let mut lookup = |did: &str| lookup_listener_handle_by_did(resolved, did);
    let _ = handle_listener_notification(
        &mut connection,
        Some(host_notify.as_ref()),
        &mut guard,
        notification,
        &session,
        secure_normalization,
        None,
        Some(&mut lookup),
    );
    let _ = listener::write_status(&guard.status_file, &guard);
}

fn flush_local_notifications_for_session(
    resolved: &Resolved,
    manager: &Manager,
    status: &Arc<Mutex<Status>>,
    host_notify: &Arc<HostNotifySinkImpl>,
    local_notifications: &Arc<Mutex<LocalNotificationQueue>>,
    session_rpcs: &Arc<Mutex<SessionRpcRegistry>>,
    record: &StoredIdentity,
) {
    let target_session = LocalNotificationFlushTargetSession::new(
        record.identity_name.clone(),
        record.identity_name.clone(),
        Some(record.did.clone()),
    );
    let mut queued = Vec::new();
    {
        let mut queue = local_notifications
            .lock()
            .expect("local notification queue mutex poisoned");
        flush_queued_local_notifications(
            &mut queue,
            Some(&target_session),
            |_, _, notification| queued.push(notification),
        );
    }
    for notification in queued {
        handle_local_notification(
            resolved,
            manager,
            status,
            host_notify,
            local_notifications,
            session_rpcs,
            record,
            notification,
        );
    }
}

fn handle_local_notification(
    resolved: &Resolved,
    manager: &Manager,
    status: &Arc<Mutex<Status>>,
    host_notify: &Arc<HostNotifySinkImpl>,
    local_notifications: &Arc<Mutex<LocalNotificationQueue>>,
    session_rpcs: &Arc<Mutex<SessionRpcRegistry>>,
    record: &StoredIdentity,
    notification: LocalNotification,
) {
    let notification = Value::Object(notification);
    let secure_normalization = normalize_secure_notification(
        resolved,
        manager,
        status,
        host_notify,
        local_notifications,
        session_rpcs,
        record,
        &notification,
    );
    let mut connection = match store::open(&resolved.paths) {
        Ok(connection) => connection,
        Err(_) => return,
    };
    if store::ensure_schema(&connection).is_err() {
        return;
    }
    let mut guard = status.lock().expect("listener status mutex poisoned");
    let session = NotificationSessionContext {
        identity_name: record.identity_name.clone(),
        did: record.did.clone(),
        handle: record.handle.clone(),
    };
    let mut lookup = |did: &str| lookup_listener_handle_by_did(resolved, did);
    let _ = handle_listener_notification(
        &mut connection,
        Some(host_notify.as_ref()),
        &mut guard,
        &notification,
        &session,
        secure_normalization,
        None,
        Some(&mut lookup),
    );
    let _ = listener::write_status(&guard.status_file, &guard);
}

fn cached_message_exists(resolved: &Resolved, owner_did: &str, message_id: &str) -> bool {
    if owner_did.trim().is_empty() || message_id.trim().is_empty() {
        return false;
    }
    let Ok(connection) = store::open(&resolved.paths) else {
        return false;
    };
    store::list_messages_by_ids(&connection, owner_did, &[message_id.to_string()])
        .map(|messages| !messages.is_empty())
        .unwrap_or(true)
}

fn consume_notifications_with_im_core_runner(
    resolved: &Resolved,
    manager: &Manager,
    status: &Arc<Mutex<Status>>,
    host_notify: &Arc<HostNotifySinkImpl>,
    local_notifications: &Arc<Mutex<LocalNotificationQueue>>,
    record: &StoredIdentity,
    transport: &mut WsTransport,
    rpc_rx: &mpsc::Receiver<SessionRpcRequest>,
    session_rpcs: &Arc<Mutex<SessionRpcRegistry>>,
    rpc_active: &Arc<Mutex<bool>>,
    shutdown: &Arc<AtomicBool>,
) -> anyhow::Result<()> {
    let (notification_tx, notification_rx) = mpsc::channel::<SessionNotificationTask>();
    let handler_resolved = resolved.clone();
    let handler_manager = manager.clone();
    let handler_status = status.clone();
    let handler_host_notify = host_notify.clone();
    let handler_local_notifications = local_notifications.clone();
    let handler_session_rpcs = session_rpcs.clone();
    let handler_record = record.clone();
    let handler = thread::spawn(move || {
        while let Ok(task) = notification_rx.recv() {
            handle_session_notification(
                &handler_resolved,
                &handler_manager,
                &handler_status,
                &handler_host_notify,
                &handler_local_notifications,
                &handler_session_rpcs,
                &handler_record,
                task.notification,
            );
        }
    });
    let sdk_shutdown =
        im_core_realtime_adapter::sdk_shutdown_signal_from_listener_shutdown(shutdown);
    let mut runner_transport = CliRealtimeRunnerTransport {
        transport,
        rpc_rx,
        shutdown: shutdown.clone(),
        sdk_shutdown: sdk_shutdown.clone(),
        notification_tx,
        next_ping: Instant::now() + SESSION_PING_INTERVAL,
        next_id: 0,
        dispatch: super::listener_wsclient::ListenerWsPendingDispatch::default(),
        pending_rpc_responses: BTreeMap::new(),
    };
    let mut event_sink = CliRealtimeEventSink {
        resolved,
        status,
        host_notify,
        record,
    };
    let control = im_core::prelude::RealtimeControl::default();
    let result = im_core::realtime::run_realtime_transport_with_event_sink_until_shutdown(
        im_core_realtime_adapter::listener_realtime_options(),
        sdk_shutdown.clone(),
        control,
        &mut runner_transport,
        &mut event_sink,
    )
    .map_err(im_core_realtime_adapter::map_sdk_runner_error);
    im_core_realtime_adapter::mark_sdk_shutdown_requested(&sdk_shutdown);
    let close_error = match &result {
        Ok(outcome) => realtime_exit_error_text(&outcome.exit),
        Err(err) => err.to_string(),
    };
    close_session_rpc_active(rpc_active);
    runner_transport.close_pending(&close_error);
    drop(runner_transport);
    let _ = handler.join();
    result.map(|_| ())
}

struct CliRealtimeRunnerTransport<'a> {
    transport: &'a mut WsTransport,
    rpc_rx: &'a mpsc::Receiver<SessionRpcRequest>,
    shutdown: Arc<AtomicBool>,
    sdk_shutdown: im_core::prelude::ShutdownSignal,
    notification_tx: mpsc::Sender<SessionNotificationTask>,
    next_ping: Instant,
    next_id: i64,
    dispatch: super::listener_wsclient::ListenerWsPendingDispatch,
    pending_rpc_responses: BTreeMap<String, PendingSessionRpc>,
}

impl CliRealtimeRunnerTransport<'_> {
    fn close_pending(&mut self, error: &str) {
        fail_session_rpc_pending(error, &mut self.dispatch, &mut self.pending_rpc_responses);
        fail_session_rpc_queued(error, self.rpc_rx);
    }
}

impl im_core::realtime::RealtimeRunnerEventSink for CliRealtimeEventSink<'_> {
    fn emit(&mut self, event: im_core::prelude::ImEvent) -> im_core::ImResult<()> {
        CliRealtimeEventSink::emit(self, event)
    }
}

impl im_core::realtime::RealtimeRunnerTransport for CliRealtimeRunnerTransport<'_> {
    fn connect(&mut self) -> im_core::ImResult<()> {
        Ok(())
    }

    fn next_notification(&mut self) -> im_core::ImResult<Option<Value>> {
        if self.shutdown.load(Ordering::SeqCst) {
            self.sdk_shutdown.request();
            return Ok(None);
        }
        drain_session_rpc_requests(
            self.transport,
            self.rpc_rx,
            &mut self.next_id,
            &mut self.dispatch,
            &mut self.pending_rpc_responses,
        )
        .map_err(|err| im_core::ImError::TransportUnavailable {
            detail: err.to_string(),
        })?;
        expire_session_rpc_pending(
            Instant::now(),
            &mut self.dispatch,
            &mut self.pending_rpc_responses,
        );
        if Instant::now() >= self.next_ping {
            if let Err(err) = self
                .transport
                .ping_with_timeout_until(SESSION_PING_TIMEOUT, || {
                    self.shutdown.load(Ordering::SeqCst)
                })
            {
                if self.shutdown.load(Ordering::SeqCst) {
                    self.sdk_shutdown.request();
                    return Ok(None);
                }
                return Err(im_core::ImError::TransportUnavailable {
                    detail: format!("websocket ping failed: {err}"),
                });
            }
            self.next_ping = Instant::now() + SESSION_PING_INTERVAL;
        }
        let notification = self
            .transport
            .read_json_message_timeout(SESSION_NOTIFICATION_READ_POLL)
            .map_err(|err| im_core::ImError::TransportUnavailable {
                detail: err.to_string(),
            })?;
        let Some(notification) = notification else {
            if self.shutdown.load(Ordering::SeqCst) {
                self.sdk_shutdown.request();
                return Ok(None);
            }
            return Ok(Some(Value::Null));
        };
        if notification.get("id").is_some() {
            route_session_rpc_response(
                notification,
                &mut self.dispatch,
                &mut self.pending_rpc_responses,
            );
            return Ok(Some(Value::Null));
        }
        let raw_notification = Value::Object(notification.clone());
        if im_core::realtime::is_direct_secure_wire_notification(&raw_notification) {
            self.notification_tx
                .send(SessionNotificationTask { notification })
                .map_err(|_| im_core::ImError::TransportUnavailable {
                    detail: "websocket notification handler is closed".to_string(),
                })?;
            return Ok(Some(Value::Null));
        }
        Ok(Some(Value::Object(notification)))
    }
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

fn handle_session_notification(
    resolved: &Resolved,
    manager: &Manager,
    status: &Arc<Mutex<Status>>,
    host_notify: &Arc<HostNotifySinkImpl>,
    local_notifications: &Arc<Mutex<LocalNotificationQueue>>,
    session_rpcs: &Arc<Mutex<SessionRpcRegistry>>,
    record: &StoredIdentity,
    notification: Map<String, Value>,
) {
    let notification = Value::Object(notification);
    let secure_normalization = normalize_secure_notification(
        resolved,
        manager,
        status,
        host_notify,
        local_notifications,
        session_rpcs,
        record,
        &notification,
    );
    let Ok(mut connection) = store::open(&resolved.paths) else {
        return;
    };
    if store::ensure_schema(&connection).is_err() {
        return;
    }
    let mut guard = status.lock().expect("listener status mutex poisoned");
    let session = NotificationSessionContext {
        identity_name: record.identity_name.clone(),
        did: record.did.clone(),
        handle: record.handle.clone(),
    };
    let mut lookup = |did: &str| lookup_listener_handle_by_did(resolved, did);
    let _ = handle_listener_notification(
        &mut connection,
        Some(host_notify.as_ref()),
        &mut guard,
        &notification,
        &session,
        secure_normalization,
        None,
        Some(&mut lookup),
    );
    let _ = listener::write_status(&guard.status_file, &guard);
}

fn normalize_secure_notification(
    resolved: &Resolved,
    manager: &Manager,
    status: &Arc<Mutex<Status>>,
    host_notify: &Arc<HostNotifySinkImpl>,
    local_notifications: &Arc<Mutex<LocalNotificationQueue>>,
    session_rpcs: &Arc<Mutex<SessionRpcRegistry>>,
    record: &StoredIdentity,
    notification: &Value,
) -> SecureNotificationNormalization {
    if !is_direct_secure_incoming_notification(notification) {
        return SecureNotificationNormalization::KeepOriginal;
    }
    let params = match notification.get("params").and_then(Value::as_object) {
        Some(params) => params.clone(),
        None => return SecureNotificationNormalization::KeepOriginal,
    };
    let mut client = match secure_client_for_record(resolved, manager, session_rpcs, record) {
        Ok(client) => client,
        Err(_) => return SecureNotificationNormalization::KeepOriginal,
    };
    let result = match client.process_incoming(params.clone()) {
        Ok(result) => result,
        Err(_) => return SecureNotificationNormalization::KeepOriginal,
    };
    if string_value(result.get("state")) != "decrypted" {
        return SecureNotificationNormalization::KeepOriginal;
    }
    let plaintext = match result.get("plaintext").and_then(Value::as_object) {
        Some(plaintext) => plaintext.clone(),
        None => return SecureNotificationNormalization::KeepOriginal,
    };

    let mut normalized = notification.clone();
    let (sender_did, message_id, original_content_type, original_body) =
        apply_decrypted_secure_plaintext(&mut normalized, &plaintext);

    if im_core::secure::is_secure_ack_plaintext(&plaintext) {
        let _ = flush_secure_outbox(resolved, manager, session_rpcs, record, &sender_did);
        set_notification_method(&mut normalized, "direct.secure.ack");
        return SecureNotificationNormalization::Replace(normalized);
    }

    if im_core::secure::is_secure_init_plaintext(&plaintext) {
        set_notification_method(&mut normalized, "direct.secure.init");
    }

    if original_content_type == "application/anp-direct-init+json" {
        let session_id = original_body
            .as_object()
            .map(|body| string_from_object(Some(body), "session_id"))
            .unwrap_or_default();
        if !session_id.is_empty() && !message_id.is_empty() {
            let ack_id = format!("ack-{session_id}");
            if !deliver_local_secure_ack_in_process(
                resolved,
                manager,
                status,
                local_notifications,
                session_rpcs,
                record,
                &sender_did,
                &session_id,
                &message_id,
                &ack_id,
            ) {
                if let Ok(ack_result) = client.send_json(
                    &sender_did,
                    im_core::secure::build_secure_ack_payload(&session_id, &message_id),
                    &ack_id,
                    &ack_id,
                ) {
                    deliver_local_secure_ack(
                        resolved,
                        manager,
                        status,
                        host_notify,
                        local_notifications,
                        session_rpcs,
                        record,
                        &sender_did,
                        &ack_id,
                        &Value::Object(ack_result),
                    );
                }
            }
            let _ = flush_peer_queued_secure_outbox(
                resolved,
                manager,
                session_rpcs,
                &sender_did,
                &record.did,
            );
        }
    }

    SecureNotificationNormalization::Replace(normalized)
}

fn apply_decrypted_secure_plaintext(
    notification: &mut Value,
    plaintext: &Map<String, Value>,
) -> (String, String, String, Value) {
    let Some(params) = notification
        .get_mut("params")
        .and_then(Value::as_object_mut)
    else {
        return (String::new(), String::new(), String::new(), Value::Null);
    };
    let original_body = params.get("body").cloned().unwrap_or(Value::Null);
    let (sender_did, message_id, original_content_type) = {
        let Some(meta) = params.get_mut("meta").and_then(Value::as_object_mut) else {
            return (String::new(), String::new(), String::new(), original_body);
        };
        let sender_did = string_from_object(Some(meta), "sender_did");
        let message_id = string_from_object(Some(meta), "message_id");
        let original_content_type = string_from_object(Some(meta), "content_type");
        meta.insert(
            "content_type".to_string(),
            Value::String(string_from_object(
                Some(plaintext),
                "application_content_type",
            )),
        );
        (sender_did, message_id, original_content_type)
    };
    params.insert(
        "body".to_string(),
        Value::Object(plaintext_body_to_notification_body(&Value::Object(
            plaintext.clone(),
        ))),
    );
    params.insert(
        "secure_state".to_string(),
        Value::String("decrypted".to_string()),
    );
    params.insert(
        "secure_wire_content_type".to_string(),
        Value::String(original_content_type.clone()),
    );
    params.insert("secure_wire_body".to_string(), original_body.clone());
    (sender_did, message_id, original_content_type, original_body)
}

fn secure_client_for_record(
    resolved: &Resolved,
    manager: &Manager,
    session_rpcs: &Arc<Mutex<SessionRpcRegistry>>,
    record: &StoredIdentity,
) -> im_core::ImResult<im_core::secure::DirectSecureFileRuntimeClient> {
    let mut rpc_sender =
        SessionSharedRpc::new(session_rpcs, &record.identity_name).map_err(|err| {
            im_core::ImError::TransportUnavailable {
                detail: err.to_string(),
            }
        })?;
    let rpc: Box<im_core::secure::DirectSecureFileRuntimeRpc> = Box::new(move |method, params| {
        rpc_sender
            .send_rpc(method, params)
            .map_err(|err| im_core::ImError::TransportUnavailable {
                detail: err.to_string(),
            })
    });
    im_core::secure::new_direct_secure_file_runtime_client(
        direct_secure_file_runtime_identity(manager, record)?,
        rpc,
        secure_did_resolver(resolved, manager, record),
    )
}

fn direct_secure_file_runtime_identity(
    manager: &Manager,
    record: &StoredIdentity,
) -> im_core::ImResult<im_core::secure::DirectSecureFileRuntimeIdentity> {
    Ok(im_core::secure::DirectSecureFileRuntimeIdentity {
        owner_identity_id: owner_identity_id(record),
        owner_did: record.did.clone(),
        identity_name: record.identity_name.clone(),
        identity_dir: identity_dir_for_record(manager, record)?,
        signing_private_pem: record.key1_private_pem.clone(),
        agreement_private_pem: record.e2ee_agreement_private_pem.clone(),
        local_did_document: record.did_document.clone().unwrap_or(Value::Null),
    })
}

fn identity_dir_for_record(
    manager: &Manager,
    record: &StoredIdentity,
) -> im_core::ImResult<PathBuf> {
    let paths = manager
        .paths_for_identity(&record.identity_name)
        .map_err(|err| im_core::ImError::CredentialFileUnreadable {
            path_kind: "identity_dir".to_string(),
            detail: err.to_string(),
        })?;
    Ok(Path::new(&paths.identity_dir).to_path_buf())
}

fn secure_did_resolver(
    _resolved: &Resolved,
    manager: &Manager,
    record: &StoredIdentity,
) -> Box<dyn FnMut(&str) -> im_core::ImResult<Value>> {
    let manager = manager.clone();
    let record = record.clone();
    Box::new(move |did| {
        if did == record.did {
            if let Some(document) = &record.did_document {
                return Ok(document.clone());
            }
        }
        local_did_document(&manager, did).ok_or_else(|| im_core::ImError::TransportUnavailable {
            detail: format!("resolve DID document: {did}"),
        })
    })
}

fn local_did_document(manager: &Manager, did: &str) -> Option<Value> {
    if did.trim().is_empty() {
        return None;
    }
    manager
        .list()
        .ok()?
        .into_iter()
        .find(|summary| summary.did == did)
        .and_then(|summary| manager.load(&summary.identity_name).ok())
        .and_then(|record| record.did_document)
}

fn owner_identity_id(record: &StoredIdentity) -> String {
    if !record.user_id.trim().is_empty() {
        record.user_id.clone()
    } else if !record.unique_id.trim().is_empty() {
        record.unique_id.clone()
    } else {
        record.identity_name.clone()
    }
}

fn compact_warnings(warnings: Vec<String>) -> Vec<String> {
    let mut seen = Vec::<String>::new();
    for warning in warnings {
        let warning = warning.trim().to_string();
        if warning.is_empty() || seen.contains(&warning) {
            continue;
        }
        seen.push(warning);
    }
    seen
}

fn flush_peer_queued_secure_outbox(
    resolved: &Resolved,
    manager: &Manager,
    session_rpcs: &Arc<Mutex<SessionRpcRegistry>>,
    owner_did: &str,
    peer_did: &str,
) -> Vec<String> {
    let Some(record) = identity_record_by_did(manager, owner_did) else {
        return Vec::new();
    };
    flush_secure_outbox(resolved, manager, session_rpcs, &record, peer_did)
}

fn flush_secure_outbox(
    resolved: &Resolved,
    manager: &Manager,
    session_rpcs: &Arc<Mutex<SessionRpcRegistry>>,
    record: &StoredIdentity,
    peer_did: &str,
) -> Vec<String> {
    let connection = match store::open(&resolved.paths) {
        Ok(connection) => connection,
        Err(err) => return vec![format!("Failed to open secure outbox store: {err}")],
    };
    if let Err(err) = store::ensure_schema(&connection) {
        return vec![format!("Failed to ensure secure outbox schema: {err}")];
    }
    let mut client = match secure_client_for_record(resolved, manager, session_rpcs, record) {
        Ok(client) => client,
        Err(err) => return vec![format!("Failed to initialize secure outbox flusher: {err}")],
    };
    im_core::secure::flush_direct_secure_file_outbox(
        &im_core::secure::DirectSecureFileOutboxFlushScope {
            owner_identity_id: owner_identity_id(record),
            owner_did: record.did.clone(),
            credential_name: record.identity_name.clone(),
            sqlite_path: PathBuf::from(&resolved.paths.database_file),
        },
        peer_did,
        &mut client,
    )
}

fn deliver_local_secure_ack_in_process(
    resolved: &Resolved,
    manager: &Manager,
    status: &Arc<Mutex<Status>>,
    local_notifications: &Arc<Mutex<LocalNotificationQueue>>,
    session_rpcs: &Arc<Mutex<SessionRpcRegistry>>,
    sender_record: &StoredIdentity,
    recipient_did: &str,
    session_id: &str,
    replied_message_id: &str,
    ack_message_id: &str,
) -> bool {
    let Some(recipient_record) = identity_record_by_did(manager, recipient_did) else {
        return false;
    };
    let Ok(sender_identity_dir) = identity_dir_for_record(manager, sender_record) else {
        return false;
    };
    let Ok(ack_body_value) = im_core::secure::encrypt_direct_secure_file_ack(
        &im_core::secure::DirectSecureLocalAckInput {
            sender_did: sender_record.did.clone(),
            recipient_did: recipient_did.to_string(),
            sender_identity_dir,
            recipient: im_core::secure::DirectSecureLocalAckRecipient {
                identity: match direct_secure_file_runtime_identity(manager, &recipient_record) {
                    Ok(identity) => identity,
                    Err(_) => return false,
                },
                session_id: session_id.to_string(),
            },
            session_id: session_id.to_string(),
            replied_message_id: replied_message_id.to_string(),
            ack_message_id: ack_message_id.to_string(),
        },
    ) else {
        return false;
    };
    let metadata_sender_did = sender_record.did.clone();
    if let Some(active_session) = active_session_by_did(status, recipient_did) {
        if SessionSharedRpc::new(session_rpcs, &active_session.identity_name).is_ok() {
            let _ = flush_secure_outbox(
                resolved,
                manager,
                session_rpcs,
                &recipient_record,
                &sender_record.did,
            );
        }
        return true;
    }
    if has_runtime_session_for_did(status, manager, recipient_did) {
        let notification = Value::Object(Map::from_iter([
            (
                "method".to_string(),
                Value::String("direct.incoming".to_string()),
            ),
            (
                "params".to_string(),
                Value::Object(Map::from_iter([
                    (
                        "meta".to_string(),
                        Value::Object(Map::from_iter([
                            (
                                "sender_did".to_string(),
                                Value::String(metadata_sender_did.clone()),
                            ),
                            (
                                "target".to_string(),
                                Value::Object(Map::from_iter([
                                    ("kind".to_string(), Value::String("agent".to_string())),
                                    ("did".to_string(), Value::String(recipient_did.to_string())),
                                ])),
                            ),
                            (
                                "message_id".to_string(),
                                Value::String(ack_message_id.to_string()),
                            ),
                            (
                                "profile".to_string(),
                                Value::String("anp.direct.e2ee.v1".to_string()),
                            ),
                            (
                                "security_profile".to_string(),
                                Value::String("direct-e2ee".to_string()),
                            ),
                            (
                                "content_type".to_string(),
                                Value::String("application/anp-direct-cipher+json".to_string()),
                            ),
                        ])),
                    ),
                    ("body".to_string(), ack_body_value),
                ])),
            ),
        ]));
        queue_local_notification(local_notifications, recipient_did, notification);
        return true;
    }
    false
}

fn deliver_local_secure_ack(
    resolved: &Resolved,
    manager: &Manager,
    status: &Arc<Mutex<Status>>,
    host_notify: &Arc<HostNotifySinkImpl>,
    local_notifications: &Arc<Mutex<LocalNotificationQueue>>,
    session_rpcs: &Arc<Mutex<SessionRpcRegistry>>,
    sender_record: &StoredIdentity,
    recipient_did: &str,
    fallback_message_id: &str,
    ack_result: &Value,
) {
    let Some(recipient_record) = identity_record_by_did(manager, recipient_did) else {
        return;
    };
    let active = active_session_by_did(status, recipient_did).is_some();
    let plan = deliver_local_secure_ack_plan(
        active,
        &sender_record.did,
        recipient_did,
        fallback_message_id,
        ack_result,
    );
    let Some(notification) = ack_delivery_notification(&plan.actions) else {
        return;
    };
    let notification = Value::Object(notification);
    let secure_normalization = normalize_secure_notification(
        resolved,
        manager,
        status,
        host_notify,
        local_notifications,
        session_rpcs,
        &recipient_record,
        &notification,
    );
    let mut connection = match store::open(&resolved.paths) {
        Ok(connection) => connection,
        Err(_) => return,
    };
    if store::ensure_schema(&connection).is_err() {
        return;
    }
    let mut guard = status.lock().expect("listener status mutex poisoned");
    let session = NotificationSessionContext {
        identity_name: recipient_record.identity_name.clone(),
        did: recipient_record.did.clone(),
        handle: recipient_record.handle.clone(),
    };
    let mut lookup = |did: &str| lookup_listener_handle_by_did(resolved, did);
    let _ = handle_listener_notification(
        &mut connection,
        Some(host_notify.as_ref()),
        &mut guard,
        &notification,
        &session,
        secure_normalization,
        None,
        Some(&mut lookup),
    );
    let _ = listener::write_status(&guard.status_file, &guard);
}

fn identity_record_by_did(manager: &Manager, did: &str) -> Option<StoredIdentity> {
    let did = did.trim();
    if did.is_empty() {
        return None;
    }
    manager
        .list()
        .ok()?
        .into_iter()
        .find(|summary| summary.did == did)
        .and_then(|summary| manager.load(&summary.identity_name).ok())
}

fn active_session_by_did(status: &Arc<Mutex<Status>>, did: &str) -> Option<SessionStatus> {
    if did.trim().is_empty() {
        return None;
    }
    status
        .lock()
        .ok()?
        .sessions
        .iter()
        .find(|session| session.did == did)
        .cloned()
}

fn has_runtime_session_for_did(status: &Arc<Mutex<Status>>, manager: &Manager, did: &str) -> bool {
    if did.trim().is_empty() {
        return false;
    }
    let sessions = status
        .lock()
        .map(|status| status.sessions.clone())
        .unwrap_or_default();
    for session in sessions {
        if session.did == did {
            return true;
        }
        if manager
            .load(&session.identity_name)
            .is_ok_and(|record| record.did == did)
        {
            return true;
        }
    }
    false
}

fn queue_local_notification(
    local_notifications: &Arc<Mutex<LocalNotificationQueue>>,
    recipient_did: &str,
    notification: Value,
) {
    let Value::Object(notification) = notification else {
        return;
    };
    local_notifications
        .lock()
        .expect("local notification queue mutex poisoned")
        .queue_local_notification(recipient_did.to_string(), Some(notification));
}

fn ack_delivery_notification(
    actions: &[LocalSecureAckDeliveryAction],
) -> Option<Map<String, Value>> {
    match actions {
        [LocalSecureAckDeliveryAction::HandleNotification { notification }] => {
            notification.as_object().cloned()
        }
        _ => None,
    }
}

fn set_notification_method(notification: &mut Value, method: &str) {
    if let Some(object) = notification.as_object_mut() {
        object.insert("method".to_string(), Value::String(method.to_string()));
    }
}

fn string_from_object(object: Option<&Map<String, Value>>, key: &str) -> String {
    string_value(object.and_then(|object| object.get(key)))
}

fn string_value(value: Option<&Value>) -> String {
    value
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn mark_session_connected(status: &Arc<Mutex<Status>>, identity_name: &str, did: &str) {
    let mut guard = status.lock().expect("listener status mutex poisoned");
    upsert_session(
        &mut guard,
        SessionStatus {
            identity_name: identity_name.to_string(),
            did: did.to_string(),
            connected: true,
            last_error: String::new(),
        },
    );
    let _ = listener::write_status(&guard.status_file, &guard);
}

fn session_is_connected(status: &Arc<Mutex<Status>>, identity_name: &str) -> bool {
    status
        .lock()
        .map(|status| {
            status
                .sessions
                .iter()
                .any(|session| session.identity_name == identity_name && session.connected)
        })
        .unwrap_or(false)
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

fn resolve_identity_name(manager: &Manager, identity_name: &str) -> anyhow::Result<String> {
    let trimmed = identity_name.trim();
    if !trimmed.is_empty() {
        return Ok(trimmed.to_string());
    }
    Ok(manager.current()?.identity_name)
}

fn user_registration_error(record: &StoredIdentity, state: &identity::UserState) -> String {
    let identity_name = if record.identity_name.trim().is_empty() {
        "active identity"
    } else {
        record.identity_name.trim()
    };
    let missing = if state.missing.is_empty() {
        "user registration metadata".to_string()
    } else {
        state.missing.join(", ")
    };
    format!(
        "registered handle user is required: identity {} is {} and missing {}; complete user setup with `awiki-cli id register --handle <handle> ...` or recover an existing handle first",
        identity_name, state.registration_state, missing
    )
}

fn resolve_runtime_boot_id(resolved: &Resolved) -> anyhow::Result<String> {
    let path = listener::boot_id_path(resolved)?;
    match listener::read_expected_boot_id(&path) {
        Ok(boot_id) if !boot_id.trim().is_empty() => Ok(boot_id.trim().to_string()),
        Ok(_) => Ok(generate_boot_id()),
        Err(err) if is_not_found(&err) => Ok(generate_boot_id()),
        Err(err) => Err(anyhow::anyhow!("read expected listener boot id: {err}")),
    }
}

fn generate_boot_id() -> String {
    let mut suffix = [0_u8; 4];
    rand::thread_rng().fill_bytes(&mut suffix);
    format!(
        "boot-{}-{}",
        now_unix_nanos(),
        suffix
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    )
}

fn now_unix_nanos() -> i128 {
    let now = OffsetDateTime::now_utc();
    i128::from(now.unix_timestamp()) * 1_000_000_000 + i128::from(now.nanosecond())
}

fn cleanup_runtime_artifacts(resolved: &Resolved) {
    if let Ok(paths) = listener::paths(resolved) {
        let _ = fs::remove_file(paths.pid_file);
        let _ = fs::remove_file(paths.status_file);
        let _ = fs::remove_file(paths.socket_path);
    }
    if let Ok(path) = listener::boot_id_path(resolved) {
        let _ = fs::remove_file(path);
    }
}

fn running_in_listener_service_mode() -> bool {
    let env_value = std::env::var("AWIKI_LISTENER_SERVICE_MODE").unwrap_or_default();
    if env_value.trim() == "1" || env_value.trim().eq_ignore_ascii_case("true") {
        return true;
    }
    let args = std::env::args().collect::<Vec<_>>();
    args.len() >= 4
        && args[1].eq_ignore_ascii_case("runtime")
        && args[2].eq_ignore_ascii_case("listener")
        && args[3].eq_ignore_ascii_case("service-run")
}

fn wait_for_shutdown_signal(shutdown: Arc<AtomicBool>) {
    wait_for_foreground_shutdown(&shutdown);
}

fn sleep_or_shutdown(shutdown: &AtomicBool, duration: Duration) {
    let deadline = std::time::Instant::now() + duration;
    while !shutdown.load(Ordering::SeqCst) && std::time::Instant::now() < deadline {
        thread::sleep(Duration::from_millis(100));
    }
}

fn is_not_found(err: &anyhow::Error) -> bool {
    err.downcast_ref::<std::io::Error>()
        .is_some_and(|err| err.kind() == std::io::ErrorKind::NotFound)
        || err.to_string().contains("No such file")
}

#[cfg(test)]
mod tests {
    use super::*;

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
