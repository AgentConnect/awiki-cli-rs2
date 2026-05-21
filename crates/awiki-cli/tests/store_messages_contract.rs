use awiki_cli::store::{self, MessageRecord, StoreError, StoreResult};
use rusqlite::Connection;
use serde_json::Value;

#[test]
fn store_message_thread_view_and_secure_upsert_match_go() -> StoreResult<()> {
    let db = Connection::open_in_memory().expect("open sqlite memory db");
    store::ensure_schema(&db)?;

    let owner = "did:wba:awiki.ai:user:a";
    let peer = "did:wba:awiki.ai:user:b";
    let thread_id = store::make_thread_id(owner, peer, "");
    store::store_message(
        &db,
        MessageRecord {
            msg_id: "msg-1".to_string(),
            owner_did: owner.to_string(),
            thread_id: thread_id.clone(),
            direction: 0,
            sender_did: peer.to_string(),
            receiver_did: owner.to_string(),
            content_type: "text".to_string(),
            content: "hello".to_string(),
            credential_name: "default".to_string(),
            ..MessageRecord::default()
        },
    )?;

    let message = single_message(&db, owner, "msg-1")?;
    assert_eq!(string_field(&message, "content"), "hello");
    let threads = store::execute_sql(&db, "SELECT thread_id, unread_count FROM threads")?;
    assert_eq!(threads.len(), 1);
    assert_eq!(string_field(&threads[0], "thread_id"), thread_id);
    assert_eq!(i64_field(&threads[0], "unread_count"), 1);

    let secure_owner = "did:wba:awiki.ai:user:bob";
    let secure_peer = "did:wba:awiki.ai:user:alice";
    let secure_thread = store::make_thread_id(secure_owner, secure_peer, "");
    store::store_message(
        &db,
        MessageRecord {
            msg_id: "msg-secure-1".to_string(),
            owner_did: secure_owner.to_string(),
            thread_id: secure_thread.clone(),
            direction: 0,
            sender_did: secure_peer.to_string(),
            receiver_did: secure_owner.to_string(),
            content_type: "application/anp-direct-cipher+json".to_string(),
            content: r#"{"ciphertext_b64u":"raw"}"#.to_string(),
            is_read: true,
            credential_name: "bob".to_string(),
            ..MessageRecord::default()
        },
    )?;
    store::store_message(
        &db,
        MessageRecord {
            msg_id: "msg-secure-1".to_string(),
            owner_did: secure_owner.to_string(),
            thread_id: secure_thread.clone(),
            direction: 0,
            sender_did: secure_peer.to_string(),
            receiver_did: secure_owner.to_string(),
            content_type: "text/plain".to_string(),
            content: "decrypted hello".to_string(),
            server_seq: Some(42),
            is_e2ee: true,
            metadata: r#"{"decryption_state":"decrypted"}"#.to_string(),
            credential_name: "bob".to_string(),
            ..MessageRecord::default()
        },
    )?;
    let message = single_message(&db, secure_owner, "msg-secure-1")?;
    assert_eq!(string_field(&message, "content_type"), "text/plain");
    assert_eq!(string_field(&message, "content"), "decrypted hello");
    assert_eq!(i64_field(&message, "is_e2ee"), 1);
    assert_eq!(i64_field(&message, "is_read"), 1);
    assert_eq!(i64_field(&message, "server_seq"), 42);

    store::store_message(
        &db,
        MessageRecord {
            msg_id: "msg-secure-2".to_string(),
            owner_did: secure_owner.to_string(),
            thread_id: secure_thread.clone(),
            direction: 0,
            sender_did: secure_peer.to_string(),
            receiver_did: secure_owner.to_string(),
            content_type: "text/plain".to_string(),
            content: "already decrypted".to_string(),
            server_seq: Some(42),
            is_e2ee: true,
            metadata: r#"{"decryption_state":"decrypted"}"#.to_string(),
            credential_name: "bob".to_string(),
            ..MessageRecord::default()
        },
    )?;
    store::store_message(
        &db,
        MessageRecord {
            msg_id: "msg-secure-2".to_string(),
            owner_did: secure_owner.to_string(),
            thread_id: secure_thread,
            direction: 0,
            sender_did: secure_peer.to_string(),
            receiver_did: secure_owner.to_string(),
            content_type: "application/anp-direct-cipher+json".to_string(),
            content: r#"{"ciphertext_b64u":"raw"}"#.to_string(),
            server_seq: Some(43),
            metadata: r#"{"content_type":"application/anp-direct-cipher+json"}"#.to_string(),
            credential_name: "bob".to_string(),
            ..MessageRecord::default()
        },
    )?;
    let message = single_message(&db, secure_owner, "msg-secure-2")?;
    assert_eq!(string_field(&message, "content_type"), "text/plain");
    assert_eq!(string_field(&message, "content"), "already decrypted");
    assert_eq!(
        string_field(&message, "metadata"),
        r#"{"decryption_state":"decrypted"}"#
    );
    assert_eq!(i64_field(&message, "server_seq"), 43);

    Ok(())
}

