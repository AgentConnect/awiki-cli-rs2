use super::*;
use serde_json::json;

fn database() -> (Connection, sync_v2::IdentityAccountBinding, String) {
    let db = Connection::open_in_memory().unwrap();
    super::super::schema::ensure_schema(&db).unwrap();
    db.execute("INSERT INTO identity_account_bindings(owner_identity_id,account_id,current_did,device_id,identity_generation,device_auth_generation,created_at,updated_at) VALUES('owner','account','did:example:owner','device','1','1',1,1)", []).unwrap();
    let binding = sync_v2::load_identity_account_binding(&db, "owner")
        .unwrap()
        .unwrap();
    let installation = sync_v2::load_or_create_sync_client_instance_id(&db, "owner").unwrap();
    (db, binding, installation)
}

fn baseline(time: &str) -> Baseline {
    Baseline {
        server_time: time.into(),
        server_cutoff: None,
        groups: vec![],
        read_states: vec![],
        message_ids: BTreeSet::new(),
        notification_ids: BTreeSet::new(),
    }
}

#[test]
fn repeated_bootstrap_at_same_anchor_keeps_each_baseline_in_receive_order() {
    let (db, binding, installation) = database();
    let first = baseline("2026-09-11T00:00:00Z");
    insert_baseline_and_events(
        &db,
        &binding,
        &installation,
        "1",
        "0",
        first.clone(),
        vec![],
        100,
    )
    .unwrap();
    let mut next = baseline("2026-09-11T00:00:01Z");
    next.read_states
        .push(json!({"thread_key":"remote-a","state_version":"2"}));
    insert_baseline_and_events(
        &db,
        &binding,
        &installation,
        "1",
        "0",
        next.clone(),
        vec![],
        101,
    )
    .expect("a fresh bootstrap is a new local baseline observation, even at the same anchor");
    let claims = sync_inbox::claim_inputs(&db, &binding, 102, 8).unwrap();
    assert_eq!(claims.len(), 1);
    assert_eq!(claims[0].payload["server_time"], first.server_time);
    let committed = apply_claim(&db, &claims[0], first, vec![], vec![], 102).unwrap();
    assert_eq!(committed.checkpoint_event_seq, "0");
    let claims = sync_inbox::claim_inputs(&db, &binding, 103, 8).unwrap();
    assert_eq!(claims.len(), 1);
    assert_eq!(claims[0].payload["read_states"], json!(next.read_states));
    apply_claim(&db, &claims[0], next, vec![], vec![], 103).unwrap();
    assert!(sync_inbox::claim_inputs(&db, &binding, 104, 8)
        .unwrap()
        .is_empty());
}

#[test]
fn snapshot_baseline_can_follow_completed_bootstrap_at_same_anchor() {
    let (db, binding, installation) = database();
    let bootstrap = baseline("2026-09-11T00:00:00Z");
    insert_baseline_and_events(
        &db,
        &binding,
        &installation,
        "1",
        "12",
        bootstrap.clone(),
        vec![],
        100,
    )
    .unwrap();
    let claim = sync_inbox::claim_inputs(&db, &binding, 101, 1)
        .unwrap()
        .remove(0);
    apply_claim(&db, &claim, bootstrap, vec![], vec![], 101).unwrap();
    let mut snapshot = baseline("2026-09-11T00:00:02Z");
    snapshot.server_cutoff = Some("2026-09-10T00:00:00Z".into());
    insert_baseline_and_events(
        &db,
        &binding,
        &installation,
        "1",
        "12",
        snapshot.clone(),
        vec![],
        102,
    )
    .expect("snapshot replacement cannot collide with a prior bootstrap receipt");
    let claim = sync_inbox::claim_inputs(&db, &binding, 103, 1)
        .unwrap()
        .remove(0);
    assert_eq!(
        claim.payload["server_cutoff"],
        snapshot.server_cutoff.unwrap()
    );
}

#[test]
fn committed_baseline_read_state_covers_messages_projected_after_its_invalidation() {
    let (db, binding, installation) = database();
    let conversation_id = "dm:peer-scope:v1:peer";
    sync_v2::upsert_sync_thread_binding(
        &db,
        &sync_v2::SyncThreadBinding {
            owner_identity_id: "owner".into(),
            remote_thread_key: "remote-thread".into(),
            thread_kind: "direct".into(),
            conversation_id: conversation_id.into(),
            updated_at: 1,
        },
    )
    .unwrap();
    let baseline = baseline("2026-09-11T00:00:00Z");
    insert_baseline_and_events(
        &db,
        &binding,
        &installation,
        "1",
        "50",
        baseline.clone(),
        vec![],
        100,
    )
    .unwrap();
    let claim = sync_inbox::claim_inputs(&db, &binding, 101, 1)
        .unwrap()
        .remove(0);
    let invalidation = apply_claim(
        &db,
        &claim,
        baseline,
        vec![],
        vec![sync_v2::ReadStateApplyV2 {
            remote_thread_key: "remote-thread".into(),
            thread_kind: "direct".into(),
            read_watermark_seq: "10".into(),
            read_watermark_message_id: None,
            state_version: "38".into(),
            occurred_at: "2026-09-11T00:00:00Z".into(),
        }],
        102,
    )
    .unwrap();
    assert!(invalidation
        .conversation_ids
        .contains(&conversation_id.to_owned()));
    assert_eq!(invalidation.checkpoint_event_seq, "50");
    let state =
        super::super::read_state::get_thread_read_state(&db, "owner", "direct", conversation_id)
            .unwrap()
            .unwrap();
    assert_eq!(state.read_watermark_seq.as_deref(), Some("10"));
    assert_eq!(state.remote_state_version.as_deref(), Some("38"));
    assert!(!state.pending_remote_ack);
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM sync_remote_read_states", [], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap(),
        0
    );
    super::super::messages::upsert_message(
        &db,
        &super::super::messages::MessageRecord {
            msg_id: "late-recovered".into(),
            owner_identity_id: "owner".into(),
            owner_did: binding.current_did,
            conversation_id: conversation_id.into(),
            thread_id: conversation_id.into(),
            wire_thread_kind: "direct".into(),
            wire_thread_ref: "did:example:peer".into(),
            wire_identity_resolution_state: "resolved".into(),
            direction: 0,
            sender_did: "did:example:peer".into(),
            receiver_did: "did:example:owner".into(),
            content_type: "text/plain".into(),
            content: "recovered".into(),
            server_seq: Some(10),
            hydration_state: super::super::messages::MessageHydrationState::Hydrated,
            stored_at: "2026-09-11T00:00:00Z".into(),
            ..super::super::messages::MessageRecord::default()
        },
    )
    .unwrap();
    assert!(db
        .query_row("SELECT is_read FROM messages", [], |row| row
            .get::<_, bool>(0))
        .unwrap());
    assert_eq!(
        db.query_row(
            "SELECT unread_count FROM conversation_summaries",
            [],
            |row| row.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    assert_eq!(
        super::super::read_state::get_thread_read_state(&db, "owner", "direct", conversation_id)
            .unwrap()
            .unwrap(),
        state
    );
}
