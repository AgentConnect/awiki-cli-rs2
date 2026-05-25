use serde_json::{Map, Value};

pub const MESSAGE_SERVICE_CAPABILITIES_METHOD: &str = "anp.get_capabilities";

#[derive(Debug, Clone, PartialEq)]
pub struct MessageServiceCapabilitiesCall {
    pub method: &'static str,
    pub params: Map<String, Value>,
}

pub fn build_message_service_capabilities_call() -> MessageServiceCapabilitiesCall {
    MessageServiceCapabilitiesCall {
        method: MESSAGE_SERVICE_CAPABILITIES_METHOD,
        params: Map::new(),
    }
}

pub fn disconnected_websocket_session_error(identity_name: &str) -> String {
    format!("websocket session is not connected for identity {identity_name}")
}

pub fn message_service_did_from_capabilities_result(
    result: &Map<String, Value>,
) -> anyhow::Result<String> {
    let service_did = match result.get("service_did") {
        Some(Value::String(value)) => value.clone(),
        _ => String::new(),
    };
    if service_did.is_empty() {
        anyhow::bail!("message service capabilities response is missing service_did");
    }
    Ok(service_did)
}

#[derive(Debug, Clone, PartialEq)]
pub struct ListenerServiceDidSession {
    pub identity_name: String,
    pub has_current_client: bool,
}

pub trait ListenerServiceDidRpc {
    fn send_rpc(
        &mut self,
        method: &str,
        params: Map<String, Value>,
    ) -> anyhow::Result<Map<String, Value>>;
}

pub fn fetch_message_service_did<R>(
    session: &ListenerServiceDidSession,
    rpc: &mut R,
) -> anyhow::Result<String>
where
    R: ListenerServiceDidRpc,
{
    if !session.has_current_client {
        anyhow::bail!(
            "{}",
            disconnected_websocket_session_error(&session.identity_name)
        );
    }
    let call = build_message_service_capabilities_call();
    let result = rpc.send_rpc(call.method, call.params)?;
    message_service_did_from_capabilities_result(&result)
}
