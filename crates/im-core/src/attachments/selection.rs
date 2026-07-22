use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub const ERR_ATTACHMENT_NOT_FOUND: &str = "attachment not found in message content";
pub const ERR_ATTACHMENT_ID_REQUIRED: &str =
    "attachment_id is required for messages with multiple attachments";
pub const ERR_ATTACHMENT_MESSAGE_INVALID: &str = "message is not an attachment manifest";

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
    pub message_security_profile: String,
    pub object_encryption_mode: String,
    pub object_cipher: Option<String>,
    pub plaintext_size: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct InternalAttachmentSelection {
    pub public: AttachmentSelection,
    pub(crate) authorization_message_id: String,
    pub(crate) message_target_did: String,
    pub object_key_b64u: Option<String>,
    pub nonce_b64u: Option<String>,
}

pub(crate) fn find_attachment_selection(
    messages: &[Value],
    requested_message_id: &str,
    requested_attachment_id: &str,
) -> crate::ImResult<AttachmentSelection> {
    find_internal_attachment_selection(messages, requested_message_id, requested_attachment_id)
        .map(|selection| selection.redacted_public())
}

pub(crate) fn find_internal_attachment_selection(
    messages: &[Value],
    requested_message_id: &str,
    requested_attachment_id: &str,
) -> crate::ImResult<InternalAttachmentSelection> {
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
        let encryption_info = selected.get("encryption_info").and_then(Value::as_object);
        let message_security_profile =
            message_security_profile_from_message(message_object, encryption_info);
        let object_encryption_mode = encryption_info
            .and_then(|value| value.get("mode"))
            .and_then(Value::as_str)
            .unwrap_or(crate::attachments::manifest::OBJECT_ENCRYPTION_MODE_NONE)
            .trim()
            .to_string();
        let raw_authorization_message_id = string_from_value(message_object.get("raw_message_id"));
        let authorization_message_id = attachment_authorization_message_id(
            &message_security_profile,
            &view_id,
            &actual_message_id,
            &raw_authorization_message_id,
        );
        return Ok(InternalAttachmentSelection {
            public: AttachmentSelection {
                message_id: actual_message_id.clone(),
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
                message_security_profile,
                object_encryption_mode: if object_encryption_mode.is_empty() {
                    crate::attachments::manifest::OBJECT_ENCRYPTION_MODE_NONE.to_string()
                } else {
                    object_encryption_mode
                },
                object_cipher: encryption_info
                    .and_then(|value| value.get("object_cipher"))
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned),
                plaintext_size: encryption_info
                    .and_then(|value| value.get("plaintext_size"))
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned),
            },
            authorization_message_id,
            message_target_did: string_from_value(message_object.get("receiver_did")),
            object_key_b64u: encryption_info
                .and_then(|value| value.get("object_key_b64u"))
                .and_then(Value::as_str)
                .map(ToOwned::to_owned),
            nonce_b64u: encryption_info
                .and_then(|value| value.get("nonce_b64u"))
                .and_then(Value::as_str)
                .map(ToOwned::to_owned),
        });
    }
    Err(crate::ImError::MessageNotFound {
        message_id: requested_message_id.to_string(),
    })
}

fn attachment_authorization_message_id(
    message_security_profile: &str,
    view_id: &str,
    actual_message_id: &str,
    raw_message_id: &str,
) -> String {
    if message_security_profile == "group-e2ee" && !view_id.trim().is_empty() {
        return view_id.to_owned();
    }
    if raw_message_id.trim().is_empty() {
        actual_message_id.to_owned()
    } else {
        raw_message_id.to_owned()
    }
}

pub(crate) fn find_attachment_selection_with_paging<F>(
    fetch_page: F,
    requested_message_id: &str,
    requested_attachment_id: &str,
) -> crate::ImResult<AttachmentSelection>
where
    F: FnMut(i64) -> crate::ImResult<(Vec<Value>, bool)>,
{
    find_internal_attachment_selection_with_paging(
        fetch_page,
        requested_message_id,
        requested_attachment_id,
    )
    .map(|selection| selection.redacted_public())
}

pub(crate) fn find_internal_attachment_selection_with_paging<F>(
    mut fetch_page: F,
    requested_message_id: &str,
    requested_attachment_id: &str,
) -> crate::ImResult<InternalAttachmentSelection>
where
    F: FnMut(i64) -> crate::ImResult<(Vec<Value>, bool)>,
{
    let mut skip = 0_i64;
    loop {
        let (messages, has_more) = fetch_page(skip)?;
        match find_internal_attachment_selection(
            &messages,
            requested_message_id,
            requested_attachment_id,
        ) {
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

impl InternalAttachmentSelection {
    pub(crate) fn redacted_public(&self) -> AttachmentSelection {
        self.public.clone()
    }

    pub(crate) fn message_security_profile(&self) -> &str {
        self.public.message_security_profile.trim()
    }

    pub(crate) fn object_encryption_mode(&self) -> &str {
        self.public.object_encryption_mode.trim()
    }

    pub(crate) fn is_object_e2ee(&self) -> bool {
        self.object_encryption_mode() == crate::attachments::manifest::OBJECT_ENCRYPTION_MODE_E2EE
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

fn message_security_profile_from_message(
    message: &Map<String, Value>,
    _encryption_info: Option<&Map<String, Value>>,
) -> String {
    for candidate in [
        message.get("message_security_profile"),
        message.get("security_profile"),
        message.get("security"),
        message
            .get("metadata")
            .and_then(Value::as_object)
            .and_then(|metadata| metadata.get("message_security_profile")),
        message
            .get("metadata")
            .and_then(Value::as_object)
            .and_then(|metadata| metadata.get("security")),
    ] {
        let value = candidate.and_then(Value::as_str).unwrap_or_default().trim();
        if !value.is_empty() {
            return normalize_message_security_profile(value);
        }
    }
    String::new()
}

fn normalize_message_security_profile(value: &str) -> String {
    match value.trim() {
        "secure-direct" | "direct" | "direct_e2ee" | "e2ee" => "direct-e2ee".to_string(),
        "group" | "group_e2ee" => "group-e2ee".to_string(),
        "" => "transport-protected".to_string(),
        value => value.to_string(),
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_e2ee_authorizes_with_canonical_timeline_message_id() {
        assert_eq!(
            attachment_authorization_message_id(
                "group-e2ee",
                "did:example:group:15",
                "logical-message-15",
                "logical-message-15",
            ),
            "did:example:group:15"
        );
    }

    #[test]
    fn direct_e2ee_authorizes_with_raw_wire_message_id() {
        assert_eq!(
            attachment_authorization_message_id(
                "direct-e2ee",
                "logical-direct-message",
                "logical-direct-message",
                "wire-direct-message",
            ),
            "wire-direct-message"
        );
    }
}
