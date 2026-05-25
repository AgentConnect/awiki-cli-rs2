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
};
use crate::config::Resolved;
use crate::identity::Manager;
use crate::im_core_adapter::realtime::{
    self as im_core_realtime_adapter, ListenerRunHostKind, ListenerRunnerMode,
};
use crate::runtime;
use crate::runtime::listener_im_event_adapter::CliRealtimeEventSink;
use rand::RngCore;
use serde_json::{Map, Value};
use std::fs;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::thread;
use std::time::Duration;
use time::OffsetDateTime;

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
        spawn_im_core_runner_session(
            self.resolved.clone(),
            self.status.clone(),
            self.host_notify.clone(),
            self.shutdown.clone(),
            identity_name.to_string(),
            did.to_string(),
        );
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
        cleanup_runtime_artifacts(&self.resolved);
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

fn spawn_im_core_runner_session(
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
        let _ = listener::write_status(&guard.status_file, &guard);
    }
    thread::spawn(move || {
        let selector = im_core::IdentitySelector::LocalAlias(identity_name.clone());
        let client = match crate::im_core_adapter::build_im_client(&resolved, selector) {
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
        let sdk_shutdown =
            im_core_realtime_adapter::sdk_shutdown_signal_from_listener_shutdown(&shutdown);
        spawn_sdk_shutdown_bridge(shutdown.clone(), sdk_shutdown.clone());
        let mut event_sink = CliRealtimeEventSink {
            status: &status,
            host_notify: &host_notify,
            identity_name: &identity_name,
            did: &did,
        };
        let exit = client
            .realtime()
            .run_until_shutdown_with_event_sink(
                im_core_realtime_adapter::listener_realtime_options(),
                sdk_shutdown,
                &mut event_sink,
            )
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

fn spawn_sdk_shutdown_bridge(
    shutdown: Arc<AtomicBool>,
    sdk_shutdown: im_core::prelude::ShutdownSignal,
) {
    thread::spawn(move || {
        while !shutdown.load(Ordering::SeqCst) && !sdk_shutdown.is_requested() {
            thread::sleep(Duration::from_millis(100));
        }
        im_core_realtime_adapter::mark_sdk_shutdown_requested(&sdk_shutdown);
    });
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
