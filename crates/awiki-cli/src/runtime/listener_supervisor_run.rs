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
use super::listener_local_notification_flush::{
    flush_queued_local_notifications, LocalNotificationFlushTargetSession,
};
use super::listener_local_notifications::{LocalNotification, LocalNotificationQueue};
use super::listener_notification_consume::SESSION_PING_INTERVAL;
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
use super::listener_secure_replay::{
    secure_pending_history_replay_candidates, secure_unread_replay_candidates, ReplayStoreLookup,
};
use super::listener_secure_sessions;
use super::listener_secure_sync::{
    SECURE_DIRECT_SYNC_TIMEOUT, SECURE_PENDING_HISTORY_LIMIT, SECURE_UNREAD_INBOX_LIMIT,
};
use super::listener_service_did::{
    fetch_message_service_did, ListenerServiceDidRpc, ListenerServiceDidSession,
};
use super::listener_session_methods::SessionDisconnectReason;
use super::listener_shutdown_signal::{
    install_foreground_shutdown_handler, wait_for_foreground_shutdown,
};
use super::listener_ws_transport::{WsDialError, WsTransport};
use crate::anpsdk::{
    self, ApplicationPlaintext, DirectEnvelopeMetadata, FileSessionStore, RatchetHeader,
};
use crate::authsdk::Session;
use crate::config::{self, Resolved};
use crate::identity::{self, types::StoredIdentity, Manager};
use crate::message;
use crate::runtime;
use crate::store;
use rand::RngCore;
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::fs;
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
    run_listener(resolved)
}

pub fn run_service(resolved: Resolved) -> anyhow::Result<()> {
    run_listener(resolved)
}

fn run_listener(resolved: Resolved) -> anyhow::Result<()> {
    let mut supervisor = ListenerSupervisor::new(resolved)?;
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
}

impl ListenerSupervisor {
    fn new(resolved: Resolved) -> anyhow::Result<Self> {
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
        })
    }

    fn run(&mut self) -> anyhow::Result<()> {
        install_foreground_shutdown_handler(self.shutdown.clone())?;
        if runtime::resolve(&self.resolved).mode != bridge::MODE_WEBSOCKET {
            anyhow::bail!("runtime mode must be websocket before starting the listener");
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
            self.ensure_session_background(&summary.identity_name, &summary.did);
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
                            );
                        }
                    }
                }
                sleep_or_shutdown(&shutdown, WATCH_IDENTITIES_INTERVAL);
            }
        });
    }

    fn ensure_session_background(&self, identity_name: &str, did: &str) {
        let already_known = self
            .lock_status()
            .sessions
            .iter()
            .any(|session| session.identity_name == identity_name);
        if already_known {
            return;
        }
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
            None,
        );
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

#[derive(Clone)]
struct SessionRpcSender {
    tx: mpsc::Sender<SessionRpcRequest>,
    active: Arc<Mutex<bool>>,
}

#[derive(Default)]
struct SessionRpcRegistry {
    senders: BTreeMap<String, SessionRpcSender>,
}

impl SessionRpcRegistry {
    fn set(&mut self, identity_name: &str, sender: SessionRpcSender) {
        self.senders.insert(identity_name.to_string(), sender);
    }

    fn remove(&mut self, identity_name: &str) {
        self.senders.remove(identity_name);
    }

    fn get(&self, identity_name: &str) -> Option<SessionRpcSender> {
        self.senders.get(identity_name).cloned()
    }
}

struct SessionRpcRequest {
    method: String,
    params: Map<String, Value>,
    timeout: Option<Duration>,
    response_tx: mpsc::Sender<anyhow::Result<Map<String, Value>>>,
}

struct PendingSessionRpc {
    response_tx: mpsc::Sender<anyhow::Result<Map<String, Value>>>,
    expires_at: Option<Instant>,
}

struct SessionNotificationTask {
    notification: Map<String, Value>,
}

struct SessionSharedRpc {
    sender: SessionRpcSender,
}

