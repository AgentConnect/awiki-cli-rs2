pub struct AttachmentService<'a> {
    client: &'a crate::core::ImClient,
}

impl<'a> AttachmentService<'a> {
    pub(crate) fn new(client: &'a crate::core::ImClient) -> Self {
        Self { client }
    }

    pub fn send(
        &self,
        target: crate::messages::MessageTarget,
        request: super::AttachmentSendRequest,
    ) -> crate::ImResult<super::AttachmentSendResult> {
        let resolved_target_did = resolve_direct_target_did(self.client, &target)?;
        let mut result = crate::internal::attachment_runtime::upload::AttachmentUploadRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .send(
            crate::internal::attachment_runtime::upload::AttachmentSendInput {
                target,
                request,
                resolved_target_did,
                credentials: None,
            },
        )?;
        #[cfg(feature = "sqlite")]
        {
            let projection = if result.target_kind == "group" {
                crate::internal::message_runtime::local_projection::persist_group_attachment_outgoing(
                    self.client,
                    &result.target_did,
                    &result.manifest,
                    &result.sdk_result,
                )
            } else {
                crate::internal::message_runtime::local_projection::persist_direct_attachment_outgoing(
                    self.client,
                    &result.target_did,
                    direct_handle_from_result(&result.sdk_result).as_deref(),
                    &result.manifest,
                    &result.sdk_result,
                )
            };
            if let Err(err) = projection {
                result
                    .sdk_result
                    .warnings
                    .push(format!("Failed to persist local attachment message: {err}"));
            }
        }
        Ok(super::AttachmentSendResult::from_upload_result(result))
    }

    pub fn download(
        &self,
        request: super::DownloadAttachmentRequest,
    ) -> crate::ImResult<super::DownloadedAttachment> {
        let resolved_peer_did = resolve_direct_thread_did(self.client, &request.thread)?;
        crate::internal::attachment_runtime::download::AttachmentDownloadRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .download(
            crate::internal::attachment_runtime::download::AttachmentDownloadInput {
                request,
                resolved_peer_did,
            },
        )
        .map(|result| result.sdk_result)
    }
}

fn direct_handle_from_result(result: &crate::messages::SendMessageResult) -> Option<String> {
    match &result.message.thread {
        crate::messages::ThreadRef::Direct(peer) if !peer.as_str().starts_with("did:") => {
            Some(peer.as_str().to_owned())
        }
        _ => None,
    }
}

fn resolve_direct_target_did(
    client: &crate::core::ImClient,
    target: &crate::messages::MessageTarget,
) -> crate::ImResult<Option<String>> {
    match target {
        crate::messages::MessageTarget::Direct(peer) => resolve_peer_did(client, peer),
        crate::messages::MessageTarget::Group(_) => Ok(None),
    }
}

fn resolve_direct_thread_did(
    client: &crate::core::ImClient,
    thread: &crate::messages::ThreadRef,
) -> crate::ImResult<Option<String>> {
    match thread {
        crate::messages::ThreadRef::Direct(peer) => resolve_peer_did(client, peer),
        crate::messages::ThreadRef::Group(_) => Ok(None),
        crate::messages::ThreadRef::Thread(_) => Ok(None),
    }
}

fn resolve_peer_did(
    client: &crate::core::ImClient,
    peer: &crate::ids::PeerRef,
) -> crate::ImResult<Option<String>> {
    let raw = peer.as_str().trim();
    if raw.is_empty() || raw.starts_with("did:") {
        return Ok(None);
    }
    let handle = crate::ids::Handle::parse(raw, "")?;
    client
        .directory()
        .lookup_handle(handle)
        .map(|lookup| Some(lookup.did.as_str().to_string()))
}
