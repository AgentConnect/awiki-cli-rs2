use super::*;
use serde_json::json;

fn plan() -> TransitionPlan {
    TransitionPlan {
        update_id: "update-1".into(),
        group_did: "did:example:group".into(),
        group_state_version: "4".into(),
        previous_did: "did:example:old".into(),
        current_did: "did:example:new".into(),
        phase: "add_pending".into(),
        eligible_device_ids: vec!["device-b".into()],
    }
}

fn endpoint(did: &str, device: &str) -> Endpoint {
    (did.into(), device.into())
}

#[test]
fn recovery_defers_later_join_and_unrelated_changes_until_frozen_transition_finishes() {
    let plan = plan();
    // The later device sorts before the original Recovery device: the actual
    // regression must not keep attempting that forbidden Add forever.
    assert_eq!(
        desired_devices(
            Some(&plan),
            &plan.current_did,
            vec!["device-a-later".into(), "device-b".into()]
        )
        .unwrap(),
        vec!["device-b"]
    );
    let extra = vec![
        endpoint(&plan.previous_did, "old-device"),
        endpoint("did:example:other", "revoked"),
    ];
    let missing = vec![
        endpoint(&plan.current_did, "device-a-later"),
        endpoint(&plan.current_did, "device-b"),
        endpoint("did:example:other", "new"),
    ];
    let (removes, adds) = restrict_delta(Some(&plan), extra.clone(), missing).unwrap();
    assert!(removes.is_empty());
    assert_eq!(adds, vec![endpoint(&plan.current_did, "device-b")]);
    let (removes, adds) = restrict_delta(Some(&plan), extra, vec![]).unwrap();
    assert_eq!(removes, vec![endpoint(&plan.previous_did, "old-device")]);
    assert!(adds.is_empty());
    let later = vec![endpoint(&plan.current_did, "device-a-later")];
    assert_eq!(
        restrict_delta(None, vec![], later.clone()).unwrap().1,
        later
    );
}

#[test]
fn transition_never_substitutes_revoked_frozen_device_or_reopens_remove_phase() {
    let mut plan = plan();
    assert!(desired_devices(Some(&plan), &plan.current_did, vec!["later".into()]).is_err());
    plan.phase = "remove_pending".into();
    assert!(restrict_delta(
        Some(&plan),
        vec![],
        vec![endpoint(&plan.current_did, "device-b")]
    )
    .is_err());
    plan.phase = "action_required".into();
    assert!(restrict_delta(Some(&plan), vec![], vec![]).is_err());
}

#[test]
fn transition_plan_requires_exact_group_version_and_well_formed_frozen_devices() {
    let state = anp::group_e2ee::V2GroupStateRef {
        group_did: "did:example:group".into(),
        group_state_version: "4".into(),
        policy_hash: None,
        roster_hash: None,
    };
    let raw = json!({"e2ee_maintenance":{"did_transition":{
        "update_id":"update-1", "group_did":"did:example:group", "group_state_version":"4",
        "previous_did":"did:example:old", "current_did":"did:example:new",
        "phase":"add_pending", "eligible_device_ids":["device-b"]
    }}});
    assert_eq!(
        TransitionPlan::from_group_response(&raw, &state).unwrap(),
        Some(plan())
    );
    assert!(TransitionPlan::from_group_response(&json!({}), &state)
        .unwrap()
        .is_none());
    for (key, value) in [
        ("group_did", json!("did:example:other")),
        ("group_state_version", json!("3")),
        ("eligible_device_ids", json!([])),
        ("eligible_device_ids", json!(["b", "b"])),
        ("phase", json!("complete")),
        ("current_did", json!("did:example:old")),
    ] {
        let mut invalid = raw.clone();
        invalid["e2ee_maintenance"]["did_transition"][key] = value;
        assert!(
            TransitionPlan::from_group_response(&invalid, &state).is_err(),
            "{key}"
        );
    }
}

#[test]
fn home_hint_is_scoped_and_cannot_supply_transition_authority() {
    let hint = json!({"method": HINT_METHOD, "params": {
        "recipient_did":"did:example:owner", "group_did":"did:example:group"
    }});
    assert!(hint_group(&hint, "did:example:owner").is_ok());
    assert!(hint_group(&hint, "did:example:other").is_err());
    let mut injected = hint.clone();
    injected["params"]["eligible_device_ids"] = json!(["attacker"]);
    assert!(hint_group(&injected, "did:example:owner").is_err());
    let legacy = json!({"method":"group.e2ee.notice", "params":{"group_did":"did:example:group"}});
    assert!(hint_group(&legacy, "did:example:owner").is_err());
}

#[test]
fn transition_rejects_roster_without_successor_or_with_predecessor() {
    let plan = plan();
    let mut members = std::collections::BTreeSet::new();
    assert!(validate_roster_members(Some(&plan), &members).is_err());
    members.insert(plan.current_did.clone());
    assert!(validate_roster_members(Some(&plan), &members).is_ok());
    members.insert(plan.previous_did.clone());
    assert!(validate_roster_members(Some(&plan), &members).is_err());
    assert!(validate_roster_members(None, &members).is_ok());
}
