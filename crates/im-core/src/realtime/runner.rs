use std::sync::mpsc;

use serde_json::Value;

pub trait RealtimeRunnerTransport {
    fn connect(&mut self) -> crate::ImResult<()>;

    fn next_notification(&mut self) -> crate::ImResult<Option<Value>>;
}

pub trait RealtimeRunnerEventSink {
    fn emit(&mut self, event: super::ImEvent) -> crate::ImResult<()>;
}

pub struct RealtimeRunnerOutcome {
    pub exit: super::RealtimeExit,
    pub handle: super::RealtimeHandle,
}

pub fn run_realtime_transport_until_shutdown<T>(
    options: super::RealtimeOptions,
    shutdown: super::ShutdownSignal,
    control: super::RealtimeControl,
    transport: &mut T,
) -> crate::ImResult<RealtimeRunnerOutcome>
where
    T: RealtimeRunnerTransport,
{
    let (sender, receiver) = mpsc::sync_channel(options.event_buffer);
    let mut events = ChannelRunnerEvents { sender };
    run_realtime_transport_loop(options, shutdown, control, transport, receiver, &mut events)
}

pub fn run_realtime_transport_with_event_sink_until_shutdown<T, S>(
    options: super::RealtimeOptions,
    shutdown: super::ShutdownSignal,
    control: super::RealtimeControl,
    transport: &mut T,
    event_sink: &mut S,
) -> crate::ImResult<RealtimeRunnerOutcome>
where
    T: RealtimeRunnerTransport,
    S: RealtimeRunnerEventSink,
{
    let (_sender, receiver) = mpsc::sync_channel(options.event_buffer);
    let mut events = SinkRunnerEvents { sink: event_sink };
    run_realtime_transport_loop(options, shutdown, control, transport, receiver, &mut events)
}

fn run_realtime_transport_loop<T, E>(
    options: super::RealtimeOptions,
    shutdown: super::ShutdownSignal,
    control: super::RealtimeControl,
    transport: &mut T,
    receiver: mpsc::Receiver<super::ImEvent>,
    events: &mut E,
) -> crate::ImResult<RealtimeRunnerOutcome>
where
    T: RealtimeRunnerTransport,
    E: RunnerEvents,
{
    let mut warnings = Vec::new();
    let mut reconnect_attempts = 0;
    let mut first_attempt = true;

    loop {
        if shutdown.is_requested() || control.is_closed() {
            emit_state(
                events,
                super::RealtimeConnectionState::Closed,
                Some("shutdown requested".to_string()),
            );
            return Ok(outcome(
                receiver,
                control,
                super::RealtimeExitReason::ShutdownRequested,
                reconnect_attempts,
                warnings,
            ));
        }
        emit_state(
            events,
            if first_attempt {
                super::RealtimeConnectionState::Connecting
            } else {
                super::RealtimeConnectionState::Reconnecting
            },
            None,
        );
        first_attempt = false;
        match transport.connect() {
            Ok(()) => break,
            Err(error) => {
                warnings.push(error.to_string());
                if !should_retry_connect(&options.reconnect, reconnect_attempts) {
                    let reason = exit_reason_for_connect_error(&error);
                    emit_state(events, super::RealtimeConnectionState::Disconnected, None);
                    return Ok(outcome(
                        receiver,
                        control,
                        reason,
                        reconnect_attempts,
                        warnings,
                    ));
                }
                reconnect_attempts += 1;
            }
        }
    }

    emit_state(events, super::RealtimeConnectionState::Connected, None);
    loop {
        if shutdown.is_requested() || control.is_closed() {
            emit_state(
                events,
                super::RealtimeConnectionState::Closed,
                Some("shutdown requested".to_string()),
            );
            return Ok(outcome(
                receiver,
                control,
                super::RealtimeExitReason::ShutdownRequested,
                reconnect_attempts,
                warnings,
            ));
        }
        match transport.next_notification()? {
            Some(notification) => {
                if notification.is_null() {
                    continue;
                }
                let projection =
                    crate::internal::realtime::projection::project_notification(&notification);
                if let Err(warning) = events.emit(projection.event) {
                    warnings.push(warning);
                    return Ok(outcome(
                        receiver,
                        control,
                        super::RealtimeExitReason::ConnectionClosed,
                        reconnect_attempts,
                        warnings,
                    ));
                }
            }
            None => {
                let shutdown_requested = shutdown.is_requested() || control.is_closed();
                emit_state(
                    events,
                    super::RealtimeConnectionState::Closed,
                    shutdown_requested.then(|| "shutdown requested".to_string()),
                );
                return Ok(outcome(
                    receiver,
                    control,
                    if shutdown_requested {
                        super::RealtimeExitReason::ShutdownRequested
                    } else {
                        super::RealtimeExitReason::ConnectionClosed
                    },
                    reconnect_attempts,
                    warnings,
                ));
            }
        }
    }
}

