use super::listener::{SessionStatus, Status};
use super::listener_bridge_runtime::BridgeSessionBootstrapResult;
use super::listener_session_bootstrap::SESSION_BOOTSTRAP_TIMEOUT;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KnownSessionStartupDecision {
    Existing,
    StartAndWait {
        identity_name: String,
        timeout: Duration,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownSessionStartupError {
    pub identity_name: String,
    pub did: String,
    pub error: String,
}

pub fn known_session_startup_decision(
    identity_name: &str,
    already_known: bool,
) -> KnownSessionStartupDecision {
    if already_known {
        return KnownSessionStartupDecision::Existing;
    }
    KnownSessionStartupDecision::StartAndWait {
        identity_name: identity_name.to_string(),
        timeout: SESSION_BOOTSTRAP_TIMEOUT,
    }
}

pub fn known_session_startup_error(
    identity_name: &str,
    did: &str,
    result: BridgeSessionBootstrapResult,
) -> Option<KnownSessionStartupError> {
    let error = match result {
        BridgeSessionBootstrapResult::Connected => return None,
        BridgeSessionBootstrapResult::InitialError(error)
        | BridgeSessionBootstrapResult::Timeout(error) => error,
    };
    Some(KnownSessionStartupError {
        identity_name: identity_name.to_string(),
        did: did.to_string(),
        error,
    })
}

pub fn record_known_session_startup_error(status: &mut Status, error: KnownSessionStartupError) {
    if let Some(existing) = status
        .sessions
        .iter_mut()
        .find(|session| session.identity_name == error.identity_name)
    {
        if existing.did.is_empty() {
            existing.did = error.did;
        }
        existing.connected = false;
        existing.last_error = error.error;
        return;
    }
    status.sessions.push(SessionStatus {
        identity_name: error.identity_name,
        did: error.did,
        connected: false,
        last_error: error.error,
    });
    status
        .sessions
        .sort_by(|left, right| left.identity_name.cmp(&right.identity_name));
}
