use std::fs;
use std::path::PathBuf;

use im_core::prelude::*;

#[test]
fn identity_service_exposes_profile_skeleton() {
    let fixture = Fixture::new();
    let client = fixture.client("alice");

    let profile = client.identity().profile().unwrap();
    assert_eq!(profile.subject.as_str(), "did:example:alice");
    assert_eq!(
        profile.handle.as_ref().unwrap().as_str(),
        "alice.awiki.test"
    );
    assert_eq!(profile.display_name.as_deref(), Some("Alice"));
    assert!(profile.metadata.is_empty());

    let updated = client.identity().update_profile(ProfilePatch {
        display_name: Some("Alice Updated".to_string()),
        bio: Some("sdk profile skeleton".to_string()),
        tags: Some(vec!["sdk".to_string()]),
        markdown: Some("# Alice".to_string()),
    });
    assert!(matches!(
        updated,
        Err(ImError::UnsupportedCapability { capability })
            if capability == "identity-profile-update"
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

fn unique_temp_root() -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("im-core-phase2-{}-{nanos}", std::process::id()))
}
