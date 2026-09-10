use super::*;
use crate::internal::local_state::{messages, peer_personas, schema};
use serde_json::json;

const OWNER: &str = "owner";
const OWNER_DID: &str = "did:example:owner";
const OLD: &str = "did:example:peer-old";
const NEW: &str = "did:example:peer-new";

fn fixture(db: &mut Connection, legacy: bool) -> (MessageRecord, MessageRecord) {
    schema::ensure_schema(db).unwrap();
    let lookup = |did: &str, generation: &str| crate::directory::HandleLookupResult {
        handle: crate::ids::Handle::parse("peer.awiki.info", "").unwrap(),
        did: crate::ids::Did::parse(did).unwrap(),
        user_id: "peer".to_owned(),
        domain: Some("awiki.info".to_owned()),
        status: Some("active".to_owned()),
        binding_generation: Some(generation.to_owned()),
        profile: None,
        warnings: vec![],
    };
    let conversation =
        peer_personas::project_verified_handle(db, OWNER, OWNER_DID, &lookup(OLD, "1")).unwrap();
    let before = MessageRecord {
        msg_id: "message".to_owned(),
        owner_identity_id: OWNER.to_owned(),
        owner_did: OWNER_DID.to_owned(),
        conversation_id: conversation.clone(),
        thread_id: conversation,
        direction: 1,
        sender_did: OWNER_DID.to_owned(),
        receiver_did: OLD.to_owned(),
        content_type: "text/plain".to_owned(),
        content: "hi".to_owned(),
        metadata: json!({"operation_id":"operation", "delivery_state": if legacy {"accepted"} else {"pending"},
            "resolved_target_did":if legacy {NEW} else {OLD}, "peer_current_did":if legacy {NEW} else {OLD}}).to_string(),
        ..MessageRecord::default()
    }.with_resolved_wire_thread("direct", OLD);
    messages::upsert_message(db, &before).unwrap();
    peer_personas::project_verified_handle(db, OWNER, OWNER_DID, &lookup(NEW, "2")).unwrap();
    let after = MessageRecord {
        receiver_did: NEW.to_owned(),
        server_seq: legacy.then_some(121),
        metadata: json!({"operation_id":"operation", "delivery_state":"accepted",
            "resolved_target_did":NEW, "peer_current_did":NEW})
        .to_string(),
        ..before.clone()
    }
    .with_resolved_wire_thread("direct", NEW);
    (before, after)
}

