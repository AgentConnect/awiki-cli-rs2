use super::*;

fn upgrade_legacy_schema_for_test(connection: &Connection) -> crate::ImResult<()> {
    let version = current_schema_version(connection)?;
    create_schema(connection, version < CONVERSATION_SUMMARIES_SCHEMA_VERSION)?;
    set_schema_version(connection, SCHEMA_VERSION)
}

fn replace_sync_state_with_release_shape(connection: &Connection) {
    connection
        .execute_batch(
            r#"
DROP INDEX IF EXISTS idx_sync_state_owner_kind;
DROP TABLE sync_state;
CREATE TABLE sync_state (
    owner_identity_id TEXT NOT NULL,
    owner_did         TEXT NOT NULL DEFAULT '',
    scope             TEXT NOT NULL,
    checkpoint_kind   TEXT NOT NULL,
    event_seq         TEXT NOT NULL DEFAULT '0',
    updated_at        TEXT NOT NULL,
    metadata_json     TEXT,
    PRIMARY KEY (owner_identity_id, scope, checkpoint_kind)
);
CREATE INDEX idx_sync_state_owner_kind
ON sync_state(owner_identity_id, checkpoint_kind, updated_at DESC);
"#,
        )
        .unwrap();
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
        ("table", "message_sync_run_state"),
        ("table", "sync_lane_inbox"),
        ("table", "sync_lane_transport_state"),
        ("table", "sync_p5_input_outcomes"),
        ("table", "sync_p5_did_cutovers"),
        ("table", "sync_p6_input_outcomes"),
        ("table", "sync_p6_legacy_migration_repairs"),
        ("table", "sync_history_scope"),
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
        ("table", "did_transition_edges"),
        ("table", "registration_retired_join_rollovers"),
        ("view", "threads"),
        ("view", "inbox"),
        ("view", "outbox"),
    ] {
        assert_schema_object_exists(&db, object.0, object.1);
    }
    assert_index_exists(&db, "idx_messages_owner_identity_thread");
    assert_index_exists(&db, "idx_messages_owner_identity_conversation");
    assert_index_exists(&db, "idx_messages_owner_hydration_conversation_seq");
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
    for index in SYNC_V1B_DURABLE_LANE_INDEXES {
        assert_index_exists(&db, index);
    }
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
    assert_index_exists(&db, "idx_did_transition_edges_owner_successor");
    assert_index_exists(&db, "registration_retired_join_rollovers_owner_phase_idx");
    assert_column_exists(&db, "sync_lane_capability_state", "client_instance_id");
    assert_column_exists(
        &db,
        "sync_lane_capability_state",
        "negotiated_capabilities_json",
    );
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
        "hydration_state",
    ] {
        assert_column_exists(&db, "messages", column);
    }
    assert_column_exists(&db, "contacts", "peer_persona_id");
    assert_column_exists(&db, "sync_state", "sync_subject_id");
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
            vec![
                "owner_identity_id",
                "sync_subject_id",
                "scope",
                "checkpoint_kind",
            ],
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
fn local_state_schema_28_auto_migrates_hydration_probes_and_survives_restart() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("im.sqlite");
    {
        let db = Connection::open(&path).unwrap();
        ensure_schema(&db).unwrap();
        for (msg_id, content_type, content, server_seq) in [
            ("known-body", "text/plain", "body", 41),
            ("metadata-placeholder", "text/plain", "", 42),
            ("valid-empty-text", "text/plain", "", 43),
            ("valid-unsupported", "application/x-awiki-future", "", 44),
        ] {
            crate::internal::local_state::messages::upsert_message(
                &db,
                &crate::internal::local_state::messages::MessageRecord {
                    msg_id: msg_id.to_owned(),
                    owner_identity_id: "alice-id".to_owned(),
                    owner_did: "did:example:alice".to_owned(),
                    conversation_id: "dm:did:example:bob".to_owned(),
                    thread_id: "dm:did:example:bob".to_owned(),
                    direction: 0,
                    sender_did: "did:example:bob".to_owned(),
                    receiver_did: "did:example:alice".to_owned(),
                    content_type: content_type.to_owned(),
                    content: content.to_owned(),
                    server_seq: Some(server_seq),
                    stored_at: format!("2026-07-24T00:00:{server_seq}Z"),
                    ..Default::default()
                },
            )
            .unwrap();
        }
        let legacy_backlog_record = crate::internal::local_state::messages::MessageRecord {
            msg_id: "legacy-backlog-metadata".to_owned(),
            owner_identity_id: "carol-id".to_owned(),
            owner_did: "did:example:carol".to_owned(),
            conversation_id: "dm:did:example:dave".to_owned(),
            thread_id: "dm:did:example:dave".to_owned(),
            direction: 0,
            sender_did: "did:example:dave".to_owned(),
            receiver_did: "did:example:carol".to_owned(),
            content_type: "text/plain".to_owned(),
            server_seq: Some(45),
            stored_at: "2026-07-24T00:00:45Z".to_owned(),
            credential_name: "carol-id".to_owned(),
            ..Default::default()
        }
        .with_resolved_wire_thread("direct", "did:example:dave");
        let mut legacy_backlog_payload = serde_json::to_value(legacy_backlog_record).unwrap();
        legacy_backlog_payload
            .as_object_mut()
            .unwrap()
            .remove("hydration_state");
        db.execute(
            r#"INSERT INTO inbound_resolution_backlog
    (owner_identity_id, owner_did, event_id, event_seq, event_type, message_id,
     peer_did, message_record_json, resolution_state, error_code, error_detail,
     attempt_count, first_seen_at, last_attempt_at)
VALUES ('carol-id', 'did:example:carol', 'legacy-event-45', '45',
        'message.created', 'legacy-backlog-metadata', 'did:example:dave', ?1,
        'pending', 'identity_unresolved', '', 1, '45', '45')"#,
            [legacy_backlog_payload.to_string()],
        )
        .unwrap();
        db.execute_batch(
            r#"
DROP VIEW IF EXISTS threads;
DROP VIEW IF EXISTS inbox;
DROP VIEW IF EXISTS outbox;
CREATE TABLE messages_schema_28 AS
SELECT msg_id, owner_identity_id, owner_did, conversation_id,
       wire_thread_kind, wire_thread_ref, wire_identity_resolution_state,
       thread_id, direction, sender_did, receiver_did, group_id, group_did,
       content_type, content, title, server_seq, sent_at, stored_at, is_e2ee,
       is_read, sender_name, metadata, mentions_current_user, credential_name
FROM messages;
DROP TABLE messages;
ALTER TABLE messages_schema_28 RENAME TO messages;
CREATE UNIQUE INDEX messages_schema_28_owner_msg
ON messages(owner_identity_id, msg_id);
PRAGMA user_version = 28;
"#,
        )
        .unwrap();
        replace_sync_state_with_release_shape(&db);
    }

    {
        let mut db = Connection::open(&path).unwrap();
        ensure_schema(&db).unwrap();
        assert_eq!(current_schema_version(&db).unwrap(), SCHEMA_VERSION);
        assert_column_exists(&db, "messages", "hydration_state");
        assert_eq!(hydration_state(&db, "known-body"), "hydrated");
        for msg_id in [
            "metadata-placeholder",
            "valid-empty-text",
            "valid-unsupported",
        ] {
            assert_eq!(hydration_state(&db, msg_id), "legacy_probe");
        }

        let lookup = crate::directory::HandleLookupResult {
            handle: crate::ids::Handle::parse("dave.example.test", "").unwrap(),
            did: crate::ids::Did::parse("did:example:dave").unwrap(),
            user_id: "user-dave".to_owned(),
            domain: Some("example.test".to_owned()),
            status: Some("active".to_owned()),
            binding_generation: Some("1".to_owned()),
            profile: None,
            warnings: Vec::new(),
        };
        crate::internal::local_state::peer_personas::project_verified_handle(
            &mut db,
            "carol-id",
            "did:example:carol",
            &lookup,
        )
        .unwrap();
        assert_eq!(
            crate::internal::local_state::inbound_resolution_backlog::pending_count(
                &db, "carol-id"
            )
            .unwrap(),
            0
        );
        assert_eq!(
            hydration_state_for_owner(&db, "carol-id", "legacy-backlog-metadata"),
            "discovered"
        );
        let thread = crate::messages::ThreadRef::Direct(
            crate::ids::PeerRef::parse("did:example:dave", "").unwrap(),
        );
        let cursor = crate::internal::local_state::messages::catch_up_server_seq_for_thread_ref_for_owner_identity(
            &db,
            "carol-id",
            "did:example:carol",
            &thread,
        )
        .unwrap();
        assert_eq!(cursor.default_after_server_seq, Some(44));
        assert_eq!(cursor.hydration_gap_after_server_seq, Some(44));
        assert!(
            crate::internal::local_state::messages::list_messages_for_thread_ref_for_owner_identity(
                &db,
                "carol-id",
                "did:example:carol",
                &thread,
                10,
                None,
            )
            .unwrap()
            .records
            .is_empty()
        );
    }

    let reopened = Connection::open(&path).unwrap();
    ensure_schema(&reopened).unwrap();
    assert_eq!(
        hydration_state(&reopened, "metadata-placeholder"),
        "legacy_probe"
    );
    assert_eq!(
        hydration_state_for_owner(&reopened, "carol-id", "legacy-backlog-metadata"),
        "discovered"
    );
}

