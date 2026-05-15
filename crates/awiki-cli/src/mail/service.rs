use super::types::{
    AccountRequest, AttachmentRequest, CommandResult, InboxRequest, MailError, MarkReadRequest,
    ReadRequest, SendRequest,
};
use super::wire::{
    account_summary, attachment_summary, build_account_rpc_call, build_attachment_rpc_call,
    build_inbox_rpc_call, build_mark_read_rpc_call, build_read_rpc_call, build_send_rpc_call,
    inbox_summary, mark_read_summary, read_summary, send_summary,
};
use super::Client;
use crate::authsdk::Session;
use crate::config::Resolved;
use crate::identity::types::StoredIdentity;
use crate::identity::Manager;
use crate::store;
use serde_json::{json, Value};

const DID_AUTH_RPC_ENDPOINT: &str = "/user-service/did-auth/rpc";

pub fn inbox(
    resolved: &Resolved,
    manager: &Manager,
    request: InboxRequest,
) -> Result<CommandResult, MailError> {
    let mut request = request;
    if request.folder.trim().is_empty() {
        request.folder = "inbox".to_string();
    }
    if request.limit <= 0 {
        request.limit = 20;
    }
    let record = require_active_identity(resolved, manager, &request.identity_name)?;
    let mut auth = auth_session(resolved, manager, &record)?;
    let client = Client::new(resolved)?;
    let call = build_inbox_rpc_call(request.clone());
    let result: Value = client.authenticated_rpc_call_profile(
        call.profile,
        call.endpoint,
        call.method,
        call.params,
        &mut auth,
    )?;
    Ok(CommandResult {
        summary: inbox_summary(&result, &request.folder),
        data: result,
        warnings: Vec::new(),
    })
}

pub fn read(
    resolved: &Resolved,
    manager: &Manager,
    request: ReadRequest,
) -> Result<CommandResult, MailError> {
    let call = build_read_rpc_call(request.clone())?;
    let record = require_active_identity(resolved, manager, &request.identity_name)?;
    let mut auth = auth_session(resolved, manager, &record)?;
    let client = Client::new(resolved)?;
    let result: Value = client.authenticated_rpc_call_profile(
        call.profile,
        call.endpoint,
        call.method,
        call.params,
        &mut auth,
    )?;
    Ok(CommandResult {
        data: result,
        summary: read_summary(&request.message_id),
        warnings: Vec::new(),
    })
}

pub fn mark_read(
    resolved: &Resolved,
    manager: &Manager,
    request: MarkReadRequest,
) -> Result<CommandResult, MailError> {
    let call = build_mark_read_rpc_call(request.clone())?;
    let record = require_active_identity(resolved, manager, &request.identity_name)?;
    let mut auth = auth_session(resolved, manager, &record)?;
    let client = Client::new(resolved)?;
    let result: Value = client.authenticated_rpc_call_profile(
        call.profile,
        call.endpoint,
        call.method,
        call.params,
        &mut auth,
    )?;
    Ok(CommandResult {
        summary: mark_read_summary(&result),
        data: result,
        warnings: Vec::new(),
    })
}

pub fn account(
    resolved: &Resolved,
    manager: &Manager,
    request: AccountRequest,
) -> Result<CommandResult, MailError> {
    let call = build_account_rpc_call(request.clone());
    let record = require_active_identity(resolved, manager, &request.identity_name)?;
    let mut auth = auth_session(resolved, manager, &record)?;
    let client = Client::new(resolved)?;
    let result: Value = client.authenticated_rpc_call_profile(
        call.profile,
        call.endpoint,
        call.method,
        call.params,
        &mut auth,
    )?;
    Ok(CommandResult {
        data: result,
        summary: account_summary().to_string(),
        warnings: Vec::new(),
    })
}

pub fn attachment(
    resolved: &Resolved,
    manager: &Manager,
    request: AttachmentRequest,
) -> Result<CommandResult, MailError> {
    let call = build_attachment_rpc_call(request.clone())?;
    let record = require_active_identity(resolved, manager, &request.identity_name)?;
    let mut auth = auth_session(resolved, manager, &record)?;
    let client = Client::new(resolved)?;
    let result: Value = client.authenticated_rpc_call_profile(
        call.profile,
        call.endpoint,
        call.method,
        call.params,
        &mut auth,
    )?;
    Ok(CommandResult {
        summary: attachment_summary(&result, request.attachment_index),
        data: result,
        warnings: Vec::new(),
    })
}

