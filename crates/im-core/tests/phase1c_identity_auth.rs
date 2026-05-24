use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

use im_core::prelude::*;
use serde_json::{json, Value};

#[test]
fn identity_registry_lists_default_and_resolves_selectors() {
    let fixture = Fixture::new();
    let core = fixture.core();

    let identities = core.identities().list().unwrap();
    assert_eq!(identities.len(), 2);

    let default = core.identities().default_identity().unwrap().unwrap();
    assert_eq!(default.local_alias.as_deref(), Some("alice"));
    assert!(default.is_default);

    let alice = core
        .identities()
        .resolve(IdentitySelector::LocalAlias("alice".to_string()))
        .unwrap();
    assert_eq!(alice.did.as_str(), "did:example:alice");

    let bob = core
        .identities()
        .resolve(IdentitySelector::Did(
            Did::parse("did:example:bob").unwrap(),
        ))
        .unwrap();
    assert_eq!(bob.local_alias.as_deref(), Some("bob"));

    let by_handle = core
        .identities()
        .resolve(IdentitySelector::Handle(
            Handle::parse("bob.awiki.test", "").unwrap(),
        ))
        .unwrap();
    assert_eq!(by_handle.did.as_str(), "did:example:bob");
}

#[test]
fn plan_default_identity_change_returns_previous_and_next() {
    let fixture = Fixture::new();
    let core = fixture.core();

    let change = core
        .identities()
        .plan_default_identity_change(IdentitySelector::LocalAlias("bob".to_string()))
        .unwrap();

    assert_eq!(
        change.previous.unwrap().local_alias.as_deref(),
        Some("alice")
    );
    assert_eq!(change.next.local_alias.as_deref(), Some("bob"));
    assert!(change.requires_default_identity_write);
}

#[test]
fn register_handle_returns_identity_and_default_change() {
    let server = TestServer::spawn(vec![ExpectedHttp::rpc_result(json!({
        "did": "did:wba:awiki.test:carol:e1_registered",
        "user_id": "user-carol",
        "handle": "carol",
        "full_handle": "carol.awiki.test",
        "access_token": "jwt-carol"
    }))]);
    let fixture = Fixture::new();
    let core = fixture.core_with_base_url(server.base_url());

    let result = core
        .identities()
        .register_handle(RegisterHandleRequest {
            local_alias: Some("carol".to_string()),
            requested_handle: Handle::parse("carol.awiki.test", "").unwrap(),
            verification: VerificationInput::AlreadyVerified,
            invite_code: None,
            profile: InitialProfile {
                display_name: Some("Carol".to_string()),
                avatar_url: None,
            },
            make_default: true,
        })
        .unwrap();

    assert_eq!(result.state, HandleRegistrationState::Registered);
    assert_eq!(result.method, RegistrationMethod::AlreadyVerified);
    assert_eq!(result.handle.as_str(), "carol.awiki.test");
    let identity = result.identity.unwrap();
    assert_eq!(identity.local_alias.as_deref(), Some("carol"));
    assert_eq!(identity.handle.unwrap().as_str(), "carol.awiki.test");
    assert_eq!(identity.display_name.as_deref(), Some("Carol"));
    assert!(identity.readiness.ready_for_auth);
    assert!(result.default_identity_change.is_some());

    let requests = server.join();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, "POST");
    assert_eq!(requests[0].path, "/user-service/did-auth/rpc");
    let body = requests[0].json_body();
    assert_eq!(body["method"], "register");
    assert_eq!(body["params"]["handle"], "carol");
    assert!(body["params"]["did_document"].is_object());
}

#[test]
fn register_phone_without_otp_returns_pending_otp_state() {
    let server = TestServer::spawn(vec![ExpectedHttp::rpc_result(json!({ "sent": true }))]);
    let fixture = Fixture::new();
    let core = fixture.core_with_base_url(server.base_url());

    let result = core
        .identities()
        .register_handle(RegisterHandleRequest {
            local_alias: Some("carol".to_string()),
            requested_handle: Handle::parse("carol.awiki.test", "").unwrap(),
            verification: VerificationInput::Phone {
                phone: "+15551234567".to_string(),
                otp: None,
            },
            invite_code: Some("invite-1".to_string()),
            profile: InitialProfile {
                display_name: Some("Carol".to_string()),
                avatar_url: None,
            },
            make_default: true,
        })
        .unwrap();

    assert_eq!(result.state, HandleRegistrationState::OtpSent);
    assert_eq!(result.method, RegistrationMethod::Phone);
    assert_eq!(result.handle.as_str(), "carol.awiki.test");
    assert!(result.identity.is_none());
    assert!(result.default_identity_change.is_none());

    let requests = server.join();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, "POST");
    assert_eq!(requests[0].path, "/user-service/handle/rpc");
    let body = requests[0].json_body();
    assert_eq!(body["method"], "send_otp");
    assert_eq!(body["params"], json!({ "phone": "+15551234567" }));
}

