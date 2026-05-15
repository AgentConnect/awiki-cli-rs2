use serde_json::{Map, Value};

pub const SECURE_ACK_SYSTEM_TYPE: &str = "awiki.direct.secure_ack.v1";
pub const SECURE_INIT_SYSTEM_TYPE: &str = "awiki.direct.secure_init.v1";

pub fn build_secure_ack_payload(session_id: &str, acked_message_id: &str) -> Map<String, Value> {
    Map::from_iter([
        (
            "system_type".to_string(),
            Value::String(SECURE_ACK_SYSTEM_TYPE.to_string()),
        ),
        (
            "session_id".to_string(),
            Value::String(session_id.trim().to_string()),
        ),
        (
            "acked_message_id".to_string(),
            Value::String(acked_message_id.trim().to_string()),
        ),
    ])
}

pub fn build_secure_init_payload() -> Map<String, Value> {
    Map::from_iter([
        (
            "system_type".to_string(),
            Value::String(SECURE_INIT_SYSTEM_TYPE.to_string()),
        ),
        (
            "reason".to_string(),
            Value::String("manual_init".to_string()),
        ),
    ])
}

pub fn is_secure_ack_plaintext(plaintext: &Map<String, Value>) -> bool {
    is_secure_control_plaintext(plaintext, SECURE_ACK_SYSTEM_TYPE)
}

pub fn is_secure_init_plaintext(plaintext: &Map<String, Value>) -> bool {
    is_secure_control_plaintext(plaintext, SECURE_INIT_SYSTEM_TYPE)
}

pub fn secure_ack_session_id(plaintext: &Map<String, Value>) -> String {
    let Some(payload) = map_from_value(plaintext.get("payload")) else {
        return String::new();
    };
    string_from_value(payload.get("session_id"))
}

pub fn is_pending_confirmation_error(message: Option<&str>) -> bool {
    let Some(message) = message else {
        return false;
    };
    let message = message.to_ascii_lowercase();
    message.contains("pending confirmation") || message.contains("pending-confirmation")
}

fn is_secure_control_plaintext(plaintext: &Map<String, Value>, system_type: &str) -> bool {
    if string_from_value(plaintext.get("application_content_type")) != "application/json" {
        return false;
    }
    let Some(payload) = map_from_value(plaintext.get("payload")) else {
        return false;
    };
    string_from_value(payload.get("system_type")) == system_type
}

fn map_from_value(value: Option<&Value>) -> Option<Map<String, Value>> {
    match value {
        Some(Value::Object(object)) => Some(object.clone()),
        Some(Value::String(value)) if !value.trim().is_empty() => {
            serde_json::from_str::<Map<String, Value>>(value).ok()
        }
        _ => None,
    }
}

fn string_from_value(value: Option<&Value>) -> String {
    value
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}
