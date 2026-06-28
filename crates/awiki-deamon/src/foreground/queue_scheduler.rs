use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::time::Duration;

use anyhow::Result;
use serde_json::json;
use tokio::sync::Notify;
use tokio::task::JoinHandle;

use super::{
    drain_cli_route_message_queue_once_limited, drain_runtime_retry_queue_once_limited,
    flush_message_sync_outbox, flush_runtime_final_outbox, sanitize_error_message,
    RuntimeCallbackOutbox, StdioHermesGateway,
};
use crate::{DaemonConfig, DaemonState, ImCoreAdapter};

const MESSAGE_SYNC_OUTBOX_BATCH_LIMIT: usize = 20;
const RUNTIME_FINAL_OUTBOX_BATCH_LIMIT: usize = 20;
const CLI_ROUTE_MESSAGE_QUEUE_BATCH_LIMIT: usize = 10;
const RUNTIME_RETRY_QUEUE_BATCH_LIMIT: usize = 10;
const DEFAULT_RECONCILIATION_INTERVAL: Duration = Duration::from_secs(30);
const DUE_RECHECK_INTERVAL: Duration = Duration::from_millis(50);
const STOP_JOIN_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum QueueKind {
    MessageSyncOutbox,
    RuntimeFinalOutbox,
    CliRouteMessageQueue,
    RuntimeRetryQueue,
}

impl QueueKind {
    fn audit_name(self) -> &'static str {
        match self {
            Self::MessageSyncOutbox => "message_sync_outbox.scheduler.failed",
            Self::RuntimeFinalOutbox => "runtime.final_outbox.scheduler.failed",
            Self::CliRouteMessageQueue => "cli_route_message_queue.scheduler.failed",
            Self::RuntimeRetryQueue => "runtime.run.retry.scheduler.failed",
        }
    }
}

#[derive(Clone)]
pub(super) struct QueueSchedulerNotifier {
    inner: Arc<QueueSchedulerNotifyInner>,
}

struct QueueSchedulerNotifyInner {
    message_sync_outbox: Notify,
    runtime_final_outbox: Notify,
    cli_route_message_queue: Notify,
    runtime_retry_queue: Notify,
}

impl QueueSchedulerNotifier {
    pub(super) fn new() -> Self {
        Self {
            inner: Arc::new(QueueSchedulerNotifyInner {
                message_sync_outbox: Notify::new(),
                runtime_final_outbox: Notify::new(),
                cli_route_message_queue: Notify::new(),
                runtime_retry_queue: Notify::new(),
            }),
        }
    }

    pub(super) fn notify_ref(&self, kind: QueueKind) {
        self.notify_handle(kind).notify_one();
    }

    pub(super) fn notify_all(&self) {
        self.notify_ref(QueueKind::MessageSyncOutbox);
        self.notify_ref(QueueKind::RuntimeFinalOutbox);
        self.notify_ref(QueueKind::CliRouteMessageQueue);
        self.notify_ref(QueueKind::RuntimeRetryQueue);
    }

    fn notify_handle(&self, kind: QueueKind) -> &Notify {
        match kind {
            QueueKind::MessageSyncOutbox => &self.inner.message_sync_outbox,
            QueueKind::RuntimeFinalOutbox => &self.inner.runtime_final_outbox,
            QueueKind::CliRouteMessageQueue => &self.inner.cli_route_message_queue,
            QueueKind::RuntimeRetryQueue => &self.inner.runtime_retry_queue,
        }
    }
}

pub(super) struct QueueScheduler {
    stop: Arc<AtomicBool>,
    notifier: QueueSchedulerNotifier,
    tasks: Vec<JoinHandle<()>>,
}

impl QueueScheduler {
    pub(super) fn start(
        config: DaemonConfig,
        state: DaemonState,
        im_core: ImCoreAdapter,
        rpc_outbox: Arc<Mutex<RuntimeCallbackOutbox>>,
        hermes_gateway: StdioHermesGateway,
        notifier: QueueSchedulerNotifier,
    ) -> Self {
        Self::start_with_reconciliation_interval(
            config,
            state,
            im_core,
            rpc_outbox,
            hermes_gateway,
            notifier,
            DEFAULT_RECONCILIATION_INTERVAL,
        )
    }

