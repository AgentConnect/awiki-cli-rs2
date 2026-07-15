use super::*;

#[test]
fn local_state_schema_creates_identity_owned_tables_views_and_version() {
    let db = Connection::open_in_memory().unwrap();

    ensure_schema(&db).unwrap();

    assert_eq!(current_schema_version(&db).unwrap(), SCHEMA_VERSION);
    for object in [
        ("table", "contacts"),
        ("table", "contact_handle_bindings"),
        ("table", "messages"),
        ("table", "conversation_summaries"),
        ("table", "groups"),
        ("table", "group_members"),
        ("table", "relationship_events"),
        ("table", "e2ee_outbox"),
        ("table", "identity_did_history"),
        ("table", "direct_e2ee_sessions"),
        ("table", "direct_e2ee_signed_prekeys"),
        ("table", "direct_e2ee_one_time_prekeys"),
        ("table", "attachment_manifest_cache"),
        ("table", "sync_state"),
        ("table", "thread_read_state"),
        ("table", "message_identity_aliases"),
        ("table", "direct_peer_routes"),
        ("table", "group_rebind_outbox"),
        ("table", "group_rebind_p6_jobs"),
        ("view", "threads"),
        ("view", "inbox"),
        ("view", "outbox"),
    ] {
        assert_schema_object_exists(&db, object.0, object.1);
    }
    assert_index_exists(&db, "idx_messages_owner_identity_thread");
    assert_index_exists(&db, "idx_messages_owner_identity_conversation");
    assert_index_exists(&db, "idx_conversation_summaries_owner_last");
    assert_index_exists(&db, "idx_conversation_summaries_owner_last_desc");
    assert_index_exists(&db, "idx_conversation_summaries_owner_unread_last");
    assert_index_exists(&db, "idx_groups_owner_identity_status_last_message");
    assert_index_exists(&db, "idx_identity_did_history_current");
    assert_index_exists(&db, "idx_identity_did_history_live_did_unique");
    assert_index_exists(&db, "idx_direct_e2ee_sessions_owner_updated");
    assert_index_exists(&db, "idx_direct_e2ee_signed_prekeys_owner_status");
    assert_index_exists(&db, "idx_direct_e2ee_one_time_prekeys_owner_status");
    assert_index_exists(&db, "idx_attachment_manifest_cache_owner_thread");
    assert_index_exists(&db, "idx_sync_state_owner_kind");
    assert_index_exists(&db, "idx_thread_read_state_owner_pending");
    assert_index_exists(&db, "idx_thread_read_state_owner_conversation");
    assert_index_exists(&db, "idx_message_identity_aliases_owner_canonical");
    assert_index_exists(&db, "idx_direct_peer_routes_owner_did");
    assert_index_exists(&db, "idx_group_rebind_outbox_resume");
    assert_index_exists(&db, "idx_group_rebind_p6_resume");
    for table in [
        "contacts",
        "contact_handle_bindings",
        "messages",
        "conversation_summaries",
        "groups",
        "group_members",
        "relationship_events",
        "e2ee_outbox",
        "identity_did_history",
        "direct_e2ee_sessions",
        "direct_e2ee_signed_prekeys",
        "direct_e2ee_one_time_prekeys",
        "attachment_manifest_cache",
        "sync_state",
        "thread_read_state",
        "message_identity_aliases",
        "direct_peer_routes",
        "group_rebind_outbox",
        "group_rebind_p6_jobs",
    ] {
        assert_column_exists(&db, table, "owner_identity_id");
    }
    assert_column_exists(&db, "direct_e2ee_sessions", "revision");
    for (table, key_columns) in [
        ("contacts", vec!["owner_identity_id", "did"]),
        (
            "contact_handle_bindings",
            vec!["owner_identity_id", "handle", "did"],
        ),
        ("messages", vec!["owner_identity_id", "msg_id"]),
        (
            "conversation_summaries",
            vec!["owner_identity_id", "conversation_id"],
        ),
        (
            "direct_peer_routes",
            vec!["owner_identity_id", "conversation_id"],
        ),
        ("groups", vec!["owner_identity_id", "group_id"]),
        (
            "group_members",
            vec!["owner_identity_id", "group_id", "user_id"],
        ),
        ("relationship_events", vec!["owner_identity_id", "event_id"]),
        ("e2ee_outbox", vec!["owner_identity_id", "outbox_id"]),
        (
            "attachment_manifest_cache",
            vec![
                "owner_identity_id",
                "thread_kind",
                "thread_id",
                "message_id",
            ],
        ),
        (
            "sync_state",
            vec!["owner_identity_id", "scope", "checkpoint_kind"],
        ),
        (
            "thread_read_state",
            vec!["owner_identity_id", "thread_scope", "thread_id"],
        ),
        (
            "message_identity_aliases",
            vec!["owner_identity_id", "alias_msg_id"],
        ),
    ] {
        assert_primary_key_columns(&db, table, &key_columns);
    }
}

