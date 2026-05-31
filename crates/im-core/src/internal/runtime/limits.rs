#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RuntimeLimits {
    network_limit: usize,
    attachment_limit: usize,
    db_queue_limit: usize,
    crypto_worker_limit: usize,
}

impl RuntimeLimits {
    const DEFAULT_NETWORK_LIMIT: usize = 64;
    const DEFAULT_ATTACHMENT_LIMIT: usize = 4;
    const DEFAULT_DB_QUEUE_LIMIT: usize = 1024;
    const DEFAULT_CRYPTO_WORKER_LIMIT: usize = 4;

    pub(crate) fn new(
        network_limit: usize,
        attachment_limit: usize,
        db_queue_limit: usize,
        crypto_worker_limit: usize,
    ) -> Self {
        Self {
            network_limit,
            attachment_limit,
            db_queue_limit,
            crypto_worker_limit,
        }
    }

    pub(crate) fn network_limit(&self) -> usize {
        self.network_limit
    }

    pub(crate) fn attachment_limit(&self) -> usize {
        self.attachment_limit
    }

    pub(crate) fn db_queue_limit(&self) -> usize {
        self.db_queue_limit
    }

    pub(crate) fn crypto_worker_limit(&self) -> usize {
        self.crypto_worker_limit
    }
}

impl Default for RuntimeLimits {
    fn default() -> Self {
        Self::new(
            Self::DEFAULT_NETWORK_LIMIT,
            Self::DEFAULT_ATTACHMENT_LIMIT,
            Self::DEFAULT_DB_QUEUE_LIMIT,
            Self::DEFAULT_CRYPTO_WORKER_LIMIT,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_limits_default_values_are_stable() {
        let limits = RuntimeLimits::default();

        assert_eq!(limits.network_limit(), 64);
        assert_eq!(limits.attachment_limit(), 4);
        assert_eq!(limits.db_queue_limit(), 1024);
        assert_eq!(limits.crypto_worker_limit(), 4);
    }

    #[test]
    fn runtime_limits_can_be_overridden() {
        let limits = RuntimeLimits::new(1, 2, 3, 4);

        assert_eq!(limits.network_limit(), 1);
        assert_eq!(limits.attachment_limit(), 2);
        assert_eq!(limits.db_queue_limit(), 3);
        assert_eq!(limits.crypto_worker_limit(), 4);
    }
}
