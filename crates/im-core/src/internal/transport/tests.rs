use super::*;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde_json::json;
use std::io::{Read as _, Write as _};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const HOST_ACCOUNT_ID: &str = "agent-account-refresh";

#[tokio::test]
async fn attachment_stream_uses_idle_timeout_instead_of_total_request_deadline() {
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = [0_u8; 1024];
        let _ = stream.read(&mut request).await.unwrap();
        stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nContent-Type: application/octet-stream\r\n\r\n",
            )
            .await
            .unwrap();
        stream.flush().await.unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
    });
    let response = reqwest::Client::new()
        .get(format!("http://{address}/object"))
        .send()
        .await
        .unwrap();
    let mut object = AsyncAttachmentObjectResponse::Response {
        response,
        content_type: Some("application/octet-stream".to_owned()),
    };

    let error = object
        .next_chunk_with_idle_timeout(Duration::from_millis(10))
        .await
        .expect_err("idle object body should time out");

    assert!(matches!(
        error,
        crate::ImError::AttachmentTransfer {
            failure: crate::AttachmentTransferFailure::Stalled,
            retryable: true,
            ..
        }
    ));
    server.await.unwrap();
}

#[test]
fn client_version_header_is_single_and_uses_configured_build_facts() {
    let mut config = crate::ImCoreConfig::new(
        crate::ServiceEndpoint::parse("https://awiki.info").unwrap(),
        "awiki.info",
    )
    .unwrap();
    config.client_version_info =
        Some(crate::ClientVersionInfo::new("awiki-cli", "0714", "1.0.16", Some(42)).unwrap());
    let mut headers = BTreeMap::from([(
        "Content-Type".to_owned(),
        crate::internal::json_rpc::CONTENT_TYPE_JSON.to_owned(),
    )]);

    append_client_version_header(
        &mut headers,
        &config,
        "https://awiki.info/user-service/v1/content/rpc",
    );
    append_client_version_header(
        &mut headers,
        &config,
        "https://awiki.info/user-service/v1/content/rpc",
    );

    assert_eq!(
        headers
            .iter()
            .filter(|(name, _)| name.eq_ignore_ascii_case(crate::CLIENT_VERSION_HEADER))
            .count(),
        1
    );
    assert_eq!(
        headers
            .get(crate::CLIENT_VERSION_HEADER)
            .map(String::as_str),
        Some("awiki-cli/0714/1.0.16+42")
    );

    let mut peer_headers = BTreeMap::new();
    append_client_version_header(
        &mut peer_headers,
        &config,
        "https://peer.example/anp-im/rpc",
    );
    assert!(!peer_headers.contains_key(crate::CLIENT_VERSION_HEADER));
}

struct FakeBearerProvider {
    missing: bool,
}

impl crate::internal::key_provider::IdentitySigner for FakeBearerProvider {
    fn did_document(&self) -> crate::ImResult<serde_json::Value> {
        unreachable!()
    }

    fn optional_did_document(&self) -> crate::ImResult<Option<serde_json::Value>> {
        unreachable!()
    }

    fn request_signing_key_id(&self) -> crate::ImResult<String> {
        unreachable!()
    }

    fn sign(&self, _kid: &str, _message: &[u8]) -> crate::ImResult<Vec<u8>> {
        unreachable!()
    }

    fn sign_root(&self, _kid: &str, _message: &[u8]) -> crate::ImResult<Vec<u8>> {
        unreachable!()
    }

    fn ecdh(
        &self,
        _kid: &str,
        _peer_public: &[u8],
    ) -> crate::ImResult<zeroize::Zeroizing<[u8; 32]>> {
        unreachable!()
    }

    fn auth_state(&self) -> crate::ImResult<crate::internal::auth::state::AuthStateSnapshot> {
        unreachable!()
    }

    fn valid_auth_token(&self) -> crate::ImResult<Option<String>> {
        if self.missing {
            return Ok(None);
        }
        Err(crate::ImError::CredentialFileUnreadable {
            path_kind: "test-secret-path-kind".to_owned(),
            detail: "test-secret-auth-state-detail".to_owned(),
        })
    }

    fn persist_auth_token(&self, _token: &str) -> crate::ImResult<()> {
        unreachable!()
    }
}

