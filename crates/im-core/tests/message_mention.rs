use awiki_im_core::messages::{
    is_message_mention_payload, parse_message_mention_payload, MessageMentionRole,
    MessageMentionSelector, MessageMentionTarget, SendMessageRequest,
};
use serde_json::json;

#[test]
fn mention_payload_accepts_schema_less_group_selector() {
    let payload = json!({
        "text": "@agents 请总结这段讨论",
        "mentions": [
            {
                "id": "men_1",
                "range": {"start": 0, "end": 7, "unit": "unicode_code_point"},
                "target": {"kind": "group_selector", "selector": "agents"}
            }
        ]
    });

    assert!(is_message_mention_payload(&payload));
    let parsed = parse_message_mention_payload(&payload).unwrap();

    assert_eq!(parsed.text, "@agents 请总结这段讨论");
    assert_eq!(parsed.mentions.len(), 1);
    assert_eq!(
        parsed.mentions[0].mention_role,
        MessageMentionRole::Addressee
    );
    assert_eq!(
        parsed.mentions[0].target,
        MessageMentionTarget::GroupSelector {
            selector: MessageMentionSelector::Agents
        }
    );
    assert!(payload.get("schema").is_none());
}

#[test]
fn mention_payload_allows_display_name_but_identity_is_did() {
    let payload = json!({
        "text": "@Alice hello",
        "mentions": [
            {
                "id": "men_1",
                "range": {"start": 0, "end": 6, "unit": "unicode_code_point"},
                "target": {
                    "kind": "human",
                    "did": "did:wba:example.com:user:alice",
                    "display_name": "Alice"
                },
                "mention_role": "cc"
            }
        ]
    });

    let parsed = parse_message_mention_payload(&payload).unwrap();
    assert_eq!(parsed.mentions[0].mention_role, MessageMentionRole::Cc);
    assert_eq!(
        parsed.mentions[0].target,
        MessageMentionTarget::Human {
            did: "did:wba:example.com:user:alice".to_owned(),
            display_name: Some("Alice".to_owned())
        }
    );
}

#[test]
fn mention_payload_rejects_forbidden_sender_and_proof_fields() {
    for field in [
        "sender",
        "sender_did",
        "from",
        "actor_did",
        "auth",
        "origin_proof",
        "proof",
        "signature",
    ] {
        let mut mention = json!({
            "id": "men_1",
            "range": {"start": 0, "end": 7, "unit": "unicode_code_point"},
            "target": {"kind": "group_selector", "selector": "agents"}
        });
        mention
            .as_object_mut()
            .unwrap()
            .insert(field.to_owned(), json!("bad"));
        let payload = json!({"text": "@agents hi", "mentions": [mention]});
        assert!(
            parse_message_mention_payload(&payload).is_err(),
            "{field} should be rejected"
        );
    }
}

#[test]
fn mention_payload_rejects_invalid_range_duplicate_id_and_bad_selector_shape() {
    let invalid_range = json!({
        "text": "@agents hi",
        "mentions": [
            {
                "id": "men_1",
                "range": {"start": 0, "end": 99, "unit": "unicode_code_point"},
                "target": {"kind": "group_selector", "selector": "agents"}
            }
        ]
    });
    assert!(parse_message_mention_payload(&invalid_range).is_err());

    let duplicate_id = json!({
        "text": "@agents @humans hi",
        "mentions": [
            {
                "id": "men_1",
                "range": {"start": 0, "end": 7, "unit": "unicode_code_point"},
                "target": {"kind": "group_selector", "selector": "agents"}
            },
            {
                "id": "men_1",
                "range": {"start": 8, "end": 15, "unit": "unicode_code_point"},
                "target": {"kind": "group_selector", "selector": "humans"}
            }
        ]
    });
    assert!(parse_message_mention_payload(&duplicate_id).is_err());

    let selector_with_did = json!({
        "text": "@agents hi",
        "mentions": [
            {
                "id": "men_1",
                "range": {"start": 0, "end": 7, "unit": "unicode_code_point"},
                "target": {
                    "kind": "group_selector",
                    "selector": "agents",
                    "did": "did:wba:example.com:agent:x"
                }
            }
        ]
    });
    assert!(parse_message_mention_payload(&selector_with_did).is_err());
}

#[test]
fn schema_less_mention_payload_can_be_used_in_send_payload_request() {
    let payload = json!({
        "text": "@humans review this",
        "mentions": [
            {
                "id": "men_1",
                "range": {"start": 0, "end": 7, "unit": "unicode_code_point"},
                "target": {"kind": "group_selector", "selector": "humans"}
            }
        ]
    });
    parse_message_mention_payload(&payload).unwrap();

    let request = SendMessageRequest {
        target: awiki_im_core::messages::MessageTarget::Group(
            awiki_im_core::ids::GroupRef::parse("did:wba:example.com:group:team").unwrap(),
        ),
        body: awiki_im_core::messages::MessageBody::Payload { payload },
        security: awiki_im_core::messages::MessageSecurityMode::DefaultPlain,
        client_message_id: None,
        delivery: awiki_im_core::messages::MessageDeliveryOptions::default(),
        delegated_signing: None,
    };

    assert!(matches!(
        request.body,
        awiki_im_core::messages::MessageBody::Payload { .. }
    ));
}
