use super::*;

#[tokio::test]
async fn db_actor_initializes_schema_and_returns_version() {
    let fixture = Fixture::new();
    let db = LocalStateDb::open(fixture.sqlite_path()).await.unwrap();

    let version = db.current_schema_version().await.unwrap();

    assert_eq!(version, super::super::schema::SCHEMA_VERSION);
    db.shutdown().await.unwrap();
}

#[tokio::test]
async fn db_actor_backfill_owner_identity_ids_is_noop_for_v17_schema() {
    let fixture = Fixture::new();
    let db = LocalStateDb::open(fixture.sqlite_path()).await.unwrap();

    let updated = db
        .backfill_owner_identity_ids(vec![super::super::schema::OwnerIdentityBackfill {
            identity_id: "alice-id".to_string(),
            owner_did: "did:example:alice".to_string(),
            credential_names: vec!["alice".to_string()],
        }])
        .await
        .unwrap();

    assert_eq!(updated, 0);
    db.shutdown().await.unwrap();
}

#[tokio::test]
async fn db_actor_stores_classifies_marks_and_lists_messages() {
    let fixture = Fixture::new();
    let db = LocalStateDb::open(fixture.sqlite_path()).await.unwrap();

    db.store_messages(vec![
        super::super::messages::MessageRecord {
            msg_id: "direct-1".to_string(),
            owner_identity_id: "alice-id".to_string(),
            owner_did: "did:example:alice".to_string(),
            conversation_id: "dm:did:example:bob".to_string(),
            thread_id: "dm:did:example:bob".to_string(),
            direction: 0,
            sender_did: "did:example:bob".to_string(),
            receiver_did: "did:example:alice".to_string(),
            content_type: "text/plain".to_string(),
            content: "hello".to_string(),
            sent_at: "2026-05-21T00:00:01Z".to_string(),
            stored_at: "2026-05-21T00:00:01Z".to_string(),
            is_read: false,
            credential_name: "alice".to_string(),
            ..super::super::messages::MessageRecord::default()
        },
        super::super::messages::MessageRecord {
            msg_id: "group-1".to_string(),
            owner_identity_id: "alice-id".to_string(),
            owner_did: "did:example:alice".to_string(),
            conversation_id: "group:did:example:group".to_string(),
            thread_id: "group:did:example:group".to_string(),
            direction: 0,
            sender_did: "did:example:bob".to_string(),
            group_id: "did:example:group".to_string(),
            group_did: "did:example:group".to_string(),
            content_type: "text/plain".to_string(),
            content: "group".to_string(),
            sent_at: "2026-05-21T00:00:02Z".to_string(),
            stored_at: "2026-05-21T00:00:02Z".to_string(),
            is_read: false,
            credential_name: "alice".to_string(),
            ..super::super::messages::MessageRecord::default()
        },
    ])
    .await
    .unwrap();

    let classification = db
        .classify_mark_read_ids(
            "alice-id",
            "did:example:alice",
            vec!["direct-1".to_string(), "group-1".to_string()],
        )
        .await
        .unwrap();
    assert_eq!(classification.direct_ids, vec!["direct-1"]);
    assert_eq!(classification.group_ids, vec!["group-1"]);

    let unread_direct = db
        .list_unread_incoming_message_ids(
            "alice-id",
            "did:example:alice",
            crate::messages::ThreadRef::Direct(
                crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
            ),
            10,
        )
        .await
        .unwrap();
    assert_eq!(unread_direct.message_ids, vec!["direct-1"]);
    assert!(!unread_direct.truncated);

    let updated = db
        .mark_messages_read("alice-id", "did:example:alice", classification.local_ids())
        .await
        .unwrap();
    assert_eq!(updated, 2);

    let conversations = db
        .list_conversations(
            "alice-id",
            "did:example:alice",
            crate::messages::ConversationQuery {
                limit: crate::ids::PageLimit(10),
                cursor: None,
                include_groups: true,
                include_direct: true,
                unread_only: false,
            },
        )
        .await
        .unwrap();
    assert_eq!(conversations.len(), 2);
    assert!(conversations
        .iter()
        .any(|record| record.thread_id == "dm:did:example:bob"));
    assert!(conversations
        .iter()
        .any(|record| record.conversation_id == "group:did:example:group"));
    db.shutdown().await.unwrap();
}

