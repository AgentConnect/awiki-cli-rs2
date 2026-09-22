use super::*;

#[test]
fn notify_offline_hydration_reducer_preserves_intent_in_public_and_stored_message() {
    let fixture = Fixture::new("notify-offline-hydration");
    let client = fixture.client();
    let event = crate::internal::wire::sync_v2::SyncEventV2 {
        event_id: "event-discovered".to_owned(),
        stream_epoch: "3".to_owned(),
        event_seq: "17".to_owned(),
        event_type: "message.created".to_owned(),
        schema_version: 1,
        ignore_safe: false,
        account_id: "account-1".to_owned(),
        recipient_device_id: None,
        origin_did: Some("did:example:bob".to_owned()),
        origin_device_id: Some("device-bob".to_owned()),
        aggregate_kind: "direct_message".to_owned(),
        aggregate_id: "message-discovered".to_owned(),
        state_version: None,
        thread_key: Some("remote-thread-bob".to_owned()),
        occurred_at: "2026-07-28T10:00:00Z".to_owned(),
        payload: json!({
            "message_kind": "direct_plain",
            "direction": "incoming",
            "sender_did_snapshot": "did:example:bob",
            "recipient_did_snapshot": "did:example:alice",
            "client_message_id": "client-message-discovered",
            "accepted_at": "2026-07-28T10:00:00.123456Z"
        }),
        source: None,
    };
    let hydrated_message = json!({
        "id": "message-discovered",
        "thread_kind": "direct",
        "sender_did": "did:example:bob",
        "receiver_did": "did:example:alice",
        "content_type": "text/plain",
        "server_seq": "17",
        "content": "已完成 · Task",
        "annotations": {"awiki.notify.v1":{"level":"urgent"}},
        "created_at": "2026-07-28T09:59:59Z"
    });
    let mut public_messages = BTreeMap::new();

    let apply = reduce_event(
        &client,
        &event,
        Some(&hydrated_message),
        None,
        &mut public_messages,
    )
    .unwrap();

    assert_eq!(apply.messages.len(), 1);
    assert_eq!(
        serde_json::from_str::<Value>(&apply.messages[0].metadata).unwrap()["notify_level"],
        "urgent"
    );
    assert_eq!(
        apply.messages[0].hydration_state,
        crate::internal::local_state::messages::MessageHydrationState::Hydrated
    );
    assert_eq!(apply.messages[0].sent_at, "2026-07-28T10:00:00.123456Z");
    assert_eq!(apply.thread_bindings.len(), 1);
    assert_eq!(
        apply.thread_bindings[0].remote_thread_key,
        "remote-thread-bob"
    );
    assert_eq!(apply.thread_bindings[0].thread_kind, "direct");
    assert_eq!(
        serde_json::from_str::<Value>(&apply.messages[0].metadata).unwrap()["remote_thread_key"],
        "remote-thread-bob"
    );
    let message = public_messages.get("event-discovered").unwrap();
    assert_eq!(
        message.sent_at.as_deref(),
        Some("2026-07-28T10:00:00.123456Z")
    );
    assert_eq!(
        message.direction,
        crate::messages::MessageDirection::Incoming
    );
    for (key, value) in [
        ("notify_level", "urgent"),
        ("stream_epoch", "3"),
        ("account_id", "account-1"),
        ("origin_device_id", "device-bob"),
        ("client_message_id", "client-message-discovered"),
        ("remote_thread_key", "remote-thread-bob"),
    ] {
        assert!(message
            .metadata
            .attributes
            .iter()
            .any(|attribute| attribute.key == key && attribute.value == value));
    }
}
