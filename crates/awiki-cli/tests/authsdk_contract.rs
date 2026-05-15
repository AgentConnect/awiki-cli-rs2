use anp::authentication::{
    create_did_wba_document, extract_signature_metadata, DidDocumentOptions,
};
use awiki_cli::anpsdk;
use awiki_cli::authsdk::{
    auth_json_headers, auth_scope, build_json_rpc_payload, decode_json_rpc_response,
    decode_json_rpc_response_optional, decode_plain_json_response, flatten_header_values,
    http_status_error, HttpError, PersistToken, RpcError, Session, CONTENT_TYPE_JSON, JSON_RPC_ID,
    JSON_RPC_VERSION,
};
use awiki_cli::transportcfg::new_http_client;
use serde_json::json;
use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

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
fn authsdk_headers_merge_json_base_and_cached_bearer_like_go() {
    let fixture = AuthFixture::new();
    let mut session = Session::new(
        &fixture.did_path,
        &fixture.key_path,
        "alice",
        fixture.did.as_str(),
        "",
        None,
    );

    session.set_bearer("https://api.example.com/orders", " cached-token ");

    let headers = session
        .headers("https://api.example.com/orders", "GET", &[], false)
        .expect("cached bearer headers");
    assert_eq!(
        headers.get("Content-Type").map(String::as_str),
        Some(CONTENT_TYPE_JSON)
    );
    assert_eq!(
        headers.get("Authorization").map(String::as_str),
        Some("Bearer cached-token")
    );
    assert!(!headers.contains_key("Signature-Input"));
    assert!(!headers.contains_key("Signature"));

    let fresh = session
        .headers(
            "https://api.example.com/orders",
            "POST",
            br#"{"item":"book"}"#,
            true,
        )
        .expect("force-new signed headers");
    assert_eq!(
        fresh.get("Content-Type").map(String::as_str),
        Some(CONTENT_TYPE_JSON)
    );
    assert!(!fresh.contains_key("Authorization"));
    assert!(fresh.contains_key("Signature-Input"));
    assert!(fresh.contains_key("Signature"));
    assert!(fresh.contains_key("Content-Digest"));
}

#[test]
fn authsdk_headers_generate_signed_json_request_headers_like_go() {
    let fixture = AuthFixture::new();
    let mut session = Session::new(
        &fixture.did_path,
        &fixture.key_path,
        "alice",
        fixture.did.as_str(),
        "",
        None,
    );

    let headers = session
        .headers(
            "https://api.example.com/orders",
            "POST",
            br#"{"item":"book"}"#,
            false,
        )
        .expect("signed request headers");
    assert_eq!(
        headers.get("Content-Type").map(String::as_str),
        Some(CONTENT_TYPE_JSON)
    );
    assert!(headers.contains_key("Signature-Input"));
    assert!(headers.contains_key("Signature"));
    assert!(headers.contains_key("Content-Digest"));
    let metadata = extract_signature_metadata(&headers).expect("signature metadata");
    assert_eq!(
        metadata.components,
        vec![
            "@method".to_string(),
            "@target-uri".to_string(),
            "@authority".to_string(),
            "content-digest".to_string(),
        ]
    );
    assert_eq!(metadata.keyid, format!("{}#key-1", fixture.did));

    let empty_body = session
        .headers("https://api.example.com/orders", "GET", &[], true)
        .expect("empty-body signed request headers");
    assert!(empty_body.contains_key("Signature-Input"));
    assert!(empty_body.contains_key("Signature"));
    assert!(!empty_body.contains_key("Content-Digest"));
}

