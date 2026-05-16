use super::service::{
    auth_session, publish_secure_prekeys_with_client, require_active_identity, resolve_target,
    CommandResult,
};
use super::types::{
    MessageError, SecureOutboxActionRequest, SecurePeerRequest, SecureStatusRequest,
    MESSAGE_RPC_ENDPOINT,
};
use super::{
    build_secure_init_payload, current_secure_session_id, flush_queued_secure_outbox_with_sender,
    new_secure_e2ee_client_for_record, Client, MessageServiceE2EEClient, SecureE2EERpc,
    SecureOutboxSendOutcome, SecureOutboxSendRequest,
};
use crate::config::Resolved;
use crate::identity::{types::StoredIdentity, Manager};
use crate::store::{self, StoreError};
use crate::transportcfg::Profile;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const SECURE_SESSION_DIR_NAME: &str = "p5-e2ee-sessions";

pub fn secure_status(
    resolved: &Resolved,
    manager: &Manager,
    request: SecureStatusRequest,
) -> Result<CommandResult, MessageError> {
    let record = require_active_identity(resolved, manager, &request.identity_name)?;
    let target = if request.with.trim().is_empty() {
        None
    } else {
        Some(resolve_target(resolved, &request.with)?)
    };
    let peer_did = target
        .as_ref()
        .map(|target| target.did.as_str())
        .unwrap_or("");
    let sessions = list_secure_sessions(manager, &record, peer_did)?;
    let connection = open_secure_store(resolved)?;
    let mut outbox_rows =
        store::list_e2ee_outbox(&connection, &record.did, &record.identity_name, "")
            .map_err(store_error)?;
    if !peer_did.is_empty() {
        outbox_rows.retain(|row| string_from_value(row.get("peer_did")) == peer_did);
    }
    let mut by_status: BTreeMap<String, usize> = BTreeMap::new();
    for row in &outbox_rows {
        let status = default_string(&string_from_value(row.get("local_status")), "unknown");
        *by_status.entry(status).or_default() += 1;
    }
    let session_total = sessions.len();
    let outbox_total = outbox_rows.len();
    let with = target
        .as_ref()
        .map(|target| peer_handle_or_did(&target.handle, &target.did))
        .unwrap_or_default();
    Ok(CommandResult {
        data: json!({
            "with": with,
            "sessions": sessions,
            "outbox": {
                "total": outbox_total,
                "by_status": by_status,
                "records": redact_secure_outbox_rows_for_status(&outbox_rows),
            },
        }),
        summary: format!(
            "Loaded {session_total} secure session(s) and {outbox_total} secure outbox record(s)"
        ),
        warnings: Vec::new(),
    })
}

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

