use awiki_im_core::{
    ids::{GroupRef, PeerRef},
    secure::SecureProblemCode,
    IdentityRegistryPaths, IdentitySelector, ImCore, ImCoreConfig, ImCorePaths, LocalStatePaths,
    MessageTransportPolicy, RuntimePaths, ServiceEndpoint,
};
use serde_json::{json, Value};

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

fn test_config() -> ImCoreConfig {
    test_config_with_base_url("https://example.test")
}

fn test_config_with_base_url(base_url: &str) -> ImCoreConfig {
    ImCoreConfig {
        service_base_url: ServiceEndpoint::parse(base_url).unwrap(),
        did_domain: "awiki.test".to_owned(),
        client_version_info: None,
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