#[tokio::test]
async fn db_actor_groups_projection_commands_use_existing_sql_helpers() {
    let fixture = Fixture::new();
    let db = LocalStateDb::open(fixture.sqlite_path()).await.unwrap();

    db.upsert_group(super::super::groups::GroupRecord {
        owner_identity_id: "alice-id".to_string(),
        owner_did: "did:example:alice".to_string(),
        group_id: "group-1".to_string(),
        group_did: "did:example:group:1".to_string(),
        name: "Async Group".to_string(),
        my_role: "owner".to_string(),
        membership_status: "active".to_string(),
        member_count: Some(2),
        last_message_at: "2026-05-21T00:00:03Z".to_string(),
        stored_at: "2026-05-21T00:00:00Z".to_string(),
        credential_name: "alice-id".to_string(),
        metadata: r#"{"message_security_profile":"group-e2ee"}"#.to_string(),
        ..super::super::groups::GroupRecord::default()
    })
    .await
    .unwrap();

    db.replace_group_members(
        "alice-id",
        "did:example:alice",
        "group-1",
        vec![
            super::super::groups::GroupMemberRecord {
                owner_identity_id: "alice-id".to_string(),
                owner_did: "did:example:alice".to_string(),
                group_id: "group-1".to_string(),
                user_id: "did:example:bob".to_string(),
                member_did: "did:example:bob".to_string(),
                member_handle: "bob.awiki.ai".to_string(),
                role: "member".to_string(),
                status: "active".to_string(),
                joined_at: "2026-05-21T00:00:01Z".to_string(),
                credential_name: "alice-id".to_string(),
                ..super::super::groups::GroupMemberRecord::default()
            },
            super::super::groups::GroupMemberRecord {
                owner_identity_id: "alice-id".to_string(),
                owner_did: "did:example:alice".to_string(),
                group_id: "group-1".to_string(),
                user_id: "did:example:carol".to_string(),
                member_did: "did:example:carol".to_string(),
                member_handle: "carol.awiki.ai".to_string(),
                role: "admin".to_string(),
                status: "active".to_string(),
                joined_at: "2026-05-21T00:00:02Z".to_string(),
                credential_name: "alice-id".to_string(),
                ..super::super::groups::GroupMemberRecord::default()
            },
        ],
        "alice-id",
    )
    .await
    .unwrap();

    db.store_messages(vec![super::super::messages::MessageRecord {
        msg_id: "group-msg-1".to_string(),
        owner_identity_id: "alice-id".to_string(),
        owner_did: "did:example:alice".to_string(),
        thread_id: "group:did:example:group:1".to_string(),
        direction: 0,
        sender_did: "did:example:bob".to_string(),
        group_id: "group-1".to_string(),
        group_did: "did:example:group:1".to_string(),
        content_type: "text/plain".to_string(),
        content: "hello group".to_string(),
        server_seq: Some(7),
        sent_at: "2026-05-21T00:00:04Z".to_string(),
        stored_at: "2026-05-21T00:00:04Z".to_string(),
        credential_name: "alice-id".to_string(),
        ..super::super::messages::MessageRecord::default()
    }])
    .await
    .unwrap();

    let snapshot = db
        .get_group_snapshot("alice-id", "did:example:alice", "did:example:group:1")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(snapshot["name"], "Async Group");
    assert_eq!(snapshot["membership_status"], "active");

    let members = db
        .list_cached_group_members("alice-id", "did:example:alice", "did:example:group:1", 10)
        .await
        .unwrap();
    assert_eq!(members.len(), 2);
    assert_eq!(members[0]["role"], "admin");
    assert_eq!(members[1]["member_did"], "did:example:bob");

    let messages = db
        .list_group_messages(
            "alice-id",
            "did:example:alice",
            "did:example:group:1",
            10,
            Some(1),
        )
        .await
        .unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0]["msg_id"], "did:example:group:1:7");

    let active_refs = db
        .list_active_group_refs("alice-id", "did:example:alice", 10)
        .await
        .unwrap();
    assert_eq!(active_refs, vec!["did:example:group:1"]);

    db.mark_group_left(
        "alice-id",
        "did:example:alice",
        "group-1",
        "did:example:group:1",
        "alice-id",
    )
    .await
    .unwrap();

    let active_refs = db
        .list_active_group_refs("alice-id", "did:example:alice", 10)
        .await
        .unwrap();
    assert!(active_refs.is_empty());
    let members = db
        .list_cached_group_members("alice-id", "did:example:alice", "did:example:group:1", 10)
        .await
        .unwrap();
    assert!(members.is_empty());

    db.shutdown().await.unwrap();
}

