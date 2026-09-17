use super::*;
use rusqlite::Connection;
use crate::{ids::GroupRef, messages::ThreadRef};

fn record(id: &str, owner: &str, group: &str, time: &str) -> MessageRecord {
    MessageRecord {
        msg_id: id.into(),
        owner_identity_id: owner.into(),
        owner_did: "did:example:owner".into(),
        conversation_id: format!("group:{group}"),
        thread_id: format!("group:{group}"),
        sender_did: "did:example:peer".into(),
        receiver_did: "did:example:owner".into(),
        group_id: group.into(),
        group_did: group.into(),
        content_type: "text/plain".into(),
        content: id.into(),
        hydration_state: MessageHydrationState::Hydrated,
        sent_at: time.into(),
        stored_at: time.into(),
        ..MessageRecord::default()
    }
}

fn fixture() -> Connection {
    let db = Connection::open_in_memory().unwrap();
    crate::internal::local_state::schema::ensure_schema(&db).unwrap();
    for (id, owner, group, time) in [
        ("a", "owner", "did:example:group", "2026-09-17T00:00:00Z"),
        ("b", "owner", "did:example:group", "2026-09-17T01:00:00Z"),
        ("c", "owner", "did:example:group", "2026-09-17T01:00:00Z"),
        ("d", "owner", "did:example:group", "2026-09-17T01:00:00Z"),
        (
            "future",
            "owner",
            "did:example:group",
            "2026-09-17T02:00:00Z",
        ),
        (
            "elsewhere",
            "owner",
            "did:example:other",
            "2026-09-17T00:00:00Z",
        ),
        (
            "foreign",
            "other",
            "did:example:group",
            "2026-09-17T00:00:00Z",
        ),
    ] {
        upsert_message(&db, &record(id, owner, group, time)).unwrap();
    }
    db
}

fn read(
    db: &Connection,
    anchor: &str,
    cursor: Option<&str>,
) -> crate::ImResult<ThreadLocalHistoryRecords> {
    list_messages_before_for_thread_ref_for_owner_identity(
        db,
        "owner",
        "did:example:owner",
        &ThreadRef::Group(GroupRef::parse("did:example:group").unwrap()),
        anchor,
        1,
        cursor,
    )
}

#[test]
fn anchored_history_pages_exclude_current_future_and_other_scopes() {
    let db = fixture();
    let first = read(&db, "c", None).unwrap();
    assert_eq!(
        first
            .records
            .iter()
            .map(|r| r.msg_id.as_str())
            .collect::<Vec<_>>(),
        ["b"]
    );
    assert!(first.has_more);
    let second = read(&db, "c", first.next_cursor.as_deref()).unwrap();
    assert_eq!(
        second
            .records
            .iter()
            .map(|r| r.msg_id.as_str())
            .collect::<Vec<_>>(),
        ["a"]
    );
    assert!(!second.has_more);
    assert!(read(&db, "a", None).unwrap().records.is_empty());
    let changed: i64 = db
        .query_row("SELECT COUNT(*) FROM messages WHERE is_read=1", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(changed, 0, "a history read must not mark messages read");
}

#[test]
fn anchored_history_rejects_wrong_scope_uncommitted_and_forward_cursors() {
    let db = fixture();
    for anchor in ["missing", "elsewhere", "foreign"] {
        assert!(read(&db, anchor, None).is_err(), "{anchor}");
    }
    db.execute(
        "UPDATE messages SET hydration_state='pending' WHERE msg_id='d'",
        [],
    )
    .unwrap();
    assert!(read(&db, "d", None).is_err());
    let later = record(
        "future",
        "owner",
        "did:example:group",
        "2026-09-17T02:00:00Z",
    );
    assert!(read(&db, "c", encode_local_history_cursor(&later).as_deref()).is_err());
    assert!(read(&db, "c", Some("not-a-cursor")).is_err());
}

#[test]
fn anchored_history_resolves_owner_scoped_message_alias() {
    let db = fixture();
    db.execute("INSERT INTO message_identity_aliases(owner_identity_id,alias_msg_id,canonical_msg_id,stored_at) VALUES('owner','wire-c','c','now')", []).unwrap();
    assert_eq!(read(&db, "wire-c", None).unwrap().records[0].msg_id, "b");
    db.execute("INSERT INTO message_identity_aliases(owner_identity_id,alias_msg_id,canonical_msg_id,stored_at) VALUES('other','foreign-alias','c','now')", []).unwrap();
    assert!(read(&db, "foreign-alias", None).is_err());
}
