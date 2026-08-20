//! Migration-only attachment helpers for `awiki-cli` wrappers.

use std::path::Path;

use serde_json::Value;

pub use crate::attachments::manifest::{
    AttachmentDescriptor, AttachmentManifest, PreparedAttachment,
};
pub use crate::attachments::selection::AttachmentSelection;
pub use crate::internal::discovery::attachment::DiscoveredAttachmentService;
pub use crate::internal::wire::attachment::{
    AttachmentCommitObjectResult, AttachmentCreateSlotResult, AttachmentDownloadTicketResult,
};

#[derive(Debug, Clone, PartialEq)]
pub struct AttachmentSendCompatResult {
    pub sdk_result: crate::messages::SendMessageResult,
    pub target_kind: &'static str,
    pub target_did: String,
    pub prepared: PreparedAttachment,
    pub slot: AttachmentCreateSlotResult,
    pub manifest: Value,
    pub raw: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AttachmentDownloadCompatResult {
    pub sdk_result: crate::attachments::DownloadedAttachment,
    pub selection: AttachmentSelection,
}

pub const ERR_ATTACHMENT_NOT_FOUND: &str = crate::attachments::selection::ERR_ATTACHMENT_NOT_FOUND;
pub const ERR_ATTACHMENT_ID_REQUIRED: &str =
    crate::attachments::selection::ERR_ATTACHMENT_ID_REQUIRED;
pub const ERR_ATTACHMENT_MESSAGE_INVALID: &str =
    crate::attachments::selection::ERR_ATTACHMENT_MESSAGE_INVALID;

#[doc(hidden)]
pub fn attachment_manifest_content_type() -> &'static str {
    crate::attachments::manifest::attachment_manifest_content_type()
}

#[doc(hidden)]
pub fn prepare_attachment_payload_from_path(
    path: &Path,
    mime_override: &str,
    payload: Vec<u8>,
) -> crate::ImResult<PreparedAttachment> {
    crate::attachments::manifest::prepare_attachment_payload_from_path(path, mime_override, payload)
}

#[doc(hidden)]
pub fn prepare_attachment_payload(
    filename: &str,
    mime_override: &str,
    payload: Vec<u8>,
) -> crate::ImResult<PreparedAttachment> {
    crate::attachments::manifest::prepare_attachment_payload(filename, mime_override, payload)
}

#[doc(hidden)]
pub fn build_attachment_manifest(descriptor: &AttachmentDescriptor, caption: &str) -> Value {
    crate::attachments::manifest::build_attachment_manifest(descriptor, caption)
        .expect("attachment manifest without mention payload should be valid")
}

#[doc(hidden)]
pub fn build_attachment_manifest_internal(
    descriptor: &AttachmentDescriptor,
    caption: &str,
) -> Value {
    crate::attachments::manifest::build_attachment_manifest_internal(descriptor, caption)
        .expect("attachment manifest without mention payload should be valid")
}

#[doc(hidden)]
pub fn redact_attachment_manifest(manifest: &Value) -> Value {
    crate::attachments::manifest::redact_attachment_manifest(manifest)
}

#[doc(hidden)]
pub fn parse_attachment_manifest(value: &Value) -> crate::ImResult<AttachmentManifest> {
    crate::attachments::manifest::parse_attachment_manifest(value)
}

#[doc(hidden)]
pub fn build_attachment_grant_ref(descriptor: &AttachmentDescriptor) -> crate::ImResult<Value> {
    crate::attachments::manifest::build_attachment_grant_ref(descriptor)
}

pub fn manifest_content_string(manifest: &Value) -> String {
    crate::attachments::manifest::manifest_content_string(manifest)
}

#[doc(hidden)]
pub fn find_attachment_selection(
    messages: &[Value],
    requested_message_id: &str,
    requested_attachment_id: &str,
) -> crate::ImResult<AttachmentSelection> {
    crate::attachments::selection::find_attachment_selection(
        messages,
        requested_message_id,
        requested_attachment_id,
    )
}

#[doc(hidden)]
pub fn find_attachment_selection_with_paging<F>(
    fetch_page: F,
    requested_message_id: &str,
    requested_attachment_id: &str,
) -> crate::ImResult<AttachmentSelection>
where
    F: FnMut(i64) -> crate::ImResult<(Vec<Value>, bool)>,
{
    crate::attachments::selection::find_attachment_selection_with_paging(
        fetch_page,
        requested_message_id,
        requested_attachment_id,
    )
}

#[doc(hidden)]
pub fn select_attachment_rpc_service_from_document(
    sender_did: &str,
    document: &Value,
) -> crate::ImResult<DiscoveredAttachmentService> {
    crate::internal::discovery::attachment::select_attachment_rpc_service_from_document(
        sender_did, document,
    )
}

#[doc(hidden)]
pub fn send_attachment_with_details(
    client: &crate::core::ImClient,
    target: crate::messages::MessageTarget,
    request: crate::attachments::AttachmentSendRequest,
    resolved_target_did: Option<String>,
) -> crate::ImResult<AttachmentSendCompatResult> {
    let result = crate::internal::attachment_runtime::upload::AttachmentUploadRuntime::new(
        client,
        crate::internal::auth::session::FileSessionProvider::new(client),
        crate::internal::transport::CoreHttpTransport::new(client),
    )
    .send(
        crate::internal::attachment_runtime::upload::AttachmentSendInput {
            target,
            request,
            resolved_target_did,
            client_message_id: None,
            credentials: None,
        },
    )?;
    Ok(AttachmentSendCompatResult {
        sdk_result: result.sdk_result,
        target_kind: result.target_kind,
        target_did: result.target_did,
        prepared: result.prepared,
        slot: result.slot,
        manifest: result.manifest,
        raw: result.raw,
    })
}

