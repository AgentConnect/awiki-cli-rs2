pub struct AttachmentService<'a> {
    client: &'a crate::core::ImClient,
}

impl<'a> AttachmentService<'a> {
    pub(crate) fn new(client: &'a crate::core::ImClient) -> Self {
        Self { client }
    }

    pub fn send(
        &self,
        _target: crate::messages::MessageTarget,
        _request: super::AttachmentSendRequest,
    ) -> crate::ImResult<crate::messages::SendMessageResult> {
        let _ = self.client;
        Err(crate::ImError::unsupported("attachments"))
    }

    pub fn download(
        &self,
        _request: super::DownloadAttachmentRequest,
    ) -> crate::ImResult<super::DownloadedAttachment> {
        let _ = self.client;
        Err(crate::ImError::unsupported("attachments"))
    }
}
