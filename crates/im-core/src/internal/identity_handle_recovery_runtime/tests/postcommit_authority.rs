use super::*;
use crate::internal::identity_handle_recovery_operation::{
    self as operations, RecoveryLifecycleClass as Lifecycle,
};
use crate::internal::identity_transition_pending::{
    self as transitions, IdentityTransitionMarker, SupersededTransitionEvidence, TransitionPhase,
};

fn local_pending(
    core: &crate::ImCore,
    operation_id: &str,
) -> (PendingHandleRecoveryV4, IdentityTransitionMarker) {
    let mut pending =
        create_v4_operation_with_transition_identities(core, operation_id, "owner-superseded");
    make_v4_operation_remote_unresolved(core, &mut pending);
    let result = remote_result_for_pending(&pending, "user-refresh-1");
    let store = PendingHandleRecoveryStore::from_core(core).unwrap();
    let revision = pending.revision;
    pending.record_remote_result(result.clone()).unwrap();
    store.save_v4_cas(&pending, revision).unwrap();
    let path = &core.inner().sdk_paths().local_state.sqlite_path;
    let marker = IdentityTransitionMarker::initiator_v4(path, &pending, &result).unwrap();
    transitions::persist(path, &marker).unwrap();
    operations::update_lifecycle(
        path,
        operation_id,
        Lifecycle::RemoteUnresolved,
        Lifecycle::RemoteCommitted,
        None,
        None,
        "2026-09-08T00:00:00Z",
    )
    .unwrap();
    operations::update_lifecycle(
        path,
        operation_id,
        Lifecycle::RemoteCommitted,
        Lifecycle::LocalTransitionPending,
        Some(&marker.state_root_fingerprint),
        None,
        "2026-09-08T00:00:01Z",
    )
    .unwrap();
    let revision = pending.revision;
    pending.mark_local_transition_pending().unwrap();
    store.save_v4_cas(&pending, revision).unwrap();
    (pending, marker)
}

#[tokio::test]
async fn newer_authoritative_binding_closes_old_local_transition_without_losing_commit() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let root = tempfile::tempdir().unwrap();
    let core = recovery_test_core(root.path(), &endpoint, [107; 32]);
    let (pending, _) = local_pending(&core, "recover-v4-superseded-binding");
    let old_identities = core.identities().list().unwrap();
    let wns = json!({"handle": pending.full_handle, "did": pending.local_previous_did, "status": "active", "binding_generation": "9"});
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        assert!(read_http_request(&mut stream).starts_with("GET /.well-known/handle/alice "));
        write_json_response(&mut stream, &wns);
    });
    let result = resume(
        &core,
        HandleRecoveryResumeRequest {
            operation_id: pending.operation_id.clone(),
        },
    )
    .await;
    assert_eq!(
        result.err().as_ref().and_then(service_code),
        Some("local_transition_superseded")
    );
    server.join().unwrap();
    let path = &core.inner().sdk_paths().local_state.sqlite_path;
    let operation = operations::load(path, &pending.operation_id)
        .unwrap()
        .unwrap();
    assert_eq!(
        operation.lifecycle_class,
        Lifecycle::SupersededByStateChange
    );
    let marker = transitions::load(path, &pending.operation_id)
        .unwrap()
        .unwrap();
    assert_eq!(marker.phase, TransitionPhase::Superseded);
    assert!(marker.applied_at.is_none());
    let (_, persisted) = PendingHandleRecoveryStore::from_core(&core)
        .unwrap()
        .load_v4(&pending.operation_id)
        .unwrap()
        .unwrap();
    assert_eq!(persisted, pending);
    assert_eq!(core.identities().list().unwrap(), old_identities);
    let progress = status(&core, &pending.operation_id).unwrap();
    assert_eq!(
        progress.failure_code,
        Some(HandleRecoveryErrorCode::LocalTransitionSuperseded)
    );
    assert_eq!(
        progress.phase,
        HandleRecoveryPhase::IdentityTransitionPending
    );
    assert!(progress.allowed_actions.is_empty());
    assert!(progress.reset_reference.is_none());
    let context = core
        .handle_recovery()
        .inspect_handle_recovery_context(crate::identity::HandleRecoveryContextRequest {
            full_handle: pending.full_handle.clone(),
            identity: None,
        })
        .await
        .unwrap();
    assert_eq!(
        context.blocked_reason,
        Some(HandleRecoveryErrorCode::LocalTransitionSuperseded)
    );
    assert_eq!(
        context.allowed_actions,
        vec![crate::identity::HandleRecoveryAction::StartNew]
    );
    // No second remote lookup, local write or retry is allowed on this exact ID.
    let repeat = resume(
        &core,
        HandleRecoveryResumeRequest {
            operation_id: pending.operation_id.clone(),
        },
    )
    .await;
    assert_eq!(
        repeat.err().as_ref().and_then(service_code),
        Some("local_transition_superseded")
    );
    crate::internal::identity_handle_recovery_context::require_registration_admission(
        &core,
        &pending.full_handle,
    )
    .unwrap();
    let connection = crate::internal::local_state::open_writable(path).unwrap();
    let active: i64 = connection.query_row("SELECT COUNT(*) FROM identity_transition_pending WHERE owner_identity_id=?1 AND phase IN ('pending','identity_switched')", [&pending.owner_identity_id], |row| row.get(0)).unwrap();
    assert_eq!(active, 0);
}

