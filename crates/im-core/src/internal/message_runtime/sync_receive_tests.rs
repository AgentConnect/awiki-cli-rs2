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

#[tokio::test]
async fn receive_snapshot_and_post_anchor_page_without_business_then_resume_after_reopen() {
    let fixture = SyncSnapshotFixture::new("receive-snapshot-stopped");
    let client = fixture.client();
    let binding = client.active_sync_account_binding().await.unwrap();
    seed_sync_snapshot_ready_state(&client, &binding, "1", "10").await;
    let db = client.core_inner().local_state_db().await.unwrap();
    let installation = db.load_or_create_sync_client_instance_id(&binding.owner_identity_id).await.unwrap();
    db.reconcile_sync_lane_capability_v1a(&binding.owner_identity_id, vec![], &binding.device_auth_generation,
        installation, "[]".to_owned()).await.unwrap();
    drop(db);
    let group = "did:example:receive-snapshot-group";
    let event = sync_snapshot_message_event(
        &binding,
        "snapshot-19",
        "2",
        "19",
        "snapshot-message",
        group,
    );
    let message = sync_snapshot_message(
        &binding,
        "snapshot-message",
        group,
        "19",
        "snapshot durable body",
    );
    let live = sync_snapshot_message_event(&binding, "live-21", "2", "21", "live-message", group);
    let live_message =
        sync_snapshot_message(&binding, "live-message", group, "21", "live durable body");
    let (first, last) = sync_snapshot_two_page_responses(
        &client,
        &binding,
        "receive-snapshot",
        "2",
        "20",
        vec![json!({"event": event, "message": message}), json!({
            "event": sync_snapshot_message_event(&binding, "snapshot-20", "2", "20", "snapshot-message-20", group),
            "message": sync_snapshot_message(&binding, "snapshot-message-20", group, "20", "second snapshot durable body")
        })],
        true,
    )
    .await;
    let transport = SyncSnapshotTransport::queued(
        Rc::new(RefCell::new(Vec::new())),
        vec![
            Ok(sync_snapshot_recovery(
                "receive-snapshot",
                "snapshot-token",
                "2",
                "20",
            )),
            Ok(first),
            Ok(last),
            Ok(sync_snapshot_delta("2", "21", vec![live])),
            Ok(json!({"items":[{"event_id":"live-21","message":live_message}],"unavailable":[]})),
        ],
    );
    let received = tokio::time::timeout(
        StdDuration::from_secs(2),
        MessageSyncRuntimeV2::new(
            &client,
            ReadySyncSnapshotSessionProvider,
            transport,
            NeverProcessingDirectory,
        )
        .receive_now(sync_snapshot_request()),
    )
    .await
    .expect("snapshot reception cannot wait on business")
    .unwrap();
    assert!(received.complete, "{received:?}");
    assert_eq!(received.events_received, 3);
    assert_eq!(
        sqlite_count(
            &fixture.sqlite_path(),
            "SELECT COUNT(*) FROM sync_lane_inbox"
        ),
        4
    );
    assert_eq!(
        sqlite_count(
            &fixture.sqlite_path(),
            "SELECT COUNT(*) FROM sync_applied_events"
        ),
        0
    );
    assert!(!fixture.has_message_content("snapshot durable body"));
    let state = load_sync_snapshot_state(&client, &binding.owner_identity_id).await;
    assert_eq!(
        (state.stream_epoch.as_str(), state.scan_seq.as_str()),
        ("2", "21")
    );
    drop(client);
    let client = fixture.client();
    let db = client.core_inner().local_state_db().await.unwrap();
    let stored = stored_binding(&client, &binding);
    let claims = db
        .run_local(move |connection| {
            crate::internal::local_state::sync_inbox::claim_inputs(
                connection,
                &stored,
                unix_time_i64(),
                8,
            )
        })
        .await
        .unwrap();
    assert_eq!(
        claims.len(),
        1,
        "only the actual baseline prerequisite is claimable"
    );
    assert_eq!(claims[0].lane, "baseline");
    crate::internal::message_runtime::sync_processing::process_ordinary(
        &client,
        &claims[0],
        &mut NeverProcessingDirectory,
    )
    .await
    .unwrap();
    let stored = stored_binding(&client, &binding);
    let claims = db
        .run_local(move |connection| {
            crate::internal::local_state::sync_inbox::claim_inputs(
                connection,
                &stored,
                unix_time_i64(),
                8,
            )
        })
        .await
        .unwrap();
    assert_eq!(
        claims.len(),
        3,
        "snapshot messages do not serialize later messages"
    );
    let mut incoming = Vec::new();
    for claim in claims {
        let processed = crate::internal::message_runtime::sync_processing::process_ordinary(
            &client,
            &claim,
            &mut NoopAsyncDirectoryTransport,
        )
        .await
        .unwrap();
        incoming.extend(processed.committed_incoming_messages);
    }
    assert!(fixture.has_message_content("snapshot durable body"));
    assert!(fixture.has_message_content("live durable body"));
    assert_eq!(incoming.len(), 1);
    assert_eq!(incoming[0].event_id, "live-21");
    assert_eq!(
        sqlite_count(
            &fixture.sqlite_path(),
            "SELECT COUNT(*) FROM sync_lane_inbox"
        ),
        0
    );
    let state = load_sync_snapshot_state(&client, &binding.owner_identity_id).await;
    assert_eq!(
        state.scan_seq, "21",
        "processing does not mutate the receive checkpoint"
    );
}
