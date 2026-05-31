use std::path::Path;

use anyhow::Result;
use serde_json::{json, Value};

use crate::local_rpc::{call_uds_once, RuntimeRpcRequest, RuntimeRpcResponse};

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
