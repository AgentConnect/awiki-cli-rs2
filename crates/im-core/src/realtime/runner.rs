use std::future::Future;
#[cfg(feature = "blocking")]
use std::sync::mpsc;
use std::time::Duration;

use serde_json::Value;
use tokio::sync::{mpsc as tokio_mpsc, oneshot, watch};

#[cfg(feature = "sqlite")]
use crate::internal::transport::AsyncRpcTransport;
#[cfg(feature = "blocking")]
use crate::internal::transport::RpcTransport;

#[cfg(feature = "blocking")]
pub trait RealtimeRunnerTransport {
    fn connect(&mut self) -> crate::ImResult<()>;

    fn next_notification(&mut self) -> crate::ImResult<Option<Value>>;
}

pub(crate) trait AsyncRealtimeRunnerTransport {
    fn connect(&mut self) -> impl Future<Output = crate::ImResult<()>> + Send + '_;

    fn next_notification(
        &mut self,
    ) -> impl Future<Output = crate::ImResult<Option<Value>>> + Send + '_;
}

#[cfg(feature = "blocking")]
pub trait RealtimeRunnerEventSink {
    fn emit(&mut self, event: super::ImEvent) -> crate::ImResult<()>;
}

#[cfg(feature = "blocking")]
pub struct RealtimeRunnerOutcome {
    pub exit: super::RealtimeExit,
    pub handle: super::RealtimeHandle,
}

pub(crate) struct RealtimeProjectionOutcome {
    pub(crate) event: Option<super::ImEvent>,
    pub(crate) additional_events: Vec<super::ImEvent>,
    pub(crate) warnings: Vec<String>,
}

pub(crate) trait RealtimeNotificationProjector {
    fn project(&mut self, notification: Value) -> RealtimeProjectionOutcome;
}

pub(crate) trait AsyncRealtimeNotificationProjector {
    fn project_async(
        &mut self,
        notification: Value,
    ) -> impl Future<Output = RealtimeProjectionOutcome> + Send + '_;
}

impl<T> AsyncRealtimeNotificationProjector for T
where
    T: RealtimeNotificationProjector + Send,
{
    async fn project_async(&mut self, notification: Value) -> RealtimeProjectionOutcome {
        RealtimeNotificationProjector::project(self, notification)
    }
}

#[cfg(feature = "blocking")]
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

#[cfg(feature = "blocking")]
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

#[cfg(feature = "blocking")]
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
                    for event in projection.additional_events {
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
            additional_events: Vec::new(),
            warnings: Vec::new(),
        }
    }
}

#[cfg(feature = "blocking")]
struct SecureRealtimeNotificationProjector<'a, R> {
    client: &'a crate::core::ImClient,
    directory_transport: R,
}

#[cfg(feature = "blocking")]
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
                additional_events: Vec::new(),
                warnings,
            };
        };
        let Some(notification) =
            normalize_group_e2ee_realtime_notification(self.client, notification, &mut warnings)
        else {
            return RealtimeProjectionOutcome {
                event: None,
                additional_events: Vec::new(),
                warnings,
            };
        };
        let event =
            Some(crate::internal::realtime::projection::project_notification(&notification).event);
        RealtimeProjectionOutcome {
            event,
            additional_events: Vec::new(),
            warnings,
        }
    }
}

#[cfg(all(feature = "blocking", feature = "sqlite"))]
struct AsyncFirstSecureRealtimeNotificationProjector<'a> {
    client: &'a crate::core::ImClient,
    runtime: Option<tokio::runtime::Runtime>,
    direct_processor:
        crate::internal::secure_direct::async_receive::AsyncDirectSecureIncomingProcessor<'a>,
}

#[cfg(all(feature = "blocking", feature = "sqlite"))]
impl<'a> AsyncFirstSecureRealtimeNotificationProjector<'a> {
    fn new(client: &'a crate::core::ImClient) -> Self {
        Self {
            client,
            runtime: tokio::runtime::Builder::new_current_thread()
                .enable_io()
                .enable_time()
                .build()
                .ok(),
            direct_processor:
                crate::internal::secure_direct::async_receive::AsyncDirectSecureIncomingProcessor::new(
                    client,
                ),
        }
    }
}

#[cfg(all(feature = "blocking", feature = "sqlite"))]
impl RealtimeNotificationProjector for AsyncFirstSecureRealtimeNotificationProjector<'_> {
    fn project(&mut self, notification: Value) -> RealtimeProjectionOutcome {
        let projection = normalize_direct_e2ee_realtime_notification_async_first(
            self.client,
            self.runtime.as_ref(),
            &self.direct_processor,
            notification,
        );
        let (notification, mut warnings, additional_notifications) = match projection {
            DirectRealtimeProjectionResult::Projected {
                notification,
                additional_notifications,
                warnings,
            } => (notification, warnings, additional_notifications),
            DirectRealtimeProjectionResult::Fallback(notification) => (
                Some(notification),
                vec![
                    "secure direct realtime async projection fell back to wire notification"
                        .to_owned(),
                ],
                Vec::new(),
            ),
        };
        let additional_events = additional_notifications
            .into_iter()
            .filter_map(|notification| {
                let mut group_warnings = Vec::new();
                normalize_group_e2ee_realtime_notification_async_first(
                    self.client,
                    self.runtime.as_ref(),
                    notification,
                    &mut group_warnings,
                )
                .map(|notification| {
                    warnings.extend(group_warnings);
                    crate::internal::realtime::projection::project_notification(&notification).event
                })
            })
            .collect::<Vec<_>>();
        let Some(notification) = notification else {
            return RealtimeProjectionOutcome {
                event: None,
                additional_events,
                warnings,
            };
        };
        let Some(notification) = normalize_group_e2ee_realtime_notification_async_first(
            self.client,
            self.runtime.as_ref(),
            notification,
            &mut warnings,
        ) else {
            return RealtimeProjectionOutcome {
                event: None,
                additional_events,
                warnings,
            };
        };
        let event =
            Some(crate::internal::realtime::projection::project_notification(&notification).event);
        RealtimeProjectionOutcome {
            event,
            additional_events,
            warnings,
        }
    }
}

#[cfg(feature = "sqlite")]
struct AsyncSecureRealtimeNotificationProjector<'a> {
    client: &'a crate::core::ImClient,
    direct_processor:
        crate::internal::secure_direct::async_receive::AsyncDirectSecureIncomingProcessor<'a>,
}

#[cfg(feature = "sqlite")]
impl<'a> AsyncSecureRealtimeNotificationProjector<'a> {
    fn new(client: &'a crate::core::ImClient) -> Self {
        Self {
            client,
            direct_processor:
                crate::internal::secure_direct::async_receive::AsyncDirectSecureIncomingProcessor::new(
                    client,
                ),
        }
    }
}

#[cfg(feature = "sqlite")]
impl AsyncRealtimeNotificationProjector for AsyncSecureRealtimeNotificationProjector<'_> {
    async fn project_async(&mut self, notification: Value) -> RealtimeProjectionOutcome {
        let projection = normalize_direct_e2ee_realtime_notification_async(
            self.client,
            &self.direct_processor,
            notification,
        )
        .await;
        let (notification, mut warnings, additional_notifications) = match projection {
            DirectRealtimeProjectionResult::Projected {
                notification,
                additional_notifications,
                warnings,
            } => (notification, warnings, additional_notifications),
            DirectRealtimeProjectionResult::Fallback(notification) => (
                Some(notification),
                vec![
                    "secure direct realtime async projection fell back to wire notification"
                        .to_owned(),
                ],
                Vec::new(),
            ),
        };
        let mut additional_events = Vec::new();
        for notification in additional_notifications {
            let mut group_warnings = Vec::new();
            if let Some(notification) = normalize_group_e2ee_realtime_notification_async(
                self.client,
                notification,
                &mut group_warnings,
            )
            .await
            {
                warnings.extend(group_warnings);
                additional_events.push(
                    crate::internal::realtime::projection::project_notification(&notification)
                        .event,
                );
            }
        }
        let Some(notification) = notification else {
            return RealtimeProjectionOutcome {
                event: None,
                additional_events,
                warnings,
            };
        };
        let Some(notification) = normalize_group_e2ee_realtime_notification_async(
            self.client,
            notification,
            &mut warnings,
        )
        .await
        else {
            return RealtimeProjectionOutcome {
                event: None,
                additional_events,
                warnings,
            };
        };
        let event =
            Some(crate::internal::realtime::projection::project_notification(&notification).event);
        RealtimeProjectionOutcome {
            event,
            additional_events,
            warnings,
        }
    }
}