#[test]
fn message_lookup_and_mark_read_respect_owner_like_go() -> StoreResult<()> {
    let db = Connection::open_in_memory().expect("open sqlite memory db");
    store::ensure_schema(&db)?;

    store::store_message(
        &db,
        direct_message(
            "did:owner-1",
            "did:peer",
            "shared-msg",
            0,
            false,
            "owner1",
            "2026-01-01T00:00:01Z",
        ),
    )?;
    store::store_message(
        &db,
        direct_message(
            "did:owner-2",
            "did:peer",
            "shared-msg",
            0,
            false,
            "owner2",
            "2026-01-01T00:00:02Z",
        ),
    )?;

    let rows = store::list_messages_by_ids(
        &db,
        "did:owner-1",
        &["shared-msg".to_string(), "missing".to_string()],
    )?;
    assert_eq!(rows.len(), 1);
    assert_eq!(string_field(&rows[0], "content"), "owner1");
    assert!(store::list_messages_by_ids(&db, "did:owner-1", &[])?.is_empty());

    let affected = store::mark_messages_read(
        &db,
        "did:owner-1",
        &["shared-msg".to_string(), "missing".to_string()],
    )?;
    assert_eq!(affected, 1);
    let owner_one = single_message(&db, "did:owner-1", "shared-msg")?;
    let owner_two = single_message(&db, "did:owner-2", "shared-msg")?;
    assert_eq!(i64_field(&owner_one, "is_read"), 1);
    assert_eq!(i64_field(&owner_two, "is_read"), 0);

    assert!(matches!(
        store::list_thread_messages(&db, "did:owner-1", "", 0),
        Err(StoreError::Invalid(_))
    ));
    let thread_id = store::make_thread_id("did:owner-1", "did:peer", "");
    let thread_rows = store::list_thread_messages(&db, "did:owner-1", &thread_id, 0)?;
    assert_eq!(thread_rows.len(), 1);
    assert_eq!(string_field(&thread_rows[0], "msg_id"), "shared-msg");

    Ok(())
}

