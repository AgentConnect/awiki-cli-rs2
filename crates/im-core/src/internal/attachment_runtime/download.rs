use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;

use crate::internal::auth::session::{AsyncSessionProvider, SessionProvider};
use crate::internal::transport::{
    AsyncAttachmentObjectTransport, AsyncAuthenticatedRpcTransport, AsyncRawJsonTransport,
    AttachmentObjectTransport, AuthenticatedRpcTransport, RawJsonTransport,
};

const MESSAGE_RPC_ENDPOINT: &str = crate::internal::message_runtime::read::MESSAGE_RPC_ENDPOINT;
const ATTACHMENT_DOWNLOAD_LOOKUP_PAGE_SIZE: i64 = 100;

pub(crate) struct AttachmentDownloadRuntime<'a, P, T> {
    client: &'a crate::core::ImClient,
    session_provider: P,
    transport: T,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AttachmentDownloadInput {
    pub request: crate::attachments::DownloadAttachmentRequest,
    pub resolved_peer_did: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AttachmentDownloadResult {
    pub sdk_result: crate::attachments::DownloadedAttachment,
    pub selection: crate::attachments::selection::AttachmentSelection,
    pub ticket: crate::internal::wire::attachment::AttachmentDownloadTicketResult,
}

impl<'a, P, T> AttachmentDownloadRuntime<'a, P, T>
where
    P: SessionProvider,
    T: AuthenticatedRpcTransport + RawJsonTransport + AttachmentObjectTransport,
{
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

    pub(crate) fn download(
        mut self,
        input: AttachmentDownloadInput,
    ) -> crate::ImResult<AttachmentDownloadResult> {
        let sink = crate::internal::blob::sink::attachment_destination_to_sink(
            input.request.destination,
            input.request.overwrite,
        )?;
        if let crate::internal::blob::sink::AttachmentSink::LocalFile { path, overwrite } = &sink {
            crate::internal::attachment_runtime::atomic_write::validate_destination(
                path, *overwrite,
            )?;
        }
        let target = download_target(&input.request.thread, input.resolved_peer_did)?;
        self.session_provider.ensure_session(auth_scope(&target))?;
        let selection = self.find_selection(
            &target,
            input.request.message_id.as_str(),
            input.request.attachment_id.as_deref().unwrap_or_default(),
        )?;
        if selection.public.sender_did.trim().is_empty() {
            return Err(crate::ImError::invalid_input(
                Some("sender_did".to_string()),
                "attachment message sender_did is required",
            ));
        }
        let attachment_service = self.resolve_attachment_service(&selection.public.sender_did)?;
        let ticket = self.get_download_ticket(&target, &selection.public, &attachment_service)?;
        let object = self
            .transport
            .get_attachment_object(&selection.public.object_uri, &ticket.download_ticket_b64u)?;
        let object_content_type = object.content_type.clone();
        let plaintext = verified_download_body(&selection, object.body)?;
        let filename =
            Some(selection.public.filename.clone()).filter(|value| !value.trim().is_empty());
        let mime_type = Some(selection.public.mime_type.clone())
            .filter(|value| !value.trim().is_empty())
            .or(object_content_type);
        let size_bytes = output_size_bytes(&selection, plaintext.len());
        let destination = match sink {
            crate::internal::blob::sink::AttachmentSink::Memory => {
                crate::attachments::DownloadedAttachmentDestination::Memory(plaintext)
            }
            crate::internal::blob::sink::AttachmentSink::LocalFile { path, overwrite } => {
                let path = crate::internal::attachment_runtime::atomic_write::write_bytes_atomic(
                    &path, &plaintext, overwrite,
                )?;
                crate::attachments::DownloadedAttachmentDestination::LocalFile(path)
            }
        };
        let public_selection = public_selection_for_download(&target, &selection);
        let sdk_result = crate::attachments::DownloadedAttachment {
            attachment_id: selection.public.attachment_id.clone(),
            filename,
            mime_type,
            size_bytes,
            destination,
            selection: Some(public_selection.clone()),
            warnings: Vec::new(),
        };
        Ok(AttachmentDownloadResult {
            sdk_result,
            selection: public_selection,
            ticket,
        })
    }

    fn find_selection(
        &mut self,
        target: &DownloadTarget,
        requested_message_id: &str,
        requested_attachment_id: &str,
    ) -> crate::ImResult<crate::attachments::selection::InternalAttachmentSelection> {
        if let Some(selection) =
            find_cached_attachment_selection(self.client, target, requested_message_id)?
        {
            if let Ok(selection) = crate::attachments::selection::find_internal_attachment_selection(
                &[selection],
                requested_message_id,
                requested_attachment_id,
            ) {
                return Ok(selection);
            }
        }
        let mut skip = 0_i64;
        loop {
            let (messages, has_more, consumed_count) = self.fetch_page(target, skip)?;
            match crate::attachments::selection::find_internal_attachment_selection(
                &messages,
                requested_message_id,
                requested_attachment_id,
            ) {
                Ok(selection) => return Ok(selection),
                Err(crate::ImError::MessageNotFound { .. }) if has_more && consumed_count > 0 => {
                    skip += consumed_count;
                }
                Err(crate::ImError::MessageNotFound { message_id }) => {
                    return Err(crate::ImError::MessageNotFound { message_id });
                }
                Err(err) => return Err(err),
            }
        }
    }

    fn fetch_page(
        &mut self,
        target: &DownloadTarget,
        skip: i64,
    ) -> crate::ImResult<(Vec<Value>, bool, i64)> {
        match target {
            DownloadTarget::Direct { peer_did } => {
                let params = crate::internal::wire::history::build_history_rpc_params(
                    &crate::internal::wire::common::WireIdentity {
                        did: self.client.did().as_str().to_string(),
                    },
                    crate::internal::wire::history::HistoryWireRequest {
                        peer_did: peer_did.clone(),
                        limit: ATTACHMENT_DOWNLOAD_LOOKUP_PAGE_SIZE,
                        cursor: None,
                        skip,
                        auth: None,
                    },
                )?;
                let mut raw = self.transport.authenticated_rpc(
                    MESSAGE_RPC_ENDPOINT,
                    "direct.get_history",
                    params,
                )?;
                let consumed_count = values_from_array(raw.get("messages")).len() as i64;
                project_secure_direct_messages_for_download(self.client, &mut raw, peer_did)?;
                Ok((
                    values_from_array(raw.get("messages")),
                    bool_from_value(raw.get("has_more")),
                    consumed_count,
                ))
            }
            DownloadTarget::Group { group } => {
                let params = crate::internal::wire::group::build_group_messages_rpc_params(
                    self.client.did().as_str(),
                    group.as_str(),
                    ATTACHMENT_DOWNLOAD_LOOKUP_PAGE_SIZE,
                    None,
                    skip,
                )?;
                let mut raw = self.transport.authenticated_rpc(
                    MESSAGE_RPC_ENDPOINT,
                    "group.list_messages",
                    params,
                )?;
                let consumed_count = values_from_array(raw.get("messages")).len() as i64;
                project_group_e2ee_messages_for_download(self.client, &mut raw);
                Ok((
                    values_from_array(raw.get("messages")),
                    bool_from_value(raw.get("has_more")),
                    consumed_count,
                ))
            }
        }
    }

    fn resolve_attachment_service(
        &mut self,
        sender_did: &str,
    ) -> crate::ImResult<crate::internal::discovery::attachment::DiscoveredAttachmentService> {
        let document = match crate::internal::discovery::did_document::resolve_did_document(
            &mut self.transport,
            sender_did,
        ) {
            Ok(document) => document,
            Err(remote_error) => {
                local_identity_document(self.client, sender_did)?.ok_or(remote_error)?
            }
        };
        crate::internal::discovery::attachment::select_attachment_rpc_service_from_document(
            sender_did, &document,
        )
    }

    fn get_download_ticket(
        &mut self,
        target: &DownloadTarget,
        selection: &crate::attachments::selection::AttachmentSelection,
        attachment_service: &crate::internal::discovery::attachment::DiscoveredAttachmentService,
    ) -> crate::ImResult<crate::internal::wire::attachment::AttachmentDownloadTicketResult> {
        let group_did = match target {
            DownloadTarget::Direct { .. } => "",
            DownloadTarget::Group { group } => group.as_str(),
        };
        let params =
            crate::internal::wire::attachment::build_attachment_download_ticket_rpc_params(
                self.client.did().as_str(),
                &attachment_service.service_did,
                &selection.sender_did,
                &selection.message_id,
                group_did,
                selection,
            )?;
        let raw = self.transport.authenticated_rpc(
            attachment_service.rpc_endpoint.as_str(),
            "attachment.get_download_ticket",
            params,
        )?;
        serde_json::from_value(raw).map_err(|err| crate::ImError::Serialization {
            detail: err.to_string(),
        })
    }
}

impl<'a, P, T> AttachmentDownloadRuntime<'a, P, T>
where
    P: AsyncSessionProvider,
    T: AsyncAuthenticatedRpcTransport + AsyncRawJsonTransport + AsyncAttachmentObjectTransport,
{
    pub(crate) async fn download_async(
        mut self,
        input: AttachmentDownloadInput,
    ) -> crate::ImResult<AttachmentDownloadResult> {
        let sink = crate::internal::blob::sink::attachment_destination_to_sink(
            input.request.destination,
            input.request.overwrite,
        )?;
        if let crate::internal::blob::sink::AttachmentSink::LocalFile { path, overwrite } = &sink {
            crate::internal::attachment_runtime::atomic_write::validate_destination(
                path, *overwrite,
            )?;
        }
        let target = download_target(&input.request.thread, input.resolved_peer_did)?;
        self.session_provider
            .ensure_session(auth_scope(&target))
            .await?;
        let selection = self
            .find_selection_async(
                &target,
                input.request.message_id.as_str(),
                input.request.attachment_id.as_deref().unwrap_or_default(),
            )
            .await?;
        if selection.public.sender_did.trim().is_empty() {
            return Err(crate::ImError::invalid_input(
                Some("sender_did".to_string()),
                "attachment message sender_did is required",
            ));
        }
        let attachment_service = self
            .resolve_attachment_service_async(&selection.public.sender_did)
            .await?;
        let ticket = self
            .get_download_ticket_async(&target, &selection.public, &attachment_service)
            .await?;
        let object = self
            .transport
            .get_attachment_object_stream(
                &selection.public.object_uri,
                &ticket.download_ticket_b64u,
            )
            .await?;
        let object_content_type = object.content_type().map(ToOwned::to_owned);
        let downloaded = object.into_bytes().await?;
        let plaintext = verified_download_body(&selection, downloaded)?;
        let filename =
            Some(selection.public.filename.clone()).filter(|value| !value.trim().is_empty());
        let mime_type = Some(selection.public.mime_type.clone())
            .filter(|value| !value.trim().is_empty())
            .or(object_content_type);
        let size_bytes = output_size_bytes(&selection, plaintext.len());
        let destination = match sink {
            crate::internal::blob::sink::AttachmentSink::Memory => {
                crate::attachments::DownloadedAttachmentDestination::Memory(plaintext)
            }
            crate::internal::blob::sink::AttachmentSink::LocalFile { path, overwrite } => {
                let path = crate::internal::attachment_runtime::atomic_write::write_stream_atomic(
                    &path,
                    crate::internal::transport::AsyncAttachmentObjectResponse::Bytes {
                        body: plaintext,
                        content_type: None,
                        consumed: false,
                    },
                    overwrite,
                )
                .await?;
                crate::attachments::DownloadedAttachmentDestination::LocalFile(path)
            }
        };
        let public_selection = public_selection_for_download(&target, &selection);
        let sdk_result = crate::attachments::DownloadedAttachment {
            attachment_id: selection.public.attachment_id.clone(),
            filename,
            mime_type,
            size_bytes,
            destination,
            selection: Some(public_selection.clone()),
            warnings: Vec::new(),
        };
        Ok(AttachmentDownloadResult {
            sdk_result,
            selection: public_selection,
            ticket,
        })
    }

    async fn find_selection_async(
        &mut self,
        target: &DownloadTarget,
        requested_message_id: &str,
        requested_attachment_id: &str,
    ) -> crate::ImResult<crate::attachments::selection::InternalAttachmentSelection> {
        if let Some(selection) =
            find_cached_attachment_selection(self.client, target, requested_message_id)?
        {
            if let Ok(selection) = crate::attachments::selection::find_internal_attachment_selection(
                &[selection],
                requested_message_id,
                requested_attachment_id,
            ) {
                return Ok(selection);
            }
        }
        let mut skip = 0_i64;
        loop {
            let (messages, has_more, consumed_count) = self.fetch_page_async(target, skip).await?;
            match crate::attachments::selection::find_internal_attachment_selection(
                &messages,
                requested_message_id,
                requested_attachment_id,
            ) {
                Ok(selection) => return Ok(selection),
                Err(crate::ImError::MessageNotFound { .. }) if has_more && consumed_count > 0 => {
                    skip += consumed_count;
                }
                Err(crate::ImError::MessageNotFound { message_id }) => {
                    return Err(crate::ImError::MessageNotFound { message_id });
                }
                Err(err) => return Err(err),
            }
        }
    }

    async fn fetch_page_async(
        &mut self,
        target: &DownloadTarget,
        skip: i64,
    ) -> crate::ImResult<(Vec<Value>, bool, i64)> {
        match target {
            DownloadTarget::Direct { peer_did } => {
                let params = crate::internal::wire::history::build_history_rpc_params(
                    &crate::internal::wire::common::WireIdentity {
                        did: self.client.did().as_str().to_string(),
                    },
                    crate::internal::wire::history::HistoryWireRequest {
                        peer_did: peer_did.clone(),
                        limit: ATTACHMENT_DOWNLOAD_LOOKUP_PAGE_SIZE,
                        cursor: None,
                        skip,
                        auth: None,
                    },
                )?;
                let mut raw = self
                    .transport
                    .authenticated_rpc(MESSAGE_RPC_ENDPOINT, "direct.get_history", params)
                    .await?;
                let consumed_count = values_from_array(raw.get("messages")).len() as i64;
                project_secure_direct_messages_for_download_async(self.client, &mut raw, peer_did)
                    .await?;
                Ok((
                    values_from_array(raw.get("messages")),
                    bool_from_value(raw.get("has_more")),
                    consumed_count,
                ))
            }
            DownloadTarget::Group { group } => {
                let params = crate::internal::wire::group::build_group_messages_rpc_params(
                    self.client.did().as_str(),
                    group.as_str(),
                    ATTACHMENT_DOWNLOAD_LOOKUP_PAGE_SIZE,
                    None,
                    skip,
                )?;
                let mut raw = self
                    .transport
                    .authenticated_rpc(MESSAGE_RPC_ENDPOINT, "group.list_messages", params)
                    .await?;
                let consumed_count = values_from_array(raw.get("messages")).len() as i64;
                project_group_e2ee_messages_for_download_async(self.client, &mut raw).await;
                Ok((
                    values_from_array(raw.get("messages")),
                    bool_from_value(raw.get("has_more")),
                    consumed_count,
                ))
            }
        }
    }

    async fn resolve_attachment_service_async(
        &mut self,
        sender_did: &str,
    ) -> crate::ImResult<crate::internal::discovery::attachment::DiscoveredAttachmentService> {
        let document = match crate::internal::discovery::did_document::resolve_did_document_async(
            &mut self.transport,
            sender_did,
        )
        .await
        {
            Ok(document) => document,
            Err(remote_error) => local_identity_document_async(self.client, sender_did)
                .await?
                .ok_or(remote_error)?,
        };
        crate::internal::discovery::attachment::select_attachment_rpc_service_from_document(
            sender_did, &document,
        )
    }

    async fn get_download_ticket_async(
        &mut self,
        target: &DownloadTarget,
        selection: &crate::attachments::selection::AttachmentSelection,
        attachment_service: &crate::internal::discovery::attachment::DiscoveredAttachmentService,
    ) -> crate::ImResult<crate::internal::wire::attachment::AttachmentDownloadTicketResult> {
        let group_did = match target {
            DownloadTarget::Direct { .. } => "",
            DownloadTarget::Group { group } => group.as_str(),
        };
        let params =
            crate::internal::wire::attachment::build_attachment_download_ticket_rpc_params(
                self.client.did().as_str(),
                &attachment_service.service_did,
                &selection.sender_did,
                &selection.message_id,
                group_did,
                selection,
            )?;
        let raw = self
            .transport
            .authenticated_rpc(
                attachment_service.rpc_endpoint.as_str(),
                "attachment.get_download_ticket",
                params,
            )
            .await?;
        serde_json::from_value(raw).map_err(|err| crate::ImError::Serialization {
            detail: err.to_string(),
        })
    }
}

fn find_cached_attachment_selection(
    client: &crate::core::ImClient,
    target: &DownloadTarget,
    requested_message_id: &str,
) -> crate::ImResult<Option<Value>> {
    #[cfg(feature = "sqlite")]
    {
        let (thread_kind, thread_id) = match target {
            DownloadTarget::Direct { peer_did } => ("direct", peer_did.as_str()),
            DownloadTarget::Group { group } => ("group", group.as_str()),
        };
        let connection = crate::internal::local_state::open_writable(
            &client.core_inner().sdk_paths().local_state.sqlite_path,
        )?;
        crate::internal::local_state::attachment_manifest_cache::get_attachment_manifest_cache_message(
            &connection,
            client.current_identity().id.as_str(),
            thread_kind,
            thread_id,
            requested_message_id,
        )
    }
    #[cfg(not(feature = "sqlite"))]
    {
        let _ = (client, target, requested_message_id);
        Ok(None)
    }
}

fn verified_download_body(
    selection: &crate::attachments::selection::InternalAttachmentSelection,
    downloaded: Vec<u8>,
) -> crate::ImResult<Vec<u8>> {
    verify_downloaded_ciphertext(selection, &downloaded)?;
    if !selection.is_object_e2ee() {
        return Ok(downloaded);
    }
    let object_key_b64u = selection
        .object_key_b64u
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            crate::ImError::invalid_input(
                Some("object_key_b64u".to_string()),
                "object_key_b64u is required for object-e2ee attachment download",
            )
        })?;
    let nonce_b64u = selection
        .nonce_b64u
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            crate::ImError::invalid_input(
                Some("nonce_b64u".to_string()),
                "nonce_b64u is required for object-e2ee attachment download",
            )
        })?;
    if selection
        .public
        .object_cipher
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_some_and(|cipher| {
            cipher != crate::internal::attachment_runtime::object_crypto::OBJECT_E2EE_CIPHER
        })
    {
        return Err(crate::ImError::invalid_input(
            Some("object_cipher".to_string()),
            "unsupported object-e2ee cipher",
        ));
    }
    let plaintext = crate::internal::attachment_runtime::object_crypto::decrypt_object_e2ee(
        &downloaded,
        object_key_b64u,
        nonce_b64u,
    )?;
    verify_plaintext_size(selection, plaintext.len())?;
    Ok(plaintext)
}

