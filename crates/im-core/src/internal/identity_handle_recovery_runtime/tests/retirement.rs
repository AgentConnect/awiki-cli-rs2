use super::*;

#[tokio::test]
async fn deleted_recovered_owner_does_not_block_a_fresh_recovery_operation() {
    let root = tempfile::tempdir().unwrap();
    // An unreachable loopback endpoint keeps this test local. Reaching the OTP
    // transport proves that provisioning and the new operation index succeeded.
    let core = recovery_test_core(root.path(), "http://127.0.0.1:1", [93_u8; 32]);
    let owner = "owner-retired-recovery";
    let operation_id = "recover-v4-retired-owner";
    let mut pending = v4_awaiting_factor_pending(operation_id, owner);
    pending.identity = crate::internal::identity_custody::provision_handle_recovery_identity(
        &core,
        "awiki.test",
        "alice",
    )
    .unwrap();
    // Recovery retains its predecessor's custody material. That historical
    // identity can have multiple devices and must never become a fresh candidate.
    let predecessor_spec =
        crate::internal::identity_generation::vnext_handle_anp_identity_create_spec(
            "awiki.test",
            "alice",
            None,
            None,
        )
        .unwrap()
        .spec;
    let predecessor = crate::internal::identity_custody::open_controller_manager(&core)
        .unwrap()
        .create(crate::internal::identity_custody::native_create_spec(
            predecessor_spec,
        ))
        .unwrap()
        .public_identity()
        .unwrap();
    pending.local_previous_did = predecessor.reference.did.clone();
    pending.previous_custody = Some(crate::internal::identity_provider::ProviderIdentityRef {
        store_id: predecessor.reference.store_id.clone(),
        identity_id: predecessor.reference.identity_id.clone(),
        did: predecessor.reference.did.clone(),
    });
    let store = PendingHandleRecoveryStore::from_core(&core).unwrap();
    store.create_v4(&pending).unwrap();
    pending
        .freeze_exchange(
            crate::internal::identity_handle_recovery_pending::RecoveryAuthoritativeBindingV4 {
                account_user_id: "user-v4-1".to_owned(),
                full_handle: pending.full_handle.clone(),
                current_did: pending.local_previous_did.clone(),
                binding_generation: "7".to_owned(),
            },
            "grant-v4-1".to_owned(),
            "2099-08-07T00:05:00Z".to_owned(),
        )
        .unwrap();
    store.save_v4_cas(&pending, pending.revision - 1).unwrap();
    pending
        .mark_commit_attempted("2026-08-07T00:01:00Z".to_owned())
        .unwrap();
    store.save_v4_cas(&pending, pending.revision - 1).unwrap();
    let result = remote_result_for_pending(&pending, "user-v4-1");
    pending.record_remote_result(result.clone()).unwrap();
    store.save_v4_cas(&pending, pending.revision - 1).unwrap();
    pending.mark_local_transition_pending().unwrap();
    store.save_v4_cas(&pending, pending.revision - 1).unwrap();
    pending.mark_applied().unwrap();
    store.save_v4_cas(&pending, pending.revision - 1).unwrap();
    let sqlite_path = &core.inner().sdk_paths().local_state.sqlite_path;
    let marker =
        crate::internal::identity_transition_pending::IdentityTransitionMarker::initiator_v4(
            sqlite_path,
            &pending,
            &result,
        )
        .unwrap();
    crate::internal::identity_transition_pending::persist(sqlite_path, &marker).unwrap();
    crate::internal::identity_transition_pending::mark_applied(
        sqlite_path,
        operation_id,
        crate::internal::identity_transition_pending::TransitionPhase::Pending,
        &result.bootstrap_device.device_id,
        "1",
        "1",
        "{}",
    )
    .unwrap();
    let record =
        crate::internal::identity_handle_recovery_operation::RecoveryOperationRecord::pre_commit(
            operation_id.to_owned(),
            owner.to_owned(),
            pending.full_handle.clone(),
            crate::internal::identity_handle_recovery_pending::pending_v4_key_id(operation_id),
            "2026-08-07T00:00:00Z".to_owned(),
        )
        .unwrap();
    crate::internal::identity_handle_recovery_operation::insert(sqlite_path, &record).unwrap();
    crate::internal::local_state::open_writable(sqlite_path)
        .unwrap()
        .execute(
            "UPDATE handle_recovery_operations_v4 SET lifecycle_class='applied',commit_attempted=1 WHERE operation_id=?1",
            [operation_id],
        )
        .unwrap();

    let ticket = crate::internal::identity_local_deletion::prepare(
        sqlite_path,
        &crate::internal::identity_local_deletion::LocalIdentityDeletionSnapshot {
            owner_identity_id: owner.to_owned(),
            current_did: pending.identity.did.as_str().to_owned(),
            full_handle: Some(pending.full_handle.clone()),
            local_alias: pending.local_alias.clone(),
            identity_dir_name: None,
            next_default_alias: None,
            protocol_device_id: Some(pending.identity.protocol_device_id.as_str().to_owned()),
        },
        crate::internal::identity_local_deletion::LocalIdentityDeletionMode::CredentialOnly,
    )
    .unwrap();
    crate::internal::identity_local_deletion::complete(&core, &ticket.deletion_id, false).unwrap();
    assert!(store.load_v4(operation_id).unwrap().is_none());
    // Completed audit keeps its phase, but deletion still fences a stale
    // pre-OTP writer trying to recreate the old journal with the same ID.
    let stale_create = v4_awaiting_factor_pending(operation_id, owner);
    assert!(store.create_v4(&stale_create).is_err());
    let history =
        crate::internal::identity_handle_recovery_operation::load(sqlite_path, operation_id)
            .unwrap()
            .unwrap();
    assert_eq!(
        history.lifecycle_class,
        crate::internal::identity_handle_recovery_operation::RecoveryLifecycleClass::Applied
    );
    assert_eq!(
        history.key_state,
        crate::internal::identity_handle_recovery_operation::RecoveryKeyState::DestroyedByDeletion
    );
    assert!(matches!(
        crate::internal::identity_custody::open_controller_manager(&core)
            .unwrap()
            .get(&predecessor.reference),
        Err(anp_identity::IdentityError::IdentityNotFound)
    ));

    let error = request_otp(
        &core,
        HandleRecoveryOtpRequest {
            identity: None,
            full_handle: pending.full_handle.clone(),
            phone: "+8613800138000".to_owned(),
        },
    )
    .await
    .unwrap_err();
    assert!(
        matches!(error, crate::ImError::TransportUnavailable { .. }),
        "{error:?}"
    );
    let operations = crate::internal::identity_handle_recovery_operation::list_handle(
        sqlite_path,
        &pending.full_handle,
    )
    .unwrap();
    assert_eq!(operations.len(), 2);
    let fresh = operations
        .iter()
        .find(|item| item.operation_id != operation_id)
        .unwrap();
    assert_ne!(fresh.owner_identity_id, owner);
    assert!(!fresh.commit_attempted);
    assert_eq!(
        fresh.lifecycle_class,
        crate::internal::identity_handle_recovery_operation::RecoveryLifecycleClass::PreCommit
    );
    let (_, fresh_pending) = store.load_v4(&fresh.operation_id).unwrap().unwrap();
    assert_ne!(
        fresh_pending.identity.did.as_str(),
        predecessor.reference.did
    );
    assert_ne!(fresh_pending.identity.did, pending.identity.did);
}

