//! AWiki-local new-device Join orchestration.
//!
//! These DTOs are first-party control-plane objects, not ANP wire models. The
//! service keeps private keys, pairing material and remote Join tokens inside
//! SecretVault and only returns public requests, proofs, ciphertext and a
//! short-lived SAS. Local approval/cancel phases support restart-safe projection;
//! they are not ANP wire states.

use serde::{Deserialize, Serialize};
use time::format_description::well_known::Rfc3339;
use time::{Duration, OffsetDateTime};

pub const DEVICE_JOIN_REQUEST_TYPE: &str = "awiki.device.join.v1";
pub const DEVICE_JOIN_REQUEST_PROOF_TYPE: &str = "awiki.device.join-request-proof.v1";
pub const DEVICE_JOIN_REQUEST_PROOF_INPUT_TYPE: &str = "awiki.device.join-request-proof-input.v1";
pub const DEVICE_JOIN_RESPONSE_SIGNATURE_INPUT_TYPE: &str =
    "awiki.device.join-response-signature-input.v1";
// Used by the independent device-revoke control path. Device Join v1 itself
// does not write this legacy proof profile.
pub(crate) const DEVICE_PROOF_TYPE: &str = "awiki-device-signature-v1";
pub const DEVICE_JOIN_CHALLENGE_ALGORITHM: &str = "X25519-HKDF-SHA256-CHACHA20POLY1305";
pub const DEVICE_JOIN_MAX_TTL_SECONDS: u64 = 600;
pub const DEVICE_JOIN_MAX_CHALLENGE_TTL_SECONDS: u64 = 300;

pub const DEVICE_JOIN_VNEXT_PROFILES: &[&str] = &[
    anp::authentication::PROFILE_CORE_BINDING_V1,
    anp::authentication::PROFILE_IDENTITY_DISCOVERY_V1,
    anp::authentication::PROFILE_DIRECT_BASE_V1,
    anp::authentication::PROFILE_DIRECT_E2EE_V2,
    anp::authentication::PROFILE_GROUP_BASE_V1,
    anp::authentication::PROFILE_GROUP_E2EE_V2,
];

