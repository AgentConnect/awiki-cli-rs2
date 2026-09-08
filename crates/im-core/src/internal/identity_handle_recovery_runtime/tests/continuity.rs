use super::*;

#[tokio::test]
async fn handle_lookup_finds_unprojected_owner_and_registration_is_fenced() {
    let root = tempfile::tempdir().unwrap();
    let core = recovery_test_core(root.path(), "https://awiki.test", [81; 32]);
    let (pending, _) =
        create_v4_awaiting_factor_operation(&core, "recover-v4-continuity", "unprojected-owner");
    let selector = crate::identity::IdentitySelector::Handle(
        crate::ids::Handle::parse(&pending.full_handle, "").unwrap(),
    );
    let operations = list_operations(&core, selector.clone()).await.unwrap();
    assert_eq!(operations.len(), 1);
    assert_eq!(
        operations[0].owner_identity_id.as_str(),
        "unprojected-owner"
    );
    assert_eq!(operations[0].operation_id, pending.operation_id);
    let request = crate::identity::RegisterHandleRequest {
        local_alias: None,
        requested_handle: crate::ids::Handle::parse(&pending.full_handle, "").unwrap(),
        verification: crate::identity::VerificationInput::AlreadyVerified,
        invite_code: None,
        profile: crate::identity::InitialProfile {
            display_name: None,
            avatar_url: None,
        },
        make_default: true,
    };
    for phase in [
        "pre_commit",
        "remote_unresolved",
        "remote_committed",
        "local_transition_pending",
    ] {
        crate::internal::local_state::open_writable(
            &core.inner().sdk_paths().local_state.sqlite_path,
        )
        .unwrap()
        .execute(
            "UPDATE handle_recovery_operations_v4 SET lifecycle_class=?1 WHERE operation_id=?2",
            rusqlite::params![phase, pending.operation_id],
        )
        .unwrap();
        let error = core
            .identities()
            .register_handle_async(request.clone())
            .await
            .unwrap_err();
        assert!(
            matches!(error, crate::ImError::Service { code: Some(ref code), .. } if code == "handle_recovery.resume_required"),
            "{error:?}"
        );
    }
    // No registration candidate or HTTP request was needed to reject this path.
    assert!(
        crate::internal::identity_registration_pending::PendingRegistrationStore::from_core(&core)
            .unwrap()
            .load("alice", "awiki.test")
            .unwrap()
            .is_none()
    );
    let other = list_operations(
        &core,
        crate::identity::IdentitySelector::Handle(
            crate::ids::Handle::parse("bob.awiki.test", "").unwrap(),
        ),
    )
    .await
    .unwrap();
    assert!(other.is_empty());
    assert!(list_operations(
        &core,
        crate::identity::IdentitySelector::Handle(
            crate::ids::Handle::parse("alice.other.test", "").unwrap()
        )
    )
    .await
    .is_err());
    drop(core);
    let reopened = recovery_test_core(root.path(), "https://awiki.test", [81; 32]);
    let operations = list_operations(&reopened, selector).await.unwrap();
    assert_eq!(operations[0].operation_id, pending.operation_id);
}
