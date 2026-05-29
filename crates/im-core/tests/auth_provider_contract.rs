use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use im_core::prelude::*;
use serde_json::Value;

#[test]
fn file_session_provider_ensures_session_from_runtime_auth_state() {
    let fixture = AuthFixture::new();
    fixture.write_runtime("alice", "did:example:alice", Some("token-alice"), true);
    let client = fixture.client("alice");

    let session = client.auth().ensure_session(AuthScope::Messaging).unwrap();
    assert_eq!(session.subject.as_str(), "did:example:alice");
    assert_eq!(session.scope, AuthScope::Messaging);
    assert_eq!(session.expires_at.as_deref(), Some("2026-05-21T00:00:00Z"));
    assert!(!session.refreshed);

    let status = client.auth().status().unwrap();
    assert!(status.has_session);
    assert!(!status.needs_refresh);
    assert!(status.warnings.is_empty());
}

#[test]
fn file_session_provider_reports_missing_token_without_faking_session() {
    let fixture = AuthFixture::new();
    fixture.write_runtime("alice", "did:example:alice", None, true);
    let client = fixture.client("alice");

    assert!(matches!(
        client.auth().ensure_session(AuthScope::Messaging),
        Err(ImError::AuthRequired)
    ));
    let status = client.auth().status().unwrap();
    assert!(!status.has_session);
    assert!(status.needs_refresh);
    assert!(status
        .warnings
        .iter()
        .any(|warning| warning.contains("JWT")));
}

#[tokio::test]
async fn file_session_provider_refreshes_jwt_with_signed_get_me_and_persists_token() {
    let server = TestServer::new(
        r#"{"jsonrpc":"2.0","result":{"access_token":"fresh-token"},"id":"req-1"}"#,
    );
    let fixture = AuthFixture::new().with_service_base_url(server.base_url());
    fixture.write_runtime("alice", "did:example:alice", Some("stale-token"), true);
    let client = fixture.client_async("alice").await;

    let update = client.auth().refresh_session_async().await.unwrap();

    assert_eq!(update.subject.as_str(), "did:example:alice");
    assert_eq!(
        update.previous_expires_at.as_deref(),
        Some("2026-05-21T00:00:00Z")
    );
    assert_eq!(update.new_expires_at, None);
    assert!(update.refreshed);
    let auth: Value =
        serde_json::from_slice(&fs::read(fixture.auth_path("alice")).unwrap()).unwrap();
    assert_eq!(auth["jwt_token"], "fresh-token");
    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].starts_with("POST /user-service/did-auth/rpc HTTP/1.1"));
    assert!(
        !requests[0].contains("Authorization: Bearer stale-token\r\n"),
        "refresh must force a fresh DID auth signature:\n{}",
        requests[0]
    );
    assert!(contains_header(&requests[0], "Signature-Input"));
    assert!(contains_header(&requests[0], "Signature"));
    let body: Value = serde_json::from_str(request_body(&requests[0])).unwrap();
    assert_eq!(body["method"], "get_me");
    assert_eq!(body["params"], serde_json::json!({}));
}

#[tokio::test]
async fn file_session_provider_refreshes_jwt_from_response_authorization_header() {
    let server = TestServer::with_authorization_header(
        r#"{"jsonrpc":"2.0","result":{"handle":"alice"},"id":"req-1"}"#,
        "fresh-header-token",
    );
    let fixture = AuthFixture::new().with_service_base_url(server.base_url());
    fixture.write_runtime("alice", "did:example:alice", Some("stale-token"), true);
    let client = fixture.client_async("alice").await;

    let update = client.auth().refresh_session_async().await.unwrap();

    assert_eq!(update.subject.as_str(), "did:example:alice");
    assert!(update.refreshed);
    let auth: Value =
        serde_json::from_slice(&fs::read(fixture.auth_path("alice")).unwrap()).unwrap();
    assert_eq!(auth["jwt_token"], "fresh-header-token");
    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    assert!(
        !requests[0].contains("Authorization: Bearer stale-token\r\n"),
        "refresh must not reuse stale bearer token:\n{}",
        requests[0]
    );
}

#[tokio::test]
async fn async_file_session_provider_refreshes_jwt_with_async_transport() {
    let server = TestServer::new(
        r#"{"jsonrpc":"2.0","result":{"access_token":"fresh-token"},"id":"req-1"}"#,
    );
    let fixture = AuthFixture::new().with_service_base_url(server.base_url());
    fixture.write_runtime("alice", "did:example:alice", Some("stale-token"), true);
    let client = fixture.client_async("alice").await;

    let update = client.auth().refresh_session_async().await.unwrap();

    assert_eq!(update.subject.as_str(), "did:example:alice");
    assert_eq!(
        update.previous_expires_at.as_deref(),
        Some("2026-05-21T00:00:00Z")
    );
    assert!(update.refreshed);
    let auth: Value =
        serde_json::from_slice(&fs::read(fixture.auth_path("alice")).unwrap()).unwrap();
    assert_eq!(auth["jwt_token"], "fresh-token");
    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].starts_with("POST /user-service/did-auth/rpc HTTP/1.1"));
    let body: Value = serde_json::from_str(request_body(&requests[0])).unwrap();
    assert_eq!(body["method"], "get_me");
    assert_eq!(body["params"], serde_json::json!({}));
}

