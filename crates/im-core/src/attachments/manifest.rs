use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

const ATTACHMENT_MANIFEST_CONTENT_TYPE: &str = "application/anp-attachment-manifest+json";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreparedAttachment {
    pub filename: String,
    pub mime_type: String,
    pub size_bytes: u64,
    pub size_string: String,
    pub digest_b64u: String,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttachmentDescriptor {
    pub attachment_id: String,
    pub filename: String,
    pub mime_type: String,
    pub size: String,
    pub digest_b64u: String,
    pub object_uri: String,
    pub encryption_mode: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttachmentManifest {
    pub attachments: Vec<AttachmentDescriptor>,
    pub primary_attachment_id: String,
    pub caption: Option<String>,
}

impl From<crate::internal::attachment_runtime::digest::PreparedAttachmentPayload>
    for PreparedAttachment
{
    fn from(value: crate::internal::attachment_runtime::digest::PreparedAttachmentPayload) -> Self {
        Self {
            filename: value.filename,
            mime_type: value.mime_type,
            size_bytes: value.size_bytes,
            size_string: value.size_string,
            digest_b64u: value.digest_b64u,
            payload: value.payload,
        }
    }
}

impl AttachmentDescriptor {
    pub fn from_prepared(
        prepared: &PreparedAttachment,
        attachment_id: impl Into<String>,
        object_uri: impl Into<String>,
    ) -> Self {
        Self {
            attachment_id: attachment_id.into(),
            filename: prepared.filename.clone(),
            mime_type: prepared.mime_type.clone(),
            size: prepared.size_string.clone(),
            digest_b64u: prepared.digest_b64u.clone(),
            object_uri: object_uri.into(),
            encryption_mode: "none".to_string(),
        }
    }
}

pub(crate) fn attachment_manifest_content_type() -> &'static str {
    ATTACHMENT_MANIFEST_CONTENT_TYPE
}

pub(crate) fn prepare_attachment_payload_from_path(
    path: &std::path::Path,
    mime_override: &str,
    payload: Vec<u8>,
) -> crate::ImResult<PreparedAttachment> {
    crate::internal::attachment_runtime::digest::prepare_attachment_payload_from_path(
        path,
        mime_override,
        payload,
    )
    .map(PreparedAttachment::from)
}

pub(crate) fn prepare_attachment_payload(
    filename: &str,
    mime_override: &str,
    payload: Vec<u8>,
) -> crate::ImResult<PreparedAttachment> {
    crate::internal::attachment_runtime::digest::prepare_attachment_payload(
        filename,
        mime_override,
        payload,
    )
    .map(PreparedAttachment::from)
}

pub(crate) fn build_attachment_manifest(descriptor: &AttachmentDescriptor, caption: &str) -> Value {
    let manifest = AttachmentManifest {
        attachments: vec![descriptor.clone()],
        primary_attachment_id: descriptor.attachment_id.clone(),
        caption: if caption.trim().is_empty() {
            None
        } else {
            Some(caption.to_string())
        },
    };
    manifest_to_value(&manifest)
}

pub(crate) fn manifest_content_string(manifest: &Value) -> String {
    serde_json::to_string(manifest).unwrap_or_default()
}

fn manifest_to_value(manifest: &AttachmentManifest) -> Value {
    let mut value = Map::new();
    value.insert(
        "attachments".to_string(),
        Value::Array(
            manifest
                .attachments
                .iter()
                .map(attachment_descriptor_to_value)
                .collect(),
        ),
    );
    value.insert(
        "primary_attachment_id".to_string(),
        Value::String(manifest.primary_attachment_id.clone()),
    );
    if let Some(caption) = manifest.caption.as_ref() {
        value.insert("caption".to_string(), Value::String(caption.clone()));
    }
    Value::Object(value)
}

fn attachment_descriptor_to_value(descriptor: &AttachmentDescriptor) -> Value {
    json!({
        "attachment_id": descriptor.attachment_id,
        "filename": descriptor.filename,
        "mime_type": descriptor.mime_type,
        "size": descriptor.size,
        "digest": {
            "alg": "sha-256",
            "value_b64u": descriptor.digest_b64u,
        },
        "access_info": {
            "object_uri": descriptor.object_uri,
        },
        "encryption_info": {
            "mode": if descriptor.encryption_mode.trim().is_empty() {
                "none"
            } else {
                descriptor.encryption_mode.as_str()
            },
        },
    })
}