#[test]
fn register_email_without_wait_returns_email_sent_state() {
    let server = TestServer::spawn(vec![
        ExpectedHttp::json(json!({ "verified": false })),
        ExpectedHttp::json(json!({ "sent": true })),
    ]);
    let fixture = Fixture::new();
    let core = fixture.core_with_base_url(server.base_url());

    let result = core
        .identities()
        .register_handle(RegisterHandleRequest {
            local_alias: Some("carol".to_string()),
            requested_handle: Handle::parse("carol.awiki.test", "").unwrap(),
            verification: VerificationInput::Email {
                email: "carol@example.test".to_string(),
                wait_for_verification: false,
            },
            invite_code: None,
            profile: InitialProfile {
                display_name: Some("Carol".to_string()),
                avatar_url: None,
            },
            make_default: true,
        })
        .unwrap();

    assert_eq!(result.state, HandleRegistrationState::EmailSent);
    assert_eq!(result.method, RegistrationMethod::Email);
    assert_eq!(result.handle.as_str(), "carol.awiki.test");
    assert!(result.identity.is_none());
    assert!(result.default_identity_change.is_none());

    let requests = server.join();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].method, "GET");
    assert_eq!(
        requests[0].path,
        "/user-service/auth/email-status?email=carol%40example.test&handle=carol.awiki.test"
    );
    assert_eq!(requests[1].method, "POST");
    assert_eq!(requests[1].path, "/user-service/auth/email-send");
    assert_eq!(
        requests[1].json_body(),
        json!({ "email": "carol@example.test", "handle": "carol.awiki.test" })
    );
}

#[test]
fn auth_service_returns_stable_structures() {
    let fixture = Fixture::new();
    let core = fixture.core();
    let client = core
        .client(IdentitySelector::LocalAlias("alice".to_string()))
        .unwrap();

    let login = client.auth().login().unwrap();
    assert_eq!(login.subject.as_str(), "did:example:alice");
    assert_eq!(login.scope, AuthScope::UserProfile);

    let ensured = client.auth().ensure_session(AuthScope::Messaging).unwrap();
    assert_eq!(ensured.subject.as_str(), "did:example:alice");
    assert_eq!(ensured.scope, AuthScope::Messaging);

    let status = client.auth().status().unwrap();
    assert_eq!(status.subject.as_str(), "did:example:alice");
    assert!(status.has_session);
    assert!(!status.needs_refresh);
}

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = unique_temp_root();
        let identities = root.join("identities");
        fs::create_dir_all(&identities).unwrap();
        fs::write(identities.join("default"), "alice\n").unwrap();
        fs::write(
            identities.join("registry.json"),
            r#"{
              "default_identity": "alice",
              "identities": [
                {
                  "id": "alice-id",
                  "did": "did:example:alice",
                  "handle": "alice.awiki.test",
                  "display_name": "Alice",
                  "local_alias": "alice",
                  "device_id": "device-a",
                  "ready_for_auth": true,
                  "ready_for_messaging": true,
                  "missing": []
                },
                {
                  "id": "bob-id",
                  "did": "did:example:bob",
                  "handle": "bob.awiki.test",
                  "display_name": "Bob",
                  "local_alias": "bob",
                  "ready_for_auth": true,
                  "ready_for_messaging": true,
                  "missing": []
                }
              ]
            }"#,
        )
        .unwrap();
        write_identity_runtime(
            &identities,
            "alice",
            "did:example:alice",
            "2026-05-21T00:00:00Z",
        );
        write_identity_runtime(
            &identities,
            "bob",
            "did:example:bob",
            "2026-05-21T00:00:00Z",
        );
        Self { root }
    }

    fn core(&self) -> ImCore {
        self.core_with_base_url("https://example.test")
    }

    fn core_with_base_url(&self, base_url: &str) -> ImCore {
        ImCore::new(
            ImCoreConfig {
                service_base_url: ServiceEndpoint::parse(base_url).unwrap(),
                did_domain: "awiki.test".to_string(),
                user_service_endpoint: None,
                message_service_endpoint: None,
                mail_service_endpoint: None,
                anp_service_endpoint: None,
                anp_service_did: None,
                ca_bundle: None,
                transport_policy: MessageTransportPolicy::HttpOnly,
            },
            ImCorePaths {
                identities: IdentityRegistryPaths {
                    identity_root_dir: self.root.join("identities"),
                    registry_path: self.root.join("identities").join("registry.json"),
                    default_identity_path: Some(self.root.join("identities").join("default")),
                },
                local_state: LocalStatePaths {
                    sqlite_path: self.root.join("local").join("im.sqlite"),
                },
                runtime: RuntimePaths {
                    cache_dir: self.root.join("cache"),
                    temp_dir: self.root.join("tmp"),
                },
            },
        )
        .unwrap()
    }
}

