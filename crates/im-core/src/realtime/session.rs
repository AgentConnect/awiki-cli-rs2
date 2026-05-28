use tokio::sync::{mpsc, oneshot, watch};

pub type RealtimeEventStream = mpsc::Receiver<super::ImEvent>;

pub struct RealtimeSession {
    events: Option<RealtimeEventStream>,
    status: watch::Receiver<super::RealtimeStatus>,
    shutdown: super::ShutdownSignal,
    exit: Option<oneshot::Receiver<super::RealtimeExit>>,
}

impl RealtimeSession {
    pub(crate) fn new(
        events: RealtimeEventStream,
        status: watch::Receiver<super::RealtimeStatus>,
        shutdown: super::ShutdownSignal,
        exit: oneshot::Receiver<super::RealtimeExit>,
    ) -> Self {
        Self {
            events: Some(events),
            status,
            shutdown,
            exit: Some(exit),
        }
    }

    pub fn subscribe(&mut self) -> crate::ImResult<RealtimeEventStream> {
        self.events.take().ok_or_else(|| {
            crate::ImError::invalid_input(
                Some("session".to_owned()),
                "realtime event stream is already attached",
            )
        })
    }

    pub fn status(&self) -> super::RealtimeStatus {
        self.status.borrow().clone()
    }

    pub fn status_updates(&self) -> watch::Receiver<super::RealtimeStatus> {
        self.status.clone()
    }

    pub async fn stop(&self) -> crate::ImResult<()> {
        self.shutdown.request();
        Ok(())
    }

    pub async fn join(&mut self) -> crate::ImResult<super::RealtimeExit> {
        let Some(exit) = self.exit.take() else {
            return Err(crate::ImError::invalid_input(
                Some("session".to_owned()),
                "realtime session exit has already been joined",
            ));
        };
        exit.await.map_err(|_| crate::ImError::Internal {
            message: "realtime session task exited without an exit result".to_owned(),
        })
    }
}

impl Drop for RealtimeSession {
    fn drop(&mut self) {
        self.shutdown.request();
    }
}

pub(crate) fn initial_realtime_status(
    options: &super::RealtimeOptions,
    state: super::RealtimeConnectionState,
    last_error: Option<String>,
) -> super::RealtimeStatus {
    super::RealtimeStatus {
        connected: state == super::RealtimeConnectionState::Connected,
        state,
        subscriptions: options.subscriptions.clone(),
        last_error,
    }
}

#[cfg(test)]
mod tests {
    use tokio::sync::{mpsc, oneshot, watch};

    use super::*;

    #[tokio::test]
    async fn realtime_session_allows_single_event_stream_and_stop_request() {
        let options = super::super::RealtimeOptions::default();
        let (sender, receiver) = mpsc::channel(options.event_buffer);
        let (status_sender, status_receiver) = watch::channel(initial_realtime_status(
            &options,
            super::super::RealtimeConnectionState::Disconnected,
            None,
        ));
        let (exit_sender, exit_receiver) = oneshot::channel();
        let shutdown = super::super::ShutdownSignal::pending();
        let mut session =
            RealtimeSession::new(receiver, status_receiver, shutdown.clone(), exit_receiver);

        let mut events = session.subscribe().unwrap();
        assert!(session.subscribe().is_err());
        sender
            .send(super::super::ImEvent::ConnectionStateChanged(
                super::super::ConnectionStateChanged {
                    state: super::super::RealtimeConnectionState::Connected,
                    reason: None,
                },
            ))
            .await
            .unwrap();
        let _ = status_sender.send(super::super::RealtimeStatus {
            connected: true,
            state: super::super::RealtimeConnectionState::Connected,
            subscriptions: options.subscriptions,
            last_error: None,
        });

        assert!(matches!(
            events.recv().await,
            Some(super::super::ImEvent::ConnectionStateChanged(
                super::super::ConnectionStateChanged {
                    state: super::super::RealtimeConnectionState::Connected,
                    ..
                }
            ))
        ));
        assert!(session.status().connected);
        session.stop().await.unwrap();
        assert!(shutdown.is_requested());
        exit_sender
            .send(super::super::RealtimeExit {
                reason: super::super::RealtimeExitReason::ShutdownRequested,
                reconnect_attempts: 0,
                warnings: Vec::new(),
            })
            .unwrap();
        let exit = session.join().await.unwrap();
        assert_eq!(
            exit.reason,
            super::super::RealtimeExitReason::ShutdownRequested
        );
    }
}
