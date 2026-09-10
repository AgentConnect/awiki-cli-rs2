use rusqlite::{params, Connection};

use super::{schema, sync_inbox, sync_v2};
use serde_json::json;

fn database() -> Connection {
    initialized_database(Connection::open_in_memory().unwrap())
}

fn initialized_database(db: Connection) -> Connection {
    db.pragma_update(None, "foreign_keys", "ON").unwrap();
    schema::ensure_schema(&db).unwrap();
    db.execute(
        "INSERT INTO identity_account_bindings(
            owner_identity_id, account_id, current_did, device_id,
            identity_generation, device_auth_generation, created_at, updated_at
         ) VALUES ('owner', 'account', 'did:example:owner', 'device', '1', '1', 1, 1)",
        [],
    )
    .unwrap();
    db
}

fn insert_raw(db: &Connection, id: &str, lane: &str, event_type: &str, created_at: i64) {
    db.execute(
        "INSERT INTO sync_lane_inbox(
            input_id, owner_identity_id, lane, lane_epoch, position, event_id,
            event_type, raw_payload_json, payload_bytes, account_id_snapshot,
            device_id_snapshot, auth_generation_snapshot, client_instance_id_snapshot,
            received_at, created_at
         ) VALUES (?1, 'owner', ?2, '1', CAST((SELECT COUNT(*)+1 FROM sync_lane_inbox) AS TEXT), ?1, ?3, '{}', 2,
                   'account', 'device', '1', 'installation', '2026-09-10T00:00:00Z', ?4)",
        params![id, lane, event_type, created_at],
    )
    .unwrap();
}

#[test]
fn ordinary_and_p5_events_share_the_existing_inbox() {
    let db = database();
    insert_raw(&db, "ordinary", "ordinary", "message.created", 10);
    insert_raw(&db, "secure", "p5_device", "p5.delivery.created", 10);
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM sync_lane_inbox", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        2
    );
}

#[test]
fn unprocessed_input_expires_after_two_days_without_changing_cursor() {
    let db = database();
    let _ = receive_batch(&db, "original", "1");
    insert_raw(&db, "expired", "p5_device", "p5.delivery.created", 10);
    assert_eq!(
        sync_v2::purge_closed_sync_lane_inputs(&db, 10 + 48 * 3600 - 1, 32).unwrap(),
        0
    );
    assert_eq!(
        sync_v2::purge_closed_sync_lane_inputs(&db, 10 + 48 * 3600, 32).unwrap(),
        1
    );
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM sync_lane_inbox", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM sync_applied_events", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(cursor(&db), "0");
}

fn binding(db: &Connection) -> sync_v2::IdentityAccountBinding {
    sync_v2::load_identity_account_binding(db, "owner")
        .unwrap()
        .unwrap()
}

fn receive_batch(db: &Connection, id: &str, sequence: &str) -> sync_inbox::OrdinaryReceiveBatch {
    let binding = binding(db);
    let installation = sync_v2::load_or_create_sync_client_instance_id(db, "owner").unwrap();
    if !matches!(
        sync_v2::load_message_sync_state(db, "owner").unwrap(),
        sync_v2::MessageSyncStateAccess::Ready(_)
    ) {
        sync_v2::bootstrap_message_sync_state(
            db,
            &sync_v2::MessageSyncState {
                owner_identity_id: "owner".into(),
                account_id: "account".into(),
                protocol_device_id: "device".into(),
                device_auth_generation: "1".into(),
                stream_epoch: "1".into(),
                scan_seq: "0".into(),
                bootstrap_state: "active".into(),
                last_server_time: None,
                last_success_at: None,
                last_error_code: None,
                metadata_json: None,
                updated_at: 1,
            },
        )
        .unwrap();
    }
    let event = crate::internal::wire::sync_v2::SyncEventV2 {
        event_id: id.into(),
        stream_epoch: "1".into(),
        event_seq: sequence.into(),
        event_type: "message.created".into(),
        schema_version: 1,
        ignore_safe: false,
        account_id: "account".into(),
        recipient_device_id: Some("device".into()),
        origin_did: None,
        origin_device_id: None,
        aggregate_kind: "direct".into(),
        aggregate_id: "conversation-a".into(),
        state_version: None,
        thread_key: Some("remote-a".into()),
        occurred_at: "2026-09-10T00:00:00Z".into(),
        payload: json!({"message_id":id}),
        source: None,
    };
    sync_inbox::OrdinaryReceiveBatch {
        binding,
        client_instance_id: installation,
        expected_run_generation: None,
        stream_epoch: "1".into(),
        expected_scan_seq: cursor(db),
        next_scan_seq: sequence.into(),
        server_time: "2026-09-10T00:00:00Z".into(),
        events: vec![sync_inbox::InboxEvent {
            event_id: id.into(),
            position: sequence.into(),
            event_type: "message.created".into(),
            payload: json!({"event":event,"hydrated":{"message_id":id,"content":"complete body","server_seq":sequence}}),
            processing_scope: "remote-a".into(),
            group_did: None,
        }],
    }
}

