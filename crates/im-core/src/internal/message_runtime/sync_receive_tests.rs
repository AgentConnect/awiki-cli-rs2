use super::*;

#[tokio::test]
async fn read_outbox_worker_refreshes_once_without_restarting_receive() {
    for rejected_again in [false, true] {
        let fixture = SyncSnapshotFixture::new(if rejected_again {
            "read-worker-rejected-twice"
        } else {
            "read-worker-refresh"
        });
        let client = fixture.client();
        let binding = client.active_sync_account_binding().await.unwrap();
        seed_sync_snapshot_ready_state(&client, &binding, "1", "10").await;
        let group = "did:wba:awiki.test:groups:read-worker";
        seed_sync_read_group_message(&client, &binding, &format!("{group}:30"), None, group, 30)
            .await;
        let db = client.core_inner().local_state_db().await.unwrap();
        db.mark_thread_read_watermark(
            binding.owner_identity_id.clone(),
            binding.current_did.clone(),
            crate::internal::local_state::messages::MarkThreadReadWatermarkInput {
                thread: crate::messages::ThreadRef::Group(
                    crate::ids::GroupRef::parse(group).unwrap(),
                ),
                read_watermark_message_id: Some(format!("{group}:30")),
                read_watermark_seq: Some("30".into()),
                read_watermark_at: Some("2026-07-28T12:00:02Z".into()),
                pending_remote_ack: true,
            },
        )
        .await
        .unwrap();
        let calls = Rc::new(RefCell::new(Vec::new()));
        let refresh_calls = Rc::new(RefCell::new(0));
        let reloads = Rc::new(RefCell::new(0));
        let rejection = || crate::ImError::Service {
            status_code: Some(409),
            code: Some("anp.device_state_changed".into()),
            message: "device authorization epoch is stale".into(),
            data: None,
        };
        let bootstrap =
            sync_snapshot_tail_bootstrap_for_current_features(&client, &binding, "1", "99").await;
        let response = if rejected_again {
            Err(rejection())
        } else {
            Ok(sync_group_read_ack(
                &binding,
                group,
                "30",
                &format!("{group}:30"),
                "2026-07-28T12:00:03Z",
            ))
        };
        let result = MessageSyncRuntimeV2::new(
            &client,
            RefreshingSyncSnapshotSessionProvider {
                refresh_calls: Rc::clone(&refresh_calls),
                fail_refresh: false,
            },
            ReloadingSyncSnapshotTransport {
                inner: SyncSnapshotTransport::queued(
                    Rc::clone(&calls),
                    vec![
                        Err(rejection()),
                        Ok(explicit_sync_negotiation_response()),
                        Ok(bootstrap),
                        response,
                    ],
                ),
                authentication_reloads: Rc::clone(&reloads),
            },
            NoopAsyncDirectoryTransport,
        )
        .process_read_outbox(&db, &binding, 1)
        .await;
        assert_eq!(
            *refresh_calls.borrow(),
            1,
            "the production read worker must refresh a registry fence once"
        );
        assert_eq!(*reloads.borrow(), 1);
        assert_eq!(result.is_err(), rejected_again);
        assert_eq!(
            calls
                .borrow()
                .iter()
                .map(|call| call.method.clone())
                .collect::<Vec<_>>(),
            [
                "read_state.mark_read",
                "anp.get_capabilities",
                "sync.bootstrap",
                "read_state.mark_read"
            ]
        );
        let state = db
            .load_message_sync_state(&binding.owner_identity_id)
            .await
            .unwrap();
        let crate::internal::local_state::sync_v2::MessageSyncStateAccess::Ready(state) = state
        else {
            panic!("receive checkpoint must remain ready")
        };
        assert_eq!(
            state.scan_seq, "10",
            "a writeback refresh cannot adopt the bootstrap ordinary cursor"
        );
        assert_eq!(
            calls.borrow()[0].params["meta"]["operation_id"],
            calls.borrow()[3].params["meta"]["operation_id"]
        );
        let pending = db
            .run_local(|connection| {
                connection
                    .query_row(
                        "SELECT pending_remote_ack FROM thread_read_state",
                        [],
                        |row| row.get::<_, bool>(0),
                    )
                    .map_err(crate::internal::local_state::local_state_unavailable)
            })
            .await
            .unwrap();
        assert_eq!(
            pending, rejected_again,
            "only a final remote ACK clears writeback responsibility"
        );
    }
}

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

