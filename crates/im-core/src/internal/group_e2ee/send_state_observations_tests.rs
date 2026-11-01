use super::*;

fn observed() -> ObservedGroup {
    ObservedGroup::refresh(
        None,
        GroupStateObservation {
            group_did: "did:example:group".into(),
            host_service_did: "did:example:host".into(),
            group_state_version: "2".into(),
            epoch: Some("3".into()),
            revision: "4".into(),
            local_identity_coverage: true,
        },
        Some("connection".into()),
        true,
    )
}

fn pending(phase: &str) -> serde_json::Value {
    json!({
        "group_did":"did:example:group", "group_state_version":"2",
        "my_role":"owner", "membership_status":"active",
        "e2ee_maintenance": {
            "reason":"group_membership_change_pending", "send_paused":true,
            "did_transition": {
                "update_id":"update", "group_did":"did:example:group", "group_state_version":"2",
                "previous_did":"did:example:old", "current_did":"did:example:new",
                "phase":phase, "eligible_device_ids":["device"]
            }
        }
    })
}

fn receive_fresh(previous: &ObservedGroup) -> ObservedGroup {
    ObservedGroup::refresh(
        Some(previous),
        previous.observation.clone(),
        previous.connection.clone(),
        true,
    )
}

#[test]
fn observed_group_read_repeated_pending_preserves_fresh_receive_without_ready() {
    let mut state = observed();
    let raw = pending("remove_pending");
    assert!(state.invalidate_from_read(&raw));
    assert!(!state.caught_up);
    let mut fresh = receive_fresh(&state);
    let generation = fresh.local_generation;
    assert!(
        !fresh.invalidate_from_read(&raw),
        "unchanged maintenance must not start another receive loop"
    );
    assert!(fresh.caught_up);
    assert!(
        fresh.ready.is_none(),
        "fresh receive does not grant prepared send authority"
    );
    assert_eq!(fresh.local_generation, generation);
}

#[test]
fn observed_group_read_member_page_is_not_maintenance_removal() {
    let mut state = observed();
    assert!(state.invalidate_from_read(&pending("add_pending")));
    let mut fresh = receive_fresh(&state);
    let generation = fresh.local_generation;
    let page = json!({
        "group_did": "did:example:group", "group_state_version": "2",
        "members": [{"agent_did":"did:example:current", "status":"active"}],
        "total": 1, "has_more": false
    });
    assert!(
        !fresh.invalidate_from_member_page(&page),
        "a member-page response does not report whether maintenance ended"
    );
    assert!(fresh.caught_up);
    assert_eq!(fresh.local_generation, generation);
    assert!(fresh.ready.is_none());
}

#[test]
fn observed_group_member_page_new_version_still_invalidates() {
    let mut state = observed();
    assert!(state.invalidate_from_read(&pending("add_pending")));
    let mut fresh = receive_fresh(&state);
    let marker = fresh.maintenance_fingerprint;
    assert!(fresh.invalidate_from_member_page(&json!({
        "group_did":"did:example:group", "group_state_version":"3",
        "members":[], "total":0, "has_more":false
    })));
    assert!(!fresh.caught_up);
    assert!(fresh.ready.is_none());
    assert_eq!(fresh.maintenance_fingerprint, marker);
}

#[test]
fn observed_group_member_page_explicit_maintenance_clear_still_invalidates() {
    let mut state = observed();
    assert!(state.invalidate_from_read(&pending("add_pending")));
    let mut fresh = receive_fresh(&state);
    assert!(fresh.invalidate_from_member_page(&json!({
        "group_did":"did:example:group", "group_state_version":"2",
        "e2ee_maintenance":null, "members":[], "total":0, "has_more":false
    })));
    assert!(!fresh.caught_up);
    assert!(fresh.ready.is_none());
}

#[test]
fn observed_group_read_legacy_maintenance_preserves_fresh_receive() {
    for (reason, paused) in [
        ("device_revocation_pending", false),
        ("device_revocation_pending", true),
        ("group_membership_change_pending", true),
    ] {
        let mut state = observed();
        let mut raw = pending("remove_pending");
        raw["e2ee_maintenance"] = json!({"reason":reason,"send_paused":paused});
        assert!(state.invalidate_from_read(&raw));
        let mut fresh = receive_fresh(&state);
        assert!(!fresh.invalidate_from_read(&raw));
        assert!(fresh.caught_up);
        assert!(fresh.ready.is_none());
    }
}

#[test]
fn observed_group_read_pending_progress_and_clear_invalidate_again() {
    let mut state = observed();
    assert!(state.invalidate_from_read(&pending("add_pending")));
    let mut state = receive_fresh(&state);
    assert!(state.invalidate_from_read(&pending("remove_pending")));
    assert!(!state.caught_up);
    let mut state = receive_fresh(&state);
    let mut cleared = pending("remove_pending");
    cleared.as_object_mut().unwrap().remove("e2ee_maintenance");
    assert!(
        state.invalidate_from_read(&cleared),
        "maintenance removal also needs fresh receive facts"
    );
    assert!(!state.caught_up);
}

#[test]
fn observed_group_read_rejects_invalid_maintenance_without_retaining_freshness() {
    for change in [
        ("/e2ee_maintenance/private", json!(true)),
        ("/e2ee_maintenance/reason", json!("unknown")),
        ("/e2ee_maintenance/send_paused", json!("true")),
        ("/e2ee_maintenance/send_paused", json!(false)),
        (
            "/e2ee_maintenance/did_transition/group_did",
            json!("did:example:other"),
        ),
        ("/e2ee_maintenance/did_transition/phase", json!("unknown")),
    ] {
        let mut state = observed();
        let mut raw = pending("remove_pending");
        if change.0.ends_with("/private") {
            raw["e2ee_maintenance"]["private"] = change.1;
        } else {
            *raw.pointer_mut(change.0).unwrap() = change.1;
        }
        assert!(state.invalidate_from_read(&raw));
        let mut fresh = receive_fresh(&state);
        assert!(fresh.invalidate_from_read(&raw));
        assert!(!fresh.caught_up);
        assert!(fresh.ready.is_none());
    }
}

#[test]
fn observed_group_read_older_version_cannot_invalidate_current_observation() {
    let mut state = observed();
    let mut raw = pending("remove_pending");
    raw["group_state_version"] = json!("1");
    assert!(!state.invalidate_from_read(&raw));
    assert!(state.caught_up);
    assert_eq!(state.local_generation, 0);
}
