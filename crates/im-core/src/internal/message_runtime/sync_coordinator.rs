use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, Weak};

use tokio::sync::oneshot;

pub(crate) type MessageSyncFuture = Pin<
    Box<dyn Future<Output = crate::ImResult<crate::messages::MessageSyncOutcome>> + Send + 'static>,
>;
pub(crate) type MessageSyncExecutor =
    Arc<dyn Fn(crate::messages::MessageSyncRequest) -> MessageSyncFuture + Send + Sync + 'static>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MessageSyncRequestKind {
    EnsureCurrent,
    DirtyAfterCurrent,
}

#[derive(Default)]
pub(crate) struct MessageSyncCoordinatorRegistry {
    coordinators: Mutex<HashMap<String, Weak<MessageSyncCoordinator>>>,
}

impl MessageSyncCoordinatorRegistry {
    pub(crate) fn for_owner(&self, owner_identity_id: &str) -> Arc<MessageSyncCoordinator> {
        let mut coordinators = self
            .coordinators
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        coordinators.retain(|_, coordinator| coordinator.strong_count() > 0);
        if let Some(coordinator) = coordinators.get(owner_identity_id).and_then(Weak::upgrade) {
            return coordinator;
        }
        let coordinator = Arc::new(MessageSyncCoordinator::default());
        coordinators.insert(owner_identity_id.to_owned(), Arc::downgrade(&coordinator));
        coordinator
    }
}

#[derive(Default)]
pub(crate) struct MessageSyncCoordinator {
    state: Mutex<MessageSyncCoordinatorState>,
}

#[derive(Default)]
struct MessageSyncCoordinatorState {
    last_started_run_id: u64,
    active_run_id: Option<u64>,
    pending: Option<PendingSyncRun>,
    waiters: Vec<SyncWaiter>,
}

struct PendingSyncRun {
    request: crate::messages::MessageSyncRequest,
    executor: MessageSyncExecutor,
}

struct SyncWaiter {
    target_run_id: u64,
    sender: oneshot::Sender<crate::ImResult<crate::messages::MessageSyncOutcome>>,
}

pub(crate) struct MessageSyncRegistration {
    pub(crate) receiver: oneshot::Receiver<crate::ImResult<crate::messages::MessageSyncOutcome>>,
    pub(crate) leader: Option<MessageSyncLeaderRun>,
}

pub(crate) struct MessageSyncLeaderRun {
    run_id: u64,
    request: crate::messages::MessageSyncRequest,
    executor: MessageSyncExecutor,
}

impl MessageSyncCoordinator {
    pub(crate) fn register(
        &self,
        request: crate::messages::MessageSyncRequest,
        kind: MessageSyncRequestKind,
        executor: MessageSyncExecutor,
    ) -> crate::ImResult<MessageSyncRegistration> {
        let (sender, receiver) = oneshot::channel();
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        let (target_run_id, leader) = match state.active_run_id {
            None => {
                let run_id = next_run_id(state.last_started_run_id)?;
                state.last_started_run_id = run_id;
                state.active_run_id = Some(run_id);
                (
                    run_id,
                    Some(MessageSyncLeaderRun {
                        run_id,
                        request,
                        executor,
                    }),
                )
            }
            Some(active_run_id) => match kind {
                MessageSyncRequestKind::EnsureCurrent => {
                    let target_run_id = if state.pending.is_some() {
                        next_run_id(active_run_id)?
                    } else {
                        active_run_id
                    };
                    (target_run_id, None)
                }
                MessageSyncRequestKind::DirtyAfterCurrent => {
                    let target_run_id = next_run_id(active_run_id)?;
                    if let Some(pending) = state.pending.as_mut() {
                        merge_pending_request(pending, request, executor);
                    } else {
                        state.pending = Some(PendingSyncRun { request, executor });
                    }
                    (target_run_id, None)
                }
            },
        };

        state.waiters.push(SyncWaiter {
            target_run_id,
            sender,
        });
        Ok(MessageSyncRegistration { receiver, leader })
    }

    pub(crate) async fn execute(
        self: &Arc<Self>,
        request: crate::messages::MessageSyncRequest,
        kind: MessageSyncRequestKind,
        executor: MessageSyncExecutor,
    ) -> crate::ImResult<crate::messages::MessageSyncOutcome> {
        let registration = self.register(request, kind, executor)?;
        if let Some(leader) = registration.leader {
            let coordinator = Arc::clone(self);
            tokio::spawn(async move {
                coordinator.run(leader).await;
            });
        }
        registration
            .receiver
            .await
            .map_err(|_| crate::ImError::LocalStateUnavailable {
                detail: "message sync coordinator ended before publishing the shared result"
                    .to_owned(),
            })?
    }

