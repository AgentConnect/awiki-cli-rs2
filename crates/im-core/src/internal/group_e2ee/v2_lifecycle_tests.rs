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
