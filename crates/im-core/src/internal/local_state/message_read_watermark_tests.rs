use super::super::read_state::{
    get_thread_read_state, upsert_thread_read_state, ThreadReadStateRecord,
};
use super::*;
use rusqlite::Connection;

fn record(kind: &str, id: &str, seq: Option<i64>) -> MessageRecord {
    let group = kind == "group";
    let conversation = if group {
        "group:did:example:group"
    } else {
        "dm:peer-scope:v1:peer"
    };
    MessageRecord {
        msg_id: id.into(),
        owner_identity_id: "owner".into(),
        owner_did: "did:example:owner".into(),
        conversation_id: conversation.into(),
        thread_id: conversation.into(),
        wire_thread_kind: kind.into(),
        wire_thread_ref: if group {
            "did:example:group"
        } else {
            "did:example:peer"
        }
        .into(),
        wire_identity_resolution_state: "resolved".into(),
        direction: 0,
        sender_did: "did:example:peer".into(),
        receiver_did: "did:example:owner".into(),
        group_id: if group { "did:example:group" } else { "" }.into(),
        group_did: if group { "did:example:group" } else { "" }.into(),
        content_type: "text/plain".into(),
        content: id.into(),
        server_seq: seq,
        hydration_state: MessageHydrationState::Hydrated,
        sent_at: "2026-09-11T00:00:00Z".into(),
        stored_at: "2026-09-11T00:00:00Z".into(),
        ..MessageRecord::default()
    }
}

fn database_with_watermark(message: &MessageRecord) -> (Connection, ThreadReadStateRecord) {
    let db = Connection::open_in_memory().unwrap();
    let state = ThreadReadStateRecord {
        owner_identity_id: message.owner_identity_id.clone(),
        owner_did: message.owner_did.clone(),
        thread_scope: message.wire_thread_kind.clone(),
        thread_id: message.conversation_id.clone(),
        conversation_id: message.conversation_id.clone(),
        read_watermark_seq: Some("10".into()),
        remote_state_version: Some("38".into()),
        updated_at: "2026-09-11T00:00:01Z".into(),
        ..ThreadReadStateRecord::default()
    };
    upsert_thread_read_state(&db, &state).unwrap();
    (db, state)
}

