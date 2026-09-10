use std::sync::Arc;

use tokio::sync::broadcast;

use crate::internal::message_runtime::sync_dispatcher::{self, OwnedUpdate, SyncDispatcher};

/// Owner-scoped business updates, separate from network reception. Missed
/// notifications do not replace committed facts: hosts repair their local view.
pub struct MessageProcessingSession {
    _dispatcher: Arc<SyncDispatcher>,
    owner_identity_id: String,
    receiver: Option<broadcast::Receiver<OwnedUpdate>>,
}

impl MessageProcessingSession {
    pub(crate) fn new(client: &crate::core::ImClient) -> Self {
        let dispatcher = sync_dispatcher::for_client(client);
        let receiver = dispatcher.subscribe();
        if tokio::runtime::Handle::try_current().is_ok() {
            dispatcher.wake_client(client.clone());
        }
        Self {
            _dispatcher: dispatcher,
            owner_identity_id: client.current_identity().id.as_str().to_owned(),
            receiver: Some(receiver),
        }
    }

    pub fn close(&mut self) {
        self.receiver.take();
        self._dispatcher.notify();
    }

    pub async fn next_async(
        &mut self,
        client: &crate::core::ImClient,
    ) -> crate::ImResult<Option<super::MessageProcessingUpdate>> {
        self.ensure_owner(client)?;
        if self.receiver.is_some() {
            self._dispatcher.wake_client(client.clone());
        }
        let Some(receiver) = self.receiver.as_mut() else {
            return Ok(None);
        };
        loop {
            match receiver.recv().await {
                Ok(owned) if owned.owner_identity_id == self.owner_identity_id => {
                    return Ok(Some(owned.update))
                }
                Ok(_) => continue,
                Err(broadcast::error::RecvError::Closed) => return Ok(None),
                Err(broadcast::error::RecvError::Lagged(_)) => return Err(updates_lagged()),
            }
        }
    }

    pub(crate) fn try_next(
        &mut self,
        client: &crate::core::ImClient,
    ) -> crate::ImResult<Option<super::MessageProcessingUpdate>> {
        self.ensure_owner(client)?;
        let Some(receiver) = self.receiver.as_mut() else {
            return Ok(None);
        };
        loop {
            match receiver.try_recv() {
                Ok(owned) if owned.owner_identity_id == self.owner_identity_id => {
                    return Ok(Some(owned.update))
                }
                Ok(_) => continue,
                Err(
                    broadcast::error::TryRecvError::Empty | broadcast::error::TryRecvError::Closed,
                ) => return Ok(None),
                Err(broadcast::error::TryRecvError::Lagged(_)) => return Err(updates_lagged()),
            }
        }
    }

    fn ensure_owner(&self, client: &crate::core::ImClient) -> crate::ImResult<()> {
        if client.current_identity().id.as_str() != self.owner_identity_id {
            return Err(crate::ImError::IdentityBindingConflict {
                detail: "message processing subscription belongs to another owner".into(),
            });
        }
        Ok(())
    }
}

fn updates_lagged() -> crate::ImError {
    crate::ImError::LocalProjectionUnavailable {
        detail: "message processing updates lagged; repair the committed local view".into(),
    }
}

pub(crate) async fn compatibility_wait(
    client: &crate::core::ImClient,
    received: super::MessageReceiveOutcome,
    mut session: MessageProcessingSession,
) -> crate::ImResult<super::MessageSyncOutcome> {
    let mut outcome = received.compatibility_outcome();
    if !matches!(
        received.status,
        super::MessageSyncStatus::Idle | super::MessageSyncStatus::Changed
    ) {
        return Ok(outcome);
    }
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(25);
    let db = client.core_inner().local_state_db().await?;
    loop {
        while let Some(update) = session.try_next(client)? {
            match update.status {
                super::MessageProcessingStatus::Applied => {
                    outcome.events_applied += 1;
                    outcome
                        .changed_conversation_ids
                        .extend(update.changed_conversation_ids);
                    outcome
                        .committed_incoming_messages
                        .extend(update.committed_incoming_messages);
                }
                super::MessageProcessingStatus::Discarded => {
                    outcome.status = super::MessageSyncStatus::Blocked;
                    outcome.error_code = update.error_code;
                }
                _ => {}
            }
        }
        let owner = client.current_identity().id.as_str().to_owned();
        let summary = db
            .run_local(move |connection| {
                crate::internal::local_state::sync_inbox::processing_summary(
                    connection,
                    &owner,
                    chrono::Utc::now().timestamp(),
                )
            })
            .await?;
        if summary.pending == 0 {
            break;
        }
        if summary.blocked > 0 && summary.active == 0 && summary.pending == summary.blocked {
            outcome.status = super::MessageSyncStatus::Blocked;
            outcome.error_code = summary.error_code;
            break;
        }
        if tokio::time::Instant::now() >= deadline {
            outcome.status = super::MessageSyncStatus::RetryableFailure;
            outcome.error_code = Some("sync.processing_pending".into());
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    outcome.changed_conversation_ids.sort();
    outcome.changed_conversation_ids.dedup();
    Ok(outcome)
}
