use super::{StoreError, StoreResult};
use rusqlite::Connection;
use serde_json::Value;

pub type ContactRecord = im_core::compat::directory::ContactRecord;

pub fn get_contact_by_did(
    connection: &Connection,
    owner_did: &str,
    did: &str,
) -> StoreResult<Value> {
    im_core::compat::directory::get_contact_by_did(connection, owner_did, did)
        .map_err(store_error_from_im_core)
}

pub fn get_current_contact_by_handle(
    connection: &Connection,
    owner_did: &str,
    handle: &str,
) -> StoreResult<Value> {
    im_core::compat::directory::get_current_contact_by_handle(connection, owner_did, handle)
        .map_err(store_error_from_im_core)
}

pub fn resolve_contact_handle_by_did(
    connection: &Connection,
    owner_did: &str,
    did: &str,
) -> StoreResult<String> {
    im_core::compat::directory::resolve_contact_handle_by_did(connection, owner_did, did)
        .map_err(store_error_from_im_core)
}

pub fn list_dids_by_handle(
    connection: &Connection,
    owner_did: &str,
    handle: &str,
) -> StoreResult<Vec<String>> {
    im_core::compat::directory::list_dids_by_handle(connection, owner_did, handle)
        .map_err(store_error_from_im_core)
}

pub fn list_contact_handle_history(
    connection: &Connection,
    handle: &str,
) -> StoreResult<Vec<Value>> {
    im_core::compat::directory::list_contact_handle_history(connection, handle)
        .map_err(store_error_from_im_core)
}

pub fn upsert_contact(connection: &mut Connection, record: ContactRecord) -> StoreResult<()> {
    im_core::compat::directory::upsert_contact(connection, record).map_err(store_error_from_im_core)
}

fn store_error_from_im_core(err: im_core::ImError) -> StoreError {
    match err {
        im_core::ImError::InvalidInput { message, .. } => StoreError::Invalid(message),
        im_core::ImError::PeerNotFound { peer } => StoreError::NotFound(peer),
        im_core::ImError::LocalStateUnavailable { detail } => StoreError::Invalid(detail),
        other => StoreError::Invalid(other.to_string()),
    }
}
