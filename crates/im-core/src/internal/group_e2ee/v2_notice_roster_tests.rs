use super::*;

fn fixture() -> (V2E2eeNotice, Value) {
    let notice = super::tests::test_notice();
    let snapshot = json!({
        "group_did": notice.group_did,
        "group_state_version": notice.group_state_ref.group_state_version,
        "group_policy": {"max_members": 500},
        "member_count": 2,
        "member_list": [
            {"agent_did":"did:example:alice", "status":"active"},
            {"agent_did":"did:example:remote-owner", "status":"active"}
        ]
    });
    (notice, snapshot)
}

#[test]
fn authoritative_roster_includes_foreign_home_members() {
    let (notice, snapshot) = fixture();
    let dids =
        member_dids_from_authoritative_roster("did:example:alice", &notice, &snapshot).unwrap();
    assert!(dids.contains("did:example:remote-owner"));
    assert!(dids.contains(&notice.subject_did));
}

#[test]
fn authoritative_roster_rejects_incomplete_duplicate_or_unbound_snapshots() {
    let (notice, snapshot) = fixture();
    let check = |candidate: Value| {
        assert!(
            member_dids_from_authoritative_roster("did:example:alice", &notice, &candidate)
                .is_err()
        )
    };
    let mut partial = snapshot.clone();
    partial["member_list"].as_array_mut().unwrap().pop();
    check(partial);
    let mut duplicate = snapshot.clone();
    duplicate["member_list"][1] = duplicate["member_list"][0].clone();
    check(duplicate);
    for (field, value) in [
        ("group_did", json!("did:example:other")),
        ("group_state_version", json!("stale")),
        ("member_list", Value::Null),
        ("member_count", json!(1)),
    ] {
        let mut candidate = snapshot.clone();
        candidate[field] = value;
        check(candidate);
    }
    let mut oversized = snapshot.clone();
    oversized["group_policy"]["max_members"] = json!(1);
    check(oversized);
    let mut no_self = snapshot;
    no_self["member_list"][0]["agent_did"] = json!("did:example:other");
    check(no_self);
}

#[test]
fn authoritative_roster_retains_remove_subject_without_counting_inactive_members() {
    let (notice, mut snapshot) = fixture();
    snapshot["member_list"]
        .as_array_mut()
        .unwrap()
        .push(json!({"agent_did":"did:example:departed", "status":"removed"}));
    let mut notice = notice;
    notice.subject_did = "did:example:departed".to_owned();
    let dids =
        member_dids_from_authoritative_roster("did:example:alice", &notice, &snapshot).unwrap();
    assert!(dids.contains("did:example:departed"));
}

#[test]
fn accepted_notice_can_resolve_documents_after_a_newer_membership_change() {
    let (mut notice, mut snapshot) = fixture();
    notice.group_state_ref.group_state_version = "3".into();
    notice.subject_status = "removed".into();
    snapshot["group_state_version"] = json!("4");
    let original_reference = notice.group_state_ref.clone();
    let dids = member_dids_from_authoritative_roster(
        "did:example:alice", &notice, &snapshot,
    ).unwrap();
    assert!(dids.contains(&notice.subject_did));
    assert_eq!(notice.group_state_ref, original_reference);
    snapshot["group_state_version"] = json!("2");
    assert!(matches!(member_dids_from_authoritative_roster(
        "did:example:alice", &notice, &snapshot,
    ), Err(crate::ImError::CursorStale)));
}
