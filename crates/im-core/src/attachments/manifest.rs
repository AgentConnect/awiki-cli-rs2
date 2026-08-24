use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

const ATTACHMENT_MANIFEST_CONTENT_TYPE: &str = "application/anp-attachment-manifest+json";
pub(crate) const OBJECT_ENCRYPTION_MODE_NONE: &str = "none";
pub(crate) const OBJECT_ENCRYPTION_MODE_E2EE: &str = "object-e2ee";
const DIGEST_ALG_SHA256: &str = "sha-256";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreparedAttachment {
    pub filename: String,
    pub mime_type: String,
    pub size_bytes: u64,
    pub size_string: String,
    pub digest_b64u: String,
    pub payload: Vec<u8>,
    pub object_encryption_mode: String,
    pub object_cipher: Option<String>,
    pub plaintext_size_bytes: Option<u64>,
    pub plaintext_size_string: Option<String>,
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
    pub object_cipher: Option<String>,
    pub plaintext_size: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttachmentManifest {
    pub attachments: Vec<AttachmentDescriptor>,
    pub primary_attachment_id: String,
    pub caption: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mention_payload: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AttachmentGrantRef {
    pub attachment_id: String,
    pub object_uri: String,
    pub size: String,
    pub digest_b64u: String,
    pub mime_type: String,
    pub object_encryption_mode: String,
    pub plaintext_size: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ObjectE2eeAttachmentSecrets {
    pub object_key_b64u: String,
    pub nonce_b64u: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PreparedObjectE2eeAttachment {
    pub prepared: PreparedAttachment,
    pub secrets: ObjectE2eeAttachmentSecrets,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParsedAttachmentManifest {
    pub attachments: Vec<ParsedAttachmentDescriptor>,
    pub primary_attachment_id: String,
    pub caption: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParsedAttachmentDescriptor {
    pub descriptor: AttachmentDescriptor,
    pub object_key_b64u: Option<String>,
    pub nonce_b64u: Option<String>,
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
            object_encryption_mode: OBJECT_ENCRYPTION_MODE_NONE.to_string(),
            object_cipher: None,
            plaintext_size_bytes: None,
            plaintext_size_string: None,
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
            encryption_mode: prepared.object_encryption_mode(),
            object_cipher: prepared.object_cipher.clone(),
            plaintext_size: prepared.plaintext_size_string.clone(),
        }
    }

    pub(crate) fn object_encryption_mode(&self) -> String {
        normalized_object_encryption_mode(&self.encryption_mode)
    }
}

impl PreparedAttachment {
    pub(crate) fn object_encryption_mode(&self) -> String {
        normalized_object_encryption_mode(&self.object_encryption_mode)
    }

    pub(crate) fn is_object_e2ee(&self) -> bool {
        self.object_encryption_mode() == OBJECT_ENCRYPTION_MODE_E2EE
    }
}

impl AttachmentGrantRef {
    pub(crate) fn from_descriptor(descriptor: &AttachmentDescriptor) -> crate::ImResult<Self> {
        if descriptor.attachment_id.trim().is_empty() {
            return Err(crate::ImError::invalid_input(
                Some("attachment_id".to_string()),
                "attachment_id is required",
            ));
        }
        if descriptor.object_uri.trim().is_empty() {
            return Err(crate::ImError::invalid_input(
                Some("object_uri".to_string()),
                "object_uri is required",
            ));
        }
        let mode = descriptor.object_encryption_mode();
        let plaintext_size = descriptor.plaintext_size.as_ref().and_then(|value| {
            let value = value.trim();
            (!value.is_empty()).then(|| value.to_string())
        });
        if mode == OBJECT_ENCRYPTION_MODE_E2EE && plaintext_size.is_none() {
            return Err(crate::ImError::invalid_input(
                Some("plaintext_size".to_string()),
                "plaintext_size is required for object-e2ee attachments",
            ));
        }
        Ok(Self {
            attachment_id: descriptor.attachment_id.clone(),
            object_uri: descriptor.object_uri.clone(),
            size: descriptor.size.clone(),
            digest_b64u: descriptor.digest_b64u.clone(),
            mime_type: descriptor.mime_type.clone(),
            object_encryption_mode: mode,
            plaintext_size,
        })
    }

    pub(crate) fn to_value(&self) -> Value {
        let mut object = Map::new();
        object.insert(
            "attachment_id".to_string(),
            Value::String(self.attachment_id.clone()),
        );
        object.insert(
            "object_uri".to_string(),
            Value::String(self.object_uri.clone()),
        );
        object.insert("size".to_string(), Value::String(self.size.clone()));
        object.insert(
            "digest".to_string(),
            json!({
                "alg": DIGEST_ALG_SHA256,
                "value_b64u": self.digest_b64u,
            }),
        );
        object.insert(
            "mime_type".to_string(),
            Value::String(self.mime_type.clone()),
        );
        object.insert(
            "object_encryption_mode".to_string(),
            Value::String(self.object_encryption_mode.clone()),
        );
        if let Some(plaintext_size) = self.plaintext_size.as_ref() {
            object.insert(
                "plaintext_size".to_string(),
                Value::String(plaintext_size.clone()),
            );
        }
        Value::Object(object)
    }
}

pub fn attachment_manifest_content_type() -> &'static str {
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

pub(crate) fn prepare_object_e2ee_attachment_payload(
    filename: &str,
    mime_override: &str,
    plaintext: Vec<u8>,
) -> crate::ImResult<PreparedObjectE2eeAttachment> {
    let plain = crate::internal::attachment_runtime::digest::prepare_attachment_payload(
        filename,
        mime_override,
        plaintext,
    )?;
    let encrypted =
        crate::internal::attachment_runtime::object_crypto::encrypt_object_e2ee(&plain.payload)?;
    Ok(PreparedObjectE2eeAttachment {
        secrets: ObjectE2eeAttachmentSecrets {
            object_key_b64u: encrypted.crypto.object_key_b64u,
            nonce_b64u: encrypted.crypto.nonce_b64u,
        },
        prepared: PreparedAttachment {
            filename: plain.filename,
            mime_type: plain.mime_type,
            size_bytes: encrypted.ciphertext_size_bytes,
            size_string: encrypted.ciphertext_size_string,
            digest_b64u: encrypted.ciphertext_digest_b64u,
            payload: encrypted.ciphertext,
            object_encryption_mode: OBJECT_ENCRYPTION_MODE_E2EE.to_string(),
            object_cipher: Some(encrypted.crypto.object_cipher),
            plaintext_size_bytes: Some(encrypted.plaintext_size_bytes),
            plaintext_size_string: Some(encrypted.plaintext_size_string),
        },
    })
}

pub(crate) async fn prepare_attachment_metadata_from_path(
    path: &std::path::Path,
    mime_override: &str,
) -> crate::ImResult<PreparedAttachment> {
    crate::internal::attachment_runtime::digest::prepare_attachment_metadata_from_path(
        path,
        mime_override,
    )
    .await
    .map(PreparedAttachment::from)
}

pub(crate) fn build_attachment_manifest(
    descriptor: &AttachmentDescriptor,
    caption: &str,
) -> crate::ImResult<Value> {
    Ok(redact_attachment_manifest(
        &build_attachment_manifest_internal(descriptor, caption)?,
    ))
}

pub(crate) fn build_attachment_manifest_with_mention_payload(
    descriptor: &AttachmentDescriptor,
    caption: &str,
    mention_payload: Option<&Value>,
) -> crate::ImResult<Value> {
    Ok(redact_attachment_manifest(
        &build_attachment_manifest_internal_with_mention_payload(
            descriptor,
            caption,
            mention_payload,
        )?,
    ))
}

pub(crate) fn build_attachment_manifest_internal(
    descriptor: &AttachmentDescriptor,
    caption: &str,
) -> crate::ImResult<Value> {
    build_attachment_manifest_internal_with_mention_payload(descriptor, caption, None)
}

pub(crate) fn build_attachment_manifest_internal_with_mention_payload(
    descriptor: &AttachmentDescriptor,
    caption: &str,
    mention_payload: Option<&Value>,
) -> crate::ImResult<Value> {
    build_attachment_manifest_value(descriptor, caption, mention_payload, None)
}

pub(crate) fn build_attachment_manifest_with_object_e2ee_secrets(
    descriptor: &AttachmentDescriptor,
    caption: &str,
    secrets: &ObjectE2eeAttachmentSecrets,
) -> crate::ImResult<Value> {
    build_attachment_manifest_value(descriptor, caption, None, Some(secrets))
}

pub(crate) fn build_attachment_manifest_with_object_e2ee_secrets_and_mention_payload(
    descriptor: &AttachmentDescriptor,
    caption: &str,
    secrets: &ObjectE2eeAttachmentSecrets,
    mention_payload: Option<&Value>,
) -> crate::ImResult<Value> {
    build_attachment_manifest_value(descriptor, caption, mention_payload, Some(secrets))
}

fn build_attachment_manifest_value(
    descriptor: &AttachmentDescriptor,
    caption: &str,
    mention_payload: Option<&Value>,
    secrets: Option<&ObjectE2eeAttachmentSecrets>,
) -> crate::ImResult<Value> {
    let mention_payload = mention_payload
        .map(|payload| {
            crate::messages::validate_message_mention_payload(payload)?;
            Ok::<Value, crate::ImError>(payload.clone())
        })
        .transpose()?;
    let manifest = AttachmentManifest {
        attachments: vec![descriptor.clone()],
        primary_attachment_id: descriptor.attachment_id.clone(),
        caption: if caption.trim().is_empty() {
            None
        } else {
            Some(caption.to_string())
        },
        mention_payload,
    };
    Ok(manifest_to_value(&manifest, secrets))
}

pub fn manifest_content_string(manifest: &Value) -> String {
    serde_json::to_string(manifest).unwrap_or_default()
}

pub(crate) fn redact_attachment_manifest(manifest: &Value) -> Value {
    let mut redacted = manifest.clone();
    if let Some(attachments) = redacted
        .get_mut("attachments")
        .and_then(Value::as_array_mut)
    {
        for attachment in attachments {
            if let Some(encryption) = attachment
                .get_mut("encryption_info")
                .and_then(Value::as_object_mut)
            {
                encryption.remove("object_key_b64u");
                encryption.remove("nonce_b64u");
            }
        }
    }
    redacted
}

pub(crate) fn build_attachment_grant_ref(
    descriptor: &AttachmentDescriptor,
) -> crate::ImResult<Value> {
    AttachmentGrantRef::from_descriptor(descriptor).map(|grant| grant.to_value())
}

/// Parses the public, redacted attachment manifest carried by a projected
/// message. Secret object keys and nonces are never returned.
pub fn parse_attachment_manifest(value: &Value) -> crate::ImResult<AttachmentManifest> {
    let parsed = parse_attachment_manifest_internal(value)?;
    Ok(AttachmentManifest {
        attachments: parsed
            .attachments
            .into_iter()
            .map(|attachment| attachment.descriptor)
            .collect(),
        primary_attachment_id: parsed.primary_attachment_id,
        caption: parsed.caption,
        mention_payload: None,
    })
}

pub(crate) fn parse_attachment_manifest_internal(
    value: &Value,
) -> crate::ImResult<ParsedAttachmentManifest> {
    let object = value.as_object().ok_or_else(|| {
        crate::ImError::invalid_input(
            Some("manifest".to_string()),
            "attachment manifest must be a JSON object",
        )
    })?;
    let attachments = object
        .get("attachments")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            crate::ImError::invalid_input(
                Some("attachments".to_string()),
                "attachment manifest attachments must be an array",
            )
        })?
        .iter()
        .map(parse_attachment_descriptor_internal)
        .collect::<crate::ImResult<Vec<_>>>()?;
    Ok(ParsedAttachmentManifest {
        attachments,
        primary_attachment_id: string_value(object.get("primary_attachment_id")),
        caption: optional_string_value(object.get("caption")),
    })
}

fn manifest_to_value(
    manifest: &AttachmentManifest,
    secrets: Option<&ObjectE2eeAttachmentSecrets>,
) -> Value {
    let mut value = Map::new();
    value.insert(
        "attachments".to_string(),
        Value::Array(
            manifest
                .attachments
                .iter()
                .map(|descriptor| attachment_descriptor_to_value(descriptor, secrets))
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
    if let Some(mention_payload) = manifest.mention_payload.as_ref() {
        if let Some(text) = mention_payload.get("text") {
            value.insert("text".to_string(), text.clone());
        }
        if let Some(mentions) = mention_payload.get("mentions") {
            value.insert("mentions".to_string(), mentions.clone());
        }
        if let Some(annotations) = mention_payload.get("annotations") {
            value.insert("annotations".to_string(), annotations.clone());
        }
    }
    Value::Object(value)
}

fn attachment_descriptor_to_value(
    descriptor: &AttachmentDescriptor,
    secrets: Option<&ObjectE2eeAttachmentSecrets>,
) -> Value {
    let mut encryption_info = Map::new();
    let mode = descriptor.object_encryption_mode();
    encryption_info.insert("mode".to_string(), Value::String(mode.clone()));
    if mode == OBJECT_ENCRYPTION_MODE_E2EE {
        encryption_info.insert(
            "object_cipher".to_string(),
            Value::String(descriptor.object_cipher.clone().unwrap_or_else(|| {
                crate::internal::attachment_runtime::object_crypto::OBJECT_E2EE_CIPHER.to_string()
            })),
        );
        if let Some(secrets) = secrets {
            encryption_info.insert(
                "object_key_b64u".to_string(),
                Value::String(secrets.object_key_b64u.clone()),
            );
            encryption_info.insert(
                "nonce_b64u".to_string(),
                Value::String(secrets.nonce_b64u.clone()),
            );
        }
        if let Some(plaintext_size) = descriptor.plaintext_size.as_ref() {
            encryption_info.insert(
                "plaintext_size".to_string(),
                Value::String(plaintext_size.clone()),
            );
        }
    }

    json!({
        "attachment_id": descriptor.attachment_id,
        "filename": descriptor.filename,
        "mime_type": descriptor.mime_type,
        "size": descriptor.size,
        "digest": {
            "alg": DIGEST_ALG_SHA256,
            "value_b64u": descriptor.digest_b64u,
        },
        "access_info": {
            "object_uri": descriptor.object_uri,
        },
        "encryption_info": Value::Object(encryption_info),
    })
}

fn parse_attachment_descriptor_internal(
    value: &Value,
) -> crate::ImResult<ParsedAttachmentDescriptor> {
    let object = value.as_object().ok_or_else(|| {
        crate::ImError::invalid_input(
            Some("attachments".to_string()),
            "attachment entry must be a JSON object",
        )
    })?;
    let digest = object.get("digest").and_then(Value::as_object);
    let access_info = object.get("access_info").and_then(Value::as_object);
    let encryption_info = object.get("encryption_info").and_then(Value::as_object);
    let mode = encryption_info
        .and_then(|value| value.get("mode"))
        .and_then(Value::as_str)
        .unwrap_or(OBJECT_ENCRYPTION_MODE_NONE);
    let descriptor = AttachmentDescriptor {
        attachment_id: string_value(object.get("attachment_id")),
        filename: string_value(object.get("filename")),
        mime_type: string_value(object.get("mime_type")),
        size: string_value(object.get("size")),
        digest_b64u: digest
            .and_then(|value| value.get("value_b64u"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        object_uri: access_info
            .and_then(|value| value.get("object_uri"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        encryption_mode: normalized_object_encryption_mode(mode),
        object_cipher: encryption_info
            .and_then(|value| value.get("object_cipher"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        plaintext_size: encryption_info
            .and_then(|value| value.get("plaintext_size"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
    };
    Ok(ParsedAttachmentDescriptor {
        descriptor,
        object_key_b64u: encryption_info
            .and_then(|value| value.get("object_key_b64u"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        nonce_b64u: encryption_info
            .and_then(|value| value.get("nonce_b64u"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
    })
}

fn normalized_object_encryption_mode(value: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        OBJECT_ENCRYPTION_MODE_NONE.to_string()
    } else {
        value.to_string()
    }
}

fn string_value(value: Option<&Value>) -> String {
    value
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn optional_string_value(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attachment_manifest_internal_full_redacted_and_parsed_secrets() {
        let e2ee = prepare_object_e2ee_attachment_payload(
            "secret.pdf",
            "application/pdf",
            b"plaintext report".to_vec(),
        )
        .expect("encrypted attachment");
        let descriptor = AttachmentDescriptor::from_prepared(
            &e2ee.prepared,
            "att-e2ee-1",
            "https://objects.example/att-e2ee-1",
        );

        let full = build_attachment_manifest_with_object_e2ee_secrets(
            &descriptor,
            "secret",
            &e2ee.secrets,
        )
        .expect("full manifest");
        assert_eq!(
            full["attachments"][0]["encryption_info"]["object_key_b64u"],
            e2ee.secrets.object_key_b64u
        );
        assert_eq!(
            full["attachments"][0]["encryption_info"]["nonce_b64u"],
            e2ee.secrets.nonce_b64u
        );

        let redacted = redact_attachment_manifest(&full);
        assert_eq!(
            redacted["attachments"][0]["encryption_info"]["mode"],
            OBJECT_ENCRYPTION_MODE_E2EE
        );
        assert_eq!(
            redacted["attachments"][0]["encryption_info"]["plaintext_size"],
            "16"
        );
        assert_eq!(
            redacted["attachments"][0]["encryption_info"].get("object_key_b64u"),
            None
        );
        assert_eq!(
            redacted["attachments"][0]["encryption_info"].get("nonce_b64u"),
            None
        );
        assert_eq!(
            build_attachment_manifest(&descriptor, "secret").expect("redacted manifest"),
            redacted
        );

        let parsed = parse_attachment_manifest_internal(&full).expect("full manifest parses");
        let parsed_attachment = &parsed.attachments[0];
        assert_eq!(
            parsed_attachment.object_key_b64u.as_deref(),
            Some(e2ee.secrets.object_key_b64u.as_str())
        );
        assert_eq!(
            parsed_attachment.nonce_b64u.as_deref(),
            Some(e2ee.secrets.nonce_b64u.as_str())
        );
        assert_eq!(
            parsed_attachment.descriptor.object_encryption_mode(),
            OBJECT_ENCRYPTION_MODE_E2EE
        );
    }

    #[test]
    fn attachment_manifest_can_carry_structured_mention_payload() {
        let prepared =
            prepare_attachment_payload("report.md", "text/markdown", b"# Report".to_vec())
                .expect("prepared attachment");
        let descriptor = AttachmentDescriptor::from_prepared(
            &prepared,
            "att-mention-1",
            "https://objects.example/att-mention-1",
        );
        let mention_payload = json!({
            "text": "@Hermes 看看这个文件",
            "mentions": [{
                "id": "men_agent",
                "range": {"start": 0, "end": 7, "unit": "unicode_code_point"},
                "target": {"kind": "agent", "did": "did:agent:hermes"},
                "mention_role": "addressee"
            }]
        });

        let manifest = build_attachment_manifest_with_mention_payload(
            &descriptor,
            "@Hermes 看看这个文件",
            Some(&mention_payload),
        )
        .expect("mention attachment manifest");

        assert_eq!(manifest["caption"], "@Hermes 看看这个文件");
        assert_eq!(manifest["text"], "@Hermes 看看这个文件");
        assert_eq!(manifest["mentions"][0]["id"], "men_agent");
        assert_eq!(manifest["attachments"][0]["attachment_id"], "att-mention-1");
        crate::messages::parse_message_mention_payload(&manifest)
            .expect("attachment manifest is also a valid mention payload");
    }

    #[test]
    fn attachment_manifest_rejects_invalid_mention_payload() {
        let prepared =
            prepare_attachment_payload("report.md", "text/markdown", b"# Report".to_vec())
                .expect("prepared attachment");
        let descriptor = AttachmentDescriptor::from_prepared(
            &prepared,
            "att-mention-1",
            "https://objects.example/att-mention-1",
        );
        let invalid = json!({
            "text": "@Hermes",
            "mentions": [{
                "id": "men_agent",
                "range": {"start": 0, "end": 100, "unit": "unicode_code_point"},
                "target": {"kind": "agent", "did": "did:agent:hermes"}
            }]
        });

        assert!(build_attachment_manifest_with_mention_payload(
            &descriptor,
            "@Hermes",
            Some(&invalid)
        )
        .is_err());
    }
}
