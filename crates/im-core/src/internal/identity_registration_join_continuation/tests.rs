use super::*;
use std::path::Path;
use std::sync::Arc;

struct Fixture {
    core: crate::ImCore,
    transition: crate::internal::identity_registration_join_preparation::RegistrationJoinTransition,
    full_handle: crate::ids::Handle,
    join_session_id: String,
}

fn paths(root: &Path) -> crate::ImCorePaths {
    crate::ImCorePaths {
        identities: crate::IdentityRegistryPaths {
            identity_root_dir: root.join("identities"),
            registry_path: root.join("identities/registry.json"),
            default_identity_path: Some(root.join("identities/default")),
        },
        local_state: crate::LocalStatePaths {
            sqlite_path: root.join("local/im.sqlite"),
        },
        runtime: crate::RuntimePaths {
            cache_dir: root.join("cache"),
            temp_dir: root.join("tmp"),
        },
    }
}

fn open_core(root: &Path) -> crate::ImCore {
    crate::ImCore::new_with_options(
        crate::ImCoreConfig {
            service_base_url: crate::ServiceEndpoint::parse("https://awiki.test").unwrap(),
            did_domain: "awiki.test".to_owned(),
            client_version_info: None,
            user_service_endpoint: None,
            message_service_endpoint: None,
            mail_service_endpoint: None,
            anp_service_endpoint: None,
            anp_service_did: None,
            ca_bundle: None,
            transport_policy: crate::MessageTransportPolicy::HttpOnly,
        },
        paths(root),
        crate::ImCoreOpenOptions::default().with_identity_secret_vault(
            crate::IdentitySecretStoragePolicy::VaultRequired,
            crate::ImCoreSecretVaultOptions::new(
                crate::vault::DeviceVaultRootKey::from_bytes([73_u8; 32]),
                root.join("vault"),
                "continuation-workspace",
                "continuation-vault-device",
            ),
        ),
    )
    .unwrap()
}

