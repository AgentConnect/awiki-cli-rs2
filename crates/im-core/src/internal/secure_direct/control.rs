use serde_json::{Map, Value};

pub const SECURE_ACK_SYSTEM_TYPE: &str = "awiki.direct.secure_ack.v1";
pub const SECURE_INIT_SYSTEM_TYPE: &str = "awiki.direct.secure_init.v1";

pub fn build_secure_ack_payload(session_id: &str, acked_message_id: &str) -> Map<String, Value> {
    Map::from_iter([
        (
            "system_type".to_owned(),
            Value::String(SECURE_ACK_SYSTEM_TYPE.to_owned()),
        ),
        (
            "session_id".to_owned(),
            Value::String(session_id.trim().to_owned()),
        ),
        (
            "acked_message_id".to_owned(),
            Value::String(acked_message_id.trim().to_owned()),
        ),
    ])
}

pub fn build_secure_init_payload() -> Map<String, Value> {
    Map::from_iter([
        (
            "system_type".to_owned(),
            Value::String(SECURE_INIT_SYSTEM_TYPE.to_owned()),
        ),
        ("reason".to_owned(), Value::String("manual_init".to_owned())),
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
    value.and_then(Value::as_str).unwrap_or_default().to_owned()
}

#[cfg(test)]
mod tests {
    use serde_json::{json, Map, Value};

    use super::*;

    #[test]
    fn secure_ack_payload_trims_session_and_acked_message_ids() {
        assert_eq!(
            Value::Object(build_secure_ack_payload(" session-1 \n", "\tmsg-9 ")),
            json!({
                "system_type": SECURE_ACK_SYSTEM_TYPE,
                "session_id": "session-1",
                "acked_message_id": "msg-9",
            })
        );
    }

    #[test]
    fn secure_init_payload_matches_manual_init_control_payload() {
        assert_eq!(
            Value::Object(build_secure_init_payload()),
            json!({
                "system_type": SECURE_INIT_SYSTEM_TYPE,
                "reason": "manual_init",
            })
        );
    }

    #[test]
    fn secure_control_plaintext_detection_accepts_object_and_json_string_payloads() {
        assert!(is_secure_ack_plaintext(&plaintext_with_payload(json!({
            "system_type": SECURE_ACK_SYSTEM_TYPE,
            "session_id": "session-1",
            "acked_message_id": "msg-9",
        }))));
        assert!(is_secure_init_plaintext(&plaintext_with_payload(json!({
            "system_type": SECURE_INIT_SYSTEM_TYPE,
            "reason": "manual_init",
        }))));
        assert!(is_secure_ack_plaintext(&plaintext_with_payload(json!(
            r#"{"system_type":"awiki.direct.secure_ack.v1","session_id":"session-from-string"}"#
        ))));
    }

    #[test]
    fn secure_control_plaintext_detection_rejects_non_matching_shapes() {
        let valid_ack_payload = json!({
            "system_type": SECURE_ACK_SYSTEM_TYPE,
            "session_id": "session-1",
            "acked_message_id": "msg-9",
        });

        let mut missing_content_type = plaintext_with_payload(valid_ack_payload.clone());
        missing_content_type.remove("application_content_type");
        assert!(!is_secure_ack_plaintext(&missing_content_type));

        let mut wrong_content_type = plaintext_with_payload(valid_ack_payload);
        wrong_content_type.insert("application_content_type".to_owned(), json!("text/plain"));
        assert!(!is_secure_ack_plaintext(&wrong_content_type));

        assert!(!is_secure_ack_plaintext(&plaintext_with_payload(json!(
            "not-an-object"
        ))));
        assert!(!is_secure_ack_plaintext(&plaintext_with_payload(json!({
            "system_type": SECURE_INIT_SYSTEM_TYPE,
            "reason": "manual_init",
        }))));
        assert!(!is_secure_init_plaintext(&plaintext_with_payload(json!({
            "system_type": SECURE_ACK_SYSTEM_TYPE,
            "session_id": "session-1",
            "acked_message_id": "msg-9",
        }))));
    }

    #[test]
    fn secure_ack_session_id_reads_only_string_session_from_payload() {
        assert_eq!(
            secure_ack_session_id(&plaintext_with_payload(json!({
                "system_type": SECURE_ACK_SYSTEM_TYPE,
                "session_id": "session-1",
            }))),
            "session-1"
        );
        assert_eq!(
            secure_ack_session_id(&plaintext_with_payload(json!({
                "system_type": SECURE_ACK_SYSTEM_TYPE,
                "session_id": 42,
            }))),
            ""
        );
        assert_eq!(
            secure_ack_session_id(&plaintext_with_payload(json!("not-an-object"))),
            ""
        );
        assert_eq!(
            secure_ack_session_id(&plaintext_with_payload(json!(
                r#"{"session_id":"session-1"}"#
            ))),
            "session-1"
        );
    }

    #[test]
    fn pending_confirmation_error_detection_matches_legacy_string_checks() {
        assert!(!is_pending_confirmation_error(None));
        assert!(is_pending_confirmation_error(Some(
            "remote returned PENDING CONFIRMATION for peer"
        )));
        assert!(is_pending_confirmation_error(Some(
            "secure state is Pending-Confirmation"
        )));
        assert!(!is_pending_confirmation_error(Some("confirmation pending")));
    }

    fn plaintext_with_payload(payload: Value) -> Map<String, Value> {
        Map::from_iter([
            (
                "application_content_type".to_owned(),
                json!("application/json"),
            ),
            ("payload".to_owned(), payload),
        ])
    }
}
