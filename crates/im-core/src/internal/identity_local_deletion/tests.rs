use super::*;

fn snapshot(owner: &str) -> LocalIdentityDeletionSnapshot {
    LocalIdentityDeletionSnapshot {
        owner_identity_id: owner.to_owned(),
        current_did: format!("did:wba:example.invalid:user:{owner}:e1_current"),
        full_handle: Some(format!("{owner}.example.invalid")),
        local_alias: owner.to_owned(),
        identity_dir_name: Some(format!("identity-{owner}")),
        next_default_alias: Some("sibling".to_owned()),
        protocol_device_id: Some(format!("device-{owner}")),
    }
}

fn path() -> (tempfile::TempDir, std::path::PathBuf) {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("im.sqlite");
    let connection = crate::internal::local_state::open_writable(&path).unwrap();
    crate::internal::local_state::schema::ensure_schema(&connection).unwrap();
    drop(connection);
    (root, path)
}

fn service_code(error: &crate::ImError) -> Option<&str> {
    match error {
        crate::ImError::Service { code, .. } => code.as_deref(),
        _ => None,
    }
}

fn insert_operation(
    path: &std::path::Path,
    owner: &str,
    operation_id: &str,
) -> crate::internal::identity_handle_recovery_operation::RecoveryOperationRecord {
    let record =
        crate::internal::identity_handle_recovery_operation::RecoveryOperationRecord::pre_commit(
            operation_id.to_owned(),
            owner.to_owned(),
            format!("{owner}.example.invalid"),
            format!("vault-{operation_id}"),
            "2026-08-29T00:00:00Z".to_owned(),
        )
        .unwrap();
    crate::internal::identity_handle_recovery_operation::insert(path, &record).unwrap();
    record
}

#[test]
fn deletion_allows_owner_without_active_control_state() {
    let (_root, path) = path();
    let record = prepare_with_id(
        &path,
        &snapshot("alice"),
        LocalIdentityDeletionMode::FullDataApp,
        "delete_alice_12345678",
        "2026-08-29T00:00:00Z",
    )
    .unwrap();

    assert_eq!(record.phase, LocalIdentityDeletionPhase::Prepared);
    assert_eq!(record.mode, LocalIdentityDeletionMode::FullDataApp);
}

#[test]
fn deletion_requires_explicit_discard_for_unattempted_precommit() {
    let (_root, path) = path();
    insert_operation(&path, "alice", "recover_precommit_12345678");

    let error = prepare_with_id(
        &path,
        &snapshot("alice"),
        LocalIdentityDeletionMode::CredentialOnly,
        "delete_alice_12345678",
        "2026-08-29T00:01:00Z",
    )
    .unwrap_err();

    assert_eq!(
        service_code(&error),
        Some("handle_recovery.precommit_discard_required")
    );
    assert!(load_active_owner(&path, "alice").unwrap().is_none());
}

#[test]
fn deletion_rejects_remote_unresolved() {
    let (_root, path) = path();
    insert_operation(&path, "alice", "recover_unresolved_12345678");
    crate::internal::identity_handle_recovery_operation::mark_commit_attempted(
        &path,
        "recover_unresolved_12345678",
        "2026-08-29T00:01:00Z",
    )
    .unwrap();

    let error = prepare_with_id(
        &path,
        &snapshot("alice"),
        LocalIdentityDeletionMode::FullDataApp,
        "delete_alice_12345678",
        "2026-08-29T00:02:00Z",
    )
    .unwrap_err();
    assert_eq!(
        service_code(&error),
        Some("handle_recovery.operation_must_resume")
    );
}

