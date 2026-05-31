use std::fmt;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc,
};
use std::time::Instant;

static NEXT_OPERATION_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct OperationId(u64);

impl OperationId {
    pub(crate) fn new() -> Self {
        Self(NEXT_OPERATION_ID.fetch_add(1, Ordering::Relaxed))
    }

    #[cfg(test)]
    pub(crate) fn as_u64(self) -> u64 {
        self.0
    }
}

impl fmt::Display for OperationId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "op-{}", self.0)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct TraceContext {
    trace_id: Option<String>,
    span_id: Option<String>,
}

impl TraceContext {
    pub(crate) fn new(trace_id: Option<String>, span_id: Option<String>) -> Self {
        Self { trace_id, span_id }
    }

    pub(crate) fn trace_id(&self) -> Option<&str> {
        self.trace_id.as_deref()
    }

    pub(crate) fn span_id(&self) -> Option<&str> {
        self.span_id.as_deref()
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct CancellationToken {
    inner: Arc<AtomicBool>,
}

impl CancellationToken {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn cancel(&self) {
        self.inner.store(true, Ordering::SeqCst);
    }

    pub(crate) fn is_cancelled(&self) -> bool {
        self.inner.load(Ordering::SeqCst)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct OperationContext {
    operation_id: OperationId,
    request_id: Option<String>,
    deadline: Option<Instant>,
    cancellation: CancellationToken,
    trace: TraceContext,
}

impl OperationContext {
    pub(crate) fn new() -> Self {
        Self {
            operation_id: OperationId::new(),
            request_id: None,
            deadline: None,
            cancellation: CancellationToken::new(),
            trace: TraceContext::default(),
        }
    }

    pub(crate) fn with_request_id(mut self, request_id: impl Into<String>) -> Self {
        self.request_id = Some(request_id.into());
        self
    }

    pub(crate) fn with_deadline(mut self, deadline: Instant) -> Self {
        self.deadline = Some(deadline);
        self
    }

    pub(crate) fn with_trace(mut self, trace: TraceContext) -> Self {
        self.trace = trace;
        self
    }

    pub(crate) fn operation_id(&self) -> OperationId {
        self.operation_id
    }

    pub(crate) fn request_id(&self) -> Option<&str> {
        self.request_id.as_deref()
    }

    pub(crate) fn deadline(&self) -> Option<Instant> {
        self.deadline
    }

    pub(crate) fn cancellation(&self) -> &CancellationToken {
        &self.cancellation
    }

    pub(crate) fn trace(&self) -> &TraceContext {
        &self.trace
    }
}

impl Default for OperationContext {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn operation_ids_are_unique_and_ordered() {
        let first = OperationId::new();
        let second = OperationId::new();

        assert_ne!(first, second);
        assert!(second.as_u64() > first.as_u64());
        assert!(first.to_string().starts_with("op-"));
    }

    #[test]
    fn cancellation_token_clone_observes_cancel() {
        let token = CancellationToken::new();
        let clone = token.clone();

        assert!(!token.is_cancelled());
        assert!(!clone.is_cancelled());

        clone.cancel();

        assert!(token.is_cancelled());
        assert!(clone.is_cancelled());
    }

    #[test]
    fn operation_context_defaults_and_builders_are_stable() {
        let deadline = Instant::now() + Duration::from_secs(5);
        let trace = TraceContext::new(Some("trace".to_string()), Some("span".to_string()));
        let context = OperationContext::new()
            .with_request_id("request-1")
            .with_deadline(deadline)
            .with_trace(trace);

        assert_eq!(context.request_id(), Some("request-1"));
        assert_eq!(context.deadline(), Some(deadline));
        assert_eq!(context.trace().trace_id(), Some("trace"));
        assert_eq!(context.trace().span_id(), Some("span"));
        assert!(!context.cancellation().is_cancelled());
        assert!(context.operation_id().as_u64() > 0);
    }
}
