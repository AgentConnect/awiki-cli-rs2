pub struct MessageService<'a> {
    client: &'a crate::core::ImClient,
}

impl<'a> MessageService<'a> {
    pub(crate) fn new(client: &'a crate::core::ImClient) -> Self {
        Self { client }
    }

    pub fn send(
        &self,
        request: super::SendMessageRequest,
    ) -> crate::ImResult<super::SendMessageResult> {
        validate_body(&request.body)?;
        validate_security(&request.security)?;
        match request.target {
            super::MessageTarget::Direct(_) => {
                crate::internal::message_runtime::direct::DirectTextSender::new(
                    self.client,
                    crate::internal::auth::session::FileSessionProvider::new(self.client),
                    crate::internal::transport::UnavailableTransport,
                )
                .send(crate::internal::message_runtime::direct::DirectTextSend {
                    request,
                    resolved_target_did: None,
                    credentials: None,
                })
                .map(|result| result.sdk_result)
            }
            super::MessageTarget::Group(_) => {
                crate::internal::message_runtime::group::GroupTextSender::new(
                    self.client,
                    crate::internal::auth::session::FileSessionProvider::new(self.client),
                    crate::internal::transport::UnavailableTransport,
                )
                .send(crate::internal::message_runtime::group::GroupTextSend {
                    request,
                    credentials: None,
                })
                .map(|result| result.sdk_result)
            }
        }
    }

    pub fn inbox(
        &self,
        query: super::InboxQuery,
    ) -> crate::ImResult<crate::ids::Page<super::Message>> {
        crate::internal::message_runtime::read::MessageReadRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::UnavailableTransport,
        )
        .inbox(crate::internal::message_runtime::read::InboxRead { query })
        .map(|result| result.page)
    }

    pub fn history(
        &self,
        thread: super::ThreadRef,
        query: super::HistoryQuery,
    ) -> crate::ImResult<crate::ids::Page<super::Message>> {
        crate::internal::message_runtime::read::MessageReadRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::UnavailableTransport,
        )
        .history(crate::internal::message_runtime::read::HistoryRead {
            thread,
            query,
            resolved_peer_did: None,
        })
        .map(|result| result.page)
    }

    pub fn mark_read(
        &self,
        ids: Vec<crate::ids::MessageId>,
    ) -> crate::ImResult<super::MarkReadResult> {
        crate::internal::message_runtime::mark_read::MessageMarkReadRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::UnavailableTransport,
        )
        .mark_read(crate::internal::message_runtime::mark_read::MarkReadInput { message_ids: ids })
        .map(|result| result.sdk_result)
    }

    pub fn conversations(
        &self,
        query: super::ConversationQuery,
    ) -> crate::ImResult<crate::ids::Page<super::Conversation>> {
        crate::internal::message_runtime::conversations::MessageConversationRuntime::new(
            self.client,
        )
        .conversations(query)
    }
}

fn validate_body(body: &super::MessageBody) -> crate::ImResult<()> {
    match body {
        super::MessageBody::Text { text, .. } if text.trim().is_empty() => {
            Err(crate::ImError::invalid_input(
                Some("text".to_string()),
                "text message must not be empty",
            ))
        }
        super::MessageBody::Text { .. } => Ok(()),
        super::MessageBody::Attachment { .. } => Err(crate::ImError::unsupported("attachments")),
    }
}

fn validate_security(security: &super::MessageSecurityMode) -> crate::ImResult<()> {
    match security {
        super::MessageSecurityMode::DefaultPlain | super::MessageSecurityMode::Plain => Ok(()),
        super::MessageSecurityMode::SecureDirect => {
            Err(crate::ImError::unsupported("secure-direct"))
        }
        super::MessageSecurityMode::GroupE2ee => Err(crate::ImError::unsupported("group-e2ee")),
    }
}
