use super::*;

fn community_capabilities() -> Value {
    let value: Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/testdata/message-sync/community-sync-v1.json"
    )))
    .unwrap();
    let mut result = value["cases"]["C01"]["response"]["result"].clone();
    result["service_did"] = json!("did:wba:awiki.test");
    result
}

fn community_bootstrap(binding: &crate::identity::ActiveSyncAccountBinding) -> Value {
    json!({"mode":"tail_only", "account_id":binding.account_id, "device_id":binding.protocol_device_id,
        "server_time":"2026-09-17T10:00:00Z", "cursor":{"stream_epoch":"1","scan_seq":"0"},
        "read_state_baseline":[], "group_state_baseline":[], "warnings":["single_device_pull_only"]})
}

#[tokio::test]
async fn community_sync_runtime_bootstraps_hydrates_and_revalidates_after_reopen() {
    let fixture = SyncSnapshotFixture::new("community-runtime");
    let client = fixture.client();
    let binding = client.active_sync_account_binding().await.unwrap();
    let group = "did:wba:awiki.test:groups:community";
    let event = sync_snapshot_message_event(
        &binding,
        "community-event",
        "1",
        "1",
        "community-message",
        group,
    );
    let message = sync_snapshot_message(
        &binding,
        "community-message",
        group,
        "1",
        "community retained message",
    );
    let calls = Rc::new(RefCell::new(Vec::new()));
    MessageSyncRuntimeV2::new(&client, ReadySyncSnapshotSessionProvider,
        SyncSnapshotTransport::queued(Rc::clone(&calls), vec![
            Ok(community_capabilities()), Ok(community_bootstrap(&binding)),
            Ok(sync_snapshot_delta("1", "1", vec![event])),
            Ok(json!({"items":[{"event_id":"community-event","message":message}],"unavailable":[]})),
        ]), NoopAsyncDirectoryTransport).sync_now(sync_snapshot_request()).await.unwrap();
    assert!(fixture.has_message_content("community retained message"));
    assert_eq!(
        calls.borrow()[1].params["body"]["capabilities"],
        json!({"sync_profile":"anp.sync.local.v2","event_schema_max":1})
    );
    assert!(
        calls
            .borrow()
            .iter()
            .all(|call| call.method != "sync.snapshot"
                && call.params.pointer("/body/lanes").is_none())
    );
    assert_eq!(
        crate::internal::community_sync::cached_mode(&client).unwrap(),
        Some(crate::internal::community_sync::SyncServiceMode::Community)
    );
    assert!(crate::internal::community_sync::guard_request(
        &client,
        &json!({"meta":{"profile":"anp.direct.e2ee.v2"}})
    )
    .is_err());
    let reopened = fixture.client();
    let resumed_calls = Rc::new(RefCell::new(Vec::new()));
    MessageSyncRuntimeV2::new(
        &reopened,
        ReadySyncSnapshotSessionProvider,
        SyncSnapshotTransport::queued(
            Rc::clone(&resumed_calls),
            vec![
                Ok(community_capabilities()),
                Ok(sync_snapshot_delta("1", "1", vec![])),
            ],
        ),
        NoopAsyncDirectoryTransport,
    )
    .sync_now(sync_snapshot_request())
    .await
    .unwrap();
    let resumed = resumed_calls.borrow();
    assert_eq!(
        resumed
            .iter()
            .map(|call| call.method.as_str())
            .collect::<Vec<_>>(),
        ["anp.get_capabilities", "sync.delta"]
    );
    assert_eq!(resumed[1].params["body"]["cursor"]["scan_seq"], "1");
}

#[tokio::test]
async fn community_sync_runtime_declines_discovery_failure_and_mode_changes_without_reset() {
    let fixture = SyncSnapshotFixture::new("community-mode-fence");
    let client = fixture.client();
    let binding = client.active_sync_account_binding().await.unwrap();
    let mut discovery = SyncSnapshotTransport::queued(
        Rc::new(RefCell::new(Vec::new())),
        vec![Ok(community_capabilities())],
    );
    crate::internal::community_sync::discover(&client, &mut discovery)
        .await
        .unwrap();
    assert!(crate::internal::community_sync::confirm(
        &client,
        &explicit_sync_negotiation_response()
    )
    .await
    .is_err());
    let calls = Rc::new(RefCell::new(Vec::new()));
    let result = MessageSyncRuntimeV2::new(
        &client,
        ReadySyncSnapshotSessionProvider,
        SyncSnapshotTransport::queued(
            Rc::clone(&calls),
            vec![Err(crate::ImError::TransportUnavailable {
                detail: "fixture discovery failure".into(),
            })],
        ),
        NoopAsyncDirectoryTransport,
    )
    .sync_now(sync_snapshot_request())
    .await;
    assert!(result.is_err());
    assert_eq!(calls.borrow().len(), 1);
    let db = client.core_inner().local_state_db().await.unwrap();
    assert!(db
        .load_lane_sync_states(binding.owner_identity_id)
        .await
        .unwrap()
        .is_empty());
}
