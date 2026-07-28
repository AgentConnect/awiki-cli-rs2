use std::future::Future;
use std::sync::Arc;
use std::sync::Mutex;

use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::dto::{
    error::DartImError,
    message::{
        DartConversationListSnapshot, DartConversationPage, DartConversationReadRef,
        DartConversationStorePatch, DartInboxHistoryOptions, DartMarkConversationReadRequest,
        DartMarkReadResult, DartMarkThreadReadResult, DartMessagePage, DartMessageSyncOutcome,
        DartMessageSyncRequest, DartSendConversationPayloadRequest,
        DartSendConversationTextRequest, DartSendMessageResult, DartSendPayloadRequest,
        DartSendTextRequest, DartSyncConversationAfterRequest, DartSyncDeltaRequest,
        DartSyncDeltaResult, DartSyncThreadAfterRequest, DartSyncThreadAfterResult,
        DartThreadMessageStorePatch, DartThreadRef,
    },
};
use crate::frb_generated::StreamSink;

#[cfg(test)]
#[path = "messages_tests.rs"]
mod tests;

struct PatchStreamLifecycle {
    label: &'static str,
    cancel: watch::Sender<bool>,
    task: Mutex<Option<JoinHandle<()>>>,
    stop_lock: tokio::sync::Mutex<()>,
}

impl PatchStreamLifecycle {
    fn new(label: &'static str) -> Self {
        let (cancel, _) = watch::channel(false);
        Self {
            label,
            cancel,
            task: Mutex::new(None),
            stop_lock: tokio::sync::Mutex::new(()),
        }
    }

    fn is_stopped(&self) -> bool {
        *self.cancel.borrow()
    }

    fn spawn<F>(&self, worker: F) -> Result<(), DartImError>
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let mut task = self
            .task
            .lock()
            .map_err(|_| DartImError::internal(format!("{} task lock poisoned", self.label)))?;
        if self.is_stopped() {
            return Err(DartImError::object_closed(self.label));
        }
        if task.is_some() {
            return Err(DartImError::invalid_input(
                Some("session".to_string()),
                format!("{} stream is already attached", self.label),
            ));
        }
        let mut cancel = self.cancel.subscribe();
        *task = Some(tokio::spawn(async move {
            tokio::select! {
                biased;
                _ = wait_for_patch_stream_cancel(&mut cancel) => {}
                _ = worker => {}
            }
        }));
        Ok(())
    }

    async fn stop(&self) -> Result<(), DartImError> {
        let _stop = self.stop_lock.lock().await;
        self.cancel.send_replace(true);
        let task = self
            .task
            .lock()
            .map_err(|_| DartImError::internal(format!("{} task lock poisoned", self.label)))?
            .take();
        if let Some(task) = task {
            task.await.map_err(|error| {
                DartImError::internal(format!("{} task failed: {error}", self.label))
            })?;
        }
        Ok(())
    }
}

impl Drop for PatchStreamLifecycle {
    fn drop(&mut self) {
        self.cancel.send_replace(true);
        if let Ok(task) = self.task.get_mut() {
            if let Some(task) = task.take() {
                task.abort();
            }
        }
    }
}

async fn wait_for_patch_stream_cancel(cancel: &mut watch::Receiver<bool>) {
    if *cancel.borrow() {
        return;
    }
    while cancel.changed().await.is_ok() {
        if *cancel.borrow() {
            return;
        }
    }
}

pub struct DartConversationPatchSession {
    session: Mutex<Option<im_core::messages::ConversationPatchSession>>,
    stream_attached: Mutex<bool>,
    lifecycle: PatchStreamLifecycle,
}

impl DartConversationPatchSession {
    fn new(session: im_core::messages::ConversationPatchSession) -> Self {
        Self {
            session: Mutex::new(Some(session)),
            stream_attached: Mutex::new(false),
            lifecycle: PatchStreamLifecycle::new("conversation patch session"),
        }
    }

    fn take_session(&self) -> Result<im_core::messages::ConversationPatchSession, DartImError> {
        if self.lifecycle.is_stopped() {
            return Err(DartImError::object_closed("DartConversationPatchSession"));
        }
        let mut attached = self
            .stream_attached
            .lock()
            .map_err(|_| DartImError::internal("conversation patch session lock poisoned"))?;
        if *attached {
            return Err(DartImError::invalid_input(
                Some("session".to_string()),
                "conversation patch stream is already attached",
            ));
        }
        let mut guard = self
            .session
            .lock()
            .map_err(|_| DartImError::internal("conversation patch session lock poisoned"))?;
        let session = guard
            .take()
            .ok_or_else(|| DartImError::object_closed("DartConversationPatchSession"))?;
        *attached = true;
        Ok(session)
    }

