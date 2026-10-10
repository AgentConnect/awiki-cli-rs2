//! Exact operation recovery. Only public request bodies/metadata are stored here;
//! MLS secrets remain in the protocol store and private keys in identity custody.
use rusqlite::{Connection, OptionalExtension, TransactionBehavior};
use serde_json::Value;

pub(crate) const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS group_operation_journal (
 owner_identity_id TEXT NOT NULL, local_device_id TEXT NOT NULL,
 operation_key TEXT NOT NULL, request_binding TEXT NOT NULL, intent_json TEXT NOT NULL,
 PRIMARY KEY(owner_identity_id, local_device_id, operation_key))";

fn device_key(client: &crate::core::ImClient) -> crate::ImResult<String> {
    match crate::internal::identity_device_state::active_authorization_for_client(client) {
        Ok(device) => Ok(device.protocol_device_id.as_str().to_owned()),
        Err(crate::ImError::PermissionDenied) => Ok("legacy".to_owned()),
        Err(error) => Err(error),
    }
}

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
    let device = device_key(client)?;
    let connection = crate::internal::local_state::open_writable(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    )?;
    let json: Option<String> = connection.query_row(
        "SELECT intent_json FROM group_operation_journal WHERE owner_identity_id=?1 AND local_device_id=?2 AND operation_key=?3",
        (client.current_identity().id.as_str(), device.as_str(), key),
        |row| row.get(0),
    ).optional().map_err(crate::internal::local_state::local_state_unavailable)?;
    json.map(|json| {
        serde_json::from_str(&json).map_err(|_| crate::ImError::LocalStateUnavailable {
            detail: "invalid group operation journal".to_owned(),
        })
    })
    .transpose()
}

/// Mutable progress is kept separately from frozen wire/request intent.
pub(crate) fn save(client: &crate::core::ImClient, key: &str, value: Value) -> crate::ImResult<()> {
    let device = device_key(client)?;
    let connection = crate::internal::local_state::open_writable(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    )?;
    save_on(
        &connection,
        client.current_identity().id.as_str(),
        &device,
        key,
        value,
    )
}

fn save_on(
    connection: &Connection,
    owner: &str,
    device: &str,
    key: &str,
    value: Value,
) -> crate::ImResult<()> {
    connection.execute(
        "INSERT INTO group_operation_journal VALUES (?1,?2,?3,?3,?4) ON CONFLICT(owner_identity_id,local_device_id,operation_key) DO UPDATE SET intent_json=excluded.intent_json",
        (owner, device, key, value.to_string()),
    ).map_err(crate::internal::local_state::local_state_unavailable)?;
    Ok(())
}

pub(crate) fn pending(client: &crate::core::ImClient) -> crate::ImResult<Vec<(String, Value)>> {
    entries(client, "recovery:%")
}

pub(crate) fn entries(
    client: &crate::core::ImClient,
    prefix: &str,
) -> crate::ImResult<Vec<(String, Value)>> {
    let device = device_key(client)?;
    let connection = crate::internal::local_state::open_writable(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    )?;
    let mut statement = connection.prepare("SELECT operation_key,intent_json FROM group_operation_journal WHERE owner_identity_id=?1 AND local_device_id=?2 AND operation_key LIKE ?3 ORDER BY operation_key").map_err(crate::internal::local_state::local_state_unavailable)?;
    let rows = statement
        .query_map(
            (
                client.current_identity().id.as_str(),
                device.as_str(),
                prefix,
            ),
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
        )
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    let mut values = Vec::new();
    for row in rows {
        let (key, value) = row.map_err(crate::internal::local_state::local_state_unavailable)?;
        let value: Value =
            serde_json::from_str(&value).map_err(|_| crate::ImError::LocalStateUnavailable {
                detail: "invalid group recovery progress".to_owned(),
            })?;
        if value["phase"] != "completed" {
            values.push((key, value));
        }
    }
    Ok(values)
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

pub(crate) fn freeze_member_payload(
    client: &crate::core::ImClient,
    payload: crate::internal::wire::direct::DirectPayload,
) -> crate::ImResult<crate::internal::wire::direct::DirectPayload> {
    let group = payload
        .meta
        .pointer("/target/did")
        .and_then(Value::as_str)
        .unwrap_or("");
    let member = payload
        .body
        .get("member_did")
        .and_then(Value::as_str)
        .unwrap_or("");
    if !client.core_inner().group_e2ee_v2_enabled() {
        return Ok(payload);
    }
    if load(client, &format!("recovery:member:{group}:{member}"))?.is_none() {
        return Ok(payload);
    }
    let key = format!("member-wire:{group}:{member}");
    let previous = load(client, &key)?.filter(|v| !v.is_null());
    let value = match previous {
        Some(value) => {
            if value["body"] != payload.body {
                return Err(crate::ImError::invalid_input(
                    Some("group_operation".to_owned()),
                    "resume the original membership request",
                ));
            }
            value
        }
        None => {
            let value = serde_json::json!({ "meta": payload.meta, "body": payload.body });
            save(client, &key, value.clone())?;
            value
        }
    };
    Ok(crate::internal::wire::direct::DirectPayload {
        method: payload.method,
        meta: value["meta"].clone(),
        body: value["body"].clone(),
    })
}