trait RunnerEvents {
    fn emit(&mut self, event: super::ImEvent) -> Result<(), String>;
}

struct ChannelRunnerEvents {
    sender: mpsc::SyncSender<super::ImEvent>,
}

impl RunnerEvents for ChannelRunnerEvents {
    fn emit(&mut self, event: super::ImEvent) -> Result<(), String> {
        self.sender
            .try_send(event)
            .map_err(|_| "realtime event buffer is full or closed".to_string())
    }
}

struct SinkRunnerEvents<'a, S> {
    sink: &'a mut S,
}

impl<S> RunnerEvents for SinkRunnerEvents<'_, S>
where
    S: RealtimeRunnerEventSink,
{
    fn emit(&mut self, event: super::ImEvent) -> Result<(), String> {
        self.sink.emit(event).map_err(|err| err.to_string())
    }
}

pub(crate) fn run_default_until_shutdown(
    client: &crate::core::ImClient,
    options: super::RealtimeOptions,
    shutdown: super::ShutdownSignal,
) -> crate::ImResult<super::RealtimeExit> {
    let control = super::RealtimeControl::default();
    let mut transport = DefaultRunnerTransport { client };
    run_realtime_transport_until_shutdown(options, shutdown, control, &mut transport)
        .map(|outcome| outcome.exit)
}

struct DefaultRunnerTransport<'a> {
    client: &'a crate::core::ImClient,
}

impl RealtimeRunnerTransport for DefaultRunnerTransport<'_> {
    fn connect(&mut self) -> crate::ImResult<()> {
        crate::internal::realtime::transport::default_connect(self.client).map(|_| ())
    }

    fn next_notification(&mut self) -> crate::ImResult<Option<Value>> {
        Ok(None)
    }
}

fn outcome(
    receiver: mpsc::Receiver<super::ImEvent>,
    control: super::RealtimeControl,
    reason: super::RealtimeExitReason,
    reconnect_attempts: u32,
    warnings: Vec<String>,
) -> RealtimeRunnerOutcome {
    RealtimeRunnerOutcome {
        exit: super::RealtimeExit {
            reason,
            reconnect_attempts,
            warnings,
        },
        handle: super::RealtimeHandle::new(receiver, control),
    }
}

fn emit_state(
    events: &mut impl RunnerEvents,
    state: super::RealtimeConnectionState,
    reason: Option<String>,
) {
    let _ = events.emit(super::ImEvent::ConnectionStateChanged(
        super::ConnectionStateChanged { state, reason },
    ));
}

fn should_retry_connect(policy: &super::ReconnectPolicy, reconnect_attempts: u32) -> bool {
    match policy {
        super::ReconnectPolicy::Disabled => false,
        super::ReconnectPolicy::Fixed { max_attempts, .. }
        | super::ReconnectPolicy::Exponential { max_attempts, .. } => max_attempts
            .map(|max_attempts| reconnect_attempts < max_attempts)
            .unwrap_or(true),
    }
}

fn exit_reason_for_connect_error(error: &crate::ImError) -> super::RealtimeExitReason {
    match error {
        crate::ImError::AuthRequired | crate::ImError::SessionExpired => {
            super::RealtimeExitReason::AuthFailed
        }
        crate::ImError::TransportUnavailable { .. } => {
            super::RealtimeExitReason::TransportUnavailable
        }
        _ => super::RealtimeExitReason::FatalError,
    }
}
