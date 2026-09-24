use super::tests::test_paths;
use super::*;

fn two_handle_index() -> (
    IndexPayload,
    [crate::internal::identity_transition_pending::IdentityTransitionMarker; 2],
) {
    let mut index = duplicate_recovery_index();
    let alice = switched_recovery_marker();
    let mut bob = alice.clone();
    bob.recovery_id = "recovery-bob".to_owned();
    bob.source_id = bob.recovery_id.clone();
    bob.handle = "bob.example.com".to_owned();
    bob.account_user_id = "account-bob".to_owned();
    bob.owner_identity_id = "bob-new-owner".to_owned();
    bob.previous_did = "did:example:bob-old".to_owned();
    bob.current_did = "did:example:bob-new".to_owned();
    for (alias, mut entry) in duplicate_recovery_index().credentials {
        entry.credential_name = format!("bob-{alias}");
        entry.unique_id = format!("bob-{}", entry.unique_id);
        entry.dir_name = entry.unique_id.clone();
        entry.did = format!("did:example:bob-{alias}");
        entry.user_id = bob.account_user_id.clone();
        entry.handle = "bob".to_owned();
        entry.full_handle = bob.handle.clone();
        index
            .credentials
            .insert(entry.credential_name.clone(), entry);
    }
    (normalize_index_payload(index).unwrap(), [alice, bob])
}

#[test]
fn independent_recoveries_reduce_duplicates_in_either_order_after_restart() {
    for reverse in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let paths = test_paths(root.path());
        let (index, mut markers) = two_handle_index();
        if reverse {
            markers.reverse();
        }
        fs::create_dir_all(&paths.identity_root_dir).unwrap();
        let before = serde_json::to_vec_pretty(&index).unwrap();
        fs::write(&paths.registry_path, &before).unwrap();
        let store = IdentityStore::new(&paths);
        store.write_default_identity("old").unwrap();
        // Simulate both operations having backed up before either committed.
        for marker in &markers {
            write_secure_bytes_atomic(
                &paths
                    .identity_root_dir
                    .join(".recovery-registry-backups")
                    .join(format!(
                        "{:x}.json",
                        Sha256::digest(marker.recovery_id.as_bytes())
                    )),
                &before,
            )
            .unwrap();
        }
        assert!(store
            .retire_duplicate_recovery_predecessor(&markers[0])
            .unwrap());
        let partial = store.load_index().unwrap();
        assert_eq!(partial.credentials.len(), 3);
        // The unselected Handle is untouched; ordinary writes still reject it.
        for (alias, entry) in &index.credentials {
            if entry.full_handle == markers[1].handle {
                assert_eq!(
                    serde_json::to_value(entry).unwrap(),
                    serde_json::to_value(partial.credentials.get(alias).unwrap()).unwrap()
                );
            }
        }
        let partial_raw = fs::read(&paths.registry_path).unwrap();
        let mut wrong = markers[1].clone();
        wrong.account_user_id = "unproven-account".to_owned();
        assert!(store.retire_duplicate_recovery_predecessor(&wrong).is_err());
        assert_eq!(fs::read(&paths.registry_path).unwrap(), partial_raw);
        {
            let lock = store.lock_index_mutation().unwrap();
            assert!(matches!(
                store.save_index_locked(&lock, partial),
                Err(crate::ImError::IdentityBindingConflict { .. })
            ));
        }
        assert_eq!(fs::read(&paths.registry_path).unwrap(), partial_raw);
        let reopened = IdentityStore::new(&paths);
        assert!(reopened
            .retire_duplicate_recovery_predecessor(&markers[1])
            .unwrap());
        assert!(!reopened
            .retire_duplicate_recovery_predecessor(&markers[0])
            .unwrap());
        let final_index = reopened.load_index().unwrap();
        assert_eq!(final_index.credentials.len(), 2);
        assert!(final_index.credentials.contains_key("new"));
        assert!(final_index.credentials.contains_key("bob-new"));
        assert_eq!(final_index.default_credential_name, "new");
        assert_eq!(
            fs::read_to_string(paths.default_identity_path.as_ref().unwrap()).unwrap(),
            "new\n"
        );
        for marker in &markers {
            assert_eq!(
                fs::read(
                    paths
                        .identity_root_dir
                        .join(".recovery-registry-backups")
                        .join(format!(
                            "{:x}.json",
                            Sha256::digest(marker.recovery_id.as_bytes())
                        ))
                )
                .unwrap(),
                before
            );
        }
    }
}

