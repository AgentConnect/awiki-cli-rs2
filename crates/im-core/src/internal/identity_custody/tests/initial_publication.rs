use super::*;

#[tokio::test]
async fn fresh_recovery_first_checkpoint_adopts_refreshed_proof_and_reopens_exactly() {
    let root = tempfile::tempdir().unwrap();
    let core = crate::ImCore::new(test_config(), test_paths(root.path())).unwrap();
    let mut identity =
        provision_handle_recovery_identity(&core, "example.test", "initial").unwrap();
    let original = identity.did_document.clone();
    let refreshed = refresh_fresh_handle_recovery_document_async(&core, &identity)
        .await
        .unwrap();
    assert_ne!(original, refreshed);
    assert_eq!(
        document_without_proof(&original).unwrap(),
        document_without_proof(&refreshed).unwrap()
    );
    let checkpoint = crate::internal::identity_device_state::IdentityInternalCheckpoint {
        document_version: 1,
        registry_version: 1,
        document_hash: crate::internal::identity_wire::document::document_hash(&refreshed).unwrap(),
    };
    assert!(matches!(
        adopt_controller_document_async(
            &core,
            ControllerDocumentAdoption::DeviceJoin {
                pending: DeviceJoinPendingDocumentChange::ExactOperation("join-first-checkpoint")
            },
            &identity.did,
            &identity.store_id,
            &identity.identity_id,
            &refreshed,
            &checkpoint,
        )
        .await,
        Err(crate::ImError::PermissionDenied)
    ));
    adopt_controller_document_async(
        &core,
        ControllerDocumentAdoption::HandleRecovery {
            pending_operation_id: "recovery-first-checkpoint",
        },
        &identity.did,
        &identity.store_id,
        &identity.identity_id,
        &refreshed,
        &checkpoint,
    )
    .await
    .unwrap();
    drop(core);
    let core = crate::ImCore::new(test_config(), test_paths(root.path())).unwrap();
    // Simulate the crash cut after provider confirmation, before the Core marker advances.
    adopt_controller_document_async(
        &core,
        ControllerDocumentAdoption::HandleRecovery {
            pending_operation_id: "recovery-first-checkpoint",
        },
        &identity.did,
        &identity.store_id,
        &identity.identity_id,
        &refreshed,
        &checkpoint,
    )
    .await
    .unwrap();
    identity.did_document = refreshed.clone();
    let session = handle_recovery_identity_async(&core, &identity)
        .await
        .unwrap();
    assert_eq!(session.public_identity().await.unwrap().document, refreshed);
    let another = refresh_fresh_handle_recovery_document_async(&core, &identity)
        .await
        .unwrap();
    let mut another_checkpoint = checkpoint;
    another_checkpoint.document_hash =
        crate::internal::identity_wire::document::document_hash(&another).unwrap();
    assert!(adopt_controller_document_async(
        &core,
        ControllerDocumentAdoption::HandleRecovery {
            pending_operation_id: "recovery-first-checkpoint"
        },
        &identity.did,
        &identity.store_id,
        &identity.identity_id,
        &another,
        &another_checkpoint,
    )
    .await
    .is_err());
    assert_eq!(session.public_identity().await.unwrap().document, refreshed);
}

#[tokio::test]
async fn recovery_identical_first_checkpoint_is_confirmed_not_only_short_circuited() {
    let root = tempfile::tempdir().unwrap();
    let core = crate::ImCore::new(test_config(), test_paths(root.path())).unwrap();
    let identity = provision_handle_recovery_identity(&core, "example.test", "unchanged").unwrap();
    let checkpoint = crate::internal::identity_device_state::IdentityInternalCheckpoint {
        document_version: 1,
        registry_version: 1,
        document_hash: crate::internal::identity_wire::document::document_hash(
            &identity.did_document,
        )
        .unwrap(),
    };
    adopt_controller_document_async(
        &core,
        ControllerDocumentAdoption::HandleRecovery {
            pending_operation_id: "recovery-unchanged-first",
        },
        &identity.did,
        &identity.store_id,
        &identity.identity_id,
        &identity.did_document,
        &checkpoint,
    )
    .await
    .unwrap();
    let refreshed = refresh_fresh_handle_recovery_document_async(&core, &identity)
        .await
        .unwrap();
    let mut changed = checkpoint;
    changed.document_hash =
        crate::internal::identity_wire::document::document_hash(&refreshed).unwrap();
    assert!(adopt_controller_document_async(
        &core,
        ControllerDocumentAdoption::HandleRecovery {
            pending_operation_id: "recovery-unchanged-first"
        },
        &identity.did,
        &identity.store_id,
        &identity.identity_id,
        &refreshed,
        &changed,
    )
    .await
    .is_err());
}