fn verify_downloaded_ciphertext(
    selection: &crate::attachments::selection::InternalAttachmentSelection,
    downloaded: &[u8],
) -> crate::ImResult<()> {
    if let Some(expected_size) = parse_optional_u64(&selection.public.size, "size")? {
        if downloaded.len() as u64 != expected_size {
            return Err(attachment_service_error(
                "anp.attachment.digest_mismatch",
                format!(
                    "attachment object size mismatch: expected {expected_size}, got {}",
                    downloaded.len()
                ),
            ));
        }
    }
    let expected_digest = selection.public.digest_b64u.trim();
    if !expected_digest.is_empty() {
        let actual = crate::internal::attachment_runtime::digest::sha256_digest_b64u(downloaded);
        if actual != expected_digest {
            return Err(attachment_service_error(
                "anp.attachment.digest_mismatch",
                "attachment object digest mismatch",
            ));
        }
    }
    Ok(())
}

fn verify_plaintext_size(
    selection: &crate::attachments::selection::InternalAttachmentSelection,
    plaintext_len: usize,
) -> crate::ImResult<()> {
    let Some(expected_size) = parse_optional_u64(
        selection
            .public
            .plaintext_size
            .as_deref()
            .unwrap_or_default(),
        "plaintext_size",
    )?
    else {
        return Err(crate::ImError::invalid_input(
            Some("plaintext_size".to_string()),
            "plaintext_size is required for object-e2ee attachment download",
        ));
    };
    if plaintext_len as u64 != expected_size {
        return Err(attachment_service_error(
            "anp.attachment.decrypt_failed",
            format!(
                "attachment plaintext size mismatch: expected {expected_size}, got {plaintext_len}"
            ),
        ));
    }
    Ok(())
}

