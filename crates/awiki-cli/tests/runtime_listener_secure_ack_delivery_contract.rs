use awiki_cli::runtime_legacy::listener_secure_ack_delivery::{
    deliver_local_secure_ack_plan, LocalSecureAckDeliveryAction, LocalSecureAckDeliveryDecision,
};
use serde_json::{json, Value};

#[test]
fn secure_ack_payload_trims_session_and_message_ids_like_go_builder() {
    assert_eq!(
        Value::Object(im_core::secure::build_secure_ack_payload(
            " session-1 ",
            " msg-9\n",
        )),
        json!({
            "system_type": "awiki.direct.secure_ack.v1",
            "session_id": "session-1",
            "acked_message_id": "msg-9",
        })
    );
}

#[test]
fn inactive_target_session_skips_without_inspecting_ack_body() {
    let plan = deliver_local_secure_ack_plan(
        false,
        "did:sender",
        "did:recipient",
        "ack-fallback",
        &json!({"body": {"ciphertext": "abc"}}),
    );

    assert_eq!(plan.decision, LocalSecureAckDeliveryDecision::Skipped);
    assert!(plan.actions.is_empty());
}

#[test]
fn missing_non_object_or_empty_ack_body_skips_like_go() {
    for ack_result in [
        json!({}),
        json!({"body": "not-object"}),
        json!({"body": []}),
        json!({"body": {}}),
    ] {
        let plan = deliver_local_secure_ack_plan(
            true,
            "did:sender",
            "did:recipient",
            "ack-fallback",
            &ack_result,
        );
        assert_eq!(plan.decision, LocalSecureAckDeliveryDecision::Skipped);
        assert!(plan.actions.is_empty());
    }
}

#[test]
fn non_string_or_blank_message_id_uses_fallback_message_id() {
    for ack_result in [
        json!({"message_id": 7, "body": {"ciphertext": "abc"}}),
        json!({"message_id": " \t\n", "body": {"ciphertext": "abc"}}),
    ] {
        let notification = delivered_notification(deliver_local_secure_ack_plan(
            true,
            "did:sender",
            "did:recipient",
            "ack-fallback",
            &ack_result,
        ));

        assert_eq!(
            notification["params"]["meta"]["message_id"],
            json!("ack-fallback")
        );
    }
}

#[test]
fn nonblank_message_id_preserves_original_untrimmed_string() {
    let notification = delivered_notification(deliver_local_secure_ack_plan(
        true,
        "did:sender",
        "did:recipient",
        "ack-fallback",
        &json!({"message_id": " msg-123 ", "body": {"ciphertext": "abc"}}),
    ));

    assert_eq!(
        notification["params"]["meta"]["message_id"],
        json!(" msg-123 ")
    );
}

#[test]
fn delivered_ack_builds_go_direct_incoming_cipher_notification() {
    let ack_result = json!({
        "message_id": "ack-session-1",
        "body": {
            "ciphertext": "abc",
            "nonce": "n-1",
        },
        "ignored": "value",
    });
    let notification = delivered_notification(deliver_local_secure_ack_plan(
        true,
        "did:sender",
        "did:recipient",
        "ack-fallback",
        &ack_result,
    ));

    assert_eq!(notification["method"], json!("direct.incoming"));
    assert_eq!(
        notification["params"]["meta"]["sender_did"],
        json!("did:sender")
    );
    assert_eq!(
        notification["params"]["meta"]["target"],
        json!({"kind": "agent", "did": "did:recipient"})
    );
    assert_eq!(
        notification["params"]["meta"]["message_id"],
        json!("ack-session-1")
    );
    assert_eq!(
        notification["params"]["meta"]["profile"],
        json!("anp.direct.e2ee.v1")
    );
    assert_eq!(
        notification["params"]["meta"]["security_profile"],
        json!("direct-e2ee")
    );
    assert_eq!(
        notification["params"]["meta"]["content_type"],
        json!("application/anp-direct-cipher+json")
    );
    assert_eq!(
        notification["params"]["body"],
        json!({"ciphertext": "abc", "nonce": "n-1"})
    );
}

fn delivered_notification(
    plan: awiki_cli::runtime_legacy::listener_secure_ack_delivery::LocalSecureAckDeliveryPlan,
) -> Value {
    assert_eq!(plan.decision, LocalSecureAckDeliveryDecision::Delivered);
    match plan.actions.as_slice() {
        [LocalSecureAckDeliveryAction::HandleNotification { notification }] => notification.clone(),
        actions => panic!("unexpected actions: {actions:?}"),
    }
}
