use super::*;
use crate::internal::local_state::{messages, schema};
use serde_json::json;

fn record() -> MessageRecord {
    MessageRecord {
        owner_identity_id: "alice-id".into(), owner_did: "did:example:alice".into(),
        msg_id: "msg-retry".into(), conversation_id: "dm:did:example:bob".into(),
        thread_id: "dm:did:example:bob".into(), direction: 1,
        sender_did: "did:example:alice".into(), receiver_did: "did:example:bob".into(),
        content_type: "text/plain".into(), content: "hello".into(), is_read: true,
        sent_at: "2026-09-10T00:00:00Z".into(), stored_at: "2026-09-10T00:00:00Z".into(),
        metadata: json!({"operation_id": "op-retry", "wire_created_at": "2026-09-10T00:00:00Z", "delivery_state": "stored_locally"}).to_string(),
        ..Default::default()
    }.with_resolved_wire_thread("direct", "did:example:bob")
}

fn db() -> Connection {
    let db = Connection::open_in_memory().unwrap();
    schema::ensure_schema(&db).unwrap();
    db
}

#[test]
fn direct_send_intent_reuses_timestamp_and_preserves_accepted_state() {
    let mut db = db();
    let initial = prepare(&mut db, record()).unwrap();
    db.execute("UPDATE messages SET server_seq = 17, metadata = json_set(metadata, '$.delivery_state', 'accepted')", []).unwrap();
    let mut retry = record();
    retry.metadata =
        json!({"operation_id": "op-retry", "wire_created_at": "2026-09-11T00:00:00Z"}).to_string();
    assert_eq!(
        prepare(&mut db, retry).unwrap().created_at,
        initial.created_at
    );
    let seq: i64 = db
        .query_row("SELECT server_seq FROM messages", [], |r| r.get(0))
        .unwrap();
    assert_eq!(seq, 17);
}

#[test]
fn direct_send_intent_rejects_changed_content_operation_target_and_reused_operation() {
    let mut db = db();
    prepare(&mut db, record()).unwrap();
    let mut changed = vec![record(); 6];
    changed[0].content = "another message".into();
    changed[1].content_type = "text/markdown".into();
    changed[2].metadata = json!({"operation_id": "another-op"}).to_string();
    changed[3].msg_id = "another-message-id".into();
    changed[4].conversation_id = "dm:did:example:mallory".into();
    changed[5].sender_did = "did:example:mallory".into();
    for (index, candidate) in changed.into_iter().enumerate() {
        if index == 5 {
            // The proposed owner must match the selected sender, including after recovery.
            let mut candidate = candidate;
            candidate.owner_did = candidate.sender_did.clone();
            assert!(matches!(
                prepare(&mut db, candidate),
                Err(crate::ImError::MessageWireIdentityConflict { .. })
            ));
        } else {
            assert!(
                matches!(
                    prepare(&mut db, candidate),
                    Err(crate::ImError::MessageWireIdentityConflict { .. })
                ),
                "case {index}"
            );
        }
    }
    let mut other_owner = record();
    other_owner.owner_identity_id = "another-owner".into();
    prepare(&mut db, other_owner).unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM messages", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        2
    );
}

#[test]
fn direct_send_intent_legacy_or_corrupt_metadata_fails_closed() {
    for metadata in [r#"{"operation_id":"op-retry"}"#, "not-json"] {
        let mut db = db();
        prepare(&mut db, record()).unwrap();
        db.execute("UPDATE messages SET metadata = ?1", [metadata])
            .unwrap();
        assert!(prepare(&mut db, record()).is_err());
    }
}

