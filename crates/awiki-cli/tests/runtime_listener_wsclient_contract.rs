use awiki_cli::config::{self, Paths, Resolved};
use awiki_cli::runtime_legacy::listener_ws_transport::WsTransport;
use awiki_cli::runtime_legacy::listener_wsclient;
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use serde::ser::{Serialize, Serializer};
use serde_json::json;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::{Duration, Instant};

#[test]
fn listener_ws_client_endpoints_match_go_new_ws_client_derivation() {
    let resolved = Resolved {
        service_base_url: "http://127.0.0.1:18080".to_string(),
        ..test_resolved()
    };
    let endpoints =
        listener_wsclient::listener_ws_client_endpoints(&resolved).expect("listener endpoints");

    assert_eq!(endpoints.request_url, "http://127.0.0.1:18080/im/ws");
    assert_eq!(endpoints.websocket_url, "ws://127.0.0.1:18080/im/ws");
    assert_eq!(
        endpoints.did_auth_url,
        "http://127.0.0.1:18080/user-service/did-auth/rpc"
    );
}

#[test]
fn derive_websocket_url_matches_go_config_helper() {
    assert_eq!(
        config::derive_websocket_url("https://awiki.ai///", "im/ws"),
        "wss://awiki.ai/im/ws"
    );
    assert_eq!(
        config::derive_websocket_url("http://127.0.0.1:18080", "/im/ws"),
        "ws://127.0.0.1:18080/im/ws"
    );
    assert_eq!(
        config::derive_websocket_url("ws://already.example", "/im/ws"),
        "ws://already.example/im/ws"
    );
    assert_eq!(
        config::derive_websocket_url("HTTP://case-sensitive.example", "/im/ws"),
        "HTTP://case-sensitive.example/im/ws"
    );
    assert_eq!(
        config::derive_websocket_url("", "/im/ws"),
        "/im/ws",
        "Go NewWSClient only errors when JoinBaseURL result is blank"
    );
    assert_eq!(
        config::join_base_url("https://awiki.ai///", ""),
        "https://awiki.ai"
    );
}

#[test]
fn listener_ws_client_endpoints_preserve_go_empty_base_boundary() {
    let resolved = Resolved {
        service_base_url: String::new(),
        ..test_resolved()
    };
    let endpoints =
        listener_wsclient::listener_ws_client_endpoints(&resolved).expect("empty base boundary");

    assert_eq!(endpoints.request_url, "/im/ws");
    assert_eq!(endpoints.websocket_url, "/im/ws");
    assert_eq!(endpoints.did_auth_url, "/user-service/did-auth/rpc");
}

#[test]
fn listener_ws_client_construction_plan_matches_go_scope_side_effects() {
    let resolved = Resolved {
        service_base_url: "http://127.0.0.1:18080/".to_string(),
        ..test_resolved()
    };
    let plan = listener_wsclient::listener_ws_client_construction_plan(&resolved)
        .expect("construction plan");

    assert_eq!(plan.endpoints.request_url, "http://127.0.0.1:18080/im/ws");
    assert_eq!(
        plan.remembered_scope_inputs,
        vec![
            "http://127.0.0.1:18080/".to_string(),
            "http://127.0.0.1:18080/user-service/did-auth/rpc".to_string(),
            "http://127.0.0.1:18080/im/ws".to_string(),
        ],
        "Go NewWSClient calls RememberScope in this exact input order"
    );
}

#[test]
fn bearer_authorization_header_trims_like_go_dial_bearer() {
    assert_eq!(
        listener_wsclient::bearer_authorization_header("  expired-token \n"),
        "Bearer expired-token"
    );
}

#[test]
fn refresh_bearer_preconditions_match_go_errors() {
    let err =
        listener_wsclient::validate_refresh_bearer_preconditions(false, "https://awiki.ai/rpc")
            .expect_err("missing auth");
    assert_eq!(
        err.to_string(),
        "auth session is required for websocket mode"
    );

    let err = listener_wsclient::validate_refresh_bearer_preconditions(true, " \n ")
        .expect_err("missing did-auth url");
    assert_eq!(
        err.to_string(),
        "did-auth rpc url is required for websocket mode"
    );

    listener_wsclient::validate_refresh_bearer_preconditions(true, "https://awiki.ai/rpc")
        .expect("valid preconditions");
}

#[test]
fn connect_simulation_uses_existing_bearer_and_stops_on_success() {
    let mut dialed = Vec::new();
    let mut refreshes = 0;
    let result = listener_wsclient::simulate_listener_ws_connect(
        "  live-token \n",
        |token| {
            dialed.push(token.to_string());
            listener_wsclient::ListenerWsDialOutcome::Connected
        },
        || {
            refreshes += 1;
            listener_wsclient::ListenerWsRefreshOutcome::Failed {
                error: "refresh should not run".to_string(),
            }
        },
    );

    assert_eq!(
        result.actions,
        vec![
            dial_action("live-token"),
            listener_wsclient::ListenerWsConnectAction::Attach,
        ]
    );
    assert_eq!(result.error, None);
    assert_eq!(dialed, vec!["live-token"]);
    assert_eq!(refreshes, 0);
}

