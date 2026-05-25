use im_core::prelude::{RealtimeOptions, RealtimeSubscription, ReconnectPolicy, ShutdownSignal};

use crate::host_runtime;

const LISTENER_REALTIME_EVENT_BUFFER: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListenerRunnerMode {
    ImCore,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListenerRunHostKind {
    Foreground,
    Service,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListenerRunnerAction {
    UseImCoreRunner { host: ListenerRunHostKind },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListenerRunnerSelection {
    pub mode: ListenerRunnerMode,
    pub actions: Vec<ListenerRunnerAction>,
}

pub fn listener_runner_selection(host: ListenerRunHostKind) -> ListenerRunnerSelection {
    ListenerRunnerSelection {
        mode: ListenerRunnerMode::ImCore,
        actions: vec![ListenerRunnerAction::UseImCoreRunner { host }],
    }
}

pub fn listener_realtime_options() -> RealtimeOptions {
    RealtimeOptions {
        reconnect: ReconnectPolicy::Disabled,
        event_buffer: LISTENER_REALTIME_EVENT_BUFFER,
        subscriptions: vec![
            RealtimeSubscription::Messages,
            RealtimeSubscription::Groups,
            RealtimeSubscription::Notifications,
            RealtimeSubscription::HostNotifications,
        ],
    }
}

pub fn sdk_shutdown_signal_from_listener_shutdown(
    shutdown: &std::sync::atomic::AtomicBool,
) -> ShutdownSignal {
    let signal = ShutdownSignal::pending();
    if shutdown.load(std::sync::atomic::Ordering::SeqCst) {
        signal.request();
    }
    signal
}

pub fn mark_sdk_shutdown_requested(signal: &ShutdownSignal) {
    signal.request();
}

pub fn map_sdk_runner_error(error: im_core::ImError) -> anyhow::Error {
    match error {
        im_core::ImError::AuthRequired | im_core::ImError::SessionExpired => {
            anyhow::anyhow!(
                "listener authentication is required: refresh the identity token and retry"
            )
        }
        im_core::ImError::TransportUnavailable { detail } => {
            anyhow::anyhow!("runtime listener websocket transport unavailable: {detail}")
        }
        im_core::ImError::UnsupportedCapability { capability } => {
            anyhow::anyhow!("runtime listener SDK runner unsupported capability: {capability}")
        }
        im_core::ImError::Service { message, .. } => {
            anyhow::anyhow!("runtime listener service error: {message}")
        }
        other => anyhow::anyhow!(other.to_string()),
    }
}

pub fn runner_host_label(host: ListenerRunHostKind) -> &'static str {
    match host {
        ListenerRunHostKind::Foreground => "runtime listener run",
        ListenerRunHostKind::Service => "runtime listener service-run",
    }
}

pub fn runtime_mode_supports_sdk_runner(resolved: &crate::workspace_config::Resolved) -> bool {
    host_runtime::resolve(resolved).mode == host_runtime::bridge::MODE_WEBSOCKET
}
