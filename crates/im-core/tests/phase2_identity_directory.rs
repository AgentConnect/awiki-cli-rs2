use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use awiki_im_core::prelude::*;
use serde_json::{json, Value};

#[tokio::test]
async fn identity_service_profile_uses_public_http_transport() {
    let fixture = Fixture::new();
    let server = RpcTestServer::spawn(vec![
        ExpectedRpc::new(
            "/user-service/v1/did/profile/rpc",
            "get_me",
            json!({}),
            json!({
                "did": "did:example:alice",
                "handle": "alice.awiki.test",
                "nick_name": "Alice Remote",
                "bio": "Rust public API",
                "tags": ["sdk", "http"],
                "profile_md": "## Alice",
                "versionId": "wns-profile-7",
            }),
        ),
        ExpectedRpc::new(
            "/user-service/v1/did/profile/rpc",
            "update_me",
            json!({
                "nick_name": "Alice Updated",
                "bio": "sdk profile skeleton",
                "tags": ["sdk"],
                "profile_md": "# Alice",
            }),
            json!({
                "did": "did:example:alice",
                "handle": "alice.awiki.test",
                "nick_name": "Alice Updated",
                "bio": "sdk profile skeleton",
                "tags": ["sdk"],
                "profile_md": "# Alice",
                "profile_version": "18446744073709551616",
                "versionId": "wns-profile-8",
            }),
        ),
    ]);
    let base_url = server.base_url().to_owned();
    let client = fixture.client_async_with_base_url("alice", &base_url).await;

    let profile = client.identity().profile_async().await.unwrap();
    assert_eq!(profile.subject.as_str(), "did:example:alice");
    assert_eq!(profile.display_name.as_deref(), Some("Alice Remote"));
    assert_eq!(profile.bio.as_deref(), Some("Rust public API"));
    assert_eq!(profile.tags, vec!["sdk", "http"]);
    assert_eq!(profile.profile_version, None);
    assert_eq!(profile.version_id.as_deref(), Some("wns-profile-7"));
    let local_identity = fixture
        .core()
        .identities()
        .resolve(IdentitySelector::Id(IdentityId::parse("alice-id").unwrap()))
        .unwrap();
    assert_eq!(
        local_identity.display_name.as_deref(),
        Some("Alice Remote"),
        "an authoritative get_me profile must refresh the local identity projection"
    );

    let updated = client
        .identity()
        .update_profile_async(ProfilePatch {
            display_name: Some("Alice Updated".to_string()),
            bio: Some("sdk profile skeleton".to_string()),
            tags: Some(vec!["sdk".to_string()]),
            markdown: Some("# Alice".to_string()),
            ..ProfilePatch::default()
        })
        .await
        .unwrap();
    assert_eq!(updated.subject.as_str(), "did:example:alice");
    assert_eq!(updated.display_name.as_deref(), Some("Alice Updated"));
    assert_eq!(updated.markdown.as_deref(), Some("# Alice"));
    assert_eq!(
        updated.profile_version.as_deref(),
        Some("18446744073709551616")
    );
    assert_eq!(updated.version_id.as_deref(), Some("wns-profile-8"));

    let requests = server.join();
    assert_eq!(requests.len(), 2);
    assert!(requests
        .iter()
        .all(|request| request.authorization.as_deref() == Some("Bearer test-token-for-alice")));
}

#[test]
fn identity_registry_display_name_projection_is_id_scoped_and_clearable() {
    let fixture = Fixture::new();
    let core = fixture.core();
    let identity_id = IdentityId::parse("alice-id").unwrap();
    let original = core
        .identities()
        .resolve(IdentitySelector::Id(identity_id.clone()))
        .unwrap();

    let updated = core
        .identities()
        .update_display_name_projection(identity_id.clone(), Some("Alice Canonical"))
        .unwrap();
    assert_eq!(updated.display_name.as_deref(), Some("Alice Canonical"));
    assert_eq!(updated.id, original.id);
    assert_eq!(updated.did, original.did);
    assert_eq!(updated.handle, original.handle);
    assert_eq!(updated.local_alias, original.local_alias);
    assert_eq!(updated.device_id, original.device_id);
    assert_eq!(updated.readiness, original.readiness);

    let cleared = core
        .identities()
        .update_display_name_projection(identity_id, None)
        .unwrap();
    assert_eq!(cleared.display_name, None);
    assert_eq!(cleared.did, original.did);
    assert_eq!(cleared.handle, original.handle);
}

#[tokio::test]
async fn identity_service_profile_prefers_full_handle_over_bare_handle() {
    let fixture = Fixture::new();
    let server = RpcTestServer::spawn(vec![ExpectedRpc::new(
        "/user-service/v1/did/profile/rpc",
        "get_me",
        json!({}),
        json!({
            "did": "did:wba:anpclaw.com:zhuocheng:e1_key",
            "handle": "zhuocheng",
            "full_handle": "zhuocheng.anpclaw.com",
            "nick_name": "Zhuocheng",
            "bio": "Rust public API",
            "tags": ["sdk", "http"],
            "profile_md": "## Zhuocheng",
        }),
    )]);
    let client = fixture
        .client_async_with_base_url("alice", server.base_url())
        .await;

    let profile = client.identity().profile_async().await.unwrap();

    assert_eq!(
        profile.handle.as_ref().map(|handle| handle.as_str()),
        Some("zhuocheng.anpclaw.com")
    );
    let requests = server.join();
    assert_eq!(requests.len(), 1);
}

#[tokio::test]
async fn identity_service_profile_expands_bare_handle_with_wba_did_domain() {
    let fixture = Fixture::new();
    let server = RpcTestServer::spawn(vec![ExpectedRpc::new(
        "/user-service/v1/did/profile/rpc",
        "get_me",
        json!({}),
        json!({
            "did": "did:wba:anpclaw.com:zhuocheng:e1_key",
            "handle": "zhuocheng",
            "nick_name": "Zhuocheng",
        }),
    )]);
    let client = fixture
        .client_async_with_base_url("alice", server.base_url())
        .await;

    let profile = client.identity().profile_async().await.unwrap();

    assert_eq!(
        profile.handle.as_ref().map(|handle| handle.as_str()),
        Some("zhuocheng.anpclaw.com")
    );
    let requests = server.join();
    assert_eq!(requests.len(), 1);
}

#[tokio::test]
async fn identity_service_profile_uses_domain_qualified_handle_when_full_handle_is_bare() {
    let fixture = Fixture::new();
    let server = RpcTestServer::spawn(vec![ExpectedRpc::new(
        "/user-service/v1/did/profile/rpc",
        "get_me",
        json!({}),
        json!({
            "did": "did:wba:anpclaw.com:zhuocheng:e1_key",
            "handle": "zhuocheng.anpclaw.com",
            "full_handle": "zhuocheng",
            "nick_name": "Zhuocheng",
        }),
    )]);
    let client = fixture
        .client_async_with_base_url("alice", server.base_url())
        .await;

    let profile = client.identity().profile_async().await.unwrap();

    assert_eq!(
        profile.handle.as_ref().map(|handle| handle.as_str()),
        Some("zhuocheng.anpclaw.com")
    );
    let requests = server.join();
    assert_eq!(requests.len(), 1);
}