#[test]
fn connect_simulation_refreshes_expired_bearer_before_retrying_websocket() {
    let mut dialed = Vec::new();
    let mut refreshes = 0;
    let result = listener_wsclient::simulate_listener_ws_connect(
        "expired-token",
        |token| {
            dialed.push(token.to_string());
            if token == "expired-token" {
                listener_wsclient::ListenerWsDialOutcome::Failed {
                    status_code: Some(401),
                    error: "websocket dial failed".to_string(),
                    response_body: Some(
                        br#"{"jsonrpc":"2.0","error":{"code":1401,"message":"expired session"}}"#
                            .to_vec(),
                    ),
                }
            } else {
                listener_wsclient::ListenerWsDialOutcome::Connected
            }
        },
        || {
            refreshes += 1;
            listener_wsclient::ListenerWsRefreshOutcome::Refreshed {
                current_jwt: " refreshed-token ".to_string(),
            }
        },
    );

    assert_eq!(
        result.actions,
        vec![
            dial_action("expired-token"),
            listener_wsclient::ListenerWsConnectAction::RefreshBearer,
            dial_action("refreshed-token"),
            listener_wsclient::ListenerWsConnectAction::Attach,
        ]
    );
    assert_eq!(result.error, None);
    assert_eq!(dialed, vec!["expired-token", "refreshed-token"]);
    assert_eq!(refreshes, 1);
}

#[test]
fn connect_simulation_bootstraps_bearer_before_opening_websocket() {
    let mut dialed = Vec::new();
    let mut refreshes = 0;
    let result = listener_wsclient::simulate_listener_ws_connect(
        " \n ",
        |token| {
            dialed.push(token.to_string());
            listener_wsclient::ListenerWsDialOutcome::Connected
        },
        || {
            refreshes += 1;
            listener_wsclient::ListenerWsRefreshOutcome::Refreshed {
                current_jwt: " bootstrapped-token ".to_string(),
            }
        },
    );

    assert_eq!(
        result.actions,
        vec![
            listener_wsclient::ListenerWsConnectAction::RefreshBearer,
            dial_action("bootstrapped-token"),
            listener_wsclient::ListenerWsConnectAction::Attach,
        ]
    );
    assert_eq!(result.error, None);
    assert_eq!(dialed, vec!["bootstrapped-token"]);
    assert_eq!(refreshes, 1);
}

#[test]
fn connect_simulation_returns_non_unauthorized_first_dial_error_without_refresh() {
    let mut dialed = Vec::new();
    let mut refreshes = 0;
    let result = listener_wsclient::simulate_listener_ws_connect(
        "stale-token",
        |token| {
            dialed.push(token.to_string());
            listener_wsclient::ListenerWsDialOutcome::Failed {
                status_code: Some(500),
                error: "websocket dial failed".to_string(),
                response_body: Some(b"  upstream down\n".to_vec()),
            }
        },
        || {
            refreshes += 1;
            listener_wsclient::ListenerWsRefreshOutcome::Refreshed {
                current_jwt: "must-not-use".to_string(),
            }
        },
    );

    assert_eq!(result.actions, vec![dial_action("stale-token")]);
    assert_eq!(
        result.error,
        Some("websocket dial failed: upstream down".to_string())
    );
    assert_eq!(dialed, vec!["stale-token"]);
    assert_eq!(refreshes, 0);
}

#[test]
fn connect_simulation_wraps_refresh_error_only_after_existing_bearer() {
    let mut dialed = Vec::new();
    let mut refreshes = 0;
    let with_token = listener_wsclient::simulate_listener_ws_connect(
        "expired-token",
        |token| {
            dialed.push(token.to_string());
            listener_wsclient::ListenerWsDialOutcome::Failed {
                status_code: Some(401),
                error: "unauthorized".to_string(),
                response_body: None,
            }
        },
        || {
            refreshes += 1;
            listener_wsclient::ListenerWsRefreshOutcome::Failed {
                error: "did-auth denied".to_string(),
            }
        },
    );
    assert_eq!(
        with_token.actions,
        vec![
            dial_action("expired-token"),
            listener_wsclient::ListenerWsConnectAction::RefreshBearer,
        ]
    );
    assert_eq!(
        with_token.error,
        Some("refresh websocket session JWT: did-auth denied".to_string())
    );
    assert_eq!(dialed, vec!["expired-token"]);
    assert_eq!(refreshes, 1);

    let mut dialed = Vec::<String>::new();
    let mut refreshes = 0;
    let without_token = listener_wsclient::simulate_listener_ws_connect(
        "",
        |token| {
            dialed.push(token.to_string());
            listener_wsclient::ListenerWsDialOutcome::Connected
        },
        || {
            refreshes += 1;
            listener_wsclient::ListenerWsRefreshOutcome::Failed {
                error: "did-auth denied".to_string(),
            }
        },
    );
    assert_eq!(
        without_token.actions,
        vec![listener_wsclient::ListenerWsConnectAction::RefreshBearer]
    );
    assert_eq!(without_token.error, Some("did-auth denied".to_string()));
    assert!(dialed.is_empty());
    assert_eq!(refreshes, 1);
}

