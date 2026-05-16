use awiki_cli::runtime::listener_local_notification_flush::{
    flush_queued_local_notifications, LocalNotificationFlushTargetSession,
};
use awiki_cli::runtime::listener_local_notifications::{LocalNotification, LocalNotificationQueue};
use serde_json::{json, Map, Value};

#[derive(Debug, Clone, PartialEq)]
struct HandledNotification {
    context_id: String,
    session_id: String,
    notification: Map<String, Value>,
}

#[test]
fn nil_target_session_is_no_op_and_keeps_queue() {
    let mut queue = LocalNotificationQueue::default();
    let mut handled = Vec::new();
    queue.queue_local_notification(
        "did:awiki:alice",
        Some(notification([("body", json!("queued"))])),
    );

    flush_queued_local_notifications(&mut queue, None, |context_id, session, notification| {
        handled.push(handled_notification(context_id, session, notification));
    });

    assert!(handled.is_empty());
    assert_eq!(
        queue.flush_queued_local_notifications(Some("did:awiki:alice")),
        vec![notification([("body", json!("queued"))])]
    );
}

#[test]
fn missing_current_did_is_no_op_and_keeps_queue() {
    let mut queue = LocalNotificationQueue::default();
    let mut handled = Vec::new();
    queue.queue_local_notification(
        "did:awiki:alice",
        Some(notification([("body", json!("queued"))])),
    );
    let target_session =
        LocalNotificationFlushTargetSession::new("ctx-1", "session-1", None::<String>);

    flush_queued_local_notifications(
        &mut queue,
        Some(&target_session),
        |context_id, session, notification| {
            handled.push(handled_notification(context_id, session, notification));
        },
    );

    assert!(handled.is_empty());
    assert_eq!(
        queue.flush_queued_local_notifications(Some("did:awiki:alice")),
        vec![notification([("body", json!("queued"))])]
    );
}

#[test]
fn blank_current_did_is_no_op_and_keeps_queue() {
    let mut queue = LocalNotificationQueue::default();
    let mut handled = Vec::new();
    queue.queue_local_notification(
        "did:awiki:alice",
        Some(notification([("body", json!("queued"))])),
    );
    let target_session =
        LocalNotificationFlushTargetSession::new("ctx-1", "session-1", Some(" \t\n"));

    flush_queued_local_notifications(
        &mut queue,
        Some(&target_session),
        |context_id, session, notification| {
            handled.push(handled_notification(context_id, session, notification));
        },
    );

    assert!(handled.is_empty());
    assert_eq!(
        queue.flush_queued_local_notifications(Some("did:awiki:alice")),
        vec![notification([("body", json!("queued"))])]
    );
}

#[test]
fn exact_current_did_flush_deletes_queue_and_handles_in_append_order() {
    let mut queue = LocalNotificationQueue::default();
    let mut handled = Vec::new();
    queue.queue_local_notification(
        "did:awiki:alice",
        Some(notification([("seq", json!(1)), ("body", json!("first"))])),
    );
    queue.queue_local_notification(
        "did:awiki:bob",
        Some(notification([("seq", json!(99)), ("body", json!("other"))])),
    );
    queue.queue_local_notification(
        "did:awiki:alice",
        Some(notification([("seq", json!(2)), ("body", json!("second"))])),
    );
    let target_session =
        LocalNotificationFlushTargetSession::new("ctx-1", "session-1", Some("did:awiki:alice"));

    flush_queued_local_notifications(
        &mut queue,
        Some(&target_session),
        |context_id, session, notification| {
            handled.push(handled_notification(context_id, session, notification));
        },
    );

    assert_eq!(
        handled,
        vec![
            HandledNotification {
                context_id: "ctx-1".to_string(),
                session_id: "session-1".to_string(),
                notification: notification([("seq", json!(1)), ("body", json!("first"))]),
            },
            HandledNotification {
                context_id: "ctx-1".to_string(),
                session_id: "session-1".to_string(),
                notification: notification([("seq", json!(2)), ("body", json!("second"))]),
            },
        ]
    );
    assert!(queue
        .flush_queued_local_notifications(Some("did:awiki:alice"))
        .is_empty());
    assert_eq!(
        queue.flush_queued_local_notifications(Some("did:awiki:bob")),
        vec![notification([("seq", json!(99)), ("body", json!("other"))])]
    );
}

#[test]
fn wrong_or_trimmed_current_did_mismatch_does_not_flush_exact_queue() {
    let mut queue = LocalNotificationQueue::default();
    let mut handled = Vec::new();
    queue.queue_local_notification(
        "  did:awiki:alice  ",
        Some(notification([("body", json!("queued"))])),
    );
    let target_session =
        LocalNotificationFlushTargetSession::new("ctx-1", "session-1", Some("did:awiki:alice"));

    flush_queued_local_notifications(
        &mut queue,
        Some(&target_session),
        |context_id, session, notification| {
            handled.push(handled_notification(context_id, session, notification));
        },
    );

    assert!(handled.is_empty());
    assert!(queue
        .flush_queued_local_notifications(Some("did:awiki:alice"))
        .is_empty());
    assert_eq!(
        queue.flush_queued_local_notifications(Some("  did:awiki:alice  ")),
        vec![notification([("body", json!("queued"))])]
    );
}

fn handled_notification(
    context_id: &str,
    session: &LocalNotificationFlushTargetSession,
    notification: LocalNotification,
) -> HandledNotification {
    HandledNotification {
        context_id: context_id.to_string(),
        session_id: session.session_id.clone(),
        notification,
    }
}

fn notification<const N: usize>(entries: [(&str, Value); N]) -> Map<String, Value> {
    Map::from_iter(entries.map(|(key, value)| (key.to_string(), value)))
}