fn assert_deletion_rejects_transition_lifecycle(
    lifecycle: crate::internal::identity_handle_recovery_operation::RecoveryLifecycleClass,
) {
    let (_root, path) = path();
    let operation_id = format!("recover_transition_{}_12345678", lifecycle.as_str());
    insert_operation(&path, "alice", &operation_id);
    crate::internal::identity_handle_recovery_operation::update_lifecycle(
        &path,
        &operation_id,
        crate::internal::identity_handle_recovery_operation::RecoveryLifecycleClass::PreCommit,
        lifecycle,
        None,
        None,
        "2026-08-29T00:01:00Z",
    )
    .unwrap();

    let error = prepare_with_id(
        &path,
        &snapshot("alice"),
        LocalIdentityDeletionMode::FullDataApp,
        "delete_alice_12345678",
        "2026-08-29T00:02:00Z",
    )
    .unwrap_err();
    assert_eq!(
        service_code(&error),
        Some("handle_recovery.transition_must_complete")
    );
}

#[test]
fn deletion_rejects_remote_committed() {
    assert_deletion_rejects_transition_lifecycle(
        crate::internal::identity_handle_recovery_operation::RecoveryLifecycleClass::RemoteCommitted,
    );
}

#[test]
fn deletion_rejects_local_transition_pending() {
    assert_deletion_rejects_transition_lifecycle(
        crate::internal::identity_handle_recovery_operation::RecoveryLifecycleClass::LocalTransitionPending,
    );
}

#[test]
fn deletion_rejects_pending_or_identity_switched_join() {
    for phase in ["pending", "identity_switched"] {
        let (_root, path) = path();
        let connection = crate::internal::local_state::open_writable(&path).unwrap();
        connection
            .execute(
                "INSERT INTO identity_transition_pending(\
                 recovery_id,schema_version,contract_version,contract_hash,source_kind,source_id,\
                 state_root_fingerprint,account_user_id,owner_identity_id,handle,previous_did,\
                 current_did,binding_generation,metadata_json,phase,created_at,updated_at) \
                 VALUES (?1,1,?2,?3,'joined_device',?4,?5,'account-alice','alice',\
                 'alice.example.invalid','did:wba:example.invalid:user:alice:e1_previous',\
                 'did:wba:example.invalid:user:alice:e1_current','2','{}',?6,\
                 '2026-08-29T00:00:00Z','2026-08-29T00:00:00Z')",
                rusqlite::params![
                    format!("joined-{phase}"),
                    crate::internal::identity_handle_recovery_pending::V4_CONTRACT_VERSION,
                    crate::internal::identity_handle_recovery_pending::V4_CONTRACT_HASH,
                    format!("join-{phase}"),
                    crate::internal::identity_transition_pending::state_root_fingerprint(&path),
                    phase,
                ],
            )
            .unwrap();
        drop(connection);

        let error = prepare_with_id(
            &path,
            &snapshot("alice"),
            LocalIdentityDeletionMode::FullDataApp,
            "delete_alice_12345678",
            "2026-08-29T00:02:00Z",
        )
        .unwrap_err();
        assert_eq!(
            service_code(&error),
            Some(if phase == "pending" {
                "handle_recovery.join_must_complete"
            } else {
                "handle_recovery.transition_must_complete"
            })
        );
    }
}

#[test]
fn deletion_rejects_prepared_retired_join_rollover() {
    let (_root, path) = path();
    let connection = crate::internal::local_state::open_writable(&path).unwrap();
    connection
        .execute(
            "INSERT INTO registration_retired_join_rollovers(\
             join_session_id,schema_version,account_user_id,owner_identity_id,handle,retired_did,\
             retired_device_id,retired_binding_generation,current_did,current_binding_generation,\
             new_device_id,join_expires_at,phase,created_at,updated_at) \
             VALUES ('join-rollover',1,'account-alice','alice','alice.example.invalid',\
             'did:wba:example.invalid:user:alice:e1_current','device-old','1',\
             'did:wba:example.invalid:user:alice:e1_current','1','device-new',\
             '2099-08-29T00:00:00Z','prepared','2026-08-29T00:00:00Z','2026-08-29T00:00:00Z')",
            [],
        )
        .unwrap();
    drop(connection);

    let error = prepare_with_id(
        &path,
        &snapshot("alice"),
        LocalIdentityDeletionMode::FullDataApp,
        "delete_alice_12345678",
        "2026-08-29T00:02:00Z",
    )
    .unwrap_err();
    assert_eq!(
        service_code(&error),
        Some("handle_recovery.join_must_complete")
    );
}