#[test]
fn persisted_bearer_read_failure_is_fail_closed_only_for_exact_device_product_auth() {
    let unreadable = FakeBearerProvider { missing: false };
    let (token, exact_device_error) = persisted_bearer_selection(&unreadable, true);
    assert_eq!(token, None);
    assert_eq!(exact_device_error, Some(DeferredAuthStateError::Unreadable));

    let (token, legacy_error) = persisted_bearer_selection(&unreadable, false);
    assert_eq!(token, None);
    assert_eq!(
        legacy_error, None,
        "Legacy compatibility keeps signature auth"
    );

    let missing = FakeBearerProvider { missing: true };
    let (token, exact_device_error) = persisted_bearer_selection(&missing, true);
    assert_eq!(token, None);
    assert_eq!(exact_device_error, Some(DeferredAuthStateError::Missing));

    let (token, legacy_error) = persisted_bearer_selection(&missing, false);
    assert_eq!(token, None);
    assert_eq!(
        legacy_error, None,
        "Legacy compatibility keeps signature auth"
    );
}

#[test]
fn exact_device_deferred_bearer_error_stops_request_with_redacted_local_error() {
    let root = tempfile::tempdir().unwrap();
    let core = host_backed_core(root.path(), "https://awiki.test");
    let (client, _, _) = host_backed_client(&core);
    let mut transport = CoreHttpTransport::new(&client);
    transport.deferred_auth_state_error = Some(DeferredAuthStateError::Unreadable);

    let error = AuthenticatedRpcTransport::authenticated_rpc(
        &mut transport,
        "/im/rpc",
        "prekey.publish",
        json!({}),
    )
    .unwrap_err();
    assert_eq!(
        error,
        crate::ImError::CredentialFileUnreadable {
            path_kind: "identity_auth_state".to_owned(),
            detail: "persisted exact-device bearer state could not be read".to_owned(),
        }
    );
    let rendered = format!("{error:?} {error}");
    assert!(!rendered.contains("test-secret"));
}

#[tokio::test]
async fn explicit_refresh_recovers_missing_and_unreadable_exact_device_bearer_state() {
    for deferred in [
        DeferredAuthStateError::Missing,
        DeferredAuthStateError::Unreadable,
    ] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let root = tempfile::tempdir().unwrap();
        let core = host_backed_core(root.path(), &format!("http://{address}"));
        let (client, bootstrap, _) = host_backed_client(&core);
        let refreshed_token = host_device_access_token(
            &bootstrap,
            HOST_ACCOUNT_ID,
            bootstrap.protocol_device_id.as_str(),
            &bootstrap.device_signing_key_id,
            1,
            &["device:manage", "device:read", "message:connect"],
            match deferred {
                DeferredAuthStateError::Missing => "refresh-after-missing",
                DeferredAuthStateError::Unreadable => "refresh-after-unreadable",
            },
        );
        let server_token = refreshed_token.clone();
        let server = std::thread::spawn(move || {
            let (mut refresh_stream, _) = listener.accept().unwrap();
            let refresh_request = read_request_headers(&mut refresh_stream);
            match deferred {
                DeferredAuthStateError::Missing => {
                    write_rpc_success_with_token(&mut refresh_stream, &server_token)
                }
                DeferredAuthStateError::Unreadable => {
                    write_rpc_success_with_body_token(&mut refresh_stream, &server_token)
                }
            }

            let (mut business_stream, _) = listener.accept().unwrap();
            let business_request = read_request_headers(&mut business_stream);
            write_rpc_success(&mut business_stream);
            (refresh_request, business_request)
        });

        let mut transport = CoreHttpTransport::new(&client);
        transport.deferred_auth_state_error = Some(deferred);
        assert_eq!(
            transport.refresh_jwt_async().await.unwrap(),
            refreshed_token
        );
        assert_eq!(transport.deferred_auth_state_error, None);
        AsyncAuthenticatedRpcTransport::authenticated_rpc(
            &mut transport,
            "/im/rpc",
            "prekey.publish",
            json!({}),
        )
        .await
        .unwrap();

        let (refresh_request, business_request) = server.join().unwrap();
        assert!(!refresh_request
            .to_ascii_lowercase()
            .contains("authorization: bearer"));
        assert!(business_request
            .to_ascii_lowercase()
            .contains(&format!("authorization: bearer {refreshed_token}").to_ascii_lowercase()));
    }
}

fn host_backed_core(root: &std::path::Path, endpoint: &str) -> crate::core::ImCore {
    host_backed_core_with_client_version(root, endpoint, None)
}

