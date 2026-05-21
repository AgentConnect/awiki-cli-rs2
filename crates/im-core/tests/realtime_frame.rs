use im_core::compat::realtime;
use serde_json::{json, Map, Value};

#[test]
fn realtime_frame_request_id_and_int64_coercion_match_legacy_helpers() {
    assert_eq!(
        realtime::request_id_from_value(&json!("req-123")),
        "req-123"
    );
    assert_eq!(realtime::request_id_from_value(&json!(42)), "42");
    assert_eq!(realtime::request_id_from_value(&json!(-7)), "-7");
    assert_eq!(realtime::request_id_from_value(&json!(1.2)), "1");
    assert_eq!(realtime::request_id_from_value(&json!(1.6)), "2");
    assert_eq!(realtime::request_id_from_value(&json!(2.5)), "2");
    assert_eq!(realtime::request_id_from_value(&json!(-2.5)), "-2");
    assert_eq!(realtime::request_id_from_value(&json!(true)), "");
    assert_eq!(realtime::request_id_from_value(&json!({})), "");

    assert_eq!(realtime::int64_from_value(&json!(42)), 42);
    assert_eq!(realtime::int64_from_value(&json!(-7)), -7);
    assert_eq!(realtime::int64_from_value(&json!(1.9)), 1);
    assert_eq!(realtime::int64_from_value(&json!(-1.9)), -1);
    assert_eq!(realtime::int64_from_value(&json!("42")), 0);
    assert_eq!(realtime::int64_from_value(&json!(null)), 0);
}

#[test]
fn realtime_frame_rpc_request_shape_matches_legacy_send_rpc_envelope() {
    let no_params = realtime::build_ws_rpc_request("req-1", "inbox.get", None);
    assert_eq!(no_params["jsonrpc"], "2.0");
    assert_eq!(no_params["id"], "req-1");
    assert_eq!(no_params["method"], "inbox.get");
    assert!(no_params.get("params").is_none());

    let with_empty_params =
        realtime::build_ws_rpc_request("req-2", "anp.get_capabilities", Some(map([])));
    assert_eq!(with_empty_params["params"], json!({}));

    let with_params = realtime::build_ws_rpc_request(
        "req-3",
        "direct.send",
        Some(map([
            ("meta", json!({ "profile": "anp.direct.base.v1" })),
            ("body", json!({ "text": "hello" })),
        ])),
    );
    assert_eq!(with_params["params"]["body"]["text"], "hello");
}

#[test]
fn realtime_frame_request_id_generation_matches_legacy_sequence() {
    let mut next_id = 0;

    assert_eq!(realtime::next_ws_rpc_request_id(&mut next_id), "req-1");
    assert_eq!(next_id, 1);
    assert_eq!(realtime::next_ws_rpc_request_id(&mut next_id), "req-2");
    assert_eq!(next_id, 2);
}

#[test]
fn realtime_frame_pending_dispatch_prepares_request_after_registering_pending_slot() {
    let mut dispatch = realtime::ListenerWsPendingDispatch::default();
    let mut next_id = 0;

    let request = dispatch.prepare_ws_rpc_request(
        &mut next_id,
        "direct.send",
        Some(map([("body", json!({ "text": "hello" }))])),
    );

    assert_eq!(request["id"], "req-1");
    assert_eq!(request["method"], "direct.send");
    assert_eq!(request["params"]["body"]["text"], "hello");
    assert!(dispatch.has_pending("req-1"));
    assert_eq!(dispatch.pending_len(), 1);
    assert!(dispatch.take_pending_response("req-1").is_none());
}

#[test]
fn realtime_frame_rpc_result_decode_matches_legacy_response_boundary() {
    let result = realtime::decode_ws_rpc_result(&map([(
        "result",
        json!({ "message_id": "msg-123", "accepted": true }),
    )]))
    .expect("result map");
    assert_eq!(result["message_id"], "msg-123");
    assert_eq!(result["accepted"], true);

    let missing_result =
        realtime::decode_ws_rpc_result(&map([])).expect("missing result is empty map");
    assert!(missing_result.is_empty());

    let scalar_result = realtime::decode_ws_rpc_result(&map([("result", json!(true))]))
        .expect("non-object result is empty map");
    assert!(scalar_result.is_empty());

    let scalar_error = realtime::decode_ws_rpc_result(&map([("error", json!("boom"))]))
        .expect("non-object error is ignored like Go type assertion");
    assert!(scalar_error.is_empty());

    let err = realtime::decode_ws_rpc_result(&map([(
        "error",
        json!({ "code": -32001, "message": "denied" }),
    )]))
    .expect_err("json-rpc error");
    assert_eq!(err, "json-rpc error -32001: denied");

    let err = realtime::decode_ws_rpc_result(&map([("error", json!({}))]))
        .expect_err("missing error fields");
    assert_eq!(err, "json-rpc error <nil>: <nil>");
}