    async fn stop(&self) -> Result<(), DartImError> {
        self.lifecycle.stop().await?;
        let session = self
            .session
            .lock()
            .map_err(|_| DartImError::internal("conversation patch session lock poisoned"))?
            .take();
        if let Some(mut session) = session {
            session.stop().await.map_err(DartImError::from)?;
        }
        Ok(())
    }
}

impl Drop for DartConversationPatchSession {
    fn drop(&mut self) {
        if let Ok(mut guard) = self.session.lock() {
            let _ = guard.take();
        }
    }
}

pub struct DartThreadMessagePatchSession {
    session: Mutex<Option<im_core::messages::ThreadMessagePatchSession>>,
    stream_attached: Mutex<bool>,
    lifecycle: PatchStreamLifecycle,
}

impl DartThreadMessagePatchSession {
    fn new(session: im_core::messages::ThreadMessagePatchSession) -> Self {
        Self {
            session: Mutex::new(Some(session)),
            stream_attached: Mutex::new(false),
            lifecycle: PatchStreamLifecycle::new("thread message patch session"),
        }
    }

    fn take_session(&self) -> Result<im_core::messages::ThreadMessagePatchSession, DartImError> {
        if self.lifecycle.is_stopped() {
            return Err(DartImError::object_closed("DartThreadMessagePatchSession"));
        }
        let mut attached = self
            .stream_attached
            .lock()
            .map_err(|_| DartImError::internal("thread message patch session lock poisoned"))?;
        if *attached {
            return Err(DartImError::invalid_input(
                Some("session".to_string()),
                "thread message patch stream is already attached",
            ));
        }
        let mut guard = self
            .session
            .lock()
            .map_err(|_| DartImError::internal("thread message patch session lock poisoned"))?;
        let session = guard
            .take()
            .ok_or_else(|| DartImError::object_closed("DartThreadMessagePatchSession"))?;
        *attached = true;
        Ok(session)
    }

    async fn stop(&self) -> Result<(), DartImError> {
        self.lifecycle.stop().await?;
        let session = self
            .session
            .lock()
            .map_err(|_| DartImError::internal("thread message patch session lock poisoned"))?
            .take();
        if let Some(mut session) = session {
            session.stop().await.map_err(DartImError::from)?;
        }
        Ok(())
    }
}

impl Drop for DartThreadMessagePatchSession {
    fn drop(&mut self) {
        if let Ok(mut guard) = self.session.lock() {
            let _ = guard.take();
        }
    }
}

fn page_limit(limit: u32) -> Result<im_core::ids::PageLimit, DartImError> {
    im_core::ids::PageLimit::new(limit).map_err(DartImError::from)
}

pub async fn send_text(
    client: &Arc<crate::api::client::DartImClient>,
    request: DartSendTextRequest,
) -> Result<DartSendMessageResult, DartImError> {
    let inner = client.clone_inner()?;
    inner
        .messages()
        .send_async(request.try_into()?)
        .await
        .map(Into::into)
        .map_err(DartImError::from)
}

pub async fn send_payload(
    client: &Arc<crate::api::client::DartImClient>,
    request: DartSendPayloadRequest,
) -> Result<DartSendMessageResult, DartImError> {
    let inner = client.clone_inner()?;
    inner
        .messages()
        .send_async(request.try_into()?)
        .await
        .map(Into::into)
        .map_err(DartImError::from)
}

pub async fn send_conversation_text(
    client: &Arc<crate::api::client::DartImClient>,
    request: DartSendConversationTextRequest,
) -> Result<DartSendMessageResult, DartImError> {
    let inner = client.clone_inner()?;
    inner
        .messages()
        .send_conversation_text_async(request.try_into()?)
        .await
        .map(Into::into)
        .map_err(DartImError::from)
}

pub async fn send_conversation_payload(
    client: &Arc<crate::api::client::DartImClient>,
    request: DartSendConversationPayloadRequest,
) -> Result<DartSendMessageResult, DartImError> {
    let inner = client.clone_inner()?;
    inner
        .messages()
        .send_conversation_payload_async(request.try_into()?)
        .await
        .map(Into::into)
        .map_err(DartImError::from)
}

