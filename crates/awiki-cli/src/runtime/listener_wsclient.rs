use crate::config::{self, Resolved};
use crate::identity::wire::DID_AUTH_RPC_ENDPOINT;
use crate::message::MESSAGE_WS_ENDPOINT;
use serde_json::{Map, Value};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListenerWsClientEndpoints {
    pub request_url: String,
    pub did_auth_url: String,
    pub websocket_url: String,
}

pub fn listener_ws_client_endpoints(
    resolved: &Resolved,
) -> anyhow::Result<ListenerWsClientEndpoints> {
    let request_url = config::join_base_url(&resolved.service_base_url, MESSAGE_WS_ENDPOINT);
    if request_url.trim().is_empty() {
        anyhow::bail!("service base url is required for websocket mode");
    }
    Ok(ListenerWsClientEndpoints {
        websocket_url: config::derive_websocket_url(
            &resolved.service_base_url,
            MESSAGE_WS_ENDPOINT,
        ),
        did_auth_url: config::join_base_url(&resolved.service_base_url, DID_AUTH_RPC_ENDPOINT),
        request_url,
    })
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IncomingWsMessage {
    Response { request_id: String },
    Notification,
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

pub fn decode_ws_rpc_result(response: &Map<String, Value>) -> anyhow::Result<Map<String, Value>> {
    if let Some(Value::Object(error)) = response.get("error") {
        anyhow::bail!(
            "json-rpc error {}: {}",
            go_fmt_value(error.get("code")),
            go_fmt_value(error.get("message"))
        );
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

pub fn host_for_url(raw: &str) -> String {
    let Some((scheme, rest)) = raw.split_once(':') else {
        return String::new();
    };
    if scheme.is_empty() {
        return raw.to_string();
    }
    if !valid_url_scheme(scheme) {
        return raw.to_string();
    }
    if !rest.starts_with("//") {
        return String::new();
    }
    let authority = rest[2..].split(['/', '?', '#']).next().unwrap_or_default();
    match parse_authority_host(authority) {
        Ok(host) => host,
        Err(_) => raw.to_string(),
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

fn valid_url_scheme(scheme: &str) -> bool {
    let mut chars = scheme.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    first.is_ascii_alphabetic()
        && chars.all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '+' | '-' | '.'))
}

fn parse_authority_host(authority: &str) -> anyhow::Result<String> {
    if authority.is_empty() {
        return Ok(String::new());
    }
    let without_userinfo = authority.rsplit('@').next().unwrap_or(authority);
    if without_userinfo.starts_with('[') {
        let Some((host, suffix)) = without_userinfo[1..].split_once(']') else {
            anyhow::bail!("missing ']' in host");
        };
        validate_host_chars(host)?;
        validate_port_suffix(suffix)?;
        return Ok(format!("[{host}]{}", port_suffix(suffix)));
    }
    if without_userinfo.contains(['[', ']']) {
        anyhow::bail!("invalid character in host name");
    }
    let (host, suffix) = without_userinfo
        .split_once(':')
        .map(|(host, port)| (host, Some(port)))
        .unwrap_or((without_userinfo, None));
    validate_host_chars(host)?;
    if let Some(port) = suffix {
        validate_port(port)?;
        return Ok(format!("{host}:{port}"));
    }
    Ok(host.to_string())
}

fn validate_host_chars(host: &str) -> anyhow::Result<()> {
    if host
        .chars()
        .any(|ch| ch.is_ascii_control() || ch.is_whitespace() || ch == '\\')
    {
        anyhow::bail!("invalid character in host name");
    }
    Ok(())
}

fn validate_port_suffix(suffix: &str) -> anyhow::Result<()> {
    if suffix.is_empty() {
        return Ok(());
    }
    let Some(port) = suffix.strip_prefix(':') else {
        anyhow::bail!("invalid character after host");
    };
    validate_port(port)
}

fn validate_port(port: &str) -> anyhow::Result<()> {
    if !port.is_empty() && !port.chars().all(|ch| ch.is_ascii_digit()) {
        anyhow::bail!("invalid port after host");
    }
    Ok(())
}

fn port_suffix(suffix: &str) -> &str {
    if suffix.is_empty() {
        ""
    } else {
        suffix
    }
}
