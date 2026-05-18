use super::group_e2ee_create::local_group_state_ref;
use super::group_e2ee_provider::{MlsExecProvider, ANP_MLS_API_VERSION};
use super::group_e2ee_repair::repair_group_e2ee_notices;
use super::group_e2ee_status::group_e2ee_status_for_recovery;
use super::group_e2ee_transport::GroupE2eeTransport;
use super::group_service::{
    cached_group_snapshot, compact_warnings, group_send_message_id, group_storage_key, i64_option,
    sync_group_state, GroupSendResult,
};
use super::service::{default_message_type, metadata_string, string_value, CommandResult};
use super::{
    content_type_for_message_type, GROUP_E2EE_CIPHER_CONTENT_TYPE, GROUP_E2EE_SECURITY_PROFILE,
};
use super::{MessageError, SendRequest};
use crate::config::Resolved;
use crate::identity::types::StoredIdentity;
use crate::identity::Manager;
use crate::store::{self, MessageRecord};
use serde_json::{json, Map, Value};

pub(crate) fn maybe_send_group_e2ee(
    resolved: &Resolved,
    manager: &Manager,
    record: &StoredIdentity,
    request: &SendRequest,
) -> Result<Option<CommandResult>, MessageError> {
    if request.secure_mode.trim().eq_ignore_ascii_case("on") {
        if !cached_group_uses_e2ee(resolved, record, &request.group) {
            return Err(MessageError::SecureNotSupported);
        }
        return send_group_e2ee(resolved, manager, record, request.clone()).map(Some);
    }
    if cached_group_uses_e2ee(resolved, record, &request.group)
        || group_has_local_e2ee_state(resolved, record, &request.group)
    {
        return send_group_e2ee(resolved, manager, record, request.clone()).map(Some);
    }
    Ok(None)
}

fn send_group_e2ee(
    resolved: &Resolved,
    manager: &Manager,
    record: &StoredIdentity,
    request: SendRequest,
) -> Result<CommandResult, MessageError> {
    let provider = MlsExecProvider::new(resolved);
    let mut warnings = sync_group_state(resolved, manager, record, &request.group, false);
    let mut device_id = "default".to_string();
    match group_e2ee_status_for_recovery(&provider, &record.did, &request.group, "") {
        Ok((status, candidate_device_id)) => {
            if string_value(status.get("status")).eq_ignore_ascii_case("active") {
                device_id = candidate_device_id;
            }
        }
        Err(err) => warnings.push(format!(
            "Group E2EE send could not inspect device-scoped MLS status before encrypt: {err}"
        )),
    }
    let (encrypt_result, delivery) = encrypt_and_send_group_e2ee(
        resolved, manager, record, &provider, &request, &device_id, false,
    )
    .or_else(|err| {
        if !is_group_e2ee_epoch_mismatch(&err) {
            return Err(err);
        }
        warnings.extend(repair_stale_group_epoch(
            resolved,
            manager,
            record,
            &request.group,
            &err,
        )?);
        if let Ok((status, candidate_device_id)) =
            group_e2ee_status_for_recovery(&provider, &record.did, &request.group, &device_id)
        {
            if string_value(status.get("status")).eq_ignore_ascii_case("active") {
                device_id = candidate_device_id;
            }
        } else {
            warnings.push(
                "Group E2EE send could not inspect device-scoped MLS status after repair"
                    .to_string(),
            );
        }
        encrypt_and_send_group_e2ee(
            resolved, manager, record, &provider, &request, &device_id, true,
        )
    })?;

    let mut command_result = persist_group_e2ee_send_result(resolved, record, &request, &delivery);
    warnings.extend(command_result.warnings);
    if let Some(data) = command_result.data.as_object_mut() {
        if let Some(message) = data.get_mut("message").and_then(Value::as_object_mut) {
            message.insert("secure".to_string(), Value::Bool(true));
            message.insert(
                "security_profile".to_string(),
                Value::String(GROUP_E2EE_SECURITY_PROFILE.to_string()),
            );
        }
        data.insert(
            "e2ee".to_string(),
            json!({
                "encrypted": true,
                "group_state_ref": group_state_ref_from_cipher(&encrypt_result),
                "cipher_object_sent": true,
            }),
        );
    }
    command_result.summary = format!(
        "Sent a group {} message with group E2EE",
        default_message_type(&request.message_type)
    );
    command_result.warnings = compact_warnings(&mut warnings);
    Ok(command_result)
}

