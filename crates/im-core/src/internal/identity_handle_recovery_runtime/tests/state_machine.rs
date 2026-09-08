use super::*;

#[tokio::test]
async fn resume_rejects_first_commit_without_mutating_attempt_authority() {
    let root = tempfile::tempdir().unwrap();
    let core = recovery_test_core(root.path(), "http://127.0.0.1:1", [94; 32]);
    let operation_id = "recover-v4-first-confirmation";
    let owner = "owner-first-confirmation";
    let mut pending = create_v4_operation_with_transition_identities(&core, operation_id, owner);
    let store = PendingHandleRecoveryStore::from_core(&core).unwrap();
    let revision = pending.revision;
    pending
        .freeze_exchange(
            crate::internal::identity_handle_recovery_pending::RecoveryAuthoritativeBindingV4 {
                account_user_id: "account-first-confirmation".to_owned(),
                full_handle: pending.full_handle.clone(),
                current_did: pending.local_previous_did.clone(),
                binding_generation: "1".to_owned(),
            },
            "test-grant".to_owned(),
            "2099-08-07T00:05:00Z".to_owned(),
        )
        .unwrap();
    store.save_v4_cas(&pending, revision).unwrap();
    let path = &core.inner().sdk_paths().local_state.sqlite_path;
    let record = crate::internal::identity_handle_recovery_operation::load(path, operation_id)
        .unwrap()
        .unwrap();
    reconcile_frozen_intent_index(path, &record, &pending, "2026-09-08T00:00:00Z").unwrap();

    let result = core
        .handle_recovery()
        .resume_handle_recovery(HandleRecoveryResumeRequest {
            operation_id: operation_id.to_owned(),
        })
        .await;
    let code = match result.err() {
        Some(crate::ImError::Service { code, .. }) => code,
        _ => None,
    };
    assert_eq!(code.as_deref(), Some("activation_required"));
    let record = crate::internal::identity_handle_recovery_operation::load(path, operation_id)
        .unwrap()
        .unwrap();
    let (_, persisted) = store.load_v4(operation_id).unwrap().unwrap();
    assert!(!record.commit_attempted);
    assert!(!persisted.commit_attempted);
    assert_eq!(persisted.revision, pending.revision);
}

#[tokio::test]
async fn context_inspection_does_not_provision_identity_or_operation() {
    use crate::identity::{HandleRecoveryAction, HandleRecoveryContextRequest};
    let root = tempfile::tempdir().unwrap();
    let core = recovery_test_core(root.path(), "http://127.0.0.1:1", [95; 32]);
    let context = core
        .handle_recovery()
        .inspect_handle_recovery_context(HandleRecoveryContextRequest {
            full_handle: "alice.awiki.test".to_owned(),
            identity: None,
        })
        .await
        .unwrap();
    assert_eq!(
        context.allowed_actions,
        vec![HandleRecoveryAction::StartNew]
    );
    assert!(context.operation.is_none());
    assert!(context.progress.is_none());
    assert!(core.identities().list().unwrap().is_empty());
    assert!(
        crate::internal::identity_handle_recovery_operation::list_handle(
            &core.inner().sdk_paths().local_state.sqlite_path,
            "alice.awiki.test",
        )
        .unwrap()
        .is_empty()
    );
    assert!(!core
        .inner()
        .sdk_paths()
        .identities
        .identity_root_dir
        .join(".anp-identity")
        .exists());
}

#[tokio::test]
async fn context_inspection_reopens_exact_operation_without_sending_or_advancing() {
    use crate::identity::{HandleRecoveryAction as Action, HandleRecoveryContextRequest};
    let root = tempfile::tempdir().unwrap();
    let core = recovery_test_core(root.path(), "http://127.0.0.1:1", [96; 32]);
    let operation_id = "recover-v4-context-existing";
    let (pending, _) = create_v4_awaiting_factor_operation(&core, operation_id, "owner-context");
    let context = core
        .handle_recovery()
        .inspect_handle_recovery_context(HandleRecoveryContextRequest {
            full_handle: pending.full_handle.clone(),
            identity: None,
        })
        .await
        .unwrap();
    assert_eq!(context.operation.unwrap().operation_id, operation_id);
    let progress = context.progress.unwrap();
    assert_eq!(progress.phase, HandleRecoveryPhase::AwaitingFactor);
    assert_eq!(
        context.allowed_actions,
        vec![
            Action::RequestOtp,
            Action::Prepare,
            Action::DiscardPreAttempt
        ]
    );
    assert_eq!(
        PendingHandleRecoveryStore::from_core(&core)
            .unwrap()
            .load_v4(operation_id)
            .unwrap()
            .unwrap()
            .1,
        pending
    );
}

