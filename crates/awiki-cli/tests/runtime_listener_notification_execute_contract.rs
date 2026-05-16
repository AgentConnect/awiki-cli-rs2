use awiki_cli::runtime::host_notify::{HostNotificationData, HostNotificationEvent};
use awiki_cli::runtime::host_notify_sink::HostNotifySink;
use awiki_cli::runtime::listener::{self, HostNotifyStatus, Status};
use awiki_cli::runtime::listener_notification_execute::{
    execute_listener_notification, execute_listener_notification_with_status,
    HostNotifyStatusUpdate,
};
use awiki_cli::runtime::listener_notification_plan::{
    NotificationRoute, NotificationSessionContext, SecureNotificationEffect,
    SecureNotificationNormalization,
};
use awiki_cli::store::{self, StoreResult};
use rusqlite::Connection;
use serde_json::{json, Value};
use std::cell::RefCell;
use std::time::{SystemTime, UNIX_EPOCH};
use time::{Date, Month, OffsetDateTime, Time, UtcOffset};

#[test]
fn direct_notification_stores_syncs_enriches_and_dispatches_after_storage() -> anyhow::Result<()> {
    let mut db = open_db()?;
    let notification = direct_notification("direct-msg-001");
    let sink = RecordingSink::new();
    let mut lookup_calls = Vec::new();
    let mut lookup = |did: &str| -> anyhow::Result<Option<String>> {
        lookup_calls.push(did.to_string());
        Ok(Some("WBA://Bob.Remote".to_string()))
    };

    let result = execute_listener_notification(
        &mut db,
        &sink,
        &notification,
        &session(" WBA://Alice.Example "),
        SecureNotificationNormalization::KeepOriginal,
        Some(fixed_received_at()),
        Some(&mut lookup),
    );

    assert_eq!(result.route, NotificationRoute::DirectIncoming);
    assert_eq!(result.secure_effect, SecureNotificationEffect::NotSecure);
    assert_eq!(result.host_notify_last_error, None);
    assert_eq!(
        result.host_notify_status_update,
        HostNotifyStatusUpdate::ClearError
    );
    assert_eq!(result.failed_effect_count, 0);
    assert_eq!(
        result.applied_effects,
        vec![
            "sync_incoming_contact sender_did=did:bob source_type=direct.incoming source_group_id=",
            "apply_host_notification_handles sender_handle=bob recipient_handle=alice",
            "store_message msg_id=direct-msg-001",
            "dispatch_host_notification should_notify=true event_id=direct-msg-001",
        ]
    );
    assert_applied_before(
        &result.applied_effects,
        "store_message",
        "dispatch_host_notification",
    );
    assert_eq!(lookup_calls, vec!["did:bob"]);
    assert_eq!(
        store::resolve_contact_handle_by_did(&db, "did:alice", "did:bob")?,
        "bob"
    );

    let messages = store::list_thread_messages(
        &db,
        "did:alice",
        &store::make_thread_id("did:alice", "did:bob", ""),
        10,
    )?;
    assert_eq!(messages.len(), 1);
    assert_eq!(text(&messages[0], "msg_id"), "direct-msg-001");
    assert_eq!(text(&messages[0], "content"), "hello direct");

    let events = sink.events.borrow();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].id, "direct-msg-001");
    let HostNotificationData::Direct(data) = events[0].data.as_ref().expect("direct data") else {
        panic!("expected direct host event");
    };
    assert_eq!(data.sender_handle, "bob");
    assert_eq!(data.recipient_handle, "alice");

    Ok(())
}

