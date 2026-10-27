pub(crate) mod conversations;
pub(crate) mod direct;
pub(crate) mod group;
pub(crate) mod local_projection;
pub(crate) mod mark_read;
pub(crate) mod read;
#[cfg(any(feature = "group-e2ee", feature = "secure-direct"))]
pub(crate) mod send_lifecycle;
pub(crate) mod state;
pub(crate) mod sync;
pub(crate) mod sync_coordinator;
#[cfg(test)]
mod sync_coordinator_tests;
pub(crate) mod sync_dispatcher;
pub(crate) mod sync_processing;
mod sync_receive_lock;
pub(crate) mod sync_v2;