#[cfg(feature = "group-e2ee")]
#[tokio::test]
async fn malformed_p5_keeps_ordinary_and_p6_checkpoints_then_retries_only_p5() {
    use crate::internal::wire::sync_v2::SyncLaneV3;
    let fixture = SyncSnapshotFixture::new("receive-malformed-p5-retry");
    let client = fixture.client();
    let binding = client.active_sync_account_binding().await.unwrap();
    seed_sync_snapshot_ready_state(&client, &binding, "1", "0").await;
    seed_lane_states(
        &client,
        &binding,
        &[(SyncLaneV3::P5Device, "41"), (SyncLaneV3::P6Group, "42")],
    )
    .await;
    let group = "did:wba:awiki.test:groups:receive-isolation";
    let ordinary = sync_group_profile_updated_event(
        &binding,
        "ordinary-1",
        "1",
        group,
        "1",
        "1",
        &binding.current_did,
    );
    let envelope = p6_lane_envelope(&binding, "p6-message-1", group, "1");
    let mut first = sync_snapshot_delta("1", "1", vec![ordinary]);
    first["lanes"] = json!({
        "p5_device": {"events":"malformed", "next_cursor":{"stream_epoch":"41","scan_seq":"1"}, "has_more":false},
        "p6_group": {"events":[p6_lane_event("p6-event-1","1",group,"1",&envelope)], "next_cursor":{"stream_epoch":"42","scan_seq":"1"}, "has_more":false}
    });
    let received = MessageSyncRuntimeV2::new(
        &client,
        ReadySyncSnapshotSessionProvider,
        SyncSnapshotTransport::queued(Rc::new(RefCell::new(vec![])), vec![Ok(first)]),
        NeverProcessingDirectory,
    )
    .receive_now(sync_snapshot_request())
    .await
    .unwrap();
    assert_eq!(received.status, crate::messages::MessageSyncStatus::Changed);
    assert!(!received.complete);
    assert_eq!(received.events_received, 2);
    assert!(received
        .warnings
        .contains(&"sync.lane.p5_device.transport_invalid".into()));
    assert_eq!(
        load_sync_snapshot_state(&client, &binding.owner_identity_id)
            .await
            .scan_seq,
        "1"
    );
    let db = client.core_inner().local_state_db().await.unwrap();
    let states = db
        .load_lane_sync_states(binding.owner_identity_id.clone())
        .await
        .unwrap();
    assert_eq!(
        states
            .iter()
            .find(|state| state.lane == SyncLaneV3::P5Device)
            .unwrap()
            .scan_seq,
        "0"
    );
    assert_eq!(
        states
            .iter()
            .find(|state| state.lane == SyncLaneV3::P6Group)
            .unwrap()
            .scan_seq,
        "1"
    );
    let mut retry = sync_snapshot_delta("1", "1", vec![]);
    retry["lanes"] = json!({
        "p5_device": {"events":[poison_p5_lane_event("p5-event-1","1")], "next_cursor":{"stream_epoch":"41","scan_seq":"1"}, "has_more":false},
        "p6_group": {"events":[], "next_cursor":{"stream_epoch":"42","scan_seq":"1"}, "has_more":false}
    });
    let calls = Rc::new(RefCell::new(vec![]));
    let received = MessageSyncRuntimeV2::new(
        &client,
        ReadySyncSnapshotSessionProvider,
        SyncSnapshotTransport::queued(Rc::clone(&calls), vec![Ok(retry)]),
        NeverProcessingDirectory,
    )
    .receive_now(sync_snapshot_request())
    .await
    .unwrap();
    assert!(received.complete);
    assert_eq!(received.events_received, 1);
    assert_eq!(
        sqlite_count(
            &fixture.sqlite_path(),
            "SELECT COUNT(*) FROM sync_lane_inbox"
        ),
        3
    );
    let calls = calls.borrow();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].params["body"]["cursor"]["scan_seq"], "1");
    assert_eq!(
        calls[0].params["body"]["lanes"]["p5_device"]["cursor"]["scan_seq"],
        "0"
    );
    assert_eq!(
        calls[0].params["body"]["lanes"]["p6_group"]["cursor"]["scan_seq"],
        "1"
    );
}