#[test]
fn host_sink_failure_still_stores_message_and_reports_last_error() -> anyhow::Result<()> {
    let mut db = open_db()?;
    let sink = RecordingSink::failing("sink boom");
    let mut lookup =
        |_did: &str| -> anyhow::Result<Option<String>> { Ok(Some("bob.remote".to_string())) };

    let result = execute_listener_notification(
        &mut db,
        &sink,
        &direct_notification("direct-msg-002"),
        &session("alice"),
        SecureNotificationNormalization::KeepOriginal,
        Some(fixed_received_at()),
        Some(&mut lookup),
    );

    assert_eq!(result.route, NotificationRoute::DirectIncoming);
    assert_eq!(result.host_notify_last_error.as_deref(), Some("sink boom"));
    assert_eq!(
        result.host_notify_status_update,
        HostNotifyStatusUpdate::SetError("sink boom".to_string())
    );
    assert_eq!(result.failed_effect_count, 1);
    assert_eq!(
        result.failed_effects[0].effect,
        "dispatch_host_notification should_notify=true event_id=direct-msg-002"
    );
    assert_eq!(result.failed_effects[0].error, "sink boom");

    let messages = store::list_thread_messages(
        &db,
        "did:alice",
        &store::make_thread_id("did:alice", "did:bob", ""),
        10,
    )?;
    assert_eq!(messages.len(), 1);
    assert_eq!(text(&messages[0], "msg_id"), "direct-msg-002");
    assert_eq!(sink.events.borrow().len(), 1);

    Ok(())
}

#[test]
fn routable_notification_without_host_event_leaves_host_notify_status_unchanged(
) -> anyhow::Result<()> {
    let mut db = open_db()?;
    let sink = RecordingSink::new();

    let result = execute_listener_notification(
        &mut db,
        &sink,
        &json!({
            "method": "group.incoming",
            "params": {
                "meta": {
                    "message_id": "group-msg-no-sender",
                    "created_at": "2026-04-07T00:00:00Z",
                    "content_type": "text/plain",
                    "target": {"did": "did:alice"}
                },
                "body": {
                    "group_did": "did:group",
                    "group_event_seq": 8,
                    "text": "hello without sender"
                }
            }
        }),
        &session("alice"),
        SecureNotificationNormalization::KeepOriginal,
        Some(fixed_received_at()),
        None,
    );

    assert_eq!(result.route, NotificationRoute::GroupIncoming);
    assert_eq!(result.host_notify_last_error, None);
    assert_eq!(
        result.host_notify_status_update,
        HostNotifyStatusUpdate::Unchanged
    );
    assert_eq!(result.failed_effect_count, 0);
    assert_eq!(sink.events.borrow().len(), 0);
    assert!(result
        .applied_effects
        .iter()
        .any(|effect| effect == "dispatch_host_notification should_notify=false event_id="));

    let messages = store::list_group_messages(&db, "did:alice", "did:group", 10, None)?;
    assert_eq!(messages.len(), 1);
    assert_eq!(text(&messages[0], "msg_id"), "group-msg-no-sender");

    Ok(())
}

#[test]
fn host_notify_status_update_applies_go_changed_only_status_semantics() -> anyhow::Result<()> {
    let status_file = temp_status_file("host-notify-status-update");
    let mut status = Status {
        mode: "websocket".to_string(),
        status_file: path_string(&status_file),
        host_notify: HostNotifyStatus {
            enabled: true,
            sink: "capture".to_string(),
            last_error: "old sink error".to_string(),
            ..HostNotifyStatus::default()
        },
        ..Status::default()
    };

    assert!(HostNotifyStatusUpdate::SetError("sink boom".to_string()).apply_to_status(&mut status));
    let loaded = listener::read_status(&status.status_file)?;
    assert_eq!(loaded.host_notify.last_error, "sink boom");

    status.mode = "same-error-not-written".to_string();
    assert!(!HostNotifyStatusUpdate::SetError("sink boom".to_string()).apply_to_status(&mut status));
    let loaded = listener::read_status(&status.status_file)?;
    assert_eq!(loaded.mode, "websocket");
    assert_eq!(loaded.host_notify.last_error, "sink boom");

    status.mode = "skipped-not-written".to_string();
    assert!(!HostNotifyStatusUpdate::Unchanged.apply_to_status(&mut status));
    let loaded = listener::read_status(&status.status_file)?;
    assert_eq!(loaded.mode, "websocket");
    assert_eq!(loaded.host_notify.last_error, "sink boom");

    status.mode = "clear-written".to_string();
    assert!(HostNotifyStatusUpdate::ClearError.apply_to_status(&mut status));
    let loaded = listener::read_status(&status.status_file)?;
    assert_eq!(loaded.mode, "clear-written");
    assert!(loaded.host_notify.last_error.is_empty());

    status.mode = "empty-clear-not-written".to_string();
    assert!(!HostNotifyStatusUpdate::ClearError.apply_to_status(&mut status));
    let loaded = listener::read_status(&status.status_file)?;
    assert_eq!(loaded.mode, "clear-written");
    assert!(loaded.host_notify.last_error.is_empty());

    let mut unwritable = Status {
        status_file: path_string(
            &status_file
                .parent()
                .expect("status parent")
                .join("missing")
                .join("listener.status.json"),
        ),
        ..Status::default()
    };
    assert!(
        HostNotifyStatusUpdate::SetError("ignored write error".to_string())
            .apply_to_status(&mut unwritable)
    );
    assert_eq!(unwritable.host_notify.last_error, "ignored write error");

    Ok(())
}

