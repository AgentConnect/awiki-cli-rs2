use super::types::{
    AccountRequest, AttachmentRequest, InboxRequest, MailError, MarkReadRequest, ReadRequest,
    SendRequest, MAIL_RPC_ENDPOINT,
};
use crate::authsdk::{HttpError, RpcError};
use crate::transportcfg::Profile;
use serde_json::{json, Value};
use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub struct RpcCall {
    pub endpoint: &'static str,
    pub method: &'static str,
    pub profile: Profile,
    pub params: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ServiceError {
    pub status_code: u16,
    pub rpc_code: i64,
    pub message: String,
    pub data: Option<Value>,
}

impl fmt::Display for ServiceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.rpc_code, self.status_code) {
            (code, _) if code != 0 => {
                write!(formatter, "service rpc error {code}: {}", self.message)
            }
            (_, status_code) if status_code != 0 => {
                write!(
                    formatter,
                    "service http error {status_code}: {}",
                    self.message
                )
            }
            _ => formatter.write_str(&self.message),
        }
    }
}

impl std::error::Error for ServiceError {}

impl From<RpcError> for ServiceError {
    fn from(value: RpcError) -> Self {
        Self {
            status_code: 0,
            rpc_code: value.code,
            message: value.message,
            data: value.data,
        }
    }
}

impl From<HttpError> for ServiceError {
    fn from(value: HttpError) -> Self {
        Self {
            status_code: value.status_code,
            rpc_code: 0,
            message: value.message,
            data: None,
        }
    }
}

pub fn build_inbox_rpc_call(mut request: InboxRequest) -> RpcCall {
    if request.folder.trim().is_empty() {
        request.folder = "inbox".to_string();
    }
    if request.limit <= 0 {
        request.limit = 20;
    }
    RpcCall {
        endpoint: MAIL_RPC_ENDPOINT,
        method: "mail.getInbox",
        profile: Profile::RpcReadHeavy,
        params: json!({
            "folder": request.folder,
            "limit": request.limit,
            "offset": request.offset,
            "unread_only": request.unread_only,
        }),
    }
}

pub fn build_read_rpc_call(request: ReadRequest) -> Result<RpcCall, MailError> {
    if request.message_id.trim().is_empty() {
        return Err(MailError::MessageIdRequired);
    }
    Ok(RpcCall {
        endpoint: MAIL_RPC_ENDPOINT,
        method: "mail.getMessage",
        profile: Profile::RpcReadHeavy,
        params: json!({ "message_id": request.message_id }),
    })
}

pub fn build_mark_read_rpc_call(request: MarkReadRequest) -> Result<RpcCall, MailError> {
    if request.message_ids.is_empty() {
        return Err(MailError::MessageIdRequired);
    }
    Ok(RpcCall {
        endpoint: MAIL_RPC_ENDPOINT,
        method: "mail.markRead",
        profile: Profile::RpcDefault,
        params: json!({
            "message_ids": request.message_ids,
            "is_read": request.is_read,
        }),
    })
}

pub fn build_account_rpc_call(_request: AccountRequest) -> RpcCall {
    RpcCall {
        endpoint: MAIL_RPC_ENDPOINT,
        method: "mail.getMailbox",
        profile: Profile::RpcDefault,
        params: json!({}),
    }
}

pub fn build_attachment_rpc_call(request: AttachmentRequest) -> Result<RpcCall, MailError> {
    if request.message_id.trim().is_empty() {
        return Err(MailError::MessageIdRequired);
    }
    if request.attachment_index < 0 {
        return Err(MailError::AttachmentIndexZero);
    }
    Ok(RpcCall {
        endpoint: MAIL_RPC_ENDPOINT,
        method: "mail.getAttachment",
        profile: Profile::RpcReadHeavy,
        params: json!({
            "message_id": request.message_id,
            "attachment_index": request.attachment_index,
        }),
    })
}

pub fn build_send_rpc_call(request: SendRequest) -> Result<RpcCall, MailError> {
    if request.to.is_empty() {
        return Err(MailError::RecipientRequired);
    }
    if request.subject.trim().is_empty() {
        return Err(MailError::SubjectRequired);
    }
    if request.body_text.trim().is_empty() {
        return Err(MailError::BodyRequired);
    }
    let mut params = json!({
        "to": request.to,
        "cc": request.cc,
        "subject": request.subject,
        "body_text": request.body_text,
        "body_html": Value::Null,
    });
    if !request.body_html.trim().is_empty() {
        params["body_html"] = Value::String(request.body_html);
    }
    Ok(RpcCall {
        endpoint: MAIL_RPC_ENDPOINT,
        method: "mail.send",
        profile: Profile::RpcDefault,
        params,
    })
}

pub fn inbox_summary(result: &Value, folder: &str) -> String {
    let total = int_value(result.get("total"), 0);
    let count = list_length(result.get("messages"));
    if total > 0 && count == 0 {
        format!("Loaded {total} messages")
    } else {
        format!("Loaded {count} messages from {folder}")
    }
}

pub fn read_summary(message_id: &str) -> String {
    format!("Loaded message {message_id}")
}

pub fn mark_read_summary(result: &Value) -> String {
    let updated = int_value(result.get("updated"), 0);
    format!("Marked {updated} message(s) as read")
}

pub fn account_summary() -> &'static str {
    "Loaded mailbox account"
}

pub fn attachment_summary(result: &Value, attachment_index: i64) -> String {
    let filename = string_value(result.get("filename"))
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| format!("attachment_{attachment_index}"));
    format!("Fetched attachment {filename}")
}

pub fn send_summary() -> &'static str {
    "Mail send request accepted"
}

fn string_value(value: Option<&Value>) -> Option<String> {
    value.and_then(Value::as_str).map(ToOwned::to_owned)
}

fn int_value(value: Option<&Value>, fallback: i64) -> i64 {
    match value {
        Some(Value::Number(number)) => number
            .as_i64()
            .or_else(|| number.as_u64().and_then(|value| i64::try_from(value).ok()))
            .or_else(|| number.as_f64().map(|value| value as i64))
            .unwrap_or(fallback),
        _ => fallback,
    }
}

fn list_length(value: Option<&Value>) -> usize {
    value.and_then(Value::as_array).map(Vec::len).unwrap_or(0)
}
