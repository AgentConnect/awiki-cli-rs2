use awiki_cli::runtime_legacy::listener_secure_normalize::{
    normalize_direct_secure_notification_plan, LocalSecureInitAckOutcome,
    NormalizeDirectSecureAction, NormalizeDirectSecureDecision, SecureProcessOutcome,
};
use serde_json::{json, Value};

#[test]
fn non_secure_notification_returns_original_without_actions() {
    let notification =
        json!({"method": "direct.incoming", "params": {"meta": {"content_type": "text/plain"}}});

    let plan = normalize_direct_secure_notification_plan(
        &notification,
        Some("did:alice"),
        true,
        true,
        SecureProcessOutcome::Result(decrypted_result(text_plaintext())),
        LocalSecureInitAckOutcome::DeliveredInProcess,
    );

    assert_eq!(plan.decision, NormalizeDirectSecureDecision::KeepOriginal);
    assert!(plan.actions.is_empty());
    assert_eq!(plan.notification, notification);
}

#[test]
fn missing_record_or_rpc_returns_original_before_client_build() {
    let notification = secure_init_notification();

    for (record, rpc) in [(None, true), (Some("did:alice"), false)] {
        let plan = normalize_direct_secure_notification_plan(
            &notification,
            record,
            rpc,
            true,
            SecureProcessOutcome::Result(decrypted_result(text_plaintext())),
            LocalSecureInitAckOutcome::DeliveredInProcess,
        );

        assert_eq!(plan.decision, NormalizeDirectSecureDecision::KeepOriginal);
        assert!(plan.actions.is_empty());
        assert_eq!(plan.notification, notification);
    }
}

#[test]
fn client_or_process_failure_returns_original_after_go_attempt_order() {
    let notification = secure_init_notification();

    let client_failure = normalize_direct_secure_notification_plan(
        &notification,
        Some("did:alice"),
        true,
        false,
        SecureProcessOutcome::Result(decrypted_result(text_plaintext())),
        LocalSecureInitAckOutcome::DeliveredInProcess,
    );
    assert_eq!(
        client_failure.actions,
        vec![NormalizeDirectSecureAction::BuildSecureE2eeClient]
    );
    assert_eq!(
        client_failure.decision,
        NormalizeDirectSecureDecision::KeepOriginal
    );

    let process_failure = normalize_direct_secure_notification_plan(
        &notification,
        Some("did:alice"),
        true,
        true,
        SecureProcessOutcome::Error,
        LocalSecureInitAckOutcome::DeliveredInProcess,
    );
    assert_eq!(
        process_failure.actions,
        vec![
            NormalizeDirectSecureAction::BuildSecureE2eeClient,
            NormalizeDirectSecureAction::ProcessIncoming,
        ]
    );
    assert_eq!(
        process_failure.decision,
        NormalizeDirectSecureDecision::KeepOriginal
    );
}

#[test]
fn non_decrypted_or_missing_plaintext_returns_original_after_process() {
    for result in [
        json!({"state": "pending-confirmation"}),
        json!({"state": "decrypted", "plaintext": "not-object"}),
    ] {
        let notification = secure_init_notification();
        let plan = normalize_direct_secure_notification_plan(
            &notification,
            Some("did:alice"),
            true,
            true,
            SecureProcessOutcome::Result(result),
            LocalSecureInitAckOutcome::DeliveredInProcess,
        );

        assert_eq!(plan.decision, NormalizeDirectSecureDecision::KeepOriginal);
        assert_eq!(
            plan.actions,
            vec![
                NormalizeDirectSecureAction::BuildSecureE2eeClient,
                NormalizeDirectSecureAction::ProcessIncoming,
            ]
        );
        assert_eq!(plan.notification, notification);
    }
}