#[tokio::test]
async fn context_inspection_rejects_ambiguous_active_owners() {
    let root = tempfile::tempdir().unwrap();
    let core = recovery_test_core(root.path(), "http://127.0.0.1:1", [97; 32]);
    create_v4_awaiting_factor_operation(&core, "recover-v4-context-first", "owner-first");
    create_v4_awaiting_factor_operation(&core, "recover-v4-context-second", "owner-second");
    let result = core
        .handle_recovery()
        .inspect_handle_recovery_context(crate::identity::HandleRecoveryContextRequest {
            full_handle: "alice.awiki.test".to_owned(),
            identity: None,
        })
        .await;
    assert!(
        matches!(result, Err(crate::ImError::Service { code: Some(code), .. }) if code == "unknown_epoch")
    );
}

#[tokio::test]
async fn registration_cannot_claim_a_recovery_owned_identity() {
    let root = tempfile::tempdir().unwrap();
    let core = recovery_test_core(root.path(), "http://127.0.0.1:1", [98; 32]);
    create_v4_awaiting_factor_operation(
        &core,
        "recover-v4-registration-reserved",
        "owner-reserved",
    );
    let result = crate::internal::identity_custody::provision_registration_identity_async(
        &core,
        "awiki.test",
        "alice",
    )
    .await;
    assert!(
        matches!(result, Err(crate::ImError::Service { code: Some(code), .. }) if code == "recovery_in_progress")
    );
}

#[tokio::test]
async fn recovery_does_not_reuse_a_pending_registration_identity() {
    use crate::internal::identity_registration_pending::{
        PendingRegistration, PendingRegistrationStore,
    };
    let root = tempfile::tempdir().unwrap();
    let core = recovery_test_core(root.path(), "http://127.0.0.1:1", [99; 32]);
    let registration = crate::internal::identity_custody::provision_registration_identity_async(
        &core,
        "awiki.test",
        "alice",
    )
    .await
    .unwrap();
    let reserved_did = registration.did.clone();
    let pending = PendingRegistration::new(
        "alice".to_owned(),
        "awiki.test".to_owned(),
        "alice".to_owned(),
        "Alice".to_owned(),
        true,
        "phone".to_owned(),
        Some("test-factor-target".to_owned()),
        None,
        registration,
    )
    .unwrap();
    PendingRegistrationStore::from_core(&core)
        .unwrap()
        .save(&pending)
        .unwrap();
    let recovery = crate::internal::identity_custody::provision_handle_recovery_identity_async(
        &core,
        "awiki.test",
        "alice",
    )
    .await
    .unwrap();
    assert_ne!(recovery.did, reserved_did);
    assert!(PendingRegistrationStore::from_core(&core)
        .unwrap()
        .load("alice", "awiki.test")
        .unwrap()
        .is_some());
}

#[tokio::test]
async fn expired_precommit_grant_exposes_factor_actions_without_changing_intent() {
    use crate::identity::{HandleRecoveryAction as Action, HandleRecoveryContextRequest};
    let root = tempfile::tempdir().unwrap();
    let core = recovery_test_core(root.path(), "http://127.0.0.1:1", [100; 32]);
    let operation_id = "recover-v4-context-expired";
    let (mut pending, _) =
        create_v4_awaiting_factor_operation(&core, operation_id, "owner-expired");
    let store = PendingHandleRecoveryStore::from_core(&core).unwrap();
    let revision = pending.revision;
    let binding =
        crate::internal::identity_handle_recovery_pending::RecoveryAuthoritativeBindingV4 {
            account_user_id: "account-expired".to_owned(),
            full_handle: pending.full_handle.clone(),
            current_did: pending.local_previous_did.clone(),
            binding_generation: "1".to_owned(),
        };
    pending
        .freeze_exchange(
            binding.clone(),
            "test-expired-grant".to_owned(),
            "2020-01-01T00:00:00Z".to_owned(),
        )
        .unwrap();
    store.save_v4_cas(&pending, revision).unwrap();
    let path = &core.inner().sdk_paths().local_state.sqlite_path;
    let record = crate::internal::identity_handle_recovery_operation::load(path, operation_id)
        .unwrap()
        .unwrap();
    reconcile_frozen_intent_index(path, &record, &pending, "2026-09-08T00:00:00Z").unwrap();
    let context = core
        .handle_recovery()
        .inspect_handle_recovery_context(HandleRecoveryContextRequest {
            full_handle: pending.full_handle.clone(),
            identity: None,
        })
        .await
        .unwrap();
    assert_eq!(
        context.blocked_reason,
        Some(HandleRecoveryErrorCode::FactorRetryRequired)
    );
    assert!(context.allowed_actions.contains(&Action::RequestOtp));
    assert!(context.allowed_actions.contains(&Action::Prepare));
    assert!(!context.allowed_actions.contains(&Action::Activate));
    assert!(!context.allowed_actions.contains(&Action::Resume));
    let intent = pending.intent_hash.clone();
    let identity = pending.identity.clone();
    let revision = pending.revision;
    pending
        .refresh_grant(
            &binding,
            "test-new-grant".to_owned(),
            "2099-01-01T00:00:00Z".to_owned(),
        )
        .unwrap();
    store.save_v4_cas(&pending, revision).unwrap();
    let renewed = core
        .handle_recovery()
        .inspect_handle_recovery_context(HandleRecoveryContextRequest {
            full_handle: pending.full_handle.clone(),
            identity: None,
        })
        .await
        .unwrap();
    assert!(renewed.allowed_actions.contains(&Action::Activate));
    assert!(!renewed.allowed_actions.contains(&Action::Prepare));
    assert_eq!(renewed.blocked_reason, None);
    assert_eq!(pending.intent_hash, intent);
    assert_eq!(pending.identity, identity);
    assert!(!pending.commit_attempted);
}

