use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

use im_core::prelude::*;
use serde_json::{json, Value};

#[test]
fn identity_service_profile_uses_public_http_transport() {
    let fixture = Fixture::new();
    let server = RpcTestServer::spawn(vec![
        ExpectedRpc::new(
            "/user-service/did/profile/rpc",
            "get_me",
            json!({}),
            json!({
                "did": "did:example:alice",
                "handle": "alice.awiki.test",
                "nick_name": "Alice Remote",
                "bio": "Rust public API",
                "tags": ["sdk", "http"],
                "profile_md": "## Alice",
            }),
        ),
        ExpectedRpc::new(
            "/user-service/did/profile/rpc",
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
            }),
        ),
    ]);
    let client = fixture.client_with_base_url("alice", server.base_url());

    let profile = client.identity().profile().unwrap();
    assert_eq!(profile.subject.as_str(), "did:example:alice");
    assert_eq!(profile.display_name.as_deref(), Some("Alice Remote"));
    assert_eq!(profile.bio.as_deref(), Some("Rust public API"));
    assert_eq!(profile.tags, vec!["sdk", "http"]);

    let updated = client
        .identity()
        .update_profile(ProfilePatch {
            display_name: Some("Alice Updated".to_string()),
            bio: Some("sdk profile skeleton".to_string()),
            tags: Some(vec!["sdk".to_string()]),
            markdown: Some("# Alice".to_string()),
        })
        .unwrap();
    assert_eq!(updated.subject.as_str(), "did:example:alice");
    assert_eq!(updated.display_name.as_deref(), Some("Alice Updated"));
    assert_eq!(updated.markdown.as_deref(), Some("# Alice"));

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
    let updated = im_core::compat::profile::update_profile_with_bridge(
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

#[test]
fn directory_service_exposes_contact_store_and_resolution_api() {
    let fixture = Fixture::new();
    let server = RpcTestServer::spawn(vec![
        ExpectedRpc::new(
            "/user-service/handle/rpc",
            "lookup",
            json!({ "handle": "bob.awiki.test" }),
            handle_lookup_value(),
        ),
        ExpectedRpc::new(
            "/user-service/did/profile/rpc",
            "get_public_profile",
            json!({ "did": "did:example:bob" }),
            public_profile_value(),
        ),
        ExpectedRpc::new(
            "/user-service/did/profile/rpc",
            "resolve",
            json!({ "did": "did:example:bob" }),
            json!({ "did": "did:example:bob", "status": "active" }),
        ),
    ]);
    let client = fixture.client_with_base_url("alice", server.base_url());
    assert_eq!(client.directory().owner_did().as_str(), "did:example:alice");

    let peer = PeerRef::parse("bob.awiki.test", "").unwrap();
    let resolved = client.directory().resolve_peer(peer.clone()).unwrap();
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
fn messages_history_with_handle_merges_local_handle_history_in_im_core() {
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
            "/user-service/handle/rpc",
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
    let client = fixture.client_with_base_url("alice", server.base_url());

    let page = client
        .messages()
        .history_with_metadata(
            ThreadRef::Direct(PeerRef::parse("bob.awiki.test", "").unwrap()),
            HistoryQuery {
                limit: PageLimit(5),
                cursor: None,
            },
        )
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

#[test]
fn directory_service_reads_public_profile_without_resolve_call() {
    let fixture = Fixture::new();
    let server = RpcTestServer::spawn(vec![
        ExpectedRpc::new(
            "/user-service/handle/rpc",
            "lookup",
            json!({ "handle": "bob.awiki.test" }),
            handle_lookup_value(),
        ),
        ExpectedRpc::new(
            "/user-service/did/profile/rpc",
            "get_public_profile",
            json!({ "did": "did:example:bob" }),
            public_profile_value(),
        ),
        ExpectedRpc::new(
            "/user-service/did/profile/rpc",
            "get_public_profile",
            json!({ "did": "did:example:bob" }),
            public_profile_value(),
        ),
    ]);
    let client = fixture.client_with_base_url("alice", server.base_url());

    let handle_profile = client
        .directory()
        .public_profile(IdentitySubject::Handle(
            Handle::parse("bob.awiki.test", "").unwrap(),
        ))
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
        .public_profile(IdentitySubject::Did(Did::parse("did:example:bob").unwrap()))
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

    fn client_with_base_url(&self, alias: &str, base_url: &str) -> ImClient {
        self.core_with_base_url(base_url)
            .client(IdentitySelector::LocalAlias(alias.to_string()))
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
    im_core::compat::local_state::ensure_schema(&connection).unwrap();
    im_core::compat::directory::upsert_contact(
        &mut connection,
        im_core::compat::directory::ContactRecord {
            owner_identity_id: "alice-id".to_owned(),
            owner_did: owner_did.to_owned(),
            did: peer_did.to_owned(),
            handle: handle.to_owned(),
            messaged: Some(true),
            first_seen_at: seen_at.to_owned(),
            last_seen_at: seen_at.to_owned(),
            credential_name: "alice-id".to_owned(),
            ..im_core::compat::directory::ContactRecord::default()
        },
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
