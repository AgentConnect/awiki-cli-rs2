use crate::messages::*;

#[test]
fn secure_attachment_result_preserves_wire_target_with_canonical_thread() {
    let prepared = crate::attachments::manifest::prepare_object_e2ee_attachment_payload(
        "test.md",
        "text/markdown",
        b"secret".to_vec(),
    )
    .unwrap();
    let descriptor = crate::attachments::manifest::AttachmentDescriptor::from_prepared(
        &prepared.prepared,
        "attachment-1",
        "https://objects.example/attachment-1",
    );
    let manifest =
        crate::attachments::manifest::build_attachment_manifest(&descriptor, "caption").unwrap();
    for (target, kind, did) in [
        (
            MessageTarget::Direct(crate::ids::PeerRef::parse("peer.example", "").unwrap()),
            "agent",
            "did:example:peer",
        ),
        (
            MessageTarget::Group(crate::ids::GroupRef::parse("did:example:group").unwrap()),
            "group",
            "did:example:group",
        ),
    ] {
        let result = SendMessageResult {
            message: Message {
                id: crate::ids::MessageId::parse("msg-attachment-1").unwrap(),
                thread: ThreadRef::Thread(
                    crate::ids::ThreadId::parse("dm:peer-scope:opaque").unwrap(),
                ),
                direction: MessageDirection::Outgoing,
                sender: crate::ids::PeerRef::parse("did:example:me", "").unwrap(),
                receiver: Some(crate::ids::PeerRef::parse("did:example:peer", "").unwrap()),
                group: None,
                body: MessageBodyView::Unsupported { content_type: None },
                sent_at: None,
                received_at: None,
                metadata: MessageMetadata {
                    attributes: vec![MessageMetadataAttribute {
                        key: "attachment_manifest".into(),
                        value: manifest.to_string(),
                    }],
                    ..Default::default()
                },
            },
            delivery: DeliveryState::Accepted,
            warnings: vec![],
        };
        let converted =
            super::attachment_send_result_from_secure_message(result.clone(), &target).unwrap();
        assert_eq!(converted.message, result);
        assert_eq!(converted.target_kind, kind);
        assert_eq!(converted.target_did, did);
        assert!(!converted.manifest.to_string().contains("object_key_b64u"));
    }
}
