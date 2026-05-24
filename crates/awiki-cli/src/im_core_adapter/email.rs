use im_core::email::{
    EmailAttachmentContent, EmailAttachmentDownloadRequest, EmailInboxQuery, EmailMarkReadRequest,
    EmailMessageId, EmailNotificationQuery, SendEmailRequest,
};
use im_core::prelude::{EmailAddress, EmailFolder, PageLimit};
use serde_json::{json, Value};

use crate::cli::ParsedCommand;
use crate::output::ExitError;

#[derive(Debug, Clone)]
pub struct CommandResult {
    pub data: Value,
    pub summary: String,
    pub warnings: Vec<String>,
}

pub fn inbox_query(command: &ParsedCommand) -> Result<EmailInboxQuery, ExitError> {
    let folder = string_flag(command, "folder");
    let folder = if folder.trim().is_empty() {
        EmailFolder::inbox()
    } else {
        EmailFolder::parse(folder).map_err(invalid_arg("folder"))?
    };
    Ok(EmailInboxQuery {
        folder,
        limit: page_limit_from_i64(int_flag(command, "limit", 20)?),
        offset: u32_from_i64("offset", int_flag(command, "offset", 0)?)?,
        unread_only: bool_flag(command, "unread"),
    })
}

pub fn read_id(command: &ParsedCommand) -> Result<EmailMessageId, ExitError> {
    let message_id = string_flag(command, "id");
    if message_id.trim().is_empty() {
        return Err(ExitError::new(
            "invalid_argument",
            2,
            "mail read requires --id.",
            "Usage: awiki-cli mail read --id <MESSAGE_ID>",
        ));
    }
    EmailMessageId::parse(message_id).map_err(invalid_arg("id"))
}

pub fn mark_read_request(command: &ParsedCommand) -> Result<EmailMarkReadRequest, ExitError> {
    if command.args.is_empty() {
        return Err(ExitError::new(
            "invalid_argument",
            2,
            "mail mark-read requires at least one message id.",
            "Usage: awiki-cli mail mark-read <MESSAGE_ID...>",
        ));
    }
    let message_ids = command
        .args
        .iter()
        .map(EmailMessageId::parse)
        .collect::<Result<Vec<_>, _>>()
        .map_err(invalid_arg("message_ids"))?;
    Ok(EmailMarkReadRequest {
        message_ids,
        is_read: true,
    })
}

pub fn send_request(command: &ParsedCommand) -> Result<SendEmailRequest, ExitError> {
    let to = split_mail_list(&string_flag(command, "to"));
    if to.is_empty() {
        return Err(ExitError::new(
            "invalid_argument",
            2,
            "mail send requires --to.",
            "Usage: awiki-cli mail send --to alice@example.com --subject \"Hello\" --body \"Hi\"",
        ));
    }
    let subject = string_flag(command, "subject");
    if subject.trim().is_empty() {
        return Err(ExitError::new(
            "invalid_argument",
            2,
            "mail send requires --subject.",
            "Provide a subject with --subject.",
        ));
    }
    let body_text = string_flag(command, "body");
    if body_text.trim().is_empty() {
        return Err(ExitError::new(
            "invalid_argument",
            2,
            "mail send requires --body.",
            "Provide the plain text body with --body.",
        ));
    }
    let cc = split_mail_list(&string_flag(command, "cc"));
    let html = string_flag(command, "html");
    Ok(SendEmailRequest {
        to: parse_addresses(to)?,
        cc: parse_addresses(cc)?,
        subject,
        body_text,
        body_html: Some(html).filter(|value| !value.trim().is_empty()),
    })
}

pub fn attachment_request(
    command: &ParsedCommand,
) -> Result<EmailAttachmentDownloadRequest, ExitError> {
    let message_id = string_flag(command, "message-id");
    if message_id.trim().is_empty() {
        return Err(ExitError::new(
            "invalid_argument",
            2,
            "mail attachment download requires --message-id.",
            "Usage: awiki-cli mail attachment download --message-id <MESSAGE_ID> --attachment-index 0",
        ));
    }
    let attachment_index = int_flag(command, "attachment-index", 0)?;
    if attachment_index < 0 {
        return Err(ExitError::new(
            "invalid_argument",
            2,
            "attachment index must be >= 0.",
            "Use --attachment-index 0 for the first attachment.",
        ));
    }
    Ok(EmailAttachmentDownloadRequest {
        message_id: EmailMessageId::parse(message_id).map_err(invalid_arg("message-id"))?,
        attachment_index: u32_from_i64("attachment-index", attachment_index)?,
    })
}

