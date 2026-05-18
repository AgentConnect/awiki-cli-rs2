use super::host_notify_sink::HostNotifySinkImpl;
use super::listener_session_bootstrap::SESSION_BOOTSTRAP_TIMEOUT;
use std::sync::{mpsc, Arc};
use std::time::Duration;

pub fn host_notify_for_bridge_created_session(
    supervisor_host_notify: &Arc<HostNotifySinkImpl>,
) -> Arc<HostNotifySinkImpl> {
    Arc::clone(supervisor_host_notify)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BridgeSessionBootstrapResult {
    Connected,
    InitialError(String),
    Timeout(String),
}

#[derive(Debug)]
pub struct BridgeSessionBootstrapSignal {
    sender: Option<mpsc::SyncSender<Result<(), String>>>,
}

impl BridgeSessionBootstrapSignal {
    pub fn signal_success(mut self) {
        if let Some(sender) = self.sender.take() {
            let _ = sender.send(Ok(()));
        }
    }

    pub fn signal_error(mut self, error: impl Into<String>) {
        if let Some(sender) = self.sender.take() {
            let _ = sender.send(Err(error.into()));
        }
    }
}

pub struct BridgeSessionBootstrapWaiter<'a> {
    identity_name: &'a str,
    receiver: mpsc::Receiver<Result<(), String>>,
}

pub fn bridge_session_bootstrap_signal_pair(
    identity_name: &str,
) -> (
    BridgeSessionBootstrapSignal,
    BridgeSessionBootstrapWaiter<'_>,
) {
    let (sender, receiver) = mpsc::sync_channel(1);
    (
        BridgeSessionBootstrapSignal {
            sender: Some(sender),
        },
        BridgeSessionBootstrapWaiter {
            identity_name,
            receiver,
        },
    )
}

pub fn wait_for_bridge_session_bootstrap(
    waiter: BridgeSessionBootstrapWaiter<'_>,
) -> BridgeSessionBootstrapResult {
    wait_for_bridge_session_bootstrap_with_timeout(waiter, SESSION_BOOTSTRAP_TIMEOUT)
}

pub fn wait_for_bridge_session_bootstrap_with_timeout(
    waiter: BridgeSessionBootstrapWaiter<'_>,
    timeout: Duration,
) -> BridgeSessionBootstrapResult {
    match waiter.receiver.recv_timeout(timeout) {
        Ok(Ok(())) => BridgeSessionBootstrapResult::Connected,
        Ok(Err(error)) => BridgeSessionBootstrapResult::InitialError(error),
        Err(mpsc::RecvTimeoutError::Timeout) | Err(mpsc::RecvTimeoutError::Disconnected) => {
            BridgeSessionBootstrapResult::Timeout(bridge_session_bootstrap_timeout_error(
                waiter.identity_name,
            ))
        }
    }
}

pub fn bridge_session_bootstrap_timeout_error(identity_name: &str) -> String {
    format!("websocket session bootstrap timed out for identity {identity_name}")
}
