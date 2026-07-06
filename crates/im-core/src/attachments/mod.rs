mod dto;
pub(crate) mod manifest;
pub(crate) mod selection;
mod service;

pub use self::dto::{
    AttachmentDestination, AttachmentInput, AttachmentSendRequest, AttachmentSendResult,
    DownloadAttachmentRequest, DownloadedAttachment, DownloadedAttachmentDestination,
    SendConversationAttachmentRequest, UploadedAttachment,
};
pub use self::manifest::{
    attachment_manifest_content_type, manifest_content_string, AttachmentDescriptor,
    AttachmentManifest, PreparedAttachment,
};
pub use self::selection::{
    AttachmentSelection, ERR_ATTACHMENT_ID_REQUIRED, ERR_ATTACHMENT_MESSAGE_INVALID,
    ERR_ATTACHMENT_NOT_FOUND,
};
pub use self::service::AttachmentService;
