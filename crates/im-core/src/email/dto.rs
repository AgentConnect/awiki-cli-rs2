use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EmailMessageId(String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EmailFolder(String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EmailAddress(String);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmailInboxQuery {
    pub folder: EmailFolder,
    pub limit: crate::ids::PageLimit,
    pub offset: u32,
    pub unread_only: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmailMarkReadRequest {
    pub message_ids: Vec<EmailMessageId>,
    pub is_read: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SendEmailRequest {
    pub to: Vec<EmailAddress>,
    pub cc: Vec<EmailAddress>,
    pub subject: String,
    pub body_text: String,
    pub body_html: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmailAttachmentDownloadRequest {
    pub message_id: EmailMessageId,
    pub attachment_index: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmailNotificationQuery {
    pub limit: crate::ids::PageLimit,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmailAccount {
    pub mailbox_address: Option<EmailAddress>,
    pub display_name: Option<String>,
    pub status: Option<String>,
    pub attributes: Vec<EmailAttribute>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmailMessageSummary {
    pub id: EmailMessageId,
    pub folder: Option<EmailFolder>,
    pub from: Vec<EmailAddress>,
    pub to: Vec<EmailAddress>,
    pub cc: Vec<EmailAddress>,
    pub subject: String,
    pub preview: Option<String>,
    pub received_at: Option<String>,
    pub sent_at: Option<String>,
    pub unread: bool,
    pub has_attachments: bool,
    pub attachment_count: Option<u32>,
    pub attributes: Vec<EmailAttribute>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmailMessage {
    pub summary: EmailMessageSummary,
    pub body_text: Option<String>,
    pub body_html: Option<String>,
    pub attachments: Vec<EmailAttachmentMetadata>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmailAttachmentMetadata {
    pub index: u32,
    pub filename: Option<String>,
    pub content_type: Option<String>,
    pub size: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmailAttachmentContent {
    pub message_id: EmailMessageId,
    pub attachment_index: u32,
    pub filename: String,
    pub content_type: String,
    pub size: Option<u64>,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmailMarkReadResult {
    pub updated: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SendEmailResult {
    pub accepted: bool,
    pub message_id: Option<EmailMessageId>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmailNotification {
    pub id: crate::ids::MessageId,
    pub mailbox_address: Option<EmailAddress>,
    pub from_addr: Option<String>,
    pub subject: String,
    pub preview: Option<String>,
    pub has_attachments: bool,
    pub received_at: Option<String>,
    pub attributes: Vec<EmailAttribute>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmailAttribute {
    pub key: String,
    pub value: String,
}

impl EmailMessageId {
    pub fn parse(input: impl AsRef<str>) -> crate::ImResult<Self> {
        parse_non_empty("message_id", input).map(Self)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl EmailFolder {
    pub fn inbox() -> Self {
        Self("inbox".to_string())
    }

    pub fn parse(input: impl AsRef<str>) -> crate::ImResult<Self> {
        parse_non_empty("folder", input).map(Self)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl EmailAddress {
    pub fn parse(input: impl AsRef<str>) -> crate::ImResult<Self> {
        let value = parse_non_empty("email_address", input)?;
        if !value.contains('@') {
            return Err(crate::ImError::invalid_input(
                Some("email_address".to_string()),
                "email address must contain @",
            ));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for EmailInboxQuery {
    fn default() -> Self {
        Self {
            folder: EmailFolder::inbox(),
            limit: crate::ids::PageLimit(20),
            offset: 0,
            unread_only: false,
        }
    }
}

impl Default for EmailNotificationQuery {
    fn default() -> Self {
        Self {
            limit: crate::ids::PageLimit(20),
        }
    }
}

fn parse_non_empty(field: &'static str, input: impl AsRef<str>) -> crate::ImResult<String> {
    let value = input.as_ref().trim();
    if value.is_empty() {
        return Err(crate::ImError::invalid_input(
            Some(field.to_string()),
            format!("{field} must not be empty"),
        ));
    }
    Ok(value.to_string())
}
