use std::fs;
use std::path::PathBuf;

use im_core::prelude::*;

#[test]
fn identity_registry_lists_default_and_resolves_selectors() {
    let fixture = Fixture::new();
    let core = fixture.core();

    let identities = core.identities().list().unwrap();
    assert_eq!(identities.len(), 2);

    let default = core.identities().default_identity().unwrap().unwrap();
    assert_eq!(default.local_alias.as_deref(), Some("alice"));
    assert!(default.is_default);

    let alice = core
        .identities()
        .resolve(IdentitySelector::LocalAlias("alice".to_string()))
        .unwrap();
    assert_eq!(alice.did.as_str(), "did:example:alice");

    let bob = core
        .identities()
        .resolve(IdentitySelector::Did(
            Did::parse("did:example:bob").unwrap(),
        ))
        .unwrap();
    assert_eq!(bob.local_alias.as_deref(), Some("bob"));

    let by_handle = core
        .identities()
        .resolve(IdentitySelector::Handle(
            Handle::parse("bob.awiki.test", "").unwrap(),
        ))
        .unwrap();
    assert_eq!(by_handle.did.as_str(), "did:example:bob");
}

#[test]
fn plan_default_identity_change_returns_previous_and_next() {
    let fixture = Fixture::new();
    let core = fixture.core();

    let change = core
        .identities()
        .plan_default_identity_change(IdentitySelector::LocalAlias("bob".to_string()))
        .unwrap();

    assert_eq!(
        change.previous.unwrap().local_alias.as_deref(),
        Some("alice")
    );
    assert_eq!(change.next.local_alias.as_deref(), Some("bob"));
    assert!(change.requires_default_identity_write);
}

#[test]
fn register_handle_returns_identity_and_default_change() {
    let fixture = Fixture::new();
    let core = fixture.core();

    let result = core
        .identities()
        .register_handle(RegisterHandleRequest {
            local_alias: Some("carol".to_string()),
            requested_handle: Handle::parse("carol.awiki.test", "").unwrap(),
            verification: VerificationInput::AlreadyVerified,
            invite_code: None,
            profile: InitialProfile {
                display_name: Some("Carol".to_string()),
                avatar_url: None,
            },
            make_default: true,
        })
        .unwrap();

    assert_eq!(result.state, HandleRegistrationState::Registered);
    assert_eq!(result.method, RegistrationMethod::AlreadyVerified);
    assert_eq!(result.handle.as_str(), "carol.awiki.test");
    let identity = result.identity.unwrap();
    assert_eq!(identity.local_alias.as_deref(), Some("carol"));
    assert_eq!(identity.handle.unwrap().as_str(), "carol.awiki.test");
    assert_eq!(identity.display_name.as_deref(), Some("Carol"));
    assert!(identity.readiness.ready_for_auth);
    assert!(result.default_identity_change.is_some());
}

#[test]
fn register_phone_without_otp_returns_pending_otp_state() {
    let fixture = Fixture::new();
    let core = fixture.core();

    let result = core
        .identities()
        .register_handle(RegisterHandleRequest {
            local_alias: Some("carol".to_string()),
            requested_handle: Handle::parse("carol.awiki.test", "").unwrap(),
            verification: VerificationInput::Phone {
                phone: "+15551234567".to_string(),
                otp: None,
            },
            invite_code: Some("invite-1".to_string()),
            profile: InitialProfile {
                display_name: Some("Carol".to_string()),
                avatar_url: None,
            },
            make_default: true,
        })
        .unwrap();

    assert_eq!(result.state, HandleRegistrationState::OtpSent);
    assert_eq!(result.method, RegistrationMethod::Phone);
    assert_eq!(result.handle.as_str(), "carol.awiki.test");
    assert!(result.identity.is_none());
    assert!(result.default_identity_change.is_none());
}

#[test]
fn register_email_without_wait_returns_email_sent_state() {
    let fixture = Fixture::new();
    let core = fixture.core();

    let result = core
        .identities()
        .register_handle(RegisterHandleRequest {
            local_alias: Some("carol".to_string()),
            requested_handle: Handle::parse("carol.awiki.test", "").unwrap(),
            verification: VerificationInput::Email {
                email: "carol@example.test".to_string(),
                wait_for_verification: false,
            },
            invite_code: None,
            profile: InitialProfile {
                display_name: Some("Carol".to_string()),
                avatar_url: None,
            },
            make_default: true,
        })
        .unwrap();

    assert_eq!(result.state, HandleRegistrationState::EmailSent);
    assert_eq!(result.method, RegistrationMethod::Email);
    assert_eq!(result.handle.as_str(), "carol.awiki.test");
    assert!(result.identity.is_none());
    assert!(result.default_identity_change.is_none());
}

#[test]
fn auth_service_returns_stable_structures() {
    let fixture = Fixture::new();
    let core = fixture.core();
    let client = core
        .client(IdentitySelector::LocalAlias("alice".to_string()))
        .unwrap();

    let login = client.auth().login().unwrap();
    assert_eq!(login.subject.as_str(), "did:example:alice");
    assert_eq!(login.scope, AuthScope::UserProfile);

    let ensured = client.auth().ensure_session(AuthScope::Messaging).unwrap();
    assert_eq!(ensured.subject.as_str(), "did:example:alice");
    assert_eq!(ensured.scope, AuthScope::Messaging);

    let status = client.auth().status().unwrap();
    assert_eq!(status.subject.as_str(), "did:example:alice");
    assert!(status.has_session);
    assert!(!status.needs_refresh);
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
              "identities": [
                {
                  "id": "alice-id",
                  "did": "did:example:alice",
                  "handle": "alice.awiki.test",
                  "display_name": "Alice",
                  "local_alias": "alice",
                  "device_id": "device-a",
                  "ready_for_auth": true,
                  "ready_for_messaging": true,
                  "missing": []
                },
                {
                  "id": "bob-id",
                  "did": "did:example:bob",
                  "handle": "bob.awiki.test",
                  "display_name": "Bob",
                  "local_alias": "bob",
                  "ready_for_auth": true,
                  "ready_for_messaging": true,
                  "missing": []
                }
              ]
            }"#,
        )
        .unwrap();
        write_identity_runtime(
            &identities,
            "alice",
            "did:example:alice",
            "2026-05-21T00:00:00Z",
        );
        write_identity_runtime(
            &identities,
            "bob",
            "did:example:bob",
            "2026-05-21T00:00:00Z",
        );
        Self { root }
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

fn write_identity_runtime(identities: &std::path::Path, alias: &str, did: &str, expires_at: &str) {
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
        format!(r#"{{"jwt_token":"test-token-for-{alias}","expires_at":"{expires_at}"}}"#),
    )
    .unwrap();
}

fn unique_temp_root() -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("im-core-phase1c-{}-{nanos}", std::process::id()))
}
