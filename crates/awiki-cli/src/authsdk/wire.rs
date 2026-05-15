use super::{HttpError, RpcError};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;

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

pub fn decode_json_rpc_response_optional(raw: &[u8]) -> Result<(), anyhow::Error> {
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
    Ok(())
}

pub fn decode_plain_json_response<T>(raw: &[u8]) -> Result<T, serde_json::Error>
where
    T: DeserializeOwned,
{
    serde_json::from_slice(raw)
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

pub fn flatten_header_values<'a, I, K, V>(headers: I) -> BTreeMap<String, String>
where
    I: IntoIterator<Item = (K, V)>,
    K: Into<String>,
    V: IntoIterator<Item = &'a str>,
{
    headers
        .into_iter()
        .filter_map(|(key, values)| {
            values
                .into_iter()
                .next()
                .map(|value| (key.into(), value.to_string()))
        })
        .collect()
}
