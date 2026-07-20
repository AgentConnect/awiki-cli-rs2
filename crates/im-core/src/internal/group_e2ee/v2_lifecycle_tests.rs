use super::*;

#[test]
fn key_package_device_selector_never_accepts_sibling_or_legacy_default() {
    assert!(require_current_device_selection(None, "device-a").is_ok());
    assert!(require_current_device_selection(Some("device-a"), "device-a").is_ok());
    assert!(require_current_device_selection(Some("device-b"), "device-a").is_err());
    assert!(require_current_device_selection(Some("default"), "device-a").is_err());
}

#[test]
fn legacy_group_state_ref_converts_without_internal_versions() {
    let converted = v2_group_state_ref(anp::group_e2ee::GroupStateRef {
        group_did: "did:example:group".to_owned(),
        group_state_version: "7".to_owned(),
        policy_hash: Some("policy".to_owned()),
    });
    assert_eq!(converted.group_state_version, "7");
    assert_eq!(converted.policy_hash.as_deref(), Some("policy"));
    assert_eq!(converted.roster_hash, None);
}

#[test]
fn create_requires_exact_p4_group_state_ref() {
    let missing = required_created_group_state_ref("did:example:group", None)
        .expect_err("P6 create must not synthesize a missing P4 state reference");
    assert!(matches!(
        missing,
        crate::ImError::LocalStateUnavailable { .. }
    ));

    let wrong_group = required_created_group_state_ref(
        "did:example:group",
        Some(anp::group_e2ee::GroupStateRef {
            group_did: "did:example:other".to_owned(),
            group_state_version: "7".to_owned(),
            policy_hash: None,
        }),
    )
    .expect_err("P6 create must bind the exact P4 group");
    assert!(matches!(wrong_group, crate::ImError::PermissionDenied));
}

#[test]
fn roster_delta_adds_same_did_device_without_duplicate_business_member() {
    let manifest = DeviceManifest {
        manifest_type: anp::authentication::DEVICE_MANIFEST_TYPE.to_owned(),
        devices: vec![
            manifest_device("device-b2", true),
            manifest_device("device-b1", true),
            manifest_device("device-b3", false),
        ],
    };
    let endpoints = vec![
        endpoint("did:example:owner", "device-a1"),
        endpoint("did:example:bob", "device-b1"),
        endpoint("did:example:carol", "device-c1"),
    ];
    let desired = eligible_device_ids(&manifest)
        .into_iter()
        .map(|device_id| ("did:example:bob".to_owned(), device_id))
        .collect::<BTreeSet<_>>();
    let observed = endpoint_set_for_member("did:example:bob", &endpoints);
    let (extra, missing) = roster_delta(&desired, &observed);
    assert!(extra.is_empty());
    assert_eq!(
        missing,
        vec![("did:example:bob".to_owned(), "device-b2".to_owned())]
    );
}

#[test]
fn roster_delta_removes_only_the_revoked_exact_device_and_keeps_siblings() {
    let desired = [
        ("did:example:owner".to_owned(), "device-a1".to_owned()),
        ("did:example:bob".to_owned(), "device-b2".to_owned()),
    ]
    .into_iter()
    .collect::<BTreeSet<_>>();
    let observed = [
        ("did:example:owner".to_owned(), "device-a1".to_owned()),
        ("did:example:bob".to_owned(), "device-b1".to_owned()),
        ("did:example:bob".to_owned(), "device-b2".to_owned()),
    ]
    .into_iter()
    .collect::<BTreeSet<_>>();

    let (extra, missing) = roster_delta(&desired, &observed);
    assert_eq!(
        extra,
        vec![("did:example:bob".to_owned(), "device-b1".to_owned())]
    );
    assert!(missing.is_empty());
    assert!(desired.contains(&("did:example:bob".to_owned(), "device-b2".to_owned())));
}

#[test]
fn empty_eligible_manifest_set_can_drive_whole_roster_leaf_removal() {
    let manifest = DeviceManifest {
        manifest_type: anp::authentication::DEVICE_MANIFEST_TYPE.to_owned(),
        devices: vec![manifest_device("device-b1", false)],
    };
    let desired = eligible_device_ids(&manifest)
        .into_iter()
        .map(|device_id| ("did:example:bob".to_owned(), device_id))
        .collect::<BTreeSet<_>>();
    let observed = endpoint_set_for_member(
        "did:example:bob",
        &[endpoint("did:example:bob", "device-b1")],
    );

    let (extra, missing) = roster_delta(&desired, &observed);
    assert_eq!(
        extra,
        vec![("did:example:bob".to_owned(), "device-b1".to_owned())]
    );
    assert!(missing.is_empty());
}

