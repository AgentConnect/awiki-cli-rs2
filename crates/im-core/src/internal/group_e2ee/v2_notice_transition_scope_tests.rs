use super::*;

fn context() -> (V2GroupNoticeMetadata, V2E2eeNotice, Value, (String, String)) {
    let notice = super::tests::test_notice();
    let meta: V2GroupNoticeMetadata = serde_json::from_value(json!({
        "profile":anp::group_e2ee::GROUP_E2EE_PROFILE_V2,
        "security_profile":anp::group_e2ee::GROUP_E2EE_TRANSPORT_PROFILE_V2,
        "sender_did":notice.group_did,
        "target":{"kind":"agent","did":"did:example:previous"},
        "recipient_device_id":"device-local", "operation_id":"operation"
    }))
    .unwrap();
    let snapshot = json!({
        "group_did":notice.group_did,"group_state_version":notice.group_state_ref.group_state_version,
        "member_count":1,"member_list":[{"agent_did":"did:example:current","status":"active","role":"owner"}]
    });
    (
        meta,
        notice,
        snapshot,
        ("did:example:previous".into(), "did:example:current".into()),
    )
}

#[test]
fn transition_notice_scope_authorizes_current_p4_member_and_resolves_old_credential() {
    let (meta, notice, snapshot, transition) = context();
    let dids = notice_document_dids(
        "did:example:current",
        "did:example:previous",
        &meta,
        &notice,
        Some(&transition),
        &snapshot,
    )
    .unwrap();
    assert!(dids.contains("did:example:current"));
    assert!(dids.contains("did:example:previous"));
    assert_eq!(snapshot["member_count"], 1);
    assert!(
        member_dids_from_authoritative_roster("did:example:previous", &notice, &snapshot).is_err()
    );
}

#[test]
fn transition_notice_scope_old_credential_does_not_grant_p4_membership() {
    let (meta, notice, snapshot, transition) = context();
    assert!(notice_document_dids(
        "did:example:current",
        "did:example:previous",
        &meta,
        &notice,
        None,
        &snapshot
    )
    .is_err());
    let other = ("did:example:other".into(), "did:example:current".into());
    assert!(notice_document_dids(
        "did:example:current",
        "did:example:previous",
        &meta,
        &notice,
        Some(&other),
        &snapshot
    )
    .is_err());
    let mut removed = snapshot.clone();
    removed["member_list"][0]["agent_did"] = json!("did:example:previous");
    assert!(notice_document_dids(
        "did:example:current",
        "did:example:previous",
        &meta,
        &notice,
        Some(&transition),
        &removed
    )
    .is_err());
}
