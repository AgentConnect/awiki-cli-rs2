use std::sync::mpsc;

pub type RealtimeEventReceiver = mpsc::Receiver<super::ImEvent>;

pub struct RealtimeHandle {
    pub events: RealtimeEventReceiver,
    pub control: super::RealtimeControl,
}

impl RealtimeHandle {
    pub(crate) fn new(events: RealtimeEventReceiver, control: super::RealtimeControl) -> Self {
        Self { events, control }
    }
}