#[test]
fn late_direct_and_group_messages_inherit_committed_watermark_without_changing_ack() {
    for kind in ["direct", "group"] {
        let recovered = record(kind, "recovered", Some(10));
        let (db, state) = database_with_watermark(&recovered);
        upsert_message(&db, &recovered).unwrap();
        upsert_message(&db, &recovered).unwrap();
        assert_eq!(
            db.query_row("SELECT COUNT(*), SUM(is_read) FROM messages", [], |row| Ok(
                (row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)
            ))
            .unwrap(),
            (1, 1),
            "{kind}: recovered messages within the durable read watermark must be read"
        );
        assert_eq!(
            db.query_row(
                "SELECT unread_count FROM conversation_summaries WHERE owner_identity_id='owner'",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
        assert_eq!(
            get_thread_read_state(&db, "owner", kind, &recovered.conversation_id)
                .unwrap()
                .unwrap(),
            state
        );
        let newer = record(kind, "post-anchor", Some(11));
        upsert_message(&db, &newer).unwrap();
        assert_eq!(
            db.query_row(
                "SELECT unread_count FROM conversation_summaries WHERE owner_identity_id='owner'",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
            1,
            "new messages above the watermark stay unread"
        );
    }
}

#[test]
fn hydration_uses_the_durable_sequence_even_if_the_later_record_omits_it() {
    let mut message = record("direct", "delayed-hydration", Some(9));
    let (db, _) = database_with_watermark(&message);
    message.hydration_state = MessageHydrationState::Discovered;
    message.content.clear();
    upsert_message(&db, &message).unwrap();
    assert!(!db
        .query_row("SELECT is_read FROM messages", [], |row| row
            .get::<_, bool>(0))
        .unwrap());
    message.hydration_state = MessageHydrationState::Hydrated;
    message.content = "hydrated".into();
    message.server_seq = None;
    upsert_message(&db, &message).unwrap();
    assert!(db
        .query_row("SELECT is_read FROM messages", [], |row| row
            .get::<_, bool>(0))
        .unwrap());
    assert_eq!(
        db.query_row(
            "SELECT unread_count FROM conversation_summaries",
            [],
            |row| row.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
}

#[test]
fn read_watermark_never_crosses_owner_conversation_or_unknown_sequence() {
    let base = record("direct", "base", Some(9));
    let (db, _) = database_with_watermark(&base);
    let mut other_owner = record("direct", "other-owner", Some(9));
    other_owner.owner_identity_id = "other-owner".into();
    let mut other_conversation = record("direct", "other-conversation", Some(9));
    other_conversation.conversation_id = "dm:peer-scope:v1:other".into();
    other_conversation.thread_id = other_conversation.conversation_id.clone();
    other_conversation.sender_did = "did:example:other".into();
    other_conversation.wire_thread_ref = "did:example:other".into();
    for message in [
        other_owner,
        other_conversation,
        record("direct", "unknown-sequence", None),
    ] {
        upsert_message(&db, &message).unwrap();
    }
    assert_eq!(
        db.query_row("SELECT COUNT(*), SUM(is_read) FROM messages", [], |row| Ok(
            (row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)
        ))
        .unwrap(),
        (3, 0)
    );
}

#[test]
fn ordinary_read_watermarks_never_consume_p5_device_inbox_sequences() {
    let mut encrypted = record("direct", "p5-after-plain", Some(3));
    encrypted.is_e2ee = true;
    encrypted.metadata = serde_json::json!({
        "p5_cache_profile": "anp.direct.e2ee.v2",
        "p5_cache_sender_did": "did:example:peer",
        "p5_cache_sender_device_id": "peer-device",
        "p5_cache_recipient_did": "did:example:owner",
        "p5_cache_recipient_device_id": "owner-device",
        "p5_cache_binding_digest": "authenticated-binding",
        "raw_message_id": "p5-raw"
    })
    .to_string();
    let (db, _) = database_with_watermark(&encrypted);
    upsert_message(&db, &encrypted).unwrap();
    upsert_message(&db, &encrypted).unwrap();
    let plain = record("direct", "ordinary", Some(11));
    upsert_message(&db, &plain).unwrap();
    assert_eq!(
        mark_thread_messages_read_up_to_seq(&db, "owner", &[plain.conversation_id.clone()], "11")
            .unwrap(),
        1
    );
    let unread: i64 = db
        .query_row(
            "SELECT unread_count FROM conversation_summaries WHERE owner_identity_id='owner'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        unread, 1,
        "new P5 message must stay unread across ordinary watermark application"
    );
    let resolution = resolve_v2_mark_read_watermarks_for_owner_identity(
        &db,
        "owner",
        &[encrypted.msg_id.clone()],
    )
    .unwrap();
    assert!(resolution.watermarks.is_empty());
    assert_eq!(
        resolution.local_only_message_ids,
        [encrypted.msg_id.clone()]
    );
    assert_eq!(
        mark_messages_read_for_owner_identity(
            &db,
            "owner",
            "did:example:owner",
            &resolution.local_only_message_ids
        )
        .unwrap(),
        1
    );
    upsert_message(&db, &encrypted).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT is_read FROM messages WHERE msg_id='p5-after-plain'",
            [],
            |row| row.get::<_, bool>(0)
        )
        .unwrap(),
        true
    );
}

#[test]
fn mixed_conversation_read_uses_authenticated_anchor_and_separate_lanes() {
    let db = Connection::open_in_memory().unwrap();
    let plain = record("direct", "plain-high-seq", Some(400));
    upsert_message(&db, &plain).unwrap();
    let mut encrypted = record("direct", "encrypted-low-seq", Some(3));
    encrypted.is_e2ee = true;
    encrypted.sent_at = "2026-09-11T00:01:00Z".into();
    encrypted.metadata = serde_json::json!({
        "p5_cache_profile": "anp.direct.e2ee.v2",
        "p5_cache_sender_did": "did:example:peer",
        "p5_cache_sender_device_id": "peer-device",
        "p5_cache_recipient_did": "did:example:owner",
        "p5_cache_recipient_device_id": "owner-device",
        "p5_cache_binding_digest": "authenticated-binding",
        "raw_message_id": "raw-low-seq"
    })
    .to_string();
    upsert_message(&db, &encrypted).unwrap();
    let mut future = encrypted.clone();
    future.msg_id = "future".into();
    future.sent_at = "2026-09-11T00:02:00Z".into();
    upsert_message(&db, &future).unwrap();
    let anchor = crate::messages::ReadWatermark {
        last_read_message_id: Some(crate::ids::MessageId::parse(&encrypted.msg_id).unwrap()),
        last_read_thread_seq: Some("999999".into()),
        read_at: Some(chrono::Utc::now()),
    };
    let plan = mixed_read::prepare(&db, "owner", &plain.conversation_id, Some(&anchor))
        .unwrap()
        .unwrap();
    assert_eq!(
        plan.ordinary
            .as_ref()
            .unwrap()
            .last_read_thread_seq
            .as_deref(),
        Some("400")
    );
    assert_eq!(plan.local_ids, [encrypted.msg_id.clone()]);
    assert_eq!(plan.effective.last_read_thread_seq, None);
    assert_eq!(
        plan.effective.read_at.unwrap().to_rfc3339(),
        "2026-09-11T00:01:00+00:00"
    );
    mark_messages_read_for_owner_identity(&db, "owner", "did:example:owner", &plan.local_ids)
        .unwrap();
    upsert_message(&db, &encrypted).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT is_read FROM messages WHERE msg_id='encrypted-low-seq'",
            [],
            |r| r.get::<_, bool>(0)
        )
        .unwrap(),
        true
    );
    assert_eq!(
        db.query_row(
            "SELECT is_read FROM messages WHERE msg_id='future'",
            [],
            |r| r.get::<_, bool>(0)
        )
        .unwrap(),
        false
    );
    let all = mixed_read::prepare(&db, "owner", &plain.conversation_id, None)
        .unwrap()
        .unwrap();
    assert_eq!(all.local_ids, ["future"]);
    let foreign = crate::messages::ReadWatermark {
        last_read_message_id: Some(
            crate::ids::MessageId::parse("not-in-this-conversation").unwrap(),
        ),
        ..anchor.clone()
    };
    assert!(mixed_read::prepare(&db, "owner", &plain.conversation_id, Some(&foreign)).is_err());
    let seq_only = crate::messages::ReadWatermark {
        last_read_message_id: None,
        ..anchor
    };
    assert!(
        mixed_read::prepare(&db, "owner", &plain.conversation_id, Some(&seq_only))
            .unwrap()
            .is_none()
    );
}