fn host_backed_core_with_client_version(
    root: &std::path::Path,
    endpoint: &str,
    client_version_info: Option<crate::ClientVersionInfo>,
) -> crate::core::ImCore {
    crate::core::ImCore::new(
        crate::config::ImCoreConfig {
            service_base_url: crate::config::ServiceEndpoint::parse(endpoint).unwrap(),
            did_domain: "awiki.test".to_owned(),
            client_version_info,
            user_service_endpoint: None,
            message_service_endpoint: None,
            mail_service_endpoint: None,
            anp_service_endpoint: None,
            anp_service_did: None,
            ca_bundle: None,
            transport_policy: crate::config::MessageTransportPolicy::HttpOnly,
        },
        crate::paths::ImCorePaths {
            identities: crate::paths::IdentityRegistryPaths {
                identity_root_dir: root.join("identities"),
                registry_path: root.join("identities").join("registry.json"),
                default_identity_path: Some(root.join("identities").join("default")),
            },
            local_state: crate::paths::LocalStatePaths {
                sqlite_path: root.join("local").join("im.sqlite"),
            },
            runtime: crate::paths::RuntimePaths {
                cache_dir: root.join("cache"),
                temp_dir: root.join("tmp"),
            },
        },
    )
    .unwrap()
}

#[tokio::test]
async fn captured_user_service_request_has_exactly_one_client_version_header() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let request = read_request_headers(&mut stream);
        write_rpc_success(&mut stream);
        request
    });

    let root = tempfile::tempdir().unwrap();
    let version = crate::ClientVersionInfo::new("awiki-cli", "0714", "1.0.16", Some(42)).unwrap();
    let core = host_backed_core_with_client_version(
        root.path(),
        &format!("http://{address}"),
        Some(version),
    );
    let (client, _, _) = host_backed_client(&core);
    let mut transport = CoreHttpTransport::new(&client);
    AsyncAuthenticatedRpcTransport::authenticated_rpc(
        &mut transport,
        "/user-service/v1/did/rpc",
        "get_me",
        json!({}),
    )
    .await
    .unwrap();

    let request = server.join().unwrap();
    let version_headers = request
        .lines()
        .filter(|line| {
            line.split_once(':')
                .is_some_and(|(name, _)| name.eq_ignore_ascii_case(crate::CLIENT_VERSION_HEADER))
        })
        .collect::<Vec<_>>();
    assert_eq!(
        version_headers,
        vec!["x-awiki-client-version: awiki-cli/0714/1.0.16+42"]
    );
}

fn host_device_access_token(
    bootstrap: &crate::identity::VNextAgentBootstrapMaterial,
    account_id: &str,
    device_id: &str,
    key_id: &str,
    auth_generation: u64,
    scopes: &[&str],
    jti: &str,
) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let claims = json!({
        "iss": "user-service",
        "aud": ["awiki-user-service", "awiki-message-service"],
        "sub": bootstrap.did.as_str(),
        "type": "access",
        "purpose": "awiki.device.access.v1",
        "did": bootstrap.did.as_str(),
        "user_id": account_id,
        "device_id": device_id,
        "key_id": key_id,
        "auth_generation": auth_generation,
        "scopes": scopes,
        "iat": now,
        "nbf": now,
        "exp": now + 3600,
        "jti": jti,
    });
    format!(
        "e30.{}.test-signature",
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).unwrap())
    )
}

fn host_backed_client(
    core: &crate::core::ImCore,
) -> (
    crate::core::ImClient,
    crate::identity::VNextAgentBootstrapMaterial,
    String,
) {
    host_backed_client_inner(core, None)
}

fn host_backed_client_inner(
    core: &crate::core::ImCore,
    persistence: Option<Arc<dyn crate::identity::HostBackedAuthTokenPersistence>>,
) -> (
    crate::core::ImClient,
    crate::identity::VNextAgentBootstrapMaterial,
    String,
) {
    let bootstrap = core
        .generate_vnext_agent_bootstrap(
            crate::identity::AgentIdentityKind::Daemon,
            "refresh-daemon",
        )
        .unwrap();
    let initial_token = host_device_access_token(
        &bootstrap,
        HOST_ACCOUNT_ID,
        bootstrap.protocol_device_id.as_str(),
        &bootstrap.device_signing_key_id,
        1,
        &["device:manage", "device:read", "message:connect"],
        "host-bootstrap-token",
    );
    let material = crate::identity::HostBackedDeviceIdentityMaterial {
        identity_id: bootstrap.identity_id.clone(),
        did: bootstrap.did.as_str().to_owned(),
        handle: Some(format!("{}.awiki.test", bootstrap.handle_local_part)),
        display_name: Some("Refresh daemon".to_owned()),
        account_id: HOST_ACCOUNT_ID.to_owned(),
        binding_generation: "1".to_owned(),
        did_document: bootstrap.did_document.clone(),
        protocol_device_id: bootstrap.protocol_device_id.clone(),
        device_signing_key_id: bootstrap.device_signing_key_id.clone(),
        device_signing_private_key_pem: bootstrap.device_signing_private_key_pem.clone(),
        device_e2ee_key_id: bootstrap.device_e2ee_key_id.clone(),
        device_e2ee_private_key_pem: bootstrap.device_e2ee_private_key_pem.clone(),
        root_key_id: bootstrap.root_key_id.clone(),
        root_private_key_pem: bootstrap.root_private_key_pem.clone(),
        authorization_status: crate::identity::IdentityDeviceAuthorizationStatus::Active,
        role: crate::identity::IdentityDeviceRole::Admin,
        management_ready: true,
        auth_generation: "1".to_owned(),
        access_token: initial_token.clone(),
    };
    let client = match persistence {
        Some(persistence) => {
            core.client_with_device_identity_material_and_auth_persistence(material, persistence)
        }
        None => core.client_with_device_identity_material(material),
    }
    .unwrap();
    (client, bootstrap, initial_token)
}