    pub(crate) async fn run(self: &Arc<Self>, mut leader: MessageSyncLeaderRun) {
        let mut completion_guard = CoordinatorRunCompletionGuard::new(Arc::clone(self));
        loop {
            let result = (leader.executor)(leader.request).await;
            let next = self.complete_run(leader.run_id, result);
            match next {
                Some(next) => leader = next,
                None => break,
            }
        }
        completion_guard.disarm();
    }

    fn complete_run(
        &self,
        run_id: u64,
        result: crate::ImResult<crate::messages::MessageSyncOutcome>,
    ) -> Option<MessageSyncLeaderRun> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.active_run_id != Some(run_id) {
            fail_all_waiters(
                &mut state,
                crate::ImError::Internal {
                    message: "message sync coordinator active run changed unexpectedly".to_owned(),
                },
            );
            return None;
        }

        publish_waiters_through(&mut state, run_id, &result);
        if shared_result_succeeded(&result) {
            if let Some(pending) = state.pending.take() {
                let next_run_id = match next_run_id(run_id) {
                    Ok(next_run_id) => next_run_id,
                    Err(error) => {
                        fail_all_waiters(&mut state, error);
                        return None;
                    }
                };
                state.last_started_run_id = next_run_id;
                state.active_run_id = Some(next_run_id);
                return Some(MessageSyncLeaderRun {
                    run_id: next_run_id,
                    request: pending.request,
                    executor: pending.executor,
                });
            }
            state.active_run_id = None;
            if !state.waiters.is_empty() {
                fail_all_waiters(
                    &mut state,
                    crate::ImError::Internal {
                        message: "message sync coordinator left uncovered waiters".to_owned(),
                    },
                );
            }
            return None;
        }

        state.pending = None;
        publish_all_waiters(&mut state, &result);
        state.active_run_id = None;
        None
    }

    fn fail_unfinished_run(&self) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.active_run_id.is_none() && state.waiters.is_empty() {
            return;
        }
        fail_all_waiters(
            &mut state,
            crate::ImError::LocalStateUnavailable {
                detail: "message sync coordinator task stopped before completion".to_owned(),
            },
        );
    }
}

struct CoordinatorRunCompletionGuard {
    coordinator: Arc<MessageSyncCoordinator>,
    armed: bool,
}

impl CoordinatorRunCompletionGuard {
    fn new(coordinator: Arc<MessageSyncCoordinator>) -> Self {
        Self {
            coordinator,
            armed: true,
        }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for CoordinatorRunCompletionGuard {
    fn drop(&mut self) {
        if self.armed {
            self.coordinator.fail_unfinished_run();
        }
    }
}

fn next_run_id(current: u64) -> crate::ImResult<u64> {
    current
        .checked_add(1)
        .ok_or_else(|| crate::ImError::Internal {
            message: "message sync coordinator run id exhausted".to_owned(),
        })
}

fn merge_pending_request(
    pending: &mut PendingSyncRun,
    request: crate::messages::MessageSyncRequest,
    executor: MessageSyncExecutor,
) {
    pending.request.reason = request.reason;
    pending.request.limit = match (pending.request.limit, request.limit) {
        (Some(left), Some(right)) => Some(left.max(right)),
        (left, right) => left.or(right),
    };
    pending.executor = executor;
}

fn shared_result_succeeded(result: &crate::ImResult<crate::messages::MessageSyncOutcome>) -> bool {
    result.as_ref().is_ok_and(|outcome| {
        matches!(
            outcome.status,
            crate::messages::MessageSyncStatus::Idle | crate::messages::MessageSyncStatus::Changed
        )
    })
}

fn publish_waiters_through(
    state: &mut MessageSyncCoordinatorState,
    run_id: u64,
    result: &crate::ImResult<crate::messages::MessageSyncOutcome>,
) {
    let mut pending = Vec::new();
    for waiter in state.waiters.drain(..) {
        if waiter.target_run_id <= run_id {
            let _ = waiter.sender.send(result.clone());
        } else {
            pending.push(waiter);
        }
    }
    state.waiters = pending;
}

fn publish_all_waiters(
    state: &mut MessageSyncCoordinatorState,
    result: &crate::ImResult<crate::messages::MessageSyncOutcome>,
) {
    for waiter in state.waiters.drain(..) {
        let _ = waiter.sender.send(result.clone());
    }
}

fn fail_all_waiters(state: &mut MessageSyncCoordinatorState, error: crate::ImError) {
    state.pending = None;
    state.active_run_id = None;
    let result = Err(error);
    publish_all_waiters(state, &result);
}
