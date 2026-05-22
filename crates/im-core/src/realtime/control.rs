use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

#[derive(Clone)]
pub struct RealtimeControl {
    closed: Arc<AtomicBool>,
}

impl RealtimeControl {
    pub(crate) fn new() -> Self {
        Self {
            closed: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn shutdown(&self) {
        self.closed.store(true, Ordering::SeqCst);
    }

    pub fn close(&self) {
        self.shutdown();
    }

    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }
}

impl Default for RealtimeControl {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for RealtimeControl {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RealtimeControl")
            .field("closed", &self.is_closed())
            .finish()
    }
}

impl PartialEq for RealtimeControl {
    fn eq(&self, other: &Self) -> bool {
        self.is_closed() == other.is_closed()
    }
}

impl Eq for RealtimeControl {}

#[derive(Clone)]
pub struct ShutdownSignal {
    requested: Arc<AtomicBool>,
}

impl ShutdownSignal {
    pub fn pending() -> Self {
        Self {
            requested: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn requested() -> Self {
        let signal = Self::pending();
        signal.request();
        signal
    }

    pub fn request(&self) {
        self.requested.store(true, Ordering::SeqCst);
    }

    pub fn is_requested(&self) -> bool {
        self.requested.load(Ordering::SeqCst)
    }
}

impl std::fmt::Debug for ShutdownSignal {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ShutdownSignal")
            .field("requested", &self.is_requested())
            .finish()
    }
}

impl PartialEq for ShutdownSignal {
    fn eq(&self, other: &Self) -> bool {
        self.is_requested() == other.is_requested()
    }
}

impl Eq for ShutdownSignal {}
