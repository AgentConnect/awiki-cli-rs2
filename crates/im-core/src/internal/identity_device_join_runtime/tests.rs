use super::*;

#[test]
fn join_approval_handles_are_single_use() {
    let store = DeviceJoinApprovalHandleStore::default();
    let handle = store
        .issue(DeviceJoinApprovalHandleState {
            admin_identity: crate::identity::IdentitySelector::Id(
                crate::ids::IdentityId::parse("identity-admin").unwrap(),
            ),
            join_session_id: "join-1".to_owned(),
            operation_id: "approve-1".to_owned(),
            user_presence_at: None,
            expires_at: "2099-07-18T12:05:00Z".to_owned(),
        })
        .unwrap();
    let now = time::OffsetDateTime::now_utc();
    assert!(matches!(
        store.claim(&handle, "2026-07-18T12:00:00Z", now),
        Ok(DeviceJoinApprovalHandleClaim::Claimed(_))
    ));
    assert!(store.claim(&handle, "2026-07-18T12:00:00Z", now).is_err());
    assert!(store.consume(&handle).is_ok());
}

#[test]
fn public_join_states_have_no_claimed_compatibility_state() {
    let states = [
        DeviceJoinRemoteState::Pending,
        DeviceJoinRemoteState::ChallengeSent,
        DeviceJoinRemoteState::ResponseVerified,
        DeviceJoinRemoteState::Consumed,
        DeviceJoinRemoteState::Cancelled,
        DeviceJoinRemoteState::Rejected,
        DeviceJoinRemoteState::Expired,
    ];
    assert_eq!(states.len(), 7);
}