pub fn notification_query(command: &ParsedCommand) -> Result<EmailNotificationQuery, ExitError> {
    Ok(EmailNotificationQuery {
        limit: page_limit_from_i64(int_flag(command, "limit", 20)?),
    })
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
                "folder": if folder.trim().is_empty() { "inbox" } else { folder },
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

pub fn notifications_plan(identity: &str, limit: i64) -> CommandResult {
    CommandResult {
        data: json!({
            "plan": {
                "action": "mail.notifications",
                "identity": identity,
                "limit": limit,
                "remote_calls": [],
            }
        }),
        summary: "Dry run: mail notifications planned".to_string(),
        warnings: Vec::new(),
    }
}

pub fn render_inbox(
    page: im_core::ids::Page<im_core::email::EmailMessageSummary>,
) -> CommandResult {
    let total = page.items.len();
    CommandResult {
        data: json!({
            "messages": page.items,
            "total": total,
            "has_more": page.has_more,
            "next_cursor": page.next_cursor,
        }),
        summary: format!("Loaded {total} messages"),
        warnings: Vec::new(),
    }
}

pub fn render_read(message: im_core::email::EmailMessage) -> CommandResult {
    let id = message.summary.id.as_str().to_string();
    CommandResult {
        data: serde_json::to_value(message).unwrap_or(Value::Null),
        summary: format!("Loaded message {id}"),
        warnings: Vec::new(),
    }
}

pub fn render_mark_read(result: im_core::email::EmailMarkReadResult) -> CommandResult {
    CommandResult {
        summary: format!("Marked {} message(s) as read", result.updated),
        data: serde_json::to_value(result).unwrap_or(Value::Null),
        warnings: Vec::new(),
    }
}

pub fn render_account(account: im_core::email::EmailAccount) -> CommandResult {
    CommandResult {
        data: serde_json::to_value(account).unwrap_or(Value::Null),
        summary: "Loaded mailbox account".to_string(),
        warnings: Vec::new(),
    }
}

pub fn render_send(result: im_core::email::SendEmailResult) -> CommandResult {
    let warnings = result.warnings.clone();
    CommandResult {
        data: serde_json::to_value(result).unwrap_or(Value::Null),
        summary: "Mail send request accepted".to_string(),
        warnings,
    }
}

pub fn render_notifications(
    page: im_core::ids::Page<im_core::email::EmailNotification>,
) -> CommandResult {
    let total = page.items.len();
    CommandResult {
        data: json!({
            "notifications": page.items,
            "total": total,
            "has_more": page.has_more,
            "next_cursor": page.next_cursor,
        }),
        summary: format!("Loaded {total} mail notification(s)"),
        warnings: Vec::new(),
    }
}

pub fn render_attachment_saved(
    content: &EmailAttachmentContent,
    path: &str,
) -> (Value, String, Vec<String>) {
    (
        json!({
            "message_id": content.message_id.as_str(),
            "attachment_index": content.attachment_index,
            "filename": content.filename,
            "content_type": content.content_type,
            "size": content.size,
            "path": path,
        }),
        format!("Attachment saved to {path}"),
        Vec::new(),
    )
}

pub fn split_mail_list(raw: &str) -> Vec<String> {
    raw.split(|ch: char| matches!(ch, ',' | ';' | '\n' | '\t' | ' '))
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

pub fn int_flag(command: &ParsedCommand, name: &str, fallback: i64) -> Result<i64, ExitError> {
    command
        .flags
        .get(name)
        .filter(|value| !value.trim().is_empty())
        .map(|value| {
            value.parse::<i64>().map_err(|_| {
                ExitError::new(
                    "invalid_argument",
                    2,
                    format!("--{name} must be an integer."),
                    "Pass a numeric value after the flag.",
                )
            })
        })
        .unwrap_or(Ok(fallback))
}

pub fn bool_flag(command: &ParsedCommand, name: &str) -> bool {
    command
        .flags
        .get(name)
        .is_some_and(|value| value.eq_ignore_ascii_case("true"))
}

fn string_flag(command: &ParsedCommand, name: &str) -> String {
    command.flags.get(name).cloned().unwrap_or_default()
}

fn page_limit_from_i64(value: i64) -> PageLimit {
    PageLimit::new(u32_from_i64_lossy(value).unwrap_or(20)).unwrap_or(PageLimit(20))
}

fn u32_from_i64(field: &'static str, value: i64) -> Result<u32, ExitError> {
    u32_from_i64_lossy(value).ok_or_else(|| {
        ExitError::new(
            "invalid_argument",
            2,
            format!("--{field} must be between 0 and {}.", u32::MAX),
            "Pass a non-negative numeric value.",
        )
    })
}

fn u32_from_i64_lossy(value: i64) -> Option<u32> {
    if value <= 0 {
        return Some(0);
    }
    u32::try_from(value).ok()
}

fn parse_addresses(values: Vec<String>) -> Result<Vec<EmailAddress>, ExitError> {
    values
        .into_iter()
        .map(|value| EmailAddress::parse(value).map_err(invalid_arg("email_address")))
        .collect()
}

fn invalid_arg(field: &'static str) -> impl FnOnce(im_core::ImError) -> ExitError {
    move |err| {
        ExitError::new(
            "invalid_argument",
            2,
            format!("invalid {field}: {err}"),
            "Check the mail command arguments and try again.",
        )
    }
}
