use awiki_cli::host_runtime::host_notify::{
    apply_host_notification_handles, normalize_host_notification, HostNotificationData,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use time::{Date, Month, OffsetDateTime, Time, UtcOffset};

#[test]
fn host_notify_direct_incoming_keeps_minimal_fields_like_go() {
    let notification = json!({
        "jsonrpc": "2.0",
        "method": "direct.incoming",
        "params": {
            "meta": {
                "profile": "anp.direct.base.v1",
                "security_profile": "transport-protected",
                "sender_did": "did:wba:a.example:agents:alice:e1_alice",
                "operation_id": "op-direct-text-001",
                "message_id": "msg-direct-text-001",
                "created_at": "2026-03-31T10:01:00Z",
                "content_type": "text/plain",
                "target": {
                    "kind": "agent",
                    "did": "did:wba:b.example:agents:bob:e1_bob"
                }
            },
            "auth": {
                "scheme": "anp-rfc9421-origin-proof-v1"
            },
            "body": {
                "conversation_id": "conv-alice-bob",
                "text": "你好，Bob。"
            },
            "server": {
                "event_id": "evt-server-only"
            }
        }
    });

    let event = normalize_host_notification(&notification, Some(fixed_received_at()))
        .expect("direct event");

    assert_eq!(event.version, "1.0");
    assert_eq!(event.id, "msg-direct-text-001");
    assert_eq!(event.topic, "im.message.received");
    assert_eq!(event.received_at, "2026-04-12T10:30:00Z");
    let HostNotificationData::Direct(data) = event.data.as_ref().expect("direct data") else {
        panic!("expected direct host notification data");
    };
    assert_eq!(data.channel, "direct");
    assert_eq!(data.source_kind, "im");
    assert_eq!(data.conversation_id, "conv-alice-bob");
    assert_eq!(data.sender_did, "did:wba:a.example:agents:alice:e1_alice");
    assert_eq!(data.recipient_did, "did:wba:b.example:agents:bob:e1_bob");
    assert_eq!(data.content_type, "text/plain");
    assert_eq!(data.text, "你好，Bob。");

    let raw = serde_json::to_value(&event).expect("event json");
    assert!(raw.pointer("/data/auth").is_none());
    assert!(raw.pointer("/data/server").is_none());
    assert!(raw.pointer("/data/origin_proof").is_none());
}

#[test]
fn host_notify_direct_id_fallbacks_match_go() {
    let operation = json!({
        "method": "direct.incoming",
        "params": {
            "meta": {
                "sender_did": "did:sender",
                "operation_id": "op-1",
                "target": {"did": "did:recipient"}
            },
            "body": {}
        }
    });
    let event =
        normalize_host_notification(&operation, Some(fixed_received_at())).expect("operation id");
    assert_eq!(event.id, "op-1");

    let generated = json!({
        "method": "direct.incoming",
        "params": {
            "meta": {
                "sender_did": "did:sender",
                "target": {"did": "did:recipient"}
            },
            "body": {}
        }
    });
    let event =
        normalize_host_notification(&generated, Some(fixed_received_at())).expect("generated id");
    assert_eq!(event.id, generated_host_id(&generated));
}

#[test]
fn host_notify_mail_notification_builds_mail_event_like_go() {
    let notification = json!({
        "jsonrpc": "2.0",
        "method": "mail.notification",
        "params": {
            "mailbox_did": "did:wba:example.com:user:alice:e1_alice",
            "mailbox_address": "alice@example.com",
            "from_addr": "sender@example.com",
            "subject": "Mail Subject",
            "preview": "First 200 chars of the body...",
            "has_attachments": true,
            "message_id": "mail-msg-001"
        }
    });

    let event =
        normalize_host_notification(&notification, Some(fixed_received_at())).expect("mail event");

    assert_eq!(event.id, "mail-msg-001");
    assert_eq!(event.topic, "im.message.received");
    let HostNotificationData::Direct(data) = event.data.expect("mail data") else {
        panic!("expected direct host notification data for mail");
    };
    assert_eq!(data.channel, "mail");
    assert_eq!(data.source_kind, "mail");
    assert_eq!(
        data.recipient_did,
        "did:wba:example.com:user:alice:e1_alice"
    );
    assert_eq!(data.content_type, "mail.notification");
    assert_eq!(data.text, "First 200 chars of the body...");
    assert_eq!(data.mailbox_address, "alice@example.com");
    assert_eq!(data.mailbox_did, "did:wba:example.com:user:alice:e1_alice");
    assert_eq!(data.from_addr, "sender@example.com");
    assert_eq!(data.subject, "Mail Subject");
    assert_eq!(data.preview, "First 200 chars of the body...");
    assert!(data.has_attachments);
}

#[test]
fn host_notify_mail_text_and_id_fallbacks_match_go() {
    let cases = [
        (" Preview text ", false, "Subject", "Preview text"),
        ("", false, " Mail Subject ", "[邮件] Mail Subject"),
        ("", true, "", "[邮件] 收到一封包含附件的邮件"),
        ("", false, "", "[邮件] 收到一封新邮件"),
    ];

    for (preview, has_attachments, subject, expected_text) in cases {
        let notification = json!({
            "method": "mail.notification",
            "params": {
                "mailbox_did": "did:mailbox",
                "subject": subject,
                "preview": preview,
                "has_attachments": has_attachments
            }
        });
        let event = normalize_host_notification(&notification, Some(fixed_received_at()))
            .expect("mail fallback event");
        assert_eq!(event.id, generated_host_id(&notification));
        let HostNotificationData::Direct(data) = event.data.expect("mail data") else {
            panic!("expected direct host notification data for mail");
        };
        assert_eq!(data.text, expected_text);
    }
}

#[test]
fn host_notify_group_incoming_omits_payload_and_stringifies_seq_like_go() {
    let notification = json!({
        "jsonrpc": "2.0",
        "method": "group.incoming",
        "params": {
            "meta": {
                "profile": "anp.group.base.v1",
                "security_profile": "transport-protected",
                "sender_did": "did:wba:a.example:agents:alice:e1_alice",
                "operation_id": "op-group-send-001",
                "content_type": "application/json",
                "target": {
                    "kind": "agent",
                    "did": "did:wba:b.example:agents:bob:e1_bob"
                }
            },
            "body": {
                "group_did": "did:wba:groups.example:groups:demo:e1_group",
                "group_state_version": 4,
                "group_event_seq": 5,
                "accepted_at": "2026-04-07T09:11:01Z",
                "payload": {
                    "kind": "attachment_manifest"
                }
            }
        }
    });

    let event =
        normalize_host_notification(&notification, Some(fixed_received_at())).expect("group event");

    assert_eq!(event.id, "did:wba:groups.example:groups:demo:e1_group:5");
    assert_eq!(event.topic, "im.group.message.received");
    let HostNotificationData::Group(data) = event.data.as_ref().expect("group data") else {
        panic!("expected group host notification data");
    };
    assert_eq!(data.channel, "group");
    assert_eq!(data.group_event_seq, "5");
    assert_eq!(data.group_state_version, "4");
    assert_eq!(data.text, "");
    assert_eq!(data.accepted_at, "2026-04-07T09:11:01Z");

    let raw = serde_json::to_value(&event).expect("event json");
    assert!(raw.pointer("/data/payload").is_none());
}

#[test]
fn host_notify_group_id_fallbacks_match_go() {
    let with_message_id = group_incoming(json!("msg-1"), json!(7), "op-1");
    assert_eq!(
        normalize_host_notification(&with_message_id, Some(fixed_received_at()))
            .expect("message id")
            .id,
        "msg-1"
    );

    let with_group_seq = group_incoming(Value::Null, json!(7), "op-1");
    assert_eq!(
        normalize_host_notification(&with_group_seq, Some(fixed_received_at()))
            .expect("group seq")
            .id,
        "did:group:7"
    );

    let with_operation = group_incoming(Value::Null, Value::Null, "op-1");
    assert_eq!(
        normalize_host_notification(&with_operation, Some(fixed_received_at()))
            .expect("operation id")
            .id,
        "op-1"
    );

    let generated = group_incoming(Value::Null, Value::Null, "");
    assert_eq!(
        normalize_host_notification(&generated, Some(fixed_received_at()))
            .expect("generated id")
            .id,
        generated_host_id(&generated)
    );
}

#[test]
fn host_notify_group_state_changed_infers_event_type_like_go() {
    let event = normalize_host_notification(
        &json!({
            "jsonrpc": "2.0",
            "method": "group.state_changed",
            "params": {
                "meta": {
                    "operation_id": "op-group-remove-001",
                    "target": {
                        "kind": "agent",
                        "did": "did:wba:a.example:agents:alice:e1_alice"
                    }
                },
                "body": {
                    "group_did": "did:wba:groups.example:groups:demo:e1_group",
                    "group_state_version": "3",
                    "group_event_seq": "3",
                    "subject_method": "group.remove",
                    "subject_did": "did:wba:c.example:agents:carol:e1_carol",
                    "actor_did": "did:wba:a.example:agents:alice:e1_alice",
                    "membership_status": "removed",
                    "changed_at": "2026-04-07T09:06:01Z"
                }
            }
        }),
        Some(fixed_received_at()),
    )
    .expect("group state event");

    assert_eq!(event.topic, "im.group.state.changed");
    assert_eq!(event.id, "did:wba:groups.example:groups:demo:e1_group:3");
    assert_eq!(event.received_at, "2026-04-12T10:30:00Z");
    let HostNotificationData::GroupState(data) = event.data.expect("group state data") else {
        panic!("expected group state host notification data");
    };
    assert_eq!(data.channel, "group");
    assert_eq!(data.event_type, "member-removed");
    assert_eq!(
        data.recipient_did,
        "did:wba:a.example:agents:alice:e1_alice"
    );
    assert_eq!(data.actor_did, "did:wba:a.example:agents:alice:e1_alice");
    assert_eq!(data.subject_method, "group.remove");
    assert_eq!(data.group_event_seq, "3");
    assert_eq!(data.changed_at, "2026-04-07T09:06:01Z");
}

#[test]
fn host_notify_group_state_event_type_fallbacks_match_go() {
    let cases = [
        ("explicit", "", "custom-event"),
        ("", "active", "member-activated"),
        ("", "activated", "member-activated"),
        ("", "removed", "member-removed"),
        ("", "left", "member-left"),
        ("group.add", "", "member-activated"),
        ("group.remove", "", "member-removed"),
        ("group.leave", "", "member-left"),
        ("group.update_profile", "", "group-profile-updated"),
        ("group.update_policy", "", "group-policy-updated"),
        ("group.join", "", ""),
    ];

    for (subject_method, membership_status, expected) in cases {
        let mut notification = group_state(Value::Null, json!(7), "op-1");
        let body = notification
            .pointer_mut("/params/body")
            .and_then(Value::as_object_mut)
            .expect("body object");
        body.insert(
            "subject_method".to_string(),
            Value::String(subject_method.to_string()),
        );
        body.insert(
            "membership_status".to_string(),
            Value::String(membership_status.to_string()),
        );
        if subject_method == "explicit" {
            body.insert(
                "event_type".to_string(),
                Value::String(expected.to_string()),
            );
        }
        let event = normalize_host_notification(&notification, Some(fixed_received_at()))
            .expect("group state event");
        let HostNotificationData::GroupState(data) = event.data.expect("group state data") else {
            panic!("expected group state host notification data");
        };
        assert_eq!(
            data.event_type, expected,
            "case {subject_method}/{membership_status}"
        );
    }
}

#[test]
fn host_notify_rejects_unknown_or_missing_required_fields_like_go() {
    assert!(
        normalize_host_notification(&json!({"method": "unknown"}), Some(fixed_received_at()))
            .is_none()
    );
    assert!(normalize_host_notification(
        &json!({"method": "direct.incoming", "params": {"meta": {"target": {"did": "did:recipient"}}}}),
        Some(fixed_received_at())
    )
    .is_none());
    assert!(normalize_host_notification(
        &json!({"method": "mail.notification", "params": {"mailbox_did": ""}}),
        Some(fixed_received_at())
    )
    .is_none());
    assert!(normalize_host_notification(
        &json!({"method": "group.incoming", "params": {"meta": {"sender_did": "did:sender", "target": {"did": "did:recipient"}}, "body": {}}}),
        Some(fixed_received_at())
    )
    .is_none());
    assert!(normalize_host_notification(
        &json!({"method": "group.state_changed", "params": {"meta": {"target": {"did": "did:recipient"}}, "body": {}}}),
        Some(fixed_received_at())
    )
    .is_none());
}

#[test]
fn host_notify_apply_handles_trims_and_preserves_existing_like_go() {
    let mut direct = normalize_host_notification(
        &json!({
            "method": "direct.incoming",
            "params": {
                "meta": {
                    "sender_did": "did:sender",
                    "message_id": "msg-1",
                    "target": {"did": "did:recipient"}
                },
                "body": {}
            }
        }),
        Some(fixed_received_at()),
    )
    .expect("direct event");

    apply_host_notification_handles(&mut direct, " alice ", " bob ");
    let HostNotificationData::Direct(data) = direct.data.as_ref().expect("direct data") else {
        panic!("expected direct host notification data");
    };
    assert_eq!(data.sender_handle, "alice");
    assert_eq!(data.recipient_handle, "bob");

    apply_host_notification_handles(&mut direct, " ", " ");
    let HostNotificationData::Direct(data) = direct.data.as_ref().expect("direct data") else {
        panic!("expected direct host notification data");
    };
    assert_eq!(data.sender_handle, "alice");
    assert_eq!(data.recipient_handle, "bob");

    let mut group_state = normalize_host_notification(
        &group_state(json!("event-1"), json!(7), "op-1"),
        Some(fixed_received_at()),
    )
    .expect("group state event");
    apply_host_notification_handles(&mut group_state, "alice", "bob");
    let HostNotificationData::GroupState(data) = group_state.data.expect("group state data") else {
        panic!("expected group state host notification data");
    };
    assert_eq!(data.actor_did, "");
}

fn fixed_received_at() -> OffsetDateTime {
    OffsetDateTime::new_in_offset(
        Date::from_calendar_date(2026, Month::April, 12).expect("date"),
        Time::from_hms(10, 30, 0).expect("time"),
        UtcOffset::UTC,
    )
}

fn generated_host_id(notification: &Value) -> String {
    let raw = serde_json::to_vec(notification).expect("notification json");
    let sum = Sha256::digest(raw);
    let prefix = sum
        .iter()
        .take(8)
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("hostevt-{prefix}")
}

fn group_incoming(message_id: Value, group_event_seq: Value, operation_id: &str) -> Value {
    json!({
        "method": "group.incoming",
        "params": {
            "meta": {
                "message_id": message_id,
                "operation_id": operation_id,
                "sender_did": "did:sender",
                "target": {"did": "did:recipient"}
            },
            "body": {
                "group_did": "did:group",
                "group_event_seq": group_event_seq
            }
        }
    })
}

fn group_state(event_id: Value, group_event_seq: Value, operation_id: &str) -> Value {
    json!({
        "method": "group.state_changed",
        "params": {
            "meta": {
                "operation_id": operation_id,
                "target": {"did": "did:recipient"}
            },
            "body": {
                "event_id": event_id,
                "group_did": "did:group",
                "group_event_seq": group_event_seq
            }
        }
    })
}