#[test]
fn deletion_allows_terminal_operations_and_preserves_rows() {
    for (index, lifecycle, commit_attempted, key_state) in [
        (0, "applied", 1_i64, "available"),
        (1, "discarded_pre_attempt", 0, "destroyed_pre_attempt"),
        (2, "superseded_by_state_change", 1, "available"),
        (3, "failed_terminal", 1, "available"),
    ] {
        let (_root, path) = path();
        let operation_id = format!("recover_terminal_{index}_12345678");
        insert_operation(&path, "alice", &operation_id);
        let connection = crate::internal::local_state::open_writable(&path).unwrap();
        connection
            .execute(
                "UPDATE handle_recovery_operations_v4 SET lifecycle_class=?2,commit_attempted=?3,key_state=?4 WHERE operation_id=?1",
                rusqlite::params![operation_id, lifecycle, commit_attempted, key_state],
            )
            .unwrap();
        drop(connection);

        prepare_with_id(
            &path,
            &snapshot("alice"),
            LocalIdentityDeletionMode::FullDataApp,
            "delete_alice_12345678",
            "2026-08-29T00:02:00Z",
        )
        .unwrap();
        assert!(
            crate::internal::identity_handle_recovery_operation::load(&path, &operation_id,)
                .unwrap()
                .is_some()
        );
    }
}

#[test]
fn deletion_allows_only_confirmed_quarantined_terminal_operation() {
    let (_root, path) = path();
    insert_operation(&path, "alice", "recover_quarantine_12345678");
    crate::internal::identity_handle_recovery_operation::quarantine_key_unavailable(
        &path,
        "recover_quarantine_12345678",
        "2026-08-29T00:01:00Z",
    )
    .unwrap();

    prepare_with_id(
        &path,
        &snapshot("alice"),
        LocalIdentityDeletionMode::CredentialOnly,
        "delete_alice_12345678",
        "2026-08-29T00:02:00Z",
    )
    .unwrap();
}

#[test]
fn deletion_rejects_unverified_or_replaced_quarantined_operation() {
    let (_root, unverified_path) = path();
    insert_operation(
        &unverified_path,
        "alice",
        "recover_quarantine_unverified_12345678",
    );
    let connection = crate::internal::local_state::open_writable(&unverified_path).unwrap();
    connection
        .execute(
            "UPDATE handle_recovery_operations_v4 SET lifecycle_class='quarantined_key_unavailable' WHERE operation_id='recover_quarantine_unverified_12345678'",
            [],
        )
        .unwrap();
    drop(connection);
    let error = prepare_with_id(
        &unverified_path,
        &snapshot("alice"),
        LocalIdentityDeletionMode::FullDataApp,
        "delete_quarantine_unverified_12345678",
        "2026-08-29T00:02:00Z",
    )
    .unwrap_err();
    assert_eq!(
        service_code(&error),
        Some("identity.local_deletion_conflict")
    );

    let (_root, replaced_path) = path();
    insert_operation(
        &replaced_path,
        "alice",
        "recover_quarantine_replaced_12345678",
    );
    crate::internal::identity_handle_recovery_operation::quarantine_key_unavailable(
        &replaced_path,
        "recover_quarantine_replaced_12345678",
        "2026-08-29T00:01:00Z",
    )
    .unwrap();
    insert_operation(
        &replaced_path,
        "alice",
        "recover_replacement_active_12345678",
    );
    assert!(
        crate::internal::identity_handle_recovery_operation::claim_quarantined_replacement(
            &replaced_path,
            "recover_replacement_active_12345678",
            "alice",
            "alice.example.invalid",
            "2026-08-29T00:02:00Z",
        )
        .unwrap()
    );
    let error = prepare_with_id(
        &replaced_path,
        &snapshot("alice"),
        LocalIdentityDeletionMode::FullDataApp,
        "delete_quarantine_replaced_12345678",
        "2026-08-29T00:03:00Z",
    )
    .unwrap_err();
    assert_eq!(
        service_code(&error),
        Some("identity.local_deletion_conflict")
    );
}

