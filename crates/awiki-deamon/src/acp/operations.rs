//! Admission/configuration locks. Never held while a model executes or while
//! answering/stopping an active task; unrelated scopes remain independent.
use crate::DaemonState;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        Arc, LockResult, Mutex, MutexGuard, OnceLock, Weak,
    },
};

type Gates = HashMap<(PathBuf, String), Weak<SessionGate>>;
static GATES: OnceLock<Mutex<Gates>> = OnceLock::new();

#[derive(Default)]
pub struct SessionGate {
    serial: Mutex<()>,
    refresh_cancel: Mutex<Option<Arc<AtomicBool>>>,
    priority_waiters: AtomicUsize,
    refresh_generation: AtomicU64,
    refresh_succeeded: AtomicBool,
}

impl SessionGate {
    /// Admission and configuration preempt metadata work, but still wait for
    /// its ACP connection/process group to be dropped before using the session.
    pub fn lock(&self) -> LockResult<MutexGuard<'_, ()>> {
        self.priority_waiters.fetch_add(1, Ordering::SeqCst);
        if let Some(cancel) = self.refresh_cancel.lock().unwrap().as_ref() {
            cancel.store(true, Ordering::SeqCst);
        }
        let guard = self.serial.lock();
        self.priority_waiters.fetch_sub(1, Ordering::SeqCst);
        guard
    }

    pub fn refresh(&self) -> anyhow::Result<RefreshGuard<'_>> {
        let generation = self.refresh_generation.load(Ordering::SeqCst);
        let serial = self
            .serial
            .lock()
            .map_err(|_| anyhow::anyhow!("acp_configuration_interrupted"))?;
        if self.priority_waiters.load(Ordering::SeqCst) != 0 {
            anyhow::bail!("model_refresh_deferred");
        }
        let coalesced = generation != self.refresh_generation.load(Ordering::SeqCst);
        let cancel = Arc::new(AtomicBool::new(false));
        *self.refresh_cancel.lock().unwrap() = Some(cancel.clone());
        // Close the race with a priority caller just before registration.
        if self.priority_waiters.load(Ordering::SeqCst) != 0 {
            cancel.store(true, Ordering::SeqCst);
        }
        Ok(RefreshGuard {
            gate: self,
            _serial: serial,
            cancel,
            coalesced,
            succeeded: false,
        })
    }
}

pub struct RefreshGuard<'a> {
    gate: &'a SessionGate,
    _serial: MutexGuard<'a, ()>,
    pub cancel: Arc<AtomicBool>,
    pub coalesced: bool,
    succeeded: bool,
}

impl RefreshGuard<'_> {
    pub fn prior_succeeded(&self) -> bool {
        self.gate.refresh_succeeded.load(Ordering::SeqCst)
    }
    pub fn mark_success(&mut self) {
        self.succeeded = true;
    }
}

impl Drop for RefreshGuard<'_> {
    fn drop(&mut self) {
        *self.gate.refresh_cancel.lock().unwrap() = None;
        if !self.coalesced {
            self.gate
                .refresh_succeeded
                .store(self.succeeded, Ordering::SeqCst);
            self.gate.refresh_generation.fetch_add(1, Ordering::SeqCst);
        }
    }
}

pub fn session_gate(state: &DaemonState, key: &str) -> Arc<SessionGate> {
    let mut gates = GATES.get_or_init(Default::default).lock().unwrap();
    if gates.len() > 128 {
        gates.retain(|_, value| value.strong_count() > 0);
    }
    let entry = gates
        .entry((state.database_path().to_owned(), key.to_owned()))
        .or_default();
    if let Some(gate) = entry.upgrade() {
        return gate;
    }
    let gate = Arc::new(SessionGate::default());
    *entry = Arc::downgrade(&gate);
    gate
}
