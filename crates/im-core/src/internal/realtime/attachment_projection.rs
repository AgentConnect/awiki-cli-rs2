use serde_json::{Map, Value};

use crate::ids::MessageId;
use crate::messages::ThreadRef;
use crate::realtime::{AttachmentDownloadAction, AttachmentMessageSummary};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AttachmentProjection {
    pub summary: Option<AttachmentMessageSummary>,
    pub download_action: Option<AttachmentDownloadAction>,
    pub warnings: Vec<String>,
}

pub(crate) fn project_attachment(
    content_type: &str,
    body: Option<&Map<String, Value>>,
    thread: &ThreadRef,
    message_id: &MessageId,
) -> Option<AttachmentProjection> {
    if content_type != crate::attachments::manifest::attachment_manifest_content_type() {
        return None;
    }
    let mut warnings = Vec::new();
    let Some(manifest) = manifest_from_body(body) else {
        warnings.push("attachment manifest payload is missing or invalid".to_string());
        return Some(AttachmentProjection {
            summary: None,
            download_action: None,
            warnings,
        });
    };

    let primary_attachment_id = string_from_object(Some(manifest), "primary_attachment_id");
    let attachments = manifest
        .get("attachments")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    let selected = select_attachment(attachments, &primary_attachment_id);
    if attachments.is_empty() {
        warnings.push("attachment manifest has no attachments".to_string());
    }
    if selected.is_none() {
        warnings.push("attachment manifest primary attachment is missing".to_string());
    }
    if primary_attachment_id.trim().is_empty() {
        warnings.push("attachment manifest primary_attachment_id is missing".to_string());
    }

    let Some(selected) = selected else {
        return Some(AttachmentProjection {
            summary: None,
            download_action: None,
            warnings,
        });
    };

    if let Some(mode) =
        unsupported_encryption_mode(selected).or_else(|| unsupported_encryption_mode(manifest))
    {
        warnings.push(format!(
            "attachment manifest encryption mode {mode} is not supported by realtime projection"
        ));
        return Some(AttachmentProjection {
            summary: None,
            download_action: None,
            warnings,
        });
    }

    let attachment_id = string_from_value(selected.get("attachment_id"))
        .or_else(|| non_empty_string(primary_attachment_id));
    let filename = string_from_value(selected.get("filename"));
    let mime_type = string_from_value(selected.get("mime_type"));
    let size_bytes =
        string_from_value(selected.get("size")).and_then(|size| size.trim().parse::<u64>().ok());
    if attachment_id.is_none() {
        warnings.push("attachment manifest attachment_id is missing".to_string());
    }
    if filename.is_none() {
        warnings.push("attachment manifest filename is missing".to_string());
    }
    if mime_type.is_none() {
        warnings.push("attachment manifest mime_type is missing".to_string());
    }
    if size_bytes.is_none() {
        warnings.push("attachment manifest size_bytes is missing".to_string());
    }

    Some(AttachmentProjection {
        download_action: Some(AttachmentDownloadAction {
            thread: thread.clone(),
            message_id: message_id.clone(),
            attachment_id: attachment_id.clone(),
        }),
        summary: Some(AttachmentMessageSummary {
            attachment_id,
            filename,
            mime_type,
            size_bytes,
            content_type: Some(content_type.to_string()),
        }),
        warnings,
    })
}

pub(crate) fn attachment_summary_attributes(
    summary: &AttachmentMessageSummary,
) -> Vec<crate::messages::MessageMetadataAttribute> {
    let mut attributes = Vec::new();
    push_attribute(
        &mut attributes,
        "attachment_id",
        summary.attachment_id.as_deref(),
    );
    push_attribute(
        &mut attributes,
        "attachment_filename",
        summary.filename.as_deref(),
    );
    push_attribute(
        &mut attributes,
        "attachment_mime_type",
        summary.mime_type.as_deref(),
    );
    let size = summary.size_bytes.map(|value| value.to_string());
    push_attribute(&mut attributes, "attachment_size_bytes", size.as_deref());
    push_attribute(
        &mut attributes,
        "attachment_content_type",
        summary.content_type.as_deref(),
    );
    attributes
}

fn manifest_from_body(body: Option<&Map<String, Value>>) -> Option<&Map<String, Value>> {
    if let Some(payload) = body
        .and_then(|body| body.get("payload"))
        .and_then(Value::as_object)
    {
        return Some(payload);
    }
    body.filter(|body| {
        body.contains_key("attachments") || body.contains_key("primary_attachment_id")
    })
}

fn select_attachment<'a>(
    attachments: &'a [Value],
    primary_attachment_id: &str,
) -> Option<&'a Map<String, Value>> {
    if !primary_attachment_id.trim().is_empty() {
        let selected = attachments.iter().find_map(|value| {
            let object = value.as_object()?;
            (string_from_object(Some(object), "attachment_id") == primary_attachment_id)
                .then_some(object)
        });
        if selected.is_some() {
            return selected;
        }
    }
    attachments.first().and_then(Value::as_object)
}

fn unsupported_encryption_mode(object: &Map<String, Value>) -> Option<String> {
    let mode = string_from_value(
        object
            .get("encryption_info")
            .and_then(Value::as_object)
            .and_then(|encryption| encryption.get("mode")),
    )
    .or_else(|| string_from_value(object.get("encryption_mode")))
    .or_else(|| string_from_value(object.get("object_encryption_mode")))?;
    if mode.eq_ignore_ascii_case("none") {
        None
    } else {
        Some(mode)
    }
}

fn push_attribute(
    attributes: &mut Vec<crate::messages::MessageMetadataAttribute>,
    key: &str,
    value: Option<&str>,
) {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return;
    };
    attributes.push(crate::messages::MessageMetadataAttribute {
        key: key.to_string(),
        value: value.to_string(),
    });
}

fn string_from_object(object: Option<&Map<String, Value>>, key: &str) -> String {
    object
        .and_then(|object| object.get(key))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn string_from_value(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn non_empty_string(value: String) -> Option<String> {
    if value.trim().is_empty() {
        None
    } else {
        Some(value)
    }
}
