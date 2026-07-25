use super::*;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde_json::json;
use std::io::{Read as _, Write as _};
use std::net::TcpListener;
use std::time::{Duration, Instant};

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
            "/user-service/handle/rpc",
            "lookup",
            json!({}),
        )
        .await,
        "lookup transport is not configured for /user-service/handle/rpc",
    );
    assert_transport_unavailable(
        AsyncRestTransport::rest_post(
            &mut transport,
            "/user-service/auth/email-send",
            "POST",
            json!({}),
        )
        .await,
        "POST transport is not configured for /user-service/auth/email-send",
    );
    assert_transport_unavailable(
        AsyncRestTransport::rest_get(
            &mut transport,
            "/user-service/auth/email-status",
            "GET",
            &BTreeMap::new(),
        )
        .await,
        "GET transport is not configured for /user-service/auth/email-status",
    );
    assert_transport_unavailable(
        AsyncAuthenticatedRestTransport::authenticated_rest_post(
            &mut transport,
            "/user-service/did/profile",
            "PATCH",
            json!({}),
        )
        .await,
        "PATCH transport is not configured for /user-service/did/profile",
    );
    assert_transport_unavailable(
        AsyncAuthenticatedRestTransport::authenticated_rest_get(
            &mut transport,
            "/user-service/did/profile",
            "GET",
            &BTreeMap::new(),
        )
        .await,
        "GET transport is not configured for /user-service/did/profile",
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
    let endpoint = crate::config::ServiceEndpoint::parse(&format!("http://{address}")).unwrap();
    let core = crate::core::ImCore::new(
        crate::config::ImCoreConfig {
            service_base_url: endpoint,
            did_domain: "awiki.test".to_owned(),
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
        "/user-service/did/device/rpc",
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

#[test]
fn registration_reconciliation_registry_requires_the_exact_single_device() {
    let generated =
        crate::internal::identity_generation::generate_vnext_handle_identity_with_default_daemon_subkey(
            "example.test",
            "alice",
            None,
            None,
        )
        .unwrap();
    let pending = crate::internal::identity_registration_pending::PendingRegistration::new(
        "alice".to_owned(),
        "example.test".to_owned(),
        "alice".to_owned(),
        "Alice".to_owned(),
        true,
        "already_verified".to_owned(),
        None,
        None,
        generated,
    )
    .unwrap();
    let registry = |e2ee_key_id: &str| {
        json!({
            "did": pending.generated.did.as_str(),
            "checkpoint": {
                "document_version": 1,
                "document_hash": pending.document_hash,
                "registry_version": 1
            },
            "devices": [{
                "device_id": pending.generated.protocol_device_id.as_str(),
                "signing_key_id": pending.generated.device_signing_key_id,
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
        registry(&pending.generated.device_e2ee_key_id),
    )
    .unwrap();
    assert_eq!(
        validate_pending_registration_registry_value(
            &pending,
            registry(&format!("{}#wrong-e2ee", pending.generated.did.as_str()))
        ),
        Err(crate::ImError::PermissionDenied)
    );
}
