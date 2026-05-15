use awiki_cli::runtime::host_notify::HostNotificationData;
use awiki_cli::runtime::listener_notification_plan::{
    handle_notification_plan, IncomingContactSyncRequest, NotificationRoute,
    NotificationSessionContext, NotificationSideEffect, SecureNotificationEffect,
    SecureNotificationNormalization,
};
use serde_json::{json, Value};
use time::{Date, Month, OffsetDateTime, Time, UtcOffset};

#[test]
fn direct_incoming_plans_contact_sync_handles_store_then_dispatch() {
    let notification = direct_notification("direct.incoming", "msg-001", "text/plain");
    let mut sync_calls = Vec::new();

    let plan = handle_notification_plan(
        &notification,
        &session(" Alice.Example "),
        SecureNotificationNormalization::KeepOriginal,
        Some(fixed_received_at()),
        |request| {
            sync_calls.push(request.clone());
            Ok("Bob.Remote".to_string())
        },
    );

    assert_eq!(plan.route, NotificationRoute::DirectIncoming);
    assert_eq!(plan.secure_effect, SecureNotificationEffect::NotSecure);
    assert_eq!(
        sync_calls,
        vec![IncomingContactSyncRequest {
            owner_did: "did:alice".to_string(),
            sender_did: "did:bob".to_string(),
            source_type: "direct.incoming".to_string(),
            source_group_id: String::new(),
        }]
    );
    assert_eq!(
        effect_names(&plan.side_effects),
        vec![
            "sync_contact",
            "apply_handles",
            "store_message",
            "dispatch_host",
        ]
    );
    let NotificationSideEffect::ApplyHostNotificationHandles {
        sender_handle,
        recipient_handle,
    } = &plan.side_effects[1]
    else {
        panic!("expected apply handles effect");
    };
    assert_eq!(sender_handle, "Bob.Remote");
    assert_eq!(recipient_handle, "alice");
    let NotificationSideEffect::StoreMessage(record) = &plan.side_effects[2] else {
        panic!("expected store message");
    };
    assert_eq!(record.msg_id, "msg-001");
    assert_eq!(record.sender_did, "did:bob");
    assert_eq!(record.receiver_did, "did:alice");
    assert_eq!(record.credential_name, "alice");
    let NotificationSideEffect::DispatchHostNotification {
        event,
        should_notify,
    } = &plan.side_effects[3]
    else {
        panic!("expected dispatch effect");
    };
    assert!(*should_notify);
    let event = event.as_ref().expect("host event");
    let HostNotificationData::Direct(data) = event.data.as_ref().expect("direct data") else {
        panic!("expected direct host event");
    };
    assert_eq!(data.sender_handle, "Bob.Remote");
    assert_eq!(data.recipient_handle, "alice");
}

#[test]
fn direct_contact_sync_error_is_ignored_like_go_blank_handle() {
    let plan = handle_notification_plan(
        &direct_notification("direct.incoming", "msg-001", "text/plain"),
        &session("alice"),
        SecureNotificationNormalization::KeepOriginal,
        Some(fixed_received_at()),
        |_request| anyhow::bail!("lookup boom"),
    );

    let NotificationSideEffect::ApplyHostNotificationHandles {
        sender_handle,
        recipient_handle,
    } = &plan.side_effects[1]
    else {
        panic!("expected apply handles effect");
    };
    assert_eq!(sender_handle, "");
    assert_eq!(recipient_handle, "alice");
    let NotificationSideEffect::DispatchHostNotification { event, .. } = &plan.side_effects[3]
    else {
        panic!("expected dispatch effect");
    };
    let HostNotificationData::Direct(data) = event.as_ref().unwrap().data.as_ref().unwrap() else {
        panic!("expected direct data");
    };
    assert_eq!(data.sender_handle, "");
    assert_eq!(data.recipient_handle, "alice");
}

#[test]
fn mail_notification_stores_and_dispatches_without_contact_sync_or_handles() {
    let plan = handle_notification_plan(
        &json!({
            "method": "mail.notification",
            "params": {
                "mailbox_did": "did:alice",
                "mailbox_address": "alice@example.com",
                "from_addr": "sender@example.com",
                "subject": "Mail Subject",
                "preview": "Preview text",
                "message_id": "mail-msg-001"
            }
        }),
        &session("alice"),
        SecureNotificationNormalization::KeepOriginal,
        Some(fixed_received_at()),
        |_request| panic!("mail must not sync contacts"),
    );

    assert_eq!(plan.route, NotificationRoute::MailNotification);
    assert_eq!(
        effect_names(&plan.side_effects),
        vec!["store_message", "dispatch_host"]
    );
    let NotificationSideEffect::StoreMessage(record) = &plan.side_effects[0] else {
        panic!("expected store message");
    };
    assert_eq!(record.msg_id, "mail-msg-001");
    assert_eq!(record.owner_did, "did:alice");
    assert_eq!(record.credential_name, "alice");
    let NotificationSideEffect::DispatchHostNotification {
        event,
        should_notify,
    } = &plan.side_effects[1]
    else {
        panic!("expected dispatch");
    };
    assert!(*should_notify);
    assert_eq!(event.as_ref().expect("event").topic, "im.message.received");
}

