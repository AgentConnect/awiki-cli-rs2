use awiki_cli::host_runtime::listener_service_did::{
    build_message_service_capabilities_call, disconnected_websocket_session_error,
    fetch_message_service_did, message_service_did_from_capabilities_result, ListenerServiceDidRpc,
    ListenerServiceDidSession, MESSAGE_SERVICE_CAPABILITIES_METHOD,
};
use serde_json::{json, Map, Value};

#[test]
fn listener_service_did_builds_go_capabilities_request_shape() {
    let call = build_message_service_capabilities_call();

    assert_eq!(call.method, MESSAGE_SERVICE_CAPABILITIES_METHOD);
    assert_eq!(call.method, "anp.get_capabilities");
    assert!(call.params.is_empty());
}

#[test]
fn listener_service_did_decodes_string_service_did_like_go() {
    let did = message_service_did_from_capabilities_result(&object_map(json!({
        "service_did": "did:wba:capabilities.example",
    })))
    .expect("service did");

    assert_eq!(did, "did:wba:capabilities.example");
}

#[test]
fn listener_service_did_preserves_listener_string_value_boundary() {
    let did = message_service_did_from_capabilities_result(&object_map(json!({
        "service_did": "  did:wba:capabilities.example  ",
    })))
    .expect("service did with spaces");
    assert_eq!(did, "  did:wba:capabilities.example  ");

    let did = message_service_did_from_capabilities_result(&object_map(json!({
        "service_did": "   ",
    })))
    .expect("whitespace-only did is non-empty in Go listener helper");
    assert_eq!(did, "   ");
}

#[test]
fn listener_service_did_rejects_missing_empty_or_non_string_values_like_go() {
    for result in [
        json!({}),
        json!({ "service_did": "" }),
        json!({ "service_did": null }),
        json!({ "service_did": 123 }),
        json!({ "service_did": true }),
        json!({ "service_did": ["did:wba:capabilities.example"] }),
        json!({ "service_did": { "did": "did:wba:capabilities.example" } }),
    ] {
        let err = message_service_did_from_capabilities_result(&object_map(result))
            .expect_err("missing service did error");
        assert_eq!(
            err.to_string(),
            "message service capabilities response is missing service_did"
        );
    }
}

#[test]
fn listener_service_did_preserves_disconnected_session_error_text() {
    assert_eq!(
        disconnected_websocket_session_error("alice"),
        "websocket session is not connected for identity alice"
    );
    assert_eq!(
        disconnected_websocket_session_error(""),
        "websocket session is not connected for identity "
    );
}

#[test]
fn fetch_message_service_did_returns_disconnected_error_before_rpc() {
    let session = session("alice", false);
    let mut rpc = RecordingRpc::ok(json!({"service_did": "did:wba:service"}));

    let err = fetch_message_service_did(&session, &mut rpc).expect_err("disconnected");

    assert_eq!(
        err.to_string(),
        "websocket session is not connected for identity alice"
    );
    assert!(rpc.calls.is_empty());
}

#[test]
fn fetch_message_service_did_sends_go_capabilities_rpc_shape() {
    let session = session("alice", true);
    let mut rpc = RecordingRpc::ok(json!({
        "service_did": "did:wba:message.example",
    }));

    let did = fetch_message_service_did(&session, &mut rpc).expect("service did");

    assert_eq!(did, "did:wba:message.example");
    assert_eq!(
        rpc.calls,
        vec![RpcCall {
            method: "anp.get_capabilities".to_string(),
            params: Map::new(),
        }]
    );
}

#[test]
fn fetch_message_service_did_propagates_send_rpc_errors_before_decoding() {
    let session = session("alice", true);
    let mut rpc = RecordingRpc::error("send failed");

    let err = fetch_message_service_did(&session, &mut rpc).expect_err("send error");

    assert_eq!(err.to_string(), "send failed");
    assert_eq!(rpc.calls.len(), 1);
}

#[test]
fn fetch_message_service_did_uses_decoder_for_missing_or_non_string_result() {
    for result in [
        json!({}),
        json!({"service_did": ""}),
        json!({"service_did": 42}),
    ] {
        let session = session("alice", true);
        let mut rpc = RecordingRpc::ok(result);

        let err = fetch_message_service_did(&session, &mut rpc).expect_err("decode error");

        assert_eq!(
            err.to_string(),
            "message service capabilities response is missing service_did"
        );
    }
}

fn object_map(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(map) => map,
        _ => Map::new(),
    }
}

fn session(identity_name: &str, has_current_client: bool) -> ListenerServiceDidSession {
    ListenerServiceDidSession {
        identity_name: identity_name.to_string(),
        has_current_client,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RpcCall {
    method: String,
    params: Map<String, Value>,
}

struct RecordingRpc {
    calls: Vec<RpcCall>,
    result: anyhow::Result<Map<String, Value>>,
}

impl RecordingRpc {
    fn ok(result: Value) -> Self {
        Self {
            calls: Vec::new(),
            result: Ok(object_map(result)),
        }
    }

    fn error(error: &str) -> Self {
        Self {
            calls: Vec::new(),
            result: Err(anyhow::anyhow!(error.to_string())),
        }
    }
}

impl ListenerServiceDidRpc for RecordingRpc {
    fn send_rpc(
        &mut self,
        method: &str,
        params: Map<String, Value>,
    ) -> anyhow::Result<Map<String, Value>> {
        self.calls.push(RpcCall {
            method: method.to_string(),
            params,
        });
        match &self.result {
            Ok(result) => Ok(result.clone()),
            Err(error) => Err(anyhow::anyhow!(error.to_string())),
        }
    }
}