fn output_size_bytes(
    selection: &crate::attachments::selection::InternalAttachmentSelection,
    plaintext_len: usize,
) -> Option<u64> {
    if selection.is_object_e2ee() {
        selection
            .public
            .plaintext_size
            .as_deref()
            .and_then(|value| value.trim().parse().ok())
            .or(Some(plaintext_len as u64))
    } else {
        selection
            .public
            .size
            .trim()
            .parse()
            .ok()
            .or(Some(plaintext_len as u64))
    }
}

fn public_selection_for_download(
    target: &DownloadTarget,
    selection: &crate::attachments::selection::InternalAttachmentSelection,
) -> crate::attachments::selection::AttachmentSelection {
    let mut public = selection.redacted_public();
    if public.message_security_profile.trim().is_empty() {
        public.message_security_profile = effective_message_security_profile(target, selection);
    }
    public
}

fn effective_message_security_profile(
    target: &DownloadTarget,
    selection: &crate::attachments::selection::InternalAttachmentSelection,
) -> String {
    let explicit = selection.message_security_profile();
    if !explicit.is_empty() {
        return explicit.to_owned();
    }
    if selection.is_object_e2ee() {
        match target {
            DownloadTarget::Direct { .. } => "direct-e2ee".to_owned(),
            DownloadTarget::Group { .. } => "group-e2ee".to_owned(),
        }
    } else {
        "transport-protected".to_owned()
    }
}

