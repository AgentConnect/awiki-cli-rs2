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

    pub async fn account_async(&self) -> crate::ImResult<super::EmailAccount> {
        crate::internal::email_runtime::EmailRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .account_async()
        .await
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

    pub async fn inbox_async(
        &self,
        query: super::EmailInboxQuery,
    ) -> crate::ImResult<crate::ids::Page<super::EmailMessageSummary>> {
        crate::internal::email_runtime::EmailRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .inbox_async(query)
        .await
    }

    pub fn read(&self, id: super::EmailMessageId) -> crate::ImResult<super::EmailMessage> {
        crate::internal::email_runtime::EmailRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .read(id)
    }

    pub async fn read_async(
        &self,
        id: super::EmailMessageId,
    ) -> crate::ImResult<super::EmailMessage> {
        crate::internal::email_runtime::EmailRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .read_async(id)
        .await
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

    pub async fn mark_read_async(
        &self,
        request: super::EmailMarkReadRequest,
    ) -> crate::ImResult<super::EmailMarkReadResult> {
        crate::internal::email_runtime::EmailRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .mark_read_async(request)
        .await
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

    pub async fn send_async(
        &self,
        request: super::SendEmailRequest,
    ) -> crate::ImResult<super::SendEmailResult> {
        crate::internal::email_runtime::EmailRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .send_async(request)
        .await
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

    pub async fn download_attachment_async(
        &self,
        request: super::EmailAttachmentDownloadRequest,
    ) -> crate::ImResult<super::EmailAttachmentContent> {
        crate::internal::email_runtime::EmailRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .download_attachment_async(request)
        .await
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

    pub async fn notifications_async(
        &self,
        query: super::EmailNotificationQuery,
    ) -> crate::ImResult<crate::ids::Page<super::EmailNotification>> {
        crate::internal::email_runtime::EmailRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .notifications_async(query)
        .await
    }
}
