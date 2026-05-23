mod dto;
pub(crate) mod manifest;
pub(crate) mod selection;
mod service;

pub use self::dto::{
    AttachmentDestination, AttachmentInput, AttachmentSendRequest, DownloadAttachmentRequest,
    DownloadedAttachment, DownloadedAttachmentDestination,
};
pub use self::service::AttachmentService;
