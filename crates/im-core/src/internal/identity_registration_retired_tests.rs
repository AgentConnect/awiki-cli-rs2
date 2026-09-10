use super::*;
use crate::internal::identity_custody::provision_registration_identity_with_transport;
use crate::internal::identity_provider::{DirectAnpIdentityCustody, IdentityCustody};
use crate::internal::identity_registration_pending::PendingRegistrationStore;
use std::{collections::BTreeMap, sync::Arc};

// Exercise real encrypted custody and pending stores with an empty tenant Core.
// Only the public directory is replaced; no user accounts or credentials are used.
struct Fixture {
    _root: tempfile::TempDir,
    core: crate::ImCore,
    provider: Arc<DirectAnpIdentityCustody>,
}
impl Fixture {
    fn new() -> Self {
        Self::with_external_custody(true)
    }
    fn with_external_custody(external: bool) -> Self {
        let root = tempfile::tempdir().unwrap();
        let path = root.path();
        let manager =
            anp_identity::IdentityManager::initialize(anp_identity::IdentityManagerConfig {
                state_root: path.join("custody"),
                root_key: anp_identity::RootKeySource::Injected(
                    anp_identity::InjectedStoreKey::new("registration-test", [0x76; 32]),
                ),
            })
            .unwrap();
        let provider = Arc::new(DirectAnpIdentityCustody::new(manager));
        let config = crate::ImCoreConfig {
            service_base_url: crate::ServiceEndpoint::parse("https://example.test").unwrap(),
            did_domain: "example.test".into(),
            client_version_info: None,
            user_service_endpoint: None,
            message_service_endpoint: None,
            mail_service_endpoint: None,
            anp_service_endpoint: None,
            anp_service_did: None,
            ca_bundle: None,
            transport_policy: crate::MessageTransportPolicy::HttpOnly,
        };
        let paths = crate::ImCorePaths {
            identities: crate::IdentityRegistryPaths {
                identity_root_dir: path.join("identities"),
                registry_path: path.join("identities/registry.json"),
                default_identity_path: Some(path.join("identities/default")),
            },
            local_state: crate::LocalStatePaths {
                sqlite_path: path.join("local/im.sqlite"),
            },
            runtime: crate::RuntimePaths {
                cache_dir: path.join("cache"),
                temp_dir: path.join("tmp"),
            },
        };
        let options = crate::ImCoreOpenOptions::default().with_identity_secret_vault(
            crate::IdentitySecretStoragePolicy::VaultRequired,
            crate::ImCoreSecretVaultOptions::new(
                crate::vault::DeviceVaultRootKey::from_bytes([0x77; 32]),
                path.join("vault"),
                "registration-test",
                "local-test-device",
            ),
        );
        let options = if external {
            options.with_identity_custody_provider(provider.clone())
        } else {
            options
        };
        let core = crate::ImCore::new_with_options(config, paths, options).unwrap();
        Self {
            _root: root,
            core,
            provider,
        }
    }
    async fn provision(
        &self,
        directory: &mut Directory,
    ) -> crate::internal::identity_registration_pending::PendingRegistrationIdentity {
        provision_registration_identity_with_transport(
            &self.core,
            "example.test",
            "alice",
            directory,
        )
        .await
        .unwrap()
    }
}

#[derive(Default)]
struct Directory {
    documents: BTreeMap<String, Value>,
    gone: std::collections::BTreeSet<String>,
    failure: Option<crate::ImError>,
    reads: Vec<String>,
}
impl Directory {
    fn retire(&mut self, did: &str) {
        self.documents.insert(
            did.into(),
            serde_json::json!({
                "id": did, "deactivated": true,
                "successorDid": "did:wba:example.test:user:alice:e1_successor"
            }),
        );
    }
}
impl crate::internal::transport::AsyncRawJsonTransport for Directory {
    async fn get_json_url(
        &mut self,
        url: &str,
        _headers: BTreeMap<String, String>,
    ) -> crate::ImResult<Value> {
        self.reads.push(url.into());
        if let Some(failure) = &self.failure {
            return Err(failure.clone());
        }
        for did in &self.gone {
            if crate::internal::discovery::did_document::did_document_url(did).unwrap() == url {
                return Err(crate::ImError::Service {
                    status_code: Some(410),
                    code: None,
                    message: r#"{"detail":"DID document is deactivated"}"#.into(),
                    data: None,
                });
            }
        }
        for (did, document) in &self.documents {
            if crate::internal::discovery::did_document::did_document_url(did).unwrap() == url {
                return Ok(document.clone());
            }
        }
        Err(crate::ImError::Service {
            status_code: Some(404),
            code: None,
            message: "absent".into(),
            data: None,
        })
    }
}
fn request() -> crate::identity::RegisterHandleRequest {
    crate::identity::RegisterHandleRequest {
        local_alias: Some("alice".into()),
        requested_handle: crate::ids::Handle::parse("alice.example.test", "").unwrap(),
        verification: crate::identity::VerificationInput::AlreadyVerified,
        invite_code: None,
        profile: crate::identity::InitialProfile {
            display_name: Some("Alice".into()),
            avatar_url: None,
        },
        make_default: true,
    }
}

