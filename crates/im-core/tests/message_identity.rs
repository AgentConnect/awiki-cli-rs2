use awiki_im_core::{
    ids::{MessageId, PeerRef},
    messages::{
        Message, MessageBodyView, MessageDirection, MessageKind, MessageMetadata,
        MessageMetadataAttribute, ThreadRef,
    },
};

fn projected_group_message() -> Message {
    let group = awiki_im_core::ids::GroupRef::parse("did:wba:example.test:groups:team").unwrap();
    Message {
        id: MessageId::parse("did:wba:example.test:groups:team:17").unwrap(),
        thread: ThreadRef::Group(group.clone()),
        direction: MessageDirection::Outgoing,
        sender: PeerRef::parse("did:wba:example.test:user:guest", "").unwrap(),
        receiver: None,
        group: Some(group),
        body: MessageBodyView::Text {
            text: "caption".to_owned(),
            kind: MessageKind::Text,
        },
        sent_at: Some("2026-09-02T06:11:54Z".to_owned()),
        received_at: None,
        metadata: MessageMetadata {
            operation_id: Some("downstream-operation".to_owned()),
            attributes: vec![MessageMetadataAttribute {
                key: "raw_message_id".to_owned(),
                value: "awg_client-message".to_owned(),
            }],
            ..Default::default()
        },
    }
}

#[test]
fn projected_message_matches_canonical_and_preserved_identity_aliases() {
    let message = projected_group_message();

    assert!(
        message.matches_identity(&MessageId::parse("did:wba:example.test:groups:team:17").unwrap())
    );
    assert!(message.matches_identity(&MessageId::parse("awg_client-message").unwrap()));
    assert!(message.matches_identity(&MessageId::parse("downstream-operation").unwrap()));
    assert!(!message.matches_identity(&MessageId::parse("unrelated-message").unwrap()));
}

#[test]
fn unrelated_metadata_is_not_treated_as_a_message_identity() {
    let mut message = projected_group_message();
    message.metadata.attributes = vec![MessageMetadataAttribute {
        key: "group_event_seq".to_owned(),
        value: "17".to_owned(),
    }];

    assert!(!message.matches_identity(&MessageId::parse("17").unwrap()));
}