impl SessionSharedRpc {
    fn new(registry: &Arc<Mutex<SessionRpcRegistry>>, identity_name: &str) -> anyhow::Result<Self> {
        let sender = registry
            .lock()
            .map(|registry| registry.get(identity_name))
            .unwrap_or(None)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "{}",
                    super::listener_service_did::disconnected_websocket_session_error(
                        identity_name
                    )
                )
            })?;
        Ok(Self { sender })
    }

    fn send_rpc_with_timeout(
        &mut self,
        method: &str,
        params: Map<String, Value>,
        timeout: Option<Duration>,
    ) -> anyhow::Result<Map<String, Value>> {
        let (response_tx, response_rx) = mpsc::channel();
        {
            let active = self
                .sender
                .active
                .lock()
                .map_err(|_| anyhow::anyhow!("websocket session rpc loop is closed"))?;
            if !*active {
                anyhow::bail!("websocket session rpc loop is closed");
            }
            self.sender
                .tx
                .send(SessionRpcRequest {
                    method: method.to_string(),
                    params,
                    timeout,
                    response_tx,
                })
                .map_err(|_| anyhow::anyhow!("websocket session rpc loop is closed"))?;
        }
        response_rx
            .recv()
            .map_err(|_| anyhow::anyhow!("websocket session rpc loop is closed"))?
    }
}