#[tokio::test]
async fn db_actor_contact_commands_use_existing_store_helpers() {
    let fixture = Fixture::new();
    let db = LocalStateDb::open(fixture.sqlite_path()).await.unwrap();

    db.upsert_contact(crate::internal::contact_store::records::ContactRecord {
        owner_identity_id: "alice-id".to_string(),
        owner_did: "did:example:alice".to_string(),
        did: "did:example:bob".to_string(),
        handle: "bob.awiki.ai".to_string(),
        name: "Bob".to_string(),
        relationship: "following".to_string(),
        followed: Some(true),
        messaged: Some(true),
        last_seen_at: "2026-05-21T00:00:02Z".to_string(),
        source_type: "directory.test".to_string(),
        credential_name: "alice-id".to_string(),
        ..crate::internal::contact_store::records::ContactRecord::default()
    })
    .await
    .unwrap();
    db.upsert_contact(crate::internal::contact_store::records::ContactRecord {
        owner_identity_id: "alice-id".to_string(),
        owner_did: "did:example:alice".to_string(),
        did: "did:example:carol".to_string(),
        handle: "carol.awiki.ai".to_string(),
        name: "Carol".to_string(),
        relationship: "none".to_string(),
        followed: Some(false),
        messaged: Some(false),
        last_seen_at: "2026-05-21T00:00:01Z".to_string(),
        source_type: "directory.test".to_string(),
        credential_name: "alice-id".to_string(),
        ..crate::internal::contact_store::records::ContactRecord::default()
    })
    .await
    .unwrap();

    let by_did = db
        .get_contact_by_did("alice-id", "did:example:alice", "did:example:bob")
        .await
        .unwrap();
    assert_eq!(by_did.handle, "bob.awiki.ai");
    assert_eq!(by_did.followed, Some(true));

    let by_handle = db
        .get_current_contact_by_handle("alice-id", "did:example:alice", "carol.awiki.ai")
        .await
        .unwrap();
    assert_eq!(by_handle.did, "did:example:carol");

    let contacts = db
        .list_contacts("alice-id", "did:example:alice", 10)
        .await
        .unwrap();
    assert_eq!(contacts.len(), 2);
    assert_eq!(contacts[0].did, "did:example:bob");
    assert_eq!(contacts[1].did, "did:example:carol");

    let history_candidates = db
        .list_contact_dids_for_message_history_recovery("alice-id", "did:example:alice", 10)
        .await
        .unwrap();
    assert_eq!(
        history_candidates,
        vec!["did:example:bob", "did:example:carol"]
    );

    let handle = db
        .resolve_contact_handle_by_did("alice-id", "did:example:alice", "did:example:bob")
        .await
        .unwrap();
    assert_eq!(handle, "bob.awiki.ai");
    let dids = db
        .list_dids_by_handle("alice-id", "did:example:alice", "bob.awiki.ai")
        .await
        .unwrap();
    assert_eq!(dids, vec!["did:example:bob"]);

    let event_id = db
        .append_relationship_event(
            crate::internal::contact_store::records::RelationshipEventRecord {
                owner_identity_id: "alice-id".to_string(),
                owner_did: "did:example:alice".to_string(),
                target_did: "did:example:bob".to_string(),
                target_handle: "bob.awiki.ai".to_string(),
                event_type: "followed".to_string(),
                source_type: "directory.test".to_string(),
                status: "applied".to_string(),
                credential_name: "alice-id".to_string(),
                ..crate::internal::contact_store::records::RelationshipEventRecord::default()
            },
        )
        .await
        .unwrap();
    assert!(!event_id.trim().is_empty());

    db.shutdown().await.unwrap();
}

#[tokio::test]
async fn db_actor_merge_recovered_handle_local_state_uses_actor_connection() {
    let fixture = Fixture::new();
    let db = LocalStateDb::open(fixture.sqlite_path()).await.unwrap();
    let old_did = "did:example:old-alice";
    let new_did = "did:example:new-alice";

    db.store_messages(vec![super::super::messages::MessageRecord {
        msg_id: "recover-msg-1".to_string(),
        owner_identity_id: "old-alice".to_string(),
        owner_did: old_did.to_string(),
        conversation_id: "dm:did:example:bob".to_string(),
        thread_id: "dm:did:example:bob".to_string(),
        direction: 1,
        sender_did: old_did.to_string(),
        receiver_did: "did:example:bob".to_string(),
        content_type: "text/plain".to_string(),
        content: "pre recovery".to_string(),
        sent_at: "2026-05-21T00:00:01Z".to_string(),
        stored_at: "2026-05-21T00:00:01Z".to_string(),
        credential_name: "old-alice".to_string(),
        ..super::super::messages::MessageRecord::default()
    }])
    .await
    .unwrap();
    db.queue_e2ee_outbox(crate::internal::store::e2ee_outbox::E2eeOutboxRecord {
        outbox_id: "recover-outbox-1".to_string(),
        owner_identity_id: "old-alice".to_string(),
        owner_did: old_did.to_string(),
        credential_name: "old-alice".to_string(),
        peer_did: "did:example:bob".to_string(),
        plaintext: "stale secret".to_string(),
        local_status: "failed".to_string(),
        created_at: "2026-05-21T00:00:02Z".to_string(),
        updated_at: "2026-05-21T00:00:02Z".to_string(),
        ..crate::internal::store::e2ee_outbox::E2eeOutboxRecord::default()
    })
    .await
    .unwrap();

    let (store_merge, e2ee_cleanup) = db
        .merge_recovered_handle_local_state(
            vec![old_did.to_string()],
            new_did,
            "alice-recovered-id",
            "alice-recovered",
        )
        .await
        .unwrap();

    assert_eq!(store_merge.get("messages"), Some(&0));
    assert_eq!(e2ee_cleanup.get("e2ee_outbox"), Some(&0));
    let messages = db
        .list_conversations(
            "old-alice",
            old_did,
            crate::messages::ConversationQuery {
                limit: crate::ids::PageLimit(10),
                cursor: None,
                include_groups: false,
                include_direct: true,
                unread_only: false,
            },
        )
        .await
        .unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].thread_id, "dm:did:example:bob");
    let old_outbox = db
        .list_e2ee_outbox(
            crate::internal::store::e2ee_outbox::E2eeOutboxOwnerScope {
                owner_identity_id: "old-alice".to_string(),
                owner_did: old_did.to_string(),
                credential_name: "old-alice".to_string(),
            },
            None,
        )
        .await
        .unwrap();
    assert_eq!(old_outbox.len(), 1);

    db.shutdown().await.unwrap();
}