#[test]
fn exact_device_selector_does_not_match_a_sibling_leaf() {
    let endpoints = vec![
        endpoint("did:example:bob", "device-b1"),
        endpoint("did:example:bob", "device-b2"),
        endpoint("did:example:alice", "device-a1"),
    ];
    assert!(has_exact_endpoint(
        &endpoints,
        "did:example:bob",
        "device-b1"
    ));
    assert!(!has_exact_endpoint(
        &endpoints,
        "did:example:bob",
        "device-a1"
    ));
}

#[test]
fn exact_device_remove_skips_groups_without_active_local_mls_state() {
    assert!(local_group_can_remove_exact_device(
        &V2LocalGroupReadiness::Active
    ));
    assert!(!local_group_can_remove_exact_device(
        &V2LocalGroupReadiness::Missing
    ));
    assert!(!local_group_can_remove_exact_device(
        &V2LocalGroupReadiness::Inactive
    ));
}

#[test]
fn p4_owner_without_local_mls_state_skips_roster_repair() {
    assert!(local_group_can_reconcile_roster(
        &V2LocalGroupReadiness::Active
    ));
    assert!(!local_group_can_reconcile_roster(
        &V2LocalGroupReadiness::Missing
    ));
    assert!(!local_group_can_reconcile_roster(
        &V2LocalGroupReadiness::Inactive
    ));
}

#[test]
fn revoke_inventory_calls_exact_remove_only_for_active_owned_groups() {
    let listed = crate::groups::GroupReadResult::from_raw_response(
        serde_json::json!({
            "groups": [
                {
                    "group_did": "did:example:owned",
                    "member_role": "owner",
                    "member_status": "active"
                },
                {
                    "group_did": "did:example:member",
                    "member_role": "member",
                    "member_status": "active"
                },
                {
                    "group_did": "did:example:old-owned",
                    "member_role": "owner",
                    "member_status": "left"
                }
            ],
            "total": 3
        }),
        Vec::new(),
    );
    let mut called = Vec::new();
    let removed = reconcile_owned_group_device_removals(listed, |group| {
        called.push(group.as_str().to_owned());
        Ok(true)
    })
    .unwrap();
    assert_eq!(called, vec!["did:example:owned"]);
    assert_eq!(removed, 1);

    let incomplete = crate::groups::GroupReadResult::from_raw_response(
        serde_json::json!({
            "groups": [{
                "group_did": "did:example:owned",
                "member_role": "owner",
                "member_status": "active"
            }],
            "total": 2
        }),
        Vec::new(),
    );
    assert!(matches!(
        reconcile_owned_group_device_removals(incomplete, |_| Ok(false)),
        Err(crate::ImError::LocalStateUnavailable { .. })
    ));
}

#[test]
fn full_page_without_completeness_marker_never_drives_leaf_deletion() {
    let groups = (0..100)
        .map(|index| {
            serde_json::json!({
                "group_did": format!("did:example:owned-{index}"),
                "member_role": "owner",
                "member_status": "active"
            })
        })
        .collect::<Vec<_>>();
    let listed = crate::groups::GroupReadResult::from_raw_response(
        serde_json::json!({ "groups": groups }),
        Vec::new(),
    );
    let mut removal_calls = 0usize;

    let error = reconcile_owned_group_device_removals(listed, |_| {
        removal_calls += 1;
        Ok(true)
    })
    .expect_err("an unmarked full page must not be treated as a complete inventory");

    assert!(matches!(
        error,
        crate::ImError::LocalStateUnavailable { .. }
    ));
    assert_eq!(removal_calls, 0);
}

