use serde_json::{json, Value};

use super::common;

#[derive(Debug, Clone, PartialEq)]
pub struct DirectPayload {
    pub method: String,
    pub meta: Value,
    pub body: Value,
}

pub(crate) fn build_direct_text_payload(
    sender_did: &str,
    target_did: &str,
    text: &str,
    content_type: &str,
) -> crate::ImResult<DirectPayload> {
    if sender_did.is_empty() || target_did.is_empty() {
        return Err(crate::ImError::invalid_input(
            None,
            "sender and target did are required",
        ));
    }
    if text.is_empty() {
        return Err(crate::ImError::invalid_input(
            Some("text".to_string()),
            "message text is required",
        ));
    }
    let content_type = if content_type.is_empty() {
        "text/plain"
    } else {
        content_type
    };
    Ok(DirectPayload {
        method: "direct.send".to_string(),
        meta: json!({
            "anp_version": "1.0",
            "profile": "anp.direct.base.v1",
            "security_profile": "transport-protected",
            "sender_did": sender_did,
            "target": {
                "kind": "agent",
                "did": target_did,
            },
            "operation_id": format!("op-{}", common::generate_operation_id()),
            "message_id": format!("msg-{}", common::generate_operation_id()),
            "created_at": common::now_rfc3339(),
            "content_type": content_type,
        }),
        body: json!({ "text": text }),
    })
}

pub(crate) fn build_direct_json_payload(
    sender_did: &str,
    target_did: &str,
    payload: Value,
) -> crate::ImResult<DirectPayload> {
    if sender_did.is_empty() || target_did.is_empty() {
        return Err(crate::ImError::invalid_input(
            None,
            "sender and target did are required",
        ));
    }
    if !payload.is_object() {
        return Err(crate::ImError::invalid_input(
            Some("payload".to_string()),
            "message payload must be a JSON object",
        ));
    }
    Ok(DirectPayload {
        method: "direct.send".to_string(),
        meta: json!({
            "anp_version": "1.0",
            "profile": "anp.direct.base.v1",
            "security_profile": "transport-protected",
            "sender_did": sender_did,
            "target": {
                "kind": "agent",
                "did": target_did,
            },
            "operation_id": format!("op-{}", common::generate_operation_id()),
            "message_id": format!("msg-{}", common::generate_operation_id()),
            "created_at": common::now_rfc3339(),
            "content_type": "application/json",
        }),
        body: json!({ "payload": payload }),
    })
}
