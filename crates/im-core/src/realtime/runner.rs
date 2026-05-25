use std::sync::mpsc;
use std::time::Duration;

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
struct LocalStateRealtimeNotificationProjector<'a, P> {
    client: &'a crate::core::ImClient,
    inner: P,
}

#[cfg(feature = "sqlite")]
impl<P> RealtimeNotificationProjector for LocalStateRealtimeNotificationProjector<'_, P>
where
    P: RealtimeNotificationProjector,
{
    fn project(&mut self, notification: Value) -> RealtimeProjectionOutcome {
        let outcome = self.inner.project(notification);
        if let Some(event) = outcome.event.as_ref() {
            if let Err(error) = project_realtime_event_to_local_state(self.client, event) {
                let mut warnings = outcome.warnings;
                warnings.push(error.to_string());
                return RealtimeProjectionOutcome {
                    event: outcome.event,
                    warnings,
                };
            }
        }
        outcome
    }
}

#[cfg(feature = "sqlite")]
fn project_realtime_event_to_local_state(
    client: &crate::core::ImClient,
    event: &super::ImEvent,
) -> crate::ImResult<()> {
    match event {
        super::ImEvent::MessageReceived(event) => project_realtime_message_received(client, event),
        super::ImEvent::GroupUpdated(event) => project_realtime_group_updated(client, event),
        super::ImEvent::ConnectionStateChanged(_)
        | super::ImEvent::MessageUpdated(_)
        | super::ImEvent::LocalNotification(_)
        | super::ImEvent::HostNotification(_)
        | super::ImEvent::UnknownNotification(_) => Ok(()),
    }
}

#[cfg(feature = "sqlite")]
fn project_realtime_message_received(
    client: &crate::core::ImClient,
    event: &super::MessageReceivedEvent,
) -> crate::ImResult<()> {
    let Some(projection) =
        crate::internal::realtime::local_projection::plan_realtime_message_local_projection(
            &crate::internal::realtime::local_projection::RealtimeMessageLocalProjectionContext {
                owner_identity_id: client.current_identity().id.as_str().to_string(),
                owner_did: client.did().as_str().to_string(),
                credential_name: client
                    .current_identity()
                    .local_alias
                    .as_deref()
                    .unwrap_or_else(|| client.current_identity().id.as_str())
                    .to_string(),
            },
            &event.message,
            event.attachment_summary.as_ref(),
            event.download_action.as_ref(),
            &event.warnings,
        )
    else {
        return Ok(());
    };
    let group_did = projection.group_did().to_string();
    let sender_did = projection.sender_did().to_string();
    let sent_at = event.message.sent_at.clone();
    let mut connection = crate::internal::local_state::open_writable(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    )?;
    crate::internal::realtime::local_projection::apply_realtime_message_local_projection(
        &connection,
        projection,
    )?;
    project_realtime_message_group(client, &connection, &group_did, sent_at.as_deref())?;
    project_realtime_message_contact(
        client,
        &mut connection,
        &sender_did,
        if group_did.trim().is_empty() {
            "realtime.direct.incoming"
        } else {
            "realtime.group.incoming"
        },
        &group_did,
    )?;
    Ok(())
}

#[cfg(feature = "sqlite")]
fn project_realtime_message_group(
    client: &crate::core::ImClient,
    connection: &rusqlite::Connection,
    group_did: &str,
    sent_at: Option<&str>,
) -> crate::ImResult<()> {
    let group = group_did.trim();
    if group.is_empty() {
        return Ok(());
    }
    crate::internal::local_state::groups::upsert_group(
        connection,
        crate::internal::local_state::groups::GroupRecord {
            owner_identity_id: client.current_identity().id.as_str().to_string(),
            owner_did: client.did().as_str().to_string(),
            group_id: group.to_string(),
            group_did: group.to_string(),
            last_message_at: sent_at
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .unwrap_or_else(now_utc_like),
            metadata: serde_json::json!({
                "source": "im-core.realtime.message",
            })
            .to_string(),
            credential_name: client
                .current_identity()
                .local_alias
                .as_deref()
                .unwrap_or_else(|| client.current_identity().id.as_str())
                .to_string(),
            ..crate::internal::local_state::groups::GroupRecord::default()
        },
    )
}

