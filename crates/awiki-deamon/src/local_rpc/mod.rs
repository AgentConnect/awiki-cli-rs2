use std::io::{BufRead, BufReader, Write};
use std::path::Path;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::outbox::RuntimeOutbox;
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
    let recipient = rpc_recipient(&method, &request.params);
    let context = state.authorize_runtime_rpc(&token, &method, recipient)?;
    apply_runtime_rpc_side_effects(state, outbox, &context, &method, &request.params)?;

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
            state.update_runtime_run_status(&context.run_id, RuntimeRunStatus::Finished)?;
            outbox.send_final(context, params.get("text").and_then(Value::as_str))?;
        }
        RpcMethod::MsgSend => {
            outbox.send_message(
                context,
                rpc_recipient(method, params),
                params.get("text").and_then(Value::as_str),
            )?;
        }
        RpcMethod::RpcPing | RpcMethod::ArtifactCreated => {}
    }
    Ok(())
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