#[test]
fn deletion_reuses_existing_active_ticket() {
    let (_root, path) = path();
    let first = prepare_with_id(
        &path,
        &snapshot("alice"),
        LocalIdentityDeletionMode::FullDataApp,
        "delete_alice_12345678",
        "2026-08-29T00:00:00Z",
    )
    .unwrap();
    let repeated = prepare_with_id(
        &path,
        &snapshot("alice"),
        LocalIdentityDeletionMode::FullDataApp,
        "delete_other_12345678",
        "2026-08-29T00:01:00Z",
    )
    .unwrap();

    assert_eq!(repeated, first);
}

#[test]
fn deletion_rejects_active_ticket_for_same_handle_with_different_owner() {
    let (_root, path) = path();
    prepare_with_id(
        &path,
        &snapshot("alice"),
        LocalIdentityDeletionMode::FullDataApp,
        "delete_alice_12345678",
        "2026-08-29T00:00:00Z",
    )
    .unwrap();
    let mut other = snapshot("bob");
    other.full_handle = Some("alice.example.invalid".to_owned());
    let error = prepare_with_id(
        &path,
        &other,
        LocalIdentityDeletionMode::FullDataApp,
        "delete_bob_12345678",
        "2026-08-29T00:01:00Z",
    )
    .unwrap_err();
    assert_eq!(
        service_code(&error),
        Some("identity.local_deletion_conflict")
    );
    assert!(load_active_owner(&path, "bob").unwrap().is_none());
}

#[test]
fn deletion_admission_matches_active_recovery_by_handle_across_owner_ids() {
    let (_root, path) = path();
    let operation =
        crate::internal::identity_handle_recovery_operation::RecoveryOperationRecord::pre_commit(
            "recover_same_handle_other_owner_12345678".to_owned(),
            "bob".to_owned(),
            "alice.example.invalid".to_owned(),
            "vault-same-handle-other-owner".to_owned(),
            "2026-08-29T00:00:00Z".to_owned(),
        )
        .unwrap();
    crate::internal::identity_handle_recovery_operation::insert(&path, &operation).unwrap();

    let error = prepare_with_id(
        &path,
        &snapshot("alice"),
        LocalIdentityDeletionMode::FullDataApp,
        "delete_alice_12345678",
        "2026-08-29T00:01:00Z",
    )
    .unwrap_err();
    assert_eq!(
        service_code(&error),
        Some("handle_recovery.precommit_discard_required")
    );
}

#[test]
fn deletion_admission_matches_join_and_rollover_by_handle_across_owner_ids() {
    let (_root, transition_path) = path();
    let connection = crate::internal::local_state::open_writable(&transition_path).unwrap();
    connection
        .execute(
            "INSERT INTO identity_transition_pending(\
             recovery_id,schema_version,contract_version,contract_hash,source_kind,source_id,\
             state_root_fingerprint,account_user_id,owner_identity_id,handle,previous_did,\
             current_did,binding_generation,metadata_json,phase,created_at,updated_at) \
             VALUES ('joined-other-owner',1,?1,?2,'joined_device','join-other-owner',?3,\
             'account-bob','bob','alice.example.invalid',\
             'did:wba:example.invalid:user:alice:e1_previous',\
             'did:wba:example.invalid:user:alice:e1_current','2','{}','pending',\
             '2026-08-29T00:00:00Z','2026-08-29T00:00:00Z')",
            rusqlite::params![
                crate::internal::identity_handle_recovery_pending::V4_CONTRACT_VERSION,
                crate::internal::identity_handle_recovery_pending::V4_CONTRACT_HASH,
                crate::internal::identity_transition_pending::state_root_fingerprint(
                    &transition_path,
                ),
            ],
        )
        .unwrap();
    drop(connection);
    let error = prepare_with_id(
        &transition_path,
        &snapshot("alice"),
        LocalIdentityDeletionMode::FullDataApp,
        "delete_join_other_owner_12345678",
        "2026-08-29T00:01:00Z",
    )
    .unwrap_err();
    assert_eq!(
        service_code(&error),
        Some("handle_recovery.join_must_complete")
    );

    let (_root, rollover_path) = path();
    let connection = crate::internal::local_state::open_writable(&rollover_path).unwrap();
    connection
        .execute(
            "INSERT INTO registration_retired_join_rollovers(\
             join_session_id,schema_version,account_user_id,owner_identity_id,handle,retired_did,\
             retired_device_id,retired_binding_generation,current_did,current_binding_generation,\
             new_device_id,join_expires_at,phase,created_at,updated_at) \
             VALUES ('join-rollover-other-owner',1,'account-bob','bob','alice.example.invalid',\
             'did:wba:example.invalid:user:alice:e1_current','device-old','1',\
             'did:wba:example.invalid:user:alice:e1_current','1','device-new',\
             '2099-08-29T00:00:00Z','prepared','2026-08-29T00:00:00Z',\
             '2026-08-29T00:00:00Z')",
            [],
        )
        .unwrap();
    drop(connection);
    let error = prepare_with_id(
        &rollover_path,
        &snapshot("alice"),
        LocalIdentityDeletionMode::FullDataApp,
        "delete_rollover_other_owner_12345678",
        "2026-08-29T00:01:00Z",
    )
    .unwrap_err();
    assert_eq!(
        service_code(&error),
        Some("handle_recovery.join_must_complete")
    );
}