#[tokio::test]
async fn identity_service_profile_async_uses_public_http_transport() {
    let fixture = Fixture::new();
    let server = RpcTestServer::spawn(vec![
        ExpectedRpc::new(
            "/user-service/v1/did/profile/rpc",
            "get_me",
            json!({}),
            json!({
                "did": "did:example:alice",
                "handle": "alice.awiki.test",
                "nick_name": "Alice Remote",
                "bio": "Rust async public API",
                "tags": ["sdk", "async"],
                "profile_md": "## Alice",
            }),
        ),
        ExpectedRpc::new(
            "/user-service/v1/did/profile/rpc",
            "update_me",
            json!({
                "nick_name": "Alice Async",
                "bio": "async profile skeleton",
                "tags": ["async"],
                "profile_md": "# Alice Async",
            }),
            json!({
                "did": "did:example:alice",
                "handle": "alice.awiki.test",
                "nick_name": "Alice Async",
                "bio": "async profile skeleton",
                "tags": ["async"],
                "profile_md": "# Alice Async",
            }),
        ),
    ]);
    let client = fixture.client_with_base_url("alice", server.base_url());

    let profile = client.identity().profile_async().await.unwrap();
    assert_eq!(profile.subject.as_str(), "did:example:alice");
    assert_eq!(profile.display_name.as_deref(), Some("Alice Remote"));
    assert_eq!(profile.bio.as_deref(), Some("Rust async public API"));
    assert_eq!(profile.tags, vec!["sdk", "async"]);

    let updated = client
        .identity()
        .update_profile_async(ProfilePatch {
            display_name: Some("Alice Async".to_string()),
            bio: Some("async profile skeleton".to_string()),
            tags: Some(vec!["async".to_string()]),
            markdown: Some("# Alice Async".to_string()),
            ..ProfilePatch::default()
        })
        .await
        .unwrap();
    assert_eq!(updated.subject.as_str(), "did:example:alice");
    assert_eq!(updated.display_name.as_deref(), Some("Alice Async"));
    assert_eq!(updated.markdown.as_deref(), Some("# Alice Async"));

    let requests = server.join();
    assert_eq!(requests.len(), 2);
    assert!(requests
        .iter()
        .all(|request| request.authorization.as_deref() == Some("Bearer test-token-for-alice")));
}

#[test]
fn identity_profile_bridge_maps_get_me_result_to_sdk_profile() {
    let fixture = Fixture::new();
    let client = fixture.client("alice");
    let result = awiki_im_core::compat::profile::read_self_profile_with_bridge(
        &client,
        ProfileSession {
            subject: client.did().clone(),
        },
        ProfileTransport::default(),
    )
    .unwrap();

    assert_eq!(result.profile.subject.as_str(), "did:example:alice");
    assert_eq!(
        result.profile.handle.as_ref().unwrap().as_str(),
        "alice.awiki.test"
    );
    assert_eq!(result.profile.display_name.as_deref(), Some("Alice Remote"));
    assert_eq!(result.profile.bio.as_deref(), Some("Rust port"));
    assert_eq!(result.profile.tags, vec!["rust", "cli"]);
    assert_eq!(result.profile.markdown.as_deref(), Some("## Alice"));
    assert_eq!(
        result.profile.avatar_url.as_deref(),
        Some("https://cdn.test/a.png")
    );
    assert_eq!(
        result.profile.avatar_uri.as_deref(),
        Some("https://cdn.test/a.png")
    );
    assert_eq!(
        result.profile.updated_at.as_deref(),
        Some("2026-05-21T00:00:00Z")
    );
    assert!(result
        .profile
        .metadata
        .iter()
        .any(|attribute| { attribute.key == "source" && attribute.value == "profile-service" }));
    assert!(result.profile.metadata.iter().any(|attribute| {
        attribute.key == "avatar_url" && attribute.value == "https://cdn.test/a.png"
    }));
    assert_eq!(result.raw["nick_name"], "Alice Remote");
}