fn cursor(db: &Connection) -> String {
    match sync_v2::load_message_sync_state(db, "owner").unwrap() {
        sync_v2::MessageSyncStateAccess::Ready(state) => state.scan_seq,
        _ => panic!("fixture cursor not ready"),
    }
}

#[test]
fn reception_advances_without_any_business_projection_and_failure_does_not_retract_it() {
    let db = database();
    let batch = receive_batch(&db, "message-102", "102");
    assert_eq!(
        sync_inbox::receive_ordinary(&db, batch, 100)
            .unwrap()
            .received,
        1
    );
    assert_eq!(cursor(&db), "102");
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM messages", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    let claim = sync_inbox::claim_inputs(&db, &binding(&db), 101, 1)
        .unwrap()
        .remove(0);
    sync_inbox::fail_claim(&db, &claim, "message_wire_identity_conflict", None, 101).unwrap();
    let batch = receive_batch(&db, "message-103", "103");
    sync_inbox::receive_ordinary(&db, batch, 102).unwrap();
    assert_eq!(cursor(&db), "103");
    let next = sync_inbox::claim_inputs(&db, &binding(&db), 103, 1).unwrap();
    assert_eq!(next.len(), 1);
    assert_eq!(next[0].event_id, "message-103");
    assert_eq!(
        db.query_row(
            "SELECT processing_error_code FROM sync_lane_inbox WHERE event_id='message-102'",
            [],
            |row| row.get::<_, String>(0)
        )
        .unwrap(),
        "message_wire_identity_conflict"
    );
}

#[test]
fn incomplete_hydration_and_failed_insert_do_not_advance_reception() {
    let db = database();
    let mut batch = receive_batch(&db, "message-1", "1");
    batch.events[0]
        .payload
        .as_object_mut()
        .unwrap()
        .remove("hydrated");
    assert!(sync_inbox::receive_ordinary(&db, batch, 100).is_err());
    assert_eq!(cursor(&db), "0");
    db.execute_batch("CREATE TRIGGER fail_inbox BEFORE INSERT ON sync_lane_inbox BEGIN SELECT RAISE(ABORT, 'storage failure'); END;").unwrap();
    assert!(sync_inbox::receive_ordinary(&db, receive_batch(&db, "message-1", "1"), 100).is_err());
    assert_eq!(cursor(&db), "0");
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM sync_lane_inbox", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[test]
fn duplicate_does_not_refresh_retention_and_conflicting_payload_does_not_overwrite() {
    let db = database();
    sync_inbox::receive_ordinary(&db, receive_batch(&db, "message-1", "1"), 100).unwrap();
    assert_eq!(
        sync_inbox::receive_ordinary(&db, receive_batch(&db, "message-1", "1"), 200)
            .unwrap()
            .duplicates,
        1
    );
    let mut conflict = receive_batch(&db, "message-1", "1");
    conflict.events[0].payload["hydrated"]["content"] = json!("different");
    assert!(sync_inbox::receive_ordinary(&db, conflict, 300).is_err());
    assert_eq!(
        db.query_row("SELECT created_at FROM sync_lane_inbox", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        100
    );
    assert_eq!(cursor(&db), "1");
}

#[test]
fn capacity_eviction_is_oldest_first_and_rolls_back_with_the_receive_transaction() {
    let db = database();
    for (id, time) in [("oldest", 1), ("middle", 2), ("newest", 3)] {
        insert_raw(&db, id, "p5_device", "p5.delivery.created", time);
    }
    let transaction = db.unchecked_transaction().unwrap();
    let removed = sync_inbox::make_room_with_limit(&transaction, 2, 10, 3).unwrap();
    assert_eq!(
        removed
            .iter()
            .map(|row| row.input_id.as_str())
            .collect::<Vec<_>>(),
        ["oldest", "middle"]
    );
    transaction.rollback().unwrap();
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM sync_lane_inbox", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        3
    );
    let transaction = db.unchecked_transaction().unwrap();
    assert!(sync_inbox::make_room_with_limit(&transaction, 0, 10, 3)
        .unwrap()
        .is_empty());
    let removed = sync_inbox::make_room_with_limit(&transaction, 1, 10, 3).unwrap();
    assert_eq!(removed[0].input_id, "oldest");
    // A new row uses the released stable position, exactly as a later lane event would.
    transaction
        .execute(
            "UPDATE sync_lane_inbox SET position=CAST(CAST(position AS INTEGER)+10 AS TEXT)",
            [],
        )
        .unwrap();
    insert_raw(
        &transaction,
        "incoming",
        "p5_device",
        "p5.delivery.created",
        10,
    );
    transaction.commit().unwrap();
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM sync_lane_inbox", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        3
    );
}

