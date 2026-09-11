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
    assert_eq!(
        db.query_row("SELECT is_read FROM messages", [], |row| row
            .get::<_, bool>(0))
            .unwrap(),
        false
    );
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
