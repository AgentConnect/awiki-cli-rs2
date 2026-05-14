use awiki_cli::anpsdk;
use awiki_cli::authsdk::{
    auth_json_headers, auth_scope, build_json_rpc_payload, decode_json_rpc_response,
    decode_json_rpc_response_optional, decode_plain_json_response, flatten_header_values,
    http_status_error, HttpError, PersistToken, RpcError, Session, CONTENT_TYPE_JSON, JSON_RPC_ID,
    JSON_RPC_VERSION,
};
use serde_json::json;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

#[test]
fn authsdk_capture_token_persists_only_configured_scopes_like_go() {
    let persisted = Arc::new(Mutex::new(Vec::<String>::new()));
    let capture = Arc::clone(&persisted);
    let persist_token: PersistToken = Box::new(move |token| {
        capture.lock().unwrap().push(token.to_string());
        Ok(())
    });
    let mut session = Session::new(
        "",
        "",
        "alice",
        "did:wba:example.com:user:alice:e1",
        "",
        Some(persist_token),
    );
    session.remember_scope("https://home.example/rpc");

    let remote_token = session.capture_token(
        "https://remote.example/rpc",
        &headers([(
            "Authentication-Info",
            r#"access_token="remote-token", token_type="Bearer", expires_in=3600"#,
        )]),
    );
    assert_eq!(remote_token, "remote-token");
    assert_eq!(session.current_jwt(), "");
    assert!(persisted.lock().unwrap().is_empty());

    let local_token = session.capture_token(
        "https://home.example/rpc",
        &headers([(
            "Authentication-Info",
            r#"access_token="fresh-home-token", token_type="Bearer", expires_in=3600"#,
        )]),
    );
    assert_eq!(local_token, "fresh-home-token");
    assert_eq!(session.current_jwt(), "fresh-home-token");
    assert_eq!(&*persisted.lock().unwrap(), &["fresh-home-token"]);
}

#[test]
fn authsdk_capture_token_accepts_legacy_authorization_response_header_like_go() {
    let persisted = Arc::new(Mutex::new(Vec::<String>::new()));
    let capture = Arc::clone(&persisted);
    let persist_token: PersistToken = Box::new(move |token| {
        capture.lock().unwrap().push(token.to_string());
        Ok(())
    });
    let mut session = Session::new(
        "",
        "",
        "alice",
        "did:wba:example.com:user:alice:e1",
        "",
        Some(persist_token),
    );
    session.remember_scope("https://home.example/rpc");

    let token = session.capture_token(
        "https://home.example/rpc",
        &headers([("Authorization", "Bearer legacy-token")]),
    );
    assert_eq!(token, "legacy-token");
    assert_eq!(session.current_jwt(), "legacy-token");
    assert_eq!(&*persisted.lock().unwrap(), &["legacy-token"]);
}