fn parse_optional_u64(value: &str, field: &str) -> crate::ImResult<Option<u64>> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(None);
    }
    value.parse::<u64>().map(Some).map_err(|_| {
        crate::ImError::invalid_input(Some(field.to_string()), format!("{field} must be a u64"))
    })
}

fn attachment_service_error(code: &str, message: impl Into<String>) -> crate::ImError {
    crate::ImError::Service {
        status_code: None,
        code: Some(code.to_string()),
        message: message.into(),
        data: None,
    }
}

fn project_secure_direct_messages_for_download(
    client: &crate::core::ImClient,
    raw: &mut Value,
    expected_peer_did: &str,
) -> crate::ImResult<()> {
    #[cfg(all(feature = "sqlite", feature = "blocking"))]
    let mut provenance = {
        let mut directory_transport = crate::internal::transport::CoreHttpTransport::new(client);
        crate::internal::message_runtime::read::project_secure_direct_messages_for_attachment_download(
                client,
                raw,
                &mut directory_transport,
                expected_peer_did,
            )
    };
    #[cfg(not(all(feature = "sqlite", feature = "blocking")))]
    let mut provenance =
        crate::internal::message_runtime::read::DirectP5ProjectionProvenance::default();
    crate::internal::message_runtime::read::retain_direct_messages_for_expected_peer(
        client,
        raw,
        expected_peer_did,
        &mut provenance,
    );
    let projectable_count = values_from_array(raw.get("messages")).len();
    crate::internal::message_runtime::read::reject_stalled_scoped_direct_page(
        raw,
        projectable_count,
    )?;
    Ok(())
}

