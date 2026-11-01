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

fn did_transition_snapshot() -> serde_json::Value {
    json!({
        "group_did": "did:example:group",
        "group_state_version": "2",
        "group": {
            "group_did": "did:example:group",
            "membership_status": "active", "my_role": "owner"
        },
        "e2ee_maintenance": {
            "reason": "group_membership_change_pending", "send_paused": true,
            "did_transition": {
                "update_id": "fixture-update", "group_did": "did:example:group",
                "group_state_version": "2", "previous_did": "did:example:old",
                "current_did": "did:example:new", "phase": "add_pending",
                "eligible_device_ids": ["device-new"]
            }
        }
    })
}

#[test]
fn group_status_did_transition_keeps_valid_bound_plan_paused_and_repairable() {
    for phase in [
        "add_pending",
        "adds_confirmed",
        "remove_pending",
        "action_required",
    ] {
        let mut raw = did_transition_snapshot();
        raw["e2ee_maintenance"]["did_transition"]["phase"] = json!(phase);
        let snapshot = crate::groups::GroupReadResult::from_raw_response(raw, Vec::new());
        let result = overlay_group_maintenance_status(local_status(true, true), &snapshot)
            .expect("a current Host transition plan is a supported maintenance projection");
        assert!(!result.can_send_secure);
        assert!(result.local_readiness.has_local_state);
        assert!(result.local_readiness.has_active_membership);
        assert_eq!(result.state, crate::secure::GroupSecureState::NeedsRepair);
    }
}

#[test]
fn group_status_did_transition_rejects_unbound_or_unknown_plan_fields() {
    for (pointer, value) in [
        (
            "/e2ee_maintenance/did_transition/group_did",
            json!("did:example:other"),
        ),
        (
            "/e2ee_maintenance/did_transition/group_state_version",
            json!("1"),
        ),
        ("/e2ee_maintenance/did_transition/phase", json!("unknown")),
        (
            "/e2ee_maintenance/did_transition/eligible_device_ids",
            json!([]),
        ),
    ] {
        let mut raw = did_transition_snapshot();
        *raw.pointer_mut(pointer).unwrap() = value;
        let snapshot = crate::groups::GroupReadResult::from_raw_response(raw, Vec::new());
        assert!(overlay_group_maintenance_status(local_status(true, true), &snapshot).is_err());
    }
    let mut raw = did_transition_snapshot();
    raw["e2ee_maintenance"]["unknown"] = json!(true);
    let snapshot = crate::groups::GroupReadResult::from_raw_response(raw, Vec::new());
    assert!(overlay_group_maintenance_status(local_status(true, true), &snapshot).is_err());
}
