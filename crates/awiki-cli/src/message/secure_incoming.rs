use super::types::MessageError;
use super::{new_secure_e2ee_client_for_record, Client};
use crate::authsdk::Session;
use crate::config::Resolved;
use crate::identity::types::StoredIdentity;
use crate::identity::Manager;
use crate::message::service::auth_session;
use crate::transportcfg::Profile;
use serde_json::{json, Map, Value};

pub type SecureIncomingRpcResult = Result<Map<String, Value>, String>;
pub type SecureIncomingRpc = dyn FnMut(&str, Map<String, Value>) -> SecureIncomingRpcResult;
pub type SecureIncomingProcessor = dyn FnMut(Map<String, Value>) -> SecureIncomingRpcResult;

pub fn maybe_decrypt_direct_e2ee_messages(
    resolved: &Resolved,
    manager: &Manager,
    record: &StoredIdentity,
    messages: &mut Vec<Value>,
) -> Vec<String> {
    if messages.is_empty() || !contains_direct_e2ee_messages(messages) {
        return Vec::new();
    }
    let auth = match auth_session(resolved, manager, record) {
        Ok(auth) => auth,
        Err(err) => {
            return compact_warnings(vec![format!(
                "Failed to initialize secure direct decryptor: {err}"
            )]);
        }
    };
    let client = match Client::new(resolved) {
        Ok(client) => client,
        Err(err) => {
            return compact_warnings(vec![format!(
                "Failed to initialize secure direct decryptor: {err}"
            )]);
        }
    };
    let rpc = secure_rpc(client, auth);
    let mut client = match new_secure_e2ee_client_for_record(Some(manager), Some(record), rpc) {
        Ok(client) => client,
        Err(err) => {
            return compact_warnings(vec![format!(
                "Failed to initialize secure direct decryptor: {err}"
            )]);
        }
    };
    maybe_decrypt_direct_e2ee_messages_with_processor(messages, |notification| {
        client.process_incoming(notification)
    })
}

pub fn maybe_decrypt_direct_e2ee_messages_with_processor(
    messages: &mut Vec<Value>,
    mut process_incoming: impl FnMut(Map<String, Value>) -> SecureIncomingRpcResult,
) -> Vec<String> {
    if messages.is_empty() || !contains_direct_e2ee_messages(messages) {
        return Vec::new();
    }
    let mut order = (0..messages.len()).collect::<Vec<_>>();
    order.sort_by(|left, right| compare_message_order(&messages[*left], &messages[*right]));
    let mut warnings = Vec::new();
    for index in order {
        let content_type = string_from_message(&messages[index], "content_type");
        if !is_direct_e2ee_wire_content_type(&content_type) {
            continue;
        }
        let notification = match direct_e2ee_notification_from_message_view(&messages[index]) {
            Ok(notification) => notification,
            Err(err) => {
                if !is_direct_e2ee_wire_control_message(&messages[index]) {
                    warnings.push(format!(
                        "Skipped secure direct message {}: {err}",
                        string_from_message(&messages[index], "id")
                    ));
                }
                continue;
            }
        };
        let result = match process_incoming(notification) {
            Ok(result) => result,
            Err(err) => {
                if !is_direct_e2ee_wire_control_message(&messages[index]) {
                    warnings.push(format!(
                        "Failed to decrypt secure direct message {}: {err}",
                        string_from_message(&messages[index], "id")
                    ));
                }
                continue;
            }
        };
        apply_direct_e2ee_processing_result(&mut messages[index], &Value::Object(result));
    }
    compact_warnings(warnings)
}

pub fn is_direct_e2ee_wire_content_type(content_type: &str) -> bool {
    matches!(
        content_type,
        "application/anp-direct-init+json" | "application/anp-direct-cipher+json"
    )
}

pub fn direct_e2ee_notification_from_message_view(
    message: &Value,
) -> Result<Map<String, Value>, String> {
    let object = message
        .as_object()
        .ok_or_else(|| "content is not a direct-e2ee object".to_string())?;
    let body = content_object_value(object.get("content"))
        .ok_or_else(|| "content is not a direct-e2ee object".to_string())?;
    let sender_did = string_value(object.get("sender_did"));
    let receiver_did = string_value(object.get("receiver_did"));
    let message_id = string_value(object.get("id"));
    if sender_did.is_empty() || receiver_did.is_empty() || message_id.is_empty() {
        return Err("missing sender_did/receiver_did/id".to_string());
    }
    let mut result = Map::from_iter([
        (
            "meta".to_string(),
            json!({
                "sender_did": sender_did,
                "target": {
                    "kind": "agent",
                    "did": receiver_did,
                },
                "message_id": message_id,
                "profile": "anp.direct.e2ee.v1",
                "security_profile": "direct-e2ee",
                "content_type": string_value(object.get("content_type")),
            }),
        ),
        ("body".to_string(), body),
    ]);
    if let Some(server_seq) = i64_value(object.get("server_seq")).filter(|value| *value != 0) {
        result.insert("server_seq".to_string(), json!(server_seq));
    }
    Ok(result)
}