#[test]
fn failed_recovery_index_commit_restores_present_or_absent_default() {
    for has_default in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let paths = test_paths(root.path());
        let store = IdentityStore::new(&paths);
        let before = serde_json::to_vec_pretty(&duplicate_recovery_index()).unwrap();
        fs::create_dir_all(&paths.identity_root_dir).unwrap();
        fs::write(&paths.registry_path, &before).unwrap();
        if has_default {
            store.write_default_identity("old").unwrap();
        }
        let mut next = duplicate_recovery_index();
        next.credentials.remove("old");
        next.default_credential_name = "new".to_owned();
        {
            let lock = store.lock_index_mutation().unwrap();
            let result = store.commit_recovery_index(&lock, &next, true, || {
                assert_eq!(
                    fs::read_to_string(paths.default_identity_path.as_ref().unwrap()).unwrap(),
                    "new\n"
                );
                Err(std::io::Error::other("injected index write failure").into())
            });
            assert!(result.is_err());
        }
        assert_eq!(fs::read(&paths.registry_path).unwrap(), before);
        let default = paths.default_identity_path.as_ref().unwrap();
        if has_default {
            assert_eq!(fs::read_to_string(default).unwrap(), "old\n");
        } else {
            assert!(!default.exists());
        }
        assert!(store
            .retire_duplicate_recovery_predecessor(&switched_recovery_marker())
            .unwrap());
    }
}

#[test]
fn crash_after_default_write_keeps_exact_pair_resumable() {
    let root = tempfile::tempdir().unwrap();
    let paths = test_paths(root.path());
    let store = IdentityStore::new(&paths);
    let before = serde_json::to_vec_pretty(&duplicate_recovery_index()).unwrap();
    fs::create_dir_all(&paths.identity_root_dir).unwrap();
    fs::write(&paths.registry_path, before).unwrap();
    // Durable crash cut: default written, duplicate index not yet committed.
    store.write_default_identity("new").unwrap();
    let reopened = IdentityStore::new(&paths);
    assert!(recovery_duplicate_projection(
        &reopened.load_index().unwrap(),
        &switched_recovery_marker()
    )
    .is_some());
    assert!(reopened
        .retire_duplicate_recovery_predecessor(&switched_recovery_marker())
        .unwrap());
    assert_eq!(
        reopened.load_index().unwrap().default_credential_name,
        "new"
    );
    assert_eq!(
        fs::read_to_string(paths.default_identity_path.as_ref().unwrap()).unwrap(),
        "new\n"
    );
}

#[test]
fn default_write_failure_does_not_commit_the_index_and_can_retry() {
    let root = tempfile::tempdir().unwrap();
    let paths = test_paths(root.path());
    let store = IdentityStore::new(&paths);
    let before = serde_json::to_vec_pretty(&duplicate_recovery_index()).unwrap();
    fs::create_dir_all(&paths.identity_root_dir).unwrap();
    fs::write(&paths.registry_path, &before).unwrap();
    let default = paths.default_identity_path.as_ref().unwrap();
    fs::create_dir(default).unwrap();
    assert!(store
        .retire_duplicate_recovery_predecessor(&switched_recovery_marker())
        .is_err());
    assert_eq!(fs::read(&paths.registry_path).unwrap(), before);
    fs::remove_dir(default).unwrap();
    assert!(store
        .retire_duplicate_recovery_predecessor(&switched_recovery_marker())
        .unwrap());
    assert_eq!(fs::read_to_string(default).unwrap(), "new\n");
}

