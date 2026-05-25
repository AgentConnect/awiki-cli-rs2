use awiki_cli::runtime::host_notify::{HostNotificationData, HostNotificationEvent};
use awiki_cli::runtime::host_notify_sink::HostNotifySink;
use awiki_cli::runtime::listener::{self, HostNotifyStatus, Status};
use awiki_cli::runtime_legacy::listener_notification_execute::HostNotifyStatusUpdate;
use awiki_cli::runtime_legacy::listener_notification_handler::handle_listener_notification;
use awiki_cli::runtime_legacy::listener_notification_plan::{
    NotificationRoute, NotificationSessionContext, SecureNotificationNormalization,
};
use awiki_cli::store::{self, StoreResult};
use rusqlite::Connection;
use serde_json::{json, Value};
use std::cell::RefCell;
use std::time::{SystemTime, UNIX_EPOCH};
use time::{Date, Month, OffsetDateTime, Time, UtcOffset};

#[test]
fn handler_applies_host_notify_status_outcomes_like_go_supervisor() -> anyhow::Result<()> {
    let mut db = open_db()?;
    let status_file = temp_status_file("handler-host-notify-status");
    let mut status = execution_status(&status_file, "old sink error");

    let failing = RecordingSink::failing("sink boom");
    let mut lookup =
        |_did: &str| -> anyhow::Result<Option<String>> { Ok(Some("bob.remote".to_string())) };
    let failed = handle_listener_notification(
        &mut db,
        Some(&failing),
        &mut status,
        &direct_notification("handler-direct-fail"),
        &session("alice"),
        SecureNotificationNormalization::KeepOriginal,
        Some(fixed_received_at()),
        Some(&mut lookup),
    );

    assert_eq!(failed.route, NotificationRoute::DirectIncoming);
    assert_eq!(
        failed.host_notify_status_update,
        HostNotifyStatusUpdate::SetError("sink boom".to_string())
    );
    assert!(failed.host_notify_status_changed);
    let loaded = listener::read_status(&status.status_file)?;
    assert_eq!(loaded.host_notify.last_error, "sink boom");

    status.mode = "same-error-not-written".to_string();
    let mut lookup =
        |_did: &str| -> anyhow::Result<Option<String>> { Ok(Some("bob.remote".to_string())) };
    let repeated = handle_listener_notification(
        &mut db,
        Some(&failing),
        &mut status,
        &direct_notification("handler-direct-repeat"),
        &session("alice"),
        SecureNotificationNormalization::KeepOriginal,
        Some(fixed_received_at()),
        Some(&mut lookup),
    );
    assert_eq!(
        repeated.host_notify_status_update,
        HostNotifyStatusUpdate::SetError("sink boom".to_string())
    );
    assert!(!repeated.host_notify_status_changed);
    let loaded = listener::read_status(&status.status_file)?;
    assert_eq!(loaded.mode, "websocket");
    assert_eq!(loaded.host_notify.last_error, "sink boom");

    status.mode = "clear-written".to_string();
    let success = RecordingSink::new();
    let mut lookup =
        |_did: &str| -> anyhow::Result<Option<String>> { Ok(Some("bob.remote".to_string())) };
    let cleared = handle_listener_notification(
        &mut db,
        Some(&success),
        &mut status,
        &direct_notification("handler-direct-success"),
        &session("alice"),
        SecureNotificationNormalization::KeepOriginal,
        Some(fixed_received_at()),
        Some(&mut lookup),
    );
    assert_eq!(
        cleared.host_notify_status_update,
        HostNotifyStatusUpdate::ClearError
    );
    assert!(cleared.host_notify_status_changed);
    let loaded = listener::read_status(&status.status_file)?;
    assert_eq!(loaded.mode, "clear-written");
    assert!(loaded.host_notify.last_error.is_empty());

    let events = success.events.borrow();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].received_at, "2026-04-12T10:30:00Z");
    let HostNotificationData::Direct(data) = events[0].data.as_ref().expect("direct data") else {
        panic!("expected direct host event");
    };
    assert_eq!(data.sender_handle, "bob");
    assert_eq!(data.recipient_handle, "alice");

    Ok(())
}

#[test]
fn handler_leaves_status_for_ignored_and_skipped_dispatches() -> anyhow::Result<()> {
    let mut db = open_db()?;
    let status_file = temp_status_file("handler-skipped-status");
    let mut status = execution_status(&status_file, "keep old error");
    listener::write_status(&status.status_file, &status)?;
    let sink = RecordingSink::new();

    status.mode = "ignored-not-written".to_string();
    let ignored = handle_listener_notification(
        &mut db,
        Some(&sink),
        &mut status,
        &json!({"method": "unknown.notification"}),
        &session("alice"),
        SecureNotificationNormalization::KeepOriginal,
        Some(fixed_received_at()),
        None,
    );
    assert_eq!(ignored.route, NotificationRoute::Ignored);
    assert_eq!(
        ignored.host_notify_status_update,
        HostNotifyStatusUpdate::Unchanged
    );
    assert!(!ignored.host_notify_status_changed);
    let loaded = listener::read_status(&status.status_file)?;
    assert_eq!(loaded.mode, "websocket");
    assert_eq!(loaded.host_notify.last_error, "keep old error");

    status.mode = "skipped-not-written".to_string();
    let skipped = handle_listener_notification(
        &mut db,
        Some(&sink),
        &mut status,
        &group_notification_without_sender("handler-group-skipped"),
        &session("alice"),
        SecureNotificationNormalization::KeepOriginal,
        Some(fixed_received_at()),
        None,
    );
    assert_eq!(skipped.route, NotificationRoute::GroupIncoming);
    assert_eq!(
        skipped.host_notify_status_update,
        HostNotifyStatusUpdate::Unchanged
    );
    assert!(!skipped.host_notify_status_changed);
    assert_eq!(sink.events.borrow().len(), 0);
    let loaded = listener::read_status(&status.status_file)?;
    assert_eq!(loaded.mode, "websocket");
    assert_eq!(loaded.host_notify.last_error, "keep old error");

    let messages = store::list_group_messages(&db, "did:alice", "did:group", 10, None)?;
    assert_eq!(messages.len(), 1);
    assert_eq!(text(&messages[0], "msg_id"), "handler-group-skipped");

    Ok(())
}

