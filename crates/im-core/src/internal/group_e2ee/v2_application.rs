//! Product application payloads for the device-scoped P6 v2 MLS runtime.
//!
//! Object bytes are uploaded by the attachment runtime before this boundary.
//! This module only places the already committed object's full manifest inside
//! one MLS application plaintext, keeps the local/UI projection redacted, and
//! exposes only its non-secret grant ref as AWiki-local delivery metadata.

use anp::group_e2ee::V2GroupApplicationPlaintext;
use serde_json::{Map, Value};

/// A prepared P6 v2 application body and its secret-free local projection.
///
/// This type intentionally has no `Debug` implementation: an attachment body
/// contains the object key until MLS encryption has completed.
pub(crate) struct V2ProductApplication {
    plaintext: V2GroupApplicationPlaintext,
    projection: V2ApplicationProjection,
    client_context: Option<Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct V2ApplicationProjection {
    pub(crate) application_content_type: String,
    pub(crate) text: Option<String>,
    pub(crate) payload: Option<Value>,
}

impl V2ProductApplication {
    pub(crate) fn text(
        group_did: &str,
        application_content_type: impl Into<String>,
        text: impl Into<String>,
    ) -> crate::ImResult<Self> {
        let application_content_type = application_content_type.into();
        let text = text.into();
        let plaintext = V2GroupApplicationPlaintext {
            application_content_type: application_content_type.clone(),
            thread_id: Some(require_group_did(group_did)?.to_owned()),
            reply_to_message_id: None,
            annotations: None,
            text: Some(text.clone()),
            payload: None,
            payload_b64u: None,
        };
        validate_plaintext(&plaintext)?;
        Ok(Self {
            plaintext,
            projection: V2ApplicationProjection {
                application_content_type,
                text: Some(text),
                payload: None,
            },
            client_context: None,
        })
    }

    pub(crate) fn json(group_did: &str, payload: Value) -> crate::ImResult<Self> {
        if is_reserved_control_payload(&payload) {
            return Err(crate::ImError::PermissionDenied);
        }
        let plaintext = V2GroupApplicationPlaintext {
            application_content_type: "application/json".to_owned(),
            thread_id: Some(require_group_did(group_did)?.to_owned()),
            reply_to_message_id: None,
            annotations: None,
            text: None,
            payload: Some(payload.clone()),
            payload_b64u: None,
        };
        validate_plaintext(&plaintext)?;
        Ok(Self {
            plaintext,
            projection: V2ApplicationProjection {
                application_content_type: "application/json".to_owned(),
                text: None,
                payload: Some(payload),
            },
            client_context: None,
        })
    }

    /// Wraps one already uploaded attachment object without uploading,
    /// re-encrypting, or duplicating it per MLS Leaf.
    pub(crate) fn committed_attachment(
        group_did: &str,
        committed: &crate::internal::attachment_runtime::upload::PreparedCommittedAttachment,
    ) -> crate::ImResult<Self> {
        let group_did = require_group_did(group_did)?;
        if committed.target_kind != "group" || committed.target_did != group_did {
            return Err(crate::ImError::invalid_input(
                Some("attachment_target".to_owned()),
                "committed attachment does not target this group",
            ));
        }

        let parsed = crate::attachments::manifest::parse_attachment_manifest_internal(
            &committed.full_manifest,
        )?;
        if parsed.attachments.is_empty() {
            return Err(crate::ImError::invalid_input(
                Some("attachments".to_owned()),
                "attachment manifest must contain an uploaded object",
            ));
        }
        for attachment in &parsed.attachments {
            if attachment.descriptor.object_encryption_mode()
                == crate::attachments::manifest::OBJECT_ENCRYPTION_MODE_E2EE
                && (attachment.object_key_b64u.is_none() || attachment.nonce_b64u.is_none())
            {
                return Err(crate::ImError::invalid_input(
                    Some("encryption_info".to_owned()),
                    "object-e2ee attachment key and nonce must be carried inside the MLS plaintext",
                ));
            }
        }

        let redacted =
            crate::attachments::manifest::redact_attachment_manifest(&committed.full_manifest);
        if redacted != committed.redacted_manifest {
            return Err(crate::ImError::invalid_input(
                Some("redacted_manifest".to_owned()),
                "redacted attachment projection does not match the committed full manifest",
            ));
        }

        let content_type =
            crate::attachments::manifest::attachment_manifest_content_type().to_owned();
        let plaintext = V2GroupApplicationPlaintext {
            application_content_type: content_type.clone(),
            thread_id: Some(group_did.to_owned()),
            reply_to_message_id: None,
            annotations: None,
            text: None,
            payload: Some(committed.full_manifest.clone()),
            payload_b64u: None,
        };
        validate_plaintext(&plaintext)?;
        Ok(Self {
            plaintext,
            projection: V2ApplicationProjection {
                application_content_type: content_type,
                text: None,
                payload: Some(committed.redacted_manifest.clone()),
            },
            client_context: Some(Value::Object(Map::from_iter([(
                "attachment_grant_refs".to_owned(),
                Value::Array(vec![committed.grant_ref.clone()]),
            )]))),
        })
    }