pub(crate) const DEVICE_JOIN_LEGACY_DRAFT_PROFILES: &[&str] = &[
    anp::authentication::PROFILE_CORE_BINDING_V2,
    anp::authentication::PROFILE_IDENTITY_DISCOVERY_V2,
    anp::authentication::PROFILE_DIRECT_BASE_V2,
    anp::authentication::PROFILE_DIRECT_E2EE_V2,
    anp::authentication::PROFILE_GROUP_BASE_V2,
    anp::authentication::PROFILE_GROUP_E2EE_V2,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceJoinSide {
    NewDevice,
    Admin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceJoinLocalPhase {
    Pending,
    ChallengePrepared,
    ResponsePrepared,
    ResponseVerified,
    ApprovalPrepared,
    Authorized,
    Cancelled,
    Expired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceJoinSessionSummary {
    pub join_session_id: String,
    pub did: crate::ids::Did,
    pub protocol_device_id: crate::ids::ProtocolDeviceId,
    pub side: DeviceJoinSide,
    pub phase: DeviceJoinLocalPhase,
    pub join_request_hash: String,
    pub challenge_id: Option<String>,
    pub expires_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceJoinStartRequest {
    pub operation_id: String,
    pub did: crate::ids::Did,
    pub ttl_seconds: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceJoinRequest {
    #[serde(rename = "type")]
    pub request_type: String,
    pub did: String,
    pub join_session_id: String,
    pub device_id: String,
    pub signing_public_key: serde_json::Value,
    pub e2ee_public_key: serde_json::Value,
    pub pairing_public_key: String,
    pub profiles: Vec<String>,
    pub requested_role: String,
    pub issued_at: String,
    pub expires_at: String,
    pub join_request_proof: DeviceJoinRequestProof,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeviceJoinStartResult {
    pub session: DeviceJoinSessionSummary,
    pub join_request: DeviceJoinRequest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceJoinRequestProof {
    #[serde(rename = "type")]
    pub proof_type: String,
    pub algorithm: String,
    pub verification_method: String,
    pub created_at: String,
    pub proof_value_b64u: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct DeviceJoinObjectProof {
    #[serde(rename = "type")]
    pub proof_type: String,
    pub cryptosuite: String,
    pub verification_method: String,
    pub proof_purpose: String,
    pub created: String,
    pub proof_value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DeviceProof {
    #[serde(rename = "type")]
    pub proof_type: String,
    pub key_id: String,
    pub created_at: String,
    pub expires_at: String,
    pub nonce: String,
    pub signature: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EncryptedJoinChallenge {
    pub algorithm: String,
    pub nonce_b64u: String,
    pub ciphertext_b64u: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceJoinChallenge {
    pub operation_id: String,
    pub join_session_id: String,
    pub challenge_id: String,
    pub admin_device_id: String,
    pub admin_pairing_public_key: String,
    pub ciphertext: EncryptedJoinChallenge,
    pub challenge_expires_at: String,
    pub proof: DeviceJoinObjectProof,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceJoinChallengeResponse {
    pub operation_id: String,
    pub join_session_id: String,
    pub challenge_id: String,
    pub challenge_hash: String,
    pub join_request_hash: String,
    pub pairing_transcript_hash: String,
    pub response_signature_b64u: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeviceJoinAdminPrepareRequest {
    pub admin_identity: super::IdentitySelector,
    pub operation_id: String,
    pub join_request: DeviceJoinRequest,
    pub challenge_ttl_seconds: u64,
    pub document_version: u64,
    pub document_hash: String,
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct DeviceJoinAdminPrepareResult {
    pub session: DeviceJoinSessionSummary,
    pub challenge: DeviceJoinChallenge,
    pub sas: String,
}

impl std::fmt::Debug for DeviceJoinAdminPrepareResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeviceJoinAdminPrepareResult")
            .field("session", &self.session)
            .field("challenge", &self.challenge)
            .field("sas", &"<redacted-sas>")
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeviceJoinNewDeviceRespondRequest {
    pub operation_id: String,
    pub challenge: DeviceJoinChallenge,
    pub admin_did_document: serde_json::Value,
    pub document_version: u64,
    pub document_hash: String,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceJoinNewDeviceRespondResult {
    pub session: DeviceJoinSessionSummary,
    pub response: DeviceJoinChallengeResponse,
    pub sas: String,
}

impl std::fmt::Debug for DeviceJoinNewDeviceRespondResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeviceJoinNewDeviceRespondResult")
            .field("session", &self.session)
            .field("response", &self.response)
            .field("sas", &"<redacted-sas>")
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceJoinAdminVerifyRequest {
    pub operation_id: String,
    pub join_session_id: String,
    pub response: DeviceJoinChallengeResponse,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceJoinAdminVerifyResult {
    pub session: DeviceJoinSessionSummary,
    pub join_request_hash: String,
    pub pairing_transcript_hash: String,
    pub sas: String,
}

/// Write-only account-verification grant consumed when a pending device Join
/// is created. The opaque value is never serializable or returned by Core.
pub struct DeviceJoinAccountVerificationGrant {
    secret: crate::internal::platform_secret::SecretBytes,
}

impl DeviceJoinAccountVerificationGrant {
    pub fn from_token(token: impl Into<String>) -> crate::ImResult<Self> {
        Self::from_bytes(token.into().into_bytes())
    }

    pub fn from_bytes(token: Vec<u8>) -> crate::ImResult<Self> {
        if token.is_empty() || token.iter().all(u8::is_ascii_whitespace) {
            return Err(crate::ImError::invalid_input(
                Some("account_verification_grant".to_owned()),
                "account verification grant must not be empty",
            ));
        }
        Ok(Self {
            secret: crate::internal::platform_secret::SecretBytes::from_vec(token),
        })
    }

    fn into_secret(self) -> crate::internal::platform_secret::SecretBytes {
        self.secret
    }
}

impl std::fmt::Debug for DeviceJoinAccountVerificationGrant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DeviceJoinAccountVerificationGrant(<redacted>)")
    }
}

pub struct DeviceJoinBeginRequest {
    pub operation_id: String,
    pub did: crate::ids::Did,
    pub ttl_seconds: u64,
    pub account_verification_grant: DeviceJoinAccountVerificationGrant,
}

impl std::fmt::Debug for DeviceJoinBeginRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeviceJoinBeginRequest")
            .field("operation_id", &self.operation_id)
            .field("did", &self.did)
            .field("ttl_seconds", &self.ttl_seconds)
            .field("account_verification_grant", &"<redacted>")
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceJoinRole {
    Member,
    Admin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceJoinAuthorizationStatus {
    Active,
    Revoked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceJoinRejectReason {
    UserRejected,
    SasMismatch,
}

impl DeviceJoinRejectReason {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::UserRejected => "user_rejected",
            Self::SasMismatch => "sas_mismatch",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceJoinRemoteState {
    Pending,
    ChallengeSent,
    ResponseVerified,
    Consumed,
    Cancelled,
    Rejected,
    Expired,
}

/// Safe host-facing Join session projection. Internal transcript hashes and
/// challenge identifiers intentionally stay in the restart-safe local state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceJoinSessionView {
    pub join_session_id: String,
    pub did: crate::ids::Did,
    pub protocol_device_id: crate::ids::ProtocolDeviceId,
    pub side: DeviceJoinSide,
    pub phase: DeviceJoinLocalPhase,
    pub expires_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceJoinAuthorizedDeviceSummary {
    pub protocol_device_id: crate::ids::ProtocolDeviceId,
    pub signing_key_id: String,
    pub e2ee_key_id: String,
    pub status: DeviceJoinAuthorizationStatus,
    pub role: DeviceJoinRole,
    pub management_ready: bool,
    pub is_current: bool,
}

/// Registry-only device projection.
///
/// Unlike Join progress, an authoritative Registry snapshot carries the
/// device authorization generation required for monotonic account-state cache
/// replacement. The generation remains a canonical decimal string at the
/// public boundary and must not be used by hosts to authorize device actions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceRegistryAuthorizedDeviceSummary {
    pub protocol_device_id: crate::ids::ProtocolDeviceId,
    pub signing_key_id: String,
    pub e2ee_key_id: String,
    pub status: DeviceJoinAuthorizationStatus,
    pub role: DeviceJoinRole,
    pub management_ready: bool,
    pub is_current: bool,
    pub auth_generation: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceJoinRequestNotice {
    pub event_id: String,
    pub join_session_id: String,
    pub did: crate::ids::Did,
    pub protocol_device_id: crate::ids::ProtocolDeviceId,
    pub candidate_key_fingerprint: String,
    pub issued_at: String,
    pub expires_at: String,
    pub state: DeviceJoinRemoteState,
    pub claimed_by_current_device: bool,
    pub can_start_verification: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceJoinRegistrySnapshot {
    pub did: crate::ids::Did,
    pub registry_version: String,
    pub devices: Vec<DeviceRegistryAuthorizedDeviceSummary>,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceJoinProgress {
    pub session: DeviceJoinSessionView,
    pub remote_state: DeviceJoinRemoteState,
    pub sas: Option<String>,
    pub authorized_device: Option<DeviceJoinAuthorizedDeviceSummary>,
}

impl std::fmt::Debug for DeviceJoinProgress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeviceJoinProgress")
            .field("session", &self.session)
            .field("remote_state", &self.remote_state)
            .field("sas", &self.sas.as_ref().map(|_| "<redacted-sas>"))
            .field("authorized_device", &self.authorized_device)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct DeviceJoinApprovalPrompt {
    pub approval_handle: String,
    pub join_session_id: String,
    pub sas: String,
    pub expires_at: String,
}

impl std::fmt::Debug for DeviceJoinApprovalPrompt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeviceJoinApprovalPrompt")
            .field("approval_handle", &"<redacted-approval-handle>")
            .field("join_session_id", &self.join_session_id)
            .field("sas", &"<redacted-sas>")
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

pub struct DeviceJoinConfirmApprovalRequest {
    pub approval_handle: String,
    pub user_presence_confirmed: bool,
}

impl std::fmt::Debug for DeviceJoinConfirmApprovalRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeviceJoinConfirmApprovalRequest")
            .field("approval_handle", &"<redacted-approval-handle>")
            .field("user_presence_confirmed", &self.user_presence_confirmed)
            .finish()
    }
}

impl std::fmt::Debug for DeviceJoinAdminVerifyResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeviceJoinAdminVerifyResult")
            .field("session", &self.session)
            .field("join_request_hash", &self.join_request_hash)
            .field("pairing_transcript_hash", &self.pairing_transcript_hash)
            .field("sas", &"<redacted-sas>")
            .finish()
    }
}

pub struct DeviceJoinService<'a> {
    core: &'a crate::core::ImCore,
}

impl<'a> DeviceJoinService<'a> {
    pub(crate) fn new(core: &'a crate::core::ImCore) -> Self {
        Self { core }
    }

    pub(crate) async fn start(
        &self,
        request: DeviceJoinStartRequest,
        resolved_document: &serde_json::Value,
    ) -> crate::ImResult<DeviceJoinStartResult> {
        crate::internal::identity_device_join::start(self.core, request, resolved_document).await
    }

    pub(crate) fn prepare_admin_challenge(
        &self,
        request: DeviceJoinAdminPrepareRequest,
    ) -> crate::ImResult<DeviceJoinAdminPrepareResult> {
        crate::internal::identity_device_join::prepare_admin_challenge(self.core, request)
    }

    pub(crate) async fn respond_as_new_device(
        &self,
        request: DeviceJoinNewDeviceRespondRequest,
    ) -> crate::ImResult<DeviceJoinNewDeviceRespondResult> {
        crate::internal::identity_device_join::respond_as_new_device(self.core, request).await
    }

    pub(crate) fn verify_response_as_admin(
        &self,
        request: DeviceJoinAdminVerifyRequest,
    ) -> crate::ImResult<DeviceJoinAdminVerifyResult> {
        crate::internal::identity_device_join::verify_response_as_admin(self.core, request)
    }

    pub(crate) fn session(
        &self,
        join_session_id: &str,
        side: DeviceJoinSide,
    ) -> crate::ImResult<DeviceJoinSessionSummary> {
        crate::internal::identity_device_join::session(self.core, join_session_id, side)
    }

    pub fn local_sessions(&self) -> crate::ImResult<Vec<DeviceJoinSessionView>> {
        crate::internal::identity_device_join::list_sessions(self.core)
            .map(|sessions| sessions.into_iter().map(Into::into).collect())
    }

    pub async fn begin_new_device_join(
        &self,
        request: DeviceJoinBeginRequest,
    ) -> crate::ImResult<DeviceJoinProgress> {
        let DeviceJoinBeginRequest {
            operation_id,
            did,
            ttl_seconds,
            account_verification_grant,
        } = request;
        let token = account_verification_grant.into_secret();
        let mut runtime =
            crate::internal::identity_device_join_runtime::DeviceJoinNewDeviceRuntime::production(
                self.core,
            );
        let session = runtime
            .begin(
                DeviceJoinStartRequest {
                    operation_id,
                    did,
                    ttl_seconds,
                },
                &token,
            )
            .await?;
        Ok(DeviceJoinProgress {
            session: session.into(),
            remote_state: DeviceJoinRemoteState::Pending,
            sas: None,
            authorized_device: None,
        })
    }

    pub(crate) async fn begin_new_device_join_with_local_hook<F>(
        &self,
        request: DeviceJoinBeginRequest,
        local_hook: F,
    ) -> crate::ImResult<DeviceJoinProgress>
    where
        F: FnOnce(&DeviceJoinSessionSummary) -> crate::ImResult<()>,
    {
        let DeviceJoinBeginRequest {
            operation_id,
            did,
            ttl_seconds,
            account_verification_grant,
        } = request;
        let token = account_verification_grant.into_secret();
        let mut runtime =
            crate::internal::identity_device_join_runtime::DeviceJoinNewDeviceRuntime::production(
                self.core,
            );
        let session = runtime
            .begin_with_local_hook(
                DeviceJoinStartRequest {
                    operation_id,
                    did,
                    ttl_seconds,
                },
                &token,
                local_hook,
            )
            .await?;
        Ok(DeviceJoinProgress {
            session: session.into(),
            remote_state: DeviceJoinRemoteState::Pending,
            sas: None,
            authorized_device: None,
        })
    }

    pub async fn poll_new_device_join(
        &self,
        join_session_id: &str,
    ) -> crate::ImResult<DeviceJoinProgress> {
        let mut runtime =
            crate::internal::identity_device_join_runtime::DeviceJoinNewDeviceRuntime::production(
                self.core,
            );
        let mut progress = public_progress(runtime.advance(join_session_id).await?)?;
        if progress.remote_state == DeviceJoinRemoteState::Consumed
            && progress.authorized_device.is_none()
            && progress.session.phase == DeviceJoinLocalPhase::Authorized
        {
            let summary = self
                .core
                .identities()
                .device_summary_async(super::IdentitySelector::Did(progress.session.did.clone()))
                .await?;
            progress.authorized_device = Some(public_current_authorized_device(
                summary,
                &progress.session.protocol_device_id,
            )?);
        }
        Ok(progress)
    }

    pub async fn cancel_new_device_join(
        &self,
        join_session_id: &str,
    ) -> crate::ImResult<DeviceJoinSessionView> {
        let mut runtime =
            crate::internal::identity_device_join_runtime::DeviceJoinNewDeviceRuntime::production(
                self.core,
            );
        runtime.cancel(join_session_id).await.map(Into::into)
    }

    pub async fn registry(
        &self,
        admin_identity: super::IdentitySelector,
    ) -> crate::ImResult<DeviceJoinRegistrySnapshot> {
        let admin_client = self.core.client_async(admin_identity.clone()).await?;
        let current_device = self
            .core
            .identities()
            .device_summary_async(admin_identity)
            .await?;
        let current = current_device.protocol_device_id;
        let mut runtime =
            crate::internal::identity_device_join_runtime::DeviceJoinAdminRuntime::production(
                self.core,
                &admin_client,
            );
        let registry = runtime.registry().await?;
        let _ = crate::internal::identity_device_revoke::recover_pending_with_registry(
            self.core,
            &admin_client,
            &registry,
        )
        .await;
        public_registry(registry, current.as_ref())
    }

    pub async fn local_device_join_requests(
        &self,
        admin_identity: super::IdentitySelector,
    ) -> crate::ImResult<Vec<DeviceJoinRequestNotice>> {
        let admin_client = self.core.client_async(admin_identity).await?;
        let mut runtime =
            crate::internal::identity_device_join_runtime::DeviceJoinAdminRuntime::production(
                self.core,
                &admin_client,
            );
        runtime.local_device_join_requests().await
    }

    /// Reads the short-lived SAS for an already verified local admin session.
    ///
    /// This method performs no RPC, does not update the durable notification
    /// projection, and does not advance the Join state machine. The SAS is
    /// available only while the local phase is `ResponseVerified` or
    /// `ApprovalPrepared`.
    pub fn local_device_join_verification_progress(
        &self,
        admin_identity: super::IdentitySelector,
        join_session_id: &str,
    ) -> crate::ImResult<DeviceJoinProgress> {
        let (session, sas) =
            crate::internal::identity_device_join::local_admin_verification_progress(
                self.core,
                &admin_identity,
                join_session_id,
            )?;
        Ok(DeviceJoinProgress {
            session: session.into(),
            remote_state: DeviceJoinRemoteState::ResponseVerified,
            sas: Some(sas),
            authorized_device: None,
        })
    }

    pub async fn start_device_join_verification(
        &self,
        admin_identity: super::IdentitySelector,
        join_session_id: &str,
        operation_id: &str,
        challenge_ttl_seconds: u64,
    ) -> crate::ImResult<DeviceJoinProgress> {
        let admin_client = self.core.client_async(admin_identity).await?;
        let mut runtime =
            crate::internal::identity_device_join_runtime::DeviceJoinAdminRuntime::production(
                self.core,
                &admin_client,
            );
        runtime
            .start_verification(join_session_id, operation_id, challenge_ttl_seconds)
            .await
            .and_then(public_progress)
    }

    pub fn prepare_device_join_approval(
        &self,
        admin_identity: super::IdentitySelector,
        join_session_id: &str,
        sas_confirmed: bool,
    ) -> crate::ImResult<DeviceJoinApprovalPrompt> {
        if !sas_confirmed {
            return Err(crate::ImError::PermissionDenied);
        }
        let (session, sas, existing) =
            crate::internal::identity_device_join::admin_approval_context(
                self.core,
                join_session_id,
            )?;
        let now = OffsetDateTime::now_utc();
        let session_expires_at = parse_approval_expiry(&session.expires_at)?;
        let (operation_id, user_presence_at, approval_expires_at) = match existing {
            Some(approval) => (
                approval.operation_id,
                Some(approval.pairing_confirmation.user_presence_at),
                session_expires_at,
            ),
            None => (
                random_public_operation_id("join-approve"),
                None,
                std::cmp::min(
                    session_expires_at,
                    now + Duration::seconds(DEVICE_JOIN_MAX_CHALLENGE_TTL_SECONDS as i64),
                ),
            ),
        };
        if approval_expires_at <= now {
            return Err(crate::ImError::SessionExpired);
        }
        let approval_expires_at = approval_expires_at.format(&Rfc3339).map_err(|error| {
            crate::ImError::Serialization {
                detail: error.to_string(),
            }
        })?;
        let handle = self.core.inner().device_join_approvals.issue(
            crate::internal::identity_device_join_runtime::DeviceJoinApprovalHandleState {
                admin_identity,
                join_session_id: session.join_session_id.clone(),
                operation_id,
                expires_at: approval_expires_at.clone(),
                user_presence_at,
            },
        )?;
        Ok(DeviceJoinApprovalPrompt {
            approval_handle: handle,
            join_session_id: session.join_session_id,
            sas,
            expires_at: approval_expires_at,
        })
    }

    pub async fn confirm_device_join_approval(
        &self,
        request: DeviceJoinConfirmApprovalRequest,
    ) -> crate::ImResult<DeviceJoinProgress> {
        if !request.user_presence_confirmed {
            self.core
                .inner()
                .device_join_approvals
                .cancel_ready(&request.approval_handle)?;
            return Err(crate::ImError::PermissionDenied);
        }
        let now = OffsetDateTime::now_utc();
        let confirmed_at = now
            .format(&Rfc3339)
            .map_err(|error| crate::ImError::Serialization {
                detail: error.to_string(),
            })?;
        let claim = self.core.inner().device_join_approvals.claim(
            &request.approval_handle,
            &confirmed_at,
            now,
        )?;
        let state = match claim {
            crate::internal::identity_device_join_runtime::DeviceJoinApprovalHandleClaim::Claimed(
                state,
            ) => state,
            crate::internal::identity_device_join_runtime::DeviceJoinApprovalHandleClaim::Expired(
                state,
            ) => {
                let _ = state;
                return Err(crate::ImError::SessionExpired);
            }
        };
        let confirmed_at = state
            .user_presence_at
            .clone()
            .ok_or(crate::ImError::PermissionDenied)?;
        let result = async {
            let admin_client = self.core.client_async(state.admin_identity.clone()).await?;
            let mut runtime =
                crate::internal::identity_device_join_runtime::DeviceJoinAdminRuntime::production(
                    self.core,
                    &admin_client,
                );
            runtime
                .approve(
                    &state.join_session_id,
                    &state.operation_id,
                    &confirmed_at,
                    true,
                )
                .await
        }
        .await;
        match &result {
            Ok(_) | Err(crate::ImError::SessionExpired) => self
                .core
                .inner()
                .device_join_approvals
                .consume(&request.approval_handle)?,
            Err(error) if approval_error_is_retryable(error) => {
                self.core
                    .inner()
                    .device_join_approvals
                    .release(&request.approval_handle)?;
            }
            Err(_) => self
                .core
                .inner()
                .device_join_approvals
                .consume(&request.approval_handle)?,
        }
        public_progress(result?)
    }

    pub async fn reject_device_join(
        &self,
        admin_identity: super::IdentitySelector,
        join_session_id: &str,
        reason: DeviceJoinRejectReason,
    ) -> crate::ImResult<DeviceJoinProgress> {
        let admin_client = self.core.client_async(admin_identity).await?;
        let mut runtime =
            crate::internal::identity_device_join_runtime::DeviceJoinAdminRuntime::production(
                self.core,
                &admin_client,
            );
        runtime
            .reject(join_session_id, reason)
            .await
            .and_then(public_progress)
    }
}

fn public_current_authorized_device(
    summary: super::IdentityDeviceSummary,
    expected_device_id: &crate::ids::ProtocolDeviceId,
) -> crate::ImResult<DeviceJoinAuthorizedDeviceSummary> {
    let protocol_device_id =
        summary
            .protocol_device_id
            .ok_or_else(|| crate::ImError::LocalStateUnavailable {
                detail: "authorized identity has no protocol device ID".to_owned(),
            })?;
    let signing_key_id =
        summary
            .signing_key_id
            .ok_or_else(|| crate::ImError::LocalStateUnavailable {
                detail: "authorized identity has no device signing key".to_owned(),
            })?;
    let e2ee_key_id = summary
        .e2ee_key_id
        .ok_or_else(|| crate::ImError::LocalStateUnavailable {
            detail: "authorized identity has no device E2EE key".to_owned(),
        })?;
    if &protocol_device_id != expected_device_id {
        return Err(crate::ImError::PermissionDenied);
    }
    let (role, management_ready) = match (summary.role, summary.readiness) {
        (Some(super::IdentityDeviceRole::Member), super::IdentityDeviceReadiness::MemberReady) => {
            (DeviceJoinRole::Member, false)
        }
        _ => return Err(crate::ImError::PermissionDenied),
    };
    Ok(DeviceJoinAuthorizedDeviceSummary {
        protocol_device_id,
        signing_key_id,
        e2ee_key_id,
        status: DeviceJoinAuthorizationStatus::Active,
        role,
        management_ready,
        is_current: true,
    })
}

fn parse_approval_expiry(value: &str) -> crate::ImResult<OffsetDateTime> {
    OffsetDateTime::parse(value, &Rfc3339).map_err(|_| crate::ImError::LocalStateUnavailable {
        detail: "device Join approval expiry is invalid".to_owned(),
    })
}

fn public_progress(
    value: crate::internal::identity_device_join_runtime::DeviceJoinAdvanceResult,
) -> crate::ImResult<DeviceJoinProgress> {
    let current_device = (value.session.side == DeviceJoinSide::NewDevice)
        .then(|| value.session.protocol_device_id.clone());
    let remote_state = public_remote_state(value.remote_state);
    let sas = if matches!(
        remote_state,
        DeviceJoinRemoteState::Consumed | DeviceJoinRemoteState::Expired
    ) || matches!(
        value.session.phase,
        DeviceJoinLocalPhase::Authorized
            | DeviceJoinLocalPhase::Cancelled
            | DeviceJoinLocalPhase::Expired
    ) {
        None
    } else {
        value.sas
    };
    Ok(DeviceJoinProgress {
        session: value.session.into(),
        remote_state,
        sas,
        authorized_device: value
            .authorization
            .map(|authorization| public_device(authorization.device, current_device.as_ref()))
            .transpose()?,
    })
}

impl From<DeviceJoinSessionSummary> for DeviceJoinSessionView {
    fn from(value: DeviceJoinSessionSummary) -> Self {
        Self {
            join_session_id: value.join_session_id,
            did: value.did,
            protocol_device_id: value.protocol_device_id,
            side: value.side,
            phase: value.phase,
            expires_at: value.expires_at,
        }
    }
}

fn public_registry(
    value: crate::internal::identity_device_join_runtime::DeviceJoinRemoteRegistry,
    current: Option<&crate::ids::ProtocolDeviceId>,
) -> crate::ImResult<DeviceJoinRegistrySnapshot> {
    Ok(DeviceJoinRegistrySnapshot {
        did: value.did,
        registry_version: value.checkpoint.registry_version.to_string(),
        devices: value
            .devices
            .into_iter()
            .map(|device| public_registry_device(device, current))
            .collect::<crate::ImResult<Vec<_>>>()?,
    })
}

fn public_registry_device(
    value: crate::internal::identity_device_join_runtime::DeviceJoinRemoteDeviceSummary,
    current: Option<&crate::ids::ProtocolDeviceId>,
) -> crate::ImResult<DeviceRegistryAuthorizedDeviceSummary> {
    let protocol_device_id = crate::ids::ProtocolDeviceId::parse(&value.device_id)?;
    Ok(DeviceRegistryAuthorizedDeviceSummary {
        is_current: current.is_some_and(|current| current == &protocol_device_id),
        protocol_device_id,
        signing_key_id: value.signing_key_id,
        e2ee_key_id: value.e2ee_key_id,
        status: match value.status {
            crate::internal::identity_device_state::DeviceAuthorizationStatus::Active => {
                DeviceJoinAuthorizationStatus::Active
            }
            crate::internal::identity_device_state::DeviceAuthorizationStatus::Revoked => {
                DeviceJoinAuthorizationStatus::Revoked
            }
        },
        role: public_role(value.role),
        management_ready: value.management_ready,
        auth_generation: value.auth_generation.to_string(),
    })
}

fn public_device(
    value: crate::internal::identity_device_join_runtime::DeviceJoinRemoteDeviceSummary,
    current: Option<&crate::ids::ProtocolDeviceId>,
) -> crate::ImResult<DeviceJoinAuthorizedDeviceSummary> {
    let protocol_device_id = crate::ids::ProtocolDeviceId::parse(&value.device_id)?;
    Ok(DeviceJoinAuthorizedDeviceSummary {
        is_current: current.is_some_and(|current| current == &protocol_device_id),
        protocol_device_id,
        signing_key_id: value.signing_key_id,
        e2ee_key_id: value.e2ee_key_id,
        status: match value.status {
            crate::internal::identity_device_state::DeviceAuthorizationStatus::Active => {
                DeviceJoinAuthorizationStatus::Active
            }
            crate::internal::identity_device_state::DeviceAuthorizationStatus::Revoked => {
                DeviceJoinAuthorizationStatus::Revoked
            }
        },
        role: public_role(value.role),
        management_ready: value.management_ready,
    })
}

fn public_role(
    value: crate::internal::identity_device_state::DeviceAuthorizationRole,
) -> DeviceJoinRole {
    match value {
        crate::internal::identity_device_state::DeviceAuthorizationRole::Member => {
            DeviceJoinRole::Member
        }
        crate::internal::identity_device_state::DeviceAuthorizationRole::Admin => {
            DeviceJoinRole::Admin
        }
    }
}

fn public_remote_state(
    value: crate::internal::identity_device_join_runtime::DeviceJoinRemoteState,
) -> DeviceJoinRemoteState {
    match value {
        crate::internal::identity_device_join_runtime::DeviceJoinRemoteState::Pending => {
            DeviceJoinRemoteState::Pending
        }
        crate::internal::identity_device_join_runtime::DeviceJoinRemoteState::ChallengeSent => {
            DeviceJoinRemoteState::ChallengeSent
        }
        crate::internal::identity_device_join_runtime::DeviceJoinRemoteState::ResponseVerified => {
            DeviceJoinRemoteState::ResponseVerified
        }
        crate::internal::identity_device_join_runtime::DeviceJoinRemoteState::Consumed => {
            DeviceJoinRemoteState::Consumed
        }
        crate::internal::identity_device_join_runtime::DeviceJoinRemoteState::Cancelled => {
            DeviceJoinRemoteState::Cancelled
        }
        crate::internal::identity_device_join_runtime::DeviceJoinRemoteState::Rejected => {
            DeviceJoinRemoteState::Rejected
        }
        crate::internal::identity_device_join_runtime::DeviceJoinRemoteState::Expired => {
            DeviceJoinRemoteState::Expired
        }
    }
}

fn random_public_operation_id(prefix: &str) -> String {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
    use rand::RngCore as _;

    let mut random = [0_u8; 16];
    rand::rngs::OsRng.fill_bytes(&mut random);
    format!("{prefix}:{}", URL_SAFE_NO_PAD.encode(random))
}

fn approval_error_is_retryable(error: &crate::ImError) -> bool {
    match error {
        crate::ImError::AuthRequired
        | crate::ImError::IdentityVault { .. }
        | crate::ImError::TransportUnavailable { .. }
        | crate::ImError::LocalStateUnavailable { .. }
        | crate::ImError::LocalStateUpgradeInProgress
        | crate::ImError::Io { .. } => true,
        crate::ImError::Service { status_code, .. } => status_code
            .is_none_or(|status| status == 408 || status == 425 || status == 429 || status >= 500),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn consumed_join_replay_projects_current_member_from_local_identity() {
        let did = crate::ids::Did::parse("did:wba:awiki.test:alice").unwrap();
        let protocol_device_id = crate::ids::ProtocolDeviceId::parse("dev-new").unwrap();
        let device = public_current_authorized_device(
            crate::identity::IdentityDeviceSummary {
                identity: crate::identity::IdentitySummary {
                    id: crate::ids::IdentityId::parse("identity-new").unwrap(),
                    did,
                    handle: None,
                    display_name: None,
                    local_alias: None,
                    device_id: None,
                    is_default: true,
                    readiness: crate::identity::IdentityReadiness {
                        ready_for_auth: true,
                        ready_for_messaging: true,
                        missing: Vec::new(),
                    },
                },
                mode: crate::identity::IdentityDeviceMode::VNext,
                protocol_device_id: Some(protocol_device_id.clone()),
                role: Some(crate::identity::IdentityDeviceRole::Member),
                signing_key_id: Some("did:wba:awiki.test:alice#sign".to_owned()),
                e2ee_key_id: Some("did:wba:awiki.test:alice#e2ee".to_owned()),
                readiness: crate::identity::IdentityDeviceReadiness::MemberReady,
                blocked_reason: None,
            },
            &protocol_device_id,
        )
        .unwrap();

        assert_eq!(device.protocol_device_id, protocol_device_id);
        assert_eq!(device.role, DeviceJoinRole::Member);
        assert!(!device.management_ready);
        assert!(device.is_current);
    }

    #[test]
    fn authorized_device_is_current_only_in_new_device_progress() {
        for (side, expected_current) in [
            (DeviceJoinSide::NewDevice, true),
            (DeviceJoinSide::Admin, false),
        ] {
            let did = crate::ids::Did::parse("did:wba:awiki.test:user:alice:e1_test").unwrap();
            let protocol_device_id = crate::ids::ProtocolDeviceId::parse("dev-new").unwrap();
            let progress = public_progress(
                crate::internal::identity_device_join_runtime::DeviceJoinAdvanceResult {
                    session: DeviceJoinSessionSummary {
                        join_session_id: "join-test".to_owned(),
                        did,
                        protocol_device_id,
                        side,
                        phase: DeviceJoinLocalPhase::Authorized,
                        join_request_hash: "sha256:test".to_owned(),
                        challenge_id: None,
                        expires_at: "2026-07-20T00:10:00Z".to_owned(),
                    },
                    remote_state:
                        crate::internal::identity_device_join_runtime::DeviceJoinRemoteState::Consumed,
                    authorization: Some(
                        crate::internal::identity_device_join_runtime::DeviceJoinRemoteAuthorization {
                            checkpoint: crate::internal::identity_device_state::IdentityInternalCheckpoint {
                                document_version: 2,
                                document_hash: "sha256:document".to_owned(),
                                registry_version: 2,
                            },
                            device: crate::internal::identity_device_join_runtime::DeviceJoinRemoteDeviceSummary {
                                device_id: "dev-new".to_owned(),
                                signing_key_id: "did:wba:awiki.test:user:alice:e1_test#sign".to_owned(),
                                e2ee_key_id: "did:wba:awiki.test:user:alice:e1_test#e2ee".to_owned(),
                                status: crate::internal::identity_device_state::DeviceAuthorizationStatus::Active,
                                role: crate::internal::identity_device_state::DeviceAuthorizationRole::Member,
                                management_ready: false,
                                auth_generation: 1,
                            },
                        },
                    ),
                    sas: Some("482917".to_owned()),
                },
            )
            .unwrap();

            assert!(progress.sas.is_none());
            assert_eq!(
                progress.authorized_device.unwrap().is_current,
                expected_current
            );
        }
    }

    #[test]
    fn registry_projection_exposes_decimal_versions_without_expanding_join_progress() {
        let snapshot = public_registry(
            crate::internal::identity_device_join_runtime::DeviceJoinRemoteRegistry {
                did: crate::ids::Did::parse("did:wba:awiki.test:user:alice:e1_test").unwrap(),
                checkpoint:
                    crate::internal::identity_device_state::IdentityInternalCheckpoint {
                        document_version: 9,
                        document_hash: "sha256:document".to_owned(),
                        registry_version: u64::MAX,
                    },
                devices: vec![
                    crate::internal::identity_device_join_runtime::DeviceJoinRemoteDeviceSummary {
                        device_id: "dev-current".to_owned(),
                        signing_key_id:
                            "did:wba:awiki.test:user:alice:e1_test#dev-current-sign".to_owned(),
                        e2ee_key_id:
                            "did:wba:awiki.test:user:alice:e1_test#dev-current-e2ee".to_owned(),
                        status:
                            crate::internal::identity_device_state::DeviceAuthorizationStatus::Active,
                        role:
                            crate::internal::identity_device_state::DeviceAuthorizationRole::Admin,
                        management_ready: true,
                        auth_generation: u64::MAX,
                    },
                ],
            },
            Some(&crate::ids::ProtocolDeviceId::parse("dev-current").unwrap()),
        )
        .unwrap();

        assert_eq!(snapshot.registry_version, u64::MAX.to_string());
        assert_eq!(snapshot.devices[0].auth_generation, u64::MAX.to_string());
        assert!(snapshot.devices[0].is_current);

        let json = serde_json::to_value(snapshot).unwrap();
        assert_eq!(
            json.get("registry_version")
                .and_then(serde_json::Value::as_str),
            Some("18446744073709551615")
        );
        assert_eq!(
            json.pointer("/devices/0/auth_generation")
                .and_then(serde_json::Value::as_str),
            Some("18446744073709551615")
        );
        assert!(json.get("document_version").is_none());
        assert!(json.get("document_hash").is_none());
    }
}
