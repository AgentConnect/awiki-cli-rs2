use awiki_cli::host_runtime::host_notify::{HostNotificationData, HostNotificationEvent};
use awiki_cli::host_runtime::host_notify_sink::HostNotifySink;
use awiki_cli::host_runtime::listener::{HostNotifyStatus, SessionStatus, Status};
use awiki_cli::host_runtime::listener_im_event_adapter::{
    handle_im_event, handle_reliable_remote_event, CliImEventRoute, IM_EVENT_UNKNOWN_WARNING_PREFIX,
};
use im_core::prelude::{
    AttachmentDownloadAction, AttachmentMessageSummary, ConnectionStateChanged, GroupRef,
    GroupUpdateKind, GroupUpdatedEvent, ImEvent, Message, MessageBodyView, MessageDirection,
    MessageId, MessageKind, MessageMetadata, MessageMetadataAttribute, MessageReceivedEvent,
    PeerRef, RealtimeConnectionState, SystemNotificationChangedEvent, SystemNotificationKind,
    SystemNotificationSnapshot, SystemNotificationState, ThreadRef, UnknownNotificationEvent,
};
use im_core::realtime::{RealtimeSyncHint, SyncDomain};
use serde_json::json;
use std::collections::BTreeSet;
use std::sync::Mutex;

#[test]
fn im_event_direct_message_dispatches_host_notification_without_cli_store_projection() {
    let sink = RecordingHostNotifySink::default();
    let mut status = status();

    let result = handle_im_event(
        Some(&sink),
        &mut status,
        direct_message_event("msg-direct-1", "hello from sdk"),
        None,
        Some("bob"),
        Some("did:wba:example:agents:bob:e1_bob"),
    );

    assert_eq!(result.route, CliImEventRoute::DirectIncoming);
    assert!(result.dispatched_host_notification);
    assert_eq!(result.host_notify_last_error, None);
    let events = sink.events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].topic, "im.message.received");
    assert_eq!(events[0].id, "msg-direct-1");
    let HostNotificationData::Direct(data) = events[0].data.as_ref().expect("direct data") else {
        panic!("expected direct host notification data");
    };
    assert_eq!(data.message_id, "msg-direct-1");
    assert_eq!(data.sender_did, "did:wba:example:agents:alice:e1_alice");
    assert_eq!(data.recipient_did, "did:wba:example:agents:bob:e1_bob");
    assert_eq!(data.text, "hello from sdk");
}

#[test]
fn im_event_attachment_message_dispatches_download_action_hint() {
    let sink = RecordingHostNotifySink::default();
    let mut status = status();

    let result = handle_im_event(
        Some(&sink),
        &mut status,
        direct_attachment_message_event(),
        None,
        Some("bob"),
        Some("did:wba:example:agents:bob:e1_bob"),
    );

    assert_eq!(result.route, CliImEventRoute::DirectIncoming);
    assert!(result.dispatched_host_notification);
    let events = sink.events();
    assert_eq!(events.len(), 1);
    let HostNotificationData::Direct(data) = events[0].data.as_ref().expect("direct data") else {
        panic!("expected direct data");
    };
    assert!(data.has_attachments);
    assert!(data.text.contains("report.pdf"));
    let attachment = data.attachment.as_ref().expect("attachment summary");
    assert_eq!(attachment.attachment_id, "att-1");
    assert_eq!(attachment.filename, "report.pdf");
    let action = data.download_action.as_ref().expect("download action");
    assert_eq!(action.command, "msg.attachment.download");
    assert_eq!(action.message_id, "msg-attachment-1");
    assert_eq!(action.attachment_id, "att-1");
    assert_eq!(action.with, "did:wba:example:agents:alice:e1_alice");
    assert!(action.group.is_empty());
}

