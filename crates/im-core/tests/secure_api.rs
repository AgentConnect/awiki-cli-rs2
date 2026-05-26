use im_core::{
    ids::{GroupRef, PeerRef},
    secure::{SecureOutboxId, SecureOutboxStatus, SecureProblemCode},
    IdentityRegistryPaths, IdentitySelector, ImCore, ImCoreConfig, ImCorePaths, LocalStatePaths,
    MessageTransportPolicy, RuntimePaths, ServiceEndpoint,
};
use serde_json::{json, Value};

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::{Duration, Instant};

#[test]
fn secure_service_api_shape_is_available_from_client() {
    let root = unique_temp_root("im-core-secure-api");
    write_identity_fixture(&root, "alice", "did:example:alice");
    let core = ImCore::new(test_config(), test_paths(&root)).unwrap();
    let client = core
        .client(IdentitySelector::LocalAlias("alice".to_owned()))
        .unwrap();

    let direct = client
        .secure()
        .direct(PeerRef::parse("did:example:bob", "").unwrap())
        .status()
        .unwrap();
    assert_eq!(direct.peer.as_str(), "did:example:bob");
    assert_eq!(
        direct.resolved_peer.as_ref().map(|did| did.as_str()),
        Some("did:example:bob")
    );
    assert_eq!(direct.state, im_core::secure::DirectSecureState::Preparing);
    assert!(!direct.can_send_secure);
    assert_eq!(direct.pending_outbox_count, 0);
    assert_eq!(
        direct.problem.as_ref().map(|problem| &problem.code),
        Some(&SecureProblemCode::PeerKeysUnavailable)
    );

    let group = client
        .secure()
        .group(GroupRef::parse("did:example:groups:secure-api").unwrap())
        .status()
        .unwrap();
    assert_eq!(group.group.as_str(), "did:example:groups:secure-api");
    assert_eq!(group.state, im_core::secure::GroupSecureState::Unavailable);
    assert!(!group.can_send_secure);
    assert!(!group.local_readiness.has_local_state);
    assert_eq!(
        group.problem.as_ref().map(|problem| &problem.code),
        Some(&SecureProblemCode::IdentityNotReady)
    );

    let failed = client.secure().outbox().list_failed().unwrap();
    assert!(failed.is_empty());
}

#[test]
fn secure_outbox_id_rejects_empty_values() {
    let result = im_core::secure::SecureOutboxId::parse("  ");
    assert!(matches!(result, Err(im_core::ImError::InvalidInput { .. })));
}

#[test]
fn secure_direct_prepare_initializes_send_state_and_returns_redacted_dto() {
    let root = unique_temp_root("im-core-secure-direct-prepare-api");
    let identity = TestIdentity::new("alice.secure-api.example", "alice");
    write_real_identity_fixture(&root, "alice", &identity);
    let server = TestServer::spawn(vec![
        ExpectedHttp::rpc_result(json!({})),
        ExpectedHttp::rpc_result(json!({})),
    ]);
    let core = ImCore::new(
        test_config_with_base_url(server.base_url()),
        test_paths(&root),
    )
    .unwrap();
    let client = core
        .client(IdentitySelector::LocalAlias("alice".to_owned()))
        .unwrap();

    let result = client
        .secure()
        .direct(PeerRef::parse("did:example:bob", "").unwrap())
        .prepare()
        .unwrap();

    assert_eq!(result.peer.as_str(), "did:example:bob");
    assert_eq!(
        result.state,
        im_core::secure::DirectSecureState::WaitingForPeer
    );
    assert!(!result.can_send_secure);
    assert!(result
        .warnings
        .iter()
        .any(|warning| warning.contains("local send state prepared")));
    let rendered = format!("{result:?}");
    assert!(!rendered.contains("direct.e2ee."));
    assert!(!rendered.contains("application/anp-direct"));

    let requests = server.join();
    assert_eq!(requests.len(), 2);
    assert!(requests.iter().all(|request| request.path == "/im/rpc"));
    assert!(requests.iter().all(|request| {
        request
            .json_body()
            .get("method")
            .and_then(Value::as_str)
            .is_some_and(|method| method.starts_with("direct.e2ee."))
    }));
}

#[test]
fn secure_outbox_failed_retry_drop_uses_redacted_public_dto() {
    let root = unique_temp_root("im-core-secure-outbox-api");
    write_identity_fixture(&root, "alice", "did:example:alice");
    let paths = test_paths(&root);
    let core = ImCore::new(test_config(), paths.clone()).unwrap();
    let client = core
        .client(IdentitySelector::LocalAlias("alice".to_owned()))
        .unwrap();
    std::fs::create_dir_all(paths.local_state.sqlite_path.parent().unwrap()).unwrap();
    let db = rusqlite::Connection::open(&paths.local_state.sqlite_path).unwrap();
    im_core::compat::local_state::ensure_schema(&db).unwrap();
    db.execute(
        r#"
INSERT INTO e2ee_outbox
    (outbox_id, owner_identity_id, owner_did, peer_did, original_type, plaintext, local_status,
     attempt_count, last_error_code, retry_hint, created_at, updated_at, credential_name)
VALUES ('outbox-secret', 'alice-id', 'did:example:alice', 'did:example:bob', 'text',
        'do not expose this plaintext', 'failed', 2, 'pending_confirmation', 'retry',
        '2026-05-24T00:00:00Z', '2026-05-24T00:00:01Z', 'alice')"#,
        [],
    )
    .unwrap();
    drop(db);

    let failed = client.secure().outbox().list_failed().unwrap();

    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].id.as_str(), "outbox-secret");
    assert!(matches!(failed[0].status, SecureOutboxStatus::Failed));
    assert_eq!(failed[0].attempt_count, 2);
    assert_eq!(
        failed[0]
            .last_error
            .as_ref()
            .map(|problem| &problem.message),
        Some(&"pending_confirmation".to_owned())
    );
    assert!(!format!("{failed:?}").contains("do not expose"));

    let retried = client
        .secure()
        .outbox()
        .retry(SecureOutboxId::parse("outbox-secret").unwrap())
        .unwrap();
    assert!(matches!(retried.status, SecureOutboxStatus::Queued));

    let dropped = client
        .secure()
        .outbox()
        .drop(SecureOutboxId::parse("outbox-secret").unwrap())
        .unwrap();
    assert!(matches!(dropped.status, SecureOutboxStatus::Dropped));
}

