use super::group_e2ee_provider::{default_string, MlsExecProvider, ANP_MLS_API_VERSION};
use super::group_service::{compact_warnings, values_from_array};
use super::service::{metadata_string, string_value};
use super::{MessageError, GROUP_E2EE_CIPHER_CONTENT_TYPE, GROUP_E2EE_SECURITY_PROFILE};
use crate::config::Resolved;
use crate::identity::types::StoredIdentity;
use serde_json::{json, Map, Value};

pub(crate) fn maybe_decrypt_group_messages(
    resolved: &Resolved,
    record: &StoredIdentity,
    group_did: &str,
    raw: &mut Value,
) -> Vec<String> {
    let mut messages = values_from_array(raw.get("messages"));
    if messages.is_empty() {
        return Vec::new();
    }
    let provider = MlsExecProvider::new(resolved);
    let device_ids = provider.candidate_device_ids(&record.did);
    let mut warnings = Vec::new();
    for item in &mut messages {
        let Some(cipher) = group_cipher_object_from_message(item) else {
            continue;
        };
        let aad = group_e2ee_aad_params_from_message(group_did, item, &cipher);
        match decrypt_group_cipher_with_devices(
            &provider,
            &record.did,
            group_did,
            &cipher,
            &aad,
            &device_ids,
        ) {
            Ok(plain) => apply_group_plaintext(item, &plain),
            Err(err) => warnings.push(format!(
                "Group E2EE decrypt failed for message {}: {err}",
                string_value(item.get("id"))
            )),
        }
    }
    if let Some(object) = raw.as_object_mut() {
        object.insert("messages".to_string(), Value::Array(messages));
    }
    compact_warnings(&mut warnings)
}

fn decrypt_group_cipher_with_devices(
    provider: &MlsExecProvider,
    agent_did: &str,
    group_did: &str,
    cipher: &Map<String, Value>,
    aad: &Map<String, Value>,
    device_ids: &[String],
) -> Result<Map<String, Value>, MessageError> {
    let mut candidates = device_ids.to_vec();
    if candidates.is_empty() {
        candidates.push("default".to_string());
    }
    let mut last_error = None;
    for device_id in candidates {
        let device_id = default_string(device_id.trim(), "default");
        let mut params = Map::new();
        params.insert(
            "agent_did".to_string(),
            Value::String(agent_did.to_string()),
        );
        params.insert(
            "recipient_did".to_string(),
            Value::String(agent_did.to_string()),
        );
        params.insert("device_id".to_string(), Value::String(device_id.clone()));
        params.insert(
            "group_did".to_string(),
            Value::String(group_did.to_string()),
        );
        params.insert(
            "group_cipher_object".to_string(),
            Value::Object(cipher.clone()),
        );
        params.insert(
            "private_message_b64u".to_string(),
            cipher
                .get("private_message_b64u")
                .cloned()
                .unwrap_or(Value::Null),
        );
        for (key, value) in aad {
            if !value.is_null() {
                params.insert(key.clone(), value.clone());
            }
        }
        let request = json!({
            "api_version": ANP_MLS_API_VERSION,
            "request_id": format!("group-e2ee-decrypt-{}", super::wire::generate_operation_id()),
            "agent_did": agent_did,
            "device_id": device_id,
            "params": params,
        });
        match provider.decrypt(&request, agent_did, &device_id) {
            Ok(plain) => return Ok(plain),
            Err(err) => last_error = Some(err),
        }
    }
    Err(last_error.unwrap_or_else(|| {
        MessageError::Internal("no candidate MLS device state found".to_string())
    }))
}

fn apply_group_plaintext(item: &mut Value, plain: &Map<String, Value>) {
    let Some(app_plaintext) = plain
        .get("application_plaintext")
        .and_then(Value::as_object)
    else {
        return;
    };
    let Some(item_object) = item.as_object_mut() else {
        return;
    };
    item_object.insert(
        "content".to_string(),
        Value::String(default_string(
            &string_value(app_plaintext.get("text")),
            &metadata_string(Value::Object(app_plaintext.clone())),
        )),
    );
    item_object.insert(
        "content_type".to_string(),
        Value::String(default_string(
            &string_value(app_plaintext.get("application_content_type")),
            "text/plain",
        )),
    );
    item_object.insert("decrypted".to_string(), Value::Bool(true));
}

fn group_cipher_object_from_message(item: &Value) -> Option<Map<String, Value>> {
    for key in ["group_cipher_object", "content"] {
        let Some(cipher) = item.get(key).and_then(Value::as_object) else {
            continue;
        };
        if let Some(nested) = cipher
            .get("group_cipher_object")
            .and_then(Value::as_object)
            .cloned()
        {
            return Some(nested);
        }
        if cipher.contains_key("private_message_b64u") {
            return Some(cipher.clone());
        }
    }
    item.get("body")
        .and_then(Value::as_object)
        .and_then(|body| body.get("group_cipher_object"))
        .and_then(Value::as_object)
        .cloned()
}

fn group_e2ee_aad_params_from_message(
    group_did: &str,
    item: &Value,
    cipher: &Map<String, Value>,
) -> Map<String, Value> {
    let receipt = item.get("receipt").and_then(Value::as_object);
    let group_state_ref = cipher
        .get("group_state_ref")
        .and_then(Value::as_object)
        .filter(|value| !value.is_empty())
        .cloned()
        .unwrap_or_else(|| {
            Map::from_iter([(
                "group_did".to_string(),
                Value::String(group_did.to_string()),
            )])
        });
    Map::from_iter([
        (
            "group_state_ref".to_string(),
            Value::Object(group_state_ref),
        ),
        (
            "sender_did".to_string(),
            Value::String(string_value(item.get("sender_did"))),
        ),
        (
            "content_type".to_string(),
            Value::String(GROUP_E2EE_CIPHER_CONTENT_TYPE.to_string()),
        ),
        (
            "security_profile".to_string(),
            Value::String(GROUP_E2EE_SECURITY_PROFILE.to_string()),
        ),
        (
            "message_id".to_string(),
            Value::String(first_non_empty_string(&[
                item.get("message_id"),
                item.get("id"),
            ])),
        ),
        (
            "operation_id".to_string(),
            Value::String(first_non_empty_string(&[
                item.get("operation_id"),
                receipt.and_then(|value| value.get("operation_id")),
            ])),
        ),
    ])
}

fn first_non_empty_string(values: &[Option<&Value>]) -> String {
    values
        .iter()
        .map(|value| string_value(*value))
        .find(|value| !value.trim().is_empty())
        .unwrap_or_default()
}