#[test]
fn identity_profile_bridge_updates_current_profile() {
    let fixture = Fixture::new();
    let client = fixture.client("alice");
    let result = awiki_im_core::compat::profile::update_profile_with_bridge(
        &client,
        ProfilePatch {
            display_name: Some(" Alice Updated ".to_string()),
            bio: Some(" Rust port ".to_string()),
            tags: Some(vec!["rust".to_string(), "cli".to_string()]),
            markdown: Some("## Alice".to_string()),
            ..ProfilePatch::default()
        },
        ProfileSession {
            subject: client.did().clone(),
        },
        ProfileUpdateTransport,
    )
    .unwrap();

    assert_eq!(
        result.changed_fields,
        vec![
            "display_name".to_string(),
            "bio".to_string(),
            "tags".to_string(),
            "profile_md".to_string(),
        ]
    );
    assert_eq!(
        result.profile.display_name.as_deref(),
        Some("Alice Updated")
    );
    assert_eq!(result.profile.bio.as_deref(), Some("Rust port"));
    assert_eq!(result.profile.tags, vec!["rust", "cli"]);
    assert_eq!(result.profile.markdown.as_deref(), Some("## Alice"));
    assert_eq!(result.raw["nick_name"], "Alice Updated");
    let identity_payload: Value = serde_json::from_slice(
        &fs::read(
            fixture
                .root
                .join("identities")
                .join("alice")
                .join("identity.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(identity_payload["name"], "Alice Updated");
    let registry_payload: Value = serde_json::from_slice(
        &fs::read(fixture.root.join("identities").join("registry.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        registry_payload["identities"][0]["display_name"],
        "Alice Updated"
    );
}

#[test]
fn identity_profile_update_empty_patch_does_not_call_transport() {
    let fixture = Fixture::new();
    let client = fixture.client("alice");
    let updated = awiki_im_core::compat::profile::update_profile_with_bridge(
        &client,
        ProfilePatch::default(),
        ProfileSession {
            subject: client.did().clone(),
        },
        ProfileUpdateTransport,
    );

    assert!(matches!(
        updated,
        Err(ImError::InvalidInput { message, .. })
            if message.contains("no profile fields were provided")
    ));
}

#[test]
fn identity_service_validates_profile_patch_before_stub() {
    let fixture = Fixture::new();
    let client = fixture.client("alice");

    let updated = client.identity().update_profile(ProfilePatch {
        display_name: Some(" ".to_string()),
        ..ProfilePatch::default()
    });
    assert!(matches!(
        updated,
        Err(ImError::InvalidInput { field: Some(field), .. }) if field == "display_name"
    ));
}

#[tokio::test]
async fn directory_service_exposes_contact_store_and_resolution_api() {
    let fixture = Fixture::new();
    let server = RpcTestServer::spawn(vec![
        ExpectedRpc::new(
            "/user-service/v1/handle/rpc",
            "lookup",
            json!({ "handle": "bob.awiki.test" }),
            handle_lookup_value(),
        ),
        ExpectedRpc::new(
            "/user-service/v1/did/profile/rpc",
            "get_public_profile",
            json!({ "did": "did:example:bob" }),
            public_profile_value(),
        ),
        ExpectedRpc::new(
            "/user-service/v1/did/profile/rpc",
            "resolve",
            json!({ "did": "did:example:bob" }),
            json!({ "did": "did:example:bob", "status": "active" }),
        ),
    ]);
    let client = fixture.client_with_base_url("alice", server.base_url());
    assert_eq!(client.directory().owner_did().as_str(), "did:example:alice");

    let peer = PeerRef::parse("bob.awiki.test", "").unwrap();
    let resolved = client
        .directory()
        .resolve_peer_async(peer.clone())
        .await
        .unwrap();
    assert_eq!(resolved.did.as_str(), "did:example:bob");
    assert_eq!(resolved.handle.as_ref().unwrap().as_str(), "bob.awiki.test");
    assert_eq!(
        resolved.profile.as_ref().unwrap().display_name.as_deref(),
        Some("Bob")
    );
    assert_eq!(
        resolved.profile.as_ref().unwrap().subject.as_str(),
        "did:example:bob"
    );
    assert!(resolved.warnings.is_empty());
    let requests = server.join();
    assert_eq!(requests.len(), 3);
    assert!(requests
        .iter()
        .all(|request| request.authorization.as_deref() == Some("Bearer test-token-for-alice")));

    let saved = client
        .directory()
        .save_contact_async(SaveContactRequest {
            peer: peer.clone(),
            did: Some(Did::parse("did:example:bob").unwrap()),
            handle: Some(Handle::parse("bob.awiki.test", "").unwrap()),
            display_name: Some("Bob".to_string()),
            relationship: Some("friend".to_string()),
            note: Some("Phase 2 contact".to_string()),
        })
        .await
        .unwrap();
    assert_eq!(saved.did.as_str(), "did:example:bob");
    assert_eq!(saved.handle.as_ref().unwrap().as_str(), "bob.awiki.test");
    assert_eq!(saved.display_name.as_deref(), Some("Bob"));
    assert_eq!(saved.relationship.as_deref(), Some("friend"));
    assert_eq!(saved.note.as_deref(), Some("Phase 2 contact"));

    let contacts = client
        .directory()
        .contacts_async(ContactListQuery {
            limit: Some(PageLimit(10)),
        })
        .await
        .unwrap();
    assert_eq!(contacts.items.len(), 1);
    assert_eq!(contacts.items[0].did.as_str(), "did:example:bob");
    assert!(!contacts.has_more);

    let relation = client
        .directory()
        .relation_status_async(peer)
        .await
        .unwrap();
    assert_eq!(relation.did.as_ref().unwrap().as_str(), "did:example:bob");
    assert!(relation.is_contact);
    assert_eq!(relation.relationship.as_deref(), Some("friend"));
    assert!(!relation.followed);
    assert!(!relation.messaged);
}

#[tokio::test]
async fn directory_service_resolves_federated_full_handle_via_current_user_service() {
    let fixture = Fixture::new();
    let server = RpcTestServer::spawn(vec![
        ExpectedRpc::new(
            "/user-service/v1/handle/rpc",
            "lookup",
            json!({ "handle": "agent.remote.example" }),
            federated_handle_lookup_value(),
        ),
        ExpectedRpc::new(
            "/user-service/v1/did/profile/rpc",
            "resolve",
            json!({ "did": "did:wba:remote.example:user:agent:e1" }),
            json!({
                "did": "did:wba:remote.example:user:agent:e1",
                "service_endpoints": [{
                    "id": "did:wba:remote.example:user:agent:e1#messaging",
                    "type": "ANPMessageService",
                    "serviceEndpoint": "https://messages.remote.example/im"
                }]
            }),
        ),
    ]);
    let client = fixture.client_with_base_url("alice", server.base_url());

    let resolved = client
        .directory()
        .resolve_peer_async(PeerRef::parse("agent.remote.example", "").unwrap())
        .await
        .unwrap();

    assert_eq!(
        resolved.did.as_str(),
        "did:wba:remote.example:user:agent:e1"
    );
    assert_eq!(
        resolved.handle.as_ref().map(|handle| handle.as_str()),
        Some("agent.remote.example")
    );
    assert_eq!(
        resolved.profile.as_ref().unwrap().display_name.as_deref(),
        Some("Remote Agent")
    );
    assert_eq!(
        resolved.profile.as_ref().unwrap().subject.as_str(),
        "did:wba:remote.example:user:agent:e1"
    );
    assert!(resolved.warnings.is_empty());

    let requests = server.join();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].path, "/user-service/v1/handle/rpc");
    assert_eq!(
        requests[0].params,
        json!({ "handle": "agent.remote.example" })
    );
    assert_eq!(requests[1].path, "/user-service/v1/did/profile/rpc");
    assert_eq!(
        requests[1].params,
        json!({ "did": "did:wba:remote.example:user:agent:e1" })
    );
}

#[tokio::test]
async fn directory_service_async_uses_actor_projection_and_resolution_api() {
    let fixture = Fixture::new();
    let server = RpcTestServer::spawn(vec![
        ExpectedRpc::new(
            "/user-service/v1/handle/rpc",
            "lookup",
            json!({ "handle": "bob.awiki.test" }),
            handle_lookup_value(),
        ),
        ExpectedRpc::new(
            "/user-service/v1/did/profile/rpc",
            "get_public_profile",
            json!({ "did": "did:example:bob" }),
            public_profile_value(),
        ),
        ExpectedRpc::new(
            "/user-service/v1/did/profile/rpc",
            "resolve",
            json!({ "did": "did:example:bob" }),
            json!({ "did": "did:example:bob", "status": "active" }),
        ),
    ]);
    let client = fixture.client_with_base_url("alice", server.base_url());
    let peer = PeerRef::parse("bob.awiki.test", "").unwrap();

    let resolved = client
        .directory()
        .resolve_peer_async(peer.clone())
        .await
        .unwrap();
    assert_eq!(resolved.did.as_str(), "did:example:bob");
    assert_eq!(resolved.handle.as_ref().unwrap().as_str(), "bob.awiki.test");
    assert_eq!(
        resolved.profile.as_ref().unwrap().display_name.as_deref(),
        Some("Bob")
    );

    let requests = server.join();
    assert_eq!(requests.len(), 3);
    assert!(requests
        .iter()
        .all(|request| request.authorization.as_deref() == Some("Bearer test-token-for-alice")));

    let saved = client
        .directory()
        .save_contact_async(SaveContactRequest {
            peer: peer.clone(),
            did: Some(Did::parse("did:example:bob").unwrap()),
            handle: Some(Handle::parse("bob.awiki.test", "").unwrap()),
            display_name: Some("Bob".to_string()),
            relationship: Some("friend".to_string()),
            note: Some("Async contact".to_string()),
        })
        .await
        .unwrap();
    assert_eq!(saved.did.as_str(), "did:example:bob");
    assert_eq!(saved.handle.as_ref().unwrap().as_str(), "bob.awiki.test");
    assert_eq!(saved.relationship.as_deref(), Some("friend"));

    let contacts = client
        .directory()
        .contacts_async(ContactListQuery {
            limit: Some(PageLimit(10)),
        })
        .await
        .unwrap();
    assert_eq!(contacts.items.len(), 1);
    assert_eq!(contacts.items[0].did.as_str(), "did:example:bob");

    let relation = client
        .directory()
        .relation_status_async(peer)
        .await
        .unwrap();
    assert_eq!(relation.did.as_ref().unwrap().as_str(), "did:example:bob");
    assert!(relation.is_contact);
    assert_eq!(relation.relationship.as_deref(), Some("friend"));
}

#[tokio::test]
async fn directory_resolution_prefers_valid_wns_profile_projection() {
    let fixture = Fixture::new();
    let server = RpcTestServer::spawn(vec![
        ExpectedRpc::new(
            "/user-service/v1/handle/rpc",
            "lookup",
            json!({ "handle": "bob.awiki.test" }),
            handle_lookup_with_profile_value(),
        ),
        ExpectedRpc::new(
            "/user-service/v1/did/profile/rpc",
            "resolve",
            json!({ "did": "did:example:bob" }),
            json!({ "did": "did:example:bob", "status": "active" }),
        ),
    ]);
    let client = fixture.client_with_base_url("alice", server.base_url());

    let resolved = client
        .directory()
        .resolve_peer_async(PeerRef::parse("bob.awiki.test", "").unwrap())
        .await
        .unwrap();

    let profile = resolved.profile.as_ref().unwrap();
    assert_eq!(profile.subject.as_str(), "did:example:bob");
    assert_eq!(profile.display_name.as_deref(), Some("Bob WNS"));
    assert_eq!(
        profile.avatar_uri.as_deref(),
        Some("https://cdn.test/bob-wns.png")
    );
    assert_eq!(
        profile.profile_uri.as_deref(),
        Some("https://bob.awiki.test/")
    );
    assert_eq!(profile.subject_type.as_deref(), Some("person"));
    assert_eq!(profile.version_id.as_deref(), Some("profile-7"));
    assert_eq!(profile.ttl, Some(300));
    assert!(resolved.warnings.is_empty());

    let requests = server.join();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].rpc_method, "lookup");
    assert_eq!(requests[1].rpc_method, "resolve");
}

#[tokio::test]
async fn directory_resolution_ignores_mismatched_wns_profile_and_falls_back() {
    let fixture = Fixture::new();
    let server = RpcTestServer::spawn(vec![
        ExpectedRpc::new(
            "/user-service/v1/handle/rpc",
            "lookup",
            json!({ "handle": "bob.awiki.test" }),
            handle_lookup_with_mismatched_profile_value(),
        ),
        ExpectedRpc::new(
            "/user-service/v1/did/profile/rpc",
            "get_public_profile",
            json!({ "did": "did:example:bob" }),
            public_profile_value(),
        ),
        ExpectedRpc::new(
            "/user-service/v1/did/profile/rpc",
            "resolve",
            json!({ "did": "did:example:bob" }),
            json!({ "did": "did:example:bob", "status": "active" }),
        ),
    ]);
    let client = fixture.client_with_base_url("alice", server.base_url());

    let resolved = client
        .directory()
        .resolve_peer_async(PeerRef::parse("bob.awiki.test", "").unwrap())
        .await
        .unwrap();

    assert_eq!(
        resolved.profile.as_ref().unwrap().display_name.as_deref(),
        Some("Bob")
    );
    assert!(resolved
        .warnings
        .iter()
        .any(|warning| warning.contains("profile.subject_did")));

    let requests = server.join();
    assert_eq!(requests.len(), 3);
    assert_eq!(requests[1].rpc_method, "get_public_profile");
}

#[tokio::test]
async fn directory_resolution_ignores_wns_profile_without_subject_and_falls_back() {
    let fixture = Fixture::new();
    let mut lookup = handle_lookup_with_profile_value();
    lookup["profile"]
        .as_object_mut()
        .unwrap()
        .remove("subject_did");
    let server = RpcTestServer::spawn(vec![
        ExpectedRpc::new(
            "/user-service/v1/handle/rpc",
            "lookup",
            json!({ "handle": "bob.awiki.test" }),
            lookup,
        ),
        ExpectedRpc::new(
            "/user-service/v1/did/profile/rpc",
            "get_public_profile",
            json!({ "did": "did:example:bob" }),
            public_profile_value(),
        ),
        ExpectedRpc::new(
            "/user-service/v1/did/profile/rpc",
            "resolve",
            json!({ "did": "did:example:bob" }),
            json!({ "did": "did:example:bob", "status": "active" }),
        ),
    ]);
    let client = fixture.client_with_base_url("alice", server.base_url());

    let resolved = client
        .directory()
        .resolve_peer_async(PeerRef::parse("bob.awiki.test", "").unwrap())
        .await
        .unwrap();

    assert_eq!(
        resolved.profile.as_ref().unwrap().display_name.as_deref(),
        Some("Bob")
    );
    assert!(resolved
        .warnings
        .iter()
        .any(|warning| warning.contains("profile.subject_did is missing")));

    let requests = server.join();
    assert_eq!(requests.len(), 3);
    assert_eq!(requests[1].rpc_method, "get_public_profile");
}

#[tokio::test]
async fn directory_display_profile_hydration_reads_local_cache_only() {
    let fixture = Fixture::new();
    let server = RpcTestServer::spawn(vec![
        ExpectedRpc::new(
            "/user-service/v1/handle/rpc",
            "lookup",
            json!({ "handle": "bob.awiki.test" }),
            handle_lookup_with_profile_value(),
        ),
        ExpectedRpc::new(
            "/user-service/v1/did/profile/rpc",
            "resolve",
            json!({ "did": "did:example:bob" }),
            json!({ "did": "did:example:bob", "status": "active" }),
        ),
    ]);
    let client = fixture.client_with_base_url("alice", server.base_url());

    client
        .directory()
        .resolve_peer_async(PeerRef::parse("bob.awiki.test", "").unwrap())
        .await
        .unwrap();
    let requests = server.join();
    assert_eq!(requests.len(), 2);

    let hydrated = client
        .directory()
        .hydrate_display_profiles_async(DisplayProfileBatchRequest {
            peers: vec![
                PeerRef::parse("bob.awiki.test", "").unwrap(),
                PeerRef::parse("did:example:bob", "").unwrap(),
                PeerRef::parse("charlie.awiki.test", "").unwrap(),
            ],
        })
        .await
        .unwrap();

    assert_eq!(hydrated.len(), 3);
    assert!(hydrated[0].cache_hit);
    assert!(!hydrated[0].is_stale);
    assert!(!hydrated[0].legacy_fallback);
    assert_eq!(
        hydrated[0].did.as_ref().unwrap().as_str(),
        "did:example:bob"
    );
    assert_eq!(hydrated[0].display_name.as_deref(), Some("Bob WNS"));
    assert_eq!(
        hydrated[0].avatar_uri.as_deref(),
        Some("https://cdn.test/bob-wns.png")
    );
    assert_eq!(
        hydrated[0].profile_uri.as_deref(),
        Some("https://bob.awiki.test/")
    );
    assert_eq!(hydrated[0].subject_type.as_deref(), Some("person"));
    assert!(hydrated[1].cache_hit);
    assert!(!hydrated[1].is_stale);
    assert!(!hydrated[1].legacy_fallback);
    assert_eq!(hydrated[1].display_name.as_deref(), Some("Bob WNS"));
    assert!(!hydrated[2].cache_hit);
    assert!(!hydrated[2].is_stale);
    assert!(!hydrated[2].legacy_fallback);
    assert!(hydrated[2].did.is_none());
    assert_eq!(
        hydrated[2].handle.as_ref().unwrap().as_str(),
        "charlie.awiki.test"
    );
}

#[tokio::test]
async fn directory_display_profile_marks_expired_persona_cache_as_stale() {
    let fixture = Fixture::new();
    let server = RpcTestServer::spawn(vec![
        ExpectedRpc::new(
            "/user-service/v1/handle/rpc",
            "lookup",
            json!({ "handle": "bob.awiki.test" }),
            handle_lookup_with_profile_value(),
        ),
        ExpectedRpc::new(
            "/user-service/v1/did/profile/rpc",
            "resolve",
            json!({ "did": "did:example:bob" }),
            json!({ "did": "did:example:bob", "status": "active" }),
        ),
    ]);
    let client = fixture.client_with_base_url("alice", server.base_url());
    client
        .directory()
        .resolve_peer_async(PeerRef::parse("bob.awiki.test", "").unwrap())
        .await
        .unwrap();
    assert_eq!(server.join().len(), 2);

    let connection = rusqlite::Connection::open(fixture.root.join("local/im.sqlite")).unwrap();
    connection
        .execute("UPDATE peer_profiles SET expires_at = '0'", [])
        .unwrap();
    drop(connection);

    let hydrated = client
        .directory()
        .hydrate_display_profiles_async(DisplayProfileBatchRequest {
            peers: vec![PeerRef::parse("did:example:bob", "").unwrap()],
        })
        .await
        .unwrap();
    assert_eq!(hydrated[0].display_name.as_deref(), Some("Bob WNS"));
    assert!(hydrated[0].is_stale);
    assert!(!hydrated[0].legacy_fallback);
}

#[tokio::test]
async fn directory_display_profile_does_not_restore_legacy_name_over_persona_profile() {
    let fixture = Fixture::new();
    let server = RpcTestServer::spawn(vec![
        ExpectedRpc::new(
            "/user-service/v1/handle/rpc",
            "lookup",
            json!({ "handle": "bob.awiki.test" }),
            handle_lookup_with_profile_value(),
        ),
        ExpectedRpc::new(
            "/user-service/v1/did/profile/rpc",
            "resolve",
            json!({ "did": "did:example:bob" }),
            json!({ "did": "did:example:bob", "status": "active" }),
        ),
    ]);
    let client = fixture.client_with_base_url("alice", server.base_url());
    client
        .directory()
        .resolve_peer_async(PeerRef::parse("bob.awiki.test", "").unwrap())
        .await
        .unwrap();
    assert_eq!(server.join().len(), 2);

    let connection = rusqlite::Connection::open(fixture.root.join("local/im.sqlite")).unwrap();
    connection
        .execute("UPDATE peer_profiles SET display_name = NULL", [])
        .unwrap();
    drop(connection);

    let hydrated = client
        .directory()
        .hydrate_display_profiles_async(DisplayProfileBatchRequest {
            peers: vec![PeerRef::parse("did:example:bob", "").unwrap()],
        })
        .await
        .unwrap();
    assert_eq!(hydrated[0].display_name, None);
    assert_eq!(
        hydrated[0].handle.as_ref().map(Handle::as_str),
        Some("bob.awiki.test")
    );
    assert!(!hydrated[0].legacy_fallback);
}

#[tokio::test]
async fn directory_display_profile_marks_contact_only_projection_as_legacy_fallback() {
    let fixture = Fixture::new();
    let client = fixture.client("alice");
    client
        .directory()
        .save_contact_async(SaveContactRequest {
            peer: PeerRef::parse("did:example:legacy", "").unwrap(),
            did: Some(Did::parse("did:example:legacy").unwrap()),
            handle: Some(Handle::parse("legacy.awiki.test", "").unwrap()),
            display_name: Some("Legacy Name".to_string()),
            relationship: Some("friend".to_string()),
            note: Some("Local note".to_string()),
        })
        .await
        .unwrap();

    let hydrated = client
        .directory()
        .hydrate_display_profiles_async(DisplayProfileBatchRequest {
            peers: vec![PeerRef::parse("did:example:legacy", "").unwrap()],
        })
        .await
        .unwrap();
    assert_eq!(hydrated[0].display_name.as_deref(), Some("Legacy Name"));
    assert!(hydrated[0].legacy_fallback);
    assert!(!hydrated[0].is_stale);
}

#[tokio::test]
async fn directory_display_profile_hydration_prefers_persona_profile_without_contact() {
    let fixture = Fixture::new();
    let server = RpcTestServer::spawn(vec![
        ExpectedRpc::new(
            "/user-service/v1/handle/rpc",
            "lookup",
            json!({ "handle": "bob.awiki.test" }),
            handle_lookup_with_profile_value(),
        ),
        ExpectedRpc::new(
            "/user-service/v1/did/profile/rpc",
            "resolve",
            json!({ "did": "did:example:bob" }),
            json!({ "did": "did:example:bob", "status": "active" }),
        ),
    ]);
    let client = fixture.client_with_base_url("alice", server.base_url());

    client
        .directory()
        .resolve_peer_async(PeerRef::parse("bob.awiki.test", "").unwrap())
        .await
        .unwrap();
    assert_eq!(server.join().len(), 2);

    let connection = rusqlite::Connection::open(fixture.root.join("local/im.sqlite")).unwrap();
    connection.execute("DELETE FROM contacts", []).unwrap();
    assert_eq!(
        connection
            .query_row("SELECT COUNT(*) FROM peer_profiles", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
        1
    );
    drop(connection);

    let hydrated = client
        .directory()
        .hydrate_display_profiles_async(DisplayProfileBatchRequest {
            peers: vec![PeerRef::parse("did:example:bob", "").unwrap()],
        })
        .await
        .unwrap();

    assert_eq!(hydrated.len(), 1);
    assert!(hydrated[0].cache_hit);
    assert_eq!(hydrated[0].display_name.as_deref(), Some("Bob WNS"));
    assert_eq!(
        hydrated[0].handle.as_ref().map(Handle::as_str),
        Some("bob.awiki.test")
    );
}

#[cfg(feature = "blocking")]
#[test]
fn directory_display_profile_hydration_blocking_prefers_persona_profile_without_contact() {
    let fixture = Fixture::new();
    let server = RpcTestServer::spawn(vec![
        ExpectedRpc::new(
            "/user-service/v1/handle/rpc",
            "lookup",
            json!({ "handle": "bob.awiki.test" }),
            handle_lookup_with_profile_value(),
        ),
        ExpectedRpc::new(
            "/user-service/v1/did/profile/rpc",
            "resolve",
            json!({ "did": "did:example:bob" }),
            json!({ "did": "did:example:bob", "status": "active" }),
        ),
    ]);
    let client = fixture.client_with_base_url("alice", server.base_url());

    client
        .directory()
        .resolve_peer(PeerRef::parse("bob.awiki.test", "").unwrap())
        .unwrap();
    assert_eq!(server.join().len(), 2);

    let connection = rusqlite::Connection::open(fixture.root.join("local/im.sqlite")).unwrap();
    connection.execute("DELETE FROM contacts", []).unwrap();
    drop(connection);

    let hydrated = client
        .directory()
        .hydrate_display_profiles(DisplayProfileBatchRequest {
            peers: vec![PeerRef::parse("did:example:bob", "").unwrap()],
        })
        .unwrap();

    assert_eq!(hydrated.len(), 1);
    assert!(hydrated[0].cache_hit);
    assert_eq!(hydrated[0].display_name.as_deref(), Some("Bob WNS"));
    assert_eq!(
        hydrated[0].handle.as_ref().map(Handle::as_str),
        Some("bob.awiki.test")
    );
}

#[tokio::test]
async fn messages_history_with_handle_merges_local_handle_history_in_im_core() {
    let fixture = Fixture::new();
    let old_did = "did:example:bob-old";
    seed_contact_binding(
        &fixture,
        "did:example:alice",
        old_did,
        "bob",
        "2026-05-20T00:00:00Z",
    );
    let server = RpcTestServer::spawn(vec![
        ExpectedRpc::new(
            "/user-service/v1/handle/rpc",
            "lookup",
            json!({ "handle": "bob.awiki.test" }),
            handle_lookup_value(),
        ),
        ExpectedRpc::new(
            "/im/rpc",
            "direct.get_history",
            json!({
                "body": {
                    "user_did": "did:example:alice",
                    "peer_did": "did:example:bob",
                    "limit": 5,
                }
            }),
            json!({
                "messages": [{
                    "id": "msg-history-1",
                    "sender_did": "did:example:bob",
                    "receiver_did": "did:example:alice",
                    "content": "hello",
                    "content_type": "text/plain",
                    "sent_at": "2026-05-21T00:00:00Z"
                }],
                "source": "remote_http"
            }),
        ),
    ]);
    let base_url = server.base_url().to_owned();
    let client = fixture.client_async_with_base_url("alice", &base_url).await;

    let page = client
        .messages()
        .history_with_metadata_async(
            ThreadRef::Direct(PeerRef::parse("bob.awiki.test", "").unwrap()),
            HistoryQuery {
                limit: PageLimit(5),
                cursor: None,
                inbox_history_options: None,
            },
        )
        .await
        .unwrap();

    assert_eq!(page.source.as_deref(), Some("remote_http"));
    assert_eq!(
        page.resolved_dids
            .iter()
            .map(Did::as_str)
            .collect::<Vec<_>>(),
        vec!["did:example:bob", old_did]
    );
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].id.as_str(), "msg-history-1");
    let requests = server.join();
    assert_eq!(requests.len(), 2);
}

