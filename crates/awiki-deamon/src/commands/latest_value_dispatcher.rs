use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};

struct LatestValueState<T> {
    pending: Option<T>,
    closed: bool,
}

/// Keeps slow side effects off a hot producer path and retains only the newest
/// value that has not started delivery yet.
pub(super) struct LatestValueDispatcher<T: Send + 'static> {
    shared: Arc<(Mutex<LatestValueState<T>>, Condvar)>,
    worker: Option<JoinHandle<()>>,
}

impl<T> LatestValueDispatcher<T>
where
    T: Send + 'static,
{
    pub(super) fn spawn<F>(thread_name: &str, mut deliver: F) -> std::io::Result<Self>
    where
        F: FnMut(T) + Send + 'static,
    {
        let shared = Arc::new((
            Mutex::new(LatestValueState {
                pending: None,
                closed: false,
            }),
            Condvar::new(),
        ));
        let worker_shared = Arc::clone(&shared);
        let worker = thread::Builder::new()
            .name(thread_name.to_string())
            .spawn(move || loop {
                let next = {
                    let (lock, ready) = &*worker_shared;
                    let mut state = lock.lock().expect("latest value dispatcher poisoned");
                    while state.pending.is_none() && !state.closed {
                        state = ready
                            .wait(state)
                            .expect("latest value dispatcher poisoned while waiting");
                    }
                    if state.closed {
                        None
                    } else {
                        state.pending.take()
                    }
                };
                let Some(next) = next else {
                    break;
                };
                deliver(next);
            })?;
        Ok(Self {
            shared,
            worker: Some(worker),
        })
    }

    pub(super) fn publish(&self, value: T) {
        let (lock, ready) = &*self.shared;
        let mut state = lock.lock().expect("latest value dispatcher poisoned");
        if state.closed {
            return;
        }
        state.pending = Some(value);
        ready.notify_one();
    }

    /// Drops queued stale progress and waits only for delivery that has
    /// already started, so no progress can arrive after a terminal result.
    pub(super) fn close(mut self) {
        self.close_inner();
    }

    fn close_inner(&mut self) {
        let (lock, ready) = &*self.shared;
        {
            let mut state = lock.lock().expect("latest value dispatcher poisoned");
            state.closed = true;
            state.pending = None;
            ready.notify_all();
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl<T: Send + 'static> Drop for LatestValueDispatcher<T> {
    fn drop(&mut self) {
        self.close_inner();
    }
}

#[cfg(test)]
#[path = "latest_value_dispatcher_tests.rs"]
mod tests;
