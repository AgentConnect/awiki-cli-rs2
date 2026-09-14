use super::*;
use crate::internal::identity_handle_recovery_registration_cleanup as cleanup;
use crate::internal::identity_registration_pending::{
    PendingRegistration, PendingRegistrationStore,
};

fn registration(core: &crate::ImCore) -> PendingRegistration {
    let identity = crate::internal::identity_custody::provision_registration_identity(
        core,
        "awiki.test",
        "alice",
    )
    .unwrap();
    let registration = PendingRegistration::new(
        "alice".into(),
        "awiki.test".into(),
        "alice".into(),
        "Alice".into(),
        true,
        "already_verified".into(),
        None,
        None,
        identity,
    )
    .unwrap();
    PendingRegistrationStore::from_core(core)
        .unwrap()
        .save(&registration)
        .unwrap();
    registration
}

fn applied(core: &crate::ImCore) -> PendingHandleRecoveryV4 {
    let (mut recovery, _) =
        create_v4_awaiting_factor_operation(core, "recovery-cleanup-1", "owner-cleanup-1");
    recovery.registration_candidate_cleanup = cleanup::capture(core, &recovery).unwrap();
    assert!(recovery.registration_candidate_cleanup.is_some());
    let revision = recovery.revision;
    recovery.revision += 1;
    let store = PendingHandleRecoveryStore::from_core(core).unwrap();
    store.save_v4_cas(&recovery, revision).unwrap();
    let result = freeze_and_commit_v4_pending(&mut recovery);
    let sqlite = &core.inner().sdk_paths().local_state.sqlite_path;
    crate::internal::identity_handle_recovery_operation::record_frozen_intent(
        sqlite,
        &recovery.operation_id,
        "user-v4-1",
        recovery.intent_hash.as_deref().unwrap(),
        "2026-08-07T00:00:30Z",
    )
    .unwrap();
    crate::internal::identity_handle_recovery_operation::mark_commit_attempted(
        sqlite,
        &recovery.operation_id,
        "2026-08-07T00:01:00Z",
    )
    .unwrap();
    let operation =
        crate::internal::identity_handle_recovery_operation::load(sqlite, &recovery.operation_id)
            .unwrap()
            .unwrap();
    reconcile_v4_lifecycle_index(sqlite, &operation, &recovery, "2026-08-07T00:01:02Z").unwrap();
    let marker =
        crate::internal::identity_transition_pending::IdentityTransitionMarker::initiator_v4(
            sqlite, &recovery, &result,
        )
        .unwrap();
    crate::internal::identity_transition_pending::persist(sqlite, &marker).unwrap();
    use crate::internal::identity_handle_recovery_operation::RecoveryLifecycleClass as Lifecycle;
    crate::internal::identity_handle_recovery_operation::update_lifecycle(
        sqlite,
        &recovery.operation_id,
        Lifecycle::RemoteCommitted,
        Lifecycle::LocalTransitionPending,
        Some(&marker.state_root_fingerprint),
        None,
        "2026-08-07T00:01:03Z",
    )
    .unwrap();
    crate::internal::identity_transition_pending::mark_applied(
        sqlite,
        &recovery.operation_id,
        crate::internal::identity_transition_pending::TransitionPhase::Pending,
        &result.bootstrap_device.device_id,
        "1",
        "1",
        "{}",
    )
    .unwrap();
    recovery.mark_local_transition_pending().unwrap();
    recovery.mark_applied().unwrap();
    let operation =
        crate::internal::identity_handle_recovery_operation::load(sqlite, &recovery.operation_id)
            .unwrap()
            .unwrap();
    reconcile_v4_lifecycle_index(sqlite, &operation, &recovery, "2026-08-07T00:01:04Z").unwrap();
    // Persist the simulated successful remote/local transition with the real Vault CAS.
    let (_, durable) = store.load_v4(&recovery.operation_id).unwrap().unwrap();
    recovery.revision = durable.revision + 1;
    store.save_v4_cas(&recovery, durable.revision).unwrap();
    recovery
}

fn exists(core: &crate::ImCore, registration: &PendingRegistration) -> bool {
    let manager = crate::internal::identity_custody::open_controller_manager(core).unwrap();
    manager
        .get(&anp_identity::IdentityRef {
            store_id: registration.identity.controller_store_id.clone(),
            identity_id: registration.identity.controller_identity_id.clone(),
            did: registration.identity.did.as_str().into(),
        })
        .is_ok()
}

