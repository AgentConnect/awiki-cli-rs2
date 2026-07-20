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
        if let Some(warnings) = project_old_admin_recovery_notice_realtime(
            self.client,
            &notification,
            time::OffsetDateTime::now_utc(),
        ) {
            return RealtimeProjectionOutcome {
                event: None,
                additional_events: Vec::new(),
                warnings,
            };
        }
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
        if let Some(warnings) = project_old_admin_recovery_notice_realtime_async(
            &self.client,
            &notification,
            time::OffsetDateTime::now_utc(),
        )
        .await
        {
            return RealtimeProjectionOutcome {
                event: None,
                additional_events: Vec::new(),
                warnings,
            };
        }
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
fn project_old_admin_recovery_notice_realtime(
    client: &crate::core::ImClient,
    notification: &Value,
    now: time::OffsetDateTime,
) -> Option<Vec<String>> {
    use crate::internal::identity_recovery_notice::RealtimeRecoveryNoticeProjection;

    match crate::internal::identity_recovery_notice::classify_realtime_notification(
        client,
        notification,
        now,
    ) {
        RealtimeRecoveryNoticeProjection::NotRecoveryControl => None,
        RealtimeRecoveryNoticeProjection::UnknownRecoveryControl => Some(vec![
            "unknown identity recovery realtime control was rejected".to_owned(),
        ]),
        RealtimeRecoveryNoticeProjection::RecoveryNotice(Err(_)) => Some(vec![
            "identity.recovery_started realtime control was rejected".to_owned(),
        ]),
        RealtimeRecoveryNoticeProjection::RecoveryNotice(Ok(record)) => {
            let result = (|| {
                let mut connection = crate::internal::local_state::open_writable(
                    &client.core_inner().sdk_paths().local_state.sqlite_path,
                )?;
                crate::internal::local_state::old_admin_recovery_notices::upsert(
                    &mut connection,
                    &record,
                )
            })();
            Some(if result.is_ok() {
                Vec::new()
            } else {
                vec!["identity.recovery_started realtime projection failed".to_owned()]
            })
        }
    }
}

