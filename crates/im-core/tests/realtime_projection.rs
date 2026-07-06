use im_core::prelude::*;
use im_core::realtime::{project_notification, NotificationProjectionRoute};
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
    let ImEvent::MessageReceived(MessageReceivedEvent {
        message,
        attachment_summary,
        download_action,
        warnings,
        ..
    }) = projection.event
    else {
        panic!("expected message received");
    };
    assert!(attachment_summary.is_none());
    assert!(download_action.is_none());
    assert!(warnings.is_empty());
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
    let ImEvent::MessageReceived(MessageReceivedEvent {
        message,
        attachment_summary,
        download_action,
        warnings,
        ..
    }) = projection.event
    else {
        panic!("expected message received");
    };
    assert!(attachment_summary.is_none());
    assert!(download_action.is_none());
    assert!(warnings.is_empty());
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
fn realtime_projection_direct_json_payload_becomes_payload_body() {
    let projection = project_notification(&json!({
        "method": "direct.incoming",
        "params": {
            "meta": {
                "message_id": "msg-payload",
                "operation_id": "op-payload",
                "sender_did": "did:example:bob",
                "target": {"did": "did:example:alice"},
                "content_type": "application/json",
                "created_at": "2026-05-22T00:00:00Z"
            },
            "body": {
                "payload": {
                    "schema": "awiki.agent.command.v1",
                    "command": "runtime.agent.create"
                }
            }
        }
    }));

    assert_eq!(
        projection.route,
        NotificationProjectionRoute::DirectIncoming
    );
    let ImEvent::MessageReceived(MessageReceivedEvent { message, .. }) = projection.event else {
        panic!("expected message received");
    };
    assert_eq!(message.id.as_str(), "msg-payload");
    assert_eq!(
        message.body,
        MessageBodyView::Payload {
            payload: json!({
                "schema": "awiki.agent.command.v1",
                "command": "runtime.agent.create"
            })
        }
    );
    assert_eq!(
        message.metadata.content_type.as_deref(),
        Some("application/json")
    );
}

#[test]
fn realtime_attachment_projection_direct_manifest_enriches_message_event() {
    let projection = project_notification(&json!({
        "method": "direct.incoming",
        "params": {
            "meta": {
                "message_id": "msg-attachment",
                "operation_id": "op-attachment",
                "sender_did": "did:example:bob",
                "target": {"did": "did:example:alice"},
                "content_type": "application/anp-attachment-manifest+json",
                "created_at": "2026-05-22T00:00:00Z"
            },
            "body": {
                "payload": attachment_manifest()
            }
        }
    }));

    assert_eq!(
        projection.route,
        NotificationProjectionRoute::DirectIncoming
    );
    let ImEvent::MessageReceived(MessageReceivedEvent {
        message,
        attachment_summary: Some(summary),
        download_action: Some(download_action),
        warnings,
        ..
    }) = projection.event
    else {
        panic!("expected attachment-enriched message received");
    };
    assert!(warnings.is_empty());
    assert_eq!(message.id.as_str(), "msg-attachment");
    assert_eq!(
        message.body,
        MessageBodyView::Payload {
            payload: attachment_manifest(),
        }
    );
    assert_eq!(summary.attachment_id.as_deref(), Some("att-1"));
    assert_eq!(summary.filename.as_deref(), Some("report.pdf"));
    assert_eq!(summary.mime_type.as_deref(), Some("application/pdf"));
    assert_eq!(summary.size_bytes, Some(1234));
    assert_eq!(
        summary.content_type.as_deref(),
        Some("application/anp-attachment-manifest+json")
    );
    assert_eq!(
        download_action.thread,
        ThreadRef::Direct(PeerRef::parse("did:example:bob", "").unwrap())
    );
    assert_eq!(download_action.message_id.as_str(), "msg-attachment");
    assert_eq!(download_action.attachment_id.as_deref(), Some("att-1"));
    assert!(message.metadata.attributes.iter().any(|attribute| {
        attribute.key == "attachment_filename" && attribute.value == "report.pdf"
    }));
}

