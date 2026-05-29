use std::fs;
use std::path::{Path, PathBuf};

use im_core::prelude::*;

mod app_sandbox_paths {
    use super::*;

    #[tokio::test]
    async fn app_can_construct_core_with_explicit_paths() {
        let fixture = AppSandboxFixture::new();
        let core = fixture.core();

        let report = core.bootstrap().validate_paths().unwrap();
        assert_path_check(&report, "identity_root_dir", fixture.identities_dir(), true);
        assert_path_check(&report, "registry_path", fixture.registry_path(), true);
        assert_path_check(
            &report,
            "default_identity_path",
            fixture.default_path(),
            true,
        );
        assert_path_check(&report, "sqlite_path", fixture.sqlite_path(), false);
        assert_path_check(&report, "cache_dir", fixture.cache_dir(), true);
        assert_path_check(&report, "temp_dir", fixture.temp_dir(), true);
        assert!(report.warnings.is_empty());

        let status = core
            .bootstrap()
            .initialize_local_state_async()
            .await
            .unwrap();
        assert_eq!(
            status.sqlite_path,
            fixture.sqlite_path().display().to_string()
        );
        assert!(status.initialized);
        assert_eq!(
            status.schema_version,
            Some(im_core::compat::local_state::SCHEMA_VERSION as u32)
        );
        assert!(fixture.sqlite_path().exists());

        let identities = core.identities().list().unwrap();
        assert_eq!(identities.len(), 2);
        assert!(identities
            .iter()
            .all(|identity| identity.readiness.ready_for_auth));
    }

    #[test]
    fn sync_local_state_bootstrap_fails_closed_by_default() {
        let fixture = AppSandboxFixture::new();
        let core = fixture.core();

        let result = core.bootstrap().initialize_local_state();
        assert!(matches!(
            result,
            Err(ImError::UnsupportedCapability { capability }) if capability == "sync-bootstrap-local-state"
        ));
    }

    #[test]
    fn default_and_local_alias_selectors_resolve_app_sandbox_identities() {
        let fixture = AppSandboxFixture::new();
        let core = fixture.core();

        let default = core
            .identities()
            .resolve(IdentitySelector::Default)
            .unwrap();
        assert_eq!(default.local_alias.as_deref(), Some("alice"));
        assert_eq!(default.did.as_str(), "did:example:alice");
        assert!(default.is_default);

        let alice = core
            .identities()
            .resolve(IdentitySelector::LocalAlias("alice".to_string()))
            .unwrap();
        let bob = core
            .identities()
            .resolve(IdentitySelector::LocalAlias("bob".to_string()))
            .unwrap();
        assert_eq!(alice.local_alias.as_deref(), Some("alice"));
        assert_eq!(bob.local_alias.as_deref(), Some("bob"));
        assert_ne!(alice.id, bob.id);
        assert_ne!(alice.did, bob.did);

        let alice_client = core
            .client(IdentitySelector::LocalAlias("alice".to_string()))
            .unwrap();
        let bob_client = core
            .client(IdentitySelector::LocalAlias("bob".to_string()))
            .unwrap();
        assert_eq!(alice_client.did().as_str(), "did:example:alice");
        assert_eq!(bob_client.did().as_str(), "did:example:bob");
        assert_eq!(
            alice_client.current_identity().local_alias.as_deref(),
            Some("alice")
        );
        assert_eq!(
            bob_client.current_identity().local_alias.as_deref(),
            Some("bob")
        );

        let alice_auth = alice_client.auth().status().unwrap();
        let bob_auth = bob_client.auth().status().unwrap();
        assert_eq!(alice_auth.subject.as_str(), "did:example:alice");
        assert_eq!(bob_auth.subject.as_str(), "did:example:bob");
        assert!(alice_auth.has_session);
        assert!(bob_auth.has_session);

        assert_identity_runtime_fixture_is_isolated(&fixture, "alice", "bob");
    }

    #[test]
    fn default_selector_does_not_mutate_sdk_global_state() {
        let fixture = AppSandboxFixture::new();
        let core = fixture.core();

        let before = core.identities().default_identity().unwrap().unwrap();
        assert_eq!(before.local_alias.as_deref(), Some("alice"));

        let bob = core
            .client(IdentitySelector::LocalAlias("bob".to_string()))
            .unwrap();
        assert_eq!(bob.did().as_str(), "did:example:bob");

        let after = core.identities().default_identity().unwrap().unwrap();
        assert_eq!(after.local_alias.as_deref(), Some("alice"));
        assert_eq!(before.id, after.id);
        assert_eq!(before.did, after.did);

        let default_client = core.client(IdentitySelector::Default).unwrap();
        assert_eq!(default_client.did().as_str(), "did:example:alice");
        assert_eq!(
            default_client.current_identity().local_alias.as_deref(),
            Some("alice")
        );
    }

    struct AppSandboxFixture {
        temp: tempfile::TempDir,
    }

