use std::sync::Arc;
use std::sync::Mutex;
use std::thread;

use crate::dto::{
    error::DartImError,
    realtime::{
        DartRealtimeCapability, DartRealtimeEvent, DartRealtimeOptions, DartRealtimeStatus,
    },
};
use crate::frb_generated::StreamSink;

pub struct DartRealtimeSession {
    handle: Mutex<Option<im_core::realtime::RealtimeHandle>>,
    event_stream_attached: Mutex<bool>,
}

impl DartRealtimeSession {
    fn new(handle: im_core::realtime::RealtimeHandle) -> Self {
        Self {
            handle: Mutex::new(Some(handle)),
            event_stream_attached: Mutex::new(false),
        }
    }

    fn take_event_receiver(
        &self,
    ) -> Result<std::sync::mpsc::Receiver<im_core::realtime::ImEvent>, DartImError> {
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
            .handle
            .lock()
            .map_err(|_| DartImError::internal("realtime session lock poisoned"))?;
        let handle = guard
            .as_mut()
            .ok_or_else(|| DartImError::object_closed("DartRealtimeSession"))?;
        let (_sender, replacement) = std::sync::mpsc::channel();
        *attached = true;
        Ok(std::mem::replace(&mut handle.events, replacement))
    }

    fn stop(&self) -> Result<(), DartImError> {
        let guard = self
            .handle
            .lock()
            .map_err(|_| DartImError::internal("realtime session lock poisoned"))?;
        if let Some(handle) = guard.as_ref() {
            handle.control.shutdown();
        }
        Ok(())
    }
}

impl Drop for DartRealtimeSession {
    fn drop(&mut self) {
        if let Ok(guard) = self.handle.lock() {
            if let Some(handle) = guard.as_ref() {
                handle.control.shutdown();
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

pub fn realtime_status(
    client: &Arc<crate::api::client::DartImClient>,
) -> Result<DartRealtimeStatus, DartImError> {
    client.with_inner(|inner| {
        inner
            .realtime()
            .status()
            .map(Into::into)
            .map_err(DartImError::from)
    })
}

pub fn realtime_connect(_client: &Arc<crate::api::client::DartImClient>) -> Result<(), DartImError> {
    let session = realtime_start(
        _client,
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
    )?;
    realtime_stop(session)
}

pub fn realtime_start(
    client: &Arc<crate::api::client::DartImClient>,
    options: DartRealtimeOptions,
) -> Result<Arc<DartRealtimeSession>, DartImError> {
    let options = options.try_into()?;
    let handle =
        client.with_inner(|inner| inner.realtime().connect(options).map_err(DartImError::from))?;
    Ok(Arc::new(DartRealtimeSession::new(handle)))
}

pub fn realtime_stop(session: Arc<DartRealtimeSession>) -> Result<(), DartImError> {
    session.stop()
}

pub fn realtime_event_stream(
    session: &Arc<DartRealtimeSession>,
    sink: StreamSink<DartRealtimeEvent>,
) -> Result<(), DartImError> {
    let receiver = session.take_event_receiver()?;
    let session_for_worker = Arc::clone(session);
    thread::Builder::new()
        .name("im-core-dart-realtime-events".to_string())
        .spawn(move || {
            for event in receiver {
                let event = crate::mapping::from_core::realtime_event_to_dart(event);
                if sink.add(event).is_err() {
                    let _ = session_for_worker.stop();
                    break;
                }
            }
        })
        .map_err(|err| DartImError::internal(format!("spawn realtime stream worker: {err}")))?;
    Ok(())
}
