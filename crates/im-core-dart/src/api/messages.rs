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

pub fn send_text(
    client: &Arc<crate::api::client::DartImClient>,
    request: DartSendTextRequest,
) -> Result<DartSendMessageResult, DartImError> {
    client.with_inner(|inner| {
        inner
            .messages()
            .send(request.try_into()?)
            .map(Into::into)
            .map_err(DartImError::from)
    })
}

pub fn inbox(
    client: &Arc<crate::api::client::DartImClient>,
    limit: u32,
    cursor: Option<String>,
    unread_only: bool,
) -> Result<DartMessagePage, DartImError> {
    client.with_inner(|inner| {
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
            .inbox(query)
            .map(Into::into)
            .map_err(DartImError::from)
    })
}

pub fn history(
    client: &Arc<crate::api::client::DartImClient>,
    thread: DartThreadRef,
    limit: u32,
    cursor: Option<String>,
) -> Result<DartMessagePage, DartImError> {
    client.with_inner(|inner| {
        let query = im_core::messages::HistoryQuery {
            limit: page_limit(limit)?,
            cursor: cursor
                .map(im_core::ids::Cursor::parse)
                .transpose()
                .map_err(DartImError::from)?,
        };
        inner
            .messages()
            .history(thread.try_into()?, query)
            .map(Into::into)
            .map_err(DartImError::from)
    })
}

pub fn mark_read(
    client: &Arc<crate::api::client::DartImClient>,
    message_ids: Vec<String>,
) -> Result<DartMarkReadResult, DartImError> {
    client.with_inner(|inner| {
        let ids = message_ids
            .into_iter()
            .map(im_core::ids::MessageId::parse)
            .collect::<Result<Vec<_>, _>>()
            .map_err(DartImError::from)?;
        inner
            .messages()
            .mark_read(ids)
            .map(Into::into)
            .map_err(DartImError::from)
    })
}

pub fn conversations(
    client: &Arc<crate::api::client::DartImClient>,
    limit: u32,
    include_groups: bool,
    include_direct: bool,
    unread_only: bool,
) -> Result<DartConversationPage, DartImError> {
    client.with_inner(|inner| {
        let query = im_core::messages::ConversationQuery {
            limit: page_limit(limit)?,
            include_groups,
            include_direct,
            unread_only,
        };
        inner
            .messages()
            .conversations(query)
            .map(Into::into)
            .map_err(DartImError::from)
    })
}

pub fn retry_message(
    _client: &Arc<crate::api::client::DartImClient>,
    _message_id: String,
) -> Result<DartSendMessageResult, DartImError> {
    Err(DartImError::unsupported("message-retry"))
}