#[test]
fn inventory_completeness_requires_consistent_has_more_or_total() {
    let groups = (0..100)
        .map(|index| serde_json::json!({ "group_did": format!("did:example:g-{index}") }))
        .collect::<Vec<_>>();

    let no_marker = serde_json::json!({ "groups": groups });
    assert!(require_complete_inventory(&no_marker, "groups", 100, None, "incomplete").is_err());

    let has_more = serde_json::json!({ "groups": no_marker["groups"], "has_more": true });
    assert!(require_complete_inventory(&has_more, "groups", 100, None, "incomplete").is_err());

    let final_page = serde_json::json!({
        "groups": no_marker["groups"],
        "has_more": false
    });
    assert!(require_complete_inventory(&final_page, "groups", 100, None, "incomplete").is_ok());

    let matching_total = serde_json::json!({ "groups": no_marker["groups"], "total": 100 });
    assert!(
        require_complete_inventory(&matching_total, "groups", 100, Some(100), "incomplete").is_ok()
    );

    let larger_total = serde_json::json!({ "groups": no_marker["groups"], "total": 101 });
    assert!(
        require_complete_inventory(&larger_total, "groups", 100, Some(101), "incomplete").is_err()
    );
}

#[test]
fn prepared_remove_wal_rebuild_preserves_operation_and_exact_device() {
    let meta = V2GroupControlMetadata {
        anp_version: Some("2.0".to_owned()),
        profile: GROUP_E2EE_PROFILE_V2.to_owned(),
        security_profile: GROUP_E2EE_SECURITY_PROFILE_V2.to_owned(),
        sender_did: "did:example:owner".to_owned(),
        sender_device_id: "device-a1".to_owned(),
        target: V2Target {
            kind: "group".to_owned(),
            did: "did:example:group".to_owned(),
        },
        operation_id: "op-remove-b1".to_owned(),
        created_at: None,
    };
    let pending = V2ReconciledPendingCommit {
        pending_commit_id: "pending-remove-b1".to_owned(),
        operation_id: "op-remove-b1".to_owned(),
        group_did: "did:example:group".to_owned(),
        previous_status: "prepared".to_owned(),
        status: "prepared".to_owned(),
        action: "awaiting-service-decision".to_owned(),
        prepared_response: Some(serde_json::json!({
            "member_did": "did:example:bob",
            "member_device_id": "device-b1",
            "group_state_ref": {
                "group_did": "did:example:group",
                "group_state_version": "9"
            },
            "crypto_group_id_b64u": "Y3J5cHRvLWdyb3Vw",
            "epoch": "7",
            "commit_b64u": "Y29tbWl0"
        })),
    };

    let PreparedMembershipSubmission::Remove(replay) =
        prepared_membership_submission(meta, pending).unwrap()
    else {
        panic!("remove WAL must reconstruct a Remove submission");
    };
    assert_eq!(replay.meta.operation_id, "op-remove-b1");
    assert_eq!(replay.meta.created_at, None);
    assert_eq!(replay.prepared.pending_commit_id, "pending-remove-b1");
    assert_eq!(replay.prepared.from_epoch, "6");
    assert_eq!(replay.prepared.body.member_did, "did:example:bob");
    assert_eq!(replay.prepared.body.member_device_id, "device-b1");
}

#[test]
fn only_explicit_nonaccepting_host_codes_are_safe_to_abort() {
    assert!(p6_host_rejection_is_deterministic(
        &crate::ImError::Service {
            status_code: Some(409),
            code: Some("group.state_version_conflict".to_owned()),
            message: "stale P4 reference".to_owned(),
            data: None,
        }
    ));
    assert!(!p6_host_rejection_is_deterministic(
        &crate::ImError::TransportUnavailable {
            detail: "response may have been lost".to_owned(),
        }
    ));
    assert!(!p6_host_rejection_is_deterministic(
        &crate::ImError::Service {
            status_code: None,
            code: Some("anp.idempotency_conflict".to_owned()),
            message: "a previous request may already have been accepted".to_owned(),
            data: None,
        }
    ));
    assert!(!p6_host_rejection_is_deterministic(
        &crate::ImError::Service {
            status_code: Some(503),
            code: Some("temporarily_unavailable".to_owned()),
            message: "outcome is not authoritative".to_owned(),
            data: None,
        }
    ));
}

#[test]
fn remove_plan_targets_all_and_only_the_selected_did_devices() {
    let endpoints = vec![
        endpoint("did:example:bob", "device-b2"),
        endpoint("did:example:alice", "device-a1"),
        endpoint("did:example:bob", "device-b1"),
        endpoint("did:example:alice", "device-a2"),
        endpoint("did:example:bob", "device-b1"),
    ];
    assert_eq!(
        member_device_ids("did:example:bob", &endpoints),
        vec!["device-b1", "device-b2"]
    );
    assert_eq!(
        member_device_ids("did:example:alice", &endpoints),
        vec!["device-a1", "device-a2"]
    );
}

