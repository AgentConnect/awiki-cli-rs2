use awiki_cli::runtime::host_notify::{
    DirectMessageNotificationData, GroupMessageNotificationData, GroupStateChangedNotificationData,
    HostNotificationData, HostNotificationEvent,
};
use awiki_cli::runtime::openclaw_host_notify::{
    build_openclaw_agent_hook_message, build_openclaw_event_text, build_openclaw_hook_request,
    FIXED_HOOK_NAME,
};
use serde_json::json;

#[test]
fn openclaw_hook_request_includes_channel_delivery_like_go() {
    let request = build_openclaw_hook_request(
        &direct_event(DirectMessageNotificationData {
            channel: "direct".to_string(),
            message_id: "msg-001".to_string(),
            conversation_id: "conv-alice-bob".to_string(),
            sender_did: "did:wba:example.com:user:alice:e1_alice".to_string(),
            recipient_did: "did:wba:example.com:user:bob:e1_bob".to_string(),
            content_type: "text/plain".to_string(),
            text: "hello".to_string(),
            ..DirectMessageNotificationData::default()
        }),
        FIXED_HOOK_NAME,
        "telegram",
        "123456",
    );

    assert!(request.deliver);
    assert_eq!(request.name, "AWiki");
    assert_eq!(request.channel, "telegram");
    assert_eq!(request.to, "123456");
    assert_eq!(request.wake_mode, "now");
    assert!(request
        .message
        .contains("You received a new im message from awiki."));

    let raw = serde_json::to_value(&request).expect("request json");
    assert_eq!(raw["wakeMode"], "now");
    assert!(raw.get("wake_mode").is_none());
}

#[test]
fn openclaw_event_text_uses_main_agent_session_format_like_go() {
    let text = build_openclaw_event_text(&direct_event(DirectMessageNotificationData {
        sender_handle: "alice".to_string(),
        sender_did: "did:wba:example.com:user:alice:e1_alice".to_string(),
        recipient_handle: "bob".to_string(),
        recipient_did: "did:wba:example.com:user:bob:e1_bob".to_string(),
        created_at: "2026-04-07T00:00:00Z".to_string(),
        text: "hello back".to_string(),
        ..DirectMessageNotificationData::default()
    }));

    assert!(text.contains("[Awiki New Direct Message]"));
    assert!(text.contains("sender_did: did:wba:example.com:user:alice:e1_alice"));
    assert!(text.contains("sender_handle: alice"));
    assert!(text.contains("recipient_handle: bob"));
    assert!(text.contains("sent_at: 2026-04-07T00:00:00Z"));
    assert!(text.ends_with("hello back"));
}

#[test]
fn openclaw_event_text_uses_mail_format_like_go() {
    let text = build_openclaw_event_text(&direct_event(mail_data(true)));

    assert!(text.contains("[Awiki New Mail]"));
    assert!(text.contains("from_addr: sender@example.com"));
    assert!(text.contains("mailbox_address: alice@example.com"));
    assert!(text.contains("subject: Mail Subject"));
    assert!(text.contains("has_attachments: true"));
    assert!(text.contains("Subject: Mail Subject"));
    assert!(text.contains("Preview text"));
    assert!(text.contains("(This message has attachments.)"));
}

#[test]
fn openclaw_hook_request_includes_mail_prompt_like_go() {
    let request = build_openclaw_hook_request(
        &direct_event(mail_data(false)),
        FIXED_HOOK_NAME,
        "telegram",
        "123456",
    );

    assert!(request
        .message
        .contains("You received a new mail notification from awiki."));
    assert!(request.message.contains("Message type: mail"));
    assert!(request.message.contains("Sender DID: sender@example.com"));
    assert!(request
        .message
        .contains("Receiver handle: alice@example.com"));
}

