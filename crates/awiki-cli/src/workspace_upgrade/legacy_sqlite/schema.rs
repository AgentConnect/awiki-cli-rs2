use super::{StoreError, StoreResult};
use rusqlite::Connection;

pub fn ensure_schema(connection: &Connection) -> StoreResult<()> {
    im_core::compat::local_state::ensure_schema(connection).map_err(store_error_from_im_core)
}

pub fn current_schema_version(connection: &Connection) -> StoreResult<i64> {
    im_core::compat::local_state::current_schema_version(connection)
        .map_err(store_error_from_im_core)
}

fn store_error_from_im_core(err: im_core::ImError) -> StoreError {
    match err {
        im_core::ImError::LocalStateUnavailable { detail } => StoreError::Invalid(detail),
        other => StoreError::Invalid(other.to_string()),
    }
}