#[test]
fn connect_simulation_requires_non_empty_refreshed_bearer() {
    let mut dialed = Vec::new();
    let mut refreshes = 0;
    let result = listener_wsclient::simulate_listener_ws_connect(
        "expired-token",
        |token| {
            dialed.push(token.to_string());
            listener_wsclient::ListenerWsDialOutcome::Failed {
                status_code: Some(401),
                error: "unauthorized".to_string(),
                response_body: None,
            }
        },
        || {
            refreshes += 1;
            listener_wsclient::ListenerWsRefreshOutcome::Refreshed {
                current_jwt: " \n ".to_string(),
            }
        },
    );

    assert_eq!(
        result.actions,
        vec![
            dial_action("expired-token"),
            listener_wsclient::ListenerWsConnectAction::RefreshBearer,
        ]
    );
    assert_eq!(
        result.error,
        Some("did-auth did not return a websocket bearer token".to_string())
    );
    assert_eq!(dialed, vec!["expired-token"]);
    assert_eq!(refreshes, 1);
}

#[test]
fn connect_simulation_formats_retry_dial_error_like_go() {
    let mut dialed = Vec::new();
    let mut refreshes = 0;
    let result = listener_wsclient::simulate_listener_ws_connect(
        "",
        |token| {
            dialed.push(token.to_string());
            listener_wsclient::ListenerWsDialOutcome::Failed {
                status_code: Some(403),
                error: "websocket dial failed".to_string(),
                response_body: Some(b" forbidden ".to_vec()),
            }
        },
        || {
            refreshes += 1;
            listener_wsclient::ListenerWsRefreshOutcome::Refreshed {
                current_jwt: "new-token".to_string(),
            }
        },
    );

    assert_eq!(
        result.actions,
        vec![
            listener_wsclient::ListenerWsConnectAction::RefreshBearer,
            dial_action("new-token"),
        ]
    );
    assert_eq!(
        result.error,
        Some("websocket dial failed: forbidden".to_string())
    );
    assert_eq!(dialed, vec!["new-token"]);
    assert_eq!(refreshes, 1);
}

#[test]
fn request_id_and_int64_coercion_match_go_helpers() {
    assert_eq!(
        listener_wsclient::request_id_from_value(&json!("req-123")),
        "req-123"
    );
    assert_eq!(listener_wsclient::request_id_from_value(&json!(42)), "42");
    assert_eq!(listener_wsclient::request_id_from_value(&json!(-7)), "-7");
    assert_eq!(listener_wsclient::request_id_from_value(&json!(1.2)), "1");
    assert_eq!(listener_wsclient::request_id_from_value(&json!(1.6)), "2");
    assert_eq!(listener_wsclient::request_id_from_value(&json!(2.5)), "2");
    assert_eq!(listener_wsclient::request_id_from_value(&json!(-2.5)), "-2");
    assert_eq!(listener_wsclient::request_id_from_value(&json!(true)), "");
    assert_eq!(listener_wsclient::request_id_from_value(&json!({})), "");

    assert_eq!(listener_wsclient::int64_from_value(&json!(42)), 42);
    assert_eq!(listener_wsclient::int64_from_value(&json!(-7)), -7);
    assert_eq!(listener_wsclient::int64_from_value(&json!(1.9)), 1);
    assert_eq!(listener_wsclient::int64_from_value(&json!(-1.9)), -1);
    assert_eq!(listener_wsclient::int64_from_value(&json!("42")), 0);
    assert_eq!(listener_wsclient::int64_from_value(&json!(null)), 0);
}

