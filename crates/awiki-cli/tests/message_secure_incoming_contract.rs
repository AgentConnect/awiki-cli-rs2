use awiki_cli::message::{
    apply_direct_e2ee_processing_result, direct_e2ee_notification_from_message_view,
    direct_init_session_id_from_message, filter_displayable_direct_e2ee_messages,
    is_direct_e2ee_control_or_undisplayable, is_direct_e2ee_wire_content_type,
    maybe_decrypt_direct_e2ee_messages_with_processor,
    maybe_decrypt_direct_e2ee_messages_with_processor_and_side_effects,
    polling_direct_init_ack_request, polling_secure_ack_flush_peer,
};
use serde_json::{json, Map, Value};

#[test]
fn direct_secure_wire_content_type_matches_go_exact_values() {
    assert!(is_direct_e2ee_wire_content_type(
        "application/anp-direct-init+json"
    ));
    assert!(is_direct_e2ee_wire_content_type(
        "application/anp-direct-cipher+json"
    ));
    assert!(!is_direct_e2ee_wire_content_type("text/plain"));
    assert!(!is_direct_e2ee_wire_content_type(""));
}

#[test]
fn direct_secure_notification_from_message_view_matches_go_shape() {
    let notification = direct_e2ee_notification_from_message_view(&json!({
        "id": "msg-secure-1",
        "sender_did": "did:alice",
        "receiver_did": "did:bob",
        "content_type": "application/anp-direct-cipher+json",
        "content": {"session_id": "sid-1"},
        "server_seq": 42,
    }))
    .expect("notification");

    assert_eq!(notification["meta"]["sender_did"], "did:alice");
    assert_eq!(
        notification["meta"]["target"],
        json!({"kind": "agent", "did": "did:bob"})
    );
    assert_eq!(notification["meta"]["message_id"], "msg-secure-1");
    assert_eq!(notification["meta"]["profile"], "anp.direct.e2ee.v1");
    assert_eq!(notification["meta"]["security_profile"], "direct-e2ee");
    assert_eq!(
        notification["meta"]["content_type"],
        "application/anp-direct-cipher+json"
    );
    assert_eq!(notification["body"]["session_id"], "sid-1");
    assert_eq!(notification["server_seq"], 42);
}

#[test]
fn direct_secure_notification_errors_match_go_boundaries() {
    let missing_ids = direct_e2ee_notification_from_message_view(&json!({
        "id": "msg-secure-1",
        "sender_did": "did:alice",
        "content_type": "application/anp-direct-cipher+json",
        "content": {"session_id": "sid-1"},
    }))
    .expect_err("missing receiver should fail");
    assert_eq!(missing_ids, "missing sender_did/receiver_did/id");

    let bad_content = direct_e2ee_notification_from_message_view(&json!({
        "id": "msg-secure-1",
        "sender_did": "did:alice",
        "receiver_did": "did:bob",
        "content_type": "application/anp-direct-cipher+json",
        "content": "not-json",
    }))
    .expect_err("bad content should fail");
    assert_eq!(bad_content, "content is not a direct-e2ee object");
}

