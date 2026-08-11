use serde_json::{json, Value};

use super::agent_message::MessagePayloadProjection;
use super::*;

fn valid_payload() -> Value {
    json!({
        "schema": AGENT_MESSAGE_SCHEMA_V1,
        "event_id": "event-001",
        "task_name": "Release verification",
        "kind": "task_result",
        "level": "urgent",
        "content": {
            "summary": "Build completed",
            "detail": "All focused checks passed."
        },
        "action": { "type": "open_conversation" }
    })
}

fn request(payload: Value) -> SendMessageRequest {
    SendMessageRequest {
        target: MessageTarget::Direct(crate::ids::PeerRef::parse("did:example:bob", "").unwrap()),
        body: MessageBody::Payload { payload },
        security: MessageSecurityMode::DefaultPlain,
        client_message_id: Some(crate::ids::MessageId::parse("msg-event-001").unwrap()),
        delivery: MessageDeliveryOptions {
            idempotency_key: Some("idem-event-001".to_owned()),
            wait_for_final_acceptance: false,
        },
        delegated_signing: None,
    }
}

#[test]
fn valid_v1_projects_closed_typed_fields_and_canonical_payload() {
    let projection = project_agent_message_payload(&valid_payload()).unwrap();
    let AgentMessageProjection::Valid(message) = projection else {
        panic!("valid payload projected invalid");
    };
    assert_eq!(message.event_id, "event-001");
    assert_eq!(message.task_name, "Release verification");
    assert_eq!(message.kind, AgentMessageKind::TaskResult);
    assert_eq!(message.requested_level, AgentMessageRequestedLevel::Urgent);
    assert_eq!(message.summary, "Build completed");
    assert_eq!(
        message.detail.as_deref(),
        Some("All focused checks passed.")
    );
    assert_eq!(message.action, AgentMessageAction::OpenConversation);
    assert_eq!(message.to_payload_value(), valid_payload());
}

#[test]
fn exact_schema_is_classified_before_broad_awiki_control_for_valid_and_invalid() {
    assert_eq!(
        classify_message_payload_for_projection(
            "application/json",
            &valid_payload().to_string(),
            "did:wba:awiki.info:agent:daemon:runtime"
        ),
        MessagePayloadProjection::VisibleValid
    );
    assert_eq!(
        classify_message_payload_for_projection(
            "application/json",
            r#"{"schema":"awiki.agent.message.v1","command_id":"daemon-control-shape"}"#,
            "did:wba:awiki.info:agent:daemon:runtime"
        ),
        MessagePayloadProjection::VisibleInvalid
    );
    for payload in [
        r#"{"schema":"awiki.agent.status.v1"}"#,
        r#"{"schema":"awiki.future.control.v9"}"#,
        r#"{"schema":" awiki.agent.message.v1 "}"#,
    ] {
        assert_eq!(
            classify_message_payload_for_projection("application/json", payload, "did:example:a"),
            MessagePayloadProjection::Control
        );
    }
}

#[test]
fn transport_protected_direct_scope_forces_other_contexts_to_generic_invalid() {
    assert!(matches!(
        project_agent_message_payload_for_scope(
            &valid_payload(),
            AgentMessageProjectionScope::DirectTransportProtected
        ),
        Some(AgentMessageProjection::Valid(_))
    ));
    assert_eq!(
        project_agent_message_payload_for_scope(
            &valid_payload(),
            AgentMessageProjectionScope::Unsupported
        ),
        Some(AgentMessageProjection::Invalid)
    );
}