#[test]
fn decrypted_plaintext_rewrites_body_and_secure_metadata_like_go() {
    let notification = secure_cipher_notification();
    let plan = normalize_direct_secure_notification_plan(
        &notification,
        Some("did:alice"),
        true,
        true,
        SecureProcessOutcome::Result(decrypted_result(text_plaintext())),
        LocalSecureInitAckOutcome::DeliveredInProcess,
    );

    assert_eq!(plan.decision, NormalizeDirectSecureDecision::Normalized);
    assert_eq!(
        plan.actions,
        vec![
            NormalizeDirectSecureAction::BuildSecureE2eeClient,
            NormalizeDirectSecureAction::ProcessIncoming,
        ]
    );
    assert_eq!(plan.notification["method"], "direct.incoming");
    let params = &plan.notification["params"];
    assert_eq!(params["meta"]["content_type"], "text/plain");
    assert_eq!(
        params["body"],
        json!({"text": "hello", "payload": {"k": "v"}})
    );
    assert_eq!(params["secure_state"], "decrypted");
    assert_eq!(
        params["secure_wire_content_type"],
        "application/anp-direct-cipher+json"
    );
    assert_eq!(params["secure_wire_body"], notification["params"]["body"]);
}

#[test]
fn secure_ack_plaintext_rewrites_method_and_flushes_outbox_without_init_ack() {
    let notification = secure_init_notification();
    let plan = normalize_direct_secure_notification_plan(
        &notification,
        Some("did:alice"),
        true,
        true,
        SecureProcessOutcome::Result(decrypted_result(secure_ack_plaintext())),
        LocalSecureInitAckOutcome::NetworkSendSucceeded {
            ack_result: json!({"body": {"ignored": true}}),
        },
    );

    assert_eq!(plan.decision, NormalizeDirectSecureDecision::Normalized);
    assert_eq!(plan.notification["method"], "direct.secure.ack");
    assert_eq!(
        plan.actions,
        vec![
            NormalizeDirectSecureAction::BuildSecureE2eeClient,
            NormalizeDirectSecureAction::ProcessIncoming,
            NormalizeDirectSecureAction::FlushQueuedSecureOutbox {
                peer_did: "did:bob".to_string(),
            },
        ]
    );
}

#[test]
fn secure_init_plaintext_on_cipher_wire_only_rewrites_method() {
    let notification = secure_cipher_notification();
    let plan = normalize_direct_secure_notification_plan(
        &notification,
        Some("did:alice"),
        true,
        true,
        SecureProcessOutcome::Result(decrypted_result(secure_init_plaintext())),
        LocalSecureInitAckOutcome::NetworkSendFailed,
    );

    assert_eq!(plan.notification["method"], "direct.secure.init");
    assert_eq!(
        plan.actions,
        vec![
            NormalizeDirectSecureAction::BuildSecureE2eeClient,
            NormalizeDirectSecureAction::ProcessIncoming,
        ]
    );
}

#[test]
fn secure_init_wire_with_ids_delivered_in_process_skips_network_ack_then_flushes_peer_queue() {
    let notification = secure_init_notification();
    let plan = normalize_direct_secure_notification_plan(
        &notification,
        Some("did:alice"),
        true,
        true,
        SecureProcessOutcome::Result(decrypted_result(secure_init_plaintext())),
        LocalSecureInitAckOutcome::DeliveredInProcess,
    );

    assert_eq!(plan.notification["method"], "direct.secure.init");
    assert_eq!(
        plan.actions,
        vec![
            NormalizeDirectSecureAction::BuildSecureE2eeClient,
            NormalizeDirectSecureAction::ProcessIncoming,
            NormalizeDirectSecureAction::DeliverLocalSecureAckInProcess {
                recipient_did: "did:bob".to_string(),
                session_id: "sess-1".to_string(),
                replied_message_id: "msg-init-1".to_string(),
                ack_message_id: "ack-sess-1".to_string(),
            },
            NormalizeDirectSecureAction::FlushPeerQueuedSecureOutbox {
                owner_did: "did:bob".to_string(),
                peer_did: "did:alice".to_string(),
            },
        ]
    );
}

