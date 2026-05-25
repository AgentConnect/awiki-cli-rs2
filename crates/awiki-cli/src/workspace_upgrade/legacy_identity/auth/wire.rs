use super::{HttpError, RpcError};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};

pub const CONTENT_TYPE_JSON: &str = "application/json";
pub const JSON_RPC_VERSION: &str = "2.0";
pub const JSON_RPC_ID: &str = "req-1";

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct JsonRpcResponseError {
    pub code: i64,
    pub message: String,
    #[serde(default)]
    pub data: Option<Value>,
}

pub fn build_json_rpc_payload(rpc_method: &str, params: Value) -> Value {
    json!({
        "jsonrpc": JSON_RPC_VERSION,
        "id": JSON_RPC_ID,
        "method": rpc_method,
        "params": params,
    })
}

pub fn decode_json_rpc_response<T>(raw: &[u8]) -> Result<T, anyhow::Error>
where
    T: DeserializeOwned,
{
    let envelope: Value = serde_json::from_slice(raw)?;
    if let Some(error) = envelope.get("error").filter(|error| !error.is_null()) {
        let error: JsonRpcResponseError = serde_json::from_value(error.clone())?;
        return Err(RpcError {
            code: error.code,
            message: error.message,
            data: error.data,
        }
        .into());
    }
    let result = envelope.get("result").cloned().unwrap_or(Value::Null);
    Ok(serde_json::from_value(result)?)
}

pub fn http_status_error(status_code: u16, body: &[u8]) -> Option<HttpError> {
    if status_code < 400 {
        return None;
    }
    Some(HttpError {
        status_code,
        message: String::from_utf8_lossy(body).trim().to_string(),
    })
}
