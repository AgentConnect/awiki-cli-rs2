use awiki_cli::runtime::host_notify::{HostNotificationData, HostNotificationEvent};
use awiki_cli::runtime::host_notify_sink::HostNotifySink;
use awiki_cli::runtime::listener::{HostNotifyStatus, Status};
use awiki_cli::runtime::listener_im_event_adapter::{
    handle_im_event, IM_EVENT_UNKNOWN_WARNING_PREFIX,
};
use awiki_cli::runtime::listener_notification_plan::{
    NotificationRoute, NotificationSessionContext,
};
use awiki_cli::store::{self, StoreResult};
use im_core::prelude::{
    AttachmentDownloadAction, AttachmentMessageSummary, GroupRef, GroupUpdateKind,
    GroupUpdatedEvent, ImEvent, Message, MessageBodyView, MessageDirection, MessageId, MessageKind,
    MessageMetadata, MessageReceivedEvent, PeerRef, ThreadRef, UnknownNotificationEvent,
};
use rusqlite::Connection;
use serde_json::{json, Value};
use std::sync::Mutex;

#[test]
fn im_event_direct_message_stores_once_and_dispatches_host_notification() -> StoreResult<()> {
    let mut db = open_db()?;
    let sink = RecordingHostNotifySink::default();
    let mut status = status();
    let session = session();
    let event = direct_message_event("msg-direct-1", "hello from sdk");

    let result = handle_im_event(
        &mut db,
        Some(&sink),
        &mut status,
        event.clone(),
        &session,
        None,
        None,
    );
    assert_eq!(result.route, NotificationRoute::DirectIncoming);
    assert_eq!(result.failed_effect_count, 0);
    assert!(result
        .applied_effects
        .iter()
        .any(|effect| effect == "store_message msg_id=msg-direct-1"));

    let result = handle_im_event(
        &mut db,
        Some(&sink),
        &mut status,
        event,
        &session,
        None,
        None,
    );
    assert_eq!(result.route, NotificationRoute::DirectIncoming);
    assert_eq!(result.failed_effect_count, 0);

    let rows = store::list_messages_by_ids(
        &db,
        "did:wba:example:agents:bob:e1_bob",
        &["msg-direct-1".to_string()],
    )?;
    assert_eq!(rows.len(), 1);
    assert_eq!(string_field(&rows[0], "content"), "hello from sdk");
    assert_eq!(string_field(&rows[0], "content_type"), "text/plain");
    assert_eq!(i64_field(&rows[0], "is_read"), 0);

    let events = sink.events();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].topic, "im.message.received");
    assert_eq!(events[0].id, "msg-direct-1");
    let HostNotificationData::Direct(data) = events[0].data.as_ref().expect("direct data") else {
        panic!("expected direct host notification data");
    };
    assert_eq!(data.message_id, "msg-direct-1");
    assert_eq!(data.sender_did, "did:wba:example:agents:alice:e1_alice");
    assert_eq!(data.recipient_did, "did:wba:example:agents:bob:e1_bob");
    assert_eq!(data.text, "hello from sdk");
    Ok(())
}

