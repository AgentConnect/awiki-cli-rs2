mod dto;
mod service;

pub use self::dto::{
    EmailAccount, EmailAddress, EmailAttachmentContent, EmailAttachmentDownloadRequest,
    EmailAttachmentMetadata, EmailAttribute, EmailFolder, EmailInboxQuery, EmailMarkReadRequest,
    EmailMarkReadResult, EmailMessage, EmailMessageId, EmailMessageSummary, EmailNotification,
    EmailNotificationQuery, SendEmailRequest, SendEmailResult,
};
pub use self::service::EmailService;