#[doc(hidden)]
pub fn download_attachment_with_details(
    client: &crate::core::ImClient,
    request: crate::attachments::DownloadAttachmentRequest,
    resolved_peer_did: Option<String>,
) -> crate::ImResult<AttachmentDownloadCompatResult> {
    let result = crate::internal::attachment_runtime::download::AttachmentDownloadRuntime::new(
        client,
        crate::internal::auth::session::FileSessionProvider::new(client),
        crate::internal::transport::CoreHttpTransport::new(client),
    )
    .download(
        crate::internal::attachment_runtime::download::AttachmentDownloadInput {
            request,
            resolved_peer_did,
        },
    )?;
    Ok(AttachmentDownloadCompatResult {
        sdk_result: result.sdk_result,
        selection: result.selection,
    })
}

#[cfg(feature = "internal-test-helpers")]
#[doc(hidden)]
pub async fn build_download_ticket_rpc_params_for_system_test(
    client: &crate::core::ImClient,
    request: crate::attachments::DownloadAttachmentRequest,
    resolved_peer_did: Option<String>,
) -> crate::ImResult<Value> {
    crate::internal::attachment_runtime::download::AttachmentDownloadRuntime::new(
        client,
        crate::internal::auth::session::FileSessionProvider::new(client),
        crate::internal::transport::CoreHttpTransport::new(client),
    )
    .build_download_ticket_rpc_params_for_system_test(
        crate::internal::attachment_runtime::download::AttachmentDownloadInput {
            request,
            resolved_peer_did,
        },
    )
    .await
}

#[derive(Debug, Clone, PartialEq)]
pub struct AttachmentSigningIdentity {
    pub identity_name: String,
    pub did: String,
    pub did_document: Option<Value>,
    pub key1_private_pem: String,
}

#[doc(hidden)]
pub fn build_attachment_create_slot_rpc_params(
    sender_did: &str,
    service_did: &str,
    target_kind: &str,
    target_did: &str,
    prepared: &PreparedAttachment,
) -> crate::ImResult<Value> {
    crate::internal::wire::attachment::build_attachment_create_slot_rpc_params(
        sender_did,
        service_did,
        target_kind,
        target_did,
        prepared,
    )
}

#[doc(hidden)]
pub fn build_attachment_create_slot_rpc_params_with_security_profile(
    sender_did: &str,
    service_did: &str,
    target_kind: &str,
    target_did: &str,
    message_security_profile: &str,
    prepared: &PreparedAttachment,
) -> crate::ImResult<Value> {
    crate::internal::wire::attachment::build_attachment_create_slot_rpc_params_with_security_profile(
        sender_did,
        service_did,
        target_kind,
        target_did,
        message_security_profile,
        prepared,
    )
}

#[doc(hidden)]
pub fn build_attachment_commit_object_rpc_params(
    sender_did: &str,
    service_did: &str,
    prepared: &PreparedAttachment,
    slot: &AttachmentCreateSlotResult,
) -> crate::ImResult<Value> {
    crate::internal::wire::attachment::build_attachment_commit_object_rpc_params(
        sender_did,
        service_did,
        prepared,
        slot,
    )
}

#[doc(hidden)]
pub fn build_attachment_download_ticket_rpc_params(
    requester_did: &str,
    service_did: &str,
    sender_did: &str,
    message_id: &str,
    group_did: &str,
    selection: &AttachmentSelection,
) -> crate::ImResult<Value> {
    crate::internal::wire::attachment::build_attachment_download_ticket_rpc_params(
        requester_did,
        service_did,
        sender_did,
        message_id,
        group_did,
        selection,
    )
}

#[doc(hidden)]
pub fn build_direct_attachment_send_rpc_params(
    identity: &AttachmentSigningIdentity,
    target_did: &str,
    manifest: Value,
) -> crate::ImResult<Value> {
    crate::internal::wire::attachment::build_direct_attachment_send_rpc_params(
        &internal_signing_identity(identity),
        target_did,
        manifest,
        None,
        None,
    )
}

#[doc(hidden)]
pub fn build_group_attachment_send_rpc_params(
    identity: &AttachmentSigningIdentity,
    group_did: &str,
    manifest: Value,
) -> crate::ImResult<Value> {
    crate::internal::wire::attachment::build_group_attachment_send_rpc_params(
        &internal_signing_identity(identity),
        group_did,
        manifest,
        None,
        None,
    )
}

fn internal_signing_identity(
    identity: &AttachmentSigningIdentity,
) -> crate::internal::wire::attachment::AttachmentSigningIdentity {
    crate::internal::wire::attachment::AttachmentSigningIdentity {
        identity_name: identity.identity_name.clone(),
        did: identity.did.clone(),
        did_document: identity.did_document.clone(),
        signer: crate::internal::proof::origin::OriginProofSigner::PrivateKeyPem(
            identity.key1_private_pem.clone(),
        ),
        verification_method: None,
    }
}
