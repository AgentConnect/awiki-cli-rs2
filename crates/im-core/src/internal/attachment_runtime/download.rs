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
            find_cached_group_attachment_selection(self.client, target, requested_message_id)?
        {
            if let Ok(selection) = crate::attachments::selection::find_internal_attachment_selection(
                &[selection],
                requested_message_id,
                requested_attachment_id,
            ) {
                return Ok(selection);
            }
        }
        crate::attachments::selection::find_internal_attachment_selection_with_paging(
            |skip| self.fetch_page(target, skip),
            requested_message_id,
            requested_attachment_id,
        )
    }

    fn fetch_page(
        &mut self,
        target: &DownloadTarget,
        skip: i64,
    ) -> crate::ImResult<(Vec<Value>, bool)> {
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
                project_secure_direct_messages_for_download(self.client, &mut raw);
                Ok((
                    values_from_array(raw.get("messages")),
                    bool_from_value(raw.get("has_more")),
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
                project_group_e2ee_messages_for_download(self.client, &mut raw);
                Ok((
                    values_from_array(raw.get("messages")),
                    bool_from_value(raw.get("has_more")),
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
            find_cached_group_attachment_selection(self.client, target, requested_message_id)?
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
            let (messages, has_more) = self.fetch_page_async(target, skip).await?;
            match crate::attachments::selection::find_internal_attachment_selection(
                &messages,
                requested_message_id,
                requested_attachment_id,
            ) {
                Ok(selection) => return Ok(selection),
                Err(crate::ImError::MessageNotFound { .. }) if has_more && !messages.is_empty() => {
                    skip += messages.len() as i64;
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
    ) -> crate::ImResult<(Vec<Value>, bool)> {
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
                project_secure_direct_messages_for_download_async(self.client, &mut raw).await;
                Ok((
                    values_from_array(raw.get("messages")),
                    bool_from_value(raw.get("has_more")),
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
                project_group_e2ee_messages_for_download_async(self.client, &mut raw).await;
                Ok((
                    values_from_array(raw.get("messages")),
                    bool_from_value(raw.get("has_more")),
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

fn find_cached_group_attachment_selection(
    client: &crate::core::ImClient,
    target: &DownloadTarget,
    requested_message_id: &str,
) -> crate::ImResult<Option<Value>> {
    #[cfg(feature = "sqlite")]
    {
        let DownloadTarget::Group { group } = target else {
            return Ok(None);
        };
        let connection = crate::internal::local_state::open_writable(
            &client.core_inner().sdk_paths().local_state.sqlite_path,
        )?;
        crate::internal::local_state::attachment_manifest_cache::get_attachment_manifest_cache_message(
            &connection,
            client.current_identity().id.as_str(),
            "group",
            group.as_str(),
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
    }
}

fn project_secure_direct_messages_for_download(client: &crate::core::ImClient, raw: &mut Value) {
    #[cfg(all(feature = "sqlite", feature = "blocking"))]
    {
        let mut directory_transport = crate::internal::transport::CoreHttpTransport::new(client);
        crate::internal::message_runtime::read::project_secure_direct_messages_for_attachment_download(
            client,
            raw,
            &mut directory_transport,
        );
    }
    #[cfg(not(all(feature = "sqlite", feature = "blocking")))]
    {
        let _ = (client, raw);
    }
}

async fn project_secure_direct_messages_for_download_async(
    client: &crate::core::ImClient,
    raw: &mut Value,
) {
    #[cfg(feature = "sqlite")]
    {
        let mut directory_transport = crate::internal::transport::CoreHttpTransport::new(client);
        crate::internal::message_runtime::read::project_secure_direct_messages_for_attachment_download_async(
            client,
            raw,
            &mut directory_transport,
        )
        .await;
    }
    #[cfg(not(feature = "sqlite"))]
    {
        let _ = (client, raw);
    }
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
mod tests {
    use super::*;
    use crate::internal::transport::{
        AttachmentObjectResponse, AttachmentObjectTransport, AuthenticatedRpcTransport,
        RawJsonTransport, RpcTransport,
    };
    use serde_json::{json, Value};
    use std::cell::RefCell;
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::PathBuf;
    use std::rc::Rc;

    #[test]
    fn attachments_download_runtime_memory_fetches_ticket_and_bytes() {
        let fixture = Fixture::new();
        let client = fixture.client();
        let calls = Rc::new(RefCell::new(Vec::new()));
        let sessions = Rc::new(RefCell::new(Vec::new()));

        let result = AttachmentDownloadRuntime::new(
            &client,
            ReadySessionProvider {
                scopes: Rc::clone(&sessions),
            },
            RecordingTransport {
                calls: Rc::clone(&calls),
            },
        )
        .download(AttachmentDownloadInput {
            request: crate::attachments::DownloadAttachmentRequest {
                thread: crate::messages::ThreadRef::Direct(
                    crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
                ),
                message_id: crate::ids::MessageId::parse("msg-attachment-1").unwrap(),
                attachment_id: Some("att-1".to_string()),
                destination: crate::attachments::AttachmentDestination::Memory,
                overwrite: false,
            },
            resolved_peer_did: None,
        })
        .unwrap();

        assert_eq!(
            sessions.borrow().as_slice(),
            &[crate::auth::AuthScope::Messaging]
        );
        assert_eq!(result.selection.attachment_id, "att-1");
        assert_eq!(result.ticket.download_ticket_b64u, "ticket-1");
        assert_eq!(result.sdk_result.attachment_id, "att-1");
        assert_eq!(result.sdk_result.filename.as_deref(), Some("report.txt"));
        assert_eq!(result.sdk_result.mime_type.as_deref(), Some("text/plain"));
        assert_eq!(result.sdk_result.size_bytes, Some(16));
        assert!(matches!(
            result.sdk_result.destination,
            crate::attachments::DownloadedAttachmentDestination::Memory(bytes)
                if bytes == b"downloaded bytes".to_vec()
        ));

        let calls = calls.borrow();
        assert_eq!(calls.len(), 4);
        let history = calls[0].rpc("direct.get_history");
        assert_eq!(history.endpoint, MESSAGE_RPC_ENDPOINT);
        assert_eq!(history.params["body"]["peer_did"], "did:example:bob");
        assert_eq!(
            history.params["body"]["limit"],
            ATTACHMENT_DOWNLOAD_LOOKUP_PAGE_SIZE
        );
        let did_doc = calls[1].get_json("https://example.com/bob/did.json");
        assert_eq!(
            did_doc.headers.get("Accept").map(String::as_str),
            Some("application/json")
        );
        let ticket = calls[2].rpc("attachment.get_download_ticket");
        assert_eq!(ticket.endpoint, "https://attachment.example/rpc");
        assert_eq!(
            ticket.params["meta"]["target"],
            json!({"kind": "service", "did": "did:web:attachment.example"})
        );
        assert_eq!(ticket.params["body"]["attachment_id"], "att-1");
        assert_eq!(
            ticket.params["body"]["object_uri"],
            "https://objects.example/att-1"
        );
        assert_eq!(
            ticket.params["body"]["sender_did"],
            "did:web:example.com:bob"
        );
        assert_eq!(ticket.params["body"]["requester_did"], "did:example:alice");
        assert_eq!(
            ticket.params["body"]["message_target_did"],
            "did:example:alice"
        );
        let object = calls[3].object_get("https://objects.example/att-1");
        assert_eq!(object.ticket, "ticket-1");
    }

    #[test]
    fn attachments_download_runtime_pages_until_selection_matches() {
        let fixture = Fixture::new();
        let client = fixture.client();
        let calls = Rc::new(RefCell::new(Vec::new()));

        let result = AttachmentDownloadRuntime::new(
            &client,
            ReadySessionProvider {
                scopes: Rc::new(RefCell::new(Vec::new())),
            },
            RecordingTransport {
                calls: Rc::clone(&calls),
            },
        )
        .download(AttachmentDownloadInput {
            request: crate::attachments::DownloadAttachmentRequest {
                thread: crate::messages::ThreadRef::Direct(
                    crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
                ),
                message_id: crate::ids::MessageId::parse("msg-second-page").unwrap(),
                attachment_id: Some("att-2".to_string()),
                destination: crate::attachments::AttachmentDestination::Memory,
                overwrite: false,
            },
            resolved_peer_did: None,
        })
        .unwrap();

        assert_eq!(result.selection.attachment_id, "att-2");
        let calls = calls.borrow();
        let first = calls[0].rpc("direct.get_history");
        assert_eq!(first.params["body"].get("skip"), None);
        let second = calls[1].rpc("direct.get_history");
        assert_eq!(second.params["body"]["skip"], 1);
    }

    #[test]
    fn attachments_download_runtime_group_uses_group_scope_and_ticket_body() {
        let fixture = Fixture::new();
        let client = fixture.client();
        let calls = Rc::new(RefCell::new(Vec::new()));
        let sessions = Rc::new(RefCell::new(Vec::new()));

        let result = AttachmentDownloadRuntime::new(
            &client,
            ReadySessionProvider {
                scopes: Rc::clone(&sessions),
            },
            RecordingTransport {
                calls: Rc::clone(&calls),
            },
        )
        .download(AttachmentDownloadInput {
            request: crate::attachments::DownloadAttachmentRequest {
                thread: crate::messages::ThreadRef::Group(
                    crate::ids::GroupRef::parse("did:example:group").unwrap(),
                ),
                message_id: crate::ids::MessageId::parse("group-msg-1").unwrap(),
                attachment_id: None,
                destination: crate::attachments::AttachmentDestination::Memory,
                overwrite: false,
            },
            resolved_peer_did: None,
        })
        .unwrap();

        assert_eq!(
            sessions.borrow().as_slice(),
            &[crate::auth::AuthScope::GroupMessaging]
        );
        assert_eq!(result.selection.attachment_id, "att-group-1");
        let calls = calls.borrow();
        let list = calls[0].rpc("group.list_messages");
        assert_eq!(list.params["body"]["group_did"], "did:example:group");
        let ticket = calls[2].rpc("attachment.get_download_ticket");
        assert_eq!(ticket.params["body"]["group_did"], "did:example:group");
        assert_eq!(ticket.params["body"].get("message_target_did"), None);
    }

    #[test]
    fn attachments_download_runtime_local_file_destination_writes_file() {
        let fixture = Fixture::new();
        let client = fixture.client();
        let calls = Rc::new(RefCell::new(Vec::new()));
        let output = fixture.root.join("downloads").join("report.txt");
        fs::create_dir_all(output.parent().unwrap()).unwrap();

        let result = AttachmentDownloadRuntime::new(
            &client,
            ReadySessionProvider {
                scopes: Rc::new(RefCell::new(Vec::new())),
            },
            RecordingTransport {
                calls: Rc::clone(&calls),
            },
        )
        .download(AttachmentDownloadInput {
            request: crate::attachments::DownloadAttachmentRequest {
                thread: crate::messages::ThreadRef::Direct(
                    crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
                ),
                message_id: crate::ids::MessageId::parse("msg-attachment-1").unwrap(),
                attachment_id: Some("att-1".to_string()),
                destination: crate::attachments::AttachmentDestination::LocalFile(output.clone()),
                overwrite: false,
            },
            resolved_peer_did: None,
        })
        .unwrap();

        assert!(matches!(
            result.sdk_result.destination,
            crate::attachments::DownloadedAttachmentDestination::LocalFile(path)
                if path == output
        ));
        assert_eq!(fs::read(&output).unwrap(), b"downloaded bytes");
        assert_no_attachment_temp_files(output.parent().unwrap());
        assert_eq!(calls.borrow().len(), 4);
    }

    #[tokio::test]
    async fn attachments_download_runtime_local_file_async_streams_to_file() {
        let fixture = Fixture::new();
        let client = fixture.client();
        let calls = Rc::new(RefCell::new(Vec::new()));
        let output = fixture.root.join("downloads").join("report-async.txt");
        fs::create_dir_all(output.parent().unwrap()).unwrap();

        let result = AttachmentDownloadRuntime::new(
            &client,
            ReadySessionProvider {
                scopes: Rc::new(RefCell::new(Vec::new())),
            },
            RecordingTransport {
                calls: Rc::clone(&calls),
            },
        )
        .download_async(AttachmentDownloadInput {
            request: crate::attachments::DownloadAttachmentRequest {
                thread: crate::messages::ThreadRef::Direct(
                    crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
                ),
                message_id: crate::ids::MessageId::parse("msg-attachment-1").unwrap(),
                attachment_id: Some("att-1".to_string()),
                destination: crate::attachments::AttachmentDestination::LocalFile(output.clone()),
                overwrite: false,
            },
            resolved_peer_did: None,
        })
        .await
        .unwrap();

        assert!(matches!(
            result.sdk_result.destination,
            crate::attachments::DownloadedAttachmentDestination::LocalFile(path)
                if path == output
        ));
        assert_eq!(fs::read(&output).unwrap(), b"downloaded bytes");
        assert_no_attachment_temp_files(output.parent().unwrap());
        let calls = calls.borrow();
        assert_eq!(calls.len(), 4);
        let object = calls[3].object_get_stream("https://objects.example/att-1");
        assert_eq!(object.ticket, "ticket-1");
    }

    #[test]
    fn attachments_download_runtime_local_file_rejects_existing_destination_without_network() {
        let fixture = Fixture::new();
        let client = fixture.client();
        let calls = Rc::new(RefCell::new(Vec::new()));
        let output = fixture.root.join("downloads").join("report.txt");
        fs::create_dir_all(output.parent().unwrap()).unwrap();
        fs::write(&output, b"existing").unwrap();

        let err = AttachmentDownloadRuntime::new(
            &client,
            ReadySessionProvider {
                scopes: Rc::new(RefCell::new(Vec::new())),
            },
            RecordingTransport {
                calls: Rc::clone(&calls),
            },
        )
        .download(AttachmentDownloadInput {
            request: crate::attachments::DownloadAttachmentRequest {
                thread: crate::messages::ThreadRef::Direct(
                    crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
                ),
                message_id: crate::ids::MessageId::parse("msg-attachment-1").unwrap(),
                attachment_id: Some("att-1".to_string()),
                destination: crate::attachments::AttachmentDestination::LocalFile(output.clone()),
                overwrite: false,
            },
            resolved_peer_did: None,
        })
        .unwrap_err();

        assert!(matches!(
            err,
            crate::ImError::InvalidInput { field: Some(field), message }
                if field == "destination" && message.contains("overwrite is false")
        ));
        assert_eq!(fs::read(&output).unwrap(), b"existing");
        assert_no_attachment_temp_files(output.parent().unwrap());
        assert!(calls.borrow().is_empty());
    }

    #[test]
    fn attachments_download_runtime_falls_back_to_local_identity_document_for_sender_service() {
        let fixture = Fixture::new();
        fixture.write_attachment_service_document(
            "sender",
            "did:web:example:alice",
            "https://local-attachment.example/rpc",
            "did:example:local-message-service",
        );
        let client = fixture.client();
        let calls = Rc::new(RefCell::new(Vec::new()));

        let result = AttachmentDownloadRuntime::new(
            &client,
            ReadySessionProvider {
                scopes: Rc::new(RefCell::new(Vec::new())),
            },
            LocalFallbackTransport {
                calls: Rc::clone(&calls),
            },
        )
        .download(AttachmentDownloadInput {
            request: crate::attachments::DownloadAttachmentRequest {
                thread: crate::messages::ThreadRef::Direct(
                    crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
                ),
                message_id: crate::ids::MessageId::parse("msg-local-sender").unwrap(),
                attachment_id: Some("att-local".to_string()),
                destination: crate::attachments::AttachmentDestination::Memory,
                overwrite: false,
            },
            resolved_peer_did: None,
        })
        .unwrap();

        assert_eq!(result.selection.sender_did, "did:web:example:alice");
        assert_eq!(result.ticket.download_ticket_b64u, "ticket-local");
        assert!(matches!(
            result.sdk_result.destination,
            crate::attachments::DownloadedAttachmentDestination::Memory(bytes)
                if bytes == b"local document bytes".to_vec()
        ));

        let calls = calls.borrow();
        assert_eq!(calls.len(), 4);
        let history = calls[0].rpc("direct.get_history");
        assert_eq!(history.params["body"]["peer_did"], "did:example:bob");
        calls[1].get_json("https://example/alice/did.json");
        let ticket = calls[2].rpc("attachment.get_download_ticket");
        assert_eq!(ticket.endpoint, "https://local-attachment.example/rpc");
        assert_eq!(
            ticket.params["meta"]["target"],
            json!({"kind": "service", "did": "did:example:local-message-service"})
        );
        assert_eq!(ticket.params["body"]["sender_did"], "did:web:example:alice");
        let object = calls[3].object_get("https://objects.example/att-local");
        assert_eq!(object.ticket, "ticket-local");
    }

    #[test]
    fn attachments_download_runtime_object_e2ee_memory_returns_plaintext_and_redacts_selection() {
        let fixture = Fixture::new();
        let client = fixture.client();
        let object = object_e2ee_case(b"e2ee plaintext".to_vec());
        let calls = Rc::new(RefCell::new(Vec::new()));

        let result = AttachmentDownloadRuntime::new(
            &client,
            ReadySessionProvider {
                scopes: Rc::new(RefCell::new(Vec::new())),
            },
            E2eeTransport {
                calls: Rc::clone(&calls),
                history: e2ee_history_response_with_legacy_missing_profile(
                    object.full_manifest.clone(),
                ),
                object_body: object.ciphertext.clone(),
                object_content_type: Some("application/octet-stream".to_string()),
            },
        )
        .download(AttachmentDownloadInput {
            request: crate::attachments::DownloadAttachmentRequest {
                thread: crate::messages::ThreadRef::Direct(
                    crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
                ),
                message_id: crate::ids::MessageId::parse("msg-e2ee-1").unwrap(),
                attachment_id: Some("att-e2ee-1".to_string()),
                destination: crate::attachments::AttachmentDestination::Memory,
                overwrite: false,
            },
            resolved_peer_did: None,
        })
        .unwrap();

        assert_eq!(result.selection.attachment_id, "att-e2ee-1");
        assert_eq!(result.selection.message_security_profile, "direct-e2ee");
        assert_eq!(
            result
                .sdk_result
                .selection
                .as_ref()
                .unwrap()
                .message_security_profile,
            "direct-e2ee"
        );
        assert_eq!(result.selection.object_encryption_mode, "object-e2ee");
        assert_eq!(
            result.selection.object_cipher.as_deref(),
            Some("chacha20-poly1305")
        );
        let expected_plaintext_size = object.plaintext.len().to_string();
        assert_eq!(
            result.selection.plaintext_size.as_deref(),
            Some(expected_plaintext_size.as_str())
        );
        assert_eq!(
            result.sdk_result.size_bytes,
            Some(object.plaintext.len() as u64)
        );
        assert!(matches!(
            result.sdk_result.destination,
            crate::attachments::DownloadedAttachmentDestination::Memory(bytes)
                if bytes == object.plaintext
        ));

        let public_selection = serde_json::to_string(&result.selection).unwrap();
        assert!(!public_selection.contains("object_key_b64u"));
        assert!(!public_selection.contains("nonce_b64u"));
        assert!(!public_selection.contains(&object.object_key_b64u));
        assert!(!public_selection.contains(&object.nonce_b64u));
        let sdk_selection = serde_json::to_string(&result.sdk_result.selection).unwrap();
        assert!(!sdk_selection.contains("object_key_b64u"));
        assert!(!sdk_selection.contains("nonce_b64u"));
        assert!(!sdk_selection.contains(&object.object_key_b64u));
        assert!(!sdk_selection.contains(&object.nonce_b64u));

        let calls = calls.borrow();
        assert_eq!(calls.len(), 4);
        let ticket = calls[2].rpc("attachment.get_download_ticket");
        assert_eq!(
            ticket.params["body"]["message_security_profile"],
            "direct-e2ee"
        );
        assert_eq!(
            ticket.params["body"]["message_target_did"],
            "did:example:alice"
        );
        assert_eq!(ticket.params["body"].get("group_did"), None);
        assert_eq!(
            ticket.params["body"].get("object_key_b64u"),
            None,
            "download ticket body must not expose object key"
        );
        assert_eq!(
            ticket.params["body"].get("nonce_b64u"),
            None,
            "download ticket body must not expose nonce"
        );
        calls[3].object_get("https://objects.example/att-e2ee-1");
    }

    #[test]
    fn attachments_download_runtime_group_object_e2ee_uses_internal_manifest_cache() {
        let fixture = Fixture::new();
        let client = fixture.client();
        let object = object_e2ee_case(b"group cached plaintext".to_vec());
        {
            let connection = crate::internal::local_state::open_writable(
                &client.core_inner().sdk_paths().local_state.sqlite_path,
            )
            .unwrap();
            crate::internal::local_state::attachment_manifest_cache::upsert_attachment_manifest_cache(
                &connection,
                &crate::internal::local_state::attachment_manifest_cache::AttachmentManifestCacheRecord {
                    owner_identity_id: client.current_identity().id.as_str().to_owned(),
                    owner_did: client.did().as_str().to_owned(),
                    thread_kind: "group".to_owned(),
                    thread_id: "did:example:group:e2ee".to_owned(),
                    message_id: "did:example:group:e2ee:7".to_owned(),
                    sender_did: "did:web:example.com:bob".to_owned(),
                    message_security_profile: "group-e2ee".to_owned(),
                    content: serde_json::to_string(&object.full_manifest).unwrap(),
                    stored_at: "2026-06-02T00:00:00Z".to_owned(),
                },
            )
            .unwrap();
        }
        let calls = Rc::new(RefCell::new(Vec::new()));

        let result = AttachmentDownloadRuntime::new(
            &client,
            ReadySessionProvider {
                scopes: Rc::new(RefCell::new(Vec::new())),
            },
            E2eeTransport {
                calls: Rc::clone(&calls),
                history: json!({
                    "messages": [],
                    "has_more": false
                }),
                object_body: object.ciphertext.clone(),
                object_content_type: Some("application/octet-stream".to_string()),
            },
        )
        .download(AttachmentDownloadInput {
            request: crate::attachments::DownloadAttachmentRequest {
                thread: crate::messages::ThreadRef::Group(
                    crate::ids::GroupRef::parse("did:example:group:e2ee").unwrap(),
                ),
                message_id: crate::ids::MessageId::parse("did:example:group:e2ee:7").unwrap(),
                attachment_id: Some("att-e2ee-1".to_string()),
                destination: crate::attachments::AttachmentDestination::Memory,
                overwrite: false,
            },
            resolved_peer_did: None,
        })
        .unwrap();

        assert_eq!(result.selection.message_security_profile, "group-e2ee");
        assert_eq!(result.selection.object_encryption_mode, "object-e2ee");
        assert!(matches!(
            result.sdk_result.destination,
            crate::attachments::DownloadedAttachmentDestination::Memory(bytes)
                if bytes == object.plaintext
        ));
        let public_selection = serde_json::to_string(&result.selection).unwrap();
        assert!(!public_selection.contains("object_key_b64u"));
        assert!(!public_selection.contains("nonce_b64u"));
        assert!(!public_selection.contains(&object.object_key_b64u));
        assert!(!public_selection.contains(&object.nonce_b64u));

        let calls = calls.borrow();
        assert_eq!(calls.len(), 3);
        calls[0].get_json("https://example.com/bob/did.json");
        let ticket = calls[1].rpc("attachment.get_download_ticket");
        assert_eq!(
            ticket.params["body"]["message_security_profile"],
            "group-e2ee"
        );
        assert_eq!(ticket.params["body"]["group_did"], "did:example:group:e2ee");
        assert_eq!(ticket.params["body"].get("object_key_b64u"), None);
        assert_eq!(ticket.params["body"].get("nonce_b64u"), None);
        calls[2].object_get("https://objects.example/att-e2ee-1");
    }

    #[test]
    fn attachments_download_runtime_object_e2ee_local_file_writes_plaintext() {
        let fixture = Fixture::new();
        let client = fixture.client();
        let object = object_e2ee_case(b"local e2ee plaintext".to_vec());
        let calls = Rc::new(RefCell::new(Vec::new()));
        let output = fixture.root.join("downloads").join("secret.pdf");
        fs::create_dir_all(output.parent().unwrap()).unwrap();

        let result = AttachmentDownloadRuntime::new(
            &client,
            ReadySessionProvider {
                scopes: Rc::new(RefCell::new(Vec::new())),
            },
            E2eeTransport {
                calls: Rc::clone(&calls),
                history: e2ee_history_response(object.full_manifest.clone(), "direct-e2ee"),
                object_body: object.ciphertext.clone(),
                object_content_type: Some("application/octet-stream".to_string()),
            },
        )
        .download(AttachmentDownloadInput {
            request: crate::attachments::DownloadAttachmentRequest {
                thread: crate::messages::ThreadRef::Direct(
                    crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
                ),
                message_id: crate::ids::MessageId::parse("msg-e2ee-1").unwrap(),
                attachment_id: Some("att-e2ee-1".to_string()),
                destination: crate::attachments::AttachmentDestination::LocalFile(output.clone()),
                overwrite: false,
            },
            resolved_peer_did: None,
        })
        .unwrap();

        assert!(matches!(
            result.sdk_result.destination,
            crate::attachments::DownloadedAttachmentDestination::LocalFile(path)
                if path == output
        ));
        assert_eq!(fs::read(&output).unwrap(), object.plaintext);
        assert_no_attachment_temp_files(output.parent().unwrap());
        assert_eq!(calls.borrow().len(), 4);
    }

    #[tokio::test]
    async fn attachments_download_runtime_object_e2ee_local_file_async_writes_plaintext() {
        let fixture = Fixture::new();
        let client = fixture.client();
        let object = object_e2ee_case(b"async local e2ee plaintext".to_vec());
        let calls = Rc::new(RefCell::new(Vec::new()));
        let output = fixture.root.join("downloads").join("secret-async.pdf");
        fs::create_dir_all(output.parent().unwrap()).unwrap();

        let result = AttachmentDownloadRuntime::new(
            &client,
            ReadySessionProvider {
                scopes: Rc::new(RefCell::new(Vec::new())),
            },
            E2eeTransport {
                calls: Rc::clone(&calls),
                history: e2ee_history_response(object.full_manifest.clone(), "direct-e2ee"),
                object_body: object.ciphertext.clone(),
                object_content_type: Some("application/octet-stream".to_string()),
            },
        )
        .download_async(AttachmentDownloadInput {
            request: crate::attachments::DownloadAttachmentRequest {
                thread: crate::messages::ThreadRef::Direct(
                    crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
                ),
                message_id: crate::ids::MessageId::parse("msg-e2ee-1").unwrap(),
                attachment_id: Some("att-e2ee-1".to_string()),
                destination: crate::attachments::AttachmentDestination::LocalFile(output.clone()),
                overwrite: false,
            },
            resolved_peer_did: None,
        })
        .await
        .unwrap();

        assert!(matches!(
            result.sdk_result.destination,
            crate::attachments::DownloadedAttachmentDestination::LocalFile(path)
                if path == output
        ));
        assert_eq!(fs::read(&output).unwrap(), object.plaintext);
        assert_no_attachment_temp_files(output.parent().unwrap());
        let calls = calls.borrow();
        assert_eq!(calls.len(), 4);
        calls[3].object_get_stream("https://objects.example/att-e2ee-1");
    }

    #[test]
    fn attachments_download_runtime_rejects_digest_mismatch_without_writing() {
        let fixture = Fixture::new();
        let client = fixture.client();
        let mut object = object_e2ee_case(b"secret with bad digest".to_vec());
        object.full_manifest["attachments"][0]["digest"]["value_b64u"] =
            Value::String("wrong-digest".to_string());
        let calls = Rc::new(RefCell::new(Vec::new()));
        let output = fixture.root.join("downloads").join("bad-digest.pdf");
        fs::create_dir_all(output.parent().unwrap()).unwrap();

        let err = AttachmentDownloadRuntime::new(
            &client,
            ReadySessionProvider {
                scopes: Rc::new(RefCell::new(Vec::new())),
            },
            E2eeTransport {
                calls: Rc::clone(&calls),
                history: e2ee_history_response(object.full_manifest.clone(), "direct-e2ee"),
                object_body: object.ciphertext.clone(),
                object_content_type: None,
            },
        )
        .download(AttachmentDownloadInput {
            request: crate::attachments::DownloadAttachmentRequest {
                thread: crate::messages::ThreadRef::Direct(
                    crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
                ),
                message_id: crate::ids::MessageId::parse("msg-e2ee-1").unwrap(),
                attachment_id: Some("att-e2ee-1".to_string()),
                destination: crate::attachments::AttachmentDestination::LocalFile(output.clone()),
                overwrite: false,
            },
            resolved_peer_did: None,
        })
        .unwrap_err();

        assert_service_error_code(err, "anp.attachment.digest_mismatch");
        assert!(!output.exists());
        assert_no_attachment_temp_files(output.parent().unwrap());
        assert_eq!(calls.borrow().len(), 4);
    }

    #[test]
    fn attachments_download_runtime_rejects_wrong_object_key_without_writing() {
        let fixture = Fixture::new();
        let client = fixture.client();
        let mut object = object_e2ee_case(b"secret with wrong key".to_vec());
        let other = object_e2ee_case(b"other secret".to_vec());
        object.full_manifest["attachments"][0]["encryption_info"]["object_key_b64u"] =
            Value::String(other.object_key_b64u);
        let calls = Rc::new(RefCell::new(Vec::new()));
        let output = fixture.root.join("downloads").join("wrong-key.pdf");
        fs::create_dir_all(output.parent().unwrap()).unwrap();

        let err = AttachmentDownloadRuntime::new(
            &client,
            ReadySessionProvider {
                scopes: Rc::new(RefCell::new(Vec::new())),
            },
            E2eeTransport {
                calls: Rc::clone(&calls),
                history: e2ee_history_response(object.full_manifest.clone(), "direct-e2ee"),
                object_body: object.ciphertext.clone(),
                object_content_type: None,
            },
        )
        .download(AttachmentDownloadInput {
            request: crate::attachments::DownloadAttachmentRequest {
                thread: crate::messages::ThreadRef::Direct(
                    crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
                ),
                message_id: crate::ids::MessageId::parse("msg-e2ee-1").unwrap(),
                attachment_id: Some("att-e2ee-1".to_string()),
                destination: crate::attachments::AttachmentDestination::LocalFile(output.clone()),
                overwrite: false,
            },
            resolved_peer_did: None,
        })
        .unwrap_err();

        assert_service_error_code(err, "anp.attachment.decrypt_failed");
        assert!(!output.exists());
        assert_no_attachment_temp_files(output.parent().unwrap());
        assert_eq!(calls.borrow().len(), 4);
    }

    #[test]
    fn attachments_download_runtime_rejects_plaintext_size_mismatch_without_writing() {
        let fixture = Fixture::new();
        let client = fixture.client();
        let mut object = object_e2ee_case(b"secret with wrong plaintext size".to_vec());
        object.full_manifest["attachments"][0]["encryption_info"]["plaintext_size"] =
            Value::String((object.plaintext.len() + 1).to_string());
        let calls = Rc::new(RefCell::new(Vec::new()));
        let output = fixture.root.join("downloads").join("wrong-size.pdf");
        fs::create_dir_all(output.parent().unwrap()).unwrap();

        let err = AttachmentDownloadRuntime::new(
            &client,
            ReadySessionProvider {
                scopes: Rc::new(RefCell::new(Vec::new())),
            },
            E2eeTransport {
                calls: Rc::clone(&calls),
                history: e2ee_history_response(object.full_manifest.clone(), "direct-e2ee"),
                object_body: object.ciphertext.clone(),
                object_content_type: None,
            },
        )
        .download(AttachmentDownloadInput {
            request: crate::attachments::DownloadAttachmentRequest {
                thread: crate::messages::ThreadRef::Direct(
                    crate::ids::PeerRef::parse("did:example:bob", "").unwrap(),
                ),
                message_id: crate::ids::MessageId::parse("msg-e2ee-1").unwrap(),
                attachment_id: Some("att-e2ee-1".to_string()),
                destination: crate::attachments::AttachmentDestination::LocalFile(output.clone()),
                overwrite: false,
            },
            resolved_peer_did: None,
        })
        .unwrap_err();

        assert_service_error_code(err, "anp.attachment.decrypt_failed");
        assert!(!output.exists());
        assert_no_attachment_temp_files(output.parent().unwrap());
        assert_eq!(calls.borrow().len(), 4);
    }

    #[derive(Clone)]
    struct ReadySessionProvider {
        scopes: Rc<RefCell<Vec<crate::auth::AuthScope>>>,
    }

    impl SessionProvider for ReadySessionProvider {
        fn ensure_session(
            &self,
            scope: crate::auth::AuthScope,
        ) -> crate::ImResult<crate::auth::SessionBundle> {
            self.scopes.borrow_mut().push(scope);
            Ok(crate::auth::SessionBundle {
                subject: crate::ids::Did::parse("did:example:alice")?,
                scope,
                expires_at: None,
                refreshed: false,
                bearer_token: None,
            })
        }

        fn refresh_session(&self) -> crate::ImResult<crate::auth::SessionUpdate> {
            unreachable!("attachment download runtime should not refresh through test provider")
        }

        fn status(&self) -> crate::ImResult<crate::auth::AuthStatus> {
            unreachable!("attachment download runtime should not read status")
        }
    }

    impl crate::internal::auth::session::AsyncSessionProvider for ReadySessionProvider {
        async fn ensure_session(
            &self,
            scope: crate::auth::AuthScope,
        ) -> crate::ImResult<crate::auth::SessionBundle> {
            self.scopes.borrow_mut().push(scope);
            Ok(crate::auth::SessionBundle {
                subject: crate::ids::Did::parse("did:example:alice")?,
                scope,
                expires_at: None,
                refreshed: false,
                bearer_token: None,
            })
        }

        async fn refresh_session(&self) -> crate::ImResult<crate::auth::SessionUpdate> {
            unreachable!("attachment download runtime should not refresh through test provider")
        }

        async fn status(&self) -> crate::ImResult<crate::auth::AuthStatus> {
            unreachable!("attachment download runtime should not read status")
        }
    }

    struct RecordingTransport {
        calls: Rc<RefCell<Vec<RecordedCall>>>,
    }

    impl AuthenticatedRpcTransport for RecordingTransport {
        fn authenticated_rpc(
            &mut self,
            endpoint: &str,
            method: &str,
            params: Value,
        ) -> crate::ImResult<Value> {
            self.calls.borrow_mut().push(RecordedCall::Rpc {
                endpoint: endpoint.to_string(),
                method: method.to_string(),
                params: params.clone(),
            });
            match method {
                "direct.get_history" => direct_history_response(params["body"]["skip"].as_i64()),
                "group.list_messages" => Ok(group_history_response()),
                "attachment.get_download_ticket" => Ok(json!({
                    "download_ticket_b64u": "ticket-1",
                    "expires_at": "2026-05-23T01:00:00Z",
                    "ticket_binding": {
                        "attachment_id": params["body"]["attachment_id"].clone()
                    }
                })),
                _ => Err(crate::ImError::TransportUnavailable {
                    detail: format!("unexpected rpc method {method} at {endpoint}"),
                }),
            }
        }
    }

    impl RawJsonTransport for RecordingTransport {
        fn get_json_url(
            &mut self,
            url: &str,
            headers: BTreeMap<String, String>,
        ) -> crate::ImResult<Value> {
            self.calls.borrow_mut().push(RecordedCall::GetJson {
                url: url.to_string(),
                headers,
            });
            Ok(json!({
                "id": "did:web:example.com:bob",
                "service": [{
                    "id": "#attachment",
                    "type": "ANPMessageService",
                    "serviceEndpoint": "https://attachment.example/rpc",
                    "serviceDid": "did:web:attachment.example",
                    "profiles": ["anp.attachment.v1"],
                    "securityProfiles": ["transport-protected"],
                    "priority": 1
                }]
            }))
        }
    }

    impl RpcTransport for RecordingTransport {
        fn rpc(&mut self, endpoint: &str, method: &str, params: Value) -> crate::ImResult<Value> {
            AuthenticatedRpcTransport::authenticated_rpc(self, endpoint, method, params)
        }
    }

    impl AttachmentObjectTransport for RecordingTransport {
        fn put_attachment_object(
            &mut self,
            _upload_uri: &str,
            _headers: BTreeMap<String, String>,
            _body: Vec<u8>,
        ) -> crate::ImResult<()> {
            unreachable!("download runtime should not upload objects")
        }

        fn get_attachment_object(
            &mut self,
            object_uri: &str,
            download_ticket: &str,
        ) -> crate::ImResult<AttachmentObjectResponse> {
            self.calls.borrow_mut().push(RecordedCall::GetObject {
                object_uri: object_uri.to_string(),
                ticket: download_ticket.to_string(),
            });
            Ok(AttachmentObjectResponse {
                body: b"downloaded bytes".to_vec(),
                content_type: Some("application/octet-stream".to_string()),
            })
        }
    }

    impl crate::internal::transport::AsyncAuthenticatedRpcTransport for RecordingTransport {
        async fn authenticated_rpc(
            &mut self,
            endpoint: &str,
            method: &str,
            params: Value,
        ) -> crate::ImResult<Value> {
            AuthenticatedRpcTransport::authenticated_rpc(self, endpoint, method, params)
        }
    }

    impl crate::internal::transport::AsyncRawJsonTransport for RecordingTransport {
        async fn get_json_url(
            &mut self,
            url: &str,
            headers: BTreeMap<String, String>,
        ) -> crate::ImResult<Value> {
            RawJsonTransport::get_json_url(self, url, headers)
        }
    }

    impl crate::internal::transport::AsyncRpcTransport for RecordingTransport {
        async fn rpc(
            &mut self,
            endpoint: &str,
            method: &str,
            params: Value,
        ) -> crate::ImResult<Value> {
            RpcTransport::rpc(self, endpoint, method, params)
        }
    }

    impl crate::internal::transport::AsyncAttachmentObjectTransport for RecordingTransport {
        async fn put_attachment_object(
            &mut self,
            upload_uri: &str,
            headers: BTreeMap<String, String>,
            body: Vec<u8>,
        ) -> crate::ImResult<()> {
            AttachmentObjectTransport::put_attachment_object(self, upload_uri, headers, body)
        }

        async fn get_attachment_object(
            &mut self,
            object_uri: &str,
            download_ticket: &str,
        ) -> crate::ImResult<AttachmentObjectResponse> {
            AttachmentObjectTransport::get_attachment_object(self, object_uri, download_ticket)
        }

        async fn get_attachment_object_stream(
            &mut self,
            object_uri: &str,
            download_ticket: &str,
        ) -> crate::ImResult<crate::internal::transport::AsyncAttachmentObjectResponse> {
            self.calls.borrow_mut().push(RecordedCall::GetObjectStream {
                object_uri: object_uri.to_string(),
                ticket: download_ticket.to_string(),
            });
            Ok(
                crate::internal::transport::AsyncAttachmentObjectResponse::Bytes {
                    body: b"downloaded bytes".to_vec(),
                    content_type: Some("application/octet-stream".to_string()),
                    consumed: false,
                },
            )
        }
    }

    struct LocalFallbackTransport {
        calls: Rc<RefCell<Vec<RecordedCall>>>,
    }

    impl AuthenticatedRpcTransport for LocalFallbackTransport {
        fn authenticated_rpc(
            &mut self,
            endpoint: &str,
            method: &str,
            params: Value,
        ) -> crate::ImResult<Value> {
            self.calls.borrow_mut().push(RecordedCall::Rpc {
                endpoint: endpoint.to_string(),
                method: method.to_string(),
                params: params.clone(),
            });
            match method {
                "direct.get_history" => Ok(json!({
                    "messages": [{
                        "id": "msg-local-sender",
                        "message_id": "msg-local-sender",
                        "sender_did": "did:web:example:alice",
                        "content": {
                            "attachments": [{
                                "attachment_id": "att-local",
                                "filename": "local.txt",
                                "mime_type": "text/plain",
                                "size": "20",
                                "digest": { "alg": "sha-256", "value_b64u": "nckhHGQ875uyaFS-XN4okcnICwLgNUjey_suVwwrcNY" },
                                "access_info": { "object_uri": "https://objects.example/att-local" }
                            }],
                            "primary_attachment_id": "att-local"
                        }
                    }],
                    "has_more": false
                })),
                "attachment.get_download_ticket" => Ok(json!({
                    "download_ticket_b64u": "ticket-local",
                    "expires_at": "2026-05-23T01:00:00Z",
                    "ticket_binding": {
                        "attachment_id": params["body"]["attachment_id"].clone()
                    }
                })),
                _ => Err(crate::ImError::TransportUnavailable {
                    detail: format!("unexpected rpc method {method} at {endpoint}"),
                }),
            }
        }
    }

    impl RawJsonTransport for LocalFallbackTransport {
        fn get_json_url(
            &mut self,
            url: &str,
            headers: BTreeMap<String, String>,
        ) -> crate::ImResult<Value> {
            self.calls.borrow_mut().push(RecordedCall::GetJson {
                url: url.to_string(),
                headers,
            });
            Err(crate::ImError::TransportUnavailable {
                detail: format!("forced DID document miss for {url}"),
            })
        }
    }

    impl RpcTransport for LocalFallbackTransport {
        fn rpc(&mut self, endpoint: &str, method: &str, params: Value) -> crate::ImResult<Value> {
            AuthenticatedRpcTransport::authenticated_rpc(self, endpoint, method, params)
        }
    }

    impl AttachmentObjectTransport for LocalFallbackTransport {
        fn put_attachment_object(
            &mut self,
            _upload_uri: &str,
            _headers: BTreeMap<String, String>,
            _body: Vec<u8>,
        ) -> crate::ImResult<()> {
            unreachable!("download runtime should not upload objects")
        }

        fn get_attachment_object(
            &mut self,
            object_uri: &str,
            download_ticket: &str,
        ) -> crate::ImResult<AttachmentObjectResponse> {
            self.calls.borrow_mut().push(RecordedCall::GetObject {
                object_uri: object_uri.to_string(),
                ticket: download_ticket.to_string(),
            });
            Ok(AttachmentObjectResponse {
                body: b"local document bytes".to_vec(),
                content_type: Some("text/plain".to_string()),
            })
        }
    }

    struct E2eeTransport {
        calls: Rc<RefCell<Vec<RecordedCall>>>,
        history: Value,
        object_body: Vec<u8>,
        object_content_type: Option<String>,
    }

    impl AuthenticatedRpcTransport for E2eeTransport {
        fn authenticated_rpc(
            &mut self,
            endpoint: &str,
            method: &str,
            params: Value,
        ) -> crate::ImResult<Value> {
            self.calls.borrow_mut().push(RecordedCall::Rpc {
                endpoint: endpoint.to_string(),
                method: method.to_string(),
                params: params.clone(),
            });
            match method {
                "direct.get_history" => Ok(self.history.clone()),
                "attachment.get_download_ticket" => Ok(json!({
                    "download_ticket_b64u": "ticket-e2ee",
                    "expires_at": "2026-05-23T01:00:00Z",
                    "ticket_binding": {
                        "attachment_id": params["body"]["attachment_id"].clone()
                    }
                })),
                _ => Err(crate::ImError::TransportUnavailable {
                    detail: format!("unexpected rpc method {method} at {endpoint}"),
                }),
            }
        }
    }

    impl RawJsonTransport for E2eeTransport {
        fn get_json_url(
            &mut self,
            url: &str,
            headers: BTreeMap<String, String>,
        ) -> crate::ImResult<Value> {
            self.calls.borrow_mut().push(RecordedCall::GetJson {
                url: url.to_string(),
                headers,
            });
            Ok(json!({
                "id": "did:web:example.com:bob",
                "service": [{
                    "id": "#attachment",
                    "type": "ANPMessageService",
                    "serviceEndpoint": "https://attachment.example/rpc",
                    "serviceDid": "did:web:attachment.example",
                    "profiles": ["anp.attachment.v1"],
                    "securityProfiles": ["transport-protected"],
                    "priority": 1
                }]
            }))
        }
    }

    impl RpcTransport for E2eeTransport {
        fn rpc(&mut self, endpoint: &str, method: &str, params: Value) -> crate::ImResult<Value> {
            AuthenticatedRpcTransport::authenticated_rpc(self, endpoint, method, params)
        }
    }

    impl AttachmentObjectTransport for E2eeTransport {
        fn put_attachment_object(
            &mut self,
            _upload_uri: &str,
            _headers: BTreeMap<String, String>,
            _body: Vec<u8>,
        ) -> crate::ImResult<()> {
            unreachable!("download runtime should not upload objects")
        }

        fn get_attachment_object(
            &mut self,
            object_uri: &str,
            download_ticket: &str,
        ) -> crate::ImResult<AttachmentObjectResponse> {
            self.calls.borrow_mut().push(RecordedCall::GetObject {
                object_uri: object_uri.to_string(),
                ticket: download_ticket.to_string(),
            });
            Ok(AttachmentObjectResponse {
                body: self.object_body.clone(),
                content_type: self.object_content_type.clone(),
            })
        }
    }

    impl crate::internal::transport::AsyncAuthenticatedRpcTransport for E2eeTransport {
        async fn authenticated_rpc(
            &mut self,
            endpoint: &str,
            method: &str,
            params: Value,
        ) -> crate::ImResult<Value> {
            AuthenticatedRpcTransport::authenticated_rpc(self, endpoint, method, params)
        }
    }

    impl crate::internal::transport::AsyncRawJsonTransport for E2eeTransport {
        async fn get_json_url(
            &mut self,
            url: &str,
            headers: BTreeMap<String, String>,
        ) -> crate::ImResult<Value> {
            RawJsonTransport::get_json_url(self, url, headers)
        }
    }

    impl crate::internal::transport::AsyncRpcTransport for E2eeTransport {
        async fn rpc(
            &mut self,
            endpoint: &str,
            method: &str,
            params: Value,
        ) -> crate::ImResult<Value> {
            RpcTransport::rpc(self, endpoint, method, params)
        }
    }

    impl crate::internal::transport::AsyncAttachmentObjectTransport for E2eeTransport {
        async fn put_attachment_object(
            &mut self,
            upload_uri: &str,
            headers: BTreeMap<String, String>,
            body: Vec<u8>,
        ) -> crate::ImResult<()> {
            AttachmentObjectTransport::put_attachment_object(self, upload_uri, headers, body)
        }

        async fn get_attachment_object(
            &mut self,
            object_uri: &str,
            download_ticket: &str,
        ) -> crate::ImResult<AttachmentObjectResponse> {
            AttachmentObjectTransport::get_attachment_object(self, object_uri, download_ticket)
        }

        async fn get_attachment_object_stream(
            &mut self,
            object_uri: &str,
            download_ticket: &str,
        ) -> crate::ImResult<crate::internal::transport::AsyncAttachmentObjectResponse> {
            self.calls.borrow_mut().push(RecordedCall::GetObjectStream {
                object_uri: object_uri.to_string(),
                ticket: download_ticket.to_string(),
            });
            Ok(
                crate::internal::transport::AsyncAttachmentObjectResponse::Bytes {
                    body: self.object_body.clone(),
                    content_type: self.object_content_type.clone(),
                    consumed: false,
                },
            )
        }
    }

    #[derive(Debug, Clone)]
    enum RecordedCall {
        Rpc {
            endpoint: String,
            method: String,
            params: Value,
        },
        GetJson {
            url: String,
            headers: BTreeMap<String, String>,
        },
        GetObject {
            object_uri: String,
            ticket: String,
        },
        GetObjectStream {
            object_uri: String,
            ticket: String,
        },
    }

    impl RecordedCall {
        fn rpc(&self, expected_method: &str) -> RecordedRpc<'_> {
            match self {
                Self::Rpc {
                    endpoint,
                    method,
                    params,
                } => {
                    assert_eq!(method, expected_method);
                    RecordedRpc { endpoint, params }
                }
                _ => panic!("expected rpc call {expected_method}, got {self:?}"),
            }
        }

        fn get_json(&self, expected_url: &str) -> RecordedGetJson<'_> {
            match self {
                Self::GetJson { url, headers } => {
                    assert_eq!(url, expected_url);
                    RecordedGetJson { headers }
                }
                _ => panic!("expected get-json call {expected_url}, got {self:?}"),
            }
        }

        fn object_get(&self, expected_uri: &str) -> RecordedGetObject<'_> {
            match self {
                Self::GetObject { object_uri, ticket } => {
                    assert_eq!(object_uri, expected_uri);
                    RecordedGetObject { ticket }
                }
                _ => panic!("expected object GET call {expected_uri}, got {self:?}"),
            }
        }

        fn object_get_stream(&self, expected_uri: &str) -> RecordedGetObject<'_> {
            match self {
                Self::GetObjectStream { object_uri, ticket } => {
                    assert_eq!(object_uri, expected_uri);
                    RecordedGetObject { ticket }
                }
                _ => panic!("expected object streaming GET call {expected_uri}, got {self:?}"),
            }
        }
    }

    struct RecordedRpc<'a> {
        endpoint: &'a str,
        params: &'a Value,
    }

    struct RecordedGetJson<'a> {
        headers: &'a BTreeMap<String, String>,
    }

    struct RecordedGetObject<'a> {
        ticket: &'a str,
    }

    fn direct_history_response(skip: Option<i64>) -> crate::ImResult<Value> {
        if skip.unwrap_or_default() == 0 {
            Ok(json!({
                "messages": [{
                    "id": "msg-attachment-1",
                    "message_id": "msg-attachment-1",
                    "sender_did": "did:web:example.com:bob",
                    "content": {
                        "attachments": [{
                            "attachment_id": "att-1",
                            "filename": "report.txt",
                            "mime_type": "text/plain",
                            "size": "16",
                            "digest": { "alg": "sha-256", "value_b64u": "exNCHlmX3QP0Pkz7u3ndQu3b5zESko9lysH2TsoflvQ" },
                            "access_info": { "object_uri": "https://objects.example/att-1" }
                        }],
                        "primary_attachment_id": "att-1",
                        "caption": "report"
                    }
                }],
                "has_more": true
            }))
        } else {
            Ok(json!({
                "messages": [{
                    "id": "msg-second-page",
                    "message_id": "msg-second-page",
                    "sender_did": "did:web:example.com:bob",
                    "content": {
                        "attachments": [{
                            "attachment_id": "att-2",
                            "filename": "second.txt",
                            "mime_type": "text/plain",
                            "size": "16",
                            "digest": { "alg": "sha-256", "value_b64u": "exNCHlmX3QP0Pkz7u3ndQu3b5zESko9lysH2TsoflvQ" },
                            "access_info": { "object_uri": "https://objects.example/att-2" }
                        }],
                        "primary_attachment_id": "att-2"
                    }
                }],
                "has_more": false
            }))
        }
    }

    fn group_history_response() -> Value {
        json!({
            "messages": [{
                "id": "group-msg-1",
                "message_id": "group-msg-1",
                "sender_did": "did:web:example.com:bob",
                "content": serde_json::to_string(&json!({
                    "attachments": [{
                        "attachment_id": "att-group-1",
                        "filename": "group.txt",
                        "mime_type": "text/plain",
                        "size": "16",
                        "digest": { "alg": "sha-256", "value_b64u": "exNCHlmX3QP0Pkz7u3ndQu3b5zESko9lysH2TsoflvQ" },
                        "access_info": { "object_uri": "https://objects.example/att-1" }
                    }],
                    "primary_attachment_id": "att-group-1"
                }))
                .unwrap()
            }],
            "has_more": false
        })
    }

    struct ObjectE2eeCase {
        plaintext: Vec<u8>,
        ciphertext: Vec<u8>,
        full_manifest: Value,
        object_key_b64u: String,
        nonce_b64u: String,
    }

    fn object_e2ee_case(plaintext: Vec<u8>) -> ObjectE2eeCase {
        let prepared = crate::attachments::manifest::prepare_object_e2ee_attachment_payload(
            "secret.pdf",
            "application/pdf",
            plaintext.clone(),
        )
        .expect("object-e2ee attachment prepares");
        let descriptor = crate::attachments::manifest::AttachmentDescriptor::from_prepared(
            &prepared.prepared,
            "att-e2ee-1",
            "https://objects.example/att-e2ee-1",
        );
        let full_manifest =
            crate::attachments::manifest::build_attachment_manifest_with_object_e2ee_secrets(
                &descriptor,
                "secret",
                &prepared.secrets,
            );
        ObjectE2eeCase {
            plaintext,
            ciphertext: prepared.prepared.payload,
            full_manifest,
            object_key_b64u: prepared.secrets.object_key_b64u,
            nonce_b64u: prepared.secrets.nonce_b64u,
        }
    }

    fn e2ee_history_response(manifest: Value, message_security_profile: &str) -> Value {
        let mut message = json!({
            "id": "msg-e2ee-1",
            "message_id": "msg-e2ee-1",
            "sender_did": "did:web:example.com:bob",
            "content": manifest
        });
        if !message_security_profile.trim().is_empty() {
            message["message_security_profile"] = json!(message_security_profile);
        }
        json!({
            "messages": [message],
            "has_more": false
        })
    }

    fn e2ee_history_response_with_legacy_missing_profile(manifest: Value) -> Value {
        json!({
            "messages": [{
                "id": "msg-e2ee-1",
                "message_id": "msg-e2ee-1",
                "sender_did": "did:web:example.com:bob",
                "content": manifest
            }],
            "has_more": false
        })
    }

    struct Fixture {
        root: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let root = unique_temp_root();
            let identities = root.join("identities");
            fs::create_dir_all(identities.join("alice")).unwrap();
            fs::write(identities.join("default"), "alice\n").unwrap();
            fs::write(
                identities.join("registry.json"),
                r#"{
                  "default_identity": "alice",
                  "identities": [{
                    "id": "alice-id",
                    "did": "did:example:alice",
                    "local_alias": "alice",
                    "ready_for_auth": true,
                    "ready_for_messaging": true,
                    "missing": []
                  }]
                }"#,
            )
            .unwrap();
            Self { root }
        }

        fn write_attachment_service_document(
            &self,
            alias: &str,
            did: &str,
            rpc_endpoint: &str,
            service_did: &str,
        ) {
            let identity_dir = self.root.join("identities").join(alias);
            fs::create_dir_all(&identity_dir).unwrap();
            fs::write(
                self.root.join("identities").join("registry.json"),
                format!(
                    r#"{{
                      "default_identity": "alice",
                      "identities": [
                        {{
                          "id": "alice-id",
                          "did": "did:example:alice",
                          "local_alias": "alice",
                          "ready_for_auth": true,
                          "ready_for_messaging": true,
                          "missing": []
                        }},
                        {{
                          "id": "{alias}-id",
                          "did": "{did}",
                          "local_alias": "{alias}",
                          "ready_for_auth": true,
                          "ready_for_messaging": true,
                          "missing": []
                        }}
                      ]
                    }}"#
                ),
            )
            .unwrap();
            fs::write(
                identity_dir.join("did.json"),
                serde_json::to_vec_pretty(&json!({
                    "id": did,
                    "service": [{
                        "id": "#attachment",
                        "type": "ANPMessageService",
                        "serviceEndpoint": rpc_endpoint,
                        "serviceDid": service_did,
                        "profiles": ["anp.attachment.v1"],
                        "securityProfiles": ["transport-protected"],
                        "priority": 1
                    }]
                }))
                .unwrap(),
            )
            .unwrap();
        }

        fn client(&self) -> crate::core::ImClient {
            crate::core::ImCore::new(
                crate::ImCoreConfig {
                    service_base_url: crate::ServiceEndpoint::parse("https://example.test")
                        .unwrap(),
                    did_domain: "awiki.test".to_string(),
                    user_service_endpoint: None,
                    message_service_endpoint: None,
                    mail_service_endpoint: None,
                    anp_service_endpoint: None,
                    anp_service_did: None,
                    ca_bundle: None,
                    transport_policy: crate::MessageTransportPolicy::HttpOnly,
                },
                crate::ImCorePaths {
                    identities: crate::paths::IdentityRegistryPaths {
                        identity_root_dir: self.root.join("identities"),
                        registry_path: self.root.join("identities").join("registry.json"),
                        default_identity_path: Some(self.root.join("identities").join("default")),
                    },
                    local_state: crate::paths::LocalStatePaths {
                        sqlite_path: self.root.join("local").join("im.sqlite"),
                    },
                    runtime: crate::paths::RuntimePaths {
                        cache_dir: self.root.join("cache"),
                        temp_dir: self.root.join("tmp"),
                    },
                },
            )
            .unwrap()
            .client(crate::identity::IdentitySelector::LocalAlias(
                "alice".to_string(),
            ))
            .unwrap()
        }
    }

    fn unique_temp_root() -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "im-core-attachment-download-runtime-{}-{nanos}",
            std::process::id()
        ))
    }

    fn assert_no_attachment_temp_files(path: &std::path::Path) {
        let leftovers: Vec<_> = fs::read_dir(path)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|name| name.contains(".awiki-attachment-download-"))
            .collect();
        assert_eq!(leftovers, Vec::<String>::new());
    }

    fn assert_service_error_code(err: crate::ImError, expected: &str) {
        assert!(matches!(
            err,
            crate::ImError::Service { code: Some(code), .. } if code == expected
        ));
    }
}
