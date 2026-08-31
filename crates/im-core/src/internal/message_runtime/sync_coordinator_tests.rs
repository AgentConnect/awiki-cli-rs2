use super::sync_coordinator::{
    MessageSyncCoordinatorRegistry, MessageSyncExecutor, MessageSyncRequestKind,
};
use crate::messages::{MessageSyncOutcome, MessageSyncRequest, MessageSyncStatus};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

fn request(reason: &str) -> MessageSyncRequest {
    MessageSyncRequest {
        reason: reason.to_owned(),
        limit: Some(100),
    }
}

fn outcome(run: usize, status: MessageSyncStatus) -> MessageSyncOutcome {
    MessageSyncOutcome {
        status,
        events_applied: 0,
        pages_fetched: 1,
        messages_hydrated: 0,
        duplicates_skipped: 0,
        changed_conversation_ids: Vec::new(),
        committed_incoming_messages: Vec::new(),
        error_code: None,
        warnings: vec![format!("test.run.{run}")],
    }
}

fn immediate_executor(calls: Arc<AtomicUsize>) -> MessageSyncExecutor {
    Arc::new(move |_request| {
        let calls = Arc::clone(&calls);
        Box::pin(async move {
            let run = calls.fetch_add(1, Ordering::SeqCst) + 1;
            Ok(outcome(run, MessageSyncStatus::Idle))
        })
    })
}

#[tokio::test]
async fn same_owner_ensure_requests_share_one_leader_and_outcome() {
    let registry = MessageSyncCoordinatorRegistry::default();
    let coordinator = registry.for_owner("owner-alice");
    let calls = Arc::new(AtomicUsize::new(0));
    let executor = immediate_executor(Arc::clone(&calls));

    let first = coordinator
        .register(
            request("session_start"),
            MessageSyncRequestKind::EnsureCurrent,
            Arc::clone(&executor),
        )
        .unwrap();
    let second = coordinator
        .register(
            request("foreground_reconcile"),
            MessageSyncRequestKind::EnsureCurrent,
            executor,
        )
        .unwrap();

    assert!(first.leader.is_some());
    assert!(second.leader.is_none());
    coordinator.run(first.leader.unwrap()).await;

    let first_outcome = first.receiver.await.unwrap().unwrap();
    let second_outcome = second.receiver.await.unwrap().unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(first_outcome, second_outcome);
    assert_eq!(first_outcome.warnings, ["test.run.1"]);
}

