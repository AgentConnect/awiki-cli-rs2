use super::*;

struct NeverProcessingDirectory;

impl AsyncRpcTransport for NeverProcessingDirectory {
    async fn rpc(
        &mut self,
        _endpoint: &str,
        _method: &str,
        _params: Value,
    ) -> crate::ImResult<Value> {
        std::future::pending().await
    }
}

#[tokio::test]
async fn receive_two_pages_completes_with_business_consumers_stopped() {
    let fixture = SyncSnapshotFixture::new("receive-business-stopped");
    let client = fixture.client();
    let binding = client.active_sync_account_binding().await.unwrap();
    seed_sync_snapshot_ready_state(&client, &binding, "1", "0").await;
    let db = client.core_inner().local_state_db().await.unwrap();
    let installation = db
        .load_or_create_sync_client_instance_id(&binding.owner_identity_id)
        .await
        .unwrap();
    db.reconcile_sync_lane_capability_v1a(
        &binding.owner_identity_id,
        vec![],
        &binding.device_auth_generation,
        installation,
        "[]".to_owned(),
    )
    .await
    .unwrap();
    let peer = "did:wba:example.test:peer:e1";
    let mut first = sync_snapshot_delta(
        "1",
        "102",
        vec![sync_direct_message_event(
            &binding,
            "event-102",
            "102",
            "message-102",
            peer,
            "thread-a",
        )],
    );
    first["has_more"] = json!(true);
    let second = sync_snapshot_delta(
        "1",
        "103",
        vec![sync_direct_message_event(
            &binding,
            "event-103",
            "103",
            "message-103",
            peer,
            "thread-a",
        )],
    );
    let calls = Rc::new(RefCell::new(Vec::new()));
    let transport = SyncSnapshotTransport::queued(
        Rc::clone(&calls),
        vec![
            Ok(first),
            Ok(
                json!({"items":[{"event_id":"event-102","message":sync_direct_message(&binding,"message-102",peer,"body-102")}],"unavailable":[]}),
            ),
            Ok(second),
            Ok(
                json!({"items":[{"event_id":"event-103","message":sync_direct_message(&binding,"message-103",peer,"body-103")}],"unavailable":[]}),
            ),
        ],
    );
    let outcome = tokio::time::timeout(
        StdDuration::from_secs(1),
        MessageSyncRuntimeV2::new(
            &client,
            ReadySyncSnapshotSessionProvider,
            transport,
            NeverProcessingDirectory,
        )
        .receive_now(sync_snapshot_request()),
    )
    .await
    .expect("business must not own reception")
    .unwrap();
    assert_eq!(outcome.status, crate::messages::MessageSyncStatus::Changed);
    assert!(outcome.complete);
    assert_eq!(outcome.events_received, 2);
    assert_eq!(outcome.pages_fetched, 2);
    assert_eq!(
        sqlite_count(
            &fixture.sqlite_path(),
            "SELECT COUNT(*) FROM sync_lane_inbox WHERE lane='ordinary'"
        ),
        2
    );
    assert_eq!(
        sqlite_count(
            &fixture.sqlite_path(),
            "SELECT COUNT(*) FROM sync_applied_events"
        ),
        0
    );
    assert!(!fixture.has_message_content("body-102"));
    assert!(!fixture.has_message_content("body-103"));
    let state = load_sync_snapshot_state(&client, &binding.owner_identity_id).await;
    assert_eq!(state.scan_seq, "103");
    assert_eq!(
        calls
            .borrow()
            .iter()
            .map(|call| call.method.as_str())
            .collect::<Vec<_>>(),
        [
            "sync.delta",
            "message.get_batch",
            "sync.delta",
            "message.get_batch"
        ]
    );
}

#[tokio::test]
async fn receive_later_hydration_failure_keeps_the_previous_committed_batch() {
    let fixture = SyncSnapshotFixture::new("receive-partial-failure");
    let client = fixture.client();
    let binding = client.active_sync_account_binding().await.unwrap();
    seed_sync_snapshot_ready_state(&client, &binding, "1", "0").await;
    let peer = "did:wba:example.test:peer:e1";
    let mut first = sync_snapshot_delta(
        "1",
        "102",
        vec![sync_direct_message_event(
            &binding,
            "event-102",
            "102",
            "message-102",
            peer,
            "thread-a",
        )],
    );
    first["has_more"] = json!(true);
    let transport = SyncSnapshotTransport::queued(
        Rc::new(RefCell::new(Vec::new())),
        vec![
            Ok(first),
            Ok(
                json!({"items":[{"event_id":"event-102","message":sync_direct_message(&binding,"message-102",peer,"body-102")}],"unavailable":[]}),
            ),
            Ok(sync_snapshot_delta(
                "1",
                "103",
                vec![sync_direct_message_event(
                    &binding,
                    "event-103",
                    "103",
                    "message-103",
                    peer,
                    "thread-a",
                )],
            )),
            Ok(json!({"items":[],"unavailable":["event-103"]})),
        ],
    );
    assert!(MessageSyncRuntimeV2::new(
        &client,
        ReadySyncSnapshotSessionProvider,
        transport,
        NeverProcessingDirectory
    )
    .receive_now(sync_snapshot_request())
    .await
    .is_err());
    let state = load_sync_snapshot_state(&client, &binding.owner_identity_id).await;
    assert_eq!(state.scan_seq, "102");
    assert_eq!(
        sqlite_count(
            &fixture.sqlite_path(),
            "SELECT COUNT(*) FROM sync_lane_inbox WHERE lane='ordinary'"
        ),
        1
    );
    assert_eq!(
        sqlite_count(
            &fixture.sqlite_path(),
            "SELECT COUNT(*) FROM sync_applied_events"
        ),
        0
    );
}
