use super::types::{CommandResult, MailError};
use crate::config::Resolved;
use crate::identity::Manager;
use crate::store;
use serde_json::{json, Value};

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
