use crate::identity::types::StoredIdentity;
use crate::message::types::MessageError;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::fs;
use std::path::Path;

pub fn attachment_manifest_content_type() -> &'static str {
    im_core::compat::attachments::attachment_manifest_content_type()
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
    let prepared = im_core::compat::attachments::prepare_attachment_payload_from_path(
        Path::new(path),
        mime_override,
        payload,
    )
    .map_err(attachment_core_error)?;
    Ok(PreparedAttachment {
        file_path: path.to_string(),
        filename: prepared.filename,
        mime_type: prepared.mime_type,
        size_bytes: i64::try_from(prepared.size_bytes).unwrap_or(i64::MAX),
        size_string: prepared.size_string,
        digest_b64u: prepared.digest_b64u,
        payload: prepared.payload,
    })
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
    im_core::compat::attachments::build_attachment_create_slot_rpc_params(
        &record.did,
        service_did,
        target_kind,
        target_did,
        &core_prepared_attachment(prepared),
    )
    .map_err(attachment_core_error)
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
    im_core::compat::attachments::build_attachment_commit_object_rpc_params(
        &record.did,
        service_did,
        &core_prepared_attachment(prepared),
        &core_create_slot_result(slot),
    )
    .map_err(attachment_core_error)
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
    im_core::compat::attachments::build_attachment_download_ticket_rpc_params(
        &record.did,
        service_did,
        sender_did,
        message_id,
        group_did,
        &core_attachment_selection(selection),
    )
    .map_err(attachment_core_error)
}

pub fn build_attachment_manifest(
    prepared: &PreparedAttachment,
    slot: &AttachmentCreateSlotResult,
    caption: &str,
) -> Value {
    let descriptor = im_core::compat::attachments::AttachmentDescriptor {
        attachment_id: slot.attachment_id.clone(),
        filename: prepared.filename.clone(),
        mime_type: prepared.mime_type.clone(),
        size: prepared.size_string.clone(),
        digest_b64u: prepared.digest_b64u.clone(),
        object_uri: slot.object_uri.clone(),
        encryption_mode: "none".to_string(),
    };
    im_core::compat::attachments::build_attachment_manifest(&descriptor, caption)
}

pub fn build_direct_attachment_send_rpc_params(
    record: &StoredIdentity,
    target_did: &str,
    manifest: Value,
) -> Result<Value, MessageError> {
    if target_did.trim().is_empty() {
        return Err(MessageError::TargetRequired);
    }
    im_core::compat::attachments::build_direct_attachment_send_rpc_params(
        &core_signing_identity(record),
        target_did,
        manifest,
    )
    .map_err(attachment_core_error)
}

pub fn build_group_attachment_send_rpc_params(
    record: &StoredIdentity,
    group_did: &str,
    manifest: Value,
) -> Result<Value, MessageError> {
    if group_did.trim().is_empty() {
        return Err(MessageError::GroupRequired);
    }
    im_core::compat::attachments::build_group_attachment_send_rpc_params(
        &core_signing_identity(record),
        group_did,
        manifest,
    )
    .map_err(attachment_core_error)
}

pub fn manifest_content_string(manifest: &Value) -> String {
    im_core::compat::attachments::manifest_content_string(manifest)
}

pub fn find_attachment_selection(
    messages: &[Value],
    requested_message_id: &str,
    requested_attachment_id: &str,
) -> Result<AttachmentSelection, MessageError> {
    im_core::compat::attachments::find_attachment_selection(
        messages,
        requested_message_id,
        requested_attachment_id,
    )
    .map(legacy_attachment_selection)
    .map_err(attachment_core_error)
}

pub(crate) fn find_attachment_selection_with_paging<F>(
    mut fetch_page: F,
    requested_message_id: &str,
    requested_attachment_id: &str,
) -> Result<AttachmentSelection, MessageError>
where
    F: FnMut(i64) -> Result<(Vec<Value>, bool), MessageError>,
{
    let mut fetch_error = None;
    let result = im_core::compat::attachments::find_attachment_selection_with_paging(
        |skip| match fetch_page(skip) {
            Ok(page) => Ok(page),
            Err(err) => {
                fetch_error = Some(err);
                Err(im_core::ImError::Internal {
                    message: "attachment page fetch failed".to_string(),
                })
            }
        },
        requested_message_id,
        requested_attachment_id,
    );
    if let Some(err) = fetch_error {
        return Err(err);
    }
    result
        .map(legacy_attachment_selection)
        .map_err(attachment_core_error)
}

