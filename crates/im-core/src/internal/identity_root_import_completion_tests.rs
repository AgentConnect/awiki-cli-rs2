use super::*;

fn fixture() -> (rusqlite::Connection, CompletionRecord, CompletionSuccess) {
    let connection = rusqlite::Connection::open_in_memory().unwrap();
    crate::internal::local_state::schema::ensure_schema(&connection).unwrap();
    let record = CompletionRecord {
        did: "did:example:owner".to_owned(),
        local_device_id: "device-b".to_owned(),
        message_id: "transfer-a".to_owned(),
        sender_device_id: "device-a".to_owned(),
        recipient_device_id: "device-b".to_owned(),
        sender_e2ee_key_id: "sender-e2ee".to_owned(),
        recipient_e2ee_key_id: "recipient-e2ee".to_owned(),
        imported_at: "2026-09-09T00:00:00Z".to_owned(),
        expires_at: "2026-09-09T00:10:00Z".to_owned(),
        pending_root_ref: RootImportCustodyRef {
            store_id: "store-a".to_owned(),
            identity_id: "identity-a".to_owned(),
            did: "did:example:owner".to_owned(),
        },
        root_key_id: "root-key".to_owned(),
        root_fingerprint: "root-fingerprint".to_owned(),
        document_version: 4,
        document_hash: "document-hash".to_owned(),
        registry_version: 7,
        phase: RootImportCompletionPhase::CompletionPending,
        completion_params_json: Some("{}".to_owned()),
        completion_request_hash: Some("request-hash".to_owned()),
        completion_result_json: None,
    };
    let success = CompletionSuccess {
        did: record.did.clone(),
        device_id: record.local_device_id.clone(),
        role: "admin".to_owned(),
        management_ready: true,
        auth_generation: 2,
        registry_version: 8,
        completed_message_id: record.message_id.clone(),
    };
    connection
        .execute(
            "INSERT INTO identity_root_import_completion_v1 (
          owner_identity_id, owner_did, local_device_id, message_id,
          sender_device_id, recipient_device_id, sender_e2ee_key_id,
          recipient_e2ee_key_id, accepted_at, imported_at, envelope_expires_at,
          pending_root_ref_json, root_key_id, root_fingerprint, document_version,
          document_hash, registry_version, phase, completion_params_json,
          completion_request_hash, created_at, updated_at
        ) VALUES ('owner-a', ?1, ?2, ?3, ?4, ?2, ?5, ?6, ?7, ?7, ?8,
          ?9, ?10, ?11, 4, ?12, 7, 'completion_pending', '{}', 'request-hash', ?7, ?7)",
            rusqlite::params![
                record.did,
                record.local_device_id,
                record.message_id,
                record.sender_device_id,
                record.sender_e2ee_key_id,
                record.recipient_e2ee_key_id,
                record.imported_at,
                record.expires_at,
                serde_json::to_string(&record.pending_root_ref).unwrap(),
                record.root_key_id,
                record.root_fingerprint,
                record.document_hash
            ],
        )
        .unwrap();
    (connection, record, success)
}

fn phase(connection: &rusqlite::Connection) -> String {
    connection
        .query_row(
            "SELECT phase FROM identity_root_import_completion_v1",
            [],
            |row| row.get(0),
        )
        .unwrap()
}

#[test]
fn completion_response_replay_preserves_later_phase() {
    let (connection, record, success) = fixture();
    store_completion_success(&connection, "owner-a", &record, "request-hash", &success).unwrap();
    // Another completion task finished while this caller retained its old record.
    connection
        .execute(
            "UPDATE identity_root_import_completion_v1 SET phase='promoted'",
            [],
        )
        .unwrap();
    store_completion_success(&connection, "owner-a", &record, "request-hash", &success).unwrap();
    assert_eq!(phase(&connection), "promoted");
}

#[test]
fn completion_phase_replay_preserves_later_phase() {
    let (connection, record, success) = fixture();
    store_completion_success(&connection, "owner-a", &record, "request-hash", &success).unwrap();
    connection
        .execute(
            "UPDATE identity_root_import_completion_v1 SET phase='registry_confirmed'",
            [],
        )
        .unwrap();
    advance_completion_phase(
        &connection,
        "owner-a",
        &record,
        &success,
        RootImportCompletionPhase::CompletionAccepted,
        RootImportCompletionPhase::TokenRefreshed,
    )
    .unwrap();
    assert_eq!(phase(&connection), "registry_confirmed");
}