#[test]
fn claims_do_not_repeat_and_expired_attempts_cannot_commit_after_reclaim() {
    let db = database();
    insert_raw(&db, "one", "p5_device", "p5.delivery.created", 100);
    let first = sync_inbox::claim_inputs(&db, &binding(&db), 101, 1)
        .unwrap()
        .remove(0);
    assert!(sync_inbox::claim_inputs(&db, &binding(&db), 102, 1)
        .unwrap()
        .is_empty());
    let second = sync_inbox::claim_inputs(&db, &binding(&db), 101 + sync_inbox::CLAIM_SECONDS, 1)
        .unwrap()
        .remove(0);
    assert_ne!(first.token, second.token);
    assert!(sync_inbox::require_claim(&db, &first, 162).is_err());
    sync_inbox::require_claim(&db, &second, 162).unwrap();
    db.execute("UPDATE identity_account_bindings SET device_auth_generation='2' WHERE owner_identity_id='owner'", []).unwrap();
    assert!(sync_inbox::require_claim(&db, &second, 162).is_err());
}

#[test]
fn expiration_invalidates_an_active_attempt_and_does_not_create_success_receipts() {
    let db = database();
    insert_raw(&db, "one", "p5_device", "p5.delivery.created", 100);
    let now = 100 + sync_inbox::RETENTION_SECONDS - 1;
    let claim = sync_inbox::claim_inputs(&db, &binding(&db), now, 1)
        .unwrap()
        .remove(0);
    assert_eq!(
        sync_inbox::purge_expired(&db, now + 1, 32).unwrap().len(),
        1
    );
    assert!(sync_inbox::complete_claim(&db, &claim, now + 1).is_err());
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM sync_lane_applied_events", [], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap(),
        0
    );
}

#[test]
fn schema44_upgrade_preserves_pending_secure_inputs_and_outcome_foreign_keys() {
    let db = database();
    db.execute_batch("DROP TABLE sync_p5_input_outcomes; DROP TABLE sync_p6_input_outcomes; DROP TABLE sync_lane_inbox;").unwrap();
    db.execute_batch(include_str!(
        "../../../testdata/message-sync/schema44-inbox.sql"
    ))
    .unwrap();
    db.execute_batch(sync_v2::SYNC_V2_SCHEMA_SQL).unwrap();
    insert_raw(&db, "legacy", "p5_device", "p5.delivery.created", 100);
    db.execute("INSERT INTO sync_p5_input_outcomes(input_id,owner_identity_id,peer_scope,status,retryable,attempt_count,next_retry_at,updated_at) VALUES('legacy','owner','did:example:peer','pending',1,2,150,101)", []).unwrap();
    db.pragma_update(None, "user_version", 44).unwrap();
    schema::ensure_schema(&db).unwrap();
    assert_eq!(schema::current_schema_version(&db).unwrap(), 45);
    assert_eq!(
        db.query_row(
            "SELECT created_at FROM sync_lane_inbox WHERE input_id='legacy'",
            [],
            |row| row.get::<_, i64>(0)
        )
        .unwrap(),
        100
    );
    assert_eq!(
        db.query_row(
            "SELECT attempt_count FROM sync_p5_input_outcomes WHERE input_id='legacy'",
            [],
            |row| row.get::<_, i64>(0)
        )
        .unwrap(),
        2
    );
    assert!(sync_inbox::claim_inputs(&db, &binding(&db), 149, 1)
        .unwrap()
        .is_empty());
    assert_eq!(
        sync_inbox::claim_inputs(&db, &binding(&db), 150, 1)
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap(),
        0
    );
}

