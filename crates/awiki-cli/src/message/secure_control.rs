use super::types::MessageError;
use crate::anpsdk::FileSessionStore;
use crate::config::Resolved;
use crate::identity::{types::StoredIdentity, Manager};
use crate::store::{self, E2EEOutboxRecord};
use serde_json::json;
use serde_json::{Map, Value};
use std::path::Path;

pub const SECURE_ACK_SYSTEM_TYPE: &str = "awiki.direct.secure_ack.v1";
pub const SECURE_INIT_SYSTEM_TYPE: &str = "awiki.direct.secure_init.v1";
pub const SECURE_SESSION_DIR_NAME: &str = "p5-e2ee-sessions";

pub fn build_secure_ack_payload(session_id: &str, acked_message_id: &str) -> Map<String, Value> {
    Map::from_iter([
        (
            "system_type".to_string(),
            Value::String(SECURE_ACK_SYSTEM_TYPE.to_string()),
        ),
        (
            "session_id".to_string(),
            Value::String(session_id.trim().to_string()),
        ),
        (
            "acked_message_id".to_string(),
            Value::String(acked_message_id.trim().to_string()),
        ),
    ])
}

pub fn build_secure_init_payload() -> Map<String, Value> {
    Map::from_iter([
        (
            "system_type".to_string(),
            Value::String(SECURE_INIT_SYSTEM_TYPE.to_string()),
        ),
        (
            "reason".to_string(),
            Value::String("manual_init".to_string()),
        ),
    ])
}

pub fn is_secure_ack_plaintext(plaintext: &Map<String, Value>) -> bool {
    is_secure_control_plaintext(plaintext, SECURE_ACK_SYSTEM_TYPE)
}

pub fn is_secure_init_plaintext(plaintext: &Map<String, Value>) -> bool {
    is_secure_control_plaintext(plaintext, SECURE_INIT_SYSTEM_TYPE)
}

pub fn secure_ack_session_id(plaintext: &Map<String, Value>) -> String {
    let Some(payload) = map_from_value(plaintext.get("payload")) else {
        return String::new();
    };
    string_from_value(payload.get("session_id"))
}

pub fn is_pending_confirmation_error(message: Option<&str>) -> bool {
    let Some(message) = message else {
        return false;
    };
    let message = message.to_ascii_lowercase();
    message.contains("pending confirmation") || message.contains("pending-confirmation")
}

pub fn queue_secure_outbox_record(
    resolved: &Resolved,
    manager: &Manager,
    record: Option<&StoredIdentity>,
    peer_did: &str,
    original_type: &str,
    plaintext: &str,
) -> Result<String, MessageError> {
    let record =
        record.ok_or_else(|| MessageError::Internal("identity record is required".to_string()))?;
    let connection = store::open(&resolved.paths).map_err(store_error)?;
    store::ensure_schema(&connection).map_err(store_error)?;
    store::queue_e2ee_outbox(
        &connection,
        E2EEOutboxRecord {
            owner_did: record.did.clone(),
            peer_did: peer_did.to_string(),
            session_id: current_secure_session_id(Some(manager), Some(record), peer_did),
            original_type: default_string(original_type, "text"),
            plaintext: plaintext.to_string(),
            local_status: "queued".to_string(),
            credential_name: record.identity_name.clone(),
            metadata: metadata_string(json!({"reason": "pending_confirmation"})),
            ..E2EEOutboxRecord::default()
        },
    )
    .map_err(store_error)
}

pub fn current_secure_session_id(
    manager: Option<&Manager>,
    record: Option<&StoredIdentity>,
    peer_did: &str,
) -> String {
    let (Some(manager), Some(record)) = (manager, record) else {
        return String::new();
    };
    let Ok(paths) = manager.paths_for_identity(&record.identity_name) else {
        return String::new();
    };
    let root = Path::new(&paths.identity_dir).join(SECURE_SESSION_DIR_NAME);
    let Ok(store) = FileSessionStore::new(root) else {
        return String::new();
    };
    let Ok(Some(session)) = store.find_by_peer_did(peer_did) else {
        return String::new();
    };
    session.session_id.trim().to_string()
}

fn is_secure_control_plaintext(plaintext: &Map<String, Value>, system_type: &str) -> bool {
    if string_from_value(plaintext.get("application_content_type")) != "application/json" {
        return false;
    }
    let Some(payload) = map_from_value(plaintext.get("payload")) else {
        return false;
    };
    string_from_value(payload.get("system_type")) == system_type
}

fn map_from_value(value: Option<&Value>) -> Option<Map<String, Value>> {
    match value {
        Some(Value::Object(object)) => Some(object.clone()),
        Some(Value::String(value)) if !value.trim().is_empty() => {
            serde_json::from_str::<Map<String, Value>>(value).ok()
        }
        _ => None,
    }
}

fn default_string(value: &str, fallback: &str) -> String {
    if value.trim().is_empty() {
        fallback.to_string()
    } else {
        value.to_string()
    }
}

fn metadata_string(value: Value) -> String {
    serde_json::to_string(&value).unwrap_or_default()
}

fn store_error(err: store::StoreError) -> MessageError {
    MessageError::Internal(err.to_string())
}

fn string_from_value(value: Option<&Value>) -> String {
    value
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}