#[tokio::test]
async fn expired_grant_exchange_preserves_intent_or_requires_explicit_new_operation() {
    use crate::identity::HandleRecoveryAction as Action;
    for changed in [false, true] {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let root = tempfile::tempdir().unwrap();
        let core = recovery_test_core(root.path(), &endpoint, [111; 32]);
        let operation_id = "recover-v4-expired-exchange";
        let mut pending = create_v4_operation_with_transition_identities(
            &core,
            operation_id,
            "owner-expired-exchange",
        );
        let store = PendingHandleRecoveryStore::from_core(&core).unwrap();
        // This fixture has custody but no projected old owner. Freeze the
        // valid fresh-local-state branch before the immutable exchange.
        pending
            .freeze_fresh_local_owner(&pending.local_previous_did.clone())
            .unwrap();
        let revision = pending.revision;
        pending
            .freeze_exchange(
                crate::internal::identity_handle_recovery_pending::RecoveryAuthoritativeBindingV4 {
                    account_user_id: "account-expired".to_owned(),
                    full_handle: pending.full_handle.clone(),
                    current_did: pending.local_previous_did.clone(),
                    binding_generation: "7".to_owned(),
                },
                "expired-grant".to_owned(),
                "2020-08-07T00:05:00Z".to_owned(),
            )
            .unwrap();
        store.save_v4_cas(&pending, revision).unwrap();
        let path = &core.inner().sdk_paths().local_state.sqlite_path;
        let record = crate::internal::identity_handle_recovery_operation::load(path, operation_id)
            .unwrap()
            .unwrap();
        reconcile_frozen_intent_index(path, &record, &pending, "2026-09-08T00:00:00Z").unwrap();
        let response = json!({
            "contract_version": crate::internal::identity_handle_recovery_pending::V4_CONTRACT_VERSION,
            "recovery_grant": "renewed-grant", "purpose": "awiki.identity.handle-recovery.v1", "expires_at": "2099-08-07T00:05:00Z",
            "current_binding": {"account_user_id": "account-expired", "full_handle": pending.full_handle,
                "current_did": if changed { pending.identity.did.as_str() } else { &pending.local_previous_did },
                "binding_generation": if changed { "8" } else { "7" }},
        });
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let request = read_http_request(&mut stream);
            assert!(request.starts_with("POST /user-service/v1/auth/handle-recovery/v4/exchange "));
            write_json_response(&mut stream, &response);
        });
        let prepared = core
            .handle_recovery()
            .prepare_handle_recovery(HandleRecoveryPrepareRequest {
                operation_id: operation_id.to_owned(),
                phone: "+8613800138000".to_owned(),
                code: "123456".to_owned(),
            })
            .await;
        server.join().unwrap();
        let current = status(&core, operation_id).unwrap();
        if changed {
            assert_eq!(
                prepared.err().as_ref().and_then(service_code),
                Some("state_changed_requires_new_operation")
            );
            assert_eq!(current.allowed_actions, vec![Action::DiscardPreAttempt]);
        } else {
            assert!(
                prepared.is_ok(),
                "same-binding renewal failed with stable code {:?}",
                prepared.err().as_ref().and_then(service_code)
            );
            assert_eq!(
                current.allowed_actions,
                vec![Action::Activate, Action::DiscardPreAttempt]
            );
            assert!(current.failure_code.is_none());
        }
        let (_, after) = store.load_v4(operation_id).unwrap().unwrap();
        assert_eq!(after.intent, pending.intent);
        assert_eq!(after.intent_hash, pending.intent_hash);
        assert_eq!(after.identity.did, pending.identity.did);
        assert_eq!(after.identity.identity_id, pending.identity.identity_id);
        assert_eq!(
            after.identity.device_signing_key_id,
            pending.identity.device_signing_key_id
        );
        assert_eq!(
            after.identity.device_e2ee_key_id,
            pending.identity.device_e2ee_key_id
        );
        assert!(!after.commit_attempted);
    }
}