#[test]
fn execute_with_status_applies_host_notify_outcome_like_go_dispatch() -> anyhow::Result<()> {
    let mut db = open_db()?;
    let status_file = temp_status_file("host-notify-execute-status");
    let mut status = execution_status(&status_file, "old sink error");

    let mut lookup =
        |_did: &str| -> anyhow::Result<Option<String>> { Ok(Some("bob.remote".to_string())) };
    let failing = RecordingSink::failing("sink boom");
    let result = execute_listener_notification_with_status(
        &mut db,
        &failing,
        &mut status,
        &direct_notification("direct-msg-status-fail"),
        &session("alice"),
        SecureNotificationNormalization::KeepOriginal,
        Some(fixed_received_at()),
        Some(&mut lookup),
    );

    assert_eq!(
        result.host_notify_status_update,
        HostNotifyStatusUpdate::SetError("sink boom".to_string())
    );
    assert!(result.host_notify_status_changed);
    let loaded = listener::read_status(&status.status_file)?;
    assert_eq!(loaded.host_notify.last_error, "sink boom");

    status.mode = "same-error-not-written".to_string();
    let mut lookup =
        |_did: &str| -> anyhow::Result<Option<String>> { Ok(Some("bob.remote".to_string())) };
    let result = execute_listener_notification_with_status(
        &mut db,
        &failing,
        &mut status,
        &direct_notification("direct-msg-status-repeat"),
        &session("alice"),
        SecureNotificationNormalization::KeepOriginal,
        Some(fixed_received_at()),
        Some(&mut lookup),
    );
    assert_eq!(
        result.host_notify_status_update,
        HostNotifyStatusUpdate::SetError("sink boom".to_string())
    );
    assert!(!result.host_notify_status_changed);
    let loaded = listener::read_status(&status.status_file)?;
    assert_eq!(loaded.mode, "websocket");
    assert_eq!(loaded.host_notify.last_error, "sink boom");

    status.mode = "clear-written".to_string();
    let success = RecordingSink::new();
    let mut lookup =
        |_did: &str| -> anyhow::Result<Option<String>> { Ok(Some("bob.remote".to_string())) };
    let result = execute_listener_notification_with_status(
        &mut db,
        &success,
        &mut status,
        &direct_notification("direct-msg-status-success"),
        &session("alice"),
        SecureNotificationNormalization::KeepOriginal,
        Some(fixed_received_at()),
        Some(&mut lookup),
    );
    assert_eq!(
        result.host_notify_status_update,
        HostNotifyStatusUpdate::ClearError
    );
    assert!(result.host_notify_status_changed);
    let loaded = listener::read_status(&status.status_file)?;
    assert_eq!(loaded.mode, "clear-written");
    assert!(loaded.host_notify.last_error.is_empty());

    status.host_notify.last_error = "keep old error".to_string();
    status.mode = "skipped-not-written".to_string();
    let skipped = execute_listener_notification_with_status(
        &mut db,
        &success,
        &mut status,
        &json!({
            "method": "group.incoming",
            "params": {
                "meta": {
                    "message_id": "group-msg-status-skipped",
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
        }),
        &session("alice"),
        SecureNotificationNormalization::KeepOriginal,
        Some(fixed_received_at()),
        None,
    );
    assert_eq!(
        skipped.host_notify_status_update,
        HostNotifyStatusUpdate::Unchanged
    );
    assert!(!skipped.host_notify_status_changed);
    let loaded = listener::read_status(&status.status_file)?;
    assert_eq!(loaded.mode, "clear-written");
    assert!(loaded.host_notify.last_error.is_empty());
    assert_eq!(status.host_notify.last_error, "keep old error");

    Ok(())
}

#[test]
fn group_state_changed_upserts_group_member_and_message_before_dispatch() -> anyhow::Result<()> {
    let mut db = open_db()?;
    let sink = RecordingSink::new();

    let result = execute_listener_notification(
        &mut db,
        &sink,
        &json!({
            "method": "group.state_changed",
            "params": {
                "meta": {"target": {"did": "did:alice"}},
                "body": {
                    "group_did": "did:group",
                    "group_event_seq": 3,
                    "event_id": "evt-3",
                    "subject_did": "did:bob",
                    "subject_method": "group.add",
                    "actor_did": "did:alice",
                    "membership_status": "active",
                    "changed_at": "2026-04-07T09:06:01Z"
                }
            }
        }),
        &session("alice"),
        SecureNotificationNormalization::KeepOriginal,
        Some(fixed_received_at()),
        None,
    );

    assert_eq!(result.route, NotificationRoute::GroupStateChanged);
    assert_eq!(
        result.host_notify_status_update,
        HostNotifyStatusUpdate::ClearError
    );
    assert_eq!(result.failed_effect_count, 0);
    assert_eq!(
        result.applied_effects,
        vec![
            "upsert_group group_id=did:group",
            "upsert_group_member group_id=did:group user_id=did:bob",
            "store_message msg_id=evt-3",
            "dispatch_host_notification should_notify=true event_id=evt-3",
        ]
    );
    assert_applied_before(
        &result.applied_effects,
        "upsert_group ",
        "dispatch_host_notification",
    );
    assert_applied_before(
        &result.applied_effects,
        "upsert_group_member",
        "dispatch_host_notification",
    );
    assert_applied_before(
        &result.applied_effects,
        "store_message",
        "dispatch_host_notification",
    );

    let group = store::get_group_snapshot(&db, "did:alice", "did:group")?;
    assert_eq!(text(&group, "group_did"), "did:group");
    assert_eq!(i64_value(&group, "last_synced_seq"), 3);

    let members = store::list_cached_group_members(&db, "did:alice", "did:group", 10)?;
    assert_eq!(members.len(), 1);
    assert_eq!(text(&members[0], "member_did"), "did:bob");
    assert_eq!(text(&members[0], "status"), "active");

    let messages = store::list_group_messages(&db, "did:alice", "did:group", 10, None)?;
    assert_eq!(messages.len(), 1);
    assert_eq!(text(&messages[0], "msg_id"), "evt-3");

    let events = sink.events.borrow();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].id, "evt-3");
    let HostNotificationData::GroupState(data) = events[0].data.as_ref().expect("group state data")
    else {
        panic!("expected group state host event");
    };
    assert_eq!(data.group_did, "did:group");
    assert_eq!(data.subject_did, "did:bob");

    Ok(())
}