#[test]
fn deletion_mode_is_one_closed_enum() {
    assert_eq!(
        LocalIdentityDeletionMode::CredentialOnly.as_str(),
        "credential_only"
    );
    assert_eq!(
        LocalIdentityDeletionMode::FullDataCore.as_str(),
        "full_data_core"
    );
    assert_eq!(
        LocalIdentityDeletionMode::FullDataApp.as_str(),
        "full_data_app"
    );
    assert!(LocalIdentityDeletionMode::parse("full_data").is_err());
}

#[test]
fn deletion_prepare_and_recovery_insert_are_mutually_exclusive() {
    let (_root, path) = path();
    let record =
        crate::internal::identity_handle_recovery_operation::RecoveryOperationRecord::pre_commit(
            "recover_racing_delete_12345678".to_owned(),
            "bob".to_owned(),
            "alice.example.invalid".to_owned(),
            "vault-racing-delete".to_owned(),
            "2026-08-29T00:01:00Z".to_owned(),
        )
        .unwrap();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
    let deletion_path = path.clone();
    let deletion_barrier = barrier.clone();
    let deletion = std::thread::spawn(move || {
        deletion_barrier.wait();
        prepare_with_id(
            &deletion_path,
            &snapshot("alice"),
            LocalIdentityDeletionMode::FullDataApp,
            "delete_alice_12345678",
            "2026-08-29T00:00:00Z",
        )
    });
    let recovery_path = path.clone();
    let recovery_barrier = barrier.clone();
    let recovery = std::thread::spawn(move || {
        recovery_barrier.wait();
        crate::internal::identity_handle_recovery_operation::insert(&recovery_path, &record)
    });
    barrier.wait();
    let deletion = deletion.join().unwrap();
    let recovery = recovery.join().unwrap();
    assert_ne!(deletion.is_ok(), recovery.is_ok());
    if let Err(error) = deletion {
        assert_eq!(
            service_code(&error),
            Some("handle_recovery.precommit_discard_required")
        );
    }
    if let Err(error) = recovery {
        assert_eq!(
            service_code(&error),
            Some("identity.local_deletion_conflict")
        );
    }
    let active_deletion = load_active_owner(&path, "alice").unwrap().is_some();
    let active_recovery = crate::internal::identity_handle_recovery_operation::load(
        &path,
        "recover_racing_delete_12345678",
    )
    .unwrap()
    .is_some();
    assert_ne!(active_deletion, active_recovery);
}