pub async fn inbox(
    client: &Arc<crate::api::client::DartImClient>,
    limit: u32,
    cursor: Option<String>,
    unread_only: bool,
    inbox_history_options: Option<DartInboxHistoryOptions>,
) -> Result<DartMessagePage, DartImError> {
    let inner = client.clone_inner()?;
    let query = im_core::messages::InboxQuery {
        scope: im_core::messages::InboxScope::All,
        limit: page_limit(limit)?,
        cursor: cursor
            .map(im_core::ids::Cursor::parse)
            .transpose()
            .map_err(DartImError::from)?,
        unread_only,
        inbox_history_options: inbox_history_options.map(Into::into),
    };
    inner
        .messages()
        .inbox_async(query)
        .await
        .map(Into::into)
        .map_err(DartImError::from)
}

pub async fn history(
    client: &Arc<crate::api::client::DartImClient>,
    thread: DartThreadRef,
    limit: u32,
    cursor: Option<String>,
    inbox_history_options: Option<DartInboxHistoryOptions>,
) -> Result<DartMessagePage, DartImError> {
    let inner = client.clone_inner()?;
    let query = im_core::messages::HistoryQuery {
        limit: page_limit(limit)?,
        cursor: cursor
            .map(im_core::ids::Cursor::parse)
            .transpose()
            .map_err(DartImError::from)?,
        inbox_history_options: inbox_history_options.map(Into::into),
    };
    inner
        .messages()
        .history_async(thread.try_into()?, query)
        .await
        .map(Into::into)
        .map_err(DartImError::from)
}

pub async fn local_history(
    client: &Arc<crate::api::client::DartImClient>,
    thread: DartThreadRef,
    limit: u32,
    cursor: Option<String>,
) -> Result<DartMessagePage, DartImError> {
    let inner = client.clone_inner()?;
    let query = im_core::messages::LocalHistoryQuery {
        limit: page_limit(limit)?,
        cursor: cursor
            .map(im_core::ids::Cursor::parse)
            .transpose()
            .map_err(DartImError::from)?,
    };
    inner
        .messages()
        .local_history_async(thread.try_into()?, query)
        .await
        .map(Into::into)
        .map_err(DartImError::from)
}

pub async fn local_conversation_timeline(
    client: &Arc<crate::api::client::DartImClient>,
    conversation: DartConversationReadRef,
    limit: u32,
    cursor: Option<String>,
) -> Result<DartMessagePage, DartImError> {
    let inner = client.clone_inner()?;
    let query = im_core::messages::LocalHistoryQuery {
        limit: page_limit(limit)?,
        cursor: cursor
            .map(im_core::ids::Cursor::parse)
            .transpose()
            .map_err(DartImError::from)?,
    };
    inner
        .messages()
        .local_conversation_timeline_async(conversation.try_into()?, query)
        .await
        .map(Into::into)
        .map_err(DartImError::from)
}

pub async fn mark_read(
    client: &Arc<crate::api::client::DartImClient>,
    message_ids: Vec<String>,
) -> Result<DartMarkReadResult, DartImError> {
    let inner = client.clone_inner()?;
    let ids = message_ids
        .into_iter()
        .map(im_core::ids::MessageId::parse)
        .collect::<Result<Vec<_>, _>>()
        .map_err(DartImError::from)?;
    inner
        .messages()
        .mark_read_async(ids)
        .await
        .map(Into::into)
        .map_err(DartImError::from)
}

pub async fn mark_thread_read(
    client: &Arc<crate::api::client::DartImClient>,
    thread: DartThreadRef,
    watermark: Option<crate::dto::message::DartReadWatermark>,
    fallback_max_message_ids: Option<u32>,
) -> Result<DartMarkThreadReadResult, DartImError> {
    let inner = client.clone_inner()?;
    let request = im_core::messages::MarkThreadReadRequest {
        thread: thread.try_into()?,
        watermark: watermark
            .map(read_watermark_to_core)
            .transpose()
            .map_err(DartImError::from)?,
        fallback_max_message_ids,
    };
    inner
        .messages()
        .mark_thread_read_async(request)
        .await
        .map(Into::into)
        .map_err(DartImError::from)
}

