#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartEmailAttribute {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartEmailAccount {
    pub mailbox_address: Option<String>,
    pub display_name: Option<String>,
    pub status: Option<String>,
    pub attributes: Vec<DartEmailAttribute>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartEmailMessageSummary {
    pub id: String,
    pub folder: Option<String>,
    pub from: Vec<String>,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub subject: String,
    pub preview: Option<String>,
    pub received_at: Option<String>,
    pub sent_at: Option<String>,
    pub unread: bool,
    pub has_attachments: bool,
    pub attachment_count: Option<u32>,
    pub attributes: Vec<DartEmailAttribute>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartEmailMessageSummaryPage {
    pub items: Vec<DartEmailMessageSummary>,
    pub next_cursor: Option<String>,
    pub has_more: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartEmailMessage {
    pub summary: DartEmailMessageSummary,
    pub body_text: Option<String>,
    pub body_html: Option<String>,
    pub attachments: Vec<DartEmailAttachmentMetadata>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartEmailAttachmentMetadata {
    pub index: u32,
    pub filename: Option<String>,
    pub content_type: Option<String>,
    pub size: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartEmailAttachmentContent {
    pub message_id: String,
    pub attachment_index: u32,
    pub filename: String,
    pub content_type: String,
    pub size: Option<u64>,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartEmailMarkReadResult {
    pub updated: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartSendEmailRequest {
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub subject: String,
    pub body_text: String,
    pub body_html: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartSendEmailResult {
    pub accepted: bool,
    pub message_id: Option<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartEmailNotification {
    pub id: String,
    pub mailbox_address: Option<String>,
    pub from_addr: Option<String>,
    pub subject: String,
    pub preview: Option<String>,
    pub has_attachments: bool,
    pub received_at: Option<String>,
    pub attributes: Vec<DartEmailAttribute>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartEmailNotificationPage {
    pub items: Vec<DartEmailNotification>,
    pub next_cursor: Option<String>,
    pub has_more: bool,
}