#[test]
fn im_event_group_message_and_update_dispatch_host_notifications_only() {
    let sink = RecordingHostNotifySink::default();
    let mut status = status();

    let message = handle_im_event(
        Some(&sink),
        &mut status,
        group_message_event(),
        None,
        Some("bob"),
        Some("did:wba:example:agents:bob:e1_bob"),
    );
    assert_eq!(message.route, CliImEventRoute::GroupIncoming);
    assert!(message.dispatched_host_notification);
    let events = sink.events();
    let group_event = events
        .iter()
        .find(|event| event.topic == "im.group.message.received")
        .expect("group message host notification");
    assert_eq!(group_event.id, "raw-group-msg-1");
    let HostNotificationData::Group(data) = group_event.data.as_ref().expect("group message data")
    else {
        panic!("expected group host notification data");
    };
    assert_eq!(data.message_id, "raw-group-msg-1");
    assert_eq!(data.group_event_seq, "7");

    let update = handle_im_event(
        Some(&sink),
        &mut status,
        ImEvent::GroupUpdated(GroupUpdatedEvent {
            group: GroupRef::parse("did:wba:groups.example:groups:demo:e1_group").unwrap(),
            update_kind: GroupUpdateKind::Updated,
            event_type: None,
            group_event_seq: None,
            group_state_version: None,
            actor_did: None,
            subject_did: None,
            subject_handle: None,
            previous_subject_did: None,
            handle_binding_generation: None,
            membership_status: None,
            changed_at: None,
            sync: None,
        }),
        None,
        Some("bob"),
        Some("did:wba:example:agents:bob:e1_bob"),
    );
    assert_eq!(update.route, CliImEventRoute::GroupStateChanged);
    assert!(update.dispatched_host_notification);

    let all_events = sink.events();
    assert!(all_events
        .iter()
        .any(|event| event.topic == "im.group.message.received"));
    assert!(all_events
        .iter()
        .any(|event| event.topic == "im.group.state.changed"));
}

#[test]
fn im_event_group_attachment_message_dispatches_group_download_action_hint() {
    let sink = RecordingHostNotifySink::default();
    let mut status = status();

    let result = handle_im_event(
        Some(&sink),
        &mut status,
        group_attachment_message_event(),
        None,
        Some("bob"),
        Some("did:wba:example:agents:bob:e1_bob"),
    );

    assert_eq!(result.route, CliImEventRoute::GroupIncoming);
    assert!(result.dispatched_host_notification);
    let events = sink.events();
    assert_eq!(events.len(), 1);
    let HostNotificationData::Group(data) = events[0].data.as_ref().expect("group data") else {
        panic!("expected group host notification data");
    };
    assert!(data.has_attachments);
    assert!(data.text.contains("report.pdf"));
    assert_eq!(
        data.download_action
            .as_ref()
            .expect("download action")
            .group,
        "did:wba:groups.example:groups:demo:e1_group"
    );
    assert!(data
        .download_action
        .as_ref()
        .expect("download action")
        .with
        .is_empty());
}

#[test]
fn im_event_unknown_notification_records_warning_without_attachment_enrichment() {
    let mut status = status();

    let result = handle_im_event(
        None,
        &mut status,
        ImEvent::UnknownNotification(UnknownNotificationEvent {
            content_type: Some("application/vnd.awiki.attachment+json".to_string()),
            notification_type: Some("attachment.ready".to_string()),
            reason: "unsupported notification method".to_string(),
            sync: None,
        }),
        None,
        Some("bob"),
        Some("did:wba:example:agents:bob:e1_bob"),
    );

    assert_eq!(result.route, CliImEventRoute::UnknownNotification);
    assert!(!result.dispatched_host_notification);
    assert_eq!(status.warnings.len(), 1);
    assert!(status.warnings[0].starts_with(IM_EVENT_UNKNOWN_WARNING_PREFIX));
    assert!(status.warnings[0].contains("attachment.ready"));
    assert!(status.warnings[0].contains("application/vnd.awiki.attachment+json"));
    assert!(!status.warnings[0].contains("AttachmentInput"));
}

