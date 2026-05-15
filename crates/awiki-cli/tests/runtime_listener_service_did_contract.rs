use awiki_cli::runtime::listener_service_did::{
    build_message_service_capabilities_call, disconnected_websocket_session_error,
    message_service_did_from_capabilities_result, MESSAGE_SERVICE_CAPABILITIES_METHOD,
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

fn object_map(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(map) => map,
        _ => Map::new(),
    }
}
