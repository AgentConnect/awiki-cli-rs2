use super::{overlay_group_maintenance_status, step4_tests::local_status};
use serde_json::json;

#[test]
fn group_status_fails_closed_for_unknown_current_membership() {
    let snapshot = crate::groups::GroupReadResult::from_raw_response(
        json!({"group_did":"did:example:group"}),
        Vec::new(),
    );
    assert!(overlay_group_maintenance_status(local_status(true, true), &snapshot).is_err());
}

#[test]
fn group_status_rejects_left_member_despite_active_local_mls_state() {
    let snapshot = crate::groups::GroupReadResult::from_raw_response(
        json!({
            "group_did":"did:example:group",
            "group":{"group_did":"did:example:group","membership_status":"left","my_role":"member"}
        }),
        Vec::new(),
    );
    let result = overlay_group_maintenance_status(local_status(true, true), &snapshot).unwrap();
    assert!(!result.can_send_secure);
    assert!(!result.local_readiness.has_active_membership);
}
