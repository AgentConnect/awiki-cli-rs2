use base64::Engine as _;
use napi::bindgen_prelude::Buffer;
use napi_derive::napi;

use crate::error::{SafeError, SafeResult};

#[napi(object)]
pub struct NodeOpenOptions {
    pub state_root: String,
    pub service_base_url: String,
    pub did_domain: String,
    pub user_service_endpoint: Option<String>,
    pub message_service_endpoint: Option<String>,
    pub anp_service_endpoint: Option<String>,
    pub anp_service_did: Option<String>,
    pub operation_timeout_ms: Option<u32>,
    pub sync_timeout_ms: Option<u32>,
    pub external_http_allow_insecure_loopback_for_testing: Option<bool>,
}

impl Clone for NodeOpenOptions {
    fn clone(&self) -> Self {
        Self {
            state_root: self.state_root.clone(),
            service_base_url: self.service_base_url.clone(),
            did_domain: self.did_domain.clone(),
            user_service_endpoint: self.user_service_endpoint.clone(),
            message_service_endpoint: self.message_service_endpoint.clone(),
            anp_service_endpoint: self.anp_service_endpoint.clone(),
            anp_service_did: self.anp_service_did.clone(),
            operation_timeout_ms: self.operation_timeout_ms,
            sync_timeout_ms: self.sync_timeout_ms,
            external_http_allow_insecure_loopback_for_testing: self
                .external_http_allow_insecure_loopback_for_testing,
        }
    }
}

#[napi(object)]
pub struct NodeExternalHttpHeader {
    pub name: String,
    pub value: String,
}

#[napi(object)]
pub struct NodeExternalHttpRequest {
    pub url: String,
    pub method: String,
    pub headers: Vec<NodeExternalHttpHeader>,
    pub body: Option<Buffer>,
}

#[napi(object)]
pub struct NodeExternalHttpResponse {
    pub status_code: u32,
    pub headers: Vec<NodeExternalHttpHeader>,
}

pub(crate) fn external_http_headers(
    headers: Vec<NodeExternalHttpHeader>,
) -> SafeResult<Vec<im_core::ExternalHttpHeader>> {
    headers
        .into_iter()
        .map(|header| {
            im_core::ExternalHttpHeader::new(header.name, header.value).map_err(SafeError::from_im)
        })
        .collect()
}

pub(crate) fn external_http_request(
    input: NodeExternalHttpRequest,
) -> SafeResult<im_core::ExternalHttpRequest> {
    im_core::ExternalHttpRequest::new(
        input.url,
        input.method,
        external_http_headers(input.headers)?,
        input.body.map(|body| body.as_ref().to_vec()),
    )
    .map_err(SafeError::from_im)
}