#[test]
fn join_requested_system_notification_wakes_host_with_redacted_review_event() {
    let sink = RecordingHostNotifySink::default();
    let mut status = status();

    let result = handle_im_event(
        Some(&sink),
        &mut status,
        ImEvent::SystemNotificationChanged(SystemNotificationChangedEvent {
            notification: SystemNotificationSnapshot {
                event_id: "evt-system-1".to_owned(),
                did: "did:wba:example:agents:bob:e1_bob".to_owned(),
                join_session_id: "join-system-1".to_owned(),
                kind: SystemNotificationKind::JoinRequested,
                state: SystemNotificationState::Pending,
                session_revision: 1,
                issued_at: "2026-07-23T02:00:00Z".to_owned(),
                expires_at: "2026-07-23T02:10:00Z".to_owned(),
                first_seen_at: "2026-07-23T02:00:01Z".to_owned(),
                terminal: false,
            },
            sync: None,
        }),
        None,
        Some("bob"),
        Some("did:wba:example:agents:bob:e1_bob"),
    );

    assert_eq!(result.route, CliImEventRoute::DeviceJoinRequested);
    assert!(result.dispatched_host_notification);
    let events = sink.events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].topic, "im.device.join.requested");
    assert_eq!(events[0].id, "evt-system-1");
    let HostNotificationData::DeviceJoinRequest(data) =
        events[0].data.as_ref().expect("device Join data")
    else {
        panic!("expected device Join host notification data");
    };
    assert_eq!(data.event_id, "evt-system-1");
    assert_eq!(data.join_session_id, "join-system-1");
    assert_eq!(data.recipient_did, "did:wba:example:agents:bob:e1_bob");
    assert_eq!(data.issued_at, "2026-07-23T02:00:00Z");
    assert_eq!(data.expires_at, "2026-07-23T02:10:00Z");

    let raw = serde_json::to_string(&events[0]).expect("serialize host event");
    for forbidden in [
        "sas",
        "challenge",
        "proof",
        "token",
        "session_revision",
        "first_seen_at",
    ] {
        assert!(
            !raw.contains(forbidden),
            "host event leaked forbidden field {forbidden}"
        );
    }
}

#[test]
fn reliable_remote_join_requested_wakes_host_after_core_commit() {
    let sink = RecordingHostNotifySink::default();
    let mut status = status();

    let result = handle_reliable_remote_event(
        Some(&sink),
        &mut status,
        ImEvent::SystemNotificationChanged(SystemNotificationChangedEvent {
            notification: SystemNotificationSnapshot {
                event_id: "evt-system-reliable-1".to_owned(),
                did: "did:wba:example:agents:bob:e1_bob".to_owned(),
                join_session_id: "join-system-reliable-1".to_owned(),
                kind: SystemNotificationKind::JoinRequested,
                state: SystemNotificationState::Pending,
                session_revision: 1,
                issued_at: "2026-07-23T02:00:00Z".to_owned(),
                expires_at: "2026-07-23T02:10:00Z".to_owned(),
                first_seen_at: "2026-07-23T02:00:01Z".to_owned(),
                terminal: false,
            },
            sync: None,
        }),
        None,
        Some("bob"),
        Some("did:wba:example:agents:bob:e1_bob"),
    );

    assert_eq!(result.route, CliImEventRoute::DeviceJoinRequested);
    assert!(result.dispatched_host_notification);
    assert!(!result.reliable_sync_requested);
    let events = sink.events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].topic, "im.device.join.requested");
    assert_eq!(events[0].id, "evt-system-reliable-1");
    let serialized = serde_json::to_value(&events[0]).expect("serialize host event");
    let data = serialized["data"]
        .as_object()
        .expect("device Join host event data");
    assert_eq!(
        data.keys().map(String::as_str).collect::<BTreeSet<_>>(),
        BTreeSet::from([
            "channel",
            "event_id",
            "expires_at",
            "issued_at",
            "join_session_id",
            "recipient_did",
        ])
    );
    assert_eq!(
        data.get("join_session_id")
            .and_then(serde_json::Value::as_str),
        Some("join-system-reliable-1")
    );
    assert_eq!(
        data.get("recipient_did")
            .and_then(serde_json::Value::as_str),
        Some("did:wba:example:agents:bob:e1_bob")
    );
    let raw = serde_json::to_string(&events[0]).expect("serialize host event");
    for forbidden in [
        "sas",
        "challenge",
        "proof",
        "token",
        "session_revision",
        "first_seen_at",
    ] {
        assert!(
            !raw.contains(forbidden),
            "host event leaked forbidden field {forbidden}"
        );
    }
}