#[tokio::test]
async fn repeated_dirty_requests_coalesce_into_one_follow_up_run() {
    let registry = MessageSyncCoordinatorRegistry::default();
    let coordinator = registry.for_owner("owner-alice");
    let calls = Arc::new(AtomicUsize::new(0));
    let executor = immediate_executor(Arc::clone(&calls));

    let first = coordinator
        .register(
            request("session_start"),
            MessageSyncRequestKind::EnsureCurrent,
            Arc::clone(&executor),
        )
        .unwrap();
    let mut dirty = Vec::new();
    for _ in 0..100 {
        dirty.push(
            coordinator
                .register(
                    request("websocket_hint"),
                    MessageSyncRequestKind::DirtyAfterCurrent,
                    Arc::clone(&executor),
                )
                .unwrap(),
        );
    }
    assert!(dirty
        .iter()
        .all(|registration| registration.leader.is_none()));

    coordinator.run(first.leader.unwrap()).await;
    assert_eq!(
        first.receiver.await.unwrap().unwrap().warnings,
        ["test.run.1"]
    );
    for registration in dirty {
        assert_eq!(
            registration.receiver.await.unwrap().unwrap().warnings,
            ["test.run.2"]
        );
    }
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn failed_leader_fails_pending_waiters_without_automatic_follow_up() {
    let registry = MessageSyncCoordinatorRegistry::default();
    let coordinator = registry.for_owner("owner-alice");
    let calls = Arc::new(AtomicUsize::new(0));
    let executor: MessageSyncExecutor = {
        let calls = Arc::clone(&calls);
        Arc::new(move |_request| {
            let calls = Arc::clone(&calls);
            Box::pin(async move {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(outcome(1, MessageSyncStatus::RetryableFailure))
            })
        })
    };

    let first = coordinator
        .register(
            request("session_start"),
            MessageSyncRequestKind::EnsureCurrent,
            Arc::clone(&executor),
        )
        .unwrap();
    let pending = coordinator
        .register(
            request("websocket_hint"),
            MessageSyncRequestKind::DirtyAfterCurrent,
            executor,
        )
        .unwrap();

    coordinator.run(first.leader.unwrap()).await;
    assert_eq!(
        first.receiver.await.unwrap().unwrap().status,
        MessageSyncStatus::RetryableFailure
    );
    assert_eq!(
        pending.receiver.await.unwrap().unwrap().status,
        MessageSyncStatus::RetryableFailure
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn cancelled_waiter_does_not_cancel_the_shared_run() {
    let registry = MessageSyncCoordinatorRegistry::default();
    let coordinator = registry.for_owner("owner-alice");
    let calls = Arc::new(AtomicUsize::new(0));
    let executor = immediate_executor(Arc::clone(&calls));

    let first = coordinator
        .register(
            request("session_start"),
            MessageSyncRequestKind::EnsureCurrent,
            Arc::clone(&executor),
        )
        .unwrap();
    let cancelled = coordinator
        .register(
            request("foreground_reconcile"),
            MessageSyncRequestKind::EnsureCurrent,
            executor,
        )
        .unwrap();
    drop(cancelled.receiver);

    coordinator.run(first.leader.unwrap()).await;
    assert_eq!(
        first.receiver.await.unwrap().unwrap().status,
        MessageSyncStatus::Idle
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn coordinator_registry_is_scoped_by_owner() {
    let registry = MessageSyncCoordinatorRegistry::default();
    let alice = registry.for_owner("owner-alice");
    let alice_again = registry.for_owner("owner-alice");
    let bob = registry.for_owner("owner-bob");

    assert!(Arc::ptr_eq(&alice, &alice_again));
    assert!(!Arc::ptr_eq(&alice, &bob));
}

#[tokio::test]
async fn coordinator_owned_run_survives_the_original_callers_cancellation() {
    let registry = MessageSyncCoordinatorRegistry::default();
    let coordinator = registry.for_owner("owner-alice");
    let calls = Arc::new(AtomicUsize::new(0));
    let started = Arc::new(tokio::sync::Barrier::new(2));
    let release = Arc::new(tokio::sync::Semaphore::new(0));
    let executor: MessageSyncExecutor = {
        let calls = Arc::clone(&calls);
        let started = Arc::clone(&started);
        let release = Arc::clone(&release);
        Arc::new(move |_request| {
            let calls = Arc::clone(&calls);
            let started = Arc::clone(&started);
            let release = Arc::clone(&release);
            Box::pin(async move {
                calls.fetch_add(1, Ordering::SeqCst);
                started.wait().await;
                let _permit = release.acquire().await.unwrap();
                Ok(outcome(1, MessageSyncStatus::Idle))
            })
        })
    };

    let original = {
        let coordinator = Arc::clone(&coordinator);
        let executor = Arc::clone(&executor);
        tokio::spawn(async move {
            coordinator
                .execute(
                    request("session_start"),
                    MessageSyncRequestKind::EnsureCurrent,
                    executor,
                )
                .await
        })
    };
    started.wait().await;
    original.abort();
    assert!(original.await.unwrap_err().is_cancelled());

    let replacement = coordinator
        .register(
            request("foreground_reconcile"),
            MessageSyncRequestKind::EnsureCurrent,
            executor,
        )
        .unwrap();
    assert!(replacement.leader.is_none());
    release.add_permits(1);

    let shared = replacement.receiver.await.unwrap().unwrap();
    assert_eq!(shared.status, MessageSyncStatus::Idle);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn different_owners_can_execute_sync_runs_concurrently() {
    let registry = MessageSyncCoordinatorRegistry::default();
    let alice = registry.for_owner("owner-alice");
    let bob = registry.for_owner("owner-bob");
    let rendezvous = Arc::new(tokio::sync::Barrier::new(3));
    let calls = Arc::new(AtomicUsize::new(0));
    let executor: MessageSyncExecutor = {
        let rendezvous = Arc::clone(&rendezvous);
        let calls = Arc::clone(&calls);
        Arc::new(move |_request| {
            let rendezvous = Arc::clone(&rendezvous);
            let calls = Arc::clone(&calls);
            Box::pin(async move {
                let run = calls.fetch_add(1, Ordering::SeqCst) + 1;
                rendezvous.wait().await;
                Ok(outcome(run, MessageSyncStatus::Idle))
            })
        })
    };

    let alice_run = tokio::spawn({
        let alice = Arc::clone(&alice);
        let executor = Arc::clone(&executor);
        async move {
            alice
                .execute(
                    request("session_start"),
                    MessageSyncRequestKind::EnsureCurrent,
                    executor,
                )
                .await
        }
    });
    let bob_run = tokio::spawn({
        let bob = Arc::clone(&bob);
        async move {
            bob.execute(
                request("session_start"),
                MessageSyncRequestKind::EnsureCurrent,
                executor,
            )
            .await
        }
    });

    tokio::time::timeout(std::time::Duration::from_secs(5), rendezvous.wait())
        .await
        .expect("different owners must reach the transport concurrently");
    assert_eq!(
        alice_run.await.unwrap().unwrap().status,
        MessageSyncStatus::Idle
    );
    assert_eq!(
        bob_run.await.unwrap().unwrap().status,
        MessageSyncStatus::Idle
    );
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn local_state_operation_lock_hides_partially_handed_off_lane_inputs() {
    let registry = MessageSyncCoordinatorRegistry::default();
    let coordinator = registry.for_owner("owner-alice");
    let sync_guard = coordinator.lock_local_state_operation().await;
    let (started_sender, started_receiver) = tokio::sync::oneshot::channel();
    let (acquired_sender, mut acquired_receiver) = tokio::sync::oneshot::channel();

    let lane_consumer = tokio::spawn({
        let coordinator = Arc::clone(&coordinator);
        async move {
            let _ = started_sender.send(());
            let _lane_guard = coordinator.lock_local_state_operation().await;
            let _ = acquired_sender.send(());
        }
    });
    started_receiver.await.unwrap();
    assert!(matches!(
        acquired_receiver.try_recv(),
        Err(tokio::sync::oneshot::error::TryRecvError::Empty)
    ));

    drop(sync_guard);
    acquired_receiver.await.unwrap();
    lane_consumer.await.unwrap();
}