pub fn send(
    resolved: &Resolved,
    manager: &Manager,
    request: SendRequest,
) -> Result<CommandResult, MailError> {
    let call = build_send_rpc_call(request.clone())?;
    let record = require_active_identity(resolved, manager, &request.identity_name)?;
    let mut auth = auth_session(resolved, manager, &record)?;
    let client = Client::new(resolved)?;
    let result: Value = client.authenticated_rpc_call_profile(
        call.profile,
        call.endpoint,
        call.method,
        call.params,
        &mut auth,
    )?;
    Ok(CommandResult {
        data: result,
        summary: send_summary().to_string(),
        warnings: Vec::new(),
    })
}

pub fn notifications(
    resolved: &Resolved,
    manager: &Manager,
    identity_name: &str,
    limit: i64,
) -> Result<CommandResult, MailError> {
    let record = require_active_identity(resolved, manager, identity_name)?;
    let db = store::open(&resolved.paths)?;
    store::ensure_schema(&db)?;
    let rows = store::list_notifications(&db, &record.did, limit)?;
    let rows = normalize_notification_rows(rows);
    let total = rows.len();
    Ok(CommandResult {
        data: json!({
            "notifications": rows,
            "total": total,
        }),
        summary: format!("Loaded {total} mail notification(s)"),
        warnings: Vec::new(),
    })
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

pub fn split_mail_list(raw: &str) -> Vec<String> {
    raw.split(|ch: char| matches!(ch, ',' | ';' | '\n' | '\t' | ' '))
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn require_active_identity(
    resolved: &Resolved,
    manager: &Manager,
    requested: &str,
) -> Result<crate::identity::types::StoredIdentity, MailError> {
    let identity_name = if requested.trim().is_empty() {
        if resolved.active_identity.trim().is_empty() {
            manager.current()?.identity_name
        } else {
            resolved.active_identity.clone()
        }
    } else {
        requested.trim().to_string()
    };
    let record = manager.load(&identity_name)?;
    let user_state = crate::identity::store::evaluate_user_state(&record.user_id, &record.handle);
    if !user_state.ready_for_messaging {
        return Err(MailError::IdentityRequired(format!(
            "identity {} requires user registration before messaging",
            record.identity_name
        )));
    }
    Ok(record)
}

fn auth_session(
    resolved: &Resolved,
    manager: &Manager,
    record: &StoredIdentity,
) -> Result<Session, MailError> {
    if record.identity_name.trim().is_empty() {
        return Err(MailError::Internal(
            "active identity is required".to_string(),
        ));
    }
    let paths = manager.paths_for_identity(&record.identity_name)?;
    let identity_name = record.identity_name.clone();
    let persist_manager = manager.clone();
    let persist_identity_name = identity_name.clone();
    let persist_token: crate::authsdk::PersistToken = Box::new(move |token| {
        persist_manager.update_jwt(&persist_identity_name, token)?;
        Ok(())
    });
    let mut session = Session::new(
        &paths.did_document_path,
        &paths.key1_private_path,
        identity_name,
        record.did.as_str(),
        record.jwt_token.as_str(),
        Some(persist_token),
    );
    let base_url = resolved.service_base_url.trim();
    let did_auth_url = crate::config::join_base_url(base_url, DID_AUTH_RPC_ENDPOINT);
    if !base_url.is_empty() {
        session.remember_scope(base_url);
        session.remember_scope(&did_auth_url);
    }
    if !resolved.mail_service_url.trim().is_empty() {
        session.remember_scope(&resolved.mail_service_url);
    }
    let token = record.jwt_token.trim();
    if !token.is_empty() && !base_url.is_empty() {
        session.set_bearer(base_url, token);
        session.set_bearer(&did_auth_url, token);
    }
    if !token.is_empty() && !resolved.mail_service_url.trim().is_empty() {
        session.set_bearer(&resolved.mail_service_url, token);
    }
    if token.is_empty() {
        let client = Client::new(resolved)?;
        if let Err(err) = client.ensure_jwt(&mut session, &did_auth_url) {
            return match err {
                MailError::Service(err) => Err(MailError::Service(err)),
                err => Err(MailError::Internal(format!(
                    "active identity does not have a JWT yet: {err}"
                ))),
            };
        }
    }
    Ok(session)
}

fn normalize_notification_rows(rows: Vec<Value>) -> Vec<Value> {
    rows.into_iter().map(normalize_notification_row).collect()
}

fn normalize_notification_row(row: Value) -> Value {
    let Some(object) = row.as_object() else {
        return row;
    };
    if !is_local_mail_notification_row(object) {
        return Value::Object(object.clone());
    }

    let metadata = parse_notification_metadata(object.get("metadata"));
    let mut mailbox_address = default_string(
        metadata.get("mailbox_address").and_then(Value::as_str),
        object
            .get("thread_id")
            .and_then(Value::as_str)
            .unwrap_or_default(),
    );
    if let Some(stripped) = mailbox_address.strip_prefix("mail:") {
        mailbox_address = stripped.to_string();
    }

    let mut subject = default_string(
        metadata.get("subject").and_then(Value::as_str),
        object
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or_default(),
    );
    if let Some(stripped) = subject.strip_prefix("[邮件] ") {
        subject = stripped.to_string();
    }
    if subject.trim().is_empty() {
        subject = "(no subject)".to_string();
    }

    let from_addr = metadata
        .get("from_addr")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let preview = metadata
        .get("preview")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let has_attachments = metadata.get("has_attachments").is_some_and(bool_from_value);

    let mut normalized = object.clone();
    normalized.insert("source_kind".to_string(), json!("mail"));
    normalized.insert("title".to_string(), json!(format!("[邮件] {subject}")));
    normalized.insert(
        "content".to_string(),
        json!(build_notification_content(
            &mailbox_address,
            from_addr,
            &subject,
            preview,
            has_attachments
        )),
    );
    Value::Object(normalized)
}

fn is_local_mail_notification_row(object: &serde_json::Map<String, Value>) -> bool {
    if object
        .get("content_type")
        .and_then(Value::as_str)
        .is_some_and(|value| value.trim() == "mail.notification")
    {
        return true;
    }
    let metadata = parse_notification_metadata(object.get("metadata"));
    metadata
        .get("source_kind")
        .and_then(Value::as_str)
        .is_some_and(|value| value.trim() == "mail")
}

fn parse_notification_metadata(value: Option<&Value>) -> serde_json::Map<String, Value> {
    match value {
        Some(Value::Object(object)) => object.clone(),
        Some(Value::String(text)) if !text.trim().is_empty() => serde_json::from_str::<Value>(text)
            .ok()
            .and_then(|value| value.as_object().cloned())
            .unwrap_or_default(),
        _ => serde_json::Map::new(),
    }
}

fn build_notification_content(
    mailbox_address: &str,
    from_addr: &str,
    subject: &str,
    preview: &str,
    has_attachments: bool,
) -> String {
    let mut lines = vec![format!("[邮件] 收件邮箱: {mailbox_address}")];
    if !from_addr.is_empty() {
        lines.push(format!("发件人: {from_addr}"));
    }
    if !subject.is_empty() {
        lines.push(format!("主题: {subject}"));
    }
    if !preview.is_empty() {
        lines.push(String::new());
        lines.push(preview.to_string());
    }
    if has_attachments {
        lines.push(String::new());
        lines.push("(这封邮件包含附件)".to_string());
    }
    lines.join("\n")
}

fn default_string(value: Option<&str>, fallback: &str) -> String {
    value
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(fallback)
        .to_string()
}

fn bool_from_value(value: &Value) -> bool {
    match value {
        Value::Bool(value) => *value,
        Value::Number(value) => value.as_i64().unwrap_or_default() != 0,
        Value::String(value) => matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "y" | "on"
        ),
        _ => false,
    }
}