#[tokio::test]
async fn directory_service_reads_public_profile_without_resolve_call() {
    let fixture = Fixture::new();
    let server = RpcTestServer::spawn(vec![
        ExpectedRpc::new(
            "/user-service/v1/handle/rpc",
            "lookup",
            json!({ "handle": "bob.awiki.test" }),
            handle_lookup_value(),
        ),
        ExpectedRpc::new(
            "/user-service/v1/did/profile/rpc",
            "get_public_profile",
            json!({ "did": "did:example:bob" }),
            public_profile_value(),
        ),
        ExpectedRpc::new(
            "/user-service/v1/did/profile/rpc",
            "get_public_profile",
            json!({ "did": "did:example:bob" }),
            public_profile_value(),
        ),
    ]);
    let client = fixture.client_with_base_url("alice", server.base_url());

    let handle_profile = client
        .directory()
        .public_profile_async(IdentitySubject::Handle(
            Handle::parse("bob.awiki.test", "").unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(handle_profile.did.as_str(), "did:example:bob");
    assert_eq!(
        handle_profile.handle.as_ref().unwrap().as_str(),
        "bob.awiki.test"
    );
    assert_eq!(handle_profile.profile.display_name.as_deref(), Some("Bob"));
    assert!(matches!(handle_profile.subject, IdentitySubject::Handle(_)));

    let did_profile = client
        .directory()
        .public_profile_async(IdentitySubject::Did(Did::parse("did:example:bob").unwrap()))
        .await
        .unwrap();
    assert_eq!(did_profile.did.as_str(), "did:example:bob");
    assert_eq!(did_profile.profile.subject.as_str(), "did:example:bob");
    assert_eq!(did_profile.profile.display_name.as_deref(), Some("Bob"));

    let requests = server.join();
    assert_eq!(requests.len(), 3);
    assert_eq!(requests[0].rpc_method, "lookup");
    assert_eq!(requests[1].rpc_method, "get_public_profile");
    assert_eq!(requests[2].rpc_method, "get_public_profile");
}

#[tokio::test]
async fn directory_bridge_resolves_handle_without_sync_projection() {
    let fixture = Fixture::new();
    let client = fixture.client("alice");
    let result = awiki_im_core::compat::directory::resolve_peer_with_bridge(
        &client,
        PeerRef::parse("bob.awiki.test", "").unwrap(),
        DirectoryTransport::handle_success(),
    )
    .unwrap();

    assert_eq!(result.resolution.did.as_str(), "did:example:bob");
    assert_eq!(
        result.resolution.handle.as_ref().unwrap().as_str(),
        "bob.awiki.test"
    );
    assert_eq!(
        result
            .resolution
            .profile
            .as_ref()
            .unwrap()
            .display_name
            .as_deref(),
        Some("Bob")
    );
    assert!(result.resolution.warnings.is_empty());
    assert_eq!(result.lookup.as_ref().unwrap()["did"], "did:example:bob");
    assert_eq!(result.public_profile.as_ref().unwrap()["nick_name"], "Bob");
    assert_eq!(result.resolve.as_ref().unwrap()["did"], "did:example:bob");

    #[cfg(not(feature = "blocking"))]
    assert!(matches!(
        client.directory().contacts(ContactListQuery {
            limit: Some(PageLimit(10)),
        }),
        Err(ImError::UnsupportedCapability { capability }) if capability == "sync-directory-contacts"
    ));
}

#[test]
fn directory_bridge_resolves_did_and_keeps_nonfatal_profile_warnings() {
    let fixture = Fixture::new();
    let client = fixture.client("alice");
    let result = awiki_im_core::compat::directory::resolve_peer_with_bridge(
        &client,
        PeerRef::parse("did:example:bob", "").unwrap(),
        DirectoryTransport::did_profile_warning(),
    )
    .unwrap();

    assert_eq!(result.resolution.did.as_str(), "did:example:bob");
    assert_eq!(
        result.resolution.handle.as_ref().unwrap().as_str(),
        "bob.awiki.test"
    );
    assert!(result.resolution.profile.is_none());
    assert!(result
        .resolution
        .warnings
        .iter()
        .any(|warning| warning.contains("Public profile lookup failed")));
    assert!(result.public_profile.is_none());
}

#[test]
fn directory_bridge_lookup_handle_maps_result() {
    let fixture = Fixture::new();
    let client = fixture.client("alice");
    let result = awiki_im_core::compat::directory::lookup_handle_with_bridge(
        &client,
        Handle::parse("bob.awiki.test", "").unwrap(),
        DirectoryTransport::handle_success(),
    )
    .unwrap();

    assert_eq!(result.did.as_str(), "did:example:bob");
    assert_eq!(result.handle.as_str(), "bob.awiki.test");
    assert_eq!(result.domain.as_deref(), Some("awiki.test"));
}

#[test]
fn directory_service_validates_contact_query_before_stub() {
    let fixture = Fixture::new();
    let client = fixture.client("alice");

    let contacts = client.directory().contacts(ContactListQuery {
        limit: Some(PageLimit(0)),
    });
    assert!(matches!(
        contacts,
        Err(ImError::InvalidInput { field: Some(field), .. }) if field == "limit"
    ));
}

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = unique_temp_root();
        let identities = root.join("identities");
        fs::create_dir_all(&identities).unwrap();
        fs::create_dir_all(root.join("local")).unwrap();
        fs::write(identities.join("default"), "alice\n").unwrap();
        fs::write(
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
        write_identity_runtime(&identities, "alice", "did:example:alice");
        Self { root }
    }

    fn client(&self, alias: &str) -> ImClient {
        self.core()
            .client(IdentitySelector::LocalAlias(alias.to_string()))
            .unwrap()
    }

    fn client_with_base_url(&self, alias: &str, base_url: &str) -> ImClient {
        self.core_with_base_url(base_url)
            .client(IdentitySelector::LocalAlias(alias.to_string()))
            .unwrap()
    }

    async fn client_async_with_base_url(&self, alias: &str, base_url: &str) -> ImClient {
        self.core_async_with_base_url(base_url)
            .await
            .client_async(IdentitySelector::LocalAlias(alias.to_string()))
            .await
            .unwrap()
    }

    fn core(&self) -> ImCore {
        self.core_with_base_url("https://example.test")
    }

    fn core_with_base_url(&self, base_url: &str) -> ImCore {
        ImCore::new(
            ImCoreConfig {
                service_base_url: ServiceEndpoint::parse(base_url).unwrap(),
                did_domain: "awiki.test".to_string(),
                client_version_info: None,
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

    async fn core_async_with_base_url(&self, base_url: &str) -> ImCore {
        ImCore::open(
            ImCoreConfig {
                service_base_url: ServiceEndpoint::parse(base_url).unwrap(),
                did_domain: "awiki.test".to_string(),
                client_version_info: None,
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
        .await
        .unwrap()
    }
}

fn write_identity_runtime(identities: &std::path::Path, alias: &str, did: &str) {
    let identity_dir = identities.join(alias);
    fs::create_dir_all(&identity_dir).unwrap();
    fs::write(
        identity_dir.join("identity.json"),
        serde_json::to_vec_pretty(&json!({
            "did": did,
            "unique_id": format!("{alias}-id"),
            "name": "Alice",
            "handle": format!("{alias}.awiki.test"),
            "full_handle": format!("{alias}.awiki.test"),
        }))
        .unwrap(),
    )
    .unwrap();
    let bundle = anp::authentication::create_did_wba_document(
        "awiki.test",
        anp::authentication::DidDocumentOptions {
            path_segments: vec!["user".to_string()],
            domain: Some("awiki.test".to_string()),
            challenge: Some(format!("phase2-directory-{alias}")),
            ..anp::authentication::DidDocumentOptions::default()
        },
    )
    .unwrap();
    fs::write(
        identity_dir.join("did.json"),
        serde_json::to_vec_pretty(&bundle.did_document).unwrap(),
    )
    .unwrap();
    fs::write(
        identity_dir.join("private.key"),
        bundle.private_key_pem("key-1").unwrap(),
    )
    .unwrap();
    fs::write(
        identity_dir.join("auth.json"),
        format!(r#"{{"jwt_token":"test-token-for-{alias}"}}"#),
    )
    .unwrap();
}

fn seed_contact_binding(
    fixture: &Fixture,
    owner_did: &str,
    peer_did: &str,
    handle: &str,
    seen_at: &str,
) {
    let sqlite_path = fixture.root.join("local").join("im.sqlite");
    if let Some(parent) = sqlite_path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    let mut connection = rusqlite::Connection::open(sqlite_path).unwrap();
    awiki_im_core::compat::local_state::ensure_schema(&connection).unwrap();
    awiki_im_core::compat::directory::upsert_contact(
        &mut connection,
        awiki_im_core::compat::directory::ContactRecord {
            owner_identity_id: "alice-id".to_owned(),
            owner_did: owner_did.to_owned(),
            did: peer_did.to_owned(),
            handle: handle.to_owned(),
            messaged: Some(true),
            first_seen_at: seen_at.to_owned(),
            last_seen_at: seen_at.to_owned(),
            credential_name: "alice-id".to_owned(),
            ..awiki_im_core::compat::directory::ContactRecord::default()
        },
    )
    .unwrap();
}

struct ProfileSession {
    subject: Did,
}

impl awiki_im_core::compat::profile::BridgeProfileSessionProvider for ProfileSession {
    fn ensure_profile_session(&self) -> ImResult<SessionBundle> {
        Ok(SessionBundle {
            subject: self.subject.clone(),
            scope: AuthScope::UserProfile,
            expires_at: None,
            refreshed: false,
            bearer_token: None,
        })
    }
}

#[derive(Default)]
struct ProfileTransport {
    requests: Vec<(String, String, Value)>,
}

impl awiki_im_core::compat::profile::BridgeProfileAuthenticatedRpcTransport for ProfileTransport {
    fn authenticated_rpc(
        &mut self,
        endpoint: &str,
        method: &str,
        params: Value,
    ) -> ImResult<Value> {
        self.requests
            .push((endpoint.to_string(), method.to_string(), params.clone()));
        assert_eq!(endpoint, "/user-service/v1/did/profile/rpc");
        assert_eq!(method, "get_me");
        assert_eq!(params, json!({}));
        Ok(json!({
            "did": "did:example:alice",
            "handle": "alice.awiki.test",
            "nick_name": "Alice Remote",
            "bio": "Rust port",
            "tags": ["rust", "cli"],
            "profile_md": "## Alice",
            "avatar_url": "https://cdn.test/a.png",
            "updated_at": "2026-05-21T00:00:00Z",
            "metadata": {
                "source": "profile-service",
            },
        }))
    }
}

#[derive(Default)]
struct ProfileUpdateTransport;

impl awiki_im_core::compat::profile::BridgeProfileAuthenticatedRpcTransport
    for ProfileUpdateTransport
{
    fn authenticated_rpc(
        &mut self,
        endpoint: &str,
        method: &str,
        params: Value,
    ) -> ImResult<Value> {
        assert_eq!(endpoint, "/user-service/v1/did/profile/rpc");
        assert_eq!(method, "update_me");
        assert_eq!(
            params,
            json!({
                "nick_name": "Alice Updated",
                "bio": "Rust port",
                "tags": ["rust", "cli"],
                "profile_md": "## Alice",
            })
        );
        Ok(json!({
            "did": "did:example:alice",
            "handle": "alice.awiki.test",
            "nick_name": "Alice Updated",
            "bio": "Rust port",
            "tags": ["rust", "cli"],
            "profile_md": "## Alice",
        }))
    }
}

struct DirectoryTransport {
    mode: DirectoryTransportMode,
    calls: Vec<(String, String, Value)>,
}

enum DirectoryTransportMode {
    HandleSuccess,
    DidProfileWarning,
}

impl DirectoryTransport {
    fn handle_success() -> Self {
        Self {
            mode: DirectoryTransportMode::HandleSuccess,
            calls: Vec::new(),
        }
    }

    fn did_profile_warning() -> Self {
        Self {
            mode: DirectoryTransportMode::DidProfileWarning,
            calls: Vec::new(),
        }
    }
}

impl awiki_im_core::compat::directory::BridgeDirectoryRpcTransport for DirectoryTransport {
    fn rpc(&mut self, endpoint: &str, method: &str, params: Value) -> ImResult<Value> {
        self.calls
            .push((endpoint.to_string(), method.to_string(), params.clone()));
        match (&self.mode, method) {
            (DirectoryTransportMode::HandleSuccess, "lookup") => {
                assert_eq!(endpoint, "/user-service/v1/handle/rpc");
                assert_eq!(params, json!({ "handle": "bob.awiki.test" }));
                Ok(handle_lookup_value())
            }
            (DirectoryTransportMode::HandleSuccess, "get_public_profile") => {
                assert_eq!(endpoint, "/user-service/v1/did/profile/rpc");
                assert_eq!(params, json!({ "did": "did:example:bob" }));
                Ok(public_profile_value())
            }
            (DirectoryTransportMode::HandleSuccess, "resolve") => {
                assert_eq!(endpoint, "/user-service/v1/did/profile/rpc");
                assert_eq!(params, json!({ "did": "did:example:bob" }));
                Ok(json!({ "did": "did:example:bob", "status": "active" }))
            }
            (DirectoryTransportMode::DidProfileWarning, "resolve") => {
                assert_eq!(params, json!({ "did": "did:example:bob" }));
                Ok(json!({ "did": "did:example:bob", "status": "active" }))
            }
            (DirectoryTransportMode::DidProfileWarning, "lookup") => {
                assert_eq!(params, json!({ "did": "did:example:bob" }));
                Ok(handle_lookup_value())
            }
            (DirectoryTransportMode::DidProfileWarning, "get_public_profile") => {
                Err(ImError::Service {
                    status_code: None,
                    code: Some("-32002".to_string()),
                    message: "profile missing".to_string(),
                    data: None,
                })
            }
            (_, method) => Err(ImError::Internal {
                message: format!("unexpected directory method {method}"),
            }),
        }
    }
}

fn handle_lookup_value() -> Value {
    json!({
        "handle": "bob",
        "full_handle": "bob.awiki.test",
        "did": "did:example:bob",
        "user_id": "user-bob",
        "domain": "awiki.test",
        "status": "active",
    })
}

fn handle_lookup_with_profile_value() -> Value {
    json!({
        "handle": "bob.awiki.test",
        "did": "did:example:bob",
        "user_id": "user-bob",
        "domain": "awiki.test",
        "status": "active",
        "profile": {
            "type": "DIDSubjectProfile",
            "subject_did": "did:example:bob",
            "subject_type": "person",
            "handle": "bob.awiki.test",
            "display_name": "Bob WNS",
            "description": "WNS projection",
            "avatar_uri": "https://cdn.test/bob-wns.png",
            "profile_uri": "https://bob.awiki.test/",
            "updated": "2026-05-21T00:00:00Z",
            "versionId": "profile-7",
            "ttl": 300
        }
    })
}

fn handle_lookup_with_mismatched_profile_value() -> Value {
    json!({
        "handle": "bob.awiki.test",
        "did": "did:example:bob",
        "user_id": "user-bob",
        "domain": "awiki.test",
        "status": "active",
        "profile": {
            "type": "DIDSubjectProfile",
            "subject_did": "did:example:mallory",
            "subject_type": "person",
            "handle": "bob.awiki.test",
            "display_name": "Mallory"
        }
    })
}

fn federated_handle_lookup_value() -> Value {
    json!({
        "handle": "agent",
        "full_handle": "agent.remote.example",
        "did": "did:wba:remote.example:user:agent:e1",
        "user_id": "federated:handle:remote-agent",
        "domain": "remote.example",
        "status": "active",
        "profile": {
            "type": "DIDSubjectProfile",
            "subject_did": "did:wba:remote.example:user:agent:e1",
            "subject_type": "agent",
            "handle": "agent.remote.example",
            "display_name": "Remote Agent",
            "description": "Federated profile",
            "avatar_uri": "https://remote.example/avatar.png",
            "profile_uri": "https://agent.remote.example/"
        }
    })
}

fn public_profile_value() -> Value {
    json!({
        "did": "did:example:bob",
        "handle": "bob.awiki.test",
        "nick_name": "Bob",
        "bio": "Directory profile",
        "tags": ["directory"],
    })
}

fn unique_temp_root() -> PathBuf {
    static NEXT_ROOT: AtomicU64 = AtomicU64::new(1);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let sequence = NEXT_ROOT.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "im-core-phase2-{}-{nanos}-{sequence}",
        std::process::id()
    ))
}

struct RpcTestServer {
    base_url: String,
    handle: thread::JoinHandle<Vec<CapturedRpc>>,
}

impl RpcTestServer {
    fn spawn(expected: Vec<ExpectedRpc>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let base_url = format!("http://{}", listener.local_addr().unwrap());
        let handle = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut captured = Vec::new();
            for expected in expected {
                let mut stream = accept_before_deadline(&listener, deadline);
                let request = read_rpc_request(&mut stream);
                assert_eq!(request.path, expected.path);
                assert_eq!(request.rpc_method, expected.rpc_method);
                if request.rpc_method == "direct.get_history" {
                    assert_eq!(request.params["body"], expected.params["body"]);
                    assert_eq!(request.params["meta"]["sender_did"], "did:example:alice");
                    assert_eq!(request.params["meta"]["profile"], "anp.direct.local.v1");
                } else {
                    assert_eq!(request.params, expected.params);
                }
                write_rpc_response(&mut stream, request.id.clone(), expected.result);
                captured.push(request);
            }
            captured
        });
        Self { base_url, handle }
    }

    fn base_url(&self) -> &str {
        &self.base_url
    }

    fn join(self) -> Vec<CapturedRpc> {
        self.handle.join().unwrap()
    }
}

struct ExpectedRpc {
    path: String,
    rpc_method: String,
    params: Value,
    result: Value,
}

impl ExpectedRpc {
    fn new(path: &str, rpc_method: &str, params: Value, result: Value) -> Self {
        Self {
            path: path.to_string(),
            rpc_method: rpc_method.to_string(),
            params,
            result,
        }
    }
}

#[derive(Debug)]
struct CapturedRpc {
    path: String,
    rpc_method: String,
    params: Value,
    id: Value,
    authorization: Option<String>,
}

fn accept_before_deadline(listener: &TcpListener, deadline: Instant) -> TcpStream {
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                stream.set_nonblocking(false).unwrap();
                return stream;
            }
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                assert!(
                    Instant::now() < deadline,
                    "timed out waiting for RPC request"
                );
                thread::sleep(Duration::from_millis(10));
            }
            Err(err) => panic!("accept RPC request: {err}"),
        }
    }
}