#[test]
fn completion_forward_steps_and_delayed_replays_are_monotonic() {
    let (connection, record, success) = fixture();
    store_completion_success(&connection, "owner-a", &record, "request-hash", &success).unwrap();
    for (from, to) in [
        (
            RootImportCompletionPhase::CompletionAccepted,
            RootImportCompletionPhase::TokenRefreshed,
        ),
        (
            RootImportCompletionPhase::TokenRefreshed,
            RootImportCompletionPhase::RegistryConfirmed,
        ),
    ] {
        advance_completion_phase(&connection, "owner-a", &record, &success, from, to).unwrap();
        assert_eq!(phase(&connection), to.as_str());
        store_completion_success(&connection, "owner-a", &record, "request-hash", &success)
            .unwrap();
        advance_completion_phase(&connection, "owner-a", &record, &success, from, to).unwrap();
        assert_eq!(phase(&connection), to.as_str());
    }
    connection
        .execute(
            "UPDATE identity_root_import_completion_v1 SET phase='promoted'",
            [],
        )
        .unwrap();
    advance_completion_phase(
        &connection,
        "owner-a",
        &record,
        &success,
        RootImportCompletionPhase::TokenRefreshed,
        RootImportCompletionPhase::RegistryConfirmed,
    )
    .unwrap();
    assert!(advance_completion_phase(
        &connection,
        "owner-a",
        &record,
        &success,
        RootImportCompletionPhase::Promoted,
        RootImportCompletionPhase::TokenRefreshed
    )
    .is_err());
    assert_eq!(phase(&connection), "promoted");
}

#[test]
fn completion_replays_reject_conflicting_request_result_custody_and_owner() {
    for mismatch in [
        "request", "result", "custody", "owner", "device", "terminal",
    ] {
        let (connection, mut record, mut success) = fixture();
        store_completion_success(&connection, "owner-a", &record, "request-hash", &success)
            .unwrap();
        connection
            .execute(
                "UPDATE identity_root_import_completion_v1 SET phase='promoted'",
                [],
            )
            .unwrap();
        let mut request = "request-hash";
        let mut owner = "owner-a";
        match mismatch {
            "request" => {
                request = "different-request";
                record.completion_request_hash = Some(request.to_owned());
            }
            "result" => success.auth_generation += 1,
            "custody" => record.pending_root_ref.identity_id = "different-identity".to_owned(),
            "owner" => owner = "owner-b",
            "device" => record.local_device_id = "device-c".to_owned(),
            "terminal" => {
                connection
                    .execute(
                        "UPDATE identity_root_import_completion_v1 SET phase='terminal_failed'",
                        [],
                    )
                    .unwrap();
            }
            _ => unreachable!(),
        }
        let before = phase(&connection);
        assert!(
            store_completion_success(&connection, owner, &record, request, &success).is_err(),
            "{mismatch}"
        );
        assert!(
            advance_completion_phase(
                &connection,
                owner,
                &record,
                &success,
                RootImportCompletionPhase::CompletionAccepted,
                RootImportCompletionPhase::TokenRefreshed
            )
            .is_err(),
            "{mismatch}"
        );
        assert_eq!(phase(&connection), before);
    }
}

#[test]
fn completion_replay_does_not_skip_required_predecessor_or_accept_missing_result() {
    let (connection, record, success) = fixture();
    assert!(advance_completion_phase(
        &connection,
        "owner-a",
        &record,
        &success,
        RootImportCompletionPhase::TokenRefreshed,
        RootImportCompletionPhase::RegistryConfirmed
    )
    .is_err());
    assert_eq!(phase(&connection), "completion_pending");
    store_completion_success(&connection, "owner-a", &record, "request-hash", &success).unwrap();
    assert!(advance_completion_phase(
        &connection,
        "owner-a",
        &record,
        &success,
        RootImportCompletionPhase::TokenRefreshed,
        RootImportCompletionPhase::RegistryConfirmed
    )
    .is_err());
    connection.execute("UPDATE identity_root_import_completion_v1 SET phase='promoted', completion_result_json=NULL", []).unwrap();
    assert!(
        store_completion_success(&connection, "owner-a", &record, "request-hash", &success)
            .is_err()
    );
    assert!(advance_completion_phase(
        &connection,
        "owner-a",
        &record,
        &success,
        RootImportCompletionPhase::CompletionAccepted,
        RootImportCompletionPhase::TokenRefreshed
    )
    .is_err());
}