#[tokio::test]
async fn db_actor_e2ee_outbox_commands_use_existing_store_helpers() {
    let fixture = Fixture::new();
    let db = LocalStateDb::open(fixture.sqlite_path()).await.unwrap();
    let alice = crate::internal::store::e2ee_outbox::E2eeOutboxOwnerScope {
        owner_identity_id: "alice-id".to_string(),
        owner_did: "did:example:alice".to_string(),
        credential_name: "alice".to_string(),
    };
    let charlie = crate::internal::store::e2ee_outbox::E2eeOutboxOwnerScope {
        owner_identity_id: "charlie-id".to_string(),
        owner_did: "did:example:charlie".to_string(),
        credential_name: "charlie".to_string(),
    };
    {
        let connection = super::super::open_writable(&fixture.sqlite_path()).unwrap();
        crate::internal::store::e2ee_outbox::queue_e2ee_outbox(
            &connection,
            crate::internal::store::e2ee_outbox::E2eeOutboxRecord {
                outbox_id: "outbox-1".to_string(),
                owner_identity_id: alice.owner_identity_id.clone(),
                owner_did: alice.owner_did.clone(),
                credential_name: alice.credential_name.clone(),
                peer_did: "did:example:bob".to_string(),
                plaintext: "secret plaintext".to_string(),
                local_status: "failed".to_string(),
                created_at: "2026-05-24T00:00:00Z".to_string(),
                updated_at: "2026-05-24T00:00:00Z".to_string(),
                ..crate::internal::store::e2ee_outbox::E2eeOutboxRecord::default()
            },
        )
        .unwrap();
    }

    let failed = db
        .list_e2ee_outbox(alice.clone(), Some("failed".to_string()))
        .await
        .unwrap();
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].outbox_id, "outbox-1");
    assert_eq!(failed[0].plaintext, "secret plaintext");

    assert!(db
        .retry_e2ee_outbox(charlie, "outbox-1")
        .await
        .unwrap()
        .is_none());
    let retried = db
        .retry_e2ee_outbox(alice.clone(), "outbox-1")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(retried.local_status, "queued");

    let dropped = db
        .drop_e2ee_outbox(alice, "outbox-1")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(dropped.local_status, "dropped");

    db.shutdown().await.unwrap();
}

#[tokio::test]
async fn db_actor_e2ee_outbox_queue_uses_existing_store_helper() {
    let fixture = Fixture::new();
    let db = LocalStateDb::open(fixture.sqlite_path()).await.unwrap();
    let scope = crate::internal::store::e2ee_outbox::E2eeOutboxOwnerScope {
        owner_identity_id: "alice-id".to_string(),
        owner_did: "did:example:alice".to_string(),
        credential_name: "alice".to_string(),
    };

    let outbox_id = db
        .queue_e2ee_outbox(crate::internal::store::e2ee_outbox::E2eeOutboxRecord {
            outbox_id: "outbox-queued-by-actor".to_string(),
            owner_identity_id: scope.owner_identity_id.clone(),
            owner_did: scope.owner_did.clone(),
            credential_name: scope.credential_name.clone(),
            peer_did: "did:example:bob".to_string(),
            original_type: "markdown".to_string(),
            plaintext: "secret queued plaintext".to_string(),
            local_status: "queued".to_string(),
            last_error_code: "pending_confirmation".to_string(),
            retry_hint: "retry".to_string(),
            metadata: r#"{"source":"direct-send"}"#.to_string(),
            ..crate::internal::store::e2ee_outbox::E2eeOutboxRecord::default()
        })
        .await
        .unwrap();

    assert_eq!(outbox_id, "outbox-queued-by-actor");
    let queued = db
        .list_e2ee_outbox(scope, Some("queued".to_string()))
        .await
        .unwrap();
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0].outbox_id, "outbox-queued-by-actor");
    assert_eq!(queued[0].original_type, "markdown");
    assert_eq!(queued[0].plaintext, "secret queued plaintext");
    assert_eq!(queued[0].last_error_code, "pending_confirmation");
    assert_eq!(queued[0].retry_hint, "retry");
    assert_eq!(queued[0].metadata, r#"{"source":"direct-send"}"#);

    db.shutdown().await.unwrap();
}

