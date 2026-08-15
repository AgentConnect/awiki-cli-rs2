use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttachmentSendRequest {
    pub input: AttachmentInput,
    pub caption: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mention_payload: Option<Value>,
    pub mime_type: Option<String>,
    pub filename: Option<String>,
    pub delivery: crate::messages::MessageDeliveryOptions,
    pub security: crate::messages::MessageSecurityMode,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SendConversationAttachmentRequest {
    pub conversation: crate::messages::ConversationReadRef,
    pub input: AttachmentInput,
    pub caption: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mention_payload: Option<Value>,
    pub mime_type: Option<String>,
    pub filename: Option<String>,
    pub security: crate::messages::MessageSecurityMode,
    pub client_message_id: Option<crate::ids::MessageId>,
    pub idempotency_key: Option<String>,
    pub wait_for_final_acceptance: bool,
}

impl SendConversationAttachmentRequest {
    pub(crate) fn into_attachment_send_request(
        self,
    ) -> (
        crate::messages::ConversationReadRef,
        Option<crate::ids::MessageId>,
        AttachmentSendRequest,
    ) {
        (
            self.conversation,
            self.client_message_id,
            AttachmentSendRequest {
                input: self.input,
                caption: self.caption,
                mention_payload: self.mention_payload,
                mime_type: self.mime_type,
                filename: self.filename,
                delivery: crate::messages::MessageDeliveryOptions {
                    idempotency_key: self.idempotency_key,
                    wait_for_final_acceptance: self.wait_for_final_acceptance,
                },
                security: self.security,
            },
        )
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AttachmentSendResult {
    pub message: crate::messages::SendMessageResult,
    pub target_kind: String,
    pub target_did: String,
    pub attachment: UploadedAttachment,
    pub manifest: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UploadedAttachment {
    pub attachment_id: String,
    pub filename: String,
    pub mime_type: String,
    pub size_bytes: u64,
    pub size: String,
    pub digest_b64u: String,
    pub object_uri: String,
    pub object_encryption_mode: String,
    pub plaintext_size_bytes: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AttachmentInput {
    LocalFile(PathBuf),
    Bytes {
        filename: Option<String>,
        mime_type: Option<String>,
        bytes: Vec<u8>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DownloadAttachmentRequest {
    pub thread: crate::messages::ThreadRef,
    pub message_id: crate::ids::MessageId,
    pub attachment_id: Option<String>,
    pub destination: AttachmentDestination,
    pub overwrite: bool,
}

/// Canonical-conversation variant of [`DownloadAttachmentRequest`].
///
/// Hosts that already hold a conversation ID must use this input instead of
/// rebuilding a Direct or Group `ThreadRef` from display metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DownloadConversationAttachmentRequest {
    pub conversation: crate::messages::ConversationReadRef,
    pub message_id: crate::ids::MessageId,
    pub attachment_id: Option<String>,
    pub destination: AttachmentDestination,
    pub overwrite: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AttachmentDestination {
    LocalFile(PathBuf),
    Memory,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DownloadedAttachment {
    pub attachment_id: String,
    pub filename: Option<String>,
    pub mime_type: Option<String>,
    pub size_bytes: Option<u64>,
    pub destination: DownloadedAttachmentDestination,
    pub selection: Option<crate::attachments::AttachmentSelection>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DownloadedAttachmentDestination {
    LocalFile(PathBuf),
    Memory(Vec<u8>),
}

impl AttachmentSendResult {
    pub(crate) fn from_upload_result(
        result: crate::internal::attachment_runtime::upload::AttachmentUploadResult,
    ) -> Self {
        let object_encryption_mode = result.prepared.object_encryption_mode();
        let plaintext_size_bytes = result.prepared.plaintext_size_bytes;
        Self {
            message: result.sdk_result,
            target_kind: result.target_kind.to_string(),
            target_did: result.target_did,
            attachment: UploadedAttachment {
                attachment_id: result.slot.attachment_id,
                filename: result.prepared.filename,
                mime_type: result.prepared.mime_type,
                size_bytes: result.prepared.size_bytes,
                size: result.prepared.size_string,
                digest_b64u: result.prepared.digest_b64u,
                object_uri: result.slot.object_uri,
                object_encryption_mode,
                plaintext_size_bytes,
            },
            manifest: result.manifest,
        }
    }
}