#[test]
fn realtime_attachment_projection_partial_manifest_warns_without_blocking_event() {
    let projection = project_notification(&json!({
        "method": "direct.incoming",
        "params": {
            "meta": {
                "message_id": "msg-partial-attachment",
                "sender_did": "did:example:bob",
                "target": {"did": "did:example:alice"},
                "content_type": "application/anp-attachment-manifest+json"
            },
            "body": {
                "payload": {
                    "attachments": [{
                        "attachment_id": "att-1"
                    }],
                    "primary_attachment_id": "att-1"
                }
            }
        }
    }));

    assert_eq!(
        projection.route,
        NotificationProjectionRoute::DirectIncoming
    );
    let ImEvent::MessageReceived(MessageReceivedEvent {
        message,
        attachment_summary: Some(summary),
        download_action: Some(download_action),
        warnings,
        ..
    }) = projection.event
    else {
        panic!("expected attachment-enriched message received");
    };
    assert_eq!(
        message.body,
        MessageBodyView::Payload {
            payload: json!({
                "attachments": [{
                    "attachment_id": "att-1"
                }],
                "primary_attachment_id": "att-1"
            }),
        }
    );
    assert_eq!(summary.attachment_id.as_deref(), Some("att-1"));
    assert!(summary.filename.is_none());
    assert!(summary.mime_type.is_none());
    assert!(summary.size_bytes.is_none());
    assert_eq!(download_action.attachment_id.as_deref(), Some("att-1"));
    assert!(warnings
        .iter()
        .any(|warning| warning.contains("filename is missing")));
    assert!(warnings
        .iter()
        .any(|warning| warning.contains("mime_type is missing")));
    assert!(warnings
        .iter()
        .any(|warning| warning.contains("size_bytes is missing")));
}

#[test]
fn realtime_attachment_projection_missing_attachment_id_uses_selection_fallback_action() {
    let projection = project_notification(&json!({
        "method": "direct.incoming",
        "params": {
            "meta": {
                "message_id": "msg-no-attachment-id",
                "sender_did": "did:example:bob",
                "target": {"did": "did:example:alice"},
                "content_type": "application/anp-attachment-manifest+json"
            },
            "body": {
                "payload": {
                    "attachments": [{
                        "filename": "report.pdf",
                        "mime_type": "application/pdf",
                        "size": "1234"
                    }]
                }
            }
        }
    }));

    let ImEvent::MessageReceived(MessageReceivedEvent {
        message,
        attachment_summary: Some(summary),
        download_action: Some(download_action),
        warnings,
        ..
    }) = projection.event
    else {
        panic!("expected attachment-enriched message received");
    };
    assert_eq!(
        message.body,
        MessageBodyView::Payload {
            payload: json!({
                "attachments": [{
                    "filename": "report.pdf",
                    "mime_type": "application/pdf",
                    "size": "1234"
                }]
            }),
        }
    );
    assert!(summary.attachment_id.is_none());
    assert_eq!(summary.filename.as_deref(), Some("report.pdf"));
    assert!(download_action.attachment_id.is_none());
    assert!(warnings
        .iter()
        .any(|warning| warning.contains("attachment_id is missing")));
}

#[test]
fn realtime_attachment_projection_missing_manifest_warns_and_keeps_generic_body() {
    let projection = project_notification(&json!({
        "method": "direct.incoming",
        "params": {
            "meta": {
                "message_id": "msg-missing-attachment",
                "sender_did": "did:example:bob",
                "target": {"did": "did:example:alice"},
                "content_type": "application/anp-attachment-manifest+json"
            },
            "body": {}
        }
    }));

    let ImEvent::MessageReceived(MessageReceivedEvent {
        message,
        attachment_summary,
        download_action,
        warnings,
        ..
    }) = projection.event
    else {
        panic!("expected message received");
    };
    assert_eq!(
        message.body,
        MessageBodyView::Unsupported {
            content_type: Some("application/anp-attachment-manifest+json".to_string()),
        }
    );
    assert!(attachment_summary.is_none());
    assert!(download_action.is_none());
    assert!(warnings
        .iter()
        .any(|warning| warning.contains("payload is missing or invalid")));
}