#[tokio::test]
async fn recovery_registration_cleanup_removes_only_captured_candidate_after_applied() {
    let root = tempfile::tempdir().unwrap();
    let core = recovery_test_core(root.path(), "https://example.invalid", [91; 32]);
    let candidate = registration(&core);
    let mut recovery = applied(&core);
    assert_ne!(candidate.identity.did, recovery.identity.did);
    assert!(exists(&core, &candidate));
    let store = PendingHandleRecoveryStore::from_core(&core).unwrap();
    let progress = advance_v4(&core, &recovery.operation_id, false)
        .await
        .unwrap();
    assert_eq!(progress.phase, HandleRecoveryPhase::Applied);
    assert!(
        !exists(&core, &candidate),
        "successful recovery must discard its unpublished registration candidate"
    );
    assert!(PendingRegistrationStore::from_core(&core)
        .unwrap()
        .load("alice", "awiki.test")
        .unwrap()
        .is_none());
    assert!(store
        .load_v4(&recovery.operation_id)
        .unwrap()
        .unwrap()
        .1
        .registration_candidate_cleanup
        .is_none());
    assert_eq!(recovery.phase, PendingRecoveryPhaseV4::Applied);
    let manager = crate::internal::identity_custody::open_controller_manager(&core).unwrap();
    assert!(manager
        .get(&anp_identity::IdentityRef {
            store_id: recovery.identity.store_id.clone(),
            identity_id: recovery.identity.identity_id.clone(),
            did: recovery.identity.did.as_str().into(),
        })
        .is_ok());
    recovery = store.load_v4(&recovery.operation_id).unwrap().unwrap().1;
    cleanup::finish(&core, &store, &mut recovery).await.unwrap();
}

#[tokio::test]
async fn recovery_registration_cleanup_preserves_ambiguous_candidate_and_retries_after_reopen() {
    let root = tempfile::tempdir().unwrap();
    let core = recovery_test_core(root.path(), "https://example.invalid", [92; 32]);
    let mut candidate = registration(&core);
    let mut recovery = applied(&core);
    let registration_store = PendingRegistrationStore::from_core(&core).unwrap();
    candidate.remote_attempted = true;
    registration_store.save(&candidate).unwrap();
    let store = PendingHandleRecoveryStore::from_core(&core).unwrap();
    let progress = advance_v4(&core, &recovery.operation_id, false)
        .await
        .unwrap();
    assert_eq!(progress.phase, HandleRecoveryPhase::Applied);
    recovery = store.load_v4(&recovery.operation_id).unwrap().unwrap().1;
    assert!(exists(&core, &candidate));
    assert!(registration_store
        .load("alice", "awiki.test")
        .unwrap()
        .is_some());
    assert_eq!(recovery.phase, PendingRecoveryPhaseV4::Applied);
    assert!(
        store
            .load_v4(&recovery.operation_id)
            .unwrap()
            .unwrap()
            .1
            .registration_candidate_cleanup
            .unwrap()
            .retry_required
    );
    // Simulate a confirmed rejected-before-acceptance reconciliation, then restart Core.
    candidate.remote_attempted = false;
    registration_store.save(&candidate).unwrap();
    drop(store);
    drop(registration_store);
    drop(core);
    let core = recovery_test_core(root.path(), "https://example.invalid", [92; 32]);
    let store = PendingHandleRecoveryStore::from_core(&core).unwrap();
    let (_, mut recovery) = store.load_v4(&recovery.operation_id).unwrap().unwrap();
    cleanup::finish(&core, &store, &mut recovery).await.unwrap();
    assert!(!exists(&core, &candidate));
    assert!(recovery.registration_candidate_cleanup.is_none());
}