fn switched_recovery_marker(
) -> crate::internal::identity_transition_pending::IdentityTransitionMarker {
    use crate::internal::identity_transition_pending::{
        IdentityTransitionMarker, TransitionPhase, TransitionSourceKind,
    };
    IdentityTransitionMarker {
        schema_version: 1,
        contract_version: crate::internal::identity_handle_recovery_pending::V4_CONTRACT_VERSION
            .to_owned(),
        contract_hash: crate::internal::identity_handle_recovery_pending::V4_CONTRACT_HASH
            .to_owned(),
        recovery_id: "recovery-exact-duplicate".to_owned(),
        source_kind: TransitionSourceKind::Initiator,
        source_id: "recovery-exact-duplicate".to_owned(),
        state_root_fingerprint: "test-root".to_owned(),
        account_user_id: "account-alice".to_owned(),
        owner_identity_id: "new-owner".to_owned(),
        handle: "alice.example.com".to_owned(),
        previous_did: "did:example:old".to_owned(),
        current_did: "did:example:new".to_owned(),
        binding_generation: "4".to_owned(),
        current_device_id: None,
        device_auth_generation: None,
        registry_version: None,
        applied_at: None,
        metadata_json: "{}".to_owned(),
        phase: TransitionPhase::IdentitySwitched,
        created_at: "2026-09-24T00:00:00Z".to_owned(),
        updated_at: "2026-09-24T00:00:00Z".to_owned(),
    }
}

fn duplicate_recovery_index() -> IndexPayload {
    let mut index = IndexPayload {
        default_credential_name: "old".to_owned(),
        ..IndexPayload::default()
    };
    for (alias, owner, did, generation) in [
        ("old", "old-owner", "did:example:old", "3"),
        ("new", "new-owner", "did:example:new", "4"),
    ] {
        index.credentials.insert(
            alias.to_owned(),
            IndexEntry {
                credential_name: alias.to_owned(),
                dir_name: owner.to_owned(),
                unique_id: owner.to_owned(),
                did: did.to_owned(),
                user_id: "account-alice".to_owned(),
                handle: "alice".to_owned(),
                full_handle: "alice.example.com".to_owned(),
                binding_generation: Some(generation.to_owned()),
                identity_custody_backend: Some("anp_identity".to_owned()),
                ..IndexEntry::default()
            },
        );
    }
    index
}

#[test]
fn recovery_duplicate_classifier_requires_the_exact_switched_tuple() {
    let index = duplicate_recovery_index();
    let marker = switched_recovery_marker();
    assert_eq!(
        recovery_duplicate_projection(&index, &marker),
        Some(RecoveryDuplicateProjection {
            predecessor_alias: "old".to_owned(),
            successor_alias: "new".to_owned(),
        })
    );
    let mut wrong = marker.clone();
    wrong.previous_did = "did:example:other".to_owned();
    assert_eq!(recovery_duplicate_projection(&index, &wrong), None);
    wrong = marker;
    wrong.phase = crate::internal::identity_transition_pending::TransitionPhase::Pending;
    assert_eq!(recovery_duplicate_projection(&index, &wrong), None);
}

#[test]
fn recovery_duplicate_retirement_backs_up_index_and_preserves_old_files() {
    let root = tempfile::tempdir().unwrap();
    let paths = test_paths(root.path());
    let store = IdentityStore::new(&paths);
    fs::create_dir_all(paths.identity_root_dir.join("old-owner")).unwrap();
    let old_file = paths
        .identity_root_dir
        .join("old-owner")
        .join("identity.json");
    fs::write(&old_file, b"historical identity").unwrap();
    let before = serde_json::to_vec_pretty(&duplicate_recovery_index()).unwrap();
    fs::write(&paths.registry_path, &before).unwrap();
    store.write_default_identity("old").unwrap();

    let marker = switched_recovery_marker();
    assert!(store
        .retire_duplicate_recovery_predecessor(&marker)
        .unwrap());
    let after = store.load_index().unwrap();
    assert_eq!(after.credentials.len(), 1);
    assert!(after.credentials.contains_key("new"));
    assert_eq!(after.default_credential_name, "new");
    assert_eq!(
        fs::read_to_string(paths.default_identity_path.as_ref().unwrap()).unwrap(),
        "new\n"
    );
    assert_eq!(fs::read(&old_file).unwrap(), b"historical identity");
    let backup = paths
        .identity_root_dir
        .join(".recovery-registry-backups")
        .join(format!(
            "{:x}.json",
            Sha256::digest(marker.recovery_id.as_bytes())
        ));
    assert_eq!(fs::read(backup).unwrap(), before);
    assert!(!store
        .retire_duplicate_recovery_predecessor(&marker)
        .unwrap());
}