#[cfg(all(feature = "blocking", feature = "sqlite"))]
struct LocalStateRealtimeNotificationProjector<'a, P> {
    client: &'a crate::core::ImClient,
    inner: P,
}

#[cfg(all(feature = "blocking", feature = "sqlite"))]
impl<P> RealtimeNotificationProjector for LocalStateRealtimeNotificationProjector<'_, P>
where
    P: RealtimeNotificationProjector,
{
    fn project(&mut self, notification: Value) -> RealtimeProjectionOutcome {
        let outcome = self.inner.project(notification);
        for event in &outcome.additional_events {
            if let Err(error) = project_realtime_event_to_local_state(self.client, event) {
                let mut warnings = outcome.warnings;
                warnings.push(error.to_string());
                return RealtimeProjectionOutcome {
                    event: outcome.event,
                    additional_events: outcome.additional_events,
                    warnings,
                };
            }
        }
        if let Some(event) = outcome.event.as_ref() {
            if let Err(error) = project_realtime_event_to_local_state(self.client, event) {
                let mut warnings = outcome.warnings;
                warnings.push(error.to_string());
                return RealtimeProjectionOutcome {
                    event: outcome.event,
                    additional_events: outcome.additional_events,
                    warnings,
                };
            }
        }
        outcome
    }
}

#[cfg(feature = "sqlite")]
struct AsyncLocalStateRealtimeNotificationProjector<P> {
    client: crate::core::ImClient,
    inner: P,
}

#[cfg(feature = "sqlite")]
impl<P> AsyncRealtimeNotificationProjector for AsyncLocalStateRealtimeNotificationProjector<P>
where
    P: AsyncRealtimeNotificationProjector + Send,
{
    async fn project_async(&mut self, notification: Value) -> RealtimeProjectionOutcome {
        let outcome = self.inner.project_async(notification).await;
        let mut warnings = outcome.warnings;
        for event in &outcome.additional_events {
            if let Err(error) =
                project_realtime_event_to_local_state_async(&self.client, event).await
            {
                warnings.push(error.to_string());
            }
        }
        if let Some(event) = outcome.event.as_ref() {
            if let Err(error) =
                project_realtime_event_to_local_state_async(&self.client, event).await
            {
                warnings.push(error.to_string());
            }
        }
        RealtimeProjectionOutcome {
            event: outcome.event,
            additional_events: outcome.additional_events,
            warnings,
        }
    }
}

#[cfg(all(feature = "blocking", feature = "sqlite"))]
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
async fn project_realtime_event_to_local_state_async(
    client: &crate::core::ImClient,
    event: &super::ImEvent,
) -> crate::ImResult<()> {
    match event {
        super::ImEvent::MessageReceived(event) => {
            project_realtime_message_received_async(client, event).await
        }
        super::ImEvent::GroupUpdated(event) => {
            project_realtime_group_updated_async(client, event).await
        }
        super::ImEvent::ConnectionStateChanged(_)
        | super::ImEvent::MessageUpdated(_)
        | super::ImEvent::LocalNotification(_)
        | super::ImEvent::HostNotification(_)
        | super::ImEvent::UnknownNotification(_) => Ok(()),
    }
}

