use std::sync::mpsc;

use serde_json::Value;

use crate::internal::transport::RpcTransport;

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

pub(crate) struct RealtimeProjectionOutcome {
    pub(crate) event: Option<super::ImEvent>,
    pub(crate) warnings: Vec<String>,
}

pub(crate) trait RealtimeNotificationProjector {
    fn project(&mut self, notification: Value) -> RealtimeProjectionOutcome;
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
    let mut projector = PlainRealtimeNotificationProjector;
    run_realtime_transport_loop(
        options,
        shutdown,
        control,
        transport,
        receiver,
        &mut events,
        &mut projector,
    )
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
    let mut projector = PlainRealtimeNotificationProjector;
    run_realtime_transport_loop(
        options,
        shutdown,
        control,
        transport,
        receiver,
        &mut events,
        &mut projector,
    )
}

fn run_realtime_transport_loop<T, E, P>(
    options: super::RealtimeOptions,
    shutdown: super::ShutdownSignal,
    control: super::RealtimeControl,
    transport: &mut T,
    receiver: mpsc::Receiver<super::ImEvent>,
    events: &mut E,
    projector: &mut P,
) -> crate::ImResult<RealtimeRunnerOutcome>
where
    T: RealtimeRunnerTransport,
    E: RunnerEvents,
    P: RealtimeNotificationProjector,
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
                let projection = projector.project(notification);
                warnings.extend(projection.warnings);
                let Some(event) = projection.event else {
                    continue;
                };
                if let Err(warning) = events.emit(event) {
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

struct PlainRealtimeNotificationProjector;

impl RealtimeNotificationProjector for PlainRealtimeNotificationProjector {
    fn project(&mut self, notification: Value) -> RealtimeProjectionOutcome {
        RealtimeProjectionOutcome {
            event: Some(
                crate::internal::realtime::projection::project_notification(&notification).event,
            ),
            warnings: Vec::new(),
        }
    }
}

struct SecureRealtimeNotificationProjector<'a, R> {
    client: &'a crate::core::ImClient,
    directory_transport: R,
}

impl<R> RealtimeNotificationProjector for SecureRealtimeNotificationProjector<'_, R>
where
    R: RpcTransport,
{
    fn project(&mut self, notification: Value) -> RealtimeProjectionOutcome {
        let (notification, mut warnings) = normalize_direct_e2ee_realtime_notification(
            self.client,
            notification,
            &mut self.directory_transport,
        );
        let Some(notification) = notification else {
            return RealtimeProjectionOutcome {
                event: None,
                warnings,
            };
        };
        let Some(notification) =
            normalize_group_e2ee_realtime_notification(self.client, notification, &mut warnings)
        else {
            return RealtimeProjectionOutcome {
                event: None,
                warnings,
            };
        };
        let event =
            Some(crate::internal::realtime::projection::project_notification(&notification).event);
        RealtimeProjectionOutcome { event, warnings }
    }
}

#[cfg(feature = "sqlite")]
fn normalize_direct_e2ee_realtime_notification<R>(
    client: &crate::core::ImClient,
    notification: Value,
    directory_transport: &mut R,
) -> (Option<Value>, Vec<String>)
where
    R: RpcTransport,
{
    let projection = crate::internal::secure_direct::incoming::
        maybe_normalize_direct_e2ee_notification_for_client(
            client,
            notification,
            directory_transport,
            crate::internal::secure_direct::incoming::DirectDecryptMode::WithSideEffects,
        );
    (projection.notification, projection.warnings)
}

#[cfg(not(feature = "sqlite"))]
fn normalize_direct_e2ee_realtime_notification<R>(
    _client: &crate::core::ImClient,
    notification: Value,
    _directory_transport: &mut R,
) -> (Option<Value>, Vec<String>)
where
    R: RpcTransport,
{
    (Some(notification), Vec::new())
}

#[cfg(feature = "group-e2ee")]
fn normalize_group_e2ee_realtime_notification(
    client: &crate::core::ImClient,
    notification: Value,
    warnings: &mut Vec<String>,
) -> Option<Value> {
    let notice_projection = crate::internal::group_e2ee::notices::
        maybe_process_group_e2ee_notice_notification_for_client(client, notification);
    warnings.extend(notice_projection.warnings);
    let notification = notice_projection.notification?;

    let projection =
        crate::internal::group_e2ee::incoming::maybe_normalize_group_e2ee_notification_for_client(
            client,
            notification,
        );
    warnings.extend(projection.warnings);
    projection.notification
}

#[cfg(not(feature = "group-e2ee"))]
fn normalize_group_e2ee_realtime_notification(
    _client: &crate::core::ImClient,
    notification: Value,
    _warnings: &mut Vec<String>,
) -> Option<Value> {
    Some(notification)
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
    let (sender, receiver) = mpsc::sync_channel(options.event_buffer);
    let mut events = ChannelRunnerEvents { sender };
    let mut projector = SecureRealtimeNotificationProjector {
        client,
        directory_transport: crate::internal::transport::CoreHttpTransport::new(client),
    };
    run_realtime_transport_loop(
        options,
        shutdown,
        control,
        &mut transport,
        receiver,
        &mut events,
        &mut projector,
    )
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