fn target(db: &Connection) -> (String, String, Option<i64>) {
    db.query_row("SELECT receiver_did,wire_thread_ref,server_seq FROM messages WHERE owner_identity_id='owner' AND msg_id='message'", [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap()
}

#[test]
fn accepted_rebind_then_remote_echo_is_exactly_once() {
    let mut db = Connection::open_in_memory().unwrap();
    let (_, accepted) = fixture(&mut db, false);
    messages::upsert_message(&db, &accepted).unwrap();
    assert_eq!(target(&db), (NEW.into(), NEW.into(), None));
    let remote = MessageRecord {
        server_seq: Some(121),
        ..accepted
    };
    messages::upsert_message(&db, &remote).unwrap();
    messages::upsert_message(&db, &remote).unwrap();
    assert_eq!(target(&db), (NEW.into(), NEW.into(), Some(121)));
    assert_eq!(
        db.query_row("SELECT count(*) FROM messages", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
}

#[test]
fn remote_confirmation_can_arrive_before_the_send_response() {
    let mut db = Connection::open_in_memory().unwrap();
    let (_, accepted) = fixture(&mut db, false);
    let remote = MessageRecord {
        server_seq: Some(121),
        metadata: json!({"operation_id":"operation"}).to_string(),
        ..accepted.clone()
    };
    messages::upsert_message(&db, &remote).unwrap();
    messages::upsert_message(&db, &accepted).unwrap();
    assert_eq!(target(&db), (NEW.into(), NEW.into(), Some(121)));
}

#[test]
fn lost_send_response_can_be_confirmed_by_exact_remote_echo() {
    let mut db = Connection::open_in_memory().unwrap();
    let (_, accepted) = fixture(&mut db, false);
    db.execute(
        "UPDATE messages SET metadata=json_set(metadata,'$.delivery_state','failed')",
        [],
    )
    .unwrap();
    assert!(messages::upsert_message(&db, &accepted).is_err());
    let remote = MessageRecord {
        server_seq: Some(121),
        metadata: json!({"operation_id":"operation"}).to_string(),
        ..accepted
    };
    messages::upsert_message(&db, &remote).unwrap();
    assert_eq!(target(&db), (NEW.into(), NEW.into(), Some(121)));
}

#[test]
fn legacy_accepted_target_recovers_after_reopen_without_resending() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.sqlite");
    let mut db = Connection::open(&path).unwrap();
    let (_, remote) = fixture(&mut db, true);
    drop(db);
    let db = Connection::open(&path).unwrap();
    messages::upsert_message(&db, &remote).unwrap();
    drop(db);
    let db = Connection::open(&path).unwrap();
    messages::upsert_message(&db, &remote).unwrap();
    assert_eq!(target(&db), (NEW.into(), NEW.into(), Some(121)));
}

#[test]
fn unproven_or_historical_conflicts_remain_conflicts() {
    for case in [
        "sequence",
        "body",
        "operation",
        "sender",
        "owner_did",
        "conversation",
        "incoming",
        "encrypted",
        "group",
        "unhydrated",
        "wrong_target",
        "no_proof",
        "untrusted_proof",
        "cross_persona",
        "cross_owner_proof",
        "malformed_metadata",
        "accepted_elsewhere",
        "missing_operation",
        "no_remote_sequence",
    ] {
        let mut db = Connection::open_in_memory().unwrap();
        let (_, mut remote) = fixture(&mut db, true);
        match case {
            "sequence" => {
                db.execute("UPDATE messages SET server_seq=120", [])
                    .unwrap();
            }
            "body" => remote.content = "different".into(),
            "operation" => remote.metadata = json!({"operation_id":"other"}).to_string(),
            "sender" => remote.sender_did = "did:example:other".into(),
            "owner_did" => remote.owner_did = "did:example:other".into(),
            "conversation" => {
                db.execute("UPDATE messages SET conversation_id='dm:elsewhere'", [])
                    .unwrap();
            }
            "incoming" => remote.direction = 0,
            "encrypted" => remote.is_e2ee = true,
            "group" => {
                db.execute("UPDATE messages SET group_did='did:example:group'", [])
                    .unwrap();
            }
            "unhydrated" => remote.hydration_state = MessageHydrationState::Discovered,
            "wrong_target" => remote.wire_thread_ref = "did:example:other".into(),
            "no_proof" => {
                db.execute(
                    "DELETE FROM peer_identifiers WHERE identifier_value=?1",
                    [NEW],
                )
                .unwrap();
            }
            "untrusted_proof" => {
                db.execute("UPDATE peer_identifiers SET source='untrusted'", [])
                    .unwrap();
            }
            "cross_persona" => {
                db.execute(
                    "UPDATE direct_peer_routes SET peer_persona_id='other-persona'",
                    [],
                )
                .unwrap();
            }
            "cross_owner_proof" => {
                db.execute(
                    "UPDATE direct_peer_routes SET owner_identity_id='other-owner'",
                    [],
                )
                .unwrap();
            }
            "malformed_metadata" => {
                db.execute("UPDATE messages SET metadata='not-json'", [])
                    .unwrap();
            }
            "accepted_elsewhere" => {
                db.execute("UPDATE messages SET metadata=?1", [json!({"operation_id":"operation","delivery_state":"accepted","resolved_target_did":OLD,"peer_current_did":OLD}).to_string()]).unwrap();
            }
            "missing_operation" => remote.metadata = "{}".into(),
            "no_remote_sequence" => remote.server_seq = None,
            _ => unreachable!(),
        }
        assert!(
            matches!(
                messages::upsert_message(&db, &remote),
                Err(crate::ImError::MessageWireIdentityConflict { .. })
            ),
            "{case}"
        );
        assert_eq!(target(&db).0, OLD, "{case}");
    }
}

#[test]
fn confirmation_rolls_back_with_the_outer_sync_page() {
    let mut db = Connection::open_in_memory().unwrap();
    let (_, remote) = fixture(&mut db, true);
    {
        let tx = db.transaction().unwrap();
        messages::upsert_message(&tx, &remote).unwrap();
        assert_eq!(target(&tx).0, NEW);
        let conflict = MessageRecord {
            server_seq: Some(999),
            ..remote.clone()
        };
        assert!(messages::upsert_message(&tx, &conflict).is_err());
        // An aborted page must not leave a target repair committed on its own.
    }
    assert_eq!(target(&db), (OLD.into(), OLD.into(), None));
    messages::upsert_message(&db, &remote).unwrap();
    assert_eq!(target(&db).0, NEW);
}

#[test]
fn local_retry_cannot_downgrade_an_accepted_identity() {
    let mut db = Connection::open_in_memory().unwrap();
    let (_, accepted) = fixture(&mut db, false);
    messages::upsert_message(&db, &accepted).unwrap();
    for status in ["stored_locally", "pending", "failed"] {
        let retry = MessageRecord {
            metadata: json!({"operation_id":"operation","delivery_state":status}).to_string(),
            ..accepted.clone()
        };
        messages::upsert_message(&db, &retry).unwrap();
        let status: String = db
            .query_row(
                "SELECT json_extract(metadata,'$.delivery_state') FROM messages",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(status, "accepted");
    }
}

#[test]
fn irreconcilable_identity_is_blocked_not_a_network_retry() {
    let outcome = crate::internal::message_runtime::sync_v2::failure_outcome(
        &crate::ImError::MessageWireIdentityConflict {
            message_id: "private-message".into(),
        },
    )
    .unwrap();
    assert_eq!(outcome.status, crate::messages::MessageSyncStatus::Blocked);
    assert_eq!(
        outcome.error_code.as_deref(),
        Some("message_wire_identity_conflict")
    );
    assert!(!format!("{outcome:?}").contains("private-message"));
}

#[test]
fn legacy_repair_commits_with_delta_cursor_and_replays_idempotently() {
    use crate::internal::local_state::sync_v2::*;
    let mut db = Connection::open_in_memory().unwrap();
    let (_, remote) = fixture(&mut db, true);
    upsert_identity_account_binding(
        &db,
        &IdentityAccountBinding {
            owner_identity_id: OWNER.into(),
            account_id: "account".into(),
            handle_scope: None,
            current_did: OWNER_DID.into(),
            protocol_device_id: "device".into(),
            identity_generation: "1".into(),
            device_auth_generation: "1".into(),
            created_at: 1,
            updated_at: 1,
        },
    )
    .unwrap();
    bootstrap_message_sync_state(
        &db,
        &MessageSyncState {
            owner_identity_id: OWNER.into(),
            account_id: "account".into(),
            protocol_device_id: "device".into(),
            device_auth_generation: "1".into(),
            stream_epoch: "1".into(),
            scan_seq: "90".into(),
            bootstrap_state: "active".into(),
            last_server_time: None,
            last_success_at: None,
            last_error_code: None,
            metadata_json: None,
            updated_at: 1,
        },
    )
    .unwrap();
    let mut input = DeltaApplyInputV2 {
        owner_identity_id: OWNER.into(),
        expected_run_generation: None,
        owner_did: OWNER_DID.into(),
        account_id: "account".into(),
        protocol_device_id: "device".into(),
        device_auth_generation: "1".into(),
        stream_epoch: "1".into(),
        next_scan_seq: "92".into(),
        server_time: "2026-09-10T07:10:00Z".into(),
        events: vec![DeltaApplyEventV2 {
            event_id: "event-91".into(),
            event_seq: "91".into(),
            event_type: "message.created".into(),
            messages: vec![remote.clone()],
            thread_bindings: vec![SyncThreadBinding {
                owner_identity_id: OWNER.into(),
                remote_thread_key: "remote-thread".into(),
                thread_kind: "direct".into(),
                conversation_id: remote.conversation_id.clone(),
                updated_at: 1,
            }],
            ..Default::default()
        }],
    };
    // A following conflict must roll back the earlier target confirmation AND
    // all receipts/checkpoint, not silently skip a poison event.
    let mut conflict = input.events[0].clone();
    conflict.event_id = "event-92".into();
    conflict.event_seq = "92".into();
    conflict.messages[0].server_seq = Some(999);
    input.events.push(conflict);
    assert!(apply_delta_v2(&db, input.clone()).is_err());
    assert_eq!(target(&db).0, OLD);
    assert_eq!(
        db.query_row("SELECT scan_seq FROM message_sync_state", [], |r| r
            .get::<_, String>(0))
            .unwrap(),
        "90"
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM sync_applied_events", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    input.events[1].messages[0].msg_id = "following-message".into();
    input.events[1].messages[0].server_seq = Some(122);
    let outcome = apply_delta_v2(&db, input.clone()).unwrap();
    assert_eq!(outcome.applied_event_ids.len(), 2);
    assert_eq!(target(&db), (NEW.into(), NEW.into(), Some(121)));
    assert_eq!(
        db.query_row("SELECT scan_seq FROM message_sync_state", [], |r| r
            .get::<_, String>(0))
            .unwrap(),
        "92"
    );
    apply_delta_v2(&db, input).unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM messages", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        2
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM sync_applied_events", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        2
    );
}
