//! Exact operation recovery. Only public request bodies/metadata are stored here;
//! MLS secrets remain in the protocol store and private keys in identity custody.
use rusqlite::{Connection, OptionalExtension, TransactionBehavior};
use serde_json::Value;

pub(crate) const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS group_operation_journal (
 owner_identity_id TEXT NOT NULL, local_device_id TEXT NOT NULL,
 operation_key TEXT NOT NULL, request_binding TEXT NOT NULL, intent_json TEXT NOT NULL,
 PRIMARY KEY(owner_identity_id, local_device_id, operation_key))";

pub(crate) fn freeze(
    client: &crate::core::ImClient,
    key: &str,
    binding: &str,
    candidate: Value,
) -> crate::ImResult<Value> {
    let device =
        match crate::internal::identity_device_state::active_authorization_for_client(client) {
            Ok(device) => Some(device),
            Err(crate::ImError::PermissionDenied) => None,
            Err(error) => return Err(error),
        };
    let mut connection = crate::internal::local_state::open_writable(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    )?;
    freeze_on(
        &mut connection,
        client.current_identity().id.as_str(),
        device
            .as_ref()
            .map(|d| d.protocol_device_id.as_str())
            .unwrap_or("legacy"),
        key,
        binding,
        candidate,
    )
}

/// Read only an existing intent owned by this exact identity and device.
pub(crate) fn load(client: &crate::core::ImClient, key: &str) -> crate::ImResult<Option<Value>> {
    let device = crate::internal::identity_device_state::active_authorization_for_client(client)?;
    let connection = crate::internal::local_state::open_writable(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    )?;
    let json: Option<String> = connection.query_row(
        "SELECT intent_json FROM group_operation_journal WHERE owner_identity_id=?1 AND local_device_id=?2 AND operation_key=?3",
        (client.current_identity().id.as_str(), device.protocol_device_id.as_str(), key),
        |row| row.get(0),
    ).optional().map_err(crate::internal::local_state::local_state_unavailable)?;
    json.map(|json| {
        serde_json::from_str(&json).map_err(|_| crate::ImError::LocalStateUnavailable {
            detail: "invalid group operation journal".to_owned(),
        })
    })
    .transpose()
}

fn freeze_on(
    connection: &mut Connection,
    owner: &str,
    device: &str,
    key: &str,
    binding: &str,
    candidate: Value,
) -> crate::ImResult<Value> {
    let error = crate::internal::local_state::local_state_unavailable;
    let tx = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(error)?;
    let stored: Option<(String, String)> = tx.query_row(
        "SELECT request_binding, intent_json FROM group_operation_journal WHERE owner_identity_id=?1 AND local_device_id=?2 AND operation_key=?3",
        (owner,device,key), |row| Ok((row.get(0)?,row.get(1)?))).optional().map_err(error)?;
    let value = if let Some((previous_binding, json)) = stored {
        if previous_binding != binding {
            return Err(crate::ImError::invalid_input(
                Some("group_operation".to_owned()),
                "a resumed group operation cannot change its original request",
            ));
        }
        serde_json::from_str(&json).map_err(|_| crate::ImError::LocalStateUnavailable {
            detail: "invalid group operation journal".to_owned(),
        })?
    } else {
        tx.execute(
            "INSERT INTO group_operation_journal VALUES (?1,?2,?3,?4,?5)",
            (owner, device, key, binding, candidate.to_string()),
        )
        .map_err(error)?;
        candidate
    };
    tx.commit().map_err(error)?;
    Ok(value)
}

pub(crate) fn freeze_create_payload(
    client: &crate::core::ImClient,
    payload: crate::internal::wire::direct::DirectPayload,
) -> crate::ImResult<crate::internal::wire::direct::DirectPayload> {
    let Some(slug) = payload
        .body
        .pointer("/group_profile/slug")
        .and_then(Value::as_str)
    else {
        return Ok(payload);
    };
    let value = freeze(
        client,
        &format!("create:{slug}"),
        &payload.body.to_string(),
        serde_json::json!({"meta": payload.meta, "body": payload.body}),
    )?;
    Ok(crate::internal::wire::direct::DirectPayload {
        method: payload.method,
        meta: value["meta"].clone(),
        body: value["body"].clone(),
    })
}

#[cfg(test)]
#[path = "journal_tests.rs"]
mod tests;
