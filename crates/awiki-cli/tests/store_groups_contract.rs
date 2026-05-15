use awiki_cli::store::{
    self, GroupMemberRecord, GroupRecord, MessageRecord, StoreError, StoreResult,
};
use rusqlite::Connection;
use serde_json::Value;

#[test]
fn group_cache_helpers_store_query_touch_and_leave_projection() -> StoreResult<()> {
    let mut db = Connection::open_in_memory().expect("open sqlite memory db");
    store::ensure_schema(&db)?;
    let owner_did = "did:owner";
    let group_id = "did:group:one";

    store::upsert_group(
        &db,
        GroupRecord {
            owner_did: owner_did.to_string(),
            group_id: group_id.to_string(),
            group_did: group_id.to_string(),
            name: "Group One".to_string(),
            group_mode: "general".to_string(),
            slug: "group-one".to_string(),
            group_owner_did: owner_did.to_string(),
            my_role: "admin".to_string(),
            membership_status: "active".to_string(),
            join_enabled: Some(true),
            member_count: Some(2),
            last_synced_seq: Some(1),
            last_message_at: "2026-01-01T00:00:01Z".to_string(),
            stored_at: "2026-01-01T00:00:01Z".to_string(),
            credential_name: "default".to_string(),
            ..GroupRecord::default()
        },
    )?;

    store::replace_group_members(
        &mut db,
        owner_did,
        group_id,
        &[
            GroupMemberRecord {
                owner_did: owner_did.to_string(),
                group_id: group_id.to_string(),
                user_id: "user-member".to_string(),
                member_did: "did:member".to_string(),
                member_handle: "alice".to_string(),
                role: "member".to_string(),
                credential_name: "ignored-by-replace-default".to_string(),
                ..GroupMemberRecord::default()
            },
            GroupMemberRecord {
                owner_did: owner_did.to_string(),
                group_id: group_id.to_string(),
                user_id: "user-admin".to_string(),
                member_did: "did:admin".to_string(),
                member_handle: "zoe".to_string(),
                role: "admin".to_string(),
                status: "active".to_string(),
                sent_message_count: Some(7),
                ..GroupMemberRecord::default()
            },
            GroupMemberRecord {
                user_id: "  ".to_string(),
                ..GroupMemberRecord::default()
            },
        ],
        "default",
    )?;

    store::store_messages_batch(
        &mut db,
        &[
            group_message(owner_did, group_id, "group-seq1", 1, "2026-01-01T00:00:01Z"),
            group_message(owner_did, group_id, "group-seq3", 3, "2026-01-01T00:00:03Z"),
            group_message(
                owner_did,
                "did:group:other",
                "other-seq9",
                9,
                "2026-01-01T00:00:09Z",
            ),
        ],
    )?;

    let snapshot = store::get_group_snapshot(&db, owner_did, group_id)?;
    assert_eq!(string_field(&snapshot, "name"), "Group One");
    assert_eq!(i64_field(&snapshot, "join_enabled"), 1);

    let members = store::list_cached_group_members(&db, owner_did, group_id, 0)?;
    assert_eq!(members.len(), 2);
    assert_eq!(string_field(&members[0], "member_handle"), "zoe");
    assert_eq!(string_field(&members[1], "member_handle"), "alice");
    assert_eq!(
        string_field(&members[1], "credential_name"),
        "ignored-by-replace-default"
    );

    let messages = store::list_group_messages(&db, owner_did, group_id, 0, Some(1))?;
    assert_eq!(messages.len(), 1);
    assert_eq!(string_field(&messages[0], "msg_id"), "group-seq3");

    store::touch_group_after_message(
        &db,
        owner_did,
        group_id,
        group_id,
        "2026-01-01T00:00:03Z",
        Some(3),
        "default",
        r#"{"source":"group.list_messages"}"#,
    )?;
    let snapshot = store::get_group_snapshot(&db, owner_did, group_id)?;
    assert_eq!(string_field(&snapshot, "name"), "Group One");
    assert_eq!(i64_field(&snapshot, "last_synced_seq"), 3);
    assert_eq!(
        string_field(&snapshot, "last_message_at"),
        "2026-01-01T00:00:03Z"
    );

    store::mark_group_left(&mut db, owner_did, group_id, group_id, "default")?;
    let snapshot = store::get_group_snapshot(&db, owner_did, group_id)?;
    assert_eq!(string_field(&snapshot, "membership_status"), "left");
    assert_eq!(snapshot["my_role"], Value::Null);
    assert!(store::list_cached_group_members(&db, owner_did, group_id, 0)?.is_empty());

    Ok(())
}

#[test]
fn group_cache_helpers_validate_required_keys() {
    let mut db = Connection::open_in_memory().expect("open sqlite memory db");
    store::ensure_schema(&db).expect("schema");

    assert!(matches!(
        store::upsert_group(
            &db,
            GroupRecord {
                owner_did: "did:owner".to_string(),
                ..GroupRecord::default()
            },
        ),
        Err(StoreError::Invalid(_))
    ));
    assert!(matches!(
        store::upsert_group_member(
            &db,
            GroupMemberRecord {
                owner_did: "did:owner".to_string(),
                group_id: "group".to_string(),
                ..GroupMemberRecord::default()
            },
        ),
        Err(StoreError::Invalid(_))
    ));
    assert!(matches!(
        store::replace_group_members(&mut db, "did:owner", "", &[], "default"),
        Err(StoreError::Invalid(_))
    ));
    assert!(matches!(
        store::list_group_messages(&db, "did:owner", "", 0, None),
        Err(StoreError::Invalid(_))
    ));
    assert!(matches!(
        store::list_cached_group_members(&db, "did:owner", "", 0),
        Err(StoreError::Invalid(_))
    ));
}

fn group_message(
    owner_did: &str,
    group_id: &str,
    msg_id: &str,
    server_seq: i64,
    sent_at: &str,
) -> MessageRecord {
    MessageRecord {
        msg_id: msg_id.to_string(),
        owner_did: owner_did.to_string(),
        thread_id: store::make_thread_id(owner_did, "", group_id),
        direction: 0,
        sender_did: "did:sender".to_string(),
        group_id: group_id.to_string(),
        group_did: group_id.to_string(),
        content_type: "text".to_string(),
        content: msg_id.to_string(),
        server_seq: Some(server_seq),
        sent_at: sent_at.to_string(),
        credential_name: "default".to_string(),
        ..MessageRecord::default()
    }
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
