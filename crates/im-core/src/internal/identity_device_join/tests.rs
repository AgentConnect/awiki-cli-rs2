use super::*;
use serde_json::json;
use std::path::Path;
use std::sync::Arc;

fn test_config() -> crate::ImCoreConfig {
    crate::ImCoreConfig {
        service_base_url: crate::ServiceEndpoint::parse("https://example.test").unwrap(),
        did_domain: "awiki.test".to_owned(),
        client_version_info: None,
        user_service_endpoint: None,
        message_service_endpoint: None,
        mail_service_endpoint: None,
        anp_service_endpoint: None,
        anp_service_did: None,
        ca_bundle: None,
        transport_policy: crate::MessageTransportPolicy::HttpOnly,
    }
}

fn test_paths(root: &Path) -> crate::ImCorePaths {
    crate::ImCorePaths {
        identities: crate::IdentityRegistryPaths {
            identity_root_dir: root.join("identities"),
            registry_path: root.join("identities").join("registry.json"),
            default_identity_path: Some(root.join("identities").join("default")),
        },
        local_state: crate::LocalStatePaths {
            sqlite_path: root.join("local").join("im.sqlite"),
        },
        runtime: crate::RuntimePaths {
            cache_dir: root.join("cache"),
            temp_dir: root.join("tmp"),
        },
    }
}

fn open_ready_admin_core(root: &Path) -> (crate::ImCore, serde_json::Value, crate::ids::Did) {
    use crate::internal::identity_device_state::{
        DeviceAuthorizationProjection, DeviceAuthorizationRole, DeviceAuthorizationStatus,
        IdentityDeviceMode, IdentityDeviceState, IdentityInternalCheckpoint,
        IDENTITY_DEVICE_STATE_SCHEMA_VERSION,
    };

    let generated =
        crate::internal::identity_generation::generate_vnext_handle_identity_with_default_daemon_subkey(
            "awiki.test",
            "alice",
            None,
            None,
        )
        .unwrap();
    let document_hash = canonical_hash(&generated.did_document).unwrap();
    let paths = test_paths(root);
    let vault = Arc::new(crate::vault::FileSecretVault::new(
        crate::vault::DeviceVaultRootKey::from_bytes([47_u8; 32]),
        crate::vault::FileSecretVaultStore::new(root.join("vault")),
    ));
    crate::internal::identity_store::IdentityStore::new(&paths.identities)
        .save_identity_with_secret_storage(
            crate::internal::identity_store::SaveIdentityInput {
                local_alias: "alice".to_owned(),
                did: generated.did.clone(),
                unique_id: generated.unique_id.clone(),
                user_id: "user-1".to_owned(),
                display_name: "Alice".to_owned(),
                handle: "alice".to_owned(),
                full_handle: "alice.awiki.test".to_owned(),
                binding_generation: None,
                jwt_token: "access-token".to_owned(),
                did_document: Some(generated.did_document.clone()),
                key_mode: crate::internal::identity_store::SaveIdentityKeyMode::VNext {
                    root_key_id: generated.root_key_id.clone(),
                    device_signing_key_id: generated.device_signing_key_id.clone(),
                    device_e2ee_key_id: generated.device_e2ee_key_id.clone(),
                },
                device_state: Some(IdentityDeviceState {
                    schema_version: IDENTITY_DEVICE_STATE_SCHEMA_VERSION,
                    mode: IdentityDeviceMode::VNext,
                    authorization: Some(DeviceAuthorizationProjection {
                        protocol_device_id: generated.protocol_device_id.clone(),
                        signing_key_id: generated.device_signing_key_id.clone(),
                        e2ee_key_id: generated.device_e2ee_key_id.clone(),
                        status: DeviceAuthorizationStatus::Active,
                        role: DeviceAuthorizationRole::Admin,
                        management_ready: true,
                        auth_generation: 1,
                    }),
                    checkpoint: Some(IdentityInternalCheckpoint {
                        document_version: 7,
                        document_hash,
                        registry_version: 3,
                    }),
                }),
                key1_private_pem: generated.root_private_pem,
                key1_public_pem: generated.root_public_pem,
                e2ee_signing_private_pem: generated.device_signing_private_pem,
                e2ee_agreement_private_pem: generated.device_e2ee_private_pem,
                daemon_subkey_package: None,
                make_default: true,
            },
            crate::internal::identity_store::SaveIdentitySecretStorage::Vault {
                workspace_id: "join-test-workspace".to_owned(),
                device_id: "join-test-vault-device".to_owned(),
                vault,
            },
        )
        .unwrap();
    let did = generated.did;
    let document = generated.did_document;
    let core = crate::ImCore::new_with_options(
        test_config(),
        paths,
        crate::ImCoreOpenOptions::default().with_identity_secret_vault(
            crate::IdentitySecretStoragePolicy::VaultRequired,
            crate::ImCoreSecretVaultOptions::new(
                crate::vault::DeviceVaultRootKey::from_bytes([47_u8; 32]),
                root.join("vault"),
                "join-test-workspace",
                "join-test-vault-device",
            ),
        ),
    )
    .unwrap();
    (core, document, did)
}

fn open_empty_vault_core(root: &Path) -> crate::ImCore {
    crate::ImCore::new_with_options(
        test_config(),
        test_paths(root),
        crate::ImCoreOpenOptions::default().with_identity_secret_vault(
            crate::IdentitySecretStoragePolicy::VaultRequired,
            crate::ImCoreSecretVaultOptions::new(
                crate::vault::DeviceVaultRootKey::from_bytes([53_u8; 32]),
                root.join("vault"),
                "join-candidate-workspace",
                "join-candidate-vault-device",
            ),
        ),
    )
    .unwrap()
}

fn open_anp_ready_admin_core(
    root: &Path,
) -> (
    crate::ImCore,
    crate::internal::identity_registration_pending::PendingRegistrationIdentity,
) {
    let core = open_empty_vault_core(root);
    let identity = crate::internal::identity_custody::provision_registration_identity(
        &core,
        "awiki.test",
        "alice",
    )
    .unwrap();
    let document_hash = canonical_hash(&identity.did_document).unwrap();
    let input = crate::internal::identity_registration_runtime::anp_vnext_bootstrap_save_input(
        crate::internal::identity_registration_runtime::AnpVNextBootstrapSaveInput {
            identity: &identity,
            document_hash: &document_hash,
            local_alias: "alice",
            display_name: "Alice",
            user_id: "user-1",
            handle: "alice",
            full_handle: "alice.awiki.test",
            binding_generation: "1",
            access_token: "access-token",
            make_default: true,
        },
    )
    .unwrap();
    let storage = crate::internal::identity_store::AnpIdentityProjectionStorage::from_core(
        &core,
        identity.controller_store_id.clone(),
        identity.controller_identity_id.clone(),
    )
    .unwrap();
    crate::internal::identity_store::IdentityStore::new(&test_paths(root).identities)
        .save_anp_identity_projection(input, storage)
        .unwrap();
    (core, identity)
}