fn apply_input(
    claim: &sync_inbox::InputClaim,
    message: super::messages::MessageRecord,
) -> sync_v2::DeltaApplyInputV2 {
    sync_v2::DeltaApplyInputV2 {
        owner_identity_id: claim.owner_identity_id.clone(),
        expected_run_generation: None,
        owner_did: claim.owner_did.clone(),
        account_id: claim.account_id.clone(),
        protocol_device_id: claim.device_id.clone(),
        device_auth_generation: claim.auth_generation.clone(),
        stream_epoch: claim.lane_epoch.clone(),
        next_scan_seq: claim.position.clone(),
        server_time: "2026-09-10T00:00:00Z".into(),
        events: vec![sync_v2::DeltaApplyEventV2 {
            event_id: claim.event_id.clone(),
            event_seq: claim.position.clone(),
            event_type: claim.event_type.clone(),
            thread_bindings: vec![sync_v2::SyncThreadBinding {
                owner_identity_id: claim.owner_identity_id.clone(),
                remote_thread_key: "remote-a".into(),
                thread_kind: "direct".into(),
                conversation_id: message.conversation_id.clone(),
                updated_at: 1,
            }],
            messages: vec![message],
            ..Default::default()
        }],
    }
}

#[test]
fn conflicting_102_does_not_block_same_conversation_103_or_roll_back_receive_cursor() {
    let mut db = database();
    let project = |db: &mut Connection, did: &str, generation: &str| {
        super::peer_personas::project_verified_handle(
            db,
            "owner",
            "did:example:owner",
            &crate::directory::HandleLookupResult {
                handle: crate::ids::Handle::parse("peer.awiki.info", "").unwrap(),
                did: crate::ids::Did::parse(did).unwrap(),
                user_id: "peer".into(),
                domain: Some("awiki.info".into()),
                status: Some("active".into()),
                binding_generation: Some(generation.into()),
                profile: None,
                warnings: vec![],
            },
        )
        .unwrap()
    };
    let conversation = project(&mut db, "did:example:old", "1");
    let echo = super::messages::MessageRecord {
        msg_id: "message-102".into(),
        owner_identity_id: "owner".into(),
        owner_did: "did:example:owner".into(),
        conversation_id: conversation.clone(),
        thread_id: conversation.clone(),
        direction: 1,
        sender_did: "did:example:owner".into(),
        receiver_did: "did:example:old".into(),
        content_type: "text/plain".into(),
        content: "original".into(),
        metadata: json!({"operation_id":"operation-102","delivery_state":"pending"}).to_string(),
        ..Default::default()
    }
    .with_resolved_wire_thread("direct", "did:example:old");
    super::messages::upsert_message(&db, &echo).unwrap();
    project(&mut db, "did:example:new", "2");
    sync_inbox::receive_ordinary(&db, receive_batch(&db, "event-102", "102"), 100).unwrap();
    sync_inbox::receive_ordinary(&db, receive_batch(&db, "event-103", "103"), 100).unwrap();
    let claims = sync_inbox::claim_inputs(&db, &binding(&db), 101, 2).unwrap();
    let first = claims
        .iter()
        .find(|claim| claim.event_id == "event-102")
        .unwrap();
    let second = claims
        .iter()
        .find(|claim| claim.event_id == "event-103")
        .unwrap();
    let conflicting = super::messages::MessageRecord {
        receiver_did: "did:example:new".into(),
        content: "different body".into(),
        server_seq: Some(102),
        ..echo.clone()
    }
    .with_resolved_wire_thread("direct", "did:example:new");
    let failure =
        sync_inbox::apply_ordinary_claim(&db, first, apply_input(first, conflicting), 101)
            .unwrap_err();
    assert!(matches!(
        failure,
        crate::ImError::MessageWireIdentityConflict { .. }
    ));
    sync_inbox::fail_claim(&db, first, "message_wire_identity_conflict", None, 101).unwrap();
    let following = super::messages::MessageRecord {
        msg_id: "message-103".into(),
        receiver_did: "did:example:new".into(),
        content: "next message".into(),
        server_seq: Some(103),
        metadata: json!({"operation_id":"operation-103"}).to_string(),
        ..echo
    }
    .with_resolved_wire_thread("direct", "did:example:new");
    let result =
        sync_inbox::apply_ordinary_claim(&db, second, apply_input(second, following), 102).unwrap();
    assert_eq!(result.applied_event_ids, ["event-103"]);
    assert_eq!(cursor(&db), "103");
    let read = sync_v2::mark_thread_read_and_update_outbox(
        &db,
        "owner",
        "did:example:owner",
        super::messages::MarkThreadReadWatermarkInput {
            thread: crate::messages::ThreadRef::Thread(
                crate::ids::ThreadId::parse(&conversation).unwrap(),
            ),
            read_watermark_message_id: Some("message-103".into()),
            read_watermark_seq: Some("103".into()),
            read_watermark_at: None,
            pending_remote_ack: true,
        },
    )
    .unwrap();
    assert_eq!(read.read_watermark_seq.as_deref(), Some("103"));
    assert!(read.outbox_operation_id.is_some());
    let read_send = sync_v2::claim_next_read_mutation(&db, "owner", chrono::Utc::now().timestamp())
        .unwrap()
        .unwrap();
    assert_eq!(read_send.operation_id, read.outbox_operation_id.unwrap());
    let payload: serde_json::Value = serde_json::from_str(&read_send.payload_json).unwrap();
    assert_eq!(payload["read_watermark_seq"], json!("103"));
    assert_eq!(
        db.query_row(
            "SELECT read_watermark_seq FROM thread_read_state",
            [],
            |row| row.get::<_, String>(0)
        )
        .unwrap(),
        "103"
    );
    assert_eq!(
        db.query_row(
            "SELECT processing_error_code FROM sync_lane_inbox WHERE event_id='event-102'",
            [],
            |row| row.get::<_, String>(0)
        )
        .unwrap(),
        "message_wire_identity_conflict"
    );

    assert_eq!(
        db.query_row(
            "SELECT content FROM messages WHERE msg_id='message-102'",
            [],
            |row| row.get::<_, String>(0)
        )
        .unwrap(),
        "original"
    );
    assert_eq!(
        db.query_row(
            "SELECT content FROM messages WHERE msg_id='message-103'",
            [],
            |row| row.get::<_, String>(0)
        )
        .unwrap(),
        "next message"
    );
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM sync_applied_events", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM sync_lane_inbox", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        sync_inbox::receive_ordinary(&db, receive_batch(&db, "event-103", "103"), 103)
            .unwrap()
            .duplicates,
        1
    );
    let mut changed = receive_batch(&db, "event-103", "103");
    changed.events[0].payload["hydrated"]["content"] = json!("changed after cleanup");
    assert!(sync_inbox::receive_ordinary(&db, changed, 104).is_err());
}