pub(crate) fn group_has_local_e2ee_state(
    resolved: &Resolved,
    record: &StoredIdentity,
    group_did: &str,
) -> bool {
    if group_did.trim().is_empty() {
        return false;
    }
    match MlsExecProvider::new(resolved).status(&record.did, "default", group_did) {
        Ok(status) => {
            let state = string_value(status.get("status")).to_ascii_lowercase();
            state == "active"
                || state == "pending_commit"
                || !string_value(status.get("crypto_group_id_b64u")).is_empty()
        }
        Err(_) => false,
    }
}

fn encrypt_and_send_group_e2ee(
    resolved: &Resolved,
    manager: &Manager,
    record: &StoredIdentity,
    provider: &MlsExecProvider,
    request: &SendRequest,
    device_id: &str,
    retry: bool,
) -> Result<(Map<String, Value>, GroupSendResult), MessageError> {
    let operation_id = format!("op-{}", super::wire::generate_operation_id());
    let message_id = format!("msg-{}", super::wire::generate_operation_id());
    let request_id_prefix = if retry {
        "group-e2ee-encrypt-retry-"
    } else {
        "group-e2ee-encrypt-"
    };
    let encrypt_request = json!({
        "api_version": ANP_MLS_API_VERSION,
        "request_id": format!("{request_id_prefix}{}", super::wire::generate_operation_id()),
        "agent_did": record.did,
        "device_id": device_id,
        "params": {
            "agent_did": record.did,
            "device_id": device_id,
            "group_did": request.group,
            "group_state_ref": local_group_state_ref(resolved, record, &request.group),
            "sender_did": record.did,
            "content_type": GROUP_E2EE_CIPHER_CONTENT_TYPE,
            "security_profile": GROUP_E2EE_SECURITY_PROFILE,
            "message_id": message_id,
            "operation_id": operation_id,
            "message_type": default_message_type(&request.message_type),
            "application_plaintext": {
                "application_content_type": content_type_for_message_type(&request.message_type),
                "text": request.text,
            },
        },
    });
    let encrypt_result = provider.encrypt(&encrypt_request, &record.did, device_id)?;
    let cipher = encrypt_result
        .get("group_cipher_object")
        .and_then(Value::as_object)
        .cloned()
        .filter(|cipher| !cipher.is_empty())
        .ok_or_else(|| {
            let detail = if retry {
                "anp-mls retry encrypt response missing group_cipher_object"
            } else {
                "anp-mls encrypt response missing group_cipher_object"
            };
            MessageError::Internal(detail.to_string())
        })?;
    let mut transport = GroupE2eeTransport::new(resolved, manager, record)?;
    let delivery = transport.send_group_e2ee(&request.group, cipher, &operation_id, &message_id)?;
    Ok((encrypt_result, delivery))
}

