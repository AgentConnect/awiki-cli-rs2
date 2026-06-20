use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::app_bridge::action::queue_runtime_app_action_request;
use crate::outbox::{
    RuntimeAttachmentSend, RuntimeMessageSend, RuntimeMessageTarget, RuntimeOutbox,
};
use crate::runtime::RuntimeRunStatus;
use crate::security::runtime_token::{RpcMethod, RuntimeRpcToken};
use crate::state::{AuthorizedRuntimeContext, DaemonState};

#[cfg(unix)]
mod uds;

#[cfg(unix)]
pub use uds::{
    bind_uds_listener, handle_uds_stream_with_outbox, serve_one_uds_request,
    verify_socket_permissions, PeerCredential,
};

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeRpcRequest {
    pub runtime_rpc_token: String,
    pub method: String,
    #[serde(default)]
    pub params: Value,
    #[serde(default)]
    pub debug: Option<RuntimeRpcDebug>,
}

impl std::fmt::Debug for RuntimeRpcRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RuntimeRpcRequest")
            .field("runtime_rpc_token", &"<redacted>")
            .field("method", &self.method)
            .field("params", &self.params)
            .field("debug", &self.debug)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeRpcDebug {
    #[serde(default)]
    pub agent_did: Option<String>,
    #[serde(default)]
    pub run_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeRpcResponse {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<RuntimeRpcError>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeRpcError {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeRpcExecution {
    pub token_id: String,
    pub agent_did: String,
    pub runtime_profile_id: String,
    pub run_id: String,
    pub method: String,
    pub method_level: String,
}

impl RuntimeRpcResponse {
    pub fn success(result: impl Serialize) -> Result<Self> {
        Ok(Self {
            ok: true,
            result: Some(serde_json::to_value(result)?),
            error: None,
        })
    }

    pub fn failure(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            ok: false,
            result: None,
            error: Some(RuntimeRpcError {
                code: code.into(),
                message: message.into(),
            }),
        }
    }
}

pub fn handle_runtime_rpc_request(
    state: &DaemonState,
    request: RuntimeRpcRequest,
) -> RuntimeRpcResponse {
    match execute_runtime_rpc_request(state, request) {
        Ok(response) => response,
        Err(error) => RuntimeRpcResponse::failure("runtime_rpc_error", error.to_string()),
    }
}

pub fn execute_runtime_rpc_request(
    state: &DaemonState,
    request: RuntimeRpcRequest,
) -> Result<RuntimeRpcResponse> {
    let method = RpcMethod::parse(&request.method)?;
    if matches!(
        method,
        RpcMethod::MsgSend | RpcMethod::SendAttachment | RpcMethod::AppActionRequest
    ) {
        bail!("message RPC methods require runtime outbox side effects");
    }
    let token = RuntimeRpcToken::parse(request.runtime_rpc_token)?;
    let recipient = rpc_recipient(&method, &request.params);
    let context = state.authorize_runtime_rpc(&token, &method, recipient)?;

    RuntimeRpcResponse::success(RuntimeRpcExecution {
        token_id: context.token_id,
        agent_did: context.agent_did,
        runtime_profile_id: context.runtime_profile_id,
        run_id: context.run_id,
        method: method.as_str().to_string(),
        method_level: format!("{:?}", method.level()).to_ascii_lowercase(),
    })
}

pub fn execute_runtime_rpc_request_with_outbox(
    state: &DaemonState,
    outbox: &impl RuntimeOutbox,
    request: RuntimeRpcRequest,
) -> Result<RuntimeRpcResponse> {
    let method = RpcMethod::parse(&request.method)?;
    let token = RuntimeRpcToken::parse(request.runtime_rpc_token)?;
    let context = match method {
        RpcMethod::MsgSend => {
            let message = RuntimeMessageSend::from_params(&request.params)?;
            let preliminary_context =
                state.authorize_runtime_rpc_for_recipient_resolution(&token, &method)?;
            ensure_runtime_route_allows_side_effects(state, &preliminary_context)?;
            if let Some(file_path) = message.file_path.as_ref() {
                ensure_attachment_under_allowed_roots(
                    state,
                    &preliminary_context.run_id,
                    &preliminary_context.agent_did,
                    file_path,
                )?;
            }
            let message = resolve_message_recipient(outbox, &preliminary_context, message)?;
            let context = state.authorize_runtime_rpc_with_message_policy(
                &token,
                &method,
                message.recipient_candidates(),
                Some(message.security.as_str()),
            )?;
            apply_msg_send_side_effect(state, outbox, &context, &message)?;
            context
        }
        RpcMethod::SendAttachment => {
            let preliminary_context =
                state.authorize_runtime_rpc_for_recipient_resolution(&token, &method)?;
            ensure_runtime_route_allows_side_effects(state, &preliminary_context)?;
            let task = state
                .load_runtime_task_for_run(&preliminary_context.run_id)
                .context("attachment.send requires a runtime task context")?;
            let attachment = RuntimeAttachmentSend::from_params(
                &request.params,
                Some(task.sender_did.as_str()),
            )?;
            ensure_attachment_under_allowed_roots(
                state,
                &preliminary_context.run_id,
                &preliminary_context.agent_did,
                &attachment.file_path,
            )?;
            let context = state.authorize_runtime_rpc(&token, &method, None)?;
            apply_attachment_send_side_effect(state, outbox, &context, &attachment)?;
            context
        }
        _ => {
            let recipient = rpc_recipient(&method, &request.params);
            let context = state.authorize_runtime_rpc(&token, &method, recipient)?;
            ensure_runtime_route_allows_side_effects(state, &context)?;
            apply_runtime_rpc_side_effects(state, outbox, &context, &method, &request.params)?;
            context
        }
    };

    RuntimeRpcResponse::success(RuntimeRpcExecution::from(context))
}

pub fn read_request_from<R: std::io::Read>(reader: R) -> Result<RuntimeRpcRequest> {
    let mut line = String::new();
    let mut reader = BufReader::new(reader);
    let read = reader.read_line(&mut line)?;
    if read == 0 {
        bail!("empty runtime RPC request");
    }
    Ok(serde_json::from_str(line.trim_end())?)
}

pub fn write_response_to<W: Write>(mut writer: W, response: &RuntimeRpcResponse) -> Result<()> {
    serde_json::to_writer(&mut writer, response)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

pub fn call_uds_once(
    socket_path: &Path,
    request: &RuntimeRpcRequest,
) -> Result<RuntimeRpcResponse> {
    #[cfg(unix)]
    {
        let mut stream = std::os::unix::net::UnixStream::connect(socket_path)
            .with_context(|| format!("connect daemon local RPC {}", socket_path.display()))?;
        serde_json::to_writer(&mut stream, request)?;
        stream.write_all(b"\n")?;
        stream.flush()?;
        let response = read_response_from(stream)?;
        Ok(response)
    }

    #[cfg(not(unix))]
    {
        let _ = socket_path;
        let _ = request;
        bail!("Unix domain socket local RPC is not supported on this platform yet");
    }
}

fn read_response_from<R: std::io::Read>(reader: R) -> Result<RuntimeRpcResponse> {
    let mut line = String::new();
    let mut reader = BufReader::new(reader);
    let read = reader.read_line(&mut line)?;
    if read == 0 {
        bail!("empty runtime RPC response");
    }
    Ok(serde_json::from_str(line.trim_end())?)
}

fn rpc_recipient<'a>(method: &RpcMethod, params: &'a Value) -> Option<&'a str> {
    if *method != RpcMethod::MsgSend {
        return None;
    }
    params
        .get("to")
        .or_else(|| params.get("recipient"))
        .and_then(Value::as_str)
}

fn ensure_runtime_route_allows_side_effects(
    state: &DaemonState,
    context: &AuthorizedRuntimeContext,
) -> Result<()> {
    let run = state.load_runtime_run(&context.run_id)?;
    if run.runtime_plugin_id != crate::agent::GENERIC_CLI_RUNTIME_PLUGIN_ID {
        return Ok(());
    }
    let Some(route_session) = state.load_cli_route_session_for_run(&context.run_id)? else {
        return Ok(());
    };
    let lock_matches = route_session.lock_run_id.as_deref() == Some(context.run_id.as_str());
    if route_session.status == "running" && lock_matches {
        return Ok(());
    }

    state.insert_audit_event_json(
        "runtime_rpc.side_effect_rejected",
        Some(&context.agent_did),
        Some(&context.runtime_profile_id),
        Some(&context.run_id),
        Some(&context.token_id),
        serde_json::json!({
            "method": context.method.as_str(),
            "reason": "late_callback_rejected",
            "route_key_hash": route_session.route_key_hash.as_str(),
            "route_status": route_session.status.as_str(),
            "route_version": route_session.version,
            "route_lock_present": route_session.lock_run_id.is_some(),
            "route_lock_matches_run": lock_matches,
        }),
    )?;
    bail!("late_callback_rejected");
}

fn apply_runtime_rpc_side_effects(
    state: &DaemonState,
    outbox: &impl RuntimeOutbox,
    context: &AuthorizedRuntimeContext,
    method: &RpcMethod,
    params: &Value,
) -> Result<()> {
    match method {
        RpcMethod::TaskStatus => {
            let state_value = params
                .get("state")
                .and_then(Value::as_str)
                .unwrap_or("running");
            let status = match state_value {
                "pending" => RuntimeRunStatus::Pending,
                "running" => RuntimeRunStatus::Running,
                "finished" => RuntimeRunStatus::Finished,
                "failed" => RuntimeRunStatus::Failed,
                _ => RuntimeRunStatus::Running,
            };
            state.update_runtime_run_status(&context.run_id, status)?;
            outbox.send_status(
                context,
                state_value,
                params.get("text").and_then(Value::as_str),
            )?;
        }
        RpcMethod::TaskFinish => {
            let run = state.load_runtime_run(&context.run_id)?;
            if run.status == RuntimeRunStatus::Finished {
                return Ok(());
            }
            state.update_runtime_run_status(&context.run_id, RuntimeRunStatus::Finished)?;
            outbox.send_final(context, params.get("text").and_then(Value::as_str))?;
        }
        RpcMethod::AppActionRequest => {
            queue_runtime_app_action_request(state, context, params)?;
        }
        RpcMethod::MsgSend | RpcMethod::SendAttachment => {}
        RpcMethod::RpcPing | RpcMethod::ArtifactCreated => {}
    }
    Ok(())
}

fn resolve_message_recipient(
    outbox: &impl RuntimeOutbox,
    context: &AuthorizedRuntimeContext,
    message: RuntimeMessageSend,
) -> Result<RuntimeMessageSend> {
    let RuntimeMessageTarget::Direct { recipient, .. } = &message.target else {
        return Ok(message);
    };
    let resolved = outbox
        .resolve_recipient_did(context, recipient)
        .with_context(|| format!("resolve msg.send recipient {recipient}"))?;
    let Some(resolved_did) = resolved else {
        bail!("msg.send recipient could not be resolved");
    };
    Ok(message.with_resolved_recipient(resolved_did))
}

fn apply_attachment_send_side_effect(
    state: &DaemonState,
    outbox: &impl RuntimeOutbox,
    context: &AuthorizedRuntimeContext,
    attachment: &RuntimeAttachmentSend,
) -> Result<()> {
    let (file_sha256, file_size_bytes) = attachment_file_audit(&attachment.file_path)?;
    let result = outbox.send_attachment(context, attachment);
    match result {
        Ok(send_result) => {
            state.insert_audit_event_json(
                "runtime.attachment_send.sent",
                Some(&context.agent_did),
                Some(&context.runtime_profile_id),
                Some(&context.run_id),
                Some(&context.token_id),
                serde_json::json!({
                    "target": send_result.target,
                    "display_filename": send_result.display_filename,
                    "size_bytes": send_result.size_bytes.unwrap_or(file_size_bytes),
                    "file_sha256": file_sha256,
                    "message_id": send_result.message_id,
                }),
            )?;
            Ok(())
        }
        Err(error) => {
            state.insert_audit_event_json(
                "runtime.attachment_send.failed",
                Some(&context.agent_did),
                Some(&context.runtime_profile_id),
                Some(&context.run_id),
                Some(&context.token_id),
                serde_json::json!({
                    "target": attachment.target,
                    "display_filename": attachment.display_filename,
                    "size_bytes": file_size_bytes,
                    "file_sha256": file_sha256,
                    "reason": error.to_string(),
                }),
            )?;
            Err(error).context("send runtime attachment")
        }
    }
}

fn attachment_file_audit(path: &Path) -> Result<(String, u64)> {
    let bytes = std::fs::read(path).with_context(|| "read attachment file for audit hash")?;
    let digest = Sha256::digest(&bytes);
    let hash = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    Ok((hash, bytes.len() as u64))
}

fn ensure_attachment_under_allowed_roots(
    state: &DaemonState,
    run_id: &str,
    agent_did: &str,
    file_path: &Path,
) -> Result<()> {
    let file_path = file_path
        .canonicalize()
        .with_context(|| "canonicalize attachment file")?;
    let allowed_roots = attachment_allowed_roots(state, run_id, agent_did)?;
    if allowed_roots.is_empty() {
        bail!("attachment.send requires a runtime workspace");
    }
    if allowed_roots.iter().any(|root| file_path.starts_with(root)) {
        return Ok(());
    }
    bail!("attachment file must be inside the runtime workspace");
}

fn attachment_allowed_roots(
    state: &DaemonState,
    run_id: &str,
    agent_did: &str,
) -> Result<Vec<PathBuf>> {
    let mut roots = Vec::new();
    if let Ok(record) = state.load_cli_driver_run(run_id) {
        roots.extend(record.workspace_instance_path);
        roots.extend(record.workspace_root);
        if let Some(final_output_path) = record.final_output_path {
            if let Some(parent) = final_output_path.parent() {
                roots.push(parent.to_path_buf());
            }
        }
    }
    if roots.is_empty() {
        let profile = state.load_runtime_agent_profile(agent_did)?;
        if let Some(workspace_root) = profile.workspace_root {
            roots.push(workspace_root);
        }
    }

    let mut canonical_roots = Vec::new();
    for root in roots {
        let canonical = root
            .canonicalize()
            .with_context(|| "canonicalize runtime workspace")?;
        if !canonical_roots.contains(&canonical) {
            canonical_roots.push(canonical);
        }
    }
    Ok(canonical_roots)
}

fn apply_msg_send_side_effect(
    state: &DaemonState,
    outbox: &impl RuntimeOutbox,
    context: &AuthorizedRuntimeContext,
    message: &RuntimeMessageSend,
) -> Result<()> {
    let result = outbox.send_message(context, message);
    match result {
        Ok(send_result) => {
            state.insert_audit_event_json(
                "runtime.msg_send.sent",
                Some(&context.agent_did),
                Some(&context.runtime_profile_id),
                Some(&context.run_id),
                Some(&context.token_id),
                serde_json::json!({
                    "raw_recipient": send_result.raw_recipient,
                    "resolved_did": send_result.resolved_did,
                    "target_kind": send_result.target_kind,
                    "security": send_result.security.as_str(),
                    "message_id": send_result.message_id,
                    "has_attachment": message.file_path.is_some(),
                }),
            )?;
            Ok(())
        }
        Err(error) => {
            state.insert_audit_event_json(
                "runtime.msg_send.failed",
                Some(&context.agent_did),
                Some(&context.runtime_profile_id),
                Some(&context.run_id),
                Some(&context.token_id),
                serde_json::json!({
                    "raw_recipient": message.raw_recipient(),
                    "resolved_did": message.resolved_recipient(),
                    "target_kind": message.target_kind(),
                    "security": message.security.as_str(),
                    "has_attachment": message.file_path.is_some(),
                    "reason": error.to_string(),
                }),
            )?;
            Err(error).context("send runtime message")
        }
    }
}

impl From<AuthorizedRuntimeContext> for RuntimeRpcExecution {
    fn from(context: AuthorizedRuntimeContext) -> Self {
        let method = context.method;
        Self {
            token_id: context.token_id,
            agent_did: context.agent_did,
            runtime_profile_id: context.runtime_profile_id,
            run_id: context.run_id,
            method: method.as_str().to_string(),
            method_level: format!("{:?}", method.level()).to_ascii_lowercase(),
        }
    }
}
