use serde_json::{json, Value};

pub(crate) const MAIL_RPC_ENDPOINT: &str = "/mail/rpc";

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct EmailRpcCall {
    pub(crate) endpoint: &'static str,
    pub(crate) method: &'static str,
    pub(crate) params: Value,
}

pub(crate) fn build_inbox_rpc_call(query: crate::email::EmailInboxQuery) -> EmailRpcCall {
    EmailRpcCall {
        endpoint: MAIL_RPC_ENDPOINT,
        method: "mail.getInbox",
        params: json!({
            "folder": query.folder.as_str(),
            "limit": query.limit.0,
            "offset": query.offset,
            "unread_only": query.unread_only,
        }),
    }
}

pub(crate) fn build_read_rpc_call(id: &crate::email::EmailMessageId) -> EmailRpcCall {
    EmailRpcCall {
        endpoint: MAIL_RPC_ENDPOINT,
        method: "mail.getMessage",
        params: json!({ "message_id": id.as_str() }),
    }
}

pub(crate) fn build_mark_read_rpc_call(
    request: crate::email::EmailMarkReadRequest,
) -> crate::ImResult<EmailRpcCall> {
    if request.message_ids.is_empty() {
        return Err(crate::ImError::invalid_input(
            Some("message_ids".to_string()),
            "message_ids must not be empty",
        ));
    }
    let ids = request
        .message_ids
        .iter()
        .map(|id| id.as_str().to_string())
        .collect::<Vec<_>>();
    Ok(EmailRpcCall {
        endpoint: MAIL_RPC_ENDPOINT,
        method: "mail.markRead",
        params: json!({
            "message_ids": ids,
            "is_read": request.is_read,
        }),
    })
}

pub(crate) fn build_account_rpc_call() -> EmailRpcCall {
    EmailRpcCall {
        endpoint: MAIL_RPC_ENDPOINT,
        method: "mail.getMailbox",
        params: json!({}),
    }
}

pub(crate) fn build_send_rpc_call(
    request: crate::email::SendEmailRequest,
) -> crate::ImResult<EmailRpcCall> {
    if request.to.is_empty() {
        return Err(crate::ImError::invalid_input(
            Some("to".to_string()),
            "mail recipient is required",
        ));
    }
    if request.subject.trim().is_empty() {
        return Err(crate::ImError::invalid_input(
            Some("subject".to_string()),
            "mail subject is required",
        ));
    }
    if request.body_text.trim().is_empty() {
        return Err(crate::ImError::invalid_input(
            Some("body_text".to_string()),
            "mail body is required",
        ));
    }
    let to = request
        .to
        .iter()
        .map(|address| address.as_str().to_string())
        .collect::<Vec<_>>();
    let cc = request
        .cc
        .iter()
        .map(|address| address.as_str().to_string())
        .collect::<Vec<_>>();
    let body_html = request
        .body_html
        .filter(|value| !value.trim().is_empty())
        .map(Value::String)
        .unwrap_or(Value::Null);
    Ok(EmailRpcCall {
        endpoint: MAIL_RPC_ENDPOINT,
        method: "mail.send",
        params: json!({
            "to": to,
            "cc": cc,
            "subject": request.subject,
            "body_text": request.body_text,
            "body_html": body_html,
        }),
    })
}

pub(crate) fn build_attachment_rpc_call(
    request: &crate::email::EmailAttachmentDownloadRequest,
) -> EmailRpcCall {
    EmailRpcCall {
        endpoint: MAIL_RPC_ENDPOINT,
        method: "mail.getAttachment",
        params: json!({
            "message_id": request.message_id.as_str(),
            "attachment_index": request.attachment_index,
        }),
    }
}

pub(crate) mod normalize {
    use base64::Engine;
    use serde_json::{Map, Value};

