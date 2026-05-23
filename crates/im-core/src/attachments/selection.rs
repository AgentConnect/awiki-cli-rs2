use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub(crate) const ERR_ATTACHMENT_NOT_FOUND: &str = "attachment not found in message content";
pub(crate) const ERR_ATTACHMENT_ID_REQUIRED: &str =
    "attachment_id is required for messages with multiple attachments";
pub(crate) const ERR_ATTACHMENT_MESSAGE_INVALID: &str = "message is not an attachment manifest";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttachmentSelection {
    pub message_id: String,
    pub requested_id: String,
    pub sender_did: String,
    pub attachment_id: String,
    pub filename: String,
    pub mime_type: String,
    pub size: String,
    pub digest_b64u: String,
    pub object_uri: String,
    pub caption: String,
}

pub(crate) fn find_attachment_selection(
    messages: &[Value],
    requested_message_id: &str,
    requested_attachment_id: &str,
) -> crate::ImResult<AttachmentSelection> {
    for message in messages {
        let Some(message_object) = message.as_object() else {
            continue;
        };
        let view_id = string_from_value(message_object.get("id"));
        let raw_message_id = string_from_value(message_object.get("message_id"));
        let actual_message_id = if raw_message_id.is_empty() {
            view_id.clone()
        } else {
            raw_message_id
        };
        if requested_message_id != view_id && requested_message_id != actual_message_id {
            continue;
        }
        let content = decode_attachment_content(message_object.get("content"))?;
        let attachments = attachments_from_content(content.get("attachments"));
        if attachments.is_empty() {
            return Err(attachment_message_invalid());
        }
        let selected = select_attachment_entry(&attachments, requested_attachment_id)?;
        let access_info = selected
            .get("access_info")
            .and_then(Value::as_object)
            .ok_or_else(attachment_message_invalid)?;
        let digest = selected.get("digest").and_then(Value::as_object);
        return Ok(AttachmentSelection {
            message_id: actual_message_id,
            requested_id: view_id,
            sender_did: string_from_value(message_object.get("sender_did")),
            attachment_id: string_from_value(selected.get("attachment_id")),
            filename: string_from_value(selected.get("filename")),
            mime_type: string_from_value(selected.get("mime_type")),
            size: string_from_value(selected.get("size")),
            digest_b64u: digest
                .and_then(|value| value.get("value_b64u"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            object_uri: access_info
                .get("object_uri")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            caption: string_from_value(content.get("caption")),
        });
    }
    Err(crate::ImError::MessageNotFound {
        message_id: requested_message_id.to_string(),
    })
}

pub(crate) fn find_attachment_selection_with_paging<F>(
    mut fetch_page: F,
    requested_message_id: &str,
    requested_attachment_id: &str,
) -> crate::ImResult<AttachmentSelection>
where
    F: FnMut(i64) -> crate::ImResult<(Vec<Value>, bool)>,
{
    let mut skip = 0_i64;
    loop {
        let (messages, has_more) = fetch_page(skip)?;
        match find_attachment_selection(&messages, requested_message_id, requested_attachment_id) {
            Ok(selection) => return Ok(selection),
            Err(crate::ImError::MessageNotFound { .. }) if has_more && !messages.is_empty() => {
                skip += messages.len() as i64;
            }
            Err(crate::ImError::MessageNotFound { message_id }) => {
                return Err(crate::ImError::MessageNotFound { message_id });
            }
            Err(err) => return Err(err),
        }
    }
}

fn decode_attachment_content(value: Option<&Value>) -> crate::ImResult<Map<String, Value>> {
    match value {
        Some(Value::Object(object)) if !object.is_empty() => Ok(object.clone()),
        Some(Value::String(text)) if !text.trim().is_empty() => {
            let decoded: Value =
                serde_json::from_str(text).map_err(|err| crate::ImError::Serialization {
                    detail: err.to_string(),
                })?;
            decoded
                .as_object()
                .filter(|object| !object.is_empty())
                .cloned()
                .ok_or_else(attachment_message_invalid)
        }
        _ => Err(attachment_message_invalid()),
    }
}

fn attachments_from_content(value: Option<&Value>) -> Vec<Map<String, Value>> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_object().cloned())
                .collect()
        })
        .unwrap_or_default()
}

fn select_attachment_entry(
    attachments: &[Map<String, Value>],
    requested_attachment_id: &str,
) -> crate::ImResult<Map<String, Value>> {
    if attachments.is_empty() {
        return Err(attachment_not_found());
    }
    if requested_attachment_id.trim().is_empty() {
        if attachments.len() > 1 {
            return Err(attachment_id_required());
        }
        return Ok(attachments[0].clone());
    }
    attachments
        .iter()
        .find(|attachment| {
            string_from_value(attachment.get("attachment_id")) == requested_attachment_id
        })
        .cloned()
        .ok_or_else(attachment_not_found)
}

fn string_from_value(value: Option<&Value>) -> String {
    value
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn attachment_not_found() -> crate::ImError {
    crate::ImError::invalid_input(Some("attachment_id".to_string()), ERR_ATTACHMENT_NOT_FOUND)
}

fn attachment_id_required() -> crate::ImError {
    crate::ImError::invalid_input(
        Some("attachment_id".to_string()),
        ERR_ATTACHMENT_ID_REQUIRED,
    )
}

fn attachment_message_invalid() -> crate::ImError {
    crate::ImError::invalid_input(Some("content".to_string()), ERR_ATTACHMENT_MESSAGE_INVALID)
}
