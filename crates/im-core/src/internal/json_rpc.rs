use serde::Deserialize;
use serde_json::{json, Value};

pub(crate) const CONTENT_TYPE_JSON: &str = "application/json";
const JSON_RPC_VERSION: &str = "2.0";
const JSON_RPC_ID: &str = "req-1";

#[derive(Debug, Deserialize)]
struct JsonRpcResponseError {
    code: i64,
    message: String,
}

pub(crate) fn build_payload(method: &str, params: Value) -> Value {
    json!({
        "jsonrpc": JSON_RPC_VERSION,
        "id": JSON_RPC_ID,
        "method": method,
        "params": params,
    })
}

pub(crate) fn decode_response(raw: &[u8]) -> crate::ImResult<Value> {
    let envelope: Value =
        serde_json::from_slice(raw).map_err(|err| crate::ImError::Serialization {
            detail: err.to_string(),
        })?;
    if let Some(error) = envelope.get("error").filter(|error| !error.is_null()) {
        let error: JsonRpcResponseError =
            serde_json::from_value(error.clone()).map_err(|err| crate::ImError::Serialization {
                detail: err.to_string(),
            })?;
        return Err(crate::ImError::Service {
            status_code: None,
            code: Some(error.code.to_string()),
            message: error.message,
        });
    }
    Ok(envelope.get("result").cloned().unwrap_or(Value::Null))
}

pub(crate) fn decode_plain_response(raw: &[u8]) -> crate::ImResult<Value> {
    serde_json::from_slice(raw).map_err(|err| crate::ImError::Serialization {
        detail: err.to_string(),
    })
}
