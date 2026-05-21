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
        let runtime = self.client.runtime();
        let _runtime_context = (
            &runtime.did_document_path,
            &runtime.private_key_path,
            &runtime.auth_state_path,
            &runtime.owner.identity_id,
            &runtime.owner.current_did,
        );
        let _transport_policy = self.client.core_inner().sdk_config().transport_policy;
        Err(crate::ImError::TransportUnavailable {
            detail: "message transport is not wired in Phase 1A".to_string(),
        })
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