#[test]
fn group_incoming_plans_group_contact_sync_handles_store_then_dispatch() {
    let notification = json!({
        "method": "group.incoming",
        "params": {
            "meta": {
                "sender_did": "did:bob",
                "message_id": "group-msg-001",
                "target": {"did": "did:alice"}
            },
            "body": {
                "group_did": "did:group",
                "group_event_seq": 9,
                "text": "hello group"
            }
        }
    });
    let mut sync_calls = Vec::new();

    let plan = handle_notification_plan(
        &notification,
        &session("wba://Alice.Example"),
        SecureNotificationNormalization::KeepOriginal,
        Some(fixed_received_at()),
        |request| {
            sync_calls.push(request.clone());
            Ok("bob".to_string())
        },
    );

    assert_eq!(plan.route, NotificationRoute::GroupIncoming);
    assert_eq!(
        sync_calls,
        vec![IncomingContactSyncRequest {
            owner_did: "did:alice".to_string(),
            sender_did: "did:bob".to_string(),
            source_type: "group.incoming".to_string(),
            source_group_id: "did:group".to_string(),
        }]
    );
    assert_eq!(
        effect_names(&plan.side_effects),
        vec![
            "sync_contact",
            "apply_handles",
            "store_message",
            "dispatch_host",
        ]
    );
    let NotificationSideEffect::StoreMessage(record) = &plan.side_effects[2] else {
        panic!("expected store message");
    };
    assert_eq!(record.msg_id, "group-msg-001");
    assert_eq!(record.group_did, "did:group");
}

#[test]
fn group_state_changed_upserts_group_member_message_then_dispatches() {
    let plan = handle_notification_plan(
        &json!({
            "method": "group.state_changed",
            "params": {
                "meta": {"target": {"did": "did:alice"}},
                "body": {
                    "group_did": "did:group",
                    "group_event_seq": 3,
                    "event_id": "evt-3",
                    "subject_did": "did:bob",
                    "subject_method": "group.remove",
                    "actor_did": "did:alice",
                    "membership_status": "removed",
                    "changed_at": "2026-04-07T09:06:01Z"
                }
            }
        }),
        &session("alice"),
        SecureNotificationNormalization::KeepOriginal,
        Some(fixed_received_at()),
        |_request| panic!("group state must not sync contacts"),
    );

    assert_eq!(plan.route, NotificationRoute::GroupStateChanged);
    assert_eq!(
        effect_names(&plan.side_effects),
        vec![
            "upsert_group",
            "upsert_group_member",
            "store_message",
            "dispatch_host",
        ]
    );
    let NotificationSideEffect::UpsertGroup(group) = &plan.side_effects[0] else {
        panic!("expected group upsert");
    };
    assert_eq!(group.group_did, "did:group");
    let NotificationSideEffect::UpsertGroupMember(member) = &plan.side_effects[1] else {
        panic!("expected member upsert");
    };
    assert_eq!(member.member_did, "did:bob");
    assert_eq!(member.status, "removed");
    let NotificationSideEffect::StoreMessage(message) = &plan.side_effects[2] else {
        panic!("expected state message store");
    };
    assert_eq!(message.msg_id, "evt-3");
}

#[test]
fn group_state_without_subject_skips_member_upsert_like_go() {
    let plan = handle_notification_plan(
        &json!({
            "method": "group.state_changed",
            "params": {
                "meta": {"target": {"did": "did:alice"}},
                "body": {
                    "group_did": "did:group",
                    "group_event_seq": 3,
                    "event_id": "evt-3"
                }
            }
        }),
        &session("alice"),
        SecureNotificationNormalization::KeepOriginal,
        Some(fixed_received_at()),
        |_request| panic!("group state must not sync contacts"),
    );

    assert_eq!(
        effect_names(&plan.side_effects),
        vec!["upsert_group", "store_message", "dispatch_host"]
    );
}

