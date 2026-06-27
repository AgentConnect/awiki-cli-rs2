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
        ("view", "threads"),
        ("view", "inbox"),
        ("view", "outbox"),
    ] {
        assert_schema_object_exists(&db, object.0, object.1);
    }
    assert_index_exists(&db, "idx_messages_owner_identity_thread");
    assert_index_exists(&db, "idx_messages_owner_identity_conversation");
    assert_index_exists(&db, "idx_conversation_summaries_owner_last");
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
