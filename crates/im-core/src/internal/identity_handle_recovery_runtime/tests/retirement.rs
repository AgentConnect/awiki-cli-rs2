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

    crate::internal::identity_retirement::retire(
        &core,
        crate::internal::identity_retirement::IdentityRetirementInput {
            identity_id: owner.to_owned(),
            did: pending.identity.did.as_str().to_owned(),
            local_alias: pending.local_alias.clone(),
            identity_dir_name: None,
            next_default_alias: None,
            protocol_device_id: Some(pending.identity.protocol_device_id.as_str().to_owned()),
        },
    )
    .unwrap();
    assert!(store.load_v4(operation_id).unwrap().is_none());
    assert!(list_operations(
        &core,
        crate::identity::IdentitySelector::Handle(
            crate::ids::Handle::parse(&pending.full_handle, "").unwrap()
        )
    )
    .await
    .unwrap()
    .is_empty());

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
