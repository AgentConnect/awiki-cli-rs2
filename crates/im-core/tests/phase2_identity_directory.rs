use std::fs;
use std::path::PathBuf;

use im_core::prelude::*;
use serde_json::{json, Value};

#[test]
fn identity_service_profile_requires_runtime_transport_after_session() {
    let fixture = Fixture::new();
    let client = fixture.client("alice");

    let profile = client.identity().profile();
    assert!(matches!(
        profile,
        Err(ImError::TransportUnavailable { detail })
            if detail.contains("get_me") && detail.contains("/user-service/did/profile/rpc")
    ));

    let updated = client.identity().update_profile(ProfilePatch {
        display_name: Some("Alice Updated".to_string()),
        bio: Some("sdk profile skeleton".to_string()),
        tags: Some(vec!["sdk".to_string()]),
        markdown: Some("# Alice".to_string()),
    });
    assert!(matches!(
        updated,
        Err(ImError::TransportUnavailable { detail })
            if detail.contains("update_me") && detail.contains("/user-service/did/profile/rpc")
    ));
}

#[test]
fn identity_profile_bridge_maps_get_me_result_to_sdk_profile() {
    let fixture = Fixture::new();
    let client = fixture.client("alice");
    let result = im_core::compat::profile::read_self_profile_with_bridge(
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
        result.profile.updated_at.as_deref(),
        Some("2026-05-21T00:00:00Z")
    );
    assert_eq!(
        result.profile.metadata,
        vec![ProfileAttribute {
            key: "source".to_string(),
            value: "profile-service".to_string(),
        }]
    );
    assert_eq!(result.raw["nick_name"], "Alice Remote");
}

