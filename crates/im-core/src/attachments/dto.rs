use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttachmentSendRequest {
    pub input: AttachmentInput,
    pub caption: Option<String>,
    pub mime_type: Option<String>,
    pub filename: Option<String>,
    pub delivery: crate::messages::MessageDeliveryOptions,
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
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DownloadedAttachmentDestination {
    LocalFile(PathBuf),
    Memory(Vec<u8>),
}
