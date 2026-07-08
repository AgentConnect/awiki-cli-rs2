use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RuntimeTimeouts {
    connect_timeout: Duration,
    request_timeout: Duration,
    websocket_idle_timeout: Duration,
    websocket_ping_timeout: Duration,
    attachment_transfer_timeout: Duration,
    db_request_timeout: Duration,
}

impl RuntimeTimeouts {
    pub(crate) fn new(
        connect_timeout: Duration,
        request_timeout: Duration,
        websocket_idle_timeout: Duration,
        websocket_ping_timeout: Duration,
        attachment_transfer_timeout: Duration,
        db_request_timeout: Duration,
    ) -> Self {
        Self {
            connect_timeout,
            request_timeout,
            websocket_idle_timeout,
            websocket_ping_timeout,
            attachment_transfer_timeout,
            db_request_timeout,
        }
    }

    pub(crate) fn connect_timeout(&self) -> Duration {
        self.connect_timeout
    }

    pub(crate) fn request_timeout(&self) -> Duration {
        self.request_timeout
    }

    pub(crate) fn websocket_idle_timeout(&self) -> Duration {
        self.websocket_idle_timeout
    }

    pub(crate) fn websocket_ping_timeout(&self) -> Duration {
        self.websocket_ping_timeout
    }

    pub(crate) fn attachment_transfer_timeout(&self) -> Duration {
        self.attachment_transfer_timeout
    }

    pub(crate) fn db_request_timeout(&self) -> Duration {
        self.db_request_timeout
    }
}

impl Default for RuntimeTimeouts {
    fn default() -> Self {
        Self::new(
            Duration::from_secs(10),
            Duration::from_secs(30),
            Duration::from_secs(90),
            Duration::from_secs(15),
            Duration::from_secs(60 * 30),
            Duration::from_secs(30),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_timeout_default_values_are_stable() {
        let timeouts = RuntimeTimeouts::default();

        assert_eq!(timeouts.connect_timeout(), Duration::from_secs(10));
        assert_eq!(timeouts.request_timeout(), Duration::from_secs(30));
        assert_eq!(timeouts.websocket_idle_timeout(), Duration::from_secs(90));
        assert_eq!(timeouts.websocket_ping_timeout(), Duration::from_secs(15));
        assert_eq!(
            timeouts.attachment_transfer_timeout(),
            Duration::from_secs(60 * 30)
        );
        assert_eq!(timeouts.db_request_timeout(), Duration::from_secs(30));
    }

    #[test]
    fn runtime_timeouts_can_be_overridden() {
        let timeouts = RuntimeTimeouts::new(
            Duration::from_secs(1),
            Duration::from_secs(2),
            Duration::from_secs(3),
            Duration::from_secs(4),
            Duration::from_secs(5),
            Duration::from_secs(6),
        );

        assert_eq!(timeouts.connect_timeout(), Duration::from_secs(1));
        assert_eq!(timeouts.request_timeout(), Duration::from_secs(2));
        assert_eq!(timeouts.websocket_idle_timeout(), Duration::from_secs(3));
        assert_eq!(timeouts.websocket_ping_timeout(), Duration::from_secs(4));
        assert_eq!(
            timeouts.attachment_transfer_timeout(),
            Duration::from_secs(5)
        );
        assert_eq!(timeouts.db_request_timeout(), Duration::from_secs(6));
    }
}
