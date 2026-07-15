use std::path::Path;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use serde_json::{json, Value};

use crate::local_rpc::{
    call_uds_once, call_uds_once_with_timeout, RuntimeRpcRequest, RuntimeRpcResponse,
};
use crate::runtime::RuntimeProgressUpdate;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliWrapperRequest {
    pub runtime_rpc_token: String,
    pub method: String,
    pub params: Value,
}

impl CliWrapperRequest {
    pub fn rpc_ping(runtime_rpc_token: impl Into<String>) -> Self {
        Self {
            runtime_rpc_token: runtime_rpc_token.into(),
            method: "rpc.ping".to_string(),
            params: json!({}),
        }
    }

    pub fn task_status(
        runtime_rpc_token: impl Into<String>,
        task_id: impl Into<String>,
        state: impl Into<String>,
        text: impl Into<String>,
    ) -> Self {
        Self {
            runtime_rpc_token: runtime_rpc_token.into(),
            method: "task.status".to_string(),
            params: json!({
                "task_id": task_id.into(),
                "state": state.into(),
                "text": text.into(),
            }),
        }
    }

    pub fn task_status_with_progress(
        runtime_rpc_token: impl Into<String>,
        task_id: impl Into<String>,
        text: impl Into<String>,
        progress: RuntimeProgressUpdate,
    ) -> Self {
        Self {
            runtime_rpc_token: runtime_rpc_token.into(),
            method: "task.status".to_string(),
            params: json!({
                "task_id": task_id.into(),
                "state": "running",
                "text": text.into(),
                "progress": progress,
            }),
        }
    }

    pub fn task_finish(
        runtime_rpc_token: impl Into<String>,
        task_id: impl Into<String>,
        text: impl Into<String>,
    ) -> Self {
        Self {
            runtime_rpc_token: runtime_rpc_token.into(),
            method: "task.finish".to_string(),
            params: json!({
                "task_id": task_id.into(),
                "text": text.into(),
            }),
        }
    }

    pub fn msg_send(
        runtime_rpc_token: impl Into<String>,
        to: impl Into<String>,
        text: impl Into<String>,
    ) -> Self {
        Self {
            runtime_rpc_token: runtime_rpc_token.into(),
            method: "msg.send".to_string(),
            params: json!({
                "to": to.into(),
                "text": text.into(),
            }),
        }
    }

    pub fn outbound_send(
        runtime_rpc_token: impl Into<String>,
        target: OutboundMessageTarget,
        text: impl Into<String>,
        file_path: Option<impl Into<String>>,
        display_filename: Option<impl Into<String>>,
        mime_type: Option<impl Into<String>>,
    ) -> Self {
        let mut params = json!({
            "text": text.into(),
        });
        match target {
            OutboundMessageTarget::DirectRecipient(recipient) => {
                params["to"] = Value::String(recipient);
            }
            OutboundMessageTarget::Group(group) => {
                params["group"] = Value::String(group);
            }
        }
        if let Some(file_path) = file_path {
            params["file_path"] = Value::String(file_path.into());
        }
        if let Some(display_filename) = display_filename {
            params["display_filename"] = Value::String(display_filename.into());
        }
        if let Some(mime_type) = mime_type {
            params["mime_type"] = Value::String(mime_type.into());
        }
        Self {
            runtime_rpc_token: runtime_rpc_token.into(),
            method: "msg.send".to_string(),
            params,
        }
    }

    pub fn attachment_send(
        runtime_rpc_token: impl Into<String>,
        file_path: impl Into<String>,
        display_filename: Option<impl Into<String>>,
        caption: Option<impl Into<String>>,
    ) -> Self {
        let mut params = json!({
            "target": "current_conversation",
            "file_path": file_path.into(),
        });
        if let Some(display_filename) = display_filename {
            params["display_filename"] = Value::String(display_filename.into());
        }
        if let Some(caption) = caption {
            params["caption"] = Value::String(caption.into());
        }
        Self {
            runtime_rpc_token: runtime_rpc_token.into(),
            method: "attachment.send".to_string(),
            params,
        }
    }