async fn project_secure_direct_messages_for_download_async(
    client: &crate::core::ImClient,
    raw: &mut Value,
    expected_peer_did: &str,
) -> crate::ImResult<()> {
    #[cfg(feature = "sqlite")]
    let mut provenance = {
        let mut directory_transport = crate::internal::transport::CoreHttpTransport::new(client);
        crate::internal::message_runtime::read::project_secure_direct_messages_for_attachment_download_async(
                client,
                raw,
                &mut directory_transport,
                expected_peer_did,
            )
            .await
    };
    #[cfg(not(feature = "sqlite"))]
    let mut provenance =
        crate::internal::message_runtime::read::DirectP5ProjectionProvenance::default();
    crate::internal::message_runtime::read::retain_direct_messages_for_expected_peer(
        client,
        raw,
        expected_peer_did,
        &mut provenance,
    );
    let projectable_count = values_from_array(raw.get("messages")).len();
    crate::internal::message_runtime::read::reject_stalled_scoped_direct_page(
        raw,
        projectable_count,
    )?;
    Ok(())
}

fn project_group_e2ee_messages_for_download(client: &crate::core::ImClient, raw: &mut Value) {
    crate::internal::message_runtime::read::project_group_e2ee_messages_for_attachment_download(
        client, raw,
    );
}