fn test_config() -> ImCoreConfig {
    test_config_with_base_url("https://example.test")
}

fn test_config_with_base_url(base_url: &str) -> ImCoreConfig {
    ImCoreConfig {
        service_base_url: ServiceEndpoint::parse(base_url).unwrap(),
        did_domain: "awiki.test".to_owned(),
        user_service_endpoint: None,
        message_service_endpoint: None,
        mail_service_endpoint: None,
        anp_service_endpoint: None,
        anp_service_did: None,
        ca_bundle: None,
        transport_policy: MessageTransportPolicy::HttpOnly,
    }
}

fn test_paths(root: &std::path::Path) -> ImCorePaths {
    ImCorePaths {
        identities: IdentityRegistryPaths {
            identity_root_dir: root.join("identities"),
            registry_path: root.join("identities").join("registry.json"),
            default_identity_path: Some(root.join("identities").join("default")),
        },
        local_state: LocalStatePaths {
            sqlite_path: root.join("local").join("im.sqlite"),
        },
        runtime: RuntimePaths {
            cache_dir: root.join("cache"),
            temp_dir: root.join("tmp"),
        },
    }
}

fn write_identity_fixture(root: &std::path::Path, alias: &str, did: &str) {
    let identity_root = root.join("identities");
    let identity_dir = identity_root.join(alias);
    std::fs::create_dir_all(&identity_dir).unwrap();
    std::fs::write(identity_root.join("default"), format!("{alias}\n")).unwrap();
    std::fs::write(
        identity_root.join("registry.json"),
        serde_json::json!({
            "default_identity": alias,
            "identities": [{
                "id": "alice-id",
                "did": did,
                "local_alias": alias,
                "ready_for_auth": true,
                "ready_for_messaging": true,
                "missing": []
            }]
        })
        .to_string(),
    )
    .unwrap();
    std::fs::write(identity_dir.join("did.json"), "{}").unwrap();
}

fn write_real_identity_fixture(root: &std::path::Path, alias: &str, identity: &TestIdentity) {
    let identity_root = root.join("identities");
    let identity_dir = identity_root.join(alias);
    std::fs::create_dir_all(&identity_dir).unwrap();
    std::fs::write(identity_root.join("default"), format!("{alias}\n")).unwrap();
    std::fs::write(
        identity_root.join("registry.json"),
        json!({
            "default_identity": alias,
            "identities": [{
                "id": "alice-id",
                "did": identity.did,
                "local_alias": alias,
                "ready_for_auth": true,
                "ready_for_messaging": true,
                "missing": []
            }]
        })
        .to_string(),
    )
    .unwrap();
    std::fs::write(
        identity_dir.join("did.json"),
        serde_json::to_vec_pretty(&identity.document).unwrap(),
    )
    .unwrap();
    std::fs::write(identity_dir.join("private.key"), &identity.key1_private_pem).unwrap();
    std::fs::write(
        identity_dir.join("e2ee-agreement-private.pem"),
        &identity.agreement_private_pem,
    )
    .unwrap();
    std::fs::write(
        identity_dir.join("auth.json"),
        r#"{"jwt_token":"test-token"}"#,
    )
    .unwrap();
}

struct TestIdentity {
    did: String,
    document: Value,
    key1_private_pem: String,
    agreement_private_pem: String,
}

impl TestIdentity {
    fn new(domain: &str, label: &str) -> Self {
        let service = anp::authentication::build_agent_message_service_with_options(
            "#message",
            format!("https://{domain}/anp-im/rpc"),
            anp::authentication::AnpMessageServiceOptions::default()
                .with_service_did(format!("did:wba:{domain}")),
        );
        let bundle = anp::authentication::create_did_wba_document(
            domain,
            anp::authentication::DidDocumentOptions {
                path_segments: vec!["agents".to_owned(), label.to_owned()],
                domain: Some(domain.to_owned()),
                challenge: Some(format!("secure-direct-prepare-api-{label}")),
                services: vec![service],
                did_profile: anp::authentication::DidProfile::E1,
                ..Default::default()
            },
        )
        .unwrap();
        let did = bundle.did().unwrap().to_owned();
        Self {
            did,
            document: bundle.did_document.clone(),
            key1_private_pem: bundle.private_key_pem("key-1").unwrap().to_owned(),
            agreement_private_pem: bundle.private_key_pem("key-3").unwrap().to_owned(),
        }
    }
}

fn unique_temp_root(prefix: &str) -> std::path::PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("{prefix}-{nanos}"));
    std::fs::create_dir_all(&root).unwrap();
    root
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
    fn rpc_result(result: Value) -> Self {
        Self {
            status_code: 200,
            body: json!({
                "jsonrpc": "2.0",
                "id": "req-1",
                "result": result,
            }),
        }
    }
}

#[derive(Debug)]
struct CapturedHttp {
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
    let _method = request_parts.next().unwrap();
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
