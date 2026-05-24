use std::sync::Arc;

use crate::dto::{
    email::{
        DartEmailAccount, DartEmailAttachmentContent, DartEmailMarkReadResult, DartEmailMessage,
        DartEmailMessageSummaryPage, DartEmailNotificationPage, DartSendEmailRequest,
        DartSendEmailResult,
    },
    error::DartImError,
};

fn page_limit(limit: u32) -> Result<im_core::ids::PageLimit, DartImError> {
    im_core::ids::PageLimit::new(limit).map_err(DartImError::from)
}

pub fn account(
    client: &Arc<crate::api::client::DartImClient>,
) -> Result<DartEmailAccount, DartImError> {
    client.with_inner(|inner| {
        inner
            .email()
            .account()
            .map(Into::into)
            .map_err(DartImError::from)
    })
}

pub fn inbox(
    client: &Arc<crate::api::client::DartImClient>,
    folder: String,
    limit: u32,
    offset: u32,
    unread_only: bool,
) -> Result<DartEmailMessageSummaryPage, DartImError> {
    client.with_inner(|inner| {
        let folder = if folder.trim().is_empty() {
            im_core::email::EmailFolder::inbox()
        } else {
            im_core::email::EmailFolder::parse(folder).map_err(DartImError::from)?
        };
        let query = im_core::email::EmailInboxQuery {
            folder,
            limit: page_limit(limit)?,
            offset,
            unread_only,
        };
        inner
            .email()
            .inbox(query)
            .map(Into::into)
            .map_err(DartImError::from)
    })
}

pub fn read(
    client: &Arc<crate::api::client::DartImClient>,
    message_id: String,
) -> Result<DartEmailMessage, DartImError> {
    client.with_inner(|inner| {
        inner
            .email()
            .read(im_core::email::EmailMessageId::parse(message_id).map_err(DartImError::from)?)
            .map(Into::into)
            .map_err(DartImError::from)
    })
}

pub fn mark_read(
    client: &Arc<crate::api::client::DartImClient>,
    message_ids: Vec<String>,
    is_read: bool,
) -> Result<DartEmailMarkReadResult, DartImError> {
    client.with_inner(|inner| {
        let message_ids = message_ids
            .into_iter()
            .map(im_core::email::EmailMessageId::parse)
            .collect::<Result<Vec<_>, _>>()
            .map_err(DartImError::from)?;
        inner
            .email()
            .mark_read(im_core::email::EmailMarkReadRequest {
                message_ids,
                is_read,
            })
            .map(Into::into)
            .map_err(DartImError::from)
    })
}

pub fn send(
    client: &Arc<crate::api::client::DartImClient>,
    request: DartSendEmailRequest,
) -> Result<DartSendEmailResult, DartImError> {
    client.with_inner(|inner| {
        inner
            .email()
            .send(request.try_into()?)
            .map(Into::into)
            .map_err(DartImError::from)
    })
}

pub fn download_attachment(
    client: &Arc<crate::api::client::DartImClient>,
    message_id: String,
    attachment_index: u32,
) -> Result<DartEmailAttachmentContent, DartImError> {
    client.with_inner(|inner| {
        inner
            .email()
            .download_attachment(im_core::email::EmailAttachmentDownloadRequest {
                message_id: im_core::email::EmailMessageId::parse(message_id)
                    .map_err(DartImError::from)?,
                attachment_index,
            })
            .map(Into::into)
            .map_err(DartImError::from)
    })
}

pub fn notifications(
    client: &Arc<crate::api::client::DartImClient>,
    limit: u32,
) -> Result<DartEmailNotificationPage, DartImError> {
    client.with_inner(|inner| {
        inner
            .email()
            .notifications(im_core::email::EmailNotificationQuery {
                limit: page_limit(limit)?,
            })
            .map(Into::into)
            .map_err(DartImError::from)
    })
}