    pub(crate) fn account(value: Value) -> crate::ImResult<crate::email::EmailAccount> {
        let object = value.as_object().cloned().unwrap_or_default();
        let mailbox = first_string(
            &object,
            &[
                "mailbox_address",
                "address",
                "email",
                "mailbox",
                "mailboxAddress",
            ],
        );
        Ok(crate::email::EmailAccount {
            mailbox_address: mailbox
                .and_then(|value| crate::email::EmailAddress::parse(value).ok()),
            display_name: first_string(&object, &["display_name", "displayName", "name"]),
            status: first_string(&object, &["status", "state"]),
            attributes: attributes(
                &object,
                &[
                    "mailbox_address",
                    "address",
                    "email",
                    "mailbox",
                    "mailboxAddress",
                    "display_name",
                    "displayName",
                    "name",
                    "status",
                    "state",
                ],
            ),
        })
    }

    pub(crate) fn inbox(
        value: Value,
    ) -> crate::ImResult<crate::ids::Page<crate::email::EmailMessageSummary>> {
        let messages = array_candidates(&value, &["messages", "items", "data"])
            .into_iter()
            .map(message_summary)
            .collect::<crate::ImResult<Vec<_>>>()?;
        let has_more = bool_candidate(&value, &["has_more", "hasMore"]).unwrap_or(false);
        let next_cursor = first_string_value(&value, &["next_cursor", "nextCursor", "cursor"])
            .and_then(|cursor| crate::ids::Cursor::parse(cursor).ok());
        Ok(crate::ids::Page {
            items: messages,
            next_cursor,
            has_more,
        })
    }

    pub(crate) fn message(value: Value) -> crate::ImResult<crate::email::EmailMessage> {
        let object = value.as_object().cloned().unwrap_or_default();
        let summary = message_summary(value.clone())?;
        let attachments = array_from_object(&object, &["attachments"])
            .into_iter()
            .enumerate()
            .map(|(index, value)| attachment_metadata(index as u32, value))
            .collect::<Vec<_>>();
        Ok(crate::email::EmailMessage {
            summary,
            body_text: first_string(&object, &["body_text", "bodyText", "text", "body"]),
            body_html: first_string(&object, &["body_html", "bodyHtml", "html"]),
            attachments,
        })
    }

    pub(crate) fn mark_read(value: Value) -> crate::email::EmailMarkReadResult {
        let updated = first_u64_value(&value, &["updated", "updated_count", "updatedCount"])
            .unwrap_or_default()
            .min(u32::MAX as u64) as u32;
        crate::email::EmailMarkReadResult { updated }
    }

    pub(crate) fn send(value: Value) -> crate::email::SendEmailResult {
        let accepted = bool_candidate(&value, &["accepted", "ok", "success"]).unwrap_or(true);
        let message_id = first_string_value(&value, &["message_id", "messageId", "id"])
            .and_then(|id| crate::email::EmailMessageId::parse(id).ok());
        let warnings = string_array_candidate(&value, &["warnings"]);
        crate::email::SendEmailResult {
            accepted,
            message_id,
            warnings,
        }
    }

    pub(crate) fn attachment(
        request: crate::email::EmailAttachmentDownloadRequest,
        value: Value,
    ) -> crate::ImResult<crate::email::EmailAttachmentContent> {
        let object = value.as_object().cloned().unwrap_or_default();
        let filename = first_string(&object, &["filename", "name"])
            .unwrap_or_else(|| format!("attachment_{}", request.attachment_index));
        let content_type = first_string(&object, &["content_type", "contentType", "mime_type"])
            .unwrap_or_else(|| "application/octet-stream".to_string());
        let size = first_u64(&object, &["size", "size_bytes", "sizeBytes"]);
        let content = first_string(&object, &["content_base64", "contentBase64", "base64"])
            .ok_or_else(|| crate::ImError::Serialization {
                detail: "mail attachment response missing content_base64".to_string(),
            })?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(content.as_bytes())
            .map_err(|err| crate::ImError::Serialization {
                detail: format!("mail attachment base64 decode failed: {err}"),
            })?;
        Ok(crate::email::EmailAttachmentContent {
            message_id: request.message_id,
            attachment_index: request.attachment_index,
            filename,
            content_type,
            size,
            bytes,
        })
    }