#[test]
fn sync_state_subject_migration_drops_ambiguous_current_did_rows() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("im.sqlite");
    {
        let db = Connection::open(&path).unwrap();
        ensure_schema(&db).unwrap();
        db.execute_batch(
            r#"
INSERT INTO identity_did_history
    (owner_identity_id, did, status, first_seen_at, last_seen_at)
VALUES
    ('retagged-id', 'did:retagged:old', 'previous', '50', '200'),
    ('retagged-id', 'did:retagged:new', 'current', '200', '200'),
    ('same-second-id', 'did:same-second:old', 'previous', '100', '200'),
    ('same-second-id', 'did:same-second:new', 'current', '200', '200'),
    ('later-updated-id', 'did:later-updated:old', 'previous', '2026-07-01T00:00:00Z', '2026-07-02T00:00:00Z'),
    ('later-updated-id', 'did:later-updated:new', 'current', '2026-07-02T00:00:00Z', '2026-07-02T00:00:00Z'),
    ('stable-id', 'did:stable', 'current', '200', '200'),
    ('historical-id', 'did:historical:old', 'previous', '50', '200'),
    ('historical-id', 'did:historical:new', 'current', '200', '200');

DROP TABLE sync_state;
CREATE TABLE sync_state (
    owner_identity_id TEXT NOT NULL,
    owner_did         TEXT NOT NULL DEFAULT '',
    scope             TEXT NOT NULL,
    checkpoint_kind   TEXT NOT NULL,
    event_seq         TEXT NOT NULL DEFAULT '0',
    updated_at        TEXT NOT NULL,
    metadata_json     TEXT,
    PRIMARY KEY (owner_identity_id, scope, checkpoint_kind)
);
CREATE INDEX idx_sync_state_owner_kind
ON sync_state(owner_identity_id, checkpoint_kind, updated_at DESC);
INSERT INTO sync_state
    (owner_identity_id, owner_did, scope, checkpoint_kind, event_seq, updated_at)
VALUES
    ('retagged-id', 'did:retagged:new', 'global', 'event_seq', '48', '100'),
    ('same-second-id', 'did:same-second:new', 'global', 'event_seq', '49', '200'),
    ('later-updated-id', 'did:later-updated:new', 'global', 'event_seq', '50', '2026-07-03T00:00:00Z'),
    ('stable-id', 'did:stable', 'global', 'event_seq', '12', '100'),
    ('historical-id', 'did:historical:old', 'global', 'event_seq', '31', '100');
"#,
        )
        .unwrap();

        migrate_sync_state_subject_scope(&db).unwrap();
        assert_eq!(current_schema_version(&db).unwrap(), SCHEMA_VERSION);
        assert_primary_key_columns(
            &db,
            "sync_state",
            &[
                "owner_identity_id",
                "sync_subject_id",
                "scope",
                "checkpoint_kind",
            ],
        );
        for (owner_identity_id, sync_subject_id) in [
            ("retagged-id", "did:retagged:new"),
            ("same-second-id", "did:same-second:new"),
            ("later-updated-id", "did:later-updated:new"),
        ] {
            assert!(
                crate::internal::local_state::sync_state::load_global_checkpoint(
                    &db,
                    owner_identity_id,
                    sync_subject_id,
                )
                .unwrap()
                .is_none()
            );
        }
        for (owner_identity_id, sync_subject_id, expected_seq) in [
            ("stable-id", "did:stable", "12"),
            ("historical-id", "did:historical:old", "31"),
        ] {
            assert_eq!(
                crate::internal::local_state::sync_state::load_global_checkpoint(
                    &db,
                    owner_identity_id,
                    sync_subject_id,
                )
                .unwrap()
                .unwrap()
                .event_seq,
                expected_seq
            );
        }
    }

    let reopened = Connection::open(&path).unwrap();
    ensure_schema(&reopened).unwrap();
    assert_eq!(current_schema_version(&reopened).unwrap(), SCHEMA_VERSION);
    assert_eq!(
        crate::internal::local_state::sync_state::load_global_checkpoint(
            &reopened,
            "historical-id",
            "did:historical:old",
        )
        .unwrap()
        .unwrap()
        .event_seq,
        "31"
    );
    for (owner_identity_id, sync_subject_id) in [
        ("retagged-id", "did:retagged:new"),
        ("same-second-id", "did:same-second:new"),
        ("later-updated-id", "did:later-updated:new"),
    ] {
        assert!(
            crate::internal::local_state::sync_state::load_global_checkpoint(
                &reopened,
                owner_identity_id,
                sync_subject_id,
            )
            .unwrap()
            .is_none()
        );
    }
}