#[cfg(all(feature = "secure-direct", feature = "group-e2ee"))]
#[tokio::test]
async fn p6_epoch_recovery_preserves_old_inputs_and_other_stream_checkpoints() {
    use crate::internal::wire::sync_v2::SyncLaneV3;
    let fixture = SyncSnapshotFixture::new("receive-p6-new-epoch");
    let client = fixture.client();
    let binding = client.active_sync_account_binding().await.unwrap();
    seed_sync_snapshot_ready_state(&client, &binding, "1", "0").await;
    seed_lane_states(
        &client,
        &binding,
        &[(SyncLaneV3::P5Device, "41"), (SyncLaneV3::P6Group, "42")],
    )
    .await;
    let group = "did:wba:awiki.test:groups:p6-epoch-recovery";
    let envelope = p6_lane_envelope(&binding, "old-p6-message", group, "1");
    let old = sync_snapshot_delta_with_lanes(
        "1",
        "0",
        json!({
            "p5_device":{"events":[],"next_cursor":{"stream_epoch":"41","scan_seq":"0"},"has_more":false},
            "p6_group":{"events":[p6_lane_event("old-p6-event","1",group,"1",&envelope)],"next_cursor":{"stream_epoch":"42","scan_seq":"1"},"has_more":false}
        }),
    );
    MessageSyncRuntimeV2::new(
        &client,
        ReadySyncSnapshotSessionProvider,
        SyncSnapshotTransport::queued(Rc::new(RefCell::new(vec![])), vec![Ok(old)]),
        NeverProcessingDirectory,
    )
    .receive_now(sync_snapshot_request())
    .await
    .unwrap();
    let ordinary = sync_group_profile_updated_event(
        &binding,
        "ordinary-1",
        "1",
        group,
        "1",
        "1",
        &binding.current_did,
    );
    let mut rejected = sync_snapshot_delta("1", "1", vec![ordinary]);
    rejected["lanes"] = json!({
        "p5_device":{"events":[poison_p5_lane_event("p5-event-1","1")],"next_cursor":{"stream_epoch":"41","scan_seq":"1"},"has_more":false},
        "p6_group":{"error":{"code":4602,"anp_code":"p6_group_recovery_required","message":"lane epoch was rotated"}}
    });
    let mut bootstrap =
        sync_snapshot_tail_bootstrap_for_current_features(&client, &binding, "1", "99").await;
    bootstrap["lanes"]["p6_group"]["cursor"]["stream_epoch"] = json!("43");
    let envelope = p6_lane_envelope(&binding, "new-p6-message", group, "2");
    let resumed = sync_snapshot_delta_with_lanes(
        "1",
        "1",
        json!({
            "p5_device":{"events":[],"next_cursor":{"stream_epoch":"41","scan_seq":"1"},"has_more":false},
            "p6_group":{"events":[p6_lane_event("new-p6-event","1",group,"2",&envelope)],"next_cursor":{"stream_epoch":"43","scan_seq":"1"},"has_more":false}
        }),
    );
    let calls = Rc::new(RefCell::new(vec![]));
    let received = MessageSyncRuntimeV2::new(
        &client,
        ReadySyncSnapshotSessionProvider,
        SyncSnapshotTransport::queued(
            Rc::clone(&calls),
            vec![
                Ok(rejected),
                Ok(explicit_sync_negotiation_response()),
                Ok(bootstrap),
                Ok(resumed),
            ],
        ),
        NeverProcessingDirectory,
    )
    .receive_now(sync_snapshot_request())
    .await
    .unwrap();
    assert!(
        received.complete,
        "new epoch must resume after one explicit lane bootstrap: {received:?}"
    );
    assert_eq!(received.events_received, 3);
    let db = client.core_inner().local_state_db().await.unwrap();
    let states = db
        .load_lane_sync_states(binding.owner_identity_id.clone())
        .await
        .unwrap();
    let p6 = states
        .iter()
        .find(|state| state.lane == SyncLaneV3::P6Group)
        .unwrap();
    assert_eq!(
        (p6.stream_epoch.as_str(), p6.scan_seq.as_str()),
        ("43", "1")
    );
    assert_eq!(
        sqlite_count(
            &fixture.sqlite_path(),
            "SELECT COUNT(*) FROM sync_lane_inbox WHERE lane='p6_group'"
        ),
        2
    );
    assert_eq!(
        sqlite_count(
            &fixture.sqlite_path(),
            "SELECT COUNT(*) FROM sync_lane_inbox WHERE lane='p6_group' AND lane_epoch='42'"
        ),
        1
    );
    assert_eq!(
        load_sync_snapshot_state(&client, &binding.owner_identity_id)
            .await
            .scan_seq,
        "1"
    );
    let calls = calls.borrow();
    assert_eq!(
        calls
            .iter()
            .map(|call| call.method.as_str())
            .collect::<Vec<_>>(),
        [
            "sync.delta",
            "anp.get_capabilities",
            "sync.bootstrap",
            "sync.delta"
        ]
    );
    assert_eq!(calls[3].params["body"]["cursor"]["scan_seq"], "1");
    assert_eq!(
        calls[3].params["body"]["lanes"]["p5_device"]["cursor"]["scan_seq"],
        "1"
    );
    assert_eq!(
        calls[3].params["body"]["lanes"]["p6_group"]["cursor"]["stream_epoch"],
        "43"
    );
    assert_eq!(
        calls[3].params["body"]["lanes"]["p6_group"]["cursor"]["scan_seq"],
        "0"
    );
}