#[test]
fn decoder_rejects_null_extra_unknown_and_open_ended_values() {
    let mutations = [
        json!({
            "schema": AGENT_MESSAGE_SCHEMA_V1,
            "event_id": "event-001",
            "kind": "message",
            "level": "normal",
            "content": {"summary": "hello"},
            "action": {"type": "open_conversation"}
        }),
        json!({
            "schema": AGENT_MESSAGE_SCHEMA_V1,
            "event_id": "event-001",
            "task_name": null,
            "kind": "message",
            "level": "normal",
            "content": {"summary": "hello"},
            "action": {"type": "open_conversation"}
        }),
        json!({
            "schema": AGENT_MESSAGE_SCHEMA_V1,
            "event_id": "event-001",
            "task_name": "Task",
            "agent_name": "Payload Agent",
            "kind": "message",
            "level": "normal",
            "content": {"summary": "hello"},
            "action": {"type": "open_conversation"}
        }),
        json!({
            "schema": AGENT_MESSAGE_SCHEMA_V1,
            "event_id": "event-001",
            "task_name": "Task",
            "kind": "message",
            "level": "normal",
            "content": {"summary": "hello", "detail": null},
            "action": {"type": "open_conversation"}
        }),
        json!({
            "schema": AGENT_MESSAGE_SCHEMA_V1,
            "event_id": "event-001",
            "task_name": "Task",
            "kind": "message",
            "level": "normal",
            "content": {"summary": "hello", "extra": true},
            "action": {"type": "open_conversation"}
        }),
        json!({
            "schema": AGENT_MESSAGE_SCHEMA_V1,
            "event_id": "event-001",
            "task_name": "Task",
            "kind": "future_kind",
            "level": "normal",
            "content": {"summary": "hello"},
            "action": {"type": "open_conversation"}
        }),
        json!({
            "schema": AGENT_MESSAGE_SCHEMA_V1,
            "event_id": "event-001",
            "task_name": "Task",
            "kind": "message",
            "level": "critical",
            "content": {"summary": "hello"},
            "action": {"type": "open_conversation"}
        }),
        json!({
            "schema": AGENT_MESSAGE_SCHEMA_V1,
            "event_id": "event-001",
            "task_name": "Task",
            "kind": "message",
            "level": "normal",
            "content": {"summary": "hello"},
            "action": {"type": "open_url"}
        }),
        json!({
            "schema": AGENT_MESSAGE_SCHEMA_V1,
            "event_id": "event-001",
            "task_name": "Task",
            "kind": "message",
            "level": "normal",
            "content": {"summary": "hello"},
            "action": {"type": "open_conversation"},
            "title": "untrusted"
        }),
    ];
    for payload in mutations {
        assert_eq!(
            project_agent_message_payload(&payload),
            Some(AgentMessageProjection::Invalid),
            "payload should fail closed: {payload}"
        );
    }
}

#[test]
fn decoder_enforces_text_and_event_boundaries() {
    let mut payload = valid_payload();
    payload["event_id"] = json!("a2345678");
    payload["task_name"] = json!("t".repeat(AGENT_MESSAGE_V1_MAX_TASK_NAME_CHARS));
    payload["content"]["summary"] = json!("s".repeat(AGENT_MESSAGE_V1_MAX_SUMMARY_CHARS));
    payload["content"]["detail"] = json!("d".repeat(AGENT_MESSAGE_V1_MAX_DETAIL_CHARS));
    assert!(matches!(
        project_agent_message_payload(&payload),
        Some(AgentMessageProjection::Valid(_))
    ));

    payload["event_id"] = json!("a".repeat(160));
    assert!(matches!(
        project_agent_message_payload(&payload),
        Some(AgentMessageProjection::Valid(_))
    ));
    payload["event_id"] = json!("a".repeat(161));
    assert_eq!(
        project_agent_message_payload(&payload),
        Some(AgentMessageProjection::Invalid)
    );

    payload = valid_payload();
    payload["task_name"] = json!("t".repeat(AGENT_MESSAGE_V1_MAX_TASK_NAME_CHARS + 1));
    assert_eq!(
        project_agent_message_payload(&payload),
        Some(AgentMessageProjection::Invalid)
    );

    for unsafe_task_name in [
        " Task",
        "Task\nname",
        "hidden\u{200b}task",
        "token=redacted",
    ] {
        let mut payload = valid_payload();
        payload["task_name"] = json!(unsafe_task_name);
        assert_eq!(
            project_agent_message_payload(&payload),
            Some(AgentMessageProjection::Invalid),
            "unsafe task_name should fail closed: {unsafe_task_name:?}"
        );
    }

    payload = valid_payload();
    payload["content"]["summary"] = json!("s".repeat(AGENT_MESSAGE_V1_MAX_SUMMARY_CHARS + 1));
    assert_eq!(
        project_agent_message_payload(&payload),
        Some(AgentMessageProjection::Invalid)
    );
    payload = valid_payload();
    payload["content"]["detail"] = json!("d".repeat(AGENT_MESSAGE_V1_MAX_DETAIL_CHARS + 1));
    assert_eq!(
        project_agent_message_payload(&payload),
        Some(AgentMessageProjection::Invalid)
    );

    payload = valid_payload();
    payload["padding"] = json!("x".repeat(AGENT_MESSAGE_V1_MAX_COMPACT_BYTES));
    assert_eq!(
        project_agent_message_payload(&payload),
        Some(AgentMessageProjection::Invalid)
    );
}