async fn fixture(root: &Path) -> Fixture {
    use crate::internal::identity_device_state::{
        DeviceAuthorizationProjection, DeviceAuthorizationRole, DeviceAuthorizationStatus,
        IdentityDeviceMode, IdentityDeviceState, IdentityInternalCheckpoint,
        IDENTITY_DEVICE_STATE_SCHEMA_VERSION,
    };
    let old = crate::internal::identity_generation::generate_vnext_handle_identity_with_default_daemon_subkey(
        "awiki.test",
        "alice-old",
        None,
        None,
    )
    .unwrap();
    let current = crate::internal::identity_generation::generate_vnext_handle_identity_with_default_daemon_subkey(
        "awiki.test",
        "alice-current",
        None,
        None,
    )
    .unwrap();
    let sdk_paths = paths(root);
    let vault = Arc::new(crate::vault::FileSecretVault::new(
        crate::vault::DeviceVaultRootKey::from_bytes([73_u8; 32]),
        crate::vault::FileSecretVaultStore::new(root.join("vault")),
    ));
    crate::internal::identity_store::IdentityStore::new(&sdk_paths.identities)
        .save_identity_with_secret_storage(
            crate::internal::identity_store::SaveIdentityInput {
                local_alias: "alice".to_owned(),
                did: old.did.clone(),
                unique_id: old.unique_id.clone(),
                user_id: "user-1".to_owned(),
                display_name: "Alice".to_owned(),
                handle: "alice".to_owned(),
                full_handle: "alice.awiki.test".to_owned(),
                binding_generation: Some("1".to_owned()),
                jwt_token: "access-token".to_owned(),
                did_document: Some(old.did_document.clone()),
                key_mode: crate::internal::identity_store::SaveIdentityKeyMode::VNext {
                    root_key_id: old.root_key_id.clone(),
                    device_signing_key_id: old.device_signing_key_id.clone(),
                    device_e2ee_key_id: old.device_e2ee_key_id.clone(),
                },
                device_state: Some(IdentityDeviceState {
                    schema_version: IDENTITY_DEVICE_STATE_SCHEMA_VERSION,
                    mode: IdentityDeviceMode::VNext,
                    authorization: Some(DeviceAuthorizationProjection {
                        protocol_device_id: old.protocol_device_id.clone(),
                        signing_key_id: old.device_signing_key_id.clone(),
                        e2ee_key_id: old.device_e2ee_key_id.clone(),
                        status: DeviceAuthorizationStatus::Active,
                        role: DeviceAuthorizationRole::Admin,
                        management_ready: true,
                        auth_generation: 1,
                    }),
                    checkpoint: Some(IdentityInternalCheckpoint {
                        document_version: 1,
                        document_hash: crate::internal::identity_wire::document::document_hash(
                            &old.did_document,
                        )
                        .unwrap(),
                        registry_version: 1,
                    }),
                }),
                key1_private_pem: old.root_private_pem,
                key1_public_pem: old.root_public_pem,
                e2ee_signing_private_pem: old.device_signing_private_pem,
                e2ee_agreement_private_pem: old.device_e2ee_private_pem,
                daemon_subkey_package: Some(old.daemon_subkey_package),
                make_default: true,
            },
            crate::internal::identity_store::SaveIdentitySecretStorage::Vault {
                workspace_id: "continuation-workspace".to_owned(),
                device_id: "continuation-vault-device".to_owned(),
                vault,
            },
        )
        .unwrap();
    let core = open_core(root);
    let started = core
        .device_join()
        .start(
            crate::identity::DeviceJoinStartRequest {
                operation_id: "continuation-create".to_owned(),
                did: current.did.clone(),
                ttl_seconds: 300,
            },
            &current.did_document,
        )
        .await
        .unwrap();
    let transition =
        crate::internal::identity_registration_join_preparation::RegistrationJoinTransition {
            account_user_id: "user-1".to_owned(),
            previous_did: old.did.as_str().to_owned(),
            current_did: current.did.as_str().to_owned(),
            binding_generation: "2".to_owned(),
        };
    let full_handle = crate::ids::Handle::parse("alice.awiki.test", "").unwrap();
    let marker =
        crate::internal::identity_transition_pending::IdentityTransitionMarker::joined_device(
            &core.inner().sdk_paths().local_state.sqlite_path,
            &started.session.join_session_id,
            &transition.account_user_id,
            &old.unique_id,
            full_handle.as_str(),
            &transition.previous_did,
            &transition.current_did,
            &transition.binding_generation,
        )
        .unwrap();
    crate::internal::identity_transition_pending::persist(
        &core.inner().sdk_paths().local_state.sqlite_path,
        &marker,
    )
    .unwrap();
    Fixture {
        core,
        transition,
        full_handle,
        join_session_id: started.session.join_session_id,
    }
}

#[tokio::test]
async fn registration_releases_local_only_join_before_retry() {
    let root = tempfile::tempdir().unwrap();
    let fixture = fixture(root.path()).await;
    assert_eq!(
        resolve(&fixture.core, &fixture.transition, &fixture.full_handle).unwrap(),
        RegistrationJoinContinuation::TerminalCleanupThenRetry {
            join_session_id: fixture.join_session_id.clone(),
            reason: "cleanup_local_only",
        }
    );
    crate::internal::identity_device_join::abort_local_only_registration_join(
        &fixture.core,
        &fixture.join_session_id,
    )
    .await
    .unwrap();
    assert!(
        crate::internal::identity_transition_pending::load_joined_device(
            &fixture.core.inner().sdk_paths().local_state.sqlite_path,
            &fixture.join_session_id,
        )
        .unwrap()
        .is_none()
    );
    let terminal = crate::internal::identity_device_join::registration_join_session_evidence(
        &fixture.core,
        &fixture.join_session_id,
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        terminal.phase,
        crate::identity::DeviceJoinLocalPhase::Cancelled
    );
    assert_eq!(
        terminal.terminal_evidence,
        Some(crate::internal::identity_device_join::JoinTerminalEvidence::LocalOnlyAbort)
    );
}

#[tokio::test]
async fn core_open_completes_local_only_abort_and_marker_cleanup() {
    let root = tempfile::tempdir().unwrap();
    let fixture = fixture(root.path()).await;
    let join_session_id = fixture.join_session_id.clone();
    drop(fixture);

    let reopened = open_core(root.path());
    assert!(
        crate::internal::identity_transition_pending::load_joined_device(
            &reopened.inner().sdk_paths().local_state.sqlite_path,
            &join_session_id,
        )
        .unwrap()
        .is_none()
    );
    let terminal = crate::internal::identity_device_join::registration_join_session_evidence(
        &reopened,
        &join_session_id,
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        terminal.phase,
        crate::identity::DeviceJoinLocalPhase::Cancelled
    );
    assert_eq!(
        terminal.terminal_evidence,
        Some(crate::internal::identity_device_join::JoinTerminalEvidence::LocalOnlyAbort)
    );
}

