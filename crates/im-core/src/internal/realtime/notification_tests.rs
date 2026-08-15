use serde_json::{json, Value};

fn direct_inline_notification() -> Value {
    json!({
        "jsonrpc": "2.0",
        "method": "sync.changed",
        "params": {
            "domains": ["message"],
            "reason": "direct_message_available"
        },
        "sync": {
            "schema_version": 3,
            "account_scan_seq_hint": "20502",
            "domain_versions": {},
            "event": {
                "event_id": "sev2d_01J",
                "stream_epoch": "3",
                "event_seq": "20502",
                "event_type": "message.created",
                "schema_version": 1,
                "ignore_safe": false,
                "account_id": "account_alice",
                "recipient_device_id": null,
                "origin_did": "did:wba:example.com:user:e1_bob",
                "origin_device_id": null,
                "aggregate_kind": "direct_message",
                "aggregate_id": "msg_901",
                "state_version": null,
                "thread_key": "dconv_01J",
                "occurred_at": "2026-07-28T12:00:00Z",
                "payload": {
                    "message_id": "msg_901",
                    "message_kind": "direct_plain",
                    "direction": "incoming",
                    "sender_did_snapshot": "did:wba:example.com:user:e1_bob",
                    "recipient_did_snapshot": "did:wba:example.com:user:e1_alice",
                    "local_account_id": "account_alice",
                    "peer_account_id": null,
                    "conversation_ref": "dconv_01J",
                    "thread_seq": "901",
                    "accepted_at": "2026-07-28T12:00:00.000000Z",
                    "content_inline": false,
                    "operation_id": "op_direct_901",
                    "client_message_id": "msg_901"
                },
                "source": {
                    "method": "direct.send",
                    "operation_id": "op_direct_901",
                    "client_message_id": "msg_901"
                }
            },
            "projection": {
                "id": "msg_901",
                "server_seq": "901",
                "thread_kind": "direct",
                "sender_did": "did:wba:example.com:user:e1_bob",
                "receiver_did": "did:wba:example.com:user:e1_alice",
                "content_type": "text/plain",
                "content": "hello",
                "created_at": "2026-07-28T12:00:00.000000Z",
                "sent_at": "2026-07-28T12:00:00Z",
                "is_read": false,
                "client_msg_id": "msg_901",
                "type": "text"
            }
        }
    })
}

#[test]
fn inline_v3_reuses_closed_event_and_hydrated_message_decoders() {
    let notification = direct_inline_notification();
    let parsed = super::parse_inline_sync_event_v3(&notification)
        .unwrap()
        .unwrap();

    assert_eq!(parsed.account_scan_seq_hint.as_deref(), Some("20502"));
    let event = parsed.ordinary_event.as_ref().unwrap();
    assert_eq!(event.event_id, "sev2d_01J");
    assert_eq!(event.aggregate_id, "msg_901");
    assert_eq!(parsed.projection["content"], "hello");
    let hint =
        crate::internal::realtime::projection::sync_hint_with_gap(&notification, Some("20500"))
            .unwrap();
    assert_eq!(hint.event_id.as_deref(), Some("sev2d_01J"));
    assert_eq!(hint.event_seq.as_deref(), Some("20502"));
    assert!(hint.sync_dirty);
    assert!(hint.gap_detected);

    let mut explicit_ordinary = notification;
    explicit_ordinary["sync"]["event"]["lane"] = json!("ordinary");
    assert_eq!(
        super::parse_inline_sync_event_v3(&explicit_ordinary)
            .unwrap()
            .unwrap()
            .lane,
        super::InlineSyncLaneV3::Ordinary
    );
}

#[test]
fn inline_v3_rejects_open_event_shape_and_projection_kind_conflict() {
    let mut extra_event_field = direct_inline_notification();
    extra_event_field["sync"]["event"]["unexpected"] = json!(true);
    assert!(super::parse_inline_sync_event_v3(&extra_event_field).is_err());

    let mut wrong_projection_kind = direct_inline_notification();
    wrong_projection_kind["sync"]["projection"]["thread_kind"] = json!("group");
    assert!(super::parse_inline_sync_event_v3(&wrong_projection_kind).is_err());
}

#[test]
fn inline_v3_rejects_event_ahead_of_hint_and_ignores_v2_hint() {
    let mut ahead = direct_inline_notification();
    ahead["sync"]["account_scan_seq_hint"] = json!("20501");
    assert!(super::parse_inline_sync_event_v3(&ahead).is_err());

    let v2 = json!({
        "method": "sync.changed",
        "sync": {"schema_version": 2}
    });
    assert_eq!(super::parse_inline_sync_event_v3(&v2).unwrap(), None);
}

#[test]
fn inline_v3_accepts_closed_p5_and_p6_lane_shapes() {
    let p5 = json!({
        "method": "sync.changed",
        "params": {"domains": ["message"], "reason": "direct_message_available"},
        "sync": {
            "schema_version": 3,
            "domain_versions": {},
            "event": {
                "lane": "p5_device",
                "event_id": "p5-delivery-1",
                "stream_epoch": "41",
                "event_seq": "37",
                "event_type": "p5.delivery.created"
            },
            "projection": {
                "meta": {
                    "profile": "anp.direct.e2ee.v2",
                    "security_profile": "direct-e2ee"
                },
                "body": {},
                "server_seq": 7
            }
        }
    });
    let parsed = super::parse_inline_sync_event_v3(&p5).unwrap().unwrap();
    assert_eq!(parsed.lane, super::InlineSyncLaneV3::P5Device);
    assert_eq!(parsed.event_id, "p5-delivery-1");
    assert!(parsed.ordinary_event.is_none());

    let mut p6 = p5;
    p6["sync"]["event"] = json!({
        "lane": "p6_group",
        "event_id": "p6-delivery-1",
        "stream_epoch": "42",
        "event_seq": "59",
        "event_type": "p6.delivery.created",
        "group_did": "did:wba:example.com:groups:team",
        "group_event_seq": "12"
    });
    p6["sync"]["projection"] = json!({
        "meta": {
            "profile": "anp.group.e2ee.v2",
            "security_profile": "group-e2ee"
        },
        "auth": {},
        "body": {}
    });
    let parsed = super::parse_inline_sync_event_v3(&p6).unwrap().unwrap();
    assert_eq!(parsed.lane, super::InlineSyncLaneV3::P6Group);
    assert_eq!(parsed.group_event_seq.as_deref(), Some("12"));

    p6["sync"]["projection"]["meta"]["profile"] = json!("anp.direct.e2ee.v2");
    assert!(super::parse_inline_sync_event_v3(&p6).is_err());
}
