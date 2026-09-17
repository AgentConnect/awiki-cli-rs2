//! Admission/configuration locks. Never held while a model executes or while
//! answering/stopping an active task; unrelated scopes remain independent.
use crate::DaemonState;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex, OnceLock, Weak},
};

type Gates = HashMap<(PathBuf, String), Weak<Mutex<()>>>;
static GATES: OnceLock<Mutex<Gates>> = OnceLock::new();

pub fn session_gate(state: &DaemonState, key: &str) -> Arc<Mutex<()>> {
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
    let gate = Arc::new(Mutex::new(()));
    *entry = Arc::downgrade(&gate);
    gate
}
