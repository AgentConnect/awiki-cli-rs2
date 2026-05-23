use std::sync::Arc;

use crate::dto::{
    attachment::{
        DartAttachmentSendRequest, DartDownloadAttachmentRequest, DartDownloadedAttachment,
    },
    error::DartImError,
    message::DartSendMessageResult,
};

pub fn send_attachment(
    client: Arc<crate::api::client::DartImClient>,
    request: DartAttachmentSendRequest,
) -> Result<DartSendMessageResult, DartImError> {
    client.with_inner(|inner| {
        let (target, request) = request.into_core()?;
        inner
            .attachments()
            .send(target, request)
            .map(Into::into)
            .map_err(DartImError::from)
    })
}

pub fn download_attachment(
    client: Arc<crate::api::client::DartImClient>,
    request: DartDownloadAttachmentRequest,
) -> Result<DartDownloadedAttachment, DartImError> {
    client.with_inner(|inner| {
        inner
            .attachments()
            .download(request.try_into()?)
            .map(Into::into)
            .map_err(DartImError::from)
    })
}