pub fn secure_init(
    resolved: &Resolved,
    manager: &Manager,
    request: SecurePeerRequest,
) -> Result<CommandResult, MessageError> {
    let record = require_active_identity(resolved, manager, &request.identity_name)?;
    if record.e2ee_agreement_private_pem.is_empty() || record.key1_private_pem.is_empty() {
        return Err(MessageError::Internal(
            "secure direct messaging requires DID signing and X25519 E2EE private keys".to_string(),
        ));
    }
    if request.with.trim().is_empty() {
        return Err(MessageError::TargetRequired);
    }
    let target = resolve_target(resolved, &request.with)?;
    let warnings = publish_secure_prekeys(resolved, manager, &record);
    if let Some(session) = load_secure_session_state(manager, &record, &target.did)? {
        return Ok(CommandResult {
            data: json!({
                "target": {
                    "did": target.did,
                    "handle": target.handle,
                    "kind": "direct",
                },
                "session": session,
                "reused": true,
            }),
            summary: format!(
                "Secure session already exists for {}",
                peer_handle_or_did(&target.handle, &target.did)
            ),
            warnings: super::compact_warnings(warnings),
        });
    }

    let auth = auth_session(resolved, manager, &record)?;
    let rpc_client = Client::new(resolved)?;
    let mut client = new_secure_e2ee_client_for_record(
        Some(manager),
        Some(&record),
        secure_retry_rpc(rpc_client, auth),
    )
    .map_err(MessageError::Internal)?;
    let message_id = format!("secure-init-{}", super::wire::generate_operation_id());
    let result = client
        .send_json(
            &target.did,
            build_secure_init_payload(),
            &message_id,
            &message_id,
        )
        .map_err(MessageError::Internal)?;
    let session = load_secure_session_state(manager, &record, &target.did)
        .ok()
        .flatten()
        .unwrap_or(Value::Null);

    Ok(CommandResult {
        data: json!({
            "target": {
                "did": target.did,
                "handle": target.handle,
                "kind": "direct",
            },
            "session": session,
            "delivery": {
                "message_id": default_string(&string_from_value(result.get("message_id")), &message_id),
                "operation_id": default_string(&string_from_value(result.get("operation_id")), &message_id),
                "target_did": default_string(&string_from_value(result.get("target_did")), &target.did),
            },
            "initialized": true,
        }),
        summary: format!(
            "Initialized secure session with {}",
            peer_handle_or_did(&target.handle, &target.did)
        ),
        warnings: super::compact_warnings(warnings),
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

pub fn secure_retry_with_sender(
    resolved: &Resolved,
    manager: &Manager,
    request: SecureOutboxActionRequest,
    mut sender: impl FnMut(SecureOutboxSendRequest) -> SecureOutboxSendOutcome,
    mut current_session_id: impl FnMut(&str) -> String,
) -> Result<CommandResult, MessageError> {
    let record = require_active_identity(resolved, manager, &request.identity_name)?;
    let connection = open_secure_store(resolved)?;
    let row = store::get_e2ee_outbox(
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
        "queued",
    )
    .map_err(store_error)?;

    let peer_did = string_from_value(row.get("peer_did"));
    let warnings = flush_queued_secure_outbox_with_sender(
        &connection,
        &record.did,
        &record.identity_name,
        &peer_did,
        &mut sender,
        &mut current_session_id,
    );
    let record_data = store::get_e2ee_outbox(
        &connection,
        &request.outbox_id,
        &record.did,
        &record.identity_name,
    )
    .unwrap_or(Value::Null);

    Ok(CommandResult {
        data: json!({
            "outbox_id": request.outbox_id,
            "record": record_data,
        }),
        summary: format!("Retried secure outbox record {}", request.outbox_id),
        warnings,
    })
}

pub fn secure_retry(
    resolved: &Resolved,
    manager: &Manager,
    request: SecureOutboxActionRequest,
) -> Result<CommandResult, MessageError> {
    let record = require_active_identity(resolved, manager, &request.identity_name)?;
    let connection = open_secure_store(resolved)?;
    let row = store::get_e2ee_outbox(
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
        "queued",
    )
    .map_err(store_error)?;

    let peer_did = string_from_value(row.get("peer_did"));
    let warnings = match secure_retry_client(resolved, manager, &record) {
        Ok(mut client) => flush_queued_secure_outbox_with_sender(
            &connection,
            &record.did,
            &record.identity_name,
            &peer_did,
            |request| secure_retry_send(&mut client, request),
            |peer_did| current_secure_session_id(Some(manager), Some(&record), peer_did),
        ),
        Err(err) => super::compact_warnings(vec![err]),
    };
    let record_data = store::get_e2ee_outbox(
        &connection,
        &request.outbox_id,
        &record.did,
        &record.identity_name,
    )
    .unwrap_or(Value::Null);

    Ok(CommandResult {
        data: json!({
            "outbox_id": request.outbox_id,
            "record": record_data,
        }),
        summary: format!("Retried secure outbox record {}", request.outbox_id),
        warnings,
    })
}

fn publish_secure_prekeys(
    resolved: &Resolved,
    manager: &Manager,
    record: &StoredIdentity,
) -> Vec<String> {
    if record.e2ee_agreement_private_pem.is_empty() || record.key1_private_pem.is_empty() {
        return Vec::new();
    }
    let auth = match auth_session(resolved, manager, record) {
        Ok(auth) => auth,
        Err(err) => {
            return super::compact_warnings(vec![format!(
                "Failed to initialize secure prekey auth: {err}"
            )])
        }
    };
    let rpc: Box<SecureE2EERpc> = if resolved.service_base_url.trim().is_empty() {
        Box::new(|_, _| Err("message service url is required".to_string()))
    } else {
        let rpc_client = match Client::new(resolved) {
            Ok(client) => client,
            Err(err) => {
                return super::compact_warnings(vec![format!(
                    "Failed to initialize secure prekey publisher: {err}"
                )])
            }
        };
        secure_retry_rpc(rpc_client, auth)
    };
    let mut client = match new_secure_e2ee_client_for_record(Some(manager), Some(record), rpc) {
        Ok(client) => client,
        Err(err) => {
            return super::compact_warnings(vec![format!(
                "Failed to initialize secure prekey publisher: {err}"
            )])
        }
    };
    publish_secure_prekeys_with_client(&mut client)
}

fn secure_retry_client(
    resolved: &Resolved,
    manager: &Manager,
    record: &StoredIdentity,
) -> Result<MessageServiceE2EEClient, String> {
    let auth = auth_session(resolved, manager, record)
        .map_err(|err| format!("Failed to initialize secure outbox sender: {err}"))?;
    let rpc_client = Client::new(resolved)
        .map_err(|err| format!("Failed to initialize secure outbox sender: {err}"))?;
    new_secure_e2ee_client_for_record(
        Some(manager),
        Some(record),
        secure_retry_rpc(rpc_client, auth),
    )
    .map_err(|err| format!("Failed to initialize secure outbox sender: {err}"))
}

fn secure_retry_rpc(
    client: Client,
    mut auth: crate::authsdk::Session,
) -> Box<super::SecureE2EERpc> {
    Box::new(move |method, params| {
        client
            .authenticated_rpc_call_profile::<Map<String, Value>, _>(
                Profile::RpcDefault,
                MESSAGE_RPC_ENDPOINT,
                method,
                params,
                &mut auth,
            )
            .map_err(|err| err.to_string())
    })
}

fn secure_retry_send(
    client: &mut MessageServiceE2EEClient,
    request: SecureOutboxSendRequest,
) -> SecureOutboxSendOutcome {
    let result = match request.original_type.as_str() {
        "text" | "" => client.send_text(
            &request.target_did,
            &request.plaintext,
            &request.outbox_id,
            &request.outbox_id,
        ),
        "json" => client.send_json(
            &request.target_did,
            request.json_payload.unwrap_or_default(),
            &request.outbox_id,
            &request.outbox_id,
        ),
        _ => Err(format!(
            "unsupported original_type: {}",
            request.original_type
        )),
    };
    match result {
        Ok(result) => SecureOutboxSendOutcome::Success {
            message_id: string_from_value(result.get("message_id")),
            operation_id: string_from_value(result.get("operation_id")),
            delivery_state: string_from_value(result.get("delivery_state")),
            accepted_at: string_from_value(result.get("accepted_at")),
        },
        Err(err) => SecureOutboxSendOutcome::Error(err),
    }
}

fn open_secure_store(resolved: &Resolved) -> Result<rusqlite::Connection, MessageError> {
    let connection = store::open(&resolved.paths).map_err(store_error)?;
    store::ensure_schema(&connection).map_err(store_error)?;
    Ok(connection)
}

fn store_error(err: StoreError) -> MessageError {
    MessageError::Internal(err.to_string())
}

fn load_secure_session_state(
    manager: &Manager,
    record: &StoredIdentity,
    peer_did: &str,
) -> Result<Option<Value>, MessageError> {
    Ok(list_secure_sessions(manager, record, peer_did)?
        .into_iter()
        .next())
}

fn list_secure_sessions(
    manager: &Manager,
    record: &StoredIdentity,
    peer_did: &str,
) -> Result<Vec<Value>, MessageError> {
    let paths = manager.paths_for_identity(&record.identity_name)?;
    let root = Path::new(&paths.identity_dir).join(SECURE_SESSION_DIR_NAME);
    let mut entries = session_json_paths(&root)?;
    entries.sort();
    let mut sessions = Vec::new();
    for path in entries {
        let raw = std::fs::read(&path).map_err(internal_error)?;
        let session: Map<String, Value> = serde_json::from_slice(&raw).map_err(internal_error)?;
        let session = Value::Object(session);
        if !peer_did.is_empty() && string_from_value(session.get("peer_did")) != peer_did {
            continue;
        }
        sessions.push(redact_secure_session_for_status(&session));
    }
    sessions.sort_by(|left, right| {
        string_from_value(left.get("peer_did")).cmp(&string_from_value(right.get("peer_did")))
    });
    Ok(sessions)
}

fn session_json_paths(root: &Path) -> Result<Vec<PathBuf>, MessageError> {
    let entries = match std::fs::read_dir(root) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(internal_error(err)),
    };
    let mut paths = Vec::new();
    for entry in entries {
        let entry = entry.map_err(internal_error)?;
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) == Some("json") {
            paths.push(path);
        }
    }
    Ok(paths)
}

fn redact_secure_session_for_status(session: &Value) -> Value {
    json!({
        "session_id": string_from_value(session.get("session_id")),
        "suite": string_from_value(session.get("suite")),
        "peer_did": string_from_value(session.get("peer_did")),
        "status": string_from_value(session.get("status")),
        "is_initiator": bool_from_value(session.get("is_initiator")),
        "send_n": int_from_value(session.get("send_n"), 0),
        "recv_n": int_from_value(session.get("recv_n"), 0),
        "previous_send_chain_length": int_from_value(
            session.get("previous_send_chain_length"),
            0,
        ),
        "skipped_key_count": count_status_array_items(session.get("skipped_message_keys")),
    })
}

fn count_status_array_items(value: Option<&Value>) -> usize {
    value
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or_default()
}

fn redact_secure_outbox_rows_for_status(rows: &[Value]) -> Vec<Value> {
    rows.iter()
        .map(|row| {
            let mut redacted = Map::new();
            insert_string(&mut redacted, "outbox_id", row);
            insert_string(&mut redacted, "peer_did", row);
            insert_string(&mut redacted, "session_id", row);
            insert_string(&mut redacted, "original_type", row);
            insert_string(&mut redacted, "local_status", row);
            redacted.insert(
                "attempt_count".to_string(),
                json!(int_from_value(row.get("attempt_count"), 0)),
            );
            insert_string(&mut redacted, "sent_msg_id", row);
            redacted.insert(
                "sent_server_seq".to_string(),
                row.get("sent_server_seq").cloned().unwrap_or(Value::Null),
            );
            insert_string(&mut redacted, "last_error_code", row);
            insert_string(&mut redacted, "retry_hint", row);
            insert_string(&mut redacted, "failed_msg_id", row);
            redacted.insert(
                "failed_server_seq".to_string(),
                row.get("failed_server_seq").cloned().unwrap_or(Value::Null),
            );
            insert_string(&mut redacted, "last_attempt_at", row);
            insert_string(&mut redacted, "created_at", row);
            insert_string(&mut redacted, "updated_at", row);
            Value::Object(redacted)
        })
        .collect()
}

fn insert_string(object: &mut Map<String, Value>, field: &str, source: &Value) {
    object.insert(
        field.to_string(),
        Value::String(string_from_value(source.get(field))),
    );
}

fn peer_handle_or_did(handle: &str, did: &str) -> String {
    if handle.is_empty() {
        did.to_string()
    } else {
        handle.to_string()
    }
}

fn string_from_value(value: Option<&Value>) -> String {
    value
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn int_from_value(value: Option<&Value>, fallback: i64) -> i64 {
    match value {
        Some(Value::Number(number)) => number
            .as_i64()
            .or_else(|| number.as_u64().and_then(|value| i64::try_from(value).ok()))
            .or_else(|| number.as_f64().map(|value| value as i64))
            .unwrap_or(fallback),
        _ => fallback,
    }
}

fn bool_from_value(value: Option<&Value>) -> bool {
    match value {
        Some(Value::Bool(value)) => *value,
        Some(Value::Number(number)) => number
            .as_i64()
            .or_else(|| number.as_u64().and_then(|value| i64::try_from(value).ok()))
            .or_else(|| number.as_f64().map(|value| value as i64))
            .is_some_and(|value| value != 0),
        Some(Value::String(value)) => value == "1" || value.eq_ignore_ascii_case("true"),
        _ => false,
    }
}

fn default_string(value: &str, fallback: &str) -> String {
    if value.trim().is_empty() {
        fallback.to_string()
    } else {
        value.to_string()
    }
}

fn internal_error(err: impl std::fmt::Display) -> MessageError {
    MessageError::Internal(err.to_string())
}