    pub(crate) fn into_plaintext(self) -> V2GroupApplicationPlaintext {
        self.plaintext
    }

    pub(crate) fn projection(&self) -> &V2ApplicationProjection {
        &self.projection
    }

    pub(crate) fn client_context(&self) -> Option<&Value> {
        self.client_context.as_ref()
    }
}

fn validate_plaintext(plaintext: &V2GroupApplicationPlaintext) -> crate::ImResult<()> {
    plaintext.validate().map_err(|err| {
        crate::ImError::invalid_input(None, format!("invalid P6 v2 application plaintext: {err}"))
    })
}

fn require_group_did(group_did: &str) -> crate::ImResult<&str> {
    let group_did = group_did.trim();
    if group_did.is_empty() || !group_did.starts_with("did:") {
        return Err(crate::ImError::invalid_input(
            Some("group".to_owned()),
            "P6 v2 group target must be a DID",
        ));
    }
    Ok(group_did)
}

/// P5 device controls and P6 Welcome/Commit notices have dedicated encrypted
/// control paths. Accepting them through the ordinary JSON constructor would
/// allow a caller to project protocol control as a user-visible group message.
fn is_reserved_control_payload(payload: &Value) -> bool {
    let Some(object) = payload.as_object() else {
        return false;
    };
    if object
        .get("system_type")
        .and_then(Value::as_str)
        .is_some_and(|value| value.trim().starts_with("awiki."))
    {
        return true;
    }
    if object
        .get("schema")
        .and_then(Value::as_str)
        .is_some_and(|value| value.trim().starts_with("awiki.group.system_event."))
    {
        return true;
    }
    object
        .get("notice_type")
        .and_then(Value::as_str)
        .is_some_and(|value| matches!(value, "welcome-delivery" | "commit-delivery"))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn ordinary_json_rejects_internal_device_and_mls_control_payloads() {
        for ordinary in [
            json!({"event": "poll", "answer": 2}),
            json!({"schema": "awiki.agent.command.v1", "command": "summarize"}),
            json!({"schema": "awiki.agent.status.v1", "status": "working"}),
            json!({"schema": "awiki.agent.mention.v1", "mentions": []}),
        ] {
            assert!(V2ProductApplication::json("did:example:group", ordinary).is_ok());
        }
        for reserved in [
            json!({"system_type": "awiki.device.root-key.v1"}),
            json!({"schema": "awiki.group.system_event.v1"}),
            json!({"notice_type": "welcome-delivery"}),
            json!({"notice_type": "commit-delivery"}),
        ] {
            assert!(matches!(
                V2ProductApplication::json("did:example:group", reserved),
                Err(crate::ImError::PermissionDenied)
            ));
        }
    }
}
