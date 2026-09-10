//! Process-local receive-queue dispatcher, shared by every Core for one DB.
//! SQLite claims fence other processes; no receiver waits for a business task.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::Duration;

use tokio::sync::{broadcast, Notify};
use tokio::task::JoinSet;

use crate::internal::local_state::sync_inbox::{self, InputClaim, RemovedInput};
use crate::messages::{MessageProcessingStatus, MessageProcessingUpdate};

const UPDATE_BUFFER: usize = 512;
const MAINTENANCE_INTERVAL: Duration = Duration::from_secs(5);
const PROCESSING_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Debug, Clone)]
pub(crate) struct OwnedUpdate {
    pub(crate) owner_identity_id: String,
    pub(crate) update: MessageProcessingUpdate,
}

#[derive(Default)]
struct DispatchState {
    running: bool,
    run_epoch: u64,
    wake_generation: u64,
    next_owner: usize,
    clients: BTreeMap<String, crate::core::ImClient>,
}

pub(crate) struct SyncDispatcher {
    state: Mutex<DispatchState>,
    wake: Notify,
    updates: broadcast::Sender<OwnedUpdate>,
    #[cfg(test)]
    test_executor: Mutex<Option<TestExecutor>>,
    #[cfg(test)]
    test_timeout: Mutex<Option<Duration>>,
}