#[test]
fn realtime_frame_pending_failure_and_incoming_classification_match_legacy_helpers() {
    let failed = realtime::pending_failure_response("req-7", "reader closed");
    assert_eq!(failed["id"], "req-7");
    assert_eq!(failed["error"]["message"], "reader closed");
    assert!(failed["error"].get("code").is_none());

    assert_eq!(
        realtime::classify_incoming_message(&map([("id", json!("req-7"))])),
        realtime::IncomingWsMessage::Response {
            request_id: "req-7".to_string()
        }
    );
    assert_eq!(
        realtime::classify_incoming_message(&map([("id", json!(7.6))])),
        realtime::IncomingWsMessage::Response {
            request_id: "8".to_string()
        }
    );
    assert_eq!(
        realtime::classify_incoming_message(&map([("method", json!("direct.incoming"))])),
        realtime::IncomingWsMessage::Notification
    );
}

#[test]
fn realtime_frame_pending_dispatch_routes_known_normalized_ids_and_drops_unknown_responses() {
    let mut dispatch = realtime::ListenerWsPendingDispatch::default();
    assert!(dispatch.register_pending("req-7"));
    assert!(dispatch.register_pending("8"));

    assert_eq!(
        dispatch.route_incoming_message(map([
            ("id", json!("req-7")),
            ("result", json!({ "accepted": true })),
        ])),
        realtime::ListenerWsDispatchOutcome::RoutedResponse {
            request_id: "req-7".to_string()
        }
    );
    assert_eq!(
        dispatch.route_incoming_message(map([
            ("id", json!(7.6)),
            ("result", json!({ "coerced": true })),
        ])),
        realtime::ListenerWsDispatchOutcome::RoutedResponse {
            request_id: "8".to_string()
        }
    );
    assert_eq!(
        dispatch.route_incoming_message(map([
            ("id", json!("missing")),
            ("method", json!("must.not.be.notification")),
        ])),
        realtime::ListenerWsDispatchOutcome::DroppedResponse {
            request_id: "missing".to_string()
        }
    );

    let response = dispatch
        .take_pending_response("req-7")
        .expect("routed response");
    assert_eq!(response["result"]["accepted"], true);
    let response = dispatch.take_pending_response("8").expect("coerced id");
    assert_eq!(response["result"]["coerced"], true);
    assert!(dispatch.take_pending_response("missing").is_none());
    assert_eq!(dispatch.notification_len(), 0);
}

#[test]
fn realtime_frame_pending_dispatch_fail_all_synthesizes_responses_without_removing_entries() {
    let mut dispatch = realtime::ListenerWsPendingDispatch::default();
    dispatch.register_pending("req-1");
    dispatch.register_pending("req-2");

    let failed = dispatch.fail_pending_requests("websocket read failed");

    assert_eq!(failed, vec!["req-1".to_string(), "req-2".to_string()]);
    assert_eq!(dispatch.pending_len(), 2);
    assert!(dispatch.has_pending("req-1"));
    assert!(dispatch.has_pending("req-2"));

    let req_1_failure = dispatch
        .take_pending_response("req-1")
        .expect("req-1 failure response");
    assert_eq!(
        req_1_failure,
        realtime::pending_failure_response("req-1", "websocket read failed")
    );
    let req_2_failure = dispatch
        .take_pending_response("req-2")
        .expect("req-2 failure response");
    assert_eq!(
        req_2_failure,
        realtime::pending_failure_response("req-2", "websocket read failed")
    );

    let removed = dispatch
        .remove_pending("req-1")
        .expect("SendRPC cleanup removes its own pending entry");
    assert!(removed.is_empty());
    assert_eq!(dispatch.pending_len(), 1);
    assert!(!dispatch.has_pending("req-1"));
    assert!(dispatch.has_pending("req-2"));
}

#[test]
fn realtime_frame_pending_dispatch_queues_notifications_with_legacy_drop_on_full_semantics() {
    let mut dispatch = realtime::ListenerWsPendingDispatch::with_notification_capacity(1);

    assert_eq!(
        dispatch.route_incoming_message(map([("method", json!("direct.incoming"))])),
        realtime::ListenerWsDispatchOutcome::QueuedNotification
    );
    assert_eq!(
        dispatch.route_incoming_message(map([("method", json!("group.incoming"))])),
        realtime::ListenerWsDispatchOutcome::DroppedNotification
    );

    assert_eq!(dispatch.notification_len(), 1);
    let queued = dispatch.pop_notification().expect("queued notification");
    assert_eq!(queued["method"], "direct.incoming");
    assert!(dispatch.pop_notification().is_none());
}

fn map<const N: usize>(entries: [(&str, Value); N]) -> Map<String, Value> {
    Map::from_iter(
        entries
            .into_iter()
            .map(|(key, value)| (key.to_string(), value)),
    )
}
