use crate::identity::types::StoredIdentity;
use crate::message::proof::{build_origin_proof, origin_auth_value};
use crate::message::types::MessageError;
use crate::message::wire::{message_meta, signed_message_meta};
use crate::message::DirectPayload;
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;

const ATTACHMENT_MANIFEST_CONTENT_TYPE: &str = "application/anp-attachment-manifest+json";

pub fn attachment_manifest_content_type() -> &'static str {
    ATTACHMENT_MANIFEST_CONTENT_TYPE
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PreparedAttachment {
    pub file_path: String,
    pub filename: String,
    pub mime_type: String,
    pub size_bytes: i64,
    pub size_string: String,
    pub digest_b64u: String,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttachmentCreateSlotResult {
    pub attachment_id: String,
    pub slot_id: String,
    pub upload_uri: String,
    pub upload_headers: Map<String, Value>,
    pub object_uri: String,
    pub commit_token: String,
    pub expires_at: String,
    #[serde(skip)]
    pub request_service_did: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttachmentCommitObjectResult {
    pub committed: bool,
    pub attachment_id: String,
    pub object_uri: String,
    pub committed_at: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttachmentDownloadTicketResult {
    #[serde(default)]
    pub download_ticket_b64u: String,
    #[serde(default)]
    pub expires_at: String,
    #[serde(default)]
    pub ticket_binding: Map<String, Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AttachmentSelection {
    pub message_id: String,
    pub requested_id: String,
    pub sender_did: String,
    pub attachment_id: String,
    pub filename: String,
    pub mime_type: String,
    pub size: String,
    pub digest_b64u: String,
    pub object_uri: String,
    pub caption: String,
}

pub fn load_attachment_file(
    file_path: &str,
    mime_override: &str,
) -> Result<PreparedAttachment, MessageError> {
    let path = file_path.trim();
    if path.is_empty() {
        return Err(MessageError::FilePathRequired);
    }
    let payload =
        fs::read(Path::new(path)).map_err(|err| MessageError::Internal(err.to_string()))?;
    let filename = Path::new(path)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_string();
    if filename.is_empty() {
        return Err(MessageError::Internal(format!(
            "attachment filename could not be derived from {path:?}"
        )));
    }
    let mime_type = if mime_override.trim().is_empty() {
        detect_attachment_mime_type(&filename, &payload)
    } else {
        mime_override.trim().to_string()
    };
    let digest = Sha256::digest(&payload);
    Ok(PreparedAttachment {
        file_path: path.to_string(),
        filename,
        mime_type,
        size_bytes: payload.len() as i64,
        size_string: payload.len().to_string(),
        digest_b64u: base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest),
        payload,
    })
}

fn detect_attachment_mime_type(filename: &str, payload: &[u8]) -> String {
    let ext = Path::new(filename)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if let Some(mime) = mime_type_by_extension(&ext) {
        return mime.to_string();
    }
    detect_attachment_content_type(payload)
}

fn mime_type_by_extension(ext: &str) -> Option<&'static str> {
    match ext {
        "txt" | "text" | "log" | "md" | "csv" => Some("text/plain; charset=utf-8"),
        "json" => Some("application/json"),
        "html" | "htm" => Some("text/html; charset=utf-8"),
        "css" => Some("text/css; charset=utf-8"),
        "js" | "mjs" => Some("text/javascript; charset=utf-8"),
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        "pdf" => Some("application/pdf"),
        "xml" => Some("text/xml; charset=utf-8"),
        "zip" => Some("application/zip"),
        "gz" => Some("application/gzip"),
        "tar" => Some("application/x-tar"),
        "wasm" => Some("application/wasm"),
        _ => None,
    }
}

fn detect_attachment_content_type(payload: &[u8]) -> String {
    if payload.is_empty() {
        return "application/octet-stream".to_string();
    }
    let sample = &payload[..payload.len().min(512)];
    if sample.starts_with(b"\x89PNG\r\n\x1a\n") {
        return "image/png".to_string();
    }
    if sample.starts_with(b"\xff\xd8\xff") {
        return "image/jpeg".to_string();
    }
    if sample.starts_with(b"GIF87a") || sample.starts_with(b"GIF89a") {
        return "image/gif".to_string();
    }
    if sample.starts_with(b"%PDF-") {
        return "application/pdf".to_string();
    }
    if sample.starts_with(b"PK\x03\x04") {
        return "application/zip".to_string();
    }
    if sample
        .iter()
        .all(|byte| matches!(*byte, b'\t' | b'\n' | b'\r' | 0x20..=0x7e))
    {
        return "text/plain; charset=utf-8".to_string();
    }
    "application/octet-stream".to_string()
}

pub fn build_attachment_create_slot_rpc_params(
    record: &StoredIdentity,
    service_did: &str,
    target_kind: &str,
    target_did: &str,
    prepared: &PreparedAttachment,
) -> Result<Value, MessageError> {
    if prepared.filename.trim().is_empty() && prepared.size_string.trim().is_empty() {
        return Err(MessageError::FilePathRequired);
    }
    if service_did.trim().is_empty() {
        return Err(MessageError::MissingMessageServiceDid);
    }
    if target_kind.trim().is_empty() || target_did.trim().is_empty() {
        return Err(MessageError::TargetRequired);
    }
    Ok(json!({
        "meta": message_meta(&record.did, service_did, "anp.attachment.v1"),
        "body": {
            "expected_size": prepared.size_string,
            "expected_digest": {
                "alg": "sha-256",
                "value_b64u": prepared.digest_b64u,
            },
            "mime_type": prepared.mime_type,
            "filename": prepared.filename,
            "intended_message_security_profile": "transport-protected",
            "intended_target": {
                "kind": target_kind,
                "did": target_did,
            },
            "object_encryption_mode": "none",
        },
    }))
}

pub fn build_attachment_commit_object_rpc_params(
    record: &StoredIdentity,
    service_did: &str,
    prepared: &PreparedAttachment,
    slot: &AttachmentCreateSlotResult,
) -> Result<Value, MessageError> {
    if prepared.filename.trim().is_empty() || slot.attachment_id.trim().is_empty() {
        return Err(MessageError::FilePathRequired);
    }
    Ok(json!({
        "meta": message_meta(&record.did, service_did, "anp.attachment.v1"),
        "body": {
            "attachment_id": slot.attachment_id,
            "slot_id": slot.slot_id,
            "commit_token": slot.commit_token,
            "size": prepared.size_string,
            "object_encryption_mode": "none",
            "digest": {
                "alg": "sha-256",
                "value_b64u": prepared.digest_b64u,
            },
        },
    }))
}

pub fn build_attachment_download_ticket_rpc_params(
    record: &StoredIdentity,
    service_did: &str,
    sender_did: &str,
    message_id: &str,
    group_did: &str,
    selection: &AttachmentSelection,
) -> Result<Value, MessageError> {
    if selection.attachment_id.trim().is_empty() {
        return Err(MessageError::AttachmentNotFound);
    }
    if service_did.trim().is_empty() {
        return Err(MessageError::MissingAttachmentServiceDid);
    }
    if sender_did.trim().is_empty() {
        return Err(MessageError::AttachmentSenderRequired);
    }
    let mut body = Map::new();
    body.insert(
        "attachment_id".to_string(),
        Value::String(selection.attachment_id.clone()),
    );
    body.insert(
        "object_uri".to_string(),
        Value::String(selection.object_uri.clone()),
    );
    body.insert(
        "sender_did".to_string(),
        Value::String(sender_did.to_string()),
    );
    body.insert(
        "requester_did".to_string(),
        Value::String(record.did.clone()),
    );
    body.insert(
        "message_security_profile".to_string(),
        Value::String("transport-protected".to_string()),
    );
    body.insert(
        "message_id".to_string(),
        Value::String(message_id.to_string()),
    );
    body.insert("one_time".to_string(), Value::Bool(true));
    if !group_did.trim().is_empty() {
        body.insert(
            "group_did".to_string(),
            Value::String(group_did.to_string()),
        );
    } else {
        body.insert(
            "message_target_did".to_string(),
            Value::String(record.did.clone()),
        );
    }
    Ok(json!({
        "meta": message_meta(&record.did, service_did, "anp.attachment.v1"),
        "body": body,
    }))
}

pub fn build_attachment_manifest(
    prepared: &PreparedAttachment,
    slot: &AttachmentCreateSlotResult,
    caption: &str,
) -> Value {
    let mut manifest = Map::new();
    manifest.insert(
        "attachments".to_string(),
        json!([{
            "attachment_id": slot.attachment_id,
            "filename": prepared.filename,
            "mime_type": prepared.mime_type,
            "size": prepared.size_string,
            "digest": {
                "alg": "sha-256",
                "value_b64u": prepared.digest_b64u,
            },
            "access_info": {
                "object_uri": slot.object_uri,
            },
            "encryption_info": {
                "mode": "none",
            },
        }]),
    );
    manifest.insert(
        "primary_attachment_id".to_string(),
        Value::String(slot.attachment_id.clone()),
    );
    if !caption.trim().is_empty() {
        manifest.insert("caption".to_string(), Value::String(caption.to_string()));
    }
    Value::Object(manifest)
}

pub fn build_direct_attachment_send_rpc_params(
    record: &StoredIdentity,
    target_did: &str,
    manifest: Value,
) -> Result<Value, MessageError> {
    if target_did.trim().is_empty() {
        return Err(MessageError::TargetRequired);
    }
    build_signed_attachment_send_rpc_params(
        record,
        "direct.send",
        "agent",
        target_did,
        "anp.direct.base.v1",
        manifest,
    )
}

pub fn build_group_attachment_send_rpc_params(
    record: &StoredIdentity,
    group_did: &str,
    manifest: Value,
) -> Result<Value, MessageError> {
    if group_did.trim().is_empty() {
        return Err(MessageError::GroupRequired);
    }
    build_signed_attachment_send_rpc_params(
        record,
        "group.send",
        "group",
        group_did,
        "anp.group.base.v1",
        manifest,
    )
}

pub fn manifest_content_string(manifest: &Value) -> String {
    serde_json::to_string(manifest).unwrap_or_default()
}

fn build_signed_attachment_send_rpc_params(
    record: &StoredIdentity,
    method: &str,
    target_kind: &str,
    target_did: &str,
    profile: &str,
    manifest: Value,
) -> Result<Value, MessageError> {
    let body = json!({ "payload": manifest });
    let payload = DirectPayload {
        method: method.to_string(),
        meta: signed_message_meta(
            &record.did,
            target_kind,
            target_did,
            profile,
            attachment_manifest_content_type(),
        ),
        body,
    };
    let origin_proof = build_origin_proof(record, &payload)?;
    Ok(json!({
        "meta": payload.meta,
        "auth": origin_auth_value(&origin_proof),
        "body": payload.body,
    }))
}

pub fn find_attachment_selection(
    messages: &[Value],
    requested_message_id: &str,
    requested_attachment_id: &str,
) -> Result<AttachmentSelection, MessageError> {
    for message in messages {
        let Some(message_object) = message.as_object() else {
            continue;
        };
        let view_id = string_from_value(message_object.get("id"));
        let raw_message_id = string_from_value(message_object.get("message_id"));
        let actual_message_id = if raw_message_id.is_empty() {
            view_id.clone()
        } else {
            raw_message_id
        };
        if requested_message_id != view_id && requested_message_id != actual_message_id {
            continue;
        }
        let content = decode_attachment_content(message_object.get("content"))?;
        let attachments = attachments_from_content(content.get("attachments"));
        if attachments.is_empty() {
            return Err(MessageError::AttachmentMessageInvalid);
        }
        let selected = select_attachment_entry(&attachments, requested_attachment_id)?;
        let access_info = selected
            .get("access_info")
            .and_then(Value::as_object)
            .ok_or(MessageError::AttachmentMessageInvalid)?;
        let digest = selected.get("digest").and_then(Value::as_object);
        return Ok(AttachmentSelection {
            message_id: actual_message_id,
            requested_id: view_id,
            sender_did: string_from_value(message_object.get("sender_did")),
            attachment_id: string_from_value(selected.get("attachment_id")),
            filename: string_from_value(selected.get("filename")),
            mime_type: string_from_value(selected.get("mime_type")),
            size: string_from_value(selected.get("size")),
            digest_b64u: digest
                .and_then(|value| value.get("value_b64u"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            object_uri: access_info
                .get("object_uri")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            caption: string_from_value(content.get("caption")),
        });
    }
    Err(MessageError::MessageNotFound)
}

pub(crate) fn find_attachment_selection_with_paging<F>(
    mut fetch_page: F,
    requested_message_id: &str,
    requested_attachment_id: &str,
) -> Result<AttachmentSelection, MessageError>
where
    F: FnMut(i64) -> Result<(Vec<Value>, bool), MessageError>,
{
    let mut skip = 0_i64;
    loop {
        let (messages, has_more) = fetch_page(skip)?;
        match find_attachment_selection(&messages, requested_message_id, requested_attachment_id) {
            Ok(selection) => return Ok(selection),
            Err(MessageError::MessageNotFound) if has_more && !messages.is_empty() => {
                skip += messages.len() as i64;
            }
            Err(MessageError::MessageNotFound) => return Err(MessageError::MessageNotFound),
            Err(err) => return Err(err),
        }
    }
}

fn decode_attachment_content(value: Option<&Value>) -> Result<Map<String, Value>, MessageError> {
    match value {
        Some(Value::Object(object)) if !object.is_empty() => Ok(object.clone()),
        Some(Value::String(text)) if !text.trim().is_empty() => {
            let decoded: Value =
                serde_json::from_str(text).map_err(|err| MessageError::Json(err.to_string()))?;
            decoded
                .as_object()
                .filter(|object| !object.is_empty())
                .cloned()
                .ok_or(MessageError::AttachmentMessageInvalid)
        }
        _ => Err(MessageError::AttachmentMessageInvalid),
    }
}

fn attachments_from_content(value: Option<&Value>) -> Vec<Map<String, Value>> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_object().cloned())
                .collect()
        })
        .unwrap_or_default()
}

fn select_attachment_entry(
    attachments: &[Map<String, Value>],
    requested_attachment_id: &str,
) -> Result<Map<String, Value>, MessageError> {
    if attachments.is_empty() {
        return Err(MessageError::AttachmentNotFound);
    }
    if requested_attachment_id.trim().is_empty() {
        if attachments.len() > 1 {
            return Err(MessageError::AttachmentIdRequired);
        }
        return Ok(attachments[0].clone());
    }
    attachments
        .iter()
        .find(|attachment| {
            string_from_value(attachment.get("attachment_id")) == requested_attachment_id
        })
        .cloned()
        .ok_or(MessageError::AttachmentNotFound)
}

fn string_from_value(value: Option<&Value>) -> String {
    value
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}