async fn project_group_e2ee_messages_for_download_async(
    client: &crate::core::ImClient,
    raw: &mut Value,
) {
    crate::internal::message_runtime::read::project_group_e2ee_messages_for_attachment_download_async(
        client, raw,
    )
    .await;
}

fn local_identity_document(
    client: &crate::core::ImClient,
    sender_did: &str,
) -> crate::ImResult<Option<Value>> {
    let sender_did = sender_did.trim();
    if sender_did.is_empty() {
        return Ok(None);
    }
    let paths = &client.core_inner().sdk_paths().identities;
    let identities = local_registry_identities(paths)?;
    for identity in identities {
        if identity.did != sender_did {
            continue;
        }
        let identity_dir = paths.identity_root_dir.join(identity.dir_name);
        let document_path = first_existing_path(&identity_dir, &["did.json", "did_document.json"]);
        let raw = match std::fs::read(&document_path) {
            Ok(raw) => raw,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(err) => {
                return Err(crate::ImError::CredentialFileUnreadable {
                    path_kind: "did_document".to_string(),
                    detail: err.to_string(),
                });
            }
        };
        return serde_json::from_slice(&raw).map(Some).map_err(|err| {
            crate::ImError::Serialization {
                detail: err.to_string(),
            }
        });
    }
    Ok(None)
}