#[test]
fn unknown_notification_is_ignored_after_secure_check() {
    let plan = handle_notification_plan(
        &json!({"method": "unknown"}),
        &session("alice"),
        SecureNotificationNormalization::KeepOriginal,
        Some(fixed_received_at()),
        |_request| panic!("unknown must not sync contacts"),
    );

    assert_eq!(plan.route, NotificationRoute::Ignored);
    assert_eq!(plan.secure_effect, SecureNotificationEffect::NotSecure);
    assert!(plan.side_effects.is_empty());
    assert!(plan.host_event.is_none());
}

#[test]
fn secure_direct_can_drop_before_host_event_or_store() {
    let plan = handle_notification_plan(
        &direct_notification(
            "direct.incoming",
            "secure-msg-001",
            "application/anp-direct-init+json",
        ),
        &session("alice"),
        SecureNotificationNormalization::Drop,
        Some(fixed_received_at()),
        |_request| panic!("dropped secure notification must not sync contacts"),
    );

    assert_eq!(plan.route, NotificationRoute::DroppedBySecureNormalization);
    assert_eq!(plan.secure_effect, SecureNotificationEffect::Dropped);
    assert!(plan.notification.is_none());
    assert!(plan.host_event.is_none());
    assert!(plan.side_effects.is_empty());
}

#[test]
fn secure_direct_replacement_routes_replaced_plaintext_notification() {
    let secure = direct_notification(
        "direct.incoming",
        "secure-msg-001",
        "application/anp-direct-init+json",
    );
    let plaintext = direct_notification("direct.secure.init", "secure-msg-001", "text/plain");

    let plan = handle_notification_plan(
        &secure,
        &session("alice"),
        SecureNotificationNormalization::Replace(plaintext),
        Some(fixed_received_at()),
        |_request| Ok("bob".to_string()),
    );

    assert_eq!(plan.secure_effect, SecureNotificationEffect::Replaced);
    assert_eq!(plan.route, NotificationRoute::Ignored);
    assert!(plan.side_effects.is_empty());
    assert_eq!(
        plan.notification.as_ref().unwrap()["method"],
        "direct.secure.init"
    );
}

#[test]
fn secure_direct_kept_original_stores_as_secure_e2ee_direct_like_go_fallback() {
    let plan = handle_notification_plan(
        &direct_notification(
            "direct.incoming",
            "secure-msg-001",
            "application/anp-direct-cipher+json",
        ),
        &session("alice"),
        SecureNotificationNormalization::KeepOriginal,
        Some(fixed_received_at()),
        |_request| Ok(String::new()),
    );

    assert_eq!(plan.route, NotificationRoute::DirectIncoming);
    assert_eq!(plan.secure_effect, SecureNotificationEffect::KeptOriginal);
    let NotificationSideEffect::StoreMessage(record) = &plan.side_effects[2] else {
        panic!("expected store message");
    };
    assert!(record.is_e2ee);
    assert_eq!(record.content_type, "application/anp-direct-cipher+json");
}

fn session(handle: &str) -> NotificationSessionContext {
    NotificationSessionContext {
        identity_name: "alice".to_string(),
        did: " did:alice ".to_string(),
        handle: handle.to_string(),
    }
}

fn direct_notification(method: &str, message_id: &str, content_type: &str) -> Value {
    json!({
        "method": method,
        "params": {
            "meta": {
                "sender_did": "did:bob",
                "message_id": message_id,
                "created_at": "2026-04-07T00:00:00Z",
                "content_type": content_type,
                "security_profile": if content_type.starts_with("application/anp-direct-") {
                    "direct-e2ee"
                } else {
                    "transport-protected"
                },
                "target": {
                    "kind": "agent",
                    "did": "did:alice"
                }
            },
            "body": {
                "text": "hello back"
            }
        }
    })
}

fn effect_names(effects: &[NotificationSideEffect]) -> Vec<&'static str> {
    effects
        .iter()
        .map(|effect| match effect {
            NotificationSideEffect::SyncIncomingContact(_) => "sync_contact",
            NotificationSideEffect::ApplyHostNotificationHandles { .. } => "apply_handles",
            NotificationSideEffect::StoreMessage(_) => "store_message",
            NotificationSideEffect::UpsertGroup(_) => "upsert_group",
            NotificationSideEffect::UpsertGroupMember(_) => "upsert_group_member",
            NotificationSideEffect::DispatchHostNotification { .. } => "dispatch_host",
        })
        .collect()
}

fn fixed_received_at() -> OffsetDateTime {
    OffsetDateTime::new_in_offset(
        Date::from_calendar_date(2026, Month::April, 12).expect("date"),
        Time::from_hms(10, 30, 0).expect("time"),
        UtcOffset::UTC,
    )
}