#[test]
fn message_owner_identity_write_and_legacy_fallback_match_phase3d() -> StoreResult<()> {
    let db = Connection::open_in_memory().expect("open sqlite memory db");
    store::ensure_schema(&db)?;

    store::store_message(
        &db,
        direct_message(
            "did:owner-stable",
            "did:peer",
            "identity-row",
            0,
            false,
            "identity content",
            "2026-01-01T00:00:01Z",
        ),
    )?;
    store::store_message(
        &db,
        direct_message(
            "did:owner-legacy",
            "did:peer",
            "legacy-row",
            0,
            false,
            "legacy content",
            "2026-01-01T00:00:02Z",
        ),
    )?;
    db.execute(
        "UPDATE messages SET owner_identity_id = NULL WHERE msg_id = 'legacy-row'",
        [],
    )?;
    store::store_message(
        &db,
        MessageRecord {
            msg_id: "other-identity-row".to_string(),
            owner_identity_id: "other".to_string(),
            owner_did: "did:owner-legacy".to_string(),
            thread_id: store::make_thread_id("did:owner-legacy", "did:peer", ""),
            direction: 0,
            sender_did: "did:peer".to_string(),
            receiver_did: "did:owner-legacy".to_string(),
            content: "other identity".to_string(),
            credential_name: "other".to_string(),
            ..MessageRecord::default()
        },
    )?;

    let identity_value: String = db.query_row(
        "SELECT owner_identity_id FROM messages WHERE msg_id = 'identity-row'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(identity_value, "default");

    let rows = store::list_messages_by_ids_for_owner_identity(
        &db,
        "default",
        "did:owner-legacy",
        &[
            "identity-row".to_string(),
            "legacy-row".to_string(),
            "other-identity-row".to_string(),
        ],
    )?;
    assert_eq!(message_ids(&rows), vec!["identity-row", "legacy-row"]);

    let affected = store::mark_messages_read_for_owner_identity(
        &db,
        "default",
        "did:owner-legacy",
        &[
            "identity-row".to_string(),
            "legacy-row".to_string(),
            "other-identity-row".to_string(),
        ],
    )?;
    assert_eq!(affected, 2);
    let other_read: i64 = db.query_row(
        "SELECT is_read FROM messages WHERE msg_id = 'other-identity-row'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(other_read, 0);

    Ok(())
}

#[test]
fn inbox_notification_filters_match_go() -> StoreResult<()> {
    let db = Connection::open_in_memory().expect("open sqlite memory db");
    store::ensure_schema(&db)?;
    let owner = "did:owner";

    store::store_message(
        &db,
        direct_message(
            owner,
            "did:peer",
            "direct-unread",
            0,
            false,
            "incoming unread",
            "2026-01-01T00:00:01Z",
        ),
    )?;
    store::store_message(
        &db,
        direct_message(
            owner,
            "did:peer",
            "direct-read",
            0,
            true,
            "incoming read",
            "2026-01-01T00:00:03Z",
        ),
    )?;
    store::store_message(
        &db,
        direct_message(
            owner,
            "did:peer",
            "direct-outgoing",
            1,
            true,
            "outgoing",
            "2026-01-01T00:00:04Z",
        ),
    )?;
    store::store_message(
        &db,
        group_message(owner, "group-1", "group-message", "2026-01-01T00:00:06Z"),
    )?;
    store::store_message(
        &db,
        mail_notification(
            owner,
            "mail-content-type",
            "mail.notification",
            r#"{"source":"legacy"}"#,
            true,
            "2026-01-01T00:00:04Z",
        ),
    )?;
    store::store_message(
        &db,
        mail_notification(
            owner,
            "mail-metadata",
            "text",
            r#"{"source_kind":"mail","subject":"Mail subject"}"#,
            false,
            "2026-01-01T00:00:05Z",
        ),
    )?;

    let direct_only = store::list_inbox_messages(&db, owner, 0, "", false, false)?;
    assert_eq!(
        message_ids(&direct_only),
        vec!["direct-read", "direct-unread"]
    );

    let with_notifications = store::list_inbox_messages(&db, owner, 0, "", false, true)?;
    assert_eq!(
        message_ids(&with_notifications),
        vec![
            "mail-metadata",
            "mail-content-type",
            "direct-read",
            "direct-unread",
        ]
    );

    let notifications = store::list_notification_inbox_messages(&db, owner, 0, false)?;
    assert_eq!(
        message_ids(&notifications),
        vec!["mail-metadata", "mail-content-type"]
    );
    let unread_notifications = store::list_notification_inbox_messages(&db, owner, 0, true)?;
    assert_eq!(message_ids(&unread_notifications), vec!["mail-metadata"]);

    let notification_rows = store::list_notifications(&db, owner, 0)?;
    assert_eq!(
        message_ids(&notification_rows),
        vec!["mail-metadata", "mail-content-type"]
    );

    Ok(())
}

fn single_message(db: &Connection, owner_did: &str, msg_id: &str) -> StoreResult<Value> {
    let rows = store::list_messages_by_ids(db, owner_did, &[msg_id.to_string()])?;
    rows.into_iter()
        .next()
        .ok_or_else(|| StoreError::NotFound(format!("message not found: {msg_id}")))
}

fn direct_message(
    owner_did: &str,
    peer_did: &str,
    msg_id: &str,
    direction: i64,
    is_read: bool,
    content: &str,
    sent_at: &str,
) -> MessageRecord {
    let (sender_did, receiver_did) = if direction == 0 {
        (peer_did, owner_did)
    } else {
        (owner_did, peer_did)
    };
    MessageRecord {
        msg_id: msg_id.to_string(),
        owner_did: owner_did.to_string(),
        thread_id: store::make_thread_id(owner_did, peer_did, ""),
        direction,
        sender_did: sender_did.to_string(),
        receiver_did: receiver_did.to_string(),
        content: content.to_string(),
        is_read,
        sent_at: sent_at.to_string(),
        credential_name: "default".to_string(),
        ..MessageRecord::default()
    }
}

fn group_message(owner_did: &str, group_id: &str, msg_id: &str, sent_at: &str) -> MessageRecord {
    MessageRecord {
        msg_id: msg_id.to_string(),
        owner_did: owner_did.to_string(),
        thread_id: store::make_thread_id(owner_did, "", group_id),
        direction: 0,
        sender_did: "did:group-sender".to_string(),
        receiver_did: owner_did.to_string(),
        group_id: group_id.to_string(),
        content: "group content".to_string(),
        sent_at: sent_at.to_string(),
        credential_name: "default".to_string(),
        ..MessageRecord::default()
    }
}

fn mail_notification(
    owner_did: &str,
    msg_id: &str,
    content_type: &str,
    metadata: &str,
    is_read: bool,
    sent_at: &str,
) -> MessageRecord {
    MessageRecord {
        msg_id: msg_id.to_string(),
        owner_did: owner_did.to_string(),
        thread_id: format!("mail:{owner_did}"),
        direction: 0,
        sender_did: "did:wba:mail:system".to_string(),
        receiver_did: owner_did.to_string(),
        content_type: content_type.to_string(),
        content: msg_id.to_string(),
        title: "Mail subject".to_string(),
        sent_at: sent_at.to_string(),
        is_read,
        metadata: metadata.to_string(),
        credential_name: "default".to_string(),
        ..MessageRecord::default()
    }
}

fn message_ids(rows: &[Value]) -> Vec<&str> {
    rows.iter()
        .map(|row| string_field(row, "msg_id"))
        .collect::<Vec<_>>()
}

fn string_field<'a>(value: &'a Value, field: &str) -> &'a str {
    value
        .get(field)
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("missing string field {field}: {value:?}"))
}

fn i64_field(value: &Value, field: &str) -> i64 {
    value
        .get(field)
        .and_then(Value::as_i64)
        .unwrap_or_else(|| panic!("missing integer field {field}: {value:?}"))
}