#[test]
fn superseded_close_requires_monotonic_binding_and_exact_local_authority() {
    let root = tempfile::tempdir().unwrap();
    let core = recovery_test_core(root.path(), "http://127.0.0.1:1", [108; 32]);
    let (pending, marker) = local_pending(&core, "recover-v4-superseded-rejections");
    let path = &core.inner().sdk_paths().local_state.sqlite_path;
    for (generation, did, owner, intent) in [
        (
            "8",
            pending.local_previous_did.as_str(),
            marker.owner_identity_id.as_str(),
            pending.intent_hash.as_deref().unwrap(),
        ),
        (
            "7",
            pending.local_previous_did.as_str(),
            marker.owner_identity_id.as_str(),
            pending.intent_hash.as_deref().unwrap(),
        ),
        (
            "09",
            pending.local_previous_did.as_str(),
            marker.owner_identity_id.as_str(),
            pending.intent_hash.as_deref().unwrap(),
        ),
        (
            "9",
            marker.current_did.as_str(),
            marker.owner_identity_id.as_str(),
            pending.intent_hash.as_deref().unwrap(),
        ),
        (
            "9",
            pending.local_previous_did.as_str(),
            "wrong-owner",
            pending.intent_hash.as_deref().unwrap(),
        ),
        (
            "9",
            pending.local_previous_did.as_str(),
            marker.owner_identity_id.as_str(),
            "wrong-intent",
        ),
    ] {
        let mut candidate = marker.clone();
        candidate.owner_identity_id = owner.to_owned();
        let evidence = SupersededTransitionEvidence::HandleBinding {
            observed_did: did.to_owned(),
            observed_binding_generation: generation.to_owned(),
        };
        assert!(transitions::mark_superseded(
            path,
            &candidate,
            &evidence,
            intent,
            "2026-09-08T00:00:02Z"
        )
        .is_err());
        assert_eq!(
            transitions::load(path, &pending.operation_id)
                .unwrap()
                .unwrap(),
            marker
        );
        assert_eq!(
            operations::load(path, &pending.operation_id)
                .unwrap()
                .unwrap()
                .lifecycle_class,
            Lifecycle::LocalTransitionPending
        );
    }
}

