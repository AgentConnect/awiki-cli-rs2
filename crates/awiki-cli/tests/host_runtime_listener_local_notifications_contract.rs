use awiki_cli::host_runtime::listener_local_notifications::LocalNotificationQueue;
use serde_json::{json, Map, Value};

#[test]
fn queue_skips_blank_recipient_and_nil_notification_like_go() {
    let mut queue = LocalNotificationQueue::default();

    queue.queue_local_notification("   ", Some(notification([("body", json!("blank"))])));
    queue.queue_local_notification("did:awiki:alice", None);

    assert!(queue
        .flush_queued_local_notifications(Some("did:awiki:alice"))
        .is_empty());
}

#[test]
fn queue_stores_under_original_recipient_did_string_like_go() {
    let mut queue = LocalNotificationQueue::default();

    queue.queue_local_notification(
        "  did:awiki:alice  ",
        Some(notification([("body", json!("queued"))])),
    );

    assert!(queue
        .flush_queued_local_notifications(Some("did:awiki:alice"))
        .is_empty());
    assert_eq!(
        queue.flush_queued_local_notifications(Some("  did:awiki:alice  ")),
        vec![notification([("body", json!("queued"))])]
    );
}

#[test]
fn queue_appends_and_flush_preserves_order_like_go() {
    let mut queue = LocalNotificationQueue::default();

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

    assert_eq!(
        queue.flush_queued_local_notifications(Some("did:awiki:alice")),
        vec![
            notification([("seq", json!(1)), ("body", json!("first"))]),
            notification([("seq", json!(2)), ("body", json!("second"))]),
        ]
    );
    assert_eq!(
        queue.flush_queued_local_notifications(Some("did:awiki:bob")),
        vec![notification([("seq", json!(99)), ("body", json!("other"))])]
    );
}

#[test]
fn flush_skips_nil_and_blank_current_did_analogs_like_go() {
    let mut queue = LocalNotificationQueue::default();

    queue.queue_local_notification(
        "did:awiki:alice",
        Some(notification([("body", json!("queued"))])),
    );

    assert!(queue.flush_queued_local_notifications(None).is_empty());
    assert!(queue.flush_queued_local_notifications(Some("")).is_empty());
    assert!(queue
        .flush_queued_local_notifications(Some(" \t\n"))
        .is_empty());
    assert_eq!(
        queue.flush_queued_local_notifications(Some("did:awiki:alice")),
        vec![notification([("body", json!("queued"))])]
    );
}

#[test]
fn flush_deletes_exact_did_queue_and_second_flush_is_empty_like_go() {
    let mut queue = LocalNotificationQueue::default();

    queue.queue_local_notification(
        "did:awiki:alice",
        Some(notification([("body", json!("queued"))])),
    );

    assert_eq!(
        queue.flush_queued_local_notifications(Some("did:awiki:alice")),
        vec![notification([("body", json!("queued"))])]
    );
    assert!(queue
        .flush_queued_local_notifications(Some("did:awiki:alice"))
        .is_empty());
}

fn notification<const N: usize>(entries: [(&str, Value); N]) -> Map<String, Value> {
    Map::from_iter(entries.map(|(key, value)| (key.to_string(), value)))
}
