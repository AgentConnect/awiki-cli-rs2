use crate::identity::types::StoredIdentity;
use crate::message::attachment_manifest_content_type;
use crate::message::types::{HistoryRequest, InboxRequest, MarkReadRequest, MessageError};
use crate::message::{build_origin_proof, origin_auth_value};
use serde_json::{json, Value};

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
    let payload = im_core::compat::wire::build_direct_text_payload(
        sender_did,
        target_did,
        text,
        content_type,
    )
    .map_err(direct_wire_error)?;
    Ok(DirectPayload {
        method: payload.method,
        meta: payload.meta,
        body: payload.body,
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
    im_core::compat::wire::build_inbox_rpc_params(&wire_identity(record), compat_inbox(request))
}

pub fn build_history_rpc_params(
    record: &StoredIdentity,
    request: HistoryRequest,
) -> Result<Value, MessageError> {
    if request.with.is_empty() {
        return Err(MessageError::TargetRequired);
    }
    im_core::compat::wire::build_history_rpc_params(
        &wire_identity(record),
        im_core::compat::wire::HistoryWireRequest {
            peer_did: request.with,
            limit: request.limit,
            cursor: if request.cursor.is_empty() {
                None
            } else {
                Some(request.cursor)
            },
            skip: request.skip,
        },
    )
    .map_err(wire_error)
}

pub fn build_mark_read_rpc_params(
    record: &StoredIdentity,
    request: MarkReadRequest,
) -> Result<Value, MessageError> {
    im_core::compat::wire::build_mark_read_rpc_params(
        &wire_identity(record),
        im_core::compat::wire::MarkReadWireRequest {
            message_ids: request.message_ids,
        },
    )
    .map_err(wire_error)
}

pub fn content_type_for_message_type(message_type: &str) -> &'static str {
    match message_type.trim().to_ascii_lowercase().as_str() {
        "attachment_manifest" => attachment_manifest_content_type(),
        "event" => "application/json",
        _ => "text/plain",
    }
}

pub(crate) fn now_rfc3339() -> String {
    im_core::compat::wire::now_rfc3339()
}

pub(crate) fn generate_operation_id() -> String {
    im_core::compat::wire::generate_operation_id()
}

fn compat_inbox(request: InboxRequest) -> im_core::compat::wire::InboxWireRequest {
    im_core::compat::wire::InboxWireRequest {
        limit: request.limit,
    }
}

fn wire_identity(record: &StoredIdentity) -> im_core::compat::wire::WireIdentity {
    im_core::compat::wire::WireIdentity {
        did: record.did.clone(),
    }
}

fn direct_wire_error(err: im_core::ImError) -> MessageError {
    match err {
        im_core::ImError::InvalidInput { field, message }
            if field.as_deref() == Some("text") && message == "message text is required" =>
        {
            MessageError::TextRequired
        }
        im_core::ImError::InvalidInput { message, .. } => MessageError::Json(message),
        err => MessageError::Json(err.to_string()),
    }
}

fn wire_error(err: im_core::ImError) -> MessageError {
    match err {
        im_core::ImError::InvalidInput { field, message }
            if field.as_deref() == Some("message_ids")
                && message == "message not found: message_ids are required" =>
        {
            MessageError::Json(message)
        }
        im_core::ImError::InvalidInput { message, .. } => MessageError::Json(message),
        err => MessageError::Json(err.to_string()),
    }
}