async fn local_identity_document_async(
    client: &crate::core::ImClient,
    sender_did: &str,
) -> crate::ImResult<Option<Value>> {
    let sender_did = sender_did.trim();
    if sender_did.is_empty() {
        return Ok(None);
    }
    let paths = &client.core_inner().sdk_paths().identities;
    let identities = local_registry_identities_async(paths).await?;
    for identity in identities {
        if identity.did != sender_did {
            continue;
        }
        let identity_dir = paths.identity_root_dir.join(identity.dir_name);
        let document_path = first_existing_path(&identity_dir, &["did.json", "did_document.json"]);
        let raw = match tokio::fs::read(&document_path).await {
            Ok(raw) => raw,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(err) => {
                return Err(crate::ImError::CredentialFileUnreadable {
                    path_kind: "did_document".to_string(),
                    detail: err.to_string(),
                });
            }
        };
        return serde_json::from_slice(&raw).map(Some).map_err(|err| {
            crate::ImError::Serialization {
                detail: err.to_string(),
            }
        });
    }
    Ok(None)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LocalRegistryIdentity {
    did: String,
    dir_name: String,
}

#[derive(Debug, Deserialize)]
struct SdkRegistryFile {
    #[serde(default)]
    identities: Vec<SdkIdentityRecord>,
}

#[derive(Debug, Deserialize)]
struct SdkIdentityRecord {
    #[serde(default)]
    id: String,
    #[serde(default)]
    did: String,
    #[serde(default)]
    dir_name: Option<String>,
    #[serde(default)]
    local_alias: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LegacyRegistryFile {
    #[serde(default)]
    credentials: BTreeMap<String, LegacyIdentityRecord>,
}

#[derive(Debug, Deserialize)]
struct LegacyIdentityRecord {
    #[serde(default)]
    credential_name: String,
    #[serde(default)]
    dir_name: String,
    #[serde(default)]
    did: String,
    #[serde(default)]
    unique_id: String,
}

fn local_registry_identities(
    paths: &crate::paths::IdentityRegistryPaths,
) -> crate::ImResult<Vec<LocalRegistryIdentity>> {
    let raw = match std::fs::read(&paths.registry_path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => {
            return Err(crate::ImError::CredentialFileUnreadable {
                path_kind: "identity_registry".to_string(),
                detail: err.to_string(),
            });
        }
    };
    parse_local_registry_identities(&raw)
}

async fn local_registry_identities_async(
    paths: &crate::paths::IdentityRegistryPaths,
) -> crate::ImResult<Vec<LocalRegistryIdentity>> {
    let raw = match tokio::fs::read(&paths.registry_path).await {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => {
            return Err(crate::ImError::CredentialFileUnreadable {
                path_kind: "identity_registry".to_string(),
                detail: err.to_string(),
            });
        }
    };
    parse_local_registry_identities(&raw)
}

fn parse_local_registry_identities(raw: &[u8]) -> crate::ImResult<Vec<LocalRegistryIdentity>> {
    if let Ok(file) = serde_json::from_slice::<SdkRegistryFile>(raw) {
        if !file.identities.is_empty() {
            return Ok(file
                .identities
                .into_iter()
                .filter_map(|record| {
                    let did = record.did.trim().to_string();
                    if did.is_empty() {
                        return None;
                    }
                    let dir_name = first_non_empty([
                        record.dir_name.as_deref(),
                        record.local_alias.as_deref(),
                        Some(record.id.as_str()),
                    ])?
                    .to_string();
                    Some(LocalRegistryIdentity { did, dir_name })
                })
                .collect());
        }
    }
    let file: LegacyRegistryFile =
        serde_json::from_slice(raw).map_err(|err| crate::ImError::Serialization {
            detail: err.to_string(),
        })?;
    Ok(file
        .credentials
        .into_iter()
        .filter_map(|(alias, record)| {
            let did = record.did.trim().to_string();
            if did.is_empty() {
                return None;
            }
            let dir_name = first_non_empty([
                Some(record.dir_name.as_str()),
                Some(record.unique_id.as_str()),
                Some(record.credential_name.as_str()),
                Some(alias.as_str()),
            ])?
            .to_string();
            Some(LocalRegistryIdentity { did, dir_name })
        })
        .collect())
}

fn first_non_empty<'a>(values: impl IntoIterator<Item = Option<&'a str>>) -> Option<&'a str> {
    values
        .into_iter()
        .flatten()
        .find(|value| !value.trim().is_empty())
}

