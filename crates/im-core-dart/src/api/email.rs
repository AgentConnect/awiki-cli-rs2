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

pub async fn account(
    client: &Arc<crate::api::client::DartImClient>,
) -> Result<DartEmailAccount, DartImError> {
    let inner = client.clone_inner()?;
    inner
        .email()
        .account_async()
        .await
        .map(Into::into)
        .map_err(DartImError::from)
}

pub async fn inbox(
    client: &Arc<crate::api::client::DartImClient>,
    folder: String,
    limit: u32,
    offset: u32,
    unread_only: bool,
) -> Result<DartEmailMessageSummaryPage, DartImError> {
    let inner = client.clone_inner()?;
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
        .inbox_async(query)
        .await
        .map(Into::into)
        .map_err(DartImError::from)
}

pub async fn read(
    client: &Arc<crate::api::client::DartImClient>,
    message_id: String,
) -> Result<DartEmailMessage, DartImError> {
    let inner = client.clone_inner()?;
    let message_id =
        im_core::email::EmailMessageId::parse(message_id).map_err(DartImError::from)?;
    inner
        .email()
        .read_async(message_id)
        .await
        .map(Into::into)
        .map_err(DartImError::from)
}

pub async fn mark_read(
    client: &Arc<crate::api::client::DartImClient>,
    message_ids: Vec<String>,
    is_read: bool,
) -> Result<DartEmailMarkReadResult, DartImError> {
    let inner = client.clone_inner()?;
    let message_ids = message_ids
        .into_iter()
        .map(im_core::email::EmailMessageId::parse)
        .collect::<Result<Vec<_>, _>>()
        .map_err(DartImError::from)?;
    inner
        .email()
        .mark_read_async(im_core::email::EmailMarkReadRequest {
            message_ids,
            is_read,
        })
        .await
        .map(Into::into)
        .map_err(DartImError::from)
}

pub async fn send(
    client: &Arc<crate::api::client::DartImClient>,
    request: DartSendEmailRequest,
) -> Result<DartSendEmailResult, DartImError> {
    let inner = client.clone_inner()?;
    inner
        .email()
        .send_async(request.try_into()?)
        .await
        .map(Into::into)
        .map_err(DartImError::from)
}

pub async fn download_attachment(
    client: &Arc<crate::api::client::DartImClient>,
    message_id: String,
    attachment_index: u32,
) -> Result<DartEmailAttachmentContent, DartImError> {
    let inner = client.clone_inner()?;
    inner
        .email()
        .download_attachment_async(im_core::email::EmailAttachmentDownloadRequest {
            message_id: im_core::email::EmailMessageId::parse(message_id)
                .map_err(DartImError::from)?,
            attachment_index,
        })
        .await
        .map(Into::into)
        .map_err(DartImError::from)
}

pub async fn notifications(
    client: &Arc<crate::api::client::DartImClient>,
    limit: u32,
) -> Result<DartEmailNotificationPage, DartImError> {
    let inner = client.clone_inner()?;
    inner
        .email()
        .notifications_async(im_core::email::EmailNotificationQuery {
            limit: page_limit(limit)?,
        })
        .await
        .map(Into::into)
        .map_err(DartImError::from)
}
