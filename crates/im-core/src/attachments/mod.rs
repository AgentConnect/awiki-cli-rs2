mod dto;
mod service;

pub use self::dto::{
    AttachmentDestination, AttachmentInput, AttachmentSendRequest, DownloadAttachmentRequest,
    DownloadedAttachment, DownloadedAttachmentDestination,
};
pub use self::service::AttachmentService;