#[tokio::test]
async fn hundred_page_snapshot_keeps_its_own_budget_and_receives_all_post_anchor_pages() {
    let fixture = SyncSnapshotFixture::new("receive-hundred-page-snapshot");
    let client = fixture.client();
    let binding = client.active_sync_account_binding().await.unwrap();
    seed_sync_snapshot_ready_state(&client, &binding, "1", "10").await;
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
    let group = "did:example:hundred-page-group";
    let items = (1..=100)
        .map(|seq| {
            let seq = seq.to_string();
            let id = format!("snapshot-{seq}");
            json!({"event":sync_snapshot_message_event(&binding,&id,"2",&seq,&id,group),
            "message":sync_snapshot_message(&binding,&id,group,&seq,&id)})
        })
        .collect::<Vec<_>>();
    let mut template = sync_snapshot_response(
        &client,
        &binding,
        "hundred-pages",
        "2",
        "100",
        items.clone(),
    )
    .await;
    template["manifest"]["total_pages"] = json!(100);
    template["manifest"]["history_policy"]["returned_pages"] = json!(100);
    template["manifest"]["history_policy"]["excluded_older_messages"] = json!(1);
    template["manifest"]["history_policy"]["older_history_excluded"] = json!(true);
    template["manifest"]["history_policy"]["truncation_reason"] = json!("max_pages");
    template["manifest"]
        .as_object_mut()
        .unwrap()
        .remove("manifest_digest");
    let digest = crate::internal::wire::sync_v2::canonical_digest(&template["manifest"]).unwrap();
    template["manifest"]["manifest_digest"] = json!(digest);
    let mut responses = vec![Ok(sync_snapshot_recovery(
        "hundred-pages",
        "token",
        "2",
        "100",
    ))];
    for (index, item) in items.iter().enumerate() {
        let mut page = template.clone();
        if index > 0 {
            page.as_object_mut().unwrap().remove("manifest");
            page["manifest_digest"] = json!(digest);
        }
        page["page"]["items"] = json!([item]);
        page["page"]["returned_items"] = json!(1);
        page["page"]["returned_encoded_bytes"] =
            json!(serde_json_canonicalizer::to_vec(item).unwrap().len());
        page["page"]["page_digest"] =
            json!(crate::internal::wire::sync_v2::canonical_digest(&vec![item.clone()]).unwrap());
        page["page"]["has_more"] = json!(index < 99);
        page["page"]["next_page_ref"] = if index < 99 {
            json!(format!("page-{}", index + 2))
        } else {
            Value::Null
        };
        responses.push(Ok(page));
    }
    for seq in [101, 102] {
        let id = format!("live-{seq}");
        let seq = seq.to_string();
        let mut delta = sync_snapshot_delta(
            "2",
            &seq,
            vec![sync_snapshot_message_event(
                &binding, &id, "2", &seq, &id, group,
            )],
        );
        delta["has_more"] = json!(seq == "101");
        responses.push(Ok(delta));
        responses.push(Ok(json!({"items":[{"event_id":id,"message":sync_snapshot_message(&binding,&id,group,&seq,&id)}],"unavailable":[]})));
    }
    let calls = Rc::new(RefCell::new(Vec::new()));
    let received = MessageSyncRuntimeV2::new(
        &client,
        ReadySyncSnapshotSessionProvider,
        SyncSnapshotTransport::queued(calls.clone(), responses),
        NeverProcessingDirectory,
    )
    .receive_now(sync_snapshot_request())
    .await
    .unwrap();
    assert!(received.complete, "{received:?}");
    assert!(received.older_history_excluded);
    assert_eq!(received.pages_fetched, 102);
    assert_eq!(received.events_received, 102);
    assert_eq!(
        load_sync_snapshot_state(&client, &binding.owner_identity_id)
            .await
            .scan_seq,
        "102"
    );
    assert_eq!(
        sqlite_count(
            &fixture.sqlite_path(),
            "SELECT COUNT(*) FROM sync_lane_inbox WHERE event_id IN ('live-101','live-102')"
        ),
        2
    );
    let calls = calls.borrow();
    let cursors = calls
        .iter()
        .filter(|call| call.method == "sync.delta")
        .map(|call| {
            call.params
                .pointer("/body/cursor/scan_seq")
                .cloned()
                .unwrap()
        })
        .collect::<Vec<_>>();
    assert_eq!(cursors, vec![json!("10"), json!("100"), json!("101")]);
}
