use crate::config::{self, Resolved};
use crate::identity::wire::DID_AUTH_RPC_ENDPOINT;
use crate::message::MESSAGE_WS_ENDPOINT;
use serde_json::Value;

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