#[derive(Clone, Default)]
struct RecordingAuthTokenPersistence {
    tokens: Arc<Mutex<Vec<String>>>,
}

impl crate::identity::HostBackedAuthTokenPersistence for RecordingAuthTokenPersistence {
    fn persist_auth_token(&self, token: &str) -> crate::ImResult<()> {
        self.tokens.lock().unwrap().push(token.to_owned());
        Ok(())
    }
}

fn assert_transport_unavailable<T>(result: crate::ImResult<T>, expected: &str) {
    match result {
        Err(crate::ImError::TransportUnavailable { detail }) => {
            assert_eq!(detail, expected);
        }
        Err(err) => panic!("expected transport unavailable, got {err:?}"),
        Ok(_) => panic!("expected transport unavailable, got success"),
    }
}

#[tokio::test]
async fn async_unavailable_transport_errors_match_sync_shape() {
    let mut transport = UnavailableTransport;
    assert_transport_unavailable(
        AsyncAuthenticatedRpcTransport::authenticated_rpc(
            &mut transport,
            "/im/rpc",
            "direct.send",
            json!({}),
        )
        .await,
        "direct.send transport is not configured for /im/rpc",
    );
    assert_transport_unavailable(
        AsyncRpcTransport::rpc(
            &mut transport,
            "/user-service/v1/handle/rpc",
            "lookup",
            json!({}),
        )
        .await,
        "lookup transport is not configured for /user-service/v1/handle/rpc",
    );
    assert_transport_unavailable(
        AsyncRestTransport::rest_post(
            &mut transport,
            "/user-service/v1/auth/email-send",
            "POST",
            json!({}),
        )
        .await,
        "POST transport is not configured for /user-service/v1/auth/email-send",
    );
    assert_transport_unavailable(
        AsyncRestTransport::rest_get(
            &mut transport,
            "/user-service/v1/auth/email-status",
            "GET",
            &BTreeMap::new(),
        )
        .await,
        "GET transport is not configured for /user-service/v1/auth/email-status",
    );
    assert_transport_unavailable(
        AsyncAuthenticatedRestTransport::authenticated_rest_post(
            &mut transport,
            "/user-service/v1/did/profile",
            "PATCH",
            json!({}),
        )
        .await,
        "PATCH transport is not configured for /user-service/v1/did/profile",
    );
    assert_transport_unavailable(
        AsyncAuthenticatedRestTransport::authenticated_rest_get(
            &mut transport,
            "/user-service/v1/did/profile",
            "GET",
            &BTreeMap::new(),
        )
        .await,
        "GET transport is not configured for /user-service/v1/did/profile",
    );
    assert_transport_unavailable(
        AsyncRawJsonTransport::get_json_url(
            &mut transport,
            "https://example.test/did.json",
            BTreeMap::new(),
        )
        .await,
        "GET transport is not configured for https://example.test/did.json",
    );
    assert_transport_unavailable(
        AsyncAttachmentObjectTransport::put_attachment_object(
            &mut transport,
            "https://object.test/upload",
            BTreeMap::new(),
            b"body".to_vec(),
        )
        .await,
        "PUT transport is not configured for https://object.test/upload",
    );
    assert_transport_unavailable(
        AsyncAttachmentObjectTransport::get_attachment_object(
            &mut transport,
            "https://object.test/download",
            "ticket",
        )
        .await,
        "GET transport is not configured for https://object.test/download",
    );
}