#[test]
fn authsdk_challenge_headers_use_response_challenge_like_go() {
    let fixture = AuthFixture::new();
    let mut session = Session::new(
        &fixture.did_path,
        &fixture.key_path,
        "alice",
        fixture.did.as_str(),
        "",
        None,
    );
    let response_headers = headers([
        (
            "WWW-Authenticate",
            "DIDWba realm=\"api.example.com\", error=\"invalid_nonce\", nonce=\"server-nonce-123\"",
        ),
        (
            "Accept-Signature",
            "sig1=(\"@method\" \"@target-uri\" \"@authority\" \"content-digest\" \"content-type\");created;expires;nonce;keyid",
        ),
    ]);

    let headers = session
        .challenge_headers(
            "https://api.example.com/orders",
            &response_headers,
            "POST",
            br#"{"item":"book"}"#,
        )
        .expect("challenge headers");
    assert!(!headers.contains_key("Content-Type"));
    assert!(headers.contains_key("Signature-Input"));
    assert!(headers.contains_key("Signature"));
    assert!(headers.contains_key("Content-Digest"));

    let metadata = extract_signature_metadata(&headers).expect("signature metadata");
    assert_eq!(metadata.nonce.as_deref(), Some("server-nonce-123"));
    assert!(metadata
        .components
        .iter()
        .any(|value| value == "content-type"));
    assert!(metadata
        .components
        .iter()
        .any(|value| value == "content-digest"));
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

    let null_error_result: BTreeMap<String, String> =
        decode_json_rpc_response(br#"{ "result": { "access_token": "fresh" }, "error": null }"#)
            .expect("json-rpc error null should be ignored like Go");
    assert_eq!(
        null_error_result.get("access_token").map(String::as_str),
        Some("fresh")
    );
    decode_json_rpc_response_optional(br#"{ "result": null, "error": null }"#)
        .expect("out nil ignores null error");

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

#[test]
fn authsdk_do_json_rpc_posts_signed_json_rpc_and_decodes_result_like_go() {
    let fixture = AuthFixture::new();
    let server = TestServer::new(vec![TestResponse::ok(
        r#"{"result":{"ok":true,"value":"done"}}"#,
    )]);
    let mut session = fixture.session("");
    let client = new_http_client("").expect("client");

    let result: serde_json::Value = session
        .do_json_rpc(
            &client,
            &server.url("/rpc"),
            "POST",
            "mail.getInbox",
            json!({"folder": "inbox"}),
        )
        .expect("json rpc");

    assert_eq!(result, json!({"ok": true, "value": "done"}));
    let request = server.requests();
    assert_eq!(request.len(), 1);
    assert!(request[0].starts_with("POST /rpc HTTP/1.1\r\n"));
    assert_contains(&request[0], "Content-Type: application/json\r\n");
    assert_contains(&request[0], "Signature-Input:");
    assert_contains(&request[0], "Signature:");
    let body = request_body(&request[0]);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(body).expect("request body"),
        json!({
            "jsonrpc": "2.0",
            "id": "req-1",
            "method": "mail.getInbox",
            "params": {"folder": "inbox"},
        })
    );
}

#[test]
fn authsdk_do_json_rpc_maps_rpc_error_like_go() {
    let fixture = AuthFixture::new();
    let server = TestServer::new(vec![TestResponse::ok(
        r#"{"error":{"code":-32602,"message":"bad params","data":{"field":"id"}}}"#,
    )]);
    let mut session = fixture.session("");
    let client = new_http_client("").expect("client");

    let err = session
        .do_json_rpc::<serde_json::Value, _>(
            &client,
            &server.url("/rpc"),
            "POST",
            "mail.getInbox",
            json!({}),
        )
        .expect_err("rpc error");

    let rpc = err.downcast_ref::<RpcError>().expect("rpc error type");
    assert_eq!(rpc.code, -32602);
    assert_eq!(rpc.message, "bad params");
    assert_eq!(rpc.data, Some(json!({"field": "id"})));
}

#[test]
fn authsdk_do_json_maps_http_error_and_trims_body_like_go() {
    let fixture = AuthFixture::new();
    let server = TestServer::new(vec![
        TestResponse::status(401, " stale token "),
        TestResponse::status(403, " forbidden \n"),
    ]);
    let mut session = fixture.session("");
    session.set_bearer(&server.url("/rest"), "stale");
    let client = new_http_client("").expect("client");

    let err = session
        .do_json::<serde_json::Value, _>(&client, "POST", &server.url("/rest"), json!({}))
        .expect_err("http error");

    let http = err.downcast_ref::<HttpError>().expect("http error type");
    assert_eq!(http.status_code, 403);
    assert_eq!(http.message, "forbidden");
    assert_eq!(http.to_string(), "http error 403: forbidden");
    let requests = server.requests();
    assert_eq!(requests.len(), 2);
    assert_contains(&requests[0], "Authorization: Bearer stale\r\n");
    assert!(!requests[1].contains("Authorization: Bearer stale\r\n"));
    assert_contains(&requests[1], "Signature-Input:");
    assert_eq!(session.current_jwt(), "");
}

#[test]
fn authsdk_do_request_retries_401_with_challenge_headers_like_go() {
    let fixture = AuthFixture::new();
    let server = TestServer::new(vec![
        TestResponse::status(401, "challenge")
            .header("WWW-Authenticate", r#"DIDWba nonce="server-nonce-123""#)
            .header(
                "Accept-Signature",
                "sig1=(\"@method\" \"@target-uri\" \"@authority\" \"content-digest\" \"content-type\");created;expires;nonce;keyid",
            ),
        TestResponse::ok(r#"{"result":{"ok":true}}"#),
    ]);
    let mut session = fixture.session("");
    let client = new_http_client("").expect("client");

    let result: serde_json::Value = session
        .do_json_rpc(&client, &server.url("/rpc"), "POST", "get_me", json!({}))
        .expect("retry response");

    assert_eq!(result, json!({"ok": true}));
    let requests = server.requests();
    assert_eq!(requests.len(), 2);
    assert_contains(&requests[0], "Signature-Input:");
    assert_contains(&requests[1], "Signature-Input:");
    let headers = parse_request_headers(&requests[1]);
    let metadata = extract_signature_metadata(&headers).expect("signature metadata");
    assert_eq!(metadata.nonce.as_deref(), Some("server-nonce-123"));
    assert!(metadata
        .components
        .iter()
        .any(|component| component == "content-type"));
}

#[test]
fn authsdk_ensure_jwt_executes_get_me_and_persists_access_token_like_go() {
    let fixture = AuthFixture::new();
    let server = TestServer::new(vec![TestResponse::ok(
        r#"{"result":{"access_token":" fresh-token "}}"#,
    )]);
    let persisted = Arc::new(Mutex::new(Vec::<String>::new()));
    let capture = Arc::clone(&persisted);
    let persist_token: PersistToken = Box::new(move |token| {
        capture.lock().unwrap().push(token.to_string());
        Ok(())
    });
    let mut session = fixture.session_with_persist("", Some(persist_token));
    let client = new_http_client("").expect("client");

    let token = session
        .ensure_jwt(&client, &server.url("/user-service/did-auth/rpc"))
        .expect("ensure jwt");

    assert_eq!(token, "fresh-token");
    assert_eq!(session.current_jwt(), "fresh-token");
    assert_eq!(&*persisted.lock().unwrap(), &["fresh-token"]);
    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(request_body(&requests[0]))
            .expect("request body"),
        json!({
            "jsonrpc": "2.0",
            "id": "req-1",
            "method": "get_me",
            "params": {},
        })
    );
}

#[test]
fn authsdk_ensure_jwt_falls_back_to_captured_header_token_like_go() {
    let fixture = AuthFixture::new();
    let server = TestServer::new(vec![TestResponse::ok(r#"{"result":{}}"#).header(
        "Authentication-Info",
        r#"access_token="header-token", token_type="Bearer", expires_in=3600"#,
    )]);
    let persisted = Arc::new(Mutex::new(Vec::<String>::new()));
    let capture = Arc::clone(&persisted);
    let persist_token: PersistToken = Box::new(move |token| {
        capture.lock().unwrap().push(token.to_string());
        Ok(())
    });
    let mut session = fixture.session_with_persist("", Some(persist_token));
    let client = new_http_client("").expect("client");

    let token = session
        .ensure_jwt(&client, &server.url("/user-service/did-auth/rpc"))
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

struct AuthFixture {
    dir: PathBuf,
    did_path: PathBuf,
    key_path: PathBuf,
    did: String,
}

impl AuthFixture {
    fn new() -> Self {
        let dir = unique_temp_dir("authsdk-contract");
        fs::create_dir_all(&dir).expect("create auth fixture dir");
        let bundle = create_did_wba_document("example.com", DidDocumentOptions::default())
            .expect("create DID document");
        let did_path = dir.join("did.json");
        let key_path = dir.join("key.pem");
        fs::write(
            &did_path,
            serde_json::to_vec(&bundle.did_document).expect("serialize DID document"),
        )
        .expect("write DID document");
        fs::write(
            &key_path,
            bundle.private_key_pem("key-1").expect("key-1 private PEM"),
        )
        .expect("write key");
        let did = bundle.did().expect("DID id").to_string();
        Self {
            dir,
            did_path,
            key_path,
            did,
        }
    }

    fn session(&self, jwt_token: &str) -> Session {
        self.session_with_persist(jwt_token, None)
    }

    fn session_with_persist(
        &self,
        jwt_token: &str,
        persist_token: Option<PersistToken>,
    ) -> Session {
        Session::new(
            &self.did_path,
            &self.key_path,
            "alice",
            self.did.as_str(),
            jwt_token,
            persist_token,
        )
    }
}

impl Drop for AuthFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn unique_temp_dir(prefix: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time before epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("{prefix}-{}-{nanos}", std::process::id()))
}

#[derive(Clone)]
struct TestResponse {
    status: u16,
    body: String,
    headers: Vec<(String, String)>,
}

impl TestResponse {
    fn ok(body: &str) -> Self {
        Self::status(200, body)
    }

    fn status(status: u16, body: &str) -> Self {
        Self {
            status,
            body: body.to_string(),
            headers: Vec::new(),
        }
    }

    fn header(mut self, key: &str, value: &str) -> Self {
        self.headers.push((key.to_string(), value.to_string()));
        self
    }
}

struct TestServer {
    address: String,
    requests: Arc<Mutex<Vec<String>>>,
    join: Option<thread::JoinHandle<()>>,
}

impl TestServer {
    fn new(responses: Vec<TestResponse>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
        let address = format!("http://{}", listener.local_addr().expect("local addr"));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let server_requests = Arc::clone(&requests);
        let join = thread::spawn(move || {
            for response in responses {
                let Ok((stream, _)) = listener.accept() else {
                    break;
                };
                handle_connection(stream, &server_requests, response);
            }
        });
        Self {
            address,
            requests,
            join: Some(join),
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.address, path)
    }

    fn requests(&self) -> Vec<String> {
        self.requests.lock().expect("requests mutex").clone()
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

fn handle_connection(
    mut stream: TcpStream,
    requests: &Arc<Mutex<Vec<String>>>,
    response: TestResponse,
) {
    let raw = read_request(&mut stream);
    requests.lock().expect("requests mutex").push(raw);
    let reason = if response.status == 200 {
        "OK"
    } else {
        "ERROR"
    };
    let mut headers = format!(
        "HTTP/1.1 {} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n",
        response.status,
        response.body.len()
    );
    for (key, value) in response.headers {
        headers.push_str(&format!("{key}: {value}\r\n"));
    }
    let raw_response = format!("{headers}\r\n{}", response.body);
    stream
        .write_all(raw_response.as_bytes())
        .expect("write response");
}

fn read_request(stream: &mut TcpStream) -> String {
    let mut raw = Vec::new();
    let mut buffer = [0_u8; 4096];
    let header_end = loop {
        let read = stream.read(&mut buffer).expect("read request");
        if read == 0 {
            panic!("connection closed before headers");
        }
        raw.extend_from_slice(&buffer[..read]);
        if let Some(end) = raw.windows(4).position(|window| window == b"\r\n\r\n") {
            break end;
        }
    };
    let headers = String::from_utf8_lossy(&raw[..header_end]).to_string();
    let content_length = headers
        .lines()
        .find_map(|line| {
            let (key, value) = line.split_once(':')?;
            key.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())
                .flatten()
        })
        .unwrap_or(0);
    while raw.len() < header_end + 4 + content_length {
        let read = stream.read(&mut buffer).expect("read body");
        if read == 0 {
            break;
        }
        raw.extend_from_slice(&buffer[..read]);
    }
    String::from_utf8_lossy(&raw).to_string()
}

fn request_body(raw: &str) -> &str {
    raw.split_once("\r\n\r\n")
        .map(|(_, body)| body)
        .expect("request body")
}

fn parse_request_headers(raw: &str) -> BTreeMap<String, String> {
    raw.split("\r\n")
        .skip(1)
        .take_while(|line| !line.is_empty())
        .filter_map(|line| {
            let (key, value) = line.split_once(':')?;
            Some((key.to_string(), value.trim().to_string()))
        })
        .collect()
}

fn assert_contains(haystack: &str, needle: &str) {
    assert!(
        haystack.contains(needle),
        "expected {needle:?} in:\n{haystack}"
    );
}