#[test]
fn identity_profile_bridge_updates_current_profile() {
    let fixture = Fixture::new();
    let client = fixture.client("alice");
    let result = im_core::compat::profile::update_profile_with_bridge(
        &client,
        ProfilePatch {
            display_name: Some(" Alice Updated ".to_string()),
            bio: Some(" Rust port ".to_string()),
            tags: Some(vec!["rust".to_string(), "cli".to_string()]),
            markdown: Some("## Alice".to_string()),
        },
        ProfileSession {
            subject: client.did().clone(),
        },
        ProfileUpdateTransport::default(),
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
}

#[test]
fn identity_profile_update_empty_patch_does_not_call_transport() {
    let fixture = Fixture::new();
    let client = fixture.client("alice");
    let updated = im_core::compat::profile::update_profile_with_bridge(
        &client,
        ProfilePatch::default(),
        ProfileSession {
            subject: client.did().clone(),
        },
        ProfileUpdateTransport::default(),
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

#[test]
fn directory_service_exposes_contact_store_and_resolution_api() {
    let fixture = Fixture::new();
    let client = fixture.client("alice");
    assert_eq!(client.directory().owner_did().as_str(), "did:example:alice");

    let peer = PeerRef::parse("bob.awiki.test", "").unwrap();
    let resolved = client.directory().resolve_peer(peer.clone());
    assert!(matches!(
        resolved,
        Err(ImError::TransportUnavailable { detail })
            if detail.contains("lookup") && detail.contains("/user-service/handle/rpc")
    ));

    let saved = client
        .directory()
        .save_contact(SaveContactRequest {
            peer: peer.clone(),
            did: Some(Did::parse("did:example:bob").unwrap()),
            handle: Some(Handle::parse("bob.awiki.test", "").unwrap()),
            display_name: Some("Bob".to_string()),
            relationship: Some("friend".to_string()),
            note: Some("Phase 2 contact".to_string()),
        })
        .unwrap();
    assert_eq!(saved.did.as_str(), "did:example:bob");
    assert_eq!(saved.handle.as_ref().unwrap().as_str(), "bob.awiki.test");
    assert_eq!(saved.display_name.as_deref(), Some("Bob"));
    assert_eq!(saved.relationship.as_deref(), Some("friend"));
    assert_eq!(saved.note.as_deref(), Some("Phase 2 contact"));

    let contacts = client
        .directory()
        .contacts(ContactListQuery {
            limit: Some(PageLimit(10)),
        })
        .unwrap();
    assert_eq!(contacts.items.len(), 1);
    assert_eq!(contacts.items[0].did.as_str(), "did:example:bob");
    assert!(!contacts.has_more);

    let relation = client.directory().relation_status(peer).unwrap();
    assert_eq!(relation.did.as_ref().unwrap().as_str(), "did:example:bob");
    assert!(relation.is_contact);
    assert_eq!(relation.relationship.as_deref(), Some("friend"));
    assert!(!relation.followed);
    assert!(!relation.messaged);
}

#[test]
fn directory_bridge_resolves_handle_with_public_profile_projection() {
    let fixture = Fixture::new();
    let client = fixture.client("alice");
    let result = im_core::compat::directory::resolve_peer_with_bridge(
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

    let contacts = client
        .directory()
        .contacts(ContactListQuery {
            limit: Some(PageLimit(10)),
        })
        .unwrap();
    assert_eq!(contacts.items.len(), 1);
    assert_eq!(contacts.items[0].did.as_str(), "did:example:bob");
    assert_eq!(contacts.items[0].display_name.as_deref(), Some("Bob"));
}

#[test]
fn directory_bridge_resolves_did_and_keeps_nonfatal_profile_warnings() {
    let fixture = Fixture::new();
    let client = fixture.client("alice");
    let result = im_core::compat::directory::resolve_peer_with_bridge(
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
    let result = im_core::compat::directory::lookup_handle_with_bridge(
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

    fn core(&self) -> ImCore {
        ImCore::new(
            ImCoreConfig {
                service_base_url: ServiceEndpoint::parse("https://example.test").unwrap(),
                did_domain: "awiki.test".to_string(),
                user_service_endpoint: None,
                message_service_endpoint: None,
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

fn write_identity_runtime(identities: &std::path::Path, alias: &str, did: &str) {
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
        format!(r#"{{"jwt_token":"test-token-for-{alias}"}}"#),
    )
    .unwrap();
}

struct ProfileSession {
    subject: Did,
}

impl im_core::compat::profile::BridgeProfileSessionProvider for ProfileSession {
    fn ensure_profile_session(&self) -> ImResult<SessionBundle> {
        Ok(SessionBundle {
            subject: self.subject.clone(),
            scope: AuthScope::UserProfile,
            expires_at: None,
            refreshed: false,
        })
    }
}

#[derive(Default)]
struct ProfileTransport {
    requests: Vec<(String, String, Value)>,
}

impl im_core::compat::profile::BridgeProfileAuthenticatedRpcTransport for ProfileTransport {
    fn authenticated_rpc(
        &mut self,
        endpoint: &str,
        method: &str,
        params: Value,
    ) -> ImResult<Value> {
        self.requests
            .push((endpoint.to_string(), method.to_string(), params.clone()));
        assert_eq!(endpoint, "/user-service/did/profile/rpc");
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

impl im_core::compat::profile::BridgeProfileAuthenticatedRpcTransport for ProfileUpdateTransport {
    fn authenticated_rpc(
        &mut self,
        endpoint: &str,
        method: &str,
        params: Value,
    ) -> ImResult<Value> {
        assert_eq!(endpoint, "/user-service/did/profile/rpc");
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

impl im_core::compat::directory::BridgeDirectoryRpcTransport for DirectoryTransport {
    fn rpc(&mut self, endpoint: &str, method: &str, params: Value) -> ImResult<Value> {
        self.calls
            .push((endpoint.to_string(), method.to_string(), params.clone()));
        match (&self.mode, method) {
            (DirectoryTransportMode::HandleSuccess, "lookup") => {
                assert_eq!(endpoint, "/user-service/handle/rpc");
                assert_eq!(params, json!({ "handle": "bob.awiki.test" }));
                Ok(handle_lookup_value())
            }
            (DirectoryTransportMode::HandleSuccess, "get_public_profile") => {
                assert_eq!(endpoint, "/user-service/did/profile/rpc");
                assert_eq!(params, json!({ "did": "did:example:bob" }));
                Ok(public_profile_value())
            }
            (DirectoryTransportMode::HandleSuccess, "resolve") => {
                assert_eq!(endpoint, "/user-service/did/profile/rpc");
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
        "domain": "awiki.test",
        "status": "active",
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
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("im-core-phase2-{}-{nanos}", std::process::id()))
}
