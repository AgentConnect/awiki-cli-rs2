use super::*;

fn empty_direct(peer: &str) -> im_core::messages::Conversation {
    let peer = im_core::ids::PeerRef::parse(peer, "").unwrap();
    im_core::messages::Conversation {
        conversation_id: "dm:peer-scope:v1:acceptance".to_owned(),
        peer_persona_id: None,
        canonical_group_did: None,
        resolution_state: im_core::messages::ConversationResolutionState::Resolved,
        thread: im_core::messages::ThreadRef::Direct(peer.clone()),
        conversation_identity: None,
        title: None,
        participants: vec![peer],
        last_message: None,
        unread_count: 0,
        unread_mention_count: 0,
        first_unread_mention_message_id: None,
        message_count: 0,
        last_message_at: None,
        activity_at: None,
    }
}

#[test]
fn empty_self_conversation_keeps_its_direct_route_in_node_roster() {
    let owner = "did:example:alice";
    let mapped = conversation(empty_direct(owner), owner).unwrap();
    assert_eq!(mapped.peer_did.as_deref(), Some(owner));
    assert_eq!(mapped.kind, "direct");
    assert!(mapped.last_message.is_none());
    assert_eq!(mapped.message_count, 0);
}

#[test]
fn empty_direct_conversation_keeps_other_peer_and_unresolved_routes_fail() {
    let owner = "did:example:alice";
    let mut direct = empty_direct("did:example:bob");
    assert_eq!(
        conversation(direct.clone(), owner)
            .unwrap()
            .peer_did
            .as_deref(),
        Some("did:example:bob")
    );
    direct.resolution_state = im_core::messages::ConversationResolutionState::LegacyUnresolved;
    assert!(conversation(direct, owner).is_err());
}
