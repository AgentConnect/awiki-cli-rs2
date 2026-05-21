use std::fs;
use std::path::PathBuf;

use im_core::prelude::*;

#[test]
fn file_session_provider_ensures_session_from_runtime_auth_state() {
    let fixture = AuthFixture::new();
    fixture.write_runtime("alice", "did:example:alice", Some("token-alice"), true);
    let client = fixture.client("alice");

    let session = client.auth().ensure_session(AuthScope::Messaging).unwrap();
    assert_eq!(session.subject.as_str(), "did:example:alice");
    assert_eq!(session.scope, AuthScope::Messaging);
    assert_eq!(session.expires_at.as_deref(), Some("2026-05-21T00:00:00Z"));
    assert!(!session.refreshed);

    let status = client.auth().status().unwrap();
    assert!(status.has_session);
    assert!(!status.needs_refresh);
    assert!(status.warnings.is_empty());
}

#[test]
fn file_session_provider_reports_missing_token_without_faking_session() {
    let fixture = AuthFixture::new();
    fixture.write_runtime("alice", "did:example:alice", None, true);
    let client = fixture.client("alice");

    assert!(matches!(
        client.auth().ensure_session(AuthScope::Messaging),
        Err(ImError::AuthRequired)
    ));
    let status = client.auth().status().unwrap();
    assert!(!status.has_session);
    assert!(status.needs_refresh);
    assert!(status
        .warnings
        .iter()
        .any(|warning| warning.contains("JWT")));
}

#[test]
fn file_session_provider_respects_messaging_readiness_for_message_scopes() {
    let fixture = AuthFixture::new();
    fixture.write_runtime("alice", "did:example:alice", Some("token-alice"), false);
    let client = fixture.client("alice");

    let profile = client
        .auth()
        .ensure_session(AuthScope::UserProfile)
        .unwrap();
    assert_eq!(profile.scope, AuthScope::UserProfile);

    assert!(matches!(
        client.auth().ensure_session(AuthScope::Messaging),
        Err(ImError::IdentityNotReady { .. })
    ));
}

struct AuthFixture {
    root: PathBuf,
}

impl AuthFixture {
    fn new() -> Self {
        let root = unique_temp_root();
        fs::create_dir_all(root.join("identities")).unwrap();
        fs::write(root.join("identities").join("default"), "alice\n").unwrap();
        Self { root }
    }

    fn write_runtime(
        &self,
        alias: &str,
        did: &str,
        token: Option<&str>,
        ready_for_messaging: bool,
    ) {
        let identities = self.root.join("identities");
        fs::write(
            identities.join("registry.json"),
            format!(
                r#"{{
                  "default_identity": "{alias}",
                  "identities": [{{
                    "id": "{alias}-id",
                    "did": "{did}",
                    "local_alias": "{alias}",
                    "ready_for_auth": true,
                    "ready_for_messaging": {ready_for_messaging},
                    "missing": []
                  }}]
                }}"#
            ),
        )
        .unwrap();
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
        let token_field = token
            .map(|token| format!(r#""jwt_token":"{token}","#))
            .unwrap_or_default();
        fs::write(
            identity_dir.join("auth.json"),
            format!(r#"{{{token_field}"expires_at":"2026-05-21T00:00:00Z"}}"#),
        )
        .unwrap();
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
                    sqlite_path: self.root.join("state").join("im.sqlite"),
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

fn unique_temp_root() -> PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!(
        "im-core-auth-provider-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&path).unwrap();
    path
}
