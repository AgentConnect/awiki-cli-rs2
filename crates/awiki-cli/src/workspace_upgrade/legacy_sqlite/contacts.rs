use rusqlite::Connection;
use serde_json::Value;

pub fn list_contact_handle_history(
    connection: &Connection,
    handle: &str,
) -> super::StoreResult<Vec<Value>> {
    im_core::compat::directory::list_contact_handle_history(connection, handle)
        .map_err(store_error_from_im_core)
}

fn store_error_from_im_core(err: im_core::ImError) -> super::StoreError {
    match err {
        im_core::ImError::InvalidInput { message, .. } => super::StoreError::Invalid(message),
        im_core::ImError::PeerNotFound { peer } => super::StoreError::NotFound(peer),
        im_core::ImError::LocalStateUnavailable { detail } => super::StoreError::Invalid(detail),
        other => super::StoreError::Invalid(other.to_string()),
    }
}
