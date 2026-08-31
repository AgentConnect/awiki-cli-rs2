use crate::dto::{
    created_group_from_snapshot, group_create_request, group_join_request, group_member,
    group_member_mutation_request, NodeAddGroupMemberInput, NodeCreateGroupInput, NodeGroupInput,
};

#[test]
fn group_create_contract_trims_input_and_pins_safe_mvp_defaults() {
    let handle = im_core::ids::Handle::parse("alice.awiki.info", "").unwrap();
    let request = group_create_request(
        NodeCreateGroupInput {
            name: "  Release Crew  ".to_owned(),
            description: Some("  ships together  ".to_owned()),
        },
        Some(&handle),
    )
    .unwrap();

    assert_eq!(request.name, "Release Crew");
    assert_eq!(request.description.as_deref(), Some("ships together"));
    assert_eq!(request.discoverability.unwrap().as_str(), "private");
    assert_eq!(request.admission_mode.unwrap().as_str(), "open-join");
    assert_eq!(
        request.message_security_profile.unwrap().as_str(),
        "transport-protected"
    );
    assert!(!request.e2ee);
    assert_eq!(request.creator_handle.as_ref(), Some(&handle));
}

#[test]
fn group_create_contract_rejects_blank_name() {
    let error = group_create_request(
        NodeCreateGroupInput {
            name: " \n ".to_owned(),
            description: None,
        },
        None,
    )
    .unwrap_err();

    assert_eq!(error.code, "invalid_input");
}

#[test]
fn group_join_contract_attaches_the_current_handle_without_public_input() {
    let handle = im_core::ids::Handle::parse("alice.awiki.info", "").unwrap();
    let request = group_join_request(
        NodeGroupInput {
            group_did: "did:wba:awiki.info:groups:release-crew".to_owned(),
        },
        Some(&handle),
    )
    .unwrap();

    assert_eq!(request.member_handle.as_ref(), Some(&handle));
    assert_eq!(
        request.group.as_str(),
        "did:wba:awiki.info:groups:release-crew"
    );
}

#[test]
fn created_group_uses_core_canonical_conversation_identity() {
    let group = im_core::groups::GroupSnapshot {
        id: Some("server-group-1".to_owned()),
        did: im_core::ids::GroupRef::parse("did:wba:awiki.info:group:release-crew").unwrap(),
        name: Some("Release Crew".to_owned()),
        display_name: None,
        description: Some("ships together".to_owned()),
        avatar_uri: None,
        my_role: Some("owner".to_owned()),
        membership_status: Some("active".to_owned()),
        member_count: Some(1),
        last_message_at: None,
    };
    let value = created_group_from_snapshot(group, "fallback");

    assert_eq!(value.did, "did:wba:awiki.info:group:release-crew");
    assert_eq!(
        value.conversation_id,
        "group:did:wba:awiki.info:group:release-crew"
    );
    assert_eq!(value.title, "Release Crew");
    assert_eq!(value.description.as_deref(), Some("ships together"));
    assert_eq!(value.member_count, Some(1));
}

#[test]
fn add_member_contract_accepts_handle_and_projects_authoritative_resolution() {
    let request = group_member_mutation_request(
        NodeAddGroupMemberInput {
            group_did: "did:wba:awiki.info:group:release-crew".to_owned(),
            member: "alice".to_owned(),
            role: Some("member".to_owned()),
        },
        "awiki.info",
    )
    .unwrap();
    assert_eq!(request.member.as_str(), "alice.awiki.info");
    assert_eq!(
        request.role.as_ref().map(|role| role.as_str()),
        Some("member")
    );

    let member = group_member(im_core::groups::GroupMemberResolution {
        did: im_core::ids::Did::parse("did:wba:awiki.info:alice").unwrap(),
        handle: Some(im_core::ids::Handle::parse("alice.awiki.info", "awiki.info").unwrap()),
    });
    assert_eq!(member.did, "did:wba:awiki.info:alice");
    assert_eq!(member.handle.as_deref(), Some("alice.awiki.info"));
}
