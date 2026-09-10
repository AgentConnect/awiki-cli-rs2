use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use crate::internal::local_state::{sync_inbox, sync_v2 as storage};

struct ActiveCall(Arc<AtomicUsize>);
impl Drop for ActiveCall {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

async fn seed_dispatch_inputs(client: &crate::core::ImClient, count: usize) {
    let binding = client.active_sync_account_binding().await.unwrap();
    seed_sync_snapshot_ready_state(client, &binding, "1", "0").await;
    let db = client.core_inner().local_state_db().await.unwrap();
    let installation = db
        .load_or_create_sync_client_instance_id(&binding.owner_identity_id)
        .await
        .unwrap();
    let binding = stored_binding(client, &binding);
    db.run_local(move |connection| {
        let transaction = connection.unchecked_transaction().unwrap();
        for index in 0..count {
            let (lane, event_type, scope, group) = match index {
                2 => ("p5_device", "p5.delivery.created", "peer", None),
                3 => (
                    "p6_group",
                    "p6.control.notice",
                    "group",
                    Some("did:example:group".to_owned()),
                ),
                0 | 1 => ("ordinary", "test.ignored", "conversation-a", None),
                _ => ("ordinary", "test.ignored", "conversation-b", None),
            };
            let event_id = format!("event-{index:03}");
            sync_inbox::insert_input(
                &transaction,
                &binding,
                &installation,
                lane,
                "1",
                &sync_inbox::InboxEvent {
                    event_id: event_id.clone(),
                    position: (index + 1).to_string(),
                    event_type: event_type.to_owned(),
                    payload: json!({"test_event":event_id}),
                    processing_scope: scope.to_owned(),
                    group_did: group,
                },
                chrono::Utc::now().timestamp(),
            )?;
        }
        transaction.commit().unwrap();
        Ok(())
    })
    .await
    .unwrap();
}

async fn commit_test_input(
    client: &crate::core::ImClient,
    claim: &sync_inbox::InputClaim,
) -> crate::ImResult<crate::messages::MessageProcessingUpdate> {
    let db = client.core_inner().local_state_db().await?;
    let committed = claim.clone();
    db.run_local(move |connection| {
        let transaction = connection.unchecked_transaction().unwrap();
        let now = chrono::Utc::now().timestamp();
        sync_inbox::require_claim(&transaction, &committed, now)?;
        if committed.lane == "ordinary" {
            storage::record_applied_event(
                &transaction,
                &storage::AppliedEventReceipt {
                    owner_identity_id: committed.owner_identity_id.clone(),
                    event_id: committed.event_id.clone(),
                    stream_epoch: committed.lane_epoch.clone(),
                    event_seq: committed.position.clone(),
                    applied_at: now,
                },
            )?;
        }
        sync_inbox::complete_claim(&transaction, &committed, now)?;
        transaction.commit().unwrap();
        Ok(())
    })
    .await?;
    Ok(crate::messages::MessageProcessingUpdate {
        event_id: claim.event_id.clone(),
        status: crate::messages::MessageProcessingStatus::Applied,
        changed_conversation_ids: Vec::new(),
        committed_incoming_messages: Vec::new(),
        error_code: None,
    })
}

async fn until(mut predicate: impl FnMut() -> bool) {
    tokio::time::timeout(StdDuration::from_secs(3), async {
        while !predicate() {
            tokio::time::sleep(StdDuration::from_millis(5)).await;
        }
    })
    .await
    .expect("dispatcher made no progress");
}

#[tokio::test]
async fn central_dispatcher_keeps_other_types_and_same_conversation_moving_with_bounded_calls() {
    let fixture = SyncSnapshotFixture::new("dispatcher-isolation");
    let client = fixture.client();
    seed_dispatch_inputs(&client, 24).await;
    let dispatcher = crate::internal::message_runtime::sync_dispatcher::for_client(&client);
    let reopened = fixture.client();
    assert!(Arc::ptr_eq(
        &dispatcher,
        &crate::internal::message_runtime::sync_dispatcher::for_client(&reopened)
    ));
    let active = Arc::new(AtomicUsize::new(0));
    let maximum = Arc::new(AtomicUsize::new(0));
    let completed = Arc::new(AtomicUsize::new(0));
    let calls = Arc::new(Mutex::new(BTreeMap::<String, usize>::new()));
    let (release, gate) = tokio::sync::oneshot::channel::<()>();
    let gate = Arc::new(Mutex::new(Some(gate)));
    let (active_copy, maximum_copy, completed_copy, calls_copy) = (
        active.clone(),
        maximum.clone(),
        completed.clone(),
        calls.clone(),
    );
    dispatcher.set_executor_for_test(
        Arc::new(move |client, claim| {
            let (active, maximum, completed, calls, gate) = (
                active_copy.clone(),
                maximum_copy.clone(),
                completed_copy.clone(),
                calls_copy.clone(),
                gate.clone(),
            );
            Box::pin(async move {
                let running = active.fetch_add(1, Ordering::SeqCst) + 1;
                maximum.fetch_max(running, Ordering::SeqCst);
                let _active = ActiveCall(active);
                *calls
                    .lock()
                    .unwrap()
                    .entry(claim.event_id.clone())
                    .or_default() += 1;
                if claim.event_id == "event-000" {
                    let waiting = gate.lock().unwrap().take().unwrap();
                    let _ = waiting.await;
                }
                let result = commit_test_input(&client, &claim).await;
                if result.is_ok() {
                    completed.fetch_add(1, Ordering::SeqCst);
                }
                result
            })
        }),
        StdDuration::from_secs(10),
    );
    let mut updates = client.messages().watch_processing_updates().unwrap();
    for _ in 0..100 {
        dispatcher.wake_client(reopened.clone());
    }
    until(|| completed.load(Ordering::SeqCst) == 23).await;
    assert_eq!(active.load(Ordering::SeqCst), 1);
    assert!((2..=sync_inbox::MAX_ACTIVE_INPUTS).contains(&maximum.load(Ordering::SeqCst)));
    for index in [0, 1, 2, 3, 4] {
        assert_eq!(
            calls.lock().unwrap().get(&format!("event-{index:03}")),
            Some(&1)
        );
    }
    release.send(()).unwrap();
    until(|| completed.load(Ordering::SeqCst) == 24).await;
    assert!(calls.lock().unwrap().values().all(|count| *count == 1));
    updates.close();
    until(|| !dispatcher.running_for_test()).await;
    assert_eq!(
        sqlite_count(
            &fixture.sqlite_path(),
            "SELECT COUNT(*) FROM sync_lane_inbox"
        ),
        0
    );
}

#[tokio::test]
async fn timed_out_task_keeps_its_slot_until_it_ends_and_cannot_commit_a_late_result() {
    let fixture = SyncSnapshotFixture::new("dispatcher-timeout");
    let client = fixture.client();
    seed_dispatch_inputs(&client, 4).await;
    let dispatcher = crate::internal::message_runtime::sync_dispatcher::for_client(&client);
    let (release, gate) = tokio::sync::oneshot::channel::<()>();
    let gate = Arc::new(Mutex::new(Some(gate)));
    let active = Arc::new(AtomicUsize::new(0));
    let completed = Arc::new(AtomicUsize::new(0));
    let starts = Arc::new(AtomicUsize::new(0));
    let (active_copy, completed_copy, starts_copy) =
        (active.clone(), completed.clone(), starts.clone());
    dispatcher.set_executor_for_test(
        Arc::new(move |client, claim| {
            let (gate, active, completed, starts) = (
                gate.clone(),
                active_copy.clone(),
                completed_copy.clone(),
                starts_copy.clone(),
            );
            Box::pin(async move {
                active.fetch_add(1, Ordering::SeqCst);
                let _active = ActiveCall(active);
                if claim.event_id == "event-000" {
                    starts.fetch_add(1, Ordering::SeqCst);
                    let waiting = gate.lock().unwrap().take().unwrap();
                    let _ = waiting.await;
                }
                let result = commit_test_input(&client, &claim).await;
                if result.is_ok() {
                    completed.fetch_add(1, Ordering::SeqCst);
                }
                result
            })
        }),
        StdDuration::from_millis(100),
    );
    let mut updates = client.messages().watch_processing_updates().unwrap();
    until(|| completed.load(Ordering::SeqCst) == 3).await;
    until(|| sqlite_count(&fixture.sqlite_path(), "SELECT COUNT(*) FROM sync_lane_inbox WHERE processing_error_code='sync.processing_timeout'") == 1).await;
    assert_eq!(active.load(Ordering::SeqCst), 1);
    assert_eq!(starts.load(Ordering::SeqCst), 1);
    for _ in 0..100 {
        dispatcher.wake_client(client.clone());
    }
    release.send(()).unwrap();
    until(|| active.load(Ordering::SeqCst) == 0).await;
    assert_eq!(completed.load(Ordering::SeqCst), 3);
    assert_eq!(
        sqlite_count(
            &fixture.sqlite_path(),
            "SELECT COUNT(*) FROM sync_applied_events WHERE event_id='event-000'"
        ),
        0
    );
    client
        .core_inner()
        .local_state_db()
        .await
        .unwrap()
        .run_local(|connection| {
            sync_inbox::purge_expired(
                connection,
                chrono::Utc::now().timestamp() + sync_inbox::RETENTION_SECONDS,
                256,
            )
        })
        .await
        .unwrap();
    updates.close();
    until(|| !dispatcher.running_for_test()).await;
}
