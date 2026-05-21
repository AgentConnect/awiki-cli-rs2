//! Migration-only realtime runtime bridge for `awiki-cli`.

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
pub use crate::internal::realtime::notification::LISTENER_WS_NOTIFICATION_QUEUE_CAPACITY;