#[cfg(feature = "provider-traits")]
fn open_external_provider_ready_admin_core(
    root: &Path,
) -> (
    crate::ImCore,
    serde_json::Value,
    crate::ids::Did,
    crate::internal::identity_device_state::IdentityInternalCheckpoint,
) {
    use crate::internal::identity_device_state::{
        DeviceAuthorizationProjection, DeviceAuthorizationRole, DeviceAuthorizationStatus,
        IdentityDeviceMode, IdentityDeviceState, IdentityInternalCheckpoint,
        IDENTITY_DEVICE_STATE_SCHEMA_VERSION,
    };
    use crate::internal::identity_provider::DirectAnpIdentityCustody;

    let provider_root = root.join("provider");
    let mut manager =
        anp_identity::IdentityManager::initialize(anp_identity::IdentityManagerConfig {
            state_root: provider_root,
            root_key: anp_identity::RootKeySource::Injected(anp_identity::InjectedStoreKey::new(
                "join-external-provider",
                [0x6b; 32],
            )),
        })
        .unwrap();
    let identity = manager
        .create(anp_identity::CreateIdentityRequest {
            profile: anp_identity::CreateIdentityProfile::E1,
            domain: "awiki.test".to_owned(),
            port: None,
            path_segments: vec!["users".to_owned(), "external-admin".to_owned()],
            capabilities: anp_identity::CreateIdentityCapabilities { did_wba: true },
            managed_keys: vec![
                anp_identity::ManagedKeyInput {
                    fragment: "root".to_owned(),
                    role: anp_identity::ManagedKeyRole::RootControl,
                },
                anp_identity::ManagedKeyInput {
                    fragment: "device".to_owned(),
                    role: anp_identity::ManagedKeyRole::DeviceSigning,
                },
                anp_identity::ManagedKeyInput {
                    fragment: "agreement".to_owned(),
                    role: anp_identity::ManagedKeyRole::E2eeAgreement,
                },
            ],
            external_keys: Vec::new(),
            services: Vec::new(),
            agent_description_url: None,
            extensions: vec![anp_identity::CreateIdentityExtension::DeviceManifest {
                devices: vec![anp_identity::DeviceManifestEntryInput {
                    device_id: "device-admin".to_owned(),
                    signing_key_id: "#device".to_owned(),
                    e2ee_key_id: "#agreement".to_owned(),
                    profiles: crate::internal::identity_generation::vnext_device_profiles(),
                }],
            }],
        })
        .unwrap();
    let public = identity.public_identity().unwrap();
    let reference = public.reference.clone();
    let key_for = |purpose| {
        public
            .active_keys
            .iter()
            .find(|key| key.purposes.contains(&purpose))
            .unwrap()
            .kid
            .clone()
    };
    let root_kid = key_for(anp_identity::KeyPurpose::RootControl);
    let signing_kid = key_for(anp_identity::KeyPurpose::DeviceAssertion);
    let agreement_kid = key_for(anp_identity::KeyPurpose::KeyAgreement);
    let document = public.document.into_value();
    let checkpoint = IdentityInternalCheckpoint {
        document_version: 1,
        document_hash: canonical_hash(&document).unwrap(),
        registry_version: 1,
    };
    let paths = test_paths(root);
    let provider = Arc::new(DirectAnpIdentityCustody::new(manager));
    let core = crate::ImCore::new_with_options(
        test_config(),
        paths.clone(),
        crate::ImCoreOpenOptions::default()
            .with_identity_secret_vault(
                crate::IdentitySecretStoragePolicy::VaultRequired,
                crate::ImCoreSecretVaultOptions::new(
                    crate::vault::DeviceVaultRootKey::from_bytes([47_u8; 32]),
                    root.join("vault"),
                    "join-external-workspace",
                    "join-external-device",
                ),
            )
            .with_identity_custody_provider(provider),
    )
    .unwrap();
    crate::internal::identity_store::IdentityStore::new(&paths.identities)
        .save_anp_identity_projection(
            crate::internal::identity_store::SaveIdentityInput {
                local_alias: "alice".to_owned(),
                did: crate::ids::Did::parse(&reference.did).unwrap(),
                unique_id: "external-admin-id".to_owned(),
                user_id: "user-1".to_owned(),
                display_name: "Alice".to_owned(),
                handle: "alice".to_owned(),
                full_handle: "alice.awiki.test".to_owned(),
                binding_generation: None,
                jwt_token: "access-token".to_owned(),
                did_document: Some(document.clone()),
                key_mode: crate::internal::identity_store::SaveIdentityKeyMode::VNext {
                    root_key_id: root_kid,
                    device_signing_key_id: signing_kid.clone(),
                    device_e2ee_key_id: agreement_kid.clone(),
                },
                device_state: Some(IdentityDeviceState {
                    schema_version: IDENTITY_DEVICE_STATE_SCHEMA_VERSION,
                    mode: IdentityDeviceMode::VNext,
                    authorization: Some(DeviceAuthorizationProjection {
                        protocol_device_id: crate::ids::ProtocolDeviceId::parse("device-admin")
                            .unwrap(),
                        signing_key_id: signing_kid,
                        e2ee_key_id: agreement_kid,
                        status: DeviceAuthorizationStatus::Active,
                        role: DeviceAuthorizationRole::Admin,
                        management_ready: true,
                        auth_generation: 1,
                    }),
                    checkpoint: Some(checkpoint.clone()),
                }),
                key1_private_pem: String::new(),
                key1_public_pem: String::new(),
                e2ee_signing_private_pem: String::new(),
                e2ee_agreement_private_pem: String::new(),
                daemon_subkey_package: None,
                make_default: true,
            },
            crate::internal::identity_store::AnpIdentityProjectionStorage::from_core(
                &core,
                reference.store_id,
                reference.identity_id,
            )
            .unwrap(),
        )
        .unwrap();
    (
        core,
        document,
        crate::ids::Did::parse(&reference.did).unwrap(),
        checkpoint,
    )
}

fn reopen_join_test_core(root: &Path) -> crate::ImCore {
    crate::ImCore::new_with_options(
        test_config(),
        test_paths(root),
        crate::ImCoreOpenOptions::default().with_identity_secret_vault(
            crate::IdentitySecretStoragePolicy::VaultRequired,
            crate::ImCoreSecretVaultOptions::new(
                crate::vault::DeviceVaultRootKey::from_bytes([47_u8; 32]),
                root.join("vault"),
                "join-test-workspace",
                "join-test-vault-device",
            ),
        ),
    )
    .unwrap()
}

fn reopen_join_test_recovery_core(root: &Path) -> crate::ImCore {
    crate::ImCore::new_with_options(
        test_config(),
        test_paths(root),
        crate::ImCoreOpenOptions::default()
            .with_multi_device_handle_recovery_enabled(true)
            .with_multi_device_audience("awiki-user-service")
            .with_identity_secret_vault(
                crate::IdentitySecretStoragePolicy::VaultRequired,
                crate::ImCoreSecretVaultOptions::new(
                    crate::vault::DeviceVaultRootKey::from_bytes([47_u8; 32]),
                    root.join("vault"),
                    "join-test-workspace",
                    "join-test-vault-device",
                ),
            ),
    )
    .unwrap()
}

fn member_access_token(did: &str, device_id: &str, signing_key_id: &str) -> String {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
    let now = time::OffsetDateTime::now_utc().unix_timestamp();
    let claims = json!({
        "iss": "user-service",
        "aud": ["awiki-user-service", "awiki-message-service"],
        "sub": did,
        "type": "access",
        "purpose": "awiki.device.access.v1",
        "did": did,
        "user_id": "user-1",
        "device_id": device_id,
        "key_id": signing_key_id,
        "auth_generation": 1,
        "scopes": ["device:read", "device:root-import-complete", "message:connect"],
        "iat": now,
        "nbf": now,
        "exp": now + 300,
        "jti": "joined-crash-window-token"
    });
    format!(
        "e30.{}.signature",
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).unwrap())
    )
}

fn mark_new_device_join_authorized(
    core: &crate::ImCore,
    join_session_id: &str,
    did: &crate::ids::Did,
    protocol_device_id: &crate::ids::ProtocolDeviceId,
) -> StoredJoinSession {
    let store = JoinStateStore::new(core);
    let mut stored = store
        .load(join_session_id, DeviceJoinSide::NewDevice)
        .unwrap()
        .unwrap();
    cleanup_consumed_join_secrets(core, &stored).unwrap();
    stored.join_request.did = did.as_str().to_owned();
    stored.join_request.device_id = protocol_device_id.as_str().to_owned();
    stored.join_request_hash =
        canonical_hash(&serde_json::to_value(&stored.join_request).unwrap()).unwrap();
    stored.phase = DeviceJoinLocalPhase::Authorized;
    stored.activation_pending = false;
    stored.join_session_token_ref = None;
    store.save(&stored).unwrap();
    stored
}

