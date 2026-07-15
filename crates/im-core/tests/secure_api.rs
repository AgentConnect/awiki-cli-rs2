use awiki_im_core::{
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

#[cfg(not(feature = "blocking"))]
#[test]
fn secure_service_sync_methods_fail_closed_by_default() {
    let root = unique_temp_root("im-core-secure-api");
    let identity = TestIdentity::new("alice.secure-api.example", "alice");
    write_real_identity_fixture(&root, "alice", &identity);
    let core = ImCore::new(test_config(), test_paths(&root)).unwrap();
    let client = core
        .client(IdentitySelector::LocalAlias("alice".to_owned()))
        .unwrap();

    let direct_status = client
        .secure()
        .direct(PeerRef::parse("did:example:bob", "").unwrap())
        .status();
    assert!(matches!(
        direct_status,
        Err(awiki_im_core::ImError::UnsupportedCapability { capability })
            if capability == "sync-direct-secure-status"
    ));

    let direct_prepare = client
        .secure()
        .direct(PeerRef::parse("did:example:bob", "").unwrap())
        .prepare();
    assert!(matches!(
        direct_prepare,
        Err(awiki_im_core::ImError::UnsupportedCapability { capability })
            if capability == "sync-direct-secure-prepare"
    ));

    let group_status = client
        .secure()
        .group(GroupRef::parse("did:example:groups:secure-api").unwrap())
        .status();
    #[cfg(feature = "group-e2ee")]
    assert!(matches!(
        group_status,
        Err(awiki_im_core::ImError::UnsupportedCapability { capability })
            if capability == "sync-group-secure-status"
    ));
    #[cfg(not(feature = "group-e2ee"))]
    {
        let group_status = group_status.unwrap();
        assert_eq!(
            group_status.state,
            awiki_im_core::secure::GroupSecureState::Unavailable
        );
        assert_eq!(
            group_status.problem.as_ref().map(|problem| &problem.code),
            Some(&SecureProblemCode::Unsupported)
        );
    }

    let failed = client.secure().outbox().list_failed();
    assert!(matches!(
        failed,
        Err(awiki_im_core::ImError::UnsupportedCapability { capability })
            if capability == "sync-secure-outbox"
    ));
}

#[cfg(not(feature = "blocking"))]
#[test]
fn direct_secure_file_outbox_flush_fails_closed_by_default() {
    let root = unique_temp_root("im-core-secure-file-outbox-sync-api");
    let identity = TestIdentity::new("alice.secure-file-outbox.example", "alice");
    let identity_dir = root.join("identities").join("alice");
    let mut client = awiki_im_core::secure::new_direct_secure_file_runtime_client(
        awiki_im_core::secure::DirectSecureFileRuntimeIdentity {
            owner_identity_id: "alice-id".to_owned(),
            owner_did: identity.did.clone(),
            identity_name: "alice".to_owned(),
            identity_dir,
            signing_private_pem: identity.key1_private_pem.clone(),
            agreement_private_pem: identity.agreement_private_pem.clone(),
            local_did_document: identity.document.clone(),
        },
        Box::new(|_, _| Ok(serde_json::Map::new())),
        Box::new(|did| {
            Err(awiki_im_core::ImError::PeerNotFound {
                peer: did.to_owned(),
            })
        }),
    )
    .unwrap();

    let warnings = awiki_im_core::secure::flush_direct_secure_file_outbox(
        &awiki_im_core::secure::DirectSecureFileOutboxFlushScope {
            owner_identity_id: "alice-id".to_owned(),
            owner_did: identity.did.clone(),
            credential_name: "alice".to_owned(),
            sqlite_path: root.join("local").join("im.sqlite"),
        },
        "did:example:bob",
        &mut client,
    );

    assert_eq!(
        warnings,
        vec!["direct secure file outbox flush is disabled in the async cutover build".to_owned()]
    );
}

#[tokio::test]
async fn secure_service_async_api_shape_is_available_from_client() {
    let root = unique_temp_root("im-core-secure-async-api");
    let identity = TestIdentity::new("alice.secure-async-api.example", "alice");
    write_real_identity_fixture(&root, "alice", &identity);
    let core = ImCore::open(test_config(), test_paths(&root))
        .await
        .unwrap();
    let client = core
        .client_async(IdentitySelector::LocalAlias("alice".to_owned()))
        .await
        .unwrap();

    let direct = client
        .secure()
        .direct(PeerRef::parse("did:example:bob", "").unwrap())
        .status_async()
        .await
        .unwrap();
    assert_eq!(direct.peer.as_str(), "did:example:bob");
    assert_eq!(
        direct.resolved_peer.as_ref().map(|did| did.as_str()),
        Some("did:example:bob")
    );
    assert_eq!(
        direct.state,
        awiki_im_core::secure::DirectSecureState::Preparing
    );
    assert!(!direct.can_send_secure);
    assert_eq!(direct.pending_outbox_count, 0);
    assert_eq!(
        direct.problem.as_ref().map(|problem| &problem.code),
        Some(&SecureProblemCode::PeerKeysUnavailable)
    );

    let group = client
        .secure()
        .group(GroupRef::parse("did:example:groups:secure-api").unwrap())
        .status_async()
        .await
        .unwrap();
    assert_eq!(group.group.as_str(), "did:example:groups:secure-api");
    #[cfg(feature = "group-e2ee")]
    assert_eq!(
        group.state,
        awiki_im_core::secure::GroupSecureState::MissingLocalState
    );
    #[cfg(not(feature = "group-e2ee"))]
    assert_eq!(
        group.state,
        awiki_im_core::secure::GroupSecureState::Unavailable
    );
    assert!(!group.can_send_secure);
    assert!(!group.local_readiness.has_local_state);
    #[cfg(feature = "group-e2ee")]
    assert_eq!(
        group.problem.as_ref().map(|problem| &problem.code),
        Some(&SecureProblemCode::GroupStateUnavailable)
    );
    #[cfg(not(feature = "group-e2ee"))]
    assert_eq!(
        group.problem.as_ref().map(|problem| &problem.code),
        Some(&SecureProblemCode::Unsupported)
    );

    let failed = client.secure().outbox().list_failed_async().await.unwrap();
    assert!(failed.is_empty());
}

#[tokio::test]
async fn secure_direct_status_async_uses_db_actor() {
    let root = unique_temp_root("im-core-secure-direct-status-async-api");
    write_identity_fixture(&root, "alice", "did:example:alice");
    let paths = test_paths(&root);
    let core = ImCore::open(test_config(), paths.clone()).await.unwrap();
    let client = core
        .client_async(IdentitySelector::LocalAlias("alice".to_owned()))
        .await
        .unwrap();
    std::fs::create_dir_all(paths.local_state.sqlite_path.parent().unwrap()).unwrap();
    let db = rusqlite::Connection::open(&paths.local_state.sqlite_path).unwrap();
    awiki_im_core::compat::local_state::ensure_schema(&db).unwrap();
    db.execute(
        r#"
INSERT INTO direct_e2ee_sessions
    (owner_identity_id, owner_did, peer_did, session_id, state_blob, metadata_json, created_at, updated_at)
VALUES ('alice-id', 'did:example:alice', 'did:example:bob', 'session-secret',
        X'7B7D', '{}', '2026-05-24T00:00:00Z', '2026-05-24T00:00:00Z')"#,
        [],
    )
    .unwrap();
    drop(db);

    let status = client
        .secure()
        .direct(PeerRef::parse("did:example:bob", "").unwrap())
        .status_async()
        .await
        .unwrap();

    assert_eq!(
        status.state,
        awiki_im_core::secure::DirectSecureState::Ready
    );
    assert!(status.can_send_secure);
    assert_eq!(
        status.resolved_peer.as_ref().map(|did| did.as_str()),
        Some("did:example:bob")
    );
    assert!(!format!("{status:?}").contains("session-secret"));
}

#[tokio::test]
async fn secure_group_status_async_api_shape_is_available() {
    let root = unique_temp_root("im-core-secure-group-status-async-shape-api");
    write_identity_fixture(&root, "alice", "did:example:alice");
    let core = ImCore::open(test_config(), test_paths(&root))
        .await
        .unwrap();
    let client = core
        .client_async(IdentitySelector::LocalAlias("alice".to_owned()))
        .await
        .unwrap();

    let group = GroupRef::parse("did:example:groups:secure-api").unwrap();
    let status = client
        .secure()
        .group(group.clone())
        .status_async()
        .await
        .unwrap();
    assert_eq!(status.group.as_str(), group.as_str());
    assert_eq!(
        status.state,
        awiki_im_core::secure::GroupSecureState::Unavailable
    );
    assert!(!status.can_send_secure);

    let prepare = client.secure().group(group).prepare_async().await.unwrap();
    assert_eq!(
        prepare.state,
        awiki_im_core::secure::GroupSecureState::Unavailable
    );
    assert!(!prepare.can_send_secure);
}

#[tokio::test]
async fn secure_group_repair_async_api_shape_is_available() {
    let root = unique_temp_root("im-core-secure-group-repair-async-shape-api");
    write_identity_fixture(&root, "alice", "did:example:alice");
    let core = ImCore::open(test_config(), test_paths(&root))
        .await
        .unwrap();
    let client = core
        .client_async(IdentitySelector::LocalAlias("alice".to_owned()))
        .await
        .unwrap();

    let err = client
        .secure()
        .group(GroupRef::parse("did:example:groups:secure-api").unwrap())
        .repair_async()
        .await
        .unwrap_err();

    assert!(matches!(
        err,
        awiki_im_core::ImError::UnsupportedCapability { .. }
            | awiki_im_core::ImError::CredentialFileUnreadable { .. }
            | awiki_im_core::ImError::AuthRequired
    ));
}

#[test]
fn secure_outbox_id_rejects_empty_values() {
    let result = awiki_im_core::secure::SecureOutboxId::parse("  ");
    assert!(matches!(
        result,
        Err(awiki_im_core::ImError::InvalidInput { .. })
    ));
}

#[cfg(feature = "blocking")]
#[test]
fn secure_direct_prepare_initializes_send_state_and_returns_redacted_dto() {
    install_test_direct_vault_root_key();
    let root = unique_temp_root("im-core-secure-direct-prepare-api");
    let identity = TestIdentity::new("alice.secure-api.example", "alice");
    write_real_identity_fixture(&root, "alice", &identity);
    let server = TestServer::spawn(vec![ExpectedHttp::rpc_result(json!({}))]);
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
        awiki_im_core::secure::DirectSecureState::WaitingForPeer
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
    assert_eq!(requests.len(), 1);
    assert!(requests.iter().all(|request| request.path == "/im/rpc"));
    assert_eq!(
        requests[0]
            .json_body()
            .get("method")
            .and_then(Value::as_str),
        Some("direct.e2ee.publish_prekey_bundle")
    );
}

#[tokio::test]
async fn secure_direct_prepare_async_initializes_send_state_and_returns_redacted_dto() {
    install_test_direct_vault_root_key();
    let root = unique_temp_root("im-core-secure-direct-prepare-async-api");
    let identity = TestIdentity::new("alice.secure-api-async.example", "alice");
    write_real_identity_fixture(&root, "alice", &identity);
    let server = TestServer::spawn(vec![ExpectedHttp::rpc_result(json!({}))]);
    let core = ImCore::open(
        test_config_with_base_url(server.base_url()),
        test_paths(&root),
    )
    .await
    .unwrap();
    let client = core
        .client_async(IdentitySelector::LocalAlias("alice".to_owned()))
        .await
        .unwrap();

    let result = client
        .secure()
        .direct(PeerRef::parse("did:example:bob", "").unwrap())
        .prepare_async()
        .await
        .unwrap();

    assert_eq!(result.peer.as_str(), "did:example:bob");
    assert_eq!(
        result.state,
        awiki_im_core::secure::DirectSecureState::WaitingForPeer
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
    assert_eq!(requests.len(), 1);
    assert!(requests.iter().all(|request| request.path == "/im/rpc"));
    assert_eq!(
        requests[0]
            .json_body()
            .get("method")
            .and_then(Value::as_str),
        Some("direct.e2ee.publish_prekey_bundle")
    );
}

#[tokio::test]
async fn secure_outbox_failed_retry_drop_uses_redacted_public_dto() {
    let root = unique_temp_root("im-core-secure-outbox-api");
    write_identity_fixture(&root, "alice", "did:example:alice");
    let paths = test_paths(&root);
    let core = ImCore::open(test_config(), paths.clone()).await.unwrap();
    let client = core
        .client_async(IdentitySelector::LocalAlias("alice".to_owned()))
        .await
        .unwrap();
    std::fs::create_dir_all(paths.local_state.sqlite_path.parent().unwrap()).unwrap();
    let db = rusqlite::Connection::open(&paths.local_state.sqlite_path).unwrap();
    awiki_im_core::compat::local_state::ensure_schema(&db).unwrap();
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

    let failed = client.secure().outbox().list_failed_async().await.unwrap();

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
        .retry_async(SecureOutboxId::parse("outbox-secret").unwrap())
        .await
        .unwrap();
    assert!(matches!(retried.status, SecureOutboxStatus::Queued));

    let dropped = client
        .secure()
        .outbox()
        .drop_async(SecureOutboxId::parse("outbox-secret").unwrap())
        .await
        .unwrap();
    assert!(matches!(dropped.status, SecureOutboxStatus::Dropped));
}

#[tokio::test]
async fn secure_outbox_async_failed_retry_drop_uses_db_actor() {
    let root = unique_temp_root("im-core-secure-outbox-async-api");
    write_identity_fixture(&root, "alice", "did:example:alice");
    let paths = test_paths(&root);
    let core = ImCore::open(test_config(), paths.clone()).await.unwrap();
    let client = core
        .client_async(IdentitySelector::LocalAlias("alice".to_owned()))
        .await
        .unwrap();
    std::fs::create_dir_all(paths.local_state.sqlite_path.parent().unwrap()).unwrap();
    let db = rusqlite::Connection::open(&paths.local_state.sqlite_path).unwrap();
    awiki_im_core::compat::local_state::ensure_schema(&db).unwrap();
    db.execute(
        r#"
INSERT INTO e2ee_outbox
    (outbox_id, owner_identity_id, owner_did, peer_did, original_type, plaintext, local_status,
     attempt_count, last_error_code, retry_hint, created_at, updated_at, credential_name)
VALUES ('outbox-secret-async', 'alice-id', 'did:example:alice', 'did:example:bob', 'text',
        'do not expose this async plaintext', 'failed', 3, 'pending_confirmation', 'retry',
        '2026-05-24T00:00:00Z', '2026-05-24T00:00:01Z', 'alice')"#,
        [],
    )
    .unwrap();
    drop(db);

    let failed = client.secure().outbox().list_failed_async().await.unwrap();

    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].id.as_str(), "outbox-secret-async");
    assert!(matches!(failed[0].status, SecureOutboxStatus::Failed));
    assert_eq!(failed[0].attempt_count, 3);
    assert!(!format!("{failed:?}").contains("do not expose"));

    let retried = client
        .secure()
        .outbox()
        .retry_async(SecureOutboxId::parse("outbox-secret-async").unwrap())
        .await
        .unwrap();
    assert!(matches!(retried.status, SecureOutboxStatus::Queued));

    let dropped = client
        .secure()
        .outbox()
        .drop_async(SecureOutboxId::parse("outbox-secret-async").unwrap())
        .await
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

fn install_test_direct_vault_root_key() {
    std::env::set_var(
        "AWIKI_IM_CORE_VAULT_ROOT_KEY_B64",
        "Hx8fHx8fHx8fHx8fHx8fHx8fHx8fHx8fHx8fHx8fHx8=",
    );
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
            Ok((stream, _)) => {
                stream.set_nonblocking(false).unwrap();
                return stream;
            }
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