#[cfg(feature = "sqlite")]
async fn project_old_admin_recovery_notice_realtime_async(
    client: &crate::core::ImClient,
    notification: &Value,
    now: time::OffsetDateTime,
) -> Option<Vec<String>> {
    use crate::internal::identity_recovery_notice::RealtimeRecoveryNoticeProjection;

    match crate::internal::identity_recovery_notice::classify_realtime_notification(
        client,
        notification,
        now,
    ) {
        RealtimeRecoveryNoticeProjection::NotRecoveryControl => None,
        RealtimeRecoveryNoticeProjection::UnknownRecoveryControl => Some(vec![
            "unknown identity recovery realtime control was rejected".to_owned(),
        ]),
        RealtimeRecoveryNoticeProjection::RecoveryNotice(Err(_)) => Some(vec![
            "identity.recovery_started realtime control was rejected".to_owned(),
        ]),
        RealtimeRecoveryNoticeProjection::RecoveryNotice(Ok(record)) => {
            let result = match client.core_inner().local_state_db().await {
                Ok(db) => db.upsert_old_admin_recovery_notice(record).await,
                Err(error) => Err(error),
            };
            Some(if result.is_ok() {
                Vec::new()
            } else {
                vec!["identity.recovery_started realtime projection failed".to_owned()]
            })
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
    let peer_lookup = realtime_direct_peer_lookup(client, &event.message);
    let peer_scope = peer_lookup
        .as_ref()
        .and_then(direct_peer_scope_from_verified_lookup);
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
    if let Some(lookup) = peer_lookup.as_ref() {
        project_realtime_verified_handle(client, &mut connection, lookup)?;
    }
    let message_stored =
        crate::internal::realtime::local_projection::apply_realtime_message_local_projection(
            &mut connection,
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
    if message_stored {
        client.emit_committed_local_message_projection("realtime_incoming");
    }
    Ok(())
}

#[cfg(feature = "sqlite")]
async fn project_realtime_message_received_async(
    client: &crate::core::ImClient,
    event: &super::MessageReceivedEvent,
) -> crate::ImResult<()> {
    let peer_lookup = realtime_direct_peer_lookup_async(client, &event.message).await;
    project_realtime_message_received_async_with_lookup(client, event, peer_lookup).await
}

#[cfg(feature = "sqlite")]
async fn project_realtime_message_received_async_with_lookup(
    client: &crate::core::ImClient,
    event: &super::MessageReceivedEvent,
    peer_lookup: Option<crate::directory::HandleLookupResult>,
) -> crate::ImResult<()> {
    let peer_scope = peer_lookup
        .as_ref()
        .and_then(direct_peer_scope_from_verified_lookup);
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
    if let Some(lookup) = peer_lookup {
        db.project_verified_handle(
            client.current_identity().id.as_str(),
            client.did().as_str(),
            lookup,
        )
        .await?;
    }
    let outcome = db
        .store_remote_messages(vec![projection.into_record()], "realtime_message")
        .await?;
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
    if outcome.stored_messages > 0 {
        client.emit_committed_local_message_projection("realtime_incoming");
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
fn realtime_direct_peer_lookup(
    client: &crate::core::ImClient,
    message: &crate::messages::Message,
) -> Option<crate::directory::HandleLookupResult> {
    let peer_did = realtime_direct_peer_did(client, message)?;
    let mut transport = crate::internal::transport::CoreHttpTransport::new(client);
    let call = crate::internal::identity_wire::directory::build_handle_lookup_by_did_rpc_call(
        peer_did.as_str(),
    )
    .ok()?;
    let raw = RpcTransport::rpc(&mut transport, call.endpoint, call.method, call.params).ok()?;
    let lookup =
        crate::internal::directory_runtime::handle_lookup_from_value_with_client(client, &raw)
            .ok()?;
    (lookup.did.as_str() == peer_did).then_some(lookup)
}

#[cfg(feature = "sqlite")]
async fn realtime_direct_peer_lookup_async(
    client: &crate::core::ImClient,
    message: &crate::messages::Message,
) -> Option<crate::directory::HandleLookupResult> {
    let peer_did = realtime_direct_peer_did(client, message)?;
    let mut transport = crate::internal::transport::CoreHttpTransport::new(client);
    let call = crate::internal::identity_wire::directory::build_handle_lookup_by_did_rpc_call(
        peer_did.as_str(),
    )
    .ok()?;
    let raw = AsyncRpcTransport::rpc(&mut transport, call.endpoint, call.method, call.params)
        .await
        .ok()?;
    let lookup =
        crate::internal::directory_runtime::handle_lookup_from_value_with_client(client, &raw)
            .ok()?;
    (lookup.did.as_str() == peer_did).then_some(lookup)
}

#[cfg(feature = "sqlite")]
fn direct_peer_scope_from_verified_lookup(
    lookup: &crate::directory::HandleLookupResult,
) -> Option<crate::internal::local_state::owner_scope::DirectPeerScope> {
    crate::internal::local_state::owner_scope::DirectPeerScope::new(
        lookup.user_id.clone(),
        lookup.handle.as_str().to_owned(),
    )
    .ok()
}

#[cfg(all(feature = "blocking", feature = "sqlite"))]
fn project_realtime_verified_handle(
    client: &crate::core::ImClient,
    connection: &mut rusqlite::Connection,
    lookup: &crate::directory::HandleLookupResult,
) -> crate::ImResult<String> {
    crate::internal::local_state::peer_personas::project_verified_handle(
        connection,
        client.current_identity().id.as_str(),
        client.did().as_str(),
        lookup,
    )
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
    crate::internal::local_state::groups::upsert_group(&connection, record)?;
    if let Some(message) = realtime_group_system_message_record(client, event) {
        crate::internal::local_state::messages::upsert_message(&connection, &message)?;
    }
    Ok(())
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
    db.upsert_group(record).await?;
    if let Some(message) = realtime_group_system_message_record(client, event) {
        db.store_messages(vec![message]).await?;
    }
    Ok(())
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
fn realtime_group_system_message_record(
    client: &crate::core::ImClient,
    event: &super::GroupUpdatedEvent,
) -> Option<crate::internal::local_state::messages::MessageRecord> {
    let group_event_seq = event.group_event_seq?;
    if !matches!(
        event.update_kind,
        super::GroupUpdateKind::MemberAdded
            | super::GroupUpdateKind::MemberRemoved
            | super::GroupUpdateKind::Updated
    ) {
        return None;
    }
    let event_type = event
        .event_type
        .clone()
        .unwrap_or_else(|| group_update_kind_label(&event.update_kind).to_owned());
    crate::internal::group_system_events::record_from_input(
        client,
        crate::internal::group_system_events::GroupSystemEventInput {
            event_type,
            group_did: event.group.as_str().to_owned(),
            group_event_seq,
            group_state_version: event.group_state_version.clone(),
            actor_did: event.actor_did.clone(),
            subject_did: event.subject_did.clone(),
            subject_handle: event.subject_handle.clone(),
            previous_subject_did: event.previous_subject_did.clone(),
            handle_binding_generation: event.handle_binding_generation.clone(),
            membership_status: event.membership_status.clone(),
            changed_at: event.changed_at.clone(),
            sync_event_id: event.sync.as_ref().and_then(|sync| sync.event_id.clone()),
            sync_event_seq: event.sync.as_ref().and_then(|sync| sync.event_seq.clone()),
            sync_event_type: event.sync.as_ref().and_then(|sync| sync.event_type.clone()),
            source: "im-core.realtime".to_owned(),
        },
    )
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
    match parse_realtime_v2_session_control(&notification) {
        Ok(Some((metadata, body))) => {
            if client.core_inner().root_key_transfer_enabled() {
                if let Some(runtime) = runtime {
                    let core = client.core_handle();
                    let _ = runtime.block_on(
                        crate::internal::identity_root_transfer_runtime::receive_session_control(
                            &core, client, metadata, body,
                        ),
                    );
                }
            }
            return dropped_v2_session_control_projection();
        }
        // A strict session-control operation ID with a malformed P5 object is
        // still confidential control traffic. Fail closed instead of falling
        // back to a normal message or notification projection.
        Err(_) => return dropped_v2_session_control_projection(),
        Ok(None) => {}
    }
    if is_p5_v2_realtime_candidate(&notification) {
        if !client.core_inner().direct_e2ee_v2_enabled() {
            return dropped_v2_session_control_projection();
        }
        let Some(runtime) = runtime else {
            return dropped_v2_session_control_projection();
        };
        return runtime
            .block_on(project_p5_v2_realtime_notification(client, notification))
            .unwrap_or_else(|_| dropped_v2_session_control_projection());
    }
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
    match parse_realtime_v2_session_control(&notification) {
        Ok(Some((metadata, body))) => {
            if client.core_inner().root_key_transfer_enabled() {
                let core = client.core_handle();
                let _ = crate::internal::identity_root_transfer_runtime::receive_session_control(
                    &core, client, metadata, body,
                )
                .await;
            }
            return dropped_v2_session_control_projection();
        }
        Err(_) => return dropped_v2_session_control_projection(),
        Ok(None) => {}
    }
    if is_p5_v2_realtime_candidate(&notification) {
        if !client.core_inner().direct_e2ee_v2_enabled() {
            return dropped_v2_session_control_projection();
        }
        return project_p5_v2_realtime_notification(client, notification)
            .await
            .unwrap_or_else(|_| dropped_v2_session_control_projection());
    }
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

#[cfg(feature = "sqlite")]
fn parse_realtime_v2_session_control(
    notification: &Value,
) -> crate::ImResult<
    Option<(
        anp::direct_e2ee::V2DirectMetadata,
        anp::direct_e2ee::V2DirectBody,
    )>,
> {
    if notification.get("method").and_then(Value::as_str) != Some("direct.incoming") {
        return Ok(None);
    }
    let Some(params) = notification.get("params") else {
        return Ok(None);
    };
    crate::internal::message_runtime::read::parse_v2_session_control(params)
}

#[cfg(feature = "sqlite")]
fn dropped_v2_session_control_projection() -> DirectRealtimeProjectionResult {
    DirectRealtimeProjectionResult::Projected {
        notification: None,
        additional_notifications: Vec::new(),
        warnings: Vec::new(),
    }
}

fn is_p5_v2_realtime_candidate(notification: &Value) -> bool {
    notification.get("method").and_then(Value::as_str) == Some("direct.incoming")
        && notification
            .pointer("/params/meta/profile")
            .and_then(Value::as_str)
            == Some("anp.direct.e2ee.v2")
}

#[cfg(feature = "sqlite")]
async fn project_p5_v2_realtime_notification(
    client: &crate::core::ImClient,
    notification: Value,
) -> crate::ImResult<DirectRealtimeProjectionResult> {
    let params = notification
        .get("params")
        .cloned()
        .ok_or(crate::ImError::PermissionDenied)?;
    let (metadata, body) =
        crate::internal::secure_direct::v2_product::parse_v2_wire_message(&params)?
            .ok_or(crate::ImError::PermissionDenied)?;
    let core = client.core_handle();
    let outcome = crate::internal::secure_direct::v2_product::receive_for_client(
        &core, client, true, metadata, body,
    )
    .await?;
    let mut projected = notification;
    let params = projected
        .get_mut("params")
        .and_then(Value::as_object_mut)
        .ok_or(crate::ImError::PermissionDenied)?;
    params.remove("auth");
    let (logical_message_id, sender_did, sender_device_id, target_did, body, own_sync) =
        match outcome {
            crate::internal::secure_direct::v2_product::V2InboundProductOutcome::Business(
                projection,
            ) => (
                projection.logical_message_id,
                projection.sender_did,
                projection.sender_device_id,
                None,
                projection.body,
                false,
            ),
            crate::internal::secure_direct::v2_product::V2InboundProductOutcome::OwnSync(
                projection,
            ) => (
                projection.logical_message_id,
                projection.original_sender_did,
                projection.original_sender_device_id,
                Some(projection.target_did),
                projection.body,
                true,
            ),
            crate::internal::secure_direct::v2_product::V2InboundProductOutcome::Replay
            | crate::internal::secure_direct::v2_product::V2InboundProductOutcome::ConsumedControl
            | crate::internal::secure_direct::v2_product::V2InboundProductOutcome::SuppressedControl => {
                return Ok(dropped_v2_session_control_projection())
            }
        };
    let meta = params
        .get_mut("meta")
        .and_then(Value::as_object_mut)
        .ok_or(crate::ImError::PermissionDenied)?;
    meta.insert("message_id".to_owned(), Value::String(logical_message_id));
    meta.insert("sender_did".to_owned(), Value::String(sender_did));
    meta.insert(
        "sender_device_id".to_owned(),
        Value::String(sender_device_id),
    );
    if let Some(target_did) = target_did {
        meta.insert(
            "target".to_owned(),
            serde_json::json!({"kind": "agent", "did": target_did}),
        );
    }
    let (content_type, safe_body) = match body {
        crate::internal::secure_direct::v2_product::V2InboundBusinessBody::Text {
            text,
            markdown,
        } => (
            if markdown {
                "text/markdown"
            } else {
                "text/plain"
            },
            serde_json::json!({"text": text}),
        ),
        crate::internal::secure_direct::v2_product::V2InboundBusinessBody::Json { payload } => {
            ("application/json", serde_json::json!({"payload": payload}))
        }
        crate::internal::secure_direct::v2_product::V2InboundBusinessBody::Attachment {
            full_manifest,
        } => (
            crate::attachments::manifest::attachment_manifest_content_type(),
            serde_json::json!({
                "payload": crate::attachments::manifest::redact_attachment_manifest(&full_manifest)
            }),
        ),
    };
    meta.insert(
        "content_type".to_owned(),
        Value::String(content_type.to_owned()),
    );
    params.insert("body".to_owned(), safe_body);
    params.insert("secure".to_owned(), Value::Bool(true));
    params.insert(
        "secure_state".to_owned(),
        Value::String("decrypted".to_owned()),
    );
    if own_sync {
        params.insert("own_device_sync".to_owned(), Value::Bool(true));
    }
    Ok(DirectRealtimeProjectionResult::Projected {
        notification: Some(projected),
        additional_notifications: Vec::new(),
        warnings: Vec::new(),
    })
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
    if is_p5_v2_realtime_candidate(&notification) {
        (None, Vec::new())
    } else {
        (Some(notification), Vec::new())
    }
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
    if is_explicit_group_e2ee_notice_control(&notification)
        && notification
            .pointer("/params/meta/profile")
            .and_then(Value::as_str)
            != Some(anp::group_e2ee::GROUP_E2EE_PROFILE_V2)
        && notification
            .pointer("/params/meta/profile")
            .and_then(Value::as_str)
            != Some(anp::group_e2ee::PROFILE)
    {
        warnings.push("unknown group E2EE control notice was rejected".to_owned());
        return None;
    }
    if is_p6_v2_realtime_candidate(&notification) {
        if !client.core_inner().group_e2ee_v2_enabled() {
            return None;
        }
        let is_notice = notification.get("method").and_then(Value::as_str)
            == Some(anp::group_e2ee::METHOD_GROUP_NOTICE_V2);
        let Some(runtime) = runtime else {
            if is_notice {
                warnings.push("P6 v2 group control notice runtime was unavailable".to_owned());
            }
            return None;
        };
        if is_notice {
            if runtime
                .block_on(
                    crate::internal::group_e2ee::v2_notice::consume_for_client_async(
                        client,
                        &notification,
                    ),
                )
                .is_err()
            {
                warnings.push("P6 v2 group control notice was rejected".to_owned());
            }
            return None;
        }
        return runtime
            .block_on(
                crate::internal::message_runtime::read::normalize_p6_v2_realtime_incoming(
                    client,
                    &notification,
                ),
            )
            .ok();
    }
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
    if is_explicit_group_e2ee_notice_control(&notification)
        && notification
            .pointer("/params/meta/profile")
            .and_then(Value::as_str)
            != Some(anp::group_e2ee::GROUP_E2EE_PROFILE_V2)
        && notification
            .pointer("/params/meta/profile")
            .and_then(Value::as_str)
            != Some(anp::group_e2ee::PROFILE)
    {
        warnings.push("unknown group E2EE control notice was rejected".to_owned());
        return None;
    }
    if is_p6_v2_realtime_candidate(&notification) {
        if !client.core_inner().group_e2ee_v2_enabled() {
            return None;
        }
        if notification.get("method").and_then(Value::as_str)
            == Some(anp::group_e2ee::METHOD_GROUP_NOTICE_V2)
        {
            if crate::internal::group_e2ee::v2_notice::consume_for_client_async(
                client,
                &notification,
            )
            .await
            .is_err()
            {
                warnings.push("P6 v2 group control notice was rejected".to_owned());
            }
            return None;
        }
        return crate::internal::message_runtime::read::normalize_p6_v2_realtime_incoming(
            client,
            &notification,
        )
        .await
        .ok();
    }
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

fn is_explicit_group_e2ee_notice_control(notification: &Value) -> bool {
    notification.get("method").and_then(Value::as_str)
        == Some(anp::group_e2ee::METHOD_GROUP_NOTICE_V2)
}

fn is_p6_v2_realtime_candidate(notification: &Value) -> bool {
    matches!(
        notification.get("method").and_then(Value::as_str),
        Some("group.incoming") | Some("group.e2ee.notice")
    ) && notification
        .pointer("/params/meta/profile")
        .and_then(Value::as_str)
        == Some("anp.group.e2ee.v2")
}

#[cfg(all(feature = "blocking", not(feature = "group-e2ee")))]
fn normalize_group_e2ee_realtime_notification(
    _client: &crate::core::ImClient,
    notification: Value,
    _warnings: &mut Vec<String>,
) -> Option<Value> {
    (!is_p6_v2_realtime_candidate(&notification)
        && !is_explicit_group_e2ee_notice_control(&notification))
    .then_some(notification)
}

#[cfg(not(feature = "group-e2ee"))]
async fn normalize_group_e2ee_realtime_notification_async(
    _client: &crate::core::ImClient,
    notification: Value,
    _warnings: &mut Vec<String>,
) -> Option<Value> {
    (!is_p6_v2_realtime_candidate(&notification)
        && !is_explicit_group_e2ee_notice_control(&notification))
    .then_some(notification)
}

#[cfg(all(feature = "blocking", not(feature = "group-e2ee")))]
fn normalize_group_e2ee_realtime_notification_async_first(
    _client: &crate::core::ImClient,
    _runtime: Option<&tokio::runtime::Runtime>,
    notification: Value,
    _warnings: &mut Vec<String>,
) -> Option<Value> {
    (!is_p6_v2_realtime_candidate(&notification)
        && !is_explicit_group_e2ee_notice_control(&notification))
    .then_some(notification)
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

#[cfg(test)]
mod tests;
