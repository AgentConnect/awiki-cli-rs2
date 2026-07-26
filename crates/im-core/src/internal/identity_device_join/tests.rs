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