    pub(crate) fn message_summary(
        value: Value,
    ) -> crate::ImResult<crate::email::EmailMessageSummary> {
        let object = value.as_object().cloned().unwrap_or_default();
        let id = first_string(
            &object,
            &["id", "message_id", "messageId", "msg_id", "msgId"],
        )
        .ok_or_else(|| crate::ImError::Serialization {
            detail: "mail message response missing message id".to_string(),
        })?;
        let folder = first_string(&object, &["folder", "mailbox"])
            .and_then(|value| crate::email::EmailFolder::parse(value).ok());
        let subject = first_string(&object, &["subject", "title"])
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "(no subject)".to_string());
        let attachment_count = first_u64(&object, &["attachment_count", "attachmentCount"])
            .map(|value| value.min(u32::MAX as u64) as u32);
        let has_attachments = bool_from_object(&object, &["has_attachments", "hasAttachments"])
            .unwrap_or_else(|| attachment_count.unwrap_or_default() > 0);
        Ok(crate::email::EmailMessageSummary {
            id: crate::email::EmailMessageId::parse(id)?,
            folder,
            from: addresses_from_object(&object, &["from", "from_addr", "fromAddr"]),
            to: addresses_from_object(&object, &["to"]),
            cc: addresses_from_object(&object, &["cc"]),
            subject,
            preview: first_string(&object, &["preview", "snippet"]),
            received_at: first_string(&object, &["received_at", "receivedAt"]),
            sent_at: first_string(&object, &["sent_at", "sentAt", "date"]),
            unread: bool_from_object(&object, &["unread"]).unwrap_or_else(|| {
                !bool_from_object(&object, &["is_read", "isRead"]).unwrap_or(false)
            }),
            has_attachments,
            attachment_count,
            attributes: attributes(
                &object,
                &[
                    "id",
                    "message_id",
                    "messageId",
                    "msg_id",
                    "msgId",
                    "folder",
                    "mailbox",
                    "from",
                    "from_addr",
                    "fromAddr",
                    "to",
                    "cc",
                    "subject",
                    "title",
                    "preview",
                    "snippet",
                    "received_at",
                    "receivedAt",
                    "sent_at",
                    "sentAt",
                    "date",
                    "unread",
                    "is_read",
                    "isRead",
                    "has_attachments",
                    "hasAttachments",
                    "attachment_count",
                    "attachmentCount",
                    "body_text",
                    "bodyText",
                    "text",
                    "body",
                    "body_html",
                    "bodyHtml",
                    "html",
                    "attachments",
                ],
            ),
        })
    }

    fn attachment_metadata(index: u32, value: Value) -> crate::email::EmailAttachmentMetadata {
        let object = value.as_object().cloned().unwrap_or_default();
        crate::email::EmailAttachmentMetadata {
            index: first_u64(&object, &["index"])
                .map(|value| value.min(u32::MAX as u64) as u32)
                .unwrap_or(index),
            filename: first_string(&object, &["filename", "name"]),
            content_type: first_string(&object, &["content_type", "contentType", "mime_type"]),
            size: first_u64(&object, &["size", "size_bytes", "sizeBytes"]),
        }
    }

    fn array_candidates(value: &Value, keys: &[&str]) -> Vec<Value> {
        if let Some(array) = value.as_array() {
            return array.clone();
        }
        let Some(object) = value.as_object() else {
            return Vec::new();
        };
        array_from_object(object, keys)
    }

    fn array_from_object(object: &Map<String, Value>, keys: &[&str]) -> Vec<Value> {
        for key in keys {
            if let Some(array) = object.get(*key).and_then(Value::as_array) {
                return array.clone();
            }
        }
        Vec::new()
    }

    fn addresses_from_object(
        object: &Map<String, Value>,
        keys: &[&str],
    ) -> Vec<crate::email::EmailAddress> {
        for key in keys {
            let Some(value) = object.get(*key) else {
                continue;
            };
            let raw = match value {
                Value::Array(items) => items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(ToOwned::to_owned)
                    .collect::<Vec<_>>(),
                Value::String(value) => value
                    .split(|ch| matches!(ch, ',' | ';'))
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(ToOwned::to_owned)
                    .collect::<Vec<_>>(),
                _ => Vec::new(),
            };
            if !raw.is_empty() {
                return raw
                    .into_iter()
                    .filter_map(|value| crate::email::EmailAddress::parse(value).ok())
                    .collect();
            }
        }
        Vec::new()
    }

    fn first_string(object: &Map<String, Value>, keys: &[&str]) -> Option<String> {
        keys.iter()
            .find_map(|key| string_value(object.get(*key)))
            .filter(|value| !value.trim().is_empty())
    }

    fn first_u64(object: &Map<String, Value>, keys: &[&str]) -> Option<u64> {
        keys.iter().find_map(|key| u64_value(object.get(*key)))
    }

    fn bool_from_object(object: &Map<String, Value>, keys: &[&str]) -> Option<bool> {
        keys.iter().find_map(|key| bool_value(object.get(*key)))
    }

    fn first_string_value(value: &Value, keys: &[&str]) -> Option<String> {
        let object = value.as_object()?;
        first_string(object, keys)
    }

    fn first_u64_value(value: &Value, keys: &[&str]) -> Option<u64> {
        let object = value.as_object()?;
        first_u64(object, keys)
    }

    fn bool_candidate(value: &Value, keys: &[&str]) -> Option<bool> {
        let object = value.as_object()?;
        bool_from_object(object, keys)
    }

    fn string_array_candidate(value: &Value, keys: &[&str]) -> Vec<String> {
        let Some(object) = value.as_object() else {
            return Vec::new();
        };
        for key in keys {
            if let Some(array) = object.get(*key).and_then(Value::as_array) {
                return array
                    .iter()
                    .filter_map(Value::as_str)
                    .map(ToOwned::to_owned)
                    .collect();
            }
        }
        Vec::new()
    }

    fn string_value(value: Option<&Value>) -> Option<String> {
        match value {
            Some(Value::String(value)) => Some(value.clone()),
            Some(Value::Number(value)) => Some(value.to_string()),
            Some(Value::Bool(value)) => Some(value.to_string()),
            _ => None,
        }
    }

    fn u64_value(value: Option<&Value>) -> Option<u64> {
        match value {
            Some(Value::Number(value)) => value.as_u64().or_else(|| {
                value
                    .as_i64()
                    .and_then(|value| u64::try_from(value).ok())
                    .or_else(|| value.as_f64().map(|value| value as u64))
            }),
            Some(Value::String(value)) => value.trim().parse().ok(),
            _ => None,
        }
    }

    fn bool_value(value: Option<&Value>) -> Option<bool> {
        match value {
            Some(Value::Bool(value)) => Some(*value),
            Some(Value::Number(value)) => Some(value.as_i64().unwrap_or_default() != 0),
            Some(Value::String(value)) => match value.trim().to_ascii_lowercase().as_str() {
                "1" | "true" | "yes" | "y" | "on" => Some(true),
                "0" | "false" | "no" | "n" | "off" => Some(false),
                _ => None,
            },
            _ => None,
        }
    }

    fn attributes(
        object: &Map<String, Value>,
        known: &[&str],
    ) -> Vec<crate::email::EmailAttribute> {
        object
            .iter()
            .filter(|(key, _)| !known.iter().any(|known| known == &key.as_str()))
            .filter_map(|(key, value)| {
                scalar_attribute_value(value).map(|value| crate::email::EmailAttribute {
                    key: key.clone(),
                    value,
                })
            })
            .collect()
    }

    fn scalar_attribute_value(value: &Value) -> Option<String> {
        match value {
            Value::Null | Value::Array(_) | Value::Object(_) => None,
            Value::String(value) => Some(value.clone()),
            Value::Bool(value) => Some(value.to_string()),
            Value::Number(value) => Some(value.to_string()),
        }
    }
}
