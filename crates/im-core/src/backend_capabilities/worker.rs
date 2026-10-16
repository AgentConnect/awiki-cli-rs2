use std::{future::Future, sync::Mutex};

/// One lazy executor per Core. Dropping it cancels outstanding work without
/// waiting for network timeouts, including when the last caller is a CLI.
#[derive(Default)]
pub(crate) struct DiscoveryWorker(Mutex<Option<Worker>>);

struct Worker {
    handle: tokio::runtime::Handle,
    _shutdown: tokio::sync::oneshot::Sender<()>,
}

impl DiscoveryWorker {
    pub(crate) fn stop(&self) {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
    }

    pub(crate) fn spawn(
        &self,
        future: impl Future<Output = ()> + Send + 'static,
    ) -> crate::ImResult<()> {
        let mut worker = self.0.lock().map_err(|_| unavailable())?;
        if worker.is_none() {
            let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel(1);
            let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
            std::thread::Builder::new()
                .name("awiki-capabilities".into())
                .spawn(move || {
                    let runtime = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build();
                    match runtime {
                        Ok(runtime) => {
                            if ready_tx.send(Ok(runtime.handle().clone())).is_ok() {
                                runtime.block_on(async {
                                    let _ = shutdown_rx.await;
                                });
                            }
                        }
                        Err(_) => {
                            let _ = ready_tx.send(Err(unavailable()));
                        }
                    }
                })
                .map_err(|_| unavailable())?;
            *worker = Some(Worker {
                handle: ready_rx.recv().map_err(|_| unavailable())??,
                _shutdown: shutdown_tx,
            });
        }
        worker.as_ref().unwrap().handle.spawn(future);
        Ok(())
    }
}

fn unavailable() -> crate::ImError {
    crate::ImError::Internal {
        message: "capability worker unavailable".into(),
    }
}
