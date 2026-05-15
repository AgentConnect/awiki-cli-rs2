use super::service::{require_active_identity, CommandResult};
use super::types::{MessageError, SecureOutboxActionRequest, SecureStatusRequest};
use crate::config::Resolved;
use crate::identity::Manager;
use crate::store::{self, StoreError};
use serde_json::json;

pub fn secure_failed(
    resolved: &Resolved,
    manager: &Manager,
    request: SecureStatusRequest,
) -> Result<CommandResult, MessageError> {
    let record = require_active_identity(resolved, manager, &request.identity_name)?;
    let connection = open_secure_store(resolved)?;
    let rows = store::list_e2ee_outbox(&connection, &record.did, &record.identity_name, "failed")
        .map_err(store_error)?;
    let total = rows.len();
    Ok(CommandResult {
        data: json!({
            "failed": rows,
            "total": total,
        }),
        summary: format!("Loaded {total} failed secure outbox record(s)"),
        warnings: Vec::new(),
    })
}

pub fn secure_drop(
    resolved: &Resolved,
    manager: &Manager,
    request: SecureOutboxActionRequest,
) -> Result<CommandResult, MessageError> {
    let record = require_active_identity(resolved, manager, &request.identity_name)?;
    let connection = open_secure_store(resolved)?;
    store::get_e2ee_outbox(
        &connection,
        &request.outbox_id,
        &record.did,
        &record.identity_name,
    )
    .map_err(store_error)?;
    store::update_e2ee_outbox_status(
        &connection,
        &request.outbox_id,
        &record.did,
        &record.identity_name,
        "dropped",
    )
    .map_err(store_error)?;

    Ok(CommandResult {
        data: json!({
            "outbox_id": request.outbox_id,
            "status": "dropped",
        }),
        summary: format!("Dropped secure outbox record {}", request.outbox_id),
        warnings: Vec::new(),
    })
}

fn open_secure_store(resolved: &Resolved) -> Result<rusqlite::Connection, MessageError> {
    let connection = store::open(&resolved.paths).map_err(store_error)?;
    store::ensure_schema(&connection).map_err(store_error)?;
    Ok(connection)
}

fn store_error(err: StoreError) -> MessageError {
    MessageError::Internal(err.to_string())
}
