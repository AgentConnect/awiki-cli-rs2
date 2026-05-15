use awiki_cli::runtime::listener_secure_notifications::{
    is_direct_secure_incoming_notification, is_secure_direct_wire_content_type,
    plaintext_body_to_notification_body, secure_notification_from_message_view,
};
use serde_json::json;

#[test]
fn secure_wire_content_type_matches_go_exact_values() {
    assert!(is_secure_direct_wire_content_type(
        "application/anp-direct-init+json"
    ));
    assert!(is_secure_direct_wire_content_type(
        "application/anp-direct-cipher+json"
    ));
    assert!(!is_secure_direct_wire_content_type(
        "Application/anp-direct-init+json"
    ));
    assert!(!is_secure_direct_wire_content_type("text/plain"));
    assert!(!is_secure_direct_wire_content_type(""));
}

#[test]
fn direct_secure_incoming_notification_detection_matches_go() {
    let secure = json!({
        "jsonrpc": "2.0",
        "method": "direct.incoming",
        "params": {
            "meta": {
                "content_type": "application/anp-direct-init+json"
            }
        }
    });
    assert!(is_direct_secure_incoming_notification(&secure));

    for notification in [
        json!({"method": "direct.incoming", "params": {"meta": {"content_type": "application/anp-direct-cipher+json"}}}),
        json!({"method": "direct.incoming", "params": {"meta": {"content_type": "text/plain"}}}),
        json!({"method": "group.incoming", "params": {"meta": {"content_type": "application/anp-direct-init+json"}}}),
        json!({"method": "direct.incoming", "params": {"meta": {"content_type": 42}}}),
        json!({"method": "direct.incoming", "params": []}),
    ] {
        let expected = notification["method"] == "direct.incoming"
            && notification["params"]["meta"]["content_type"]
                == "application/anp-direct-cipher+json";
        assert_eq!(
            is_direct_secure_incoming_notification(&notification),
            expected,
            "{notification}"
        );
    }
}

#[test]
fn secure_notification_from_message_view_builds_go_direct_incoming_shape() {
    let message_view = json!({
        "id": "msg-secure-001",
        "sender_did": "did:wba:example.com:user:bob:e1_bob",
        "receiver_did": "did:wba:example.com:user:alice:e1_alice",
        "content_type": "application/anp-direct-cipher+json",
        "server_seq": 42,
        "content": {
            "session_id": "sess-001",
            "ciphertext": "abc",
            "aad": {"kid": "did:wba:example.com:user:bob:e1_bob#key-3"}
        }
    });

    let notification =
        secure_notification_from_message_view(&message_view).expect("secure notification");

    assert_eq!(notification["method"], "direct.incoming");
    let params = &notification["params"];
    assert_eq!(params["server_seq"], 42);
    let meta = &params["meta"];
    assert_eq!(meta["sender_did"], "did:wba:example.com:user:bob:e1_bob");
    assert_eq!(meta["target"]["kind"], "agent");
    assert_eq!(
        meta["target"]["did"],
        "did:wba:example.com:user:alice:e1_alice"
    );
    assert_eq!(meta["message_id"], "msg-secure-001");
    assert_eq!(meta["profile"], "anp.direct.e2ee.v1");
    assert_eq!(meta["security_profile"], "direct-e2ee");
    assert_eq!(meta["content_type"], "application/anp-direct-cipher+json");
    assert_eq!(params["body"], message_view["content"]);
}

#[test]
fn secure_notification_from_message_view_errors_match_go_boundaries() {
    let err = secure_notification_from_message_view(&json!({
        "id": "msg-secure-001",
        "sender_did": "did:sender",
        "receiver_did": "did:receiver",
        "content": "not-object"
    }))
    .expect_err("content object required");
    assert_eq!(err.to_string(), "content is not a direct-e2ee object");

    let err = secure_notification_from_message_view(&json!({
        "id": "msg-secure-001",
        "sender_did": "did:sender",
        "content": {}
    }))
    .expect_err("required ids");
    assert_eq!(err.to_string(), "missing sender_did/receiver_did/id");

    let no_server_seq = secure_notification_from_message_view(&json!({
        "id": "msg-secure-001",
        "sender_did": "did:sender",
        "receiver_did": "did:receiver",
        "content_type": 123,
        "server_seq": null,
        "content": {}
    }))
    .expect("missing optional fields");
    assert_eq!(no_server_seq["params"]["meta"]["content_type"], "");
    assert!(no_server_seq["params"].get("server_seq").is_none());
}

#[test]
fn plaintext_body_to_notification_body_copies_go_whitelist_only() {
    let plaintext = json!({
        "conversation_id": "",
        "reply_to_message_id": "reply-001",
        "annotations": {"priority": "high"},
        "text": "hello secure listener",
        "payload": "",
        "payload_b64u": "YWJj",
        "application_content_type": "text/plain",
        "ignored": "value"
    });

    let body = plaintext_body_to_notification_body(&plaintext);

    assert_eq!(body.get("conversation_id"), Some(&json!("")));
    assert_eq!(body.get("reply_to_message_id"), Some(&json!("reply-001")));
    assert_eq!(body.get("annotations"), Some(&json!({"priority": "high"})));
    assert_eq!(body.get("text"), Some(&json!("hello secure listener")));
    assert_eq!(body.get("payload"), Some(&json!("")));
    assert_eq!(body.get("payload_b64u"), Some(&json!("YWJj")));
    assert!(!body.contains_key("application_content_type"));
    assert!(!body.contains_key("ignored"));
}

#[test]
fn plaintext_body_to_notification_body_preserves_go_empty_and_nil_boundaries() {
    let plaintext = json!({
        "conversation_id": null,
        "reply_to_message_id": null,
        "annotations": null,
        "text": "",
        "payload": null,
        "payload_b64u": "",
        "payload_extra": {"ignored": true}
    });
    let body = plaintext_body_to_notification_body(&plaintext);
    assert!(body.is_empty());

    let non_string_text = plaintext_body_to_notification_body(&json!({
        "text": 7,
        "payload_b64u": 8,
        "payload": {"kind": "custom"}
    }));
    assert!(!non_string_text.contains_key("text"));
    assert!(!non_string_text.contains_key("payload_b64u"));
    assert_eq!(
        non_string_text.get("payload"),
        Some(&json!({"kind": "custom"}))
    );
}