#[test]
fn non_pending_system_notification_does_not_wake_join_review() {
    let sink = RecordingHostNotifySink::default();
    let mut status = status();

    let result = handle_im_event(
        Some(&sink),
        &mut status,
        ImEvent::SystemNotificationChanged(SystemNotificationChangedEvent {
            notification: SystemNotificationSnapshot {
                event_id: "evt-system-completed".to_owned(),
                did: "did:wba:example:agents:bob:e1_bob".to_owned(),
                join_session_id: "join-system-1".to_owned(),
                kind: SystemNotificationKind::JoinCompleted,
                state: SystemNotificationState::Consumed,
                session_revision: 4,
                issued_at: "2026-07-23T02:00:00Z".to_owned(),
                expires_at: "2026-07-23T02:10:00Z".to_owned(),
                first_seen_at: "2026-07-23T02:04:00Z".to_owned(),
                terminal: true,
            },
            sync: None,
        }),
        None,
        Some("bob"),
        Some("did:wba:example:agents:bob:e1_bob"),
    );

    assert_eq!(result.route, CliImEventRoute::Ignored);
    assert!(!result.dispatched_host_notification);
    assert!(sink.events().is_empty());
}

#[test]
fn im_event_connection_state_updates_only_named_session() {
    let mut status = status();
    status.sessions = vec![
        SessionStatus {
            identity_name: "bob".to_string(),
            did: String::new(),
            connected: false,
            last_error: "old".to_string(),
        },
        SessionStatus {
            identity_name: "alice".to_string(),
            did: "did:alice".to_string(),
            connected: false,
            last_error: "keep".to_string(),
        },
    ];

    let result = handle_im_event(
        None,
        &mut status,
        ImEvent::ConnectionStateChanged(ConnectionStateChanged {
            state: RealtimeConnectionState::Connected,
            reason: None,
        }),
        None,
        Some("bob"),
        Some("did:bob"),
    );

    assert_eq!(result.route, CliImEventRoute::ConnectionStateChanged);
    assert!(result.connection_became_connected);
    let bob = status
        .sessions
        .iter()
        .find(|session| session.identity_name == "bob")
        .expect("bob session");
    assert!(bob.connected);
    assert_eq!(bob.did, "did:bob");
    assert!(bob.last_error.is_empty());
    let alice = status
        .sessions
        .iter()
        .find(|session| session.identity_name == "alice")
        .expect("alice session");
    assert!(!alice.connected);
    assert_eq!(alice.last_error, "keep");

    let duplicate = handle_im_event(
        None,
        &mut status,
        ImEvent::ConnectionStateChanged(ConnectionStateChanged {
            state: RealtimeConnectionState::Connected,
            reason: None,
        }),
        None,
        Some("bob"),
        Some("did:bob"),
    );
    assert!(!duplicate.connection_became_connected);
}

#[test]
fn dirty_gap_and_unknown_hints_request_reliable_v2_sync() {
    let mut status = status();
    let mut event = direct_message_event("msg-dirty-1", "dirty");
    let ImEvent::MessageReceived(message) = &mut event else {
        panic!("expected message event");
    };
    message.sync = Some(RealtimeSyncHint {
        event_id: None,
        event_seq: Some("9".to_owned()),
        event_type: None,
        domains: BTreeSet::from([SyncDomain::Message]),
        reason: Some("message_changed".to_owned()),
        dirty_lanes: Default::default(),
        sync_dirty: true,
        gap_detected: false,
        has_unknown_domain: false,
    });

    let dirty =
        handle_reliable_remote_event(None, &mut status, event, None, Some("bob"), Some("did:bob"));
    assert!(dirty.reliable_sync_requested);
    assert_eq!(dirty.route, CliImEventRoute::Ignored);
    assert!(!dirty.dispatched_host_notification);

    let mut gap_event = direct_message_event("msg-gap-1", "gap");
    let ImEvent::MessageReceived(message) = &mut gap_event else {
        panic!("expected message event");
    };
    message.sync = Some(RealtimeSyncHint {
        event_id: None,
        event_seq: Some("10".to_owned()),
        event_type: None,
        domains: BTreeSet::from([SyncDomain::Message]),
        reason: Some("gap_detected".to_owned()),
        dirty_lanes: Default::default(),
        sync_dirty: false,
        gap_detected: true,
        has_unknown_domain: false,
    });
    let gap = handle_reliable_remote_event(
        None,
        &mut status,
        gap_event,
        None,
        Some("bob"),
        Some("did:bob"),
    );
    assert!(gap.reliable_sync_requested);
    assert_eq!(gap.route, CliImEventRoute::Ignored);

    let unknown = handle_reliable_remote_event(
        None,
        &mut status,
        ImEvent::UnknownNotification(UnknownNotificationEvent {
            content_type: None,
            notification_type: Some("future.notification".to_owned()),
            reason: "unknown notification".to_owned(),
            sync: None,
        }),
        None,
        Some("bob"),
        Some("did:bob"),
    );
    assert!(unknown.reliable_sync_requested);
    assert_eq!(unknown.route, CliImEventRoute::Ignored);
}