#[test]
fn decoder_rejects_invisible_controls_secrets_paths_and_object_urls() {
    for unsafe_text in [
        "hidden\u{200b}text",
        "hidden\u{2060}text",
        "hidden\u{feff}text",
        "blob:https://example.invalid/id",
        "file:///etc/passwd",
        "/Users/example/private.txt",
        "authorization: Bearer redacted",
        "token=redacted",
        "```sh",
    ] {
        let mut payload = valid_payload();
        payload["content"]["summary"] = json!(unsafe_text);
        assert_eq!(
            project_agent_message_payload(&payload),
            Some(AgentMessageProjection::Invalid),
            "unsafe text should fail closed: {unsafe_text:?}"
        );
    }
}

#[test]
fn invalid_visible_sanitizer_never_returns_raw_payload_text() {
    let raw = json!({
        "schema": AGENT_MESSAGE_SCHEMA_V1,
        "event_id": "event-001",
        "kind": "alert",
        "level": "urgent",
        "content": {"summary": "token=must-not-cross-boundary"},
        "action": {"type": "open_conversation"},
        "raw": {"path": "/private/secret"}
    });
    let sanitized = sanitize_projected_json_payload(raw);
    assert_eq!(sanitized, json!({"schema": AGENT_MESSAGE_SCHEMA_V1}));
    assert!(!sanitized.to_string().contains("token"));
    assert!(!sanitized.to_string().contains("private"));
    assert_eq!(
        project_agent_message_payload(&sanitized),
        Some(AgentMessageProjection::Invalid)
    );
}

#[test]
fn send_preflight_is_local_only_and_defers_receiver_capability_to_receiving_home() {
    assert_eq!(
        validate_agent_message_send_request(&request(valid_payload())),
        Ok(())
    );

    let mut missing_idempotency = request(valid_payload());
    missing_idempotency.delivery.idempotency_key = None;
    assert!(matches!(
        validate_agent_message_send_request(&missing_idempotency),
        Err(crate::ImError::InvalidInput { field: Some(field), .. })
            if field == "idempotency_key"
    ));

    let mut group = request(valid_payload());
    group.target = MessageTarget::Group(crate::ids::GroupRef::parse("did:example:group").unwrap());
    assert!(matches!(
        validate_agent_message_send_request(&group),
        Err(crate::ImError::UnsupportedCapability { capability })
            if capability == "agent_message_direct_only"
    ));

    let mut e2ee = request(valid_payload());
    e2ee.security = MessageSecurityMode::SecureDirect;
    assert!(matches!(
        validate_agent_message_send_request(&e2ee),
        Err(crate::ImError::UnsupportedCapability { capability })
            if capability == "agent_message_transport_protected_only"
    ));

    let invalid = json!({"schema": AGENT_MESSAGE_SCHEMA_V1});
    assert!(matches!(
        validate_agent_message_send_request(&request(invalid)),
        Err(crate::ImError::InvalidInput { field: Some(field), .. }) if field == "payload"
    ));

    let ordinary = json!({"schema": "example.message.v1", "text": "hello"});
    assert_eq!(
        validate_agent_message_send_request(&request(ordinary)),
        Ok(())
    );
}