#[test]
fn im_event_attachment_message_projects_metadata_and_download_action_hint() -> StoreResult<()> {
    let mut db = open_db()?;
    let sink = RecordingHostNotifySink::default();
    let mut status = status();
    let session = session();

    let result = handle_im_event(
        &mut db,
        Some(&sink),
        &mut status,
        direct_attachment_message_event(),
        &session,
        None,
        None,
    );
    assert_eq!(result.route, NotificationRoute::DirectIncoming);
    assert_eq!(result.failed_effect_count, 0);

    let result = handle_im_event(
        &mut db,
        Some(&sink),
        &mut status,
        direct_attachment_message_event(),
        &session,
        None,
        None,
    );
    assert_eq!(result.route, NotificationRoute::DirectIncoming);
    assert_eq!(result.failed_effect_count, 0);

    let rows = store::list_messages_by_ids(
        &db,
        "did:wba:example:agents:bob:e1_bob",
        &["msg-attachment-1".to_string()],
    )?;
    assert_eq!(rows.len(), 1);
    assert_eq!(
        string_field(&rows[0], "content_type"),
        "application/anp-attachment-manifest+json"
    );
    let metadata: Value =
        serde_json::from_str(string_field(&rows[0], "metadata")).expect("metadata json");
    assert_eq!(metadata["has_attachments"], true);
    assert_eq!(metadata["attachment_summary"]["attachment_id"], "att-1");
    assert_eq!(metadata["attachment_summary"]["filename"], "report.pdf");
    assert_eq!(
        metadata["attachment_summary"]["mime_type"],
        "application/pdf"
    );
    assert_eq!(metadata["attachment_summary"]["size_bytes"], 1234);
    assert_eq!(
        metadata["attachment_download_action"]["command"],
        "msg.attachment.download"
    );
    assert_eq!(
        metadata["attachment_download_action"]["message_id"],
        "msg-attachment-1"
    );
    assert_eq!(
        metadata["attachment_download_action"]["attachment_id"],
        "att-1"
    );
    assert_eq!(
        metadata["attachment_download_action"]["with"],
        "did:wba:example:agents:alice:e1_alice"
    );
    assert!(metadata["attachment_download_action"]
        .get("output")
        .is_none());
    let thread_id = store::make_thread_id(
        "did:wba:example:agents:bob:e1_bob",
        "did:wba:example:agents:alice:e1_alice",
        "",
    );
    let history_rows =
        store::list_thread_messages(&db, "did:wba:example:agents:bob:e1_bob", &thread_id, 10)?;
    assert_eq!(history_rows.len(), 1);
    let history_metadata: Value = serde_json::from_str(string_field(&history_rows[0], "metadata"))
        .expect("history metadata json");
    assert_eq!(
        history_metadata["attachment_summary"]["filename"],
        "report.pdf"
    );
    assert_eq!(
        history_metadata["attachment_download_action"]["message_id"],
        "msg-attachment-1"
    );
    let inbox_rows = store::list_inbox_messages(
        &db,
        "did:wba:example:agents:bob:e1_bob",
        10,
        "",
        false,
        true,
    )?;
    assert_eq!(inbox_rows.len(), 1);
    let inbox_metadata: Value = serde_json::from_str(string_field(&inbox_rows[0], "metadata"))
        .expect("inbox metadata json");
    assert_eq!(inbox_metadata["has_attachments"], true);
    assert_eq!(
        inbox_metadata["attachment_summary"]["filename"],
        "report.pdf"
    );

    let events = sink.events();
    assert_eq!(events.len(), 2);
    let HostNotificationData::Direct(data) = events[0].data.as_ref().expect("direct data") else {
        panic!("expected direct host notification data");
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
    Ok(())
}

#[test]
fn im_event_group_message_and_update_project_to_local_state() -> StoreResult<()> {
    let mut db = open_db()?;
    let sink = RecordingHostNotifySink::default();
    let mut status = status();
    let session = session();

    let result = handle_im_event(
        &mut db,
        Some(&sink),
        &mut status,
        group_message_event(),
        &session,
        None,
        None,
    );
    assert_eq!(result.route, NotificationRoute::GroupIncoming);
    assert_eq!(result.failed_effect_count, 0);

    let messages = store::list_group_messages(
        &db,
        "did:wba:example:agents:bob:e1_bob",
        "did:wba:groups.example:groups:demo:e1_group",
        0,
        None,
    )?;
    assert_eq!(messages.len(), 1);
    assert_eq!(string_field(&messages[0], "msg_id"), "msg-group-1");
    assert_eq!(i64_field(&messages[0], "server_seq"), 7);

    let result = handle_im_event(
        &mut db,
        Some(&sink),
        &mut status,
        ImEvent::GroupUpdated(GroupUpdatedEvent {
            group: GroupRef::parse("did:wba:groups.example:groups:demo:e1_group").unwrap(),
            update_kind: GroupUpdateKind::Updated,
        }),
        &session,
        None,
        None,
    );
    assert_eq!(result.route, NotificationRoute::GroupStateChanged);
    assert_eq!(result.failed_effect_count, 0);

    let group = store::get_group_snapshot(
        &db,
        "did:wba:example:agents:bob:e1_bob",
        "did:wba:groups.example:groups:demo:e1_group",
    )?;
    assert_eq!(
        string_field(&group, "group_id"),
        "did:wba:groups.example:groups:demo:e1_group"
    );
    assert!(sink
        .events()
        .iter()
        .any(|event| event.topic == "im.group.state.changed"));
    Ok(())
}

#[test]
fn im_event_group_attachment_message_projects_metadata_and_download_action_hint() -> StoreResult<()>
{
    let mut db = open_db()?;
    let sink = RecordingHostNotifySink::default();
    let mut status = status();
    let session = session();

    let result = handle_im_event(
        &mut db,
        Some(&sink),
        &mut status,
        group_attachment_message_event(),
        &session,
        None,
        None,
    );
    assert_eq!(result.route, NotificationRoute::GroupIncoming);
    assert_eq!(result.failed_effect_count, 0);

    let messages = store::list_group_messages(
        &db,
        "did:wba:example:agents:bob:e1_bob",
        "did:wba:groups.example:groups:demo:e1_group",
        0,
        None,
    )?;
    assert_eq!(messages.len(), 1);
    assert_eq!(
        string_field(&messages[0], "msg_id"),
        "msg-group-attachment-1"
    );
    let metadata: Value =
        serde_json::from_str(string_field(&messages[0], "metadata")).expect("metadata json");
    assert_eq!(metadata["has_attachments"], true);
    assert_eq!(metadata["attachment_summary"]["attachment_id"], "att-1");
    assert_eq!(metadata["attachment_summary"]["filename"], "report.pdf");
    assert_eq!(
        metadata["attachment_download_action"]["command"],
        "msg.attachment.download"
    );
    assert_eq!(
        metadata["attachment_download_action"]["message_id"],
        "msg-group-attachment-1"
    );
    assert_eq!(
        metadata["attachment_download_action"]["group"],
        "did:wba:groups.example:groups:demo:e1_group"
    );
    assert!(metadata["attachment_download_action"]
        .get("output")
        .is_none());
    let group_inbox_rows = store::list_group_inbox_messages(
        &db,
        "did:wba:example:agents:bob:e1_bob",
        10,
        "did:wba:groups.example:groups:demo:e1_group",
        false,
    )?;
    assert_eq!(group_inbox_rows.len(), 1);
    let group_inbox_metadata: Value =
        serde_json::from_str(string_field(&group_inbox_rows[0], "metadata"))
            .expect("group inbox metadata json");
    assert_eq!(group_inbox_metadata["has_attachments"], true);
    assert_eq!(
        group_inbox_metadata["attachment_summary"]["filename"],
        "report.pdf"
    );

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
    Ok(())
}

#[test]
fn im_event_unknown_notification_records_warning_without_attachment_enrichment() -> StoreResult<()>
{
    let mut db = open_db()?;
    let mut status = status();
    let session = session();

    let result = handle_im_event(
        &mut db,
        None,
        &mut status,
        ImEvent::UnknownNotification(UnknownNotificationEvent {
            content_type: Some("application/vnd.awiki.attachment+json".to_string()),
            notification_type: Some("attachment.ready".to_string()),
            reason: "unsupported notification method".to_string(),
        }),
        &session,
        None,
        None,
    );

    assert_eq!(result.route, NotificationRoute::Ignored);
    assert_eq!(result.applied_effect_count, 0);
    assert_eq!(status.warnings.len(), 1);
    assert!(status.warnings[0].starts_with(IM_EVENT_UNKNOWN_WARNING_PREFIX));
    assert!(status.warnings[0].contains("attachment.ready"));
    assert!(status.warnings[0].contains("application/vnd.awiki.attachment+json"));
    assert!(!status.warnings[0].contains("AttachmentInput"));
    Ok(())
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

fn open_db() -> StoreResult<Connection> {
    let db = Connection::open_in_memory().expect("open sqlite memory db");
    store::ensure_schema(&db)?;
    Ok(db)
}

fn session() -> NotificationSessionContext {
    NotificationSessionContext {
        identity_name: "bob".to_string(),
        did: "did:wba:example:agents:bob:e1_bob".to_string(),
        handle: "bob.example".to_string(),
    }
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
                ..MessageMetadata::default()
            },
        },
        attachment_summary: None,
        download_action: None,
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

fn string_field<'a>(value: &'a Value, field: &str) -> &'a str {
    value
        .get(field)
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("missing string field {field}: {value:?}"))
}

fn i64_field(value: &Value, field: &str) -> i64 {
    value
        .get(field)
        .and_then(Value::as_i64)
        .unwrap_or_else(|| panic!("missing integer field {field}: {value:?}"))
}
