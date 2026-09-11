use std::collections::BTreeSet;
use std::path::Path;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const ACP_PROTOCOL_VERSION: u64 = 1;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AcpPermissionPolicy {
    #[default]
    AllowOnce,
    RejectOnce,
}

impl AcpPermissionPolicy {
    pub fn option_id(self) -> &'static str {
        match self {
            Self::AllowOnce => "allow-once",
            Self::RejectOnce => "reject-once",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum AcpInbound {
    Response {
        id: String,
        result: Option<Value>,
        error: Option<Value>,
    },
    SessionUpdate {
        session_id: String,
        update: Value,
        text: Option<String>,
    },
    PermissionRequest {
        id: Value,
        session_id: String,
        option_ids: BTreeSet<String>,
    },
    Notification {
        method: String,
        params: Value,
    },
    Request {
        id: Value,
        method: String,
        params: Value,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcpTextAccumulator {
    session_id: String,
    text: String,
}

impl AcpTextAccumulator {
    pub fn new(session_id: impl Into<String>) -> Self {
        Self {
            session_id: session_id.into(),
            text: String::new(),
        }
    }

    pub fn observe(&mut self, message: &AcpInbound) -> bool {
        let AcpInbound::SessionUpdate {
            session_id, text, ..
        } = message
        else {
            return false;
        };
        if session_id != &self.session_id {
            return false;
        }
        let Some(text) = text else {
            return false;
        };
        self.text.push_str(text);
        true
    }

    pub fn final_text(&self) -> Option<String> {
        (!self.text.trim().is_empty()).then(|| self.text.clone())
    }
}

pub fn initialize_request(id: u64) -> Value {
    request(
        id,
        "initialize",
        json!({
            "protocolVersion": ACP_PROTOCOL_VERSION,
            "clientInfo": {
                "name": "awiki-daemon",
                "version": env!("CARGO_PKG_VERSION"),
            },
            "clientCapabilities": {
                "auth": { "terminal": false },
                "elicitation": null,
                "fs": {
                    "readTextFile": false,
                    "writeTextFile": false,
                },
                "nes": null,
                "plan": null,
                "positionEncodings": [],
                "terminal": false,
            },
        }),
    )
}

pub fn new_session_request(id: u64, cwd: &Path) -> Result<Value> {
    if !cwd.is_absolute() {
        bail!("ACP session cwd must be an absolute path");
    }
    let cwd = cwd
        .to_str()
        .context("ACP session cwd must be valid UTF-8")?;
    Ok(request(
        id,
        "session/new",
        json!({
            "cwd": cwd,
            "mcpServers": [],
            "additionalDirectories": [],
        }),
    ))
}

pub fn prompt_request(id: u64, session_id: &str, prompt: &str) -> Result<Value> {
    let session_id = require_non_empty(session_id, "ACP session_id")?;
    if prompt.trim().is_empty() {
        bail!("ACP prompt must not be empty");
    }
    Ok(request(
        id,
        "session/prompt",
        json!({
            "sessionId": session_id,
            "prompt": [{ "type": "text", "text": prompt }],
        }),
    ))
}

pub fn cancel_notification(session_id: &str) -> Result<Value> {
    let session_id = require_non_empty(session_id, "ACP session_id")?;
    Ok(json!({
        "jsonrpc": "2.0",
        "method": "session/cancel",
        "params": { "sessionId": session_id },
    }))
}

pub fn parse_inbound(line: &str) -> Result<AcpInbound> {
    let value: Value = serde_json::from_str(line).context("parse ACP JSON-RPC frame")?;
    if value.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        bail!("ACP frame must declare jsonrpc 2.0");
    }

    let method = value.get("method").and_then(Value::as_str);
    let id = value.get("id");
    if method.is_none() {
        let id = id
            .and_then(json_id_as_string)
            .context("ACP response must include a string or numeric id")?;
        if value.get("result").is_none() && value.get("error").is_none() {
            bail!("ACP response must include result or error");
        }
        return Ok(AcpInbound::Response {
            id,
            result: value.get("result").cloned(),
            error: value.get("error").cloned(),
        });
    }

    let method = method.expect("checked above").to_string();
    let params = value.get("params").cloned().unwrap_or(Value::Null);
    match (method.as_str(), id) {
        ("session/update", None) => parse_session_update(params),
        ("session/request_permission", Some(id)) => parse_permission_request(id.clone(), params),
        (_, Some(id)) => Ok(AcpInbound::Request {
            id: id.clone(),
            method,
            params,
        }),
        (_, None) => Ok(AcpInbound::Notification { method, params }),
    }
}

pub fn permission_response(request: &AcpInbound, policy: AcpPermissionPolicy) -> Result<Value> {
    let AcpInbound::PermissionRequest { id, option_ids, .. } = request else {
        bail!("ACP permission response requires a permission request");
    };
    let option_id = policy.option_id();
    if !option_ids.contains(option_id) {
        bail!("ACP permission request did not advertise {option_id}");
    }
    Ok(json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {
            "outcome": {
                "outcome": "selected",
                "optionId": option_id,
            },
        },
    }))
}

fn request(id: u64, method: &str, params: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": method,
        "params": params,
    })
}

fn parse_session_update(params: Value) -> Result<AcpInbound> {
    let session_id = params
        .get("sessionId")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .context("ACP session/update must include sessionId")?
        .to_string();
    let update = params
        .get("update")
        .cloned()
        .context("ACP session/update must include update")?;
    let text = (update.get("sessionUpdate").and_then(Value::as_str) == Some("agent_message_chunk")
        && update
            .get("content")
            .and_then(|content| content.get("type"))
            == Some(&Value::String("text".to_string())))
    .then(|| {
        update
            .get("content")
            .and_then(|content| content.get("text"))
            .and_then(Value::as_str)
            .map(str::to_string)
    })
    .flatten();
    Ok(AcpInbound::SessionUpdate {
        session_id,
        update,
        text,
    })
}

fn parse_permission_request(id: Value, params: Value) -> Result<AcpInbound> {
    if json_id_as_string(&id).is_none() {
        bail!("ACP permission request id must be a string or number");
    }
    let session_id = params
        .get("sessionId")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .context("ACP permission request must include sessionId")?
        .to_string();
    let options = params
        .get("options")
        .and_then(Value::as_array)
        .context("ACP permission request must include options")?;
    let option_ids = options
        .iter()
        .filter_map(|option| option.get("optionId").and_then(Value::as_str))
        .map(str::to_string)
        .collect();
    Ok(AcpInbound::PermissionRequest {
        id,
        session_id,
        option_ids,
    })
}

fn require_non_empty<'a>(value: &'a str, field: &str) -> Result<&'a str> {
    let value = value.trim();
    if value.is_empty() {
        bail!("{field} must not be empty");
    }
    Ok(value)
}

fn json_id_as_string(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(str::to_string)
        .or_else(|| value.as_i64().map(|id| id.to_string()))
        .or_else(|| value.as_u64().map(|id| id.to_string()))
}