#[test]
fn ordinary_v2_remote_message_without_hint_is_fail_safe_sync_only() {
    let sink = RecordingHostNotifySink::default();
    let mut status = status();

    let result = handle_reliable_remote_event(
        Some(&sink),
        &mut status,
        direct_message_event("msg-no-hint-1", "must reconcile first"),
        None,
        Some("bob"),
        Some("did:bob"),
    );

    assert!(result.reliable_sync_requested);
    assert_eq!(result.route, CliImEventRoute::Ignored);
    assert!(!result.dispatched_host_notification);
    assert!(sink.events.lock().unwrap().is_empty());
}

#[test]
fn verified_p5_message_dispatches_without_account_sync_reconcile() {
    let sink = RecordingHostNotifySink::default();
    let mut status = status();
    let mut event = direct_message_event("msg-p5-verified-1", "verified P5 message");
    let ImEvent::MessageReceived(received) = &mut event else {
        panic!("expected message event");
    };
    received.message.metadata.attributes.extend([
        MessageMetadataAttribute {
            key: "security".to_owned(),
            value: "direct-e2ee".to_owned(),
        },
        MessageMetadataAttribute {
            key: "decryption_state".to_owned(),
            value: "decrypted".to_owned(),
        },
    ]);

    let result = handle_reliable_remote_event(
        Some(&sink),
        &mut status,
        event,
        None,
        Some("bob"),
        Some("did:bob"),
    );

    assert!(!result.reliable_sync_requested);
    assert_eq!(result.route, CliImEventRoute::DirectIncoming);
    assert!(result.dispatched_host_notification);
    assert_eq!(sink.events().len(), 1);
}

#[test]
fn outgoing_or_group_p5_projection_stays_on_account_sync_path() {
    for mut event in [
        direct_message_event("msg-p5-own-sync-1", "outgoing own-device sync"),
        group_message_event(),
    ] {
        let ImEvent::MessageReceived(received) = &mut event else {
            panic!("expected message event");
        };
        received.message.metadata.attributes.extend([
            MessageMetadataAttribute {
                key: "security".to_owned(),
                value: "direct-e2ee".to_owned(),
            },
            MessageMetadataAttribute {
                key: "decryption_state".to_owned(),
                value: "decrypted".to_owned(),
            },
        ]);
        if matches!(received.message.thread, ThreadRef::Direct(_)) {
            received.message.direction = MessageDirection::Outgoing;
        }

        let sink = RecordingHostNotifySink::default();
        let result = handle_reliable_remote_event(
            Some(&sink),
            &mut status(),
            event,
            None,
            Some("bob"),
            Some("did:bob"),
        );

        assert!(result.reliable_sync_requested);
        assert_eq!(result.route, CliImEventRoute::Ignored);
        assert!(!result.dispatched_host_notification);
        assert!(sink.events().is_empty());
    }
}

#[test]
fn conflicting_p5_verification_attributes_fail_closed_to_account_sync() {
    for (key, conflicting_value) in [("security", "plaintext"), ("decryption_state", "failed")] {
        let sink = RecordingHostNotifySink::default();
        let mut status = status();
        let mut event = direct_message_event("msg-p5-conflict-1", "conflicting P5 metadata");
        let ImEvent::MessageReceived(received) = &mut event else {
            panic!("expected message event");
        };
        received.message.metadata.attributes.extend([
            MessageMetadataAttribute {
                key: "security".to_owned(),
                value: "direct-e2ee".to_owned(),
            },
            MessageMetadataAttribute {
                key: "decryption_state".to_owned(),
                value: "decrypted".to_owned(),
            },
            MessageMetadataAttribute {
                key: key.to_owned(),
                value: conflicting_value.to_owned(),
            },
        ]);

        let result = handle_reliable_remote_event(
            Some(&sink),
            &mut status,
            event,
            None,
            Some("bob"),
            Some("did:bob"),
        );

        assert!(result.reliable_sync_requested);
        assert_eq!(result.route, CliImEventRoute::Ignored);
        assert!(!result.dispatched_host_notification);
        assert!(sink.events().is_empty());
    }
}