pub(crate) fn read_watermark_to_core(
    value: crate::dto::message::DartReadWatermark,
) -> im_core::ImResult<im_core::messages::ReadWatermark> {
    Ok(im_core::messages::ReadWatermark {
        last_read_message_id: value
            .last_read_message_id
            .map(im_core::ids::MessageId::parse)
            .transpose()?,
        last_read_thread_seq: value.last_read_thread_seq,
        read_at: value
            .read_at
            .map(|value| {
                chrono::DateTime::parse_from_rfc3339(value.trim())
                    .map(|value| value.with_timezone(&chrono::Utc))
                    .map_err(|err| {
                        im_core::ImError::invalid_input(Some("read_at".to_owned()), err.to_string())
                    })
            })
            .transpose()?,
    })
}

pub async fn mark_conversation_read(
    client: &Arc<crate::api::client::DartImClient>,
    request: DartMarkConversationReadRequest,
) -> Result<DartMarkThreadReadResult, DartImError> {
    let inner = client.clone_inner()?;
    inner
        .messages()
        .mark_conversation_read_async(request.try_into()?)
        .await
        .map(Into::into)
        .map_err(DartImError::from)
}

pub async fn sync_delta(
    client: &Arc<crate::api::client::DartImClient>,
    request: DartSyncDeltaRequest,
) -> Result<DartSyncDeltaResult, DartImError> {
    let inner = client.clone_inner()?;
    inner
        .messages()
        .sync_delta_async(request.into())
        .await
        .map(Into::into)
        .map_err(DartImError::from)
}

pub async fn sync_now(
    client: &Arc<crate::api::client::DartImClient>,
    request: DartMessageSyncRequest,
) -> Result<DartMessageSyncOutcome, DartImError> {
    let inner = client.clone_inner()?;
    inner
        .messages()
        .sync_now_async(im_core::messages::MessageSyncRequest {
            reason: request.reason,
            limit: request.limit,
        })
        .await
        .map(Into::into)
        .map_err(DartImError::from)
}

pub async fn sync_thread_after(
    client: &Arc<crate::api::client::DartImClient>,
    request: DartSyncThreadAfterRequest,
) -> Result<DartSyncThreadAfterResult, DartImError> {
    let inner = client.clone_inner()?;
    inner
        .messages()
        .sync_thread_after_async(request.try_into()?)
        .await
        .map(Into::into)
        .map_err(DartImError::from)
}

pub async fn sync_conversation_after(
    client: &Arc<crate::api::client::DartImClient>,
    request: DartSyncConversationAfterRequest,
) -> Result<DartSyncThreadAfterResult, DartImError> {
    let inner = client.clone_inner()?;
    inner
        .messages()
        .sync_conversation_after_async(request.try_into()?)
        .await
        .map(Into::into)
        .map_err(DartImError::from)
}

pub async fn conversations(
    client: &Arc<crate::api::client::DartImClient>,
    limit: u32,
    cursor: Option<String>,
    include_groups: bool,
    include_direct: bool,
    unread_only: bool,
) -> Result<DartConversationPage, DartImError> {
    let inner = client.clone_inner()?;
    let cursor = cursor
        .filter(|value| !value.trim().is_empty())
        .map(im_core::ids::Cursor::parse)
        .transpose()?;
    let query = im_core::messages::ConversationQuery {
        limit: page_limit(limit)?,
        cursor,
        include_groups,
        include_direct,
        unread_only,
    };
    inner
        .messages()
        .conversations_async(query)
        .await
        .map(Into::into)
        .map_err(DartImError::from)
}

pub async fn ensure_conversation(
    client: &Arc<crate::api::client::DartImClient>,
    conversation_id: String,
) -> Result<(), DartImError> {
    let inner = client.clone_inner()?;
    inner
        .messages()
        .ensure_conversation_async(im_core::messages::ConversationReadRef::new(
            conversation_id,
        )?)
        .await
        .map_err(DartImError::from)
}

pub async fn load_conversation_snapshot(
    client: &Arc<crate::api::client::DartImClient>,
) -> Result<Option<DartConversationListSnapshot>, DartImError> {
    let inner = client.clone_inner()?;
    inner
        .messages()
        .load_conversation_snapshot_async()
        .await
        .map(|snapshot| snapshot.map(Into::into))
        .map_err(DartImError::from)
}

