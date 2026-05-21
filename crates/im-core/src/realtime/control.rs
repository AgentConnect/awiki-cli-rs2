#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RealtimeControl {
    closed: bool,
}

impl RealtimeControl {
    pub fn is_closed(&self) -> bool {
        self.closed
    }
}

impl Default for RealtimeControl {
    fn default() -> Self {
        Self { closed: false }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShutdownSignal {
    requested: bool,
}

impl ShutdownSignal {
    pub fn pending() -> Self {
        Self { requested: false }
    }

    pub fn requested() -> Self {
        Self { requested: true }
    }

    pub fn is_requested(&self) -> bool {
        self.requested
    }
}