#[tokio::test]
async fn retired_candidate_is_replaced_without_deleting_custody_or_projecting_an_owner() {
    assert_retired_candidate_replacement(false).await;
}

#[tokio::test]
async fn http_410_retired_candidate_is_replaced_without_deleting_custody_or_projecting_an_owner() {
    assert_retired_candidate_replacement(true).await;
}

async fn assert_retired_candidate_replacement(gone: bool) {
    let fixture = Fixture::new();
    let mut directory = Directory::default();
    let old = fixture.provision(&mut directory).await;
    assert!(
        directory.reads.is_empty(),
        "fresh registration needs no DID lookup"
    );
    if gone {
        directory.gone.insert(old.did.as_str().into());
    } else {
        directory.retire(old.did.as_str());
    }
    let replacement = fixture.provision(&mut directory).await;
    assert_ne!(old.did, replacement.did);
    let identities = fixture.provider.list_identities().await.unwrap();
    assert_eq!(identities.len(), 2);
    assert!(identities
        .iter()
        .any(|entry| entry.reference.did == old.did.as_str()));
    let connection = crate::internal::local_state::open_writable(
        &fixture.core.inner().sdk_paths().local_state.sqlite_path,
    )
    .unwrap();
    let owners: i64 = connection
        .query_row(
            "SELECT count(*) FROM identity_account_bindings",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        owners, 0,
        "candidate selection cannot register or recover an account"
    );
    // A second click must reuse the new unpublished candidate, not create a third.
    assert_eq!(replacement.did, fixture.provision(&mut directory).await.did);
    assert_eq!(fixture.provider.list_identities().await.unwrap().len(), 2);
}

#[tokio::test]
async fn active_and_unpublished_candidates_are_reused_exactly() {
    let fixture = Fixture::new();
    let mut directory = Directory::default();
    let identity = fixture.provision(&mut directory).await;
    assert_eq!(identity, fixture.provision(&mut directory).await);
    directory
        .documents
        .insert(identity.did.as_str().into(), identity.did_document.clone());
    assert_eq!(identity, fixture.provision(&mut directory).await);
    assert_eq!(fixture.provider.list_identities().await.unwrap().len(), 1);
}

#[tokio::test]
async fn unavailable_or_invalid_directory_preserves_custody_and_blocks_selection() {
    let fixture = Fixture::new();
    let mut directory = Directory::default();
    let identity = fixture.provision(&mut directory).await;
    for failure in [
        crate::ImError::TransportUnavailable {
            detail: "offline".into(),
        },
        crate::ImError::Service {
            status_code: Some(500),
            code: None,
            message: "unavailable".into(),
            data: None,
        },
    ] {
        directory.failure = Some(failure);
        assert!(provision_registration_identity_with_transport(
            &fixture.core,
            "example.test",
            "alice",
            &mut directory
        )
        .await
        .is_err());
    }
    directory.failure = None;
    directory.documents.insert(
        identity.did.as_str().into(),
        serde_json::json!({ "id": "wrong", "deactivated": true }),
    );
    assert!(provision_registration_identity_with_transport(
        &fixture.core,
        "example.test",
        "alice",
        &mut directory
    )
    .await
    .is_err());
    assert_eq!(fixture.provider.list_identities().await.unwrap().len(), 1);
}

#[tokio::test]
async fn retired_attempted_pending_restarts_with_fresh_candidate_and_retries_exactly() {
    assert_retired_pending_replacement(false).await;
}

#[tokio::test]
async fn http_410_retired_attempted_pending_restarts_with_fresh_candidate_and_retries_exactly() {
    assert_retired_pending_replacement(true).await;
}

