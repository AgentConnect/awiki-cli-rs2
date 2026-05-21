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
fn directory_service_exposes_contact_and_resolution_skeleton() {
    let fixture = Fixture::new();
    let client = fixture.client("alice");
    assert_eq!(client.directory().owner_did().as_str(), "did:example:alice");

    let peer = PeerRef::parse("bob.awiki.test", "").unwrap();
    let resolved = client.directory().resolve_peer(peer.clone());
    assert!(matches!(
        resolved,
        Err(ImError::UnsupportedCapability { capability })
            if capability == "directory-resolve-peer"
    ));

    let saved = client.directory().save_contact(SaveContactRequest {
        peer: peer.clone(),
        did: Some(Did::parse("did:example:bob").unwrap()),
        handle: Some(Handle::parse("bob.awiki.test", "").unwrap()),
        display_name: Some("Bob".to_string()),
        relationship: Some("friend".to_string()),
        note: Some("Phase 2 skeleton".to_string()),
    });
    assert!(matches!(
        saved,
        Err(ImError::UnsupportedCapability { capability })
            if capability == "directory-save-contact"
    ));

    let contacts = client.directory().contacts(ContactListQuery {
        limit: Some(PageLimit(10)),
    });
    assert!(matches!(
        contacts,
        Err(ImError::UnsupportedCapability { capability }) if capability == "directory-contacts"
    ));

    let relation = client.directory().relation_status(peer);
    assert!(matches!(
        relation,
        Err(ImError::UnsupportedCapability { capability })
            if capability == "directory-relation-status"
    ));
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

fn unique_temp_root() -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("im-core-phase2-{}-{nanos}", std::process::id()))
}