#[tokio::test]
async fn core_open_preserves_attempting_join_for_registration_resume() {
    let root = tempfile::tempdir().unwrap();
    let fixture = fixture(root.path()).await;
    crate::internal::identity_device_join::mark_new_device_remote_create_attempting(
        &fixture.core,
        &fixture.join_session_id,
    )
    .unwrap();
    let transition = fixture.transition.clone();
    let full_handle = fixture.full_handle.clone();
    let join_session_id = fixture.join_session_id.clone();
    drop(fixture);

    let reopened = open_core(root.path());
    let RegistrationJoinContinuation::Resume(evidence) =
        resolve(&reopened, &transition, &full_handle).unwrap()
    else {
        panic!("Core open did not preserve the attempting Join")
    };
    assert_eq!(evidence.join_session_id, join_session_id);
    assert_eq!(evidence.reason, "resume_attempting");
}

#[tokio::test]
async fn core_open_completes_joined_identity_switched_marker() {
    let root = tempfile::tempdir().unwrap();
    let fixture = fixture(root.path()).await;
    let sqlite_path = fixture
        .core
        .inner()
        .sdk_paths()
        .local_state
        .sqlite_path
        .clone();
    let marker = crate::internal::identity_transition_pending::load_joined_device(
        &sqlite_path,
        &fixture.join_session_id,
    )
    .unwrap()
    .unwrap();
    crate::internal::identity_device_join::test_set_activation_pending_for_identity_switch(
        &fixture.core,
        &fixture.join_session_id,
    )
    .unwrap();
    let identity_store = crate::internal::identity_store::IdentityStore::new(
        &fixture.core.inner().sdk_paths().identities,
    );
    let lock = identity_store.lock_index_mutation().unwrap();
    let mut index = identity_store.load_index().unwrap();
    let entry = index
        .credentials
        .values_mut()
        .find(|entry| entry.unique_id == marker.owner_identity_id)
        .unwrap();
    entry.did = marker.current_did.clone();
    entry.binding_generation = Some(marker.binding_generation.clone());
    identity_store.save_index_locked(&lock, index).unwrap();
    drop(lock);
    crate::internal::identity_transition_pending::update_phase(
        &sqlite_path,
        &marker.recovery_id,
        crate::internal::identity_transition_pending::TransitionPhase::Pending,
        crate::internal::identity_transition_pending::TransitionPhase::IdentitySwitched,
    )
    .unwrap();
    let join_session_id = fixture.join_session_id.clone();
    drop(fixture);

    let reopened = open_core(root.path());
    let completed = crate::internal::identity_transition_pending::load_joined_device(
        &reopened.inner().sdk_paths().local_state.sqlite_path,
        &join_session_id,
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        completed.phase,
        crate::internal::identity_transition_pending::TransitionPhase::Completed
    );
    let session = crate::internal::identity_device_join::registration_join_session_evidence(
        &reopened,
        &join_session_id,
    )
    .unwrap()
    .unwrap();
    assert!(session.activation_pending);
}

