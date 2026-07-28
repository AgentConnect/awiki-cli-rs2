use super::*;

fn upgrade_legacy_schema_for_test(connection: &Connection) -> crate::ImResult<()> {
    let version = current_schema_version(connection)?;
    create_schema(connection, version < CONVERSATION_SUMMARIES_SCHEMA_VERSION)?;
    set_schema_version(connection, SCHEMA_VERSION)
}

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
        ("table", "sync_installation_state"),
        ("table", "identity_account_bindings"),
        ("table", "message_sync_state"),
        ("table", "sync_applied_events"),
        ("table", "sync_recovery_state"),
        ("table", "local_mutation_outbox"),
        ("table", "sync_thread_bindings"),
        ("table", "sync_remote_read_states"),
        ("table", "thread_read_state"),
        ("table", "message_identity_aliases"),
        ("table", "direct_peer_routes"),
        ("table", "peer_personas"),
        ("table", "peer_identifiers"),
        ("table", "peer_profiles"),
        ("table", "conversation_aliases"),
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
    assert_index_exists(&db, "identity_account_device_idx");
    assert_index_exists(&db, "message_sync_state_account_device_idx");
    assert_index_exists(&db, "sync_applied_events_prune_idx");
    assert_index_exists(&db, "sync_applied_events_applied_at_idx");
    assert_index_exists(&db, "sync_recovery_state_status_idx");
    assert_index_exists(&db, "local_mutation_outbox_drain_idx");
    assert_index_exists(&db, "sync_thread_bindings_conversation_idx");
    assert_index_exists(&db, "idx_thread_read_state_owner_pending");
    assert_index_exists(&db, "idx_thread_read_state_owner_conversation");
    assert_index_exists(&db, "idx_message_identity_aliases_owner_canonical");
    assert_index_exists(&db, "idx_direct_peer_routes_owner_did");
    assert_index_exists(&db, "idx_peer_identifiers_owner_persona");
    assert_index_exists(&db, "idx_conversation_aliases_owner_target");
    assert_index_exists(&db, "idx_conversation_registry_active_direct_persona");
    assert_index_exists(&db, "idx_conversation_registry_active_group_did");
    assert_index_exists(&db, "idx_group_members_owner_membership");
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
    for column in [
        "peer_persona_id",
        "canonical_group_did",
        "lifecycle_state",
        "resolution_state",
        "merged_into_conversation_id",
    ] {
        assert_column_exists(&db, "conversation_registry", column);
    }
    for column in [
        "membership_id",
        "peer_persona_id",
        "member_credential_did",
        "membership_epoch",
    ] {
        assert_column_exists(&db, "group_members", column);
    }
    for column in [
        "wire_thread_kind",
        "wire_thread_ref",
        "wire_identity_resolution_state",
    ] {
        assert_column_exists(&db, "messages", column);
    }
    assert_column_exists(&db, "contacts", "peer_persona_id");
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
        ("sync_installation_state", vec!["owner_identity_id"]),
        ("identity_account_bindings", vec!["owner_identity_id"]),
        ("message_sync_state", vec!["owner_identity_id"]),
        ("sync_applied_events", vec!["owner_identity_id", "event_id"]),
        ("sync_recovery_state", vec!["owner_identity_id"]),
        (
            "local_mutation_outbox",
            vec!["owner_identity_id", "mutation_id"],
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

    upgrade_legacy_schema_for_test(&db).unwrap();

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
    (owner_identity_id, owner_did, group_id, user_id, membership_id, member_did, member_handle,
     last_synced_at, credential_name)
VALUES ('owner-id', 'did:owner', 'did:group', 'legacy-id', '', 'did:member',
        'member.example.com', '2026-07-12T00:00:00Z', 'owner')"#,
        [],
    )
    .unwrap();
    db.pragma_update(None, "user_version", 22).unwrap();

    upgrade_legacy_schema_for_test(&db).unwrap();

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

    upgrade_legacy_schema_for_test(&db).unwrap();

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

    upgrade_legacy_schema_for_test(&db).unwrap();

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

    upgrade_legacy_schema_for_test(&db).unwrap();

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
    upgrade_legacy_schema_for_test(&db).unwrap();
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

    upgrade_legacy_schema_for_test(&db).unwrap();

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

    upgrade_legacy_schema_for_test(&db).unwrap();

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
    assert_column_exists(&db, "messages", "wire_thread_kind");
    assert_column_exists(&db, "messages", "wire_thread_ref");
    assert_column_exists(&db, "messages", "wire_identity_resolution_state");
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
        Err(crate::ImError::LocalStateUpgradeRequired {
            from_version: 15,
            target_version: SCHEMA_VERSION,
        })
    ));
}