#[tokio::test]
async fn db_actor_e2ee_outbox_delivery_updates_are_owner_scoped() {
    let fixture = Fixture::new();
    let db = LocalStateDb::open(fixture.sqlite_path()).await.unwrap();
    let alice = crate::internal::store::e2ee_outbox::E2eeOutboxOwnerScope {
        owner_identity_id: "alice-id".to_string(),
        owner_did: "did:example:alice".to_string(),
        credential_name: "alice".to_string(),
    };
    let charlie = crate::internal::store::e2ee_outbox::E2eeOutboxOwnerScope {
        owner_identity_id: "charlie-id".to_string(),
        owner_did: "did:example:charlie".to_string(),
        credential_name: "charlie".to_string(),
    };
    {
        let connection = super::super::open_writable(&fixture.sqlite_path()).unwrap();
        crate::internal::store::e2ee_outbox::queue_e2ee_outbox(
            &connection,
            crate::internal::store::e2ee_outbox::E2eeOutboxRecord {
                outbox_id: "outbox-delivery".to_string(),
                owner_identity_id: alice.owner_identity_id.clone(),
                owner_did: alice.owner_did.clone(),
                credential_name: alice.credential_name.clone(),
                peer_did: "did:example:bob".to_string(),
                session_id: "old-session".to_string(),
                plaintext: "secret plaintext".to_string(),
                local_status: "queued".to_string(),
                attempt_count: 2,
                last_error_code: "previous_error".to_string(),
                retry_hint: "retry".to_string(),
                failed_msg_id: "failed-msg".to_string(),
                failed_server_seq: Some(41),
                metadata: "old-metadata".to_string(),
                created_at: "2026-05-24T00:00:00Z".to_string(),
                updated_at: "2026-05-24T00:00:00Z".to_string(),
                ..crate::internal::store::e2ee_outbox::E2eeOutboxRecord::default()
            },
        )
        .unwrap();
    }

    db.mark_e2ee_outbox_sent(
        charlie.clone(),
        "outbox-delivery",
        "wrong-session",
        "wrong-msg",
        Some(99),
        "wrong-metadata",
    )
    .await
    .unwrap();
    let still_queued = db
        .list_e2ee_outbox(alice.clone(), Some("queued".to_string()))
        .await
        .unwrap();
    assert_eq!(still_queued.len(), 1);
    assert_eq!(still_queued[0].session_id, "old-session");
    assert_eq!(still_queued[0].attempt_count, 2);

    db.mark_e2ee_outbox_sent(
        alice.clone(),
        "outbox-delivery",
        "session-new",
        "msg-new",
        Some(42),
        "sent-metadata",
    )
    .await
    .unwrap();
    let sent = db
        .list_e2ee_outbox(alice.clone(), Some("sent".to_string()))
        .await
        .unwrap();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].local_status, "sent");
    assert_eq!(sent[0].attempt_count, 3);
    assert_eq!(sent[0].session_id, "session-new");
    assert_eq!(sent[0].sent_msg_id, "msg-new");
    assert_eq!(sent[0].sent_server_seq, Some(42));
    assert_eq!(sent[0].metadata, "sent-metadata");
    assert!(sent[0].last_error_code.is_empty());
    assert!(sent[0].retry_hint.is_empty());
    assert!(sent[0].failed_msg_id.is_empty());
    assert_eq!(sent[0].failed_server_seq, None);
    assert!(!sent[0].last_attempt_at.trim().is_empty());

    db.set_e2ee_outbox_failure(
        charlie,
        "outbox-delivery",
        "wrong_error",
        "drop",
        "wrong-metadata",
    )
    .await
    .unwrap();
    let still_sent = db
        .list_e2ee_outbox(alice.clone(), Some("sent".to_string()))
        .await
        .unwrap();
    assert_eq!(still_sent.len(), 1);
    assert!(still_sent[0].last_error_code.is_empty());

    db.set_e2ee_outbox_failure(
        alice.clone(),
        "outbox-delivery",
        "network_timeout",
        "retry",
        "failure-metadata",
    )
    .await
    .unwrap();
    let failed = db
        .list_e2ee_outbox(alice, Some("failed".to_string()))
        .await
        .unwrap();
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].local_status, "failed");
    assert_eq!(failed[0].last_error_code, "network_timeout");
    assert_eq!(failed[0].retry_hint, "retry");
    assert_eq!(failed[0].metadata, "failure-metadata");
    assert_eq!(failed[0].attempt_count, 3);

    db.shutdown().await.unwrap();
}

#[tokio::test]
async fn db_actor_e2ee_outbox_mark_sent_and_store_message_is_transactional() {
    let fixture = Fixture::new();
    let db = LocalStateDb::open(fixture.sqlite_path()).await.unwrap();
    let alice = crate::internal::store::e2ee_outbox::E2eeOutboxOwnerScope {
        owner_identity_id: "alice-id".to_string(),
        owner_did: "did:example:alice".to_string(),
        credential_name: "alice".to_string(),
    };
    let charlie = crate::internal::store::e2ee_outbox::E2eeOutboxOwnerScope {
        owner_identity_id: "charlie-id".to_string(),
        owner_did: "did:example:charlie".to_string(),
        credential_name: "charlie".to_string(),
    };
    {
        let connection = super::super::open_writable(&fixture.sqlite_path()).unwrap();
        crate::internal::store::e2ee_outbox::queue_e2ee_outbox(
            &connection,
            crate::internal::store::e2ee_outbox::E2eeOutboxRecord {
                outbox_id: "outbox-tx".to_string(),
                owner_identity_id: alice.owner_identity_id.clone(),
                owner_did: alice.owner_did.clone(),
                credential_name: alice.credential_name.clone(),
                peer_did: "did:example:bob".to_string(),
                plaintext: "secret plaintext".to_string(),
                local_status: "queued".to_string(),
                created_at: "2026-05-24T00:00:00Z".to_string(),
                updated_at: "2026-05-24T00:00:00Z".to_string(),
                ..crate::internal::store::e2ee_outbox::E2eeOutboxRecord::default()
            },
        )
        .unwrap();
    }
    let message = super::super::messages::MessageRecord {
        msg_id: "msg-outbox-tx".to_string(),
        owner_identity_id: alice.owner_identity_id.clone(),
        owner_did: alice.owner_did.clone(),
        conversation_id: "dm:did:example:bob".to_string(),
        thread_id: "dm:did:example:bob".to_string(),
        direction: 1,
        sender_did: alice.owner_did.clone(),
        receiver_did: "did:example:bob".to_string(),
        content_type: "text/plain".to_string(),
        content: "secret plaintext".to_string(),
        sent_at: "2026-05-24T00:00:01Z".to_string(),
        stored_at: "2026-05-24T00:00:01Z".to_string(),
        is_e2ee: true,
        is_read: true,
        metadata: "sent-metadata".to_string(),
        credential_name: alice.credential_name.clone(),
        ..super::super::messages::MessageRecord::default()
    };

    let wrong_owner = db
        .mark_e2ee_outbox_sent_and_store_message(
            charlie,
            "outbox-tx",
            "session-wrong",
            "msg-wrong",
            Some(99),
            "wrong-metadata",
            message.clone(),
        )
        .await;
    assert!(matches!(
        wrong_owner,
        Err(crate::ImError::MessageNotFound { message_id }) if message_id == "outbox-tx"
    ));
    let queued = db
        .list_e2ee_outbox(alice.clone(), Some("queued".to_string()))
        .await
        .unwrap();
    assert_eq!(queued.len(), 1);
    {
        let connection = rusqlite::Connection::open(fixture.sqlite_path()).unwrap();
        let count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM messages WHERE msg_id = 'msg-outbox-tx'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
    }

    db.mark_e2ee_outbox_sent_and_store_message(
        alice.clone(),
        "outbox-tx",
        "session-ok",
        "msg-outbox-tx",
        Some(7),
        "sent-metadata",
        message,
    )
    .await
    .unwrap();
    let sent = db
        .list_e2ee_outbox(alice.clone(), Some("sent".to_string()))
        .await
        .unwrap();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].session_id, "session-ok");
    assert_eq!(sent[0].sent_msg_id, "msg-outbox-tx");
    assert_eq!(sent[0].sent_server_seq, Some(7));
    assert_eq!(sent[0].metadata, "sent-metadata");
    {
        let connection = rusqlite::Connection::open(fixture.sqlite_path()).unwrap();
        let (content, is_e2ee): (String, i64) = connection
            .query_row(
                "SELECT content, is_e2ee FROM messages WHERE msg_id = 'msg-outbox-tx'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(content, "secret plaintext");
        assert_eq!(is_e2ee, 1);
    }

    db.shutdown().await.unwrap();
}

