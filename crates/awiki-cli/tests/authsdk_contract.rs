use awiki_cli::anpsdk;
use awiki_cli::authsdk::{auth_scope, HttpError, PersistToken, RpcError, Session};
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

fn headers<const N: usize>(pairs: [(&str, &str); N]) -> BTreeMap<String, String> {
    pairs
        .into_iter()
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect()
}