#[tokio::test]
async fn local_deletion_cleans_unfinished_recovery_keys_and_allows_a_new_operation() {
    use crate::internal::identity_handle_recovery_operation as operations;
    use crate::internal::identity_local_deletion as deletion;
    for phase in [
        "unindexed",
        "pre_commit",
        "remote_unresolved",
        "remote_committed",
        "local_transition_pending",
    ] {
        let root = tempfile::tempdir().unwrap();
        let core = recovery_test_core(root.path(), "http://127.0.0.1:1", [94_u8; 32]);
        let operation_id = format!("recover-delete-{phase}");
        let mut pending = v4_awaiting_factor_pending(&operation_id, "owner-delete-recovery");
        pending.identity = crate::internal::identity_custody::provision_handle_recovery_identity(
            &core,
            "awiki.test",
            "alice",
        )
        .unwrap();
        let store = PendingHandleRecoveryStore::from_core(&core).unwrap();
        store.create_v4(&pending).unwrap();
        let path = &core.inner().sdk_paths().local_state.sqlite_path;
        if phase != "unindexed" {
            operations::insert(
                path,
                &operations::RecoveryOperationRecord::pre_commit(
                    operation_id.clone(),
                    pending.owner_identity_id.clone(),
                    pending.full_handle.clone(),
                    crate::internal::identity_handle_recovery_pending::pending_v4_key_id(
                        &operation_id,
                    ),
                    "2026-08-29T00:00:00Z".to_owned(),
                )
                .unwrap(),
            )
            .unwrap();
        }
        if !matches!(phase, "pre_commit" | "unindexed") {
            pending.freeze_exchange(crate::internal::identity_handle_recovery_pending::RecoveryAuthoritativeBindingV4 {
                account_user_id: "user-v4-1".to_owned(), full_handle: pending.full_handle.clone(), current_did: pending.local_previous_did.clone(), binding_generation: "7".to_owned(),
            }, "grant-v4-1".to_owned(), "2099-08-07T00:05:00Z".to_owned()).unwrap();
            store.save_v4_cas(&pending, pending.revision - 1).unwrap();
            pending
                .mark_commit_attempted("2026-08-29T00:01:00Z".to_owned())
                .unwrap();
            store.save_v4_cas(&pending, pending.revision - 1).unwrap();
        }
        if matches!(phase, "remote_committed" | "local_transition_pending") {
            let result = remote_result_for_pending(&pending, "user-v4-1");
            pending.record_remote_result(result.clone()).unwrap();
            store.save_v4_cas(&pending, pending.revision - 1).unwrap();
            let marker = crate::internal::identity_transition_pending::IdentityTransitionMarker::initiator_v4(path, &pending, &result).unwrap();
            crate::internal::identity_transition_pending::persist(path, &marker).unwrap();
            if phase == "local_transition_pending" {
                pending.mark_local_transition_pending().unwrap();
                store.save_v4_cas(&pending, pending.revision - 1).unwrap();
            }
        }
        crate::internal::local_state::open_writable(path).unwrap().execute(
            "UPDATE handle_recovery_operations_v4 SET lifecycle_class=?2,commit_attempted=?3 WHERE operation_id=?1",
            rusqlite::params![operation_id, phase, i64::from(pending.commit_attempted)],
        ).unwrap();
        let snapshot = deletion::LocalIdentityDeletionSnapshot {
            owner_identity_id: pending.owner_identity_id.clone(),
            current_did: pending.identity.did.as_str().to_owned(),
            full_handle: Some(pending.full_handle.clone()),
            local_alias: pending.local_alias.clone(),
            identity_dir_name: None,
            next_default_alias: None,
            protocol_device_id: Some(pending.identity.protocol_device_id.as_str().to_owned()),
        };
        if phase == "unindexed" {
            assert!(!deletion::has_pending_recovery(path, &snapshot).unwrap());
            assert_eq!(
                deletion::unindexed_recoveries(&core, &snapshot)
                    .unwrap()
                    .len(),
                1
            );
            deletion::reconcile_recovery_deletion_inputs(&core, &snapshot).unwrap();
        }
        assert!(deletion::has_pending_recovery(path, &snapshot).unwrap());
        let ticket = deletion::prepare(
            path,
            &snapshot,
            deletion::LocalIdentityDeletionMode::CredentialOnly,
        )
        .unwrap();
        // Crash after accepting deletion: persisted keys exist, but no old work
        // can write, activate or resume them, even before cleanup is finished.
        assert!(store.load_v4(&operation_id).unwrap().is_some());
        assert!(status(&core, &operation_id).is_err());
        assert!(resume(
            &core,
            crate::identity::HandleRecoveryResumeRequest {
                operation_id: operation_id.clone()
            }
        )
        .await
        .is_err());
        let mut stale = pending.clone();
        stale.revision += 1;
        assert!(store.save_v4_cas(&stale, pending.revision).is_err());
        deletion::recover_before_retirement(&core).unwrap();
        deletion::complete(&core, &ticket.deletion_id, false).unwrap();
        deletion::complete(&core, &ticket.deletion_id, false).unwrap();
        assert!(store.load_v4(&operation_id).unwrap().is_none());
        let retired = operations::load(path, &operation_id).unwrap().unwrap();
        assert_eq!(
            retired.lifecycle_class,
            operations::RecoveryLifecycleClass::LocallyDeleted
        );
        assert_eq!(
            retired.key_state,
            operations::RecoveryKeyState::DestroyedByDeletion
        );
        assert!(operations::list_pending(path).unwrap().is_empty());
        let reference = anp_identity::IdentityRef {
            store_id: pending.identity.store_id.clone(),
            identity_id: pending.identity.identity_id.clone(),
            did: pending.identity.did.as_str().to_owned(),
        };
        assert!(matches!(
            crate::internal::identity_custody::open_controller_manager(&core)
                .unwrap()
                .get(&reference),
            Err(anp_identity::IdentityError::IdentityNotFound)
        ));
        // Reopening runs the real Core startup recovery, then a fresh request
        // reaches transport without resurrecting the deleted operation/custody.
        drop(store);
        drop(core);
        let core = recovery_test_core(root.path(), "http://127.0.0.1:1", [94_u8; 32]);
        let error = request_otp(
            &core,
            HandleRecoveryOtpRequest {
                identity: None,
                full_handle: pending.full_handle.clone(),
                phone: "+8613800138000".to_owned(),
            },
        )
        .await
        .unwrap_err();
        assert!(
            matches!(error, crate::ImError::TransportUnavailable { .. }),
            "{phase}: {error:?}"
        );
        let operations =
            operations::list_pending(&core.inner().sdk_paths().local_state.sqlite_path).unwrap();
        assert_eq!(operations.len(), 1);
        assert_ne!(operations[0].operation_id, operation_id);
        assert_ne!(operations[0].owner_identity_id, pending.owner_identity_id);
    }
}