#[test]
fn hosted_legacy_identity_accepts_refreshed_did_access_token() {
    let root = tempfile::tempdir().unwrap();
    let endpoint = crate::config::ServiceEndpoint::parse("https://awiki.test").unwrap();
    let core = crate::core::ImCore::new(
        crate::config::ImCoreConfig {
            service_base_url: endpoint,
            did_domain: "awiki.test".to_owned(),
            client_version_info: None,
            user_service_endpoint: None,
            message_service_endpoint: None,
            mail_service_endpoint: None,
            anp_service_endpoint: None,
            anp_service_did: None,
            ca_bundle: None,
            transport_policy: crate::config::MessageTransportPolicy::HttpOnly,
        },
        crate::paths::ImCorePaths {
            identities: crate::paths::IdentityRegistryPaths {
                identity_root_dir: root.path().join("identities"),
                registry_path: root.path().join("identities").join("registry.json"),
                default_identity_path: Some(root.path().join("identities").join("default")),
            },
            local_state: crate::paths::LocalStatePaths {
                sqlite_path: root.path().join("local").join("im.sqlite"),
            },
            runtime: crate::paths::RuntimePaths {
                cache_dir: root.path().join("cache"),
                temp_dir: root.path().join("tmp"),
            },
        },
    )
    .unwrap();
    let did = "did:wba:awiki.test:agent:daemon:edgehost:e1_demo";
    let client = core
        .client_with_identity_material(crate::identity::HostedIdentityMaterial {
            identity_id: "daemon-agent".to_owned(),
            did: did.to_owned(),
            handle: Some("edgehost.awiki.test".to_owned()),
            display_name: None,
            did_document: json!({"id": did}),
            default_signing_private_key_pem: "signing-secret".to_owned(),
            e2ee_agreement_private_key_pem: Some("agreement-secret".to_owned()),
            auth_token: None,
        })
        .unwrap();
    let token = format!(
        "e30.{}.signature",
        URL_SAFE_NO_PAD.encode(
            serde_json::to_vec(&json!({
                "iss": "user-service",
                "sub": did,
                "type": "access",
                "exp": time::OffsetDateTime::now_utc().unix_timestamp() + 300
            }))
            .unwrap()
        )
    );

    validate_access_token_for_client(&client, &token).unwrap();
}

#[tokio::test]
async fn ephemeral_bearer_401_does_not_retry_or_persist_response_token() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let first_request = read_request_headers(&mut stream);
        write_unauthorized_with_token(&mut stream);

        listener.set_nonblocking(true).unwrap();
        let deadline = Instant::now() + Duration::from_millis(300);
        let mut request_count = 1;
        while Instant::now() < deadline {
            match listener.accept() {
                Ok((mut retry, _)) => {
                    request_count += 1;
                    let _ = read_request_headers(&mut retry);
                    write_unauthorized_with_token(&mut retry);
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("accept retry request: {error}"),
            }
        }
        (first_request, request_count)
    });

    let root = tempfile::tempdir().unwrap();
    let identities = root.path().join("identities");
    let identity_dir = identities.join("alice");
    std::fs::create_dir_all(&identity_dir).unwrap();
    std::fs::write(identities.join("default"), "alice\n").unwrap();
    std::fs::write(
        identities.join("registry.json"),
        r#"{
          "default_identity": "alice",
          "identities": [{
            "id": "alice-id",
            "did": "did:example:alice",
            "handle": "alice.awiki.test",
            "display_name": "Alice",
            "local_alias": "alice",
            "ready_for_auth": true,
            "ready_for_messaging": true,
            "missing": []
          }]
        }"#,
    )
    .unwrap();
    std::fs::write(
        identity_dir.join("did.json"),
        r#"{"id":"did:example:alice","controller":"did:example:alice"}"#,
    )
    .unwrap();
    std::fs::write(identity_dir.join("private.key"), "unused\n").unwrap();
    let auth_path = identity_dir.join("auth.json");
    std::fs::write(&auth_path, r#"{"jwt_token":"persisted-old-token"}"#).unwrap();
    let endpoint = crate::config::ServiceEndpoint::parse(format!("http://{address}")).unwrap();
    let core = crate::core::ImCore::new(
        crate::config::ImCoreConfig {
            service_base_url: endpoint,
            did_domain: "awiki.test".to_owned(),
            client_version_info: None,
            user_service_endpoint: None,
            message_service_endpoint: None,
            mail_service_endpoint: None,
            anp_service_endpoint: None,
            anp_service_did: None,
            ca_bundle: None,
            transport_policy: crate::config::MessageTransportPolicy::HttpOnly,
        },
        crate::paths::ImCorePaths {
            identities: crate::paths::IdentityRegistryPaths {
                identity_root_dir: identities.clone(),
                registry_path: identities.join("registry.json"),
                default_identity_path: Some(identities.join("default")),
            },
            local_state: crate::paths::LocalStatePaths {
                sqlite_path: root.path().join("local").join("im.sqlite"),
            },
            runtime: crate::paths::RuntimePaths {
                cache_dir: root.path().join("cache"),
                temp_dir: root.path().join("tmp"),
            },
        },
    )
    .unwrap();
    let client = core
        .client(crate::identity::IdentitySelector::LocalAlias(
            "alice".to_owned(),
        ))
        .unwrap();
    let mut transport =
        CoreHttpTransport::new_with_ephemeral_bearer(&client, "probe-old-token").unwrap();
    let result = AsyncAuthenticatedRpcTransport::authenticated_rpc(
        &mut transport,
        "/user-service/v1/did/device/rpc",
        "device_registry_get",
        json!({"did":"did:example:alice"}),
    )
    .await;
    assert!(matches!(
        result,
        Err(crate::ImError::Service {
            status_code: Some(401),
            code: None,
            ..
        })
    ));

    let (request, request_count) = server.join().unwrap();
    assert_eq!(
        request_count, 1,
        "ephemeral probe must not retry with a signature"
    );
    assert!(request
        .to_ascii_lowercase()
        .contains("authorization: bearer probe-old-token"));
    assert_eq!(
        std::fs::read_to_string(auth_path).unwrap(),
        r#"{"jwt_token":"persisted-old-token"}"#
    );
}