#[tokio::test]
async fn db_actor_direct_secure_status_uses_existing_store_helpers() {
    let fixture = Fixture::new();
    let db = LocalStateDb::open(fixture.sqlite_path()).await.unwrap();
    let scope = crate::internal::secure_direct::status::DirectSecureStatusScope {
        owner_identity_id: "alice-id".to_string(),
        owner_did: "did:example:alice".to_string(),
    };
    {
        let connection = super::super::open_writable(&fixture.sqlite_path()).unwrap();
        let store =
            crate::internal::secure_direct::sqlite_store::SqliteDirectSecureStateStore::new(
                &connection,
            )
            .unwrap();
        store
            .upsert_session(
                &crate::internal::secure_direct::sqlite_store::DirectSessionRecord {
                    owner_identity_id: scope.owner_identity_id.clone(),
                    owner_did: scope.owner_did.clone(),
                    peer_did: "did:example:bob".to_string(),
                    session_id: "session-secret".to_string(),
                    state_blob: b"{}".to_vec(),
                    metadata_json: "{}".to_string(),
                    revision: 0,
                    created_at: "2026-05-24T00:00:00Z".to_string(),
                    updated_at: "2026-05-24T00:00:00Z".to_string(),
                },
            )
            .unwrap();
    }

    let status = db
        .direct_secure_status(
            scope,
            crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(status.state, crate::secure::DirectSecureState::Ready);
    assert!(status.can_send_secure);
    assert_eq!(
        status.resolved_peer.as_ref().map(crate::ids::Did::as_str),
        Some("did:example:bob")
    );
    assert!(!format!("{status:?}").contains("session-secret"));
    db.shutdown().await.unwrap();
}

#[tokio::test]
async fn db_actor_direct_secure_repair_removes_session_and_requeues_failed_outbox() {
    let fixture = Fixture::new();
    let db = LocalStateDb::open(fixture.sqlite_path()).await.unwrap();
    let scope = crate::internal::secure_direct::status::DirectSecureStatusScope {
        owner_identity_id: "alice-id".to_string(),
        owner_did: "did:example:alice".to_string(),
    };
    {
        let connection = super::super::open_writable(&fixture.sqlite_path()).unwrap();
        let store =
            crate::internal::secure_direct::sqlite_store::SqliteDirectSecureStateStore::new(
                &connection,
            )
            .unwrap();
        store
            .upsert_session(
                &crate::internal::secure_direct::sqlite_store::DirectSessionRecord {
                    owner_identity_id: scope.owner_identity_id.clone(),
                    owner_did: scope.owner_did.clone(),
                    peer_did: "did:example:bob".to_string(),
                    session_id: "session-to-repair".to_string(),
                    state_blob: b"secret-state".to_vec(),
                    metadata_json: "{}".to_string(),
                    revision: 0,
                    created_at: "2026-05-24T00:00:00Z".to_string(),
                    updated_at: "2026-05-24T00:00:00Z".to_string(),
                },
            )
            .unwrap();
        crate::internal::store::e2ee_outbox::queue_e2ee_outbox(
            &connection,
            crate::internal::store::e2ee_outbox::E2eeOutboxRecord {
                outbox_id: "outbox-direct-repair".to_string(),
                owner_identity_id: scope.owner_identity_id.clone(),
                owner_did: scope.owner_did.clone(),
                credential_name: "alice".to_string(),
                peer_did: "did:example:bob".to_string(),
                original_type: "text".to_string(),
                plaintext: "queued plaintext".to_string(),
                local_status: "failed".to_string(),
                last_error_code: "peer_error".to_string(),
                retry_hint: "retry".to_string(),
                ..Default::default()
            },
        )
        .unwrap();
    }

    let plan = db
        .direct_secure_repair(
            scope,
            crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
        )
        .await
        .unwrap();

    assert!(plan.removed_session);
    assert_eq!(plan.requeued_outbox_count, 1);
    assert_eq!(
        plan.status.state,
        crate::secure::DirectSecureState::Preparing
    );
    assert!(!format!("{plan:?}").contains("secret-state"));
    {
        let connection = super::super::open_writable(&fixture.sqlite_path()).unwrap();
        let store =
            crate::internal::secure_direct::sqlite_store::SqliteDirectSecureStateStore::new(
                &connection,
            )
            .unwrap();
        assert!(store
            .get_session("alice-id", "did:example:bob")
            .unwrap()
            .is_none());
        let status = connection
            .query_row(
                "SELECT local_status FROM e2ee_outbox WHERE outbox_id = 'outbox-direct-repair'",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap();
        assert_eq!(status, "queued");
    }
    db.shutdown().await.unwrap();
}

#[tokio::test]
async fn db_actor_prepare_direct_secure_prekeys_persists_local_state_and_returns_publish_request() {
    let fixture = Fixture::new();
    let db = LocalStateDb::open(fixture.sqlite_path()).await.unwrap();
    let identity = TestIdentity::new("alice.actor-prekeys.example", "alice");

    let result = db
        .prepare_direct_secure_prekeys(
            crate::internal::secure_direct::prepare::DirectSecurePrekeyPrepareInput {
                owner_identity_id: "alice-id".to_string(),
                owner_did: identity.did.clone(),
                identity_name: "alice".to_string(),
                signing_key_id: format!("{}#key-1", identity.did),
                agreement_key_id: format!("{}#key-3", identity.did),
                signing_private_pem: identity.signing_private_pem,
                agreement_private_pem: identity.agreement_private_pem,
                local_did_document: identity.document,
                peer: crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
            },
        )
        .await
        .unwrap();

    assert_eq!(
        result.status.state,
        crate::secure::DirectSecureState::WaitingForPeer
    );
    assert_eq!(
        result.publish_request.method,
        "direct.e2ee.publish_prekey_bundle"
    );
    let params = serde_json::Value::Object(result.publish_request.params.clone());
    assert!(params.pointer("/body/prekey_bundle").is_some());
    assert_eq!(
        params
            .pointer("/body/one_time_prekeys")
            .and_then(serde_json::Value::as_array)
            .map(Vec::len),
        Some(16)
    );
    assert!(!format!("{result:?}").contains("PRIVATE KEY"));
    {
        let connection = super::super::open_writable(&fixture.sqlite_path()).unwrap();
        let store =
            crate::internal::secure_direct::sqlite_store::SqliteDirectSecureStateStore::new(
                &connection,
            )
            .unwrap();
        assert!(store.active_signed_prekey("alice-id").unwrap().is_some());
        assert_eq!(
            store
                .list_available_one_time_prekeys("alice-id")
                .unwrap()
                .len(),
            16
        );
    }
    db.shutdown().await.unwrap();
}

#[tokio::test]
async fn db_actor_direct_secure_session_cas_rejects_stale_updates() {
    let fixture = Fixture::new();
    let db = LocalStateDb::open(fixture.sqlite_path()).await.unwrap();
    {
        let connection = super::super::open_writable(&fixture.sqlite_path()).unwrap();
        let store =
            crate::internal::secure_direct::sqlite_store::SqliteDirectSecureStateStore::new(
                &connection,
            )
            .unwrap();
        store
            .upsert_session(
                &crate::internal::secure_direct::sqlite_store::DirectSessionRecord {
                    owner_identity_id: "alice-id".to_string(),
                    owner_did: "did:example:alice".to_string(),
                    peer_did: "did:example:bob".to_string(),
                    session_id: "session-1".to_string(),
                    state_blob: b"state-v1".to_vec(),
                    metadata_json: "{}".to_string(),
                    revision: 0,
                    created_at: "2026-05-24T00:00:00Z".to_string(),
                    updated_at: "2026-05-24T00:00:00Z".to_string(),
                },
            )
            .unwrap();
    }

    let loaded = db
        .get_direct_secure_session("alice-id", "did:example:bob")
        .await
        .unwrap()
        .unwrap();
    let mut first_update = loaded.clone();
    first_update.state_blob = b"state-v2".to_vec();
    first_update.updated_at = "2026-05-24T00:00:01Z".to_string();
    let saved = db
        .save_direct_secure_session_if_revision(first_update, loaded.revision)
        .await
        .unwrap();
    let crate::internal::secure_direct::sqlite_store::DirectSessionCasResult::Saved(saved) = saved
    else {
        panic!("first update should save");
    };
    assert_eq!(saved.revision, loaded.revision + 1);

    let mut stale_update = loaded;
    stale_update.state_blob = b"stale-state".to_vec();
    stale_update.updated_at = "2026-05-24T00:00:02Z".to_string();
    let stale_revision = stale_update.revision;
    let stale = db
        .save_direct_secure_session_if_revision(stale_update, stale_revision)
        .await
        .unwrap();
    let crate::internal::secure_direct::sqlite_store::DirectSessionCasResult::Stale {
        current,
        expected_revision,
    } = stale
    else {
        panic!("second update should be stale");
    };
    assert_eq!(expected_revision, 0);
    let current = current.unwrap();
    assert_eq!(current.revision, 1);
    assert_eq!(current.state_blob, b"state-v2");
    db.shutdown().await.unwrap();
}

#[tokio::test]
async fn db_actor_incoming_direct_init_session_saves_session_and_consumes_opk_atomically() {
    let fixture = Fixture::new();
    let db = LocalStateDb::open(fixture.sqlite_path()).await.unwrap();
    {
        let connection = super::super::open_writable(&fixture.sqlite_path()).unwrap();
        let store =
            crate::internal::secure_direct::sqlite_store::SqliteDirectSecureStateStore::new(
                &connection,
            )
            .unwrap();
        store
            .upsert_one_time_prekey(
                &crate::internal::secure_direct::sqlite_store::DirectOneTimePrekeyRecord {
                    owner_identity_id: "alice-id".to_owned(),
                    owner_did: "did:example:alice".to_owned(),
                    key_id: "opk-init".to_owned(),
                    private_key_blob: b"private".to_vec(),
                    public_key_blob: b"public".to_vec(),
                    status:
                        crate::internal::secure_direct::sqlite_store::DirectPrekeyStatus::Available,
                    metadata_json: serde_json::json!({
                        "metadata": {
                            "key_id": "opk-init",
                            "public_key_b64u": "public",
                        }
                    })
                    .to_string(),
                    created_at: "2026-05-24T00:00:00Z".to_owned(),
                    consumed_at: String::new(),
                },
            )
            .unwrap();
    }

    let saved = db
        .save_incoming_direct_init_session(
            crate::internal::secure_direct::sqlite_store::DirectInitSessionCommit {
                record: crate::internal::secure_direct::sqlite_store::DirectSessionRecord {
                    owner_identity_id: "alice-id".to_owned(),
                    owner_did: "did:example:alice".to_owned(),
                    peer_did: "did:example:bob".to_owned(),
                    session_id: "session-init".to_owned(),
                    state_blob: b"state".to_vec(),
                    metadata_json: "{}".to_owned(),
                    revision: 0,
                    created_at: "2026-05-24T00:00:01Z".to_owned(),
                    updated_at: "2026-05-24T00:00:01Z".to_owned(),
                },
                expected_peer_revision: None,
                consume_one_time_prekey_id: Some("opk-init".to_owned()),
                consumed_at: "2026-05-24T00:00:02Z".to_owned(),
            },
        )
        .await
        .unwrap();

    assert!(matches!(
        saved,
        crate::internal::secure_direct::sqlite_store::DirectInitSessionCommitResult::Saved(_)
    ));
    {
        let connection = super::super::open_writable(&fixture.sqlite_path()).unwrap();
        let store =
            crate::internal::secure_direct::sqlite_store::SqliteDirectSecureStateStore::new(
                &connection,
            )
            .unwrap();
        assert!(store
            .get_session("alice-id", "did:example:bob")
            .unwrap()
            .is_some());
        let opk = store
            .get_one_time_prekey("alice-id", "opk-init")
            .unwrap()
            .unwrap();
        assert_eq!(
            opk.status,
            crate::internal::secure_direct::sqlite_store::DirectPrekeyStatus::Consumed
        );
        assert_eq!(opk.consumed_at, "2026-05-24T00:00:02Z");
    }
    db.shutdown().await.unwrap();
}

#[tokio::test]
async fn db_actor_shutdown_returns_closed_error_for_new_commands() {
    let fixture = Fixture::new();
    let db = LocalStateDb::open(fixture.sqlite_path()).await.unwrap();
    db.shutdown().await.unwrap();

    let err = db.current_schema_version().await.unwrap_err();

    assert_eq!(
        err,
        crate::ImError::LocalStateUnavailable {
            detail: "local state actor is closed".to_string(),
        }
    );
}

struct Fixture {
    root: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        Self {
            root: tempfile::tempdir().unwrap(),
        }
    }

    fn sqlite_path(&self) -> PathBuf {
        self.root.path().join("local").join("im.sqlite")
    }
}

struct TestIdentity {
    did: String,
    document: serde_json::Value,
    signing_private_pem: String,
    agreement_private_pem: String,
}

impl TestIdentity {
    fn new(domain: &str, label: &str) -> Self {
        let service = anp::authentication::build_agent_message_service_with_options(
            "#message",
            format!("https://{domain}/anp-im/rpc"),
            anp::authentication::AnpMessageServiceOptions::default()
                .with_service_did(format!("did:wba:{domain}")),
        );
        let bundle = anp::authentication::create_did_wba_document(
            domain,
            anp::authentication::DidDocumentOptions {
                path_segments: vec!["agents".to_owned(), label.to_owned()],
                domain: Some(domain.to_owned()),
                challenge: Some(format!("direct-prekeys-{label}")),
                services: vec![service],
                did_profile: anp::authentication::DidProfile::E1,
                ..Default::default()
            },
        )
        .unwrap();
        Self {
            did: bundle.did().unwrap().to_owned(),
            document: bundle.did_document.clone(),
            signing_private_pem: bundle.private_key_pem("key-1").unwrap().to_owned(),
            agreement_private_pem: bundle.private_key_pem("key-3").unwrap().to_owned(),
        }
    }
}