#[test]
fn claims_include_other_lanes_and_do_not_queue_behind_processing_inputs() {
    let db = database();
    for index in 1..=10 {
        insert_raw(
            &db,
            &format!("ordinary-{index}"),
            "ordinary",
            "message.created",
            100 + index,
        );
    }
    insert_raw(&db, "p5", "p5_device", "p5.delivery.created", 120);
    db.execute("INSERT INTO sync_lane_inbox(input_id,owner_identity_id,lane,lane_epoch,position,event_id,event_type,raw_payload_json,payload_bytes,account_id_snapshot,device_id_snapshot,auth_generation_snapshot,client_instance_id_snapshot,group_did,received_at,created_at) VALUES('p6','owner','p6_group','1','1','p6','p6.control.notice','{}',2,'account','device','1','installation','did:example:group','2026-09-10T00:00:00Z',121)", []).unwrap();
    let claims = sync_inbox::claim_inputs(&db, &binding(&db), 130, 3).unwrap();
    assert_eq!(
        claims
            .iter()
            .map(|claim| claim.lane.as_str())
            .collect::<Vec<_>>(),
        ["ordinary", "p5_device", "p6_group"]
    );
    let next = sync_inbox::claim_inputs(&db, &binding(&db), 130, 1).unwrap();
    assert_eq!(next.len(), 1);
    assert_ne!(next[0].input_id, claims[0].input_id);
}