#[cfg(feature = "sqlite")]
fn project_realtime_message_contact(
    client: &crate::core::ImClient,
    connection: &mut rusqlite::Connection,
    peer_did: &str,
    source_type: &str,
    source_group_id: &str,
) -> crate::ImResult<()> {
    let peer_did = peer_did.trim();
    if peer_did.is_empty() || peer_did == client.did().as_str() {
        return Ok(());
    }
    crate::internal::contact_store::records::upsert_contact(
        connection,
        crate::internal::contact_store::records::ContactRecord {
            owner_identity_id: client.current_identity().id.as_str().to_string(),
            owner_did: client.did().as_str().to_string(),
            did: peer_did.to_string(),
            source_type: source_type.to_string(),
            source_group_id: source_group_id.trim().to_string(),
            messaged: Some(true),
            first_seen_at: now_utc_like(),
            last_seen_at: now_utc_like(),
            credential_name: client
                .current_identity()
                .local_alias
                .as_deref()
                .unwrap_or_else(|| client.current_identity().id.as_str())
                .to_string(),
            ..crate::internal::contact_store::records::ContactRecord::default()
        },
    )
}

#[cfg(feature = "sqlite")]
fn project_realtime_group_updated(
    client: &crate::core::ImClient,
    event: &super::GroupUpdatedEvent,
) -> crate::ImResult<()> {
    let group = event.group.as_str().trim();
    if group.is_empty() {
        return Ok(());
    }
    let connection = crate::internal::local_state::open_writable(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    )?;
    crate::internal::local_state::groups::upsert_group(
        &connection,
        crate::internal::local_state::groups::GroupRecord {
            owner_identity_id: client.current_identity().id.as_str().to_string(),
            owner_did: client.did().as_str().to_string(),
            group_id: group.to_string(),
            group_did: group.to_string(),
            last_message_at: now_utc_like(),
            metadata: serde_json::json!({
                "source": "im-core.realtime",
                "update_kind": group_update_kind_label(&event.update_kind),
            })
            .to_string(),
            credential_name: client
                .current_identity()
                .local_alias
                .as_deref()
                .unwrap_or_else(|| client.current_identity().id.as_str())
                .to_string(),
            ..crate::internal::local_state::groups::GroupRecord::default()
        },
    )
}

#[cfg(feature = "sqlite")]
fn group_update_kind_label(kind: &super::GroupUpdateKind) -> &'static str {
    match kind {
        super::GroupUpdateKind::Created => "created",
        super::GroupUpdateKind::Updated => "updated",
        super::GroupUpdateKind::MemberAdded => "member_added",
        super::GroupUpdateKind::MemberRemoved => "member_removed",
        super::GroupUpdateKind::MessageAdded => "message_added",
        super::GroupUpdateKind::Unknown => "unknown",
    }
}

#[cfg(feature = "sqlite")]
fn now_utc_like() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
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
    let mut transport = DefaultRunnerTransport {
        client,
        socket: None,
        idle_ticks: 0,
    };
    let (sender, receiver) = mpsc::sync_channel(options.event_buffer);
    let mut events = ChannelRunnerEvents { sender };
    let projector = SecureRealtimeNotificationProjector {
        client,
        directory_transport: crate::internal::transport::CoreHttpTransport::new(client),
    };
    #[cfg(feature = "sqlite")]
    let mut projector = LocalStateRealtimeNotificationProjector {
        client,
        inner: projector,
    };
    #[cfg(not(feature = "sqlite"))]
    let mut projector = projector;
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