    fn start_with_reconciliation_interval(
        config: DaemonConfig,
        state: DaemonState,
        im_core: ImCoreAdapter,
        rpc_outbox: Arc<Mutex<RuntimeCallbackOutbox>>,
        hermes_gateway: StdioHermesGateway,
        notifier: QueueSchedulerNotifier,
        reconciliation_interval: Duration,
    ) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let tasks = vec![
            spawn_message_sync_outbox_scheduler(
                config.clone(),
                state.clone(),
                im_core.clone(),
                Arc::clone(&stop),
                notifier.clone(),
                reconciliation_interval,
            ),
            spawn_runtime_final_outbox_scheduler(
                state.clone(),
                Arc::clone(&rpc_outbox),
                Arc::clone(&stop),
                notifier.clone(),
                reconciliation_interval,
            ),
            spawn_cli_route_message_queue_scheduler(
                config.clone(),
                state.clone(),
                Arc::clone(&rpc_outbox),
                Arc::clone(&stop),
                notifier.clone(),
                reconciliation_interval,
            ),
            spawn_runtime_retry_queue_scheduler(
                config,
                state,
                rpc_outbox,
                hermes_gateway,
                stop.clone(),
                notifier.clone(),
                reconciliation_interval,
            ),
        ];
        notifier.notify_all();
        Self {
            stop,
            notifier,
            tasks,
        }
    }

    pub(super) async fn stop(mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.notifier.notify_all();
        for task in self.tasks.drain(..) {
            let abort_handle = task.abort_handle();
            if tokio::time::timeout(STOP_JOIN_TIMEOUT, task).await.is_err() {
                abort_handle.abort();
            }
        }
    }
}

impl Drop for QueueScheduler {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.notifier.notify_all();
        for task in &self.tasks {
            task.abort();
        }
    }
}

fn spawn_message_sync_outbox_scheduler(
    config: DaemonConfig,
    state: DaemonState,
    im_core: ImCoreAdapter,
    stop: Arc<AtomicBool>,
    notifier: QueueSchedulerNotifier,
    reconciliation_interval: Duration,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let scheduler_state = state.clone();
        let drain_state = state.clone();
        run_queue_scheduler(
            QueueKind::MessageSyncOutbox,
            scheduler_state,
            stop,
            notifier,
            reconciliation_interval,
            move || {
                flush_message_sync_outbox(
                    &config,
                    &drain_state,
                    &im_core,
                    MESSAGE_SYNC_OUTBOX_BATCH_LIMIT,
                )
            },
            |state| state.next_pending_message_sync_outbox_due_ms(),
        )
        .await;
    })
}

fn spawn_runtime_final_outbox_scheduler(
    state: DaemonState,
    rpc_outbox: Arc<Mutex<RuntimeCallbackOutbox>>,
    stop: Arc<AtomicBool>,
    notifier: QueueSchedulerNotifier,
    reconciliation_interval: Duration,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let scheduler_state = state.clone();
        let drain_state = state.clone();
        run_queue_scheduler(
            QueueKind::RuntimeFinalOutbox,
            scheduler_state,
            stop,
            notifier,
            reconciliation_interval,
            move || {
                let outbox = rpc_outbox
                    .lock()
                    .map_err(|_| anyhow::anyhow!("runtime callback outbox lock poisoned"))?;
                flush_runtime_final_outbox(&drain_state, &*outbox, RUNTIME_FINAL_OUTBOX_BATCH_LIMIT)
            },
            |state| state.next_pending_runtime_final_outbox_due_ms(),
        )
        .await;
    })
}

fn spawn_cli_route_message_queue_scheduler(
    config: DaemonConfig,
    state: DaemonState,
    rpc_outbox: Arc<Mutex<RuntimeCallbackOutbox>>,
    stop: Arc<AtomicBool>,
    notifier: QueueSchedulerNotifier,
    reconciliation_interval: Duration,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let scheduler_state = state.clone();
        let drain_state = state.clone();
        run_queue_scheduler(
            QueueKind::CliRouteMessageQueue,
            scheduler_state,
            stop,
            notifier,
            reconciliation_interval,
            move || {
                let outbox = rpc_outbox
                    .lock()
                    .map_err(|_| anyhow::anyhow!("runtime callback outbox lock poisoned"))?;
                drain_cli_route_message_queue_once_limited(
                    &config,
                    &drain_state,
                    &*outbox,
                    CLI_ROUTE_MESSAGE_QUEUE_BATCH_LIMIT,
                )
            },
            |state| state.next_queued_cli_route_message_queue_due_ms(),
        )
        .await;
    })
}

