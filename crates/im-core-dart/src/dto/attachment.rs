use crate::dto::message::{
    DartMessageSecurityMode, DartMessageTarget, DartSendMessageResult, DartThreadRef,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DartAttachmentInput {
    LocalFile {
        path: String,
    },
    Bytes {
        filename: Option<String>,
        mime_type: Option<String>,
        bytes: Vec<u8>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartAttachmentSendRequest {
    pub target: DartMessageTarget,
    pub input: DartAttachmentInput,
    pub caption: Option<String>,
    pub mime_type: Option<String>,
    pub filename: Option<String>,
    pub security: DartMessageSecurityMode,
    pub idempotency_key: Option<String>,
    pub wait_for_final_acceptance: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartAttachmentSendResult {
    pub message: DartSendMessageResult,
    pub target_kind: String,
    pub target_did: String,
    pub attachment: DartUploadedAttachment,
    pub manifest_json: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartUploadedAttachment {
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DartAttachmentDestination {
    LocalFile { path: String },
    Memory,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartDownloadAttachmentRequest {
    pub thread: DartThreadRef,
    pub message_id: String,
    pub attachment_id: Option<String>,
    pub destination: DartAttachmentDestination,
    pub overwrite: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartDownloadedAttachment {
    pub attachment_id: String,
    pub filename: Option<String>,
    pub mime_type: Option<String>,
    pub size_bytes: Option<u64>,
    pub destination: DartDownloadedAttachmentDestination,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DartDownloadedAttachmentDestination {
    LocalFile { path: String },
    Memory { bytes: Vec<u8> },
}