#[test]
fn openclaw_prompt_uses_group_and_group_state_parts_like_go() {
    let group = HostNotificationEvent {
        version: "1.0".to_string(),
        id: "group-msg-1".to_string(),
        topic: "im.group.message.received".to_string(),
        received_at: "2026-04-12T10:30:00Z".to_string(),
        data: Some(HostNotificationData::Group(GroupMessageNotificationData {
            channel: "group".to_string(),
            message_id: "group-msg-1".to_string(),
            group_did: "did:group".to_string(),
            sender_handle: "alice".to_string(),
            sender_did: "did:alice".to_string(),
            recipient_handle: "bob".to_string(),
            recipient_did: "did:bob".to_string(),
            content_type: "application/json".to_string(),
            accepted_at: "2026-04-07T09:11:01Z".to_string(),
            ..GroupMessageNotificationData::default()
        })),
    };

    let text = build_openclaw_event_text(&group);
    assert!(text.contains("[Awiki New Group Message]"));
    assert!(text.contains("group_did: did:group"));
    assert!(text.contains("sent_at: 2026-04-07T09:11:01Z"));
    assert!(text.ends_with("[application/json]"));

    let prompt = build_openclaw_agent_hook_message(&HostNotificationEvent {
        version: "1.0".to_string(),
        id: "state-1".to_string(),
        topic: "im.group.state.changed".to_string(),
        received_at: "2026-04-12T10:30:00Z".to_string(),
        data: Some(HostNotificationData::GroupState(
            GroupStateChangedNotificationData {
                channel: "group".to_string(),
                event_id: "state-1".to_string(),
                event_type: "member-removed".to_string(),
                group_did: "did:group".to_string(),
                recipient_did: "did:bob".to_string(),
                actor_did: "did:alice".to_string(),
                subject_did: "did:carol".to_string(),
                subject_method: "group.remove".to_string(),
                membership_status: "removed".to_string(),
                ..GroupStateChangedNotificationData::default()
            },
        )),
    });
    assert!(prompt.contains("Message type: group"));
    assert!(prompt.contains("Group ID: did:group"));
    assert!(prompt.contains("Sender DID: did:alice"));
    assert!(prompt.contains("Receiver DID: did:bob"));
    assert!(prompt.contains("Group state changed. event_type=member-removed"));
}

#[test]
fn openclaw_mail_content_fallbacks_and_unknown_event_match_go() {
    let mail = direct_event(DirectMessageNotificationData {
        source_kind: "mail".to_string(),
        recipient_did: "did:mailbox".to_string(),
        content_type: "mail.notification".to_string(),
        ..DirectMessageNotificationData::default()
    });
    let text = build_openclaw_event_text(&mail);
    assert!(text.ends_with("[mail notification]"));

    let event = HostNotificationEvent {
        version: "1.0".to_string(),
        id: "unknown-1".to_string(),
        topic: "unknown".to_string(),
        received_at: "2026-04-12T10:30:00Z".to_string(),
        data: None,
    };
    let text = build_openclaw_event_text(&event);
    assert!(text.contains("[Awiki Notification]"));
    assert!(text.contains(r#""id":"unknown-1""#));

    let prompt = build_openclaw_agent_hook_message(&event);
    assert!(prompt.contains("You received a new notification from awiki."));
    assert!(prompt.contains("Message type: notification"));
    assert!(prompt.contains(r#""topic":"unknown""#));
}

fn direct_event(data: DirectMessageNotificationData) -> HostNotificationEvent {
    HostNotificationEvent {
        version: "1.0".to_string(),
        id: fallback_id(&data.message_id),
        topic: "im.message.received".to_string(),
        received_at: "2026-04-12T10:30:00Z".to_string(),
        data: Some(HostNotificationData::Direct(data)),
    }
}

fn mail_data(has_attachments: bool) -> DirectMessageNotificationData {
    DirectMessageNotificationData {
        channel: "mail".to_string(),
        source_kind: "mail".to_string(),
        message_id: "mail-msg-001".to_string(),
        recipient_did: "did:wba:example.com:user:alice:e1_alice".to_string(),
        content_type: "mail.notification".to_string(),
        text: "Preview text".to_string(),
        mailbox_address: "alice@example.com".to_string(),
        mailbox_did: "did:wba:example.com:user:alice:e1_alice".to_string(),
        from_addr: "sender@example.com".to_string(),
        subject: "Mail Subject".to_string(),
        preview: "Preview text".to_string(),
        has_attachments,
        ..DirectMessageNotificationData::default()
    }
}

fn fallback_id(value: &str) -> String {
    if value.trim().is_empty() {
        "event-1".to_string()
    } else {
        value.to_string()
    }
}

#[test]
fn openclaw_hook_request_json_shape_matches_go() {
    let request = build_openclaw_hook_request(
        &direct_event(DirectMessageNotificationData {
            message_id: "msg-001".to_string(),
            sender_did: "did:alice".to_string(),
            recipient_did: "did:bob".to_string(),
            content_type: "text/plain".to_string(),
            text: "hello".to_string(),
            ..DirectMessageNotificationData::default()
        }),
        FIXED_HOOK_NAME,
        "telegram",
        "123456",
    );

    assert_eq!(
        serde_json::to_value(&request).expect("request json"),
        json!({
            "message": request.message,
            "name": "AWiki",
            "wakeMode": "now",
            "deliver": true,
            "channel": "telegram",
            "to": "123456"
        })
    );
}
