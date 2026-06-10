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
        if attachment_security_is_secure(&request.security) {
            let result = self
                .client
                .messages()
                .send_secure_attachment(message_request_from_attachment(target, request)?)?;
            return attachment_send_result_from_secure_message(result);
        }
        let resolved_target = resolve_direct_target(self.client, &target)?;
        let mut result = crate::internal::attachment_runtime::upload::AttachmentUploadRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .send(
            crate::internal::attachment_runtime::upload::AttachmentSendInput {
                target,
                request,
                resolved_target_did: resolved_target.target_did.clone(),
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
                    resolved_target.direct_handle.as_deref(),
                    resolved_target.peer_scope.as_ref(),
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

    pub async fn send_async(
        &self,
        target: crate::messages::MessageTarget,
        request: super::AttachmentSendRequest,
    ) -> crate::ImResult<super::AttachmentSendResult> {
        if attachment_security_is_secure(&request.security) {
            let result = self
                .client
                .messages()
                .send_secure_attachment_async(message_request_from_attachment(target, request)?)
                .await?;
            return attachment_send_result_from_secure_message(result);
        }
        let resolved_target = resolve_direct_target_async(self.client, &target).await?;
        let mut result = crate::internal::attachment_runtime::upload::AttachmentUploadRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .send_async(
            crate::internal::attachment_runtime::upload::AttachmentSendInput {
                target,
                request,
                resolved_target_did: resolved_target.target_did.clone(),
                credentials: None,
            },
        )
        .await?;
        #[cfg(feature = "sqlite")]
        {
            let projection = if result.target_kind == "group" {
                crate::internal::message_runtime::local_projection::persist_group_attachment_outgoing_async(
                    self.client,
                    &result.target_did,
                    &result.manifest,
                    &result.sdk_result,
                )
                .await
            } else {
                crate::internal::message_runtime::local_projection::persist_direct_attachment_outgoing_async(
                    self.client,
                    &result.target_did,
                    resolved_target.direct_handle.as_deref(),
                    resolved_target.peer_scope.as_ref(),
                    &result.manifest,
                    &result.sdk_result,
                )
                .await
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

    pub async fn download_async(
        &self,
        request: super::DownloadAttachmentRequest,
    ) -> crate::ImResult<super::DownloadedAttachment> {
        let resolved_peer_did =
            resolve_direct_thread_did_async(self.client, &request.thread).await?;
        crate::internal::attachment_runtime::download::AttachmentDownloadRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .download_async(
            crate::internal::attachment_runtime::download::AttachmentDownloadInput {
                request,
                resolved_peer_did,
            },
        )
        .await
        .map(|result| result.sdk_result)
    }
}

fn attachment_security_is_secure(security: &crate::messages::MessageSecurityMode) -> bool {
    matches!(
        security,
        crate::messages::MessageSecurityMode::E2eeRequired
            | crate::messages::MessageSecurityMode::SecureDirect
            | crate::messages::MessageSecurityMode::GroupE2ee
    )
}

fn message_request_from_attachment(
    target: crate::messages::MessageTarget,
    request: super::AttachmentSendRequest,
) -> crate::ImResult<crate::messages::SendMessageRequest> {
    Ok(crate::messages::SendMessageRequest {
        target,
        body: crate::messages::MessageBody::Attachment {
            input: request.input,
            caption: request.caption,
            mime_type: request.mime_type,
            filename: request.filename,
        },
        security: request.security,
        client_message_id: None,
        delivery: request.delivery,
    })
}

fn attachment_send_result_from_secure_message(
    message: crate::messages::SendMessageResult,
) -> crate::ImResult<super::AttachmentSendResult> {
    let manifest = secure_attachment_manifest_from_message(&message)?;
    let parsed = crate::attachments::manifest::parse_attachment_manifest(&manifest)?;
    let descriptor = selected_attachment_descriptor(&parsed)?;
    let target_kind = match &message.message.thread {
        crate::messages::ThreadRef::Direct(_) => "agent".to_owned(),
        crate::messages::ThreadRef::Group(_) => "group".to_owned(),
        crate::messages::ThreadRef::Thread(_) => {
            return Err(crate::ImError::unsupported("thread-attachment-send"));
        }
    };
    let target_did = secure_attachment_target_did(&message);
    let size_bytes = descriptor.size.trim().parse::<u64>().map_err(|_| {
        crate::ImError::invalid_input(
            Some("size".to_owned()),
            "attachment size must be an unsigned integer",
        )
    })?;
    let plaintext_size_bytes = descriptor
        .plaintext_size
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| {
            value.parse::<u64>().map_err(|_| {
                crate::ImError::invalid_input(
                    Some("plaintext_size".to_owned()),
                    "attachment plaintext_size must be an unsigned integer",
                )
            })
        })
        .transpose()?;

    Ok(super::AttachmentSendResult {
        message,
        target_kind,
        target_did,
        attachment: super::UploadedAttachment {
            attachment_id: descriptor.attachment_id.clone(),
            filename: descriptor.filename.clone(),
            mime_type: descriptor.mime_type.clone(),
            size_bytes,
            size: descriptor.size.clone(),
            digest_b64u: descriptor.digest_b64u.clone(),
            object_uri: descriptor.object_uri.clone(),
            object_encryption_mode: descriptor.object_encryption_mode(),
            plaintext_size_bytes,
        },
        manifest,
    })
}

fn secure_attachment_manifest_from_message(
    message: &crate::messages::SendMessageResult,
) -> crate::ImResult<serde_json::Value> {
    let manifest = message
        .message
        .metadata
        .attributes
        .iter()
        .find(|attribute| attribute.key == "attachment_manifest")
        .map(|attribute| attribute.value.as_str())
        .ok_or_else(|| crate::ImError::Internal {
            message: "secure attachment result missing redacted attachment manifest".to_owned(),
        })?;
    serde_json::from_str(manifest).map_err(|err| crate::ImError::Serialization {
        detail: err.to_string(),
    })
}

fn selected_attachment_descriptor(
    manifest: &crate::attachments::manifest::AttachmentManifest,
) -> crate::ImResult<&crate::attachments::manifest::AttachmentDescriptor> {
    if !manifest.primary_attachment_id.trim().is_empty() {
        return manifest
            .attachments
            .iter()
            .find(|attachment| attachment.attachment_id == manifest.primary_attachment_id)
            .ok_or_else(|| {
                crate::ImError::invalid_input(
                    Some("primary_attachment_id".to_owned()),
                    "attachment manifest primary attachment is missing",
                )
            });
    }
    if manifest.attachments.len() == 1 {
        return Ok(&manifest.attachments[0]);
    }
    Err(crate::ImError::invalid_input(
        Some("attachment_id".to_owned()),
        "attachment_id is required for messages with multiple attachments",
    ))
}

fn secure_attachment_target_did(message: &crate::messages::SendMessageResult) -> String {
    match &message.message.thread {
        crate::messages::ThreadRef::Direct(peer) => message
            .message
            .metadata
            .attributes
            .iter()
            .find(|attribute| attribute.key == "resolved_target_did")
            .map(|attribute| attribute.value.clone())
            .filter(|value| !value.trim().is_empty())
            .or_else(|| {
                message
                    .message
                    .receiver
                    .as_ref()
                    .map(|peer| peer.as_str().to_owned())
            })
            .unwrap_or_else(|| peer.as_str().to_owned()),
        crate::messages::ThreadRef::Group(group) => group.as_str().to_owned(),
        crate::messages::ThreadRef::Thread(thread) => thread.as_str().to_owned(),
    }
}

struct ResolvedDirectTarget {
    target_did: Option<String>,
    direct_handle: Option<String>,
    peer_scope: Option<crate::internal::local_state::owner_scope::DirectPeerScope>,
}

fn resolve_direct_target(
    client: &crate::core::ImClient,
    target: &crate::messages::MessageTarget,
) -> crate::ImResult<ResolvedDirectTarget> {
    match target {
        crate::messages::MessageTarget::Direct(peer) => resolve_peer(client, peer),
        crate::messages::MessageTarget::Group(_) => Ok(ResolvedDirectTarget {
            target_did: None,
            direct_handle: None,
            peer_scope: None,
        }),
    }
}

async fn resolve_direct_target_async(
    client: &crate::core::ImClient,
    target: &crate::messages::MessageTarget,
) -> crate::ImResult<ResolvedDirectTarget> {
    match target {
        crate::messages::MessageTarget::Direct(peer) => resolve_peer_async(client, peer).await,
        crate::messages::MessageTarget::Group(_) => Ok(ResolvedDirectTarget {
            target_did: None,
            direct_handle: None,
            peer_scope: None,
        }),
    }
}

fn resolve_direct_thread_did(
    client: &crate::core::ImClient,
    thread: &crate::messages::ThreadRef,
) -> crate::ImResult<Option<String>> {
    match thread {
        crate::messages::ThreadRef::Direct(peer) => resolve_peer_current_did(client, peer),
        crate::messages::ThreadRef::Group(_) => Ok(None),
        crate::messages::ThreadRef::Thread(_) => Ok(None),
    }
}

async fn resolve_direct_thread_did_async(
    client: &crate::core::ImClient,
    thread: &crate::messages::ThreadRef,
) -> crate::ImResult<Option<String>> {
    match thread {
        crate::messages::ThreadRef::Direct(peer) => {
            resolve_peer_current_did_async(client, peer).await
        }
        crate::messages::ThreadRef::Group(_) => Ok(None),
        crate::messages::ThreadRef::Thread(_) => Ok(None),
    }
}

fn resolve_peer_current_did(
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

async fn resolve_peer_current_did_async(
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
        .lookup_handle_async(handle)
        .await
        .map(|lookup| Some(lookup.did.as_str().to_string()))
}

fn resolve_peer(
    client: &crate::core::ImClient,
    peer: &crate::ids::PeerRef,
) -> crate::ImResult<ResolvedDirectTarget> {
    let raw = peer.as_str().trim();
    if raw.is_empty() || raw.starts_with("did:") {
        return Ok(ResolvedDirectTarget {
            target_did: None,
            direct_handle: None,
            peer_scope: None,
        });
    }
    let handle = crate::ids::Handle::parse(raw, "")?;
    let lookup = client.directory().lookup_handle(handle)?;
    Ok(ResolvedDirectTarget {
        target_did: Some(lookup.did.as_str().to_string()),
        direct_handle: Some(lookup.handle.as_str().to_string()),
        peer_scope: Some(
            crate::internal::local_state::owner_scope::DirectPeerScope::new(
                lookup.user_id,
                lookup.handle.as_str().to_string(),
            )?,
        ),
    })
}

async fn resolve_peer_async(
    client: &crate::core::ImClient,
    peer: &crate::ids::PeerRef,
) -> crate::ImResult<ResolvedDirectTarget> {
    let raw = peer.as_str().trim();
    if raw.is_empty() || raw.starts_with("did:") {
        return Ok(ResolvedDirectTarget {
            target_did: None,
            direct_handle: None,
            peer_scope: None,
        });
    }
    let handle = crate::ids::Handle::parse(raw, "")?;
    let lookup = client.directory().lookup_handle_async(handle).await?;
    Ok(ResolvedDirectTarget {
        target_did: Some(lookup.did.as_str().to_string()),
        direct_handle: Some(lookup.handle.as_str().to_string()),
        peer_scope: Some(
            crate::internal::local_state::owner_scope::DirectPeerScope::new(
                lookup.user_id,
                lookup.handle.as_str().to_string(),
            )?,
        ),
    })
}