#[tokio::test]
async fn core_open_completes_remote_terminal_cleanup_and_keeps_audit_session() {
    let root = tempfile::tempdir().unwrap();
    let fixture = fixture(root.path()).await;
    crate::internal::identity_device_join::mark_new_device_remote_create_attempting(
        &fixture.core,
        &fixture.join_session_id,
    )
    .unwrap();
    let intent = crate::internal::identity_device_join::new_device_remote_create_intent(
        &fixture.core,
        &fixture.join_session_id,
    )
    .unwrap();
    crate::internal::identity_device_join::bind_new_device_remote_session(
        &fixture.core,
        &fixture.join_session_id,
        &crate::internal::platform_secret::SecretBytes::from_vec(b"terminal-token".to_vec()),
        &intent.session.expires_at,
    )
    .unwrap();
    crate::internal::identity_device_join::test_set_remote_terminal_without_cleanup(
        &fixture.core,
        &fixture.join_session_id,
        crate::internal::identity_device_join::JoinTerminalEvidence::RemoteCancelled,
    )
    .unwrap();
    let join_session_id = fixture.join_session_id.clone();
    drop(fixture);

    let reopened = open_core(root.path());
    assert!(
        crate::internal::identity_transition_pending::load_joined_device(
            &reopened.inner().sdk_paths().local_state.sqlite_path,
            &join_session_id,
        )
        .unwrap()
        .is_none()
    );
    let terminal = crate::internal::identity_device_join::registration_join_session_evidence(
        &reopened,
        &join_session_id,
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        terminal.phase,
        crate::identity::DeviceJoinLocalPhase::Cancelled
    );
    assert_eq!(
        terminal.terminal_evidence,
        Some(crate::internal::identity_device_join::JoinTerminalEvidence::RemoteCancelled)
    );
    assert!(
        crate::internal::identity_device_join::open_new_device_remote_session_token(
            &reopened,
            &join_session_id,
        )
        .is_err()
    );
}

#[tokio::test]
async fn terminal_cleanup_failure_keeps_exact_marker_for_restart_repair() {
    let root = tempfile::tempdir().unwrap();
    let fixture = fixture(root.path()).await;
    crate::internal::identity_device_join::mark_new_device_remote_create_attempting(
        &fixture.core,
        &fixture.join_session_id,
    )
    .unwrap();
    let intent = crate::internal::identity_device_join::new_device_remote_create_intent(
        &fixture.core,
        &fixture.join_session_id,
    )
    .unwrap();
    crate::internal::identity_device_join::bind_new_device_remote_session(
        &fixture.core,
        &fixture.join_session_id,
        &crate::internal::platform_secret::SecretBytes::from_vec(b"terminal-token".to_vec()),
        &intent.session.expires_at,
    )
    .unwrap();
    crate::internal::identity_device_join::test_corrupt_join_custody_enrollment(
        &fixture.core,
        &fixture.join_session_id,
    )
    .unwrap();
    crate::internal::identity_device_join::test_set_remote_terminal_without_cleanup(
        &fixture.core,
        &fixture.join_session_id,
        crate::internal::identity_device_join::JoinTerminalEvidence::RemoteRejected,
    )
    .unwrap();
    assert!(
        crate::internal::identity_device_join::cleanup_terminal_registration_join(
            &fixture.core,
            &fixture.join_session_id,
        )
        .await
        .is_err()
    );
    assert!(
        crate::internal::identity_transition_pending::load_joined_device(
            &fixture.core.inner().sdk_paths().local_state.sqlite_path,
            &fixture.join_session_id,
        )
        .unwrap()
        .is_some()
    );
    let terminal = crate::internal::identity_device_join::registration_join_session_evidence(
        &fixture.core,
        &fixture.join_session_id,
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        terminal.phase,
        crate::identity::DeviceJoinLocalPhase::Cancelled
    );
    assert_eq!(
        terminal.terminal_evidence,
        Some(crate::internal::identity_device_join::JoinTerminalEvidence::RemoteRejected)
    );
}

#[tokio::test]
async fn registration_adopts_exact_pending_join_after_preparation_loss() {
    let root = tempfile::tempdir().unwrap();
    let fixture = fixture(root.path()).await;
    crate::internal::identity_device_join::mark_new_device_remote_create_attempting(
        &fixture.core,
        &fixture.join_session_id,
    )
    .unwrap();
    let RegistrationJoinContinuation::Resume(evidence) =
        resolve(&fixture.core, &fixture.transition, &fixture.full_handle).unwrap()
    else {
        panic!("expected exact Resume continuation")
    };
    assert_eq!(evidence.join_session_id, fixture.join_session_id);
    assert_eq!(evidence.reason, "resume_attempting");
}