#[test]
fn active_deletion_does_not_block_existing_recovery_resume_or_apply() {
    let (_root, path) = path();
    let operation_id = "recover-existing-during-delete-12345678";
    insert_operation(&path, "alice", operation_id);
    let connection = crate::internal::local_state::open_writable(&path).unwrap();
    connection
        .execute(
            r#"INSERT INTO local_identity_deletions(
deletion_id,schema_version,mode,owner_identity_id,current_did,full_handle,local_alias,
identity_dir_name,next_default_alias,protocol_device_id,phase,created_at,updated_at,completed_at)
VALUES ('delete_existing_recovery_12345678',1,'full_data_app','alice',
'did:wba:example.invalid:user:alice:e1_current','alice.example.invalid','alice',
NULL,NULL,NULL,'prepared','2026-08-29T00:00:01Z','2026-08-29T00:00:01Z',NULL)"#,
            [],
        )
        .unwrap();
    drop(connection);

    crate::internal::identity_handle_recovery_operation::mark_commit_attempted(
        &path,
        operation_id,
        "2026-08-29T00:00:02Z",
    )
    .unwrap();
    for (expected, next, timestamp) in [
        (
            crate::internal::identity_handle_recovery_operation::RecoveryLifecycleClass::RemoteUnresolved,
            crate::internal::identity_handle_recovery_operation::RecoveryLifecycleClass::RemoteCommitted,
            "2026-08-29T00:00:03Z",
        ),
        (
            crate::internal::identity_handle_recovery_operation::RecoveryLifecycleClass::RemoteCommitted,
            crate::internal::identity_handle_recovery_operation::RecoveryLifecycleClass::LocalTransitionPending,
            "2026-08-29T00:00:04Z",
        ),
        (
            crate::internal::identity_handle_recovery_operation::RecoveryLifecycleClass::LocalTransitionPending,
            crate::internal::identity_handle_recovery_operation::RecoveryLifecycleClass::Applied,
            "2026-08-29T00:00:05Z",
        ),
    ] {
        crate::internal::identity_handle_recovery_operation::update_lifecycle(
            &path,
            operation_id,
            expected,
            next,
            None,
            None,
            timestamp,
        )
        .unwrap();
    }

    let operation = crate::internal::identity_handle_recovery_operation::load(&path, operation_id)
        .unwrap()
        .unwrap();
    assert_eq!(
        operation.lifecycle_class,
        crate::internal::identity_handle_recovery_operation::RecoveryLifecycleClass::Applied
    );
    assert!(load_active_owner(&path, "alice").unwrap().is_some());
}

#[test]
fn deletion_prepare_and_join_marker_insert_are_mutually_exclusive() {
    let (_root, path) = path();
    let marker =
        crate::internal::identity_transition_pending::IdentityTransitionMarker::joined_device(
            &path,
            "join-after-delete",
            "account-alice",
            "bob",
            "alice.example.invalid",
            "did:wba:example.invalid:user:alice:e1_previous",
            "did:wba:example.invalid:user:alice:e1_current",
            "2",
        )
        .unwrap();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
    let deletion_path = path.clone();
    let deletion_barrier = barrier.clone();
    let deletion = std::thread::spawn(move || {
        deletion_barrier.wait();
        prepare_with_id(
            &deletion_path,
            &snapshot("alice"),
            LocalIdentityDeletionMode::CredentialOnly,
            "delete_alice_12345678",
            "2026-08-29T00:00:00Z",
        )
    });
    let marker_path = path.clone();
    let marker_barrier = barrier.clone();
    let marker = std::thread::spawn(move || {
        marker_barrier.wait();
        crate::internal::identity_transition_pending::persist(&marker_path, &marker)
    });
    barrier.wait();
    let deletion = deletion.join().unwrap();
    let marker = marker.join().unwrap();
    assert_ne!(deletion.is_ok(), marker.is_ok());
    if let Err(error) = deletion {
        assert_eq!(
            service_code(&error),
            Some("handle_recovery.join_must_complete")
        );
    }
    if let Err(error) = marker {
        assert_eq!(
            service_code(&error),
            Some("identity.local_deletion_conflict")
        );
    }
    let active_deletion = load_active_owner(&path, "alice").unwrap().is_some();
    let active_marker = crate::internal::identity_transition_pending::load_joined_device(
        &path,
        "join-after-delete",
    )
    .unwrap()
    .is_some();
    assert_ne!(active_deletion, active_marker);
}

