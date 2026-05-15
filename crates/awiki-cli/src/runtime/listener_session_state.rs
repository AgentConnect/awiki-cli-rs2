use super::listener::SessionStatus;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Default, Eq, PartialEq)]
pub struct ListenerSessionState {
    sessions: BTreeMap<String, SessionEntry>,
    bridge_available: bool,
}

#[derive(Debug, Clone, Default, Eq, PartialEq)]
struct SessionEntry {
    did: Option<String>,
    connected: bool,
    last_error: String,
}

impl ListenerSessionState {
    pub fn mark_connected(&mut self, identity_name: impl Into<String>, did: impl Into<String>) {
        let entry = self.sessions.entry(identity_name.into()).or_default();
        entry.connected = true;
        entry.did = optional_nonempty(did.into());
        entry.last_error.clear();
    }

    pub fn mark_disconnected(&mut self, identity_name: impl Into<String>, error: Option<&str>) {
        let entry = self.sessions.entry(identity_name.into()).or_default();
        entry.connected = false;
        if let Some(error) = error.filter(|error| !error.is_empty()) {
            entry.last_error = error.to_string();
        }
    }

    pub fn record_session_error(
        &mut self,
        identity_name: impl Into<String>,
        did: impl Into<String>,
        error: impl Into<String>,
    ) {
        let entry = self
            .sessions
            .entry(identity_name.into())
            .or_insert_with(|| SessionEntry {
                did: optional_nonempty(did.into()),
                ..SessionEntry::default()
            });
        entry.connected = false;
        entry.last_error = error.into();
    }

    pub fn snapshot_sessions(&self) -> Vec<SessionStatus> {
        self.sessions
            .iter()
            .map(|(identity_name, entry)| SessionStatus {
                identity_name: identity_name.clone(),
                did: entry.did.clone().unwrap_or_default(),
                connected: entry.connected,
                last_error: entry.last_error.clone(),
            })
            .collect()
    }

    pub fn bridge_available(&self) -> bool {
        self.bridge_available
    }

    pub fn set_bridge_available(&mut self, bridge_available: bool) -> bool {
        let changed = self.bridge_available != bridge_available;
        self.bridge_available = bridge_available;
        changed
    }
}

fn optional_nonempty(value: String) -> Option<String> {
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}