#[cfg(test)]
type TestExecutor = Arc<
    dyn Fn(
            crate::core::ImClient,
            InputClaim,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = crate::ImResult<MessageProcessingUpdate>> + Send>,
        > + Send
        + Sync,
>;

struct RunGuard {
    dispatcher: Arc<SyncDispatcher>,
    epoch: u64,
}

impl Drop for RunGuard {
    fn drop(&mut self) {
        let mut state = self
            .dispatcher
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.run_epoch == self.epoch {
            state.running = false;
        }
    }
}

pub(crate) fn for_client(client: &crate::core::ImClient) -> Arc<SyncDispatcher> {
    static REGISTRY: OnceLock<Mutex<HashMap<String, Weak<SyncDispatcher>>>> = OnceLock::new();
    let key = crate::internal::identity_transition_pending::state_root_fingerprint(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    );
    let mut registry = REGISTRY
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    registry.retain(|_, value| value.strong_count() > 0);
    if let Some(dispatcher) = registry.get(&key).and_then(Weak::upgrade) {
        return dispatcher;
    }
    let (updates, _) = broadcast::channel(UPDATE_BUFFER);
    let dispatcher = Arc::new(SyncDispatcher {
        state: Mutex::new(DispatchState::default()),
        wake: Notify::new(),
        updates,
        #[cfg(test)]
        test_executor: Mutex::new(None),
        #[cfg(test)]
        test_timeout: Mutex::new(None),
    });
    registry.insert(key, Arc::downgrade(&dispatcher));
    dispatcher
}

pub(crate) fn wake_client(client: &crate::core::ImClient) {
    for_client(client).wake_client(client.clone());
}

pub(crate) fn publish_removed(client: &crate::core::ImClient, removed: &[RemovedInput]) {
    let dispatcher = for_client(client);
    dispatcher.publish_removed(removed);
}

impl SyncDispatcher {
    pub(crate) fn subscribe(&self) -> broadcast::Receiver<OwnedUpdate> {
        self.updates.subscribe()
    }

    pub(crate) fn notify(&self) {
        self.wake.notify_one();
    }

    #[cfg(test)]
    pub(crate) fn set_executor_for_test(&self, executor: TestExecutor, timeout: Duration) {
        *self.test_executor.lock().unwrap() = Some(executor);
        *self.test_timeout.lock().unwrap() = Some(timeout);
    }

    #[cfg(test)]
    pub(crate) fn running_for_test(&self) -> bool {
        self.state.lock().unwrap().running
    }

    pub(crate) fn wake_client(self: &Arc<Self>, client: crate::core::ImClient) {
        let start_epoch = {
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            state
                .clients
                .insert(client.current_identity().id.as_str().to_owned(), client);
            state.wake_generation = state.wake_generation.wrapping_add(1);
            let start = !state.running;
            state.running = true;
            if start {
                state.run_epoch = state.run_epoch.wrapping_add(1);
                Some(state.run_epoch)
            } else {
                None
            }
        };
        self.wake.notify_one();
        if let Some(epoch) = start_epoch {
            let dispatcher = Arc::clone(self);
            tokio::spawn(async move {
                dispatcher.run(epoch).await;
            });
        }
    }

    fn publish(&self, owner_identity_id: String, update: MessageProcessingUpdate) {
        let _ = self.updates.send(OwnedUpdate {
            owner_identity_id,
            update,
        });
    }

    fn publish_removed(&self, removed: &[RemovedInput]) {
        for input in removed {
            self.publish(
                input.owner_identity_id.clone(),
                MessageProcessingUpdate {
                    event_id: input.event_id.clone(),
                    status: MessageProcessingStatus::Discarded,
                    changed_conversation_ids: Vec::new(),
                    committed_incoming_messages: Vec::new(),
                    error_code: Some("sync.input_discarded".into()),
                },
            );
        }
    }

    async fn run(self: Arc<Self>, epoch: u64) {
        let _guard = RunGuard {
            dispatcher: Arc::clone(&self),
            epoch,
        };
        let mut jobs = JoinSet::new();
        let mut active = HashMap::<String, InputClaim>::new();
        let mut task_tokens = HashMap::new();
        let mut maintenance = tokio::time::interval(MAINTENANCE_INTERVAL);
        maintenance.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            let (clients, generation) = {
                let mut state = self
                    .state
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let mut clients = state.clients.values().cloned().collect::<Vec<_>>();
                if !clients.is_empty() {
                    let offset = state.next_owner % clients.len();
                    clients.rotate_left(offset);
                    state.next_owner = state.next_owner.wrapping_add(1);
                }
                (clients, state.wake_generation)
            };
            let mut retained = false;
            for client in &clients {
                let Ok(db) = client.core_inner().local_state_db().await else {
                    continue;
                };
                if active.is_empty() {
                    if let Ok(removed) = db
                        .run_local(|connection| {
                            sync_inbox::purge_expired(
                                connection,
                                super::sync_processing::now(),
                                256,
                            )
                        })
                        .await
                    {
                        self.publish_removed(&removed);
                    }
                }
                let owner = client.current_identity().id.as_str().to_owned();
                if let Ok(summary) = db
                    .run_local(move |connection| {
                        sync_inbox::processing_summary(
                            connection,
                            &owner,
                            super::sync_processing::now(),
                        )
                    })
                    .await
                {
                    retained |= summary.pending > 0;
                }
                if active.len() >= sync_inbox::MAX_ACTIVE_INPUTS {
                    continue;
                }
                let Ok(binding) = client.active_sync_account_binding().await else {
                    continue;
                };
                let binding = super::sync_v2::stored_binding(client, &binding);
                let limit = (sync_inbox::MAX_ACTIVE_INPUTS - active.len()) as u32;
                let claims = match db
                    .run_local(move |connection| {
                        sync_inbox::claim_inputs(
                            connection,
                            &binding,
                            super::sync_processing::now(),
                            limit,
                        )
                    })
                    .await
                {
                    Ok(claims) => claims,
                    Err(_) => continue,
                };
                for claim in claims {
                    active.insert(claim.token.clone(), claim.clone());
                    let client = client.clone();
                    let dispatcher = Arc::clone(&self);
                    let task_token = claim.token.clone();
                    let task = jobs.spawn(async move {
                        let token = claim.token.clone();
                        dispatcher.run_claim(client, claim).await;
                        token
                    });
                    task_tokens.insert(task.id(), task_token);
                }
            }
            if active.is_empty() && !retained && self.updates.receiver_count() == 0 {
                let mut state = self
                    .state
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if state.wake_generation == generation {
                    state.running = false;
                    state.clients.clear();
                    return;
                }
                continue;
            }
            tokio::select! {
                _ = self.wake.notified() => {},
                finished = jobs.join_next(), if !jobs.is_empty() => {
                    match finished {
                        Some(Ok(token)) => {
                            active.remove(&token);
                            task_tokens.retain(|_, value| value != &token);
                        }
                        Some(Err(error)) => {
                            // A panicked worker cannot retain an in-memory slot forever.
                            // Its SQLite lease still fences writers until restart/reclaim.
                            if let Some(token) = task_tokens.remove(&error.id()) {
                                active.remove(&token);
                            }
                        }
                        None => {},
                    }
                },
                _ = maintenance.tick() => {
                    if let Some(client) = clients.first() {
                        if let Ok(db) = client.core_inner().local_state_db().await {
                            let claims = active.values().cloned().collect::<Vec<_>>();
                            let _ = db.run_local(move |connection| sync_inbox::renew_claims(connection, &claims, super::sync_processing::now())).await;
                            if let Ok(removed) = db.run_local(|connection| sync_inbox::purge_expired(connection, super::sync_processing::now(), 256)).await {
                                self.publish_removed(&removed);
                            }
                        }
                    }
                },
            }
        }
    }

    async fn run_claim(&self, client: crate::core::ImClient, claim: InputClaim) {
        let owner = claim.owner_identity_id.clone();
        let Ok(db) = client.core_inner().local_state_db().await else {
            return;
        };
        let mut work = Box::pin(async {
            #[cfg(test)]
            {
                let executor = { self.test_executor.lock().unwrap().clone() };
                if let Some(executor) = executor {
                    return executor(client.clone(), claim.clone()).await;
                }
            }
            if claim.lane == "ordinary" {
                super::sync_processing::process_ordinary(
                    &client,
                    &claim,
                    &mut crate::internal::transport::CoreHttpTransport::new(&client),
                )
                .await
            } else {
                super::sync_v2::process_secure_claim(&client, &claim).await
            }
        });
        #[cfg(test)]
        let timeout = self
            .test_timeout
            .lock()
            .unwrap()
            .unwrap_or(PROCESSING_TIMEOUT);
        #[cfg(not(test))]
        let timeout = PROCESSING_TIMEOUT;
        let result = match tokio::time::timeout(timeout, &mut work).await {
            Ok(result) => result,
            Err(_) => {
                let parked = claim.clone();
                let parked = db
                    .run_local(move |connection| {
                        sync_inbox::park_timeout(connection, &parked, super::sync_processing::now())
                    })
                    .await
                    .unwrap_or(false);
                if parked {
                    self.publish(
                        owner.clone(),
                        MessageProcessingUpdate {
                            event_id: claim.event_id.clone(),
                            status: MessageProcessingStatus::Retrying,
                            changed_conversation_ids: Vec::new(),
                            committed_incoming_messages: Vec::new(),
                            error_code: Some("sync.processing_timeout".into()),
                        },
                    );
                }
                // Keep this job and its activity slot until the underlying future
                // ends. A timeout alone never authorizes a replacement worker.
                let late_result = work.await;
                if !parked {
                    late_result
                } else {
                    let finished = claim.clone();
                    let _ = db
                        .run_local(move |connection| {
                            sync_inbox::release_timeout(
                                connection,
                                &finished,
                                super::sync_processing::now() + 2,
                            )
                        })
                        .await;
                    Err(sync_inbox::error(
                        "sync.processing_timeout",
                        "business processing exceeded its time budget",
                    ))
                }
            }
        };
        match result {
            Ok(update) => self.publish(owner, update),
            Err(error) => {
                let code = super::sync_processing::failure_code(&error);
                let retry_at = super::sync_processing::retry_at(&error, claim.attempt_count);
                let failed = claim.clone();
                let stored_code = code.clone();
                let recorded = db
                    .run_local(move |connection| {
                        sync_inbox::fail_claim(
                            connection,
                            &failed,
                            &stored_code,
                            retry_at,
                            super::sync_processing::now(),
                        )
                    })
                    .await
                    .is_ok();
                let discarded = !recorded && code != "sync.processing_timeout";
                self.publish(
                    owner,
                    MessageProcessingUpdate {
                        event_id: claim.event_id.clone(),
                        status: if discarded {
                            MessageProcessingStatus::Discarded
                        } else if retry_at.is_some() {
                            MessageProcessingStatus::Retrying
                        } else {
                            MessageProcessingStatus::Blocked
                        },
                        changed_conversation_ids: Vec::new(),
                        committed_incoming_messages: Vec::new(),
                        error_code: Some(if discarded {
                            "sync.input_discarded".into()
                        } else {
                            code
                        }),
                    },
                );
            }
        }
    }
}
