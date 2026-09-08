use super::*;

#[test]
fn pending_discovery_includes_all_unfinished_lifecycles_and_excludes_history() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("im.sqlite");
    let lifecycles = [
        RecoveryLifecycleClass::PreCommit,
        RecoveryLifecycleClass::RemoteUnresolved,
        RecoveryLifecycleClass::RemoteCommitted,
        RecoveryLifecycleClass::LocalTransitionPending,
        RecoveryLifecycleClass::QuarantinedKeyUnavailable,
        RecoveryLifecycleClass::Applied,
        RecoveryLifecycleClass::DiscardedPreAttempt,
        RecoveryLifecycleClass::SupersededByStateChange,
        RecoveryLifecycleClass::FailedTerminal,
    ];
    for (index, lifecycle) in lifecycles.into_iter().enumerate() {
        let mut value = RecoveryOperationRecord::pre_commit(
            format!("op_discovery_{index:08}"),
            format!("fresh-owner-{index}"),
            "alice.example.invalid".to_owned(),
            format!("secret-vault-{index}"),
            "2026-09-08T00:00:00Z".to_owned(),
        )
        .unwrap();
        value.lifecycle_class = lifecycle;
        insert(&path, &value).unwrap();
    }
    let actual = list_pending(&path).unwrap();
    assert_eq!(actual.len(), 5);
    assert_eq!(
        actual
            .iter()
            .map(|v| v.operation_id.as_str())
            .collect::<Vec<_>>(),
        [
            "op_discovery_00000004",
            "op_discovery_00000003",
            "op_discovery_00000002",
            "op_discovery_00000001",
            "op_discovery_00000000"
        ]
    );
    // Discovery neither publishes fresh owners nor advances/deletes their journals.
    assert_eq!(list_pending(&path).unwrap().len(), actual.len());
    for index in 0..9 {
        assert!(load(&path, &format!("op_discovery_{index:08}"))
            .unwrap()
            .is_some());
    }
    assert!(list_pending(&root.path().join("another-scope.sqlite"))
        .unwrap()
        .is_empty());
}