#[test]
fn im_core_runner_legacy_raw_filter_keeps_only_secure_direct_on_legacy_path() {
    let secure = json!({
        "method": "direct.incoming",
        "params": {
            "meta": {
                "sender_did": "did:sender",
                "target": {"did": "did:receiver"},
                "content_type": "application/anp-direct-cipher+json"
            },
            "body": {}
        }
    });
    let plain = json!({
        "method": "direct.incoming",
        "params": {
            "meta": {
                "sender_did": "did:sender",
                "target": {"did": "did:receiver"},
                "content_type": "text/plain"
            },
            "body": {"text": "plain"}
        }
    });
    let group = json!({
        "method": "group.incoming",
        "params": {
            "meta": {"sender_did": "did:sender", "target": {"did": "did:receiver"}},
            "body": {"group_did": "did:group", "text": "group"}
        }
    });

    assert!(im_core::realtime::is_direct_secure_wire_notification(
        &secure
    ));
    assert!(!im_core::realtime::is_direct_secure_wire_notification(
        &plain
    ));
    assert!(!im_core::realtime::is_direct_secure_wire_notification(
        &group
    ));
}

fn status() -> Status {
    Status {
        host_notify: HostNotifyStatus {
            enabled: true,
            sink: "file".to_string(),
            ..HostNotifyStatus::default()
        },
        ..Status::default()
    }
}

fn direct_message_event(message_id: &str, text: &str) -> ImEvent {
    ImEvent::MessageReceived(MessageReceivedEvent {
        message: Message {
            id: MessageId::parse(message_id).unwrap(),
            thread: ThreadRef::Direct(
                PeerRef::parse("did:wba:example:agents:alice:e1_alice", "").unwrap(),
            ),
            direction: MessageDirection::Incoming,
            sender: PeerRef::parse("did:wba:example:agents:alice:e1_alice", "").unwrap(),
            receiver: Some(PeerRef::parse("did:wba:example:agents:bob:e1_bob", "").unwrap()),
            group: None,
            body: MessageBodyView::Text {
                text: text.to_string(),
                kind: MessageKind::Text,
            },
            sent_at: Some("2026-05-22T00:00:00Z".to_string()),
            received_at: None,
            metadata: MessageMetadata {
                operation_id: Some("op-direct-1".to_string()),
                content_type: Some("text/plain".to_string()),
                ..MessageMetadata::default()
            },
        },
        attachment_summary: None,
        download_action: None,
        sync: None,
        warnings: Vec::new(),
    })
}

fn direct_attachment_message_event() -> ImEvent {
    ImEvent::MessageReceived(MessageReceivedEvent {
        message: Message {
            id: MessageId::parse("msg-attachment-1").unwrap(),
            thread: ThreadRef::Direct(
                PeerRef::parse("did:wba:example:agents:alice:e1_alice", "").unwrap(),
            ),
            direction: MessageDirection::Incoming,
            sender: PeerRef::parse("did:wba:example:agents:alice:e1_alice", "").unwrap(),
            receiver: Some(PeerRef::parse("did:wba:example:agents:bob:e1_bob", "").unwrap()),
            group: None,
            body: MessageBodyView::Unsupported {
                content_type: Some("application/anp-attachment-manifest+json".to_string()),
            },
            sent_at: Some("2026-05-22T00:00:00Z".to_string()),
            received_at: None,
            metadata: MessageMetadata {
                operation_id: Some("op-attachment-1".to_string()),
                content_type: Some("application/anp-attachment-manifest+json".to_string()),
                ..MessageMetadata::default()
            },
        },
        attachment_summary: Some(AttachmentMessageSummary {
            attachment_id: Some("att-1".to_string()),
            filename: Some("report.pdf".to_string()),
            mime_type: Some("application/pdf".to_string()),
            size_bytes: Some(1234),
            content_type: Some("application/anp-attachment-manifest+json".to_string()),
        }),
        download_action: Some(AttachmentDownloadAction {
            thread: ThreadRef::Direct(
                PeerRef::parse("did:wba:example:agents:alice:e1_alice", "").unwrap(),
            ),
            message_id: MessageId::parse("msg-attachment-1").unwrap(),
            attachment_id: Some("att-1".to_string()),
        }),
        sync: None,
        warnings: Vec::new(),
    })
}