#[test]
fn ws_rpc_request_shape_matches_go_send_rpc_envelope() {
    let no_params = listener_wsclient::build_ws_rpc_request("req-1", "inbox.get", None);
    assert_eq!(no_params["jsonrpc"], "2.0");
    assert_eq!(no_params["id"], "req-1");
    assert_eq!(no_params["method"], "inbox.get");
    assert!(no_params.get("params").is_none());

    let with_empty_params =
        listener_wsclient::build_ws_rpc_request("req-2", "anp.get_capabilities", Some(map([])));
    assert_eq!(with_empty_params["params"], json!({}));

    let with_params = listener_wsclient::build_ws_rpc_request(
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
fn ws_rpc_request_id_generation_matches_go_send_rpc_sequence() {
    let mut next_id = 0;

    assert_eq!(
        listener_wsclient::next_ws_rpc_request_id(&mut next_id),
        "req-1"
    );
    assert_eq!(next_id, 1);
    assert_eq!(
        listener_wsclient::next_ws_rpc_request_id(&mut next_id),
        "req-2"
    );
    assert_eq!(next_id, 2);
}

#[test]
fn pending_dispatch_prepares_request_after_registering_pending_slot() {
    let mut dispatch = listener_wsclient::ListenerWsPendingDispatch::default();
    let mut next_id = 0;

    let request = dispatch.prepare_ws_rpc_request(
        &mut next_id,
        "direct.send",
        Some(map([("body", json!({ "text": "hello" }))])),
    );

    assert_eq!(request["id"], "req-1");
    assert_eq!(request["method"], "direct.send");
    assert_eq!(request["params"]["body"]["text"], "hello");
    assert!(
        dispatch.has_pending("req-1"),
        "SendRPC registers pending before the websocket write"
    );
    assert_eq!(dispatch.pending_len(), 1);
    assert!(dispatch.take_pending_response("req-1").is_none());
}

#[test]
fn ws_rpc_result_decode_matches_go_send_rpc_response_boundary() {
    let result = listener_wsclient::decode_ws_rpc_result(&map([(
        "result",
        json!({ "message_id": "msg-123", "accepted": true }),
    )]))
    .expect("result map");
    assert_eq!(result["message_id"], "msg-123");
    assert_eq!(result["accepted"], true);

    let missing_result =
        listener_wsclient::decode_ws_rpc_result(&map([])).expect("missing result is empty map");
    assert!(missing_result.is_empty());

    let scalar_result = listener_wsclient::decode_ws_rpc_result(&map([("result", json!(true))]))
        .expect("non-object result is empty map");
    assert!(scalar_result.is_empty());

    let scalar_error = listener_wsclient::decode_ws_rpc_result(&map([("error", json!("boom"))]))
        .expect("non-object error is ignored like Go type assertion");
    assert!(scalar_error.is_empty());

    let err = listener_wsclient::decode_ws_rpc_result(&map([(
        "error",
        json!({ "code": -32001, "message": "denied" }),
    )]))
    .expect_err("json-rpc error");
    assert_eq!(err.to_string(), "json-rpc error -32001: denied");

    let err = listener_wsclient::decode_ws_rpc_result(&map([("error", json!({}))]))
        .expect_err("missing error fields");
    assert_eq!(err.to_string(), "json-rpc error <nil>: <nil>");
}

#[test]
fn pending_failure_and_incoming_classification_match_go_read_loop_helpers() {
    let failed = listener_wsclient::pending_failure_response("req-7", "reader closed");
    assert_eq!(failed["id"], "req-7");
    assert_eq!(failed["error"]["message"], "reader closed");
    assert!(failed["error"].get("code").is_none());

    assert_eq!(
        listener_wsclient::classify_incoming_message(&map([("id", json!("req-7"))])),
        listener_wsclient::IncomingWsMessage::Response {
            request_id: "req-7".to_string()
        }
    );
    assert_eq!(
        listener_wsclient::classify_incoming_message(&map([("id", json!(7.6))])),
        listener_wsclient::IncomingWsMessage::Response {
            request_id: "8".to_string()
        }
    );
    assert_eq!(
        listener_wsclient::classify_incoming_message(&map([("method", json!("direct.incoming"))])),
        listener_wsclient::IncomingWsMessage::Notification
    );
}

#[test]
fn pending_dispatch_routes_known_normalized_ids_and_drops_unknown_responses() {
    let mut dispatch = listener_wsclient::ListenerWsPendingDispatch::default();
    assert!(dispatch.register_pending("req-7"));
    assert!(dispatch.register_pending("8"));

    assert_eq!(
        dispatch.route_incoming_message(map([
            ("id", json!("req-7")),
            ("result", json!({ "accepted": true })),
        ])),
        listener_wsclient::ListenerWsDispatchOutcome::RoutedResponse {
            request_id: "req-7".to_string()
        }
    );
    assert_eq!(
        dispatch.route_incoming_message(map([
            ("id", json!(7.6)),
            ("result", json!({ "coerced": true })),
        ])),
        listener_wsclient::ListenerWsDispatchOutcome::RoutedResponse {
            request_id: "8".to_string()
        }
    );
    assert_eq!(
        dispatch.route_incoming_message(map([
            ("id", json!("missing")),
            ("method", json!("must.not.be.notification")),
        ])),
        listener_wsclient::ListenerWsDispatchOutcome::DroppedResponse {
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
    assert_eq!(
        dispatch.notification_len(),
        0,
        "unknown responses are dropped instead of reclassified as notifications"
    );
}

#[test]
fn pending_dispatch_fail_all_synthesizes_responses_without_removing_pending_entries() {
    let mut dispatch = listener_wsclient::ListenerWsPendingDispatch::default();
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
        listener_wsclient::pending_failure_response("req-1", "websocket read failed")
    );
    let req_2_failure = dispatch
        .take_pending_response("req-2")
        .expect("req-2 failure response");
    assert_eq!(
        req_2_failure,
        listener_wsclient::pending_failure_response("req-2", "websocket read failed")
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
fn pending_dispatch_queues_notifications_with_go_drop_on_full_semantics() {
    let mut dispatch = listener_wsclient::ListenerWsPendingDispatch::with_notification_capacity(1);

    assert_eq!(
        dispatch.route_incoming_message(map([("method", json!("direct.incoming"))])),
        listener_wsclient::ListenerWsDispatchOutcome::QueuedNotification
    );
    assert_eq!(
        dispatch.route_incoming_message(map([("method", json!("group.incoming"))])),
        listener_wsclient::ListenerWsDispatchOutcome::DroppedNotification
    );

    assert_eq!(dispatch.notification_len(), 1);
    let queued = dispatch.pop_notification().expect("queued notification");
    assert_eq!(queued["method"], "direct.incoming");
    assert!(dispatch.pop_notification().is_none());
}

#[test]
fn ws_json_write_marshals_then_writes_text_frame_like_go_helper() {
    let mut conn = RecordingJsonConnection::default();

    listener_wsclient::ws_json_write(
        &mut conn,
        &json!({
            "jsonrpc": "2.0",
            "id": "req-1",
            "method": "direct.send",
            "params": {"body": {"text": "hello"}},
        }),
    )
    .expect("write frame");

    assert_eq!(
        conn.writes,
        vec![(
            listener_wsclient::ListenerWsFrameKind::Text,
            br#"{"id":"req-1","jsonrpc":"2.0","method":"direct.send","params":{"body":{"text":"hello"}}}"#
                .to_vec(),
        )]
    );
}

#[test]
fn ws_json_write_returns_marshal_error_before_writing_like_go_helper() {
    let mut conn = RecordingJsonConnection::default();

    let err =
        listener_wsclient::ws_json_write(&mut conn, &FailingSerialize).expect_err("marshal error");

    assert!(
        err.to_string().contains("forced serialize error"),
        "unexpected error: {err}"
    );
    assert!(conn.writes.is_empty());
}

#[test]
fn ws_json_write_propagates_write_error_after_successful_marshal() {
    let mut conn = RecordingJsonConnection {
        write_error: Some("websocket write failed".to_string()),
        ..RecordingJsonConnection::default()
    };

    let err =
        listener_wsclient::ws_json_write(&mut conn, &json!({"ok": true})).expect_err("write error");

    assert_eq!(err.to_string(), "websocket write failed");
    assert_eq!(
        conn.writes,
        vec![(
            listener_wsclient::ListenerWsFrameKind::Text,
            br#"{"ok":true}"#.to_vec(),
        )]
    );
}

#[test]
fn ws_json_read_reads_frame_then_unmarshals_like_go_helper() {
    let mut conn = RecordingJsonConnection {
        reads: vec![(
            listener_wsclient::ListenerWsFrameKind::Text,
            br#"{"result":{"accepted":true}}"#.to_vec(),
        )],
        ..RecordingJsonConnection::default()
    };

    let value: serde_json::Value = listener_wsclient::ws_json_read(&mut conn).expect("read json");

    assert_eq!(value["result"]["accepted"], true);
    assert_eq!(conn.read_count, 1);
}

#[test]
fn ws_json_read_propagates_read_error_before_unmarshal() {
    let mut conn = RecordingJsonConnection {
        read_error: Some("websocket read failed".to_string()),
        reads: vec![(
            listener_wsclient::ListenerWsFrameKind::Text,
            br#"{"ignored":true}"#.to_vec(),
        )],
        ..RecordingJsonConnection::default()
    };

    let err: anyhow::Error =
        listener_wsclient::ws_json_read::<_, serde_json::Value>(&mut conn).expect_err("read error");

    assert_eq!(err.to_string(), "websocket read failed");
    assert_eq!(conn.read_count, 1);
}

#[test]
fn ws_json_read_returns_unmarshal_error_after_read_like_go_helper() {
    let mut conn = RecordingJsonConnection {
        reads: vec![(
            listener_wsclient::ListenerWsFrameKind::Binary,
            b"{not-json".to_vec(),
        )],
        ..RecordingJsonConnection::default()
    };

    let err: anyhow::Error = listener_wsclient::ws_json_read::<_, serde_json::Value>(&mut conn)
        .expect_err("json decode error");

    assert!(
        err.to_string().contains("expected ident")
            || err.to_string().contains("key must be a string"),
        "unexpected error: {err}"
    );
    assert_eq!(conn.read_count, 1);
}

#[test]
fn format_dial_error_message_matches_go_response_body_boundary() {
    assert_eq!(
        listener_wsclient::format_dial_error_message(None, Some(b"body")),
        None
    );
    assert_eq!(
        listener_wsclient::format_dial_error_message(Some("dial failed"), None),
        Some("dial failed".to_string())
    );
    assert_eq!(
        listener_wsclient::format_dial_error_message(Some("dial failed"), Some(b"")),
        Some("dial failed".to_string())
    );
    assert_eq!(
        listener_wsclient::format_dial_error_message(
            Some("dial failed"),
            Some(b"  unauthorized token  \n"),
        ),
        Some("dial failed: unauthorized token".to_string())
    );

    let mut body = vec![b'a'; listener_wsclient::DIAL_ERROR_BODY_LIMIT + 4];
    body[listener_wsclient::DIAL_ERROR_BODY_LIMIT - 1] = b'z';
    body[listener_wsclient::DIAL_ERROR_BODY_LIMIT] = b'b';
    assert_eq!(
        listener_wsclient::format_dial_error_message(Some("dial failed"), Some(&body)),
        Some(format!(
            "dial failed: {}z",
            "a".repeat(listener_wsclient::DIAL_ERROR_BODY_LIMIT - 1)
        ))
    );
}

#[test]
fn host_for_url_matches_go_net_url_host_boundary() {
    assert_eq!(
        listener_wsclient::host_for_url("http://example.com:8080/path"),
        "example.com:8080"
    );
    assert_eq!(
        listener_wsclient::host_for_url("https://user:pass@example.com/path"),
        "example.com"
    );
    assert_eq!(
        listener_wsclient::host_for_url("http://[::1]:8765/notify"),
        "[::1]:8765"
    );
    assert_eq!(
        listener_wsclient::host_for_url("example.com/path"),
        "",
        "Go url.Parse treats this as a path-only URL"
    );
    assert_eq!(
        listener_wsclient::host_for_url("http://[::1"),
        "http://[::1",
        "Go helper falls back to raw input on parse errors"
    );
    assert_eq!(
        listener_wsclient::host_for_url(" http://example.com"),
        " http://example.com",
        "Go helper does not trim before url.Parse"
    );
}

#[test]
fn ws_transport_ping_waits_for_peer_pong_before_succeeding() {
    let peer = DelayedPongWebsocketPeer::spawn(Duration::from_millis(300));
    let mut transport =
        WsTransport::connect(&peer.websocket_url, "session-token", "").expect("connect peer");
    let started = Instant::now();
    let result = transport.ping().map_err(|err| err.to_string());
    let elapsed = started.elapsed();
    peer.join();

    assert_eq!(result, Ok(()));
    assert!(
        elapsed >= Duration::from_millis(300),
        "WsTransport::ping returned after {elapsed:?}, before the peer sent its delayed pong; Go Conn.Ping waits for pong or context timeout"
    );
}

fn dial_action(token: &str) -> listener_wsclient::ListenerWsConnectAction {
    listener_wsclient::ListenerWsConnectAction::DialBearer {
        token: token.to_string(),
        authorization: format!("Bearer {token}"),
    }
}

fn map<const N: usize>(
    entries: [(&str, serde_json::Value); N],
) -> serde_json::Map<String, serde_json::Value> {
    serde_json::Map::from_iter(
        entries
            .into_iter()
            .map(|(key, value)| (key.to_string(), value)),
    )
}

fn test_resolved() -> Resolved {
    Resolved {
        paths: Paths {
            workspace_home_dir: "/tmp/awiki-workspace".to_string(),
            root_dir: "/tmp/awiki-workspace".to_string(),
            config_dir: "/tmp/awiki-workspace".to_string(),
            data_dir: "/tmp/awiki-workspace/data".to_string(),
            state_dir: "/tmp/awiki-workspace/runtime".to_string(),
            cache_dir: "/tmp/awiki-workspace/cache".to_string(),
            logs_dir: "/tmp/awiki-workspace/logs".to_string(),
            config_file: "/tmp/awiki-workspace/config.yaml".to_string(),
            identity_dir: "/tmp/awiki-workspace/identities".to_string(),
            database_file: "/tmp/awiki-workspace/awiki-cli.db".to_string(),
            legacy_credentials_dir: String::new(),
            legacy_data_dir: String::new(),
        },
        config_schema_version: 1,
        active_identity: String::new(),
        runtime_mode: "websocket".to_string(),
        runtime_socket_path: String::new(),
        runtime_listener_enabled: true,
        runtime_listener_auto_install: true,
        runtime_listener_auto_start: true,
        host_notify_enabled: true,
        host_notify_sink: "log".to_string(),
        host_notify_file_path: String::new(),
        host_notify_openclaw_hook_url: String::new(),
        host_notify_openclaw_agent_id: String::new(),
        host_notify_openclaw_hook_name: String::new(),
        host_notify_hermes_notify_url: String::new(),
        host_notify_hermes_deliver: String::new(),
        output_format: "json".to_string(),
        no_color: false,
        service_base_url: "https://awiki.ai".to_string(),
        did_domain: "awiki.ai".to_string(),
        anp_service_endpoint: "https://awiki.ai/anp-im/rpc".to_string(),
        anp_service_did: "did:wba:awiki.ai".to_string(),
        mail_service_url: "https://awiki.ai".to_string(),
        ca_bundle: String::new(),
        update_disable_strict_version: false,
        update_metadata_cache_ttl_seconds: 0,
        config_exists: false,
        config_error: String::new(),
        env_hits: Vec::new(),
        sources: std::collections::BTreeMap::new(),
    }
}

#[derive(Default)]
struct RecordingJsonConnection {
    writes: Vec<(listener_wsclient::ListenerWsFrameKind, Vec<u8>)>,
    reads: Vec<(listener_wsclient::ListenerWsFrameKind, Vec<u8>)>,
    write_error: Option<String>,
    read_error: Option<String>,
    read_count: usize,
}

impl listener_wsclient::ListenerWsJsonConnection for RecordingJsonConnection {
    fn write_frame(
        &mut self,
        kind: listener_wsclient::ListenerWsFrameKind,
        raw: Vec<u8>,
    ) -> anyhow::Result<()> {
        self.writes.push((kind, raw));
        if let Some(error) = &self.write_error {
            anyhow::bail!(error.clone());
        }
        Ok(())
    }

    fn read_frame(&mut self) -> anyhow::Result<(listener_wsclient::ListenerWsFrameKind, Vec<u8>)> {
        self.read_count += 1;
        if let Some(error) = &self.read_error {
            anyhow::bail!(error.clone());
        }
        self.reads
            .pop()
            .ok_or_else(|| anyhow::anyhow!("no frame queued"))
    }
}

struct FailingSerialize;

impl Serialize for FailingSerialize {
    fn serialize<S>(&self, _serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        Err(serde::ser::Error::custom("forced serialize error"))
    }
}

struct DelayedPongWebsocketPeer {
    websocket_url: String,
    join: thread::JoinHandle<()>,
}

impl DelayedPongWebsocketPeer {
    fn spawn(pong_delay: Duration) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind websocket peer");
        let addr = listener.local_addr().expect("peer address");
        let join = thread::spawn(move || run_delayed_pong_peer(listener, pong_delay));

        Self {
            websocket_url: format!("ws://{addr}/im/ws"),
            join,
        }
    }

    fn join(self) {
        self.join.join().expect("join websocket peer");
    }
}

fn run_delayed_pong_peer(listener: TcpListener, pong_delay: Duration) {
    let (mut stream, _) = listener.accept().expect("accept websocket client");
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .expect("set peer read timeout");
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .expect("set peer write timeout");

    let request = read_http_request(&mut stream);
    let websocket_key = http_header_value(&request, "Sec-WebSocket-Key")
        .expect("websocket handshake should include Sec-WebSocket-Key");
    let response = format!(
        "HTTP/1.1 101 Switching Protocols\r\n\
         Upgrade: websocket\r\n\
         Connection: Upgrade\r\n\
         Sec-WebSocket-Accept: {}\r\n\
         \r\n",
        websocket_accept(&websocket_key)
    );
    stream
        .write_all(response.as_bytes())
        .expect("write websocket handshake response");
    stream.flush().expect("flush websocket handshake response");

    let (opcode, payload) = read_ws_frame(&mut stream);
    assert_eq!(opcode, 0x9, "client should send a websocket ping frame");
    assert_eq!(
        payload, b"1",
        "Go coder/websocket Conn.Ping starts with ping payload \"1\""
    );
    thread::sleep(pong_delay);
    let _ = write_ws_frame(&mut stream, 0xA, &payload);
}

fn read_http_request(stream: &mut TcpStream) -> String {
    let mut raw = Vec::new();
    let mut byte = [0_u8; 1];
    while raw.len() < 16 * 1024 {
        let read = stream.read(&mut byte).expect("read HTTP request");
        assert!(read != 0, "HTTP request ended before header terminator");
        raw.push(byte[0]);
        if raw.ends_with(b"\r\n\r\n") {
            return String::from_utf8(raw).expect("HTTP request is UTF-8");
        }
    }
    panic!("HTTP request exceeded header limit");
}

fn http_header_value(request: &str, name: &str) -> Option<String> {
    let expected = name.to_ascii_lowercase();
    request.lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        (key.trim().eq_ignore_ascii_case(&expected)).then(|| value.trim().to_string())
    })
}

fn read_ws_frame(stream: &mut TcpStream) -> (u8, Vec<u8>) {
    let mut head = [0_u8; 2];
    stream.read_exact(&mut head).expect("read websocket frame");
    let opcode = head[0] & 0x0F;
    let masked = head[1] & 0x80 != 0;
    let mut len = u64::from(head[1] & 0x7F);
    if len == 126 {
        let mut bytes = [0_u8; 2];
        stream
            .read_exact(&mut bytes)
            .expect("read websocket frame length");
        len = u64::from(u16::from_be_bytes(bytes));
    } else if len == 127 {
        let mut bytes = [0_u8; 8];
        stream
            .read_exact(&mut bytes)
            .expect("read websocket frame length");
        len = u64::from_be_bytes(bytes);
    }

    let mut mask = [0_u8; 4];
    if masked {
        stream.read_exact(&mut mask).expect("read websocket mask");
    }
    let mut payload = vec![0_u8; len as usize];
    stream
        .read_exact(&mut payload)
        .expect("read websocket payload");
    if masked {
        for (idx, byte) in payload.iter_mut().enumerate() {
            *byte ^= mask[idx % 4];
        }
    }
    (opcode, payload)
}

fn write_ws_frame(stream: &mut TcpStream, opcode: u8, payload: &[u8]) -> std::io::Result<()> {
    let mut frame = Vec::with_capacity(10 + payload.len());
    frame.push(0x80 | (opcode & 0x0F));
    if payload.len() < 126 {
        frame.push(payload.len() as u8);
    } else if payload.len() <= u16::MAX as usize {
        frame.push(126);
        frame.extend_from_slice(&(payload.len() as u16).to_be_bytes());
    } else {
        frame.push(127);
        frame.extend_from_slice(&(payload.len() as u64).to_be_bytes());
    }
    frame.extend_from_slice(payload);
    stream.write_all(&frame)?;
    stream.flush()
}

fn websocket_accept(key: &str) -> String {
    const WS_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
    let mut raw = Vec::with_capacity(key.len() + WS_GUID.len());
    raw.extend_from_slice(key.as_bytes());
    raw.extend_from_slice(WS_GUID.as_bytes());
    BASE64_STANDARD.encode(sha1_digest(&raw))
}

fn sha1_digest(input: &[u8]) -> [u8; 20] {
    let mut h0: u32 = 0x67452301;
    let mut h1: u32 = 0xEFCDAB89;
    let mut h2: u32 = 0x98BADCFE;
    let mut h3: u32 = 0x10325476;
    let mut h4: u32 = 0xC3D2E1F0;

    let bit_len = (input.len() as u64) * 8;
    let mut data = input.to_vec();
    data.push(0x80);
    while data.len() % 64 != 56 {
        data.push(0);
    }
    data.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in data.chunks_exact(64) {
        let mut words = [0_u32; 80];
        for (idx, word) in words.iter_mut().take(16).enumerate() {
            let offset = idx * 4;
            *word = u32::from_be_bytes([
                chunk[offset],
                chunk[offset + 1],
                chunk[offset + 2],
                chunk[offset + 3],
            ]);
        }
        for idx in 16..80 {
            words[idx] = (words[idx - 3] ^ words[idx - 8] ^ words[idx - 14] ^ words[idx - 16])
                .rotate_left(1);
        }

        let mut a = h0;
        let mut b = h1;
        let mut c = h2;
        let mut d = h3;
        let mut e = h4;

        for (idx, word) in words.iter().enumerate() {
            let (f, k) = match idx {
                0..=19 => ((b & c) | ((!b) & d), 0x5A827999),
                20..=39 => (b ^ c ^ d, 0x6ED9EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDC),
                _ => (b ^ c ^ d, 0xCA62C1D6),
            };
            let temp = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(*word);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = temp;
        }

        h0 = h0.wrapping_add(a);
        h1 = h1.wrapping_add(b);
        h2 = h2.wrapping_add(c);
        h3 = h3.wrapping_add(d);
        h4 = h4.wrapping_add(e);
    }

    let mut out = [0_u8; 20];
    out[..4].copy_from_slice(&h0.to_be_bytes());
    out[4..8].copy_from_slice(&h1.to_be_bytes());
    out[8..12].copy_from_slice(&h2.to_be_bytes());
    out[12..16].copy_from_slice(&h3.to_be_bytes());
    out[16..20].copy_from_slice(&h4.to_be_bytes());
    out
}
