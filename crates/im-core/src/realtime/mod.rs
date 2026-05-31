mod control;
mod dto;
mod events;
#[cfg(feature = "blocking")]
mod handle;
pub(crate) mod runner;
mod service;
mod session;
#[doc(hidden)]
pub mod wire;

pub use self::control::{RealtimeControl, ShutdownSignal};
pub use self::dto::{
    RealtimeConnectionState, RealtimeExit, RealtimeExitReason, RealtimeOptions, RealtimeStatus,
    RealtimeSubscription, ReconnectPolicy,
};
pub use self::events::{
    AttachmentDownloadAction, AttachmentMessageSummary, ConnectionStateChanged, GroupUpdateKind,
    GroupUpdatedEvent, HostNotificationEvent, HostNotificationKind, ImEvent,
    LocalNotificationEvent, MessageReceivedEvent, MessageUpdateKind, MessageUpdatedEvent,
    UnknownNotificationEvent,
};
#[cfg(feature = "blocking")]
pub use self::handle::{RealtimeEventReceiver, RealtimeHandle};
#[cfg(feature = "blocking")]
pub use self::runner::{
    run_realtime_transport_until_shutdown, run_realtime_transport_with_event_sink_until_shutdown,
    RealtimeRunnerEventSink, RealtimeRunnerOutcome, RealtimeRunnerTransport,
};
pub use self::service::RealtimeService;
pub use self::session::{RealtimeEventStream, RealtimeSession};
#[doc(hidden)]
pub use crate::internal::realtime::dispatch::{
    ListenerWsDispatchOutcome, ListenerWsPendingDispatch,
};
#[doc(hidden)]
pub use crate::internal::realtime::frame::{
    build_ws_rpc_request, classify_incoming_message, decode_ws_rpc_result, int64_from_value,
    next_ws_rpc_request_id, pending_failure_response, request_id_from_value, IncomingWsMessage,
};
#[doc(hidden)]
pub use crate::internal::realtime::heartbeat::{
    consume_notifications_step, ConsumeNotificationsAction, ConsumeNotificationsDecision,
    ConsumeNotificationsEvent, ConsumeNotificationsStep, NotificationPingOutcome,
    SESSION_PING_INTERVAL, SESSION_PING_TIMEOUT,
};
#[doc(hidden)]
#[cfg(feature = "sqlite")]
pub use crate::internal::realtime::local_projection::{
    apply_realtime_message_local_projection, plan_realtime_message_local_projection,
    RealtimeMessageLocalProjection, RealtimeMessageLocalProjectionContext,
};
#[doc(hidden)]
pub use crate::internal::realtime::notification::LISTENER_WS_NOTIFICATION_QUEUE_CAPACITY;
#[doc(hidden)]
pub use crate::internal::realtime::projection::{
    is_direct_secure_wire_notification, project_notification, NotificationProjection,
    NotificationProjectionRoute,
};
#[doc(hidden)]
pub use crate::internal::realtime::reconnect::{
    ConsumeFinishedDecision, ContextSleep, SessionLoopBackoff, SessionLoopRetryDecision,
    SessionLoopRetryPhase, SESSION_RECONNECT_BASE_DELAY, SESSION_RECONNECT_MAX_DELAY,
};
#[doc(hidden)]
pub use crate::internal::realtime::session_loop::{
    secure_prekey_retry_decision, session_loop_start_decision, ConnectedSessionAction,
    ConsumeFinishedAction, InitialSessionSignal, SecurePrekeyRetryDecision,
    SessionLoopStartDecision, CONNECTED_SESSION_ACTIONS, CONSUME_FINISHED_ACTIONS,
    SECURE_PREKEY_RETRY_DELAY,
};
#[doc(hidden)]
pub use crate::internal::realtime::shutdown::{shutdown_decision, RealtimeShutdownDecision};
#[doc(hidden)]
pub use crate::internal::realtime::transport::{
    bearer_authorization_header, derive_websocket_url, format_dial_error_message, join_base_url,
    realtime_client_construction_plan, realtime_client_endpoints, simulate_realtime_connect,
    validate_refresh_bearer_preconditions, RealtimeClientConstructionPlan, RealtimeClientEndpoints,
    RealtimeConnectAction, RealtimeConnectSimulation, RealtimeDialOutcome, RealtimeRefreshOutcome,
    DIAL_ERROR_BODY_LIMIT, MESSAGE_WS_ENDPOINT,
};
#[doc(hidden)]
#[cfg(feature = "blocking")]
pub use crate::internal::realtime::transport::{
    connect_realtime_with_transport, RealtimeAuthProvider, RealtimeTransport,
};
