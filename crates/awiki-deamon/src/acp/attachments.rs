use crate::{runtime::RuntimeTask, DaemonState};
use agent_client_protocol::schema::v1::ContentBlock;
use anyhow::{bail, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::path::PathBuf;

/// Safe transport diagnostics. Never carry SDK URLs, credentials or local paths
/// into reliable chat control events.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttachmentFailure {
    pub code: String,
    pub stage: String,
    pub retryable: bool,
}

impl Default for AttachmentFailure {
    fn default() -> Self {
        Self {
            code: "attachment_download_failed".into(),
            stage: "download".into(),
            retryable: false,
        }
    }
}

impl std::fmt::Display for AttachmentFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.code)
    }
}
impl std::error::Error for AttachmentFailure {}

impl AttachmentFailure {
    pub fn from_core(error: &im_core::ImError) -> Self {
        use im_core::ImError;
        match error {
            ImError::AttachmentPreparation {
                stage,
                retryable,
                cause,
            } => {
                let mut failure = Self::from_core(cause);
                failure.stage = stage.as_str().into();
                failure.retryable = *retryable;
                failure
            }
            ImError::AttachmentTransfer {
                failure, retryable, ..
            } => Self {
                code: failure.code().into(),
                stage: "transfer".into(),
                retryable: *retryable,
            },
            ImError::TransportUnavailable { .. } => Self {
                code: "attachment_download_network".into(),
                stage: "download".into(),
                retryable: true,
            },
            ImError::PermissionDenied | ImError::AuthRequired | ImError::SessionExpired => Self {
                code: "attachment_permission_denied".into(),
                ..Self::default()
            },
            ImError::MessageNotFound { .. } => Self {
                code: "attachment_not_found".into(),
                ..Self::default()
            },
            ImError::Service {
                status_code, code, ..
            } => {
                let code = match (status_code, code.as_deref()) {
                    (Some(401 | 403), _) => "attachment_permission_denied",
                    (Some(404), _) => "attachment_not_found",
                    (_, Some("anp.attachment.digest_mismatch")) => "attachment_integrity_failed",
                    (Some(429 | 502 | 503 | 504), _) => "attachment_service_unavailable",
                    _ => "attachment_download_failed",
                };
                Self {
                    code: code.into(),
                    retryable: matches!(status_code, Some(429 | 502 | 503 | 504)),
                    ..Self::default()
                }
            }
            _ => Self::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthorizedAttachment {
    pub path: PathBuf,
    pub filename: String,
    pub mime_type: String,
    pub digest: String,
}

impl AuthorizedAttachment {
    pub fn from_download(path: PathBuf, filename: String, mime_type: String) -> Result<Self> {
        let bytes = std::fs::read(&path)?;
        Ok(Self {
            path,
            filename,
            mime_type,
            digest: format!("{:x}", Sha256::digest(bytes)),
        })
    }
}

pub fn remember(
    state: &DaemonState,
    agent: &str,
    message: &str,
    items: &[AuthorizedAttachment],
) -> Result<()> {
    state.connection()?.execute("INSERT INTO acp_attachments(agent_did,message_id,items) VALUES(?1,?2,?3) ON CONFLICT(agent_did,message_id) DO UPDATE SET items=excluded.items",rusqlite::params![agent,message,serde_json::to_string(items)?])?;
    Ok(())
}

pub fn prompt_blocks(state: &DaemonState, task: &RuntimeTask) -> Result<Vec<ContentBlock>> {
    use rusqlite::OptionalExtension;
    let raw: Option<String> = state
        .connection()?
        .query_row(
            "SELECT items FROM acp_attachments WHERE agent_did=?1 AND message_id=?2",
            rusqlite::params![task.agent_did, task.correlation().source_message_id],
            |r| r.get(0),
        )
        .optional()?;
    let Some(raw) = raw else { return Ok(vec![]) };
    let stored: serde_json::Value = serde_json::from_str(&raw)?;
    if stored["error"] == "attachment_download_failed" {
        let failure = serde_json::from_value::<AttachmentFailure>(stored["failure"].clone())
            .unwrap_or_default();
        return Err(failure.into());
    }
    let mut blocks = vec![];
    for item in serde_json::from_str::<Vec<AuthorizedAttachment>>(&raw)? {
        if std::fs::symlink_metadata(&item.path)?
            .file_type()
            .is_symlink()
        {
            bail!("attachment_changed");
        }
        let bytes = std::fs::read(&item.path).context("attachment_unavailable")?;
        if format!("{:x}", Sha256::digest(&bytes)) != item.digest {
            bail!("attachment_changed");
        }
        let value = if item.mime_type.starts_with("image/") {
            if bytes.len() > 20 * 1024 * 1024 {
                bail!("image_too_large")
            }
            json!({"type":"image","data":STANDARD.encode(&bytes),"mimeType":item.mime_type})
        } else {
            let url = reqwest::Url::from_file_path(&item.path)
                .map_err(|_| anyhow::anyhow!("invalid_attachment_path"))?;
            json!({"type":"resource_link","uri":url.as_str(),"name":item.filename,"mimeType":item.mime_type,"size":bytes.len()})
        };
        blocks.push(serde_json::from_value(value)?);
    }
    Ok(blocks)
}

pub fn remember_failure(state: &DaemonState, agent: &str, message: &str) -> Result<()> {
    remember_failure_details(state, agent, message, &AttachmentFailure::default())
}

pub fn remember_failure_details(
    state: &DaemonState,
    agent: &str,
    message: &str,
    failure: &AttachmentFailure,
) -> Result<()> {
    state.connection()?.execute("INSERT INTO acp_attachments(agent_did,message_id,items) VALUES(?1,?2,?3) ON CONFLICT(agent_did,message_id) DO UPDATE SET items=excluded.items",rusqlite::params![agent,message,json!({"error":"attachment_download_failed","failure":failure}).to_string()])?;
    Ok(())
}
