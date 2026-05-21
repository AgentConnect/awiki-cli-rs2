pub struct RealtimeService<'a> {
    _client: &'a crate::core::ImClient,
}

impl<'a> RealtimeService<'a> {
    pub(crate) fn new(client: &'a crate::core::ImClient) -> Self {
        Self { _client: client }
    }

    pub fn status(&self) -> crate::ImResult<super::RealtimeStatus> {
        Ok(super::RealtimeStatus {
            connected: false,
            state: super::RealtimeConnectionState::Disconnected,
            subscriptions: Vec::new(),
            last_error: None,
        })
    }

    pub fn connect(
        &self,
        options: super::RealtimeOptions,
    ) -> crate::ImResult<super::RealtimeHandle> {
        validate_options(&options)?;
        Err(crate::ImError::unsupported("realtime-connect"))
    }

    pub fn run_until_shutdown(
        &self,
        options: super::RealtimeOptions,
        shutdown: super::ShutdownSignal,
    ) -> crate::ImResult<super::RealtimeExit> {
        validate_options(&options)?;
        if shutdown.is_requested() {
            return Ok(super::RealtimeExit {
                reason: super::RealtimeExitReason::ShutdownRequested,
                reconnect_attempts: 0,
                warnings: Vec::new(),
            });
        }
        Err(crate::ImError::unsupported("realtime-runner"))
    }
}

fn validate_options(options: &super::RealtimeOptions) -> crate::ImResult<()> {
    if options.event_buffer == 0 {
        return Err(crate::ImError::invalid_input(
            Some("event_buffer".to_string()),
            "event buffer must be greater than zero",
        ));
    }
    validate_reconnect_policy(&options.reconnect)
}

fn validate_reconnect_policy(policy: &super::ReconnectPolicy) -> crate::ImResult<()> {
    match policy {
        super::ReconnectPolicy::Disabled => Ok(()),
        super::ReconnectPolicy::Fixed { delay_ms, .. } if *delay_ms == 0 => {
            Err(crate::ImError::invalid_input(
                Some("reconnect.delay_ms".to_string()),
                "fixed reconnect delay must be greater than zero",
            ))
        }
        super::ReconnectPolicy::Fixed { .. } => Ok(()),
        super::ReconnectPolicy::Exponential {
            base_delay_ms,
            max_delay_ms,
            ..
        } if *base_delay_ms == 0 || *max_delay_ms == 0 => Err(crate::ImError::invalid_input(
            Some("reconnect".to_string()),
            "exponential reconnect delays must be greater than zero",
        )),
        super::ReconnectPolicy::Exponential {
            base_delay_ms,
            max_delay_ms,
            ..
        } if base_delay_ms > max_delay_ms => Err(crate::ImError::invalid_input(
            Some("reconnect".to_string()),
            "base reconnect delay must not exceed max delay",
        )),
        super::ReconnectPolicy::Exponential { .. } => Ok(()),
    }
}