    pub fn into_rpc_request(self) -> RuntimeRpcRequest {
        RuntimeRpcRequest {
            runtime_rpc_token: self.runtime_rpc_token,
            method: self.method,
            params: self.params,
            debug: None,
        }
    }
}

pub fn call(socket_path: &Path, request: CliWrapperRequest) -> Result<RuntimeRpcResponse> {
    call_uds_once(socket_path, &request.into_rpc_request())
}

pub fn call_progress(socket_path: &Path, request: CliWrapperRequest) -> Result<RuntimeRpcResponse> {
    call_uds_once_with_timeout(
        socket_path,
        &request.into_rpc_request(),
        Some(Duration::from_millis(750)),
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutboundMessageTarget {
    DirectRecipient(String),
    Group(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliWrapperCommand {
    Send {
        socket_path: std::path::PathBuf,
        runtime_rpc_token: String,
        target: OutboundMessageTarget,
        text: String,
        file_path: Option<String>,
        display_filename: Option<String>,
        mime_type: Option<String>,
    },
    SendMessage {
        socket_path: std::path::PathBuf,
        runtime_rpc_token: String,
        to_handle: String,
        text: String,
    },
    SendAttachment {
        socket_path: std::path::PathBuf,
        runtime_rpc_token: String,
        file_path: String,
        display_filename: Option<String>,
        caption: Option<String>,
    },
}

pub fn run_wrapper_command(command: CliWrapperCommand) -> Result<RuntimeRpcResponse> {
    match command {
        CliWrapperCommand::Send {
            socket_path,
            runtime_rpc_token,
            target,
            text,
            file_path,
            display_filename,
            mime_type,
        } => {
            let target = normalize_outbound_target(target)?;
            let text = text.trim();
            if text.is_empty() {
                bail!("text is required");
            }
            call(
                &socket_path,
                CliWrapperRequest::outbound_send(
                    runtime_rpc_token,
                    target,
                    text.to_string(),
                    file_path,
                    display_filename,
                    mime_type,
                ),
            )
        }
        CliWrapperCommand::SendMessage {
            socket_path,
            runtime_rpc_token,
            to_handle,
            text,
        } => {
            let to_handle = normalize_direct_recipient(&to_handle)?;
            let text = text.trim();
            if text.is_empty() {
                bail!("text is required");
            }
            call(
                &socket_path,
                CliWrapperRequest::msg_send(runtime_rpc_token, to_handle, text.to_string()),
            )
        }
        CliWrapperCommand::SendAttachment {
            socket_path,
            runtime_rpc_token,
            file_path,
            display_filename,
            caption,
        } => call(
            &socket_path,
            CliWrapperRequest::attachment_send(
                runtime_rpc_token,
                file_path,
                display_filename,
                caption,
            ),
        ),
    }
}

fn normalize_outbound_target(target: OutboundMessageTarget) -> Result<OutboundMessageTarget> {
    match target {
        OutboundMessageTarget::DirectRecipient(recipient) => Ok(
            OutboundMessageTarget::DirectRecipient(normalize_direct_recipient(&recipient)?),
        ),
        OutboundMessageTarget::Group(group) => {
            let group = group.trim();
            if group.is_empty() {
                bail!("group is required");
            }
            Ok(OutboundMessageTarget::Group(group.to_string()))
        }
    }
}

pub fn socket_from_env_or_arg(socket: Option<std::path::PathBuf>) -> Result<std::path::PathBuf> {
    socket
        .or_else(|| std::env::var_os("AWIKI_DAEMON_RPC_SOCKET").map(std::path::PathBuf::from))
        .context("--socket or AWIKI_DAEMON_RPC_SOCKET is required")
}

pub fn runtime_token_from_env_or_arg(token: Option<String>) -> Result<String> {
    token
        .or_else(|| std::env::var("AWIKI_RUNTIME_RPC_TOKEN").ok())
        .context("--token or AWIKI_RUNTIME_RPC_TOKEN is required")
}

pub fn normalize_direct_recipient(input: &str) -> Result<String> {
    let value = input.trim();
    if value.is_empty() {
        bail!("recipient is required");
    }
    if value.starts_with("did:") {
        return Ok(value.to_string());
    }
    Ok(value.trim_start_matches('@').to_string())
}