#[test]
fn response_signature_input_is_closed_and_canonicalizable() {
    let value = response_params_value(
        "respond-1",
        "join-1",
        "challenge-1",
        "sha256:challenge",
        "sha256:request",
        "sha256:transcript",
    );
    assert_eq!(
        value,
        json!({
            "type": DEVICE_JOIN_RESPONSE_SIGNATURE_INPUT_TYPE,
            "operation_id": "respond-1",
            "join_session_id": "join-1",
            "challenge_id": "challenge-1",
            "challenge_hash": "sha256:challenge",
            "join_request_hash": "sha256:request",
            "pairing_transcript_hash": "sha256:transcript"
        })
    );
    assert!(canonical_bytes(&value).is_ok());
}

#[tokio::test]
async fn join_start_emits_mixed_profiles() {
    let candidate_root = tempfile::tempdir().unwrap();
    let candidate = open_empty_vault_core(candidate_root.path());
    let generated = crate::internal::identity_generation::generate_vnext_handle_identity_with_default_daemon_subkey(
        "example.com",
        "alice",
        None,
        None,
    )
    .unwrap();
    let started = candidate
        .device_join()
        .start(
            DeviceJoinStartRequest {
                operation_id: "start-mixed-profiles".to_owned(),
                did: generated.did,
                ttl_seconds: 300,
            },
            &generated.did_document,
        )
        .await
        .unwrap();

    assert_eq!(
        started.join_request.profiles,
        [
            "anp.core.binding.v1",
            "anp.identity.discovery.v1",
            "anp.direct.base.v1",
            "anp.direct.e2ee.v2",
            "anp.group.base.v1",
            "anp.group.e2ee.v2",
        ]
    );
    let stored = JoinStateStore::new(&candidate)
        .load(&started.session.join_session_id, DeviceJoinSide::NewDevice)
        .unwrap()
        .unwrap();
    let encoded = serde_json::to_string(&stored).unwrap();
    assert!(!encoded.contains("private_pem"));
    assert!(!encoded.contains("IdentityDeviceSigningPrivate"));
    assert!(!encoded.contains("IdentityE2eeAgreementPrivate"));
    assert!(required_vault(&candidate)
        .unwrap()
        .list()
        .unwrap()
        .iter()
        .all(|secret_ref| !matches!(
            secret_ref.kind,
            SecretKind::IdentityDeviceSigningPrivate | SecretKind::IdentityE2eeAgreementPrivate
        )));
    let custody = stored.join_custody.unwrap();
    let manager = crate::internal::identity_custody::open_controller_manager(&candidate).unwrap();
    let descriptor = manager
        .list()
        .unwrap()
        .into_iter()
        .find(|descriptor| descriptor.reference.did == started.session.did.as_str())
        .unwrap();
    let identity = manager.get(&descriptor.reference).unwrap();
    assert_eq!(identity.reference().identity_id, custody.identity_id);
    assert_eq!(
        identity.public_identity().unwrap().state,
        anp_identity::PublicIdentityState::Enrolling
    );
}

#[tokio::test]
async fn join_start_resumes_enrollment_after_crash_before_local_session_commit() {
    let candidate_root = tempfile::tempdir().unwrap();
    let candidate = open_empty_vault_core(candidate_root.path());
    let generated = crate::internal::identity_generation::generate_vnext_handle_identity_with_default_daemon_subkey(
        "example.com",
        "alice",
        None,
        None,
    )
    .unwrap();
    let operation_id = "start-resume-after-provider-side-effect";
    let request = DeviceJoinStartRequest {
        operation_id: operation_id.to_owned(),
        did: generated.did.clone(),
        ttl_seconds: 300,
    };
    *FAIL_AFTER_JOIN_ENROLLMENT_PREPARED.lock().unwrap() = Some(operation_id.to_owned());

    let error = candidate
        .device_join()
        .start(request.clone(), &generated.did_document)
        .await
        .unwrap_err();
    assert!(matches!(error, crate::ImError::Internal { .. }));
    let journal = DeviceJoinCreationJournalStore::new(&candidate)
        .load(operation_id)
        .unwrap()
        .unwrap();
    let custody = journal.custody.clone().unwrap();
    assert!(journal.enrollment.is_some());
    let identities = crate::internal::identity_custody::open_controller_manager(&candidate)
        .unwrap()
        .list()
        .unwrap();
    assert_eq!(
        identities
            .iter()
            .filter(|identity| identity.reference.did == generated.did.as_str())
            .count(),
        1
    );

    let started = candidate
        .device_join()
        .start(request, &generated.did_document)
        .await
        .unwrap();
    assert_eq!(
        JoinStateStore::new(&candidate)
            .load(&started.session.join_session_id, DeviceJoinSide::NewDevice)
            .unwrap()
            .unwrap()
            .join_custody,
        Some(custody)
    );
    assert!(DeviceJoinCreationJournalStore::new(&candidate)
        .load(operation_id)
        .unwrap()
        .is_none());
    let identities = crate::internal::identity_custody::open_controller_manager(&candidate)
        .unwrap()
        .list()
        .unwrap();
    assert_eq!(
        identities
            .iter()
            .filter(|identity| identity.reference.did == generated.did.as_str())
            .count(),
        1
    );
}

#[tokio::test]
async fn cancelled_new_device_join_discards_unpublished_custody_and_pairing_secret() {
    let root = tempfile::tempdir().unwrap();
    let core = open_empty_vault_core(root.path());
    let generated = crate::internal::identity_generation::generate_vnext_handle_identity_with_default_daemon_subkey(
        "awiki.test",
        "cancelled",
        None,
        None,
    )
    .unwrap();
    let started = core
        .device_join()
        .start(
            DeviceJoinStartRequest {
                operation_id: "cancel-unpublished".to_owned(),
                did: generated.did.clone(),
                ttl_seconds: 300,
            },
            &generated.did_document,
        )
        .await
        .unwrap();
    let stored = JoinStateStore::new(&core)
        .load(&started.session.join_session_id, DeviceJoinSide::NewDevice)
        .unwrap()
        .unwrap();

    cancel_join(
        &core,
        &started.session.join_session_id,
        DeviceJoinSide::NewDevice,
    )
    .await
    .unwrap();

    assert!(
        crate::internal::identity_custody::open_controller_manager(&core)
            .unwrap()
            .list()
            .unwrap()
            .into_iter()
            .all(|descriptor| descriptor.reference.did != generated.did.as_str())
    );
    assert!(required_vault(&core)
        .unwrap()
        .open(&stored.pairing_private_ref)
        .is_err());
}

#[test]
fn join_profile_reader_accepts_only_canonical_or_legacy_complete_sets() {
    let canonical = DEVICE_JOIN_VNEXT_PROFILES
        .iter()
        .map(|profile| (*profile).to_owned())
        .collect::<Vec<_>>();
    let legacy = DEVICE_JOIN_LEGACY_DRAFT_PROFILES
        .iter()
        .map(|profile| (*profile).to_owned())
        .collect::<Vec<_>>();
    let mut hybrid = canonical.clone();
    hybrid[0] = anp::authentication::PROFILE_CORE_BINDING_V2.to_owned();

    assert!(join_profiles_are_supported(&canonical));
    assert!(join_profiles_are_supported(&legacy));
    assert!(!join_profiles_are_supported(&hybrid));
}

