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
        _query: super::InboxQuery,
    ) -> crate::ImResult<crate::ids::Page<super::Message>> {
        Err(crate::ImError::TransportUnavailable {
            detail: "message inbox transport is not wired in Phase 1A".to_string(),
        })
    }

    pub fn history(
        &self,
        _thread: super::ThreadRef,
        _query: super::HistoryQuery,
    ) -> crate::ImResult<crate::ids::Page<super::Message>> {
        Err(crate::ImError::TransportUnavailable {
            detail: "message history transport is not wired in Phase 1A".to_string(),
        })
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