#[test]
fn deletion_prepare_and_rollover_insert_are_mutually_exclusive() {
    let (_root, path) = path();
    let rollover = crate::internal::identity_registration_retired_join::RetiredJoinRollover {
        join_session_id: "join-rollover-after-delete".to_owned(),
        schema_version: 1,
        account_user_id: "account-alice".to_owned(),
        owner_identity_id: "bob".to_owned(),
        handle: "alice.example.invalid".to_owned(),
        retired_did: "did:wba:example.invalid:user:alice:e1_current".to_owned(),
        retired_device_id: "device-old".to_owned(),
        retired_binding_generation: "1".to_owned(),
        current_did: "did:wba:example.invalid:user:alice:e1_current".to_owned(),
        current_binding_generation: "1".to_owned(),
        new_device_id: "device-new".to_owned(),
        join_expires_at: "2099-08-29T00:00:00Z".to_owned(),
        completed_auth_generation: None,
        phase:
            crate::internal::identity_registration_retired_join::RetiredJoinRolloverPhase::Prepared,
        created_at: "2026-08-29T00:01:00Z".to_owned(),
        updated_at: "2026-08-29T00:01:00Z".to_owned(),
        completed_at: None,
    };
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
    let deletion_path = path.clone();
    let deletion_barrier = barrier.clone();
    let deletion = std::thread::spawn(move || {
        deletion_barrier.wait();
        prepare_with_id(
            &deletion_path,
            &snapshot("alice"),
            LocalIdentityDeletionMode::FullDataCore,
            "delete_alice_12345678",
            "2026-08-29T00:00:00Z",
        )
    });
    let rollover_path = path.clone();
    let rollover_barrier = barrier.clone();
    let rollover = std::thread::spawn(move || {
        rollover_barrier.wait();
        crate::internal::identity_registration_retired_join::insert_prepared(
            &rollover_path,
            &rollover,
        )
    });
    barrier.wait();
    let deletion = deletion.join().unwrap();
    let rollover = rollover.join().unwrap();
    assert_ne!(deletion.is_ok(), rollover.is_ok());
    if let Err(error) = deletion {
        assert_eq!(
            service_code(&error),
            Some("handle_recovery.join_must_complete")
        );
    }
    if let Err(error) = rollover {
        assert_eq!(
            service_code(&error),
            Some("identity.local_deletion_conflict")
        );
    }
    let active_deletion = load_active_owner(&path, "alice").unwrap().is_some();
    let active_rollover = crate::internal::identity_registration_retired_join::load(
        &path,
        "join-rollover-after-delete",
    )
    .unwrap()
    .is_some();
    assert_ne!(active_deletion, active_rollover);
}

fn seed_owner_rows(path: &std::path::Path) {
    let connection = crate::internal::local_state::open_writable(path).unwrap();
    connection
        .execute(
            "INSERT INTO messages(msg_id,owner_identity_id,owner_did,thread_id,stored_at) VALUES ('alice-message','alice','did:wba:example.invalid:user:alice:e1_current','dm:bob','now'),('sibling-message','sibling','did:wba:example.invalid:user:sibling:e1_current','dm:alice','now')",
            [],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO identity_did_history(owner_identity_id,did,status,first_seen_at,last_seen_at) VALUES ('alice','did:wba:example.invalid:user:alice:e1_current','current','now','now'),('sibling','did:wba:example.invalid:user:sibling:e1_current','current','now','now')",
            [],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO identity_account_bindings(owner_identity_id,account_id,handle_scope,current_did,device_id,identity_generation,device_auth_generation,created_at,updated_at) VALUES ('alice','account-alice','alice.example.invalid','did:wba:example.invalid:user:alice:e1_current','device-alice','1','1',1,1),('sibling','account-sibling','sibling.example.invalid','did:wba:example.invalid:user:sibling:e1_current','device-sibling','1','1',1,1)",
            [],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO handle_recovery_operations_v4(operation_id,owner_identity_id,full_handle,lifecycle_class,commit_attempted,key_state,vault_key_id,created_at,updated_at) VALUES ('audit-alice','alice','alice.example.invalid','applied',1,'available','vault-audit','now','now')",
            [],
        )
        .unwrap();
}