fn write_identity_runtime(identities: &std::path::Path, alias: &str, did: &str, expires_at: &str) {
    let identity_dir = identities.join(alias);
    fs::create_dir_all(&identity_dir).unwrap();
    fs::write(
        identity_dir.join("did.json"),
        format!(r#"{{"id":"{did}","controller":"{did}"}}"#),
    )
    .unwrap();
    fs::write(
        identity_dir.join("private.key"),
        format!("test-private-key-for-{alias}\n"),
    )
    .unwrap();
    fs::write(
        identity_dir.join("auth.json"),
        format!(r#"{{"jwt_token":"test-token-for-{alias}","expires_at":"{expires_at}"}}"#),
    )
    .unwrap();
}

fn unique_temp_root() -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("im-core-phase1c-{}-{nanos}", std::process::id()))
}

struct TestServer {
    base_url: String,
    handle: thread::JoinHandle<Vec<CapturedHttp>>,
}

impl TestServer {
    fn spawn(responses: Vec<ExpectedHttp>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let base_url = format!("http://{}", listener.local_addr().unwrap());
        let handle = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut captured = Vec::new();
            for response in responses {
                let mut stream = accept_before_deadline(&listener, deadline);
                let request = read_http_request(&mut stream);
                write_json_response(&mut stream, response.status_code, &response.body);
                captured.push(request);
            }
            captured
        });
        Self { base_url, handle }
    }

    fn base_url(&self) -> &str {
        &self.base_url
    }

    fn join(self) -> Vec<CapturedHttp> {
        self.handle.join().unwrap()
    }
}

struct ExpectedHttp {
    status_code: u16,
    body: Value,
}

impl ExpectedHttp {
    fn json(body: Value) -> Self {
        Self {
            status_code: 200,
            body,
        }
    }

    fn rpc_result(result: Value) -> Self {
        Self::json(json!({
            "jsonrpc": "2.0",
            "id": "req-1",
            "result": result,
        }))
    }
}

#[derive(Debug)]
struct CapturedHttp {
    method: String,
    path: String,
    body: Vec<u8>,
}

impl CapturedHttp {
    fn json_body(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap()
    }
}

fn accept_before_deadline(listener: &TcpListener, deadline: Instant) -> TcpStream {
    loop {
        match listener.accept() {
            Ok((stream, _)) => return stream,
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                assert!(Instant::now() < deadline, "timed out waiting for request");
                thread::sleep(Duration::from_millis(10));
            }
            Err(err) => panic!("accept request: {err}"),
        }
    }
}

fn read_http_request(stream: &mut TcpStream) -> CapturedHttp {
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut raw = Vec::new();
    let mut buffer = [0_u8; 4096];
    let header_end = loop {
        let count = stream.read(&mut buffer).unwrap();
        assert!(count > 0, "request closed before headers");
        raw.extend_from_slice(&buffer[..count]);
        if let Some(index) = raw.windows(4).position(|window| window == b"\r\n\r\n") {
            break index;
        }
    };
    let headers_text = std::str::from_utf8(&raw[..header_end]).unwrap();
    let mut lines = headers_text.lines();
    let request_line = lines.next().unwrap();
    let mut request_parts = request_line.split_whitespace();
    let method = request_parts.next().unwrap().to_string();
    let path = request_parts.next().unwrap().to_string();
    let headers = lines
        .filter_map(|line| {
            let (key, value) = line.split_once(':')?;
            Some((key.trim().to_ascii_lowercase(), value.trim().to_string()))
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    let content_length = headers
        .get("content-length")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(0);
    let body_start = header_end + 4;
    while raw.len() < body_start + content_length {
        let count = stream.read(&mut buffer).unwrap();
        assert!(count > 0, "request closed before body");
        raw.extend_from_slice(&buffer[..count]);
    }
    CapturedHttp {
        method,
        path,
        body: raw[body_start..body_start + content_length].to_vec(),
    }
}

fn write_json_response(stream: &mut TcpStream, status_code: u16, body: &Value) {
    let body = body.to_string();
    write!(
        stream,
        "HTTP/1.1 {status_code} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    )
    .unwrap();
}