#[test]
fn sync_state_schema_rejects_partial_subject_scoped_checkpoint_shape() {
    let db = Connection::open_in_memory().unwrap();
    ensure_schema(&db).unwrap();
    db.execute_batch(
        r#"
DROP TABLE sync_state;
CREATE TABLE sync_state (
    owner_identity_id TEXT NOT NULL,
    sync_subject_id   TEXT NOT NULL,
    scope             TEXT NOT NULL,
    checkpoint_kind   TEXT NOT NULL,
    event_seq         TEXT NOT NULL DEFAULT '0',
    updated_at        TEXT NOT NULL,
    metadata_json     TEXT,
    PRIMARY KEY (owner_identity_id, scope, checkpoint_kind)
);
"#,
    )
    .unwrap();

    assert!(matches!(
        ensure_sync_state_schema(&db),
        Err(crate::ImError::LocalStateUnavailable { detail })
            if detail.contains("sync_state subject-scoped schema is incomplete")
    ));
    assert_eq!(current_schema_version(&db).unwrap(), SCHEMA_VERSION);
}

#[test]
fn did_history_transition_does_not_relabel_sync_checkpoint_subject() {
    let mut db = Connection::open_in_memory().unwrap();
    ensure_schema(&db).unwrap();
    record_identity_did_history_transition(
        &mut db,
        "alice-id",
        "did:example:alice:old",
        &[] as &[&str],
    )
    .unwrap();
    {
        let tx = db.transaction().unwrap();
        crate::internal::local_state::sync_state::store_global_checkpoint_tx(
            &tx,
            "alice-id",
            "did:example:alice:old",
            "48",
            None,
        )
        .unwrap();
        tx.commit().unwrap();
    }

    let snapshot_counts = record_identity_did_history_transition(
        &mut db,
        "alice-id",
        "did:example:alice:new",
        &["did:example:alice:old"],
    )
    .unwrap();

    assert!(!snapshot_counts.contains_key("sync_state"));
    assert_eq!(
        crate::internal::local_state::sync_state::load_global_checkpoint(
            &db,
            "alice-id",
            "did:example:alice:old",
        )
        .unwrap()
        .unwrap()
        .event_seq,
        "48"
    );
    assert!(
        crate::internal::local_state::sync_state::load_global_checkpoint(
            &db,
            "alice-id",
            "did:example:alice:new",
        )
        .unwrap()
        .is_none()
    );
}

#[test]
fn canonical_direct_wire_repair_updates_only_provable_rows() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("im.sqlite");
    {
        let mut db = Connection::open(&path).unwrap();
        ensure_schema(&db).unwrap();
        let conversation_id = crate::internal::local_state::peer_personas::project_verified_handle(
            &mut db,
            "alice-id",
            "did:example:alice",
            &crate::directory::HandleLookupResult {
                handle: crate::ids::Handle::parse("bob.awiki.test", "").unwrap(),
                did: crate::ids::Did::parse("did:example:bob").unwrap(),
                user_id: "user-bob".to_owned(),
                domain: Some("awiki.test".to_owned()),
                status: Some("active".to_owned()),
                binding_generation: Some("1".to_owned()),
                profile: None,
                warnings: Vec::new(),
            },
        )
        .unwrap();
        for (message_id, sender_did, receiver_did, wire_peer_did) in [
            (
                "provable-old-wire",
                "did:example:alice",
                "did:example:bob",
                "did:example:bob",
            ),
            (
                "ambiguous-owner-snapshot",
                "did:example:bob",
                "did:example:mallory",
                "did:example:bob",
            ),
            (
                "unverified-peer-snapshot",
                "did:example:alice",
                "did:example:mallory",
                "did:example:mallory",
            ),
        ] {
            crate::internal::local_state::messages::upsert_message(
                &db,
                &crate::internal::local_state::messages::MessageRecord {
                    msg_id: message_id.to_owned(),
                    owner_identity_id: "alice-id".to_owned(),
                    owner_did: "did:example:alice".to_owned(),
                    conversation_id: conversation_id.clone(),
                    thread_id: conversation_id.clone(),
                    direction: 1,
                    sender_did: sender_did.to_owned(),
                    receiver_did: receiver_did.to_owned(),
                    content_type: "text/plain".to_owned(),
                    content: "schema 32 wire repair fixture".to_owned(),
                    server_seq: Some(5),
                    stored_at: "2026-07-26T00:00:00Z".to_owned(),
                    ..Default::default()
                }
                .with_resolved_wire_thread("direct", wire_peer_did),
            )
            .unwrap();
        }
        db.execute(
            r#"UPDATE messages
SET wire_thread_kind = 'thread', wire_thread_ref = conversation_id,
    wire_identity_resolution_state = 'resolved'
WHERE msg_id IN ('provable-old-wire', 'ambiguous-owner-snapshot',
                 'unverified-peer-snapshot')"#,
            [],
        )
        .unwrap();
        crate::internal::local_state::messages::repair_legacy_canonical_direct_wire_identities(&db)
            .unwrap();
        assert_eq!(current_schema_version(&db).unwrap(), SCHEMA_VERSION);
        assert_eq!(
            message_wire_identity(&db, "provable-old-wire"),
            ("direct".to_owned(), "did:example:bob".to_owned())
        );
        for message_id in ["ambiguous-owner-snapshot", "unverified-peer-snapshot"] {
            assert_eq!(
                message_wire_identity(&db, message_id),
                ("thread".to_owned(), conversation_id.clone())
            );
        }
    }

    let reopened = Connection::open(&path).unwrap();
    ensure_schema(&reopened).unwrap();
    assert_eq!(current_schema_version(&reopened).unwrap(), SCHEMA_VERSION);
    assert_eq!(
        message_wire_identity(&reopened, "provable-old-wire"),
        ("direct".to_owned(), "did:example:bob".to_owned())
    );
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
        &[
            "owner_identity_id",
            "sync_subject_id",
            "scope",
            "checkpoint_kind",
        ],
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
    assert_merged_v34_shape(&db);
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
    assert_merged_v34_shape(&db);
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
    assert_merged_v34_shape(&db);
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
    assert_merged_v34_shape(&db);

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
fn local_state_development_schema_v31_fails_closed_without_mutation() {
    let db = Connection::open_in_memory().unwrap();
    ensure_schema(&db).unwrap();
    db.execute(
        "INSERT INTO sync_state
         (owner_identity_id, sync_subject_id, scope, checkpoint_kind, event_seq, updated_at)
         VALUES ('alice-id', 'did:example:alice', 'global', 'event_seq', '9', '9')",
        [],
    )
    .unwrap();
    db.pragma_update(None, "user_version", 31).unwrap();

    assert!(matches!(
        ensure_schema(&db),
        Err(crate::ImError::LocalStateUnavailable { detail })
            if detail.contains("incompatible development profile")
                && detail.contains("reset this development local state")
    ));
    assert_eq!(current_schema_version(&db).unwrap(), 31);
    let event_seq: String = db
        .query_row("SELECT event_seq FROM sync_state", [], |row| row.get(0))
        .unwrap();
    assert_eq!(event_seq, "9");
}

