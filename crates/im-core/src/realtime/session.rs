use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::JoinHandle;

pub type RealtimeEventStream = mpsc::Receiver<super::ImEvent>;

pub struct RealtimeSession {
    events: Option<RealtimeEventStream>,
    status: watch::Receiver<super::RealtimeStatus>,
    shutdown: super::ShutdownSignal,
    exit: Option<oneshot::Receiver<super::RealtimeExit>>,
    worker: Option<JoinHandle<()>>,
}

impl RealtimeSession {
    pub(crate) fn new(
        events: RealtimeEventStream,
        status: watch::Receiver<super::RealtimeStatus>,
        shutdown: super::ShutdownSignal,
        exit: oneshot::Receiver<super::RealtimeExit>,
        worker: Option<JoinHandle<()>>,
    ) -> Self {
        Self {
            events: Some(events),
            status,
            shutdown,
            exit: Some(exit),
            worker,
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
        let exit = match exit.await {
            Ok(exit) => exit,
            Err(_) => {
                return Err(join_worker_error(self.worker.take()).await);
            }
        };
        if let Some(worker) = self.worker.take() {
            worker.await.map_err(|err| crate::ImError::Internal {
                message: if err.is_panic() {
                    "realtime session task panicked".to_owned()
                } else if err.is_cancelled() {
                    "realtime session task was cancelled".to_owned()
                } else {
                    format!("realtime session task failed: {err}")
                },
            })?;
        }
        Ok(exit)
    }
}

impl Drop for RealtimeSession {
    fn drop(&mut self) {
        self.shutdown.request();
    }
}

async fn join_worker_error(worker: Option<JoinHandle<()>>) -> crate::ImError {
    if let Some(worker) = worker {
        match worker.await {
            Ok(()) => crate::ImError::Internal {
                message: "realtime session task exited without an exit result".to_owned(),
            },
            Err(err) if err.is_panic() => crate::ImError::Internal {
                message: "realtime session task panicked".to_owned(),
            },
            Err(err) if err.is_cancelled() => crate::ImError::Internal {
                message: "realtime session task was cancelled".to_owned(),
            },
            Err(err) => crate::ImError::Internal {
                message: format!("realtime session task failed: {err}"),
            },
        }
    } else {
        crate::ImError::Internal {
            message: "realtime session task exited without an exit result".to_owned(),
        }
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
        let mut session = RealtimeSession::new(
            receiver,
            status_receiver,
            shutdown.clone(),
            exit_receiver,
            None,
        );

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

    #[tokio::test]
    async fn realtime_session_drop_requests_worker_shutdown_without_join() {
        let options = super::super::RealtimeOptions::default();
        let (_sender, receiver) = mpsc::channel(options.event_buffer);
        let (_status_sender, status_receiver) = watch::channel(initial_realtime_status(
            &options,
            super::super::RealtimeConnectionState::Disconnected,
            None,
        ));
        let (exit_sender, exit_receiver) = oneshot::channel();
        drop(exit_sender);
        let shutdown = super::super::ShutdownSignal::pending();
        let worker_shutdown = shutdown.clone();
        let (observed_sender, observed_receiver) = oneshot::channel();
        let worker = tokio::spawn(async move {
            while !worker_shutdown.is_requested() {
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
            let _ = observed_sender.send(());
        });

        let session = RealtimeSession::new(
            receiver,
            status_receiver,
            shutdown,
            exit_receiver,
            Some(worker),
        );
        drop(session);

        tokio::time::timeout(std::time::Duration::from_millis(250), observed_receiver)
            .await
            .expect("drop should request shutdown before timeout")
            .expect("worker should observe shutdown");
    }

    #[tokio::test]
    async fn realtime_session_join_reports_worker_panic() {
        let options = super::super::RealtimeOptions::default();
        let (_sender, receiver) = mpsc::channel(options.event_buffer);
        let (_status_sender, status_receiver) = watch::channel(initial_realtime_status(
            &options,
            super::super::RealtimeConnectionState::Disconnected,
            None,
        ));
        let (exit_sender, exit_receiver) = oneshot::channel();
        drop(exit_sender);
        let shutdown = super::super::ShutdownSignal::pending();
        let worker = tokio::spawn(async {
            panic!("realtime worker panic test");
        });
        let mut session = RealtimeSession::new(
            receiver,
            status_receiver,
            shutdown,
            exit_receiver,
            Some(worker),
        );

        let error = session.join().await.unwrap_err();

        assert!(
            matches!(error, crate::ImError::Internal { message } if message == "realtime session task panicked")
        );
    }
}
