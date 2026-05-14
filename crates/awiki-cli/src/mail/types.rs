use serde_json::{json, Value};
use std::fmt;

#[derive(Debug, Clone)]
pub struct CommandResult {
    pub data: Value,
    pub summary: String,
    pub warnings: Vec<String>,
}

#[derive(Debug)]
pub enum MailError {
    MessageIdRequired,
    RecipientRequired,
    SubjectRequired,
    BodyRequired,
    AttachmentIndexZero,
    IdentityRequired(String),
    Store(crate::store::StoreError),
    Identity(crate::identity::IdentityError),
    Internal(String),
}

impl fmt::Display for MailError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MessageIdRequired => f.write_str("message id is required"),
            Self::RecipientRequired => f.write_str("mail recipient is required"),
            Self::SubjectRequired => f.write_str("mail subject is required"),
            Self::BodyRequired => f.write_str("mail body is required"),
            Self::AttachmentIndexZero => f.write_str("attachment index must be >= 0"),
            Self::IdentityRequired(message) | Self::Internal(message) => f.write_str(message),
            Self::Store(err) => write!(f, "{err}"),
            Self::Identity(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for MailError {}

impl From<crate::store::StoreError> for MailError {
    fn from(value: crate::store::StoreError) -> Self {
        Self::Store(value)
    }
}

impl From<crate::identity::IdentityError> for MailError {
    fn from(value: crate::identity::IdentityError) -> Self {
        Self::Identity(value)
    }
}

pub fn inbox_plan(
    identity: &str,
    folder: &str,
    limit: i64,
    offset: i64,
    unread_only: bool,
) -> CommandResult {
    CommandResult {
        data: json!({
            "plan": {
                "action": "mail.getInbox",
                "identity": identity,
                "folder": default_string(folder, "inbox"),
                "limit": limit,
                "offset": offset,
                "unread_only": unread_only,
                "remote_calls": ["POST /mail/rpc mail.getInbox"],
            }
        }),
        summary: "Dry run: mail inbox planned".to_string(),
        warnings: Vec::new(),
    }
}

pub fn read_plan(identity: &str, message_id: &str) -> CommandResult {
    CommandResult {
        data: json!({
            "plan": {
                "action": "mail.getMessage",
                "identity": identity,
                "message_id": message_id,
                "remote_calls": ["POST /mail/rpc mail.getMessage"],
            }
        }),
        summary: "Dry run: mail read planned".to_string(),
        warnings: Vec::new(),
    }
}

pub fn mark_read_plan(identity: &str, message_ids: &[String]) -> CommandResult {
    CommandResult {
        data: json!({
            "plan": {
                "action": "mail.markRead",
                "identity": identity,
                "message_ids": message_ids,
                "remote_calls": ["POST /mail/rpc mail.markRead"],
            }
        }),
        summary: "Dry run: mail mark-read planned".to_string(),
        warnings: Vec::new(),
    }
}

pub fn account_plan(identity: &str) -> CommandResult {
    CommandResult {
        data: json!({
            "plan": {
                "action": "mail.getMailbox",
                "identity": identity,
                "remote_calls": ["POST /mail/rpc mail.getMailbox"],
            }
        }),
        summary: "Dry run: mail account lookup planned".to_string(),
        warnings: Vec::new(),
    }
}

pub fn send_plan(
    identity: &str,
    to: &[String],
    cc: &[String],
    subject: &str,
    html: &str,
) -> CommandResult {
    CommandResult {
        data: json!({
            "plan": {
                "action": "mail.send",
                "identity": identity,
                "to": to,
                "cc": cc,
                "subject": subject,
                "has_html": !html.trim().is_empty(),
                "remote_calls": ["POST /mail/rpc mail.send"],
            }
        }),
        summary: "Dry run: mail send planned".to_string(),
        warnings: Vec::new(),
    }
}

pub fn attachment_download_plan(
    identity: &str,
    message_id: &str,
    attachment_index: i64,
    output: &str,
) -> CommandResult {
    CommandResult {
        data: json!({
            "plan": {
                "action": "mail.getAttachment",
                "identity": identity,
                "message_id": message_id,
                "attachment_index": attachment_index,
                "output": output,
                "remote_calls": ["POST /mail/rpc mail.getAttachment"],
            }
        }),
        summary: "Dry run: mail attachment download planned".to_string(),
        warnings: Vec::new(),
    }
}

fn default_string(value: &str, fallback: &str) -> String {
    if value.trim().is_empty() {
        fallback.to_string()
    } else {
        value.to_string()
    }
}