#[test]
fn local_state_schema_v31_rejects_incomplete_release_shape_without_mutation() {
    let db = Connection::open_in_memory().unwrap();
    install_v31_fixture(&db);
    db.execute("DROP TABLE identity_root_transfer_sender_v1", [])
        .unwrap();

    assert!(matches!(
        ensure_schema(&db),
        Err(crate::ImError::LocalStateUnavailable { detail })
            if detail.contains("does not match the release/0714 predecessor shape")
    ));
    assert_eq!(current_schema_version(&db).unwrap(), 31);
    assert!(!has_column(&db, "messages", "hydration_state").unwrap());
    assert!(has_column(&db, "sync_state", "owner_did").unwrap());
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
    assert_merged_v34_shape(&db);
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
    assert_merged_v34_shape(&db);
}

#[test]
fn local_state_schema_converges_current_v32_shape_without_losing_checkpoint_data() {
    let db = Connection::open_in_memory().unwrap();
    install_current_reliable_sync_fixture(&db, 32);

    ensure_schema(&db).unwrap();

    assert_eq!(current_schema_version(&db).unwrap(), SCHEMA_VERSION);
    assert_merged_v34_shape(&db);
    assert_eq!(
        db.query_row(
            "SELECT event_seq FROM sync_state
             WHERE owner_identity_id = 'owner-current'
               AND sync_subject_id = 'did:example:current'",
            [],
            |row| row.get::<_, String>(0),
        )
        .unwrap(),
        "73"
    );
    assert_eq!(
        hydration_state_for_owner(&db, "owner-current", "current-message"),
        "hydrated"
    );
}

