use super::*;
use serde_json::json;

fn snapshot() -> Value {
    json!({
        "group_did": "did:example:group",
        "group_state_version": "7",
        "group_policy": {"message_security_profile": "group-e2ee"},
        "group": {"group_did": "did:example:group", "membership_status": "active", "my_role": "member"}
    })
}

fn state_ref(value: Value) -> crate::ImResult<anp::group_e2ee::V2GroupStateRef> {
    group_state_ref_from_send_snapshot(
        "did:example:group",
        &crate::groups::GroupReadResult::from_raw_response(value, Vec::new()),
    )
}

#[test]
fn group_send_snapshot_preserves_the_verified_state_version() {
    let state = state_ref(snapshot()).expect("complete verified snapshot");
    assert_eq!(state.group_state_version, "7");
    assert_eq!(state.group_did, "did:example:group");
}

#[test]
fn group_send_snapshot_cannot_bypass_membership_policy_or_maintenance() {
    for (path, replacement) in [
        ("/group/membership_status", json!("left")),
        ("/group/membership_status", Value::Null),
        ("/group/group_did", json!("did:example:other")),
        ("/group_state_version", Value::Null),
        ("/group_policy/message_security_profile", json!("transport-protected")),
    ] {
        let mut value = snapshot();
        *value.pointer_mut(path).unwrap() = replacement;
        assert!(state_ref(value).is_err(), "{path}");
    }
    let mut paused = snapshot();
    paused["e2ee_maintenance"] = json!({"reason":"group_membership_change_pending","send_paused":true});
    assert!(state_ref(paused).is_err());
}
