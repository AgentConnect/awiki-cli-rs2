use std::sync::Arc;

use crate::dto::{
    error::DartImError,
    realtime::{DartRealtimeCapability, DartRealtimeStatus},
};

pub fn realtime_capability(
    _client: Arc<crate::api::client::DartImClient>,
) -> Result<DartRealtimeCapability, DartImError> {
    Ok(DartRealtimeCapability {
        status_supported: true,
        connect_supported: false,
        runner_exposed: false,
        reason: Some("Dart SDK v0.1 does not expose realtime runner yet".to_string()),
    })
}

pub fn realtime_status(
    client: Arc<crate::api::client::DartImClient>,
) -> Result<DartRealtimeStatus, DartImError> {
    client.with_inner(|inner| {
        inner
            .realtime()
            .status()
            .map(Into::into)
            .map_err(DartImError::from)
    })
}

pub fn realtime_connect(_client: Arc<crate::api::client::DartImClient>) -> Result<(), DartImError> {
    Err(DartImError::unsupported("realtime-runner"))
}
