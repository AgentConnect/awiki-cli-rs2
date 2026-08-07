pub struct AttachmentService<'a> {
    client: &'a crate::core::ImClient,
}

struct ResolvedConversationAttachmentRequest {
    target: crate::messages::MessageTarget,
    request: super::AttachmentSendRequest,
    client_message_id: crate::ids::MessageId,
    target_did: Option<String>,
    target_handle: Option<String>,
    peer_scope: Option<crate::internal::local_state::owner_scope::DirectPeerScope>,
    conversation_identity: crate::messages::ConversationIdentity,
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
        self.send_with_optional_client_message_id(target, request, None)
    }

    pub fn send_with_client_message_id(
        &self,
        target: crate::messages::MessageTarget,
        request: super::AttachmentSendRequest,
        client_message_id: crate::ids::MessageId,
    ) -> crate::ImResult<super::AttachmentSendResult> {
        self.send_with_optional_client_message_id(target, request, Some(client_message_id))
    }

    fn send_with_optional_client_message_id(
        &self,
        target: crate::messages::MessageTarget,
        request: super::AttachmentSendRequest,
        client_message_id: Option<crate::ids::MessageId>,
    ) -> crate::ImResult<super::AttachmentSendResult> {
        if attachment_security_is_secure(&request.security) {
            let result =
                self.client
                    .messages()
                    .send_secure_attachment(message_request_from_attachment(
                        target,
                        request,
                        client_message_id,
                    )?)?;
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
                client_message_id,
                credentials: None,
            },
        )?;
        if result.target_kind != "group" {
            crate::messages::normalize_direct_send_result_for_peer_scope(
                &mut result.sdk_result,
                resolved_target.peer_scope.as_ref(),
                resolved_target.direct_handle.as_deref(),
                Some(result.target_did.as_str()),
            )?;
        }
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
            match projection {
                Ok(()) => self
                    .client
                    .emit_committed_local_message_projection("local_send"),
                Err(err) => {
                    result
                        .sdk_result
                        .warnings
                        .push(format!("Failed to persist local attachment message: {err}"));
                }
            }
        }
        Ok(super::AttachmentSendResult::from_upload_result(result))
    }

    pub fn send_conversation(
        &self,
        request: super::SendConversationAttachmentRequest,
    ) -> crate::ImResult<super::AttachmentSendResult> {
        let resolved = self.resolve_conversation_attachment_request(request)?;
        if attachment_security_is_secure(&resolved.request.security) {
            let mut result = attachment_send_result_from_secure_message(
                self.client.messages().send_secure_attachment(
                    message_request_from_resolved_conversation_attachment(&resolved)?,
                )?,
            )?;
            result.message.message.metadata.conversation_identity =
                Some(resolved.conversation_identity);
            return Ok(result);
        }
        let mut result = crate::internal::attachment_runtime::upload::AttachmentUploadRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .send(
            crate::internal::attachment_runtime::upload::AttachmentSendInput {
                target: resolved.target,
                request: resolved.request,
                resolved_target_did: resolved.target_did.clone(),
                client_message_id: Some(resolved.client_message_id),
                credentials: None,
            },
        )?;
        apply_conversation_identity(
            &mut result.sdk_result,
            resolved.conversation_identity.clone(),
        );
        if result.target_kind != "group" {
            crate::messages::normalize_direct_send_result_for_peer_scope(
                &mut result.sdk_result,
                resolved.peer_scope.as_ref(),
                resolved.target_handle.as_deref(),
                Some(result.target_did.as_str()),
            )?;
            apply_conversation_identity(
                &mut result.sdk_result,
                resolved.conversation_identity.clone(),
            );
        }
        persist_attachment_projection_or_fail(
            self.client,
            result.target_kind,
            result.target_did.as_str(),
            resolved.target_handle.as_deref(),
            resolved.peer_scope.as_ref(),
            &result.manifest,
            &result.sdk_result,
        )?;
        Ok(super::AttachmentSendResult::from_upload_result(result))
    }

    pub async fn send_conversation_async(
        &self,
        request: super::SendConversationAttachmentRequest,
    ) -> crate::ImResult<super::AttachmentSendResult> {
        let resolved = self.resolve_conversation_attachment_request(request)?;
        if attachment_security_is_secure(&resolved.request.security) {
            let mut result = attachment_send_result_from_secure_message(
                self.client
                    .messages()
                    .send_secure_attachment_async(
                        message_request_from_resolved_conversation_attachment(&resolved)?,
                    )
                    .await?,
            )?;
            result.message.message.metadata.conversation_identity =
                Some(resolved.conversation_identity);
            return Ok(result);
        }
        let mut result = crate::internal::attachment_runtime::upload::AttachmentUploadRuntime::new(
            self.client,
            crate::internal::auth::session::FileSessionProvider::new(self.client),
            crate::internal::transport::CoreHttpTransport::new(self.client),
        )
        .send_async(
            crate::internal::attachment_runtime::upload::AttachmentSendInput {
                target: resolved.target,
                request: resolved.request,
                resolved_target_did: resolved.target_did.clone(),
                client_message_id: Some(resolved.client_message_id),
                credentials: None,
            },
        )
        .await?;
        apply_conversation_identity(
            &mut result.sdk_result,
            resolved.conversation_identity.clone(),
        );
        if result.target_kind != "group" {
            crate::messages::normalize_direct_send_result_for_peer_scope(
                &mut result.sdk_result,
                resolved.peer_scope.as_ref(),
                resolved.target_handle.as_deref(),
                Some(result.target_did.as_str()),
            )?;
            apply_conversation_identity(
                &mut result.sdk_result,
                resolved.conversation_identity.clone(),
            );
        }
        persist_attachment_projection_or_fail_async(
            self.client,
            result.target_kind,
            result.target_did.as_str(),
            resolved.target_handle.as_deref(),
            resolved.peer_scope.as_ref(),
            &result.manifest,
            &result.sdk_result,
        )
        .await?;
        Ok(super::AttachmentSendResult::from_upload_result(result))
    }

    pub async fn send_async(
        &self,
        target: crate::messages::MessageTarget,
        request: super::AttachmentSendRequest,
    ) -> crate::ImResult<super::AttachmentSendResult> {
        self.send_async_with_optional_client_message_id(target, request, None)
            .await
    }

    pub async fn send_with_client_message_id_async(
        &self,
        target: crate::messages::MessageTarget,
        request: super::AttachmentSendRequest,
        client_message_id: crate::ids::MessageId,
    ) -> crate::ImResult<super::AttachmentSendResult> {
        self.send_async_with_optional_client_message_id(target, request, Some(client_message_id))
            .await
    }

    async fn send_async_with_optional_client_message_id(
        &self,
        target: crate::messages::MessageTarget,
        request: super::AttachmentSendRequest,
        client_message_id: Option<crate::ids::MessageId>,
    ) -> crate::ImResult<super::AttachmentSendResult> {
        if attachment_security_is_secure(&request.security) {
            let result = self
                .client
                .messages()
                .send_secure_attachment_async(message_request_from_attachment(
                    target,
                    request,
                    client_message_id,
                )?)
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
                client_message_id,
                credentials: None,
            },
        )
        .await?;
        if result.target_kind != "group" {
            crate::messages::normalize_direct_send_result_for_peer_scope(
                &mut result.sdk_result,
                resolved_target.peer_scope.as_ref(),
                resolved_target.direct_handle.as_deref(),
                Some(result.target_did.as_str()),
            )?;
        }
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
            match projection {
                Ok(()) => self
                    .client
                    .emit_committed_local_message_projection("local_send"),
                Err(err) => {
                    result
                        .sdk_result
                        .warnings
                        .push(format!("Failed to persist local attachment message: {err}"));
                }
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

    fn resolve_conversation_attachment_request(
        &self,
        request: super::SendConversationAttachmentRequest,
    ) -> crate::ImResult<ResolvedConversationAttachmentRequest> {
        let (conversation, request_client_message_id, mut attachment) =
            request.into_attachment_send_request();
        let client_message_id = conversation_client_message_id(request_client_message_id)?;
        attachment.delivery.idempotency_key = attachment
            .delivery
            .idempotency_key
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
            .or_else(|| Some(format!("op-{}", client_message_id.as_str())));
        let resolved =
            crate::messages::resolve_conversation_send_target(self.client, &conversation)?;
        Ok(ResolvedConversationAttachmentRequest {
            target: resolved.target,
            request: attachment,
            client_message_id,
            target_did: resolved.target_did,
            target_handle: resolved.target_handle,
            peer_scope: resolved.peer_scope,
            conversation_identity: crate::messages::ConversationIdentity::from_thread_ref_for_owner(
                &conversation.as_thread_ref()?,
                self.client.did().as_str(),
            ),
        })
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
    client_message_id: Option<crate::ids::MessageId>,
) -> crate::ImResult<crate::messages::SendMessageRequest> {
    Ok(crate::messages::SendMessageRequest {
        target,
        body: crate::messages::MessageBody::Attachment {
            input: request.input,
            caption: request.caption,
            mention_payload: request.mention_payload,
            mime_type: request.mime_type,
            filename: request.filename,
        },
        security: request.security,
        client_message_id,
        delivery: request.delivery,
        delegated_signing: None,
    })
}

fn message_request_from_resolved_conversation_attachment(
    resolved: &ResolvedConversationAttachmentRequest,
) -> crate::ImResult<crate::messages::SendMessageRequest> {
    Ok(crate::messages::SendMessageRequest {
        target: resolved.target.clone(),
        body: crate::messages::MessageBody::Attachment {
            input: resolved.request.input.clone(),
            caption: resolved.request.caption.clone(),
            mention_payload: resolved.request.mention_payload.clone(),
            mime_type: resolved.request.mime_type.clone(),
            filename: resolved.request.filename.clone(),
        },
        security: resolved.request.security.clone(),
        client_message_id: Some(resolved.client_message_id.clone()),
        delivery: resolved.request.delivery.clone(),
        delegated_signing: None,
    })
}

fn conversation_client_message_id(
    client_message_id: Option<crate::ids::MessageId>,
) -> crate::ImResult<crate::ids::MessageId> {
    match client_message_id {
        Some(id) => Ok(id),
        None => crate::ids::MessageId::parse(format!(
            "msg-{}",
            crate::internal::wire::common::generate_operation_id()
        )),
    }
}

fn apply_conversation_identity(
    result: &mut crate::messages::SendMessageResult,
    identity: crate::messages::ConversationIdentity,
) {
    result.message.metadata.conversation_identity = Some(identity);
}

fn persist_attachment_projection_or_fail(
    client: &crate::core::ImClient,
    target_kind: &str,
    target_did: &str,
    target_handle: Option<&str>,
    peer_scope: Option<&crate::internal::local_state::owner_scope::DirectPeerScope>,
    manifest: &serde_json::Value,
    sdk_result: &crate::messages::SendMessageResult,
) -> crate::ImResult<()> {
    #[cfg(feature = "sqlite")]
    {
        let projection = if target_kind == "group" {
            crate::internal::message_runtime::local_projection::persist_group_attachment_outgoing(
                client, target_did, manifest, sdk_result,
            )
        } else {
            crate::internal::message_runtime::local_projection::persist_direct_attachment_outgoing(
                client,
                target_did,
                target_handle,
                peer_scope,
                manifest,
                sdk_result,
            )
        };
        projection?;
        client.emit_committed_local_message_projection("local_send");
    }
    #[cfg(not(feature = "sqlite"))]
    {
        let _ = (
            client,
            target_kind,
            target_did,
            target_handle,
            peer_scope,
            manifest,
            sdk_result,
        );
    }
    Ok(())
}

async fn persist_attachment_projection_or_fail_async(
    client: &crate::core::ImClient,
    target_kind: &str,
    target_did: &str,
    target_handle: Option<&str>,
    peer_scope: Option<&crate::internal::local_state::owner_scope::DirectPeerScope>,
    manifest: &serde_json::Value,
    sdk_result: &crate::messages::SendMessageResult,
) -> crate::ImResult<()> {
    #[cfg(feature = "sqlite")]
    {
        let projection = if target_kind == "group" {
            crate::internal::message_runtime::local_projection::persist_group_attachment_outgoing_async(
                client,
                target_did,
                manifest,
                sdk_result,
            )
            .await
        } else {
            crate::internal::message_runtime::local_projection::persist_direct_attachment_outgoing_async(
                client,
                target_did,
                target_handle,
                peer_scope,
                manifest,
                sdk_result,
            )
            .await
        };
        projection?;
        client.emit_committed_local_message_projection("local_send");
    }
    #[cfg(not(feature = "sqlite"))]
    {
        let _ = (
            client,
            target_kind,
            target_did,
            target_handle,
            peer_scope,
            manifest,
            sdk_result,
        );
    }
    Ok(())
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
    crate::internal::handle_discovery::resolve_direct_handle(client, raw)
        .map(|lookup| Some(lookup.target_did))
}

async fn resolve_peer_current_did_async(
    client: &crate::core::ImClient,
    peer: &crate::ids::PeerRef,
) -> crate::ImResult<Option<String>> {
    let raw = peer.as_str().trim();
    if raw.is_empty() || raw.starts_with("did:") {
        return Ok(None);
    }
    crate::internal::handle_discovery::resolve_direct_handle_async(client, raw)
        .await
        .map(|lookup| Some(lookup.target_did))
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
    let lookup = crate::internal::handle_discovery::resolve_direct_handle(client, raw)?;
    resolved_direct_target_from_handle(lookup)
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
    let lookup =
        crate::internal::handle_discovery::resolve_direct_handle_async(client, raw).await?;
    resolved_direct_target_from_handle(lookup)
}

fn resolved_direct_target_from_handle(
    lookup: crate::internal::handle_discovery::DirectHandleResolution,
) -> crate::ImResult<ResolvedDirectTarget> {
    let peer_scope = lookup.peer_scope()?;
    Ok(ResolvedDirectTarget {
        target_did: Some(lookup.target_did),
        direct_handle: Some(lookup.full_handle),
        peer_scope: Some(peer_scope),
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn target_attachment_retry_preserves_core_message_identity() {
        let target = crate::messages::MessageTarget::Direct(
            crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
        );
        let message_id = crate::ids::MessageId::parse("msg-attachment-retry").unwrap();
        let request = || crate::attachments::AttachmentSendRequest {
            input: crate::attachments::AttachmentInput::Bytes {
                filename: Some("retry.txt".to_owned()),
                mime_type: Some("text/plain".to_owned()),
                bytes: b"same logical attachment".to_vec(),
            },
            caption: Some("retry".to_owned()),
            mention_payload: None,
            mime_type: None,
            filename: None,
            delivery: crate::messages::MessageDeliveryOptions {
                idempotency_key: Some("op-msg-attachment-retry".to_owned()),
                wait_for_final_acceptance: false,
            },
            security: crate::messages::MessageSecurityMode::E2eeRequired,
        };

        let first = super::message_request_from_attachment(
            target.clone(),
            request(),
            Some(message_id.clone()),
        )
        .unwrap();
        let retry =
            super::message_request_from_attachment(target, request(), Some(message_id)).unwrap();

        assert_eq!(retry, first);
        assert_eq!(
            first
                .client_message_id
                .as_ref()
                .map(crate::ids::MessageId::as_str),
            Some("msg-attachment-retry")
        );
        assert_eq!(
            first.delivery.idempotency_key.as_deref(),
            Some("op-msg-attachment-retry")
        );
    }

    #[test]
    fn attachment_target_preserves_cross_domain_handle_subject() {
        let resolved = super::resolved_direct_target_from_handle(
            crate::internal::handle_discovery::DirectHandleResolution {
                target_did: "did:wba:remote.example:user:peer:e1".to_owned(),
                full_handle: "peer.remote.example".to_owned(),
                authority_subject_id: "peer.remote.example".to_owned(),
            },
        )
        .unwrap();

        assert_eq!(resolved.peer_scope.unwrap().user_id, "peer.remote.example");
    }
}
