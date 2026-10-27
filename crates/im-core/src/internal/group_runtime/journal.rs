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
    freeze_with(client, key, binding, || Ok(candidate))
}

/// Prepare only after excluding another process's durable intent. The producer
/// must not submit anything remotely: the journal commit precedes submission.
pub(crate) fn freeze_with(
    client: &crate::core::ImClient,
    key: &str,
    binding: &str,
    candidate: impl FnOnce() -> crate::ImResult<Value>,
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
    freeze_on_with(
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

pub(crate) fn save_on(
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

/// Merge a new local progress signal under the SQLite writer lock. Unlike a
/// completion of remote work, this update has no earlier observation to fence.
/// The callback must do local, synchronous work only.
pub(crate) fn update_progress_with<T>(
    client: &crate::core::ImClient,
    key: &str,
    update: impl FnOnce(Option<Value>) -> crate::ImResult<(Value, T)>,
) -> crate::ImResult<T> {
    let device = device_key(client)?;
    let mut connection = crate::internal::local_state::open_writable(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    )?;
    update_progress_on_with(
        &mut connection,
        client.current_identity().id.as_str(),
        &device,
        key,
        update,
    )
}

fn update_progress_on_with<T>(
    connection: &mut Connection,
    owner: &str,
    device: &str,
    key: &str,
    update: impl FnOnce(Option<Value>) -> crate::ImResult<(Value, T)>,
) -> crate::ImResult<T> {
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    let raw: Option<String> = transaction.query_row(
        "SELECT intent_json FROM group_operation_journal WHERE owner_identity_id=?1 AND local_device_id=?2 AND operation_key=?3",
        (owner, device, key), |row| row.get(0),
    ).optional().map_err(crate::internal::local_state::local_state_unavailable)?;
    let previous = raw
        .map(|raw| serde_json::from_str(&raw))
        .transpose()
        .map_err(|_| crate::ImError::LocalStateUnavailable {
            detail: "invalid group recovery progress".to_owned(),
        })?;
    let (value, result) = update(previous)?;
    save_on(&transaction, owner, device, key, value)?;
    transaction
        .commit()
        .map_err(crate::internal::local_state::local_state_unavailable)?;
    Ok(result)
}

/// Replace only the progress observed before doing asynchronous work. A newer
/// change must survive completion, retry accounting, and failure of older work.
pub(crate) fn replace_if_unchanged(
    client: &crate::core::ImClient,
    key: &str,
    expected: &Value,
    replacement: Value,
) -> crate::ImResult<bool> {
    let device = device_key(client)?;
    let connection = crate::internal::local_state::open_writable(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    )?;
    replace_if_unchanged_on(
        &connection,
        client.current_identity().id.as_str(),
        &device,
        key,
        expected,
        replacement,
    )
}

fn replace_if_unchanged_on(
    connection: &Connection,
    owner: &str,
    device: &str,
    key: &str,
    expected: &Value,
    replacement: Value,
) -> crate::ImResult<bool> {
    let changed = connection.execute(
        "UPDATE group_operation_journal SET intent_json=?4 WHERE owner_identity_id=?1 AND local_device_id=?2 AND operation_key=?3 AND intent_json=?5",
        (owner, device, key, replacement.to_string(), expected.to_string()),
    ).map_err(crate::internal::local_state::local_state_unavailable)?;
    Ok(changed == 1)
}

/// Schedule maintenance in the same transaction as an authenticated ordinary
/// group-state projection. This is a trigger, never permission to mutate MLS:
/// the existing repair executor still checks the current owner and device.
#[cfg(feature = "group-e2ee")]
pub(crate) fn schedule_group_state_change_on(
    connection: &Connection,
    owner: &str,
    device: &str,
    group: &str,
    change_id: &str,
) -> crate::ImResult<bool> {
    let eligible: Option<(Option<String>, String, Option<String>)> = connection.query_row(
        "SELECT my_role,membership_status,metadata FROM groups WHERE owner_identity_id=?1 AND group_id=?2",
        (owner, group),
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    ).optional().map_err(crate::internal::local_state::local_state_unavailable)?;
    let Some((role, membership, metadata)) = eligible else {
        return Ok(false);
    };
    if role.as_deref() != Some("owner") || membership != "active" {
        return Ok(false);
    }
    let Some(metadata) = metadata else {
        return Ok(false);
    };
    let metadata: Value =
        serde_json::from_str(&metadata).map_err(|_| crate::ImError::LocalStateUnavailable {
            detail: "invalid group security projection".to_owned(),
        })?;
    let profiles = [
        metadata.pointer("/group_policy/message_security_profile"),
        metadata.get("message_security_profile"),
        metadata.get("required_security_profile"),
    ];
    // Unknown policy is not authorization to prepare a plain group as MLS.
    let mut encrypted = false;
    for profile in profiles
        .into_iter()
        .flatten()
        .filter(|value| !value.is_null())
    {
        if profile.as_str() != Some("group-e2ee") {
            return Ok(false);
        }
        encrypted = true;
    }
    if !encrypted {
        return Ok(false);
    }
    schedule_preparation_on(connection, owner, device, group, change_id)
}

/// Authenticated dependency observations can trigger preparation even before a
/// complete local group projection exists. The executor checks authoritative
/// policy and role; this trigger grants no MLS authority.
#[cfg(feature = "group-e2ee")]
pub(crate) fn schedule_preparation_on(
    connection: &Connection,
    owner: &str,
    device: &str,
    group: &str,
    change_id: &str,
) -> crate::ImResult<bool> {
    let key = format!("recovery:repair:{group}");
    let previous: Option<String> = connection.query_row(
        "SELECT intent_json FROM group_operation_journal WHERE owner_identity_id=?1 AND local_device_id=?2 AND operation_key=?3",
        (owner, device, &key), |row| row.get(0),
    ).optional().map_err(crate::internal::local_state::local_state_unavailable)?;
    let mut value = previous
        .map(|raw| serde_json::from_str::<Value>(&raw))
        .transpose()
        .map_err(|_| crate::ImError::LocalStateUnavailable {
            detail: "invalid group recovery progress".to_owned(),
        })?
        .unwrap_or_else(|| serde_json::json!({}));
    if !value.is_object() {
        return Err(crate::ImError::LocalStateUnavailable {
            detail: "invalid group recovery progress".to_owned(),
        });
    }
    if value["last_change_id"].as_str() == Some(change_id) {
        return Ok(false);
    }
    let generation = value["change_generation"]
        .as_u64()
        .unwrap_or(0)
        .checked_add(1)
        .ok_or_else(|| crate::ImError::LocalStateUnavailable {
            detail: "group recovery generation exhausted".to_owned(),
        })?;
    value["phase"] = serde_json::json!("preparing");
    value["group_did"] = serde_json::json!(group);
    value["last_change_id"] = serde_json::json!(change_id);
    value["change_generation"] = serde_json::json!(generation);
    value["attempts"] = serde_json::json!(0);
    value["next_attempt_at"] = serde_json::json!(0);
    value["status"] = serde_json::json!({
        "operation_id": key, "group_did": group, "phase": "preparing",
        "business_committed": true, "encryption_ready": false,
        "reason": "encryption_preparation_pending", "next_action": "wait",
    });
    save_on(connection, owner, device, &key, value)?;
    Ok(true)
}

/// Enqueue Welcome replenishment in the same transaction as sync completion.
/// Replayed notices must not reopen an already completed package publication.
pub(crate) fn schedule_key_package_replenishment_on(
    connection: &Connection,
    owner: &str,
    device: &str,
    notice_id: &str,
) -> crate::ImResult<()> {
    let key = format!("key-package-replenish:{notice_id}");
    let inserted = connection.execute(
        "INSERT INTO group_operation_journal VALUES (?1,?2,?3,?3,?4) ON CONFLICT(owner_identity_id,local_device_id,operation_key) DO NOTHING",
        (owner, device, &key, serde_json::json!({"notice_id": notice_id, "completed": false}).to_string()),
    ).map_err(crate::internal::local_state::local_state_unavailable)?;
    if inserted != 0 {
        save_on(
            connection,
            owner,
            device,
            "device-preparation-status",
            serde_json::json!({"next_attempt_at": 0}),
        )?;
    }
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
    freeze_on_with(connection, owner, device, key, binding, || Ok(candidate))
}

fn freeze_on_with(
    connection: &mut Connection,
    owner: &str,
    device: &str,
    key: &str,
    binding: &str,
    candidate: impl FnOnce() -> crate::ImResult<Value>,
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
        let candidate = candidate()?;
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