#[test]
fn authsdk_session_scope_and_error_strings_match_go_boundaries() {
    assert_eq!(
        anpsdk::MODULE_PATH,
        "github.com/agent-network-protocol/anp/golang"
    );
    assert_eq!(anpsdk::MODULE_VERSION, "v0.8.7");
    assert_eq!(
        anpsdk::AUTH_MODE_HTTP_SIGNATURES,
        anpsdk::AuthMode::HttpSignatures
    );

    let mut session = Session::new(
        "",
        "",
        "alice",
        "did:wba:example.com:user:alice:e1",
        "stored-token",
        None,
    );
    assert_eq!(session.identity_name(), "alice");
    assert_eq!(session.did(), "did:wba:example.com:user:alice:e1");
    assert_eq!(auth_scope("https://home.example:8443/rpc"), "home.example");
    assert_eq!(auth_scope("not a url"), "not a url");
    assert_eq!(auth_scope("https:///missing-host"), "https:///missing-host");

    session.set_bearer("https://home.example/rpc", " refreshed ");
    assert_eq!(session.current_jwt(), "refreshed");
    session.clear_token("https://home.example/rpc");
    assert_eq!(session.current_jwt(), "");

    assert!(
        session.should_retry_after_401(&headers([("WWW-Authenticate", r#"DIDWba nonce="n-1""#,)]))
    );
    assert!(!session.should_retry_after_401(&headers([(
        "WWW-Authenticate",
        r#"DIDWba error="invalid_did""#,
    )])));

    assert_eq!(
        HttpError {
            status_code: 401,
            message: "unauthorized".to_string(),
        }
        .to_string(),
        "http error 401: unauthorized"
    );
    assert_eq!(
        RpcError {
            code: -32001,
            message: "denied".to_string(),
            data: Some(json!({"reason": "test"})),
        }
        .to_string(),
        "rpc error -32001: denied"
    );
}

#[test]
fn authsdk_json_rpc_wire_contract_matches_go_session_helpers() {
    assert_eq!(
        build_json_rpc_payload("mail.getInbox", json!({ "folder": "inbox" })),
        json!({
            "jsonrpc": JSON_RPC_VERSION,
            "id": JSON_RPC_ID,
            "method": "mail.getInbox",
            "params": { "folder": "inbox" },
        })
    );

    let decoded: BTreeMap<String, String> =
        decode_json_rpc_response(br#"{ "result": { "access_token": "fresh" } }"#)
            .expect("decode result");
    assert_eq!(
        decoded.get("access_token").map(String::as_str),
        Some("fresh")
    );

    let null_result: serde_json::Value =
        decode_json_rpc_response(br#"{ "result": null }"#).expect("decode null result");
    assert_eq!(null_result, serde_json::Value::Null);

    let missing_typed =
        decode_json_rpc_response::<BTreeMap<String, String>>(br#"{ "jsonrpc": "2.0" }"#)
            .expect_err("missing result cannot decode typed output");
    assert!(missing_typed.to_string().contains("invalid type: null"));

    decode_json_rpc_response_optional(br#"{ "jsonrpc": "2.0" }"#)
        .expect("out nil ignores missing result");

    let err = decode_json_rpc_response::<serde_json::Value>(
        br#"{ "result": { "ok": true }, "error": { "code": -32602, "message": "bad params", "data": { "field": "id" } } }"#,
    )
    .expect_err("rpc error");
    let rpc = err.downcast_ref::<RpcError>().expect("rpc error type");
    assert_eq!(rpc.code, -32602);
    assert_eq!(rpc.message, "bad params");
    assert_eq!(rpc.data, Some(json!({ "field": "id" })));

    assert_eq!(
        http_status_error(401, b" unauthorized \n"),
        Some(HttpError {
            status_code: 401,
            message: "unauthorized".to_string(),
        })
    );
    assert_eq!(http_status_error(399, b"ignored"), None);

    let plain: BTreeMap<String, String> =
        decode_plain_json_response(br#"{ "status": "ok" }"#).expect("plain json");
    assert_eq!(plain.get("status").map(String::as_str), Some("ok"));

    assert_eq!(
        flatten_header_values([
            ("Authentication-Info", vec!["first", "second"]),
            ("Empty", Vec::<&str>::new()),
            ("WWW-Authenticate", vec!["challenge"]),
        ]),
        BTreeMap::from([
            ("Authentication-Info".to_string(), "first".to_string()),
            ("WWW-Authenticate".to_string(), "challenge".to_string()),
        ])
    );

    assert_eq!(
        auth_json_headers(),
        BTreeMap::from([("Content-Type".to_string(), CONTENT_TYPE_JSON.to_string(),)])
    );
}

#[test]
fn authsdk_ensure_jwt_result_contract_matches_go_fallback_order() {
    let persisted = Arc::new(Mutex::new(Vec::<String>::new()));
    let capture = Arc::clone(&persisted);
    let persist_token: PersistToken = Box::new(move |token| {
        capture.lock().unwrap().push(token.to_string());
        Ok(())
    });
    let mut session = Session::new(
        "",
        "",
        "alice",
        "did:wba:example.com:user:alice:e1",
        "stored",
        Some(persist_token),
    );

    let token = session
        .ensure_jwt_from_result(
            "https://home.example/user-service/did-auth/rpc",
            &json!({ "access_token": " fresh-token " }),
        )
        .expect("fresh token");
    assert_eq!(token, "fresh-token");
    assert_eq!(session.current_jwt(), "fresh-token");
    assert_eq!(&*persisted.lock().unwrap(), &["fresh-token"]);

    let fallback = session
        .ensure_jwt_from_result("https://home.example/user-service/did-auth/rpc", &json!({}))
        .expect("stored token fallback");
    assert_eq!(fallback, "fresh-token");

    session.clear_token("https://home.example/user-service/did-auth/rpc");
    let err = session
        .ensure_jwt_from_result(
            "https://home.example/user-service/did-auth/rpc",
            &json!({ "access_token": "   " }),
        )
        .expect_err("missing token");
    assert_eq!(
        err.to_string(),
        "did-auth get_me succeeded but no access token was returned"
    );
}

#[test]
fn authsdk_ensure_jwt_can_fallback_to_header_token_captured_before_body_decode() {
    let persisted = Arc::new(Mutex::new(Vec::<String>::new()));
    let capture = Arc::clone(&persisted);
    let persist_token: PersistToken = Box::new(move |token| {
        capture.lock().unwrap().push(token.to_string());
        Ok(())
    });
    let mut session = Session::new(
        "",
        "",
        "alice",
        "did:wba:example.com:user:alice:e1",
        "",
        Some(persist_token),
    );
    let request_url = "https://home.example/user-service/did-auth/rpc";

    session.remember_scope(request_url);
    let captured = session.capture_token(
        request_url,
        &headers([(
            "Authentication-Info",
            r#"access_token="header-token", token_type="Bearer", expires_in=3600"#,
        )]),
    );
    assert_eq!(captured, "header-token");

    let token = session
        .ensure_jwt_from_result(request_url, &json!({}))
        .expect("header token fallback");
    assert_eq!(token, "header-token");
    assert_eq!(session.current_jwt(), "header-token");
    assert_eq!(&*persisted.lock().unwrap(), &["header-token"]);
}

fn headers<const N: usize>(pairs: [(&str, &str); N]) -> BTreeMap<String, String> {
    pairs
        .into_iter()
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect()
}
