mod dto;
mod service;

use std::sync::LazyLock;

static UNSAFE_ATTACHMENT_FILENAME: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"[\p{C}\u{061c}\u{200e}\u{200f}\u{202a}-\u{202e}\u{2066}-\u{2069}]")
        .expect("attachment filename Unicode policy must compile")
});

pub const EMAIL_ATTACHMENT_MAX_COUNT: usize = 10;
pub const EMAIL_ATTACHMENT_MAX_BYTES: usize = 10 * 1024 * 1024;
pub const EMAIL_ATTACHMENT_TOTAL_MAX_BYTES: usize = 18 * 1024 * 1024;

/// Return whether an external mail attachment filename is one bounded basename.
///
/// Unicode category C code points and bidi controls are rejected uniformly at
/// the Core wire, Node facade, and CLI filesystem boundaries.
pub fn valid_attachment_filename(value: &str) -> bool {
    !value.is_empty()
        && value.trim() == value
        && value.trim_end_matches(['.', ' ']) == value
        && value != "."
        && value != ".."
        && value.len() <= 255
        && !value.contains(['/', '\\'])
        && !UNSAFE_ATTACHMENT_FILENAME.is_match(value)
}

pub use self::dto::{
    EmailAccount, EmailAddress, EmailAttachmentContent, EmailAttachmentDownloadRequest,
    EmailAttachmentInput, EmailAttachmentMetadata, EmailAttribute, EmailFolder, EmailInboxQuery,
    EmailMarkReadRequest, EmailMarkReadResult, EmailMessage, EmailMessageId, EmailMessageSummary,
    EmailNotification, EmailNotificationQuery, SendEmailRequest, SendEmailResult,
    SendEmailWithAttachmentsRequest,
};
pub use self::service::EmailService;