#[cfg(all(feature = "blocking", feature = "sqlite"))]
fn project_realtime_message_received(
    client: &crate::core::ImClient,
    event: &super::MessageReceivedEvent,
) -> crate::ImResult<()> {
    let peer_scope = realtime_direct_peer_scope(client, &event.message);
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
                peer_scope,
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
async fn project_realtime_message_received_async(
    client: &crate::core::ImClient,
    event: &super::MessageReceivedEvent,
) -> crate::ImResult<()> {
    let peer_scope = realtime_direct_peer_scope_async(client, &event.message).await;
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
                peer_scope,
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
    let db = client.core_inner().local_state_db().await?;
    db.store_messages(vec![projection.into_record()]).await?;
    if let Some(record) = realtime_message_group_record(client, &group_did, sent_at.as_deref()) {
        db.upsert_group(record).await?;
    }
    if let Some(record) = realtime_message_contact_record(
        client,
        &sender_did,
        if group_did.trim().is_empty() {
            "realtime.direct.incoming"
        } else {
            "realtime.group.incoming"
        },
        &group_did,
    ) {
        db.upsert_contact(record).await?;
    }
    Ok(())
}

#[cfg(all(feature = "blocking", feature = "sqlite"))]
fn project_realtime_message_group(
    client: &crate::core::ImClient,
    connection: &rusqlite::Connection,
    group_did: &str,
    sent_at: Option<&str>,
) -> crate::ImResult<()> {
    let Some(record) = realtime_message_group_record(client, group_did, sent_at) else {
        return Ok(());
    };
    crate::internal::local_state::groups::upsert_group(connection, record)
}

#[cfg(feature = "sqlite")]
fn realtime_message_group_record(
    client: &crate::core::ImClient,
    group_did: &str,
    sent_at: Option<&str>,
) -> Option<crate::internal::local_state::groups::GroupRecord> {
    let group = group_did.trim();
    if group.is_empty() {
        return None;
    }
    Some(crate::internal::local_state::groups::GroupRecord {
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
    })
}

#[cfg(all(feature = "blocking", feature = "sqlite"))]
fn realtime_direct_peer_scope(
    client: &crate::core::ImClient,
    message: &crate::messages::Message,
) -> Option<crate::internal::local_state::owner_scope::DirectPeerScope> {
    let peer_did = realtime_direct_peer_did(client, message)?;
    let mut transport = crate::internal::transport::CoreHttpTransport::new(client);
    let call = crate::internal::identity_wire::directory::build_handle_lookup_by_did_rpc_call(
        peer_did.as_str(),
    )
    .ok()?;
    let raw = RpcTransport::rpc(&mut transport, call.endpoint, call.method, call.params).ok()?;
    let lookup = crate::internal::directory_runtime::handle_lookup_from_value(&raw).ok()?;
    crate::internal::local_state::owner_scope::DirectPeerScope::new(
        lookup.user_id,
        lookup.handle.as_str().to_owned(),
    )
    .ok()
}

#[cfg(feature = "sqlite")]
async fn realtime_direct_peer_scope_async(
    client: &crate::core::ImClient,
    message: &crate::messages::Message,
) -> Option<crate::internal::local_state::owner_scope::DirectPeerScope> {
    let peer_did = realtime_direct_peer_did(client, message)?;
    let mut transport = crate::internal::transport::CoreHttpTransport::new(client);
    let call = crate::internal::identity_wire::directory::build_handle_lookup_by_did_rpc_call(
        peer_did.as_str(),
    )
    .ok()?;
    let raw =
        AsyncRpcTransport::rpc(&mut transport, call.endpoint, call.method, call.params)
            .await
            .ok()?;
    let lookup = crate::internal::directory_runtime::handle_lookup_from_value(&raw).ok()?;
    crate::internal::local_state::owner_scope::DirectPeerScope::new(
        lookup.user_id,
        lookup.handle.as_str().to_owned(),
    )
    .ok()
}

#[cfg(feature = "sqlite")]
fn realtime_direct_peer_did(
    client: &crate::core::ImClient,
    message: &crate::messages::Message,
) -> Option<String> {
    if message.group.is_some() || matches!(message.thread, crate::messages::ThreadRef::Group(_)) {
        return None;
    }
    let owner = client.did().as_str().trim();
    let sender = message.sender.as_str().trim();
    let receiver = message
        .receiver
        .as_ref()
        .map(|peer| peer.as_str().trim())
        .unwrap_or_default();
    let peer = if !sender.is_empty() && sender != owner {
        sender
    } else if !receiver.is_empty() && receiver != owner {
        receiver
    } else {
        ""
    };
    (!peer.is_empty() && peer.starts_with("did:")).then(|| peer.to_owned())
}

#[cfg(all(feature = "blocking", feature = "sqlite"))]
fn project_realtime_message_contact(
    client: &crate::core::ImClient,
    connection: &mut rusqlite::Connection,
    peer_did: &str,
    source_type: &str,
    source_group_id: &str,
) -> crate::ImResult<()> {
    let Some(record) =
        realtime_message_contact_record(client, peer_did, source_type, source_group_id)
    else {
        return Ok(());
    };
    crate::internal::contact_store::records::upsert_contact(connection, record)
}

#[cfg(feature = "sqlite")]
fn realtime_message_contact_record(
    client: &crate::core::ImClient,
    peer_did: &str,
    source_type: &str,
    source_group_id: &str,
) -> Option<crate::internal::contact_store::records::ContactRecord> {
    let peer_did = peer_did.trim();
    if peer_did.is_empty() || peer_did == client.did().as_str() {
        return None;
    }
    Some(crate::internal::contact_store::records::ContactRecord {
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
    })
}

#[cfg(all(feature = "blocking", feature = "sqlite"))]
fn project_realtime_group_updated(
    client: &crate::core::ImClient,
    event: &super::GroupUpdatedEvent,
) -> crate::ImResult<()> {
    let Some(record) = realtime_group_update_record(client, event) else {
        return Ok(());
    };
    let connection = crate::internal::local_state::open_writable(
        &client.core_inner().sdk_paths().local_state.sqlite_path,
    )?;
    crate::internal::local_state::groups::upsert_group(&connection, record)
}

#[cfg(feature = "sqlite")]
async fn project_realtime_group_updated_async(
    client: &crate::core::ImClient,
    event: &super::GroupUpdatedEvent,
) -> crate::ImResult<()> {
    let Some(record) = realtime_group_update_record(client, event) else {
        return Ok(());
    };
    let db = client.core_inner().local_state_db().await?;
    db.upsert_group(record).await
}

#[cfg(feature = "sqlite")]
fn realtime_group_update_record(
    client: &crate::core::ImClient,
    event: &super::GroupUpdatedEvent,
) -> Option<crate::internal::local_state::groups::GroupRecord> {
    let group = event.group.as_str().trim();
    if group.is_empty() {
        return None;
    }
    Some(crate::internal::local_state::groups::GroupRecord {
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
    })
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

#[cfg(all(feature = "blocking", feature = "sqlite"))]
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

#[cfg(feature = "sqlite")]
enum DirectRealtimeProjectionResult {
    Projected {
        notification: Option<Value>,
        additional_notifications: Vec<Value>,
        warnings: Vec<String>,
    },
    Fallback(Value),
}

#[cfg(all(feature = "blocking", feature = "sqlite"))]
fn normalize_direct_e2ee_realtime_notification_async_first(
    client: &crate::core::ImClient,
    runtime: Option<&tokio::runtime::Runtime>,
    processor: &crate::internal::secure_direct::async_receive::AsyncDirectSecureIncomingProcessor<
        '_,
    >,
    notification: Value,
) -> DirectRealtimeProjectionResult {
    if !crate::internal::realtime::projection::is_direct_secure_wire_notification(&notification) {
        return DirectRealtimeProjectionResult::Projected {
            notification: Some(notification),
            additional_notifications: Vec::new(),
            warnings: Vec::new(),
        };
    }
    let original = notification.clone();
    let outcome = runtime
        .map(|runtime| run_realtime_async_projection(client, runtime, processor, notification))
        .unwrap_or(None);
    match outcome {
        Some(crate::internal::secure_direct::incoming::DirectRealtimeAsyncProjectionOutcome::Projected(projection)) => {
            DirectRealtimeProjectionResult::Projected {
                notification: projection.notification,
                additional_notifications: projection.additional_notifications,
                warnings: projection.warnings,
            }
        }
        Some(crate::internal::secure_direct::incoming::DirectRealtimeAsyncProjectionOutcome::Fallback(notification)) => {
            DirectRealtimeProjectionResult::Fallback(notification)
        }
        None => {
            DirectRealtimeProjectionResult::Fallback(original)
        }
    }
}

#[cfg(feature = "sqlite")]
async fn normalize_direct_e2ee_realtime_notification_async(
    client: &crate::core::ImClient,
    processor: &crate::internal::secure_direct::async_receive::AsyncDirectSecureIncomingProcessor<
        '_,
    >,
    notification: Value,
) -> DirectRealtimeProjectionResult {
    if !crate::internal::realtime::projection::is_direct_secure_wire_notification(&notification) {
        return DirectRealtimeProjectionResult::Projected {
            notification: Some(notification),
            additional_notifications: Vec::new(),
            warnings: Vec::new(),
        };
    }
    let original = notification.clone();
    let mut message_transport = crate::internal::transport::CoreHttpTransport::new(client);
    let mut directory_transport = crate::internal::transport::CoreHttpTransport::new(client);
    let outcome =
        crate::internal::secure_direct::incoming::normalize_direct_e2ee_notification_with_async_processor_and_directory(
            client,
            processor,
            notification,
            crate::internal::secure_direct::incoming::DirectDecryptMode::WithSideEffects,
            &mut message_transport,
            &mut directory_transport,
        )
        .await;
    match outcome {
        crate::internal::secure_direct::incoming::DirectRealtimeAsyncProjectionOutcome::Projected(
            projection,
        ) => DirectRealtimeProjectionResult::Projected {
            notification: projection.notification,
            additional_notifications: projection.additional_notifications,
            warnings: projection.warnings,
        },
        crate::internal::secure_direct::incoming::DirectRealtimeAsyncProjectionOutcome::Fallback(
            notification,
        ) => {
            if notification.is_null() {
                DirectRealtimeProjectionResult::Fallback(original)
            } else {
                DirectRealtimeProjectionResult::Fallback(notification)
            }
        }
    }
}

#[cfg(all(feature = "blocking", feature = "sqlite"))]
fn run_realtime_async_projection(
    client: &crate::core::ImClient,
    runtime: &tokio::runtime::Runtime,
    processor: &crate::internal::secure_direct::async_receive::AsyncDirectSecureIncomingProcessor<
        '_,
    >,
    notification: Value,
) -> Option<crate::internal::secure_direct::incoming::DirectRealtimeAsyncProjectionOutcome> {
    let mut message_transport = crate::internal::transport::CoreHttpTransport::new(client);
    let mut directory_transport = crate::internal::transport::CoreHttpTransport::new(client);
    Some(runtime.block_on(crate::internal::secure_direct::incoming::normalize_direct_e2ee_notification_with_async_processor_and_directory(
        client,
        processor,
        notification,
        crate::internal::secure_direct::incoming::DirectDecryptMode::WithSideEffects,
        &mut message_transport,
        &mut directory_transport,
    )))
}

#[cfg(all(feature = "blocking", not(feature = "sqlite")))]
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

#[cfg(all(feature = "blocking", feature = "group-e2ee"))]
fn normalize_group_e2ee_realtime_notification(
    client: &crate::core::ImClient,
    notification: Value,
    warnings: &mut Vec<String>,
) -> Option<Value> {
    normalize_group_e2ee_realtime_notification_async_first(client, None, notification, warnings)
}

#[cfg(all(feature = "blocking", feature = "group-e2ee"))]
fn normalize_group_e2ee_realtime_notification_async_first(
    client: &crate::core::ImClient,
    runtime: Option<&tokio::runtime::Runtime>,
    notification: Value,
    warnings: &mut Vec<String>,
) -> Option<Value> {
    let notice_projection = runtime
        .map(|runtime| {
            runtime.block_on(
                crate::internal::group_e2ee::notices::
                    maybe_process_group_e2ee_notice_notification_for_client_async(
                        client,
                        notification.clone(),
                    ),
            )
        })
        .unwrap_or_else(|| {
            crate::internal::group_e2ee::notices::
                maybe_process_group_e2ee_notice_notification_for_client(client, notification)
        });
    warnings.extend(notice_projection.warnings);
    let notification = notice_projection.notification?;

    let projection = runtime
        .map(|runtime| {
            runtime.block_on(
                crate::internal::group_e2ee::incoming::
                    maybe_normalize_group_e2ee_notification_for_client_async(client, notification.clone()),
            )
        })
        .unwrap_or_else(|| {
            crate::internal::group_e2ee::incoming::
                maybe_normalize_group_e2ee_notification_for_client(client, notification)
        });
    warnings.extend(projection.warnings);
    projection.notification
}

#[cfg(feature = "group-e2ee")]
async fn normalize_group_e2ee_realtime_notification_async(
    client: &crate::core::ImClient,
    notification: Value,
    warnings: &mut Vec<String>,
) -> Option<Value> {
    let notice_projection =
        crate::internal::group_e2ee::notices::
            maybe_process_group_e2ee_notice_notification_for_client_async(
                client,
                notification,
            )
            .await;
    warnings.extend(notice_projection.warnings);
    let notification = notice_projection.notification?;

    let projection =
        crate::internal::group_e2ee::incoming::
            maybe_normalize_group_e2ee_notification_for_client_async(client, notification)
                .await;
    warnings.extend(projection.warnings);
    projection.notification
}

#[cfg(all(feature = "blocking", not(feature = "group-e2ee")))]
fn normalize_group_e2ee_realtime_notification(
    _client: &crate::core::ImClient,
    notification: Value,
    _warnings: &mut Vec<String>,
) -> Option<Value> {
    Some(notification)
}

#[cfg(not(feature = "group-e2ee"))]
async fn normalize_group_e2ee_realtime_notification_async(
    _client: &crate::core::ImClient,
    notification: Value,
    _warnings: &mut Vec<String>,
) -> Option<Value> {
    Some(notification)
}

#[cfg(all(feature = "blocking", not(feature = "group-e2ee")))]
fn normalize_group_e2ee_realtime_notification_async_first(
    _client: &crate::core::ImClient,
    _runtime: Option<&tokio::runtime::Runtime>,
    notification: Value,
    _warnings: &mut Vec<String>,
) -> Option<Value> {
    Some(notification)
}

#[cfg(feature = "blocking")]
struct ChannelRunnerEvents {
    sender: mpsc::SyncSender<super::ImEvent>,
}

#[cfg(feature = "blocking")]
impl RunnerEvents for ChannelRunnerEvents {
    fn emit(&mut self, event: super::ImEvent) -> Result<(), String> {
        self.sender
            .try_send(event)
            .map_err(|_| "realtime event buffer is full or closed".to_string())
    }
}

#[cfg(feature = "blocking")]
struct SinkRunnerEvents<'a, S> {
    sink: &'a mut S,
}

#[cfg(feature = "blocking")]
impl<S> RunnerEvents for SinkRunnerEvents<'_, S>
where
    S: RealtimeRunnerEventSink,
{
    fn emit(&mut self, event: super::ImEvent) -> Result<(), String> {
        self.sink.emit(event).map_err(|err| err.to_string())
    }
}

pub(crate) async fn run_realtime_async_transport_until_shutdown<T, P>(
    options: super::RealtimeOptions,
    shutdown: super::ShutdownSignal,
    control: super::RealtimeControl,
    transport: &mut T,
    mut events: TokioRunnerEvents,
    projector: &mut P,
) -> crate::ImResult<super::RealtimeExit>
where
    T: AsyncRealtimeRunnerTransport,
    P: AsyncRealtimeNotificationProjector,
{
    let mut warnings = Vec::new();
    let mut reconnect_attempts = 0;
    let mut first_attempt = true;

    'connect: loop {
        if shutdown.is_requested() || control.is_closed() {
            emit_state(
                &mut events,
                super::RealtimeConnectionState::Closed,
                Some("shutdown requested".to_string()),
            );
            return Ok(realtime_exit(
                super::RealtimeExitReason::ShutdownRequested,
                reconnect_attempts,
                warnings,
            ));
        }
        emit_state(
            &mut events,
            if first_attempt {
                super::RealtimeConnectionState::Connecting
            } else {
                super::RealtimeConnectionState::Reconnecting
            },
            None,
        );
        first_attempt = false;

        let connect_result = tokio::select! {
            _ = wait_for_realtime_shutdown(&shutdown, &control) => {
                emit_state(
                    &mut events,
                    super::RealtimeConnectionState::Closed,
                    Some("shutdown requested".to_string()),
                );
                return Ok(realtime_exit(
                    super::RealtimeExitReason::ShutdownRequested,
                    reconnect_attempts,
                    warnings,
                ));
            }
            result = transport.connect() => result,
        };

        match connect_result {
            Ok(()) => {}
            Err(error) => {
                warnings.push(error.to_string());
                if !should_retry_connect(&options.reconnect, reconnect_attempts) {
                    let reason = exit_reason_for_connect_error(&error);
                    emit_state(
                        &mut events,
                        super::RealtimeConnectionState::Disconnected,
                        None,
                    );
                    return Ok(realtime_exit(reason, reconnect_attempts, warnings));
                }
                reconnect_attempts += 1;
                sleep_before_reconnect(&options.reconnect).await;
                continue;
            }
        }

        emit_state(&mut events, super::RealtimeConnectionState::Connected, None);
        loop {
            if shutdown.is_requested() || control.is_closed() {
                emit_state(
                    &mut events,
                    super::RealtimeConnectionState::Closed,
                    Some("shutdown requested".to_string()),
                );
                return Ok(realtime_exit(
                    super::RealtimeExitReason::ShutdownRequested,
                    reconnect_attempts,
                    warnings,
                ));
            }
            let notification_result = tokio::select! {
                _ = wait_for_realtime_shutdown(&shutdown, &control) => {
                    emit_state(
                        &mut events,
                        super::RealtimeConnectionState::Closed,
                        Some("shutdown requested".to_string()),
                    );
                    return Ok(realtime_exit(
                        super::RealtimeExitReason::ShutdownRequested,
                        reconnect_attempts,
                        warnings,
                    ));
                }
                result = transport.next_notification() => result,
            };
            match notification_result {
                Ok(Some(notification)) => {
                    if notification.is_null() {
                        continue;
                    }
                    let projection = projector.project_async(notification).await;
                    warnings.extend(projection.warnings);
                    for event in projection.additional_events {
                        if let Err(warning) = events.emit(event) {
                            events.set_status(
                                super::RealtimeConnectionState::Closed,
                                Some(warning.clone()),
                            );
                            warnings.push(warning);
                            return Ok(realtime_exit(
                                super::RealtimeExitReason::ConnectionClosed,
                                reconnect_attempts,
                                warnings,
                            ));
                        }
                    }
                    let Some(event) = projection.event else {
                        continue;
                    };
                    if let Err(warning) = events.emit(event) {
                        events.set_status(
                            super::RealtimeConnectionState::Closed,
                            Some(warning.clone()),
                        );
                        warnings.push(warning);
                        return Ok(realtime_exit(
                            super::RealtimeExitReason::ConnectionClosed,
                            reconnect_attempts,
                            warnings,
                        ));
                    }
                }
                Ok(None) => {
                    if shutdown.is_requested() || control.is_closed() {
                        emit_state(
                            &mut events,
                            super::RealtimeConnectionState::Closed,
                            Some("shutdown requested".to_string()),
                        );
                        return Ok(realtime_exit(
                            super::RealtimeExitReason::ShutdownRequested,
                            reconnect_attempts,
                            warnings,
                        ));
                    }
                    if should_retry_connect(&options.reconnect, reconnect_attempts) {
                        reconnect_attempts += 1;
                        sleep_before_reconnect(&options.reconnect).await;
                        continue 'connect;
                    }
                    emit_state(&mut events, super::RealtimeConnectionState::Closed, None);
                    return Ok(realtime_exit(
                        super::RealtimeExitReason::ConnectionClosed,
                        reconnect_attempts,
                        warnings,
                    ));
                }
                Err(error) => {
                    warnings.push(error.to_string());
                    if should_retry_connect(&options.reconnect, reconnect_attempts) {
                        reconnect_attempts += 1;
                        sleep_before_reconnect(&options.reconnect).await;
                        continue 'connect;
                    }
                    let reason = exit_reason_for_connect_error(&error);
                    emit_state(
                        &mut events,
                        super::RealtimeConnectionState::Disconnected,
                        None,
                    );
                    return Ok(realtime_exit(reason, reconnect_attempts, warnings));
                }
            }
        }
    }
}