#[test]
fn operation_action_matrix_is_closed_for_lifecycle_factor_and_key_states() {
    use crate::identity::HandleRecoveryAction as A;
    use crate::internal::identity_handle_recovery_operation::{
        RecoveryKeyState as K, RecoveryLifecycleClass as L,
    };
    use crate::internal::identity_handle_recovery_pending::PendingRecoveryPhaseV4 as P;
    let root = tempfile::tempdir().unwrap();
    let core = recovery_test_core(root.path(), "http://127.0.0.1:1", [112; 32]);
    let (initial, _) =
        create_v4_awaiting_factor_operation(&core, "recover-v4-matrix", "owner-matrix");
    let record = crate::internal::identity_handle_recovery_operation::load(
        &core.inner().sdk_paths().local_state.sqlite_path,
        &initial.operation_id,
    )
    .unwrap()
    .unwrap();
    let now = time::OffsetDateTime::now_utc();
    for (lifecycle, phase, attempted, expiry, failure, expected) in [
        (
            L::PreCommit,
            P::AwaitingFactor,
            false,
            None,
            None,
            vec![A::RequestOtp, A::Prepare, A::DiscardPreAttempt],
        ),
        (
            L::PreCommit,
            P::ReadyToCommit,
            false,
            Some("2099-08-07T00:05:00Z"),
            None,
            vec![A::Activate, A::DiscardPreAttempt],
        ),
        (
            L::PreCommit,
            P::ReadyToCommit,
            false,
            Some("2020-08-07T00:05:00Z"),
            None,
            vec![A::RequestOtp, A::Prepare, A::DiscardPreAttempt],
        ),
        (
            L::PreCommit,
            P::ReadyToCommit,
            false,
            Some("2099-08-07T00:05:00Z"),
            Some("state_changed_requires_new_operation"),
            vec![A::DiscardPreAttempt],
        ),
        (
            L::PreCommit,
            P::ReadyToCommit,
            false,
            Some("2099-08-07T00:05:00Z"),
            Some("local_migration_unsupported"),
            vec![A::DiscardPreAttempt],
        ),
        (
            L::RemoteUnresolved,
            P::RemoteOutcomeUnknown,
            true,
            None,
            None,
            vec![A::Resume, A::RequestOtp, A::Prepare],
        ),
        (
            L::RemoteCommitted,
            P::RemoteCommitted,
            true,
            None,
            None,
            vec![A::Resume],
        ),
        (
            L::LocalTransitionPending,
            P::LocalTransitionPending,
            true,
            None,
            Some("local_transition_pending"),
            vec![A::Resume],
        ),
        (L::Applied, P::Applied, true, None, None, vec![]),
        (
            L::DiscardedPreAttempt,
            P::AwaitingFactor,
            false,
            None,
            None,
            vec![],
        ),
        (
            L::SupersededByStateChange,
            P::LocalTransitionPending,
            true,
            None,
            Some("local_transition_superseded"),
            vec![],
        ),
        (
            L::FailedTerminal,
            P::RemoteOutcomeUnknown,
            true,
            None,
            None,
            vec![],
        ),
    ] {
        for key_state in [
            K::Available,
            K::TemporarilyLocked,
            K::PermanentlyUnavailable,
            K::DestroyedPreAttempt,
        ] {
            let mut record = record.clone();
            record.lifecycle_class = lifecycle;
            record.commit_attempted = attempted;
            record.key_state = key_state;
            let mut pending = initial.clone();
            pending.phase = phase;
            pending.commit_attempted = attempted;
            pending.grant_expires_at = expiry.map(str::to_owned);
            pending.last_error_code = failure.map(str::to_owned);
            let actual = crate::internal::identity_handle_recovery_context::operation_actions(
                &record, &pending, now,
            )
            .unwrap();
            let expected = match key_state {
                K::Available => expected.clone(),
                K::PermanentlyUnavailable
                    if crate::internal::identity_handle_recovery_context::is_actionable(
                        lifecycle,
                    ) =>
                {
                    vec![A::QuarantineKeyUnavailable]
                }
                _ => vec![],
            };
            assert_eq!(actual, expected, "{lifecycle:?}/{phase:?}/{key_state:?}");
        }
    }
    let mut wrong = record.clone();
    wrong.owner_identity_id = "other-owner".to_owned();
    assert!(
        crate::internal::identity_handle_recovery_context::operation_actions(&wrong, &initial, now)
            .is_err()
    );
}
