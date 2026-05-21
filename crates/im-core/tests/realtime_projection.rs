use im_core::compat::realtime::{project_notification, NotificationProjectionRoute};
use im_core::prelude::*;
use serde_json::json;

#[test]
fn realtime_projection_direct_incoming_becomes_message_received() {
    let projection = project_notification(&json!({
        "method": "direct.incoming",
        "params": {
            "meta": {
                "message_id": "msg-1",
                "operation_id": "op-1",
                "sender_did": "did:example:bob",
                "target": {"did": "did:example:alice"},
                "content_type": "text/plain",
                "created_at": "2026-05-22T00:00:00Z"
            },
            "body": {
                "text": "hello"
            }
        }
    }));

    assert_eq!(
        projection.route,
        NotificationProjectionRoute::DirectIncoming
    );
    let ImEvent::MessageReceived(MessageReceivedEvent { message }) = projection.event else {
        panic!("expected message received");
    };
    assert_eq!(message.id.as_str(), "msg-1");
    assert_eq!(message.sender.as_str(), "did:example:bob");
    assert_eq!(
        message.receiver.as_ref().map(PeerRef::as_str),
        Some("did:example:alice")
    );
    assert_eq!(
        message.thread,
        ThreadRef::Direct(PeerRef::parse("did:example:bob", "").unwrap())
    );
    assert_eq!(message.direction, MessageDirection::Incoming);
    assert_eq!(
        message.body,
        MessageBodyView::Text {
            text: "hello".to_string(),
            kind: MessageKind::Text,
        }
    );
    assert_eq!(message.sent_at.as_deref(), Some("2026-05-22T00:00:00Z"));
    assert_eq!(message.metadata.operation_id.as_deref(), Some("op-1"));
    assert_eq!(message.metadata.content_type.as_deref(), Some("text/plain"));
    assert!(message.group.is_none());
}

#[test]
fn realtime_projection_direct_attachment_like_notification_is_generic_unsupported_message() {
    let projection = project_notification(&json!({
        "method": "direct.incoming",
        "params": {
            "meta": {
                "message_id": "msg-attachment",
                "sender_did": "did:example:bob",
                "target": {"did": "did:example:alice"},
                "content_type": "application/octet-stream"
            },
            "body": {
                "attachment_id": "att-1"
            }
        }
    }));

    assert_eq!(
        projection.route,
        NotificationProjectionRoute::DirectIncoming
    );
    let ImEvent::MessageReceived(MessageReceivedEvent { message }) = projection.event else {
        panic!("expected message received");
    };
    assert_eq!(message.id.as_str(), "msg-attachment");
    assert_eq!(
        message.body,
        MessageBodyView::Unsupported {
            content_type: Some("application/octet-stream".to_string()),
        }
    );
    assert_eq!(
        message.metadata.content_type.as_deref(),
        Some("application/octet-stream")
    );
}

#[test]
fn realtime_projection_group_incoming_becomes_group_message_received() {
    let projection = project_notification(&json!({
        "method": "group.incoming",
        "params": {
            "meta": {
                "message_id": "group-msg-1",
                "operation_id": "group-op-1",
                "sender_did": "did:example:bob",
                "target": {"did": "did:example:alice"},
                "content_type": "text/plain"
            },
            "body": {
                "group_did": "did:example:group",
                "group_event_seq": 42,
                "text": "hello group",
                "accepted_at": "2026-05-22T01:00:00Z"
            }
        }
    }));

    assert_eq!(projection.route, NotificationProjectionRoute::GroupIncoming);
    let ImEvent::MessageReceived(MessageReceivedEvent { message }) = projection.event else {
        panic!("expected message received");
    };
    assert_eq!(message.id.as_str(), "group-msg-1");
    assert_eq!(
        message.thread,
        ThreadRef::Group(GroupRef::parse("did:example:group").unwrap())
    );
    assert_eq!(
        message.group.as_ref().map(GroupRef::as_str),
        Some("did:example:group")
    );
    assert_eq!(message.direction, MessageDirection::Incoming);
    assert_eq!(
        message.body,
        MessageBodyView::Text {
            text: "hello group".to_string(),
            kind: MessageKind::Text,
        }
    );
    assert_eq!(message.metadata.server_sequence, Some(42));
    assert_eq!(message.metadata.operation_id.as_deref(), Some("group-op-1"));
}