async fn assert_retired_pending_replacement(gone: bool) {
    let fixture = Fixture::new();
    let store = PendingRegistrationStore::from_core(&fixture.core).unwrap();
    let target = registration_target("alice.example.test", "example.test").unwrap();
    let mut directory = Directory::default();
    let (_, mut old) = load_or_create_pending_registration_with_transport(
        &fixture.core,
        &store,
        &request(),
        &target,
        &mut directory,
    )
    .await
    .unwrap();
    crate::internal::identity_custody::begin_registration_publication_async(
        &fixture.core,
        &old.identity,
    )
    .await
    .unwrap();
    old.remote_attempted = true;
    store.save(&old).unwrap();
    if gone {
        directory.gone.insert(old.identity.did.as_str().into());
    } else {
        directory.retire(old.identity.did.as_str());
    }
    let (_, replacement) = load_or_create_pending_registration_with_transport(
        &fixture.core,
        &store,
        &request(),
        &target,
        &mut directory,
    )
    .await
    .unwrap();
    assert_ne!(old.identity.did, replacement.identity.did);
    assert!(!replacement.remote_attempted);
    assert!(replacement.remote_result.is_none());
    assert_eq!(
        store.load("alice", "example.test").unwrap().unwrap().1,
        replacement
    );
    let (_, retry) = load_or_create_pending_registration_with_transport(
        &fixture.core,
        &store,
        &request(),
        &target,
        &mut directory,
    )
    .await
    .unwrap();
    assert_eq!(retry, replacement);
    assert_eq!(fixture.provider.list_identities().await.unwrap().len(), 2);
    let call = register_call(&retry, &request(), None).unwrap();
    assert_eq!(
        call.params["did_document"]["id"],
        retry.identity.did.as_str()
    );
    assert_ne!(call.params["did_document"]["id"], old.identity.did.as_str());
    let outcome = parse_register_outcome(
        &retry,
        serde_json::json!({
            "state": "join_required", "handle": "alice", "domain": "example.test",
            "full_handle": "alice.example.test", "did": "did:wba:example.test:existing",
            "account_verification_token": "test-single-use-verification"
        }),
    )
    .unwrap();
    let RegistrationRemoteOutcome::JoinRequired(parsed) = outcome else {
        panic!("existing Handle must require Join")
    };
    let preparation = prepare_join_required_async(
        &fixture.core,
        parsed,
        crate::internal::identity_registration_join_preparation::RegistrationPendingCleanup {
            secret_ref: store.load("alice", "example.test").unwrap().unwrap().0,
            identity: retry.identity.clone(),
        },
    )
    .await
    .unwrap();
    assert_eq!(
        preparation.mode,
        crate::identity::HandleRegistrationJoinMode::Ordinary
    );
    assert!(
        !preparation.requires_user_presence,
        "candidate replacement must not initiate Recovery"
    );
    let result = join_required_result(&request(), target.full_handle, preparation).unwrap();
    assert_eq!(
        result.sdk_result.state,
        crate::identity::HandleRegistrationState::JoinRequired
    );
    assert!(result.sdk_result.identity.is_none());
}

#[tokio::test]
async fn unavailable_directory_preserves_exact_attempted_pending() {
    let fixture = Fixture::new();
    let store = PendingRegistrationStore::from_core(&fixture.core).unwrap();
    let target = registration_target("alice.example.test", "example.test").unwrap();
    let mut directory = Directory::default();
    let (_, mut old) = load_or_create_pending_registration_with_transport(
        &fixture.core,
        &store,
        &request(),
        &target,
        &mut directory,
    )
    .await
    .unwrap();
    old.remote_attempted = true;
    store.save(&old).unwrap();
    directory.failure = Some(crate::ImError::TransportUnavailable {
        detail: "offline".into(),
    });
    assert!(load_or_create_pending_registration_with_transport(
        &fixture.core,
        &store,
        &request(),
        &target,
        &mut directory
    )
    .await
    .is_err());
    assert_eq!(store.load("alice", "example.test").unwrap().unwrap().1, old);
    assert_eq!(fixture.provider.list_identities().await.unwrap().len(), 1);
}

#[tokio::test]
async fn known_committed_pending_is_preserved_without_a_directory_lookup() {
    use crate::internal::identity_registration_pending::{
        PendingRegistrationPhase, PendingRegistrationRemoteResult,
    };
    let fixture = Fixture::new();
    let store = PendingRegistrationStore::from_core(&fixture.core).unwrap();
    let target = registration_target("alice.example.test", "example.test").unwrap();
    let mut directory = Directory::default();
    let (_, mut committed) = load_or_create_pending_registration_with_transport(
        &fixture.core,
        &store,
        &request(),
        &target,
        &mut directory,
    )
    .await
    .unwrap();
    committed.remote_attempted = true;
    committed.phase = PendingRegistrationPhase::RemoteCommitted;
    committed.remote_result = Some(PendingRegistrationRemoteResult {
        did: committed.identity.did.as_str().into(),
        user_id: "test-user".into(),
        handle: "alice".into(),
        full_handle: "alice.example.test".into(),
        binding_generation: "1".into(),
        access_token: "test-access-token".into(),
    });
    store.save(&committed).unwrap();
    directory.retire(committed.identity.did.as_str());
    let (_, actual) = load_or_create_pending_registration_with_transport(
        &fixture.core,
        &store,
        &request(),
        &target,
        &mut directory,
    )
    .await
    .unwrap();
    assert_eq!(actual, committed);
    assert!(directory.reads.is_empty());
    assert_eq!(fixture.provider.list_identities().await.unwrap().len(), 1);
}

#[tokio::test]
async fn native_custody_pending_keeps_its_existing_retry_path() {
    let fixture = Fixture::with_external_custody(false);
    let store = PendingRegistrationStore::from_core(&fixture.core).unwrap();
    let target = registration_target("alice.example.test", "example.test").unwrap();
    let mut directory = Directory::default();
    let (_, pending) = load_or_create_pending_registration_with_transport(
        &fixture.core,
        &store,
        &request(),
        &target,
        &mut directory,
    )
    .await
    .unwrap();
    directory.failure = Some(crate::ImError::TransportUnavailable {
        detail: "offline".into(),
    });
    let (_, retry) = load_or_create_pending_registration_with_transport(
        &fixture.core,
        &store,
        &request(),
        &target,
        &mut directory,
    )
    .await
    .unwrap();
    assert_eq!(pending, retry);
    assert!(directory.reads.is_empty());
}