#[tokio::test]
async fn completion_lock_serializes_same_transfer_and_isolates_other_scopes() {
    let first = root_import_lock("root-a", "owner-a", "device-a", "transfer-a");
    let same = root_import_lock("root-a", "owner-a", "device-a", "transfer-a");
    assert!(std::sync::Arc::ptr_eq(&first, &same));
    let held = first.lock_owned().await;
    for scope in [
        ("root-b", "owner-a", "device-a", "transfer-a"),
        ("root-a", "owner-b", "device-a", "transfer-a"),
        ("root-a", "owner-a", "device-b", "transfer-a"),
        ("root-a", "owner-a", "device-a", "transfer-b"),
    ] {
        assert!(root_import_lock(scope.0, scope.1, scope.2, scope.3)
            .try_lock_owned()
            .is_ok());
    }
    let (started, entered) = tokio::sync::oneshot::channel();
    let waiting = tokio::spawn(async move {
        started.send(()).unwrap();
        let _held = same.lock_owned().await;
    });
    entered.await.unwrap();
    tokio::task::yield_now().await;
    assert!(!waiting.is_finished());
    drop(held);
    waiting.await.unwrap();
}

#[test]
fn queued_root_handoff_and_receipt_commit_together_and_reject_evicted_attempts() {
    use crate::internal::local_state::{sync_inbox, sync_v2};
    let (mut db, record, _) = fixture();
    db.execute("DELETE FROM identity_root_import_completion_v1", [])
        .unwrap();
    let now = chrono::Utc::now().timestamp();
    let binding = sync_v2::IdentityAccountBinding {
        owner_identity_id: "owner-a".into(),
        account_id: "account-a".into(),
        handle_scope: None,
        current_did: record.did.clone(),
        protocol_device_id: record.local_device_id.clone(),
        identity_generation: "1".into(),
        device_auth_generation: "1".into(),
        created_at: now,
        updated_at: now,
    };
    sync_v2::upsert_identity_account_binding(&db, &binding).unwrap();
    let event = sync_inbox::InboxEvent {
        event_id: "root-delivery".into(),
        position: "1".into(),
        event_type: "p5.delivery.created".into(),
        payload: serde_json::json!({"ciphertext":"test-only-opaque-input"}),
        processing_scope: record.did.clone(),
        group_did: None,
    };
    sync_inbox::insert_input(&db, &binding, "installation", "p5_device", "1", &event, now).unwrap();
    let claim = sync_inbox::claim_inputs(&db, &binding, now, 1)
        .unwrap()
        .remove(0);
    let plan = RootImportSealedPlan {
        owner_identity_id: binding.owner_identity_id.clone(),
        owner_did: record.did.clone(),
        local_device_id: record.local_device_id.clone(),
        message_id: record.message_id.clone(),
        sender_device_id: record.sender_device_id,
        recipient_device_id: record.recipient_device_id,
        sender_e2ee_key_id: record.sender_e2ee_key_id,
        recipient_e2ee_key_id: record.recipient_e2ee_key_id,
        accepted_at: record.imported_at.clone(),
        imported_at: record.imported_at.clone(),
        envelope_expires_at: record.expires_at,
        pending_root_ref_json: serde_json::to_string(&record.pending_root_ref).unwrap(),
        root_key_id: record.root_key_id,
        root_fingerprint: record.root_fingerprint,
        document_version: record.document_version,
        document_hash: record.document_hash,
        registry_version: record.registry_version,
        now: record.imported_at,
    };
    db.execute_batch("CREATE TRIGGER reject_root_receipt BEFORE INSERT ON sync_lane_applied_events BEGIN SELECT RAISE(ABORT,'receipt write failed'); END;").unwrap();
    {
        let tx = db.transaction().unwrap();
        assert!(persist_received_root_handoff(&tx, Some(&plan), Some(&claim)).is_err());
        tx.rollback().unwrap();
    }
    assert_eq!(
        db.query_row(
            "SELECT COUNT(*) FROM identity_root_import_completion_v1",
            [],
            |row| row.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    sync_inbox::require_claim(&db, &claim, now).unwrap();
    db.execute_batch("DROP TRIGGER reject_root_receipt;")
        .unwrap();
    db.execute(
        "DELETE FROM sync_lane_inbox WHERE input_id=?1",
        [&claim.input_id],
    )
    .unwrap();
    sync_inbox::insert_input(&db, &binding, "installation", "p5_device", "1", &event, now).unwrap();
    let replacement = sync_inbox::claim_inputs(&db, &binding, now, 1)
        .unwrap()
        .remove(0);
    {
        let tx = db.transaction().unwrap();
        assert!(persist_received_root_handoff(&tx, Some(&plan), Some(&claim)).is_err());
        tx.rollback().unwrap();
    }
    let tx = db.transaction().unwrap();
    persist_received_root_handoff(&tx, Some(&plan), Some(&replacement)).unwrap();
    tx.commit().unwrap();
    assert!(sync_inbox::claim_was_completed(&db, &replacement).unwrap());
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM sync_lane_inbox", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        db.query_row(
            "SELECT phase FROM identity_root_import_completion_v1",
            [],
            |row| row.get::<_, String>(0)
        )
        .unwrap(),
        "import_sealed"
    );
}