fn read_rpc_request(stream: &mut TcpStream) -> CapturedRpc {
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut raw = Vec::new();
    let mut buffer = [0_u8; 4096];
    let header_end = loop {
        let count = stream.read(&mut buffer).unwrap();
        assert!(count > 0, "RPC request closed before headers");
        raw.extend_from_slice(&buffer[..count]);
        if let Some(index) = raw.windows(4).position(|window| window == b"\r\n\r\n") {
            break index;
        }
    };
    let headers_text = std::str::from_utf8(&raw[..header_end]).unwrap();
    let mut lines = headers_text.lines();
    let request_line = lines.next().unwrap();
    let mut request_parts = request_line.split_whitespace();
    assert_eq!(request_parts.next(), Some("POST"));
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
        assert!(count > 0, "RPC request closed before body");
        raw.extend_from_slice(&buffer[..count]);
    }
    let body = &raw[body_start..body_start + content_length];
    let payload: Value = serde_json::from_slice(body).unwrap();
    CapturedRpc {
        path,
        rpc_method: payload["method"].as_str().unwrap().to_string(),
        params: payload["params"].clone(),
        id: payload["id"].clone(),
        authorization: headers.get("authorization").cloned(),
    }
}

fn write_rpc_response(stream: &mut TcpStream, id: Value, result: Value) {
    let body = json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result,
    })
    .to_string();
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    )
    .unwrap();
}
