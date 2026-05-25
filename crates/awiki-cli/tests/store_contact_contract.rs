use awiki_cli::store::{self, ContactRecord, MessageRecord, StoreResult};
use rusqlite::Connection;

#[test]
fn contact_upsert_rebinds_current_handle_and_preserves_history() -> StoreResult<()> {
    let mut db = Connection::open_in_memory().expect("open sqlite memory db");
    store::ensure_schema(&db)?;

    store::upsert_contact(
        &mut db,
        ContactRecord {
            owner_did: "did:owner".to_string(),
            did: "did:peer-old".to_string(),
            handle: "alice".to_string(),
            source_type: "listener.direct_incoming".to_string(),
            credential_name: "default".to_string(),
            ..ContactRecord::default()
        },
    )?;
    store::upsert_contact(
        &mut db,
        ContactRecord {
            owner_did: "did:owner".to_string(),
            did: "did:peer-new".to_string(),
            handle: "alice".to_string(),
            source_type: "listener.direct_incoming".to_string(),
            credential_name: "default".to_string(),
            ..ContactRecord::default()
        },
    )?;

    let current = store::get_current_contact_by_handle(&db, "did:owner", "alice")?;
    assert_eq!(string_field(&current, "did"), "did:peer-new");
    let old_contact = store::get_contact_by_did(&db, "did:owner", "did:peer-old")?;
    assert!(old_contact["handle"].is_null());
    assert_eq!(
        store::resolve_contact_handle_by_did(&db, "did:owner", "did:peer-old")?,
        "alice"
    );
    assert_eq!(
        store::list_dids_by_handle(&db, "did:owner", "alice")?,
        vec!["did:peer-new".to_string(), "did:peer-old".to_string()]
    );

    Ok(())
}

#[test]
fn list_dids_by_handle_falls_back_to_contacts_without_history_bindings() -> StoreResult<()> {
    let db = Connection::open_in_memory().expect("open sqlite memory db");
    store::ensure_schema(&db)?;
    db.execute(
        r#"
INSERT INTO contacts
    (owner_did, did, handle, first_seen_at, last_seen_at, metadata)
VALUES (?1, ?2, ?3, ?4, ?5, ?6)"#,
        (
            "did:owner",
            "did:peer",
            "alice",
            "2026-01-01T00:00:00Z",
            "2026-01-01T00:00:00Z",
            r#"{"source":"seed"}"#,
        ),
    )?;

    assert_eq!(
        store::list_dids_by_handle(&db, "did:owner", "alice")?,
        vec!["did:peer".to_string()]
    );

    Ok(())
}

#[test]
fn list_direct_messages_by_peer_dids_filters_unread_inbox_only_and_deduplicates() -> StoreResult<()>
{
    let mut db = Connection::open_in_memory().expect("open sqlite memory db");
    store::ensure_schema(&db)?;
    let owner_did = "did:owner";
    let peer_one = "did:peer-1";
    let peer_two = "did:peer-2";

    store::store_messages_batch(
        &mut db,
        &[
            direct_message(
                owner_did,
                peer_one,
                "direct-unread",
                0,
                false,
                "2026-01-01T00:00:01Z",
            ),
            direct_message(
                owner_did,
                peer_one,
                "direct-outgoing",
                1,
                true,
                "2026-01-01T00:00:02Z",
            ),
            direct_message(
                owner_did,
                peer_two,
                "direct-read",
                0,
                true,
                "2026-01-01T00:00:03Z",
            ),
            MessageRecord {
                msg_id: "group-message".to_string(),
                owner_did: owner_did.to_string(),
                thread_id: store::make_thread_id(owner_did, "", "group-1"),
                direction: 0,
                sender_did: peer_one.to_string(),
                receiver_did: owner_did.to_string(),
                group_id: "group-1".to_string(),
                content: "group content".to_string(),
                sent_at: "2026-01-01T00:00:04Z".to_string(),
                credential_name: "default".to_string(),
                ..MessageRecord::default()
            },
        ],
    )?;

    let rows = store::list_direct_messages_by_peer_dids(
        &db,
        owner_did,
        &[
            format!("  {peer_one}  "),
            peer_one.to_string(),
            String::new(),
            peer_two.to_string(),
        ],
        0,
        false,
        false,
    )?;
    assert_eq!(rows.len(), 3);
    assert_eq!(string_field(&rows[0], "msg_id"), "direct-read");
    assert_eq!(string_field(&rows[1], "msg_id"), "direct-outgoing");
    assert_eq!(string_field(&rows[2], "msg_id"), "direct-unread");

    let filtered = store::list_direct_messages_by_peer_dids(
        &db,
        owner_did,
        &[peer_one.to_string(), peer_two.to_string()],
        0,
        true,
        true,
    )?;
    assert_eq!(filtered.len(), 1);
    assert_eq!(string_field(&filtered[0], "msg_id"), "direct-unread");
    assert!(
        store::list_direct_messages_by_peer_dids(&db, owner_did, &[], 0, false, false)?.is_empty()
    );

    Ok(())
}

fn direct_message(
    owner_did: &str,
    peer_did: &str,
    msg_id: &str,
    direction: i64,
    is_read: bool,
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
        content: msg_id.to_string(),
        is_read,
        sent_at: sent_at.to_string(),
        credential_name: "default".to_string(),
        ..MessageRecord::default()
    }
}

fn string_field<'a>(value: &'a serde_json::Value, field: &str) -> &'a str {
    value
        .get(field)
        .and_then(serde_json::Value::as_str)
        .unwrap_or_else(|| panic!("missing string field {field}: {value:?}"))
}