#[test]
fn local_state_schema_sync_state_is_created_during_v17_upgrade() {
    let db = Connection::open_in_memory().unwrap();
    create_identity_owned_schema(&db, IdentityOwnedSchemaTableMode::Final).unwrap();
    db.pragma_update(None, "user_version", IDENTITY_OWNED_SCHEMA_VERSION)
        .unwrap();

    ensure_schema(&db).unwrap();

    assert_eq!(current_schema_version(&db).unwrap(), SCHEMA_VERSION);
    assert_schema_object_exists(&db, "table", "sync_state");
    assert_index_exists(&db, "idx_sync_state_owner_kind");
    assert_schema_object_exists(&db, "table", "thread_read_state");
    assert_index_exists(&db, "idx_thread_read_state_owner_pending");
    assert_primary_key_columns(
        &db,
        "sync_state",
        &["owner_identity_id", "scope", "checkpoint_kind"],
    );
    assert_primary_key_columns(
        &db,
        "thread_read_state",
        &["owner_identity_id", "thread_scope", "thread_id"],
    );
}

#[test]
fn local_state_schema_backfills_existing_group_members_as_did_only() {
    let db = Connection::open_in_memory().unwrap();
    create_identity_owned_schema(&db, IdentityOwnedSchemaTableMode::Final).unwrap();
    db.execute(
        r#"
INSERT INTO group_members
    (owner_identity_id, owner_did, group_id, user_id, member_did, member_handle,
     last_synced_at, credential_name)
VALUES ('owner-id', 'did:owner', 'did:group', 'legacy-id', 'did:member',
        'member.example.com', '2026-07-12T00:00:00Z', 'owner')"#,
        [],
    )
    .unwrap();
    db.pragma_update(None, "user_version", 22).unwrap();

    ensure_schema(&db).unwrap();

    let identity = db
        .query_row(
            "SELECT user_id, anchor_kind, anchor_value, handle_binding_generation FROM group_members",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(
        identity,
        (
            "legacy-id".to_owned(),
            "did".to_owned(),
            "did:member".to_owned(),
            None
        )
    );
}

#[test]
fn local_state_schema_upgrades_remote_v23_without_losing_direct_routes() {
    let db = Connection::open_in_memory().unwrap();
    create_identity_owned_schema(&db, IdentityOwnedSchemaTableMode::Final).unwrap();
    db.execute_batch(DIRECT_PEER_ROUTES_SQL).unwrap();
    db.execute(
        r#"
INSERT INTO direct_peer_routes
    (owner_identity_id, conversation_id, peer_user_id, full_handle, current_did, updated_at)
VALUES ('owner-id', 'dm:alice.example.com', 'peer-id', 'alice.example.com',
        'did:example:alice', '2026-07-12T00:00:00Z')"#,
        [],
    )
    .unwrap();
    db.pragma_update(None, "user_version", 23).unwrap();

    ensure_schema(&db).unwrap();

    assert_eq!(current_schema_version(&db).unwrap(), SCHEMA_VERSION);
    assert_schema_object_exists(&db, "table", "group_rebind_outbox");
    assert_schema_object_exists(&db, "table", "group_rebind_p6_jobs");
    let route = db
        .query_row(
            "SELECT peer_user_id, full_handle, current_did FROM direct_peer_routes",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(
        route,
        (
            "peer-id".to_owned(),
            "alice.example.com".to_owned(),
            "did:example:alice".to_owned(),
        )
    );
}

#[test]
fn local_state_schema_upgrades_local_v24_without_losing_rebind_jobs() {
    let db = Connection::open_in_memory().unwrap();
    create_identity_owned_schema(&db, IdentityOwnedSchemaTableMode::Final).unwrap();
    db.execute_batch(crate::internal::group_rebind_recovery::GROUP_REBIND_RECOVERY_SQL)
        .unwrap();
    db.execute(
        r#"
INSERT INTO group_rebind_outbox
    (job_id, owner_identity_id, group_did, member_handle, previous_member_did,
     new_member_did, binding_generation, phase, created_at, updated_at)
VALUES ('job-1', 'owner-id', 'did:example:group', 'alice.example.com',
        'did:example:alice-old', 'did:example:alice-new', '2', 'awaiting_p6',
        '2026-07-12T00:00:00Z', '2026-07-12T00:00:00Z')"#,
        [],
    )
    .unwrap();
    db.pragma_update(None, "user_version", 24).unwrap();

    ensure_schema(&db).unwrap();

    assert_eq!(current_schema_version(&db).unwrap(), SCHEMA_VERSION);
    assert_schema_object_exists(&db, "table", "direct_peer_routes");
    let job = db
        .query_row(
            "SELECT member_handle, binding_generation, phase FROM group_rebind_outbox WHERE job_id='job-1'",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(
        job,
        (
            "alice.example.com".to_owned(),
            "2".to_owned(),
            "awaiting_p6".to_owned(),
        )
    );
}

#[test]
fn local_state_schema_upgrades_v17_with_conversation_summary_backfill() {
    let db = Connection::open_in_memory().unwrap();
    create_identity_owned_schema(&db, IdentityOwnedSchemaTableMode::Final).unwrap();
    db.execute_batch(
        r#"
INSERT INTO messages
    (msg_id, owner_identity_id, owner_did, conversation_id, thread_id, direction,
     sender_did, receiver_did, content_type, content, sent_at, stored_at, is_read)
VALUES
    ('old', 'alice-id', 'did:alice', 'dm:did:bob', 'dm:did:bob', 0,
     'did:bob', 'did:alice', 'text/plain', 'old', '2026-05-01T00:00:00Z', '2026-05-01T00:00:00Z', 1),
    ('new', 'alice-id', 'did:alice', 'dm:did:bob', 'dm:did:bob', 0,
     'did:bob', 'did:alice', 'text/plain', 'new', '2026-05-02T00:00:00Z', '2026-05-02T00:00:00Z', 0)"#,
    )
    .unwrap();
    db.pragma_update(None, "user_version", IDENTITY_OWNED_SCHEMA_VERSION)
        .unwrap();

    ensure_schema(&db).unwrap();

    assert_eq!(current_schema_version(&db).unwrap(), SCHEMA_VERSION);
    assert_schema_object_exists(&db, "table", "conversation_summaries");
    let summary = db
        .query_row(
            r#"
SELECT message_count, unread_count, last_message_id, last_content
FROM conversation_summaries
WHERE owner_identity_id = 'alice-id' AND conversation_id = 'dm:did:bob'"#,
            [],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(summary, (2, 1, "new".to_owned(), "new".to_owned()));
}

#[test]
fn local_state_schema_v27_rebuilds_control_only_summaries_without_losing_registry() {
    let db = Connection::open_in_memory().unwrap();
    ensure_schema(&db).unwrap();
    db.execute_batch(
        r#"
INSERT INTO messages
    (msg_id, owner_identity_id, owner_did, conversation_id, thread_id, direction,
     sender_did, group_id, group_did, content_type, content, sent_at, stored_at, is_read)
VALUES
    ('did:example:group:1', 'alice-id', 'did:alice',
     'group:did:example:group', 'group:did:example:group', 1,
     'did:alice', 'did:example:group', 'did:example:group',
     'application/json', '', '2026-07-13T00:00:00Z', '2026-07-13T00:00:00Z', 1);
INSERT INTO conversation_summaries
    (owner_identity_id, owner_did, conversation_id, thread_id,
     message_count, unread_count, unread_mention_count,
     last_message_id, last_message_at, last_content, last_content_type,
     group_id, group_did, updated_at)
VALUES
    ('alice-id', 'did:alice', 'group:did:example:group', 'group:did:example:group',
     1, 0, 0, 'did:example:group:1', '2026-07-13T00:00:00Z', '',
     'application/json', 'did:example:group', 'did:example:group',
     '2026-07-13T00:00:00Z');
INSERT INTO conversation_registry
    (owner_identity_id, owner_did, conversation_id, thread_kind, thread_id,
     activity_at, created_at, updated_at, is_active)
VALUES
    ('alice-id', 'did:alice', 'group:did:example:group', 'group',
     'did:example:group', '2026-07-13T00:00:00Z', '2026-07-13T00:00:00Z',
     '2026-07-13T00:00:00Z', 1);
"#,
    )
    .unwrap();
    db.pragma_update(None, "user_version", 26).unwrap();

    ensure_schema(&db).unwrap();

    let summary_count: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM conversation_summaries WHERE owner_identity_id = 'alice-id' AND conversation_id = 'group:did:example:group'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let registry_count: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM conversation_registry WHERE owner_identity_id = 'alice-id' AND conversation_id = 'group:did:example:group' AND is_active = 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(summary_count, 0);
    assert_eq!(registry_count, 1);
    assert_eq!(current_schema_version(&db).unwrap(), SCHEMA_VERSION);
}

#[test]
fn local_state_schema_repairs_group_message_aliases_during_upgrade() {
    let db = Connection::open_in_memory().unwrap();
    create_identity_owned_schema(&db, IdentityOwnedSchemaTableMode::Final).unwrap();
    db.execute(
        r#"
INSERT INTO messages
    (msg_id, owner_identity_id, owner_did, conversation_id, thread_id, direction,
     sender_did, receiver_did, group_id, group_did, content_type, content, server_seq,
     sent_at, stored_at, metadata, is_read)
VALUES
    ('did:example:group:5', 'alice-id', 'did:example:alice',
     'group:did:example:group', 'group:did:example:group', 0,
     'did:example:bob', 'did:example:alice',
     'did:example:group', 'did:example:group',
     'text/plain', 'hello group', 5,
     '2026-05-02T00:00:00Z', '2026-05-02T00:00:00Z',
     '{"raw_message_id":"msg-local-5"}', 0)"#,
        [],
    )
    .unwrap();
    db.pragma_update(None, "user_version", IDENTITY_OWNED_SCHEMA_VERSION)
        .unwrap();

    ensure_schema(&db).unwrap();

    let classification =
        crate::internal::local_state::messages::classify_mark_read_ids_for_owner_identity(
            &db,
            "alice-id",
            "did:example:alice",
            &["msg-local-5".to_owned()],
        )
        .unwrap();
    assert_eq!(classification.group_ids, vec!["msg-local-5"]);
    assert!(classification.direct_ids.is_empty());
}

#[test]
fn local_state_schema_v17_creates_identity_owned_primary_keys_without_bumping_active_version() {
    let db = Connection::open_in_memory().unwrap();

    create_identity_owned_schema(&db, IdentityOwnedSchemaTableMode::Final).unwrap();

    assert_eq!(current_schema_version(&db).unwrap(), 0);
    for (table, key_columns) in [
        ("contacts", vec!["owner_identity_id", "did"]),
        (
            "contact_handle_bindings",
            vec!["owner_identity_id", "handle", "did"],
        ),
        ("messages", vec!["owner_identity_id", "msg_id"]),
        ("groups", vec!["owner_identity_id", "group_id"]),
        (
            "group_members",
            vec!["owner_identity_id", "group_id", "user_id"],
        ),
        ("relationship_events", vec!["owner_identity_id", "event_id"]),
        ("e2ee_outbox", vec!["owner_identity_id", "outbox_id"]),
        ("identity_did_history", vec!["owner_identity_id", "did"]),
        (
            "attachment_manifest_cache",
            vec![
                "owner_identity_id",
                "thread_kind",
                "thread_id",
                "message_id",
            ],
        ),
    ] {
        assert_primary_key_columns(&db, table, &key_columns);
    }
    assert_column_exists(&db, "messages", "conversation_id");
    assert_index_exists(&db, "idx_identity_did_history_current");
    assert_index_exists(&db, "idx_identity_did_history_live_did_unique");
}

#[test]
fn local_state_schema_v17_can_create_rebuild_staging_tables() {
    let db = Connection::open_in_memory().unwrap();

    create_identity_owned_schema(&db, IdentityOwnedSchemaTableMode::RebuildNew).unwrap();

    assert_schema_object_exists(&db, "table", "messages_new");
    assert_schema_object_exists(&db, "table", "identity_did_history_new");
    assert_schema_object_exists(&db, "table", "attachment_manifest_cache_new");
    assert_primary_key_columns(&db, "messages_new", &["owner_identity_id", "msg_id"]);
    assert_primary_key_columns(
        &db,
        "attachment_manifest_cache_new",
        &[
            "owner_identity_id",
            "thread_kind",
            "thread_id",
            "message_id",
        ],
    );
    assert_index_exists(&db, "idx_messages_owner_identity_conversation_new");
    assert_index_exists(&db, "idx_identity_did_history_current_new");
    assert_index_exists(&db, "idx_attachment_manifest_cache_owner_thread_new");
}

#[test]
fn owner_invariant_helpers_report_only_counts_and_labels() {
    let db = Connection::open_in_memory().unwrap();
    create_identity_owned_schema(&db, IdentityOwnedSchemaTableMode::Final).unwrap();
    db.execute(
            r#"
INSERT INTO messages
    (msg_id, owner_identity_id, owner_did, conversation_id, thread_id, direction, content, stored_at)
VALUES ('msg-1', 'alice-id', 'did:example:alice', 'dm:did:example:alice:did:example:bob', 'thread-1', 0, 'private body', '2026-05-30T00:00:00Z')"#,
            [],
        )
        .unwrap();

    let violations =
        identity_owned_owner_invariants(&db, IdentityOwnedSchemaTableMode::Final).unwrap();

    assert!(violations.iter().any(|violation| {
        violation.table == "messages"
            && violation.invariant == "conversation_id_must_not_include_owner_did"
            && violation.row_count == 1
    }));
    let debug = format!("{violations:?}");
    assert!(!debug.contains("private body"));
    assert!(!debug.contains("dm:did:example:alice"));
}

#[test]
fn owner_invariant_helpers_rely_on_schema_constraints_for_duplicate_keys() {
    let db = Connection::open_in_memory().unwrap();
    create_identity_owned_schema(&db, IdentityOwnedSchemaTableMode::Final).unwrap();
    db.execute(
            r#"
INSERT INTO identity_did_history
    (owner_identity_id, did, status, first_seen_at, last_seen_at)
VALUES ('alice-id', 'did:example:alice', 'current', '2026-05-30T00:00:00Z', '2026-05-30T00:00:00Z')"#,
            [],
        )
        .unwrap();

    assert!(db
            .execute(
                r#"
INSERT INTO identity_did_history
    (owner_identity_id, did, status, first_seen_at, last_seen_at)
VALUES ('alice-id', 'did:example:alice-2', 'current', '2026-05-30T00:00:00Z', '2026-05-30T00:00:00Z')"#,
                [],
            )
            .is_err());

    let violations =
        identity_owned_owner_invariants(&db, IdentityOwnedSchemaTableMode::Final).unwrap();
    assert!(violations.is_empty());
}

#[test]
fn rebuild_owner_resolution_uses_identity_id_then_did_history_without_credential_fallback() {
    let hints = vec![LocalStateOwnerHint {
        owner_identity_id: "alice-id".to_owned(),
        current_did: "did:example:alice-current".to_owned(),
        historical_dids: vec!["did:example:alice-old".to_owned()],
    }];

    let resolved_by_identity = resolve_rebuild_row_owner(
        RebuildRowOwnershipInput {
            table: "messages",
            row_key: "msg-1".to_owned(),
            owner_identity_id: "alice-id".to_owned(),
            owner_did: String::new(),
            credential_name: "alice".to_owned(),
        },
        &hints,
    )
    .unwrap();
    assert!(matches!(
        resolved_by_identity,
        RebuildRowOwnerResolution::Resolved(scope)
            if scope.owner_identity_id == "alice-id"
                && scope.owner_did == "did:example:alice-current"
                && scope.credential_name.as_deref() == Some("alice")
    ));

    let resolved_by_history = resolve_rebuild_row_owner(
        RebuildRowOwnershipInput {
            table: "messages",
            row_key: "msg-2".to_owned(),
            owner_identity_id: String::new(),
            owner_did: "did:example:alice-old".to_owned(),
            credential_name: "wrong-credential".to_owned(),
        },
        &hints,
    )
    .unwrap();
    assert!(matches!(
        resolved_by_history,
        RebuildRowOwnerResolution::Resolved(scope)
            if scope.owner_identity_id == "alice-id"
                && scope.owner_did == "did:example:alice-current"
                && scope.credential_name.as_deref() == Some("wrong-credential")
    ));

    let unresolved = resolve_rebuild_row_owner(
        RebuildRowOwnershipInput {
            table: "messages",
            row_key: "msg-secret".to_owned(),
            owner_identity_id: String::new(),
            owner_did: String::new(),
            credential_name: "alice".to_owned(),
        },
        &hints,
    )
    .unwrap();
    assert_eq!(
        unresolved,
        RebuildRowOwnerResolution::Unresolved(RedactedRebuildRow {
            table: "messages",
            row_key: "msg-secret".to_owned(),
            reason: "missing_owner_identity_id"
        })
    );
}

#[test]
fn identity_owned_merge_specs_cover_active_business_tables() {
    let specs = identity_owned_merge_specs();

    for table in [
        "messages",
        "contacts",
        "contact_handle_bindings",
        "groups",
        "group_members",
        "relationship_events",
        "e2ee_outbox",
        "attachment_manifest_cache",
    ] {
        assert!(
            specs.iter().any(|spec| spec.table == table),
            "missing merge spec for {table}"
        );
    }
    assert!(specs
        .iter()
        .all(|spec| spec.key_columns.first() == Some(&"owner_identity_id")));
}

#[test]
fn local_state_schema_rejects_pre_v17_without_workspace_migration() {
    let db = Connection::open_in_memory().unwrap();
    db.pragma_update(None, "user_version", 15).unwrap();
    db.execute_batch(
        r#"
CREATE TABLE direct_e2ee_sessions (
    owner_identity_id TEXT NOT NULL,
    owner_did         TEXT NOT NULL DEFAULT '',
    peer_did          TEXT NOT NULL,
    session_id        TEXT NOT NULL,
    state_blob        BLOB NOT NULL,
    metadata_json     TEXT,
    created_at        TEXT NOT NULL,
    updated_at        TEXT NOT NULL,
    PRIMARY KEY (owner_identity_id, peer_did),
    UNIQUE (owner_identity_id, session_id)
)"#,
    )
    .unwrap();

    assert!(matches!(
        ensure_schema(&db),
        Err(crate::ImError::LocalStateUnavailable { detail })
            if detail.contains("requires owner-identity migration")
    ));
}

#[test]
fn local_state_schema_rejects_unsupported_versions() {
    let old = Connection::open_in_memory().unwrap();
    old.pragma_update(None, "user_version", 5).unwrap();
    assert!(matches!(
        ensure_schema(&old),
        Err(crate::ImError::LocalStateUnavailable { detail })
            if detail.contains("requires owner-identity migration")
    ));

    let future = Connection::open_in_memory().unwrap();
    future
        .pragma_update(None, "user_version", SCHEMA_VERSION + 1)
        .unwrap();
    assert!(matches!(
        ensure_schema(&future),
        Err(crate::ImError::LocalStateUnavailable { detail })
            if detail.contains("newer than supported")
    ));
}

#[test]
fn local_state_schema_rejects_v6_handle_backfill_until_workspace_migration() {
    let db = Connection::open_in_memory().unwrap();
    db.pragma_update(None, "user_version", 6).unwrap();
    db.execute_batch(V6_TABLES_SQL).unwrap();
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
            r#"{"source":"legacy"}"#,
        ),
    )
    .unwrap();

    assert!(matches!(
        ensure_schema(&db),
        Err(crate::ImError::LocalStateUnavailable { detail })
            if detail.contains("requires owner-identity migration")
    ));
}

#[test]
fn local_state_owner_backfill_is_legacy_only_after_v17_cutover() {
    let db = Connection::open_in_memory().unwrap();
    ensure_schema(&db).unwrap();

    let updated = backfill_owner_identity_ids(
        &db,
        &[OwnerIdentityBackfill {
            identity_id: "alice-id".to_string(),
            owner_did: "did:alice".to_string(),
            credential_names: vec!["alice".to_string()],
        }],
    )
    .unwrap();

    assert_eq!(updated, 0);
}

fn assert_schema_object_exists(db: &Connection, object_type: &str, name: &str) {
    let count = db
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = ?1 AND name = ?2",
            (object_type, name),
            |row| row.get::<_, i64>(0),
        )
        .unwrap();
    assert_eq!(count, 1, "missing {object_type} {name}");
}

fn assert_index_exists(db: &Connection, name: &str) {
    assert_schema_object_exists(db, "index", name);
}

fn assert_column_exists(db: &Connection, table: &str, column: &str) {
    assert!(column_exists(db, table, column), "missing {table}.{column}");
}

fn assert_primary_key_columns(db: &Connection, table: &str, expected: &[&str]) {
    let mut statement = db.prepare(&format!("PRAGMA table_info({table})")).unwrap();
    let rows = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(1)?, row.get::<_, i64>(5)?))
        })
        .unwrap();
    let mut keyed = rows
        .map(|row| row.unwrap())
        .filter(|(_, pk)| *pk > 0)
        .collect::<Vec<_>>();
    keyed.sort_by_key(|(_, pk)| *pk);
    let columns = keyed.into_iter().map(|(name, _)| name).collect::<Vec<_>>();
    assert_eq!(columns, expected, "unexpected primary key for {table}");
}

fn column_exists(db: &Connection, table: &str, column: &str) -> bool {
    let mut statement = db.prepare(&format!("PRAGMA table_info({table})")).unwrap();
    let mut rows = statement
        .query_map([], |row| row.get::<_, String>(1))
        .unwrap();
    rows.any(|name| name.unwrap() == column)
}
