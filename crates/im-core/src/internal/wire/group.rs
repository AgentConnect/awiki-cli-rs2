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

pub(crate) fn build_group_get_rpc_params(
    sender_did: &str,
    group_did: &str,
) -> crate::ImResult<Value> {
    let group_did = require_group(group_did)?;
    Ok(json!({
        "meta": group_local_meta(sender_did, Some(group_did)),
        "body": {
            "group_did": group_did,
        },
    }))
}

pub(crate) fn build_group_list_rpc_params(sender_did: &str, limit: i64) -> Value {
    json!({
        "meta": group_local_meta(sender_did, None),
        "body": {
            "limit": if limit <= 0 { 50 } else { limit },
        },
    })
}

pub(crate) fn build_group_members_rpc_params(
    sender_did: &str,
    group_did: &str,
    limit: i64,
) -> crate::ImResult<Value> {
    let group_did = require_group(group_did)?;
    Ok(json!({
        "meta": group_local_meta(sender_did, Some(group_did)),
        "body": {
            "group_did": group_did,
            "limit": if limit <= 0 { 100 } else { limit },
        },
    }))
}

pub(crate) fn build_group_messages_rpc_params(
    sender_did: &str,
    group_did: &str,
    limit: i64,
    cursor: Option<&str>,
    skip: i64,
) -> crate::ImResult<Value> {
    let group_did = require_group(group_did)?;
    let mut body = Map::new();
    body.insert(
        "group_did".to_string(),
        Value::String(group_did.to_string()),
    );
    body.insert(
        "limit".to_string(),
        json!(if limit <= 0 { 50 } else { limit }),
    );
    if let Some(cursor) = cursor.map(str::trim).filter(|value| !value.is_empty()) {
        body.insert("since_seq".to_string(), Value::String(cursor.to_string()));
    }
    if skip > 0 {
        body.insert("skip".to_string(), json!(skip));
    }
    Ok(json!({
        "meta": group_local_meta(sender_did, Some(group_did)),
        "body": body,
    }))
}

fn require_group(group_did: &str) -> crate::ImResult<&str> {
    let group_did = group_did.trim();
    if group_did.is_empty() {
        return Err(crate::ImError::invalid_input(
            Some("group".to_string()),
            "group target is required",
        ));
    }
    Ok(group_did)
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

fn group_local_meta(sender_did: &str, group_did: Option<&str>) -> Value {
    let mut meta = Map::new();
    meta.insert("anp_version".to_string(), Value::String("1.0".to_string()));
    meta.insert(
        "profile".to_string(),
        Value::String("anp.group.local.v1".to_string()),
    );
    meta.insert(
        "security_profile".to_string(),
        Value::String("transport-protected".to_string()),
    );
    meta.insert(
        "sender_did".to_string(),
        Value::String(sender_did.to_string()),
    );
    if let Some(group_did) = group_did {
        meta.insert(
            "target".to_string(),
            json!({
                "kind": "group",
                "did": group_did,
            }),
        );
    }
    Value::Object(meta)
}