pub fn apply_direct_e2ee_processing_result(message: &mut Value, result: &Value) {
    let Some(message) = message.as_object_mut() else {
        return;
    };
    message.insert("secure".to_string(), Value::Bool(true));
    let state = string_value(result.get("state"));
    if state.is_empty() {
        return;
    }
    message.insert("decryption_state".to_string(), Value::String(state.clone()));
    if state != "decrypted" {
        return;
    }
    let Some(plaintext) = result.get("plaintext").and_then(Value::as_object) else {
        return;
    };
    if is_secure_control_plaintext(plaintext, "awiki.direct.secure_ack.v1")
        || is_secure_control_plaintext(plaintext, "awiki.direct.secure_init.v1")
    {
        message.insert("secure_control".to_string(), Value::Bool(true));
        message.insert(
            "type".to_string(),
            Value::String("secure_control".to_string()),
        );
        message.insert("content".to_string(), Value::String(String::new()));
        return;
    }
    let content_type = string_value(plaintext.get("application_content_type"));
    if !content_type.is_empty() {
        message.insert(
            "content_type".to_string(),
            Value::String(content_type.clone()),
        );
    }
    if let Some(text) = plaintext
        .get("text")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
    {
        message.insert("content".to_string(), Value::String(text.to_string()));
        message.insert("type".to_string(), Value::String("text".to_string()));
    } else if let Some(payload) = plaintext.get("payload") {
        message.insert("content".to_string(), payload.clone());
        let message_type = if content_type == super::attachment_manifest_content_type() {
            "attachment_manifest"
        } else {
            "json"
        };
        message.insert("type".to_string(), Value::String(message_type.to_string()));
    } else if let Some(payload_b64u) = plaintext
        .get("payload_b64u")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
    {
        message.insert(
            "content".to_string(),
            Value::String(payload_b64u.to_string()),
        );
        message.insert("type".to_string(), Value::String("binary".to_string()));
    }
}

pub fn filter_displayable_direct_e2ee_messages(messages: Vec<Value>) -> Vec<Value> {
    messages
        .into_iter()
        .filter(|message| !is_direct_e2ee_control_or_undisplayable(message))
        .collect()
}

pub fn is_direct_e2ee_control_or_undisplayable(message: &Value) -> bool {
    let Some(object) = message.as_object() else {
        return false;
    };
    if bool_value(object.get("secure_control")) {
        return true;
    }
    if !is_direct_e2ee_wire_content_type(&string_value(object.get("content_type"))) {
        return false;
    }
    matches!(
        string_value(object.get("decryption_state")).as_str(),
        "" | "undecryptable" | "failed"
    )
}

fn secure_rpc(client: Client, mut auth: Session) -> Box<SecureIncomingRpc> {
    let endpoint = super::MESSAGE_RPC_ENDPOINT.to_string();
    Box::new(move |method, params| {
        client
            .authenticated_rpc_call_profile::<Map<String, Value>, _>(
                Profile::RpcDefault,
                &endpoint,
                method,
                params,
                &mut auth,
            )
            .map_err(|err: MessageError| err.to_string())
    })
}

fn compare_message_order(left: &Value, right: &Value) -> std::cmp::Ordering {
    let left_seq =
        i64_value(left.as_object().and_then(|value| value.get("server_seq"))).unwrap_or_default();
    let right_seq =
        i64_value(right.as_object().and_then(|value| value.get("server_seq"))).unwrap_or_default();
    if left_seq == right_seq {
        return string_from_message(left, "id").cmp(&string_from_message(right, "id"));
    }
    if left_seq == 0 {
        return std::cmp::Ordering::Greater;
    }
    if right_seq == 0 {
        return std::cmp::Ordering::Less;
    }
    left_seq.cmp(&right_seq)
}

fn content_object_value(value: Option<&Value>) -> Option<Value> {
    match value? {
        Value::Object(_) => Some(value.expect("checked Some above").clone()),
        Value::String(text) if !text.trim().is_empty() => {
            let decoded: Value = serde_json::from_str(text).ok()?;
            if decoded.is_object() {
                Some(decoded)
            } else {
                None
            }
        }
        _ => None,
    }
}

fn contains_direct_e2ee_messages(messages: &[Value]) -> bool {
    messages.iter().any(|message| {
        is_direct_e2ee_wire_content_type(&string_from_message(message, "content_type"))
    })
}

fn is_direct_e2ee_wire_control_message(message: &Value) -> bool {
    let content_type = string_from_message(message, "content_type");
    if content_type == "application/anp-direct-init+json" {
        return true;
    }
    let mut id = string_from_message(message, "id");
    if id.is_empty() {
        id = string_from_message(message, "msg_id");
    }
    id.starts_with("secure-init-") || id.starts_with("ack-")
}

fn is_secure_control_plaintext(plaintext: &Map<String, Value>, system_type: &str) -> bool {
    if string_value(plaintext.get("application_content_type")) != "application/json" {
        return false;
    }
    let Some(payload) = content_object_value(plaintext.get("payload")) else {
        return false;
    };
    string_value(payload.get("system_type")) == system_type
}

fn string_from_message(message: &Value, key: &str) -> String {
    message
        .as_object()
        .map(|object| string_value(object.get(key)))
        .unwrap_or_default()
}

fn string_value(value: Option<&Value>) -> String {
    value
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn bool_value(value: Option<&Value>) -> bool {
    match value {
        Some(Value::Bool(value)) => *value,
        Some(Value::Number(number)) => number.as_i64().unwrap_or_default() != 0,
        Some(Value::String(value)) => value == "1" || value.eq_ignore_ascii_case("true"),
        _ => false,
    }
}

fn i64_value(value: Option<&Value>) -> Option<i64> {
    match value {
        Some(Value::Number(number)) => number
            .as_i64()
            .or_else(|| number.as_u64().and_then(|value| i64::try_from(value).ok()))
            .or_else(|| number.as_f64().map(|value| value as i64)),
        _ => None,
    }
}

fn compact_warnings(warnings: Vec<String>) -> Vec<String> {
    let mut compact = Vec::new();
    for warning in warnings {
        let warning = warning.trim();
        if warning.is_empty() || compact.iter().any(|known: &String| known == warning) {
            continue;
        }
        compact.push(warning.to_string());
    }
    compact
}