#[test]
fn direct_send_intent_is_atomic_across_connections_and_reopen() {
    let path = std::env::temp_dir().join(format!(
        "awiki-send-intent-{}.sqlite",
        crate::internal::wire::common::generate_operation_id()
    ));
    {
        let db = Connection::open(&path).unwrap();
        schema::ensure_schema(&db).unwrap();
    }
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let threads = (0..2).map(|i| {
        let path = path.clone(); let barrier = barrier.clone();
        std::thread::spawn(move || {
            let mut db = Connection::open(path).unwrap();
            db.busy_timeout(std::time::Duration::from_secs(5)).unwrap();
            let mut record = record();
            record.metadata = json!({"operation_id": "op-retry", "wire_created_at": format!("2026-09-10T00:00:0{i}Z")}).to_string();
            barrier.wait();
            prepare(&mut db, record).unwrap().created_at
        })
    }).collect::<Vec<_>>();
    let results = threads
        .into_iter()
        .map(|thread| thread.join().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(results[0], results[1]);
    let mut reopened = Connection::open(&path).unwrap();
    assert_eq!(
        prepare(&mut reopened, record()).unwrap().created_at,
        results[0]
    );
    assert_eq!(
        reopened
            .query_row("SELECT count(*) FROM messages", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
    drop(reopened);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn mark_read_outgoing_is_explicit_and_foreign_owner_remains_not_found() {
    let mut db = db();
    prepare(&mut db, record()).unwrap();
    let resolve = |owner| {
        messages::resolve_v2_mark_read_watermarks_for_owner_identity(
            &db,
            owner,
            &["msg-retry".to_owned()],
        )
    };
    assert!(
        matches!(resolve("alice-id"), Err(crate::ImError::InvalidInput { field, .. }) if field.as_deref() == Some("message_ids.direction"))
    );
    assert!(matches!(
        resolve("another-owner"),
        Err(crate::ImError::MessageNotFound { .. })
    ));
}

#[test]
fn mark_read_mixed_batch_does_not_partially_mark_received_messages() {
    let mut db = db();
    prepare(&mut db, record()).unwrap();
    let mut incoming = record();
    incoming.msg_id = "received".into();
    incoming.direction = 0;
    incoming.is_read = false;
    incoming.sender_did = "did:example:bob".into();
    incoming.receiver_did = "did:example:alice".into();
    incoming.metadata = "{}".into();
    incoming.server_seq = Some(1);
    messages::upsert_message(&db, &incoming).unwrap();
    let received = messages::resolve_v2_mark_read_watermarks_for_owner_identity(
        &db,
        "alice-id",
        &["received".into()],
    )
    .unwrap();
    assert_eq!(received.watermarks[0].message_id, "received");
    assert!(matches!(
        messages::resolve_v2_mark_read_watermarks_for_owner_identity(
            &db,
            "alice-id",
            &["received".into(), "msg-retry".into()]
        ),
        Err(crate::ImError::InvalidInput { .. })
    ));
    assert!(!db
        .query_row(
            "SELECT is_read FROM messages WHERE msg_id = 'received'",
            [],
            |r| r.get::<_, bool>(0)
        )
        .unwrap());
}

#[test]
fn direct_send_intent_survives_remote_projection_without_client_only_metadata() {
    let mut db = db();
    let initial = prepare(&mut db, record()).unwrap();
    let mut hydrated = record();
    hydrated.server_seq = Some(42);
    hydrated.metadata =
        json!({"delivery_state": "accepted", "remote_thread_key": "remote-bob"}).to_string();
    messages::upsert_message(&db, &hydrated).unwrap();
    let replay = prepare(&mut db, record()).unwrap();
    assert_eq!(replay.created_at, initial.created_at);
    let metadata: String = db
        .query_row("SELECT metadata FROM messages", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&metadata).unwrap()["remote_thread_key"],
        "remote-bob"
    );
}

#[test]
fn direct_send_intent_does_not_claim_other_message_runtimes_operations() {
    let mut db = db();
    let mut group = record();
    group.msg_id = "group-send".into();
    group.conversation_id = "group:did:example:group".into();
    group.thread_id = group.conversation_id.clone();
    group.group_id = "did:example:group".into();
    group.group_did = group.group_id.clone();
    group.receiver_did.clear();
    group = group.with_resolved_wire_thread("group", "did:example:group");
    messages::upsert_message(&db, &group).unwrap();
    prepare(&mut db, record()).unwrap();
}
