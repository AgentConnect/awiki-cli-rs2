use serde_json::{Map, Value};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IncomingWsMessage {
    Response { request_id: String },
    Notification,
}

pub fn request_id_from_value(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        Value::Number(number) => {
            if let Some(value) = number.as_i64() {
                value.to_string()
            } else if let Some(value) = number.as_u64() {
                value.to_string()
            } else if let Some(value) = number.as_f64() {
                format!("{value:.0}")
            } else {
                String::new()
            }
        }
        _ => String::new(),
    }
}

pub fn int64_from_value(value: &Value) -> i64 {
    match value {
        Value::Number(number) => number
            .as_i64()
            .or_else(|| number.as_u64().and_then(|value| i64::try_from(value).ok()))
            .or_else(|| number.as_f64().map(|value| value as i64))
            .unwrap_or_default(),
        _ => 0,
    }
}

pub fn next_ws_rpc_request_id(next_id: &mut i64) -> String {
    *next_id = next_id.wrapping_add(1);
    format!("req-{}", *next_id)
}

pub fn build_ws_rpc_request(
    request_id: &str,
    method: &str,
    params: Option<Map<String, Value>>,
) -> Map<String, Value> {
    let mut request = Map::new();
    request.insert("jsonrpc".to_string(), Value::String("2.0".to_string()));
    request.insert("id".to_string(), Value::String(request_id.to_string()));
    request.insert("method".to_string(), Value::String(method.to_string()));
    if let Some(params) = params {
        request.insert("params".to_string(), Value::Object(params));
    }
    request
}

pub fn decode_ws_rpc_result(response: &Map<String, Value>) -> Result<Map<String, Value>, String> {
    if let Some(Value::Object(error)) = response.get("error") {
        return Err(format!(
            "json-rpc error {}: {}",
            go_fmt_value(error.get("code")),
            go_fmt_value(error.get("message"))
        ));
    }
    match response.get("result") {
        Some(Value::Object(result)) => Ok(result.clone()),
        _ => Ok(Map::new()),
    }
}

pub fn pending_failure_response(request_id: &str, error: &str) -> Map<String, Value> {
    Map::from_iter([
        (
            "error".to_string(),
            Value::Object(Map::from_iter([(
                "message".to_string(),
                Value::String(error.to_string()),
            )])),
        ),
        ("id".to_string(), Value::String(request_id.to_string())),
    ])
}

pub fn classify_incoming_message(message: &Map<String, Value>) -> IncomingWsMessage {
    match message.get("id") {
        Some(id) => IncomingWsMessage::Response {
            request_id: request_id_from_value(id),
        },
        None => IncomingWsMessage::Notification,
    }
}

fn go_fmt_value(value: Option<&Value>) -> String {
    match value {
        Some(Value::Null) | None => "<nil>".to_string(),
        Some(Value::String(value)) => value.clone(),
        Some(Value::Number(value)) => value.to_string(),
        Some(Value::Bool(value)) => value.to_string(),
        Some(Value::Array(value)) => format!("{value:?}"),
        Some(Value::Object(value)) => format!("{value:?}"),
    }
}
