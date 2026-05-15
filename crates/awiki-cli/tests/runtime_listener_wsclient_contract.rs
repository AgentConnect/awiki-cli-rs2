use awiki_cli::config::{self, Paths, Resolved};
use awiki_cli::runtime::listener_wsclient;
use serde_json::json;

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