pub(crate) fn run_default_with_event_sink_until_shutdown<S>(
    client: &crate::core::ImClient,
    options: super::RealtimeOptions,
    shutdown: super::ShutdownSignal,
    event_sink: &mut S,
) -> crate::ImResult<super::RealtimeExit>
where
    S: RealtimeRunnerEventSink,
{
    let control = super::RealtimeControl::default();
    let mut transport = DefaultRunnerTransport {
        client,
        socket: None,
        idle_ticks: 0,
    };
    let (_sender, receiver) = mpsc::sync_channel(options.event_buffer);
    let mut events = SinkRunnerEvents { sink: event_sink };
    let projector = SecureRealtimeNotificationProjector {
        client,
        directory_transport: crate::internal::transport::CoreHttpTransport::new(client),
    };
    #[cfg(feature = "sqlite")]
    let mut projector = LocalStateRealtimeNotificationProjector {
        client,
        inner: projector,
    };
    #[cfg(not(feature = "sqlite"))]
    let mut projector = projector;
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
            let receiver = mpsc::channel().1;
            let mut sink = SenderRunnerEvents { sender };
            let mut events = SinkRunnerEvents { sink: &mut sink };
            let projector = SecureRealtimeNotificationProjector {
                client: &client,
                directory_transport: crate::internal::transport::CoreHttpTransport::new(&client),
            };
            #[cfg(feature = "sqlite")]
            let mut projector = LocalStateRealtimeNotificationProjector {
                client: &client,
                inner: projector,
            };
            #[cfg(not(feature = "sqlite"))]
            let mut projector = projector;
            let _ = run_realtime_transport_loop(
                options,
                super::ShutdownSignal::pending(),
                worker_control,
                &mut transport,
                receiver,
                &mut events,
                &mut projector,
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

#[cfg(all(test, feature = "sqlite"))]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn realtime_local_state_projector_stores_direct_message_and_contact() {
        let fixture = TestClientFixture::new("direct");
        let client = fixture.client();
        let mut projector = LocalStateRealtimeNotificationProjector {
            client: &client,
            inner: FixedProjector {
                event: Some(super::super::ImEvent::MessageReceived(
                    direct_message_event(client.did().as_str()),
                )),
            },
        };

        let outcome = projector.project(json!({"method": "test.direct"}));

        assert!(outcome.warnings.is_empty());
        assert!(matches!(
            outcome.event,
            Some(super::super::ImEvent::MessageReceived(_))
        ));
        let connection = rusqlite::Connection::open(fixture.sqlite_path()).unwrap();
        let message = connection
            .query_row(
                r#"
SELECT owner_identity_id, owner_did, sender_did, receiver_did, content_type, content, is_read, credential_name
FROM messages
WHERE msg_id = ?1 AND owner_did = ?2"#,
                rusqlite::params!["msg-direct-1", client.did().as_str()],
                |row| {
                    Ok(StoredDirectMessage {
                        owner_identity_id: row.get(0)?,
                        owner_did: row.get(1)?,
                        sender_did: row.get(2)?,
                        receiver_did: row.get(3)?,
                        content_type: row.get(4)?,
                        content: row.get(5)?,
                        is_read: row.get::<_, i64>(6)?,
                        credential_name: row.get(7)?,
                    })
                },
            )
            .unwrap();
        assert_eq!(message.owner_identity_id, "alice");
        assert_eq!(message.owner_did, client.did().as_str());
        assert_eq!(message.sender_did, "did:example:bob");
        assert_eq!(message.receiver_did, client.did().as_str());
        assert_eq!(message.content_type, "text/plain");
        assert_eq!(message.content, "hello from realtime");
        assert_eq!(message.is_read, 0);
        assert_eq!(message.credential_name, "alice");
        let contact_count: i64 = connection
            .query_row(
                r#"
SELECT COUNT(*)
FROM contacts
WHERE owner_identity_id = ?1 AND owner_did = ?2 AND did = ?3 AND messaged = 1"#,
                rusqlite::params!["alice", client.did().as_str(), "did:example:bob"],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(contact_count, 1);
    }

    #[test]
    fn realtime_local_state_projector_stores_group_update() {
        let fixture = TestClientFixture::new("group");
        let client = fixture.client();
        let mut projector = LocalStateRealtimeNotificationProjector {
            client: &client,
            inner: FixedProjector {
                event: Some(super::super::ImEvent::GroupUpdated(
                    super::super::GroupUpdatedEvent {
                        group: crate::ids::GroupRef::parse("did:example:group:blue").unwrap(),
                        update_kind: super::super::GroupUpdateKind::Updated,
                    },
                )),
            },
        };

        let outcome = projector.project(json!({"method": "test.group"}));

        assert!(outcome.warnings.is_empty());
        assert!(matches!(
            outcome.event,
            Some(super::super::ImEvent::GroupUpdated(_))
        ));
        let connection = rusqlite::Connection::open(fixture.sqlite_path()).unwrap();
        let group = connection
            .query_row(
                r#"
SELECT owner_identity_id, owner_did, group_id, group_did, metadata, credential_name
FROM groups
WHERE owner_did = ?1 AND group_id = ?2"#,
                rusqlite::params![client.did().as_str(), "did:example:group:blue"],
                |row| {
                    Ok(StoredGroup {
                        owner_identity_id: row.get(0)?,
                        owner_did: row.get(1)?,
                        group_id: row.get(2)?,
                        group_did: row.get(3)?,
                        metadata: row.get(4)?,
                        credential_name: row.get(5)?,
                    })
                },
            )
            .unwrap();
        assert_eq!(group.owner_identity_id, "alice");
        assert_eq!(group.owner_did, client.did().as_str());
        assert_eq!(group.group_id, "did:example:group:blue");
        assert_eq!(group.group_did, "did:example:group:blue");
        assert_eq!(group.credential_name, "alice");
        let metadata = serde_json::from_str::<serde_json::Value>(&group.metadata).unwrap();
        assert_eq!(metadata["source"], "im-core.realtime");
        assert_eq!(metadata["update_kind"], "updated");
    }

    struct FixedProjector {
        event: Option<super::super::ImEvent>,
    }

    impl RealtimeNotificationProjector for FixedProjector {
        fn project(&mut self, _notification: serde_json::Value) -> RealtimeProjectionOutcome {
            RealtimeProjectionOutcome {
                event: self.event.take(),
                warnings: Vec::new(),
            }
        }
    }

    #[derive(Debug, PartialEq, Eq)]
    struct StoredDirectMessage {
        owner_identity_id: String,
        owner_did: String,
        sender_did: String,
        receiver_did: String,
        content_type: String,
        content: String,
        is_read: i64,
        credential_name: String,
    }

    #[derive(Debug, PartialEq, Eq)]
    struct StoredGroup {
        owner_identity_id: String,
        owner_did: String,
        group_id: String,
        group_did: String,
        metadata: String,
        credential_name: String,
    }

    struct TestClientFixture {
        root: std::path::PathBuf,
    }

    impl TestClientFixture {
        fn new(name: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "im-core-realtime-runner-{name}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            Self { root }
        }

        fn client(&self) -> crate::core::ImClient {
            self.core()
                .client(crate::identity::IdentitySelector::LocalAlias(
                    "alice".to_string(),
                ))
                .unwrap()
        }

        fn core(&self) -> crate::core::ImCore {
            crate::core::ImCore::new(
                crate::ImCoreConfig {
                    service_base_url: crate::ServiceEndpoint::parse("https://example.test")
                        .unwrap(),
                    did_domain: "awiki.info".to_string(),
                    user_service_endpoint: None,
                    message_service_endpoint: None,
                    mail_service_endpoint: None,
                    anp_service_endpoint: None,
                    anp_service_did: None,
                    ca_bundle: None,
                    transport_policy: crate::MessageTransportPolicy::Auto,
                },
                crate::ImCorePaths {
                    identities: crate::IdentityRegistryPaths {
                        identity_root_dir: self.root.join("identities"),
                        registry_path: self.root.join("identities").join("registry.json"),
                        default_identity_path: Some(self.root.join("identities").join("default")),
                    },
                    local_state: crate::LocalStatePaths {
                        sqlite_path: self.sqlite_path(),
                    },
                    runtime: crate::RuntimePaths {
                        cache_dir: self.root.join("cache"),
                        temp_dir: self.root.join("tmp"),
                    },
                },
            )
            .unwrap()
        }

        fn sqlite_path(&self) -> std::path::PathBuf {
            self.root.join("local").join("im.sqlite")
        }
    }

    fn direct_message_event(owner_did: &str) -> super::super::MessageReceivedEvent {
        let sender = crate::ids::PeerRef::parse("did:example:bob", "awiki.info").unwrap();
        let receiver = crate::ids::PeerRef::parse(owner_did, "awiki.info").unwrap();
        super::super::MessageReceivedEvent {
            message: crate::messages::Message {
                id: crate::ids::MessageId::parse("msg-direct-1").unwrap(),
                thread: crate::messages::ThreadRef::Direct(sender.clone()),
                direction: crate::messages::MessageDirection::Incoming,
                sender,
                receiver: Some(receiver),
                group: None,
                body: crate::messages::MessageBodyView::Text {
                    text: "hello from realtime".to_string(),
                    kind: crate::messages::MessageKind::Text,
                },
                sent_at: Some("2026-05-25T00:00:00Z".to_string()),
                received_at: None,
                metadata: crate::messages::MessageMetadata {
                    content_type: Some("text/plain".to_string()),
                    ..crate::messages::MessageMetadata::default()
                },
            },
            attachment_summary: None,
            download_action: None,
            warnings: Vec::new(),
        }
    }
}