#[tokio::test]
async fn retiring_authorized_new_device_join_is_exact_and_idempotent() {
    let root = tempfile::tempdir().unwrap();
    let core = open_empty_vault_core(root.path());
    let generated = crate::internal::identity_generation::generate_vnext_handle_identity_with_default_daemon_subkey("awiki.test", "alice", None, None).unwrap();
    let other_generated = crate::internal::identity_generation::generate_vnext_handle_identity_with_default_daemon_subkey("awiki.test", "bob", None, None).unwrap();
    let did = generated.did.clone();
    let other_did = other_generated.did.clone();
    let authorized = core
        .device_join()
        .start(
            DeviceJoinStartRequest {
                operation_id: "retire-authorized".to_owned(),
                did: did.clone(),
                ttl_seconds: 300,
            },
            &generated.did_document,
        )
        .await
        .unwrap();
    let pending = core
        .device_join()
        .start(
            DeviceJoinStartRequest {
                operation_id: "preserve-pending".to_owned(),
                did: did.clone(),
                ttl_seconds: 300,
            },
            &generated.did_document,
        )
        .await
        .unwrap();
    let other = core
        .device_join()
        .start(
            DeviceJoinStartRequest {
                operation_id: "preserve-other".to_owned(),
                did: other_did.clone(),
                ttl_seconds: 300,
            },
            &other_generated.did_document,
        )
        .await
        .unwrap();
    let authorized_device_id = authorized.session.protocol_device_id.clone();
    let authorized_state = mark_new_device_join_authorized(
        &core,
        &authorized.session.join_session_id,
        &did,
        &authorized_device_id,
    );
    mark_new_device_join_authorized(
        &core,
        &other.session.join_session_id,
        &other_did,
        &other.session.protocol_device_id,
    );

    retire_authorized_new_device_sessions(&core, did.as_str(), authorized_device_id.as_str())
        .unwrap();
    retire_authorized_new_device_sessions(&core, did.as_str(), authorized_device_id.as_str())
        .unwrap();

    let store = JoinStateStore::new(&core);
    assert!(store
        .load(
            &authorized.session.join_session_id,
            DeviceJoinSide::NewDevice,
        )
        .unwrap()
        .is_none());
    assert!(store
        .load(&pending.session.join_session_id, DeviceJoinSide::NewDevice)
        .unwrap()
        .is_some());
    assert!(store
        .load(&other.session.join_session_id, DeviceJoinSide::NewDevice)
        .unwrap()
        .is_some());
    assert!(required_vault(&core)
        .unwrap()
        .open(&authorized_state.pairing_private_ref)
        .is_err());
}

#[tokio::test]
async fn identity_retirement_replays_exact_join_cleanup_after_restart() {
    let root = tempfile::tempdir().unwrap();
    let (core, document, did) = open_ready_admin_core(root.path());
    let protocol_device_id = core
        .identities()
        .device_summary(crate::identity::IdentitySelector::Default)
        .unwrap()
        .protocol_device_id
        .unwrap();
    let authorized = core
        .device_join()
        .start(
            DeviceJoinStartRequest {
                operation_id: "identity-retire-authorized".to_owned(),
                did: did.clone(),
                ttl_seconds: 300,
            },
            &document,
        )
        .await
        .unwrap();
    let pending = core
        .device_join()
        .start(
            DeviceJoinStartRequest {
                operation_id: "identity-retire-preserve-pending".to_owned(),
                did: did.clone(),
                ttl_seconds: 300,
            },
            &document,
        )
        .await
        .unwrap();
    let late_authorized = mark_new_device_join_authorized(
        &core,
        &authorized.session.join_session_id,
        &did,
        &protocol_device_id,
    );

    core.identities()
        .delete_local_identity(crate::identity::IdentitySelector::Default)
        .unwrap();
    let store = JoinStateStore::new(&core);
    assert!(store
        .load(
            &authorized.session.join_session_id,
            DeviceJoinSide::NewDevice,
        )
        .unwrap()
        .is_none());
    assert!(store
        .load(&pending.session.join_session_id, DeviceJoinSide::NewDevice)
        .unwrap()
        .is_some());

    // A late operation admitted before retirement may recreate the terminal
    // record. The durable retirement marker must remove it on the next open.
    store.save(&late_authorized).unwrap();
    drop(core);

    let reopened = reopen_join_test_core(root.path());
    let reopened_store = JoinStateStore::new(&reopened);
    assert!(reopened_store
        .load(
            &authorized.session.join_session_id,
            DeviceJoinSide::NewDevice,
        )
        .unwrap()
        .is_none());
    assert!(reopened_store
        .load(&pending.session.join_session_id, DeviceJoinSide::NewDevice)
        .unwrap()
        .is_some());
}

#[test]
fn identity_retirement_reopens_ordinary_registration_join_for_same_handle() {
    let root = tempfile::tempdir().unwrap();
    let paths = test_paths(root.path());
    let (core, _, did) = open_ready_admin_core(root.path());
    let identity = core.identities().default_identity().unwrap().unwrap();
    let protocol_device_id = core
        .identities()
        .device_summary(crate::identity::IdentitySelector::Default)
        .unwrap()
        .protocol_device_id
        .unwrap();
    let connection =
        crate::internal::local_state::open_writable(&paths.local_state.sqlite_path).unwrap();
    connection
        .execute(
            "INSERT INTO identity_account_bindings(owner_identity_id,account_id,handle_scope,current_did,device_id,identity_generation,device_auth_generation,created_at,updated_at) VALUES (?1,'user-1','alice.awiki.test',?2,?3,'1','1',1,1)",
            rusqlite::params![identity.id.as_str(), did.as_str(), protocol_device_id.as_str()],
        )
        .unwrap();
    drop(connection);

    core.identities()
        .delete_local_identity(crate::identity::IdentitySelector::Default)
        .unwrap();
    let index = crate::internal::identity_store::IdentityStore::new(&paths.identities)
        .load_index()
        .unwrap();
    assert!(index.credentials.is_empty());
    assert_eq!(
        crate::internal::identity_local_owner_matcher::match_stable_owner_without_transition(
            &paths.local_state.sqlite_path,
            &paths.identities.identity_root_dir,
            &index,
            "alice.awiki.test",
            did.as_str(),
        )
        .unwrap(),
        crate::internal::identity_local_owner_matcher::StableOwnerMatch::None
    );
}

