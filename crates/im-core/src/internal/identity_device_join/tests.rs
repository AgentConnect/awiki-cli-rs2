use super::*;
use serde_json::json;
use std::path::Path;
use std::sync::Arc;

fn test_config() -> crate::ImCoreConfig {
    crate::ImCoreConfig {
        service_base_url: crate::ServiceEndpoint::parse("https://example.test").unwrap(),
        did_domain: "awiki.test".to_owned(),
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

#[test]
fn join_start_emits_mixed_profiles() {
    let candidate_root = tempfile::tempdir().unwrap();
    let candidate = open_empty_vault_core(candidate_root.path());
    let started = candidate
        .device_join()
        .start(DeviceJoinStartRequest {
            operation_id: "start-mixed-profiles".to_owned(),
            did: crate::ids::Did::parse("did:wba:example.com:user:alice").unwrap(),
            ttl_seconds: 300,
        })
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

#[test]
fn local_admin_verification_progress_is_phase_gated_and_read_only() {
    let admin_root = tempfile::tempdir().unwrap();
    let candidate_root = tempfile::tempdir().unwrap();
    let (core, document, did) = open_ready_admin_core(admin_root.path());
    let candidate = open_empty_vault_core(candidate_root.path());
    let document_hash = canonical_hash(&document).unwrap();
    let started = candidate
        .device_join()
        .start(DeviceJoinStartRequest {
            operation_id: "start-local-progress".to_owned(),
            did,
            ttl_seconds: 300,
        })
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

#[test]
fn admin_rejects_legacy_join_before_preparing_document_mutation() {
    let admin_root = tempfile::tempdir().unwrap();
    let candidate_root = tempfile::tempdir().unwrap();
    let (admin, document, did) = open_ready_admin_core(admin_root.path());
    let candidate = open_empty_vault_core(candidate_root.path());
    let document_hash = canonical_hash(&document).unwrap();
    let started = candidate
        .device_join()
        .start(DeviceJoinStartRequest {
            operation_id: "start-legacy-approval".to_owned(),
            did,
            ttl_seconds: 300,
        })
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

#[test]
fn local_new_device_sas_is_restart_safe_and_read_only() {
    let admin_root = tempfile::tempdir().unwrap();
    let candidate_root = tempfile::tempdir().unwrap();
    let (core, document, did) = open_ready_admin_core(admin_root.path());
    let candidate = open_empty_vault_core(candidate_root.path());
    let document_hash = canonical_hash(&document).unwrap();
    let started = candidate
        .device_join()
        .start(DeviceJoinStartRequest {
            operation_id: "start-candidate-progress".to_owned(),
            did,
            ttl_seconds: 300,
        })
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

#[test]
fn recovery_join_accepts_missing_historical_generation_and_reopens_after_identity_save() {
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
        .start(DeviceJoinStartRequest {
            operation_id: "joined-crash-start".to_owned(),
            did: current_did.clone(),
            ttl_seconds: 300,
        })
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
    prepare_new_device_activation(
        &candidate,
        &started.session.join_session_id,
        &authorization,
        &approval.new_document,
    )
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
        finalize_new_device_activation(&candidate, &started.session.join_session_id),
        Err(crate::ImError::Service {
            status_code: None,
            code: Some(code),
            ..
        }) if code == "handle_recovery_transition_mismatch"
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
        finalize_new_device_activation(&candidate, &started.session.join_session_id),
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
        finalize_new_device_activation(&candidate, &started.session.join_session_id),
        Err(crate::ImError::Internal { message })
            if message == "injected crash after recovery Join identity save"
    ));
    drop(candidate);

    let reopened = reopen_join_test_core(candidate_root.path());
    finalize_new_device_activation(&reopened, &started.session.join_session_id).unwrap();
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
    let db = rusqlite::Connection::open_with_flags(
        &candidate_paths.local_state.sqlite_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
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
