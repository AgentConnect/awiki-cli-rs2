use super::listener_secure_notifications::{
    is_secure_direct_wire_content_type, secure_notification_from_message_view,
};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplayStoreLookup {
    Exists,
    Missing,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecureReplayCandidate {
    pub message_id: String,
    pub owner_did: String,
    pub credential_name: String,
    pub notification: Value,
}

pub fn secure_unread_replay_candidates(
    messages: &[Value],
    session_record_did: &str,
    session_identity_name: &str,
    mut lookup: impl FnMut(&str, &str, &str) -> ReplayStoreLookup,
) -> Vec<SecureReplayCandidate> {
    secure_replay_candidates(
        messages,
        session_record_did,
        session_identity_name,
        false,
        &mut lookup,
    )
}

pub fn secure_pending_history_replay_candidates(
    messages: &[Value],
    session_record_did: &str,
    session_identity_name: &str,
    mut lookup: impl FnMut(&str, &str, &str) -> ReplayStoreLookup,
) -> Vec<SecureReplayCandidate> {
    secure_replay_candidates(
        messages,
        session_record_did,
        session_identity_name,
        true,
        &mut lookup,
    )
}

fn secure_replay_candidates(
    messages: &[Value],
    session_record_did: &str,
    session_identity_name: &str,
    skip_self_sent: bool,
    lookup: &mut impl FnMut(&str, &str, &str) -> ReplayStoreLookup,
) -> Vec<SecureReplayCandidate> {
    let mut candidates = Vec::new();
    for message in messages {
        let Some(view) = message.as_object() else {
            continue;
        };
        if !is_secure_direct_wire_content_type(&string_from_view(view, "content_type")) {
            continue;
        }
        if skip_self_sent && string_from_view(view, "sender_did") == session_record_did {
            continue;
        }
        let owner_did =
            fallback_owner_did(&string_from_view(view, "receiver_did"), session_record_did);
        let message_id = string_from_view(view, "id");
        match lookup(&message_id, &owner_did, session_identity_name) {
            ReplayStoreLookup::Exists | ReplayStoreLookup::Error => continue,
            ReplayStoreLookup::Missing => {}
        }
        let Ok(notification) = secure_notification_from_message_view(message) else {
            continue;
        };
        candidates.push(SecureReplayCandidate {
            message_id,
            owner_did,
            credential_name: session_identity_name.to_string(),
            notification,
        });
    }
    candidates
}

fn fallback_owner_did(receiver_did: &str, session_record_did: &str) -> String {
    if receiver_did.is_empty() {
        session_record_did.to_string()
    } else {
        receiver_did.to_string()
    }
}

fn string_from_view(view: &serde_json::Map<String, Value>, key: &str) -> String {
    match view.get(key) {
        Some(Value::String(value)) => value.clone(),
        _ => String::new(),
    }
}