#[test]
fn recovery_duplicate_retirement_rejects_mismatched_authority_without_changes() {
    let root = tempfile::tempdir().unwrap();
    let paths = test_paths(root.path());
    let store = IdentityStore::new(&paths);
    fs::create_dir_all(&paths.identity_root_dir).unwrap();
    let before = serde_json::to_vec_pretty(&duplicate_recovery_index()).unwrap();
    fs::write(&paths.registry_path, &before).unwrap();
    let mut marker = switched_recovery_marker();
    marker.account_user_id = "other-account".to_owned();
    assert!(matches!(
        store.retire_duplicate_recovery_predecessor(&marker),
        Err(crate::ImError::IdentityBindingConflict { .. })
    ));
    assert_eq!(fs::read(&paths.registry_path).unwrap(), before);
    assert!(!paths
        .identity_root_dir
        .join(".recovery-registry-backups")
        .exists());
}

#[test]
fn recovery_duplicate_retirement_replays_a_completed_backup_before_index_commit() {
    let root = tempfile::tempdir().unwrap();
    let paths = test_paths(root.path());
    let store = IdentityStore::new(&paths);
    fs::create_dir_all(&paths.identity_root_dir).unwrap();
    let before = serde_json::to_vec_pretty(&duplicate_recovery_index()).unwrap();
    fs::write(&paths.registry_path, &before).unwrap();
    let marker = switched_recovery_marker();
    let backup = paths
        .identity_root_dir
        .join(".recovery-registry-backups")
        .join(format!(
            "{:x}.json",
            Sha256::digest(marker.recovery_id.as_bytes())
        ));
    write_secure_bytes_atomic(&backup, &before).unwrap();
    assert!(store
        .retire_duplicate_recovery_predecessor(&marker)
        .unwrap());
    assert_eq!(fs::read(&backup).unwrap(), before);
    assert_eq!(store.load_index().unwrap().credentials.len(), 1);

    // A different preimage under the same operation ID cannot authorize
    // removal after a local replacement or copied backup.
    let mut changed = duplicate_recovery_index();
    changed.credentials.get_mut("new").unwrap().anp_identity_id =
        Some("replaced-custody".to_owned());
    let changed_raw = serde_json::to_vec_pretty(&changed).unwrap();
    fs::write(&paths.registry_path, &changed_raw).unwrap();
    assert!(matches!(
        store.retire_duplicate_recovery_predecessor(&marker),
        Err(crate::ImError::IdentityBindingConflict { .. })
    ));
    assert_eq!(fs::read(&paths.registry_path).unwrap(), changed_raw);
}

#[cfg(unix)]
#[test]
fn recovery_duplicate_retirement_rejects_redirected_backup_directory() {
    let root = tempfile::tempdir().unwrap();
    let paths = test_paths(root.path());
    let store = IdentityStore::new(&paths);
    fs::create_dir_all(&paths.identity_root_dir).unwrap();
    let before = serde_json::to_vec_pretty(&duplicate_recovery_index()).unwrap();
    fs::write(&paths.registry_path, &before).unwrap();
    let outside = root.path().join("outside");
    fs::create_dir(&outside).unwrap();
    std::os::unix::fs::symlink(
        &outside,
        paths.identity_root_dir.join(".recovery-registry-backups"),
    )
    .unwrap();
    assert!(matches!(
        store.retire_duplicate_recovery_predecessor(&switched_recovery_marker()),
        Err(crate::ImError::PermissionDenied)
    ));
    assert_eq!(fs::read(&paths.registry_path).unwrap(), before);
    assert_eq!(fs::read_dir(&outside).unwrap().count(), 0);
}