#[test]
fn local_state_schema_converges_current_and_release_v33_shapes() {
    let current = Connection::open_in_memory().unwrap();
    install_current_reliable_sync_fixture(&current, 33);
    ensure_schema(&current).unwrap();
    assert_eq!(current_schema_version(&current).unwrap(), SCHEMA_VERSION);
    assert_merged_v34_shape(&current);
    assert_eq!(
        current
            .query_row(
                "SELECT event_seq FROM sync_state
                 WHERE owner_identity_id = 'owner-current'",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
        "73"
    );

    let release = Connection::open_in_memory().unwrap();
    install_v33_fixture(&release);
    ensure_schema(&release).unwrap();
    assert_eq!(current_schema_version(&release).unwrap(), SCHEMA_VERSION);
    assert_merged_v34_shape(&release);
    assert_eq!(
        release
            .query_row(
                "SELECT scan_seq FROM message_sync_state
                 WHERE owner_identity_id = 'owner-v32'",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
        "42"
    );
}

#[test]
fn local_state_schema_converges_single_side_v34_shapes_idempotently() {
    let current = Connection::open_in_memory().unwrap();
    install_current_reliable_sync_fixture(&current, 34);
    ensure_schema(&current).unwrap();
    ensure_schema(&current).unwrap();
    assert_eq!(current_schema_version(&current).unwrap(), SCHEMA_VERSION);
    assert_merged_v34_shape(&current);
    assert_schema_object_exists(&current, "table", "inbound_resolution_thread_bindings");

    let release = Connection::open_in_memory().unwrap();
    install_v34_fixture(&release);
    ensure_schema(&release).unwrap();
    ensure_schema(&release).unwrap();
    assert_eq!(current_schema_version(&release).unwrap(), SCHEMA_VERSION);
    assert_merged_v34_shape(&release);
    assert_schema_object_exists(&release, "table", "inbound_resolution_thread_bindings");
    assert_eq!(
        release
            .query_row(
                "SELECT scan_seq FROM message_sync_state
                 WHERE owner_identity_id = 'owner-v32'",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
        "42"
    );
}

#[test]
fn local_state_schema_v27_remains_preopen_upgrade_gate() {
    let db = Connection::open_in_memory().unwrap();
    ensure_sync_state_schema(&db).unwrap();
    replace_sync_state_with_release_shape(&db);
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

fn hydration_state(db: &Connection, message_id: &str) -> String {
    hydration_state_for_owner(db, "alice-id", message_id)
}

fn hydration_state_for_owner(db: &Connection, owner_identity_id: &str, message_id: &str) -> String {
    db.query_row(
        "SELECT hydration_state FROM messages WHERE owner_identity_id = ?1 AND msg_id = ?2",
        (owner_identity_id, message_id),
        |row| row.get(0),
    )
    .unwrap()
}

fn message_wire_identity(db: &Connection, message_id: &str) -> (String, String) {
    db.query_row(
        "SELECT wire_thread_kind, wire_thread_ref FROM messages WHERE msg_id = ?1",
        [message_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .unwrap()
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

pub(crate) fn install_release_predecessor_fixture(db: &Connection, version: i64) {
    match version {
        28 => install_v28_fixture(db),
        29 => install_v29_fixture(db),
        30 => install_v30_fixture(db),
        31 => install_v31_fixture(db),
        other => panic!("unsupported release predecessor fixture version {other}"),
    }
}

pub(crate) fn downgrade_current_schema_to_release_v30_fixture(db: &Connection) {
    db.execute_batch(
        r#"
DROP VIEW IF EXISTS threads;
DROP VIEW IF EXISTS inbox;
DROP VIEW IF EXISTS outbox;
CREATE TABLE messages_release_v30 AS
SELECT msg_id, owner_identity_id, owner_did, conversation_id,
       wire_thread_kind, wire_thread_ref, wire_identity_resolution_state,
       thread_id, direction, sender_did, receiver_did, group_id, group_did,
       content_type, content, title, server_seq, sent_at, stored_at, is_e2ee,
       is_read, sender_name, metadata, mentions_current_user, credential_name
FROM messages;
DROP TABLE messages;
ALTER TABLE messages_release_v30 RENAME TO messages;
CREATE UNIQUE INDEX messages_release_v30_owner_msg
ON messages(owner_identity_id, msg_id);
"#,
    )
    .unwrap();
    replace_sync_state_with_release_shape(db);
    db.pragma_update(None, "user_version", 30).unwrap();
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

fn install_v33_fixture(db: &Connection) {
    install_v32_fixture(db);
    crate::internal::local_state::sync_v2::create_installation_schema(db).unwrap();
    db.pragma_update(None, "user_version", 33).unwrap();
}

fn install_v34_fixture(db: &Connection) {
    install_v33_fixture(db);
    db.execute_batch(crate::internal::local_state::sync_v2::READ_RECOVERY_SCHEMA_SQL)
        .unwrap();
    db.pragma_update(None, "user_version", 34).unwrap();
}

fn install_current_reliable_sync_fixture(db: &Connection, version: i64) {
    create_schema(db, true).unwrap();
    db.execute_batch(
        r#"
DROP TABLE sync_remote_read_states;
DROP TABLE sync_thread_bindings;
DROP TABLE local_mutation_outbox;
DROP TABLE sync_recovery_state;
DROP TABLE sync_applied_events;
DROP TABLE message_sync_state;
DROP TABLE identity_account_bindings;
DROP TABLE sync_installation_state;
ALTER TABLE thread_read_state DROP COLUMN remote_state_version;

INSERT INTO sync_state
    (owner_identity_id, sync_subject_id, scope, checkpoint_kind, event_seq, updated_at)
VALUES
    ('owner-current', 'did:example:current', 'global', 'sync_delta', '73', 'old');

INSERT INTO messages
    (msg_id, owner_identity_id, owner_did, conversation_id, thread_id,
     content_type, content, server_seq, hydration_state, stored_at, credential_name)
VALUES
    ('current-message', 'owner-current', 'did:example:current',
     'dm:did:example:peer', 'dm:did:example:peer', 'text/plain',
     'current-body', 73, 'hydrated', 'old', 'current');
"#,
    )
    .unwrap();
    db.pragma_update(None, "user_version", version).unwrap();
}

/// Historical v28 schema builder, copied from the v28 create path at
/// `0e543604`. It deliberately does not call the current `create_schema`,
/// because doing so and merely rewriting `user_version` would manufacture
/// v29-v32 objects that never existed in a real v28 database.
fn install_complete_v28_schema(db: &Connection) {
    create_identity_owned_schema(db, IdentityOwnedSchemaTableMode::Final).unwrap();
    db.execute_batch("ALTER TABLE messages DROP COLUMN hydration_state;")
        .unwrap();
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
    ensure_sync_state_schema(db).unwrap();
    replace_sync_state_with_release_shape(db);
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

fn assert_merged_v34_shape(db: &Connection) {
    for table in [
        "sync_installation_state",
        "identity_account_bindings",
        "message_sync_state",
        "sync_applied_events",
        "sync_recovery_state",
        "local_mutation_outbox",
        "sync_thread_bindings",
        "sync_remote_read_states",
    ] {
        assert_schema_object_exists(db, "table", table);
    }
    assert_column_exists(db, "messages", "hydration_state");
    assert_index_exists(db, "idx_messages_owner_hydration_conversation_seq");
    assert_column_exists(db, "sync_state", "sync_subject_id");
    assert_primary_key_columns(
        db,
        "sync_state",
        &[
            "owner_identity_id",
            "sync_subject_id",
            "scope",
            "checkpoint_kind",
        ],
    );
    assert_index_exists(db, "idx_sync_state_owner_kind");
    assert_column_exists(db, "thread_read_state", "remote_state_version");
    for index in SYNC_V2_REQUIRED_INDEXES {
        assert_index_exists(db, index);
    }
}

#[test]
fn v35_to_v36_creates_handle_recovery_state_when_transition_table_is_absent() {
    let db = Connection::open_in_memory().unwrap();
    create_schema(&db, true).unwrap();
    db.execute_batch(
        r#"
DROP TABLE handle_recovery_operations_v4;
DROP TABLE identity_transition_pending;
PRAGMA user_version=35;
"#,
    )
    .unwrap();

    ensure_schema(&db).unwrap();

    assert_eq!(current_schema_version(&db).unwrap(), SCHEMA_VERSION);
    assert_schema_object_exists(&db, "table", "identity_transition_pending");
    assert_schema_object_exists(&db, "table", "handle_recovery_operations_v4");
    for column in [
        "current_device_id",
        "device_auth_generation",
        "registry_version",
        "applied_at",
        "metadata_json",
    ] {
        assert_column_exists(&db, "identity_transition_pending", column);
    }
    assert_index_exists(&db, "idx_identity_transition_source");
    assert_index_exists(&db, "idx_identity_transition_active_owner");
    assert_index_exists(&db, "idx_identity_transition_owner_phase");
    assert_index_exists(&db, "idx_identity_transition_account_generation");
    assert_index_exists(&db, "idx_identity_transition_handle_epoch");

    ensure_schema(&db).unwrap();
    assert_eq!(current_schema_version(&db).unwrap(), SCHEMA_VERSION);
}

#[test]
fn current_schema_with_missing_recovery_index_fails_closed_without_repair() {
    let db = Connection::open_in_memory().unwrap();
    create_schema(&db, true).unwrap();
    set_schema_version(&db, SCHEMA_VERSION).unwrap();
    db.execute_batch("DROP INDEX idx_handle_recovery_operations_active_owner")
        .unwrap();

    let error = ensure_schema(&db).unwrap_err();
    assert!(matches!(
        error,
        crate::ImError::LocalStateUnavailable { .. }
    ));
    assert!(!has_index(&db, "idx_handle_recovery_operations_active_owner").unwrap());
}

#[test]
fn schema_37_migrates_v36_transactionally_and_reopens_idempotently() {
    let db = Connection::open_in_memory().unwrap();
    create_schema(&db, true).unwrap();
    db.execute(
        "INSERT INTO direct_peer_routes(owner_identity_id,conversation_id,peer_user_id,full_handle,current_did,updated_at) VALUES ('owner-1','dm:peer-scope:v1:alice:bob','user-2','bob.example.com','did:wba:example.com:users:bob:e1_old','1')",
        [],
    )
    .unwrap();
    db.execute_batch("DROP TABLE did_transition_edges; PRAGMA user_version=36;")
        .unwrap();

    ensure_schema(&db).unwrap();
    assert_eq!(current_schema_version(&db).unwrap(), SCHEMA_VERSION);
    assert_schema_object_exists(&db, "table", "did_transition_edges");
    assert_index_exists(&db, "idx_did_transition_edges_owner_successor");
    assert_eq!(
        db.query_row("SELECT current_did FROM direct_peer_routes", [], |row| {
            row.get::<_, String>(0)
        })
        .unwrap(),
        "did:wba:example.com:users:bob:e1_old"
    );
    for absent in ["did_transition_conflicts", "did_transition_reconcile_jobs"] {
        assert_eq!(
            db.query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
                [absent],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
            0
        );
    }

    ensure_schema(&db).unwrap();
    assert_eq!(current_schema_version(&db).unwrap(), SCHEMA_VERSION);
}

#[test]
fn schema_37_failed_migration_rolls_back_version_and_partial_index() {
    let db = Connection::open_in_memory().unwrap();
    create_schema(&db, true).unwrap();
    db.execute_batch(
        r#"
DROP TABLE did_transition_edges;
CREATE TABLE did_transition_edges (owner_identity_id TEXT NOT NULL);
PRAGMA user_version=36;
"#,
    )
    .unwrap();

    assert!(ensure_schema(&db).is_err());
    assert_eq!(current_schema_version(&db).unwrap(), 36);
    assert!(!has_index(&db, "idx_did_transition_edges_owner_successor").unwrap());
}

#[test]
fn schema_38_migrates_v37_transactionally_and_reopens_idempotently() {
    let db = Connection::open_in_memory().unwrap();
    create_schema(&db, true).unwrap();
    db.execute_batch("DROP TABLE registration_retired_join_rollovers; PRAGMA user_version=37;")
        .unwrap();

    ensure_schema(&db).unwrap();
    assert_eq!(current_schema_version(&db).unwrap(), SCHEMA_VERSION);
    assert_schema_object_exists(&db, "table", "registration_retired_join_rollovers");
    assert_index_exists(&db, "registration_retired_join_rollovers_owner_phase_idx");

    ensure_schema(&db).unwrap();
    assert_eq!(current_schema_version(&db).unwrap(), SCHEMA_VERSION);
}

#[test]
fn schema_38_failed_migration_rolls_back_table_index_and_version() {
    let db = Connection::open_in_memory().unwrap();
    create_schema(&db, true).unwrap();
    db.execute_batch("DROP TABLE registration_retired_join_rollovers; PRAGMA user_version=37;")
        .unwrap();
    FAIL_SCHEMA_38_AFTER_TABLE_CREATE.with(|fail| fail.set(true));

    assert!(ensure_schema(&db).is_err());
    assert_eq!(current_schema_version(&db).unwrap(), 37);
    assert!(!has_table(&db, "registration_retired_join_rollovers").unwrap());
    assert!(!has_index(&db, "registration_retired_join_rollovers_owner_phase_idx").unwrap());
}

#[test]
fn schema_39_migrates_v38_transactionally_and_reopens_idempotently() {
    let db = Connection::open_in_memory().unwrap();
    create_schema(&db, true).unwrap();
    db.execute_batch("DROP TABLE IF EXISTS local_identity_deletions; PRAGMA user_version=38;")
        .unwrap();

    ensure_schema(&db).unwrap();
    assert_eq!(current_schema_version(&db).unwrap(), SCHEMA_VERSION);
    assert_schema_object_exists(&db, "table", "local_identity_deletions");
    assert_index_exists(&db, "local_identity_deletions_active_owner_idx");
    assert_index_exists(&db, "local_identity_deletions_handle_phase_idx");

    ensure_schema(&db).unwrap();
    assert_eq!(current_schema_version(&db).unwrap(), SCHEMA_VERSION);
}

#[test]
fn schema_39_failed_migration_rolls_back_table_indexes_and_version() {
    let db = Connection::open_in_memory().unwrap();
    create_schema(&db, true).unwrap();
    db.execute_batch("DROP TABLE IF EXISTS local_identity_deletions; PRAGMA user_version=38;")
        .unwrap();
    FAIL_SCHEMA_39_AFTER_TABLE_CREATE.with(|fail| fail.set(true));

    assert!(ensure_schema(&db).is_err());
    assert_eq!(current_schema_version(&db).unwrap(), 38);
    assert!(!has_table(&db, "local_identity_deletions").unwrap());
    assert!(!has_index(&db, "local_identity_deletions_active_owner_idx").unwrap());
    assert!(!has_index(&db, "local_identity_deletions_handle_phase_idx").unwrap());
}

fn drop_sync_v1b_durable_lane_shape(db: &Connection) {
    db.execute_batch(
        r#"
DROP TABLE IF EXISTS sync_p5_input_outcomes;
DROP TABLE IF EXISTS sync_p6_input_outcomes;
DROP TABLE IF EXISTS sync_p5_did_cutovers;
DROP TABLE IF EXISTS sync_p6_legacy_migration_repairs;
DROP TABLE IF EXISTS sync_lane_transport_state;
DROP TABLE IF EXISTS sync_lane_inbox;
DROP TABLE IF EXISTS sync_history_scope;
"#,
    )
    .unwrap();
}

fn assert_sync_v1b_durable_lane_shape(db: &Connection) {
    for (table, columns) in SYNC_V1B_DURABLE_LANE_TABLE_COLUMNS {
        assert_schema_object_exists(db, "table", table);
        for column in *columns {
            assert_column_exists(db, table, column);
        }
    }
    for index in SYNC_V1B_DURABLE_LANE_INDEXES {
        assert_index_exists(db, index);
    }
}

fn downgrade_sync_v1a_reliability_shape_to_v39(db: &Connection) {
    db.execute_batch(
        r#"
DROP TABLE message_sync_run_state;
ALTER TABLE sync_lane_capability_state RENAME TO sync_lane_capability_state_v40;
CREATE TABLE sync_lane_capability_state (
    owner_identity_id                  TEXT PRIMARY KEY,
    negotiated_device_auth_generation TEXT NOT NULL,
    CHECK (
        negotiated_device_auth_generation <> ''
        AND negotiated_device_auth_generation NOT GLOB '*[^0-9]*'
        AND substr(negotiated_device_auth_generation, 1, 1) <> '0'
    ),
    FOREIGN KEY (owner_identity_id)
        REFERENCES identity_account_bindings(owner_identity_id)
        ON DELETE CASCADE
);
INSERT INTO sync_lane_capability_state(
    owner_identity_id,
    negotiated_device_auth_generation
)
SELECT owner_identity_id, negotiated_device_auth_generation
FROM sync_lane_capability_state_v40;
DROP TABLE sync_lane_capability_state_v40;
PRAGMA user_version=39;
"#,
    )
    .unwrap();
}

#[test]
fn schema_41_migrates_v39_without_losing_capability_state() {
    let db = Connection::open_in_memory().unwrap();
    create_schema(&db, true).unwrap();
    db.execute(
        "INSERT INTO identity_account_bindings(owner_identity_id,account_id,current_did,device_id,identity_generation,device_auth_generation,created_at,updated_at) VALUES ('owner-1','account-1','did:wba:example.com:users:alice','device-1','1','1',1,1)",
        [],
    )
    .unwrap();
    db.execute(
        "INSERT INTO sync_lane_capability_state(owner_identity_id,negotiated_device_auth_generation) VALUES ('owner-1','1')",
        [],
    )
    .unwrap();
    downgrade_sync_v1a_reliability_shape_to_v39(&db);
    drop_sync_v1b_durable_lane_shape(&db);

    ensure_schema(&db).unwrap();

    assert_eq!(current_schema_version(&db).unwrap(), SCHEMA_VERSION);
    assert_sync_v1b_durable_lane_shape(&db);
    assert_schema_object_exists(&db, "table", "message_sync_run_state");
    assert_column_exists(&db, "sync_lane_capability_state", "client_instance_id");
    assert_column_exists(
        &db,
        "sync_lane_capability_state",
        "negotiated_capabilities_json",
    );
    assert_eq!(
        db.query_row(
            "SELECT negotiated_device_auth_generation FROM sync_lane_capability_state WHERE owner_identity_id='owner-1'",
            [],
            |row| row.get::<_, String>(0),
        )
        .unwrap(),
        "1",
    );
    assert_eq!(
        db.query_row(
            "SELECT COUNT(*) FROM sync_lane_capability_state WHERE client_instance_id IS NULL AND negotiated_capabilities_json IS NULL",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap(),
        1,
    );

    ensure_schema(&db).unwrap();
    assert_eq!(current_schema_version(&db).unwrap(), SCHEMA_VERSION);
}

#[test]
fn schema_41_failed_v39_migration_rolls_back_shape_and_version() {
    let db = Connection::open_in_memory().unwrap();
    create_schema(&db, true).unwrap();
    downgrade_sync_v1a_reliability_shape_to_v39(&db);
    drop_sync_v1b_durable_lane_shape(&db);
    FAIL_SCHEMA_41_AFTER_DURABLE_LANES_CREATE.with(|fail| fail.set(true));

    assert!(ensure_schema(&db).is_err());
    assert_eq!(current_schema_version(&db).unwrap(), 39);
    assert!(!has_table(&db, "message_sync_run_state").unwrap());
    for (table, _) in SYNC_V1B_DURABLE_LANE_TABLE_COLUMNS {
        assert!(!has_table(&db, table).unwrap());
    }
    assert!(!has_column(&db, "sync_lane_capability_state", "client_instance_id").unwrap());
    assert!(!has_column(
        &db,
        "sync_lane_capability_state",
        "negotiated_capabilities_json"
    )
    .unwrap());
}

#[test]
fn schema_41_rejects_a_malformed_v39_reliability_table_without_repair() {
    let db = Connection::open_in_memory().unwrap();
    create_schema(&db, true).unwrap();
    downgrade_sync_v1a_reliability_shape_to_v39(&db);
    drop_sync_v1b_durable_lane_shape(&db);
    db.execute_batch("CREATE TABLE message_sync_run_state(owner_identity_id TEXT PRIMARY KEY);")
        .unwrap();

    let error = ensure_schema(&db).unwrap_err();

    assert!(matches!(
        error,
        crate::ImError::LocalStateUnavailable { .. }
    ));
    assert_eq!(current_schema_version(&db).unwrap(), 39);
    assert!(!has_column(&db, "message_sync_run_state", "run_generation").unwrap());
    assert!(!has_column(&db, "sync_lane_capability_state", "client_instance_id").unwrap());
}

#[test]
fn schema_41_repairs_incomplete_v40_and_lane_transport_state_is_usable() {
    let db = Connection::open_in_memory().unwrap();
    create_schema(&db, true).unwrap();
    db.execute(
        "INSERT INTO identity_account_bindings(owner_identity_id,account_id,current_did,device_id,identity_generation,device_auth_generation,created_at,updated_at) VALUES ('owner-1','account-1','did:wba:example.com:users:alice','device-1','1','1',1,1)",
        [],
    )
    .unwrap();
    drop_sync_v1b_durable_lane_shape(&db);
    db.pragma_update(None, "user_version", SYNC_V1A_RELIABILITY_SCHEMA_VERSION)
        .unwrap();

    ensure_schema(&db).unwrap();

    assert_eq!(current_schema_version(&db).unwrap(), SCHEMA_VERSION);
    assert_sync_v1b_durable_lane_shape(&db);
    assert_eq!(
        db.query_row(
            "SELECT COUNT(*) FROM identity_account_bindings",
            [],
            |row| { row.get::<_, i64>(0) }
        )
        .unwrap(),
        1,
    );
    crate::internal::local_state::sync_v2::record_lane_transport_error(
        &db,
        "owner-1",
        crate::internal::wire::sync_v2::SyncLaneV3::P5Device,
        Some("temporary_transport_failure"),
        1,
    )
    .unwrap();
    let states =
        crate::internal::local_state::sync_v2::load_lane_transport_states(&db, "owner-1").unwrap();
    assert_eq!(states.len(), 1);
    assert_eq!(
        states[0].last_transport_error.as_deref(),
        Some("temporary_transport_failure")
    );

    ensure_schema(&db).unwrap();
    assert_eq!(current_schema_version(&db).unwrap(), SCHEMA_VERSION);
}

#[test]
fn schema_41_migrates_legacy_p6_rows_after_the_v40_schema_transaction() {
    let db = Connection::open_in_memory().unwrap();
    db.pragma_update(None, "foreign_keys", "ON").unwrap();
    create_schema(&db, true).unwrap();
    db.execute(
        "INSERT INTO identity_account_bindings(owner_identity_id,account_id,current_did,device_id,identity_generation,device_auth_generation,created_at,updated_at) VALUES ('owner-1','account-1','did:wba:example.com:users:alice','device-1','1','1',1,1)",
        [],
    )
    .unwrap();
    db.execute(
        "INSERT INTO sync_installation_state(owner_identity_id,client_instance_id,created_at) VALUES ('owner-1','installation-1',1)",
        [],
    )
    .unwrap();
    db.execute(
        r#"
INSERT INTO p6_lane_blockers(
    owner_identity_id,event_id,stream_epoch,event_seq,event_type,
    group_did,group_event_seq,payload_json,attempt_count,
    last_error_code,created_at,updated_at
) VALUES (
    'owner-1','legacy-event-1','1','1','p6.delivery.created',
    'did:example:group','1',
    '{"meta":{},"body":{"group_did":"did:example:group"}}',
    1,'deferred',1,1
)
"#,
        [],
    )
    .unwrap();
    drop_sync_v1b_durable_lane_shape(&db);
    db.pragma_update(None, "user_version", SYNC_V1A_RELIABILITY_SCHEMA_VERSION)
        .unwrap();

    ensure_schema(&db).unwrap();

    assert_eq!(current_schema_version(&db).unwrap(), SCHEMA_VERSION);
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM sync_lane_inbox", [], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap(),
        1,
    );
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM p6_lane_blockers", [], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap(),
        1,
    );

    ensure_schema(&db).unwrap();
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM sync_lane_inbox", [], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap(),
        1,
    );
}

#[test]
fn schema_41_failed_v40_migration_rolls_back_all_new_lane_objects() {
    let db = Connection::open_in_memory().unwrap();
    create_schema(&db, true).unwrap();
    drop_sync_v1b_durable_lane_shape(&db);
    db.pragma_update(None, "user_version", SYNC_V1A_RELIABILITY_SCHEMA_VERSION)
        .unwrap();
    FAIL_SCHEMA_41_AFTER_DURABLE_LANES_CREATE.with(|fail| fail.set(true));

    assert!(ensure_schema(&db).is_err());

    assert_eq!(
        current_schema_version(&db).unwrap(),
        SYNC_V1A_RELIABILITY_SCHEMA_VERSION
    );
    for (table, _) in SYNC_V1B_DURABLE_LANE_TABLE_COLUMNS {
        assert!(!has_table(&db, table).unwrap());
    }
    for index in SYNC_V1B_DURABLE_LANE_INDEXES {
        assert!(!has_index(&db, index).unwrap());
    }
}

#[test]
fn current_schema_with_missing_v1b_table_fails_closed_without_repair() {
    let db = Connection::open_in_memory().unwrap();
    create_schema(&db, true).unwrap();
    set_schema_version(&db, SCHEMA_VERSION).unwrap();
    db.execute_batch("DROP TABLE sync_lane_transport_state")
        .unwrap();

    let error = ensure_schema(&db).unwrap_err();

    assert!(matches!(
        error,
        crate::ImError::LocalStateUnavailable { .. }
    ));
    assert_eq!(current_schema_version(&db).unwrap(), SCHEMA_VERSION);
    assert!(!has_table(&db, "sync_lane_transport_state").unwrap());
}

#[test]
fn v35_to_v36_adds_handle_recovery_receipt_fields_and_operation_index() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("state.sqlite");
    let db = crate::internal::local_state::open_writable(&path).unwrap();
    db.execute_batch(
        r#"
DROP INDEX IF EXISTS idx_identity_transition_source;
DROP INDEX IF EXISTS idx_identity_transition_active_owner;
DROP INDEX IF EXISTS idx_identity_transition_owner_phase;
DROP INDEX IF EXISTS idx_identity_transition_account_generation;
DROP INDEX IF EXISTS idx_identity_transition_handle_epoch;
ALTER TABLE identity_transition_pending RENAME TO identity_transition_pending_v36;
CREATE TABLE identity_transition_pending (
    recovery_id TEXT PRIMARY KEY,
    schema_version INTEGER NOT NULL,
    contract_version TEXT NOT NULL,
    contract_hash TEXT NOT NULL,
    source_kind TEXT NOT NULL CHECK(source_kind IN ('initiator','joined_device')),
    source_id TEXT NOT NULL,
    state_root_fingerprint TEXT NOT NULL,
    account_user_id TEXT NOT NULL,
    owner_identity_id TEXT NOT NULL,
    handle TEXT NOT NULL,
    previous_did TEXT NOT NULL,
    current_did TEXT NOT NULL,
    binding_generation TEXT NOT NULL,
    phase TEXT NOT NULL CHECK(phase IN ('pending','identity_switched','completed')),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
DROP TABLE identity_transition_pending_v36;
DROP TABLE handle_recovery_operations_v4;
PRAGMA user_version=35;
"#,
    )
    .unwrap();
    db.execute(
        "INSERT INTO identity_transition_pending(recovery_id,schema_version,contract_version,contract_hash,source_kind,source_id,state_root_fingerprint,account_user_id,owner_identity_id,handle,previous_did,current_did,binding_generation,phase,created_at,updated_at) VALUES ('legacy-incomplete-receipt',1,?1,?2,'initiator','op_legacy_12345678',?3,'user-1','owner-1','alice.example.invalid','did:wba:example.invalid:users:alice-old','did:wba:example.invalid:users:alice-new','7','completed','2026-08-07T00:00:00Z','2026-08-07T00:00:01Z')",
        rusqlite::params![
            crate::internal::identity_handle_recovery_pending::V4_CONTRACT_VERSION,
            crate::internal::identity_handle_recovery_pending::V4_CONTRACT_HASH,
            crate::internal::identity_transition_pending::state_root_fingerprint(&path),
        ],
    )
    .unwrap();

    ensure_schema(&db).unwrap();

    assert_eq!(current_schema_version(&db).unwrap(), SCHEMA_VERSION);
    for column in [
        "current_device_id",
        "device_auth_generation",
        "registry_version",
        "applied_at",
        "metadata_json",
    ] {
        assert_column_exists(&db, "identity_transition_pending", column);
    }
    assert_schema_object_exists(&db, "table", "handle_recovery_operations_v4");
    assert_index_exists(&db, "idx_identity_transition_owner_phase");
    assert_index_exists(&db, "idx_identity_transition_account_generation");
    assert_index_exists(&db, "idx_identity_transition_handle_epoch");
    assert_eq!(
        db.query_row(
            "SELECT COUNT(*) FROM identity_transition_pending",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap(),
        1,
    );
    assert!(
        crate::internal::identity_transition_pending::load(&path, "legacy-incomplete-receipt")
            .is_err()
    );
}