pub async fn clear_conversation_snapshot(
    client: &Arc<crate::api::client::DartImClient>,
) -> Result<(), DartImError> {
    let inner = client.clone_inner()?;
    inner
        .messages()
        .clear_conversation_snapshot_async()
        .await
        .map_err(DartImError::from)
}

pub async fn watch_conversation_patches(
    client: &Arc<crate::api::client::DartImClient>,
) -> Result<Arc<DartConversationPatchSession>, DartImError> {
    let inner = client.clone_inner()?;
    let session = inner
        .messages()
        .watch_conversation_patches_async()
        .await
        .map_err(DartImError::from)?;
    Ok(Arc::new(DartConversationPatchSession::new(session)))
}

pub async fn conversation_patch_stream(
    session: &Arc<DartConversationPatchSession>,
    sink: StreamSink<DartConversationStorePatch>,
) -> Result<(), DartImError> {
    let mut patch_session = session.take_session()?;
    session.lifecycle.spawn(async move {
        while let Some(patch) = patch_session.next_patch().await {
            if sink.add(patch.into()).is_err() {
                let _ = patch_session.stop().await;
                break;
            }
        }
    })
}

pub async fn stop_conversation_patch_session(
    session: &Arc<DartConversationPatchSession>,
) -> Result<(), DartImError> {
    session.stop().await
}

pub async fn repair_conversation_store(
    client: &Arc<crate::api::client::DartImClient>,
) -> Result<DartConversationStorePatch, DartImError> {
    let inner = client.clone_inner()?;
    inner
        .messages()
        .repair_conversation_store_async()
        .await
        .map(Into::into)
        .map_err(DartImError::from)
}

pub async fn watch_thread_patches(
    client: &Arc<crate::api::client::DartImClient>,
    thread: DartThreadRef,
    limit: Option<u32>,
) -> Result<Arc<DartThreadMessagePatchSession>, DartImError> {
    let inner = client.clone_inner()?;
    let session = inner
        .messages()
        .watch_thread_patches_async(thread.try_into()?, limit)
        .await
        .map_err(DartImError::from)?;
    Ok(Arc::new(DartThreadMessagePatchSession::new(session)))
}

pub async fn watch_conversation_timeline_patches(
    client: &Arc<crate::api::client::DartImClient>,
    conversation: DartConversationReadRef,
    limit: Option<u32>,
) -> Result<Arc<DartThreadMessagePatchSession>, DartImError> {
    let inner = client.clone_inner()?;
    let session = inner
        .messages()
        .watch_conversation_timeline_patches_async(conversation.try_into()?, limit)
        .await
        .map_err(DartImError::from)?;
    Ok(Arc::new(DartThreadMessagePatchSession::new(session)))
}

pub async fn thread_message_patch_stream(
    session: &Arc<DartThreadMessagePatchSession>,
    sink: StreamSink<DartThreadMessageStorePatch>,
) -> Result<(), DartImError> {
    let mut patch_session = session.take_session()?;
    session.lifecycle.spawn(async move {
        while let Some(patch) = patch_session.next_patch().await {
            if sink.add(patch.into()).is_err() {
                let _ = patch_session.stop().await;
                break;
            }
        }
    })
}

pub async fn stop_thread_message_patch_session(
    session: &Arc<DartThreadMessagePatchSession>,
) -> Result<(), DartImError> {
    session.stop().await
}

pub async fn repair_thread_store(
    client: &Arc<crate::api::client::DartImClient>,
    thread: DartThreadRef,
    limit: Option<u32>,
) -> Result<DartThreadMessageStorePatch, DartImError> {
    let inner = client.clone_inner()?;
    inner
        .messages()
        .repair_thread_store_async(thread.try_into()?, limit)
        .await
        .map(Into::into)
        .map_err(DartImError::from)
}

pub async fn repair_conversation_timeline_store(
    client: &Arc<crate::api::client::DartImClient>,
    conversation: DartConversationReadRef,
    limit: Option<u32>,
) -> Result<DartThreadMessageStorePatch, DartImError> {
    let inner = client.clone_inner()?;
    inner
        .messages()
        .repair_conversation_timeline_store_async(conversation.try_into()?, limit)
        .await
        .map(Into::into)
        .map_err(DartImError::from)
}

pub fn retry_message(
    _client: &Arc<crate::api::client::DartImClient>,
    _message_id: String,
) -> Result<DartSendMessageResult, DartImError> {
    Err(DartImError::unsupported("message-retry"))
}