#[test]
fn secure_init_wire_with_network_ack_success_delivers_local_ack_result() {
    let plan = normalize_direct_secure_notification_plan(
        &secure_init_notification(),
        Some("did:alice"),
        true,
        true,
        SecureProcessOutcome::Result(decrypted_result(secure_init_plaintext())),
        LocalSecureInitAckOutcome::NetworkSendSucceeded {
            ack_result: json!({"message_id": "ack-remote", "body": {"ciphertext": "abc"}}),
        },
    );

    assert_eq!(
        plan.actions[2],
        NormalizeDirectSecureAction::DeliverLocalSecureAckInProcess {
            recipient_did: "did:bob".to_string(),
            session_id: "sess-1".to_string(),
            replied_message_id: "msg-init-1".to_string(),
            ack_message_id: "ack-sess-1".to_string(),
        }
    );
    assert_eq!(
        plan.actions[3],
        NormalizeDirectSecureAction::SendSecureAckJson {
            recipient_did: "did:bob".to_string(),
            payload: im_core::secure::build_secure_ack_payload("sess-1", "msg-init-1"),
            message_id: "ack-sess-1".to_string(),
            request_id: "ack-sess-1".to_string(),
        }
    );
    assert_eq!(
        plan.actions[4],
        NormalizeDirectSecureAction::DeliverLocalSecureAck {
            sender_did: "did:alice".to_string(),
            recipient_did: "did:bob".to_string(),
            fallback_message_id: "ack-sess-1".to_string(),
            ack_result: json!({"message_id": "ack-remote", "body": {"ciphertext": "abc"}}),
        }
    );
}

#[test]
fn secure_init_wire_without_session_or_message_id_does_not_ack_or_flush_peer_queue() {
    for notification in [
        secure_init_notification_without_body_session(),
        secure_init_notification_without_message_id(),
    ] {
        let plan = normalize_direct_secure_notification_plan(
            &notification,
            Some("did:alice"),
            true,
            true,
            SecureProcessOutcome::Result(decrypted_result(secure_init_plaintext())),
            LocalSecureInitAckOutcome::NetworkSendFailed,
        );

        assert_eq!(
            plan.actions,
            vec![
                NormalizeDirectSecureAction::BuildSecureE2eeClient,
                NormalizeDirectSecureAction::ProcessIncoming,
            ]
        );
    }
}

fn decrypted_result(plaintext: Value) -> Value {
    json!({"state": "decrypted", "plaintext": plaintext})
}

fn text_plaintext() -> Value {
    json!({
        "application_content_type": "text/plain",
        "text": "hello",
        "payload": {"k": "v"},
    })
}

fn secure_ack_plaintext() -> Value {
    json!({
        "application_content_type": "application/json",
        "payload": {"system_type": "awiki.direct.secure_ack.v1", "session_id": "sess-1"},
    })
}

fn secure_init_plaintext() -> Value {
    json!({
        "application_content_type": "application/json",
        "payload": {"system_type": "awiki.direct.secure_init.v1", "reason": "manual_init"},
    })
}

fn secure_init_notification() -> Value {
    json!({
        "method": "direct.incoming",
        "params": {
            "meta": {
                "sender_did": "did:bob",
                "target": {"kind": "agent", "did": "did:alice"},
                "message_id": "msg-init-1",
                "content_type": "application/anp-direct-init+json",
            },
            "body": {"session_id": "sess-1", "ciphertext": "wire"},
        },
    })
}

fn secure_cipher_notification() -> Value {
    json!({
        "method": "direct.incoming",
        "params": {
            "meta": {
                "sender_did": "did:bob",
                "target": {"kind": "agent", "did": "did:alice"},
                "message_id": "msg-cipher-1",
                "content_type": "application/anp-direct-cipher+json",
            },
            "body": {"ciphertext": "wire"},
        },
    })
}

fn secure_init_notification_without_body_session() -> Value {
    json!({
        "method": "direct.incoming",
        "params": {
            "meta": {
                "sender_did": "did:bob",
                "message_id": "msg-init-1",
                "content_type": "application/anp-direct-init+json",
            },
            "body": {"ciphertext": "wire"},
        },
    })
}

fn secure_init_notification_without_message_id() -> Value {
    json!({
        "method": "direct.incoming",
        "params": {
            "meta": {
                "sender_did": "did:bob",
                "content_type": "application/anp-direct-init+json",
            },
            "body": {"session_id": "sess-1"},
        },
    })
}