#[test]
fn current_controller_preflight_requires_the_exact_authenticated_leaf() {
    let endpoints = vec![
        endpoint("did:example:alice", "device-a1"),
        endpoint("did:example:alice", "device-a2"),
    ];
    assert!(
        require_current_controller_endpoint(&endpoints, "did:example:alice", "device-a1").is_ok()
    );
    assert!(
        require_current_controller_endpoint(&endpoints, "did:example:alice", "device-a3").is_err()
    );
}

#[test]
fn p4_member_transition_must_match_group_member_status_and_version() {
    let group = "did:example:group";
    let member = "did:example:bob";
    let accepted = crate::groups::GroupReadResult::from_raw_response(
        serde_json::json!({
            "group_did": group,
            "member_did": member,
            "membership_status": "active",
            "group_state_ref": {
                "group_did": group,
                "group_state_version": "7"
            }
        }),
        Vec::new(),
    );
    let transition = required_member_transition(group, Some(member), "active", &accepted).unwrap();
    assert_eq!(transition.member_did, member);
    assert_eq!(transition.group_state_ref.group_did, group);
    assert_eq!(transition.group_state_ref.group_state_version, "7");

    assert!(
        required_member_transition(group, Some("did:example:other"), "active", &accepted).is_err()
    );
    assert!(required_member_transition(group, Some(member), "removed", &accepted).is_err());

    let pre_p4_handle_lookup = "did:example:bob-old";
    let handle_race_response = crate::groups::GroupReadResult::from_raw_response(
        serde_json::json!({
            "group_did": group,
            "member_did": member,
            "membership_status": "active",
            "group_state_version": "8"
        }),
        Vec::new(),
    );
    let handle_raced =
        required_member_transition(group, None, "active", &handle_race_response).unwrap();
    assert_ne!(handle_raced.member_did, pre_p4_handle_lookup);
    assert_eq!(
        handle_raced.member_did, member,
        "P6 subject must come from the successful P4 response, not a pre-P4 Handle lookup"
    );

    let removed_without_optional_status = crate::groups::GroupReadResult::from_raw_response(
        serde_json::json!({
            "group_did": group,
            "member_did": member,
            "group_state_version": "8"
        }),
        Vec::new(),
    );
    assert!(required_member_transition(
        group,
        Some(member),
        "removed",
        &removed_without_optional_status
    )
    .is_ok());

    let conflicting_reference = crate::groups::GroupReadResult::from_raw_response(
        serde_json::json!({
            "group_did": group,
            "member_did": member,
            "membership_status": "active",
            "group_state_ref": {
                "group_did": "did:example:other",
                "group_state_version": "9"
            }
        }),
        Vec::new(),
    );
    assert!(matches!(
        required_member_transition(group, Some(member), "active", &conflicting_reference),
        Err(crate::ImError::PermissionDenied)
    ));

    let conflicting_version = crate::groups::GroupReadResult::from_raw_response(
        serde_json::json!({
            "group_did": group,
            "member_did": member,
            "membership_status": "active",
            "group_state_version": "9",
            "group": {
                "group_state_ref": {
                    "group_did": group,
                    "group_state_version": "10"
                }
            }
        }),
        Vec::new(),
    );
    assert!(matches!(
        required_member_transition(group, Some(member), "active", &conflicting_version),
        Err(crate::ImError::PermissionDenied)
    ));
}

fn manifest_device(device_id: &str, group_e2ee: bool) -> anp::authentication::DeviceManifestEntry {
    let mut profiles = vec![
        "anp.core.binding.v2".to_owned(),
        "anp.identity.discovery.v2".to_owned(),
        "anp.group.base.v2".to_owned(),
    ];
    if group_e2ee {
        profiles.push(PROFILE_GROUP_E2EE_V2.to_owned());
    }
    anp::authentication::DeviceManifestEntry {
        device_id: device_id.to_owned(),
        signing_key_id: format!("did:example:bob#{device_id}-sign"),
        e2ee_key_id: format!("did:example:bob#{device_id}-e2ee"),
        profiles,
    }
}

fn endpoint(member_did: &str, member_device_id: &str) -> V2LocalGroupMemberEndpoint {
    V2LocalGroupMemberEndpoint {
        member_did: member_did.to_owned(),
        member_device_id: member_device_id.to_owned(),
    }
}
