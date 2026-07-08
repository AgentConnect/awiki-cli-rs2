use std::sync::Arc;

use crate::dto::{
    attachment::{
        DartAttachmentSendRequest, DartAttachmentSendResult, DartDownloadAttachmentRequest,
        DartDownloadedAttachment, DartSendConversationAttachmentRequest,
    },
    error::DartImError,
};

pub async fn send_attachment(
    client: &Arc<crate::api::client::DartImClient>,
    request: DartAttachmentSendRequest,
) -> Result<DartAttachmentSendResult, DartImError> {
    let inner = client.clone_inner()?;
    let (target, request) = request.into_core()?;
    inner
        .attachments()
        .send_async(target, request)
        .await
        .map(Into::into)
        .map_err(DartImError::from)
}

pub async fn send_conversation_attachment(
    client: &Arc<crate::api::client::DartImClient>,
    request: DartSendConversationAttachmentRequest,
) -> Result<DartAttachmentSendResult, DartImError> {
    let inner = client.clone_inner()?;
    inner
        .attachments()
        .send_conversation_async(request.try_into()?)
        .await
        .map(Into::into)
        .map_err(DartImError::from)
}

pub async fn download_attachment(
    client: &Arc<crate::api::client::DartImClient>,
    request: DartDownloadAttachmentRequest,
) -> Result<DartDownloadedAttachment, DartImError> {
    let inner = client.clone_inner()?;
    inner
        .attachments()
        .download_async(request.try_into()?)
        .await
        .map(Into::into)
        .map_err(DartImError::from)
}