struct RecordingSink {
    events: RefCell<Vec<HostNotificationEvent>>,
    fail_with: Option<String>,
}

impl RecordingSink {
    fn new() -> Self {
        Self {
            events: RefCell::new(Vec::new()),
            fail_with: None,
        }
    }

    fn failing(error: &str) -> Self {
        Self {
            fail_with: Some(error.to_string()),
            ..Self::new()
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
                "security_profile": "transport-protected",
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

fn text(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn i64_value(value: &Value, key: &str) -> i64 {
    value.get(key).and_then(Value::as_i64).unwrap_or_default()
}

fn assert_applied_before(effects: &[String], before: &str, after: &str) {
    let before_index = effects
        .iter()
        .position(|effect| effect.starts_with(before))
        .expect("before effect");
    let after_index = effects
        .iter()
        .position(|effect| effect.starts_with(after))
        .expect("after effect");
    assert!(
        before_index < after_index,
        "{before} must be applied before {after}: {effects:?}"
    );
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

fn path_string(path: &std::path::Path) -> String {
    path.to_string_lossy().into_owned()
}

fn fixed_received_at() -> OffsetDateTime {
    OffsetDateTime::new_in_offset(
        Date::from_calendar_date(2026, Month::April, 12).expect("date"),
        Time::from_hms(10, 30, 0).expect("time"),
        UtcOffset::UTC,
    )
}
