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