#[test]
fn request_role_is_member_only() {
    let request = json!({
        "type": DEVICE_JOIN_REQUEST_TYPE,
        "did": "did:wba:example.com:user:alice",
        "join_session_id": "join-1",
        "device_id": "device-new",
        "signing_public_key": {
            "id": "did:wba:example.com:user:alice#device-new-sign",
            "type": "JsonWebKey2020",
            "controller": "did:wba:example.com:user:alice",
            "publicKeyJwk": {"kty": "OKP", "crv": "Ed25519", "x": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"}
        },
        "e2ee_public_key": {
            "id": "did:wba:example.com:user:alice#device-new-e2ee",
            "type": "JsonWebKey2020",
            "controller": "did:wba:example.com:user:alice",
            "publicKeyJwk": {"kty": "OKP", "crv": "X25519", "x": "AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE"}
        },
        "pairing_public_key": "AgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgI",
        "profiles": [],
        "requested_role": "admin",
        "issued_at": "2026-07-18T12:00:00Z",
        "expires_at": "2026-07-18T12:10:00Z",
        "join_request_proof": {
            "type": DEVICE_JOIN_REQUEST_PROOF_TYPE,
            "algorithm": JOIN_PROOF_ALGORITHM,
            "verification_method": "did:wba:example.com:user:alice#device-new-sign",
            "created_at": "2026-07-18T12:00:00Z",
            "proof_value_b64u": "signature"
        }
    });
    let parsed: DeviceJoinRequest = serde_json::from_value(request).unwrap();
    assert!(matches!(
        validate_join_request(&parsed, OffsetDateTime::now_utc()),
        Err(crate::ImError::InvalidInput {
            field: Some(field),
            ..
        }) if field == "join_request.requested_role"
    ));
}

#[tokio::test]
async fn local_admin_verification_progress_is_phase_gated_and_read_only() {
    let admin_root = tempfile::tempdir().unwrap();
    let candidate_root = tempfile::tempdir().unwrap();
    let (core, document, did) = open_ready_admin_core(admin_root.path());
    let candidate = open_empty_vault_core(candidate_root.path());
    let document_hash = canonical_hash(&document).unwrap();
    let started = candidate
        .device_join()
        .start(
            DeviceJoinStartRequest {
                operation_id: "start-local-progress".to_owned(),
                did,
                ttl_seconds: 300,
            },
            &document,
        )
        .await
        .unwrap();
    let prepared = core
        .device_join()
        .prepare_admin_challenge(DeviceJoinAdminPrepareRequest {
            admin_identity: crate::identity::IdentitySelector::Default,
            operation_id: "prepare-local-progress".to_owned(),
            join_request: started.join_request,
            challenge_ttl_seconds: 180,
            document_version: 7,
            document_hash: document_hash.clone(),
        })
        .unwrap();

    assert!(core
        .device_join()
        .local_device_join_verification_progress(
            crate::identity::IdentitySelector::Default,
            &started.session.join_session_id,
        )
        .is_err());

    let responded = candidate
        .device_join()
        .respond_as_new_device(DeviceJoinNewDeviceRespondRequest {
            operation_id: "respond-local-progress".to_owned(),
            challenge: prepared.challenge,
            admin_did_document: document,
            document_version: 7,
            document_hash,
        })
        .await
        .unwrap();
    let verified = core
        .device_join()
        .verify_response_as_admin(DeviceJoinAdminVerifyRequest {
            operation_id: "verify-local-progress".to_owned(),
            join_session_id: started.session.join_session_id.clone(),
            response: responded.response,
        })
        .unwrap();
    let before = JoinStateStore::new(&core)
        .load(&started.session.join_session_id, DeviceJoinSide::Admin)
        .unwrap()
        .unwrap();

    let progress = core
        .device_join()
        .local_device_join_verification_progress(
            crate::identity::IdentitySelector::Default,
            &started.session.join_session_id,
        )
        .unwrap();
    let after = JoinStateStore::new(&core)
        .load(&started.session.join_session_id, DeviceJoinSide::Admin)
        .unwrap()
        .unwrap();

    assert_eq!(
        before, after,
        "local progress must not advance or rewrite state"
    );
    assert_eq!(
        progress.session.phase,
        DeviceJoinLocalPhase::ResponseVerified
    );
    assert_eq!(
        progress.remote_state,
        crate::identity::DeviceJoinRemoteState::ResponseVerified
    );
    assert_eq!(progress.sas.as_deref(), Some(verified.sas.as_str()));
    assert!(progress.authorized_device.is_none());
}

#[cfg(feature = "provider-traits")]
#[tokio::test]
async fn external_provider_completes_admin_join_signing_and_document_change() {
    let admin_root = tempfile::tempdir().unwrap();
    let candidate_root = tempfile::tempdir().unwrap();
    let (admin, document, did, checkpoint) =
        open_external_provider_ready_admin_core(admin_root.path());
    let candidate = open_empty_vault_core(candidate_root.path());
    let started = candidate
        .device_join()
        .start(
            DeviceJoinStartRequest {
                operation_id: "start-external-admin".to_owned(),
                did,
                ttl_seconds: 300,
            },
            &document,
        )
        .await
        .unwrap();
    let challenged = prepare_admin_challenge_async(
        &admin,
        DeviceJoinAdminPrepareRequest {
            admin_identity: crate::identity::IdentitySelector::Default,
            operation_id: "challenge-external-admin".to_owned(),
            join_request: started.join_request.clone(),
            challenge_ttl_seconds: 180,
            document_version: checkpoint.document_version,
            document_hash: checkpoint.document_hash.clone(),
        },
    )
    .await
    .unwrap();
    let responded = candidate
        .device_join()
        .respond_as_new_device(DeviceJoinNewDeviceRespondRequest {
            operation_id: "respond-external-admin".to_owned(),
            challenge: challenged.challenge,
            admin_did_document: document,
            document_version: checkpoint.document_version,
            document_hash: checkpoint.document_hash.clone(),
        })
        .await
        .unwrap();
    verify_response_as_admin(
        &admin,
        DeviceJoinAdminVerifyRequest {
            operation_id: "verify-external-admin".to_owned(),
            join_session_id: started.session.join_session_id.clone(),
            response: responded.response,
        },
    )
    .unwrap();
    let prepared = prepare_admin_approval_async(
        &admin,
        "approve-external-admin",
        &started.session.join_session_id,
        &checkpoint,
        &format_time(OffsetDateTime::now_utc()).unwrap(),
        true,
    )
    .await
    .unwrap();
    validate_authorized_document(&started.join_request, &prepared.new_document).unwrap();

    let committed = crate::internal::identity_device_state::IdentityInternalCheckpoint {
        document_version: checkpoint.document_version + 1,
        document_hash: canonical_hash(&prepared.new_document).unwrap(),
        registry_version: checkpoint.registry_version + 1,
    };
    let client = admin
        .client_async(crate::identity::IdentitySelector::Default)
        .await
        .unwrap();
    complete_provider_document_change(&client, &prepared.new_document, &committed)
        .await
        .unwrap();
    let public = client
        .runtime()
        .identity_session
        .as_ref()
        .unwrap()
        .public_identity()
        .await
        .unwrap();
    assert_eq!(
        canonical_hash(&public.document).unwrap(),
        committed.document_hash
    );

    let authorization =
        crate::internal::identity_device_join_runtime::DeviceJoinRemoteAuthorization {
            checkpoint: committed,
            device: crate::internal::identity_device_join_runtime::DeviceJoinRemoteDeviceSummary {
                device_id: started.join_request.device_id.clone(),
                signing_key_id: method_id(
                    &started.join_request.signing_public_key,
                    "join_request.signing_public_key",
                )
                .unwrap()
                .to_owned(),
                e2ee_key_id: method_id(
                    &started.join_request.e2ee_public_key,
                    "join_request.e2ee_public_key",
                )
                .unwrap()
                .to_owned(),
                status: crate::internal::identity_device_state::DeviceAuthorizationStatus::Active,
                role: crate::internal::identity_device_state::DeviceAuthorizationRole::Member,
                management_ready: false,
                auth_generation: 1,
            },
        };
    let session = mark_join_authorized_async(
        &admin,
        &started.session.join_session_id,
        &authorization,
        &prepared.new_document,
    )
    .await
    .unwrap();
    assert_eq!(session.phase, DeviceJoinLocalPhase::Authorized);

    let rejected = prepare_admin_rejection_async(
        &admin,
        crate::identity::IdentitySelector::Default,
        "another-join-session",
        crate::identity::DeviceJoinRejectReason::UserRejected,
    )
    .await
    .unwrap();
    assert_eq!(rejected.rejecting_device_id, "device-admin");
    assert!(!rejected.proof.proof_value.is_empty());

    let removed_document = provider_document_change_candidate(
        &client,
        json!({
            "changes": [{
                "change": "remove_device",
                "deviceId": started.join_request.device_id,
            }],
        }),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(
        anp::authentication::validate_device_manifest(&removed_document)
            .unwrap()
            .unwrap()
            .devices
            .iter()
            .all(|device| device.device_id != started.join_request.device_id)
    );
    let removed_checkpoint = crate::internal::identity_device_state::IdentityInternalCheckpoint {
        document_version: authorization.checkpoint.document_version + 1,
        document_hash: canonical_hash(&removed_document).unwrap(),
        registry_version: authorization.checkpoint.registry_version + 1,
    };
    complete_provider_document_change(&client, &removed_document, &removed_checkpoint)
        .await
        .unwrap();
}

#[tokio::test]
async fn admin_rejects_legacy_join_before_preparing_document_mutation() {
    let admin_root = tempfile::tempdir().unwrap();
    let candidate_root = tempfile::tempdir().unwrap();
    let (admin, document, did) = open_ready_admin_core(admin_root.path());
    let candidate = open_empty_vault_core(candidate_root.path());
    let document_hash = canonical_hash(&document).unwrap();
    let started = candidate
        .device_join()
        .start(
            DeviceJoinStartRequest {
                operation_id: "start-legacy-approval".to_owned(),
                did,
                ttl_seconds: 300,
            },
            &document,
        )
        .await
        .unwrap();
    let challenged = admin
        .device_join()
        .prepare_admin_challenge(DeviceJoinAdminPrepareRequest {
            admin_identity: crate::identity::IdentitySelector::Default,
            operation_id: "challenge-legacy-approval".to_owned(),
            join_request: started.join_request,
            challenge_ttl_seconds: 180,
            document_version: 7,
            document_hash: document_hash.clone(),
        })
        .unwrap();
    let responded = candidate
        .device_join()
        .respond_as_new_device(DeviceJoinNewDeviceRespondRequest {
            operation_id: "respond-legacy-approval".to_owned(),
            challenge: challenged.challenge,
            admin_did_document: document,
            document_version: 7,
            document_hash: document_hash.clone(),
        })
        .await
        .unwrap();
    admin
        .device_join()
        .verify_response_as_admin(DeviceJoinAdminVerifyRequest {
            operation_id: "verify-legacy-approval".to_owned(),
            join_session_id: started.session.join_session_id.clone(),
            response: responded.response,
        })
        .unwrap();

    let store = JoinStateStore::new(&admin);
    let mut stored = store
        .load(&started.session.join_session_id, DeviceJoinSide::Admin)
        .unwrap()
        .unwrap();
    stored.join_request.profiles = DEVICE_JOIN_LEGACY_DRAFT_PROFILES
        .iter()
        .map(|profile| (*profile).to_owned())
        .collect();
    stored.join_request_hash =
        canonical_hash(&serde_json::to_value(&stored.join_request).unwrap()).unwrap();
    store.save(&stored).unwrap();
    let checkpoint = crate::internal::identity_device_state::IdentityInternalCheckpoint {
        document_version: 7,
        document_hash,
        registry_version: 3,
    };

    let error = prepare_admin_approval(
        &admin,
        "approve-legacy-approval",
        &started.session.join_session_id,
        &checkpoint,
        &format_time(OffsetDateTime::now_utc()).unwrap(),
        true,
    )
    .unwrap_err();
    assert!(matches!(
        error,
        crate::ImError::InvalidInput {
            field: Some(field),
            ..
        } if field == "join_request.profiles"
    ));
    let unchanged = store
        .load(&started.session.join_session_id, DeviceJoinSide::Admin)
        .unwrap()
        .unwrap();
    assert_eq!(unchanged.phase, DeviceJoinLocalPhase::ResponseVerified);
    assert!(unchanged.approval.is_none());
}

#[tokio::test]
async fn local_new_device_sas_is_restart_safe_and_read_only() {
    let admin_root = tempfile::tempdir().unwrap();
    let candidate_root = tempfile::tempdir().unwrap();
    let (core, document, did) = open_ready_admin_core(admin_root.path());
    let candidate = open_empty_vault_core(candidate_root.path());
    let document_hash = canonical_hash(&document).unwrap();
    let started = candidate
        .device_join()
        .start(
            DeviceJoinStartRequest {
                operation_id: "start-candidate-progress".to_owned(),
                did,
                ttl_seconds: 300,
            },
            &document,
        )
        .await
        .unwrap();
    assert!(
        local_new_device_verification_sas(&candidate, &started.session.join_session_id,).is_err()
    );
    let prepared = core
        .device_join()
        .prepare_admin_challenge(DeviceJoinAdminPrepareRequest {
            admin_identity: crate::identity::IdentitySelector::Default,
            operation_id: "prepare-candidate-progress".to_owned(),
            join_request: started.join_request,
            challenge_ttl_seconds: 180,
            document_version: 7,
            document_hash: document_hash.clone(),
        })
        .unwrap();
    let responded = candidate
        .device_join()
        .respond_as_new_device(DeviceJoinNewDeviceRespondRequest {
            operation_id: "respond-candidate-progress".to_owned(),
            challenge: prepared.challenge,
            admin_did_document: document,
            document_version: 7,
            document_hash,
        })
        .await
        .unwrap();
    drop(candidate);

    let reopened = open_empty_vault_core(candidate_root.path());
    let before = JoinStateStore::new(&reopened)
        .load(&started.session.join_session_id, DeviceJoinSide::NewDevice)
        .unwrap()
        .unwrap();
    let recovered =
        local_new_device_verification_sas(&reopened, &started.session.join_session_id).unwrap();
    let after = JoinStateStore::new(&reopened)
        .load(&started.session.join_session_id, DeviceJoinSide::NewDevice)
        .unwrap()
        .unwrap();

    assert_eq!(recovered, responded.sas);
    assert_eq!(before, after, "local SAS read must not rewrite Join state");
}

#[test]
fn admin_join_projection_advances_checkpoint_and_is_repeat_safe() {
    let root = tempfile::tempdir().unwrap();
    let (core, document, did) = open_ready_admin_core(root.path());
    let checkpoint = crate::internal::identity_device_state::IdentityInternalCheckpoint {
        document_version: 8,
        document_hash: canonical_hash(&document).unwrap(),
        registry_version: 4,
    };

    commit_admin_join_projection(
        &core,
        &crate::identity::IdentitySelector::Default,
        &checkpoint,
        &document,
    )
    .unwrap();
    commit_admin_join_projection(
        &core,
        &crate::identity::IdentitySelector::Default,
        &checkpoint,
        &document,
    )
    .unwrap();

    let paths = test_paths(root.path());
    let index = crate::internal::identity_store::IdentityStore::new(&paths.identities)
        .load_index()
        .unwrap();
    let state = index
        .credentials
        .get("alice")
        .unwrap()
        .device_state
        .as_ref()
        .unwrap();
    assert_eq!(state.checkpoint.as_ref(), Some(&checkpoint));
    state.validate_for_did(&did).unwrap();
}

#[test]
fn admin_join_projection_adopts_published_document_into_anp_identity() {
    let root = tempfile::tempdir().unwrap();
    let (core, projected) = open_anp_ready_admin_core(root.path());
    let manager = crate::internal::identity_custody::open_controller_manager(&core).unwrap();
    let descriptor = manager
        .list()
        .unwrap()
        .into_iter()
        .find(|descriptor| descriptor.reference.did == projected.did.as_str())
        .unwrap();
    let mut identity = manager.get(&descriptor.reference).unwrap();
    let current = anp_identity::host::IdentityStatusPort::host_status(&identity)
        .unwrap()
        .checkpoint
        .unwrap();
    let signing_private = ed25519_dalek::SigningKey::from_bytes(&[91_u8; 32]);
    let mut signing_multikey = vec![0xed, 0x01];
    signing_multikey.extend_from_slice(&signing_private.verifying_key().to_bytes());
    let agreement_private = x25519_dalek::StaticSecret::from([92_u8; 32]);
    let mut agreement_multikey = vec![0xec, 0x01];
    agreement_multikey
        .extend_from_slice(&x25519_dalek::PublicKey::from(&agreement_private).to_bytes());
    let mut change = identity
        .prepare_document_change(anp_identity::DocumentChangeRequest {
            changes: vec![anp_identity::DocumentChange::AddDevice {
                device: anp_identity::DeviceInput {
                    device_id: "peer-device".to_owned(),
                    signing_key: anp_identity::PublicKeyInput {
                        kid: format!("{}#peer-sign", projected.did.as_str()),
                        public_key_multibase: format!(
                            "z{}",
                            bs58::encode(signing_multikey).into_string()
                        ),
                    },
                    agreement_key: anp_identity::PublicKeyInput {
                        kid: format!("{}#peer-e2ee", projected.did.as_str()),
                        public_key_multibase: format!(
                            "z{}",
                            bs58::encode(agreement_multikey).into_string()
                        ),
                    },
                    profiles: crate::internal::identity_generation::vnext_device_profiles(),
                },
            }],
        })
        .unwrap();
    let updated_document = change.candidate().candidate_document.clone().into_value();
    let attempt = change.begin_publication().unwrap();
    change
        .complete(
            attempt,
            anp_identity::PublicationResult::RejectedBeforeAcceptance,
        )
        .unwrap();
    drop(identity);
    drop(manager);
    let checkpoint = crate::internal::identity_device_state::IdentityInternalCheckpoint {
        document_version: current.document_version + 1,
        document_hash: canonical_hash(&updated_document).unwrap(),
        registry_version: current.registry_version + 1,
    };

    commit_admin_join_projection(
        &core,
        &crate::identity::IdentitySelector::Default,
        &checkpoint,
        &updated_document,
    )
    .unwrap();
    commit_admin_join_projection(
        &core,
        &crate::identity::IdentitySelector::Default,
        &checkpoint,
        &updated_document,
    )
    .unwrap();

    let manager = crate::internal::identity_custody::open_controller_manager(&core).unwrap();
    let descriptor = manager
        .list()
        .unwrap()
        .into_iter()
        .find(|descriptor| descriptor.reference.did == projected.did.as_str())
        .unwrap();
    let identity = manager.get(&descriptor.reference).unwrap();
    assert_eq!(
        identity.public_identity().unwrap().document.as_value(),
        &updated_document
    );
    assert_eq!(
        anp_identity::host::IdentityStatusPort::host_status(&identity)
            .unwrap()
            .checkpoint
            .unwrap()
            .document_digest,
        checkpoint.document_hash
    );
}

#[test]
fn admin_join_projection_rejects_checkpoint_regression() {
    let root = tempfile::tempdir().unwrap();
    let (core, document, _) = open_ready_admin_core(root.path());
    let regressed = crate::internal::identity_device_state::IdentityInternalCheckpoint {
        document_version: 6,
        document_hash: canonical_hash(&document).unwrap(),
        registry_version: 2,
    };

    assert!(matches!(
        commit_admin_join_projection(
            &core,
            &crate::identity::IdentitySelector::Default,
            &regressed,
            &document,
        ),
        Err(crate::ImError::PermissionDenied)
    ));

    let paths = test_paths(root.path());
    let index = crate::internal::identity_store::IdentityStore::new(&paths.identities)
        .load_index()
        .unwrap();
    let checkpoint = index
        .credentials
        .get("alice")
        .unwrap()
        .device_state
        .as_ref()
        .unwrap()
        .checkpoint
        .as_ref()
        .unwrap();
    assert_eq!(checkpoint.document_version, 7);
    assert_eq!(checkpoint.registry_version, 3);
}

#[tokio::test]
async fn recovery_join_accepts_missing_historical_generation_and_reopens_after_identity_save() {
    let admin_root = tempfile::tempdir().unwrap();
    let candidate_root = tempfile::tempdir().unwrap();
    let (admin, admin_document, current_did) = open_ready_admin_core(admin_root.path());
    let (candidate, _, previous_did) = open_ready_admin_core(candidate_root.path());
    let candidate_paths = test_paths(candidate_root.path());
    let mut registry: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&candidate_paths.identities.registry_path).unwrap())
            .unwrap();
    assert!(registry["credentials"]["alice"]
        .get("binding_generation")
        .is_none());
    let owner_id = registry["credentials"]["alice"]["unique_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let db = crate::internal::local_state::open_writable(&candidate_paths.local_state.sqlite_path)
        .unwrap();
    crate::internal::local_state::schema::ensure_schema(&db).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT COUNT(*) FROM identity_account_bindings WHERE owner_identity_id=?1",
            [&owner_id],
            |row| row.get::<_, i64>(0),
        )
        .unwrap(),
        0
    );
    drop(db);

    let started = candidate
        .device_join()
        .start(
            DeviceJoinStartRequest {
                operation_id: "joined-crash-start".to_owned(),
                did: current_did.clone(),
                ttl_seconds: 300,
            },
            &admin_document,
        )
        .await
        .unwrap();
    let join_request = started.join_request.clone();
    let marker =
        crate::internal::identity_transition_pending::IdentityTransitionMarker::joined_device(
            &candidate_paths.local_state.sqlite_path,
            &started.session.join_session_id,
            "user-1",
            &owner_id,
            "alice.awiki.test",
            previous_did.as_str(),
            current_did.as_str(),
            "8",
        )
        .unwrap();
    crate::internal::identity_transition_pending::persist(
        &candidate_paths.local_state.sqlite_path,
        &marker,
    )
    .unwrap();
    let admin_hash = canonical_hash(&admin_document).unwrap();
    let challenged = admin
        .device_join()
        .prepare_admin_challenge(DeviceJoinAdminPrepareRequest {
            admin_identity: crate::identity::IdentitySelector::Default,
            operation_id: "joined-crash-challenge".to_owned(),
            join_request: started.join_request,
            challenge_ttl_seconds: 180,
            document_version: 7,
            document_hash: admin_hash.clone(),
        })
        .unwrap();
    let responded = candidate
        .device_join()
        .respond_as_new_device(DeviceJoinNewDeviceRespondRequest {
            operation_id: "joined-crash-response".to_owned(),
            challenge: challenged.challenge,
            admin_did_document: admin_document,
            document_version: 7,
            document_hash: admin_hash.clone(),
        })
        .await
        .unwrap();
    admin
        .device_join()
        .verify_response_as_admin(DeviceJoinAdminVerifyRequest {
            operation_id: "joined-crash-verify".to_owned(),
            join_session_id: started.session.join_session_id.clone(),
            response: responded.response,
        })
        .unwrap();
    let expected_checkpoint = crate::internal::identity_device_state::IdentityInternalCheckpoint {
        document_version: 7,
        document_hash: admin_hash,
        registry_version: 3,
    };
    let approval = prepare_admin_approval(
        &admin,
        "joined-crash-approve",
        &started.session.join_session_id,
        &expected_checkpoint,
        &format_time(OffsetDateTime::now_utc()).unwrap(),
        true,
    )
    .unwrap();
    let authorization =
        crate::internal::identity_device_join_runtime::DeviceJoinRemoteAuthorization {
            checkpoint: crate::internal::identity_device_state::IdentityInternalCheckpoint {
                document_version: 8,
                document_hash: canonical_hash(&approval.new_document).unwrap(),
                registry_version: 4,
            },
            device: crate::internal::identity_device_join_runtime::DeviceJoinRemoteDeviceSummary {
                device_id: join_request.device_id.clone(),
                signing_key_id: method_id(&join_request.signing_public_key, "signing")
                    .unwrap()
                    .to_owned(),
                e2ee_key_id: method_id(&join_request.e2ee_public_key, "e2ee")
                    .unwrap()
                    .to_owned(),
                status: crate::internal::identity_device_state::DeviceAuthorizationStatus::Active,
                role: crate::internal::identity_device_state::DeviceAuthorizationRole::Member,
                management_ready: false,
                auth_generation: 1,
            },
        };
    mark_join_authorized_async(
        &admin,
        &started.session.join_session_id,
        &authorization,
        &approval.new_document,
    )
    .await
    .unwrap();
    prepare_new_device_activation_async(
        &candidate,
        &started.session.join_session_id,
        &authorization,
        &approval.new_document,
    )
    .await
    .unwrap();
    record_new_device_access_result(
        &candidate,
        &started.session.join_session_id,
        crate::internal::identity_device_join_runtime::DeviceJoinAccessResult {
            user_id: "user-1".to_owned(),
            access_token: member_access_token(
                current_did.as_str(),
                &authorization.device.device_id,
                &authorization.device.signing_key_id,
            ),
        },
    )
    .unwrap();

    let session_before_conflicts = JoinStateStore::new(&candidate)
        .load(&started.session.join_session_id, DeviceJoinSide::NewDevice)
        .unwrap()
        .unwrap();
    let pending_before_conflicts =
        load_pending_new_device_activation(&candidate, &started.session.join_session_id)
            .unwrap()
            .unwrap();
    assert!(pending_before_conflicts.access_result.is_some());
    let marker_before_conflicts = crate::internal::identity_transition_pending::load_joined_device(
        &candidate_paths.local_state.sqlite_path,
        &started.session.join_session_id,
    )
    .unwrap()
    .unwrap();

    registry["credentials"]["alice"]["binding_generation"] = json!("6");
    std::fs::write(
        &candidate_paths.identities.registry_path,
        serde_json::to_vec_pretty(&registry).unwrap(),
    )
    .unwrap();
    assert!(matches!(
        finalize_new_device_activation_async(&candidate, &started.session.join_session_id).await,
        Err(crate::ImError::Service {
            status_code: None,
            code: Some(code),
            ..
        }) if code == "unknown_epoch"
    ));
    assert_eq!(
        JoinStateStore::new(&candidate)
            .load(&started.session.join_session_id, DeviceJoinSide::NewDevice,)
            .unwrap()
            .unwrap(),
        session_before_conflicts
    );
    assert_eq!(
        load_pending_new_device_activation(&candidate, &started.session.join_session_id)
            .unwrap()
            .unwrap(),
        pending_before_conflicts
    );
    assert_eq!(
        crate::internal::identity_transition_pending::load_joined_device(
            &candidate_paths.local_state.sqlite_path,
            &started.session.join_session_id,
        )
        .unwrap()
        .unwrap(),
        marker_before_conflicts
    );
    let index_after_generation_conflict =
        crate::internal::identity_store::IdentityStore::new(&candidate_paths.identities)
            .load_index()
            .unwrap();
    let entry_after_generation_conflict = index_after_generation_conflict
        .credentials
        .get("alice")
        .unwrap();
    assert_eq!(entry_after_generation_conflict.did, previous_did.as_str());
    assert_eq!(
        entry_after_generation_conflict
            .binding_generation
            .as_deref(),
        Some("6")
    );

    registry["credentials"]["alice"]
        .as_object_mut()
        .unwrap()
        .remove("binding_generation");
    std::fs::write(
        &candidate_paths.identities.registry_path,
        serde_json::to_vec_pretty(&registry).unwrap(),
    )
    .unwrap();
    let db = crate::internal::local_state::open_writable(&candidate_paths.local_state.sqlite_path)
        .unwrap();
    db.execute(
        "INSERT INTO identity_account_bindings(owner_identity_id,account_id,handle_scope,current_did,device_id,identity_generation,device_auth_generation,created_at,updated_at) VALUES (?1,'user-1','alice.awiki.test',?2,'previous-device','6','1',1,1)",
        rusqlite::params![owner_id, previous_did.as_str()],
    )
    .unwrap();
    drop(db);
    assert!(matches!(
        finalize_new_device_activation_async(&candidate, &started.session.join_session_id).await,
        Err(crate::ImError::PermissionDenied)
    ));
    assert_eq!(
        JoinStateStore::new(&candidate)
            .load(&started.session.join_session_id, DeviceJoinSide::NewDevice,)
            .unwrap()
            .unwrap(),
        session_before_conflicts
    );
    assert_eq!(
        load_pending_new_device_activation(&candidate, &started.session.join_session_id)
            .unwrap()
            .unwrap(),
        pending_before_conflicts
    );
    assert_eq!(
        crate::internal::identity_transition_pending::load_joined_device(
            &candidate_paths.local_state.sqlite_path,
            &started.session.join_session_id,
        )
        .unwrap()
        .unwrap(),
        marker_before_conflicts
    );
    let db = crate::internal::local_state::open_writable(&candidate_paths.local_state.sqlite_path)
        .unwrap();
    let conflicting_binding = db
        .query_row(
            "SELECT current_did,identity_generation FROM identity_account_bindings WHERE owner_identity_id=?1",
            [&owner_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .unwrap();
    assert_eq!(
        conflicting_binding,
        (previous_did.as_str().to_owned(), "6".to_owned())
    );
    db.execute(
        "DELETE FROM identity_account_bindings WHERE owner_identity_id=?1",
        [&owner_id],
    )
    .unwrap();
    drop(db);

    FAIL_AFTER_RECOVERY_JOIN_IDENTITY_SAVE.store(true, std::sync::atomic::Ordering::SeqCst);
    assert!(matches!(
        finalize_new_device_activation_async(&candidate, &started.session.join_session_id).await,
        Err(crate::ImError::Internal { message })
            if message == "injected crash after recovery Join identity save"
    ));
    drop(candidate);

    // A Group repair blocked for this exact identity epoch must remain in the
    // Group journal without holding the joined-device identity transition
    // open after restart.
    let group_did = "did:wba:awiki.test:groups:engineering";
    let group_job_id = "historical-group-rebind-job";
    let db = crate::internal::local_state::open_writable(&candidate_paths.local_state.sqlite_path)
        .unwrap();
    db.execute(
        r#"INSERT INTO group_rebind_outbox
(job_id, owner_identity_id, group_did, member_handle, previous_member_did,
 new_member_did, binding_generation, phase, created_at, updated_at)
VALUES (?1,?2,?3,'alice.awiki.test',?4,?5,'8','blocked','now','now')"#,
        rusqlite::params![
            group_job_id,
            owner_id,
            group_did,
            previous_did.as_str(),
            current_did.as_str(),
        ],
    )
    .unwrap();
    drop(db);

    let reopened = reopen_join_test_core(candidate_root.path());
    finalize_new_device_activation_async(&reopened, &started.session.join_session_id)
        .await
        .unwrap();
    let marker = crate::internal::identity_transition_pending::load_joined_device(
        &candidate_paths.local_state.sqlite_path,
        &started.session.join_session_id,
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        marker.phase,
        crate::internal::identity_transition_pending::TransitionPhase::Completed
    );
    assert_eq!(
        marker.contract_version,
        crate::internal::identity_handle_recovery_pending::V4_CONTRACT_VERSION
    );
    let index = crate::internal::identity_store::IdentityStore::new(&candidate_paths.identities)
        .load_index()
        .unwrap();
    let matches = index
        .credentials
        .values()
        .filter(|entry| entry.unique_id == owner_id)
        .collect::<Vec<_>>();
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].did, current_did.as_str());
    assert_eq!(matches[0].binding_generation.as_deref(), Some("8"));
    assert_eq!(
        matches[0].identity_custody_backend.as_deref(),
        Some("anp_identity")
    );
    assert!(matches[0].anp_identity_store_id.is_some());
    assert!(matches[0].anp_identity_id.is_some());
    let runtime = reopened
        .identities()
        .load_runtime(crate::identity::IdentitySelector::Did(current_did.clone()))
        .unwrap();
    runtime
        .key_provider
        .sign(&authorization.device.signing_key_id, b"joined device")
        .unwrap();
    assert_eq!(
        runtime.key_provider.ensure_root_control_available(),
        Err(crate::ImError::PermissionDenied)
    );
    drop(reopened);
    let recovery_core = reopen_join_test_recovery_core(candidate_root.path());
    let receipt = super::super::identity_handle_recovery_runtime::authorized_receipt(
        &recovery_core,
        crate::identity::IdentitySelector::Did(current_did.clone()),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(
        receipt.source_kind,
        crate::identity::HandleRecoveryTransitionSourceKind::JoinedDevice
    );
    assert_eq!(receipt.source_id, started.session.join_session_id);
    assert_eq!(receipt.current_did, current_did);
    assert_eq!(
        receipt.current_device_id.as_str(),
        authorization.device.device_id
    );
    assert_eq!(receipt.device_auth_generation, 1);
    assert_eq!(receipt.registry_version, 4);

    let db = crate::internal::local_state::open_writable(&candidate_paths.local_state.sqlite_path)
        .unwrap();
    db.execute(
        "UPDATE identity_transition_pending SET contract_version=?1,contract_hash=?2 WHERE recovery_id=?3",
        rusqlite::params![
            "unsupported-handle-recovery-contract",
            "0".repeat(64),
            marker.recovery_id,
        ],
    )
    .unwrap();
    drop(db);
    assert_eq!(
        super::super::identity_handle_recovery_runtime::authorized_receipt(
            &recovery_core,
            crate::identity::IdentitySelector::Did(receipt.current_did.clone()),
        )
        .await
        .unwrap_err(),
        crate::ImError::PermissionDenied
    );
    let db = rusqlite::Connection::open_with_flags(
        &candidate_paths.local_state.sqlite_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let legacy_group_journal: (String, i64) = db
        .query_row(
            "SELECT phase,attempt_count FROM group_rebind_outbox WHERE job_id=?1",
            [group_job_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(legacy_group_journal, ("blocked".to_owned(), 0));
    let binding = db
        .query_row(
            "SELECT account_id,handle_scope,current_did,device_id,identity_generation,device_auth_generation FROM identity_account_bindings WHERE owner_identity_id=?1",
            [&owner_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(
        binding,
        (
            "user-1".to_owned(),
            Some("alice.awiki.test".to_owned()),
            current_did.as_str().to_owned(),
            authorization.device.device_id,
            "8".to_owned(),
            "1".to_owned(),
        )
    );
}