    impl AppSandboxFixture {
        fn new() -> Self {
            let temp = tempfile::tempdir().unwrap();
            let fixture = Self { temp };

            fs::create_dir_all(fixture.identities_dir()).unwrap();
            fs::create_dir_all(fixture.cache_dir()).unwrap();
            fs::create_dir_all(fixture.temp_dir()).unwrap();
            fs::write(fixture.default_path(), "alice\n").unwrap();
            fs::write(fixture.registry_path(), registry_json()).unwrap();
            fixture.write_identity_runtime("alice");
            fixture.write_identity_runtime("bob");

            fixture
        }

        fn core(&self) -> ImCore {
            ImCore::new(self.config(), self.paths()).unwrap()
        }

        fn config(&self) -> ImCoreConfig {
            ImCoreConfig {
                service_base_url: ServiceEndpoint::parse("https://example.test").unwrap(),
                did_domain: "awiki.test".to_string(),
                user_service_endpoint: None,
                message_service_endpoint: None,
                mail_service_endpoint: None,
                anp_service_endpoint: None,
                anp_service_did: None,
                ca_bundle: None,
                transport_policy: MessageTransportPolicy::HttpOnly,
            }
        }

        fn paths(&self) -> ImCorePaths {
            ImCorePaths {
                identities: IdentityRegistryPaths {
                    identity_root_dir: self.identities_dir(),
                    registry_path: self.registry_path(),
                    default_identity_path: Some(self.default_path()),
                },
                local_state: LocalStatePaths {
                    sqlite_path: self.sqlite_path(),
                },
                runtime: RuntimePaths {
                    cache_dir: self.cache_dir(),
                    temp_dir: self.temp_dir(),
                },
            }
        }

        fn root(&self) -> &Path {
            self.temp.path()
        }

        fn identities_dir(&self) -> PathBuf {
            self.root().join("identities")
        }

        fn registry_path(&self) -> PathBuf {
            self.identities_dir().join("registry.json")
        }

        fn default_path(&self) -> PathBuf {
            self.identities_dir().join("default")
        }

        fn identity_dir(&self, alias: &str) -> PathBuf {
            self.identities_dir().join(alias)
        }

        fn sqlite_path(&self) -> PathBuf {
            self.root().join("state").join("local").join("im.sqlite")
        }

        fn cache_dir(&self) -> PathBuf {
            self.root().join("cache")
        }

        fn temp_dir(&self) -> PathBuf {
            self.root().join("runtime").join("tmp")
        }

        fn write_identity_runtime(&self, alias: &str) {
            let identity_dir = self.identity_dir(alias);
            fs::create_dir_all(&identity_dir).unwrap();

            let did = format!("did:example:{alias}");
            fs::write(
                identity_dir.join("did.json"),
                format!(r#"{{"id":"{did}","controller":"{did}"}}"#),
            )
            .unwrap();
            fs::write(
                identity_dir.join("did_document.json"),
                format!(r#"{{"id":"{did}","controller":"{did}"}}"#),
            )
            .unwrap();
            fs::write(
                identity_dir.join("private.key"),
                format!("test-private-key-for-{alias}\n"),
            )
            .unwrap();
            fs::write(
                identity_dir.join("key-1-private.pem"),
                format!("test-pem-private-key-for-{alias}\n"),
            )
            .unwrap();
            fs::write(
                identity_dir.join("auth.json"),
                format!(r#"{{"subject":"{did}","token":"test-token-for-{alias}"}}"#),
            )
            .unwrap();
        }
    }

    fn assert_path_check(
        report: &PathValidationReport,
        kind: &str,
        expected_path: PathBuf,
        expected_exists: bool,
    ) {
        let check = report
            .checked
            .iter()
            .find(|check| check.kind == kind)
            .unwrap_or_else(|| panic!("missing path check for {kind}"));
        assert_eq!(check.path, expected_path.display().to_string());
        assert_eq!(check.exists, expected_exists);
        assert_eq!(check.readable, expected_exists);
    }

    fn assert_identity_runtime_fixture_is_isolated(
        fixture: &AppSandboxFixture,
        first_alias: &str,
        second_alias: &str,
    ) {
        let first_auth = fixture.identity_dir(first_alias).join("auth.json");
        let second_auth = fixture.identity_dir(second_alias).join("auth.json");
        assert_ne!(first_auth, second_auth);
        assert!(first_auth.exists());
        assert!(second_auth.exists());
        assert!(fixture.identity_dir(first_alias).join("did.json").exists());
        assert!(fixture.identity_dir(second_alias).join("did.json").exists());
        assert!(fixture
            .identity_dir(first_alias)
            .join("private.key")
            .exists());
        assert!(fixture
            .identity_dir(second_alias)
            .join("private.key")
            .exists());
    }

    fn registry_json() -> &'static str {
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
          "device_id": "device-b",
          "ready_for_auth": true,
          "ready_for_messaging": true,
          "missing": []
        }
      ]
    }"#
    }
}