#[tokio::test]
async fn external_custody_open_resumes_deletion_and_preserves_other_identity() {
    use crate::internal::identity_handle_recovery_operation as operations;
    use crate::internal::identity_local_deletion as deletion;
    use crate::internal::identity_provider::{
        DirectAnpIdentityCustody, IdentityCustody, ProviderIdentityRef,
    };

    let root = tempfile::tempdir().unwrap();
    let key = [95_u8; 32];
    let core = recovery_test_core(root.path(), "http://127.0.0.1:1", key);
    let mut pending =
        v4_awaiting_factor_pending("recover-delete-external", "owner-delete-external");
    pending.identity = crate::internal::identity_custody::provision_handle_recovery_identity(
        &core,
        "awiki.test",
        "alice",
    )
    .unwrap();
    let predecessor_spec =
        crate::internal::identity_generation::vnext_handle_anp_identity_create_spec(
            "awiki.test",
            "alice",
            None,
            None,
        )
        .unwrap()
        .spec;
    let predecessor = crate::internal::identity_custody::open_controller_manager(&core)
        .unwrap()
        .create(crate::internal::identity_custody::native_create_spec(
            predecessor_spec,
        ))
        .unwrap()
        .public_identity()
        .unwrap()
        .reference;
    pending.local_previous_did = predecessor.did.clone();
    let predecessor_ref = ProviderIdentityRef {
        store_id: predecessor.store_id,
        identity_id: predecessor.identity_id,
        did: predecessor.did,
    };
    pending.previous_custody = Some(predecessor_ref.clone());
    let unrelated = crate::internal::identity_custody::provision_handle_recovery_identity(
        &core,
        "awiki.test",
        "bob",
    )
    .unwrap();
    let unrelated_ref = ProviderIdentityRef {
        store_id: unrelated.store_id,
        identity_id: unrelated.identity_id,
        did: unrelated.did.as_str().to_owned(),
    };
    let store = PendingHandleRecoveryStore::from_core(&core).unwrap();
    store.create_v4(&pending).unwrap();
    let path = &core.inner().sdk_paths().local_state.sqlite_path;
    operations::insert(
        path,
        &operations::RecoveryOperationRecord::pre_commit(
            pending.operation_id.clone(),
            pending.owner_identity_id.clone(),
            pending.full_handle.clone(),
            crate::internal::identity_handle_recovery_pending::pending_v4_key_id(
                &pending.operation_id,
            ),
            "2026-08-29T00:00:00Z".to_owned(),
        )
        .unwrap(),
    )
    .unwrap();
    let ticket = deletion::prepare(
        path,
        &deletion::LocalIdentityDeletionSnapshot {
            owner_identity_id: pending.owner_identity_id.clone(),
            current_did: pending.identity.did.as_str().to_owned(),
            full_handle: Some(pending.full_handle.clone()),
            local_alias: pending.local_alias.clone(),
            identity_dir_name: None,
            next_default_alias: None,
            protocol_device_id: Some(pending.identity.protocol_device_id.as_str().to_owned()),
        },
        deletion::LocalIdentityDeletionMode::CredentialOnly,
    )
    .unwrap();

    let provider = std::sync::Arc::new(DirectAnpIdentityCustody::new(
        crate::internal::identity_custody::open_controller_manager(&core).unwrap(),
    ));
    // Simulate a crash after the durable decision but before external custody
    // cleanup. Reopen must await the host SPI before exposing the Core runtime.
    let config = core.inner().sdk_config().clone();
    let paths = core.inner().sdk_paths().clone();
    drop(store);
    drop(core);
    let reopened = crate::ImCore::open_with_options(
        config,
        paths.clone(),
        crate::ImCoreOpenOptions::default()
            .with_multi_device_handle_recovery_enabled(true)
            .with_multi_device_audience("awiki-user-service")
            .with_identity_custody_provider(provider.clone())
            .with_identity_secret_vault(
                crate::IdentitySecretStoragePolicy::VaultRequired,
                crate::ImCoreSecretVaultOptions::new(
                    crate::vault::DeviceVaultRootKey::from_bytes(key),
                    root.path().join("vault"),
                    "recovery-reopen-workspace",
                    "recovery-reopen-device",
                ),
            ),
    )
    .await
    .unwrap();
    assert!(deletion::list_incomplete(&paths.local_state.sqlite_path)
        .unwrap()
        .is_empty());
    assert!(PendingHandleRecoveryStore::from_core(&reopened)
        .unwrap()
        .load_v4(&pending.operation_id)
        .unwrap()
        .is_none());
    for reference in [
        predecessor_ref,
        ProviderIdentityRef {
            store_id: pending.identity.store_id,
            identity_id: pending.identity.identity_id,
            did: pending.identity.did.as_str().to_owned(),
        },
    ] {
        let error = match provider.open_identity(&reference).await {
            Ok(_) => panic!("deleted recovery key still accessible"),
            Err(error) => error,
        };
        assert_eq!(
            error.code,
            crate::internal::identity_provider::IdentityProviderErrorCode::IdentityNotFound
        );
    }
    provider.open_identity(&unrelated_ref).await.unwrap();
    let record = operations::load(&paths.local_state.sqlite_path, &pending.operation_id)
        .unwrap()
        .unwrap();
    assert_eq!(
        record.lifecycle_class,
        operations::RecoveryLifecycleClass::LocallyDeleted
    );
    assert_eq!(
        record.key_state,
        operations::RecoveryKeyState::DestroyedByDeletion
    );
    deletion::complete_async(&reopened, &ticket.deletion_id, false)
        .await
        .unwrap();
}