fn persist_group_e2ee_send_result(
    resolved: &Resolved,
    record: &StoredIdentity,
    request: &SendRequest,
    result: &GroupSendResult,
) -> CommandResult {
    let message_type = default_message_type(&request.message_type).to_string();
    let mut warnings = Vec::new();
    let message_id = group_send_message_id(&request.group, result);
    match store::open(&resolved.paths) {
        Ok(connection) => {
            if let Err(err) = store::ensure_schema(&connection) {
                warnings.push(format!(
                    "Failed to ensure local schema for group send: {err}"
                ));
            } else {
                if let Err(err) = store::store_message(
                    &connection,
                    MessageRecord {
                        msg_id: message_id.clone(),
                        owner_did: record.did.clone(),
                        thread_id: store::make_thread_id(
                            &record.did,
                            "",
                            &group_storage_key(&request.group),
                        ),
                        direction: 1,
                        sender_did: record.did.clone(),
                        group_id: group_storage_key(&request.group),
                        group_did: request.group.clone(),
                        content_type: content_type_for_message_type(&message_type).to_string(),
                        content: request.text.clone(),
                        sent_at: result.accepted_at.clone(),
                        is_e2ee: true,
                        is_read: true,
                        metadata: metadata_string(json!({
                            "group_event_seq": result.group_event_seq,
                            "group_state_version": result.group_state_version,
                            "operation_id": result.operation_id,
                            "security_profile": GROUP_E2EE_SECURITY_PROFILE,
                        })),
                        credential_name: record.identity_name.clone(),
                        ..MessageRecord::default()
                    },
                ) {
                    warnings.push(format!("Failed to persist local group message: {err}"));
                }
                if let Err(err) = store::touch_group_after_message(
                    &connection,
                    &record.did,
                    &group_storage_key(&request.group),
                    &request.group,
                    &result.accepted_at,
                    i64_option(Some(&Value::String(result.group_event_seq.clone()))),
                    &record.identity_name,
                    &metadata_string(json!({ "group_state_version": result.group_state_version })),
                ) {
                    warnings.push(format!("Failed to update group cache: {err}"));
                }
            }
        }
        Err(_) => warnings.push("Failed to open local store for group send".to_string()),
    }
    CommandResult {
        data: json!({
            "action": "send_message",
            "target": {
                "kind": "group",
                "did": request.group,
            },
            "message": {
                "id": message_id,
                "type": message_type,
                "secure": true,
                "security_profile": GROUP_E2EE_SECURITY_PROFILE,
                "sent_at": result.accepted_at,
            },
            "delivery": result,
            "source": "remote_http",
        }),
        summary: String::new(),
        warnings,
    }
}

fn repair_stale_group_epoch(
    resolved: &Resolved,
    manager: &Manager,
    record: &StoredIdentity,
    group_did: &str,
    original_error: &MessageError,
) -> Result<Vec<String>, MessageError> {
    let mut warnings = Vec::new();
    let provider = MlsExecProvider::new(resolved);
    if let Ok(status) = provider.status(&record.did, "default", group_did) {
        for pending in super::group_e2ee_repair::notice_objects(status.get("pending_commits")) {
            let pending_id = string_value(pending.get("pending_commit_id"));
            if pending_id.is_empty() {
                continue;
            }
            let mut prepared = Map::new();
            prepared.insert("pending_commit_id".to_string(), Value::String(pending_id));
            if super::group_e2ee_remove::finalize_prepared_group_e2ee_commit(
                resolved, record, group_did, &prepared,
            )
            .is_ok()
            {
                warnings.push(
                    "Group E2EE local pending commit finalized after service epoch mismatch."
                        .to_string(),
                );
                break;
            }
        }
    }
    match repair_group_e2ee_notices(resolved, manager, &record.identity_name, group_did, 50) {
        Ok(result) => {
            warnings.push(
                "Group E2EE local epoch was stale; repaired pending notices and retried send."
                    .to_string(),
            );
            warnings.extend(result.warnings);
            Ok(warnings)
        }
        Err(err) => {
            warnings.push(format!(
                "Group E2EE send saw stale epoch and notice repair failed: {err}"
            ));
            Err(MessageError::Internal(original_error.to_string()))
        }
    }
}

fn is_group_e2ee_epoch_mismatch(err: &MessageError) -> bool {
    let text = err.to_string().to_ascii_lowercase();
    text.contains("group.e2ee.send") && text.contains("epoch mismatch")
}

fn group_state_ref_from_cipher(encrypt_result: &Map<String, Value>) -> Value {
    encrypt_result
        .get("group_cipher_object")
        .and_then(|value| value.get("group_state_ref"))
        .cloned()
        .unwrap_or(Value::Null)
}

pub(crate) fn cached_group_uses_e2ee(
    resolved: &Resolved,
    record: &StoredIdentity,
    group_did: &str,
) -> bool {
    cached_group_snapshot(resolved, record, group_did)
        .as_ref()
        .is_some_and(super::group_e2ee_add::group_snapshot_uses_e2ee)
}