fn group_message_event() -> ImEvent {
    ImEvent::MessageReceived(MessageReceivedEvent {
        message: Message {
            id: MessageId::parse("msg-group-1").unwrap(),
            thread: ThreadRef::Group(
                GroupRef::parse("did:wba:groups.example:groups:demo:e1_group").unwrap(),
            ),
            direction: MessageDirection::Incoming,
            sender: PeerRef::parse("did:wba:example:agents:alice:e1_alice", "").unwrap(),
            receiver: Some(PeerRef::parse("did:wba:example:agents:bob:e1_bob", "").unwrap()),
            group: Some(GroupRef::parse("did:wba:groups.example:groups:demo:e1_group").unwrap()),
            body: MessageBodyView::Text {
                text: "hello group".to_string(),
                kind: MessageKind::Text,
            },
            sent_at: Some("2026-05-22T00:00:01Z".to_string()),
            received_at: None,
            metadata: MessageMetadata {
                operation_id: Some("op-group-1".to_string()),
                server_sequence: Some(7),
                content_type: Some("text/plain".to_string()),
                attributes: vec![MessageMetadataAttribute {
                    key: "raw_message_id".to_string(),
                    value: "raw-group-msg-1".to_string(),
                }],
                ..MessageMetadata::default()
            },
        },
        attachment_summary: None,
        download_action: None,
        sync: None,
        warnings: Vec::new(),
    })
}

fn group_attachment_message_event() -> ImEvent {
    ImEvent::MessageReceived(MessageReceivedEvent {
        message: Message {
            id: MessageId::parse("msg-group-attachment-1").unwrap(),
            thread: ThreadRef::Group(
                GroupRef::parse("did:wba:groups.example:groups:demo:e1_group").unwrap(),
            ),
            direction: MessageDirection::Incoming,
            sender: PeerRef::parse("did:wba:example:agents:alice:e1_alice", "").unwrap(),
            receiver: Some(PeerRef::parse("did:wba:example:agents:bob:e1_bob", "").unwrap()),
            group: Some(GroupRef::parse("did:wba:groups.example:groups:demo:e1_group").unwrap()),
            body: MessageBodyView::Unsupported {
                content_type: Some("application/anp-attachment-manifest+json".to_string()),
            },
            sent_at: Some("2026-05-22T00:00:01Z".to_string()),
            received_at: None,
            metadata: MessageMetadata {
                operation_id: Some("op-group-attachment-1".to_string()),
                server_sequence: Some(8),
                content_type: Some("application/anp-attachment-manifest+json".to_string()),
                ..MessageMetadata::default()
            },
        },
        attachment_summary: Some(AttachmentMessageSummary {
            attachment_id: Some("att-1".to_string()),
            filename: Some("report.pdf".to_string()),
            mime_type: Some("application/pdf".to_string()),
            size_bytes: Some(1234),
            content_type: Some("application/anp-attachment-manifest+json".to_string()),
        }),
        download_action: Some(AttachmentDownloadAction {
            thread: ThreadRef::Group(
                GroupRef::parse("did:wba:groups.example:groups:demo:e1_group").unwrap(),
            ),
            message_id: MessageId::parse("msg-group-attachment-1").unwrap(),
            attachment_id: Some("att-1".to_string()),
        }),
        sync: None,
        warnings: Vec::new(),
    })
}

#[derive(Default)]
struct RecordingHostNotifySink {
    events: Mutex<Vec<HostNotificationEvent>>,
}

impl RecordingHostNotifySink {
    fn events(&self) -> Vec<HostNotificationEvent> {
        self.events.lock().expect("events lock").clone()
    }
}

impl HostNotifySink for RecordingHostNotifySink {
    fn notify(&self, event: &HostNotificationEvent) -> anyhow::Result<()> {
        self.events.lock().expect("events lock").push(event.clone());
        Ok(())
    }

    fn close(&self) -> anyhow::Result<()> {
        Ok(())
    }
}
