use crate::config::Resolved;
use serde::{de::DeserializeOwned, Serialize};
use serde_json::{Map, Value};

pub use im_core::compat::realtime::{
    build_ws_rpc_request, classify_incoming_message, int64_from_value, next_ws_rpc_request_id,
    pending_failure_response, request_id_from_value, IncomingWsMessage, ListenerWsDispatchOutcome,
    ListenerWsPendingDispatch, LISTENER_WS_NOTIFICATION_QUEUE_CAPACITY,
};
pub use im_core::compat::realtime::{
    RealtimeConnectAction as ListenerWsConnectAction,
    RealtimeConnectSimulation as ListenerWsConnectSimulation,
    RealtimeDialOutcome as ListenerWsDialOutcome,
    RealtimeRefreshOutcome as ListenerWsRefreshOutcome,
};

pub const DIAL_ERROR_BODY_LIMIT: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListenerWsClientEndpoints {
    pub request_url: String,
    pub did_auth_url: String,
    pub websocket_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListenerWsClientConstructionPlan {
    pub endpoints: ListenerWsClientEndpoints,
    pub remembered_scope_inputs: Vec<String>,
}

pub fn listener_ws_client_endpoints(
    resolved: &Resolved,
) -> anyhow::Result<ListenerWsClientEndpoints> {
    let endpoints =
        im_core::compat::realtime::realtime_client_endpoints(&resolved.service_base_url)
            .map_err(anyhow::Error::msg)?;
    Ok(ListenerWsClientEndpoints {
        request_url: endpoints.request_url,
        did_auth_url: endpoints.did_auth_url,
        websocket_url: endpoints.websocket_url,
    })
}

pub fn listener_ws_client_construction_plan(
    resolved: &Resolved,
) -> anyhow::Result<ListenerWsClientConstructionPlan> {
    let plan =
        im_core::compat::realtime::realtime_client_construction_plan(&resolved.service_base_url)
            .map_err(anyhow::Error::msg)?;
    let endpoints = plan.endpoints;
    Ok(ListenerWsClientConstructionPlan {
        endpoints: ListenerWsClientEndpoints {
            request_url: endpoints.request_url,
            did_auth_url: endpoints.did_auth_url,
            websocket_url: endpoints.websocket_url,
        },
        remembered_scope_inputs: plan.remembered_scope_inputs,
    })
}

pub fn bearer_authorization_header(token: &str) -> String {
    im_core::compat::realtime::bearer_authorization_header(token)
}

pub fn validate_refresh_bearer_preconditions(
    has_auth_session: bool,
    did_auth_url: &str,
) -> anyhow::Result<()> {
    im_core::compat::realtime::validate_refresh_bearer_preconditions(has_auth_session, did_auth_url)
        .map_err(anyhow::Error::msg)
}

pub fn simulate_listener_ws_connect(
    current_jwt: &str,
    dial_bearer: impl FnMut(&str) -> ListenerWsDialOutcome,
    refresh_bearer: impl FnMut() -> ListenerWsRefreshOutcome,
) -> ListenerWsConnectSimulation {
    im_core::compat::realtime::simulate_realtime_connect(current_jwt, dial_bearer, refresh_bearer)
}

pub fn decode_ws_rpc_result(response: &Map<String, Value>) -> anyhow::Result<Map<String, Value>> {
    im_core::compat::realtime::decode_ws_rpc_result(response).map_err(anyhow::Error::msg)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListenerWsFrameKind {
    Text,
    Binary,
}

pub trait ListenerWsJsonConnection {
    fn write_frame(&mut self, kind: ListenerWsFrameKind, raw: Vec<u8>) -> anyhow::Result<()>;
    fn read_frame(&mut self) -> anyhow::Result<(ListenerWsFrameKind, Vec<u8>)>;
}

pub fn ws_json_write<C, T>(conn: &mut C, payload: &T) -> anyhow::Result<()>
where
    C: ListenerWsJsonConnection,
    T: Serialize,
{
    let raw = serde_json::to_vec(payload)?;
    conn.write_frame(ListenerWsFrameKind::Text, raw)
}

pub fn ws_json_read<C, T>(conn: &mut C) -> anyhow::Result<T>
where
    C: ListenerWsJsonConnection,
    T: DeserializeOwned,
{
    let (_, raw) = conn.read_frame()?;
    Ok(serde_json::from_slice(&raw)?)
}

pub fn format_dial_error_message(
    error: Option<&str>,
    response_body: Option<&[u8]>,
) -> Option<String> {
    im_core::compat::realtime::format_dial_error_message(error, response_body)
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