#[test]
fn realtime_attachment_projection_encrypted_manifest_warns_without_enrichment() {
    let projection = project_notification(&json!({
        "method": "direct.incoming",
        "params": {
            "meta": {
                "message_id": "msg-encrypted-attachment",
                "sender_did": "did:example:bob",
                "target": {"did": "did:example:alice"},
                "content_type": "application/anp-attachment-manifest+json"
            },
            "body": {
                "payload": {
                    "attachments": [{
                        "attachment_id": "att-1",
                        "filename": "secret.pdf",
                        "mime_type": "application/pdf",
                        "size": "1234",
                        "encryption_info": {
                            "mode": "group-e2ee"
                        }
                    }],
                    "primary_attachment_id": "att-1"
                }
            }
        }
    }));

    assert_eq!(
        projection.route,
        NotificationProjectionRoute::DirectIncoming
    );
    let ImEvent::MessageReceived(MessageReceivedEvent {
        message,
        attachment_summary,
        download_action,
        warnings,
        ..
    }) = projection.event
    else {
        panic!("expected message received");
    };
    assert_eq!(
        message.body,
        MessageBodyView::Payload {
            payload: json!({
                "attachments": [{
                    "attachment_id": "att-1",
                    "filename": "secret.pdf",
                    "mime_type": "application/pdf",
                    "size": "1234",
                    "encryption_info": {
                        "mode": "group-e2ee"
                    }
                }],
                "primary_attachment_id": "att-1"
            }),
        }
    );
    assert!(attachment_summary.is_none());
    assert!(download_action.is_none());
    assert!(message
        .metadata
        .attributes
        .iter()
        .all(|attribute| !attribute.key.starts_with("attachment_")));
    assert!(warnings.iter().any(|warning| {
        warning.contains("encryption mode group-e2ee")
            && warning.contains("not supported by realtime projection")
    }));
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
    let ImEvent::MessageReceived(MessageReceivedEvent {
        message,
        attachment_summary,
        download_action,
        warnings,
        ..
    }) = projection.event
    else {
        panic!("expected message received");
    };
    assert!(attachment_summary.is_none());
    assert!(download_action.is_none());
    assert!(warnings.is_empty());
    assert_eq!(message.id.as_str(), "did:example:group:42");
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
    assert!(message.metadata.attributes.iter().any(|attribute| {
        attribute.key == "raw_message_id" && attribute.value == "group-msg-1"
    }));
    assert!(message
        .metadata
        .attributes
        .iter()
        .any(|attribute| attribute.key == "group_event_seq" && attribute.value == "42"));
}

#[test]
fn realtime_attachment_projection_group_manifest_enriches_message_event() {
    let projection = project_notification(&json!({
        "method": "group.incoming",
        "params": {
            "meta": {
                "message_id": "group-msg-attachment",
                "operation_id": "group-op-attachment",
                "sender_did": "did:example:bob",
                "target": {"did": "did:example:alice"},
                "content_type": "application/anp-attachment-manifest+json"
            },
            "body": {
                "group_did": "did:example:group",
                "group_event_seq": 43,
                "payload": attachment_manifest()
            }
        }
    }));

    assert_eq!(projection.route, NotificationProjectionRoute::GroupIncoming);
    let ImEvent::MessageReceived(MessageReceivedEvent {
        message,
        attachment_summary: Some(summary),
        download_action: Some(download_action),
        warnings,
        ..
    }) = projection.event
    else {
        panic!("expected attachment-enriched group message received");
    };
    assert!(warnings.is_empty());
    assert_eq!(message.id.as_str(), "did:example:group:43");
    assert_eq!(
        download_action.thread,
        ThreadRef::Group(GroupRef::parse("did:example:group").unwrap())
    );
    assert_eq!(download_action.message_id.as_str(), "did:example:group:43");
    assert_eq!(download_action.attachment_id.as_deref(), Some("att-1"));
    assert_eq!(summary.filename.as_deref(), Some("report.pdf"));
    assert_eq!(
        message.body,
        MessageBodyView::Payload {
            payload: attachment_manifest(),
        }
    );
    assert_eq!(message.metadata.server_sequence, Some(43));
    assert!(message.metadata.attributes.iter().any(|attribute| {
        attribute.key == "raw_message_id" && attribute.value == "group-msg-attachment"
    }));
    assert!(message
        .metadata
        .attributes
        .iter()
        .any(|attribute| attribute.key == "group_event_seq" && attribute.value == "43"));
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
            sync: None,
        })
    );
}

fn attachment_manifest() -> serde_json::Value {
    json!({
        "attachments": [{
            "attachment_id": "att-1",
            "filename": "report.pdf",
            "mime_type": "application/pdf",
            "size": "1234",
            "digest": {
                "alg": "sha-256",
                "value_b64u": "digest"
            },
            "access_info": {
                "object_uri": "http://127.0.0.1:8080/objects/obj-1"
            },
            "encryption_info": {
                "mode": "none"
            }
        }],
        "primary_attachment_id": "att-1",
        "caption": "quarterly report"
    })
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
            sync: None,
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
            sync: None,
        })
    );
}

#[test]
fn realtime_projection_unknown_notification_preserves_sync_hint_for_delta_recovery() {
    let projection = project_notification(&json!({
        "method": "direct.incoming",
        "params": {
            "meta": {
                "content_type": "text/plain",
                "target": {"did": "did:example:alice"}
            },
            "body": {"text": "missing sender"}
        },
        "sync": {
            "event_id": "sev-20",
            "event_seq": "20",
            "event_type": "message.created"
        }
    }));

    let ImEvent::UnknownNotification(event) = projection.event else {
        panic!("expected unknown notification");
    };
    let sync = event.sync.expect("sync hint should be preserved");
    assert_eq!(sync.event_seq.as_deref(), Some("20"));
    assert!(sync.sync_dirty);
    assert!(!sync.gap_detected);
}