#[test]
fn apply_direct_secure_processing_result_rewrites_decrypted_text_json_binary_and_control() {
    let mut text = message(
        "msg-text",
        "application/anp-direct-cipher+json",
        json!({}),
        1,
    );
    apply_direct_e2ee_processing_result(
        &mut text,
        &json!({
            "state": "decrypted",
            "plaintext": {
                "application_content_type": "text/plain",
                "text": "hello decrypted"
            }
        }),
    );
    assert_eq!(text["secure"], true);
    assert_eq!(text["decryption_state"], "decrypted");
    assert_eq!(text["content_type"], "text/plain");
    assert_eq!(text["content"], "hello decrypted");
    assert_eq!(text["type"], "text");

    let mut json_msg = message(
        "msg-json",
        "application/anp-direct-cipher+json",
        json!({}),
        2,
    );
    apply_direct_e2ee_processing_result(
        &mut json_msg,
        &json!({
            "state": "decrypted",
            "plaintext": {
                "application_content_type": "application/json",
                "payload": {"event": "wave"}
            }
        }),
    );
    assert_eq!(json_msg["content"], json!({"event": "wave"}));
    assert_eq!(json_msg["type"], "json");

    let mut binary = message(
        "msg-bin",
        "application/anp-direct-cipher+json",
        json!({}),
        3,
    );
    apply_direct_e2ee_processing_result(
        &mut binary,
        &json!({
            "state": "decrypted",
            "plaintext": {
                "application_content_type": "application/octet-stream",
                "payload_b64u": "AAEC"
            }
        }),
    );
    assert_eq!(binary["content"], "AAEC");
    assert_eq!(binary["type"], "binary");

    let mut control = message(
        "msg-ack",
        "application/anp-direct-cipher+json",
        json!({}),
        4,
    );
    apply_direct_e2ee_processing_result(
        &mut control,
        &json!({
            "state": "decrypted",
            "plaintext": {
                "application_content_type": "application/json",
                "payload": {
                    "system_type": "awiki.direct.secure_ack.v1",
                    "session_id": "sid-1"
                }
            }
        }),
    );
    assert_eq!(control["secure_control"], true);
    assert_eq!(control["type"], "secure_control");
    assert_eq!(control["content"], "");

    let mut control_string_payload = message(
        "msg-ack-string",
        "application/anp-direct-cipher+json",
        json!({}),
        5,
    );
    apply_direct_e2ee_processing_result(
        &mut control_string_payload,
        &json!({
            "state": "decrypted",
            "plaintext": {
                "application_content_type": "application/json",
                "payload": r#"{"system_type":"awiki.direct.secure_ack.v1","session_id":"sid-1"}"#
            }
        }),
    );
    assert_eq!(control_string_payload["secure_control"], true);
    assert_eq!(control_string_payload["type"], "secure_control");
}

#[test]
fn apply_direct_secure_processing_result_keeps_pending_and_undecryptable_undisplayable() {
    let mut pending = message(
        "msg-pending",
        "application/anp-direct-cipher+json",
        json!({}),
        1,
    );
    apply_direct_e2ee_processing_result(&mut pending, &json!({"state": "pending"}));
    assert_eq!(pending["secure"], true);
    assert_eq!(pending["decryption_state"], "pending");
    assert!(!is_direct_e2ee_control_or_undisplayable(&pending));

    let mut undecryptable = message(
        "msg-bad",
        "application/anp-direct-cipher+json",
        json!({}),
        2,
    );
    apply_direct_e2ee_processing_result(&mut undecryptable, &json!({"state": "undecryptable"}));
    assert!(is_direct_e2ee_control_or_undisplayable(&undecryptable));

    let displayable = filter_displayable_direct_e2ee_messages(vec![pending, undecryptable]);
    assert_eq!(displayable.len(), 1);
    assert_eq!(displayable[0]["id"], "msg-pending");
}

#[test]
fn maybe_decrypt_direct_secure_messages_processes_in_go_order_and_compacts_warnings() {
    let mut messages = vec![
        message(
            "msg-late",
            "application/anp-direct-cipher+json",
            json!({"late": true}),
            3,
        ),
        message(
            "msg-b",
            "application/anp-direct-cipher+json",
            json!({"b": true}),
            1,
        ),
        message(
            "msg-a",
            "application/anp-direct-cipher+json",
            json!({"a": true}),
            1,
        ),
        message("msg-plain", "text/plain", json!({}), 2),
    ];
    let mut seen = Vec::new();
    let warnings =
        maybe_decrypt_direct_e2ee_messages_with_processor(&mut messages, |notification| {
            let id = notification["meta"]["message_id"]
                .as_str()
                .expect("message id")
                .to_string();
            seen.push(id.clone());
            if id == "msg-b" {
                return Err("decrypt failed".to_string());
            }
            Ok(Map::from_iter([
                ("state".to_string(), Value::String("decrypted".to_string())),
                (
                    "plaintext".to_string(),
                    json!({
                        "application_content_type": "text/plain",
                        "text": format!("plain-{id}")
                    }),
                ),
            ]))
        });

    assert_eq!(seen, vec!["msg-a", "msg-b", "msg-late"]);
    assert_eq!(
        warnings,
        vec!["Failed to decrypt secure direct message msg-b: decrypt failed"]
    );
    assert_eq!(messages[2]["content"], "plain-msg-a");
    assert_eq!(messages[0]["content"], "plain-msg-late");
    assert_eq!(
        messages[1]["content_type"],
        "application/anp-direct-cipher+json"
    );
}

