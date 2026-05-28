#[cfg(feature = "sqlite")]
use rusqlite::types::ValueRef;
#[cfg(feature = "sqlite")]
use serde_json::{Map, Value};

#[cfg(feature = "sqlite")]
pub(crate) fn list_mail_notifications(
    sqlite_path: &std::path::Path,
    owner_identity_id: &str,
    owner_did: &str,
    limit: crate::ids::PageLimit,
) -> crate::ImResult<crate::ids::Page<crate::email::EmailNotification>> {
    let connection =
        rusqlite::Connection::open(sqlite_path).map_err(super::local_state_unavailable)?;
    super::configure(&connection)?;
    super::schema::ensure_schema(&connection)?;
    list_mail_notifications_from_connection(&connection, owner_identity_id, owner_did, limit)
}

#[cfg(feature = "sqlite")]
pub(crate) fn list_mail_notifications_from_connection(
    connection: &rusqlite::Connection,
    owner_identity_id: &str,
    owner_did: &str,
    limit: crate::ids::PageLimit,
) -> crate::ImResult<crate::ids::Page<crate::email::EmailNotification>> {
    let rows = query_mail_notification_rows(connection, owner_identity_id, owner_did, limit.0)?;
    let items = rows
        .into_iter()
        .filter_map(normalize_notification_row)
        .collect::<Vec<_>>();
    Ok(crate::ids::Page {
        items,
        next_cursor: None,
        has_more: false,
    })
}

#[cfg(not(feature = "sqlite"))]
pub(crate) fn list_mail_notifications(
    _sqlite_path: &std::path::Path,
    _owner_identity_id: &str,
    _owner_did: &str,
    _limit: crate::ids::PageLimit,
) -> crate::ImResult<crate::ids::Page<crate::email::EmailNotification>> {
    Err(crate::ImError::LocalStateUnavailable {
        detail: "sqlite feature is disabled".to_string(),
    })
}

#[cfg(feature = "sqlite")]
fn query_mail_notification_rows(
    connection: &rusqlite::Connection,
    owner_identity_id: &str,
    owner_did: &str,
    limit: u32,
) -> crate::ImResult<Vec<Value>> {
    let mut statement = connection
        .prepare(
            r#"
SELECT *
FROM messages
WHERE (owner_identity_id = ?1 OR ((owner_identity_id IS NULL OR TRIM(owner_identity_id) = '') AND owner_did = ?2))
  AND (COALESCE(content_type, '') = 'mail.notification'
       OR COALESCE(metadata, '') LIKE '%"source_kind":"mail"%')
ORDER BY COALESCE(sent_at, stored_at) DESC
LIMIT ?3
"#,
        )
        .map_err(super::local_state_unavailable)?;
    let names = statement
        .column_names()
        .into_iter()
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    let owner_identity_id = owner_identity_id.trim().to_string();
    let owner_did = owner_did.trim().to_string();
    let limit = i64::from(limit);
    let mut rows = statement
        .query(rusqlite::params![owner_identity_id, owner_did, limit])
        .map_err(super::local_state_unavailable)?;
    let mut results = Vec::new();
    while let Some(row) = rows.next().map_err(super::local_state_unavailable)? {
        let mut object = Map::new();
        for (index, name) in names.iter().enumerate() {
            object.insert(
                name.clone(),
                value_ref_to_json(row.get_ref(index).map_err(super::local_state_unavailable)?),
            );
        }
        results.push(Value::Object(object));
    }
    Ok(results)
}

#[cfg(feature = "sqlite")]
fn normalize_notification_row(row: Value) -> Option<crate::email::EmailNotification> {
    let object = row.as_object()?;
    if !is_local_mail_notification_row(object) {
        return None;
    }
    let metadata = parse_metadata(object.get("metadata"));
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
    let preview = metadata
        .get("preview")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(ToOwned::to_owned);
    let from_addr = metadata
        .get("from_addr")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(ToOwned::to_owned);
    let has_attachments = metadata.get("has_attachments").is_some_and(bool_from_value);
    let id = object
        .get("msg_id")
        .and_then(Value::as_str)
        .or_else(|| object.get("id").and_then(Value::as_str))
        .and_then(|id| crate::ids::MessageId::parse(id).ok())?;
    Some(crate::email::EmailNotification {
        id,
        mailbox_address: crate::email::EmailAddress::parse(&mailbox_address).ok(),
        from_addr,
        subject,
        preview,
        has_attachments,
        received_at: object
            .get("sent_at")
            .and_then(Value::as_str)
            .or_else(|| object.get("stored_at").and_then(Value::as_str))
            .filter(|value| !value.trim().is_empty())
            .map(ToOwned::to_owned),
        attributes: notification_attributes(object, &metadata),
    })
}

#[cfg(feature = "sqlite")]
fn is_local_mail_notification_row(object: &Map<String, Value>) -> bool {
    if object
        .get("content_type")
        .and_then(Value::as_str)
        .is_some_and(|value| value.trim() == "mail.notification")
    {
        return true;
    }
    parse_metadata(object.get("metadata"))
        .get("source_kind")
        .and_then(Value::as_str)
        .is_some_and(|value| value.trim() == "mail")
}

#[cfg(feature = "sqlite")]
fn parse_metadata(value: Option<&Value>) -> Map<String, Value> {
    match value {
        Some(Value::Object(object)) => object.clone(),
        Some(Value::String(text)) if !text.trim().is_empty() => serde_json::from_str::<Value>(text)
            .ok()
            .and_then(|value| value.as_object().cloned())
            .unwrap_or_default(),
        _ => Map::new(),
    }
}

#[cfg(feature = "sqlite")]
fn notification_attributes(
    row: &Map<String, Value>,
    metadata: &Map<String, Value>,
) -> Vec<crate::email::EmailAttribute> {
    let mut attrs = Vec::new();
    for key in [
        "content_type",
        "thread_id",
        "sender_did",
        "receiver_did",
        "is_read",
    ] {
        if let Some(value) = row.get(key).and_then(scalar_value) {
            attrs.push(crate::email::EmailAttribute {
                key: key.to_string(),
                value,
            });
        }
    }
    for (key, value) in metadata {
        if matches!(
            key.as_str(),
            "mailbox_address" | "subject" | "preview" | "from_addr" | "has_attachments"
        ) {
            continue;
        }
        if let Some(value) = scalar_value(value) {
            attrs.push(crate::email::EmailAttribute {
                key: key.clone(),
                value,
            });
        }
    }
    attrs
}

#[cfg(feature = "sqlite")]
fn default_string(value: Option<&str>, fallback: &str) -> String {
    value
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(fallback)
        .to_string()
}

#[cfg(feature = "sqlite")]
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

#[cfg(feature = "sqlite")]
fn scalar_value(value: &Value) -> Option<String> {
    match value {
        Value::Null | Value::Array(_) | Value::Object(_) => None,
        Value::String(value) => Some(value.clone()),
        Value::Bool(value) => Some(value.to_string()),
        Value::Number(value) => Some(value.to_string()),
    }
}

#[cfg(feature = "sqlite")]
fn value_ref_to_json(value: ValueRef<'_>) -> Value {
    match value {
        ValueRef::Null => Value::Null,
        ValueRef::Integer(value) => serde_json::json!(value),
        ValueRef::Real(value) => serde_json::Number::from_f64(value)
            .map(Value::Number)
            .unwrap_or(Value::Null),
        ValueRef::Text(value) => Value::String(String::from_utf8_lossy(value).into_owned()),
        ValueRef::Blob(value) => Value::String(String::from_utf8_lossy(value).into_owned()),
    }
}