impl ListenerServiceDidRpc for SessionSharedRpc {
    fn send_rpc(
        &mut self,
        method: &str,
        params: Map<String, Value>,
    ) -> anyhow::Result<Map<String, Value>> {
        self.send_rpc_with_timeout(method, params, None)
    }
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
                    flush_local_notifications_for_session(
                        &resolved,
                        &manager,
                        &status,
                        &host_notify,
                        &local_notifications,
                        &session_rpcs,
                        &record,
                    );
                    spawn_secure_prekey_retry(
                        resolved.clone(),
                        manager.clone(),
                        status.clone(),
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
                    let error = consume_notifications(
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
                    .map(|err| err.to_string());
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
    let mut auth = message::auth_session(resolved, manager, &record)?;
    seed_message_scopes(resolved, &mut auth, record.jwt_token.trim());
    let token = ensure_ws_bearer(resolved, &mut auth)?;
    if token.trim() != record.jwt_token.trim() {
        let _ = manager.update_jwt(&record.identity_name, token.trim());
    }
    let endpoints = super::listener_wsclient::listener_ws_client_endpoints(resolved)?;
    let transport = connect_ws_with_refresh(
        resolved,
        manager,
        &record.identity_name,
        &mut auth,
        &endpoints.websocket_url,
        &token,
    )?;
    let mut updated = record;
    updated.jwt_token = auth.current_jwt().to_string();
    Ok((updated, transport))
}

fn connect_ws_with_refresh(
    resolved: &Resolved,
    manager: &Manager,
    identity_name: &str,
    auth: &mut Session,
    websocket_url: &str,
    token: &str,
) -> anyhow::Result<WsTransport> {
    match WsTransport::connect(websocket_url, token, &resolved.ca_bundle) {
        Ok(transport) => Ok(transport),
        Err(err) if err.status_code == Some(401) => {
            let refreshed = ensure_ws_bearer(resolved, auth)?;
            manager.update_jwt(identity_name, refreshed.trim())?;
            WsTransport::connect(websocket_url, &refreshed, &resolved.ca_bundle)
                .map_err(|err| anyhow::anyhow!(err))
        }
        Err(err) => Err(format_dial_error(err)),
    }
}

fn ensure_ws_bearer(resolved: &Resolved, auth: &mut Session) -> anyhow::Result<String> {
    let endpoints = super::listener_wsclient::listener_ws_client_endpoints(resolved)?;
    if !auth.current_jwt().trim().is_empty() {
        return Ok(auth.current_jwt().trim().to_string());
    }
    auth.ensure_jwt_profile_traced(
        &crate::transportcfg::new_http_client(&resolved.ca_bundle)?,
        crate::transportcfg::Profile::AuthRefresh,
        &endpoints.did_auth_url,
        "listener_websocket_bootstrap",
    )
}

fn format_dial_error(err: WsDialError) -> anyhow::Error {
    anyhow::anyhow!(err)
}

fn spawn_secure_prekey_retry(
    resolved: Arc<Resolved>,
    manager: Manager,
    status: Arc<Mutex<Status>>,
    shutdown: Arc<AtomicBool>,
    record: StoredIdentity,
) {
    let identity_name = record.identity_name.clone();
    thread::spawn(move || {
        while !shutdown.load(Ordering::SeqCst) && session_is_connected(&status, &identity_name) {
            let warnings = message::maybe_publish_secure_prekeys(&resolved, &manager, &record);
            match super::listener_session_loop::secure_prekey_retry_decision(
                &identity_name,
                &warnings,
            ) {
                super::listener_session_loop::SecurePrekeyRetryDecision::Finished => return,
                super::listener_session_loop::SecurePrekeyRetryDecision::Retry {
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
    let params = message::build_inbox_rpc_params(
        record,
        message::InboxRequest {
            scope: "direct".to_string(),
            unread_only: true,
            limit: SECURE_UNREAD_INBOX_LIMIT,
            ..message::InboxRequest::default()
        },
    );
    let Value::Object(params) = params else {
        return;
    };
    let Ok(result) =
        rpc.send_rpc_with_timeout("inbox.get", params, Some(SECURE_DIRECT_SYNC_TIMEOUT))
    else {
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
        let Ok(params) = message::build_history_rpc_params(
            record,
            message::HistoryRequest {
                with: peer_did,
                limit: SECURE_PENDING_HISTORY_LIMIT,
                ..message::HistoryRequest::default()
            },
        ) else {
            continue;
        };
        let Value::Object(params) = params else {
            continue;
        };
        let Ok(result) = rpc.send_rpc_with_timeout(
            "direct.get_history",
            params,
            Some(SECURE_DIRECT_SYNC_TIMEOUT),
        ) else {
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
    let messages = result
        .get("messages")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut lookup = |message_id: &str, owner_did: &str, _credential_name: &str| {
        if cached_message_exists(resolved, owner_did, message_id) {
            ReplayStoreLookup::Exists
        } else {
            ReplayStoreLookup::Missing
        }
    };
    let candidates = if skip_self_sent {
        secure_pending_history_replay_candidates(
            &messages,
            &record.did,
            &record.identity_name,
            &mut lookup,
        )
    } else {
        secure_unread_replay_candidates(&messages, &record.did, &record.identity_name, &mut lookup)
    };
    for candidate in candidates {
        handle_replayed_secure_notification(
            resolved,
            manager,
            status,
            host_notify,
            local_notifications,
            session_rpcs,
            record,
            &candidate.notification,
        );
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

fn set_session_rpc_sender(
    registry: &Arc<Mutex<SessionRpcRegistry>>,
    identity_name: &str,
    sender: SessionRpcSender,
) {
    if let Ok(mut registry) = registry.lock() {
        registry.set(identity_name, sender);
    }
}

fn remove_session_rpc_sender(registry: &Arc<Mutex<SessionRpcRegistry>>, identity_name: &str) {
    if let Ok(mut registry) = registry.lock() {
        registry.remove(identity_name);
    }
}

fn drain_session_rpc_requests(
    transport: &mut WsTransport,
    rpc_rx: &mpsc::Receiver<SessionRpcRequest>,
    next_id: &mut i64,
    dispatch: &mut super::listener_wsclient::ListenerWsPendingDispatch,
    pending_rpc_responses: &mut BTreeMap<String, PendingSessionRpc>,
) -> anyhow::Result<()> {
    loop {
        match rpc_rx.try_recv() {
            Ok(request) => {
                let request_id = super::listener_wsclient::next_ws_rpc_request_id(next_id);
                dispatch.register_pending(request_id.clone());
                pending_rpc_responses.insert(
                    request_id.clone(),
                    PendingSessionRpc {
                        response_tx: request.response_tx,
                        expires_at: request.timeout.map(|timeout| Instant::now() + timeout),
                    },
                );
                let payload = super::listener_wsclient::build_ws_rpc_request(
                    &request_id,
                    &request.method,
                    Some(request.params),
                );
                if let Err(err) = transport.send_json(&payload) {
                    let _ = dispatch.remove_pending(&request_id);
                    if let Some(pending) = pending_rpc_responses.remove(&request_id) {
                        let _ = pending.response_tx.send(Err(err));
                    }
                }
            }
            Err(mpsc::TryRecvError::Empty) => return Ok(()),
            Err(mpsc::TryRecvError::Disconnected) => return Ok(()),
        }
    }
}

fn route_session_rpc_response(
    message: Map<String, Value>,
    dispatch: &mut super::listener_wsclient::ListenerWsPendingDispatch,
    pending_rpc_responses: &mut BTreeMap<String, PendingSessionRpc>,
) {
    let outcome = dispatch.route_incoming_message(message);
    let super::listener_wsclient::ListenerWsDispatchOutcome::RoutedResponse { request_id } =
        outcome
    else {
        return;
    };
    let Some(response) = dispatch.take_pending_response(&request_id) else {
        return;
    };
    let _ = dispatch.remove_pending(&request_id);
    if let Some(pending) = pending_rpc_responses.remove(&request_id) {
        let _ = pending
            .response_tx
            .send(super::listener_wsclient::decode_ws_rpc_result(&response));
    }
}

fn fail_session_rpc_pending(
    error: &str,
    dispatch: &mut super::listener_wsclient::ListenerWsPendingDispatch,
    pending_rpc_responses: &mut BTreeMap<String, PendingSessionRpc>,
) {
    for request_id in dispatch.fail_pending_requests(error) {
        let response = dispatch
            .take_pending_response(&request_id)
            .unwrap_or_else(|| {
                super::listener_wsclient::pending_failure_response(&request_id, error)
            });
        let _ = dispatch.remove_pending(&request_id);
        if let Some(pending) = pending_rpc_responses.remove(&request_id) {
            let _ = pending
                .response_tx
                .send(super::listener_wsclient::decode_ws_rpc_result(&response));
        }
    }
}

fn fail_session_rpc_queued(error: &str, rpc_rx: &mpsc::Receiver<SessionRpcRequest>) {
    loop {
        match rpc_rx.try_recv() {
            Ok(request) => {
                let _ = request
                    .response_tx
                    .send(Err(anyhow::anyhow!(error.to_string())));
            }
            Err(mpsc::TryRecvError::Empty | mpsc::TryRecvError::Disconnected) => break,
        }
    }
}

fn close_session_rpc_active(active: &Arc<Mutex<bool>>) {
    if let Ok(mut active) = active.lock() {
        *active = false;
    }
}

fn expire_session_rpc_pending(
    now: Instant,
    dispatch: &mut super::listener_wsclient::ListenerWsPendingDispatch,
    pending_rpc_responses: &mut BTreeMap<String, PendingSessionRpc>,
) {
    let expired = pending_rpc_responses
        .iter()
        .filter_map(|(request_id, pending)| {
            pending
                .expires_at
                .filter(|expires_at| *expires_at <= now)
                .map(|_| request_id.clone())
        })
        .collect::<Vec<_>>();
    for request_id in expired {
        let _ = dispatch.remove_pending(&request_id);
        if let Some(pending) = pending_rpc_responses.remove(&request_id) {
            let _ = pending
                .response_tx
                .send(Err(anyhow::anyhow!("context deadline exceeded")));
        }
    }
}

fn consume_notifications(
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
    let mut next_ping = Instant::now() + SESSION_PING_INTERVAL;
    let mut next_id = 0_i64;
    let mut dispatch = super::listener_wsclient::ListenerWsPendingDispatch::default();
    let mut pending_rpc_responses = BTreeMap::<String, PendingSessionRpc>::new();
    let mut loop_result = Ok(());
    while !shutdown.load(Ordering::SeqCst) {
        if let Err(err) = drain_session_rpc_requests(
            transport,
            rpc_rx,
            &mut next_id,
            &mut dispatch,
            &mut pending_rpc_responses,
        ) {
            loop_result = Err(err);
            break;
        }
        expire_session_rpc_pending(Instant::now(), &mut dispatch, &mut pending_rpc_responses);
        if Instant::now() >= next_ping {
            if let Err(err) = transport
                .ping_with_timeout_until(
                    super::listener_notification_consume::SESSION_PING_TIMEOUT,
                    || shutdown.load(Ordering::SeqCst),
                )
                .map_err(|err| anyhow::anyhow!("websocket ping failed: {err}"))
            {
                loop_result = Err(err);
                break;
            }
            next_ping = Instant::now() + SESSION_PING_INTERVAL;
        }
        let notification = match transport.read_json_message_timeout(SESSION_NOTIFICATION_READ_POLL)
        {
            Ok(Some(notification)) => notification,
            Ok(None) => continue,
            Err(err) => {
                loop_result = Err(err);
                break;
            }
        };
        if notification.get("id").is_some() {
            route_session_rpc_response(notification, &mut dispatch, &mut pending_rpc_responses);
            continue;
        }
        if notification_tx
            .send(SessionNotificationTask { notification })
            .is_err()
        {
            loop_result = Err(anyhow::anyhow!("websocket notification handler is closed"));
            break;
        }
    }
    let close_error = loop_result
        .as_ref()
        .err()
        .map(|err| err.to_string())
        .unwrap_or_else(|| "websocket notification loop closed".to_string());
    close_session_rpc_active(rpc_active);
    fail_session_rpc_pending(&close_error, &mut dispatch, &mut pending_rpc_responses);
    fail_session_rpc_queued(&close_error, rpc_rx);
    drop(notification_tx);
    let _ = handler.join();
    loop_result
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
    let mut client = match secure_client_for_record(manager, session_rpcs, record) {
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

    if message::is_secure_ack_plaintext(&plaintext) {
        let _ = flush_secure_outbox(resolved, manager, session_rpcs, record, &sender_did);
        set_notification_method(&mut normalized, "direct.secure.ack");
        return SecureNotificationNormalization::Replace(normalized);
    }

    if message::is_secure_init_plaintext(&plaintext) {
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
                    message::build_secure_ack_payload(&session_id, &message_id),
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
    manager: &Manager,
    session_rpcs: &Arc<Mutex<SessionRpcRegistry>>,
    record: &StoredIdentity,
) -> Result<message::MessageServiceE2EEClient, String> {
    let mut rpc_sender = SessionSharedRpc::new(session_rpcs, &record.identity_name)
        .map_err(|err| err.to_string())?;
    let rpc: Box<message::SecureE2EERpc> = Box::new(move |method, params| {
        rpc_sender
            .send_rpc(method, params)
            .map_err(|err| err.to_string())
    });
    message::new_secure_e2ee_client_for_record(Some(manager), Some(record), rpc)
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
    let mut client = match secure_client_for_record(manager, session_rpcs, record) {
        Ok(client) => client,
        Err(err) => return vec![format!("Failed to initialize secure outbox flusher: {err}")],
    };
    message::flush_queued_secure_outbox_with_sender(
        &connection,
        &record.did,
        &record.identity_name,
        peer_did,
        |request| {
            let result = match request.original_type.as_str() {
                "text" | "" => client.send_text(
                    &request.target_did,
                    &request.plaintext,
                    &request.outbox_id,
                    &request.outbox_id,
                ),
                "json" => client.send_json(
                    &request.target_did,
                    request.json_payload.unwrap_or_default(),
                    &request.outbox_id,
                    &request.outbox_id,
                ),
                _ => Err(format!(
                    "unsupported original_type: {}",
                    request.original_type
                )),
            };
            match result {
                Ok(result) => message::SecureOutboxSendOutcome::Success {
                    message_id: string_value(result.get("message_id")),
                    operation_id: string_value(result.get("operation_id")),
                    delivery_state: string_value(result.get("delivery_state")),
                    accepted_at: string_value(result.get("accepted_at")),
                },
                Err(err) => message::SecureOutboxSendOutcome::Error(err),
            }
        },
        |peer_did| message::current_secure_session_id(Some(manager), Some(record), peer_did),
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
    let Ok(sender_paths) = manager.paths_for_identity(&sender_record.identity_name) else {
        return false;
    };
    let mut sender_store = match FileSessionStore::new(
        std::path::Path::new(&sender_paths.identity_dir).join("p5-e2ee-sessions"),
    ) {
        Ok(store) => store,
        Err(_) => return false,
    };
    let sender_session = match sender_store.find_by_peer_did(recipient_did) {
        Ok(Some(session)) => session,
        _ => return false,
    };
    let metadata = DirectEnvelopeMetadata {
        sender_did: sender_record.did.clone(),
        recipient_did: recipient_did.to_string(),
        message_id: ack_message_id.to_string(),
        profile: "anp.direct.e2ee.v1".to_string(),
        security_profile: "direct-e2ee".to_string(),
    };
    let ack_plaintext = ApplicationPlaintext {
        application_content_type: "application/json".to_string(),
        conversation_id: None,
        reply_to_message_id: None,
        annotations: None,
        text: None,
        payload: Some(Value::Object(message::build_secure_ack_payload(
            session_id,
            replied_message_id,
        ))),
        payload_b64u: None,
    };
    let mut candidate_session = sender_session;
    let Ok((_pending, ack_body)) = anpsdk::DirectE2eeSession::encrypt_follow_up(
        &mut candidate_session,
        &metadata,
        ack_message_id,
        &ack_plaintext,
    ) else {
        return false;
    };
    let ack_body_value = serde_json::to_value(&ack_body).unwrap_or(Value::Null);
    if !recipient_can_process_local_secure_ack(
        manager,
        &recipient_record,
        &metadata,
        &ack_body_value,
        session_id,
    ) {
        return false;
    }
    if sender_store.save_session(&candidate_session).is_err() {
        return false;
    }
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
                                Value::String(metadata.sender_did.clone()),
                            ),
                            (
                                "target".to_string(),
                                Value::Object(Map::from_iter([
                                    ("kind".to_string(), Value::String("agent".to_string())),
                                    (
                                        "did".to_string(),
                                        Value::String(metadata.recipient_did.clone()),
                                    ),
                                ])),
                            ),
                            (
                                "message_id".to_string(),
                                Value::String(metadata.message_id.clone()),
                            ),
                            (
                                "profile".to_string(),
                                Value::String(metadata.profile.clone()),
                            ),
                            (
                                "security_profile".to_string(),
                                Value::String(metadata.security_profile.clone()),
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

fn recipient_can_process_local_secure_ack(
    manager: &Manager,
    recipient_record: &StoredIdentity,
    metadata: &DirectEnvelopeMetadata,
    ack_body: &Value,
    session_id: &str,
) -> bool {
    let mut client = match message::new_secure_e2ee_client_for_record(
        Some(manager),
        Some(recipient_record),
        Box::new(|_, _| Err("local secure ack delivery does not use outbound rpc".to_string())),
    ) {
        Ok(client) => client,
        Err(_) => return false,
    };
    let processed = client.process_incoming(Map::from_iter([
        (
            "meta".to_string(),
            Value::Object(Map::from_iter([
                (
                    "sender_did".to_string(),
                    Value::String(metadata.sender_did.clone()),
                ),
                (
                    "target".to_string(),
                    Value::Object(Map::from_iter([
                        ("kind".to_string(), Value::String("agent".to_string())),
                        (
                            "did".to_string(),
                            Value::String(metadata.recipient_did.clone()),
                        ),
                    ])),
                ),
                (
                    "message_id".to_string(),
                    Value::String(metadata.message_id.clone()),
                ),
                (
                    "profile".to_string(),
                    Value::String(metadata.profile.clone()),
                ),
                (
                    "security_profile".to_string(),
                    Value::String(metadata.security_profile.clone()),
                ),
                (
                    "content_type".to_string(),
                    Value::String("application/anp-direct-cipher+json".to_string()),
                ),
            ])),
        ),
        ("body".to_string(), ack_body.clone()),
    ]));
    if processed
        .ok()
        .is_some_and(|result| string_value(result.get("state")) == "decrypted")
    {
        return true;
    }

    let Ok(paths) = manager.paths_for_identity(&recipient_record.identity_name) else {
        return false;
    };
    let mut recipient_store = match FileSessionStore::new(
        std::path::Path::new(&paths.identity_dir).join("p5-e2ee-sessions"),
    ) {
        Ok(store) => store,
        Err(_) => return false,
    };
    let mut recipient_session = match recipient_store.load_session(session_id) {
        Ok(session) => session,
        Err(_) => return false,
    };
    let Some(ack_body_object) = ack_body.as_object() else {
        return false;
    };
    let ratchet_header = ack_body_object
        .get("ratchet_header")
        .and_then(Value::as_object);
    let direct_body = anpsdk::DirectCipherBody {
        session_id: string_from_object(Some(ack_body_object), "session_id"),
        suite: nonempty_string(ack_body_object.get("suite")),
        ratchet_header: RatchetHeader {
            dh_pub_b64u: string_from_object(ratchet_header, "dh_pub_b64u"),
            pn: string_from_object(ratchet_header, "pn"),
            n: string_from_object(ratchet_header, "n"),
        },
        ciphertext_b64u: string_from_object(Some(ack_body_object), "ciphertext_b64u"),
    };
    if anpsdk::DirectE2eeSession::decrypt_follow_up(
        &mut recipient_session,
        metadata,
        &direct_body,
        "",
    )
    .is_err()
    {
        return false;
    }
    recipient_store.save_session(&recipient_session).is_ok()
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

fn nonempty_string(value: Option<&Value>) -> Option<String> {
    let value = string_value(value);
    (!value.is_empty()).then_some(value)
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

fn seed_message_scopes(resolved: &Resolved, auth: &mut Session, token: &str) {
    let base_url = resolved.service_base_url.trim();
    if base_url.is_empty() {
        return;
    }
    let did_auth_url =
        config::join_base_url(base_url, crate::identity::wire::DID_AUTH_RPC_ENDPOINT);
    let message_ws_url = config::join_base_url(base_url, message::MESSAGE_WS_ENDPOINT);
    auth.remember_scope(base_url);
    auth.remember_scope(&did_auth_url);
    auth.remember_scope(&message_ws_url);
    if !token.trim().is_empty() {
        auth.set_bearer(base_url, token);
        auth.set_bearer(&did_auth_url, token);
        auth.set_bearer(&message_ws_url, token);
    }
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
    use serde_json::json;

    #[test]
    fn session_shared_rpc_sends_timeout_bound_request_through_session_channel() {
        let (tx, rx) = mpsc::channel();
        let mut rpc = SessionSharedRpc {
            sender: SessionRpcSender {
                tx,
                active: Arc::new(Mutex::new(true)),
            },
        };
        let handle = thread::spawn(move || {
            let mut params = Map::new();
            params.insert("limit".to_string(), json!(100));
            rpc.send_rpc_with_timeout("inbox.get", params, Some(SECURE_DIRECT_SYNC_TIMEOUT))
        });

        let request = rx.recv().expect("session rpc request");
        assert_eq!(request.method, "inbox.get");
        assert_eq!(request.timeout, Some(SECURE_DIRECT_SYNC_TIMEOUT));
        assert_eq!(request.params["limit"], 100);
        request
            .response_tx
            .send(Ok(Map::from_iter([("ok".to_string(), json!(true))])))
            .expect("send response");

        let result = handle.join().expect("rpc thread").expect("rpc result");
        assert_eq!(result["ok"], true);
    }

    #[test]
    fn inactive_session_shared_rpc_fails_before_queueing_request() {
        let (tx, rx) = mpsc::channel();
        let mut rpc = SessionSharedRpc {
            sender: SessionRpcSender {
                tx,
                active: Arc::new(Mutex::new(false)),
            },
        };

        let error = rpc
            .send_rpc_with_timeout("inbox.get", Map::new(), Some(SECURE_DIRECT_SYNC_TIMEOUT))
            .expect_err("inactive rpc should fail");

        assert!(error.to_string().contains("rpc loop is closed"));
        assert!(matches!(rx.try_recv(), Err(mpsc::TryRecvError::Empty)));
    }

    #[test]
    fn expired_session_rpc_pending_removes_pending_and_wakes_caller() {
        let mut dispatch = crate::runtime::listener_wsclient::ListenerWsPendingDispatch::default();
        dispatch.register_pending("req-1");
        let (response_tx, response_rx) = mpsc::channel();
        let now = Instant::now();
        let mut pending = BTreeMap::from_iter([(
            "req-1".to_string(),
            PendingSessionRpc {
                response_tx,
                expires_at: Some(now - Duration::from_millis(1)),
            },
        )]);

        expire_session_rpc_pending(now, &mut dispatch, &mut pending);

        assert!(!dispatch.has_pending("req-1"));
        assert!(pending.is_empty());
        let error = response_rx
            .recv()
            .expect("timeout response")
            .expect_err("timeout should fail");
        assert_eq!(error.to_string(), "context deadline exceeded");
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