#[tokio::test]
async fn host_backed_401_refresh_accepts_and_persists_exact_device_access() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let root = tempfile::tempdir().unwrap();
    let core = host_backed_core(root.path(), &format!("http://{address}"));
    let persistence = RecordingAuthTokenPersistence::default();
    let persisted_tokens = persistence.tokens.clone();
    let (client, bootstrap, initial_token) =
        host_backed_client_inner(&core, Some(Arc::new(persistence)));
    let refreshed_token = host_device_access_token(
        &bootstrap,
        HOST_ACCOUNT_ID,
        bootstrap.protocol_device_id.as_str(),
        &bootstrap.device_signing_key_id,
        1,
        &["device:manage", "device:read", "message:connect"],
        "host-refreshed-token",
    );
    let server_token = refreshed_token.clone();
    let server = std::thread::spawn(move || {
        let (mut first_stream, _) = listener.accept().unwrap();
        let first = read_request_headers(&mut first_stream);
        write_unauthorized(&mut first_stream);

        let (mut second_stream, _) = listener.accept().unwrap();
        let second = read_request_headers(&mut second_stream);
        write_rpc_success_with_token(&mut second_stream, &server_token);
        (first, second)
    });

    let mut transport = CoreHttpTransport::new(&client);
    let expected = transport
        .expected_device_access
        .as_ref()
        .expect("host-backed transport must carry exact Device Access claims");
    assert_eq!(expected.did, bootstrap.did.as_str());
    assert_eq!(expected.user_id, HOST_ACCOUNT_ID);
    assert_eq!(expected.device_id, bootstrap.protocol_device_id.as_str());
    assert_eq!(expected.key_id, bootstrap.device_signing_key_id);
    assert_eq!(expected.auth_generation, 1);
    assert_eq!(
        expected.role,
        crate::internal::identity_device_state::DeviceAuthorizationRole::Admin
    );
    assert!(expected.management_ready);

    let result = AsyncAuthenticatedRpcTransport::authenticated_rpc(
        &mut transport,
        "/user-service/v1/did/rpc",
        "get_me",
        json!({}),
    )
    .await
    .unwrap();
    assert_eq!(result, json!({"ok": true}));
    let (first, second) = server.join().unwrap();
    assert!(first
        .to_ascii_lowercase()
        .contains(&format!("authorization: bearer {initial_token}").to_ascii_lowercase()));
    assert!(!second.contains(&initial_token));
    assert_eq!(
        client
            .runtime()
            .key_provider
            .valid_auth_token()
            .unwrap()
            .as_deref(),
        Some(refreshed_token.as_str())
    );
    assert_eq!(
        transport.jwt_token.as_deref(),
        Some(refreshed_token.as_str())
    );
    assert_eq!(
        persisted_tokens.lock().unwrap().as_slice(),
        &[refreshed_token]
    );
}

