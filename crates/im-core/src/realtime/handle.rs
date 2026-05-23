use std::sync::mpsc;
use std::thread::JoinHandle;

pub type RealtimeEventReceiver = mpsc::Receiver<super::ImEvent>;

pub struct RealtimeHandle {
    pub events: RealtimeEventReceiver,
    pub control: super::RealtimeControl,
    _worker: Option<JoinHandle<()>>,
}

impl RealtimeHandle {
    pub(crate) fn new(events: RealtimeEventReceiver, control: super::RealtimeControl) -> Self {
        Self {
            events,
            control,
            _worker: None,
        }
    }

    pub(crate) fn with_worker(
        events: RealtimeEventReceiver,
        control: super::RealtimeControl,
        worker: JoinHandle<()>,
    ) -> Self {
        Self {
            events,
            control,
            _worker: Some(worker),
        }
    }
}
