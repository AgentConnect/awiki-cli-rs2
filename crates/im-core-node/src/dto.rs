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
    pub mail_service_endpoint: Option<String>,
    pub anp_service_endpoint: Option<String>,
    pub anp_service_did: Option<String>,
    pub operation_timeout_ms: Option<u32>,
    pub sync_timeout_ms: Option<u32>,
    pub multi_device_handle_recovery_enabled: Option<bool>,
    pub multi_device_audience: Option<String>,
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
            mail_service_endpoint: self.mail_service_endpoint.clone(),
            anp_service_endpoint: self.anp_service_endpoint.clone(),
            anp_service_did: self.anp_service_did.clone(),
            operation_timeout_ms: self.operation_timeout_ms,
            sync_timeout_ms: self.sync_timeout_ms,
            multi_device_handle_recovery_enabled: self.multi_device_handle_recovery_enabled,
            multi_device_audience: self.multi_device_audience.clone(),
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
    /// Unix milliseconds represented as a decimal string.
    pub registered_at_ms: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[napi(object)]
pub struct NodeExistingHandleRegistration {
    /// Opaque Core preparation identifier. Callers must not parse or persist it.
    pub continuation_id: String,
    pub full_handle: String,
    pub expected_did: String,
    /// `ordinary` or `handle_recovery_rebind`.
    pub mode: String,
    pub requires_user_presence: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[napi(object)]
pub struct NodeRegistrationOutcome {
    /// `registered` or `existing_handle`.
    pub status: String,
    pub identity: Option<NodeIdentity>,
    pub existing_handle: Option<NodeExistingHandleRegistration>,
    pub warnings: Vec<String>,
}

#[napi(object)]
pub struct NodePreparedRegistrationJoinInput {
    pub continuation_id: String,
    pub operation_id: String,
    pub ttl_seconds: Option<u32>,
}

#[napi(object)]
pub struct NodePreparedRegistrationJoinResumeInput {
    pub join_session_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[napi(object)]
pub struct NodePreparedRegistrationJoinProgress {
    pub join_session_id: String,
    pub did: String,
    pub local_phase: String,
    pub remote_state: String,
    pub completed: bool,
    pub identity: Option<NodeIdentity>,
}

pub(crate) fn existing_handle_registration_outcome(
    preparation: im_core::identity::HandleRegistrationJoinRequiredPreparation,
    warnings: Vec<String>,
) -> NodeRegistrationOutcome {
    let mode = match preparation.mode {
        im_core::identity::HandleRegistrationJoinMode::Ordinary => "ordinary",
        im_core::identity::HandleRegistrationJoinMode::HandleRecoveryRebind => {
            "handle_recovery_rebind"
        }
    };
    NodeRegistrationOutcome {
        status: "existing_handle".to_owned(),
        identity: None,
        existing_handle: Some(NodeExistingHandleRegistration {
            continuation_id: preparation.preparation_id,
            full_handle: preparation.full_handle.as_str().to_owned(),
            expected_did: preparation.expected_did.as_str().to_owned(),
            mode: mode.to_owned(),
            requires_user_presence: preparation.requires_user_presence,
        }),
        warnings,
    }
}

pub(crate) fn prepared_registration_join_progress(
    value: im_core::identity::AuthorizedJoinActivationProgress,
    identity: Option<NodeIdentity>,
) -> NodePreparedRegistrationJoinProgress {
    let completed = value.join.session.phase == im_core::identity::DeviceJoinLocalPhase::Authorized
        && value.join.remote_state == im_core::identity::DeviceJoinRemoteState::Consumed;
    NodePreparedRegistrationJoinProgress {
        join_session_id: value.join.session.join_session_id,
        did: value.join.session.did.as_str().to_owned(),
        local_phase: device_join_local_phase(value.join.session.phase).to_owned(),
        remote_state: device_join_remote_state(value.join.remote_state).to_owned(),
        completed,
        identity,
    }
}

fn device_join_local_phase(value: im_core::identity::DeviceJoinLocalPhase) -> &'static str {
    use im_core::identity::DeviceJoinLocalPhase::*;
    match value {
        Pending => "pending",
        ChallengePrepared => "challenge_prepared",
        ResponsePrepared => "response_prepared",
        ResponseVerified => "response_verified",
        ApprovalPrepared => "approval_prepared",
        Authorized => "authorized",
        Cancelled => "cancelled",
        Expired => "expired",
    }
}

fn device_join_remote_state(value: im_core::identity::DeviceJoinRemoteState) -> &'static str {
    use im_core::identity::DeviceJoinRemoteState::*;
    match value {
        Pending => "pending",
        ChallengeSent => "challenge_sent",
        ResponseVerified => "response_verified",
        Consumed => "consumed",
        Cancelled => "cancelled",
        Rejected => "rejected",
        Expired => "expired",
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[napi(object)]
pub struct NodeProfile {
    pub did: String,
    pub handle: Option<String>,
    pub display_name: Option<String>,
    pub bio: Option<String>,
    pub tags: Vec<String>,
    pub updated_at: Option<String>,
}

#[napi(object)]
pub struct NodeUpdateProfileInput {
    pub display_name: String,
    pub bio: Option<String>,
    pub tags: Option<Vec<String>>,
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

#[derive(Debug, Clone, PartialEq, Eq)]
#[napi(object)]
pub struct NodeDisplayProfileBatchInput {
    pub peers: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[napi(object)]
pub struct NodeDisplayProfile {
    pub did: Option<String>,
    pub handle: Option<String>,
    pub display_name: Option<String>,
    pub cache_hit: bool,
    pub is_stale: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[napi(object)]
pub struct NodeCreateGroupInput {
    pub name: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[napi(object)]
pub struct NodeGroup {
    pub did: String,
    pub conversation_id: String,
    pub title: String,
    pub description: Option<String>,
    pub member_count: Option<u32>,
    pub my_role: Option<String>,
    pub membership_status: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[napi(object)]
pub struct NodeAddGroupMemberInput {
    pub group_did: String,
    pub member: String,
    pub role: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[napi(object)]
pub struct NodeGroupMember {
    pub did: String,
    pub handle: Option<String>,
}

#[napi(object)]
pub struct NodeGroupInput {
    pub group_did: String,
}

#[napi(object)]
pub struct NodeGroupMembersInput {
    pub group_did: String,
    pub cursor: Option<String>,
    pub limit: Option<u32>,
}

#[napi(object)]
pub struct NodeRemoveGroupMemberInput {
    pub group_did: String,
    pub member: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[napi(object)]
pub struct NodeGroupMemberRecord {
    pub membership_id: Option<String>,
    pub peer_persona_id: Option<String>,
    pub did: Option<String>,
    pub credential_did: Option<String>,
    pub handle: Option<String>,
    pub role: Option<String>,
    pub status: Option<String>,
    pub joined_at: Option<String>,
    pub subject_type: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[napi(object)]
pub struct NodeGroupMemberPage {
    pub items: Vec<NodeGroupMemberRecord>,
    pub total: Option<u32>,
    pub next_cursor: Option<String>,
    pub has_more: bool,
    pub page_group: Option<String>,
    pub group_state_version: Option<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[napi(object)]
pub struct NodeGroupRebindRecoverySummary {
    pub processed: u32,
    pub completed: u32,
    pub pending: u32,
    pub blocked: u32,
    pub send_paused_groups: Vec<String>,
    pub items: Vec<NodeGroupRebindRecoveryItem>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[napi(object)]
pub struct NodeGroupRebindRecoveryItem {
    pub group_did: String,
    pub layer: String,
    pub phase: String,
    pub blocked: bool,
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
pub struct NodeRealtimeOptions {
    /// Bounded native event buffer. Defaults to 128.
    pub event_buffer: Option<u32>,
    /// Initial exponential reconnect delay. Defaults to 1 second.
    pub reconnect_base_delay_ms: Option<u32>,
    /// Maximum exponential reconnect delay. Defaults to 30 seconds.
    pub reconnect_max_delay_ms: Option<u32>,
    /// When absent, reconnect attempts are not artificially capped.
    pub reconnect_max_attempts: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[napi(object)]
pub struct NodeRealtimeStatus {
    pub connected: bool,
    /// `disconnected`, `connecting`, `connected`, `reconnecting`, or `closed`.
    pub state: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[napi(object)]
pub struct NodeRealtimeEvent {
    /// `connection_state_changed` or `sync_required`.
    pub kind: String,
    pub state: Option<String>,
    /// High-level scheduling cause. Never a wire event type or checkpoint.
    pub cause: Option<String>,
    pub dirty: Option<bool>,
    pub gap_detected: Option<bool>,
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
pub struct NodePageOfGroups {
    pub items: Vec<NodeGroup>,
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
pub struct NodeSendPayloadInput {
    pub conversation_id: String,
    pub payload_json: String,
    pub client_message_id: Option<String>,
    pub idempotency_key: Option<String>,
}

#[napi(object)]
pub struct NodeHandleRecoveryOtpInput {
    pub full_handle: String,
    pub phone: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[napi(object)]
pub struct NodeHandleRecoveryOtpResult {
    pub owner_identity_id: String,
    pub full_handle: String,
    pub operation_id: String,
    pub accepted: bool,
    pub retry_after_seconds: u32,
    pub retry_at: String,
}

#[napi(object)]
pub struct NodeHandleRecoveryPrepareInput {
    pub operation_id: String,
    pub phone: String,
    pub otp: String,
}

#[napi(object)]
pub struct NodeHandleRecoveryOperationInput {
    pub operation_id: String,
}

#[napi(object)]
pub struct NodeHandleRecoveryAttestationResult {
    pub attestation: String,
    pub expires_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[napi(object)]
pub struct NodeHandleRecoveryImpact {
    pub local_ordinary_data_will_migrate: bool,
    pub other_devices_must_rejoin: bool,
    #[napi(js_name = "unsupportedE2eeGroupCount")]
    pub unsupported_e2ee_group_count: u32,
    pub unsupported_did_only_group_count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[napi(object)]
pub struct NodeHandleRecoveryProgress {
    pub operation_id: String,
    pub owner_identity_id: String,
    pub full_handle: String,
    pub previous_did: Option<String>,
    pub current_did: String,
    pub phase: String,
    pub failure_code: Option<String>,
    pub retryable: bool,
    pub impact: NodeHandleRecoveryImpact,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[napi(object)]
pub struct NodeHandleRecoveryOperationSummary {
    pub operation_id: String,
    pub owner_identity_id: String,
    pub full_handle: String,
    pub lifecycle: String,
    pub commit_attempted: bool,
    pub key_state: String,
    pub last_error_code: Option<String>,
    pub created_at: String,
    pub updated_at: String,
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

#[napi(object)]
pub struct NodeMailInboxInput {
    pub folder: Option<String>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
    pub unread_only: Option<bool>,
}

#[napi(object)]
pub struct NodeMarkMailReadInput {
    pub message_ids: Vec<String>,
}

#[napi(object)]
pub struct NodeSendMailAttachmentInput {
    pub file_name: String,
    pub content_type: String,
    pub bytes: Buffer,
}

#[napi(object)]
pub struct NodeSendMailInput {
    pub to: Vec<String>,
    pub cc: Option<Vec<String>>,
    pub subject: String,
    pub body_text: String,
    pub attachments: Option<Vec<NodeSendMailAttachmentInput>>,
}

#[napi(object)]
pub struct NodeDownloadMailAttachmentInput {
    pub message_id: String,
    pub attachment_index: u32,
}

#[napi(object)]
pub struct NodeMailAttachmentDownload {
    pub file_name: String,
    pub content_type: String,
    pub size_bytes: String,
    pub bytes: Buffer,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[napi(object)]
pub struct NodeMailAccount {
    pub mailbox_address: Option<String>,
    pub display_name: Option<String>,
    pub status: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[napi(object)]
pub struct NodeMailMessageSummary {
    pub id: String,
    pub folder: Option<String>,
    pub from: Vec<String>,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub subject: String,
    pub subject_truncated: bool,
    pub preview: Option<String>,
    pub preview_truncated: bool,
    pub received_at: Option<String>,
    pub sent_at: Option<String>,
    pub unread: bool,
    pub has_attachments: bool,
    pub attachment_count: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[napi(object)]
pub struct NodeMailAttachmentMetadata {
    pub index: u32,
    pub file_name: Option<String>,
    pub content_type: Option<String>,
    pub size_bytes: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[napi(object)]
pub struct NodeMailMessage {
    pub summary: NodeMailMessageSummary,
    pub body_text: Option<String>,
    pub body_truncated: bool,
    pub has_html_body: bool,
    pub attachments: Vec<NodeMailAttachmentMetadata>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[napi(object)]
pub struct NodeMailInboxPage {
    pub items: Vec<NodeMailMessageSummary>,
    pub next_offset: Option<u32>,
    pub has_more: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[napi(object)]
pub struct NodeMarkMailReadResult {
    pub updated: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[napi(object)]
pub struct NodeSendMailResult {
    pub accepted: bool,
    pub message_id: Option<String>,
    pub warnings: Vec<String>,
}

const MAIL_SUBJECT_MAX_BYTES: usize = 1_024;
const MAIL_PREVIEW_MAX_BYTES: usize = 4_096;
const MAIL_BODY_MAX_BYTES: usize = 65_536;
const MAIL_ADDRESS_COLLECTION_MAX_ITEMS: usize = 100;
const MAIL_ATTACHMENT_MAX_ITEMS: usize = 100;
const MAIL_ATTACHMENT_FILENAME_MAX_BYTES: usize = 255;
const MAIL_ATTACHMENT_CONTENT_TYPE_MAX_BYTES: usize = 255;
const MAIL_ACCOUNT_DISPLAY_NAME_MAX_BYTES: usize = 512;
const MAIL_ACCOUNT_STATUS_MAX_BYTES: usize = 128;
const MAIL_WARNING_MAX_ITEMS: usize = 100;
const MAIL_WARNING_MAX_BYTES: usize = 1_024;

pub(crate) fn mail_account(value: im_core::email::EmailAccount) -> SafeResult<NodeMailAccount> {
    Ok(NodeMailAccount {
        mailbox_address: value
            .mailbox_address
            .as_ref()
            .map(mail_address)
            .transpose()?,
        display_name: checked_optional_remote_text(
            value.display_name,
            MAIL_ACCOUNT_DISPLAY_NAME_MAX_BYTES,
        )?,
        status: checked_optional_remote_text(value.status, MAIL_ACCOUNT_STATUS_MAX_BYTES)?,
    })
}

pub(crate) fn mail_inbox(
    value: im_core::ids::Page<im_core::email::EmailMessageSummary>,
    offset: u32,
    requested_limit: u32,
) -> SafeResult<NodeMailInboxPage> {
    if value.items.len() > requested_limit as usize || (value.has_more && value.items.is_empty()) {
        return Err(invalid_mail_response());
    }
    let item_count = u32::try_from(value.items.len()).map_err(|_| invalid_mail_response())?;
    let next_offset = if value.has_more {
        Some(
            offset
                .checked_add(item_count)
                .ok_or_else(invalid_mail_response)?,
        )
    } else {
        None
    };
    Ok(NodeMailInboxPage {
        items: value
            .items
            .into_iter()
            .map(mail_summary)
            .collect::<SafeResult<Vec<_>>>()?,
        next_offset,
        has_more: value.has_more,
    })
}

pub(crate) fn mail_message(value: im_core::email::EmailMessage) -> SafeResult<NodeMailMessage> {
    if value.attachments.len() > MAIL_ATTACHMENT_MAX_ITEMS {
        return Err(invalid_mail_response());
    }
    let has_html_body = value.body_html.is_some();
    let (body_text, body_truncated) = truncate_optional_utf8(value.body_text, MAIL_BODY_MAX_BYTES);
    Ok(NodeMailMessage {
        summary: mail_summary(value.summary)?,
        body_text,
        body_truncated,
        has_html_body,
        attachments: value
            .attachments
            .into_iter()
            .map(mail_attachment)
            .collect::<SafeResult<Vec<_>>>()?,
    })
}

pub(crate) fn mark_mail_read_result(
    value: im_core::email::EmailMarkReadResult,
) -> NodeMarkMailReadResult {
    NodeMarkMailReadResult {
        updated: value.updated,
    }
}

pub(crate) fn send_mail_result(
    value: im_core::email::SendEmailResult,
) -> SafeResult<NodeSendMailResult> {
    if value.warnings.len() > MAIL_WARNING_MAX_ITEMS {
        return Err(invalid_mail_response());
    }
    Ok(NodeSendMailResult {
        accepted: value.accepted,
        message_id: value
            .message_id
            .as_ref()
            .map(|id| checked_mail_token(id.as_str(), 2_048))
            .transpose()?,
        warnings: value
            .warnings
            .into_iter()
            .map(|warning| checked_remote_text(warning, MAIL_WARNING_MAX_BYTES))
            .collect::<SafeResult<Vec<_>>>()?,
    })
}

pub(crate) fn mail_attachment_download(
    value: im_core::email::EmailAttachmentContent,
) -> SafeResult<NodeMailAttachmentDownload> {
    let size = value.size.ok_or_else(invalid_mail_response)?;
    if !valid_mail_file_name(&value.filename)
        || !valid_mail_content_type(&value.content_type)
        || u64::try_from(value.bytes.len()).ok() != Some(size)
    {
        return Err(invalid_mail_response());
    }
    Ok(NodeMailAttachmentDownload {
        file_name: value.filename,
        content_type: value.content_type,
        size_bytes: size.to_string(),
        bytes: Buffer::from(value.bytes),
    })
}

fn mail_summary(value: im_core::email::EmailMessageSummary) -> SafeResult<NodeMailMessageSummary> {
    let (subject, subject_truncated) = truncate_utf8(value.subject, MAIL_SUBJECT_MAX_BYTES);
    let (preview, preview_truncated) =
        truncate_optional_utf8(value.preview, MAIL_PREVIEW_MAX_BYTES);
    Ok(NodeMailMessageSummary {
        id: checked_mail_token(value.id.as_str(), 2_048)?,
        folder: value
            .folder
            .as_ref()
            .map(|folder| checked_mail_token(folder.as_str(), 64))
            .transpose()?,
        from: mail_addresses(value.from)?,
        to: mail_addresses(value.to)?,
        cc: mail_addresses(value.cc)?,
        subject,
        subject_truncated,
        preview,
        preview_truncated,
        received_at: checked_timestamp(value.received_at)?,
        sent_at: checked_timestamp(value.sent_at)?,
        unread: value.unread,
        has_attachments: value.has_attachments,
        attachment_count: value.attachment_count,
    })
}

fn mail_attachment(
    value: im_core::email::EmailAttachmentMetadata,
) -> SafeResult<NodeMailAttachmentMetadata> {
    Ok(NodeMailAttachmentMetadata {
        index: value.index,
        file_name: checked_optional_remote_text(
            value.filename,
            MAIL_ATTACHMENT_FILENAME_MAX_BYTES,
        )?,
        content_type: checked_optional_remote_text(
            value.content_type,
            MAIL_ATTACHMENT_CONTENT_TYPE_MAX_BYTES,
        )?,
        size_bytes: value.size.map(|size| size.to_string()),
    })
}

fn mail_addresses(values: Vec<im_core::email::EmailAddress>) -> SafeResult<Vec<String>> {
    if values.len() > MAIL_ADDRESS_COLLECTION_MAX_ITEMS {
        return Err(invalid_mail_response());
    }
    values.iter().map(mail_address).collect()
}

fn mail_address(value: &im_core::email::EmailAddress) -> SafeResult<String> {
    let value = value.as_str();
    if !(3..=320).contains(&value.chars().count())
        || !value.contains('@')
        || value
            .chars()
            .any(|character| character.is_whitespace() || character.is_control())
    {
        return Err(invalid_mail_response());
    }
    Ok(value.to_owned())
}

fn checked_mail_token(value: &str, max_chars: usize) -> SafeResult<String> {
    if value.is_empty()
        || value.trim() != value
        || value.chars().count() > max_chars
        || value.chars().any(char::is_control)
    {
        return Err(invalid_mail_response());
    }
    Ok(value.to_owned())
}

fn checked_timestamp(value: Option<String>) -> SafeResult<Option<String>> {
    value
        .map(|value| {
            if chrono::DateTime::parse_from_rfc3339(&value).is_ok() {
                return Ok(value);
            }
            let naive = chrono::NaiveDateTime::parse_from_str(&value, "%Y-%m-%dT%H:%M:%S%.f")
                .map_err(|_| invalid_mail_response())?;
            Ok(naive
                .and_utc()
                .to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true))
        })
        .transpose()
}

fn checked_optional_remote_text(
    value: Option<String>,
    max_bytes: usize,
) -> SafeResult<Option<String>> {
    value
        .map(|value| checked_remote_text(value, max_bytes))
        .transpose()
}

fn checked_remote_text(value: String, max_bytes: usize) -> SafeResult<String> {
    if value.len() > max_bytes || value.contains('\0') {
        return Err(invalid_mail_response());
    }
    Ok(value)
}

pub(crate) fn valid_mail_file_name(value: &str) -> bool {
    value.len() <= MAIL_ATTACHMENT_FILENAME_MAX_BYTES
        && im_core::email::valid_attachment_filename(value)
}

pub(crate) fn valid_mail_content_type(value: &str) -> bool {
    let Some((kind, subtype)) = value.split_once('/') else {
        return false;
    };
    !kind.is_empty()
        && !subtype.is_empty()
        && !subtype.contains('/')
        && kind.bytes().all(valid_mime_token_byte)
        && subtype.bytes().all(valid_mime_token_byte)
}

fn valid_mime_token_byte(value: u8) -> bool {
    value.is_ascii_alphanumeric()
        || matches!(
            value,
            b'!' | b'#' | b'$' | b'&' | b'^' | b'_' | b'.' | b'+' | b'-'
        )
}

fn truncate_optional_utf8(value: Option<String>, max_bytes: usize) -> (Option<String>, bool) {
    match value {
        Some(value) => {
            let (value, truncated) = truncate_utf8(value, max_bytes);
            (Some(value), truncated)
        }
        None => (None, false),
    }
}

fn truncate_utf8(mut value: String, max_bytes: usize) -> (String, bool) {
    if value.len() <= max_bytes {
        return (value, false);
    }
    let mut boundary = max_bytes;
    while !value.is_char_boundary(boundary) {
        boundary -= 1;
    }
    value.truncate(boundary);
    (value, true)
}

fn invalid_mail_response() -> SafeError {
    SafeError::new(
        "remote_response_invalid",
        "The mail service returned an invalid response.",
        false,
    )
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
        registered_at_ms,
    }
}

pub(crate) fn profile(value: im_core::identity::Profile) -> NodeProfile {
    NodeProfile {
        did: value.subject.as_str().to_owned(),
        handle: value.handle.map(|handle| handle.as_str().to_owned()),
        display_name: value.display_name,
        bio: value.bio.or(value.description),
        tags: value.tags,
        updated_at: value.updated_at,
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

pub(crate) fn display_profile_batch_request(
    input: NodeDisplayProfileBatchInput,
    default_domain: &str,
) -> SafeResult<im_core::directory::DisplayProfileBatchRequest> {
    if input.peers.len() > 100 {
        return Err(SafeError::new(
            "invalid_input",
            "The display profile batch exceeds 100 peers.",
            false,
        ));
    }
    Ok(im_core::directory::DisplayProfileBatchRequest {
        peers: input
            .peers
            .into_iter()
            .map(|peer| {
                im_core::ids::PeerRef::parse(peer, default_domain).map_err(SafeError::from_im)
            })
            .collect::<SafeResult<Vec<_>>>()?,
    })
}

pub(crate) fn display_profiles(
    values: Vec<im_core::directory::DisplayProfile>,
) -> Vec<NodeDisplayProfile> {
    values
        .into_iter()
        .map(|value| NodeDisplayProfile {
            did: value.did.map(|did| did.as_str().to_owned()),
            handle: value.handle.map(|handle| handle.as_str().to_owned()),
            display_name: value.display_name,
            cache_hit: value.cache_hit,
            is_stale: value.is_stale,
        })
        .collect()
}

pub(crate) fn group_create_request(
    input: NodeCreateGroupInput,
    creator_handle: Option<&im_core::ids::Handle>,
) -> SafeResult<im_core::groups::GroupCreateRequest> {
    let name = input.name.trim().to_owned();
    if name.is_empty() {
        return Err(SafeError::new(
            "invalid_input",
            "The group name must not be empty.",
            false,
        ));
    }
    let mut request = im_core::groups::GroupCreateRequest::new(name);
    request.creator_handle = creator_handle.cloned();
    request.description = input
        .description
        .map(|description| description.trim().to_owned())
        .filter(|description| !description.is_empty());
    request.discoverability = Some(im_core::groups::GroupDiscoverability::Private);
    request.admission_mode = Some(im_core::groups::GroupAdmissionMode::OpenJoin);
    request.message_security_profile =
        Some(im_core::groups::GroupMessageSecurityProfile::TransportProtected);
    Ok(request)
}

pub(crate) fn group_join_request(
    input: NodeGroupInput,
    member_handle: Option<&im_core::ids::Handle>,
) -> SafeResult<im_core::groups::GroupJoinRequest> {
    Ok(im_core::groups::GroupJoinRequest {
        group: im_core::ids::GroupRef::parse(input.group_did).map_err(SafeError::from_im)?,
        member_handle: member_handle.cloned(),
        reason_text: None,
    })
}

pub(crate) fn created_group(
    value: im_core::groups::GroupReadResult,
    fallback_title: &str,
) -> SafeResult<NodeGroup> {
    let snapshot = value.group.ok_or_else(SafeError::internal)?;
    Ok(created_group_from_snapshot(snapshot, fallback_title))
}

pub(crate) fn created_group_from_snapshot(
    snapshot: im_core::groups::GroupSnapshot,
    fallback_title: &str,
) -> NodeGroup {
    let conversation_id = im_core::messages::ConversationIdentity::from_thread_ref(
        &im_core::messages::ThreadRef::Group(snapshot.did.clone()),
    )
    .conversation_id;
    let title = snapshot
        .display_name
        .clone()
        .or_else(|| snapshot.name.clone())
        .unwrap_or_else(|| fallback_title.to_owned());
    NodeGroup {
        did: snapshot.did.as_str().to_owned(),
        conversation_id,
        title,
        description: snapshot.description,
        member_count: snapshot.member_count,
        my_role: snapshot.my_role,
        membership_status: snapshot.membership_status,
    }
}

pub(crate) fn group_from_summary(snapshot: im_core::groups::GroupSummary) -> NodeGroup {
    let conversation_id = im_core::messages::ConversationIdentity::from_thread_ref(
        &im_core::messages::ThreadRef::Group(snapshot.did.clone()),
    )
    .conversation_id;
    let title = snapshot
        .display_name
        .or(snapshot.name)
        .unwrap_or_else(|| snapshot.did.as_str().to_owned());
    NodeGroup {
        did: snapshot.did.as_str().to_owned(),
        conversation_id,
        title,
        description: None,
        member_count: snapshot.member_count,
        my_role: snapshot.my_role,
        membership_status: snapshot.membership_status,
    }
}

pub(crate) fn groups(value: im_core::groups::GroupReadResult) -> NodePageOfGroups {
    NodePageOfGroups {
        items: value.groups.into_iter().map(group_from_summary).collect(),
        next_cursor: value.next_cursor.map(|cursor| cursor.as_str().to_owned()),
        has_more: value.has_more,
    }
}

pub(crate) fn group_member_mutation_request(
    input: NodeAddGroupMemberInput,
    default_domain: &str,
) -> SafeResult<im_core::groups::GroupMemberMutationRequest> {
    Ok(im_core::groups::GroupMemberMutationRequest {
        group: im_core::ids::GroupRef::parse(input.group_did).map_err(SafeError::from_im)?,
        member: im_core::groups::GroupMemberRef::parse(input.member, default_domain)
            .map_err(SafeError::from_im)?,
        role: input
            .role
            .map(im_core::groups::GroupMemberRole::parse)
            .transpose()
            .map_err(SafeError::from_im)?,
        reason_text: None,
        leave_request_id: None,
        security: im_core::groups::GroupSecurityRequirement::default(),
    })
}

pub(crate) fn group_member_page(value: im_core::groups::GroupReadResult) -> NodeGroupMemberPage {
    NodeGroupMemberPage {
        items: value
            .members
            .into_iter()
            .map(|member| NodeGroupMemberRecord {
                membership_id: member.membership_id,
                peer_persona_id: member.peer_persona_id,
                did: member.did.map(|did| did.as_str().to_owned()),
                credential_did: member.credential_did.map(|did| did.as_str().to_owned()),
                handle: member.handle.map(|handle| handle.as_str().to_owned()),
                role: member.role,
                status: member.status,
                joined_at: member.joined_at,
                subject_type: member.subject_type,
            })
            .collect(),
        total: value.total,
        next_cursor: value.next_cursor.map(|cursor| cursor.as_str().to_owned()),
        has_more: value.has_more,
        page_group: value.page_group.map(|group| group.as_str().to_owned()),
        group_state_version: value.group_state_version,
        warnings: value.warnings,
    }
}

pub(crate) fn rebind_recovery_summary(
    value: im_core::groups::GroupRebindRecoverySummary,
) -> NodeGroupRebindRecoverySummary {
    NodeGroupRebindRecoverySummary {
        processed: value.processed,
        completed: value.completed,
        pending: value.pending,
        blocked: value.blocked,
        send_paused_groups: value
            .send_paused_groups
            .into_iter()
            .map(|group| group.as_str().to_owned())
            .collect(),
        items: value
            .items
            .into_iter()
            .map(|item| NodeGroupRebindRecoveryItem {
                group_did: item.group.as_str().to_owned(),
                layer: item.layer,
                phase: item.phase,
                blocked: item.blocked,
            })
            .collect(),
        warnings: value.warnings,
    }
}

pub(crate) fn recovery_otp_result(
    value: im_core::identity::HandleRecoveryOtpResult,
) -> NodeHandleRecoveryOtpResult {
    NodeHandleRecoveryOtpResult {
        owner_identity_id: value.owner_identity_id.as_str().to_owned(),
        full_handle: value.full_handle,
        operation_id: value.operation_id,
        accepted: value.accepted,
        retry_after_seconds: value.retry_after_seconds,
        retry_at: value.retry_at,
    }
}

pub(crate) fn recovery_progress(
    value: im_core::identity::HandleRecoveryProgress,
) -> NodeHandleRecoveryProgress {
    let failure_code = value.failure_code.map(|code| code.as_str().to_owned());
    let retryable = value
        .failure_code
        .is_some_and(im_core::identity::HandleRecoveryErrorCode::retryable);
    NodeHandleRecoveryProgress {
        operation_id: value.operation_id,
        owner_identity_id: value.owner_identity_id.as_str().to_owned(),
        full_handle: value.full_handle,
        previous_did: value.local_previous_did.map(|did| did.as_str().to_owned()),
        current_did: value.current_did.as_str().to_owned(),
        phase: recovery_phase(value.phase).to_owned(),
        failure_code,
        retryable,
        impact: NodeHandleRecoveryImpact {
            local_ordinary_data_will_migrate: value.impact.local_ordinary_data_will_migrate,
            other_devices_must_rejoin: value.impact.other_devices_must_rejoin,
            unsupported_e2ee_group_count: value.impact.unsupported_e2ee_group_count,
            unsupported_did_only_group_count: value.impact.unsupported_did_only_group_count,
        },
    }
}

pub(crate) fn recovery_attestation(
    value: im_core::identity::HandleRecoveryAttestation,
) -> NodeHandleRecoveryAttestationResult {
    let attestation = value.expose_attestation().to_owned();
    NodeHandleRecoveryAttestationResult {
        attestation,
        expires_at: value.expires_at,
    }
}

pub(crate) fn recovery_operation_summary(
    value: im_core::identity::HandleRecoveryOperationSummary,
) -> NodeHandleRecoveryOperationSummary {
    NodeHandleRecoveryOperationSummary {
        operation_id: value.operation_id,
        owner_identity_id: value.owner_identity_id.as_str().to_owned(),
        full_handle: value.full_handle,
        lifecycle: recovery_lifecycle(value.lifecycle_class).to_owned(),
        commit_attempted: value.commit_attempted,
        key_state: recovery_key_state(value.key_state).to_owned(),
        last_error_code: value.last_error_code,
        created_at: value.created_at,
        updated_at: value.updated_at,
    }
}

fn recovery_phase(value: im_core::identity::HandleRecoveryPhase) -> &'static str {
    use im_core::identity::HandleRecoveryPhase::*;
    match value {
        AwaitingFactor => "awaiting_factor",
        ReadyToCommit => "ready_to_commit",
        RemoteOutcomeUnknown => "remote_outcome_unknown",
        RemoteCommitted => "remote_committed",
        IdentityTransitionPending => "identity_transition_pending",
        Applied => "applied",
        QuarantinedKeyUnavailable => "quarantined_key_unavailable",
    }
}

fn recovery_lifecycle(value: im_core::identity::HandleRecoveryOperationLifecycle) -> &'static str {
    use im_core::identity::HandleRecoveryOperationLifecycle::*;
    match value {
        PreCommit => "pre_commit",
        RemoteUnresolved => "remote_unresolved",
        RemoteCommitted => "remote_committed",
        LocalTransitionPending => "local_transition_pending",
        Applied => "applied",
        DiscardedPreAttempt => "discarded_pre_attempt",
        QuarantinedKeyUnavailable => "quarantined_key_unavailable",
        SupersededByStateChange => "superseded_by_state_change",
        FailedTerminal => "failed_terminal",
    }
}

fn recovery_key_state(value: im_core::identity::HandleRecoveryKeyState) -> &'static str {
    use im_core::identity::HandleRecoveryKeyState::*;
    match value {
        Available => "available",
        TemporarilyLocked => "temporarily_locked",
        PermanentlyUnavailable => "permanently_unavailable",
        DestroyedPreAttempt => "destroyed_pre_attempt",
    }
}

pub(crate) fn added_group_member(
    value: im_core::groups::GroupReadResult,
) -> SafeResult<NodeGroupMember> {
    let member = value.resolved_member.ok_or_else(SafeError::internal)?;
    Ok(group_member(member))
}

pub(crate) fn group_member(member: im_core::groups::GroupMemberResolution) -> NodeGroupMember {
    NodeGroupMember {
        did: member.did.as_str().to_owned(),
        handle: member.handle.map(|handle| handle.as_str().to_owned()),
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
    if value.resolution_state != im_core::messages::ConversationResolutionState::Resolved {
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
    let sender_display_name = message_attribute(&value, "sender_display_name")
        .or_else(|| message_attribute(&value, "senderName"))
        .or_else(|| message_attribute(&value, "sender_name"));
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
            metadata: im_core::messages::MessageMetadata {
                attributes: vec![im_core::messages::MessageMetadataAttribute {
                    key: "sender_name".to_owned(),
                    value: "Bob".to_owned(),
                }],
                ..Default::default()
            },
        };
        let mapped = sent_message(message, "group:did:example:group").unwrap();
        assert_eq!(mapped.id, "message-1");
        assert_eq!(mapped.conversation_id, "group:did:example:group");
        assert_eq!(mapped.conversation_kind, "group");
        assert_eq!(mapped.sender_display_name.as_deref(), Some("Bob"));
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
    fn display_profile_batch_is_bounded_and_preserves_cached_labels() {
        let request = display_profile_batch_request(
            NodeDisplayProfileBatchInput {
                peers: vec!["did:wba:awiki.ai:user:bob".to_owned()],
            },
            "awiki.ai",
        )
        .unwrap();
        assert_eq!(request.peers[0].as_str(), "did:wba:awiki.ai:user:bob");

        let mapped = display_profiles(vec![im_core::directory::DisplayProfile {
            did: Some(im_core::ids::Did::parse("did:wba:awiki.ai:user:bob").unwrap()),
            handle: Some(im_core::ids::Handle::parse("bob.awiki.ai", "awiki.ai").unwrap()),
            display_name: Some("Bob".to_owned()),
            avatar_uri: None,
            avatar_url: None,
            profile_uri: None,
            subject_type: None,
            cache_hit: true,
            is_stale: false,
            legacy_fallback: false,
            warnings: Vec::new(),
        }]);
        assert_eq!(mapped[0].handle.as_deref(), Some("bob.awiki.ai"));
        assert_eq!(mapped[0].display_name.as_deref(), Some("Bob"));
        assert!(mapped[0].cache_hit);

        let error = display_profile_batch_request(
            NodeDisplayProfileBatchInput {
                peers: vec!["did:wba:awiki.ai:user:bob".to_owned(); 101],
            },
            "awiki.ai",
        )
        .unwrap_err();
        assert_eq!(error.code, "invalid_input");
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