#[test]
fn local_state_schema_rejects_unsupported_versions() {
    let old = Connection::open_in_memory().unwrap();
    old.pragma_update(None, "user_version", 5).unwrap();
    assert!(matches!(
        ensure_schema(&old),
        Err(crate::ImError::LocalStateUpgradeRequired {
            from_version: 5,
            target_version: SCHEMA_VERSION,
        })
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
fn local_state_schema_atomically_upgrades_v28_with_system_notification_tables() {
    let db = Connection::open_in_memory().unwrap();
    install_v28_fixture(&db);

    ensure_schema(&db).unwrap();

    assert_eq!(current_schema_version(&db).unwrap(), SCHEMA_VERSION);
    for table in [
        "system_notification_receipts",
        "system_notification_join_state",
        "identity_root_import_completion_v1",
        "identity_root_transfer_sender_v1",
    ] {
        let count: i64 = db
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
                [table],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }
    assert_common_v28_data_preserved(&db);
    assert_v33_sync_foundation_exists(&db);
}

#[test]
fn local_state_schema_atomically_upgrades_v29_with_root_import_tables() {
    let db = Connection::open_in_memory().unwrap();
    install_v29_fixture(&db);

    ensure_schema(&db).unwrap();

    assert_eq!(current_schema_version(&db).unwrap(), SCHEMA_VERSION);
    for table in [
        "identity_root_import_completion_v1",
        "identity_root_transfer_sender_v1",
    ] {
        let count: i64 = db
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
                [table],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }
    assert_common_v28_data_preserved(&db);
    assert_v29_notification_data_preserved(&db);
    assert_v33_sync_foundation_exists(&db);
}

#[test]
fn local_state_schema_atomically_upgrades_v30_with_p5_retirement_boundary() {
    let db = Connection::open_in_memory().unwrap();
    install_v30_fixture(&db);

    ensure_schema(&db).unwrap();

    assert_eq!(current_schema_version(&db).unwrap(), SCHEMA_VERSION);
    for table in [
        "direct_e2ee_v2_sessions",
        "direct_e2ee_v2_pending",
        "direct_e2ee_v2_prekey_bundles",
        "direct_e2ee_v2_one_time_prekeys",
    ] {
        let count: i64 = db
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
                [table],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }
    assert_common_v28_data_preserved(&db);
    assert_v29_notification_data_preserved(&db);
    assert_v30_root_import_data_preserved(&db);
    assert_direct_v2_ordinary_data_preserved(&db);
    assert_retired_direct_v2_private_data_removed(&db);
    assert_v33_sync_foundation_exists(&db);
}

#[test]
fn local_state_schema_upgrades_true_v31_fixture_and_is_idempotent() {
    let db = Connection::open_in_memory().unwrap();
    install_v31_fixture(&db);

    ensure_schema(&db).unwrap();
    assert_eq!(current_schema_version(&db).unwrap(), SCHEMA_VERSION);
    assert_common_v28_data_preserved(&db);
    assert_v29_notification_data_preserved(&db);
    assert_v30_root_import_data_preserved(&db);
    assert_direct_v2_ordinary_data_preserved(&db);
    assert_retired_direct_v2_private_data_removed(&db);
    assert_v33_sync_foundation_exists(&db);

    ensure_schema(&db).unwrap();
    assert_eq!(current_schema_version(&db).unwrap(), SCHEMA_VERSION);
    assert_common_v28_data_preserved(&db);
    assert_v29_notification_data_preserved(&db);
    assert_v30_root_import_data_preserved(&db);
    assert_direct_v2_ordinary_data_preserved(&db);

    for table in [
        "identity_root_import_completion_v1",
        "identity_root_transfer_sender_v1",
    ] {
        let count: i64 = db
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
                [table],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }
}

#[test]
fn local_state_schema_upgrades_true_v32_fixture_through_v34_and_is_idempotent() {
    let db = Connection::open_in_memory().unwrap();
    install_v32_fixture(&db);

    assert_eq!(current_schema_version(&db).unwrap(), 32);
    assert_eq!(
        db.query_row(
            "SELECT COUNT(*) FROM sqlite_master
             WHERE type = 'table' AND name = 'sync_installation_state'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap(),
        0,
        "a true v32 fixture must not contain the v33 installation table"
    );

    ensure_schema(&db).unwrap();
    assert_eq!(current_schema_version(&db).unwrap(), SCHEMA_VERSION);
    assert_v33_sync_foundation_exists(&db);
    assert_eq!(
        db.query_row(
            "SELECT scan_seq FROM message_sync_state
             WHERE owner_identity_id = 'owner-v32'",
            [],
            |row| row.get::<_, String>(0),
        )
        .unwrap(),
        "42"
    );

    ensure_schema(&db).unwrap();
    assert_eq!(current_schema_version(&db).unwrap(), SCHEMA_VERSION);
    assert_v33_sync_foundation_exists(&db);
}

#[test]
fn local_state_schema_v27_remains_preopen_upgrade_gate() {
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch(SYNC_STATE_SQL).unwrap();
    db.pragma_update(None, "user_version", 27).unwrap();

    assert!(matches!(
        ensure_schema(&db),
        Err(crate::ImError::LocalStateUpgradeRequired {
            from_version: 27,
            target_version: SCHEMA_VERSION,
        })
    ));
    assert!(!column_exists(
        &db,
        "identity_account_bindings",
        "owner_identity_id"
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
        Err(crate::ImError::LocalStateUpgradeRequired {
            from_version: 6,
            target_version: SCHEMA_VERSION,
        })
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

fn install_v28_fixture(db: &Connection) {
    install_complete_v28_schema(db);
    db.execute_batch(
        r#"
INSERT INTO messages (
    msg_id, owner_identity_id, owner_did, conversation_id,
    wire_thread_kind, wire_thread_ref, wire_identity_resolution_state, thread_id,
    direction, sender_did, receiver_did, group_id, group_did, content_type,
    content, server_seq, sent_at, stored_at, is_e2ee, is_read, sender_name,
    mentions_current_user, credential_name
) VALUES (
    'did:example:legacy-group:411', 'owner-legacy', 'did:example:legacy',
    'group:did:example:legacy-group', 'group', 'did:example:legacy-group',
    'resolved', 'group:did:example:legacy-group', 0,
    'did:example:peer', 'did:example:legacy', 'legacy-group',
    'did:example:legacy-group', 'text/plain', 'legacy-v28-message', 411,
    '2026-07-20T00:00:00Z', '2026-07-20T00:00:01Z', 0, 0, 'Legacy peer',
    1, 'alice'
);

INSERT INTO conversation_summaries (
    owner_identity_id, owner_did, conversation_id, thread_id,
    message_count, unread_count, unread_mention_count,
    first_unread_mention_message_id, last_message_id, last_message_at,
    last_content, last_content_type, last_sender_did, last_sender_name,
    last_payload_json, group_id, group_did, updated_at
) VALUES (
    'owner-legacy', 'did:example:legacy',
    'group:did:example:legacy-group', 'group:did:example:legacy-group',
    1, 1, 1, 'did:example:legacy-group:411', 'did:example:legacy-group:411',
    '2026-07-20T00:00:00Z',
    'legacy-v28-message', 'text/plain', 'did:example:peer', 'Legacy peer',
    '{"fixture":"v28"}', 'legacy-group', 'did:example:legacy-group',
    '2026-07-20T00:00:01Z'
);

INSERT INTO conversation_registry (
    owner_identity_id, owner_did, conversation_id, thread_kind, thread_id,
    activity_at, created_at, updated_at, is_active, canonical_group_did,
    lifecycle_state, resolution_state
) VALUES (
    'owner-legacy', 'did:example:legacy',
    'group:did:example:legacy-group', 'group', 'group:did:example:legacy-group',
    '2026-07-20T00:00:00Z', '2026-07-20T00:00:00Z',
    '2026-07-20T00:00:01Z', 1, 'did:example:legacy-group',
    'active', 'resolved'
);
"#,
    )
    .unwrap();
    db.execute(
        r#"
INSERT INTO sync_state
    (owner_identity_id, owner_did, scope, checkpoint_kind, event_seq, updated_at)
VALUES ('owner-legacy', 'did:example:legacy', 'global', 'sync_delta', '912', 'old')"#,
        [],
    )
    .unwrap();
    db.pragma_update(None, "user_version", 28).unwrap();
}

fn install_v29_fixture(db: &Connection) {
    install_v28_fixture(db);
    db.execute_batch(crate::internal::system_notification::store::SYSTEM_NOTIFICATION_SCHEMA_SQL)
        .unwrap();
    db.execute(
        r#"
INSERT INTO system_notification_receipts
    (owner_identity_id, owner_did, protocol_device_id, event_id, join_session_id,
     session_revision, payload_hash, proof_hash, first_seen_at, expires_at)
VALUES ('owner-legacy', 'did:example:legacy', 'device-1', 'event-old', 'join-old',
        1, 'payload', 'proof', 'old', 'later')"#,
        [],
    )
    .unwrap();
    db.pragma_update(None, "user_version", 29).unwrap();
}

fn install_v30_fixture(db: &Connection) {
    install_v29_fixture(db);
    db.execute_batch(ROOT_IMPORT_COORDINATOR_SQL).unwrap();
    db.execute(
        r#"
INSERT INTO identity_root_transfer_sender_v1
    (owner_identity_id, owner_did, local_device_id, message_id, recipient_device_id,
     phase, created_at, updated_at)
VALUES ('owner-legacy', 'did:example:legacy', 'device-1', 'message-old', 'device-2',
        'pending_delivery', 'old', 'old')"#,
        [],
    )
    .unwrap();
    install_direct_v2_fixture(db, true);
    db.pragma_update(None, "user_version", 30).unwrap();
}

fn install_v31_fixture(db: &Connection) {
    install_v29_fixture(db);
    db.execute_batch(ROOT_IMPORT_COORDINATOR_SQL).unwrap();
    db.execute(
        r#"
INSERT INTO identity_root_transfer_sender_v1
    (owner_identity_id, owner_did, local_device_id, message_id, recipient_device_id,
     phase, created_at, updated_at)
VALUES ('owner-legacy', 'did:example:legacy', 'device-1', 'message-old', 'device-2',
        'pending_delivery', 'old', 'old')"#,
        [],
    )
    .unwrap();
    install_direct_v2_fixture(db, false);
    db.pragma_update(None, "user_version", 31).unwrap();
}

fn install_v32_fixture(db: &Connection) {
    install_v31_fixture(db);
    db.execute_batch(crate::internal::local_state::sync_v2::SYNC_V2_SCHEMA_SQL)
        .unwrap();
    db.execute(
        r#"
INSERT INTO identity_account_bindings
    (owner_identity_id, account_id, handle_scope, current_did, device_id,
     identity_generation, device_auth_generation, created_at, updated_at)
VALUES
    ('owner-v32', 'account-v32', NULL, 'did:example:v32', 'device-v32',
     '1', '1', 1, 1)"#,
        [],
    )
    .unwrap();
    db.execute(
        r#"
INSERT INTO message_sync_state
    (owner_identity_id, account_id, device_id, device_auth_generation,
     stream_epoch, scan_seq, bootstrap_state, updated_at)
VALUES
    ('owner-v32', 'account-v32', 'device-v32', '1',
     '1', '42', 'active', 1)"#,
        [],
    )
    .unwrap();
    db.pragma_update(None, "user_version", 32).unwrap();
}

/// Historical v28 schema builder, copied from the v28 create path at
/// `0e543604`. It deliberately does not call the current `create_schema`,
/// because doing so and merely rewriting `user_version` would manufacture
/// v29-v32 objects that never existed in a real v28 database.
fn install_complete_v28_schema(db: &Connection) {
    create_identity_owned_schema(db, IdentityOwnedSchemaTableMode::Final).unwrap();
    ensure_column(db, "contacts", "peer_persona_id", "TEXT").unwrap();
    ensure_group_member_identity_columns(db).unwrap();
    db.execute_batch(ATTACHMENT_MANIFEST_CACHE_SQL).unwrap();
    ensure_column(
        db,
        "attachment_manifest_cache",
        "wire_message_id",
        "TEXT NOT NULL DEFAULT ''",
    )
    .unwrap();
    db.execute_batch(SYNC_STATE_SQL).unwrap();
    db.execute_batch(THREAD_READ_STATE_SQL).unwrap();
    db.execute_batch(MESSAGE_IDENTITY_ALIASES_SQL).unwrap();
    db.execute_batch(crate::internal::group_rebind_recovery::GROUP_REBIND_RECOVERY_SQL)
        .unwrap();
    db.execute_batch(DIRECT_PEER_ROUTES_SQL).unwrap();
    ensure_column(db, "direct_peer_routes", "peer_persona_id", "TEXT").unwrap();
    ensure_column(db, "direct_peer_routes", "authority_namespace", "TEXT").unwrap();
    assert!(!ensure_message_projection_columns(db).unwrap());
    crate::internal::local_state::conversation_summaries::create_schema(db).unwrap();
    crate::internal::local_state::conversation_registry::create_schema(db).unwrap();
    crate::internal::local_state::peer_personas::create_schema(db).unwrap();
    crate::internal::local_state::peer_identifiers::create_schema(db).unwrap();
    crate::internal::local_state::peer_profiles::create_schema(db).unwrap();
    crate::internal::local_state::conversation_aliases::create_schema(db).unwrap();
    crate::internal::local_state::inbound_resolution_backlog::create_schema(db).unwrap();
    for statement in VIEW_STATEMENTS {
        db.execute(statement, []).unwrap();
    }

    for table in [
        "contacts",
        "contact_handle_bindings",
        "messages",
        "conversation_summaries",
        "conversation_registry",
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
        "peer_personas",
        "peer_identifiers",
        "peer_profiles",
        "conversation_aliases",
        "inbound_resolution_backlog",
        "group_rebind_outbox",
        "group_rebind_p6_jobs",
    ] {
        assert_schema_object_exists(db, "table", table);
    }
    for view in ["threads", "inbox", "outbox"] {
        assert_schema_object_exists(db, "view", view);
    }
    for later_table in [
        "system_notification_receipts",
        "identity_root_import_completion_v1",
        "sync_installation_state",
        "identity_account_bindings",
        "message_sync_state",
    ] {
        let count = db
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                [later_table],
                |row| row.get::<_, i64>(0),
            )
            .unwrap();
        assert_eq!(count, 0, "v28 fixture unexpectedly contains {later_table}");
    }
}

fn install_direct_v2_fixture(db: &Connection, include_retired_private_delivery: bool) {
    db.execute_batch(crate::internal::secure_direct::v2_store::DIRECT_E2EE_V2_SCHEMA)
        .unwrap();
    db.execute_batch(
        r#"
INSERT INTO direct_e2ee_v2_sessions (
    owner_identity_id, owner_did, local_device_id, peer_did, peer_device_id,
    session_id, state_blob, revision, disabled, created_at, updated_at
) VALUES (
    'owner-legacy', 'did:example:legacy', 'device-1',
    'did:example:peer', 'peer-device', 'ordinary-session',
    X'01', 7, 0, 'old', 'old'
);

INSERT INTO direct_e2ee_v2_pending (
    owner_identity_id, owner_did, local_device_id, peer_did, peer_device_id,
    operation_id, message_id, session_id, session_revision, pending_blob,
    created_at, updated_at
) VALUES (
    'owner-legacy', 'did:example:legacy', 'device-1',
    'did:example:peer', 'peer-device', 'ordinary-operation',
    'ordinary-message', 'ordinary-session', 7, X'02', 'old', 'old'
);

INSERT INTO direct_e2ee_v2_prekey_bundles (
    owner_identity_id, owner_did, local_device_id, bundle_id, bundle_json,
    signed_prekey_private_blob, status, created_at, updated_at
) VALUES (
    'owner-legacy', 'did:example:legacy', 'device-1', 'ordinary-bundle',
    '{"fixture":"v30"}', X'03', 'published', 'old', 'old'
);
"#,
    )
    .unwrap();
    if !include_retired_private_delivery {
        return;
    }
    db.execute_batch(
        r#"
CREATE TABLE direct_e2ee_v2_private_outbound (
    owner_identity_id TEXT NOT NULL,
    local_device_id   TEXT NOT NULL,
    operation_id      TEXT NOT NULL,
    PRIMARY KEY (owner_identity_id, local_device_id, operation_id)
);
CREATE TABLE direct_e2ee_v2_private_outbound_tombstones (
    operation_id TEXT PRIMARY KEY
);

INSERT INTO direct_e2ee_v2_sessions (
    owner_identity_id, owner_did, local_device_id, peer_did, peer_device_id,
    session_id, state_blob, revision, disabled, created_at, updated_at
) VALUES (
    'owner-legacy', 'did:example:legacy', 'device-1',
    'did:example:legacy', 'device-2', 'retired-session',
    X'10', 1, 0, 'old', 'old'
);

INSERT INTO direct_e2ee_v2_pending (
    owner_identity_id, owner_did, local_device_id, peer_did, peer_device_id,
    operation_id, message_id, session_id, session_revision, pending_blob,
    created_at, updated_at
) VALUES (
    'owner-legacy', 'did:example:legacy', 'device-1',
    'did:example:legacy', 'device-2', 'retired-operation',
    'retired-message', 'retired-session', 1, X'11', 'old', 'old'
);

INSERT INTO direct_e2ee_v2_private_outbound
    (owner_identity_id, local_device_id, operation_id)
VALUES ('owner-legacy', 'device-1', 'retired-operation');
INSERT INTO direct_e2ee_v2_private_outbound_tombstones (operation_id)
VALUES ('retired-tombstone');
"#,
    )
    .unwrap();
}

fn assert_common_v28_data_preserved(db: &Connection) {
    let event_seq = db
        .query_row(
            "SELECT event_seq FROM sync_state
             WHERE owner_identity_id = 'owner-legacy'
               AND scope = 'global'
               AND checkpoint_kind = 'sync_delta'",
            [],
            |row| row.get::<_, String>(0),
        )
        .unwrap();
    assert_eq!(event_seq, "912");
    let message = db
        .query_row(
            r#"
SELECT content, conversation_id, wire_thread_kind, wire_thread_ref, server_seq
FROM messages
WHERE owner_identity_id = 'owner-legacy'
  AND msg_id = 'did:example:legacy-group:411'"#,
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(
        message,
        (
            "legacy-v28-message".to_owned(),
            "group:did:example:legacy-group".to_owned(),
            "group".to_owned(),
            "did:example:legacy-group".to_owned(),
            411,
        )
    );
    let summary = db
        .query_row(
            r#"
SELECT message_count, unread_count, unread_mention_count, last_message_id, last_content
FROM conversation_summaries
WHERE owner_identity_id = 'owner-legacy'
  AND conversation_id = 'group:did:example:legacy-group'"#,
            [],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(
        summary,
        (
            1,
            1,
            1,
            "did:example:legacy-group:411".to_owned(),
            "legacy-v28-message".to_owned(),
        )
    );
}

fn assert_v29_notification_data_preserved(db: &Connection) {
    let receipt = db
        .query_row(
            r#"
SELECT protocol_device_id, join_session_id, session_revision, payload_hash, proof_hash
FROM system_notification_receipts
WHERE owner_identity_id = 'owner-legacy' AND event_id = 'event-old'"#,
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(
        receipt,
        (
            "device-1".to_owned(),
            "join-old".to_owned(),
            1,
            "payload".to_owned(),
            "proof".to_owned(),
        )
    );
}

fn assert_v30_root_import_data_preserved(db: &Connection) {
    let transfer = db
        .query_row(
            r#"
SELECT owner_did, recipient_device_id, phase
FROM identity_root_transfer_sender_v1
WHERE owner_identity_id = 'owner-legacy'
  AND local_device_id = 'device-1'
  AND message_id = 'message-old'"#,
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
        transfer,
        (
            "did:example:legacy".to_owned(),
            "device-2".to_owned(),
            "pending_delivery".to_owned(),
        )
    );
}

fn assert_direct_v2_ordinary_data_preserved(db: &Connection) {
    let session = db
        .query_row(
            r#"
SELECT peer_did, peer_device_id, revision, hex(state_blob)
FROM direct_e2ee_v2_sessions
WHERE owner_identity_id = 'owner-legacy'
  AND local_device_id = 'device-1'
  AND session_id = 'ordinary-session'"#,
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(
        session,
        (
            "did:example:peer".to_owned(),
            "peer-device".to_owned(),
            7,
            "01".to_owned(),
        )
    );
    let pending_count = db
        .query_row(
            r#"
SELECT COUNT(*)
FROM direct_e2ee_v2_pending
WHERE owner_identity_id = 'owner-legacy'
  AND local_device_id = 'device-1'
  AND operation_id = 'ordinary-operation'
  AND session_revision = 7
  AND hex(pending_blob) = '02'"#,
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap();
    assert_eq!(pending_count, 1);
    let bundle_count = db
        .query_row(
            r#"
SELECT COUNT(*)
FROM direct_e2ee_v2_prekey_bundles
WHERE owner_identity_id = 'owner-legacy'
  AND local_device_id = 'device-1'
  AND bundle_id = 'ordinary-bundle'
  AND status = 'published'"#,
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap();
    assert_eq!(bundle_count, 1);
}

fn assert_retired_direct_v2_private_data_removed(db: &Connection) {
    for table in [
        "direct_e2ee_v2_private_outbound",
        "direct_e2ee_v2_private_outbound_tombstones",
    ] {
        let count = db
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                [table],
                |row| row.get::<_, i64>(0),
            )
            .unwrap();
        assert_eq!(count, 0, "retired table {table} must not survive v31");
    }
    let retired_pending = db
        .query_row(
            "SELECT COUNT(*) FROM direct_e2ee_v2_pending
             WHERE operation_id = 'retired-operation'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap();
    assert_eq!(retired_pending, 0);
}

fn assert_v33_sync_foundation_exists(db: &Connection) {
    for table in [
        "sync_installation_state",
        "identity_account_bindings",
        "message_sync_state",
        "sync_applied_events",
        "sync_recovery_state",
        "local_mutation_outbox",
    ] {
        assert_schema_object_exists(db, "table", table);
    }
}