#[test]
fn realtime_projection_group_state_changed_becomes_group_updated() {
    let projection = project_notification(&json!({
        "method": "group.state_changed",
        "params": {
            "meta": {"target": {"did": "did:example:alice"}},
            "body": {
                "group_did": "did:example:group",
                "subject_method": "group.add",
                "subject_did": "did:example:bob",
                "membership_status": "active",
                "group_event_seq": 7
            }
        }
    }));

    assert_eq!(
        projection.route,
        NotificationProjectionRoute::GroupStateChanged
    );
    assert_eq!(
        projection.event,
        ImEvent::GroupUpdated(GroupUpdatedEvent {
            group: GroupRef::parse("did:example:group").unwrap(),
            update_kind: GroupUpdateKind::MemberAdded,
        })
    );
}

#[test]
fn realtime_projection_local_notification_becomes_local_event() {
    let projection = project_notification(&json!({
        "method": "local.notification",
        "params": {
            "id": "local-1",
            "title": "Title",
            "body": "Body",
            "source": "listener"
        }
    }));

    assert_eq!(
        projection.route,
        NotificationProjectionRoute::LocalNotification
    );
    assert_eq!(
        projection.event,
        ImEvent::LocalNotification(LocalNotificationEvent {
            notification_id: Some("local-1".to_string()),
            title: Some("Title".to_string()),
            body: Some("Body".to_string()),
            source: Some("listener".to_string()),
        })
    );
}

#[test]
fn realtime_projection_host_notification_becomes_host_domain_event_without_delivery() {
    let projection = project_notification(&json!({
        "method": "host.notification",
        "params": {
            "kind": "group_message",
            "title": "Group",
            "body": "New message"
        }
    }));

    assert_eq!(
        projection.route,
        NotificationProjectionRoute::HostNotification
    );
    assert_eq!(
        projection.event,
        ImEvent::HostNotification(HostNotificationEvent {
            event_type: HostNotificationKind::GroupMessage,
            title: Some("Group".to_string()),
            body: Some("New message".to_string()),
            thread: None,
        })
    );
}

#[test]
fn realtime_projection_unknown_notification_preserves_type_and_content_type() {
    let projection = project_notification(&json!({
        "method": "attachment.ready",
        "params": {
            "meta": {
                "content_type": "application/vnd.awiki.attachment+json"
            },
            "body": {
                "attachment_id": "att-1"
            }
        }
    }));

    assert_eq!(projection.route, NotificationProjectionRoute::Unknown);
    assert_eq!(
        projection.event,
        ImEvent::UnknownNotification(UnknownNotificationEvent {
            content_type: Some("application/vnd.awiki.attachment+json".to_string()),
            notification_type: Some("attachment.ready".to_string()),
            reason: "unsupported notification method".to_string(),
        })
    );
}

#[test]
fn realtime_projection_missing_required_fields_becomes_unknown_instead_of_panic() {
    let projection = project_notification(&json!({
        "method": "direct.incoming",
        "params": {
            "meta": {
                "content_type": "text/plain",
                "target": {"did": "did:example:alice"}
            },
            "body": {"text": "missing sender"}
        }
    }));

    assert_eq!(projection.route, NotificationProjectionRoute::Unknown);
    assert_eq!(
        projection.event,
        ImEvent::UnknownNotification(UnknownNotificationEvent {
            content_type: Some("text/plain".to_string()),
            notification_type: Some("direct.incoming".to_string()),
            reason: "missing sender or target".to_string(),
        })
    );
}