#[test]
fn host_backed_response_token_rejects_wrong_exact_claims_without_secret_leak_or_store() {
    let root = tempfile::tempdir().unwrap();
    let core = host_backed_core(root.path(), "https://awiki.test");
    let (client, bootstrap, initial_token) = host_backed_client(&core);
    let admin_scopes = ["device:manage", "device:read", "message:connect"];
    let member_scopes = [
        "device:read",
        "device:root-import-complete",
        "message:connect",
    ];
    let cases = [
        host_device_access_token(
            &bootstrap,
            "wrong-account-secret",
            bootstrap.protocol_device_id.as_str(),
            &bootstrap.device_signing_key_id,
            1,
            &admin_scopes,
            "wrong-account-token",
        ),
        host_device_access_token(
            &bootstrap,
            HOST_ACCOUNT_ID,
            "wrong-device-secret",
            &bootstrap.device_signing_key_id,
            1,
            &admin_scopes,
            "wrong-device-token",
        ),
        host_device_access_token(
            &bootstrap,
            HOST_ACCOUNT_ID,
            bootstrap.protocol_device_id.as_str(),
            &bootstrap.device_signing_key_id,
            2,
            &admin_scopes,
            "wrong-generation-token",
        ),
        host_device_access_token(
            &bootstrap,
            HOST_ACCOUNT_ID,
            bootstrap.protocol_device_id.as_str(),
            "did:wba:awiki.test:wrong-signing-key-secret",
            1,
            &admin_scopes,
            "wrong-key-token",
        ),
        host_device_access_token(
            &bootstrap,
            HOST_ACCOUNT_ID,
            bootstrap.protocol_device_id.as_str(),
            &bootstrap.device_signing_key_id,
            1,
            &member_scopes,
            "wrong-readiness-token",
        ),
    ];

    let mut transport = CoreHttpTransport::new(&client);
    for token in cases {
        let headers = BTreeMap::from([(
            "Authentication-Info".to_owned(),
            format!("access_token={token}"),
        )]);
        let error = transport
            .capture_token("https://awiki.test/user-service/v1/did/rpc", &headers)
            .unwrap_err();
        assert_eq!(error, crate::ImError::PermissionDenied);
        let rendered = format!("{error:?} {error}");
        assert!(!rendered.contains(&token));
        assert!(!rendered.contains("wrong-account-secret"));
        assert!(!rendered.contains("wrong-device-secret"));
        assert!(!rendered.contains("wrong-signing-key-secret"));
        assert_eq!(
            client
                .runtime()
                .key_provider
                .valid_auth_token()
                .unwrap()
                .as_deref(),
            Some(initial_token.as_str())
        );
        assert!(transport.pending_auth_commit.is_none());
    }
}

fn read_request_headers(stream: &mut std::net::TcpStream) -> String {
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let mut request = Vec::new();
    let mut buffer = [0_u8; 2048];
    while !request.windows(4).any(|window| window == b"\r\n\r\n") {
        let read = stream.read(&mut buffer).unwrap();
        assert!(read > 0, "request closed before headers");
        request.extend_from_slice(&buffer[..read]);
    }
    String::from_utf8_lossy(&request).into_owned()
}

fn write_unauthorized_with_token(stream: &mut std::net::TcpStream) {
    stream
        .write_all(
            b"HTTP/1.1 401 Unauthorized\r\nContent-Type: application/json\r\nContent-Length: 2\r\nAuthentication-Info: access_token=server-poison-token\r\nConnection: close\r\n\r\n{}",
        )
        .unwrap();
    stream.flush().unwrap();
}

fn write_unauthorized(stream: &mut std::net::TcpStream) {
    stream
        .write_all(
            b"HTTP/1.1 401 Unauthorized\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}",
        )
        .unwrap();
    stream.flush().unwrap();
}

fn write_rpc_success_with_token(stream: &mut std::net::TcpStream, token: &str) {
    let body = br#"{"jsonrpc":"2.0","id":1,"result":{"ok":true}}"#;
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nAuthentication-Info: access_token={}\r\nConnection: close\r\n\r\n",
        body.len(),
        token
    )
    .unwrap();
    stream.write_all(body).unwrap();
    stream.flush().unwrap();
}

fn write_rpc_success_with_body_token(stream: &mut std::net::TcpStream, token: &str) {
    let body = serde_json::to_vec(&json!({
        "jsonrpc": "2.0",
        "id": 1,
        "result": {"ok": true, "access_token": token},
    }))
    .unwrap();
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len(),
    )
    .unwrap();
    stream.write_all(&body).unwrap();
    stream.flush().unwrap();
}

fn write_rpc_success(stream: &mut std::net::TcpStream) {
    let body = br#"{"jsonrpc":"2.0","id":1,"result":{"ok":true}}"#;
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len(),
    )
    .unwrap();
    stream.write_all(body).unwrap();
    stream.flush().unwrap();
}

