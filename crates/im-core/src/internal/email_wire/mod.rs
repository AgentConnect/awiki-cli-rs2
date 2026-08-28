use base64::Engine as _;
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
    build_send_with_attachments_rpc_call(request.into())
}

pub(crate) fn build_send_with_attachments_rpc_call(
    request: crate::email::SendEmailWithAttachmentsRequest,
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
    if request.attachments.len() > crate::email::EMAIL_ATTACHMENT_MAX_COUNT {
        return Err(crate::ImError::invalid_input(
            Some("attachments".to_string()),
            "mail attachment collection exceeds the supported count",
        ));
    }
    let mut attachment_total_bytes = 0usize;
    for attachment in &request.attachments {
        if !crate::email::valid_attachment_filename(&attachment.filename)
            || !valid_attachment_content_type(&attachment.content_type)
            || attachment.bytes.len() > crate::email::EMAIL_ATTACHMENT_MAX_BYTES
        {
            return Err(crate::ImError::invalid_input(
                Some("attachments".to_string()),
                "mail attachment is invalid",
            ));
        }
        attachment_total_bytes = attachment_total_bytes
            .checked_add(attachment.bytes.len())
            .ok_or_else(|| {
                crate::ImError::invalid_input(
                    Some("attachments".to_string()),
                    "mail attachment collection is too large",
                )
            })?;
        if attachment_total_bytes > crate::email::EMAIL_ATTACHMENT_TOTAL_MAX_BYTES {
            return Err(crate::ImError::invalid_input(
                Some("attachments".to_string()),
                "mail attachment collection is too large",
            ));
        }
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
    let attachments = request
        .attachments
        .into_iter()
        .map(|attachment| {
            json!({
                "filename": attachment.filename,
                "content_type": attachment.content_type,
                "content_base64": base64::engine::general_purpose::STANDARD.encode(attachment.bytes),
            })
        })
        .collect::<Vec<_>>();
    let mut params = json!({
        "to": to,
        "cc": cc,
        "subject": request.subject,
        "body_text": request.body_text,
        "body_html": body_html,
    });
    if !attachments.is_empty() {
        params
            .as_object_mut()
            .expect("mail.send params are always an object")
            .insert("attachments".to_string(), Value::Array(attachments));
    }
    Ok(EmailRpcCall {
        endpoint: MAIL_RPC_ENDPOINT,
        method: "mail.send",
        params,
    })
}

fn valid_attachment_content_type(value: &str) -> bool {
    let Some((kind, subtype)) = value.split_once('/') else {
        return false;
    };
    !kind.is_empty()
        && !subtype.is_empty()
        && !subtype.contains('/')
        && kind.bytes().all(valid_mime_token_byte)
        && subtype.bytes().all(valid_mime_token_byte)
}

fn valid_mime_token_byte(value: u8) -> bool {
    value.is_ascii_alphanumeric()
        || matches!(
            value,
            b'!' | b'#' | b'$' | b'&' | b'^' | b'_' | b'.' | b'+' | b'-'
        )
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

    pub(crate) fn send(value: Value) -> crate::ImResult<crate::email::SendEmailResult> {
        let object = value.as_object().ok_or_else(mail_send_not_accepted)?;
        let accepted = object.get("accepted").and_then(Value::as_bool);
        let status = object.get("status").and_then(Value::as_str);
        let legacy_flags_are_consistent = ["ok", "success"].iter().all(|key| {
            object
                .get(*key)
                .map(|value| value.as_bool() == Some(true))
                .unwrap_or(true)
        });
        if accepted != Some(true) || status != Some("sent") || !legacy_flags_are_consistent {
            return Err(mail_send_not_accepted());
        }
        let message_id = first_string_value(&value, &["message_id", "messageId", "id"])
            .and_then(|id| crate::email::EmailMessageId::parse(id).ok());
        let warnings = string_array_candidate(&value, &["warnings"]);
        Ok(crate::email::SendEmailResult {
            accepted: true,
            message_id,
            warnings,
        })
    }

    pub(crate) fn attachment(
        request: crate::email::EmailAttachmentDownloadRequest,
        value: Value,
    ) -> crate::ImResult<crate::email::EmailAttachmentContent> {
        let object = value.as_object().cloned().unwrap_or_default();
        let response_index =
            first_strict_u64(&object, &["index", "attachment_index", "attachmentIndex"])
                .ok_or_else(|| crate::ImError::Serialization {
                    detail: "mail attachment response missing index".to_string(),
                })?;
        if response_index != u64::from(request.attachment_index) {
            return Err(crate::ImError::Serialization {
                detail: "mail attachment response index mismatch".to_string(),
            });
        }
        let filename = first_string(&object, &["filename", "name"]).ok_or_else(|| {
            crate::ImError::Serialization {
                detail: "mail attachment response missing filename".to_string(),
            }
        })?;
        if !crate::email::valid_attachment_filename(&filename) {
            return Err(crate::ImError::Serialization {
                detail: "mail attachment response filename is unsafe".to_string(),
            });
        }
        let content_type = first_string(&object, &["content_type", "contentType", "mime_type"])
            .ok_or_else(|| crate::ImError::Serialization {
                detail: "mail attachment response missing content_type".to_string(),
            })?;
        if !super::valid_attachment_content_type(&content_type) {
            return Err(crate::ImError::Serialization {
                detail: "mail attachment response content_type is invalid".to_string(),
            });
        }
        let size =
            first_strict_u64(&object, &["size", "size_bytes", "sizeBytes"]).ok_or_else(|| {
                crate::ImError::Serialization {
                    detail: "mail attachment response missing size".to_string(),
                }
            })?;
        if size > crate::email::EMAIL_ATTACHMENT_MAX_BYTES as u64 {
            return Err(crate::ImError::Serialization {
                detail: "mail attachment response exceeds the supported size".to_string(),
            });
        }
        let content =
            first_string_allow_empty(&object, &["content_base64", "contentBase64", "base64"])
                .ok_or_else(|| crate::ImError::Serialization {
                    detail: "mail attachment response missing content_base64".to_string(),
                })?;
        let expected_encoded_len = usize::try_from(size)
            .ok()
            .and_then(|size| size.checked_add(2))
            .map(|size| size / 3)
            .and_then(|groups| groups.checked_mul(4))
            .ok_or_else(|| crate::ImError::Serialization {
                detail: "mail attachment response size is invalid".to_string(),
            })?;
        if content.len() != expected_encoded_len {
            return Err(crate::ImError::Serialization {
                detail: "mail attachment response encoded size mismatch".to_string(),
            });
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(content.as_bytes())
            .map_err(|err| crate::ImError::Serialization {
                detail: format!("mail attachment base64 decode failed: {err}"),
            })?;
        if base64::engine::general_purpose::STANDARD.encode(&bytes) != content {
            return Err(crate::ImError::Serialization {
                detail: "mail attachment response base64 is not canonical".to_string(),
            });
        }
        if u64::try_from(bytes.len()).ok() != Some(size) {
            return Err(crate::ImError::Serialization {
                detail: "mail attachment response size mismatch".to_string(),
            });
        }
        Ok(crate::email::EmailAttachmentContent {
            message_id: request.message_id,
            attachment_index: request.attachment_index,
            filename,
            content_type,
            size: Some(size),
            bytes,
        })
    }

    fn mail_send_not_accepted() -> crate::ImError {
        crate::ImError::Service {
            status_code: None,
            code: Some("mail.send_not_accepted".to_string()),
            message: "mail send was not explicitly accepted".to_string(),
            data: None,
        }
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
                    .split([',', ';'])
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

    fn first_string_allow_empty(object: &Map<String, Value>, keys: &[&str]) -> Option<String> {
        keys.iter()
            .find_map(|key| object.get(*key).and_then(Value::as_str))
            .map(ToOwned::to_owned)
    }

    fn first_u64(object: &Map<String, Value>, keys: &[&str]) -> Option<u64> {
        keys.iter().find_map(|key| u64_value(object.get(*key)))
    }

    fn first_strict_u64(object: &Map<String, Value>, keys: &[&str]) -> Option<u64> {
        keys.iter().find_map(|key| match object.get(*key) {
            Some(Value::Number(value)) => value.as_u64(),
            Some(Value::String(value)) => value.parse().ok(),
            _ => None,
        })
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