fn first_existing_path(identity_dir: &Path, names: &[&str]) -> std::path::PathBuf {
    names
        .iter()
        .map(|name| identity_dir.join(name))
        .find(|path| path.exists())
        .unwrap_or_else(|| identity_dir.join(names[0]))
}

fn download_target(
    thread: &crate::messages::ThreadRef,
    resolved_peer_did: Option<String>,
) -> crate::ImResult<DownloadTarget> {
    match thread {
        crate::messages::ThreadRef::Direct(peer) => {
            let resolved = resolved_peer_did
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| peer.as_str().trim());
            if !resolved.starts_with("did:") {
                return Err(crate::ImError::PeerNotFound {
                    peer: peer.as_str().to_string(),
                });
            }
            Ok(DownloadTarget::Direct {
                peer_did: resolved.to_string(),
            })
        }
        crate::messages::ThreadRef::Group(group) => Ok(DownloadTarget::Group {
            group: group.clone(),
        }),
        crate::messages::ThreadRef::Thread(_) => {
            Err(crate::ImError::unsupported("thread-attachment-download"))
        }
    }
}

fn auth_scope(target: &DownloadTarget) -> crate::auth::AuthScope {
    match target {
        DownloadTarget::Direct { .. } => crate::auth::AuthScope::Messaging,
        DownloadTarget::Group { .. } => crate::auth::AuthScope::GroupMessaging,
    }
}

fn values_from_array(value: Option<&Value>) -> Vec<Value> {
    value.and_then(Value::as_array).cloned().unwrap_or_default()
}

fn bool_from_value(value: Option<&Value>) -> bool {
    matches!(value, Some(Value::Bool(true)))
}

#[derive(Debug, Clone, PartialEq)]
enum DownloadTarget {
    Direct { peer_did: String },
    Group { group: crate::ids::GroupRef },
}

#[cfg(test)]
mod tests;
