pub struct EmailService<'a> {
    client: &'a crate::core::ImClient,
}

impl<'a> EmailService<'a> {
    pub(crate) fn new(client: &'a crate::core::ImClient) -> Self {
        Self { client }
    }

    pub fn account(&self) -> crate::ImResult<super::EmailAccount> {
        crate::internal::email_runtime::EmailRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .account()
    }

    pub fn inbox(
        &self,
        query: super::EmailInboxQuery,
    ) -> crate::ImResult<crate::ids::Page<super::EmailMessageSummary>> {
        crate::internal::email_runtime::EmailRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .inbox(query)
    }

    pub fn read(&self, id: super::EmailMessageId) -> crate::ImResult<super::EmailMessage> {
        crate::internal::email_runtime::EmailRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .read(id)
    }

    pub fn mark_read(
        &self,
        request: super::EmailMarkReadRequest,
    ) -> crate::ImResult<super::EmailMarkReadResult> {
        crate::internal::email_runtime::EmailRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .mark_read(request)
    }

    pub fn send(
        &self,
        request: super::SendEmailRequest,
    ) -> crate::ImResult<super::SendEmailResult> {
        crate::internal::email_runtime::EmailRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .send(request)
    }

    pub fn download_attachment(
        &self,
        request: super::EmailAttachmentDownloadRequest,
    ) -> crate::ImResult<super::EmailAttachmentContent> {
        crate::internal::email_runtime::EmailRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .download_attachment(request)
    }

    pub fn notifications(
        &self,
        query: super::EmailNotificationQuery,
    ) -> crate::ImResult<crate::ids::Page<super::EmailNotification>> {
        crate::internal::email_runtime::EmailRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .notifications(query)
    }
}
