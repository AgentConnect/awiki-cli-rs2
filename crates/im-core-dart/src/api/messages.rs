use std::sync::Arc;

use crate::dto::{
    error::DartImError,
    message::{
        DartConversationPage, DartMarkReadResult, DartMessagePage, DartSendMessageResult,
        DartSendTextRequest, DartThreadRef,
    },
};

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

pub async fn inbox(
    client: &Arc<crate::api::client::DartImClient>,
    limit: u32,
    cursor: Option<String>,
    unread_only: bool,
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
) -> Result<DartMessagePage, DartImError> {
    let inner = client.clone_inner()?;
    let query = im_core::messages::HistoryQuery {
        limit: page_limit(limit)?,
        cursor: cursor
            .map(im_core::ids::Cursor::parse)
            .transpose()
            .map_err(DartImError::from)?,
    };
    inner
        .messages()
        .history_async(thread.try_into()?, query)
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

pub async fn conversations(
    client: &Arc<crate::api::client::DartImClient>,
    limit: u32,
    include_groups: bool,
    include_direct: bool,
    unread_only: bool,
) -> Result<DartConversationPage, DartImError> {
    let inner = client.clone_inner()?;
    let query = im_core::messages::ConversationQuery {
        limit: page_limit(limit)?,
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

pub fn retry_message(
    _client: &Arc<crate::api::client::DartImClient>,
    _message_id: String,
) -> Result<DartSendMessageResult, DartImError> {
    Err(DartImError::unsupported("message-retry"))
}
