//! Migration-only realtime runtime bridge for `awiki-cli`.
//!
//! The remaining runner/transport traits in this module are a temporary CLI
//! migration bridge. They should be removed once the CLI listener can host the
//! stable public `im_core::realtime` runner API directly.

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
pub use crate::internal::realtime::notification::LISTENER_WS_NOTIFICATION_QUEUE_CAPACITY;
#[doc(hidden)]
pub use crate::internal::realtime::projection::{
    project_notification, NotificationProjection, NotificationProjectionRoute,
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
#[cfg(feature = "blocking")]
pub use crate::internal::realtime::transport::{
    connect_realtime_with_transport, RealtimeAuthProvider, RealtimeTransport,
};
#[doc(hidden)]
#[cfg(feature = "blocking")]
pub use crate::realtime::runner::{
    run_realtime_transport_until_shutdown, run_realtime_transport_with_event_sink_until_shutdown,
    RealtimeRunnerEventSink, RealtimeRunnerOutcome, RealtimeRunnerTransport,
};