#[test]
fn credential_only_journal_preserves_business_data_and_binding() {
    let (_root, path) = path();
    seed_owner_rows(&path);
    let record = prepare_with_id(
        &path,
        &snapshot("alice"),
        LocalIdentityDeletionMode::CredentialOnly,
        "delete_credential_12345678",
        "2026-08-29T00:00:00Z",
    )
    .unwrap();

    let advanced = advance_sqlite_phase(&path, &record.deletion_id, false).unwrap();
    assert_eq!(advanced.phase, LocalIdentityDeletionPhase::RetirementReady);
    let connection = crate::internal::local_state::open_writable(&path).unwrap();
    for table in [
        "messages",
        "identity_did_history",
        "identity_account_bindings",
    ] {
        assert_eq!(
            connection
                .query_row(
                    &format!("SELECT COUNT(*) FROM {table} WHERE owner_identity_id='alice'"),
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            1,
            "{table}",
        );
    }
}

#[test]
fn full_data_transaction_deletes_business_history_binding_and_preserves_control() {
    let (_root, path) = path();
    seed_owner_rows(&path);
    let record = prepare_with_id(
        &path,
        &snapshot("alice"),
        LocalIdentityDeletionMode::FullDataCore,
        "delete_full_12345678",
        "2026-08-29T00:00:00Z",
    )
    .unwrap();

    let advanced = advance_sqlite_phase(&path, &record.deletion_id, false).unwrap();
    assert_eq!(advanced.phase, LocalIdentityDeletionPhase::RetirementReady);
    let connection = crate::internal::local_state::open_writable(&path).unwrap();
    for table in [
        "messages",
        "identity_did_history",
        "identity_account_bindings",
    ] {
        assert_eq!(
            connection
                .query_row(
                    &format!("SELECT COUNT(*) FROM {table} WHERE owner_identity_id='alice'"),
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            0,
            "{table}",
        );
        assert_eq!(
            connection
                .query_row(
                    &format!("SELECT COUNT(*) FROM {table} WHERE owner_identity_id='sibling'"),
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            1,
            "sibling {table}",
        );
    }
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM handle_recovery_operations_v4 WHERE operation_id='audit-alice'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        1,
    );
}

#[test]
fn failed_full_data_transaction_keeps_prepared_journal_and_business_rows() {
    let (_root, path) = path();
    seed_owner_rows(&path);
    let record = prepare_with_id(
        &path,
        &snapshot("alice"),
        LocalIdentityDeletionMode::FullDataCore,
        "delete_fail_12345678",
        "2026-08-29T00:00:00Z",
    )
    .unwrap();
    FAIL_AFTER_BUSINESS_DELETE.with(|fail| fail.set(true));

    assert!(advance_sqlite_phase(&path, &record.deletion_id, false).is_err());
    assert_eq!(
        load(&path, &record.deletion_id).unwrap().unwrap().phase,
        LocalIdentityDeletionPhase::Prepared
    );
    let connection = crate::internal::local_state::open_writable(&path).unwrap();
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM messages WHERE owner_identity_id='alice'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        1,
    );
}

#[test]
fn full_data_app_waits_until_explicit_complete() {
    let (_root, path) = path();
    seed_owner_rows(&path);
    let record = prepare_with_id(
        &path,
        &snapshot("alice"),
        LocalIdentityDeletionMode::FullDataApp,
        "delete_app_12345678",
        "2026-08-29T00:00:00Z",
    )
    .unwrap();

    let waiting = advance_sqlite_phase(&path, &record.deletion_id, false).unwrap();
    assert_eq!(waiting.phase, LocalIdentityDeletionPhase::Prepared);
    let advanced = advance_sqlite_phase(&path, &record.deletion_id, true).unwrap();
    assert_eq!(advanced.phase, LocalIdentityDeletionPhase::RetirementReady);
}