#[test]
fn maybe_decrypt_direct_secure_messages_treats_string_server_seq_like_go_zero() {
    let mut messages = vec![
        message(
            "msg-numeric",
            "application/anp-direct-cipher+json",
            json!({"numeric": true}),
            2,
        ),
        json!({
            "id": "msg-string",
            "sender_did": "did:alice",
            "receiver_did": "did:bob",
            "content_type": "application/anp-direct-cipher+json",
            "content": {"string": true},
            "server_seq": "1",
        }),
    ];
    let mut seen = Vec::new();
    let warnings =
        maybe_decrypt_direct_e2ee_messages_with_processor(&mut messages, |notification| {
            seen.push(
                notification["meta"]["message_id"]
                    .as_str()
                    .expect("message id")
                    .to_string(),
            );
            Ok(Map::from_iter([(
                "state".to_string(),
                Value::String("pending".to_string()),
            )]))
        });

    assert!(warnings.is_empty());
    assert_eq!(seen, vec!["msg-numeric", "msg-string"]);
    assert!(direct_e2ee_notification_from_message_view(&messages[1])
        .expect("notification")
        .get("server_seq")
        .is_none());
}

#[test]
fn maybe_decrypt_direct_secure_messages_suppresses_control_message_warnings_like_go() {
    let mut messages = vec![
        message(
            "secure-init-1",
            "application/anp-direct-cipher+json",
            json!("bad"),
            1,
        ),
        message(
            "ack-1",
            "application/anp-direct-cipher+json",
            json!("bad"),
            2,
        ),
        message(
            "msg-user",
            "application/anp-direct-cipher+json",
            json!("bad"),
            3,
        ),
    ];
    let warnings =
        maybe_decrypt_direct_e2ee_messages_with_processor(&mut messages, |_notification| {
            panic!("bad content must skip before process")
        });

    assert_eq!(
        warnings,
        vec!["Skipped secure direct message msg-user: content is not a direct-e2ee object"]
    );
}

