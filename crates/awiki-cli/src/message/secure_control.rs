use super::types::MessageError;
use crate::anpsdk::FileSessionStore;
use crate::config::Resolved;
use crate::identity::{types::StoredIdentity, Manager};
use crate::store::{self, E2EEOutboxRecord};
use serde_json::json;
use serde_json::{Map, Value};
use std::path::Path;

pub use im_core::compat::secure::{SECURE_ACK_SYSTEM_TYPE, SECURE_INIT_SYSTEM_TYPE};
pub const SECURE_SESSION_DIR_NAME: &str = "p5-e2ee-sessions";

pub fn build_secure_ack_payload(session_id: &str, acked_message_id: &str) -> Map<String, Value> {
    im_core::compat::secure::build_secure_ack_payload(session_id, acked_message_id)
}

pub fn build_secure_init_payload() -> Map<String, Value> {
    im_core::compat::secure::build_secure_init_payload()
}

pub fn is_secure_ack_plaintext(plaintext: &Map<String, Value>) -> bool {
    im_core::compat::secure::is_secure_ack_plaintext(plaintext)
}

pub fn is_secure_init_plaintext(plaintext: &Map<String, Value>) -> bool {
    im_core::compat::secure::is_secure_init_plaintext(plaintext)
}

pub fn secure_ack_session_id(plaintext: &Map<String, Value>) -> String {
    im_core::compat::secure::secure_ack_session_id(plaintext)
}

pub fn is_pending_confirmation_error(message: Option<&str>) -> bool {
    im_core::compat::secure::is_pending_confirmation_error(message)
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