fn legacy_attachment_selection(
    selection: im_core::compat::attachments::AttachmentSelection,
) -> AttachmentSelection {
    AttachmentSelection {
        message_id: selection.message_id,
        requested_id: selection.requested_id,
        sender_did: selection.sender_did,
        attachment_id: selection.attachment_id,
        filename: selection.filename,
        mime_type: selection.mime_type,
        size: selection.size,
        digest_b64u: selection.digest_b64u,
        object_uri: selection.object_uri,
        caption: selection.caption,
    }
}

fn core_prepared_attachment(
    prepared: &PreparedAttachment,
) -> im_core::compat::attachments::PreparedAttachment {
    im_core::compat::attachments::PreparedAttachment {
        filename: prepared.filename.clone(),
        mime_type: prepared.mime_type.clone(),
        size_bytes: u64::try_from(prepared.size_bytes).unwrap_or_default(),
        size_string: prepared.size_string.clone(),
        digest_b64u: prepared.digest_b64u.clone(),
        payload: prepared.payload.clone(),
    }
}

fn core_create_slot_result(
    slot: &AttachmentCreateSlotResult,
) -> im_core::compat::attachments::AttachmentCreateSlotResult {
    im_core::compat::attachments::AttachmentCreateSlotResult {
        attachment_id: slot.attachment_id.clone(),
        slot_id: slot.slot_id.clone(),
        upload_uri: slot.upload_uri.clone(),
        upload_headers: slot.upload_headers.clone(),
        object_uri: slot.object_uri.clone(),
        commit_token: slot.commit_token.clone(),
        expires_at: slot.expires_at.clone(),
        request_service_did: slot.request_service_did.clone(),
    }
}

fn core_attachment_selection(
    selection: &AttachmentSelection,
) -> im_core::compat::attachments::AttachmentSelection {
    im_core::compat::attachments::AttachmentSelection {
        message_id: selection.message_id.clone(),
        requested_id: selection.requested_id.clone(),
        sender_did: selection.sender_did.clone(),
        attachment_id: selection.attachment_id.clone(),
        filename: selection.filename.clone(),
        mime_type: selection.mime_type.clone(),
        size: selection.size.clone(),
        digest_b64u: selection.digest_b64u.clone(),
        object_uri: selection.object_uri.clone(),
        caption: selection.caption.clone(),
    }
}

fn core_signing_identity(
    record: &StoredIdentity,
) -> im_core::compat::attachments::AttachmentSigningIdentity {
    im_core::compat::attachments::AttachmentSigningIdentity {
        identity_name: record.identity_name.clone(),
        did: record.did.clone(),
        did_document: record.did_document.clone(),
        key1_private_pem: record.key1_private_pem.clone(),
    }
}

fn attachment_core_error(err: im_core::ImError) -> MessageError {
    match err {
        im_core::ImError::MessageNotFound { .. } => MessageError::MessageNotFound,
        im_core::ImError::Serialization { detail } => MessageError::Json(detail),
        im_core::ImError::InvalidInput { field, message } => {
            if message == im_core::compat::attachments::ERR_ATTACHMENT_NOT_FOUND {
                MessageError::AttachmentNotFound
            } else if message == im_core::compat::attachments::ERR_ATTACHMENT_ID_REQUIRED {
                MessageError::AttachmentIdRequired
            } else if message == im_core::compat::attachments::ERR_ATTACHMENT_MESSAGE_INVALID {
                MessageError::AttachmentMessageInvalid
            } else if field.as_deref() == Some("file_path") {
                MessageError::Internal(message)
            } else if field.as_deref() == Some("service_did")
                && message == "message service did is required"
            {
                MessageError::MissingMessageServiceDid
            } else if field.as_deref() == Some("service_did")
                && message == "attachment service did is required"
            {
                MessageError::MissingAttachmentServiceDid
            } else if field.as_deref() == Some("sender_did") {
                MessageError::AttachmentSenderRequired
            } else if field.as_deref() == Some("target") || field.as_deref() == Some("target_did") {
                MessageError::TargetRequired
            } else if field.as_deref() == Some("group_did") {
                MessageError::GroupRequired
            } else {
                MessageError::Internal(message)
            }
        }
        im_core::ImError::Io { detail } => MessageError::Internal(detail),
        err => MessageError::Internal(err.to_string()),
    }
}