#[test]
fn schema_42_preserves_same_development_pending_records() {
    let root = tempfile::tempdir().unwrap();
    let core = recovery_test_core(root.path(), "http://127.0.0.1:1", [109; 32]);
    let (pending, marker) = local_pending(&core, "recover-v4-schema-42-pending");
    let path = &core.inner().sdk_paths().local_state.sqlite_path;
    let connection = crate::internal::local_state::open_writable(path).unwrap();
    // Reconstruct only the previous CHECK in this disposable fixture.
    let old_sql = transitions::IDENTITY_TRANSITION_SQL.replace(",'superseded'", "");
    connection.execute_batch("ALTER TABLE identity_transition_pending RENAME TO fixture_transition_saved;
        DROP INDEX idx_identity_transition_source; DROP INDEX idx_identity_transition_active_owner;
        DROP INDEX idx_identity_transition_owner_phase; DROP INDEX idx_identity_transition_account_generation; DROP INDEX idx_identity_transition_handle_epoch;").unwrap();
    connection.execute_batch(&old_sql).unwrap();
    connection.execute_batch("INSERT INTO identity_transition_pending SELECT * FROM fixture_transition_saved; DROP TABLE fixture_transition_saved; PRAGMA user_version=41;").unwrap();
    crate::internal::local_state::schema::ensure_schema(&connection).unwrap();
    let version: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap();
    assert_eq!(version, 42);
    assert_eq!(
        transitions::load(path, &pending.operation_id)
            .unwrap()
            .unwrap(),
        marker
    );
    assert_eq!(
        PendingHandleRecoveryStore::from_core(&core)
            .unwrap()
            .load_v4(&pending.operation_id)
            .unwrap()
            .unwrap()
            .1,
        pending
    );
    transitions::mark_superseded(
        path,
        &marker,
        &SupersededTransitionEvidence::HandleBinding {
            observed_did: pending.local_previous_did.clone(),
            observed_binding_generation: "9".to_owned(),
        },
        pending.intent_hash.as_deref().unwrap(),
        "2026-09-08T00:00:02Z",
    )
    .unwrap();
}

#[cfg(all(feature = "provider-traits", feature = "identity-native-anp"))]
#[tokio::test]
async fn revoked_bootstrap_requires_independent_root_verified_authority() {
    use crate::internal::identity_handle_recovery_authority::removed_bootstrap_authority;
    let root = tempfile::tempdir().unwrap();
    let core = recovery_test_core(root.path(), "http://127.0.0.1:1", [110; 32]);
    let (pending, marker) = local_pending(&core, "recover-v4-revoked-bootstrap");
    assert!(
        removed_bootstrap_authority(&pending, &pending.identity.did_document)
            .unwrap()
            .is_none()
    );
    let mut unsigned = pending.identity.did_document.clone();
    unsigned.as_object_mut().unwrap().remove("proof");
    unsigned["authentication"] = json!([]);
    assert!(removed_bootstrap_authority(&pending, &unsigned)
        .unwrap()
        .is_none());
    let provider = crate::internal::identity_custody::controller_custody_provider(&core)
        .await
        .unwrap();
    let reference = crate::internal::identity_provider::ProviderIdentityRef {
        store_id: pending.identity.store_id.clone(),
        identity_id: pending.identity.identity_id.clone(),
        did: pending.identity.did.as_str().to_owned(),
    };
    let signed = provider
        .sign_document_proof(
            &reference,
            crate::internal::identity_provider::ProviderDocumentProofRequest {
                key: crate::internal::identity_provider::ProviderKeySelector::Kid(
                    pending.identity.root_key_id.clone(),
                ),
                document: unsigned,
                options: crate::internal::identity_provider::ProviderDocumentProofOptions {
                    proof_purpose: Some("assertionMethod".to_owned()),
                    proof_type: Some(anp::proof::PROOF_TYPE_DATA_INTEGRITY.to_owned()),
                    cryptosuite: Some(anp::proof::CRYPTOSUITE_EDDSA_JCS_2022.to_owned()),
                    created: Some(now_second_z().unwrap()),
                    domain: Some("awiki.test".to_owned()),
                    challenge: Some("revoked-bootstrap-fixture".to_owned()),
                },
            },
        )
        .await
        .unwrap();
    let evidence = removed_bootstrap_authority(&pending, &signed)
        .unwrap()
        .expect("a verified removal is terminal authority");
    let path = &core.inner().sdk_paths().local_state.sqlite_path;
    transitions::update_phase(
        path,
        &pending.operation_id,
        TransitionPhase::Pending,
        TransitionPhase::IdentitySwitched,
    )
    .unwrap();
    let mut marker = marker;
    marker.phase = TransitionPhase::IdentitySwitched;
    transitions::mark_superseded(
        path,
        &marker,
        &evidence,
        pending.intent_hash.as_deref().unwrap(),
        "2026-09-08T00:00:02Z",
    )
    .unwrap();
    assert_eq!(
        transitions::load(path, &pending.operation_id)
            .unwrap()
            .unwrap()
            .phase,
        TransitionPhase::Superseded
    );
    assert_eq!(
        PendingHandleRecoveryStore::from_core(&core)
            .unwrap()
            .load_v4(&pending.operation_id)
            .unwrap()
            .unwrap()
            .1,
        pending
    );
}

#[test]
fn local_retry_error_updates_both_active_authorities_without_masking_its_code() {
    let root = tempfile::tempdir().unwrap();
    let core = recovery_test_core(root.path(), "http://127.0.0.1:1", [113; 32]);
    let (mut pending, _) = local_pending(&core, "recover-v4-local-error-index");
    let original_result = pending.remote_result.clone();
    let store = PendingHandleRecoveryStore::from_core(&core).unwrap();
    assert!(
        persist_nonterminal_error_v4(
            &core,
            &store,
            &mut pending,
            HandleRecoveryErrorCode::LocalTransitionPending
        )
        .is_ok(),
        "local retry error must not be replaced by an index PermissionDenied"
    );
    let indexed = operations::load(
        &core.inner().sdk_paths().local_state.sqlite_path,
        &pending.operation_id,
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        indexed.last_error_code.as_deref(),
        Some("local_transition_pending")
    );
    assert!(
        store
            .load_v4(&pending.operation_id)
            .unwrap()
            .unwrap()
            .1
            .remote_result
            == original_result
    );
}

#[cfg(all(feature = "provider-traits", feature = "identity-native-anp"))]
#[tokio::test]
async fn committed_result_is_durable_before_local_custody_finalization() {
    // Runtime assertions below verify the journal projection. This source
    // contract additionally pins the failure boundary: custody confirmation is
    // exclusively a local-transition step after the authority read.
    let source = include_str!("../../identity_handle_recovery_runtime.rs");
    let send = source
        .split("async fn send_commit_v4(")
        .nth(1)
        .unwrap()
        .split("fn unsigned_recovery_predecessor_document(")
        .next()
        .unwrap();
    assert!(!send.contains("confirm_handle_recovery_transition_published("));
    let apply = source
        .split("async fn apply_local_transition_v4(")
        .nth(1)
        .unwrap()
        .split("fn validate_switched_identity_v4(")
        .next()
        .unwrap();
    assert!(
        apply.find("require_current_binding(").unwrap()
            < apply
                .find("confirm_handle_recovery_transition_published(")
                .unwrap()
    );

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let root = tempfile::tempdir().unwrap();
    let core = recovery_test_core(
        root.path(),
        &format!("http://{}", listener.local_addr().unwrap()),
        [114; 32],
    );
    let mut pending = create_v4_operation_with_transition_identities(
        &core,
        "recover-v4-journal-before-custody",
        "owner-journal-first",
    );
    make_v4_operation_remote_unresolved(&core, &mut pending);
    let result = remote_result_for_pending(&pending, "user-refresh-1");
    let response = serde_json::to_value(&result).unwrap();
    let provider = crate::internal::identity_custody::controller_custody_provider(&core)
        .await
        .unwrap();
    let before = provider.list_identities().await.unwrap();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let raw = read_http_request(&mut stream);
        let request: serde_json::Value =
            serde_json::from_str(raw.split_once("\r\n\r\n").unwrap().1).unwrap();
        assert_eq!(request["method"], "handle_recovery_commit_v4");
        write_json_response(
            &mut stream,
            &json!({"jsonrpc":"2.0","id":request["id"],"result":response}),
        );
    });
    let store = PendingHandleRecoveryStore::from_core(&core).unwrap();
    assert!(send_commit_v4(&core, &store, &mut pending).await.unwrap());
    server.join().unwrap();
    let persisted = store.load_v4(&pending.operation_id).unwrap().unwrap().1;
    assert!(persisted.remote_result == Some(result));
    assert_eq!(persisted.phase, PendingRecoveryPhaseV4::RemoteCommitted);
    assert!(provider.list_identities().await.unwrap() == before,
        "local custody must not finalize until the committed journal and current authority are checked");
}

