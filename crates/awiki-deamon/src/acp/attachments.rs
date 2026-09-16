use crate::{runtime::RuntimeTask, DaemonState};
use agent_client_protocol::schema::v1::ContentBlock;
use anyhow::{bail, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::path::PathBuf;

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
    if serde_json::from_str::<serde_json::Value>(&raw)?["error"] == "attachment_download_failed" {
        bail!("attachment_download_failed");
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
    state.connection()?.execute("INSERT INTO acp_attachments(agent_did,message_id,items) VALUES(?1,?2,?3) ON CONFLICT(agent_did,message_id) DO UPDATE SET items=excluded.items",rusqlite::params![agent,message,r#"{"error":"attachment_download_failed"}"#])?;
    Ok(())
}
