use std::sync::mpsc;
use std::time::Duration;

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

    'connect: loop {
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
            Ok(()) => {}
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
                continue;
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
            match transport.next_notification() {
                Ok(Some(notification)) => {
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
                Ok(None) => {
                    let shutdown_requested = shutdown.is_requested() || control.is_closed();
                    if shutdown_requested {
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
                    if should_retry_connect(&options.reconnect, reconnect_attempts) {
                        reconnect_attempts += 1;
                        continue 'connect;
                    }
                    emit_state(events, super::RealtimeConnectionState::Closed, None);
                    return Ok(outcome(
                        receiver,
                        control,
                        super::RealtimeExitReason::ConnectionClosed,
                        reconnect_attempts,
                        warnings,
                    ));
                }
                Err(error) => {
                    warnings.push(error.to_string());
                    if should_retry_connect(&options.reconnect, reconnect_attempts) {
                        reconnect_attempts += 1;
                        continue 'connect;
                    }
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
    let mut transport = DefaultRunnerTransport {
        client,
        socket: None,
        idle_ticks: 0,
    };
    run_realtime_transport_until_shutdown(options, shutdown, control, &mut transport)
        .map(|outcome| outcome.exit)
}

pub(crate) fn spawn_default(
    client: crate::core::ImClient,
    options: super::RealtimeOptions,
) -> crate::ImResult<super::RealtimeHandle> {
    crate::internal::realtime::transport::require_realtime_auth_token(&client)?;
    let (sender, receiver) = mpsc::sync_channel(options.event_buffer);
    let control = super::RealtimeControl::default();
    let worker_control = control.clone();
    let worker = std::thread::Builder::new()
        .name("im-core-realtime".to_string())
        .spawn(move || {
            let mut transport = DefaultRunnerTransport {
                client: &client,
                socket: None,
                idle_ticks: 0,
            };
            let mut sink = SenderRunnerEvents { sender };
            let _ = run_realtime_transport_with_event_sink_until_shutdown(
                options,
                super::ShutdownSignal::pending(),
                worker_control,
                &mut transport,
                &mut sink,
            );
        })
        .map_err(|err| crate::ImError::Internal {
            message: format!("spawn realtime worker: {err}"),
        })?;
    Ok(super::RealtimeHandle::with_worker(
        receiver, control, worker,
    ))
}

struct DefaultRunnerTransport<'a> {
    client: &'a crate::core::ImClient,
    socket: Option<crate::internal::realtime::ws_transport::WsTransport>,
    idle_ticks: u32,
}

impl RealtimeRunnerTransport for DefaultRunnerTransport<'_> {
    fn connect(&mut self) -> crate::ImResult<()> {
        let socket =
            crate::internal::realtime::transport::connect_native_websocket_session(self.client)?;
        self.socket = Some(socket);
        self.idle_ticks = 0;
        Ok(())
    }

    fn next_notification(&mut self) -> crate::ImResult<Option<Value>> {
        let Some(socket) = self.socket.as_mut() else {
            return Ok(None);
        };
        match socket.read_json_message_timeout(Duration::from_millis(250)) {
            Ok(Some(message)) => {
                self.idle_ticks = 0;
                Ok(Some(Value::Object(message)))
            }
            Ok(None) => {
                self.idle_ticks = self.idle_ticks.saturating_add(1);
                if self.idle_ticks >= 60 {
                    self.idle_ticks = 0;
                    socket
                        .ping()
                        .map_err(|error| crate::ImError::TransportUnavailable {
                            detail: error.message,
                        })?;
                }
                Ok(Some(Value::Null))
            }
            Err(error) if error.is_closed() => Ok(None),
            Err(error) => Err(crate::ImError::TransportUnavailable {
                detail: error.message,
            }),
        }
    }
}

struct SenderRunnerEvents {
    sender: mpsc::SyncSender<super::ImEvent>,
}

impl RealtimeRunnerEventSink for SenderRunnerEvents {
    fn emit(&mut self, event: super::ImEvent) -> crate::ImResult<()> {
        self.sender
            .try_send(event)
            .map_err(|_| crate::ImError::TransportUnavailable {
                detail: "realtime event buffer is full or closed".to_string(),
            })
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
