use std::sync::Arc;
use std::sync::Mutex;

use crate::dto::{
    error::DartImError,
    realtime::{
        DartRealtimeCapability, DartRealtimeEvent, DartRealtimeOptions, DartRealtimeStatus,
    },
};
use crate::frb_generated::StreamSink;

pub struct DartRealtimeSession {
    session: Mutex<Option<im_core::realtime::RealtimeSession>>,
    event_stream_attached: Mutex<bool>,
}

impl DartRealtimeSession {
    fn new(session: im_core::realtime::RealtimeSession) -> Self {
        Self {
            session: Mutex::new(Some(session)),
            event_stream_attached: Mutex::new(false),
        }
    }

    fn take_event_receiver(&self) -> Result<im_core::realtime::RealtimeEventStream, DartImError> {
        let mut attached = self
            .event_stream_attached
            .lock()
            .map_err(|_| DartImError::internal("realtime session lock poisoned"))?;
        if *attached {
            return Err(DartImError::invalid_input(
                Some("session".to_string()),
                "realtime event stream is already attached",
            ));
        }
        let mut guard = self
            .session
            .lock()
            .map_err(|_| DartImError::internal("realtime session lock poisoned"))?;
        let session = guard
            .as_mut()
            .ok_or_else(|| DartImError::object_closed("DartRealtimeSession"))?;
        let receiver = session.subscribe().map_err(DartImError::from)?;
        *attached = true;
        Ok(receiver)
    }

    fn status(&self) -> Result<DartRealtimeStatus, DartImError> {
        let guard = self
            .session
            .lock()
            .map_err(|_| DartImError::internal("realtime session lock poisoned"))?;
        let session = guard
            .as_ref()
            .ok_or_else(|| DartImError::object_closed("DartRealtimeSession"))?;
        Ok(session.status().into())
    }

    async fn stop(&self) -> Result<(), DartImError> {
        let session = self
            .session
            .lock()
            .map_err(|_| DartImError::internal("realtime session lock poisoned"))?
            .take();
        if let Some(session) = session {
            session.stop().await.map_err(DartImError::from)?;
        }
        Ok(())
    }
}

impl Drop for DartRealtimeSession {
    fn drop(&mut self) {
        if let Ok(mut guard) = self.session.lock() {
            if let Some(session) = guard.take() {
                drop(session);
            }
        }
    }
}

pub fn realtime_capability(
    _client: &Arc<crate::api::client::DartImClient>,
) -> Result<DartRealtimeCapability, DartImError> {
    Ok(DartRealtimeCapability {
        status_supported: true,
        connect_supported: true,
        runner_exposed: true,
        reason: None,
    })
}

pub async fn realtime_status(
    client: &Arc<crate::api::client::DartImClient>,
) -> Result<DartRealtimeStatus, DartImError> {
    let inner = client.clone_inner()?;
    inner
        .realtime()
        .status()
        .map(Into::into)
        .map_err(DartImError::from)
}

pub async fn realtime_connect(
    client: &Arc<crate::api::client::DartImClient>,
) -> Result<(), DartImError> {
    let session = realtime_start(
        client,
        DartRealtimeOptions {
            reconnect: "disabled".to_string(),
            event_buffer: 128,
            reconnect_delay_ms: None,
            reconnect_base_delay_ms: None,
            reconnect_max_delay_ms: None,
            reconnect_max_attempts: None,
            subscriptions: vec![
                "messages".to_string(),
                "groups".to_string(),
                "notifications".to_string(),
            ],
        },
    )
    .await?;
    realtime_stop(&session).await
}

pub async fn realtime_start(
    client: &Arc<crate::api::client::DartImClient>,
    options: DartRealtimeOptions,
) -> Result<Arc<DartRealtimeSession>, DartImError> {
    let options = options.try_into()?;
    let inner = client.clone_inner()?;
    let session = inner
        .realtime()
        .start_async(options)
        .await
        .map_err(DartImError::from)?;
    Ok(Arc::new(DartRealtimeSession::new(session)))
}

pub async fn realtime_stop(session: &Arc<DartRealtimeSession>) -> Result<(), DartImError> {
    session.stop().await
}

pub fn realtime_session_status(
    session: &Arc<DartRealtimeSession>,
) -> Result<DartRealtimeStatus, DartImError> {
    session.status()
}

pub async fn realtime_event_stream(
    session: &Arc<DartRealtimeSession>,
    sink: StreamSink<DartRealtimeEvent>,
) -> Result<(), DartImError> {
    let mut receiver = session.take_event_receiver()?;
    let session_for_worker = Arc::clone(session);
    tokio::spawn(async move {
        while let Some(event) = receiver.recv().await {
            let event = crate::mapping::from_core::realtime_event_to_dart(event);
            if sink.add(event).is_err() {
                let _ = session_for_worker.stop().await;
                break;
            }
        }
    });
    Ok(())
}
