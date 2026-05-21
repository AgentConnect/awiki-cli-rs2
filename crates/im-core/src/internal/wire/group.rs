use serde_json::{json, Map, Value};

use super::direct::DirectPayload;

pub(crate) fn build_group_send_payload(
    sender_did: &str,
    group_did: &str,
    text: &str,
    content_type: &str,
) -> crate::ImResult<DirectPayload> {
    let group_did = group_did.trim();
    if group_did.is_empty() {
        return Err(crate::ImError::invalid_input(
            Some("group_did".to_string()),
            "group target is required",
        ));
    }
    if text.trim().is_empty() {
        return Err(crate::ImError::invalid_input(
            Some("text".to_string()),
            "message text is required",
        ));
    }

    Ok(DirectPayload {
        method: "group.send".to_string(),
        meta: signed_group_meta(sender_did, "group", group_did, content_type, true),
        body: json!({ "text": text }),
    })
}

fn signed_group_meta(
    sender_did: &str,
    target_kind: &str,
    target_did: &str,
    content_type: &str,
    include_message_id: bool,
) -> Value {
    let mut meta = match group_base_meta(sender_did, Some((target_kind, target_did))) {
        Value::Object(meta) => meta,
        _ => Map::new(),
    };
    meta.insert(
        "operation_id".to_string(),
        Value::String(format!("op-{}", super::common::generate_operation_id())),
    );
    if include_message_id {
        meta.insert(
            "message_id".to_string(),
            Value::String(format!("msg-{}", super::common::generate_operation_id())),
        );
    }
    meta.insert(
        "created_at".to_string(),
        Value::String(super::common::now_rfc3339()),
    );
    meta.insert(
        "content_type".to_string(),
        Value::String(content_type.to_string()),
    );
    Value::Object(meta)
}

fn group_base_meta(sender_did: &str, target: Option<(&str, &str)>) -> Value {
    let mut meta = Map::new();
    meta.insert("anp_version".to_string(), Value::String("1.0".to_string()));
    meta.insert(
        "profile".to_string(),
        Value::String("anp.group.base.v1".to_string()),
    );
    meta.insert(
        "security_profile".to_string(),
        Value::String("transport-protected".to_string()),
    );
    meta.insert(
        "sender_did".to_string(),
        Value::String(sender_did.to_string()),
    );
    if let Some((kind, did)) = target {
        meta.insert(
            "target".to_string(),
            json!({
                "kind": kind,
                "did": did,
            }),
        );
    }
    Value::Object(meta)
}
