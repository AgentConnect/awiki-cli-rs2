#[test]
fn retired_recovery_handles_remote_epoch_changes_without_adopting_history() {
    for case in [
        "completed",
        "missing",
        "wrong_device",
        "future",
        "same_did_stale_generation",
        "wrong_account",
        "wrong_handle",
        "live",
        "live_owner_other_scope",
        "pending_other_owner",
        "unfinished_transition",
        "unfinished_deletion",
        "mixed",
        "multiple_completed",
    ] {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("im.sqlite");
        let identities = root.path().join("identities");
        let current = "did:wba:example.invalid:user:alice:current";
        let old = if case == "same_did_stale_generation" {
            current
        } else {
            "did:wba:example.invalid:user:alice:old"
        };
        insert_binding(
            &path,
            "owner-alice",
            if case == "wrong_account" {
                "account-other"
            } else {
                "account-alice"
            },
            if case == "wrong_handle" {
                "other.example.invalid"
            } else {
                "alice.example.invalid"
            },
            old,
            if case == "future" { "9" } else { "5" },
        );
        if case != "missing" {
            write_completed_retirement(
                &identities,
                "owner-alice",
                old,
                if case == "wrong_device" {
                    "device-other"
                } else {
                    "device-owner-alice"
                },
            );
        }
        if matches!(case, "mixed" | "multiple_completed") {
            let older = "did:wba:example.invalid:user:alice:older";
            insert_binding(
                &path,
                "owner-older",
                "account-alice",
                "alice.example.invalid",
                older,
                "4",
            );
            if case == "multiple_completed" {
                write_completed_retirement(&identities, "owner-older", older, "device-owner-older");
            }
        }
        if case == "pending_other_owner" {
            let operation = crate::internal::identity_handle_recovery_operation::RecoveryOperationRecord::pre_commit(
                "recover-v4-competing".into(), "owner-other".into(), "alice.example.invalid".into(), "pending-key".into(), "2026-10-29T00:00:00Z".into()).unwrap();
            crate::internal::identity_handle_recovery_operation::insert(&path, &operation).unwrap();
        }
        if case == "unfinished_transition" {
            insert_completed_recovery(&path, "owner-alice", old, current, "7");
            crate::internal::local_state::open_writable(&path)
                .unwrap()
                .execute("UPDATE identity_transition_pending SET phase='pending'", [])
                .unwrap();
        }
        if case == "unfinished_deletion" {
            crate::internal::identity_local_deletion::prepare(
                &path,
                &crate::internal::identity_local_deletion::LocalIdentityDeletionSnapshot {
                    owner_identity_id: "owner-alice".into(),
                    current_did: old.into(),
                    full_handle: Some("alice.example.invalid".into()),
                    local_alias: "alice".into(),
                    identity_dir_name: None,
                    next_default_alias: None,
                    protocol_device_id: Some("device-owner-alice".into()),
                },
                crate::internal::identity_local_deletion::LocalIdentityDeletionMode::CredentialOnly,
            )
            .unwrap();
        }
        let mut index = if matches!(case, "live" | "live_owner_other_scope") {
            fixture_index()
        } else {
            Default::default()
        };
        if case == "live_owner_other_scope" {
            let entry = index.credentials.get_mut("alice").unwrap();
            entry.user_id = "other-account".into();
            entry.full_handle = "other.example.invalid".into();
            entry.did = "did:wba:example.invalid:user:other:live".into();
        }
        let authority = StableOwnerAuthority {
            account_user_id: "account-alice",
            full_handle: "alice.example.invalid",
            previous_did: current,
            binding_generation: "8",
        };
        let expected = if matches!(case, "completed" | "multiple_completed") {
            StableOwnerMatch::None
        } else {
            StableOwnerMatch::Conflict
        };
        assert_eq!(
            match_recovery_owner(
                &path,
                &identities,
                &index,
                authority.clone(),
                "recover-v4-current"
            )
            .unwrap(),
            expected,
            "{case}"
        );
        // Ordinary Join must never inherit Recovery's fresh-owner exception.
        assert_eq!(
            match_stable_owner(&path, &index, authority, None, None).unwrap(),
            StableOwnerMatch::Conflict,
            "{case}"
        );
        let count: i64 = crate::internal::local_state::open_writable(&path)
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM identity_account_bindings",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            count,
            if matches!(case, "mixed" | "multiple_completed") {
                2
            } else {
                1
            }
        );
    }
}
