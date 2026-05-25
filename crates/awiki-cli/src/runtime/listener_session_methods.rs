use crate::legacy_identity::types::StoredIdentity;

#[derive(Debug, Clone, Default)]
pub struct ListenerSessionMethods {
    identity_name: String,
    record: Option<StoredIdentity>,
    client_id: Option<String>,
    secure_rpc_override_available: bool,
    connected: bool,
    last_error: String,
    initial_signaled: bool,
}

#[derive(Debug, Clone)]
pub struct SessionMethodsSnapshot {
    pub record: Option<StoredIdentity>,
    pub connected: bool,
    pub last_error: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SecureRpcSource {
    Override,
    Client(String),
    None,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionMethodAction {
    CloseClient { client_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionDisconnectReason {
    ContextCanceled,
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitialSessionResult {
    pub error: Option<String>,
    pub channel_closed: bool,
}

impl ListenerSessionMethods {
    pub fn new(identity_name: impl Into<String>) -> Self {
        Self {
            identity_name: identity_name.into(),
            ..Self::default()
        }
    }

    pub fn identity_name(&self) -> &str {
        &self.identity_name
    }

    pub fn current_client(&self) -> Option<&str> {
        self.client_id.as_deref()
    }

    pub fn current_record(&self) -> Option<&StoredIdentity> {
        self.record.as_ref()
    }

    pub fn snapshot(&self) -> SessionMethodsSnapshot {
        SessionMethodsSnapshot {
            record: self.record.clone(),
            connected: self.connected,
            last_error: self.last_error.clone(),
        }
    }

    pub fn secure_rpc_source(&self) -> SecureRpcSource {
        if self.secure_rpc_override_available {
            SecureRpcSource::Override
        } else if let Some(client_id) = self.client_id.as_ref() {
            SecureRpcSource::Client(client_id.clone())
        } else {
            SecureRpcSource::None
        }
    }

    pub fn set_secure_rpc_override_available(&mut self, available: bool) {
        self.secure_rpc_override_available = available;
    }

    pub fn mark_connected(
        &mut self,
        record: Option<StoredIdentity>,
        client_id: Option<String>,
    ) -> Vec<SessionMethodAction> {
        let actions =
            close_replaced_client_actions(self.client_id.as_deref(), client_id.as_deref());
        self.record = record;
        self.client_id = client_id;
        self.connected = true;
        self.last_error.clear();
        actions
    }

    pub fn mark_disconnected(
        &mut self,
        err: Option<SessionDisconnectReason>,
    ) -> Vec<SessionMethodAction> {
        let actions = close_current_client_actions(self.client_id.take());
        self.connected = false;
        if let Some(SessionDisconnectReason::Other(error)) = err {
            self.last_error = error;
        }
        actions
    }

    pub fn close_current_client(&mut self) -> Vec<SessionMethodAction> {
        let actions = close_current_client_actions(self.client_id.take());
        self.connected = false;
        actions
    }

    pub fn signal_initial(&mut self, error: Option<String>) -> Option<InitialSessionResult> {
        if self.initial_signaled {
            return None;
        }
        self.initial_signaled = true;
        Some(InitialSessionResult {
            error,
            channel_closed: true,
        })
    }

    pub fn initial_signaled(&self) -> bool {
        self.initial_signaled
    }
}

fn close_replaced_client_actions(
    current_client_id: Option<&str>,
    next_client_id: Option<&str>,
) -> Vec<SessionMethodAction> {
    let Some(current_client_id) = current_client_id else {
        return Vec::new();
    };
    if Some(current_client_id) == next_client_id {
        Vec::new()
    } else {
        vec![SessionMethodAction::CloseClient {
            client_id: current_client_id.to_string(),
        }]
    }
}

fn close_current_client_actions(client_id: Option<String>) -> Vec<SessionMethodAction> {
    client_id
        .map(|client_id| vec![SessionMethodAction::CloseClient { client_id }])
        .unwrap_or_default()
}
