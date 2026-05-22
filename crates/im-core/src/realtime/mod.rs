mod control;
mod dto;
mod events;
mod handle;
pub(crate) mod runner;
mod service;

pub use self::control::{RealtimeControl, ShutdownSignal};
pub use self::dto::{
    RealtimeConnectionState, RealtimeExit, RealtimeExitReason, RealtimeOptions, RealtimeStatus,
    RealtimeSubscription, ReconnectPolicy,
};
pub use self::events::{
    ConnectionStateChanged, GroupUpdateKind, GroupUpdatedEvent, HostNotificationEvent,
    HostNotificationKind, ImEvent, LocalNotificationEvent, MessageReceivedEvent, MessageUpdateKind,
    MessageUpdatedEvent, UnknownNotificationEvent,
};
pub use self::handle::{RealtimeEventReceiver, RealtimeHandle};
pub use self::runner::{
    run_realtime_transport_until_shutdown, run_realtime_transport_with_event_sink_until_shutdown,
    RealtimeRunnerEventSink, RealtimeRunnerOutcome, RealtimeRunnerTransport,
};
pub use self::service::RealtimeService;
pub use crate::internal::realtime::heartbeat::{
    consume_notifications_step, ConsumeNotificationsAction, ConsumeNotificationsDecision,
    ConsumeNotificationsEvent, ConsumeNotificationsStep, NotificationPingOutcome,
    SESSION_PING_INTERVAL, SESSION_PING_TIMEOUT,
};
pub use crate::internal::realtime::reconnect::{
    ConsumeFinishedDecision, ContextSleep, SessionLoopBackoff, SessionLoopRetryDecision,
    SessionLoopRetryPhase, SESSION_RECONNECT_BASE_DELAY, SESSION_RECONNECT_MAX_DELAY,
};
pub use crate::internal::realtime::session_loop::{
    secure_prekey_retry_decision, session_loop_start_decision, ConnectedSessionAction,
    ConsumeFinishedAction, InitialSessionSignal, SecurePrekeyRetryDecision,
    SessionLoopStartDecision, CONNECTED_SESSION_ACTIONS, CONSUME_FINISHED_ACTIONS,
    SECURE_PREKEY_RETRY_DELAY,
};
pub use crate::internal::realtime::shutdown::{shutdown_decision, RealtimeShutdownDecision};
pub use crate::internal::realtime::transport::{
    bearer_authorization_header, connect_realtime_with_transport, format_dial_error_message,
    realtime_client_construction_plan, realtime_client_endpoints, simulate_realtime_connect,
    validate_refresh_bearer_preconditions, RealtimeAuthProvider, RealtimeClientConstructionPlan,
    RealtimeClientEndpoints, RealtimeConnectAction, RealtimeConnectSimulation, RealtimeDialOutcome,
    RealtimeRefreshOutcome, RealtimeTransport,
};