fn spawn_runtime_retry_queue_scheduler(
    config: DaemonConfig,
    state: DaemonState,
    rpc_outbox: Arc<Mutex<RuntimeCallbackOutbox>>,
    hermes_gateway: StdioHermesGateway,
    stop: Arc<AtomicBool>,
    notifier: QueueSchedulerNotifier,
    reconciliation_interval: Duration,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let scheduler_state = state.clone();
        let drain_state = state.clone();
        run_queue_scheduler(
            QueueKind::RuntimeRetryQueue,
            scheduler_state,
            stop,
            notifier,
            reconciliation_interval,
            move || {
                let outbox = rpc_outbox
                    .lock()
                    .map_err(|_| anyhow::anyhow!("runtime callback outbox lock poisoned"))?;
                drain_runtime_retry_queue_once_limited(
                    &config,
                    &drain_state,
                    &*outbox,
                    &hermes_gateway,
                    RUNTIME_RETRY_QUEUE_BATCH_LIMIT,
                )
            },
            |state| state.next_queued_runtime_retry_due_ms(),
        )
        .await;
    })
}

async fn run_queue_scheduler<D, N>(
    kind: QueueKind,
    state: DaemonState,
    stop: Arc<AtomicBool>,
    notifier: QueueSchedulerNotifier,
    reconciliation_interval: Duration,
    mut drain: D,
    next_due: N,
) where
    D: FnMut() -> Result<usize> + Send + 'static,
    N: Fn(&DaemonState) -> Result<Option<i64>> + Send + 'static,
{
    loop {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        match drain() {
            Ok(processed) if processed > 0 => {
                continue;
            }
            Ok(_) => {}
            Err(error) => {
                record_scheduler_error(&state, kind, &error);
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
        let next_due_ms = match next_due(&state) {
            Ok(value) => value,
            Err(error) => {
                record_scheduler_error(&state, kind, &error);
                tokio::time::sleep(Duration::from_millis(100)).await;
                None
            }
        };
        wait_for_queue_wakeup(&stop, &notifier, kind, next_due_ms, reconciliation_interval).await;
    }
}

async fn wait_for_queue_wakeup(
    stop: &AtomicBool,
    notifier: &QueueSchedulerNotifier,
    kind: QueueKind,
    next_due_ms: Option<i64>,
    reconciliation_interval: Duration,
) {
    let notified = notifier.notify_handle(kind).notified();
    tokio::pin!(notified);
    let reconciliation = tokio::time::sleep(reconciliation_interval);
    tokio::pin!(reconciliation);
    match next_due_ms {
        Some(due_ms) => {
            let due_wait = duration_until_ms(due_ms);
            let due_timer = tokio::time::sleep(due_wait);
            tokio::pin!(due_timer);
            tokio::select! {
                _ = &mut notified => {}
                _ = &mut due_timer => {}
                _ = &mut reconciliation => {}
            }
        }
        None => {
            tokio::select! {
                _ = &mut notified => {}
                _ = &mut reconciliation => {}
            }
        }
    }
    if stop.load(Ordering::Relaxed) {
        notifier.notify_ref(kind);
    }
}

fn duration_until_ms(due_ms: i64) -> Duration {
    let now = crate::security::runtime_token::current_time_millis().unwrap_or(0);
    if due_ms <= now {
        DUE_RECHECK_INTERVAL
    } else {
        Duration::from_millis((due_ms - now) as u64)
    }
}

fn record_scheduler_error(state: &DaemonState, kind: QueueKind, error: &anyhow::Error) {
    if let Err(audit_error) = state.insert_audit_event_json(
        kind.audit_name(),
        None,
        None,
        None,
        None,
        json!({
            "error": sanitize_error_message(&error.to_string()),
        }),
    ) {
        eprintln!(
            "warning: daemon queue scheduler audit failed: {}",
            sanitize_error_message(&audit_error.to_string())
        );
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    use super::*;

    #[tokio::test]
    async fn queue_scheduler_notify_runs_without_waiting_for_reconciliation() {
        let state = test_state();
        let stop = Arc::new(AtomicBool::new(false));
        let notifier = QueueSchedulerNotifier::new();
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_for_drain = Arc::clone(&calls);
        let handle = tokio::spawn(run_queue_scheduler(
            QueueKind::MessageSyncOutbox,
            state,
            Arc::clone(&stop),
            notifier.clone(),
            Duration::from_secs(60),
            move || {
                calls_for_drain.fetch_add(1, Ordering::SeqCst);
                Ok(0)
            },
            |_| Ok(None),
        ));
        tokio::task::yield_now().await;
        let after_startup = calls.load(Ordering::SeqCst);

        notifier.notify_ref(QueueKind::MessageSyncOutbox);
        tokio::task::yield_now().await;

        assert!(calls.load(Ordering::SeqCst) > after_startup);
        stop.store(true, Ordering::Relaxed);
        notifier.notify_all();
        tokio::time::timeout(Duration::from_secs(1), handle)
            .await
            .unwrap()
            .unwrap();
    }

    #[tokio::test]
    async fn queue_scheduler_waits_until_due_before_drain() {
        let state = test_state();
        let stop = Arc::new(AtomicBool::new(false));
        let notifier = QueueSchedulerNotifier::new();
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_for_drain = Arc::clone(&calls);
        let startup = crate::security::runtime_token::current_time_millis().unwrap();
        let handle = tokio::spawn(run_queue_scheduler(
            QueueKind::RuntimeRetryQueue,
            state,
            Arc::clone(&stop),
            notifier.clone(),
            Duration::from_secs(60),
            move || {
                calls_for_drain.fetch_add(1, Ordering::SeqCst);
                Ok(0)
            },
            move |_| Ok(Some(startup + 300)),
        ));
        tokio::task::yield_now().await;
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        tokio::time::sleep(Duration::from_millis(20)).await;
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        tokio::time::sleep(Duration::from_millis(350)).await;
        assert!(calls.load(Ordering::SeqCst) >= 2);
        stop.store(true, Ordering::Relaxed);
        notifier.notify_all();
        tokio::time::timeout(Duration::from_secs(1), handle)
            .await
            .unwrap()
            .unwrap();
    }

    #[tokio::test]
    async fn queue_scheduler_reconciliation_recovers_missed_notify() {
        let state = test_state();
        let stop = Arc::new(AtomicBool::new(false));
        let notifier = QueueSchedulerNotifier::new();
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_for_drain = Arc::clone(&calls);
        let handle = tokio::spawn(run_queue_scheduler(
            QueueKind::CliRouteMessageQueue,
            state,
            Arc::clone(&stop),
            notifier.clone(),
            Duration::from_millis(20),
            move || {
                calls_for_drain.fetch_add(1, Ordering::SeqCst);
                Ok(0)
            },
            |_| Ok(None),
        ));
        tokio::task::yield_now().await;
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        tokio::time::sleep(Duration::from_millis(40)).await;
        assert!(calls.load(Ordering::SeqCst) >= 2);
        stop.store(true, Ordering::Relaxed);
        notifier.notify_all();
        tokio::time::timeout(Duration::from_secs(1), handle)
            .await
            .unwrap()
            .unwrap();
    }

    #[test]
    fn queue_scheduler_notifier_can_wake_all_queues() {
        let notifier = QueueSchedulerNotifier::new();
        notifier.notify_all();
        notifier.notify_ref(QueueKind::RuntimeFinalOutbox);
    }

    #[test]
    fn queue_scheduler_due_now_uses_short_recheck_interval() {
        let now = crate::security::runtime_token::current_time_millis().unwrap();
        assert_eq!(duration_until_ms(now - 1), DUE_RECHECK_INTERVAL);
        assert_eq!(duration_until_ms(now), DUE_RECHECK_INTERVAL);
    }

    fn test_state() -> DaemonState {
        let root = tempfile::tempdir().unwrap();
        let config = DaemonConfig::for_state_root(root.path()).unwrap();
        config.ensure_state_layout().unwrap();
        let state = DaemonState::open(&config).unwrap();
        state.initialize().unwrap();
        state
    }
}