#[test]
fn handler_without_host_notify_sink_preserves_local_effects_and_status() -> anyhow::Result<()> {
    let mut db = open_db()?;
    let status_file = temp_status_file("handler-no-host-sink");
    let mut status = execution_status(&status_file, "existing sink error");
    listener::write_status(&status.status_file, &status)?;

    status.mode = "nil-sink-not-written".to_string();
    let mut lookup =
        |_did: &str| -> anyhow::Result<Option<String>> { Ok(Some("bob.remote".to_string())) };
    let result = handle_listener_notification(
        &mut db,
        None,
        &mut status,
        &direct_notification("handler-no-sink"),
        &session("alice"),
        SecureNotificationNormalization::KeepOriginal,
        Some(fixed_received_at()),
        Some(&mut lookup),
    );

    assert_eq!(result.route, NotificationRoute::DirectIncoming);
    assert_eq!(
        result.host_notify_status_update,
        HostNotifyStatusUpdate::Unchanged
    );
    assert!(!result.host_notify_status_changed);
    assert_eq!(result.host_notify_last_error, None);
    assert_eq!(status.host_notify.last_error, "existing sink error");

    let loaded = listener::read_status(&status.status_file)?;
    assert_eq!(loaded.mode, "websocket");
    assert_eq!(loaded.host_notify.last_error, "existing sink error");

    let messages = store::list_thread_messages(
        &db,
        "did:alice",
        &store::make_thread_id("did:alice", "did:bob", ""),
        10,
    )?;
    assert_eq!(messages.len(), 1);
    assert_eq!(text(&messages[0], "msg_id"), "handler-no-sink");

    Ok(())
}

#[derive(Default)]
struct RecordingSink {
    events: RefCell<Vec<HostNotificationEvent>>,
    fail_with: Option<String>,
}

impl RecordingSink {
    fn new() -> Self {
        Self::default()
    }

    fn failing(message: &str) -> Self {
        Self {
            events: RefCell::new(Vec::new()),
            fail_with: Some(message.to_string()),
        }
    }
}

impl HostNotifySink for RecordingSink {
    fn notify(&self, event: &HostNotificationEvent) -> anyhow::Result<()> {
        self.events.borrow_mut().push(event.clone());
        match self.fail_with.as_ref() {
            Some(error) => anyhow::bail!("{error}"),
            None => Ok(()),
        }
    }

    fn close(&self) -> anyhow::Result<()> {
        Ok(())
    }
}

fn open_db() -> StoreResult<Connection> {
    let db = Connection::open_in_memory().expect("open sqlite memory db");
    store::ensure_schema(&db)?;
    Ok(db)
}

fn session(handle: &str) -> NotificationSessionContext {
    NotificationSessionContext {
        identity_name: "alice".to_string(),
        did: " did:alice ".to_string(),
        handle: handle.to_string(),
    }
}

fn direct_notification(message_id: &str) -> Value {
    json!({
        "method": "direct.incoming",
        "params": {
            "meta": {
                "sender_did": "did:bob",
                "message_id": message_id,
                "created_at": "2026-04-07T00:00:00Z",
                "content_type": "text/plain",
                "target": {
                    "kind": "agent",
                    "did": "did:alice"
                }
            },
            "body": {
                "text": "hello direct"
            }
        }
    })
}

fn group_notification_without_sender(message_id: &str) -> Value {
    json!({
        "method": "group.incoming",
        "params": {
            "meta": {
                "message_id": message_id,
                "created_at": "2026-04-07T00:00:00Z",
                "content_type": "text/plain",
                "target": {"did": "did:alice"}
            },
            "body": {
                "group_did": "did:group",
                "group_event_seq": 9,
                "text": "hello without sender"
            }
        }
    })
}

fn execution_status(status_file: &std::path::Path, last_error: &str) -> Status {
    Status {
        mode: "websocket".to_string(),
        status_file: path_string(status_file),
        host_notify: HostNotifyStatus {
            enabled: true,
            sink: "capture".to_string(),
            last_error: last_error.to_string(),
            ..HostNotifyStatus::default()
        },
        ..Status::default()
    }
}

fn temp_status_file(prefix: &str) -> std::path::PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "awiki-cli-rs2-{prefix}-{}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&root).expect("create temp status root");
    root.join("listener.status.json")
}

fn path_string(path: &std::path::Path) -> String {
    path.to_string_lossy().into_owned()
}

fn text(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn fixed_received_at() -> OffsetDateTime {
    OffsetDateTime::new_in_offset(
        Date::from_calendar_date(2026, Month::April, 12).expect("date"),
        Time::from_hms(10, 30, 0).expect("time"),
        UtcOffset::UTC,
    )
}