#[test]
fn file_session_provider_respects_messaging_readiness_for_message_scopes() {
    let fixture = AuthFixture::new();
    fixture.write_runtime("alice", "did:example:alice", Some("token-alice"), false);
    let client = fixture.client("alice");

    let profile = client
        .auth()
        .ensure_session(AuthScope::UserProfile)
        .unwrap();
    assert_eq!(profile.scope, AuthScope::UserProfile);

    assert!(matches!(
        client.auth().ensure_session(AuthScope::Messaging),
        Err(ImError::IdentityNotReady { .. })
    ));
}

struct AuthFixture {
    root: PathBuf,
    service_base_url: String,
}

impl AuthFixture {
    fn new() -> Self {
        let root = unique_temp_root();
        fs::create_dir_all(root.join("identities")).unwrap();
        fs::write(root.join("identities").join("default"), "alice\n").unwrap();
        Self {
            root,
            service_base_url: "https://example.test".to_string(),
        }
    }

    fn with_service_base_url(mut self, service_base_url: String) -> Self {
        self.service_base_url = service_base_url;
        self
    }

    fn write_runtime(
        &self,
        alias: &str,
        did: &str,
        token: Option<&str>,
        ready_for_messaging: bool,
    ) {
        let identities = self.root.join("identities");
        fs::write(
            identities.join("registry.json"),
            format!(
                r#"{{
                  "default_identity": "{alias}",
                  "identities": [{{
                    "id": "{alias}-id",
                    "did": "{did}",
                    "local_alias": "{alias}",
                    "ready_for_auth": true,
                    "ready_for_messaging": {ready_for_messaging},
                    "missing": []
                  }}]
                }}"#
            ),
        )
        .unwrap();
        let identity_dir = identities.join(alias);
        fs::create_dir_all(&identity_dir).unwrap();
        let bundle = generated_identity_bundle(alias);
        let did_document = did_document_for_runtime(did, &bundle.did_document);
        fs::write(
            identity_dir.join("did.json"),
            serde_json::to_vec_pretty(&did_document).unwrap(),
        )
        .unwrap();
        fs::write(
            identity_dir.join("private.key"),
            bundle.private_key_pem("key-1").unwrap(),
        )
        .unwrap();
        let token_field = token
            .map(|token| format!(r#""jwt_token":"{token}","#))
            .unwrap_or_default();
        fs::write(
            identity_dir.join("auth.json"),
            format!(r#"{{{token_field}"expires_at":"2026-05-21T00:00:00Z"}}"#),
        )
        .unwrap();
    }

    fn auth_path(&self, alias: &str) -> PathBuf {
        self.root.join("identities").join(alias).join("auth.json")
    }

    fn client(&self, alias: &str) -> ImClient {
        self.core()
            .client(IdentitySelector::LocalAlias(alias.to_string()))
            .unwrap()
    }

    async fn client_async(&self, alias: &str) -> ImClient {
        self.core_async()
            .await
            .client_async(IdentitySelector::LocalAlias(alias.to_string()))
            .await
            .unwrap()
    }

    fn core(&self) -> ImCore {
        ImCore::new(self.config(), self.paths()).unwrap()
    }

    async fn core_async(&self) -> ImCore {
        ImCore::open(self.config(), self.paths()).await.unwrap()
    }

    fn config(&self) -> ImCoreConfig {
        ImCoreConfig {
            service_base_url: ServiceEndpoint::parse(&self.service_base_url).unwrap(),
            did_domain: "awiki.test".to_string(),
            user_service_endpoint: None,
            message_service_endpoint: None,
            mail_service_endpoint: None,
            anp_service_endpoint: None,
            anp_service_did: None,
            ca_bundle: None,
            transport_policy: MessageTransportPolicy::HttpOnly,
        }
    }

    fn paths(&self) -> ImCorePaths {
        ImCorePaths {
            identities: IdentityRegistryPaths {
                identity_root_dir: self.root.join("identities"),
                registry_path: self.root.join("identities").join("registry.json"),
                default_identity_path: Some(self.root.join("identities").join("default")),
            },
            local_state: LocalStatePaths {
                sqlite_path: self.root.join("state").join("im.sqlite"),
            },
            runtime: RuntimePaths {
                cache_dir: self.root.join("cache"),
                temp_dir: self.root.join("tmp"),
            },
        }
    }
}

fn generated_identity_bundle(alias: &str) -> anp::authentication::DidDocumentBundle {
    anp::authentication::create_did_wba_document(
        "awiki.test",
        anp::authentication::DidDocumentOptions {
            path_segments: vec!["user".to_string()],
            domain: Some("awiki.test".to_string()),
            challenge: Some(format!("auth-provider-{alias}")),
            ..anp::authentication::DidDocumentOptions::default()
        },
    )
    .unwrap()
}

fn did_document_for_runtime(did: &str, generated: &Value) -> Value {
    let mut document = generated.clone();
    if let Some(object) = document.as_object_mut() {
        object.insert("id".to_string(), Value::String(did.to_string()));
        for key in ["verificationMethod", "authentication", "assertionMethod"] {
            rewrite_did_references(object.get_mut(key), did);
        }
    }
    document
}

fn rewrite_did_references(value: Option<&mut Value>, did: &str) {
    match value {
        Some(Value::Array(items)) => {
            for item in items {
                rewrite_did_references(Some(item), did);
            }
        }
        Some(Value::Object(object)) => {
            if object.get("id").and_then(Value::as_str).is_some() {
                object.insert("id".to_string(), Value::String(format!("{did}#key-1")));
            }
            if object.get("controller").and_then(Value::as_str).is_some() {
                object.insert("controller".to_string(), Value::String(did.to_string()));
            }
        }
        Some(Value::String(value)) => {
            *value = format!("{did}#key-1");
        }
        _ => {}
    }
}

fn unique_temp_root() -> PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!(
        "im-core-auth-provider-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&path).unwrap();
    path
}

struct TestServer {
    address: String,
    requests: Arc<Mutex<Vec<String>>>,
    join: Option<thread::JoinHandle<()>>,
}

impl TestServer {
    fn new(response_body: &'static str) -> Self {
        Self::new_inner(response_body, None)
    }

    fn with_authorization_header(response_body: &'static str, token: &'static str) -> Self {
        Self::new_inner(response_body, Some(token))
    }

    fn new_inner(response_body: &'static str, authorization_token: Option<&'static str>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let server_requests = Arc::clone(&requests);
        let join = thread::spawn(move || {
            let Some(mut stream) = accept_with_timeout(&listener) else {
                return;
            };
            let request = read_http_request(&mut stream);
            server_requests.lock().unwrap().push(request);
            let authorization_header = authorization_token
                .map(|token| format!("Authorization: Bearer {token}\r\n"))
                .unwrap_or_default();
            let raw = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n{authorization_header}Content-Length: {}\r\nConnection: close\r\n\r\n{}",
                response_body.len(),
                response_body
            );
            stream.write_all(raw.as_bytes()).unwrap();
        });
        Self {
            address,
            requests,
            join: Some(join),
        }
    }

    fn base_url(&self) -> String {
        self.address.clone()
    }

    fn requests(&self) -> Vec<String> {
        self.requests.lock().unwrap().clone()
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

fn accept_with_timeout(listener: &TcpListener) -> Option<TcpStream> {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        match listener.accept() {
            Ok((stream, _)) => return Some(stream),
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                if std::time::Instant::now() >= deadline {
                    return None;
                }
                thread::sleep(Duration::from_millis(10));
            }
            Err(_) => return None,
        }
    }
}