#[tokio::test]
async fn registration_adoption_returns_same_bound_join_session_id() {
    let root = tempfile::tempdir().unwrap();
    let fixture = fixture(root.path()).await;
    crate::internal::identity_device_join::mark_new_device_remote_create_attempting(
        &fixture.core,
        &fixture.join_session_id,
    )
    .unwrap();
    let intent = crate::internal::identity_device_join::new_device_remote_create_intent(
        &fixture.core,
        &fixture.join_session_id,
    )
    .unwrap();
    crate::internal::identity_device_join::bind_new_device_remote_session(
        &fixture.core,
        &fixture.join_session_id,
        &crate::internal::platform_secret::SecretBytes::from_vec(b"join-token".to_vec()),
        &intent.session.expires_at,
    )
    .unwrap();
    let RegistrationJoinContinuation::Resume(evidence) =
        resolve(&fixture.core, &fixture.transition, &fixture.full_handle).unwrap()
    else {
        panic!("expected bound Resume continuation")
    };
    assert_eq!(evidence.join_session_id, fixture.join_session_id);
    assert_eq!(evidence.reason, "resume_bound");
}

#[tokio::test]
async fn legacy_cancelled_before_deadline_returns_terminal_wait() {
    let root = tempfile::tempdir().unwrap();
    let fixture = fixture(root.path()).await;
    crate::internal::identity_device_join::test_set_legacy_terminal_join(
        &fixture.core,
        &fixture.join_session_id,
        false,
    )
    .unwrap();
    assert_eq!(
        resolve(&fixture.core, &fixture.transition, &fixture.full_handle).unwrap(),
        RegistrationJoinContinuation::WaitForLegacyDeadline
    );
    assert!(
        crate::internal::identity_transition_pending::load_joined_device(
            &fixture.core.inner().sdk_paths().local_state.sqlite_path,
            &fixture.join_session_id,
        )
        .unwrap()
        .is_some()
    );
}

#[tokio::test]
async fn deadline_elapsed_releases_tokenless_unknown_marker() {
    let root = tempfile::tempdir().unwrap();
    let fixture = fixture(root.path()).await;
    crate::internal::identity_device_join::test_set_legacy_terminal_join(
        &fixture.core,
        &fixture.join_session_id,
        true,
    )
    .unwrap();
    assert!(matches!(
        resolve(&fixture.core, &fixture.transition, &fixture.full_handle).unwrap(),
        RegistrationJoinContinuation::TerminalCleanupThenRetry { .. }
    ));
    crate::internal::identity_device_join::cleanup_terminal_registration_join(
        &fixture.core,
        &fixture.join_session_id,
    )
    .await
    .unwrap();
    assert!(
        crate::internal::identity_transition_pending::load_joined_device(
            &fixture.core.inner().sdk_paths().local_state.sqlite_path,
            &fixture.join_session_id,
        )
        .unwrap()
        .is_none()
    );
    let terminal = crate::internal::identity_device_join::registration_join_session_evidence(
        &fixture.core,
        &fixture.join_session_id,
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        terminal.phase,
        crate::identity::DeviceJoinLocalPhase::Expired
    );
    assert_eq!(
        terminal.terminal_evidence,
        Some(
            crate::internal::identity_device_join::JoinTerminalEvidence::AuthoritativeDeadlineElapsed
        )
    );
}

#[tokio::test]
async fn registration_adoption_rejects_marker_session_mismatch() {
    let root = tempfile::tempdir().unwrap();
    let fixture = fixture(root.path()).await;
    let marker = crate::internal::identity_transition_pending::load_joined_device(
        &fixture.core.inner().sdk_paths().local_state.sqlite_path,
        &fixture.join_session_id,
    )
    .unwrap()
    .unwrap();
    crate::internal::identity_transition_pending::delete_joined_device_terminal(
        &fixture.core.inner().sdk_paths().local_state.sqlite_path,
        &marker,
    )
    .unwrap();
    let mismatched =
        crate::internal::identity_transition_pending::IdentityTransitionMarker::joined_device(
            &fixture.core.inner().sdk_paths().local_state.sqlite_path,
            "missing-session",
            &fixture.transition.account_user_id,
            &marker.owner_identity_id,
            fixture.full_handle.as_str(),
            &fixture.transition.previous_did,
            &fixture.transition.current_did,
            &fixture.transition.binding_generation,
        )
        .unwrap();
    crate::internal::identity_transition_pending::persist(
        &fixture.core.inner().sdk_paths().local_state.sqlite_path,
        &mismatched,
    )
    .unwrap();
    assert_eq!(
        resolve(&fixture.core, &fixture.transition, &fixture.full_handle).unwrap(),
        RegistrationJoinContinuation::Conflict
    );
}