#[test]
fn direct_init_session_id_from_message_matches_go_map_from_any_boundaries() {
    assert_eq!(
        direct_init_session_id_from_message(&message(
            "msg-init",
            "application/anp-direct-init+json",
            json!({"session_id": " session-1 "}),
            1,
        )),
        " session-1 "
    );
    assert_eq!(
        direct_init_session_id_from_message(&message(
            "msg-init",
            "application/anp-direct-init+json",
            json!(r#"{"session_id":"session-from-json-string"}"#),
            1,
        )),
        "session-from-json-string"
    );
    for content in [
        json!(""),
        json!("not-json"),
        json!([]),
        json!({}),
        json!({"session_id": 123}),
    ] {
        assert_eq!(
            direct_init_session_id_from_message(&message(
                "msg-init",
                "application/anp-direct-init+json",
                content,
                1,
            )),
            ""
        );
    }
}

#[test]
fn polling_secure_ack_flush_peer_matches_go_conditions() {
    let ack_result = Map::from_iter([
        ("state".to_string(), json!("decrypted")),
        (
            "plaintext".to_string(),
            json!({
                "application_content_type": "application/json",
                "payload": r#"{"system_type":"awiki.direct.secure_ack.v1","session_id":"sid-1"}"#
            }),
        ),
    ]);
    assert_eq!(
        polling_secure_ack_flush_peer(
            "did:bob",
            &message("ack-1", "application/anp-direct-cipher+json", json!({}), 1),
            &ack_result
        ),
        Some("did:alice".to_string())
    );

    let mut not_decrypted = ack_result.clone();
    not_decrypted.insert("state".to_string(), json!("pending"));
    assert_eq!(
        polling_secure_ack_flush_peer(
            "did:bob",
            &message("ack-1", "application/anp-direct-cipher+json", json!({}), 1),
            &not_decrypted,
        ),
        None
    );
    let self_sender = json!({
        "id": "ack-1",
        "sender_did": "did:bob",
        "receiver_did": "did:bob",
        "content_type": "application/anp-direct-cipher+json",
        "content": {},
    });
    assert_eq!(
        polling_secure_ack_flush_peer("did:bob", &self_sender, &ack_result),
        None
    );
    let non_ack = Map::from_iter([
        ("state".to_string(), json!("decrypted")),
        (
            "plaintext".to_string(),
            json!({
                "application_content_type": "application/json",
                "payload": {"system_type": "awiki.direct.secure_init.v1"}
            }),
        ),
    ]);
    assert_eq!(
        polling_secure_ack_flush_peer(
            "did:bob",
            &message(
                "msg-init",
                "application/anp-direct-init+json",
                json!({"session_id": "sid-1"}),
                1,
            ),
            &non_ack,
        ),
        None
    );
}

#[test]
fn polling_direct_init_ack_request_matches_go_conditions_and_payload() {
    let decrypted = Map::from_iter([("state".to_string(), json!("decrypted"))]);
    let request = polling_direct_init_ack_request(
        "did:bob",
        &message(
            "secure-init-1",
            "application/anp-direct-init+json",
            json!({"session_id": "sid-1"}),
            1,
        ),
        &decrypted,
    )
    .expect("ack request");

    assert_eq!(request.peer_did, "did:alice");
    assert_eq!(request.session_id, "sid-1");
    assert_eq!(request.message_id, "secure-init-1");
    assert_eq!(request.ack_id, "ack-sid-1");
    assert_eq!(
        Value::Object(request.payload),
        json!({
            "system_type": "awiki.direct.secure_ack.v1",
            "session_id": "sid-1",
            "acked_message_id": "secure-init-1"
        })
    );

    let mut pending = decrypted.clone();
    pending.insert("state".to_string(), json!("pending"));
    assert!(polling_direct_init_ack_request(
        "did:bob",
        &message(
            "secure-init-1",
            "application/anp-direct-init+json",
            json!({"session_id": "sid-1"}),
            1,
        ),
        &pending,
    )
    .is_none());
    assert!(polling_direct_init_ack_request(
        "did:bob",
        &message(
            "secure-init-1",
            "application/anp-direct-cipher+json",
            json!({"session_id": "sid-1"}),
            1,
        ),
        &decrypted,
    )
    .is_none());
    assert!(polling_direct_init_ack_request(
        "did:bob",
        &message(
            "secure-init-1",
            "application/anp-direct-init+json",
            json!({}),
            1,
        ),
        &decrypted,
    )
    .is_none());
    assert!(polling_direct_init_ack_request(
        "did:alice",
        &message(
            "secure-init-1",
            "application/anp-direct-init+json",
            json!({"session_id": "sid-1"}),
            1,
        ),
        &decrypted,
    )
    .is_none());
}

#[test]
fn maybe_decrypt_direct_secure_messages_runs_side_effects_before_applying_result() {
    let mut messages = vec![message(
        "msg-init",
        "application/anp-direct-init+json",
        json!({"session_id": "sid-1"}),
        1,
    )];
    let warnings = maybe_decrypt_direct_e2ee_messages_with_processor_and_side_effects(
        &mut messages,
        |_notification| {
            Ok(Map::from_iter([
                ("state".to_string(), json!("decrypted")),
                (
                    "plaintext".to_string(),
                    json!({
                        "application_content_type": "text/plain",
                        "text": "plain-after-side-effect"
                    }),
                ),
            ]))
        },
        |message, result| {
            assert_eq!(message["content_type"], "application/anp-direct-init+json");
            assert_eq!(message["content"], json!({"session_id": "sid-1"}));
            assert_eq!(result["state"], "decrypted");
            vec![
                "side-effect warning".to_string(),
                "side-effect warning".to_string(),
            ]
        },
    );

    assert_eq!(warnings, vec!["side-effect warning"]);
    assert_eq!(messages[0]["content_type"], "text/plain");
    assert_eq!(messages[0]["content"], "plain-after-side-effect");
    assert_eq!(messages[0]["decryption_state"], "decrypted");
}

fn message(id: &str, content_type: &str, content: Value, server_seq: i64) -> Value {
    json!({
        "id": id,
        "sender_did": "did:alice",
        "receiver_did": "did:bob",
        "content_type": content_type,
        "content": content,
        "server_seq": server_seq,
        "sent_at": "2026-05-16T00:00:00Z",
        "is_read": false,
    })
}
