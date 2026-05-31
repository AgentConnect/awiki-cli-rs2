use crate::internal::auth::session::{AsyncSessionProvider, SessionProvider};
use crate::internal::transport::{AsyncAuthenticatedRpcTransport, AuthenticatedRpcTransport};

pub(crate) struct EmailRuntime<'a, P, T> {
    client: &'a crate::core::ImClient,
    session_provider: P,
    transport: T,
}

impl<'a, P, T> EmailRuntime<'a, P, T> {
    pub(crate) fn new(
        client: &'a crate::core::ImClient,
        session_provider: P,
        transport: T,
    ) -> Self {
        Self {
            client,
            session_provider,
            transport,
        }
    }
}

impl<'a, P, T> EmailRuntime<'a, P, T>
where
    P: SessionProvider,
    T: AuthenticatedRpcTransport,
{
    pub(crate) fn account(mut self) -> crate::ImResult<crate::email::EmailAccount> {
        self.ensure_messaging_session()?;
        let call = crate::internal::email_wire::build_account_rpc_call();
        let raw = self
            .transport
            .authenticated_rpc(call.endpoint, call.method, call.params)?;
        crate::internal::email_wire::normalize::account(raw)
    }

    pub(crate) fn inbox(
        mut self,
        query: crate::email::EmailInboxQuery,
    ) -> crate::ImResult<crate::ids::Page<crate::email::EmailMessageSummary>> {
        self.ensure_messaging_session()?;
        let call = crate::internal::email_wire::build_inbox_rpc_call(query);
        let raw = self
            .transport
            .authenticated_rpc(call.endpoint, call.method, call.params)?;
        crate::internal::email_wire::normalize::inbox(raw)
    }

    pub(crate) fn read(
        mut self,
        id: crate::email::EmailMessageId,
    ) -> crate::ImResult<crate::email::EmailMessage> {
        self.ensure_messaging_session()?;
        let call = crate::internal::email_wire::build_read_rpc_call(&id);
        let raw = self
            .transport
            .authenticated_rpc(call.endpoint, call.method, call.params)?;
        crate::internal::email_wire::normalize::message(raw)
    }

    pub(crate) fn mark_read(
        mut self,
        request: crate::email::EmailMarkReadRequest,
    ) -> crate::ImResult<crate::email::EmailMarkReadResult> {
        self.ensure_messaging_session()?;
        let call = crate::internal::email_wire::build_mark_read_rpc_call(request)?;
        let raw = self
            .transport
            .authenticated_rpc(call.endpoint, call.method, call.params)?;
        Ok(crate::internal::email_wire::normalize::mark_read(raw))
    }

    pub(crate) fn send(
        mut self,
        request: crate::email::SendEmailRequest,
    ) -> crate::ImResult<crate::email::SendEmailResult> {
        self.ensure_messaging_session()?;
        let call = crate::internal::email_wire::build_send_rpc_call(request)?;
        let raw = self
            .transport
            .authenticated_rpc(call.endpoint, call.method, call.params)?;
        Ok(crate::internal::email_wire::normalize::send(raw))
    }

    pub(crate) fn download_attachment(
        mut self,
        request: crate::email::EmailAttachmentDownloadRequest,
    ) -> crate::ImResult<crate::email::EmailAttachmentContent> {
        self.ensure_messaging_session()?;
        let call = crate::internal::email_wire::build_attachment_rpc_call(&request);
        let raw = self
            .transport
            .authenticated_rpc(call.endpoint, call.method, call.params)?;
        crate::internal::email_wire::normalize::attachment(request, raw)
    }

    pub(crate) fn notifications(
        self,
        query: crate::email::EmailNotificationQuery,
    ) -> crate::ImResult<crate::ids::Page<crate::email::EmailNotification>> {
        let owner = self.client.runtime().owner.current_did.as_str();
        let owner_identity_id = self.client.runtime().owner.identity_id.as_str();
        crate::internal::local_state::email::list_mail_notifications(
            self.client
                .core_inner()
                .sdk_paths()
                .local_state
                .sqlite_path
                .as_path(),
            owner_identity_id,
            owner,
            query.limit,
        )
    }

    fn ensure_messaging_session(&self) -> crate::ImResult<crate::auth::SessionBundle> {
        self.session_provider
            .ensure_session(crate::auth::AuthScope::Messaging)
    }
}