async fn wait_for_realtime_shutdown(
    shutdown: &super::ShutdownSignal,
    control: &super::RealtimeControl,
) {
    while !shutdown.is_requested() && !control.is_closed() {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn sleep_before_reconnect(policy: &super::ReconnectPolicy) {
    let delay = match policy {
        super::ReconnectPolicy::Disabled => None,
        super::ReconnectPolicy::Fixed { delay_ms, .. } => Some(Duration::from_millis(*delay_ms)),
        super::ReconnectPolicy::Exponential { base_delay_ms, .. } => {
            Some(Duration::from_millis(*base_delay_ms))
        }
    };
    if let Some(delay) = delay {
        tokio::time::sleep(delay).await;
    }
}

pub(crate) struct TokioRunnerEvents {
    sender: tokio_mpsc::Sender<super::ImEvent>,
    status: watch::Sender<super::RealtimeStatus>,
    subscriptions: Vec<super::RealtimeSubscription>,
}

impl TokioRunnerEvents {
    fn set_status(&self, state: super::RealtimeConnectionState, last_error: Option<String>) {
        let _ = self.status.send(super::RealtimeStatus {
            connected: state == super::RealtimeConnectionState::Connected,
            state,
            subscriptions: self.subscriptions.clone(),
            last_error,
        });
    }
}

impl RunnerEvents for TokioRunnerEvents {
    fn emit(&mut self, event: super::ImEvent) -> Result<(), String> {
        if let super::ImEvent::ConnectionStateChanged(state) = &event {
            self.set_status(state.state.clone(), state.reason.clone());
        }
        self.sender
            .try_send(event)
            .map_err(|_| "realtime event buffer is full or closed".to_string())
    }
}

#[cfg(feature = "blocking")]
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
    #[cfg(feature = "sqlite")]
    let projector = AsyncFirstSecureRealtimeNotificationProjector::new(client);
    #[cfg(not(feature = "sqlite"))]
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

#[cfg(feature = "blocking")]
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
    #[cfg(feature = "sqlite")]
    let projector = AsyncFirstSecureRealtimeNotificationProjector::new(client);
    #[cfg(not(feature = "sqlite"))]
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

#[cfg(feature = "blocking")]
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
            #[cfg(feature = "sqlite")]
            let projector = AsyncFirstSecureRealtimeNotificationProjector::new(&client);
            #[cfg(not(feature = "sqlite"))]
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

pub(crate) async fn spawn_default_async(
    client: crate::core::ImClient,
    options: super::RealtimeOptions,
) -> crate::ImResult<super::RealtimeSession> {
    crate::internal::realtime::transport::require_realtime_auth_token_async(&client).await?;
    let (event_sender, event_receiver) = tokio_mpsc::channel(options.event_buffer);
    let initial_status = super::session::initial_realtime_status(
        &options,
        super::RealtimeConnectionState::Disconnected,
        None,
    );
    let (status_sender, status_receiver) = watch::channel(initial_status);
    let (exit_sender, exit_receiver) = oneshot::channel();
    let shutdown = super::ShutdownSignal::pending();
    let worker_shutdown = shutdown.clone();
    let worker_options = options.clone();
    let worker = tokio::spawn(async move {
        let mut transport = AsyncDefaultRunnerTransport {
            client: client.clone(),
            socket: None,
            idle_ticks: 0,
        };
        let events = TokioRunnerEvents {
            sender: event_sender,
            status: status_sender,
            subscriptions: worker_options.subscriptions.clone(),
        };
        #[cfg(feature = "sqlite")]
        let projector = AsyncSecureRealtimeNotificationProjector::new(&client);
        #[cfg(not(feature = "sqlite"))]
        let projector = SecureRealtimeNotificationProjector {
            client: &client,
            directory_transport: crate::internal::transport::CoreHttpTransport::new(&client),
        };
        #[cfg(feature = "sqlite")]
        let mut projector = AsyncLocalStateRealtimeNotificationProjector {
            client: client.clone(),
            inner: projector,
        };
        #[cfg(not(feature = "sqlite"))]
        let mut projector = projector;
        let exit = run_realtime_async_transport_until_shutdown(
            worker_options,
            worker_shutdown,
            super::RealtimeControl::default(),
            &mut transport,
            events,
            &mut projector,
        )
        .await
        .unwrap_or_else(|err| super::RealtimeExit {
            reason: exit_reason_for_connect_error(&err),
            reconnect_attempts: 0,
            warnings: vec![err.to_string()],
        });
        let _ = exit_sender.send(exit);
    });
    Ok(super::RealtimeSession::new(
        event_receiver,
        status_receiver,
        shutdown,
        exit_receiver,
        Some(worker),
    ))
}

struct AsyncDefaultRunnerTransport {
    client: crate::core::ImClient,
    socket: Option<crate::internal::realtime::async_ws_transport::AsyncWsTransport>,
    idle_ticks: u32,
}

impl AsyncRealtimeRunnerTransport for AsyncDefaultRunnerTransport {
    async fn connect(&mut self) -> crate::ImResult<()> {
        let socket =
            crate::internal::realtime::transport::connect_async_websocket_session(&self.client)
                .await?;
        self.socket = Some(socket);
        self.idle_ticks = 0;
        Ok(())
    }

    async fn next_notification(&mut self) -> crate::ImResult<Option<Value>> {
        let Some(socket) = self.socket.as_mut() else {
            return Ok(None);
        };
        match tokio::time::timeout(Duration::from_millis(250), socket.read_json_message()).await {
            Ok(Ok(Some(message))) => {
                self.idle_ticks = 0;
                Ok(Some(Value::Object(message)))
            }
            Ok(Ok(None)) => Ok(None),
            Ok(Err(err)) => Err(err),
            Err(_) => {
                self.idle_ticks = self.idle_ticks.saturating_add(1);
                if self.idle_ticks >= 60 {
                    self.idle_ticks = 0;
                    socket.ping().await?;
                }
                Ok(Some(Value::Null))
            }
        }
    }
}

#[cfg(feature = "blocking")]
struct DefaultRunnerTransport<'a> {
    client: &'a crate::core::ImClient,
    socket: Option<crate::internal::realtime::ws_transport::WsTransport>,
    idle_ticks: u32,
}

#[cfg(feature = "blocking")]
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

#[cfg(feature = "blocking")]
struct SenderRunnerEvents {
    sender: mpsc::SyncSender<super::ImEvent>,
}

#[cfg(feature = "blocking")]
impl RealtimeRunnerEventSink for SenderRunnerEvents {
    fn emit(&mut self, event: super::ImEvent) -> crate::ImResult<()> {
        self.sender
            .try_send(event)
            .map_err(|_| crate::ImError::TransportUnavailable {
                detail: "realtime event buffer is full or closed".to_string(),
            })
    }
}

#[cfg(feature = "blocking")]
fn outcome(
    receiver: mpsc::Receiver<super::ImEvent>,
    control: super::RealtimeControl,
    reason: super::RealtimeExitReason,
    reconnect_attempts: u32,
    warnings: Vec<String>,
) -> RealtimeRunnerOutcome {
    RealtimeRunnerOutcome {
        exit: super::RealtimeExit {
            reason: reason.clone(),
            reconnect_attempts,
            warnings: warnings.clone(),
        },
        handle: super::RealtimeHandle::new(receiver, control),
    }
}

fn realtime_exit(
    reason: super::RealtimeExitReason,
    reconnect_attempts: u32,
    warnings: Vec<String>,
) -> super::RealtimeExit {
    super::RealtimeExit {
        reason,
        reconnect_attempts,
        warnings,
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
    #[cfg(feature = "group-e2ee")]
    use anp::group_e2ee::operations::{
        add_member_prepare, create_group_prepare, encrypt, finalize_commit, generate_key_package,
        process_welcome, AddMemberInput, CreateGroupInput, EncryptInput, FinalizeCommitInput,
        GenerateKeyPackageInput, ProcessWelcomeInput,
    };
    #[cfg(feature = "group-e2ee")]
    use anp::group_e2ee::storage::ImCoreSqliteGroupMlsStore;
    #[cfg(feature = "group-e2ee")]
    use anp::group_e2ee::{GroupApplicationPlaintext, GroupStateRef};
    use serde_json::json;
    use std::collections::VecDeque;

    use super::*;

    #[test]
    #[cfg(feature = "blocking")]
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
    #[cfg(feature = "blocking")]
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

    #[tokio::test]
    async fn realtime_async_local_state_projector_uses_db_actor_for_message_projection() {
        let fixture = TestClientFixture::new("async-local-state");
        let client = fixture.client();
        let mut projector = AsyncLocalStateRealtimeNotificationProjector {
            client: client.clone(),
            inner: FixedProjector {
                event: Some(super::super::ImEvent::MessageReceived(
                    direct_message_event(client.did().as_str()),
                )),
            },
        };

        let outcome = projector
            .project_async(json!({"method": "test.direct"}))
            .await;

        assert!(outcome.warnings.is_empty());
        assert!(matches!(
            outcome.event,
            Some(super::super::ImEvent::MessageReceived(_))
        ));
        let db = client.core_inner().local_state_db().await.unwrap();
        db.shutdown().await.unwrap();
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

    #[tokio::test]
    async fn realtime_async_projector_uses_actor_cas_for_direct_cipher() {
        let fixture = TestClientFixture::new("async-direct-cipher");
        fixture.write_identity("did:example:alice", "test-key", "test-key");
        let client = fixture.client();
        let exchange =
            crate::internal::secure_direct::async_receive::test_support::established_exchange();
        let db = client.core_inner().local_state_db().await.unwrap();
        db.save_direct_secure_session_if_revision(
            crate::internal::secure_direct::sqlite_store::DirectSessionRecord {
                owner_identity_id: "alice".to_owned(),
                owner_did: "did:example:alice".to_owned(),
                peer_did: "did:example:bob".to_owned(),
                session_id: exchange.alice_session.session_id.clone(),
                state_blob: crate::internal::secure_direct::sqlite_store::direct_session_to_blob(
                    &exchange.alice_session,
                )
                .unwrap(),
                metadata_json:
                    crate::internal::secure_direct::sqlite_store::direct_session_metadata_json(
                        &exchange.alice_session,
                    )
                    .unwrap(),
                revision: 0,
                created_at: "2026-05-24T00:00:00Z".to_owned(),
                updated_at: "2026-05-24T00:00:00Z".to_owned(),
            },
            0,
        )
        .await
        .unwrap();
        let mut projector = AsyncSecureRealtimeNotificationProjector::new(&client);
        let outcome = projector
            .project_async(
                crate::internal::secure_direct::async_receive::test_support::direct_cipher_realtime_notification(
                    &exchange.message_metadata,
                    &exchange.cipher_body,
                ),
            )
            .await;

        assert!(outcome.warnings.is_empty());
        let Some(super::super::ImEvent::MessageReceived(event)) = outcome.event else {
            panic!("expected direct message event");
        };
        assert_eq!(event.message.id.as_str(), "msg-async-receive");
        assert_eq!(
            event.message.body,
            crate::messages::MessageBodyView::Text {
                text: "async receive secret".to_owned(),
                kind: crate::messages::MessageKind::Text,
            }
        );

        let saved = db
            .get_direct_secure_session("alice", "did:example:bob")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(saved.revision, 1);
        db.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn realtime_async_projector_replays_pending_direct_cipher_after_init() {
        let exchange =
            crate::internal::secure_direct::async_receive::test_support::incoming_init_exchange();
        let fixture = TestClientFixture::new("async-direct-pending-replay");
        fixture.write_identity(
            &exchange.recipient_did,
            "test-key",
            &exchange.recipient_agreement_private.to_pem(),
        );
        fixture.cache_identity_document(&exchange.sender_did, &exchange.sender_document);
        seed_direct_init_prekeys(&fixture, &exchange);
        let client = fixture.client();
        let mut sender_session = exchange.sender_session.clone();
        sender_session.status = anp::direct_e2ee::models::SESSION_STATUS_ESTABLISHED.to_owned();
        sender_session.recv_chain_key_b64u = sender_session.send_chain_key_b64u.clone();
        sender_session.peer_ratchet_public_key_b64u =
            Some(sender_session.ratchet_public_key_b64u.clone());
        sender_session.send_n = 1;
        let follow_up_metadata = anp::direct_e2ee::DirectEnvelopeMetadata {
            sender_did: exchange.sender_did.clone(),
            recipient_did: exchange.recipient_did.clone(),
            message_id: "msg-realtime-pending-follow-up".to_owned(),
            profile: "anp.direct.e2ee.v1".to_owned(),
            security_profile: "direct-e2ee".to_owned(),
        };
        let (_, follow_up_body) = anp::direct_e2ee::DirectE2eeSession::encrypt_follow_up(
            &mut sender_session,
            &follow_up_metadata,
            "msg-realtime-pending-follow-up",
            &anp::direct_e2ee::ApplicationPlaintext::new_text(
                "text/plain",
                "replayed realtime follow-up",
            ),
        )
        .unwrap();
        let mut projector = AsyncSecureRealtimeNotificationProjector::new(&client);

        let pending_outcome = projector
            .project_async(
                crate::internal::secure_direct::async_receive::test_support::direct_cipher_realtime_notification(
                    &follow_up_metadata,
                    &follow_up_body,
                ),
            )
            .await;

        assert!(pending_outcome.additional_events.is_empty());
        let init_outcome = projector
            .project_async(json!({
                "method": "direct.incoming",
                "params": crate::internal::secure_direct::async_receive::test_support::direct_init_notification(
                    &exchange.metadata,
                    &exchange.init_body,
                )
            }))
            .await;

        assert!(init_outcome.warnings.is_empty());
        let replayed = init_outcome
            .additional_events
            .iter()
            .find_map(|event| match event {
                super::super::ImEvent::MessageReceived(event)
                    if event.message.id.as_str() == "msg-realtime-pending-follow-up" =>
                {
                    Some(event)
                }
                _ => None,
            })
            .expect("expected pending direct cipher to replay after init");
        assert_eq!(
            replayed.message.body,
            crate::messages::MessageBodyView::Text {
                text: "replayed realtime follow-up".to_owned(),
                kind: crate::messages::MessageKind::Text,
            }
        );
        let db = client.core_inner().local_state_db().await.unwrap();
        let saved = db
            .get_direct_secure_session("alice", &exchange.sender_did)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(saved.revision, 1);
        let saved_session = crate::internal::secure_direct::sqlite_store::direct_session_from_blob(
            &saved.state_blob,
        )
        .unwrap();
        assert_eq!(saved_session.recv_n, 2);
        db.shutdown().await.unwrap();
    }

    #[cfg(feature = "group-e2ee")]
    #[tokio::test]
    async fn realtime_async_projector_uses_async_group_e2ee_normalizer() {
        let fixture = TestClientFixture::new("async-group-e2ee-realtime");
        fixture.write_identity_with_device(
            "did:wba:example.com:users:bob:e1",
            "test-key",
            "test-key",
            crate::internal::group_e2ee::DEFAULT_GROUP_MLS_DEVICE_ID,
        );
        let client = fixture.client();
        let group_did = "did:wba:example.com:groups:realtime-e2ee:e1";
        let cipher = prepare_group_e2ee_realtime_cipher(&fixture, group_did);
        let mut projector = AsyncSecureRealtimeNotificationProjector::new(&client);

        let outcome = projector
            .project_async(group_e2ee_realtime_notification(group_did, cipher))
            .await;

        assert!(outcome.warnings.is_empty());
        let Some(super::super::ImEvent::MessageReceived(event)) = outcome.event else {
            panic!("expected group message event");
        };
        assert_eq!(
            event.message.id.as_str(),
            "did:wba:example.com:groups:realtime-e2ee:e1:7"
        );
        assert_eq!(
            event.message.body,
            crate::messages::MessageBodyView::Text {
                text: "async realtime group secret".to_owned(),
                kind: crate::messages::MessageKind::Text,
            }
        );
        assert_eq!(event.message.group.as_ref().unwrap().as_str(), group_did);
    }

    #[tokio::test]
    async fn realtime_async_runner_uses_tokio_channels_and_status_watch() {
        let options = super::super::RealtimeOptions {
            event_buffer: 8,
            ..super::super::RealtimeOptions::default()
        };
        let (sender, mut receiver) = tokio_mpsc::channel(options.event_buffer);
        let (status_sender, status_receiver) =
            watch::channel(super::super::session::initial_realtime_status(
                &options,
                super::super::RealtimeConnectionState::Disconnected,
                None,
            ));
        let events = TokioRunnerEvents {
            sender,
            status: status_sender,
            subscriptions: options.subscriptions.clone(),
        };
        let mut transport = FakeAsyncRealtimeTransport {
            connect_attempts: 0,
            connect_results: VecDeque::from([Ok(())]),
            notifications: VecDeque::from([
                Ok(Some(json!({
                    "method": "local.notification",
                    "params": {"id": "async-local-1", "title": "async title"}
                }))),
                Ok(None),
            ]),
            shutdown_on_next_notification: None,
        };
        let control = super::super::RealtimeControl::default();
        let mut projector = PlainRealtimeNotificationProjector;

        let exit = run_realtime_async_transport_until_shutdown(
            options,
            super::super::ShutdownSignal::pending(),
            control,
            &mut transport,
            events,
            &mut projector,
        )
        .await
        .unwrap();

        assert_eq!(
            exit.reason,
            super::super::RealtimeExitReason::ConnectionClosed
        );
        assert_eq!(transport.connect_attempts, 1);
        let mut observed = Vec::new();
        while let Some(event) = receiver.recv().await {
            observed.push(event);
        }
        assert!(matches!(
            observed.as_slice(),
            [
                super::super::ImEvent::ConnectionStateChanged(
                    super::super::ConnectionStateChanged {
                        state: super::super::RealtimeConnectionState::Connecting,
                        ..
                    },
                ),
                super::super::ImEvent::ConnectionStateChanged(
                    super::super::ConnectionStateChanged {
                        state: super::super::RealtimeConnectionState::Connected,
                        ..
                    },
                ),
                super::super::ImEvent::LocalNotification(
                    super::super::LocalNotificationEvent {
                        notification_id: Some(id),
                        ..
                    },
                ),
                super::super::ImEvent::ConnectionStateChanged(
                    super::super::ConnectionStateChanged {
                        state: super::super::RealtimeConnectionState::Closed,
                        ..
                    },
                ),
            ] if id == "async-local-1"
        ));
        assert_eq!(
            status_receiver.borrow().state,
            super::super::RealtimeConnectionState::Closed
        );
    }

    #[tokio::test]
    async fn realtime_async_runner_stops_on_shutdown_signal() {
        let options = super::super::RealtimeOptions::default();
        let (sender, mut receiver) = tokio_mpsc::channel(options.event_buffer);
        let (status_sender, _status_receiver) =
            watch::channel(super::super::session::initial_realtime_status(
                &options,
                super::super::RealtimeConnectionState::Disconnected,
                None,
            ));
        let events = TokioRunnerEvents {
            sender,
            status: status_sender,
            subscriptions: options.subscriptions.clone(),
        };
        let control = super::super::RealtimeControl::default();
        let mut transport = FakeAsyncRealtimeTransport {
            connect_attempts: 0,
            connect_results: VecDeque::from([Ok(())]),
            notifications: VecDeque::from([Ok(Some(Value::Null))]),
            shutdown_on_next_notification: Some(control.clone()),
        };
        let mut projector = PlainRealtimeNotificationProjector;

        let exit = run_realtime_async_transport_until_shutdown(
            options,
            super::super::ShutdownSignal::pending(),
            control,
            &mut transport,
            events,
            &mut projector,
        )
        .await
        .unwrap();

        assert_eq!(
            exit.reason,
            super::super::RealtimeExitReason::ShutdownRequested
        );
        let events = {
            let mut observed = Vec::new();
            while let Some(event) = receiver.recv().await {
                observed.push(event);
            }
            observed
        };
        assert!(matches!(
            events.last(),
            Some(super::super::ImEvent::ConnectionStateChanged(
                super::super::ConnectionStateChanged {
                    state: super::super::RealtimeConnectionState::Closed,
                    reason: Some(reason),
                },
            )) if reason == "shutdown requested"
        ));
    }

    #[tokio::test]
    async fn realtime_async_runner_exits_when_event_buffer_is_full() {
        let options = super::super::RealtimeOptions {
            event_buffer: 1,
            ..super::super::RealtimeOptions::default()
        };
        let (sender, receiver) = tokio_mpsc::channel(options.event_buffer);
        let (status_sender, status_receiver) =
            watch::channel(super::super::session::initial_realtime_status(
                &options,
                super::super::RealtimeConnectionState::Disconnected,
                None,
            ));
        let events = TokioRunnerEvents {
            sender,
            status: status_sender,
            subscriptions: options.subscriptions.clone(),
        };
        let mut transport = FakeAsyncRealtimeTransport {
            connect_attempts: 0,
            connect_results: VecDeque::from([Ok(())]),
            notifications: VecDeque::from([Ok(Some(json!({
                "method": "local.notification",
                "params": {"id": "buffered-local", "title": "buffered"}
            })))]),
            shutdown_on_next_notification: None,
        };
        let mut projector = PlainRealtimeNotificationProjector;

        let exit = run_realtime_async_transport_until_shutdown(
            options,
            super::super::ShutdownSignal::pending(),
            super::super::RealtimeControl::default(),
            &mut transport,
            events,
            &mut projector,
        )
        .await
        .unwrap();

        assert_eq!(
            exit.reason,
            super::super::RealtimeExitReason::ConnectionClosed
        );
        assert_eq!(
            exit.warnings,
            vec!["realtime event buffer is full or closed".to_owned()]
        );
        assert_eq!(
            status_receiver.borrow().state,
            super::super::RealtimeConnectionState::Closed
        );
        assert_eq!(
            status_receiver.borrow().last_error.as_deref(),
            Some("realtime event buffer is full or closed")
        );
        drop(receiver);
    }

    struct FixedProjector {
        event: Option<super::super::ImEvent>,
    }

    struct FakeAsyncRealtimeTransport {
        connect_attempts: usize,
        connect_results: VecDeque<crate::ImResult<()>>,
        notifications: VecDeque<crate::ImResult<Option<Value>>>,
        shutdown_on_next_notification: Option<super::super::RealtimeControl>,
    }

    impl AsyncRealtimeRunnerTransport for FakeAsyncRealtimeTransport {
        async fn connect(&mut self) -> crate::ImResult<()> {
            self.connect_attempts += 1;
            self.connect_results.pop_front().unwrap_or(Ok(()))
        }

        async fn next_notification(&mut self) -> crate::ImResult<Option<Value>> {
            if let Some(control) = self.shutdown_on_next_notification.take() {
                control.shutdown();
            }
            self.notifications.pop_front().unwrap_or(Ok(None))
        }
    }

    impl RealtimeNotificationProjector for FixedProjector {
        fn project(&mut self, _notification: serde_json::Value) -> RealtimeProjectionOutcome {
            RealtimeProjectionOutcome {
                event: self.event.take(),
                additional_events: Vec::new(),
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
            std::fs::create_dir_all(root.join("identities").join("alice")).unwrap();
            std::fs::create_dir_all(root.join("local")).unwrap();
            std::fs::write(root.join("identities").join("default"), "alice\n").unwrap();
            let fixture = Self { root };
            fixture.write_identity("did:example:alice", "test-key", "test-key");
            fixture
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

        fn write_identity(
            &self,
            did: &str,
            signing_private_pem: &str,
            agreement_private_pem: &str,
        ) {
            self.write_identity_with_device(did, signing_private_pem, agreement_private_pem, "");
        }

        fn write_identity_with_device(
            &self,
            did: &str,
            signing_private_pem: &str,
            agreement_private_pem: &str,
            device_id: &str,
        ) {
            let identity_dir = self.root.join("identities").join("alice");
            let mut identity = json!({
                "id": "alice",
                "did": did,
                "local_alias": "alice",
                "ready_for_auth": true,
                "ready_for_messaging": true,
                "missing": []
            });
            if !device_id.trim().is_empty() {
                identity
                    .as_object_mut()
                    .unwrap()
                    .insert("device_id".to_owned(), json!(device_id));
            }
            std::fs::write(
                self.root.join("identities").join("registry.json"),
                json!({
                    "default_identity": "alice",
                    "identities": [identity]
                })
                .to_string(),
            )
            .unwrap();
            std::fs::write(
                identity_dir.join("did.json"),
                json!({
                    "id": did,
                    "verificationMethod": [],
                })
                .to_string(),
            )
            .unwrap();
            std::fs::write(identity_dir.join("private.key"), signing_private_pem).unwrap();
            std::fs::write(
                identity_dir.join("e2ee-agreement-private.pem"),
                agreement_private_pem,
            )
            .unwrap();
            std::fs::write(
                identity_dir.join("auth.json"),
                r#"{"jwt_token":"test-token"}"#,
            )
            .unwrap();
        }

        fn cache_identity_document(&self, did: &str, document: &serde_json::Value) {
            let safe_name = did
                .chars()
                .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
                .collect::<String>();
            let identity_dir = self.root.join("identities").join(safe_name.clone());
            std::fs::create_dir_all(&identity_dir).unwrap();
            std::fs::write(
                identity_dir.join("did.json"),
                serde_json::to_vec_pretty(document).unwrap(),
            )
            .unwrap();
            let registry_path = self.root.join("identities").join("registry.json");
            let mut registry = std::fs::read_to_string(&registry_path)
                .ok()
                .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
                .unwrap_or_else(|| {
                    json!({
                        "default_identity": "alice",
                        "identities": []
                    })
                });
            let identities = registry
                .get_mut("identities")
                .and_then(serde_json::Value::as_array_mut)
                .unwrap();
            identities.push(json!({
                "id": safe_name,
                "did": did,
                "dir_name": safe_name,
                "local_alias": safe_name,
                "ready_for_auth": false,
                "ready_for_messaging": false,
                "missing": []
            }));
            std::fs::write(registry_path, registry.to_string()).unwrap();
        }
    }

    fn seed_direct_init_prekeys(
        fixture: &TestClientFixture,
        exchange: &crate::internal::secure_direct::async_receive::test_support::IncomingInitExchange,
    ) {
        let connection =
            crate::internal::local_state::open_writable(&fixture.sqlite_path()).unwrap();
        let store =
            crate::internal::secure_direct::sqlite_store::SqliteDirectSecureStateStore::new(
                &connection,
            )
            .unwrap();
        store
            .upsert_signed_prekey(
                &crate::internal::secure_direct::sqlite_store::DirectSignedPrekeyRecord {
                    owner_identity_id: "alice".to_owned(),
                    owner_did: exchange.recipient_did.clone(),
                    key_id: exchange.recipient_signed_prekey.key_id.clone(),
                    private_key_blob: exchange
                        .recipient_signed_prekey_private
                        .to_pem()
                        .into_bytes(),
                    public_key_blob: exchange
                        .recipient_signed_prekey
                        .public_key_b64u
                        .as_bytes()
                        .to_vec(),
                    status:
                        crate::internal::secure_direct::sqlite_store::DirectPrekeyStatus::Active,
                    metadata_json: serde_json::to_string(&json!({
                        "metadata": exchange.recipient_signed_prekey,
                    }))
                    .unwrap(),
                    created_at: "2026-05-24T00:00:00Z".to_owned(),
                    updated_at: "2026-05-24T00:00:00Z".to_owned(),
                },
            )
            .unwrap();
        store
            .upsert_one_time_prekey(
                &crate::internal::secure_direct::sqlite_store::DirectOneTimePrekeyRecord {
                    owner_identity_id: "alice".to_owned(),
                    owner_did: exchange.recipient_did.clone(),
                    key_id: exchange.recipient_one_time_prekey.key_id.clone(),
                    private_key_blob: exchange
                        .recipient_one_time_prekey_private
                        .to_pem()
                        .into_bytes(),
                    public_key_blob: exchange
                        .recipient_one_time_prekey
                        .public_key_b64u
                        .as_bytes()
                        .to_vec(),
                    status:
                        crate::internal::secure_direct::sqlite_store::DirectPrekeyStatus::Available,
                    metadata_json: serde_json::to_string(&json!({
                        "metadata": exchange.recipient_one_time_prekey,
                    }))
                    .unwrap(),
                    created_at: "2026-05-24T00:00:00Z".to_owned(),
                    consumed_at: String::new(),
                },
            )
            .unwrap();
    }

    #[cfg(feature = "group-e2ee")]
    fn prepare_group_e2ee_realtime_cipher(
        fixture: &TestClientFixture,
        group_did: &str,
    ) -> anp::group_e2ee::GroupCipherObject {
        let alice_store = ImCoreSqliteGroupMlsStore::from_local_state_sqlite_path(
            fixture.root.join("alice-local").join("im.sqlite"),
            "alice-sender",
            "did:wba:example.com:users:alice:e1",
            crate::internal::group_e2ee::DEFAULT_GROUP_MLS_DEVICE_ID,
        )
        .unwrap();
        let bob_store = ImCoreSqliteGroupMlsStore::from_local_state_sqlite_path(
            fixture.sqlite_path(),
            "alice",
            "did:wba:example.com:users:bob:e1",
            crate::internal::group_e2ee::DEFAULT_GROUP_MLS_DEVICE_ID,
        )
        .unwrap();
        let bob_key_package = generate_key_package(
            &bob_store,
            GenerateKeyPackageInput {
                owner_did: "did:wba:example.com:users:bob:e1".to_owned(),
                device_id: crate::internal::group_e2ee::DEFAULT_GROUP_MLS_DEVICE_ID.to_owned(),
                operation_id: "op-realtime-bob-kp".to_owned(),
                request_id: "req-realtime-bob-kp".to_owned(),
                key_package_id: None,
                purpose: None,
                group_did: Some(group_did.to_owned()),
            },
        )
        .unwrap();
        let create = create_group_prepare(
            &alice_store,
            CreateGroupInput {
                creator_did: "did:wba:example.com:users:alice:e1".to_owned(),
                device_id: crate::internal::group_e2ee::DEFAULT_GROUP_MLS_DEVICE_ID.to_owned(),
                group_did: group_did.to_owned(),
                operation_id: "op-realtime-create".to_owned(),
                request_id: "req-realtime-create".to_owned(),
                pending_commit_id: Some("pc-realtime-create".to_owned()),
            },
        )
        .unwrap();
        finalize_commit(
            &alice_store,
            FinalizeCommitInput {
                pending_commit_id: create.pending_commit_id,
                request_id: "req-realtime-create-finalize".to_owned(),
            },
        )
        .unwrap();
        let add = add_member_prepare(
            &alice_store,
            AddMemberInput {
                actor_did: "did:wba:example.com:users:alice:e1".to_owned(),
                device_id: crate::internal::group_e2ee::DEFAULT_GROUP_MLS_DEVICE_ID.to_owned(),
                group_did: group_did.to_owned(),
                member_did: "did:wba:example.com:users:bob:e1".to_owned(),
                group_key_package: bob_key_package.group_key_package,
                operation_id: "op-realtime-add-bob".to_owned(),
                request_id: "req-realtime-add-bob".to_owned(),
                pending_commit_id: Some("pc-realtime-add-bob".to_owned()),
            },
        )
        .unwrap();
        finalize_commit(
            &alice_store,
            FinalizeCommitInput {
                pending_commit_id: add.pending_commit_id.clone(),
                request_id: "req-realtime-add-finalize".to_owned(),
            },
        )
        .unwrap();
        process_welcome(
            &bob_store,
            ProcessWelcomeInput {
                agent_did: "did:wba:example.com:users:bob:e1".to_owned(),
                device_id: crate::internal::group_e2ee::DEFAULT_GROUP_MLS_DEVICE_ID.to_owned(),
                group_did: group_did.to_owned(),
                welcome_b64u: add.welcome_b64u.expect("welcome"),
                ratchet_tree_b64u: add.ratchet_tree_b64u.expect("ratchet tree"),
                group_state_ref: GroupStateRef {
                    group_did: group_did.to_owned(),
                    group_state_version: "1".to_owned(),
                    policy_hash: None,
                },
                crypto_group_id_b64u: add.crypto_group_id_b64u,
                epoch: add.epoch,
                request_id: "req-realtime-bob-welcome".to_owned(),
            },
        )
        .unwrap();
        encrypt(
            &alice_store,
            EncryptInput {
                sender_did: "did:wba:example.com:users:alice:e1".to_owned(),
                device_id: crate::internal::group_e2ee::DEFAULT_GROUP_MLS_DEVICE_ID.to_owned(),
                group_state_ref: GroupStateRef {
                    group_did: group_did.to_owned(),
                    group_state_version: "1".to_owned(),
                    policy_hash: None,
                },
                message_id: "msg-group-realtime-e2ee".to_owned(),
                operation_id: "op-group-realtime-e2ee".to_owned(),
                application_plaintext: GroupApplicationPlaintext {
                    application_content_type: "text/plain".to_owned(),
                    thread_id: Some(group_did.to_owned()),
                    reply_to_message_id: None,
                    annotations: Default::default(),
                    text: Some("async realtime group secret".to_owned()),
                    payload: None,
                    payload_b64u: None,
                },
                request_id: "req-realtime-encrypt".to_owned(),
            },
        )
        .unwrap()
        .group_cipher_object
    }

    #[cfg(feature = "group-e2ee")]
    fn group_e2ee_realtime_notification(
        group_did: &str,
        cipher: anp::group_e2ee::GroupCipherObject,
    ) -> serde_json::Value {
        let mut body = serde_json::to_value(cipher).unwrap();
        if let Some(object) = body.as_object_mut() {
            object.insert("group_did".to_owned(), json!(group_did));
            object.insert("group_event_seq".to_owned(), json!(7));
        }
        json!({
            "method": "group.incoming",
            "params": {
                "meta": {
                    "message_id": "msg-group-realtime-e2ee",
                    "operation_id": "op-group-realtime-e2ee",
                    "sender_did": "did:wba:example.com:users:alice:e1",
                    "target": {
                        "kind": "group",
                        "did": group_did
                    },
                    "content_type": anp::group_e2ee::GROUP_CIPHER_CONTENT_TYPE,
                    "created_at": "2026-05-24T00:00:00Z"
                },
                "body": body
            }
        })
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
