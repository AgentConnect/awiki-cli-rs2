use crate::identity::types::StoredIdentity;
use crate::message::attachment_manifest_content_type;
use crate::message::types::{HistoryRequest, InboxRequest, MarkReadRequest, MessageError};
use crate::message::{build_origin_proof, origin_auth_value};
use serde_json::{json, Map, Value};
use time::OffsetDateTime;

#[derive(Debug, Clone, PartialEq)]
pub struct DirectPayload {
    pub method: String,
    pub meta: Value,
    pub body: Value,
}

pub fn build_direct_text_payload(
    sender_did: &str,
    target_did: &str,
    text: &str,
    content_type: &str,
) -> Result<DirectPayload, MessageError> {
    if sender_did.is_empty() || target_did.is_empty() {
        return Err(MessageError::Json(
            "sender and target did are required".to_string(),
        ));
    }
    if text.is_empty() {
        return Err(MessageError::TextRequired);
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
            "operation_id": format!("op-{}", generate_operation_id()),
            "message_id": format!("msg-{}", generate_operation_id()),
            "created_at": now_rfc3339(),
            "content_type": content_type,
        }),
        body: json!({ "text": text }),
    })
}

pub fn build_direct_send_rpc_params(
    record: &StoredIdentity,
    target_did: &str,
    text: &str,
    message_type: &str,
) -> Result<Value, MessageError> {
    let payload = build_direct_text_payload(
        &record.did,
        target_did,
        text,
        content_type_for_message_type(message_type),
    )?;
    let origin_proof = build_origin_proof(record, &payload)?;
    Ok(json!({
        "meta": payload.meta,
        "auth": origin_auth_value(&origin_proof),
        "body": payload.body,
    }))
}

pub fn build_inbox_rpc_params(record: &StoredIdentity, request: InboxRequest) -> Value {
    let limit = if request.limit <= 0 {
        20
    } else {
        request.limit
    };
    json!({
        "meta": local_meta(&record.did, "anp.inbox.local.v1"),
        "body": {
            "user_did": record.did,
            "limit": limit,
        },
    })
}

pub fn build_history_rpc_params(
    record: &StoredIdentity,
    request: HistoryRequest,
) -> Result<Value, MessageError> {
    if request.with.is_empty() {
        return Err(MessageError::TargetRequired);
    }
    let limit = if request.limit <= 0 {
        50
    } else {
        request.limit
    };
    let mut body = Map::new();
    body.insert("user_did".to_string(), Value::String(record.did.clone()));
    body.insert("peer_did".to_string(), Value::String(request.with));
    body.insert("limit".to_string(), json!(limit));
    if !request.cursor.is_empty() {
        body.insert("since_seq".to_string(), Value::String(request.cursor));
    }
    if request.skip > 0 {
        body.insert("skip".to_string(), json!(request.skip));
    }
    Ok(json!({
        "meta": local_meta(&record.did, "anp.direct.local.v1"),
        "body": body,
    }))
}

pub fn build_mark_read_rpc_params(
    record: &StoredIdentity,
    request: MarkReadRequest,
) -> Result<Value, MessageError> {
    if request.message_ids.is_empty() {
        return Err(MessageError::Json(
            "message not found: message_ids are required".to_string(),
        ));
    }
    Ok(json!({
        "meta": local_meta(&record.did, "anp.inbox.local.v1"),
        "body": {
            "user_did": record.did,
            "message_ids": request.message_ids,
        },
    }))
}

pub fn content_type_for_message_type(message_type: &str) -> &'static str {
    match message_type.trim().to_ascii_lowercase().as_str() {
        "attachment_manifest" => attachment_manifest_content_type(),
        "event" => "application/json",
        _ => "text/plain",
    }
}

fn local_meta(sender_did: &str, profile: &str) -> Value {
    json!({
        "anp_version": "1.0",
        "profile": profile,
        "security_profile": "transport-protected",
        "sender_did": sender_did,
        "operation_id": format!("op-{}", generate_operation_id()),
        "created_at": now_rfc3339(),
    })
}

pub(crate) fn message_meta(sender_did: &str, service_did: &str, profile: &str) -> Value {
    json!({
        "anp_version": "1.0",
        "profile": profile,
        "security_profile": "transport-protected",
        "sender_did": sender_did,
        "target": {
            "kind": "service",
            "did": service_did,
        },
        "operation_id": format!("op-{}", generate_operation_id()),
        "created_at": now_rfc3339(),
    })
}

pub(crate) fn signed_message_meta(
    sender_did: &str,
    target_kind: &str,
    target_did: &str,
    profile: &str,
    content_type: &str,
) -> Value {
    json!({
        "anp_version": "1.0",
        "profile": profile,
        "security_profile": "transport-protected",
        "sender_did": sender_did,
        "target": {
            "kind": target_kind,
            "did": target_did,
        },
        "operation_id": format!("op-{}", generate_operation_id()),
        "message_id": format!("msg-{}", generate_operation_id()),
        "created_at": now_rfc3339(),
        "content_type": content_type,
    })
}

pub(crate) fn now_rfc3339() -> String {
    let now = OffsetDateTime::now_utc()
        .replace_nanosecond(0)
        .unwrap_or_else(|_| OffsetDateTime::UNIX_EPOCH);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        now.year(),
        u8::from(now.month()),
        now.day(),
        now.hour(),
        now.minute(),
        now.second()
    )
}

pub(crate) fn generate_operation_id() -> String {
    use rand::RngCore;
    let mut bytes = [0_u8; 8];
    rand::thread_rng().fill_bytes(&mut bytes);
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