#[test]
fn baseline_fences_an_old_attempt_without_discarding_it_across_epoch_change() {
    let db = database();
    sync_inbox::receive_ordinary(&db, receive_batch(&db, "old-event", "10"), 100).unwrap();
    let old = sync_inbox::claim_inputs(&db, &binding(&db), 101, 1)
        .unwrap()
        .remove(0);
    insert_raw(&db, "baseline", "baseline", "sync.baseline", 102);
    db.execute("UPDATE message_sync_state SET stream_epoch='2',scan_seq='20' WHERE owner_identity_id='owner'", []).unwrap();
    let mut input = apply_input(&old, super::messages::MessageRecord::default());
    input.events[0].messages.clear();
    input.events[0].thread_bindings.clear();
    let failure = sync_inbox::apply_ordinary_claim(&db, &old, input.clone(), 103).unwrap_err();
    assert!(
        matches!(failure, crate::ImError::Service { code: Some(ref code), .. } if code == "sync.baseline_pending")
    );
    sync_inbox::fail_claim(&db, &old, "sync.baseline_pending", Some(104), 103).unwrap();
    let claims = sync_inbox::claim_inputs(&db, &binding(&db), 104, 8).unwrap();
    assert_eq!(claims.len(), 1);
    assert_eq!(claims[0].lane, "baseline");
    let baseline = super::sync_baseline::Baseline {
        server_time: "2026-09-10T00:00:00Z".into(),
        server_cutoff: None,
        groups: vec![],
        read_states: vec![],
        message_ids: Default::default(),
        notification_ids: Default::default(),
    };
    super::sync_baseline::apply_claim(&db, &claims[0], baseline, vec![], vec![], 104).unwrap();
    let next = sync_inbox::claim_inputs(&db, &binding(&db), 105, 8).unwrap();
    assert_eq!(next.len(), 1);
    assert_eq!(next[0].event_id, "old-event");
    assert_eq!(next[0].lane_epoch, "1");
    assert_eq!(next[0].attempt_count, 2);
    assert!(sync_inbox::apply_ordinary_claim(&db, &old, input.clone(), 105).is_err());
    sync_inbox::apply_ordinary_claim(&db, &next[0], input, 105).unwrap();
    assert_eq!(cursor(&db), "20");
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM sync_lane_inbox", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[test]
fn eviction_does_not_release_physical_attempt_capacity_for_another_connection() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("inbox.sqlite");
    let db = initialized_database(Connection::open(&path).unwrap());
    for index in 0..8 {
        insert_raw(
            &db,
            &format!("active-{index}"),
            "p5_device",
            "p5.delivery.created",
            100,
        );
    }
    let claims = sync_inbox::claim_inputs(&db, &binding(&db), 100, 8).unwrap();
    assert_eq!(claims.len(), 8);
    let removed = sync_inbox::make_room_with_limit(&db, 1, 101, 8).unwrap();
    assert_eq!(removed.len(), 1);
    insert_raw(&db, "new-message", "ordinary", "message.created", 101);
    let other = Connection::open(&path).unwrap();
    assert!(sync_inbox::claim_inputs(&other, &binding(&other), 101, 8)
        .unwrap()
        .is_empty());
    sync_inbox::renew_claims(&db, &claims, 155).unwrap();
    assert!(sync_inbox::claim_inputs(&other, &binding(&other), 161, 8)
        .unwrap()
        .is_empty());
    let evicted = claims
        .iter()
        .find(|claim| claim.input_id == removed[0].input_id)
        .unwrap();
    assert!(sync_inbox::require_claim(&other, evicted, 161).is_err());
    sync_inbox::release_claim_lease(&db, evicted).unwrap();
    let next = sync_inbox::claim_inputs(&other, &binding(&other), 161, 8).unwrap();
    assert_eq!(next.len(), 1);
    assert_eq!(next[0].event_id, "new-message");
    assert_eq!(
        other
            .query_row("SELECT COUNT(*) FROM sync_input_leases", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        8
    );
    assert_eq!(
        other
            .query_row("SELECT COUNT(*) FROM sync_lane_applied_events", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
        0
    );
}