#[tokio::test]
async fn inspection_repairs_committed_and_applied_half_indexes_without_business_io() {
    let root = tempfile::tempdir().unwrap();
    let core = recovery_test_core(root.path(), "http://127.0.0.1:1", [115; 32]);
    let mut pending = create_v4_operation_with_transition_identities(
        &core,
        "recover-v4-inspect-committed-half",
        "owner-inspect-half",
    );
    make_v4_operation_remote_unresolved(&core, &mut pending);
    let result = remote_result_for_pending(&pending, "user-refresh-1");
    let store = PendingHandleRecoveryStore::from_core(&core).unwrap();
    let revision = pending.revision;
    pending.record_remote_result(result).unwrap();
    store.save_v4_cas(&pending, revision).unwrap();
    let context = core
        .handle_recovery()
        .inspect_handle_recovery_context(crate::identity::HandleRecoveryContextRequest {
            full_handle: pending.full_handle.clone(),
            identity: None,
        })
        .await
        .unwrap();
    assert_eq!(
        context.operation.unwrap().lifecycle_class,
        crate::identity::HandleRecoveryOperationLifecycle::RemoteCommitted
    );
    assert_eq!(
        context.progress.unwrap().phase,
        HandleRecoveryPhase::RemoteCommitted
    );
    assert!(store.load_v4(&pending.operation_id).unwrap().unwrap().1 == pending);

    let root = tempfile::tempdir().unwrap();
    let core = recovery_test_core(root.path(), "http://127.0.0.1:1", [116; 32]);
    let (mut pending, _) = local_pending(&core, "recover-v4-inspect-applied-half");
    let result = pending.remote_result.as_ref().unwrap();
    let path = &core.inner().sdk_paths().local_state.sqlite_path;
    transitions::mark_applied(
        path,
        &pending.operation_id,
        TransitionPhase::Pending,
        &result.bootstrap_device.device_id,
        "1",
        "1",
        "{}",
    )
    .unwrap();
    let store = PendingHandleRecoveryStore::from_core(&core).unwrap();
    let revision = pending.revision;
    pending.mark_applied().unwrap();
    store.save_v4_cas(&pending, revision).unwrap();
    let context = core
        .handle_recovery()
        .inspect_handle_recovery_context(crate::identity::HandleRecoveryContextRequest {
            full_handle: pending.full_handle.clone(),
            identity: None,
        })
        .await
        .unwrap();
    assert_eq!(
        context.operation.unwrap().lifecycle_class,
        crate::identity::HandleRecoveryOperationLifecycle::Applied
    );
    assert_eq!(
        context.progress.unwrap().phase,
        HandleRecoveryPhase::Applied
    );
    assert!(store.load_v4(&pending.operation_id).unwrap().unwrap().1 == pending);
}