impl<'a, P, T> EmailRuntime<'a, P, T>
where
    P: AsyncSessionProvider,
    T: AsyncAuthenticatedRpcTransport,
{
    pub(crate) async fn account_async(mut self) -> crate::ImResult<crate::email::EmailAccount> {
        self.ensure_messaging_session_async().await?;
        let call = crate::internal::email_wire::build_account_rpc_call();
        let raw = self
            .transport
            .authenticated_rpc(call.endpoint, call.method, call.params)
            .await?;
        crate::internal::email_wire::normalize::account(raw)
    }

    pub(crate) async fn inbox_async(
        mut self,
        query: crate::email::EmailInboxQuery,
    ) -> crate::ImResult<crate::ids::Page<crate::email::EmailMessageSummary>> {
        self.ensure_messaging_session_async().await?;
        let call = crate::internal::email_wire::build_inbox_rpc_call(query);
        let raw = self
            .transport
            .authenticated_rpc(call.endpoint, call.method, call.params)
            .await?;
        crate::internal::email_wire::normalize::inbox(raw)
    }

    pub(crate) async fn read_async(
        mut self,
        id: crate::email::EmailMessageId,
    ) -> crate::ImResult<crate::email::EmailMessage> {
        self.ensure_messaging_session_async().await?;
        let call = crate::internal::email_wire::build_read_rpc_call(&id);
        let raw = self
            .transport
            .authenticated_rpc(call.endpoint, call.method, call.params)
            .await?;
        crate::internal::email_wire::normalize::message(raw)
    }

    pub(crate) async fn mark_read_async(
        mut self,
        request: crate::email::EmailMarkReadRequest,
    ) -> crate::ImResult<crate::email::EmailMarkReadResult> {
        self.ensure_messaging_session_async().await?;
        let call = crate::internal::email_wire::build_mark_read_rpc_call(request)?;
        let raw = self
            .transport
            .authenticated_rpc(call.endpoint, call.method, call.params)
            .await?;
        Ok(crate::internal::email_wire::normalize::mark_read(raw))
    }

    pub(crate) async fn send_async(
        mut self,
        request: crate::email::SendEmailRequest,
    ) -> crate::ImResult<crate::email::SendEmailResult> {
        self.ensure_messaging_session_async().await?;
        let call = crate::internal::email_wire::build_send_rpc_call(request)?;
        let raw = self
            .transport
            .authenticated_rpc(call.endpoint, call.method, call.params)
            .await?;
        Ok(crate::internal::email_wire::normalize::send(raw))
    }

    pub(crate) async fn download_attachment_async(
        mut self,
        request: crate::email::EmailAttachmentDownloadRequest,
    ) -> crate::ImResult<crate::email::EmailAttachmentContent> {
        self.ensure_messaging_session_async().await?;
        let call = crate::internal::email_wire::build_attachment_rpc_call(&request);
        let raw = self
            .transport
            .authenticated_rpc(call.endpoint, call.method, call.params)
            .await?;
        crate::internal::email_wire::normalize::attachment(request, raw)
    }

    pub(crate) async fn notifications_async(
        self,
        query: crate::email::EmailNotificationQuery,
    ) -> crate::ImResult<crate::ids::Page<crate::email::EmailNotification>> {
        let owner = self.client.runtime().owner.current_did.as_str();
        let owner_identity_id = self.client.runtime().owner.identity_id.as_str();
        let db = self.client.core_inner().local_state_db().await?;
        db.list_mail_notifications(owner_identity_id, owner, query.limit)
            .await
    }

    async fn ensure_messaging_session_async(&self) -> crate::ImResult<crate::auth::SessionBundle> {
        self.session_provider
            .ensure_session(crate::auth::AuthScope::Messaging)
            .await
    }
}
