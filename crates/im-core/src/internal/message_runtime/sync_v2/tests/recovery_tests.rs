use super::*;

struct PausedDeltaTransport {
    entered: Rc<tokio::sync::Notify>,
    release: Rc<tokio::sync::Notify>,
}

impl AsyncAuthenticatedRpcTransport for PausedDeltaTransport {
    async fn authenticated_rpc(
        &mut self,
        _endpoint: &str,
        method: &str,
        _params: Value,
    ) -> crate::ImResult<Value> {
        assert_eq!(method, "sync.delta");
        self.entered.notify_one();
        self.release.notified().await;
        Ok(sync_snapshot_delta("1", "0", vec![]))
    }
}

#[tokio::test]
async fn concurrent_cores_receive_without_superseding_each_other() {
    let fixture = SyncSnapshotFixture::new("concurrent-cores-receive");
    let first = fixture.client();
    let binding = first.active_sync_account_binding().await.unwrap();
    seed_sync_snapshot_ready_state(&first, &binding, "1", "0").await;
    seed_lane_states(&first, &binding, &[]).await;
    let second = fixture.client();
    let entered = Rc::new(tokio::sync::Notify::new());
    let release = Rc::new(tokio::sync::Notify::new());
    let first_run = MessageSyncRuntimeV2::new(
        &first,
        ReadySyncSnapshotSessionProvider,
        PausedDeltaTransport {
            entered: Rc::clone(&entered),
            release: Rc::clone(&release),
        },
        NoopAsyncDirectoryTransport,
    )
    .receive_now(sync_snapshot_request());
    let second_run = async {
        entered.notified().await;
        let run = MessageSyncRuntimeV2::new(
            &second,
            ReadySyncSnapshotSessionProvider,
            SyncSnapshotTransport::queued(
                Rc::new(RefCell::new(Vec::new())),
                vec![Ok(sync_snapshot_delta("1", "0", vec![]))],
            ),
            NoopAsyncDirectoryTransport,
        )
        .receive_now(sync_snapshot_request());
        tokio::pin!(run);
        let early = tokio::select! {
            result = &mut run => Some(result),
            _ = tokio::time::sleep(StdDuration::from_millis(100)) => None,
        };
        release.notify_one();
        match early {
            Some(result) => result,
            None => run.await,
        }
    };
    let (first_result, second_result) = tokio::join!(first_run, second_run);
    assert!(first_result.is_ok(), "first receiver: {first_result:?}");
    assert!(second_result.is_ok(), "second receiver: {second_result:?}");
    assert!(first_result.unwrap().complete);
    assert!(second_result.unwrap().complete);
}

#[tokio::test]
async fn promotion_bootstrap_keeps_unconsumed_ordinary_events() {
    for (epoch, expected_cursor) in [("1", "10"), ("2", "20")] {
        let fixture = SyncSnapshotFixture::new("promotion-bootstrap-unconsumed");
        let old = fixture.client();
        let binding = old.active_sync_account_binding().await.unwrap();
        seed_sync_snapshot_ready_state(&old, &binding, "1", "10").await;

        let paths = SyncSnapshotFixture::paths(&fixture.root);
        let store = crate::internal::identity_store::IdentityStore::new(&paths.identities);
        let mut device = store.load_index().unwrap().credentials["alice"]
            .device_state
            .clone()
            .unwrap();
        let authorization = device.authorization.as_mut().unwrap();
        authorization.auth_generation = 2;
        authorization.role = crate::internal::identity_device_state::DeviceAuthorizationRole::Admin;
        authorization.management_ready = true;
        store.save_device_state("alice", device).unwrap();
        let client = fixture.client();
        let binding = client.active_sync_account_binding().await.unwrap();
        assert_eq!(binding.device_auth_generation, "2");
        let bootstrap =
            sync_snapshot_tail_bootstrap_for_current_features(&client, &binding, epoch, "20").await;
        let calls = Rc::new(RefCell::new(Vec::new()));
        let mut delta = sync_snapshot_delta(epoch, "20", vec![]);
        let mut lanes = Map::new();
        for (name, lane) in bootstrap["lanes"].as_object().unwrap() {
            lanes.insert(
                name.clone(),
                json!({
                    "events": [], "next_cursor": lane["cursor"], "has_more": false,
                }),
            );
        }
        delta["lanes"] = Value::Object(lanes);
        MessageSyncRuntimeV2::new(
            &client,
            ReadySyncSnapshotSessionProvider,
            SyncSnapshotTransport::queued(
                Rc::clone(&calls),
                vec![
                    Ok(explicit_sync_negotiation_response()),
                    Ok(bootstrap),
                    Ok(delta),
                ],
            ),
            NoopAsyncDirectoryTransport,
        )
        .receive_now(sync_snapshot_request())
        .await
        .unwrap();
        let calls = calls.borrow();
        assert_eq!(calls[1].method, "sync.bootstrap");
        assert_eq!(calls[2].method, "sync.delta");
        assert_eq!(
            calls[2].params.pointer("/body/cursor/scan_seq"),
            Some(&json!(expected_cursor)),
            "fresh authorization must not skip events delivered before its bootstrap"
        );
    }
}

#[tokio::test]
async fn receiver_lock_wait_respects_budget_without_starting_a_new_generation() {
    let fixture = SyncSnapshotFixture::new("receiver-lock-budget");
    let client = fixture.client();
    let binding = client.active_sync_account_binding().await.unwrap();
    seed_sync_snapshot_ready_state(&client, &binding, "1", "0").await;
    seed_lane_states(&client, &binding, &[]).await;
    let held = crate::internal::message_runtime::sync_receive_lock::ReceiveLock::acquire(
        &fixture.sqlite_path(),
        &binding.owner_identity_id,
    )
    .await
    .unwrap();
    let error = MessageSyncRuntimeV2::new(
        &client,
        ReadySyncSnapshotSessionProvider,
        SyncSnapshotTransport::queued(Rc::new(RefCell::new(Vec::new())), vec![]),
        NoopAsyncDirectoryTransport,
    )
    .with_run_deadline_for_test(StdDuration::from_millis(50))
    .receive_now(sync_snapshot_request())
    .await
    .unwrap_err();
    assert!(
        matches!(error, crate::ImError::Service { code: Some(code), .. }
        if code == "SYNC_RECEIVE_BUSY")
    );
    let connection = rusqlite::Connection::open(fixture.sqlite_path()).unwrap();
    assert_eq!(
        connection
            .query_row("SELECT COUNT(*) FROM message_sync_run_state", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
    drop(held);
    let received = MessageSyncRuntimeV2::new(
        &client,
        ReadySyncSnapshotSessionProvider,
        SyncSnapshotTransport::queued(
            Rc::new(RefCell::new(Vec::new())),
            vec![Ok(sync_snapshot_delta("1", "0", vec![]))],
        ),
        NoopAsyncDirectoryTransport,
    )
    .receive_now(sync_snapshot_request())
    .await
    .unwrap();
    assert!(received.complete);
}