#[tokio::test]
async fn recovery_registration_cleanup_resumes_after_custody_deleted_before_pending() {
    let root = tempfile::tempdir().unwrap();
    let core = recovery_test_core(root.path(), "https://example.invalid", [93; 32]);
    let candidate = registration(&core);
    let mut recovery = applied(&core);
    crate::internal::identity_custody::discard_unpublished_registration_async(
        &core,
        &candidate.identity,
    )
    .await
    .unwrap();
    assert!(PendingRegistrationStore::from_core(&core)
        .unwrap()
        .load("alice", "awiki.test")
        .unwrap()
        .is_some());
    let store = PendingHandleRecoveryStore::from_core(&core).unwrap();
    cleanup::finish(&core, &store, &mut recovery).await.unwrap();
    assert!(PendingRegistrationStore::from_core(&core)
        .unwrap()
        .load("alice", "awiki.test")
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn recovery_registration_cleanup_never_guesses_targets_for_old_records_or_before_success() {
    let root = tempfile::tempdir().unwrap();
    let core = recovery_test_core(root.path(), "https://example.invalid", [94; 32]);
    let candidate = registration(&core);
    let (mut recovery, _) =
        create_v4_awaiting_factor_operation(&core, "cleanup-before-commit", "owner-cleanup-2");
    recovery.registration_candidate_cleanup = cleanup::capture(&core, &recovery).unwrap();
    let store = PendingHandleRecoveryStore::from_core(&core).unwrap();
    cleanup::finish(&core, &store, &mut recovery).await.unwrap();
    assert!(exists(&core, &candidate));
    let mut recovery = applied(&core);
    let mut encoded = serde_json::to_value(&recovery).unwrap();
    encoded
        .as_object_mut()
        .unwrap()
        .remove("registration_candidate_cleanup");
    recovery = serde_json::from_value(encoded).unwrap();
    recovery.validate().unwrap();
    cleanup::finish(&core, &store, &mut recovery).await.unwrap();
    assert!(exists(&core, &candidate));
}

#[tokio::test]
async fn recovery_registration_cleanup_preserves_changed_pending_identity() {
    let root = tempfile::tempdir().unwrap();
    let core = recovery_test_core(root.path(), "https://example.invalid", [95; 32]);
    let candidate = registration(&core);
    let mut recovery = applied(&core);
    // Same Handle but a different document-change reference is not the captured operation.
    let mut changed = candidate.clone();
    changed.identity.controller_revision_id = Some("another-document-change".into());
    PendingRegistrationStore::from_core(&core)
        .unwrap()
        .save(&changed)
        .unwrap();
    let store = PendingHandleRecoveryStore::from_core(&core).unwrap();
    assert!(cleanup::finish(&core, &store, &mut recovery).await.is_err());
    assert!(exists(&core, &candidate));
    assert_eq!(
        PendingRegistrationStore::from_core(&core)
            .unwrap()
            .load("alice", "awiki.test")
            .unwrap()
            .unwrap()
            .1
            .identity,
        changed.identity
    );
}

#[tokio::test]
async fn recovery_registration_cleanup_preserves_committed_registration() {
    use crate::internal::identity_registration_pending::{
        PendingRegistrationPhase, PendingRegistrationRemoteResult,
    };
    let root = tempfile::tempdir().unwrap();
    let core = recovery_test_core(root.path(), "https://example.invalid", [96; 32]);
    let mut candidate = registration(&core);
    let mut recovery = applied(&core);
    candidate.remote_attempted = true;
    candidate.phase = PendingRegistrationPhase::RemoteCommitted;
    candidate.remote_result = Some(PendingRegistrationRemoteResult {
        did: candidate.identity.did.as_str().into(),
        user_id: "user-v4-1".into(),
        handle: "alice".into(),
        full_handle: "alice.awiki.test".into(),
        binding_generation: "9".into(),
        access_token: "test-only-token".into(),
    });
    PendingRegistrationStore::from_core(&core)
        .unwrap()
        .save(&candidate)
        .unwrap();
    assert!(cleanup::capture(&core, &recovery).unwrap().is_none());
    let store = PendingHandleRecoveryStore::from_core(&core).unwrap();
    assert!(cleanup::finish(&core, &store, &mut recovery).await.is_err());
    assert!(exists(&core, &candidate));
}

#[test]
fn recovery_registration_cleanup_capture_excludes_current_predecessor_and_other_handle() {
    let root = tempfile::tempdir().unwrap();
    let core = recovery_test_core(root.path(), "https://example.invalid", [97; 32]);
    let candidate = registration(&core);
    let (recovery, _) =
        create_v4_awaiting_factor_operation(&core, "cleanup-capture-guards", "owner-cleanup-3");
    assert!(cleanup::capture(&core, &recovery).unwrap().is_some());
    let mut current = recovery.clone();
    current.identity.did = candidate.identity.did.clone();
    assert!(cleanup::capture(&core, &current).unwrap().is_none());
    let mut predecessor = recovery.clone();
    predecessor.local_previous_did = candidate.identity.did.as_str().into();
    assert!(cleanup::capture(&core, &predecessor).unwrap().is_none());
    let mut different_handle = recovery;
    different_handle.full_handle = "bob.awiki.test".into();
    assert!(cleanup::capture(&core, &different_handle)
        .unwrap()
        .is_none());
    assert!(exists(&core, &candidate));
}

#[tokio::test]
async fn recovery_registration_cleanup_request_otp_captures_candidate_once_and_preserves_it_until_success(
) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = std::thread::spawn(move || {
        for _ in 0..2 {
            let (mut stream, _) = listener.accept().unwrap();
            let request = read_http_request(&mut stream);
            let body: Value =
                serde_json::from_str(request.split("\r\n\r\n").nth(1).unwrap()).unwrap();
            write_json_response(
                &mut stream,
                &json!({
                    "jsonrpc": "2.0", "id": body["id"],
                    "result": {"ok": true, "retry_after_seconds": 60, "retry_at": "2099-08-07T00:01:00Z"}
                }),
            );
        }
    });
    let root = tempfile::tempdir().unwrap();
    let core = recovery_test_core(root.path(), &endpoint, [98; 32]);
    let candidate = registration(&core);
    let request = HandleRecoveryOtpRequest {
        identity: None,
        full_handle: "alice.awiki.test".into(),
        phone: "+8613800000000".into(),
    };
    let first = request_otp(&core, request.clone()).await.unwrap();
    let second = request_otp(&core, request).await.unwrap();
    assert_eq!(first.operation_id, second.operation_id);
    let store = PendingHandleRecoveryStore::from_core(&core).unwrap();
    let (_, recovery) = store.load_v4(&first.operation_id).unwrap().unwrap();
    assert_ne!(recovery.identity.did, candidate.identity.did);
    assert_eq!(
        recovery.registration_candidate_cleanup.unwrap().identity,
        candidate.identity
    );
    assert!(exists(&core, &candidate));
    assert_eq!(
        store.list_v4_for_handle("alice.awiki.test").unwrap().len(),
        1
    );
    server.join().unwrap();
}