#[test]
fn registration_reconciliation_registry_requires_the_exact_single_device() {
    let create = crate::internal::identity_generation::vnext_handle_anp_identity_create_spec(
        "example.test",
        "alice",
        None,
        None,
    )
    .unwrap();
    let root = tempfile::tempdir().unwrap();
    let mut custody = anp_identity::DidStore::initialize_local_file(root.path()).unwrap();
    let generated = custody.create_identity(create.spec).unwrap();
    let manifest = anp::authentication::validate_device_manifest(generated.document())
        .unwrap()
        .unwrap();
    let device = &manifest.devices[0];
    let identity = crate::internal::identity_registration_pending::PendingRegistrationIdentity {
        controller_store_id: "controller-store".to_owned(),
        controller_identity_id: "controller-identity".to_owned(),
        did: crate::ids::Did::parse(generated.did()).unwrap(),
        did_document: generated.document().clone(),
        protocol_device_id: crate::ids::ProtocolDeviceId::parse(&device.device_id).unwrap(),
        root_key_id: format!("{}#key-1", generated.did()),
        device_signing_key_id: device.signing_key_id.clone(),
        device_e2ee_key_id: device.e2ee_key_id.clone(),
        legacy_daemon_authorization: false,
        controller_revision_id: Some("revision-1".to_owned()),
    };
    let pending = crate::internal::identity_registration_pending::PendingRegistration::new(
        "alice".to_owned(),
        "example.test".to_owned(),
        "alice".to_owned(),
        "Alice".to_owned(),
        true,
        "already_verified".to_owned(),
        None,
        None,
        identity,
    )
    .unwrap();
    let registry = |e2ee_key_id: &str| {
        json!({
            "did": pending.identity.did.as_str(),
            "checkpoint": {
                "document_version": 1,
                "document_hash": pending.document_hash,
                "registry_version": 1
            },
            "devices": [{
                "device_id": pending.identity.protocol_device_id.as_str(),
                "signing_key_id": pending.identity.device_signing_key_id,
                "e2ee_key_id": e2ee_key_id,
                "status": "active",
                "role": "admin",
                "management_ready": true,
                "auth_generation": 1
            }]
        })
    };

    validate_pending_registration_registry_value(
        &pending,
        registry(&pending.identity.device_e2ee_key_id),
    )
    .unwrap();
    assert_eq!(
        validate_pending_registration_registry_value(
            &pending,
            registry(&format!("{}#wrong-e2ee", pending.identity.did.as_str()))
        ),
        Err(crate::ImError::PermissionDenied)
    );
}

#[test]
fn registration_reconciliation_requires_exact_structured_absence_reason() {
    let body = serde_json::to_vec(&json!({
        "jsonrpc": "2.0",
        "id": 1,
        "error": {
            "code": -32000,
            "message": "DID is not active",
            "data": {"awiki_code": "did_auth.active_did_not_found"}
        }
    }))
    .unwrap();
    let absent = decode_rpc_http_response(401, &body).unwrap_err();
    assert!(registration_is_explicitly_absent(&absent));

    for data in [
        None,
        Some(json!({"awiki_code": "did_auth.invalid_signature"})),
        Some(json!({"awiki_code": "did_auth.active_did_not_found_extra"})),
    ] {
        let error = crate::ImError::Service {
            status_code: Some(401),
            code: Some("-32000".to_owned()),
            message: "Unauthenticated".to_owned(),
            data,
        };
        assert!(!registration_is_explicitly_absent(&error));
    }
}

#[test]
fn successful_json_rpc_with_empty_body_is_transport_unavailable_without_body_detail() {
    for status_code in [200, 204, 299] {
        assert_transport_unavailable(
            decode_rpc_http_response(status_code, b""),
            "JSON-RPC response body is empty",
        );
    }
}

#[test]
fn successful_json_rpc_with_nonempty_malformed_body_remains_serialization() {
    let error = decode_rpc_http_response(200, b"not-json").unwrap_err();
    assert!(matches!(error, crate::ImError::Serialization { .. }));
}

#[test]
fn redirect_json_rpc_bodies_keep_existing_http_service_mapping() {
    for body in [b"".as_slice(), b"not-json".as_slice()] {
        let error = decode_rpc_http_response(302, body).unwrap_err();
        assert!(matches!(
            error,
            crate::ImError::Service {
                status_code: Some(302),
                ..
            }
        ));
    }
}

#[test]
fn skill_onboarding_rpc_error_preserves_reason_on_non_success_http_status() {
    let body = serde_json::to_vec(&json!({
        "jsonrpc": "2.0",
        "id": 1,
        "error": {
            "code": -32001,
            "message": "request rejected",
            "data": {"reason": "skill_onboarding_token_expired"}
        }
    }))
    .unwrap();

    let error = decode_rpc_http_response(503, &body).unwrap_err();
    match error {
        crate::ImError::Service {
            status_code, data, ..
        } => {
            assert_eq!(status_code, Some(503));
            assert_eq!(
                data.and_then(|value| value.get("reason").cloned()),
                Some(json!("skill_onboarding_token_expired"))
            );
        }
        other => panic!("expected service error, got {other:?}"),
    }
}