fn read_http_request(stream: &mut TcpStream) -> String {
    let mut raw = Vec::new();
    let mut buf = [0_u8; 512];
    loop {
        let count = stream.read(&mut buf).unwrap();
        if count == 0 {
            break;
        }
        raw.extend_from_slice(&buf[..count]);
        if let Some(header_end) = find_header_end(&raw) {
            let headers = String::from_utf8_lossy(&raw[..header_end]);
            let content_length = headers
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("Content-Length")
                        .then(|| value.trim())
                })
                .and_then(|value| value.trim().parse::<usize>().ok())
                .unwrap_or_default();
            let expected = header_end + content_length;
            while raw.len() < expected {
                let count = stream.read(&mut buf).unwrap();
                if count == 0 {
                    break;
                }
                raw.extend_from_slice(&buf[..count]);
            }
            break;
        }
    }
    String::from_utf8_lossy(&raw).into_owned()
}

fn find_header_end(raw: &[u8]) -> Option<usize> {
    raw.windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|index| index + 4)
}

fn request_body(raw: &str) -> &str {
    raw.split("\r\n\r\n").nth(1).unwrap_or_default()
}

fn contains_header(raw: &str, name: &str) -> bool {
    raw.lines()
        .filter_map(|line| line.split_once(':').map(|(header, _)| header))
        .any(|header| header.eq_ignore_ascii_case(name))
}