pub(crate) fn external_http_response(
    input: NodeExternalHttpResponse,
) -> SafeResult<im_core::ExternalHttpResponse> {
    let status_code = u16::try_from(input.status_code).map_err(|_| {
        SafeError::new(
            "invalid_input",
            "The external HTTP response is invalid.",
            false,
        )
    })?;
    im_core::ExternalHttpResponse::new(status_code, external_http_headers(input.headers)?)
        .map_err(SafeError::from_im)
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[napi(object)]
pub struct NodeClearLocalDataResult {
    pub cleared: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[napi(object)]
pub struct NodeIdentity {
    pub identity_id: String,
    pub did: String,
    pub handle: Option<String>,
    pub display_name: Option<String>,
    pub is_default: bool,
    /// Unix milliseconds represented as a decimal string.
    pub registered_at_ms: String,
}

#[napi(object)]
pub struct NodeRegistrationInput {
    pub handle: String,
    pub phone: String,
}

#[napi(object)]
pub struct NodeRegistrationWithOtp {
    pub handle: String,
    pub phone: String,
    pub otp: String,
}

#[derive(Debug, Clone)]
#[napi(object)]
pub struct NodeSkillAgentProvisionInput {
    pub operation_id: String,
    pub display_name: String,
    pub controller_identity_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[napi(object)]
pub struct NodeOtpChallenge {
    pub retry_after_seconds: u32,
    /// RFC 3339 UTC timestamp.
    pub retry_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[napi(object)]
pub struct NodePeer {
    pub did: String,
    pub handle: Option<String>,
    pub display_name: Option<String>,
    pub conversation_id: String,
}

#[napi(object)]
pub struct NodePageInput {
    pub cursor: Option<String>,
    pub limit: Option<u32>,
}

#[napi(object)]
pub struct NodeHistoryInput {
    pub conversation_id: String,
    pub cursor: Option<String>,
    pub limit: Option<u32>,
}

#[napi(object)]
pub struct NodeSyncOptions {
    pub reason: Option<String>,
    pub limit: Option<u32>,
    pub timeout_ms: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[napi(object)]
pub struct NodeSyncResult {
    pub status: String,
    pub events_applied: u32,
    pub pages_fetched: u32,
    pub messages_hydrated: u32,
    pub duplicates_skipped: u32,
    pub changed_conversation_ids: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[napi(object)]
pub struct NodePageOfConversations {
    pub items: Vec<NodeConversation>,
    pub next_cursor: Option<String>,
    pub has_more: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[napi(object)]
pub struct NodePageOfMessages {
    pub items: Vec<NodeMessage>,
    pub next_cursor: Option<String>,
    pub has_more: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[napi(object)]
pub struct NodeConversation {
    pub id: String,
    pub kind: String,
    pub peer_did: Option<String>,
    pub peer_handle: Option<String>,
    pub group_did: Option<String>,
    pub title: Option<String>,
    pub participants: Vec<String>,
    pub unread_count: u32,
    pub message_count: u32,
    /// RFC 3339 timestamp.
    pub last_message_at: Option<String>,
    pub last_message: Option<NodeMessage>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[napi(object)]
pub struct NodeMessage {
    pub id: String,
    pub conversation_id: String,
    pub conversation_kind: String,
    pub sender_did: String,
    pub sender_handle: Option<String>,
    pub sender_display_name: Option<String>,
    /// RFC 3339 timestamp.
    pub sent_at: Option<String>,
    pub outgoing: bool,
    pub content: NodeMessageContent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[napi(object)]
pub struct NodeMessageContent {
    /// `text`, `attachment`, `payload`, or `unsupported`.
    pub kind: String,
    pub text: Option<String>,
    pub attachment: Option<NodeAttachment>,
    pub caption: Option<String>,
    pub payload_json: Option<String>,
    pub unsupported_content_type: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[napi(object)]
pub struct NodeAttachment {
    pub id: String,
    pub file_name: String,
    pub mime_type: String,
    /// Decimal byte count; kept as a string to avoid JS integer truncation.
    pub size_bytes: String,
    pub digest_b64u: String,
    pub sha256_hex: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[napi(object)]
pub struct NodeMarkReadResult {
    pub updated_count: u32,
    pub remote_acknowledged: bool,
    pub partial: bool,
    pub fallback_used: bool,
    pub pending_remote_ack: bool,
    pub warnings: Vec<String>,
}

#[napi(object)]
pub struct NodeSendTextInput {
    pub conversation_id: String,
    pub text: String,
    pub markdown: Option<bool>,
    pub client_message_id: Option<String>,
    pub idempotency_key: Option<String>,
}

#[napi(object)]
pub struct NodeSendAttachmentInput {
    pub conversation_id: String,
    pub file_name: String,
    pub mime_type: String,
    pub bytes: Buffer,
    pub caption: Option<String>,
    pub client_message_id: Option<String>,
    pub idempotency_key: Option<String>,
}

#[napi(object)]
pub struct NodeDownloadAttachmentInput {
    pub conversation_id: String,
    pub message_id: String,
    pub attachment_id: Option<String>,
    pub timeout_ms: Option<u32>,
}

#[napi(object)]
pub struct NodeDownload {
    pub attachment: NodeAttachment,
    pub bytes: Buffer,
}

pub(crate) fn identity(
    value: &im_core::identity::IdentitySummary,
    display_name: Option<String>,
    registered_at_ms: String,
) -> NodeIdentity {
    NodeIdentity {
        identity_id: value.id.as_str().to_owned(),
        did: value.did.as_str().to_owned(),
        handle: value
            .handle
            .as_ref()
            .map(|handle| handle.as_str().to_owned()),
        display_name: display_name.or_else(|| value.display_name.clone()),
        is_default: value.is_default,
        registered_at_ms,
    }
}

pub(crate) fn peer(value: im_core::directory::DirectoryResolution) -> NodePeer {
    NodePeer {
        did: value.did.as_str().to_owned(),
        handle: value.handle.map(|handle| handle.as_str().to_owned()),
        display_name: value.profile.and_then(|profile| profile.display_name),
        conversation_id: value.conversation_id,
    }
}

pub(crate) fn sync_result(value: im_core::messages::MessageSyncOutcome) -> NodeSyncResult {
    NodeSyncResult {
        status: sync_status(value.status).to_owned(),
        events_applied: value.events_applied,
        pages_fetched: value.pages_fetched,
        messages_hydrated: value.messages_hydrated,
        duplicates_skipped: value.duplicates_skipped,
        changed_conversation_ids: value.changed_conversation_ids,
        warnings: value.warnings,
    }
}

pub(crate) fn conversations(
    page: im_core::ids::Page<im_core::messages::Conversation>,
    owner_did: &str,
) -> SafeResult<NodePageOfConversations> {
    Ok(NodePageOfConversations {
        items: page
            .items
            .into_iter()
            .map(|value| conversation(value, owner_did))
            .collect::<SafeResult<Vec<_>>>()?,
        next_cursor: page.next_cursor.map(|cursor| cursor.as_str().to_owned()),
        has_more: page.has_more,
    })
}

pub(crate) fn messages(
    page: im_core::ids::Page<im_core::messages::Message>,
    conversation_id: &str,
) -> SafeResult<NodePageOfMessages> {
    Ok(NodePageOfMessages {
        items: page
            .items
            .into_iter()
            .map(|value| message(value, Some(conversation_id)))
            .collect::<SafeResult<Vec<_>>>()?,
        next_cursor: page.next_cursor.map(|cursor| cursor.as_str().to_owned()),
        has_more: page.has_more,
    })
}

pub(crate) fn sent_message(
    value: im_core::messages::Message,
    conversation_id: &str,
) -> SafeResult<NodeMessage> {
    message(value, Some(conversation_id))
}

pub(crate) fn uploaded_attachment(
    value: im_core::attachments::AttachmentSendResult,
    conversation_id: &str,
) -> SafeResult<NodeMessage> {
    let mut result = message(value.message.message, Some(conversation_id))?;
    result.content = NodeMessageContent {
        kind: "attachment".to_owned(),
        text: None,
        attachment: Some(NodeAttachment {
            id: value.attachment.attachment_id,
            file_name: value.attachment.filename,
            mime_type: value.attachment.mime_type,
            size_bytes: value.attachment.size,
            sha256_hex: digest_hex(&value.attachment.digest_b64u),
            digest_b64u: value.attachment.digest_b64u,
        }),
        caption: value
            .manifest
            .get("caption")
            .and_then(serde_json::Value::as_str)
            .map(ToOwned::to_owned),
        payload_json: None,
        unsupported_content_type: None,
    };
    Ok(result)
}

pub(crate) fn downloaded_attachment(
    value: im_core::attachments::DownloadedAttachment,
) -> SafeResult<NodeDownload> {
    let selection = value.selection;
    let attachment = NodeAttachment {
        id: selection
            .as_ref()
            .map(|item| item.attachment_id.clone())
            .unwrap_or(value.attachment_id),
        file_name: selection
            .as_ref()
            .map(|item| item.filename.clone())
            .or(value.filename)
            .unwrap_or_default(),
        mime_type: selection
            .as_ref()
            .map(|item| item.mime_type.clone())
            .or(value.mime_type)
            .unwrap_or_else(|| "application/octet-stream".to_owned()),
        size_bytes: selection
            .as_ref()
            .map(|item| item.size.clone())
            .or_else(|| value.size_bytes.map(|size| size.to_string()))
            .unwrap_or_else(|| "0".to_owned()),
        digest_b64u: selection
            .as_ref()
            .map(|item| item.digest_b64u.clone())
            .unwrap_or_default(),
        sha256_hex: selection
            .as_ref()
            .and_then(|item| digest_hex(&item.digest_b64u)),
    };
    let bytes = match value.destination {
        im_core::attachments::DownloadedAttachmentDestination::Memory(bytes) => bytes.into(),
        im_core::attachments::DownloadedAttachmentDestination::LocalFile(_) => {
            return Err(SafeError::internal());
        }
    };
    Ok(NodeDownload { attachment, bytes })
}

fn conversation(
    value: im_core::messages::Conversation,
    owner_did: &str,
) -> SafeResult<NodeConversation> {
    if !conversation_is_projectable(&value.conversation_id, value.resolution_state) {
        return Err(SafeError::new(
            "conversation_unresolved",
            "The IM conversation route is unresolved.",
            false,
        ));
    }
    let id = value.conversation_id;
    let kind = if value.canonical_group_did.is_some() || id.starts_with("group:") {
        "group"
    } else {
        "direct"
    };
    let participants = value
        .participants
        .iter()
        .map(|participant| participant.as_str().to_owned())
        .collect::<Vec<_>>();
    let peer_did = (kind == "direct")
        .then(|| {
            participants
                .iter()
                .find(|participant| participant.as_str() != owner_did)
                .cloned()
                .or_else(|| direct_peer_from_message(value.last_message.as_ref(), owner_did))
        })
        .flatten();
    let peer_handle = (kind == "direct")
        .then(|| {
            value
                .last_message
                .as_ref()
                .and_then(|message| message_attribute(message, "peer_full_handle"))
        })
        .flatten();
    let last_message = value
        .last_message
        .map(|item| message(item, Some(&id)))
        .transpose()?;
    Ok(NodeConversation {
        id,
        kind: kind.to_owned(),
        peer_did,
        peer_handle,
        group_did: value.canonical_group_did,
        title: value.title,
        participants,
        unread_count: value.unread_count,
        message_count: value.message_count,
        last_message_at: value.last_message_at,
        last_message,
    })
}

fn conversation_is_projectable(
    conversation_id: &str,
    resolution_state: im_core::messages::ConversationResolutionState,
) -> bool {
    resolution_state == im_core::messages::ConversationResolutionState::Resolved
        || conversation_id.starts_with("dm:")
}

fn message(
    value: im_core::messages::Message,
    conversation_id: Option<&str>,
) -> SafeResult<NodeMessage> {
    let canonical_conversation_id = value
        .metadata
        .conversation_identity
        .as_ref()
        .map(|identity| identity.conversation_id.clone())
        .or_else(|| conversation_id.map(ToOwned::to_owned))
        .ok_or_else(|| {
            SafeError::new(
                "conversation_unresolved",
                "The IM message has no canonical conversation route.",
                false,
            )
        })?;
    let conversation_kind = if canonical_conversation_id.starts_with("group:") {
        "group"
    } else {
        "direct"
    };
    let sender_handle = message_attribute(&value, "sender_handle")
        .or_else(|| message_attribute(&value, "peer_full_handle"));
    let sender_display_name = message_attribute(&value, "sender_display_name");
    Ok(NodeMessage {
        id: value.id.as_str().to_owned(),
        conversation_id: canonical_conversation_id,
        conversation_kind: conversation_kind.to_owned(),
        sender_did: value.sender.as_str().to_owned(),
        sender_handle,
        sender_display_name,
        sent_at: value.sent_at.or(value.received_at),
        outgoing: value.direction == im_core::messages::MessageDirection::Outgoing,
        content: message_content(value.body, value.metadata.content_type.as_deref()),
    })
}

fn message_content(
    body: im_core::messages::MessageBodyView,
    content_type: Option<&str>,
) -> NodeMessageContent {
    match body {
        im_core::messages::MessageBodyView::Text { text, .. } => NodeMessageContent {
            kind: "text".to_owned(),
            text: Some(text),
            attachment: None,
            caption: None,
            payload_json: None,
            unsupported_content_type: None,
        },
        im_core::messages::MessageBodyView::Payload { payload }
            if content_type == Some(im_core::attachments::attachment_manifest_content_type()) =>
        {
            match im_core::attachments::parse_attachment_manifest(&payload) {
                Ok(manifest) => {
                    let selected = manifest
                        .attachments
                        .iter()
                        .find(|attachment| {
                            attachment.attachment_id == manifest.primary_attachment_id
                        })
                        .or_else(|| manifest.attachments.first());
                    match selected {
                        Some(attachment) => NodeMessageContent {
                            kind: "attachment".to_owned(),
                            text: None,
                            attachment: Some(NodeAttachment {
                                id: attachment.attachment_id.clone(),
                                file_name: attachment.filename.clone(),
                                mime_type: attachment.mime_type.clone(),
                                size_bytes: attachment.size.clone(),
                                digest_b64u: attachment.digest_b64u.clone(),
                                sha256_hex: digest_hex(&attachment.digest_b64u),
                            }),
                            caption: manifest.caption,
                            payload_json: None,
                            unsupported_content_type: None,
                        },
                        None => payload_content(payload),
                    }
                }
                Err(_) => payload_content(payload),
            }
        }
        im_core::messages::MessageBodyView::Payload { payload } => payload_content(payload),
        im_core::messages::MessageBodyView::Unsupported { content_type } => NodeMessageContent {
            kind: "unsupported".to_owned(),
            text: None,
            attachment: None,
            caption: None,
            payload_json: None,
            unsupported_content_type: content_type,
        },
    }
}

fn payload_content(payload: serde_json::Value) -> NodeMessageContent {
    NodeMessageContent {
        kind: "payload".to_owned(),
        text: None,
        attachment: None,
        caption: None,
        payload_json: Some(payload.to_string()),
        unsupported_content_type: None,
    }
}

fn direct_peer_from_message(
    message: Option<&im_core::messages::Message>,
    owner_did: &str,
) -> Option<String> {
    let message = message?;
    if message.sender.as_str() != owner_did {
        return Some(message.sender.as_str().to_owned());
    }
    message
        .receiver
        .as_ref()
        .map(|receiver| receiver.as_str().to_owned())
}

fn message_attribute(message: &im_core::messages::Message, key: &str) -> Option<String> {
    message
        .metadata
        .attributes
        .iter()
        .find(|attribute| attribute.key == key)
        .map(|attribute| attribute.value.clone())
}

fn sync_status(status: im_core::messages::MessageSyncStatus) -> &'static str {
    match status {
        im_core::messages::MessageSyncStatus::Idle => "idle",
        im_core::messages::MessageSyncStatus::Changed => "changed",
        im_core::messages::MessageSyncStatus::RecoveryRequired => "recovery_required",
        im_core::messages::MessageSyncStatus::RetryableFailure => "retryable_failure",
        im_core::messages::MessageSyncStatus::AuthRevoked => "auth_revoked",
    }
}

fn digest_hex(value: &str) -> Option<String> {
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(value)
        .ok()?;
    (bytes.len() == 32).then(|| {
        let mut output = String::with_capacity(64);
        for byte in bytes {
            use std::fmt::Write as _;
            let _ = write!(output, "{byte:02x}");
        }
        output
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_peer_and_sync_dtos_have_stable_golden_shapes() {
        let identity_value = im_core::identity::IdentitySummary {
            id: im_core::ids::IdentityId::parse("identity-1").unwrap(),
            did: im_core::ids::Did::parse("did:example:alice").unwrap(),
            handle: Some(im_core::ids::Handle::parse("alice.awiki.test", "").unwrap()),
            display_name: Some("stale name".to_owned()),
            local_alias: Some("default".to_owned()),
            device_id: None,
            is_default: true,
            readiness: im_core::identity::IdentityReadiness {
                ready_for_auth: true,
                ready_for_messaging: true,
                missing: Vec::new(),
            },
        };
        assert_eq!(
            identity(
                &identity_value,
                Some("Alice".to_owned()),
                "1700000000000".to_owned()
            ),
            NodeIdentity {
                identity_id: "identity-1".to_owned(),
                did: "did:example:alice".to_owned(),
                handle: Some("alice.awiki.test".to_owned()),
                display_name: Some("Alice".to_owned()),
                is_default: true,
                registered_at_ms: "1700000000000".to_owned(),
            }
        );

        let mut profile =
            im_core::identity::Profile::new(im_core::ids::Did::parse("did:example:bob").unwrap());
        profile.display_name = Some("Bob".to_owned());
        assert_eq!(
            peer(im_core::directory::DirectoryResolution {
                input: "bob.awiki.test".to_owned(),
                did: im_core::ids::Did::parse("did:example:bob").unwrap(),
                handle: Some(im_core::ids::Handle::parse("bob.awiki.test", "").unwrap()),
                conversation_id: "dm:peer-scope:user-bob".to_owned(),
                profile: Some(profile),
                warnings: vec!["not exposed".to_owned()],
            }),
            NodePeer {
                did: "did:example:bob".to_owned(),
                handle: Some("bob.awiki.test".to_owned()),
                display_name: Some("Bob".to_owned()),
                conversation_id: "dm:peer-scope:user-bob".to_owned(),
            }
        );

        assert_eq!(
            sync_result(im_core::messages::MessageSyncOutcome {
                status: im_core::messages::MessageSyncStatus::Changed,
                events_applied: 2,
                pages_fetched: 1,
                messages_hydrated: 3,
                duplicates_skipped: 4,
                changed_conversation_ids: vec!["group:did:example:group".to_owned()],
                committed_incoming_messages: Vec::new(),
                error_code: None,
                warnings: vec!["safe-warning".to_owned()],
            }),
            NodeSyncResult {
                status: "changed".to_owned(),
                events_applied: 2,
                pages_fetched: 1,
                messages_hydrated: 3,
                duplicates_skipped: 4,
                changed_conversation_ids: vec!["group:did:example:group".to_owned()],
                warnings: vec!["safe-warning".to_owned()],
            }
        );
    }

    #[test]
    fn unresolved_direct_conversations_remain_browser_projectable() {
        assert!(conversation_is_projectable(
            "dm:did:example:alice:did:example:bob",
            im_core::messages::ConversationResolutionState::LegacyUnresolved,
        ));
        assert!(!conversation_is_projectable(
            "group:did:example:group",
            im_core::messages::ConversationResolutionState::LegacyUnresolved,
        ));
    }

    #[test]
    fn message_and_download_dtos_preserve_canonical_ids_and_owned_bytes() {
        let message = im_core::messages::Message {
            id: im_core::ids::MessageId::parse("message-1").unwrap(),
            thread: im_core::messages::ThreadRef::Group(
                im_core::ids::GroupRef::parse("did:example:group").unwrap(),
            ),
            direction: im_core::messages::MessageDirection::Incoming,
            sender: im_core::ids::PeerRef::parse("did:example:bob", "").unwrap(),
            receiver: None,
            group: Some(im_core::ids::GroupRef::parse("did:example:group").unwrap()),
            body: im_core::messages::MessageBodyView::Text {
                text: "hello".to_owned(),
                kind: im_core::messages::MessageKind::Text,
            },
            sent_at: Some("2026-08-15T12:00:00Z".to_owned()),
            received_at: None,
            metadata: im_core::messages::MessageMetadata::default(),
        };
        let mapped = sent_message(message, "group:did:example:group").unwrap();
        assert_eq!(mapped.id, "message-1");
        assert_eq!(mapped.conversation_id, "group:did:example:group");
        assert_eq!(mapped.conversation_kind, "group");
        assert_eq!(mapped.content.kind, "text");
        assert_eq!(mapped.content.text.as_deref(), Some("hello"));

        let expected = (0..(256 * 1024 + 17))
            .map(|index| (index % 251) as u8)
            .collect::<Vec<_>>();
        let download = downloaded_attachment(im_core::attachments::DownloadedAttachment {
            attachment_id: "attachment-1".to_owned(),
            filename: Some("bytes.bin".to_owned()),
            mime_type: Some("application/octet-stream".to_owned()),
            size_bytes: Some(expected.len() as u64),
            destination: im_core::attachments::DownloadedAttachmentDestination::Memory(
                expected.clone(),
            ),
            selection: None,
            warnings: Vec::new(),
        })
        .unwrap();
        assert_eq!(download.attachment.id, "attachment-1");
        assert_eq!(download.attachment.size_bytes, expected.len().to_string());
        assert_eq!(download.bytes.to_vec(), expected);
    }

    #[test]
    fn attachment_digest_maps_without_base64_json_round_trip() {
        let digest = "LPJNul-wow4m6DsqxbninhsWHlwfp0JecwQzYpOLmCQ";
        assert_eq!(
            digest_hex(digest).as_deref(),
            Some("2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824")
        );
    }

    #[test]
    fn sync_status_is_a_closed_stable_string_union() {
        assert_eq!(
            [
                im_core::messages::MessageSyncStatus::Idle,
                im_core::messages::MessageSyncStatus::Changed,
                im_core::messages::MessageSyncStatus::RecoveryRequired,
                im_core::messages::MessageSyncStatus::RetryableFailure,
                im_core::messages::MessageSyncStatus::AuthRevoked,
            ]
            .map(sync_status),
            [
                "idle",
                "changed",
                "recovery_required",
                "retryable_failure",
                "auth_revoked"
            ]
        );
    }

    #[test]
    fn owned_buffer_keeps_large_multichunk_bytes_exact() {
        let expected = (0..(256 * 1024 + 17))
            .map(|index| (index % 251) as u8)
            .collect::<Vec<_>>();
        let buffer: napi::bindgen_prelude::Buffer = expected.clone().into();
        assert_eq!(buffer.to_vec(), expected);
    }
}
